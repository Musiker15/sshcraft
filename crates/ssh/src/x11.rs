//! X11 forwarding: the server gets a random cookie, never the real one. Each X11 channel the
//! server opens must start with that cookie; it is swapped for the local display's real cookie
//! (or for no authorisation at all) before the bytes reach the local X server. A channel with
//! another cookie is closed before anything reaches the display.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use russh::client::Msg;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The authorisation protocol both cookies use.
pub const PROTOCOL: &str = "MIT-MAGIC-COOKIE-1";
/// Display `N` listens on TCP port `6000 + N`.
const BASE_PORT: u16 = 6000;
/// The length of the cookie the server gets.
const FAKE_LEN: usize = 16;
/// The longest cookie read from `xauth`.
const MAX_COOKIE: usize = 256;
/// The connection setup is 12 bytes, then the protocol name and the cookie, each padded to 4.
const SETUP_FIXED: usize = 12;
/// How long `xauth` may take.
const XAUTH_TIMEOUT: Duration = Duration::from_secs(3);
/// How long the server may take to send the connection setup.
const SETUP_TIMEOUT: Duration = Duration::from_secs(10);
/// X11 connections open at once on one session.
const MAX_CONNECTIONS: usize = 64;

/// Where the local X server listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Tcp(String, u16),
    Unix(PathBuf),
}

/// A local display, from a name like `:0`, `localhost:10.0` or a socket path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    /// The name as given, for `xauth`.
    pub name: String,
    pub target: Target,
    pub screen: u32,
}

/// Reads a display name. `:N` is the local socket on Unix and TCP port `6000 + N` on
/// Windows (where X servers such as VcXsrv listen); `host:N` is always TCP; a name starting
/// with `/` is a socket path (XQuartz on macOS).
pub fn parse_display(name: &str) -> Option<Display> {
    let (host, rest) = name.rsplit_once(':')?;
    let (number, screen) = match rest.split_once('.') {
        Some((n, s)) => (n, s.parse::<u32>().ok()?),
        None => (rest, 0),
    };
    let number: u16 = number.parse().ok()?;
    let port = BASE_PORT.checked_add(number)?;
    let target = if host.starts_with('/') {
        Target::Unix(PathBuf::from(format!("{host}:{number}")))
    } else if host.is_empty() || host == "unix" {
        local_target(number, port)
    } else if host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':')) {
        Target::Tcp(host.trim_start_matches('[').trim_end_matches(']').to_string(), port)
    } else {
        return None;
    };
    Some(Display { name: name.to_string(), target, screen })
}

#[cfg(unix)]
fn local_target(number: u16, _port: u16) -> Target {
    Target::Unix(PathBuf::from(format!("/tmp/.X11-unix/X{number}")))
}

#[cfg(not(unix))]
fn local_target(_number: u16, port: u16) -> Target {
    Target::Tcp("127.0.0.1".into(), port)
}

/// The display of this computer: `DISPLAY`, or `:0` on Windows, where it is rarely set.
pub fn local_display() -> Option<Display> {
    match std::env::var("DISPLAY") {
        Ok(name) if !name.trim().is_empty() => parse_display(name.trim()),
        _ if cfg!(windows) => parse_display(":0"),
        _ => None,
    }
}

/// The display's real cookie from `xauth list` (run without a shell), or none.
pub async fn real_cookie(display: &Display) -> Option<Vec<u8>> {
    // `xauth` names a local display `unix:N`.
    let name = match display.name.strip_prefix(':') {
        Some(rest) => format!("unix:{rest}"),
        None => display.name.clone(),
    };
    let output = tokio::process::Command::new("xauth")
        .arg("list")
        .arg(name)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(XAUTH_TIMEOUT, output).await.ok()?.ok()?;
    cookie_from_xauth(&String::from_utf8_lossy(&output.stdout))
}

/// The first MIT-MAGIC-COOKIE-1 in `xauth list` output (`host/unix:0  PROTOCOL  hex`).
pub fn cookie_from_xauth(output: &str) -> Option<Vec<u8>> {
    output.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let _display = words.next()?;
        if words.next()? != PROTOCOL {
            return None;
        }
        let hex = words.next()?;
        if hex.len() > MAX_COOKIE * 2 {
            return None;
        }
        data_encoding::HEXLOWER_PERMISSIVE.decode(hex.as_bytes()).ok().filter(|c| !c.is_empty())
    })
}

/// What a session forwards to: the display and its real cookie (`None`: no authorisation).
#[derive(Debug, Clone)]
pub struct X11Options {
    pub display: Display,
    pub cookie: Option<Vec<u8>>,
}

/// The forwarding state of one session.
pub(crate) struct Forward {
    options: X11Options,
    fake: [u8; FAKE_LEN],
    /// Set once a shell asked the server for forwarding; channels before that are refused.
    requested: AtomicBool,
    open: Arc<AtomicUsize>,
}

impl Forward {
    pub(crate) fn new(options: X11Options) -> Forward {
        Forward { options, fake: rand::random(), requested: AtomicBool::new(false), open: Arc::new(AtomicUsize::new(0)) }
    }

    /// The cookie the server gets, as hex.
    pub(crate) fn fake_hex(&self) -> String {
        data_encoding::HEXLOWER.encode(&self.fake)
    }

    pub(crate) fn screen(&self) -> u32 {
        self.options.display.screen
    }

    pub(crate) fn mark_requested(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    /// Takes a channel the server opened: refused (by dropping `reply`) unless a shell asked for
    /// forwarding, too many are open, or the local display cannot be reached.
    pub(crate) async fn accept(self: &Arc<Self>, channel: russh::Channel<Msg>, reply: russh::client::ChannelOpenHandle) {
        if !self.requested.load(Ordering::SeqCst) || self.open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            return;
        }
        let Ok(local) = connect(&self.options.display.target).await else { return };
        reply.accept().await;
        let forward = self.clone();
        let open = self.open.clone();
        open.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async move {
            let _ = forward.serve(channel, local).await;
            open.fetch_sub(1, Ordering::SeqCst);
        });
    }

    async fn serve(&self, channel: russh::Channel<Msg>, mut local: Box<dyn crate::Io>) -> std::io::Result<()> {
        let mut stream = channel.into_stream();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 512];
        let rewritten = tokio::time::timeout(SETUP_TIMEOUT, async {
            loop {
                match rewrite_setup(&buf, &self.fake, self.options.cookie.as_deref()) {
                    Setup::Ready(out) => return Ok::<_, std::io::Error>(Some(out)),
                    Setup::Refused => return Ok(None),
                    Setup::Incomplete => {}
                }
                let n = stream.read(&mut chunk).await?;
                if n == 0 {
                    return Ok(None);
                }
                buf.extend_from_slice(chunk.get(..n).unwrap_or_default());
            }
        })
        .await
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))??;
        let Some(out) = rewritten else { return Ok(()) };
        local.write_all(&out).await?;
        tokio::io::copy_bidirectional(&mut stream, &mut local).await?;
        Ok(())
    }
}

async fn connect(target: &Target) -> std::io::Result<Box<dyn crate::Io>> {
    match target {
        Target::Tcp(host, port) => Ok(Box::new(tokio::net::TcpStream::connect((host.as_str(), *port)).await?)),
        #[cfg(unix)]
        Target::Unix(path) => Ok(Box::new(tokio::net::UnixStream::connect(path).await?)),
        #[cfg(not(unix))]
        Target::Unix(_) => Err(std::io::ErrorKind::Unsupported.into()),
    }
}

/// The connection setup with the cookie swapped.
#[derive(Debug, PartialEq, Eq)]
pub enum Setup {
    /// More bytes needed.
    Incomplete,
    /// Not the fake cookie, or not an X11 setup: close the channel.
    Refused,
    /// The setup for the local display, followed by whatever came after it.
    Ready(Vec<u8>),
}

fn pad4(n: usize) -> usize {
    n.div_ceil(4).saturating_mul(4)
}

/// Swaps `fake` in the X11 connection setup at the start of `buf` for `real` (none: no
/// authorisation). A setup with another protocol or cookie is refused.
pub fn rewrite_setup(buf: &[u8], fake: &[u8], real: Option<&[u8]>) -> Setup {
    let Some(head) = buf.get(..SETUP_FIXED) else { return Setup::Incomplete };
    let read = |at: usize| -> Option<u16> {
        let bytes = [*head.get(at)?, *head.get(at + 1)?];
        match head.first()? {
            b'B' => Some(u16::from_be_bytes(bytes)),
            b'l' => Some(u16::from_le_bytes(bytes)),
            _ => None,
        }
    };
    let (Some(name_len), Some(data_len)) = (read(6), read(8)) else { return Setup::Refused };
    let (name_len, data_len) = (usize::from(name_len), usize::from(data_len));
    // Anything longer than the fake protocol and cookie cannot be them.
    if name_len != PROTOCOL.len() || data_len != fake.len() {
        return Setup::Refused;
    }
    let total = SETUP_FIXED + pad4(name_len) + pad4(data_len);
    if buf.len() < total {
        return Setup::Incomplete;
    }
    let name = buf.get(SETUP_FIXED..SETUP_FIXED + name_len).unwrap_or_default();
    let data_at = SETUP_FIXED + pad4(name_len);
    let data = buf.get(data_at..data_at + data_len).unwrap_or_default();
    if name != PROTOCOL.as_bytes() || !same(data, fake) {
        return Setup::Refused;
    }
    let (new_name, new_data): (&[u8], &[u8]) = match real {
        Some(cookie) => (PROTOCOL.as_bytes(), cookie),
        None => (&[], &[]),
    };
    let write = |n: usize| -> [u8; 2] {
        // Both lengths are at most MAX_COOKIE or the protocol name.
        let n = u16::try_from(n).unwrap_or(0);
        if head.first() == Some(&b'B') { n.to_be_bytes() } else { n.to_le_bytes() }
    };
    let mut out = Vec::with_capacity(buf.len());
    out.extend_from_slice(head.get(..6).unwrap_or_default());
    out.extend_from_slice(&write(new_name.len()));
    out.extend_from_slice(&write(new_data.len()));
    out.extend_from_slice(head.get(10..12).unwrap_or_default());
    for part in [new_name, new_data] {
        out.extend_from_slice(part);
        out.resize(out.len() + pad4(part.len()) - part.len(), 0);
    }
    out.extend_from_slice(buf.get(total..).unwrap_or_default());
    Setup::Ready(out)
}

/// Compares two cookies without stopping at the first difference.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(order: u8, name: &[u8], data: &[u8]) -> Vec<u8> {
        let len = |n: usize| if order == b'B' { (n as u16).to_be_bytes() } else { (n as u16).to_le_bytes() };
        let mut out = vec![order, 0, 0, 11, 0, 0];
        out.extend_from_slice(&len(name.len()));
        out.extend_from_slice(&len(data.len()));
        out.extend_from_slice(&[0, 0]);
        for part in [name, data] {
            out.extend_from_slice(part);
            out.resize(out.len() + pad4(part.len()) - part.len(), 0);
        }
        out
    }

    #[test]
    fn display_names_are_read() {
        let tcp = parse_display("localhost:10.2").unwrap();
        assert_eq!(tcp.target, Target::Tcp("localhost".into(), 6010));
        assert_eq!(tcp.screen, 2);
        let local = parse_display(":0").unwrap();
        if cfg!(unix) {
            assert_eq!(local.target, Target::Unix("/tmp/.X11-unix/X0".into()));
        } else {
            assert_eq!(local.target, Target::Tcp("127.0.0.1".into(), 6000));
        }
        let quartz = parse_display("/private/tmp/com.apple.launchd.x/org.xquartz:0").unwrap();
        assert_eq!(quartz.target, Target::Unix("/private/tmp/com.apple.launchd.x/org.xquartz:0".into()));
        assert_eq!(parse_display("nodisplay"), None);
        assert_eq!(parse_display(":70000"), None);
        assert_eq!(parse_display(":60000"), None);
        assert_eq!(parse_display("evil host;rm:0"), None);
    }

    #[test]
    fn the_xauth_cookie_is_read() {
        let out = "box/unix:0  MIT-MAGIC-COOKIE-1  0011aabb\nbox/unix:0  XDM-AUTHORIZATION-1  ff\n";
        assert_eq!(cookie_from_xauth(out), Some(vec![0x00, 0x11, 0xaa, 0xbb]));
        assert_eq!(cookie_from_xauth("box/unix:0  XDM-AUTHORIZATION-1  ff"), None);
        assert_eq!(cookie_from_xauth(""), None);
    }

    #[test]
    fn the_fake_cookie_is_swapped_for_the_real_one() {
        let fake = [7u8; FAKE_LEN];
        for order in *b"Bl" {
            let mut sent = setup(order, PROTOCOL.as_bytes(), &fake);
            sent.extend_from_slice(b"rest");
            let mut want = setup(order, PROTOCOL.as_bytes(), &[1, 2, 3]);
            want.extend_from_slice(b"rest");
            assert_eq!(rewrite_setup(&sent, &fake, Some(&[1, 2, 3])), Setup::Ready(want));
        }
    }

    #[test]
    fn without_a_real_cookie_the_setup_carries_none() {
        let fake = [7u8; FAKE_LEN];
        let sent = setup(b'l', PROTOCOL.as_bytes(), &fake);
        assert_eq!(rewrite_setup(&sent, &fake, None), Setup::Ready(setup(b'l', b"", b"")));
    }

    #[test]
    fn another_cookie_is_refused() {
        let fake = [7u8; FAKE_LEN];
        assert_eq!(rewrite_setup(&setup(b'l', PROTOCOL.as_bytes(), &[8u8; FAKE_LEN]), &fake, None), Setup::Refused);
        assert_eq!(rewrite_setup(&setup(b'l', b"", b""), &fake, None), Setup::Refused);
        assert_eq!(rewrite_setup(&setup(b'x', PROTOCOL.as_bytes(), &fake), &fake, None), Setup::Refused);
    }

    #[test]
    fn a_setup_in_pieces_waits_for_the_rest() {
        let fake = [7u8; FAKE_LEN];
        let sent = setup(b'B', PROTOCOL.as_bytes(), &fake);
        for cut in [0, 5, 12, sent.len() - 1] {
            assert_eq!(rewrite_setup(&sent[..cut], &fake, None), Setup::Incomplete);
        }
    }
}

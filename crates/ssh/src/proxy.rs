//! The first connection of a route: straight over TCP, through an HTTP (CONNECT) or SOCKS5
//! proxy, or through a local program (a proxy command). Proxy credentials go only to the proxy.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zeroize::Zeroizing;

use crate::{Io, ProxyConfig, ProxyTarget, SshError};

/// The longest answer to `CONNECT` read before giving up.
const MAX_HTTP_RESPONSE: usize = 8 * 1024;

/// How much of a proxy command's standard error is kept for the error message.
const MAX_COMMAND_ERRORS: usize = 4 * 1024;

/// A stream to the first hop, and for a proxy command what it writes to standard error.
pub(crate) struct Opened {
    pub stream: Box<dyn Io>,
    pub stderr: Option<tokio::task::JoinHandle<String>>,
}

impl From<Box<dyn Io>> for Opened {
    fn from(stream: Box<dyn Io>) -> Self {
        Self { stream, stderr: None }
    }
}

pub(crate) async fn open(proxy: Option<&ProxyConfig>, address: &str, port: u16, timeout: Duration) -> Result<Opened, SshError> {
    let stream: Box<dyn Io> = match proxy {
        Some(ProxyConfig::Command { argv }) => return spawn_command(argv),
        None => {
            let tcp = tokio::time::timeout(timeout, TcpStream::connect((address, port)))
                .await
                .map_err(|_| SshError::Connect("timed out".into()))?
                .map_err(|e| SshError::Connect(e.to_string()))?;
            let _ = tcp.set_nodelay(true);
            Box::new(tcp)
        }
        Some(ProxyConfig::Http(target)) => {
            tokio::time::timeout(timeout, http(target, address, port)).await.map_err(|_| SshError::Proxy("timed out".into()))??
        }
        Some(ProxyConfig::Socks5(target)) => {
            tokio::time::timeout(timeout, socks5(target, address, port)).await.map_err(|_| SshError::Proxy("timed out".into()))??
        }
    };
    Ok(stream.into())
}

/// Starts a proxy command with an argument list (never a shell). The program lives as long as
/// the stream; dropping the stream kills it.
fn spawn_command(argv: &[String]) -> Result<Opened, SshError> {
    let (program, args) = argv.split_first().ok_or_else(|| SshError::Proxy("the proxy command is empty".into()))?;
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: a console program must not open a window of its own.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| SshError::Proxy(format!("could not start {program}: {e}")))?;
    let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        return Err(SshError::Proxy(format!("could not start {program}: no pipes")));
    };
    let errors = tokio::spawn(async move {
        let mut stderr = stderr;
        let mut kept = Vec::new();
        let mut buf = [0u8; 1024];
        // Read to the end so the program never blocks on a full pipe, keep only the start.
        while let Ok(n) = stderr.read(&mut buf).await {
            if n == 0 {
                break;
            }
            let room = MAX_COMMAND_ERRORS.saturating_sub(kept.len());
            kept.extend_from_slice(buf.get(..n.min(room)).unwrap_or_default());
        }
        String::from_utf8_lossy(&kept).trim().to_owned()
    });
    Ok(Opened { stream: Box::new(CommandStream { _child: child, io: tokio::io::join(stdout, stdin) }), stderr: Some(errors) })
}

/// What a proxy command wrote to standard error before it ended, if anything; waits a moment
/// for it to finish.
pub(crate) async fn command_said(errors: tokio::task::JoinHandle<String>) -> Option<String> {
    match tokio::time::timeout(Duration::from_secs(2), errors).await {
        Ok(Ok(text)) if !text.is_empty() => Some(text),
        _ => None,
    }
}

/// A proxy command's standard output and input as one stream, keeping the program alive.
struct CommandStream {
    _child: tokio::process::Child,
    io: tokio::io::Join<tokio::process::ChildStdout, tokio::process::ChildStdin>,
}

impl tokio::io::AsyncRead for CommandStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for CommandStream {
    fn poll_write(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &[u8]) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_flush(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

fn authority(address: &str, port: u16) -> String {
    if address.contains(':') { format!("[{address}]:{port}") } else { format!("{address}:{port}") }
}

/// Text that goes into the proxy request: no control characters (no header can be smuggled in
/// through a synced or shared value).
fn clean(text: &str, what: &str) -> Result<(), SshError> {
    if text.chars().any(char::is_control) {
        return Err(SshError::Proxy(format!("invalid {what}")));
    }
    Ok(())
}

async fn http(proxy: &ProxyTarget, address: &str, port: u16) -> Result<Box<dyn Io>, SshError> {
    clean(address, "target address")?;
    if let Some(user) = &proxy.username {
        clean(user, "proxy user")?;
        // Basic authentication joins user and password with a colon.
        if user.contains(':') {
            return Err(SshError::Proxy("invalid proxy user".into()));
        }
    }
    if let Some(password) = &proxy.password {
        clean(password, "proxy password")?;
    }
    let mut tcp = TcpStream::connect((proxy.address.as_str(), proxy.port)).await.map_err(|e| SshError::Proxy(e.to_string()))?;
    let _ = tcp.set_nodelay(true);
    let target = authority(address, port);
    let mut request = Zeroizing::new(format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n"));
    if let Some(user) = &proxy.username {
        let password = proxy.password.as_ref().map_or("", |p| p.as_str());
        let pair = Zeroizing::new(format!("{user}:{password}"));
        let token = Zeroizing::new(data_encoding::BASE64.encode(pair.as_bytes()));
        request.push_str("Proxy-Authorization: Basic ");
        request.push_str(&token);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    tcp.write_all(request.as_bytes()).await.map_err(|e| SshError::Proxy(e.to_string()))?;
    // Byte by byte, so nothing of the SSH stream behind the answer is read away.
    let mut answer = Vec::new();
    while !answer.ends_with(b"\r\n\r\n") {
        if answer.len() >= MAX_HTTP_RESPONSE {
            return Err(SshError::Proxy("answer too long".into()));
        }
        let byte = tcp.read_u8().await.map_err(|e| SshError::Proxy(e.to_string()))?;
        answer.push(byte);
    }
    let status =
        String::from_utf8_lossy(&answer).lines().next().and_then(|line| line.split_whitespace().nth(1).map(str::to_owned)).unwrap_or_default();
    match status.as_str() {
        "200" => Ok(Box::new(tcp)),
        "407" => Err(SshError::ProxyAuthFailed),
        "" => Err(SshError::Proxy("no HTTP answer".into())),
        code => Err(SshError::Proxy(format!("HTTP {code}"))),
    }
}

async fn socks5(proxy: &ProxyTarget, address: &str, port: u16) -> Result<Box<dyn Io>, SshError> {
    use tokio_socks::tcp::Socks5Stream;
    let server = (proxy.address.as_str(), proxy.port);
    // The name goes to the proxy as it is: the proxy resolves it.
    let target = (address, port);
    let stream = match &proxy.username {
        Some(user) => {
            let password = proxy.password.as_ref().map_or("", |p| p.as_str());
            Socks5Stream::connect_with_password(server, target, user, password).await
        }
        None => Socks5Stream::connect(server, target).await,
    }
    .map_err(|e| match e {
        tokio_socks::Error::PasswordAuthFailure(_) => SshError::ProxyAuthFailed,
        other => SshError::Proxy(other.to_string()),
    })?;
    Ok(Box::new(stream))
}

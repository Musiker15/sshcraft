//! An in-process SSH server for tests. Accepts one client key, a password and rounds of
//! keyboard-interactive questions, as [`TestAuth`] says, and records what clients sent.
//!
//! The shell echoes every input chunk (unless [`TestAuth::quiet`]), except two commands sent as
//! their own chunk: `exit\n` ends the shell with exit status 0, `flood\n` sends [`flood_pattern`].

// Test helpers, built only with the `testing` feature: a failing setup ends the test that uses
// it, which is how a test reports.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use russh::keys::ssh_encoding::bytes::Bytes;
use russh::keys::{Algorithm, Certificate, HashAlg, PrivateKey, PublicKey};
use russh::server::{self, Auth, Msg, Server as _, Session, VerifiedPublicKey};
use russh::{Channel, ChannelId};
use russh::{MethodKind, MethodSet};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

mod sftp_server;

const FLOOD_LEN: usize = 5 * 1024 * 1024;
/// What the test server answers to any command.
pub const TEST_EXEC_OUTPUT: &str = "NAME=\"Debian\"\nID=debian\nLinux\n";
const FLOOD_CHUNK: usize = 32 * 1024;

/// 5 MiB of bytes 0..=255 interleaved with multi-byte UTF-8, so chunk borders split characters.
pub fn flood_pattern() -> Vec<u8> {
    let mut block: Vec<u8> = (0..=255u8).collect();
    block.extend_from_slice("äöü🙂".as_bytes());
    block.iter().copied().cycle().take(FLOOD_LEN).collect()
}

/// What the test server accepts.
#[derive(Clone, Default)]
pub struct TestAuth {
    pub key: Option<PublicKey>,
    pub password: Option<String>,
    /// Rounds of keyboard-interactive questions: (question, echo, expected answer).
    pub kbd: Vec<Vec<(String, bool, String)>>,
    /// The key alone is not enough: keyboard-interactive follows it.
    pub key_then_kbd: bool,
    /// The password alone is not enough: keyboard-interactive follows it.
    pub password_then_kbd: bool,
    /// User certificates signed by this CA are accepted when their principals name the user.
    pub ca: Option<PublicKey>,
    /// Turns down `ssh-rsa` (SHA-1) for keys and certificates, as OpenSSH does since 8.8.
    pub refuse_sha1_rsa: bool,
    /// Turns down `tcpip-forward` and `direct-tcpip`, as `AllowTcpForwarding no` does.
    pub refuse_forwarding: bool,
    /// The shell echoes nothing (`exit` and `flood` still work), as with a password typed at
    /// a prompt.
    pub quiet: bool,
}

/// A user certificate for `key`, signed by `ca`, for `principal`, valid in the given window.
pub fn user_certificate(key: &PublicKey, ca: &PrivateKey, principal: &str, valid_after: u64, valid_before: u64) -> Certificate {
    use russh::keys::ssh_key::certificate::{Builder, CertType};
    let mut builder =
        Builder::new_with_random_nonce(&mut rand::rng(), key.key_data().clone(), valid_after, valid_before).expect("certificate builder");
    builder.cert_type(CertType::User).expect("cert type");
    builder.key_id("test").expect("key id");
    builder.valid_principal(principal).expect("principal");
    builder.sign(ca).expect("signed certificate")
}

/// What clients sent to the test server.
#[derive(Default)]
struct Records {
    /// SSH connections clients opened.
    connections: std::sync::atomic::AtomicUsize,
    /// Passwords and keyboard-interactive answers, in the order they came.
    passwords: Mutex<Vec<String>>,
    /// Environment variables clients asked for.
    env: Mutex<Vec<(String, String)>>,
    /// The first try to use a forwarded agent: how many keys it listed, `None` if refused.
    agent: Mutex<Option<Option<usize>>>,
    /// How many certificates clients offered.
    certificates: Mutex<usize>,
    /// Commands clients ran over `exec`, in order.
    commands: Mutex<Vec<String>>,
    /// What `exec` answers; `None`: [`TEST_EXEC_OUTPUT`].
    exec_output: Mutex<Option<String>>,
    /// Extra for `exec`: text on standard error, exit status, and a delay before answering.
    exec_stderr: Mutex<String>,
    exec_status: Mutex<u32>,
    exec_delay: Mutex<std::time::Duration>,
    /// Terminal types clients asked for with `pty-req`.
    terms: Mutex<Vec<String>>,
    /// What clients sent on the standard input of `exec` channels.
    exec_input: Mutex<Vec<u8>>,
    /// `exec` keeps its channel open after the output (a program that keeps running).
    exec_keep_open: std::sync::atomic::AtomicBool,
    /// Input that makes an open `exec` channel answer: (text in the input, answer), each once.
    exec_replies: Mutex<Vec<(String, String)>>,
    /// Input of `exec` channels not yet matched by a reply: a command may come in two packets.
    exec_pending: Mutex<String>,
    /// The protocol and cookie of the last `x11-req`.
    x11_request: Mutex<Option<(String, String)>>,
    /// Sent in X11 setups instead of the requested cookie.
    x11_cookie: Mutex<Option<Vec<u8>>>,
    /// Whether the client took an X11 channel opened right after sign-in, before any request.
    x11_unasked: Mutex<Option<bool>>,
    /// The folder the `sftp` subsystem serves; none: the subsystem is refused.
    sftp_root: Mutex<Option<std::path::PathBuf>>,
    sftp: Arc<sftp_server::SftpRecords>,
}

pub struct TestServer {
    pub port: u16,
    pub host_fingerprint: String,
    auth: TestAuth,
    records: Arc<Records>,
    host_key: PrivateKey,
    running: Option<(server::RunningServerHandle, JoinHandle<()>)>,
}

impl TestServer {
    pub async fn start(authorized: PublicKey) -> TestServer {
        Self::start_with(TestAuth { key: Some(authorized), ..Default::default() }).await
    }

    pub async fn start_with(auth: TestAuth) -> TestServer {
        let host_key = random_host_key();
        let mut server =
            TestServer { port: 0, host_fingerprint: fingerprint(&host_key), auth, records: Arc::new(Records::default()), host_key, running: None };
        server.listen().await;
        server
    }

    /// How many SSH connections clients opened.
    pub fn connections(&self) -> usize {
        self.records.connections.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Passwords and keyboard-interactive answers clients sent, in order.
    pub fn received_passwords(&self) -> Vec<String> {
        self.records.passwords.lock().unwrap().clone()
    }

    /// Serves `root` as the `sftp` subsystem: `/` on the wire is that folder.
    pub fn serve_sftp(&self, root: &std::path::Path) {
        *self.records.sftp_root.lock().unwrap() = Some(root.to_path_buf());
    }

    /// The mode a client last set on `path` (as sent on the wire, made canonical) over SFTP.
    pub fn sftp_mode(&self, path: &str) -> Option<u32> {
        self.records.sftp.modes.lock().unwrap().get(path).copied()
    }

    /// Answers every later `exec` with `output`.
    pub fn set_exec_output(&self, output: &str) {
        *self.records.exec_output.lock().unwrap() = Some(output.to_owned());
    }

    /// What `exec` writes to standard error, its exit status and how long it takes.
    pub fn set_exec_result(&self, stderr: &str, status: u32, delay: std::time::Duration) {
        *self.records.exec_stderr.lock().unwrap() = stderr.to_owned();
        *self.records.exec_status.lock().unwrap() = status;
        *self.records.exec_delay.lock().unwrap() = delay;
    }

    /// Commands clients ran over `exec`, in order.
    pub fn received_commands(&self) -> Vec<String> {
        self.records.commands.lock().unwrap().clone()
    }

    /// How many certificates clients offered.
    pub fn received_certificates(&self) -> usize {
        *self.records.certificates.lock().unwrap()
    }

    /// Everything clients sent to commands run with `exec`.
    pub fn received_exec_input(&self) -> Vec<u8> {
        self.records.exec_input.lock().unwrap().clone()
    }

    /// Terminal types clients asked for, in order.
    pub fn requested_terms(&self) -> Vec<String> {
        self.records.terms.lock().unwrap().clone()
    }

    pub fn received_env(&self) -> Vec<(String, String)> {
        self.records.env.lock().unwrap().clone()
    }

    /// Answers `answer` on an `exec` channel once its input contains `trigger`, once per pair.
    pub fn set_exec_replies(&self, replies: Vec<(String, String)>) {
        *self.records.exec_replies.lock().unwrap() = replies;
    }

    /// Keeps `exec` channels open after their output, like a program that keeps running.
    pub fn set_exec_keep_open(&self, open: bool) {
        self.records.exec_keep_open.store(open, std::sync::atomic::Ordering::SeqCst);
    }

    /// The protocol and cookie of the last X11 request.
    pub fn x11_request(&self) -> Option<(String, String)> {
        self.records.x11_request.lock().unwrap().clone()
    }

    /// Makes the server send `cookie` in its X11 setups instead of the one it was given.
    pub fn set_x11_cookie(&self, cookie: Vec<u8>) {
        *self.records.x11_cookie.lock().unwrap() = Some(cookie);
    }

    /// `None` until the server tried an X11 channel after sign-in; then whether it was taken.
    pub fn x11_unasked(&self) -> Option<bool> {
        *self.records.x11_unasked.lock().unwrap()
    }

    /// `None` while the server has not tried the agent yet; then whether it got one (and how
    /// many keys it lists).
    pub fn agent_attempt(&self) -> Option<Option<usize>> {
        *self.records.agent.lock().unwrap()
    }

    /// Drops every connection and stops listening.
    pub fn stop(&mut self) {
        let _ = self.stop_task();
    }

    fn stop_task(&mut self) -> Option<JoinHandle<()>> {
        let (handle, task) = self.running.take()?;
        handle.shutdown("test server stopped".into());
        task.abort();
        Some(task)
    }

    /// Stops and listens again on the same port, optionally with a new host key.
    pub async fn restart(&mut self, new_host_key: bool) {
        if let Some(task) = self.stop_task() {
            // The listener is only released once the aborted task has been dropped.
            let _ = task.await;
        }
        if new_host_key {
            self.host_key = random_host_key();
            self.host_fingerprint = fingerprint(&self.host_key);
        }
        self.listen().await;
    }

    async fn listen(&mut self) {
        let socket = TcpListener::bind(("127.0.0.1", self.port)).await.expect("bind test server");
        self.port = socket.local_addr().expect("local addr").port();
        let mut methods: Vec<MethodKind> = Vec::new();
        if self.auth.key.is_some() {
            methods.push(MethodKind::PublicKey);
        }
        if self.auth.password.is_some() {
            methods.push(MethodKind::Password);
        }
        if !self.auth.kbd.is_empty() && !self.auth.key_then_kbd && !self.auth.password_then_kbd {
            methods.push(MethodKind::KeyboardInteractive);
        }
        let config = Arc::new(server::Config {
            keys: vec![self.host_key.clone()],
            auth_rejection_time: std::time::Duration::from_millis(10),
            auth_rejection_time_initial: Some(std::time::Duration::ZERO),
            methods: MethodSet::from(&methods[..]),
            ..Default::default()
        });
        let auth = self.auth.clone();
        let records = self.records.clone();
        let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut factory = Factory { auth, records };
            let running = factory.run_on_socket(config, &socket);
            let _ = handle_tx.send(running.handle());
            let _ = running.await;
        });
        let handle = handle_rx.await.expect("test server handle");
        self.running = Some((handle, task));
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn random_host_key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("host key")
}

fn fingerprint(key: &PrivateKey) -> String {
    key.public_key().fingerprint(HashAlg::Sha256).to_string()
}

#[derive(Clone)]
struct Factory {
    auth: TestAuth,
    records: Arc<Records>,
}

impl server::Server for Factory {
    type Handler = Handler;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Handler {
        self.records.connections.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Handler {
            auth: self.auth.clone(),
            records: self.records.clone(),
            key_ok: false,
            kbd_round: 0,
            sessions: std::collections::HashSet::new(),
            execs: std::collections::HashSet::new(),
            opened: std::collections::HashMap::new(),
            listeners: Vec::new(),
        }
    }
}

struct Handler {
    auth: TestAuth,
    records: Arc<Records>,
    /// The key was accepted and keyboard-interactive must follow.
    key_ok: bool,
    kbd_round: usize,
    /// Session channels: only their bytes are echoed (tunnels pass theirs through).
    sessions: std::collections::HashSet<ChannelId>,
    /// Channels that run a command: their bytes are its input.
    execs: std::collections::HashSet<ChannelId>,
    /// Session channels until a shell, a command or a subsystem is asked for on them; the `sftp`
    /// subsystem needs the channel itself.
    opened: std::collections::HashMap<ChannelId, Channel<Msg>>,
    /// Listeners for `tcpip-forward`: (address, port, accept task).
    listeners: Vec<(String, u32, JoinHandle<()>)>,
}

impl Drop for Handler {
    fn drop(&mut self) {
        for (_, _, task) in &self.listeners {
            task.abort();
        }
    }
}

impl Handler {
    fn kbd_question(&self) -> Auth {
        let round = &self.auth.kbd[self.kbd_round];
        let prompts: Vec<(Cow<'static, str>, bool)> = round.iter().map(|(q, echo, _)| (Cow::Owned(q.clone()), *echo)).collect();
        Auth::Partial { name: Cow::Borrowed(""), instructions: Cow::Borrowed(""), prompts: Cow::Owned(prompts) }
    }
}

impl server::Handler for Handler {
    type Error = russh::Error;

    async fn auth_publickey(&mut self, _user: &str, key: VerifiedPublicKey<'_>) -> Result<Auth, Self::Error> {
        let matches = self.auth.key.as_ref().is_some_and(|k| key.public_key.key_data() == k.key_data());
        if !matches {
            return Ok(Auth::reject());
        }
        if self.auth.key_then_kbd {
            self.key_ok = true;
            return Ok(Auth::Reject { proceed_with_methods: Some(MethodSet::from(&[MethodKind::KeyboardInteractive][..])), partial_success: true });
        }
        Ok(Auth::Accept)
    }

    async fn auth_publickey_offered(&mut self, _user: &str, offer: server::PublicKeyOffer<'_>) -> Result<Auth, Self::Error> {
        if self.auth.refuse_sha1_rsa && *offer.algorithm == (Algorithm::Rsa { hash: None }) {
            return Ok(Auth::reject());
        }
        Ok(Auth::Accept)
    }

    async fn auth_openssh_certificate(&mut self, user: &str, certificate: &Certificate) -> Result<Auth, Self::Error> {
        *self.records.certificates.lock().unwrap() += 1;
        let trusted = self.auth.ca.as_ref().is_some_and(|ca| certificate.signature_key() == ca.key_data());
        let for_user = certificate.valid_principals().iter().any(|p| p == user);
        Ok(if trusted && for_user && crate::certificate_valid_now(certificate) { Auth::Accept } else { Auth::reject() })
    }

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        self.records.passwords.lock().unwrap().push(password.to_owned());
        if self.auth.password.as_deref() == Some(password) {
            if self.auth.password_then_kbd {
                self.key_ok = true;
                return Ok(Auth::Reject {
                    proceed_with_methods: Some(MethodSet::from(&[MethodKind::KeyboardInteractive][..])),
                    partial_success: true,
                });
            }
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        _user: &str,
        _submethods: &str,
        response: Option<server::Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        let staged = self.auth.key_then_kbd || self.auth.password_then_kbd;
        if self.auth.kbd.is_empty() || (staged && !self.key_ok) {
            return Ok(Auth::reject());
        }
        let Some(response) = response else {
            self.kbd_round = 0;
            return Ok(self.kbd_question());
        };
        let answers: Vec<String> = response.map(|a| String::from_utf8_lossy(&a).into_owned()).collect();
        self.records.passwords.lock().unwrap().extend(answers.iter().cloned());
        let expected: Vec<&String> = self.auth.kbd[self.kbd_round].iter().map(|(_, _, a)| a).collect();
        if answers.iter().collect::<Vec<_>>() != expected {
            self.kbd_round = 0;
            return Ok(Auth::reject());
        }
        self.kbd_round += 1;
        if self.kbd_round == self.auth.kbd.len() { Ok(Auth::Accept) } else { Ok(self.kbd_question()) }
    }

    /// After every sign-in the server tries to use a forwarded agent, asked for or not, and
    /// notes whether the client let it.
    async fn auth_succeeded(&mut self, session: &mut Session) -> Result<(), Self::Error> {
        let handle = session.handle();
        let records = self.records.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            // `None`: the client refused the channel; `Some(0)` also when it opened one but no
            // agent answered behind it.
            let result = match handle.channel_open_agent().await {
                Err(_) => None,
                Ok(channel) => {
                    let mut agent = russh::keys::agent::client::AgentClient::connect(channel.into_stream());
                    let listed = tokio::time::timeout(std::time::Duration::from_secs(5), agent.request_identities()).await;
                    Some(match listed {
                        Ok(Ok(keys)) => keys.len(),
                        _ => 0,
                    })
                }
            };
            let mut slot = records.agent.lock().unwrap();
            if slot.is_none() {
                *slot = Some(result);
            }
        });
        // And an X11 channel nobody asked for.
        let handle = session.handle();
        let records = self.records.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let taken = handle.channel_open_x11("127.0.0.1", 0).await.is_ok();
            records.x11_unasked.lock().unwrap().get_or_insert(taken);
        });
        Ok(())
    }

    /// Notes the request, then opens one X11 channel and sends a connection setup with the
    /// cookie it got (or the one set with [`TestServer::set_x11_cookie`]), then `hello`.
    async fn x11_request(
        &mut self,
        channel: ChannelId,
        _single: bool,
        protocol: &str,
        cookie: &str,
        _screen: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        *self.records.x11_request.lock().unwrap() = Some((protocol.to_owned(), cookie.to_owned()));
        let sent =
            self.records.x11_cookie.lock().unwrap().clone().unwrap_or_else(|| data_encoding::HEXLOWER_PERMISSIVE.decode(cookie.as_bytes()).unwrap());
        let mut setup = x11_setup(protocol.as_bytes(), &sent);
        setup.extend_from_slice(b"hello");
        let handle = session.handle();
        tokio::spawn(async move {
            if let Ok(channel) = handle.channel_open_x11("127.0.0.1", 0).await {
                let mut stream = channel.into_stream();
                let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, &setup).await;
                let mut sink = Vec::new();
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut sink)).await;
            }
        });
        session.channel_success(channel)
    }

    async fn env_request(&mut self, _channel: ChannelId, name: &str, value: &str, _session: &mut Session) -> Result<(), Self::Error> {
        self.records.env.lock().unwrap().push((name.to_owned(), value.to_owned()));
        Ok(())
    }

    async fn agent_request(&mut self, _channel: ChannelId, _session: &mut Session) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Jump host: connects to the wanted port on this machine and passes the bytes through.
    /// A Unix socket on the "server": answers with the path, then echoes.
    async fn channel_open_direct_streamlocal(
        &mut self,
        channel: Channel<Msg>,
        socket_path: &str,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.auth.refuse_forwarding {
            return Ok(());
        }
        reply.accept().await;
        let path = socket_path.as_bytes().to_vec();
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, &path).await;
            let (mut read, mut write) = tokio::io::split(stream);
            let _ = tokio::io::copy(&mut read, &mut write).await;
        });
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let Ok(port) = u16::try_from(port_to_connect) else {
            return Ok(());
        };
        if self.auth.refuse_forwarding {
            return Ok(());
        }
        let Ok(mut tcp) = tokio::net::TcpStream::connect((host_to_connect, port)).await else {
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut tcp).await;
        });
        Ok(())
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.sessions.insert(channel.id());
        self.opened.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn subsystem_request(&mut self, channel: ChannelId, name: &str, session: &mut Session) -> Result<(), Self::Error> {
        let root = self.records.sftp_root.lock().unwrap().clone();
        match (name, root, self.opened.remove(&channel)) {
            ("sftp", Some(root), Some(opened)) => {
                self.sessions.remove(&channel);
                session.channel_success(channel)?;
                let server = sftp_server::SftpServer::new(root, self.records.sftp.clone());
                russh_sftp::server::run(opened.into_stream(), server).await;
            }
            _ => session.channel_failure(channel)?,
        }
        Ok(())
    }

    /// Remote forwarding: listens like sshd and opens a `forwarded-tcpip` channel per connection.
    async fn tcpip_forward(&mut self, address: &str, port: &mut u32, session: &mut Session) -> Result<bool, Self::Error> {
        if self.auth.refuse_forwarding {
            return Ok(false);
        }
        let Ok(wanted) = u16::try_from(*port) else {
            return Ok(false);
        };
        let Ok(listener) = TcpListener::bind((address, wanted)).await else {
            return Ok(false);
        };
        let bound = u32::from(listener.local_addr().map(|a| a.port()).unwrap_or(wanted));
        *port = bound;
        let handle = session.handle();
        let connected = address.to_owned();
        let task = tokio::spawn(async move {
            while let Ok((mut tcp, peer)) = listener.accept().await {
                let handle = handle.clone();
                let connected = connected.clone();
                tokio::spawn(async move {
                    let Ok(channel) = handle.channel_open_forwarded_tcpip(connected, bound, peer.ip().to_string(), u32::from(peer.port())).await
                    else {
                        return;
                    };
                    let mut stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut tcp).await;
                });
            }
        });
        self.listeners.push((address.to_owned(), bound, task));
        Ok(true)
    }

    async fn cancel_tcpip_forward(&mut self, address: &str, port: u32, _session: &mut Session) -> Result<bool, Self::Error> {
        let before = self.listeners.len();
        self.listeners.retain(|(a, p, task)| {
            let hit = a == address && *p == port;
            if hit {
                task.abort();
            }
            !hit
        });
        Ok(self.listeners.len() < before)
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        _cols: u32,
        _rows: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.records.terms.lock().unwrap().push(term.to_owned());
        session.channel_success(channel)
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        self.opened.remove(&channel);
        session.channel_success(channel)
    }

    async fn exec_request(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        self.opened.remove(&channel);
        session.channel_success(channel)?;
        // A command's input is recorded, not echoed like a shell's.
        self.sessions.remove(&channel);
        self.execs.insert(channel);
        self.records.commands.lock().unwrap().push(String::from_utf8_lossy(data).into_owned());
        let output = self.records.exec_output.lock().unwrap().clone().unwrap_or_else(|| TEST_EXEC_OUTPUT.to_owned());
        let stderr = self.records.exec_stderr.lock().unwrap().clone();
        let status = *self.records.exec_status.lock().unwrap();
        let delay = *self.records.exec_delay.lock().unwrap();
        let keep_open = self.records.exec_keep_open.load(std::sync::atomic::Ordering::SeqCst);
        let handle = session.handle();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = handle.data(channel, Bytes::from(output.into_bytes())).await;
            if keep_open {
                return;
            }
            if !stderr.is_empty() {
                let _ = handle.extended_data(channel, 1, Bytes::from(stderr.into_bytes())).await;
            }
            let _ = handle.exit_status_request(channel, status).await;
            let _ = handle.eof(channel).await;
            let _ = handle.close(channel).await;
        });
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        _cols: u32,
        _rows: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn data(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        if self.execs.contains(&channel) {
            self.records.exec_input.lock().unwrap().extend_from_slice(data);
            let mut pending = self.records.exec_pending.lock().unwrap();
            pending.push_str(&String::from_utf8_lossy(data));
            let mut replies = self.records.exec_replies.lock().unwrap();
            // Several commands may come in one packet, and one command in two: each is answered
            // once its trigger is complete, in the order they came.
            let mut due: Vec<(usize, String)> = Vec::new();
            let mut used = 0;
            replies.retain(|(trigger, answer)| match pending.find(trigger.as_str()) {
                Some(at) => {
                    due.push((at, answer.clone()));
                    used = used.max(at + trigger.len());
                    false
                }
                None => true,
            });
            pending.drain(..used);
            due.sort_by_key(|(at, _)| *at);
            for (_, answer) in due {
                session.data(channel, answer.into_bytes())?;
            }
            return Ok(());
        }
        if !self.sessions.contains(&channel) {
            return Ok(());
        }
        let handle = session.handle();
        match data {
            b"exit\n" => {
                tokio::spawn(async move {
                    let _ = handle.exit_status_request(channel, 0).await;
                    let _ = handle.eof(channel).await;
                    let _ = handle.close(channel).await;
                });
            }
            b"flood\n" => {
                tokio::spawn(async move {
                    for chunk in flood_pattern().chunks(FLOOD_CHUNK) {
                        if handle.data(channel, Bytes::from(chunk.to_vec())).await.is_err() {
                            return;
                        }
                    }
                });
            }
            _ if self.auth.quiet => {}
            other => session.data(channel, other.to_vec())?,
        }
        Ok(())
    }
}

/// An X11 connection setup (little-endian, protocol 11.0) with this protocol name and cookie.
pub fn x11_setup(name: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = vec![b'l', 0, 11, 0, 0, 0];
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    for part in [name, data] {
        out.extend_from_slice(part);
        out.resize(out.len() + part.len().div_ceil(4) * 4 - part.len(), 0);
    }
    out
}

/// An in-memory SSH agent holding `keys`. With `confirm == false` it refuses every signature.
pub async fn spawn_agent(keys: &[PrivateKey], confirm: bool) -> crate::DynAgent {
    use russh::keys::agent::client::AgentClient;

    let (client_side, server_side) = tokio::io::duplex(64 * 1024);
    let listener = futures::stream::iter(vec![Ok::<_, std::io::Error>(server_side)]);
    let mode = if confirm { AgentMode::Normal } else { AgentMode::RefuseSign };
    tokio::spawn(async move {
        let _ = russh::keys::agent::server::serve(listener, TestAgent { mode }).await;
    });
    let mut client = AgentClient::connect(client_side).dynamic();
    for key in keys {
        client.add_identity(key, &[]).await.expect("add identity");
    }
    client
}

/// An in-memory SSH agent that accepts keys but never answers a key list request,
/// like a Proton Pass that holds the pipe open while it hangs.
pub async fn spawn_stalled_agent(keys: &[PrivateKey]) -> crate::DynAgent {
    use russh::keys::agent::client::AgentClient;

    let (client_side, server_side) = tokio::io::duplex(64 * 1024);
    let listener = futures::stream::iter(vec![Ok::<_, std::io::Error>(server_side)]);
    tokio::spawn(async move {
        let agent = TestAgent { mode: AgentMode::Stall };
        let _ = russh::keys::agent::server::serve(listener, agent).await;
    });
    let mut client = AgentClient::connect(client_side).dynamic();
    for key in keys {
        client.add_identity(key, &[]).await.expect("add identity");
    }
    client
}

#[derive(Clone, Copy)]
enum AgentMode {
    Normal,
    RefuseSign,
    Stall,
}

#[derive(Clone)]
struct TestAgent {
    mode: AgentMode,
}

impl russh::keys::agent::server::Agent for TestAgent {
    fn confirm_request(&self, msg: russh::keys::agent::server::MessageType) -> impl std::future::Future<Output = bool> + Send {
        use russh::keys::agent::server::MessageType;
        let mode = self.mode;
        async move {
            match (mode, msg) {
                (AgentMode::RefuseSign, MessageType::Sign) => false,
                (AgentMode::Stall, MessageType::RequestKeys) => std::future::pending().await,
                _ => true,
            }
        }
    }
}

/// A proxy for tests, counting connections and noting where they went (`host:port` as asked).
pub struct TestProxy {
    pub port: u16,
    connects: Arc<std::sync::atomic::AtomicUsize>,
    targets: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl TestProxy {
    pub fn connects(&self) -> usize {
        self.connects.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn targets(&self) -> Vec<String> {
        self.targets.lock().unwrap().clone()
    }
}

impl Drop for TestProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn spawn_proxy<F, Fut>(serve: F) -> TestProxy
where
    F: Fn(tokio::net::TcpStream, Arc<Mutex<Vec<String>>>) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = std::io::Result<()>> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
    let port = listener.local_addr().expect("proxy addr").port();
    let connects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let targets = Arc::new(Mutex::new(Vec::new()));
    let (c, t) = (connects.clone(), targets.clone());
    let serve = Arc::new(serve);
    let task = tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let fut = serve(socket, t.clone());
            tokio::spawn(async move {
                let _ = fut.await;
            });
        }
    });
    TestProxy { port, connects, targets, task }
}

/// An HTTP proxy that understands `CONNECT`; with credentials it wants Basic auth.
pub async fn spawn_http_proxy(credentials: Option<(String, String)>) -> TestProxy {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let wanted = credentials.map(|(u, p)| {
        let token = data_encoding::BASE64.encode(format!("{u}:{p}").as_bytes());
        format!("proxy-authorization: basic {}", token.to_lowercase())
    });
    spawn_proxy(move |mut socket, targets| {
        let wanted = wanted.clone();
        async move {
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(socket.read_u8().await?);
            }
            let head = String::from_utf8_lossy(&head).to_string();
            let target =
                head.lines().next().and_then(|l| l.strip_prefix("CONNECT ")).and_then(|l| l.split_whitespace().next()).unwrap_or_default().to_owned();
            if let Some(wanted) = wanted
                && !head.to_lowercase().lines().any(|l| l == wanted)
            {
                socket.write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n").await?;
                return Ok(());
            }
            targets.lock().unwrap().push(target.clone());
            let mut upstream = tokio::net::TcpStream::connect(target.as_str()).await?;
            socket.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await?;
            tokio::io::copy_bidirectional(&mut socket, &mut upstream).await?;
            Ok(())
        }
    })
    .await
}

/// A SOCKS5 proxy (RFC 1928); with credentials it wants user and password (RFC 1929).
pub async fn spawn_socks5_proxy(credentials: Option<(String, String)>) -> TestProxy {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    spawn_proxy(move |mut socket, targets| {
        let credentials = credentials.clone();
        async move {
            let _version = socket.read_u8().await?;
            let count = socket.read_u8().await?;
            let mut methods = vec![0u8; count as usize];
            socket.read_exact(&mut methods).await?;
            match &credentials {
                Some((user, password)) => {
                    socket.write_all(&[5, 2]).await?;
                    let _sub = socket.read_u8().await?;
                    let len = socket.read_u8().await?;
                    let mut u = vec![0u8; len as usize];
                    socket.read_exact(&mut u).await?;
                    let len = socket.read_u8().await?;
                    let mut p = vec![0u8; len as usize];
                    socket.read_exact(&mut p).await?;
                    if u != user.as_bytes() || p != password.as_bytes() {
                        socket.write_all(&[1, 1]).await?;
                        return Ok(());
                    }
                    socket.write_all(&[1, 0]).await?;
                }
                None => socket.write_all(&[5, 0]).await?,
            }
            let mut request = [0u8; 4];
            socket.read_exact(&mut request).await?;
            let host = match request[3] {
                1 => {
                    let mut ip = [0u8; 4];
                    socket.read_exact(&mut ip).await?;
                    std::net::Ipv4Addr::from(ip).to_string()
                }
                3 => {
                    let len = socket.read_u8().await?;
                    let mut name = vec![0u8; len as usize];
                    socket.read_exact(&mut name).await?;
                    String::from_utf8_lossy(&name).into_owned()
                }
                _ => {
                    let mut ip = [0u8; 16];
                    socket.read_exact(&mut ip).await?;
                    std::net::Ipv6Addr::from(ip).to_string()
                }
            };
            let port = socket.read_u16().await?;
            targets.lock().unwrap().push(format!("{host}:{port}"));
            let mut upstream = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
            socket.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
            tokio::io::copy_bidirectional(&mut socket, &mut upstream).await?;
            Ok(())
        }
    })
    .await
}

/// An agent holding `keys`, opened afresh for each forwarded channel.
pub fn agent_opener(keys: &[PrivateKey]) -> Arc<dyn crate::AgentOpener> {
    Arc::new(TestAgentOpener { keys: keys.to_vec() })
}

struct TestAgentOpener {
    keys: Vec<PrivateKey>,
}

#[async_trait::async_trait]
impl crate::AgentOpener for TestAgentOpener {
    async fn open(&self) -> Result<Box<dyn crate::Io>, crate::SshError> {
        use russh::keys::agent::client::AgentClient;
        // One agent, two connections: the first loads the keys, the second is forwarded.
        let (load, load_server) = tokio::io::duplex(64 * 1024);
        let (forward, forward_server) = tokio::io::duplex(64 * 1024);
        let listener = futures::stream::iter(vec![Ok::<_, std::io::Error>(load_server), Ok(forward_server)]);
        tokio::spawn(async move {
            let agent = TestAgent { mode: AgentMode::Normal };
            let _ = russh::keys::agent::server::serve(listener, agent).await;
        });
        let mut client = AgentClient::connect(load);
        for key in &self.keys {
            client.add_identity(key, &[]).await.map_err(|e| crate::SshError::AgentUnavailable(e.to_string()))?;
        }
        Ok(Box::new(forward))
    }
}

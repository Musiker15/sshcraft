//! An in-process SSH server for tests. Accepts one client key, a password and rounds of
//! keyboard-interactive questions, as [`TestAuth`] says, and records what clients sent.
//!
//! The shell echoes every input chunk, except two commands sent as their own chunk:
//! `exit\n` ends the shell with exit status 0, `flood\n` sends [`flood_pattern`].

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use russh::keys::ssh_encoding::bytes::Bytes;
use russh::keys::{Algorithm, Certificate, HashAlg, PrivateKey, PublicKey};
use russh::server::{self, Auth, Msg, Server as _, Session, VerifiedPublicKey};
use russh::{Channel, ChannelId};
use russh::{MethodKind, MethodSet};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

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
}

/// A user certificate for `key`, signed by `ca`, for `principal`, valid in the given window.
pub fn user_certificate(
    key: &PublicKey,
    ca: &PrivateKey,
    principal: &str,
    valid_after: u64,
    valid_before: u64,
) -> Certificate {
    use russh::keys::ssh_key::certificate::{Builder, CertType};
    let mut builder = Builder::new_with_random_nonce(
        &mut rand::rng(),
        key.key_data().clone(),
        valid_after,
        valid_before,
    )
    .expect("certificate builder");
    builder.cert_type(CertType::User).expect("cert type");
    builder.key_id("test").expect("key id");
    builder.valid_principal(principal).expect("principal");
    builder.sign(ca).expect("signed certificate")
}

/// What clients sent to the test server.
#[derive(Default)]
struct Records {
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
        Self::start_with(TestAuth {
            key: Some(authorized),
            ..Default::default()
        })
        .await
    }

    pub async fn start_with(auth: TestAuth) -> TestServer {
        let host_key = random_host_key();
        let mut server = TestServer {
            port: 0,
            host_fingerprint: fingerprint(&host_key),
            auth,
            records: Arc::new(Records::default()),
            host_key,
            running: None,
        };
        server.listen().await;
        server
    }

    /// Passwords and keyboard-interactive answers clients sent, in order.
    pub fn received_passwords(&self) -> Vec<String> {
        self.records.passwords.lock().unwrap().clone()
    }

    /// Answers every later `exec` with `output`.
    pub fn set_exec_output(&self, output: &str) {
        *self.records.exec_output.lock().unwrap() = Some(output.to_owned());
    }

    /// Commands clients ran over `exec`, in order.
    pub fn received_commands(&self) -> Vec<String> {
        self.records.commands.lock().unwrap().clone()
    }

    /// How many certificates clients offered.
    pub fn received_certificates(&self) -> usize {
        *self.records.certificates.lock().unwrap()
    }

    pub fn received_env(&self) -> Vec<(String, String)> {
        self.records.env.lock().unwrap().clone()
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
        let socket = TcpListener::bind(("127.0.0.1", self.port))
            .await
            .expect("bind test server");
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
        Handler {
            auth: self.auth.clone(),
            records: self.records.clone(),
            key_ok: false,
            kbd_round: 0,
            tunnels: std::collections::HashSet::new(),
        }
    }
}

struct Handler {
    auth: TestAuth,
    records: Arc<Records>,
    /// The key was accepted and keyboard-interactive must follow.
    key_ok: bool,
    kbd_round: usize,
    /// `direct-tcpip` channels: their bytes go to the tunnel, not to the echo.
    tunnels: std::collections::HashSet<ChannelId>,
}

impl Handler {
    fn kbd_question(&self) -> Auth {
        let round = &self.auth.kbd[self.kbd_round];
        let prompts: Vec<(Cow<'static, str>, bool)> = round
            .iter()
            .map(|(q, echo, _)| (Cow::Owned(q.clone()), *echo))
            .collect();
        Auth::Partial {
            name: Cow::Borrowed(""),
            instructions: Cow::Borrowed(""),
            prompts: Cow::Owned(prompts),
        }
    }
}

impl server::Handler for Handler {
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self,
        _user: &str,
        key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        let matches = self
            .auth
            .key
            .as_ref()
            .is_some_and(|k| key.public_key.key_data() == k.key_data());
        if !matches {
            return Ok(Auth::reject());
        }
        if self.auth.key_then_kbd {
            self.key_ok = true;
            return Ok(Auth::Reject {
                proceed_with_methods: Some(MethodSet::from(&[MethodKind::KeyboardInteractive][..])),
                partial_success: true,
            });
        }
        Ok(Auth::Accept)
    }

    async fn auth_openssh_certificate(
        &mut self,
        user: &str,
        certificate: &Certificate,
    ) -> Result<Auth, Self::Error> {
        *self.records.certificates.lock().unwrap() += 1;
        let trusted = self
            .auth
            .ca
            .as_ref()
            .is_some_and(|ca| certificate.signature_key() == ca.key_data());
        let for_user = certificate.valid_principals().iter().any(|p| p == user);
        Ok(
            if trusted && for_user && crate::certificate_valid_now(certificate) {
                Auth::Accept
            } else {
                Auth::reject()
            },
        )
    }

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        self.records
            .passwords
            .lock()
            .unwrap()
            .push(password.to_owned());
        if self.auth.password.as_deref() == Some(password) {
            if self.auth.password_then_kbd {
                self.key_ok = true;
                return Ok(Auth::Reject {
                    proceed_with_methods: Some(MethodSet::from(
                        &[MethodKind::KeyboardInteractive][..],
                    )),
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
        let answers: Vec<String> = response
            .map(|a| String::from_utf8_lossy(&a).into_owned())
            .collect();
        self.records
            .passwords
            .lock()
            .unwrap()
            .extend(answers.iter().cloned());
        let expected: Vec<&String> = self.auth.kbd[self.kbd_round]
            .iter()
            .map(|(_, _, a)| a)
            .collect();
        if answers.iter().collect::<Vec<_>>() != expected {
            self.kbd_round = 0;
            return Ok(Auth::reject());
        }
        self.kbd_round += 1;
        if self.kbd_round == self.auth.kbd.len() {
            Ok(Auth::Accept)
        } else {
            Ok(self.kbd_question())
        }
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
                    let mut agent =
                        russh::keys::agent::client::AgentClient::connect(channel.into_stream());
                    let listed = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        agent.request_identities(),
                    )
                    .await;
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
        Ok(())
    }

    async fn env_request(
        &mut self,
        _channel: ChannelId,
        name: &str,
        value: &str,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.records
            .env
            .lock()
            .unwrap()
            .push((name.to_owned(), value.to_owned()));
        Ok(())
    }

    async fn agent_request(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    /// Jump host: connects to the wanted port on this machine and passes the bytes through.
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
        let Ok(mut tcp) = tokio::net::TcpStream::connect((host_to_connect, port)).await else {
            return Ok(());
        };
        self.tunnels.insert(channel.id());
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut tcp).await;
        });
        Ok(())
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _cols: u32,
        _rows: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        self.records
            .commands
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(data).into_owned());
        let output = self
            .records
            .exec_output
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| TEST_EXEC_OUTPUT.to_owned());
        let handle = session.handle();
        tokio::spawn(async move {
            let _ = handle.data(channel, Bytes::from(output.into_bytes())).await;
            let _ = handle.exit_status_request(channel, 0).await;
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

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.tunnels.contains(&channel) {
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
                        if handle
                            .data(channel, Bytes::from(chunk.to_vec()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                });
            }
            other => session.data(channel, other.to_vec())?,
        }
        Ok(())
    }
}

/// An in-memory SSH agent holding `keys`. With `confirm == false` it refuses every signature.
pub async fn spawn_agent(keys: &[PrivateKey], confirm: bool) -> crate::DynAgent {
    use russh::keys::agent::client::AgentClient;

    let (client_side, server_side) = tokio::io::duplex(64 * 1024);
    let listener = futures::stream::iter(vec![Ok::<_, std::io::Error>(server_side)]);
    let mode = if confirm {
        AgentMode::Normal
    } else {
        AgentMode::RefuseSign
    };
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
        let agent = TestAgent {
            mode: AgentMode::Stall,
        };
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
    fn confirm_request(
        &self,
        msg: russh::keys::agent::server::MessageType,
    ) -> impl std::future::Future<Output = bool> + Send {
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
    TestProxy {
        port,
        connects,
        targets,
        task,
    }
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
            let target = head
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("CONNECT "))
                .and_then(|l| l.split_whitespace().next())
                .unwrap_or_default()
                .to_owned();
            if let Some(wanted) = wanted
                && !head.to_lowercase().lines().any(|l| l == wanted)
            {
                socket
                    .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                    .await?;
                return Ok(());
            }
            targets.lock().unwrap().push(target.clone());
            let mut upstream = tokio::net::TcpStream::connect(target.as_str()).await?;
            socket
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await?;
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
    Arc::new(TestAgentOpener {
        keys: keys.to_vec(),
    })
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
        let listener = futures::stream::iter(vec![
            Ok::<_, std::io::Error>(load_server),
            Ok(forward_server),
        ]);
        tokio::spawn(async move {
            let agent = TestAgent {
                mode: AgentMode::Normal,
            };
            let _ = russh::keys::agent::server::serve(listener, agent).await;
        });
        let mut client = AgentClient::connect(load);
        for key in &self.keys {
            client
                .add_identity(key, &[])
                .await
                .map_err(|e| crate::SshError::AgentUnavailable(e.to_string()))?;
        }
        Ok(Box::new(forward))
    }
}

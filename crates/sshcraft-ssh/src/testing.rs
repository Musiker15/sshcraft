//! An in-process SSH server for tests. Accepts one client key, a password and rounds of
//! keyboard-interactive questions, as [`TestAuth`] says, and records what clients sent.
//!
//! The shell echoes every input chunk, except two commands sent as their own chunk:
//! `exit\n` ends the shell with exit status 0, `flood\n` sends [`flood_pattern`].

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use russh::keys::ssh_encoding::bytes::Bytes;
use russh::keys::{Algorithm, HashAlg, PrivateKey, PublicKey};
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
}

/// What clients sent to the test server.
#[derive(Default)]
struct Records {
    /// Passwords and keyboard-interactive answers, in the order they came.
    passwords: Mutex<Vec<String>>,
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
        if !self.auth.kbd.is_empty() && !self.auth.key_then_kbd {
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
        }
    }
}

struct Handler {
    auth: TestAuth,
    records: Arc<Records>,
    /// The key was accepted and keyboard-interactive must follow.
    key_ok: bool,
    kbd_round: usize,
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

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        self.records
            .passwords
            .lock()
            .unwrap()
            .push(password.to_owned());
        if self.auth.password.as_deref() == Some(password) {
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
        if self.auth.kbd.is_empty() || (self.auth.key_then_kbd && !self.key_ok) {
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
        _data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        let handle = session.handle();
        tokio::spawn(async move {
            let _ = handle
                .data(channel, Bytes::from_static(TEST_EXEC_OUTPUT.as_bytes()))
                .await;
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

//! SSH session engine for SSHCraft: connect with a key, a password or keyboard-interactive
//! answers, verify the server's host key through the caller, run an interactive shell and stream
//! its raw output bytes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use russh::client::{self, Msg};
use russh::keys::{HashAlg, PrivateKey, PublicKeyOrCertificate};
use russh::{ChannelMsg, ChannelWriteHalf};
use sshcraft_core::policy::Disconnect;
use tokio::sync::mpsc;

#[cfg(windows)]
pub mod agent;
mod auth;
mod proxy;
pub use auth::{Auth, AuthPromptLine, AuthPrompter, AuthRequest, NoPrompter};
#[cfg(feature = "testing")]
pub mod testing;

/// An SSH agent connection, independent of its transport (named pipe, socket, test stream).
pub type DynAgent = russh::keys::agent::client::AgentClient<
    Box<dyn russh::keys::agent::client::AgentStream + Send + Unpin + 'static>,
>;

/// How to authenticate: with a private key, or with a key held by an agent.
// Lives only while connecting, so the size difference between the variants does not matter.
#[allow(clippy::large_enum_variant)]
pub enum Credential {
    Key(PrivateKey),
    Agent {
        fingerprint: String,
        agent: DynAgent,
    },
}

/// Public facts about a key held by an agent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AgentKey {
    pub fingerprint: String,
    pub algorithm: String,
    pub comment: String,
    pub public_key: String,
}

/// How long the agent may take to list its keys.
const AGENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a signature may take; generous, because the user may have to confirm it.
pub(crate) const AGENT_SIGN_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) async fn agent_identities(
    agent: &mut DynAgent,
) -> Result<Vec<russh::keys::agent::AgentIdentity>, SshError> {
    tokio::time::timeout(AGENT_REQUEST_TIMEOUT, agent.request_identities())
        .await
        .map_err(|_| SshError::AgentUnavailable("no answer".into()))?
        .map_err(|e| SshError::AgentUnavailable(e.to_string()))
}

pub async fn list_agent_keys(agent: &mut DynAgent) -> Result<Vec<AgentKey>, SshError> {
    let identities = agent_identities(agent).await?;
    Ok(identities
        .iter()
        .map(|identity| {
            let key = identity.public_key();
            AgentKey {
                fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
                algorithm: key.algorithm().as_str().to_owned(),
                comment: match identity {
                    russh::keys::agent::AgentIdentity::PublicKey { comment, .. } => comment.clone(),
                    _ => String::new(),
                },
                public_key: key.to_openssh().unwrap_or_default(),
            }
        })
        .collect())
}

const TERM: &str = "xterm-256color";
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
const KEEPALIVE_MAX: usize = 3;
const EVENT_BUFFER: usize = 1024;
const EXEC_TIMEOUT: Duration = Duration::from_secs(5);
/// How long reaching one hop may take: TCP, a proxy's handshake or a channel through the hop
/// before.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// A byte stream an SSH connection can run over: TCP, a proxy tunnel or a channel through a jump
/// host.
pub trait Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Io for T {}

/// One host on the way: its name (for host key questions and errors), where it is and how to
/// sign in there. Each hop only ever gets its own credentials.
pub struct Hop {
    pub host_name: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub auth: Auth,
}

/// A proxy for the first connection of a route.
pub enum ProxyConfig {
    Http(ProxyTarget),
    Socks5(ProxyTarget),
}

pub struct ProxyTarget {
    pub address: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<zeroize::Zeroizing<String>>,
}

/// Opens a fresh connection to the SSH agent whose keys a session may forward (the system agent,
/// Proton Pass on Windows; never SSHCraft's own vault).
#[async_trait::async_trait]
pub trait AgentOpener: Send + Sync {
    async fn open(&self) -> Result<Box<dyn Io>, SshError>;
}

/// What a session does besides the shell itself.
#[derive(Default)]
pub struct SessionOptions {
    /// Forward this agent to the target (never to a jump host). `None`: no forwarding, and an
    /// agent channel the server opens anyway is refused.
    pub agent_forwarding: Option<Arc<dyn AgentOpener>>,
    /// Sent before the shell starts; the server takes only what its `AcceptEnv` allows.
    pub env: Vec<(String, String)>,
}

/// How to reach a host: the proxy (for the first connection only), then each hop in turn, the
/// last one being the target.
pub struct RouteParams {
    pub hops: Vec<Hop>,
    pub proxy: Option<ProxyConfig>,
    pub options: SessionOptions,
    pub connect_timeout: Duration,
}

pub struct ConnectParams {
    pub address: String,
    pub port: u16,
    pub username: String,
    pub auth: Auth,
}

/// Decides whether to trust the server's host key. Fingerprints look like `SHA256:…`;
/// `host_name` names the hop the question is about.
#[async_trait::async_trait]
pub trait HostKeyVerifier: Send + Sync {
    async fn verify(&self, host_name: &str, address: &str, port: u16, fingerprint: &str) -> bool;

    /// Whether the host key of this server was trusted only during this connection (a first
    /// contact). A stored password never goes to such a server: the address may have been
    /// changed by someone else, so the user is asked instead.
    fn first_contact(&self, _address: &str, _port: u16) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SshError {
    #[error("the server's host key was rejected")]
    HostKeyRejected,
    #[error("the server rejected the key")]
    AuthFailed,
    #[error("could not connect: {0}")]
    Connect(String),
    #[error("ssh protocol error: {0}")]
    Protocol(String),
    #[error("the ssh agent is not reachable: {0}")]
    AgentUnavailable(String),
    #[error("the ssh agent does not hold this key")]
    AgentKeyMissing,
    #[error("the ssh agent refused to sign")]
    AgentRefused,
    #[error("the command did not finish in time")]
    Timeout,
    #[error("sign-in was cancelled")]
    AuthCancelled,
    #[error("the server rejected the password")]
    PasswordRejected,
    #[error("the proxy failed: {0}")]
    Proxy(String),
    #[error("the proxy rejected the credentials")]
    ProxyAuthFailed,
    #[error("jump host {host_name}: {error}")]
    Hop {
        index: usize,
        host_name: String,
        error: Box<SshError>,
    },
}

impl From<&SshError> for Disconnect {
    fn from(e: &SshError) -> Self {
        match e {
            SshError::HostKeyRejected => Disconnect::HostKeyRejected,
            SshError::AuthFailed
            | SshError::AgentUnavailable(_)
            | SshError::AgentKeyMissing
            | SshError::AgentRefused
            | SshError::AuthCancelled
            | SshError::PasswordRejected => Disconnect::AuthFailed,
            SshError::Connect(m) | SshError::Protocol(m) => Disconnect::Lost(m.clone()),
            SshError::Timeout => Disconnect::Lost("timed out".into()),
            SshError::ProxyAuthFailed => Disconnect::AuthFailed,
            SshError::Proxy(m) => Disconnect::Lost(m.clone()),
            SshError::Hop { error, .. } => Disconnect::from(error.as_ref()),
        }
    }
}

pub(crate) fn protocol(e: impl std::fmt::Display) -> SshError {
    SshError::Protocol(e.to_string())
}

pub use sshcraft_core::policy::ShellEvent;

struct ClientHandler {
    /// Set on the target when forwarding is on; jump hosts never get the agent.
    agent: Option<Arc<dyn AgentOpener>>,
    host_name: String,
    address: String,
    port: u16,
    verifier: Arc<dyn HostKeyVerifier>,
    rejected: Arc<AtomicBool>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key_data = match server_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.key_data(),
            PublicKeyOrCertificate::Certificate(cert) => cert.public_key(),
        };
        let fingerprint = key_data.fingerprint(HashAlg::Sha256).to_string();
        let trusted = self
            .verifier
            .verify(&self.host_name, &self.address, self.port, &fingerprint)
            .await;
        if !trusted {
            self.rejected.store(true, Ordering::SeqCst);
        }
        Ok(trusted)
    }

    /// Only the target with forwarding switched on gets a channel to the agent; dropping the
    /// reply refuses it.
    async fn server_channel_open_agent_forward(
        &mut self,
        channel: russh::Channel<Msg>,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let Some(opener) = self.agent.clone() else {
            return Ok(());
        };
        let Ok(mut agent) = opener.open().await else {
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut agent).await;
        });
        Ok(())
    }
}

/// An authenticated SSH connection. Shells opened from it keep it alive.
pub struct Session {
    handle: Arc<client::Handle<ClientHandler>>,
    env: Arc<Vec<(String, String)>>,
    forward_agent: bool,
    /// The jump hosts' connections the session runs through, kept open while it lives.
    jumps: Arc<Vec<client::Handle<ClientHandler>>>,
}

pub struct Shell {
    pub events: mpsc::Receiver<ShellEvent>,
    writer: ShellWriter,
}

/// The sending half of a shell, separate from its event stream.
#[derive(Clone)]
pub struct ShellWriter {
    channel: Arc<ChannelWriteHalf<Msg>>,
    _session: Arc<client::Handle<ClientHandler>>,
    _jumps: Arc<Vec<client::Handle<ClientHandler>>>,
}

/// The client settings for every hop: keep-alive, and russh's default algorithms, which put the
/// post-quantum key exchange `mlkem768x25519-sha256` first.
pub fn client_config() -> client::Config {
    client::Config {
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        keepalive_max: KEEPALIVE_MAX,
        ..Default::default()
    }
}

/// Connects to one host directly.
pub async fn connect(
    params: ConnectParams,
    verifier: Arc<dyn HostKeyVerifier>,
    prompter: Arc<dyn AuthPrompter>,
) -> Result<Session, SshError> {
    let hop = Hop {
        host_name: params.address.clone(),
        address: params.address,
        port: params.port,
        username: params.username,
        auth: params.auth,
    };
    connect_route(
        RouteParams {
            hops: vec![hop],
            proxy: None,
            options: SessionOptions::default(),
            connect_timeout: CONNECT_TIMEOUT,
        },
        verifier,
        prompter,
    )
    .await
}

/// Connects along a route: the first hop over TCP or the proxy, each further hop through a
/// `direct-tcpip` channel of the one before. Each hop checks its own host key and signs in with
/// its own credentials. A failure at a jump host comes as [`SshError::Hop`].
pub async fn connect_route(
    params: RouteParams,
    verifier: Arc<dyn HostKeyVerifier>,
    prompter: Arc<dyn AuthPrompter>,
) -> Result<Session, SshError> {
    let config = Arc::new(client_config());
    let last = params
        .hops
        .len()
        .checked_sub(1)
        .ok_or_else(|| SshError::Connect("no host to connect to".into()))?;
    let timeout = params.connect_timeout;
    let mut connected: Vec<client::Handle<ClientHandler>> = Vec::new();
    for (index, hop) in params.hops.into_iter().enumerate() {
        let host_name = hop.host_name.clone();
        let wrap = |error: SshError| {
            if index < last {
                SshError::Hop {
                    index,
                    host_name: host_name.clone(),
                    error: Box::new(error),
                }
            } else {
                error
            }
        };
        let stream: Box<dyn Io> = match connected.last() {
            None => proxy::open(params.proxy.as_ref(), &hop.address, hop.port, timeout)
                .await
                .map_err(wrap)?,
            Some(before) => {
                let opened = before.channel_open_direct_tcpip(
                    hop.address.clone(),
                    u32::from(hop.port),
                    "127.0.0.1",
                    0,
                );
                let channel = tokio::time::timeout(timeout, opened)
                    .await
                    .map_err(|_| wrap(SshError::Connect("timed out".into())))?
                    .map_err(|e| wrap(SshError::Connect(e.to_string())))?;
                Box::new(channel.into_stream())
            }
        };
        let rejected = Arc::new(AtomicBool::new(false));
        let handler = ClientHandler {
            agent: if index == last {
                params.options.agent_forwarding.clone()
            } else {
                None
            },
            host_name: hop.host_name.clone(),
            address: hop.address.clone(),
            port: hop.port,
            verifier: verifier.clone(),
            rejected: rejected.clone(),
        };
        let mut handle = client::connect_stream(config.clone(), stream, handler)
            .await
            .map_err(|e| {
                wrap(if rejected.load(Ordering::SeqCst) {
                    SshError::HostKeyRejected
                } else {
                    SshError::Connect(e.to_string())
                })
            })?;
        let mut credentials = hop.auth;
        if verifier.first_contact(&hop.address, hop.port) {
            credentials.password = None;
        }
        auth::authenticate(
            &mut handle,
            &hop.username,
            &hop.host_name,
            credentials,
            prompter.as_ref(),
        )
        .await
        .map_err(wrap)?;
        connected.push(handle);
    }
    let handle = connected.pop().expect("at least one hop");
    Ok(Session {
        handle: Arc::new(handle),
        env: Arc::new(params.options.env),
        forward_agent: params.options.agent_forwarding.is_some(),
        jumps: Arc::new(connected),
    })
}

impl Session {
    /// Runs a command on its own channel and returns at most `limit` bytes of its output;
    /// gives up after a few seconds.
    pub async fn exec(&self, command: &str, limit: usize) -> Result<Vec<u8>, SshError> {
        let run = async {
            let mut channel = self.handle.channel_open_session().await.map_err(protocol)?;
            channel.exec(true, command).await.map_err(protocol)?;
            let mut out = Vec::new();
            while let Some(msg) = channel.wait().await {
                match msg {
                    ChannelMsg::Data { data } => {
                        let room = limit.saturating_sub(out.len());
                        out.extend_from_slice(&data[..data.len().min(room)]);
                        if out.len() >= limit {
                            break;
                        }
                    }
                    ChannelMsg::Eof | ChannelMsg::Close => break,
                    _ => {}
                }
            }
            let _ = channel.close().await;
            Ok(out)
        };
        tokio::time::timeout(EXEC_TIMEOUT, run)
            .await
            .map_err(|_| SshError::Timeout)?
    }

    pub async fn open_shell(&self, cols: u32, rows: u32) -> Result<Shell, SshError> {
        let channel = self.handle.channel_open_session().await.map_err(protocol)?;
        channel
            .request_pty(false, TERM, cols, rows, 0, 0, &[])
            .await
            .map_err(protocol)?;
        for (name, value) in self.env.iter() {
            channel
                .set_env(false, name.as_str(), value.as_str())
                .await
                .map_err(protocol)?;
        }
        if self.forward_agent {
            channel.agent_forward(false).await.map_err(protocol)?;
        }
        channel.request_shell(false).await.map_err(protocol)?;
        let (mut read, write) = channel.split();
        let (tx, rx) = mpsc::channel(EVENT_BUFFER);

        tokio::spawn(async move {
            let mut exit_status = None;
            let mut closed = false;
            while let Some(msg) = read.wait().await {
                match msg {
                    ChannelMsg::Data { data } => {
                        if tx.send(ShellEvent::Data(data.to_vec())).await.is_err() {
                            return;
                        }
                    }
                    ChannelMsg::ExitStatus { exit_status: code } => exit_status = Some(code),
                    ChannelMsg::Close => {
                        closed = true;
                        break;
                    }
                    _ => {}
                }
            }
            let reason = if exit_status.is_some() || closed {
                Disconnect::Exited(exit_status)
            } else {
                Disconnect::Lost("connection closed".into())
            };
            let _ = tx.send(ShellEvent::Ended(reason)).await;
        });

        Ok(Shell {
            events: rx,
            writer: ShellWriter {
                channel: Arc::new(write),
                _session: self.handle.clone(),
                _jumps: self.jumps.clone(),
            },
        })
    }

    pub async fn disconnect(self) {
        let _ = self
            .handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
        for jump in self.jumps.iter().rev() {
            let _ = jump
                .disconnect(russh::Disconnect::ByApplication, "", "en")
                .await;
        }
    }
}

impl Shell {
    pub async fn write(&self, data: &[u8]) -> Result<(), SshError> {
        self.writer.write(data).await
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> Result<(), SshError> {
        self.writer.resize(cols, rows).await
    }

    pub fn split(self) -> (ShellWriter, mpsc::Receiver<ShellEvent>) {
        (self.writer, self.events)
    }
}

impl ShellWriter {
    pub async fn write(&self, data: &[u8]) -> Result<(), SshError> {
        self.channel.data(data).await.map_err(protocol)
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> Result<(), SshError> {
        self.channel
            .window_change(cols, rows, 0, 0)
            .await
            .map_err(protocol)
    }
}

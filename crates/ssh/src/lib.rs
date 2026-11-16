//! SSH session engine for SSHCraft: connect with a key, a password or keyboard-interactive
//! answers, verify the server's host key through the caller, run an interactive shell and stream
//! its raw output bytes.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use russh::client::{self, Msg};
use russh::keys::{HashAlg, PrivateKey, PublicKeyOrCertificate};
use russh::{ChannelMsg, ChannelWriteHalf};
use sshcraft_model::policy::Disconnect;
use tokio::sync::mpsc;

#[cfg(windows)]
pub mod agent;
mod auth;
pub mod authorized_keys;
pub mod hostkeys;
mod proxy;
pub mod socks;
mod tunnel;
pub mod x11;
pub use auth::{Auth, AuthPromptLine, AuthPrompter, AuthRequest, CodeMaker, NoPrompter, certificate_valid_now};
pub use tunnel::{MAX_TUNNEL_CONNECTIONS, Tunnel};

/// Signs for a key whose private half SSHCraft does not hold (a FIDO2 key, Windows Hello).
#[async_trait::async_trait]
pub trait KeySigner: Send {
    /// The SSH signature blob for `data`: `string algorithm, string signature`, and for `sk-`
    /// keys also `byte flags, uint32 counter`. Without the length in front.
    async fn sign(&mut self, data: &[u8]) -> Result<Vec<u8>, SshError>;
}
#[cfg(feature = "testing")]
pub mod testing;

/// An SSH agent connection, independent of its transport (named pipe, socket, test stream).
pub type DynAgent = russh::keys::agent::client::AgentClient<Box<dyn russh::keys::agent::client::AgentStream + Send + Unpin + 'static>>;

/// How to authenticate: with a private key, or with a key held by an agent.
// Lives only while connecting, so the size difference between the variants does not matter.
#[allow(clippy::large_enum_variant)]
pub enum Credential {
    Key(PrivateKey),
    Agent {
        fingerprint: String,
        agent: DynAgent,
    },
    /// A key that signs elsewhere: only its public half is here.
    Signer {
        public_key: russh::keys::PublicKey,
        signer: Box<dyn KeySigner>,
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
/// How long a security key may take: longer than the 120 s WebAuthn waits itself, so its own
/// "cancelled" arrives first.
pub(crate) const SIGNER_TIMEOUT: Duration = Duration::from_secs(130);

pub(crate) async fn agent_identities(agent: &mut DynAgent) -> Result<Vec<russh::keys::agent::AgentIdentity>, SshError> {
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

/// How often [`Session::closed`] looks whether the connection is still there.
const CLOSED_POLL: Duration = Duration::from_millis(250);
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
    /// A local program whose standard input and output carry the connection; `argv` has its
    /// tokens already replaced and is started without a shell.
    Command {
        argv: Vec<String>,
    },
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
    /// Forward X11 to this display (on the target only). `None`: an X11 channel the server
    /// opens is refused.
    pub x11: Option<x11::X11Options>,
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

    /// [`verify`](Self::verify) with the whole key, for a verifier that keeps it.
    async fn verify_key(&self, host_name: &str, address: &str, port: u16, key: &hostkeys::PresentedKey) -> bool {
        self.verify(host_name, address, port, &key.fingerprint).await
    }

    /// The target announced all its host keys after sign-in (`UpdateHostKeys`); `used` is the
    /// fingerprint this connection was verified with.
    fn announced(&self, _address: &str, _port: u16, _used: &str, _keys: &[hostkeys::PresentedKey]) -> hostkeys::AnnounceAction {
        hostkeys::AnnounceAction::Ignore
    }

    /// Applies an announcement: `proven` are the new keys the server proved it holds; only
    /// they may be added.
    fn apply(&self, _address: &str, _port: u16, _used: &str, _keys: &[hostkeys::PresentedKey], _proven: &[hostkeys::PresentedKey]) {}

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
    #[error("signing was cancelled")]
    SignerCancelled,
    #[error("signing failed: {0}")]
    SignerFailed(String),
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
    #[error("the port is already in use")]
    PortInUse,
    #[error("could not listen: {0}")]
    Bind(String),
    #[error("the server refused the remote forwarding")]
    ForwardRefused,
    #[error("jump host {host_name}: {error}")]
    Hop { index: usize, host_name: String, error: Box<SshError> },
}

impl From<&SshError> for Disconnect {
    fn from(e: &SshError) -> Self {
        match e {
            SshError::HostKeyRejected => Disconnect::HostKeyRejected,
            SshError::AuthFailed
            | SshError::AgentUnavailable(_)
            | SshError::AgentKeyMissing
            | SshError::AgentRefused
            | SshError::SignerCancelled
            | SshError::SignerFailed(_)
            | SshError::AuthCancelled
            | SshError::PasswordRejected => Disconnect::AuthFailed,
            SshError::Connect(m) | SshError::Protocol(m) => Disconnect::Lost(m.clone()),
            SshError::Timeout => Disconnect::Lost("timed out".into()),
            SshError::ProxyAuthFailed => Disconnect::AuthFailed,
            SshError::Proxy(m) => Disconnect::Lost(m.clone()),
            SshError::PortInUse | SshError::ForwardRefused => Disconnect::Lost(e.to_string()),
            SshError::Bind(m) => Disconnect::Lost(m.clone()),
            SshError::Hop { error, .. } => Disconnect::from(error.as_ref()),
        }
    }
}

pub(crate) fn protocol(e: impl std::fmt::Display) -> SshError {
    SshError::Protocol(e.to_string())
}

pub use sshcraft_model::policy::ShellEvent;

struct ClientHandler {
    /// Set on the target when forwarding is on; jump hosts never get the agent.
    agent: Option<Arc<dyn AgentOpener>>,
    /// Set on the target when X11 forwarding is on.
    x11: Option<Arc<x11::Forward>>,
    host_name: String,
    address: String,
    port: u16,
    verifier: Arc<dyn HostKeyVerifier>,
    rejected: Arc<AtomicBool>,
    /// Targets of the remote forwardings on this connection.
    remotes: tunnel::Remotes,
    /// Where the target's host key announcements go; none for a jump host.
    announce: Option<mpsc::UnboundedSender<Vec<russh::keys::PublicKey>>>,
    /// The fingerprint this connection was verified with.
    used: Arc<std::sync::Mutex<Option<String>>>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let key_data = match server_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.key_data(),
            PublicKeyOrCertificate::Certificate(cert) => cert.public_key(),
        };
        let presented = hostkeys::PresentedKey::of(key_data);
        let trusted = self.verifier.verify_key(&self.host_name, &self.address, self.port, &presented).await;
        if trusted {
            *self.used.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(presented.fingerprint);
        } else {
            self.rejected.store(true, Ordering::SeqCst);
        }
        Ok(trusted)
    }

    /// Hands the announcement to a task that can ask for proof (the handle is busy here).
    async fn openssh_ext_host_keys_announced(
        &mut self,
        keys: Vec<russh::keys::PublicKey>,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        // One announcement per connection, as in OpenSSH: a server sending more gets nothing
        // queued, proved or written for them.
        if let Some(announce) = self.announce.take() {
            let _ = announce.send(keys);
        }
        Ok(())
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

    /// An X11 connection; refused unless a shell asked for forwarding (dropping the reply
    /// refuses it).
    async fn server_channel_open_x11(
        &mut self,
        channel: russh::Channel<Msg>,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if let Some(forward) = &self.x11 {
            forward.accept(channel, reply).await;
        }
        Ok(())
    }

    /// A connection to a port the server listens on for us; one we did not ask for is refused
    /// (dropping the reply refuses it).
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let Some(target) = tunnel::remote_target(&self.remotes, connected_address, connected_port) else {
            return Ok(());
        };
        if target.count().load(std::sync::atomic::Ordering::SeqCst) >= MAX_TUNNEL_CONNECTIONS {
            return Ok(());
        }
        reply.accept().await;
        let count = target.count();
        tunnel::serve_remote(channel, target, count);
        Ok(())
    }
}

/// Runs a command on its own channel and returns at most `limit` bytes of its output; gives up
/// after a few seconds.
async fn exec_on(handle: &client::Handle<ClientHandler>, command: &str, input: Option<&[u8]>, limit: usize) -> Result<Vec<u8>, SshError> {
    let run = async {
        let mut channel = handle.channel_open_session().await.map_err(protocol)?;
        channel.exec(true, command).await.map_err(protocol)?;
        if let Some(input) = input {
            channel.data(input).await.map_err(protocol)?;
            channel.eof().await.map_err(protocol)?;
        }
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
    tokio::time::timeout(EXEC_TIMEOUT, run).await.map_err(|_| SshError::Timeout)?
}

/// What a command run with [`Session::run`] gave back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunOutput {
    /// Standard output and standard error as they came, at most the limit.
    pub output: Vec<u8>,
    /// More came than the limit allowed.
    pub truncated: bool,
    /// The exit status the server reported, if it did.
    pub exit_status: Option<u32>,
}

/// Runs a command to its end: output and errors together (up to `limit` bytes, the rest is
/// read and dropped) and the exit status. No time limit of its own; the caller sets one.
async fn run_on(handle: &client::Handle<ClientHandler>, command: &str, limit: usize) -> Result<RunOutput, SshError> {
    let mut channel = handle.channel_open_session().await.map_err(protocol)?;
    channel.exec(true, command).await.map_err(protocol)?;
    let mut result = RunOutput::default();
    let take = |data: &[u8], result: &mut RunOutput| {
        let room = limit.saturating_sub(result.output.len());
        if data.len() > room {
            result.truncated = true;
        }
        result.output.extend_from_slice(&data[..data.len().min(room)]);
    };
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data } => take(&data, &mut result),
            ChannelMsg::ExtendedData { data, .. } => take(&data, &mut result),
            ChannelMsg::ExitStatus { exit_status } => result.exit_status = Some(exit_status),
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    let _ = channel.close().await;
    Ok(result)
}

/// Checks whether a connection still answers, after the computer slept or its network changed.
#[derive(Clone)]
pub struct Prober {
    handle: Arc<client::Handle<ClientHandler>>,
}

impl Prober {
    /// Whether the server answers a ping within `timeout`.
    pub async fn alive(&self, timeout: Duration) -> bool {
        matches!(tokio::time::timeout(timeout, self.handle.send_ping()).await, Ok(Ok(()))) && !self.handle.is_closed()
    }

    /// Gives the connection up (it did not answer).
    pub async fn close(&self) {
        let _ = self.handle.disconnect(russh::Disconnect::ByApplication, "no answer", "").await;
    }
}

/// An authenticated SSH connection. Shells opened from it keep it alive.
pub struct Session {
    handle: Arc<client::Handle<ClientHandler>>,
    env: Arc<Vec<(String, String)>>,
    forward_agent: bool,
    /// The jump hosts' connections the session runs through, kept open while it lives.
    jumps: Arc<Vec<client::Handle<ClientHandler>>>,
    /// The target's certificate had expired, so the key went alone.
    certificate_expired: bool,
    /// Targets of this session's remote forwardings.
    remotes: tunnel::Remotes,
    x11: Option<Arc<x11::Forward>>,
    /// Runs once the last user let go of the connection (the steps after a session).
    on_drop: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(f) = self.on_drop.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
            f();
        }
    }
}

pub struct Shell {
    pub events: mpsc::Receiver<ShellEvent>,
    writer: ShellWriter,
}

/// The sending half of a shell, separate from its event stream.
#[derive(Clone)]
pub struct ShellWriter {
    channel: Arc<ChannelWriteHalf<Msg>>,
    session: Arc<client::Handle<ClientHandler>>,
    _jumps: Arc<Vec<client::Handle<ClientHandler>>>,
}

/// The client settings for every hop: keep-alive, and russh's default algorithms, which put the
/// post-quantum key exchange `mlkem768x25519-sha256` first.
pub fn client_config() -> client::Config {
    client::Config { keepalive_interval: Some(KEEPALIVE_INTERVAL), keepalive_max: KEEPALIVE_MAX, ..Default::default() }
}

/// Connects to one host directly.
pub async fn connect(params: ConnectParams, verifier: Arc<dyn HostKeyVerifier>, prompter: Arc<dyn AuthPrompter>) -> Result<Session, SshError> {
    let hop = Hop { host_name: params.address.clone(), address: params.address, port: params.port, username: params.username, auth: params.auth };
    connect_route(
        RouteParams { hops: vec![hop], proxy: None, options: SessionOptions::default(), connect_timeout: CONNECT_TIMEOUT },
        verifier,
        prompter,
    )
    .await
}

/// Connects along a route: the first hop over TCP or the proxy, each further hop through a
/// `direct-tcpip` channel of the one before. Each hop checks its own host key and signs in with
/// its own credentials. A failure at a jump host comes as [`SshError::Hop`].
pub async fn connect_route(params: RouteParams, verifier: Arc<dyn HostKeyVerifier>, prompter: Arc<dyn AuthPrompter>) -> Result<Session, SshError> {
    let config = Arc::new(client_config());
    let last = params.hops.len().checked_sub(1).ok_or_else(|| SshError::Connect("no host to connect to".into()))?;
    let timeout = params.connect_timeout;
    let mut connected: Vec<client::Handle<ClientHandler>> = Vec::new();
    let mut certificate_expired = false;
    let remotes = tunnel::Remotes::default();
    let mut target_announcements = None;
    let x11 = params.options.x11.map(|o| Arc::new(x11::Forward::new(o)));
    for (index, hop) in params.hops.into_iter().enumerate() {
        let host_name = hop.host_name.clone();
        let wrap = |error: SshError| {
            if index < last { SshError::Hop { index, host_name: host_name.clone(), error: Box::new(error) } } else { error }
        };
        let mut command_errors = None;
        let stream: Box<dyn Io> = match connected.last() {
            None => {
                let opened = proxy::open(params.proxy.as_ref(), &hop.address, hop.port, timeout).await.map_err(wrap)?;
                command_errors = opened.stderr;
                opened.stream
            }
            Some(before) => {
                let opened = before.channel_open_direct_tcpip(hop.address.clone(), u32::from(hop.port), "127.0.0.1", 0);
                let channel = tokio::time::timeout(timeout, opened)
                    .await
                    .map_err(|_| wrap(SshError::Connect("timed out".into())))?
                    .map_err(|e| wrap(SshError::Connect(e.to_string())))?;
                Box::new(channel.into_stream())
            }
        };
        let rejected = Arc::new(AtomicBool::new(false));
        let used = Arc::new(std::sync::Mutex::new(None));
        let (announce, announcements) = if index == last {
            let (tx, rx) = mpsc::unbounded_channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let handler = ClientHandler {
            announce,
            used: used.clone(),
            agent: if index == last { params.options.agent_forwarding.clone() } else { None },
            x11: if index == last { x11.clone() } else { None },
            host_name: hop.host_name.clone(),
            address: hop.address.clone(),
            port: hop.port,
            verifier: verifier.clone(),
            rejected: rejected.clone(),
            // Only the target forwards ports for us.
            remotes: if index == last { remotes.clone() } else { tunnel::Remotes::default() },
        };
        let mut handle = match client::connect_stream(config.clone(), stream, handler).await {
            Ok(handle) => handle,
            Err(e) if rejected.load(Ordering::SeqCst) => {
                drop(e);
                return Err(wrap(SshError::HostKeyRejected));
            }
            Err(e) => {
                // A proxy command that gave up usually said why on its standard error.
                let said = match command_errors.take() {
                    Some(errors) => proxy::command_said(errors).await,
                    None => None,
                };
                return Err(wrap(match said {
                    Some(text) => SshError::Proxy(format!("the proxy command failed: {text}")),
                    None => SshError::Connect(e.to_string()),
                }));
            }
        };
        let mut credentials = hop.auth;
        certificate_expired = credentials.certificate.as_ref().is_some_and(|c| !certificate_valid_now(c));
        if verifier.first_contact(&hop.address, hop.port) {
            credentials.password = None;
        }
        auth::authenticate(&mut handle, &hop.username, &hop.host_name, credentials, prompter.as_ref()).await.map_err(wrap)?;
        if let Some(announcements) = announcements {
            target_announcements = Some((announcements, hostkeys::Announced { address: hop.address.clone(), port: hop.port, used }));
        }
        connected.push(handle);
    }
    // The route always ends at the target, so there is a last hop; an empty route is an error,
    // not a crash.
    let Some(handle) = connected.pop() else {
        return Err(SshError::Connect("no host to connect to".into()));
    };
    let handle = Arc::new(handle);
    if let Some((announcements, about)) = target_announcements {
        hostkeys::spawn(Arc::downgrade(&handle), verifier.clone(), about, announcements);
    }
    Ok(Session {
        handle,
        env: Arc::new(params.options.env),
        forward_agent: params.options.agent_forwarding.is_some(),
        jumps: Arc::new(connected),
        certificate_expired,
        remotes,
        x11,
        on_drop: std::sync::Mutex::new(None),
    })
}

impl Session {
    /// Listens on `bind_address:bind_port` here; connections reach `target_host:target_port`
    /// as seen from the server (`ssh -L`).
    pub async fn forward_local(&self, bind_address: &str, bind_port: u16, target_host: &str, target_port: u16) -> Result<Tunnel, SshError> {
        let target = tunnel::Dest::Tcp(target_host.to_owned(), target_port);
        tunnel::listen(self.handle.clone(), self.jumps.clone(), bind_address, bind_port, target).await
    }

    /// Listens on `bind_address:bind_port` here; connections reach the Unix socket `path` on
    /// the server (`ssh -L port:/path`).
    pub async fn forward_socket(&self, bind_address: &str, bind_port: u16, path: &str) -> Result<Tunnel, SshError> {
        tunnel::listen(self.handle.clone(), self.jumps.clone(), bind_address, bind_port, tunnel::Dest::Socket(path.to_owned())).await
    }

    /// A SOCKS5 proxy on `bind_address:bind_port` here; each connection asks for its own target
    /// (`ssh -D`).
    pub async fn forward_dynamic(&self, bind_address: &str, bind_port: u16) -> Result<Tunnel, SshError> {
        tunnel::listen(self.handle.clone(), self.jumps.clone(), bind_address, bind_port, tunnel::Dest::Socks).await
    }

    /// The server listens on `bind_address:bind_port`; connections reach
    /// `target_host:target_port` as seen from here (`ssh -R`).
    pub async fn forward_remote(&self, bind_address: &str, bind_port: u16, target_host: &str, target_port: u16) -> Result<Tunnel, SshError> {
        tunnel::remote(self.handle.clone(), self.jumps.clone(), self.remotes.clone(), bind_address, bind_port, target_host, target_port).await
    }

    /// Checks the connection to the target.
    pub fn prober(&self) -> Prober {
        Prober { handle: self.handle.clone() }
    }

    /// Whether the connection to the target or to a jump host ended.
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed() || self.jumps.iter().any(|j| j.is_closed())
    }

    /// Runs `f` once the last user of this connection let go of it.
    pub fn on_last_drop(&self, f: Box<dyn FnOnce() + Send>) {
        *self.on_drop.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(f);
    }

    /// Completes once the connection to the target or to a jump host is gone.
    pub fn closed(&self) -> impl Future<Output = ()> + Send + 'static {
        let handle = self.handle.clone();
        let jumps = self.jumps.clone();
        async move {
            while !handle.is_closed() && !jumps.iter().any(|j| j.is_closed()) {
                tokio::time::sleep(CLOSED_POLL).await;
            }
        }
    }

    /// Whether the target's certificate had expired, so the key signed in alone.
    pub fn certificate_expired(&self) -> bool {
        self.certificate_expired
    }

    /// Runs a command on its own channel and returns at most `limit` bytes of its output;
    /// gives up after a few seconds.
    pub async fn exec(&self, command: &str, limit: usize) -> Result<Vec<u8>, SshError> {
        exec_on(&self.handle, command, None, limit).await
    }

    /// Runs a command to its end and returns output, errors and exit status; see [`RunOutput`].
    /// Without a time limit: the caller decides how long to wait.
    pub async fn run(&self, command: &str, limit: usize) -> Result<RunOutput, SshError> {
        run_on(&self.handle, command, limit).await
    }

    /// Opens a shell with a terminal of type `term` (`TERM` on the server).
    pub async fn open_shell(&self, cols: u32, rows: u32, term: &str) -> Result<Shell, SshError> {
        self.open_terminal(cols, rows, term, None).await
    }

    /// Opens a terminal of type `term` that runs `command` instead of the login shell (a fixed
    /// tmux or screen command), or the shell with `None`.
    pub async fn open_terminal(&self, cols: u32, rows: u32, term: &str, command: Option<&str>) -> Result<Shell, SshError> {
        let channel = self.handle.channel_open_session().await.map_err(protocol)?;
        channel.request_pty(false, term, cols, rows, 0, 0, &[]).await.map_err(protocol)?;
        for (name, value) in self.env.iter() {
            channel.set_env(false, name.as_str(), value.as_str()).await.map_err(protocol)?;
        }
        if self.forward_agent {
            channel.agent_forward(false).await.map_err(protocol)?;
        }
        if let Some(forward) = &self.x11 {
            // Marked first: the server may open a channel as soon as it has the request.
            forward.mark_requested();
            channel.request_x11(false, false, x11::PROTOCOL, forward.fake_hex(), forward.screen()).await.map_err(protocol)?;
        }
        match command {
            Some(command) => channel.exec(false, command).await.map_err(protocol)?,
            None => channel.request_shell(false).await.map_err(protocol)?,
        }
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
            let reason = if exit_status.is_some() || closed { Disconnect::Exited(exit_status) } else { Disconnect::Lost("connection closed".into()) };
            let _ = tx.send(ShellEvent::Ended(reason)).await;
        });

        Ok(Shell { events: rx, writer: ShellWriter { channel: Arc::new(write), session: self.handle.clone(), _jumps: self.jumps.clone() } })
    }

    pub async fn disconnect(self) {
        let _ = self.handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        for jump in self.jumps.iter().rev() {
            let _ = jump.disconnect(russh::Disconnect::ByApplication, "", "en").await;
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
    /// Runs a command on its own channel of the same connection, beside the shell (for path
    /// completion), with `input` on its standard input; at most `limit` bytes of output.
    pub async fn exec(&self, command: &str, input: Option<&[u8]>, limit: usize) -> Result<Vec<u8>, SshError> {
        exec_on(&self.session, command, input, limit).await
    }

    pub async fn write(&self, data: &[u8]) -> Result<(), SshError> {
        self.channel.data(data).await.map_err(protocol)
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> Result<(), SshError> {
        self.channel.window_change(cols, rows, 0, 0).await.map_err(protocol)
    }
}

//! Runs terminal sessions for the UI. SSH: resolves the route, loads keys and passwords, checks
//! host keys against the store (asking the user for unknown ones) and reconnects lost sessions.
//! Also a local shell and a serial port.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use russh::keys::{Certificate, PrivateKey, PublicKey};
use serde::Serialize;
use sshcraft_fido::presence::{Presence, UserPresence};
use sshcraft_fido::{Attachment, Authenticator, SkSigner};
use sshcraft_model::keytypes::KeyKind;
use sshcraft_model::model::{Directory, HostId, KeyId, KeyRef, PasswordId, Proxy, ProxyEndpoint, ResolveError, ResolvedHost, Route, resolve_route};
use sshcraft_model::policy::{Disconnect, HostKeyStatus, check_host_key, next_reconnect_delay};

use crate::waker::{PING_TIMEOUT, WakeEvent};
use sshcraft_ssh::authorized_keys::{InstallOutcome, install_command, parse_outcome};
use sshcraft_ssh::{
    Auth, AuthPromptLine, AuthPrompter, AuthRequest, Credential, Hop, HostKeyVerifier, ProxyConfig, ProxyTarget, RouteParams, SessionOptions,
    ShellEvent, ShellWriter, SshError,
};
use sshcraft_store::store::Store;
use sshcraft_store::vault::Vault;
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::error::{EngineError, ErrorCode};
use crate::settings::EngineSettings;

/// Reads the current settings (terminal type, language, recording) when they are needed.
pub type ReadSettings = Arc<dyn Fn() -> EngineSettings + Send + Sync>;

pub type SessionId = Uuid;

/// The most bytes a resource probe reads.
const RESOURCES_LIMIT: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Exited,
    Lost,
    HostKeyRejected,
    HostKeyMismatch,
    AuthFailed,
    /// The user cancelled a sign-in question.
    AuthCancelled,
    /// The very first attempt could not reach the server; nothing to reconnect to.
    ConnectFailed,
    /// Could not even start: host incomplete, key missing, vault locked.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionStatus {
    Connecting,
    Connected {
        /// Something to point out while connected (agent forwarding not possible here).
        #[serde(skip_serializing_if = "Option::is_none")]
        notice: Option<ErrorCode>,
    },
    Reconnecting {
        attempt: u32,
        delay_secs: u64,
    },
    Ended {
        reason: EndReason,
        /// Translatable cause, if there is a more specific one than `reason`.
        code: Option<ErrorCode>,
        /// Technical detail in English, shown untranslated.
        detail: Option<String>,
    },
}

/// Where a session's output and status go (Tauri channels in the app, vectors in tests).
pub trait SessionSink: Send + Sync + 'static {
    fn data(&self, bytes: Vec<u8>);
    fn status(&self, status: SessionStatus);
}

/// Asks the user whether to trust an unknown host key.
#[async_trait::async_trait]
pub trait HostKeyPrompter: Send + Sync + 'static {
    async fn ask(&self, host_name: &str, address: &str, port: u16, fingerprint: &str) -> bool;

    /// Whether a proxy command that is not approved on this device may run.
    async fn ask_command(&self, _host_name: &str, _command: &str) -> bool {
        false
    }

    /// Whether a password may be fetched from a password manager for this host.
    async fn ask_secret_ref(&self, _host_name: &str, _description: &str) -> bool {
        false
    }
}

/// The UI may hold this many unacknowledged output bytes before SSHCraft stops reading from
/// the server. Keeps a runaway `cat` from flooding the webview.
pub const OUTPUT_HIGH_WATERMARK: usize = 512 * 1024;

/// Output bytes handed to the UI but not yet acknowledged as rendered.
#[derive(Default)]
pub(crate) struct Flow {
    in_flight: AtomicUsize,
    drained: Notify,
}

impl Flow {
    fn add(&self, bytes: usize) {
        self.in_flight.fetch_add(bytes, Ordering::SeqCst);
    }

    /// Output not yet acknowledged.
    pub(crate) fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    pub(crate) fn ack(&self, bytes: usize) {
        let _ = self.in_flight.try_update(Ordering::SeqCst, Ordering::SeqCst, |v| Some(v.saturating_sub(bytes)));
        self.drained.notify_waiters();
    }

    async fn wait_below_watermark(&self) {
        loop {
            let drained = self.drained.notified();
            if self.in_flight.load(Ordering::SeqCst) <= OUTPUT_HIGH_WATERMARK {
                return;
            }
            drained.await;
        }
    }
}

/// What a tab is connected to.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionTarget {
    Host {
        host_id: HostId,
    },
    Local {
        shell: sshcraft_local::pty::ShellCommand,
        /// Where the shell starts; the home directory when missing or not a directory.
        #[serde(default)]
        cwd: Option<String>,
    },
    Serial {
        settings: sshcraft_local::serial::SerialSettings,
    },
}

/// The sending half of any terminal: SSH shell, local shell or serial port.
#[async_trait::async_trait]
pub(crate) trait TermWriter: Send + Sync {
    async fn write(&self, data: &[u8]) -> Result<(), String>;
    async fn resize(&self, cols: u32, rows: u32) -> Result<(), String>;
    /// The tab is closed: end what runs behind it now (a local shell).
    fn close(&self) {}
    /// Names in `dir` starting with `prefix`, for path completion; nothing where a terminal has
    /// no files.
    async fn list_dir(&self, _dir: &str, _prefix: &str) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
    /// Runs the fixed `command` on a channel of its own beside the shell, at most `limit` bytes
    /// back; nothing where a terminal has no server.
    async fn probe(&self, _command: &str, _limit: usize) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }
}

#[async_trait::async_trait]
impl TermWriter for ShellWriter {
    async fn write(&self, data: &[u8]) -> Result<(), String> {
        ShellWriter::write(self, data).await.map_err(|e| e.to_string())
    }

    async fn resize(&self, cols: u32, rows: u32) -> Result<(), String> {
        ShellWriter::resize(self, cols, rows).await.map_err(|e| e.to_string())
    }

    async fn list_dir(&self, dir: &str, prefix: &str) -> Result<Vec<String>, String> {
        let Some(input) = crate::listing::listing_input(dir) else {
            return Ok(Vec::new());
        };
        let output = self.exec(crate::listing::LIST_COMMAND, Some(input.as_bytes()), crate::listing::MAX_OUTPUT).await.map_err(|e| e.to_string())?;
        Ok(crate::listing::parse_listing(&output, prefix))
    }

    async fn probe(&self, command: &str, limit: usize) -> Result<Option<Vec<u8>>, String> {
        self.exec(command, None, limit).await.map(Some).map_err(|e| e.to_string())
    }
}

fn to_u16(v: u32) -> u16 {
    u16::try_from(v).unwrap_or(u16::MAX)
}

#[async_trait::async_trait]
impl TermWriter for sshcraft_local::pty::LocalWriter {
    async fn write(&self, data: &[u8]) -> Result<(), String> {
        let writer = self.clone();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || writer.write(&data)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
    }

    async fn resize(&self, cols: u32, rows: u32) -> Result<(), String> {
        sshcraft_local::pty::LocalWriter::resize(self, to_u16(cols), to_u16(rows)).map_err(|e| e.to_string())
    }

    fn close(&self) {
        sshcraft_local::pty::LocalWriter::close(self);
    }

    async fn list_dir(&self, dir: &str, prefix: &str) -> Result<Vec<String>, String> {
        let (dir, prefix) = (dir.to_owned(), prefix.to_owned());
        tokio::task::spawn_blocking(move || crate::listing::list_local(&dir, &prefix)).await.map_err(|e| e.to_string())?
    }
}

#[async_trait::async_trait]
impl TermWriter for sshcraft_local::serial::SerialWriter {
    async fn write(&self, data: &[u8]) -> Result<(), String> {
        let writer = self.clone();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || writer.write(&data)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
    }

    /// A serial line has no window size.
    async fn resize(&self, _cols: u32, _rows: u32) -> Result<(), String> {
        Ok(())
    }
}

type Writer = Arc<dyn TermWriter>;

struct Live {
    /// Keystrokes in call order. A per-connection task forwards them to the server.
    input: mpsc::UnboundedSender<Vec<u8>>,
    /// Cloned out before awaiting, so no lock is held while talking to the server.
    writer: Arc<Mutex<Option<Writer>>>,
    size: Arc<Mutex<(u32, u32)>>,
    flow: Arc<Flow>,
    task: JoinHandle<()>,
    /// Present while the session is recorded into a session log.
    recorder: Option<Arc<crate::recording::Recorder>>,
}

pub struct SessionManager {
    ctx: Arc<Context>,
    sessions: Mutex<HashMap<SessionId, Live>>,
}

pub(crate) struct Context {
    /// SSH connections in use, shared by users of the same route.
    pub(crate) pool: crate::pool::Pool,
    pub(crate) store: Arc<Mutex<Store>>,
    vault: Option<Arc<Vault>>,
    prompter: Arc<dyn HostKeyPrompter>,
    auth: Arc<dyn AuthPrompter>,
    agent: Arc<dyn AgentConnector>,
    gate: KeyGate,
    pub(crate) delay_scale: f32,
    /// Signs for FIDO2 and Windows Hello keys; `None` where there is none.
    security: Option<Arc<dyn Authenticator>>,
    /// Told when a session log ended.
    log_finished: Mutex<Option<crate::recording::OnFinish>>,
    settings: Mutex<ReadSettings>,
    /// Where sessions run, also when `open` is called from a thread outside the runtime.
    pub(crate) runtime: tokio::runtime::Handle,
    /// Tells sessions and forwards that the computer woke up or its network changed.
    pub(crate) wake: tokio::sync::broadcast::Sender<WakeEvent>,
    /// Fetches passwords from password managers.
    secrets: Mutex<Arc<dyn crate::secrets::SecretSource>>,
    /// KeePassXC database passwords given this run, by database file; never stored.
    kdbx: Mutex<HashMap<String, zeroize::Zeroizing<String>>>,
}

impl Context {
    pub(crate) fn settings(&self) -> EngineSettings {
        let read = self.settings.lock().unwrap_or_else(PoisonError::into_inner).clone();
        read()
    }

    pub(crate) fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }
}

/// What a [`SessionManager`] is built from.
pub struct ManagerParts {
    pub store: Arc<Mutex<Store>>,
    pub vault: Option<Arc<Vault>>,
    pub prompter: Arc<dyn HostKeyPrompter>,
    pub auth: Arc<dyn AuthPrompter>,
    pub agent: Arc<dyn AgentConnector>,
    pub presence: Arc<dyn UserPresence>,
    pub security: Option<Arc<dyn Authenticator>>,
    pub settings: ReadSettings,
    pub runtime: tokio::runtime::Handle,
    /// Shortens reconnect delays in tests; 1.0 in the app.
    pub delay_scale: f32,
}

/// How often SSHCraft asks for a key's passphrase before giving up.
const MAX_PASSPHRASE_TRIES: usize = 3;

/// Asks Windows Hello once per app run before a key from the vault is used.
pub struct KeyGate {
    presence: Arc<dyn UserPresence>,
    verified: tokio::sync::Mutex<bool>,
    hello_missing: std::sync::atomic::AtomicBool,
    on_unavailable: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

/// Text in the Windows Hello dialog, in the app's language (AUTO follows the system).
pub fn hello_reason(language: crate::settings::LanguageSetting, system_locale: Option<&str>) -> &'static str {
    use crate::settings::LanguageSetting as L;
    let german = match language {
        L::De => true,
        L::En => false,
        L::Auto => system_locale.is_some_and(|l| l.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("de"))),
    };
    if german { "SSHCraft möchte deine gespeicherten SSH-Schlüssel verwenden." } else { "SSHCraft wants to use your stored SSH keys." }
}

impl KeyGate {
    fn new(presence: Arc<dyn UserPresence>) -> Self {
        Self {
            presence,
            verified: tokio::sync::Mutex::new(false),
            hello_missing: std::sync::atomic::AtomicBool::new(false),
            on_unavailable: Mutex::new(None),
        }
    }

    /// Concurrent callers wait on the same lock, so one prompt serves them all.
    /// Forgets that the user was verified: the next key or password asks again.
    async fn relock(&self) {
        *self.verified.lock().await = false;
    }

    async fn ensure(&self, reason: &str) -> Result<(), ErrorCode> {
        let mut verified = self.verified.lock().await;
        if *verified {
            return Ok(());
        }
        match self.presence.verify(reason).await {
            Presence::Verified => {
                *verified = true;
                Ok(())
            }
            Presence::Unavailable => {
                let first = !self.hello_missing.swap(true, std::sync::atomic::Ordering::SeqCst);
                if first && let Some(report) = &*self.on_unavailable.lock().unwrap_or_else(PoisonError::into_inner) {
                    report();
                }
                *verified = true;
                Ok(())
            }
            Presence::Declined => Err(ErrorCode::HelloDeclined),
        }
    }
}

/// Opens a connection to the SSH agent (Proton Pass on Windows, an in-memory agent in tests).
#[async_trait::async_trait]
pub trait AgentConnector: Send + Sync + 'static {
    async fn connect(&self) -> Result<sshcraft_ssh::DynAgent, SshError>;

    /// The agent a session may forward; `None` where there is none to forward.
    fn opener(&self) -> Option<Arc<dyn sshcraft_ssh::AgentOpener>> {
        None
    }
}

/// The Windows agent pipe served by the Proton Pass desktop app.
pub struct WindowsAgent;

#[async_trait::async_trait]
impl AgentConnector for WindowsAgent {
    async fn connect(&self) -> Result<sshcraft_ssh::DynAgent, SshError> {
        #[cfg(windows)]
        {
            sshcraft_ssh::agent::open_windows_agent().await
        }
        #[cfg(not(windows))]
        {
            Err(SshError::AgentUnavailable("no agent support on this platform yet".into()))
        }
    }

    fn opener(&self) -> Option<Arc<dyn sshcraft_ssh::AgentOpener>> {
        #[cfg(windows)]
        {
            Some(Arc::new(sshcraft_ssh::agent::WindowsAgentOpener))
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

/// What `run` shares with the manager for one session.
struct Shared {
    sink: Arc<dyn SessionSink>,
    input: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
    writer: Arc<Mutex<Option<Writer>>>,
    size: Arc<Mutex<(u32, u32)>>,
    flow: Arc<Flow>,
}

impl SessionManager {
    pub fn new(parts: ManagerParts) -> Self {
        Self {
            ctx: Arc::new(Context {
                store: parts.store,
                vault: parts.vault,
                prompter: parts.prompter,
                auth: parts.auth,
                agent: parts.agent,
                gate: KeyGate::new(parts.presence),
                delay_scale: parts.delay_scale,
                security: parts.security,
                log_finished: Mutex::new(None),
                pool: crate::pool::Pool::default(),
                settings: Mutex::new(parts.settings),
                runtime: parts.runtime,
                wake: tokio::sync::broadcast::channel(16).0,
                secrets: Mutex::new(Arc::new(crate::secrets::CliSecrets)),
                kdbx: Mutex::new(HashMap::new()),
            }),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// The app was locked: keys and passwords need the user's presence again.
    pub async fn relock(&self) {
        self.ctx.gate.relock().await;
        // A new connection after the lock signs in again: an open one is not handed out.
        self.ctx.pool.clear();
        // KeePassXC database passwords of this run are asked again too.
        self.ctx.kdbx.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    /// Asks for the user's presence (to unlock the app); a declined one is an error. Where the
    /// system has none, the lock only hides the window and unlocking needs nothing.
    pub async fn verify_user(&self) -> Result<(), ErrorCode> {
        let reason = hello_reason(self.ctx.settings().general.language, sys_locale::get_locale().as_deref());
        match self.ctx.gate.presence.verify(reason).await {
            Presence::Verified | Presence::Unavailable => Ok(()),
            Presence::Declined => Err(ErrorCode::HelloDeclined),
        }
    }

    /// Where passwords from password managers come from (a fake in tests).
    pub fn set_secrets(&self, source: Arc<dyn crate::secrets::SecretSource>) {
        *self.ctx.secrets.lock().unwrap_or_else(PoisonError::into_inner) = source;
    }

    /// Checks every connection now and retries every waiting reconnect: the computer woke up or
    /// its network changed.
    pub fn wake(&self, event: WakeEvent) {
        let _ = self.ctx.wake.send(event);
    }

    fn sessions(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, Live>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The output not yet acknowledged of a session; `None` for an unknown one.
    pub fn unacknowledged(&self, id: SessionId) -> Option<usize> {
        self.sessions().get(&id).map(|live| live.flow.in_flight())
    }

    /// The flow control of a session, for its terminal stream.
    pub(crate) fn flow(&self, id: SessionId) -> Option<Arc<Flow>> {
        self.sessions().get(&id).map(|live| live.flow.clone())
    }

    /// Ids of the running sessions.
    pub fn ids(&self) -> Vec<SessionId> {
        self.sessions().keys().copied().collect()
    }

    /// Called once, the first time Windows Hello turns out not to be set up on this PC.
    pub fn on_hello_unavailable(&self, report: Box<dyn Fn() + Send + Sync>) {
        *self.ctx.gate.on_unavailable.lock().unwrap_or_else(PoisonError::into_inner) = Some(report);
    }

    /// Called with each session log that ended, after it is stored.
    pub fn on_log_finished(&self, report: crate::recording::OnFinish) {
        *self.ctx.log_finished.lock().unwrap_or_else(PoisonError::into_inner) = Some(report);
    }

    /// True once Windows Hello turned out not to be set up on this PC.
    pub fn hello_missing(&self) -> bool {
        self.ctx.gate.hello_missing.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Lets FIDO2 and Windows Hello keys sign through `auth`. Only before the first session; later
    /// the manager stays as it is.
    pub fn with_security_keys(mut self, auth: Arc<dyn Authenticator>) -> Self {
        if let Some(ctx) = Arc::get_mut(&mut self.ctx) {
            ctx.security = Some(auth);
        }
        self
    }

    /// The containers (pods) `runtime` runs on a host, each name checked: connects like a tab
    /// does, runs the fixed listing command, opens no shell.
    pub async fn list_containers(
        &self,
        host_id: HostId,
        runtime: sshcraft_model::container::Runtime,
        namespace: Option<String>,
    ) -> Result<Vec<String>, (ErrorCode, Option<String>)> {
        const LIMIT: usize = 64 * 1024;
        let command = sshcraft_model::container::list_command(runtime, namespace.as_deref()).ok_or((ErrorCode::ContainerNameInvalid, namespace))?;
        let (session, _) = connect_session(&self.ctx, host_id, &OpenedKeys::default(), true).await.map_err(failure_code)?;
        let output = session.exec(&command, LIMIT).await.map_err(|e| (ErrorCode::Session, Some(e.to_string())));
        crate::pool::release(session).await;
        let output = output?;
        Ok(sshcraft_model::container::parse_list(runtime, &String::from_utf8_lossy(&output), output.len() >= LIMIT))
    }

    /// Puts a public key into `authorized_keys` on a host: connects like a tab does (with every
    /// question), runs one command, opens no shell.
    pub async fn install_key(&self, host_id: HostId, key: KeyRef) -> Result<InstallOutcome, (ErrorCode, Option<String>)> {
        let (public, verify_required) = public_half(&self.ctx, &key).await?;
        let (session, _) = connect_session(&self.ctx, host_id, &OpenedKeys::default(), true).await.map_err(failure_code)?;
        let output = session
            .exec(&install_command(&public, verify_required), 64 * 1024)
            .await
            .map_err(|e| (ErrorCode::InstallKeyFailed, Some(e.to_string())))?;
        crate::pool::release(session).await;
        parse_outcome(&output).map_err(|e| match e {
            sshcraft_ssh::authorized_keys::AuthorizedKeysError::Failed => (ErrorCode::InstallKeyFailed, None),
            sshcraft_ssh::authorized_keys::AuthorizedKeysError::Unsupported => (ErrorCode::InstallKeyUnsupported, None),
        })
    }

    /// Keys the SSH agent offers. Only public facts, never key material.
    pub async fn agent_keys(&self) -> Result<Vec<sshcraft_ssh::AgentKey>, (ErrorCode, Option<String>)> {
        let unavailable = |e: SshError| (ErrorCode::AgentUnavailable, Some(e.to_string()));
        let mut agent = self.ctx.agent.connect().await.map_err(unavailable)?;
        sshcraft_ssh::list_agent_keys(&mut agent).await.map_err(unavailable)
    }

    pub fn open(&self, target: SessionTarget, cols: u32, rows: u32, sink: Arc<dyn SessionSink>) -> SessionId {
        self.start(target, cols, rows, sink, true)
    }

    /// [`Self::open`] for a tab the user reconnects: its launch actions ran already.
    pub fn reopen(&self, target: SessionTarget, cols: u32, rows: u32, sink: Arc<dyn SessionSink>) -> SessionId {
        self.start(target, cols, rows, sink, false)
    }

    fn start(&self, target: SessionTarget, cols: u32, rows: u32, sink: Arc<dyn SessionSink>, launch: bool) -> SessionId {
        // `open` may come from a thread outside the runtime (the interface); the session and
        // the recorder's flushing run on the engine's runtime either way.
        let _runtime = self.ctx.runtime.enter();
        let id = Uuid::new_v4();
        let (input, input_rx) = mpsc::unbounded_channel();
        let recorder = start_recording(&self.ctx, &target);
        let sink: Arc<dyn SessionSink> = match &recorder {
            Some(recorder) => Arc::new(crate::recording::LogSink { inner: sink, recorder: recorder.clone() }),
            None => sink,
        };
        let shared = Shared {
            sink,
            input: Arc::new(tokio::sync::Mutex::new(input_rx)),
            writer: Arc::new(Mutex::new(None)),
            size: Arc::new(Mutex::new((cols, rows))),
            flow: Arc::new(Flow::default()),
        };
        let live = Live {
            input,
            writer: shared.writer.clone(),
            size: shared.size.clone(),
            flow: shared.flow.clone(),
            // The session's questions carry its id.
            task: self.ctx.runtime.spawn(crate::prompts::ASKING_SESSION.scope(id, run(self.ctx.clone(), target, shared, launch))),
            recorder,
        };
        self.sessions().insert(id, live);
        id
    }

    /// Queues input for the server. Synchronous, so calls keep their order.
    /// Input typed while disconnected is sent after the reconnect.
    pub fn write(&self, id: SessionId, data: &[u8]) -> Result<(), String> {
        let sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        let live = sessions.get(&id).ok_or("unknown session")?;
        live.input.send(data.to_vec()).map_err(|_| "session has ended".to_owned())
    }

    pub async fn resize(&self, id: SessionId, cols: u32, rows: u32) -> Result<(), String> {
        let writer = {
            let sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
            let live = sessions.get(&id).ok_or("unknown session")?;
            *live.size.lock().unwrap_or_else(PoisonError::into_inner) = (cols, rows);
            live.writer.lock().unwrap_or_else(PoisonError::into_inner).clone()
        };
        match writer {
            Some(w) => w.resize(cols, rows).await.map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    /// Names in `dir` starting with `prefix` on the session's machine (path completion).
    pub async fn list_dir(&self, id: SessionId, dir: &str, prefix: &str) -> Result<Vec<String>, String> {
        let writer = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .and_then(|live| live.writer.lock().unwrap_or_else(PoisonError::into_inner).clone())
            .ok_or("not connected")?;
        writer.list_dir(dir, prefix).await
    }

    /// Load, memory and disk of the tab's server, read by [`sshcraft_model::resources::PROBE`]
    /// beside the shell; `None` for a tab without a server or a server that told nothing.
    pub async fn resources(&self, id: SessionId) -> Result<Option<sshcraft_model::resources::Resources>, String> {
        let writer = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .and_then(|live| live.writer.lock().unwrap_or_else(PoisonError::into_inner).clone())
            .ok_or("not connected")?;
        let output = writer.probe(sshcraft_model::resources::PROBE, RESOURCES_LIMIT).await?;
        Ok(output.and_then(|o| sshcraft_model::resources::parse(&String::from_utf8_lossy(&o))))
    }

    /// The UI has rendered `bytes` more output bytes.
    pub fn ack(&self, id: SessionId, bytes: usize) {
        if let Some(live) = self.sessions.lock().unwrap_or_else(PoisonError::into_inner).get(&id) {
            live.flow.ack(bytes);
        }
    }

    /// What connecting needs (store, vault, prompts, agent, Windows Hello), shared with the
    /// port forwarding.
    pub(crate) fn context(&self) -> Arc<Context> {
        self.ctx.clone()
    }

    pub fn close(&self, id: SessionId) {
        let live = self.sessions.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
        if let Some(live) = live {
            if let Some(writer) = live.writer.lock().unwrap_or_else(PoisonError::into_inner).clone() {
                writer.close();
            }
            live.task.abort();
            if let Some(recorder) = live.recorder {
                recorder.finish();
            }
        }
    }

    /// Whether the session is being recorded.
    pub fn is_recording(&self, id: SessionId) -> bool {
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .and_then(|live| live.recorder.as_ref())
            .is_some_and(|recorder| recorder.is_active())
    }

    /// Ends every running recording (the app is quitting): the rest is stored and each log
    /// ends now instead of at the next start.
    pub fn finish_recordings(&self) {
        let recorders: Vec<_> =
            self.sessions.lock().unwrap_or_else(PoisonError::into_inner).values().filter_map(|live| live.recorder.clone()).collect();
        for recorder in recorders {
            recorder.finish();
        }
    }

    /// A bookmark in the session's log at the output so far.
    pub fn bookmark(&self, id: SessionId, text: &str) -> Result<sshcraft_store::session_log::LogMark, sshcraft_store::session_log::LogError> {
        let recorder = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .and_then(|live| live.recorder.clone())
            .ok_or(sshcraft_store::session_log::LogError::NotFound)?;
        recorder.bookmark(text)
    }
}

/// Starts a session log when the host, its groups or the app setting ask for one and the vault
/// is open.
fn start_recording(ctx: &Context, target: &SessionTarget) -> Option<Arc<crate::recording::Recorder>> {
    use sshcraft_store::session_log::{LogKind, LogMeta, MAX_TITLE};
    let vault = ctx.vault.clone()?;
    let global = ctx.settings().logs.session_logs;
    let (title, kind, host_id, enabled) = {
        let store = ctx.store();
        match target {
            SessionTarget::Host { host_id } => {
                let host = store.list_hosts().ok()?.into_iter().find(|h| h.id == *host_id)?;
                let groups = store.list_groups().ok()?;
                let enabled = sshcraft_model::model::session_log_for(&host, &groups).unwrap_or(global);
                (host.name, LogKind::Ssh, Some(*host_id), enabled)
            }
            SessionTarget::Local { shell, .. } => (shell.label.clone(), LogKind::Local, None, global),
            SessionTarget::Serial { settings } => (settings.port.clone(), LogKind::Serial, None, global),
        }
    };
    if !enabled {
        return None;
    }
    let title: String = title.chars().filter(|c| !c.is_control()).take(MAX_TITLE).collect();
    let title = if title.trim().is_empty() { "Terminal".to_owned() } else { title };
    let meta = LogMeta {
        id: sshcraft_model::model::LogId(Uuid::new_v4()),
        title,
        kind,
        host_id,
        started_at: crate::recording::now_ms(),
        ended_at: None,
        size: 0,
        truncated: false,
        marks: Vec::new(),
    };
    crate::recording::Recorder::start(
        ctx.store.clone(),
        vault,
        &meta,
        crate::recording::Limits::default(),
        ctx.log_finished.lock().unwrap_or_else(PoisonError::into_inner).clone(),
    )
}

/// Marks the host as connected to just now, for "recent connections". A failure here must
/// not end the session, so it is ignored.
fn remember_connection(ctx: &Context, host_id: HostId) {
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    let _ = ctx.store.lock().unwrap_or_else(PoisonError::into_inner).touch_recent(host_id, now_ms);
}

fn end(sink: &dyn SessionSink, reason: EndReason, code: Option<ErrorCode>, detail: Option<String>) {
    sink.status(SessionStatus::Ended { reason, code, detail });
}

/// Forwards queued input to one connection, in order. Aborted when the connection ends,
/// which also frees a write that is stuck on a dead connection.
async fn forward_input(input: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>, writer: Writer) {
    let mut input = input.lock().await;
    while let Some(data) = input.recv().await {
        if writer.write(&data).await.is_err() {
            return;
        }
    }
}

/// The whole life of one tab: connect, pump output, reconnect after a drop, end.
/// A line from SSHCraft itself in the terminal of the session, set apart from the server's output.
fn local_note(shared: &Shared, text: &str) {
    let bytes = format!(
        "
[2m[SSHCraft] {text}[0m
"
    )
    .into_bytes();
    shared.flow.add(bytes.len());
    shared.sink.data(bytes);
}

/// The login script of `host_id` when this device approved exactly these steps; says so in the
/// terminal when one is set but not approved.
fn login_script(ctx: &Context, host_id: HostId, shared: &Shared) -> Option<crate::login_script::LoginScript> {
    let store = ctx.store.lock().unwrap_or_else(PoisonError::into_inner);
    let host = store.list_hosts().ok()?.into_iter().find(|h| h.id == host_id)?;
    let groups = store.list_groups().ok()?;
    let steps = sshcraft_model::login_script::login_script_for(&host, &groups).filter(|s| !s.is_empty())?;
    sshcraft_model::login_script::check(&steps).ok()?;
    let digest = sshcraft_store::store::command_digest(&sshcraft_model::login_script::approval_argv(&steps));
    if !store.command_approved(&digest).unwrap_or(false) {
        drop(store);
        local_note(
            shared,
            "This host has a login script that is not allowed on this device yet. Open the host, or the group it comes from, tick Allow this login script on this device, and save.",
        );
        return None;
    }
    Some(crate::login_script::LoginScript::new(steps, std::time::Instant::now()))
}

/// Launch actions to type: their texts, each followed by Enter, and whether they wait for a
/// login script (on Telnet the first prompt is the device's own login).
struct Launch {
    bytes: Vec<u8>,
    needs_script: bool,
}

/// The host's launch actions; `None` without any, when they are not allowed on this device or the
/// session stays in tmux or screen (the terminal says so), or the vault is locked.
fn launch_actions(ctx: &Context, host_id: HostId, shared: &Shared) -> Option<Launch> {
    let store = ctx.store.lock().unwrap_or_else(PoisonError::into_inner);
    let host = store.list_hosts().ok()?.into_iter().find(|h| h.id == host_id)?;
    let groups = store.list_groups().ok()?;
    let (level, ids) = sshcraft_model::launch::launch_for(&host, &groups).filter(|(_, ids)| !ids.is_empty())?;
    let route = lookup(&store, host_id).ok()?;
    let snippets = sshcraft_store::snippets::list(ctx.vault.as_ref()?, &store).ok()?;
    let texts: Vec<String> = ids
        .iter()
        .take(sshcraft_model::launch::MAX_LAUNCH)
        .filter_map(|id| snippets.iter().find(|s| s.id == *id))
        .map(|s| s.content.clone())
        .collect();
    // A snippet with blanks to fill needs the user: it is not typed by itself.
    let texts: Vec<String> = texts.into_iter().filter(|t| sshcraft_model::placeholders::find(t).is_empty()).collect();
    if texts.is_empty() {
        return None;
    }
    // A session that stays may run a program already: typing into it would hit that program.
    if route.target.persistence.is_some() && route.target.container.is_none() {
        drop(store);
        local_note(shared, "Launch actions are not typed into a session that stays (tmux or screen): a program may run there already.");
        return None;
    }
    let digest = sshcraft_store::store::command_digest(&sshcraft_model::launch::approval_argv(&level, &texts));
    if !store.command_approved(&digest).unwrap_or(false) {
        drop(store);
        local_note(
            shared,
            "This host has launch actions that are not allowed on this device yet. Open the host, tick Allow the launch actions on this device, and save.",
        );
        return None;
    }
    Some(Launch {
        bytes: texts.iter().flat_map(|t| t.trim_end_matches(['\r', '\n']).bytes().chain(std::iter::once(b'\r'))).collect(),
        needs_script: route.target.protocol == sshcraft_model::model::Protocol::Telnet,
    })
}

async fn run(ctx: Arc<Context>, target: SessionTarget, shared: Shared, launch_wanted: bool) {
    let sink = shared.sink.clone();
    let mut attempt = 0u32;
    let mut ever_connected = false;
    // Launch actions on the first connect only, and not when the user reconnects a tab.
    let mut launch_wanted = launch_wanted;
    let opened = OpenedKeys::default();
    let mut wake = ctx.wake.subscribe();
    loop {
        sink.status(SessionStatus::Connecting);
        let reason = match connect_target(&ctx, &target, &shared.size, &opened).await {
            Ok((writer, mut events, notice, prober, _connection)) => {
                *shared.writer.lock().unwrap_or_else(PoisonError::into_inner) = Some(writer.clone());
                let script_writer = writer.clone();
                let forwarder = tokio::spawn(forward_input(shared.input.clone(), writer));
                // Output from before the drop may still wait in the stream unacknowledged: it keeps
                // counting, so a reconnect does not let the session read another watermark's worth.
                sink.status(SessionStatus::Connected { notice });
                if let SessionTarget::Host { host_id } = &target {
                    remember_connection(&ctx, *host_id);
                }
                let mut script = match &target {
                    SessionTarget::Host { host_id } => login_script(&ctx, *host_id, &shared),
                    _ => None,
                };
                // Launch actions after the login script.
                let mut launch = match &target {
                    SessionTarget::Host { host_id } if launch_wanted => launch_actions(&ctx, *host_id, &shared),
                    _ => None,
                };
                launch_wanted = false;
                if script.is_none()
                    && let Some(l) = launch.take()
                {
                    if l.needs_script {
                        local_note(
                            &shared,
                            "Launch actions on a Telnet host wait for a login script that signs in. This host has none, so they are not typed.",
                        );
                    } else {
                        let _sent = script_writer.write(&l.bytes).await;
                    }
                }
                ever_connected = true;
                attempt = 0;
                let mut reason = Disconnect::Lost("connection closed".into());
                loop {
                    shared.flow.wait_below_watermark().await;
                    let event = tokio::select! {
                        event = events.recv() => event,
                        woke = wake.recv() => {
                            if woke_up(&woke)
                                && let Some(prober) = &prober
                                && !prober.alive(PING_TIMEOUT.mul_f32(ctx.delay_scale)).await
                            {
                                prober.close().await;
                                reason = Disconnect::Lost("no answer after the computer woke up or the network changed".into());
                                break;
                            }
                            continue;
                        }
                    };
                    match event {
                        Some(ShellEvent::Data(bytes)) => {
                            let fed = script.as_mut().map(|s| s.feed(&bytes, std::time::Instant::now()));
                            shared.flow.add(bytes.len());
                            sink.data(bytes);
                            match fed {
                                Some(crate::login_script::Feed::Send(line)) => {
                                    // The answer goes like typing: a failed write ends nothing.
                                    let _sent = script_writer.write(&line).await;
                                }
                                Some(crate::login_script::Feed::TimedOut) => {
                                    local_note(&shared, "The login script stopped: a step did not see its text within 15 seconds.");
                                    // What follows is not what the script expected: nothing more is typed.
                                    launch = None;
                                }
                                Some(crate::login_script::Feed::Nothing) | None => {}
                            }
                            if script.as_ref().is_some_and(crate::login_script::LoginScript::completed)
                                && let Some(l) = launch.take()
                            {
                                let _sent = script_writer.write(&l.bytes).await;
                            }
                        }
                        Some(ShellEvent::Ended(r)) => {
                            reason = r;
                            break;
                        }
                        None => break,
                    }
                }
                forwarder.abort();
                *shared.writer.lock().unwrap_or_else(PoisonError::into_inner) = None;
                reason
            }
            Err(Failure::End(reason, code, detail)) => {
                return end(sink.as_ref(), reason, code, detail);
            }
            Err(Failure::Lost(code, detail)) if !ever_connected => {
                return end(sink.as_ref(), EndReason::ConnectFailed, Some(code), Some(detail));
            }
            Err(Failure::Lost(_, detail)) => Disconnect::Lost(detail),
        };

        attempt += 1;
        // A local shell that ended is not started again.
        let retry = match &target {
            SessionTarget::Local { .. } => None,
            _ => next_reconnect_delay(&reason, attempt, Some(ctx.settings().terminal.reconnect_attempts)),
        };
        match retry {
            Some(delay) => {
                sink.status(SessionStatus::Reconnecting { attempt, delay_secs: delay.as_secs() });
                wait_or_wake(delay.mul_f32(ctx.delay_scale), &mut wake).await;
            }
            None => {
                let (reason, code, detail) = match reason {
                    Disconnect::Exited(_) => (EndReason::Exited, None, None),
                    Disconnect::Lost(d) => (EndReason::Lost, Some(ErrorCode::Network), Some(d)),
                    Disconnect::HostKeyRejected => (EndReason::HostKeyRejected, None, None),
                    Disconnect::AuthFailed => (EndReason::AuthFailed, None, None),
                };
                return end(sink.as_ref(), reason, code, detail);
            }
        }
    }
}

pub(crate) enum Failure {
    /// Do not retry.
    End(EndReason, Option<ErrorCode>, Option<String>),
    /// Network trouble, may retry. The code says what failed (the network, a proxy, a jump
    /// host).
    Lost(ErrorCode, String),
}

pub(crate) fn end_error(code: ErrorCode, detail: Option<String>) -> Failure {
    Failure::End(EndReason::Error, Some(code), detail)
}

/// Asks for Windows Hello once per run before the first secret from the vault (a key or a
/// password) is used.
async fn unlock_secrets(ctx: &Context) -> Result<&Arc<Vault>, Failure> {
    let vault = ctx.vault.as_ref().ok_or_else(|| end_error(ErrorCode::VaultLocked, None))?;
    presence_for_secrets(ctx).await?;
    Ok(vault)
}

/// Windows Hello before a secret, unless the user switched it off
/// (`general.hello_before_secrets`).
async fn presence_for_secrets(ctx: &Context) -> Result<(), Failure> {
    let settings = ctx.settings();
    if !settings.general.hello_before_secrets {
        return Ok(());
    }
    let reason = hello_reason(settings.general.language, sys_locale::get_locale().as_deref());
    ctx.gate.ensure(reason).await.map_err(|code| end_error(code, None))
}

fn vault_failure(e: sshcraft_store::vault::VaultError) -> Failure {
    let e = EngineError::from(e);
    end_error(e.failure(), e.detail_text())
}

/// A stored key ready to sign in, with its certificate. Windows Hello guards only keys whose
/// secret lies open in the vault: a key with a passphrase asks for it instead, and a security
/// key's authenticator checks the user itself.
/// Keys opened with their passphrase during one tab: a reconnect or a second hop with the same
/// key does not ask again. Lives as long as the tab's task, then drops (and wipes) the keys.
#[derive(Default)]
pub(crate) struct OpenedKeys(Mutex<HashMap<KeyId, PrivateKey>>);

impl OpenedKeys {
    fn get(&self, id: KeyId) -> Option<PrivateKey> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).get(&id).cloned()
    }

    fn keep(&self, id: KeyId, key: &PrivateKey) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(id, key.clone());
    }
}

async fn stored_credential(
    ctx: &Context,
    id: KeyId,
    resolved: &ResolvedHost,
    opened: &OpenedKeys,
) -> Result<(Credential, Option<Certificate>), Failure> {
    // A synced host can point to a key that lives on another device: say so before asking for
    // Windows Hello for nothing.
    let record = ctx
        .store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .key_record(id)
        .map_err(|e| end_error(ErrorCode::Store, Some(e.to_string())))?
        .ok_or_else(|| end_error(ErrorCode::KeyNotOnDevice, None))?;
    let certificate = record.certificate.as_deref().and_then(|text| Certificate::from_openssh(text).ok());
    let vault = ctx.vault.as_ref().ok_or_else(|| end_error(ErrorCode::VaultLocked, None))?;
    let stored = {
        let store = ctx.store.lock().unwrap_or_else(PoisonError::into_inner);
        vault.stored_openssh(&store, id).map_err(vault_failure)?
    };
    let key = PrivateKey::from_openssh(stored.as_str()).map_err(|e| end_error(ErrorCode::Vault, Some(e.to_string())))?;
    // A key with a passphrase is opened first, whatever kind it is: a security key that came
    // encrypted (from `ssh-keygen`) signs once its handle is readable.
    let asked = key.is_encrypted();
    let key = if !asked {
        key
    } else if let Some(open) = opened.get(id) {
        open
    } else {
        let open = ask_passphrase(ctx, &record.name, &key, resolved).await?;
        opened.keep(id, &open);
        open
    };
    if sshcraft_fido::is_security_key(&key) {
        let auth = ctx.security.clone().ok_or_else(|| end_error(ErrorCode::NotSupported, None))?;
        let attachment = match record.kind {
            KeyKind::Device => Attachment::Platform,
            KeyKind::Vault => Attachment::CrossPlatform,
        };
        let signer = SkSigner::new(auth, &key, attachment).map_err(|e| end_error(ErrorCode::SecurityKeyFailed, Some(e.to_string())))?;
        let credential = Credential::Signer { public_key: key.public_key().clone(), signer: Box::new(signer) };
        return Ok((credential, certificate));
    }
    if asked {
        return Ok((Credential::Key(key), certificate));
    }
    unlock_secrets(ctx).await?;
    Ok((Credential::Key(key), certificate))
}

/// Asks for the passphrase of `key` (named `name`) at most [`MAX_PASSPHRASE_TRIES`] times.
async fn ask_passphrase(ctx: &Context, name: &str, key: &PrivateKey, resolved: &ResolvedHost) -> Result<PrivateKey, Failure> {
    for _ in 0..MAX_PASSPHRASE_TRIES {
        let answers = ctx
            .auth
            .answer(AuthRequest {
                host_name: resolved.name.clone(),
                username: resolved.username.clone(),
                name: String::new(),
                instructions: String::new(),
                prompts: vec![AuthPromptLine { text: "Passphrase:".into(), echo: false }],
                passphrase_for: Some(name.to_owned()),
            })
            .await
            .ok_or(Failure::End(EndReason::AuthCancelled, None, None))?;
        let Some(passphrase) = answers.into_iter().next() else {
            return Err(Failure::End(EndReason::AuthCancelled, None, None));
        };
        // bcrypt takes up to a second per try: not on an async worker.
        let encrypted = key.clone();
        let opened = tokio::task::spawn_blocking(move || encrypted.decrypt(passphrase.as_str()))
            .await
            .map_err(|e| end_error(ErrorCode::Internal, Some(e.to_string())))?;
        if let Ok(key) = opened {
            return Ok(key);
        }
    }
    Err(Failure::End(EndReason::AuthFailed, Some(ErrorCode::WrongPassphrase), None))
}

/// The public half of a key to put into `authorized_keys`, and whether the server should
/// demand the key's PIN (`verify-required`).
async fn public_half(ctx: &Context, key: &KeyRef) -> Result<(PublicKey, bool), (ErrorCode, Option<String>)> {
    match key {
        KeyRef::Stored { id } => {
            let record = ctx
                .store
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .key_record(*id)
                .map_err(|e| (ErrorCode::Store, Some(e.to_string())))?
                .ok_or((ErrorCode::KeyNotOnDevice, None))?;
            let public = PublicKey::from_openssh(&record.public_key).map_err(|e| (ErrorCode::InvalidKey, Some(e.to_string())))?;
            let mut verify_required = false;
            if record.algorithm.starts_with("sk-")
                && let Some(vault) = &ctx.vault
            {
                let stored = vault
                    .stored_openssh(&ctx.store.lock().unwrap_or_else(PoisonError::into_inner), *id)
                    .map_err(|e| (ErrorCode::Vault, Some(e.to_string())))?;
                let flags = match PrivateKey::from_openssh(stored.as_str()).ok().map(|k| k.key_data().clone()) {
                    Some(russh::keys::ssh_key::private::KeypairData::SkEcdsaSha2NistP256(sk)) => sk.flags(),
                    Some(russh::keys::ssh_key::private::KeypairData::SkEd25519(sk)) => sk.flags(),
                    _ => 0,
                };
                verify_required = flags & sshcraft_fido::FLAG_USER_VERIFICATION != 0;
            }
            Ok((public, verify_required))
        }
        KeyRef::Agent { fingerprint } => {
            let mut agent = match ctx.agent.connect().await {
                Ok(agent) => agent,
                Err(e) => {
                    let twin = stored_twin(ctx, fingerprint).map_err(failure_code)?;
                    return match twin {
                        Some(id) => Box::pin(public_half(ctx, &KeyRef::Stored { id })).await,
                        None => Err((ErrorCode::AgentUnavailable, Some(e.to_string()))),
                    };
                }
            };
            let keys = sshcraft_ssh::list_agent_keys(&mut agent).await.map_err(|e| (ErrorCode::AgentUnavailable, Some(e.to_string())))?;
            let found = keys.into_iter().find(|k| &k.fingerprint == fingerprint).ok_or((ErrorCode::AgentKeyMissing, None))?;
            let public = PublicKey::from_openssh(&found.public_key).map_err(|e| (ErrorCode::InvalidKey, Some(e.to_string())))?;
            Ok((public, false))
        }
    }
}

/// A key in the vault with the fingerprint of an agent key: where there is no agent (Android),
/// the user imports the same key, and the host stays set to the agent for the devices that have
/// one. A device key never stands in, only one the user imported.
fn stored_twin(ctx: &Context, fingerprint: &str) -> Result<Option<KeyId>, Failure> {
    // Without the vault no stored key signs: the agent stays what is missing.
    if ctx.vault.is_none() {
        return Ok(None);
    }
    let store = ctx.store.lock().unwrap_or_else(PoisonError::into_inner);
    let keys = sshcraft_store::vault::list_keys(&store).map_err(vault_failure)?;
    Ok(keys.into_iter().find(|k| k.fingerprint == fingerprint && k.kind == KeyKind::Vault).map(|k| k.id))
}

/// A failure as a command error.
pub(crate) fn failure_code(f: Failure) -> (ErrorCode, Option<String>) {
    match f {
        Failure::End(reason, code, detail) => (
            code.unwrap_or(match reason {
                EndReason::AuthCancelled => ErrorCode::SignInCancelled,
                EndReason::HostKeyMismatch => ErrorCode::HostKeyChanged,
                EndReason::HostKeyRejected => ErrorCode::HostKeyRejected,
                _ => ErrorCode::Session,
            }),
            detail,
        ),
        Failure::Lost(code, detail) => (code, Some(detail)),
    }
}

/// A stored password, or `None` when it is not on this device (then the server's question goes
/// to the user).
async fn stored_password(ctx: &Context, id: PasswordId) -> Result<Option<Zeroizing<String>>, Failure> {
    let here = ctx
        .store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .password_record(id)
        .map_err(|e| end_error(ErrorCode::Store, Some(e.to_string())))?
        .is_some();
    if !here {
        return Ok(None);
    }
    let vault = unlock_secrets(ctx).await?;
    let store = ctx.store.lock().unwrap_or_else(PoisonError::into_inner);
    vault.password(&store, id).map(Some).map_err(vault_failure)
}

/// One hop with its own credentials only.
async fn hop(ctx: &Context, resolved: &ResolvedHost, opened: &OpenedKeys) -> Result<Hop, Failure> {
    let mut certificate = None;
    let key = match &resolved.key {
        None => None,
        Some(KeyRef::Stored { id }) => {
            let (credential, cert) = stored_credential(ctx, *id, resolved, opened).await?;
            certificate = cert;
            Some(credential)
        }
        Some(KeyRef::Agent { fingerprint }) => match ctx.agent.connect().await {
            Ok(agent) => Some(Credential::Agent { fingerprint: fingerprint.clone(), agent }),
            Err(e) => match stored_twin(ctx, fingerprint)? {
                Some(id) => {
                    let (credential, cert) = stored_credential(ctx, id, resolved, opened).await?;
                    certificate = cert;
                    Some(credential)
                }
                None => return Err(end_error(ErrorCode::AgentUnavailable, Some(e.to_string()))),
            },
        },
    };
    // The TOTP secret becomes a maker of the code of the moment it is asked for.
    let totp = match resolved.totp {
        Some(id) => match stored_password(ctx, id).await? {
            Some(text) => {
                let secret =
                    zeroize::Zeroizing::new(sshcraft_model::totp::decode_secret(&text).ok_or_else(|| end_error(ErrorCode::TotpInvalid, None))?);
                let make: sshcraft_ssh::CodeMaker = Arc::new(move || {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                    zeroize::Zeroizing::new(sshcraft_model::totp::code(&secret, now))
                });
                Some(make)
            }
            None => None,
        },
        None => None,
    };
    let password = match (&resolved.password_ref, resolved.password) {
        (Some(reference), _) => Some(referenced_password(ctx, resolved, reference).await?),
        (None, Some(id)) => stored_password(ctx, id).await?,
        (None, None) => None,
    };
    Ok(Hop {
        host_name: resolved.name.clone(),
        address: resolved.address.clone(),
        port: resolved.port,
        username: resolved.username.clone(),
        auth: Auth { key, password, certificate, totp },
    })
}

async fn proxy_target(ctx: &Context, endpoint: &ProxyEndpoint) -> Result<ProxyTarget, Failure> {
    let password = match endpoint.password {
        Some(id) => stored_password(ctx, id).await?,
        None => None,
    };
    Ok(ProxyTarget { address: endpoint.address.clone(), port: endpoint.port, username: endpoint.username.clone(), password })
}

/// What a failed connection means for the tab.
fn failure(e: SshError, verifier: &StoreVerifier) -> Failure {
    match e {
        SshError::HostKeyRejected => match verifier.mismatch.lock().unwrap_or_else(PoisonError::into_inner).take() {
            Some(detail) => Failure::End(EndReason::HostKeyMismatch, None, Some(detail)),
            None => Failure::End(EndReason::HostKeyRejected, None, None),
        },
        SshError::AuthFailed => Failure::End(EndReason::AuthFailed, None, None),
        SshError::AuthCancelled => Failure::End(EndReason::AuthCancelled, None, None),
        SshError::PasswordRejected => Failure::End(EndReason::AuthFailed, Some(ErrorCode::PasswordRejected), None),
        SshError::ProxyAuthFailed => Failure::End(EndReason::AuthFailed, Some(ErrorCode::ProxyAuthFailed), None),
        SshError::AgentUnavailable(detail) => end_error(ErrorCode::AgentUnavailable, Some(detail)),
        SshError::AgentKeyMissing => end_error(ErrorCode::AgentKeyMissing, None),
        SshError::AgentRefused => Failure::End(EndReason::AuthFailed, Some(ErrorCode::AgentRefused), None),
        SshError::SignerCancelled => Failure::End(EndReason::AuthCancelled, Some(ErrorCode::SecurityKeyCancelled), None),
        SshError::SignerFailed(detail) => Failure::End(EndReason::AuthFailed, Some(ErrorCode::SecurityKeyFailed), Some(detail)),
        SshError::Proxy(detail) => Failure::Lost(ErrorCode::ProxyFailed, detail),
        SshError::Hop { host_name, error, .. } => match failure(*error, verifier) {
            kept @ Failure::End(EndReason::HostKeyRejected | EndReason::HostKeyMismatch, ..) => kept,
            // What went wrong stays; the detail names the jump host.
            Failure::End(reason, code, _) => Failure::End(reason, Some(code.unwrap_or(ErrorCode::JumpFailed)), Some(host_name)),
            Failure::Lost(code, detail) => Failure::Lost(code, format!("{host_name}: {detail}")),
        },
        other => Failure::Lost(ErrorCode::Network, other.to_string()),
    }
}

/// Whether a wake event arrived (a missed one counts too); a closed channel never wakes.
pub(crate) fn woke_up(received: &Result<WakeEvent, tokio::sync::broadcast::error::RecvError>) -> bool {
    !matches!(received, Err(tokio::sync::broadcast::error::RecvError::Closed))
}

/// Waits before the next reconnect attempt, or less when the computer woke up or the network
/// changed: then the attempt is worth making now.
pub(crate) async fn wait_or_wake(delay: std::time::Duration, wake: &mut tokio::sync::broadcast::Receiver<WakeEvent>) {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);
    tokio::select! {
        () = &mut sleep => {}
        woke = wake.recv() => {
            // Closed: nobody will wake us, so the delay is waited out.
            if !woke_up(&woke) {
                sleep.await;
            }
        }
    }
}

/// The shell's writer and events, a notice, a prober, and the SSH connection the shell runs on
/// (kept while the tab lives, so other users can share it).
type Connected =
    (Writer, tokio::sync::mpsc::Receiver<ShellEvent>, Option<ErrorCode>, Option<sshcraft_ssh::Prober>, Option<Arc<sshcraft_ssh::Session>>);

async fn connect_target(ctx: &Arc<Context>, target: &SessionTarget, size: &Mutex<(u32, u32)>, opened: &OpenedKeys) -> Result<Connected, Failure> {
    match target {
        SessionTarget::Host { host_id } => connect_once(ctx, *host_id, size, opened).await,
        SessionTarget::Local { shell, cwd } => {
            let (cols, rows) = *size.lock().unwrap_or_else(PoisonError::into_inner);
            let term = ctx.settings().terminal.terminal_type;
            let (writer, events) = sshcraft_local::pty::spawn(shell, to_u16(cols), to_u16(rows), &term, cwd.as_deref().map(std::path::Path::new))
                .map_err(|e| end_error(ErrorCode::LocalShellFailed, Some(e.to_string())))?;
            Ok((Arc::new(writer), events, None, None, None))
        }
        SessionTarget::Serial { settings } => {
            settings.validate().map_err(|e| end_error(ErrorCode::SerialFailed, Some(e.to_string())))?;
            // A port that is gone may come back (an adapter plugged in again): retried like a
            // lost connection once it was open.
            let (writer, events) = sshcraft_local::serial::open(settings).map_err(|e| Failure::Lost(ErrorCode::SerialFailed, e.to_string()))?;
            Ok((Arc::new(writer), events, None, None, None))
        }
    }
}

async fn connect_once(ctx: &Arc<Context>, host_id: HostId, size: &Mutex<(u32, u32)>, opened: &OpenedKeys) -> Result<Connected, Failure> {
    // A session that outlives the connection: the fixed tmux or screen command, attached again on
    // every connect.
    let route = {
        let store = ctx.store.lock().unwrap_or_else(PoisonError::into_inner);
        lookup(&store, host_id).map_err(|(code, detail)| end_error(code, detail))?
    };
    if route.target.protocol == sshcraft_model::model::Protocol::Telnet {
        return connect_telnet(ctx, &route, size).await;
    }
    let persistence = route.target.persistence.clone();
    // A container opens instead of the host's shell; otherwise a session that stays.
    let command = match (&route.target.container, &persistence) {
        (Some(c), _) => Some(c.command().ok_or_else(|| end_error(ErrorCode::ContainerNameInvalid, Some(c.name.clone())))?),
        (None, Some(p)) => Some(p.command().ok_or_else(|| end_error(ErrorCode::SessionNameInvalid, Some(p.name.clone())))?),
        (None, None) => None,
    };
    let (session, notice) = connect_session(ctx, host_id, opened, true).await?;
    let (cols, rows) = *size.lock().unwrap_or_else(PoisonError::into_inner);
    // Read on every connect, so a reconnect uses the type set now.
    let term = ctx.settings().terminal.terminal_type;
    let shell = session.open_terminal(cols, rows, &term, command.as_deref()).await.map_err(|e| Failure::Lost(ErrorCode::Network, e.to_string()))?;
    let prober = Some(session.prober());
    if needs_os(&ctx.store, host_id) {
        tokio::spawn(detect_os(ctx.store.clone(), host_id, session.clone()));
    }
    let (writer, events) = shell.split();
    Ok((Arc::new(writer), events, notice, prober, Some(session)))
}

/// A Telnet host: straight to its address, no sign-in by SSHCraft (the device asks, the user
/// types), never stored credentials.
async fn connect_telnet(ctx: &Arc<Context>, route: &Route, size: &Mutex<(u32, u32)>) -> Result<Connected, Failure> {
    if !route.jumps.is_empty() || route.target.proxy.is_some() {
        return Err(end_error(ErrorCode::TelnetDirectOnly, None));
    }
    // Plaintext only where it was chosen on this device.
    if !telnet_allowed(&ctx.store.lock().unwrap_or_else(PoisonError::into_inner), route.target.host_id) {
        return Err(end_error(ErrorCode::TelnetNotAllowed, None));
    }
    // Wake the device, knock, start the VPN; the steps after run when the connection goes.
    crate::hooks::run(ctx, route.target.host_id, crate::hooks::When::Before).await?;
    let after: Arc<dyn std::any::Any + Send + Sync> = Arc::new(crate::hooks::AfterGuard::new(ctx.clone(), route.target.host_id));
    // A host without its own port has SSH's: Telnet's is 23.
    let port = if route.target.port == sshcraft_model::model::DEFAULT_PORT { sshcraft_telnet::DEFAULT_PORT } else { route.target.port };
    let (cols, rows) = *size.lock().unwrap_or_else(PoisonError::into_inner);
    let term = ctx.settings().terminal.terminal_type;
    let size = (u16::try_from(cols).unwrap_or(u16::MAX), u16::try_from(rows).unwrap_or(u16::MAX));
    let (writer, events) = sshcraft_telnet::connect(&route.target.address, port, &term, size, sshcraft_ssh::CONNECT_TIMEOUT)
        .await
        .map_err(|e| Failure::Lost(ErrorCode::Network, e.to_string()))?;
    Ok((Arc::new(writer.keep(after)), events, Some(ErrorCode::TelnetUnencrypted), None, None))
}

#[async_trait::async_trait]
impl TermWriter for sshcraft_telnet::TelnetWriter {
    async fn write(&self, data: &[u8]) -> Result<(), String> {
        sshcraft_telnet::TelnetWriter::write(self, data).await.map_err(|e| e.to_string())
    }

    async fn resize(&self, cols: u32, rows: u32) -> Result<(), String> {
        sshcraft_telnet::TelnetWriter::resize(self, to_u16(cols), to_u16(rows)).await.map_err(|e| e.to_string())
    }

    fn close(&self) {
        let writer = self.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move { writer.close().await });
        }
    }
}

/// Connects to a host along its route and signs in on every hop; no shell yet. The notice is
/// something to show while connected.
/// `share`: the connection may be one already open to the same route, and later users may share
/// it (setting `terminal.reuse_connections`).
pub(crate) async fn connect_session(
    ctx: &Arc<Context>,
    host_id: HostId,
    opened: &OpenedKeys,
    share: bool,
) -> Result<(Arc<sshcraft_ssh::Session>, Option<ErrorCode>), Failure> {
    let route = {
        let store = ctx.store.lock().unwrap_or_else(PoisonError::into_inner);
        lookup(&store, host_id).map_err(|(code, detail)| end_error(code, detail))?
    };
    let share = share && ctx.settings().terminal.reuse_connections;
    let pool_key = crate::pool::key(host_id, &route);
    // Users of one route that start at once wait for the first one's connection.
    let gate = share.then(|| ctx.pool.gate(&pool_key));
    let _connecting = match &gate {
        Some(gate) => Some(gate.lock().await),
        None => None,
    };
    if share && let Some(session) = ctx.pool.get(&pool_key) {
        return Ok((session, None));
    }
    // Wake the computer, knock, start the VPN: before every new connection.
    crate::hooks::run(ctx, host_id, crate::hooks::When::Before).await?;
    // From here the steps after the session run once this connection, or this try, is over.
    let after = crate::hooks::AfterGuard::new(ctx.clone(), host_id);
    let mut hops = Vec::new();
    for resolved in &route.jumps {
        // A jump host that cannot be used is named.
        hops.push(hop(ctx, resolved, opened).await.map_err(|f| match f {
            Failure::End(reason, code, None) => Failure::End(reason, code, Some(resolved.name.clone())),
            other => other,
        })?);
    }
    hops.push(hop(ctx, &route.target, opened).await?);
    let proxy = match &route.target.proxy {
        Some(Proxy::Http(endpoint)) => Some(ProxyConfig::Http(proxy_target(ctx, endpoint).await?)),
        Some(Proxy::Socks5(endpoint)) => Some(ProxyConfig::Socks5(proxy_target(ctx, endpoint).await?)),
        Some(Proxy::Command { argv }) => {
            allow_command(ctx, &route.target.name, argv).await?;
            // The tokens name the first hop, the one the program connects to.
            let first = route.jumps.first().unwrap_or(&route.target);
            let tokens =
                sshcraft_model::proxy_command::Tokens { host: &first.address, port: first.port, user: &first.username, alias: &route.target.name };
            sshcraft_model::proxy_command::check_tokens(argv, &tokens)
                .map_err(|e| end_error(ErrorCode::ProxyCommandNotAllowed, Some(e.to_string())))?;
            let mut argv = sshcraft_model::proxy_command::expand(argv, &tokens);
            // `~/` at the start of an argument is the home folder, as `sh` reads it.
            if let Some(home) = sshcraft_platform::home_dir() {
                for arg in &mut argv {
                    if let Some(rest) = arg.strip_prefix("~/") {
                        *arg = home.join(rest).to_string_lossy().into_owned();
                    }
                }
            }
            Some(ProxyConfig::Command { argv })
        }
        Some(Proxy::Direct) | None => None,
    };
    let opener = if route.target.agent_forwarding { ctx.agent.opener() } else { None };
    // X11 hands the server this computer's display: only where it was allowed on this device.
    let x11_allowed = route.target.x11 && x11_allowed(&ctx.store.lock().unwrap_or_else(PoisonError::into_inner), host_id);
    let display = if x11_allowed { sshcraft_ssh::x11::local_display() } else { None };
    let notice = (route.target.agent_forwarding && opener.is_none())
        .then_some(ErrorCode::AgentForwardingUnavailable)
        .or_else(|| (route.target.x11 && !x11_allowed).then_some(ErrorCode::X11NotAllowed))
        .or_else(|| (x11_allowed && display.is_none()).then_some(ErrorCode::X11Unavailable));
    let x11 = match display {
        Some(display) => {
            let cookie = sshcraft_ssh::x11::real_cookie(&display).await;
            Some(sshcraft_ssh::x11::X11Options { display, cookie })
        }
        None => None,
    };
    let options = SessionOptions { agent_forwarding: opener, env: route.target.env.iter().map(|v| (v.name.clone(), v.value.clone())).collect(), x11 };
    // The host key question runs in russh's own task. When this connect is given up (its tab
    // closed, its rule stopped), `alive` drops and the question is withdrawn.
    let (_alive, abandoned) = tokio::sync::watch::channel(());
    let asking = crate::prompts::asking_session();
    let verifier = Arc::new(StoreVerifier {
        session: asking,
        store: ctx.store.clone(),
        prompter: ctx.prompter.clone(),
        mismatch: Mutex::new(None),
        first: Mutex::new(Vec::new()),
        abandoned,
        update_keys: ctx.settings().general.update_host_keys,
    });
    let params = RouteParams { hops, proxy, options, connect_timeout: sshcraft_ssh::CONNECT_TIMEOUT };
    // The sign-in questions may be asked from another task: they carry the session along.
    let auth: Arc<dyn AuthPrompter> = match asking {
        Some(session) => Arc::new(SessionAuth { inner: ctx.auth.clone(), session }),
        None => ctx.auth.clone(),
    };
    let session = sshcraft_ssh::connect_route(params, verifier.clone(), auth).await.map_err(|e| failure(e, &verifier))?;
    let notice = notice.or(session.certificate_expired().then_some(ErrorCode::CertificateExpired));
    // The steps after the session run when the last tab, forward or command lets go of it.
    session.on_last_drop(Box::new(move || drop(after)));
    let session = Arc::new(session);
    if share {
        ctx.pool.put(pool_key, &session);
    }
    Ok((session, notice))
}

/// A password from a password manager: only for an address it was allowed for on this device
/// (else the user is asked; a shared host could otherwise have another of the user's secrets
/// sent to a server of its choosing), then fetched by the manager's program.
async fn referenced_password(
    ctx: &Context,
    resolved: &ResolvedHost,
    reference: &sshcraft_model::secret_ref::SecretRef,
) -> Result<zeroize::Zeroizing<String>, Failure> {
    use sshcraft_model::secret_ref::Manager;
    reference.check().map_err(|e| end_error(ErrorCode::SecretRefFailed, Some(e.to_string())))?;
    let digest = crate::secrets::approval_digest(reference, &resolved.address, resolved.port);
    let store_error = |e: sshcraft_store::store::StoreError| end_error(ErrorCode::Store, Some(e.to_string()));
    let approved = ctx.store.lock().unwrap_or_else(PoisonError::into_inner).command_approved(&digest).map_err(store_error)?;
    if !approved {
        let description = format!("{} {} → {}:{}", reference.manager.label(), reference.reference, resolved.address, resolved.port);
        if !ctx.prompter.ask_secret_ref(&resolved.name, &description).await {
            return Err(end_error(ErrorCode::SecretRefNotAllowed, Some(description)));
        }
        ctx.store.lock().unwrap_or_else(PoisonError::into_inner).approve_command(&digest).map_err(store_error)?;
    }
    // A password from a manager is a secret like a stored one: Windows Hello first (it needs
    // no vault, so the gate is asked directly).
    presence_for_secrets(ctx).await?;
    let database = match reference.manager {
        Manager::KeePassXc => reference.reference.split_once('#').map(|(db, _)| db.to_owned()),
        _ => None,
    };
    let mut asked = None;
    if let Some(db) = &database
        && !ctx.kdbx.lock().unwrap_or_else(PoisonError::into_inner).contains_key(db)
    {
        let request = AuthRequest {
            host_name: resolved.name.clone(),
            username: String::new(),
            name: "KeePassXC".into(),
            instructions: format!("The password of the KeePassXC database {db}, for this run of SSHCraft."),
            prompts: vec![AuthPromptLine { text: "Database password:".into(), echo: false }],
            passphrase_for: None,
        };
        let answer = ctx.auth.answer(request).await.and_then(|mut a| a.pop()).ok_or(Failure::End(EndReason::AuthCancelled, None, None))?;
        asked = Some(answer);
    }
    let db_password = match (&database, &asked) {
        (_, Some(a)) => Some(a.clone()),
        (Some(db), None) => ctx.kdbx.lock().unwrap_or_else(PoisonError::into_inner).get(db).cloned(),
        _ => None,
    };
    let source = ctx.secrets.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let password = source
        .fetch(reference, db_password.as_ref().map(|p| p.as_str()))
        .await
        .map_err(|e| Failure::End(EndReason::AuthFailed, Some(ErrorCode::SecretRefFailed), Some(e)))?;
    // A database password that worked is kept for this run.
    if let (Some(db), Some(a)) = (database, asked) {
        ctx.kdbx.lock().unwrap_or_else(PoisonError::into_inner).insert(db, a);
    }
    Ok(password)
}

/// A proxy command runs only when it was set on this device or allowed here once: one that
/// came with sync, a share or an agent's change is asked about first, and the answer is kept.
async fn allow_command(ctx: &Context, host_name: &str, argv: &[String]) -> Result<(), Failure> {
    allow_program(ctx, host_name, argv, argv).await
}

/// Lets a local program run once this device allowed `digest_of` (its arguments, maybe with a
/// prefix saying what it is for); else asks the user with `argv` shown.
/// What allows Telnet on this device for the host or group `id` that chooses it.
pub(crate) fn telnet_approval(id: &str) -> Vec<String> {
    vec!["telnet".to_string(), id.to_string()]
}

/// Whether Telnet for `host_id` was chosen on this device: the level that sets the protocol (the
/// host, or the nearest group that sets it) was allowed here.
pub(crate) fn telnet_allowed(store: &Store, host_id: HostId) -> bool {
    use sshcraft_model::model::Protocol;
    let (Ok(hosts), Ok(groups)) = (store.list_hosts(), store.list_groups()) else { return false };
    let Some(host) = hosts.iter().find(|h| h.id == host_id) else { return false };
    let source = match host.settings.protocol {
        Some(Protocol::Telnet) => Some(host.id.0.to_string()),
        Some(Protocol::Ssh) => None,
        None => host
            .group_id
            .map(|g| sshcraft_model::model::group_chain(&groups, g))
            .unwrap_or_default()
            .into_iter()
            .find(|g| g.defaults.protocol.is_some())
            .filter(|g| g.defaults.protocol == Some(Protocol::Telnet))
            .map(|g| g.id.0.to_string()),
    };
    source.is_some_and(|id| store.command_approved(&sshcraft_store::store::command_digest(&telnet_approval(&id))).unwrap_or(false))
}

/// What allows X11 forwarding on this device for the host or group `id` that switches it on.
pub(crate) fn x11_approval(id: &str) -> Vec<String> {
    vec!["x11".to_string(), id.to_string()]
}

/// Whether X11 forwarding for `host_id` is allowed on this device: the level that switches it
/// on (the host, or the nearest group that sets it) was allowed here.
pub(crate) fn x11_allowed(store: &Store, host_id: HostId) -> bool {
    let (Ok(hosts), Ok(groups)) = (store.list_hosts(), store.list_groups()) else { return false };
    let Some(host) = hosts.iter().find(|h| h.id == host_id) else { return false };
    let source = match host.settings.x11 {
        Some(true) => Some(host.id.0.to_string()),
        Some(false) => None,
        None => host
            .group_id
            .map(|g| sshcraft_model::model::group_chain(&groups, g))
            .unwrap_or_default()
            .into_iter()
            .find(|g| g.defaults.x11.is_some())
            .filter(|g| g.defaults.x11 == Some(true))
            .map(|g| g.id.0.to_string()),
    };
    source.is_some_and(|id| store.command_approved(&sshcraft_store::store::command_digest(&x11_approval(&id))).unwrap_or(false))
}

/// Whether the program `digest_of` names was allowed on this device; asks nobody.
pub(crate) fn program_allowed(ctx: &Context, digest_of: &[String]) -> bool {
    let digest = sshcraft_store::store::command_digest(digest_of);
    ctx.store.lock().unwrap_or_else(PoisonError::into_inner).command_approved(&digest).unwrap_or(false)
}

pub(crate) async fn allow_program(ctx: &Context, host_name: &str, digest_of: &[String], argv: &[String]) -> Result<(), Failure> {
    let digest = sshcraft_store::store::command_digest(digest_of);
    let approved = ctx
        .store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .command_approved(&digest)
        .map_err(|e| end_error(ErrorCode::Store, Some(e.to_string())))?;
    if approved {
        return Ok(());
    }
    let line = sshcraft_model::proxy_command::join(argv);
    if !ctx.prompter.ask_command(host_name, &line).await {
        return Err(end_error(ErrorCode::ProxyCommandNotAllowed, Some(line)));
    }
    ctx.store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .approve_command(&digest)
        .map_err(|e| end_error(ErrorCode::Store, Some(e.to_string())))?;
    Ok(())
}

fn probed_setting(host_id: HostId) -> String {
    format!("os.probed.{}", host_id.0)
}

/// Notes on this device that a host was asked for its system, recognized or not.
pub(crate) fn mark_probed(store: &Store, host_id: HostId) {
    let _ = store.put_setting(&probed_setting(host_id), "1");
}

/// Whether a host has no symbol yet and was never asked from this device.
pub(crate) fn needs_os(store: &Mutex<Store>, host_id: HostId) -> bool {
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    let asked = store.get_setting(&probed_setting(host_id)).is_ok_and(|v| v.is_some());
    !asked && store.list_hosts().is_ok_and(|hosts| hosts.iter().any(|h| h.id == host_id && h.os.is_none()))
}

/// Asks the server once which operating system it runs and stores it on the host (sent like
/// any change, unless the host came in a share that may only be read). Failures are ignored:
/// the symbol simply stays neutral.
async fn detect_os(store: Arc<Mutex<Store>>, host_id: HostId, session: Arc<sshcraft_ssh::Session>) {
    // Asked once: a server whose system is not recognized (or that runs a forced command for
    // every session) is not asked again on each connection.
    mark_probed(&store.lock().unwrap_or_else(PoisonError::into_inner), host_id);
    let Ok(output) = session.exec(sshcraft_model::os_detect::OS_PROBE, 16 * 1024).await else {
        return;
    };
    let Some(os) = sshcraft_model::os_detect::from_output(&String::from_utf8_lossy(&output)) else {
        return;
    };
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    let Ok(hosts) = store.list_hosts() else {
        return;
    };
    let Some(mut host) = hosts.into_iter().find(|h| h.id == host_id && h.os.is_none()) else {
        return;
    };
    host.os = Some(os);
    let id = host.id.0.to_string();
    let read_only = host.location.is_some_and(|server| store.read_only(server, sshcraft_store::store::ItemKind::Host, &id).unwrap_or(true));
    if store.upsert_host(&host).is_err() || read_only {
        return;
    }
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    // Just after the version this device knows: a change made meanwhile on another device
    // (a rename) is newer and wins over this background update.
    let at = host
        .location
        .and_then(|server| store.record_entry(server, sshcraft_store::store::ItemKind::Host, &id).ok().flatten())
        .map_or(now_ms, |entry| entry.modified_at.saturating_add(1));
    let _ = sshcraft_sync::engine::record_local_change(&store, sshcraft_store::store::ItemKind::Host, &id, at);
}

fn lookup(store: &Store, host_id: HostId) -> Result<Route, (ErrorCode, Option<String>)> {
    let store_error = |e: sshcraft_store::store::StoreError| (ErrorCode::Store, Some(e.to_string()));
    let hosts = store.list_hosts().map_err(store_error)?;
    let groups = store.list_groups().map_err(store_error)?;
    let identities = store.list_identities().map_err(store_error)?;
    let dir = Directory { groups: &groups, hosts: &hosts, identities: &identities };
    resolve_route(&dir, host_id).map_err(|e| {
        // A jump host that cannot be used is named.
        let detail = match &e {
            ResolveError::Jump { host_id, .. } => hosts.iter().find(|h| h.id == *host_id).map(|h| h.name.clone()),
            _ => None,
        };
        (ErrorCode::from(&e), detail)
    })
}

/// Asks the sign-in questions of one session, naming it.
struct SessionAuth {
    inner: Arc<dyn AuthPrompter>,
    session: SessionId,
}

#[async_trait::async_trait]
impl AuthPrompter for SessionAuth {
    async fn answer(&self, request: sshcraft_ssh::AuthRequest) -> Option<Vec<zeroize::Zeroizing<String>>> {
        crate::prompts::ASKING_SESSION.scope(self.session, self.inner.answer(request)).await
    }
}

struct StoreVerifier {
    /// The session that connects, for its question (russh asks from its own task).
    session: Option<SessionId>,
    store: Arc<Mutex<Store>>,
    prompter: Arc<dyn HostKeyPrompter>,
    mismatch: Mutex<Option<String>>,
    /// Servers whose host key the user trusted during this connection.
    first: Mutex<Vec<(String, u16)>>,
    /// Closes when whoever connects gives up: an open question is then withdrawn.
    abandoned: tokio::sync::watch::Receiver<()>,
    /// Take the keys a server announces after sign-in (`general.update_host_keys`).
    update_keys: bool,
}

/// A presented key as the store keeps it.
fn stored_key(address: &str, port: u16, key: &sshcraft_ssh::hostkeys::PresentedKey) -> sshcraft_store::store::HostKey {
    sshcraft_store::store::HostKey {
        address: address.to_owned(),
        port,
        algorithm: Some(key.algorithm.clone()),
        public_key: Some(key.public_key.clone()),
        fingerprint: key.fingerprint.clone(),
        added_at: crate::now_ms() / 1000,
    }
}

impl StoreVerifier {
    fn known_fingerprints(&self, address: &str, port: u16) -> Option<Vec<String>> {
        let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        store.host_keys(address, port).ok().map(|keys| keys.into_iter().map(|k| k.fingerprint).collect())
    }

    /// Checks a key; with the whole key at hand it is stored whole (also filling in a key
    /// known only by its fingerprint).
    async fn check(
        &self,
        host_name: &str,
        address: &str,
        port: u16,
        fingerprint: &str,
        whole: Option<&sshcraft_ssh::hostkeys::PresentedKey>,
    ) -> bool {
        let Some(known) = self.known_fingerprints(address, port) else {
            return false;
        };
        let keep = |store: &Store| match whole {
            Some(key) => store.add_host_key(&stored_key(address, port, key)),
            None => store.set_known_host(address, port, fingerprint),
        };
        match check_host_key(&known, fingerprint) {
            HostKeyStatus::Trusted => {
                if whole.is_some() {
                    let _ = keep(&self.store.lock().unwrap_or_else(PoisonError::into_inner));
                }
                true
            }
            HostKeyStatus::Mismatch { expected } => {
                *self.mismatch.lock().unwrap_or_else(PoisonError::into_inner) = Some(format!("expected {expected}, got {fingerprint}"));
                false
            }
            HostKeyStatus::Unknown => {
                let mut abandoned = self.abandoned.clone();
                let accepted = tokio::select! {
                    accepted = async {
                        match self.session {
                            Some(id) => crate::prompts::ASKING_SESSION.scope(id, self.prompter.ask(host_name, address, port, fingerprint)).await,
                            None => self.prompter.ask(host_name, address, port, fingerprint).await,
                        }
                    } => accepted,
                    // Nobody waits for the answer any more: drop the question, trust nothing.
                    _ = abandoned.changed() => return false,
                };
                if accepted {
                    let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
                    if keep(&store).is_err() {
                        return false;
                    }
                    self.first.lock().unwrap_or_else(PoisonError::into_inner).push((address.to_owned(), port));
                }
                accepted
            }
        }
    }
}

#[async_trait::async_trait]
impl HostKeyVerifier for StoreVerifier {
    async fn verify(&self, host_name: &str, address: &str, port: u16, fingerprint: &str) -> bool {
        self.check(host_name, address, port, fingerprint, None).await
    }

    async fn verify_key(&self, host_name: &str, address: &str, port: u16, key: &sshcraft_ssh::hostkeys::PresentedKey) -> bool {
        self.check(host_name, address, port, &key.fingerprint, Some(key)).await
    }

    fn announced(
        &self,
        address: &str,
        port: u16,
        used: &str,
        keys: &[sshcraft_ssh::hostkeys::PresentedKey],
    ) -> sshcraft_ssh::hostkeys::AnnounceAction {
        use sshcraft_ssh::hostkeys::AnnounceAction;
        let announced: Vec<String> = keys.iter().map(|k| k.fingerprint.clone()).collect();
        let plan = match (self.update_keys, self.known_fingerprints(address, port)) {
            (true, Some(known)) => sshcraft_model::policy::plan_key_update(&known, &announced, used),
            _ => None,
        };
        match plan {
            None => AnnounceAction::Ignore,
            Some(plan) if plan.add.is_empty() => AnnounceAction::Apply,
            Some(plan) => AnnounceAction::Prove(keys.iter().filter(|k| plan.add.contains(&k.fingerprint)).cloned().collect()),
        }
    }

    fn apply(
        &self,
        address: &str,
        port: u16,
        used: &str,
        keys: &[sshcraft_ssh::hostkeys::PresentedKey],
        proven: &[sshcraft_ssh::hostkeys::PresentedKey],
    ) {
        if !self.update_keys {
            return;
        }
        let announced: Vec<String> = keys.iter().map(|k| k.fingerprint.clone()).collect();
        let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        let Ok(known) = store.host_keys(address, port) else {
            return;
        };
        let known: Vec<String> = known.into_iter().map(|k| k.fingerprint).collect();
        let Some(plan) = sshcraft_model::policy::plan_key_update(&known, &announced, used) else {
            return;
        };
        // The plan is made again (the user may have changed the keys meanwhile), but only keys
        // the server proved are added.
        for key in proven.iter().filter(|k| plan.add.contains(&k.fingerprint)) {
            let _ = store.add_host_key(&stored_key(address, port, key));
        }
        for fingerprint in &plan.remove {
            let _ = store.remove_host_key(address, port, fingerprint);
        }
        // The key in use was already known; fill in its algorithm and key if they were missing.
        for key in keys.iter().filter(|k| k.fingerprint == used) {
            let _ = store.add_host_key(&stored_key(address, port, key));
        }
    }

    fn first_contact(&self, address: &str, port: u16) -> bool {
        self.first.lock().unwrap_or_else(PoisonError::into_inner).iter().any(|(a, p)| a == address && *p == port)
    }
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;

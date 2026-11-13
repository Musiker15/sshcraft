//! Runs terminal sessions for the UI. SSH: resolves the route, loads keys and passwords, checks
//! host keys against the store (asking the user for unknown ones) and reconnects lost sessions.
//! On the desktop also a local shell and a serial port.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use russh::keys::{Certificate, PrivateKey, PublicKey};
use serde::Serialize;
use sshcraft_core::authorized_keys::{InstallOutcome, install_command, parse_outcome};
use sshcraft_core::keytypes::KeyKind;
use sshcraft_core::model::{
    Directory, HostId, KeyId, KeyRef, PasswordId, Proxy, ProxyEndpoint, ResolveError, ResolvedHost,
    Route, resolve_route,
};
use sshcraft_core::policy::{Disconnect, HostKeyStatus, check_host_key, next_reconnect_delay};
use sshcraft_core::store::Store;
use sshcraft_core::vault::Vault;
use sshcraft_fido::{Attachment, Authenticator, SkSigner};
use sshcraft_ssh::{
    Auth, AuthPromptLine, AuthPrompter, AuthRequest, Credential, Hop, HostKeyVerifier, ProxyConfig,
    ProxyTarget, RouteParams, SessionOptions, ShellEvent, ShellWriter, SshError,
};
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::errors::ErrorCode;
use crate::hello::{Presence, UserPresence};

pub type SessionId = Uuid;

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
}

/// The UI may hold this many unacknowledged output bytes before SSHCraft stops reading from
/// the server. Keeps a runaway `cat` from flooding the webview.
pub const OUTPUT_HIGH_WATERMARK: usize = 512 * 1024;

/// Output bytes handed to the UI but not yet acknowledged as rendered.
#[derive(Default)]
struct Flow {
    in_flight: AtomicUsize,
    drained: Notify,
}

impl Flow {
    fn add(&self, bytes: usize) {
        self.in_flight.fetch_add(bytes, Ordering::SeqCst);
    }

    fn ack(&self, bytes: usize) {
        let _ = self
            .in_flight
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                Some(v.saturating_sub(bytes))
            });
        self.drained.notify_waiters();
    }

    fn reset(&self) {
        self.in_flight.store(0, Ordering::SeqCst);
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
    #[cfg(desktop)]
    Local {
        shell: sshcraft_local::pty::ShellCommand,
    },
    #[cfg(desktop)]
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
}

#[async_trait::async_trait]
impl TermWriter for ShellWriter {
    async fn write(&self, data: &[u8]) -> Result<(), String> {
        ShellWriter::write(self, data)
            .await
            .map_err(|e| e.to_string())
    }

    async fn resize(&self, cols: u32, rows: u32) -> Result<(), String> {
        ShellWriter::resize(self, cols, rows)
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(desktop)]
fn to_u16(v: u32) -> u16 {
    u16::try_from(v).unwrap_or(u16::MAX)
}

#[cfg(desktop)]
#[async_trait::async_trait]
impl TermWriter for sshcraft_local::pty::LocalWriter {
    async fn write(&self, data: &[u8]) -> Result<(), String> {
        let writer = self.clone();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || writer.write(&data))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    async fn resize(&self, cols: u32, rows: u32) -> Result<(), String> {
        sshcraft_local::pty::LocalWriter::resize(self, to_u16(cols), to_u16(rows))
            .map_err(|e| e.to_string())
    }

    fn close(&self) {
        sshcraft_local::pty::LocalWriter::close(self);
    }
}

#[cfg(desktop)]
#[async_trait::async_trait]
impl TermWriter for sshcraft_local::serial::SerialWriter {
    async fn write(&self, data: &[u8]) -> Result<(), String> {
        let writer = self.clone();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || writer.write(&data))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
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
}

pub struct SessionManager {
    ctx: Arc<Context>,
    sessions: Mutex<HashMap<SessionId, Live>>,
}

struct Context {
    store: Arc<Mutex<Store>>,
    vault: Option<Arc<Vault>>,
    prompter: Arc<dyn HostKeyPrompter>,
    auth: Arc<dyn AuthPrompter>,
    agent: Arc<dyn AgentConnector>,
    gate: KeyGate,
    delay_scale: f32,
    /// Signs for FIDO2 and Windows Hello keys; `None` where there is none.
    security: Option<Arc<dyn Authenticator>>,
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
pub fn hello_reason(
    language: crate::settings::LanguageSetting,
    system_locale: Option<&str>,
) -> &'static str {
    use crate::settings::LanguageSetting as L;
    let german = match language {
        L::De => true,
        L::En => false,
        L::Auto => {
            system_locale.is_some_and(|l| l.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("de")))
        }
    };
    if german {
        "SSHCraft möchte deine gespeicherten SSH-Schlüssel verwenden."
    } else {
        "SSHCraft wants to use your stored SSH keys."
    }
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
                let first = !self
                    .hello_missing
                    .swap(true, std::sync::atomic::Ordering::SeqCst);
                if first && let Some(report) = &*self.on_unavailable.lock().expect("hook lock") {
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
            Err(SshError::AgentUnavailable(
                "no agent support on this platform yet".into(),
            ))
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
    pub fn new(
        store: Arc<Mutex<Store>>,
        vault: Option<Arc<Vault>>,
        prompter: Arc<dyn HostKeyPrompter>,
        auth: Arc<dyn AuthPrompter>,
        agent: Arc<dyn AgentConnector>,
        presence: Arc<dyn UserPresence>,
    ) -> Self {
        Self::with_delay_scale(store, vault, prompter, auth, agent, presence, 1.0)
    }

    /// Called once, the first time Windows Hello turns out not to be set up on this PC.
    pub fn on_hello_unavailable(&self, report: Box<dyn Fn() + Send + Sync>) {
        *self.ctx.gate.on_unavailable.lock().expect("hook lock") = Some(report);
    }

    /// True once Windows Hello turned out not to be set up on this PC.
    pub fn hello_missing(&self) -> bool {
        self.ctx
            .gate
            .hello_missing
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// `delay_scale` shortens reconnect delays in tests.
    pub fn with_delay_scale(
        store: Arc<Mutex<Store>>,
        vault: Option<Arc<Vault>>,
        prompter: Arc<dyn HostKeyPrompter>,
        auth: Arc<dyn AuthPrompter>,
        agent: Arc<dyn AgentConnector>,
        presence: Arc<dyn UserPresence>,
        delay_scale: f32,
    ) -> Self {
        Self {
            ctx: Arc::new(Context {
                store,
                vault,
                prompter,
                auth,
                agent,
                gate: KeyGate::new(presence),
                delay_scale,
                security: None,
            }),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Lets FIDO2 and Windows Hello keys sign through `auth`. Call before the first session.
    pub fn with_security_keys(mut self, auth: Arc<dyn Authenticator>) -> Self {
        Arc::get_mut(&mut self.ctx)
            .expect("no session started yet")
            .security = Some(auth);
        self
    }

    /// Puts a public key into `authorized_keys` on a host: connects like a tab does (with every
    /// question), runs one command, opens no shell.
    pub async fn install_key(
        &self,
        host_id: HostId,
        key: KeyRef,
    ) -> Result<InstallOutcome, (ErrorCode, Option<String>)> {
        let (public, verify_required) = public_half(&self.ctx, &key).await?;
        let (session, _) = connect_session(&self.ctx, host_id, &OpenedKeys::default())
            .await
            .map_err(failure_code)?;
        let output = session
            .exec(&install_command(&public, verify_required), 64 * 1024)
            .await
            .map_err(|e| (ErrorCode::InstallKeyFailed, Some(e.to_string())))?;
        session.disconnect().await;
        parse_outcome(&output).map_err(|e| match e {
            sshcraft_core::authorized_keys::AuthorizedKeysError::Failed => {
                (ErrorCode::InstallKeyFailed, None)
            }
            sshcraft_core::authorized_keys::AuthorizedKeysError::Unsupported => {
                (ErrorCode::InstallKeyUnsupported, None)
            }
        })
    }

    pub fn open(
        &self,
        target: SessionTarget,
        cols: u32,
        rows: u32,
        sink: Arc<dyn SessionSink>,
    ) -> SessionId {
        let id = Uuid::new_v4();
        let (input, input_rx) = mpsc::unbounded_channel();
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
            task: tokio::spawn(run(self.ctx.clone(), target, shared)),
        };
        self.sessions
            .lock()
            .expect("sessions lock")
            .insert(id, live);
        id
    }

    /// Queues input for the server. Synchronous, so calls keep their order.
    /// Input typed while disconnected is sent after the reconnect.
    pub fn write(&self, id: SessionId, data: &[u8]) -> Result<(), String> {
        let sessions = self.sessions.lock().expect("sessions lock");
        let live = sessions.get(&id).ok_or("unknown session")?;
        live.input
            .send(data.to_vec())
            .map_err(|_| "session has ended".to_owned())
    }

    pub async fn resize(&self, id: SessionId, cols: u32, rows: u32) -> Result<(), String> {
        let writer = {
            let sessions = self.sessions.lock().expect("sessions lock");
            let live = sessions.get(&id).ok_or("unknown session")?;
            *live.size.lock().expect("size lock") = (cols, rows);
            live.writer.lock().expect("writer lock").clone()
        };
        match writer {
            Some(w) => w.resize(cols, rows).await.map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    /// The UI has rendered `bytes` more output bytes.
    pub fn ack(&self, id: SessionId, bytes: usize) {
        if let Some(live) = self.sessions.lock().expect("sessions lock").get(&id) {
            live.flow.ack(bytes);
        }
    }

    pub fn close(&self, id: SessionId) {
        if let Some(live) = self.sessions.lock().expect("sessions lock").remove(&id) {
            if let Some(writer) = live.writer.lock().expect("writer lock").clone() {
                writer.close();
            }
            live.task.abort();
        }
    }
}

/// Marks the host as connected to just now, for "recent connections". A failure here must
/// not end the session, so it is ignored.
fn remember_connection(ctx: &Context, host_id: HostId) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    let _ = ctx
        .store
        .lock()
        .expect("store lock")
        .touch_recent(host_id, now_ms);
}

fn end(sink: &dyn SessionSink, reason: EndReason, code: Option<ErrorCode>, detail: Option<String>) {
    sink.status(SessionStatus::Ended {
        reason,
        code,
        detail,
    });
}

/// Forwards queued input to one connection, in order. Aborted when the connection ends,
/// which also frees a write that is stuck on a dead connection.
async fn forward_input(
    input: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
    writer: Writer,
) {
    let mut input = input.lock().await;
    while let Some(data) = input.recv().await {
        if writer.write(&data).await.is_err() {
            return;
        }
    }
}

/// The whole life of one tab: connect, pump output, reconnect after a drop, end.
async fn run(ctx: Arc<Context>, target: SessionTarget, shared: Shared) {
    let sink = shared.sink.clone();
    let mut attempt = 0u32;
    let mut ever_connected = false;
    let opened = OpenedKeys::default();
    loop {
        sink.status(SessionStatus::Connecting);
        let reason = match connect_target(&ctx, &target, &shared.size, &opened).await {
            Ok((writer, mut events, notice)) => {
                *shared.writer.lock().expect("writer lock") = Some(writer.clone());
                let forwarder = tokio::spawn(forward_input(shared.input.clone(), writer));
                shared.flow.reset();
                sink.status(SessionStatus::Connected { notice });
                if let SessionTarget::Host { host_id } = &target {
                    remember_connection(&ctx, *host_id);
                }
                ever_connected = true;
                attempt = 0;
                let mut reason = Disconnect::Lost("connection closed".into());
                loop {
                    shared.flow.wait_below_watermark().await;
                    match events.recv().await {
                        Some(ShellEvent::Data(bytes)) => {
                            shared.flow.add(bytes.len());
                            sink.data(bytes);
                        }
                        Some(ShellEvent::Ended(r)) => {
                            reason = r;
                            break;
                        }
                        None => break,
                    }
                }
                forwarder.abort();
                *shared.writer.lock().expect("writer lock") = None;
                reason
            }
            Err(Failure::End(reason, code, detail)) => {
                return end(sink.as_ref(), reason, code, detail);
            }
            Err(Failure::Lost(code, detail)) if !ever_connected => {
                return end(
                    sink.as_ref(),
                    EndReason::ConnectFailed,
                    Some(code),
                    Some(detail),
                );
            }
            Err(Failure::Lost(_, detail)) => Disconnect::Lost(detail),
        };

        attempt += 1;
        // A local shell that ended is not started again.
        let retry = match &target {
            #[cfg(desktop)]
            SessionTarget::Local { .. } => None,
            _ => next_reconnect_delay(&reason, attempt),
        };
        match retry {
            Some(delay) => {
                sink.status(SessionStatus::Reconnecting {
                    attempt,
                    delay_secs: delay.as_secs(),
                });
                tokio::time::sleep(delay.mul_f32(ctx.delay_scale)).await;
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

enum Failure {
    /// Do not retry.
    End(EndReason, Option<ErrorCode>, Option<String>),
    /// Network trouble, may retry. The code says what failed (the network, a proxy, a jump
    /// host).
    Lost(ErrorCode, String),
}

fn end_error(code: ErrorCode, detail: Option<String>) -> Failure {
    Failure::End(EndReason::Error, Some(code), detail)
}

/// Asks for Windows Hello once per run before the first secret from the vault (a key or a
/// password) is used.
async fn unlock_secrets(ctx: &Context) -> Result<&Arc<Vault>, Failure> {
    let vault = ctx
        .vault
        .as_ref()
        .ok_or_else(|| end_error(ErrorCode::VaultLocked, None))?;
    let reason = {
        let store = ctx.store.lock().expect("store lock");
        let language = crate::settings::load(&store).language;
        hello_reason(language, sys_locale::get_locale().as_deref())
    };
    ctx.gate
        .ensure(reason)
        .await
        .map_err(|code| end_error(code, None))?;
    Ok(vault)
}

fn vault_failure(e: sshcraft_core::vault::VaultError) -> Failure {
    let e = crate::errors::CommandError::from(e);
    end_error(e.code, e.detail)
}

/// A stored key ready to sign in, with its certificate. Windows Hello guards only keys whose
/// secret lies open in the vault: a key with a passphrase asks for it instead, and a security
/// key's authenticator checks the user itself.
/// Keys opened with their passphrase during one tab: a reconnect or a second hop with the same
/// key does not ask again. Lives as long as the tab's task, then drops (and wipes) the keys.
#[derive(Default)]
struct OpenedKeys(Mutex<HashMap<KeyId, PrivateKey>>);

impl OpenedKeys {
    fn get(&self, id: KeyId) -> Option<PrivateKey> {
        self.0.lock().expect("opened keys lock").get(&id).cloned()
    }

    fn keep(&self, id: KeyId, key: &PrivateKey) {
        self.0
            .lock()
            .expect("opened keys lock")
            .insert(id, key.clone());
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
        .expect("store lock")
        .key_record(id)
        .map_err(|e| end_error(ErrorCode::Store, Some(e.to_string())))?
        .ok_or_else(|| end_error(ErrorCode::KeyNotOnDevice, None))?;
    let certificate = record
        .certificate
        .as_deref()
        .and_then(|text| Certificate::from_openssh(text).ok());
    let vault = ctx
        .vault
        .as_ref()
        .ok_or_else(|| end_error(ErrorCode::VaultLocked, None))?;
    let stored = {
        let store = ctx.store.lock().expect("store lock");
        vault.stored_openssh(&store, id).map_err(vault_failure)?
    };
    let key = PrivateKey::from_openssh(stored.as_str())
        .map_err(|e| end_error(ErrorCode::Vault, Some(e.to_string())))?;
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
        let auth = ctx
            .security
            .clone()
            .ok_or_else(|| end_error(ErrorCode::NotSupported, None))?;
        let attachment = match record.kind {
            KeyKind::Device => Attachment::Platform,
            KeyKind::Vault => Attachment::CrossPlatform,
        };
        let signer = SkSigner::new(auth, &key, attachment)
            .map_err(|e| end_error(ErrorCode::SecurityKeyFailed, Some(e.to_string())))?;
        let credential = Credential::Signer {
            public_key: key.public_key().clone(),
            signer: Box::new(signer),
        };
        return Ok((credential, certificate));
    }
    if asked {
        return Ok((Credential::Key(key), certificate));
    }
    unlock_secrets(ctx).await?;
    Ok((Credential::Key(key), certificate))
}

/// Asks for the passphrase of `key` (named `name`) at most [`MAX_PASSPHRASE_TRIES`] times.
async fn ask_passphrase(
    ctx: &Context,
    name: &str,
    key: &PrivateKey,
    resolved: &ResolvedHost,
) -> Result<PrivateKey, Failure> {
    for _ in 0..MAX_PASSPHRASE_TRIES {
        let answers = ctx
            .auth
            .answer(AuthRequest {
                host_name: resolved.name.clone(),
                username: resolved.username.clone(),
                name: String::new(),
                instructions: String::new(),
                prompts: vec![AuthPromptLine {
                    text: "Passphrase:".into(),
                    echo: false,
                }],
                passphrase_for: Some(name.to_owned()),
            })
            .await
            .ok_or(Failure::End(EndReason::AuthCancelled, None, None))?;
        let Some(passphrase) = answers.into_iter().next() else {
            return Err(Failure::End(EndReason::AuthCancelled, None, None));
        };
        if let Ok(key) = key.decrypt(passphrase.as_str()) {
            return Ok(key);
        }
    }
    Err(Failure::End(
        EndReason::AuthFailed,
        Some(ErrorCode::WrongPassphrase),
        None,
    ))
}

/// The public half of a key to put into `authorized_keys`, and whether the server should
/// demand the key's PIN (`verify-required`).
async fn public_half(
    ctx: &Context,
    key: &KeyRef,
) -> Result<(PublicKey, bool), (ErrorCode, Option<String>)> {
    match key {
        KeyRef::Stored { id } => {
            let record = ctx
                .store
                .lock()
                .expect("store lock")
                .key_record(*id)
                .map_err(|e| (ErrorCode::Store, Some(e.to_string())))?
                .ok_or((ErrorCode::KeyNotOnDevice, None))?;
            let public = PublicKey::from_openssh(&record.public_key)
                .map_err(|e| (ErrorCode::InvalidKey, Some(e.to_string())))?;
            let mut verify_required = false;
            if record.algorithm.starts_with("sk-")
                && let Some(vault) = &ctx.vault
            {
                let stored = vault
                    .stored_openssh(&ctx.store.lock().expect("store lock"), *id)
                    .map_err(|e| (ErrorCode::Vault, Some(e.to_string())))?;
                let flags = match PrivateKey::from_openssh(stored.as_str())
                    .ok()
                    .map(|k| k.key_data().clone())
                {
                    Some(russh::keys::ssh_key::private::KeypairData::SkEcdsaSha2NistP256(sk)) => {
                        sk.flags()
                    }
                    Some(russh::keys::ssh_key::private::KeypairData::SkEd25519(sk)) => sk.flags(),
                    _ => 0,
                };
                verify_required = flags & sshcraft_fido::FLAG_USER_VERIFICATION != 0;
            }
            Ok((public, verify_required))
        }
        KeyRef::Agent { fingerprint } => {
            let mut agent = ctx
                .agent
                .connect()
                .await
                .map_err(|e| (ErrorCode::AgentUnavailable, Some(e.to_string())))?;
            let keys = sshcraft_ssh::list_agent_keys(&mut agent)
                .await
                .map_err(|e| (ErrorCode::AgentUnavailable, Some(e.to_string())))?;
            let found = keys
                .into_iter()
                .find(|k| &k.fingerprint == fingerprint)
                .ok_or((ErrorCode::AgentKeyMissing, None))?;
            let public = PublicKey::from_openssh(&found.public_key)
                .map_err(|e| (ErrorCode::InvalidKey, Some(e.to_string())))?;
            Ok((public, false))
        }
    }
}

/// A failure as a command error.
fn failure_code(f: Failure) -> (ErrorCode, Option<String>) {
    match f {
        Failure::End(reason, code, detail) => (
            code.unwrap_or(match reason {
                EndReason::AuthCancelled => ErrorCode::SignInCancelled,
                _ => ErrorCode::Session,
            }),
            detail,
        ),
        Failure::Lost(code, detail) => (code, Some(detail)),
    }
}

/// A stored password, or `None` when it is not on this device (then the server's question goes
/// to the user).
async fn stored_password(
    ctx: &Context,
    id: PasswordId,
) -> Result<Option<Zeroizing<String>>, Failure> {
    let here = ctx
        .store
        .lock()
        .expect("store lock")
        .password_record(id)
        .map_err(|e| end_error(ErrorCode::Store, Some(e.to_string())))?
        .is_some();
    if !here {
        return Ok(None);
    }
    let vault = unlock_secrets(ctx).await?;
    let store = ctx.store.lock().expect("store lock");
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
        Some(KeyRef::Agent { fingerprint }) => Some(Credential::Agent {
            fingerprint: fingerprint.clone(),
            agent: ctx
                .agent
                .connect()
                .await
                .map_err(|e| end_error(ErrorCode::AgentUnavailable, Some(e.to_string())))?,
        }),
    };
    let password = match resolved.password {
        Some(id) => stored_password(ctx, id).await?,
        None => None,
    };
    Ok(Hop {
        host_name: resolved.name.clone(),
        address: resolved.address.clone(),
        port: resolved.port,
        username: resolved.username.clone(),
        auth: Auth {
            key,
            password,
            certificate,
        },
    })
}

async fn proxy_target(ctx: &Context, endpoint: &ProxyEndpoint) -> Result<ProxyTarget, Failure> {
    let password = match endpoint.password {
        Some(id) => stored_password(ctx, id).await?,
        None => None,
    };
    Ok(ProxyTarget {
        address: endpoint.address.clone(),
        port: endpoint.port,
        username: endpoint.username.clone(),
        password,
    })
}

/// What a failed connection means for the tab.
fn failure(e: SshError, verifier: &StoreVerifier) -> Failure {
    match e {
        SshError::HostKeyRejected => {
            match verifier.mismatch.lock().expect("mismatch lock").take() {
                Some(detail) => Failure::End(EndReason::HostKeyMismatch, None, Some(detail)),
                None => Failure::End(EndReason::HostKeyRejected, None, None),
            }
        }
        SshError::AuthFailed => Failure::End(EndReason::AuthFailed, None, None),
        SshError::AuthCancelled => Failure::End(EndReason::AuthCancelled, None, None),
        SshError::PasswordRejected => Failure::End(
            EndReason::AuthFailed,
            Some(ErrorCode::PasswordRejected),
            None,
        ),
        SshError::ProxyAuthFailed => Failure::End(
            EndReason::AuthFailed,
            Some(ErrorCode::ProxyAuthFailed),
            None,
        ),
        SshError::AgentUnavailable(detail) => end_error(ErrorCode::AgentUnavailable, Some(detail)),
        SshError::AgentKeyMissing => end_error(ErrorCode::AgentKeyMissing, None),
        SshError::AgentRefused => {
            Failure::End(EndReason::AuthFailed, Some(ErrorCode::AgentRefused), None)
        }
        SshError::SignerCancelled => Failure::End(
            EndReason::AuthCancelled,
            Some(ErrorCode::SecurityKeyCancelled),
            None,
        ),
        SshError::SignerFailed(detail) => Failure::End(
            EndReason::AuthFailed,
            Some(ErrorCode::SecurityKeyFailed),
            Some(detail),
        ),
        SshError::Proxy(detail) => Failure::Lost(ErrorCode::ProxyFailed, detail),
        SshError::Hop {
            host_name, error, ..
        } => match failure(*error, verifier) {
            kept @ Failure::End(EndReason::HostKeyRejected | EndReason::HostKeyMismatch, ..) => {
                kept
            }
            // What went wrong stays; the detail names the jump host.
            Failure::End(reason, code, _) => Failure::End(
                reason,
                Some(code.unwrap_or(ErrorCode::JumpFailed)),
                Some(host_name),
            ),
            Failure::Lost(code, detail) => Failure::Lost(code, format!("{host_name}: {detail}")),
        },
        other => Failure::Lost(ErrorCode::Network, other.to_string()),
    }
}

type Connected = (
    Writer,
    tokio::sync::mpsc::Receiver<ShellEvent>,
    Option<ErrorCode>,
);

async fn connect_target(
    ctx: &Context,
    target: &SessionTarget,
    size: &Mutex<(u32, u32)>,
    opened: &OpenedKeys,
) -> Result<Connected, Failure> {
    match target {
        SessionTarget::Host { host_id } => connect_once(ctx, *host_id, size, opened).await,
        #[cfg(desktop)]
        SessionTarget::Local { shell } => {
            let (cols, rows) = *size.lock().expect("size lock");
            let (writer, events) = sshcraft_local::pty::spawn(shell, to_u16(cols), to_u16(rows))
                .map_err(|e| end_error(ErrorCode::LocalShellFailed, Some(e.to_string())))?;
            Ok((Arc::new(writer), events, None))
        }
        #[cfg(desktop)]
        SessionTarget::Serial { settings } => {
            settings
                .validate()
                .map_err(|e| end_error(ErrorCode::SerialFailed, Some(e.to_string())))?;
            // A port that is gone may come back (an adapter plugged in again): retried like a
            // lost connection once it was open.
            let (writer, events) = sshcraft_local::serial::open(settings)
                .map_err(|e| Failure::Lost(ErrorCode::SerialFailed, e.to_string()))?;
            Ok((Arc::new(writer), events, None))
        }
    }
}

async fn connect_once(
    ctx: &Context,
    host_id: HostId,
    size: &Mutex<(u32, u32)>,
    opened: &OpenedKeys,
) -> Result<Connected, Failure> {
    let (session, notice) = connect_session(ctx, host_id, opened).await?;
    let (cols, rows) = *size.lock().expect("size lock");
    let shell = session
        .open_shell(cols, rows)
        .await
        .map_err(|e| Failure::Lost(ErrorCode::Network, e.to_string()))?;
    if needs_os(&ctx.store, host_id) {
        tokio::spawn(detect_os(ctx.store.clone(), host_id, session));
    }
    let (writer, events) = shell.split();
    Ok((Arc::new(writer), events, notice))
}

/// Connects to a host along its route and signs in on every hop; no shell yet. The notice is
/// something to show while connected.
async fn connect_session(
    ctx: &Context,
    host_id: HostId,
    opened: &OpenedKeys,
) -> Result<(sshcraft_ssh::Session, Option<ErrorCode>), Failure> {
    let route = {
        let store = ctx.store.lock().expect("store lock");
        lookup(&store, host_id).map_err(|(code, detail)| end_error(code, detail))?
    };
    let mut hops = Vec::new();
    for resolved in &route.jumps {
        // A jump host that cannot be used is named.
        hops.push(hop(ctx, resolved, opened).await.map_err(|f| match f {
            Failure::End(reason, code, None) => {
                Failure::End(reason, code, Some(resolved.name.clone()))
            }
            other => other,
        })?);
    }
    hops.push(hop(ctx, &route.target, opened).await?);
    let proxy = match &route.target.proxy {
        Some(Proxy::Http(endpoint)) => Some(ProxyConfig::Http(proxy_target(ctx, endpoint).await?)),
        Some(Proxy::Socks5(endpoint)) => {
            Some(ProxyConfig::Socks5(proxy_target(ctx, endpoint).await?))
        }
        _ => None,
    };
    let opener = if route.target.agent_forwarding {
        ctx.agent.opener()
    } else {
        None
    };
    let notice = (route.target.agent_forwarding && opener.is_none())
        .then_some(ErrorCode::AgentForwardingUnavailable);
    let options = SessionOptions {
        agent_forwarding: opener,
        env: route
            .target
            .env
            .iter()
            .map(|v| (v.name.clone(), v.value.clone()))
            .collect(),
    };
    let verifier = Arc::new(StoreVerifier {
        store: ctx.store.clone(),
        prompter: ctx.prompter.clone(),
        mismatch: Mutex::new(None),
        first: Mutex::new(Vec::new()),
    });
    let params = RouteParams {
        hops,
        proxy,
        options,
        connect_timeout: sshcraft_ssh::CONNECT_TIMEOUT,
    };
    let session = sshcraft_ssh::connect_route(params, verifier.clone(), ctx.auth.clone())
        .await
        .map_err(|e| failure(e, &verifier))?;
    let notice = notice.or(session
        .certificate_expired()
        .then_some(ErrorCode::CertificateExpired));
    Ok((session, notice))
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
    let store = store.lock().expect("store lock");
    let asked = store
        .get_setting(&probed_setting(host_id))
        .is_ok_and(|v| v.is_some());
    !asked
        && store
            .list_hosts()
            .is_ok_and(|hosts| hosts.iter().any(|h| h.id == host_id && h.os.is_none()))
}

/// Asks the server once which operating system it runs and stores it on the host (sent like
/// any change, unless the host came in a share that may only be read). Failures are ignored:
/// the symbol simply stays neutral.
async fn detect_os(store: Arc<Mutex<Store>>, host_id: HostId, session: sshcraft_ssh::Session) {
    // Asked once: a server whose system is not recognized (or that runs a forced command for
    // every session) is not asked again on each connection.
    mark_probed(&store.lock().expect("store lock"), host_id);
    let Ok(output) = session
        .exec(sshcraft_core::os_detect::OS_PROBE, 16 * 1024)
        .await
    else {
        return;
    };
    let Some(os) = sshcraft_core::os_detect::from_output(&String::from_utf8_lossy(&output)) else {
        return;
    };
    let store = store.lock().expect("store lock");
    let Ok(hosts) = store.list_hosts() else {
        return;
    };
    let Some(mut host) = hosts
        .into_iter()
        .find(|h| h.id == host_id && h.os.is_none())
    else {
        return;
    };
    host.os = Some(os);
    let id = host.id.0.to_string();
    let read_only = host.location.is_some_and(|server| {
        store
            .read_only(server, sshcraft_core::store::ItemKind::Host, &id)
            .unwrap_or(true)
    });
    if store.upsert_host(&host).is_err() || read_only {
        return;
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    // Just after the version this device knows: a change made meanwhile on another device
    // (a rename) is newer and wins over this background update.
    let at = host
        .location
        .and_then(|server| {
            store
                .record_entry(server, sshcraft_core::store::ItemKind::Host, &id)
                .ok()
                .flatten()
        })
        .map_or(now_ms, |entry| entry.modified_at.saturating_add(1));
    let _ = sshcraft_sync::engine::record_local_change(
        &store,
        sshcraft_core::store::ItemKind::Host,
        &id,
        at,
    );
}

fn lookup(store: &Store, host_id: HostId) -> Result<Route, (ErrorCode, Option<String>)> {
    let store_error = |e: sshcraft_core::store::StoreError| (ErrorCode::Store, Some(e.to_string()));
    let hosts = store.list_hosts().map_err(store_error)?;
    let groups = store.list_groups().map_err(store_error)?;
    let identities = store.list_identities().map_err(store_error)?;
    let dir = Directory {
        groups: &groups,
        hosts: &hosts,
        identities: &identities,
    };
    resolve_route(&dir, host_id).map_err(|e| {
        // A jump host that cannot be used is named.
        let detail = match &e {
            ResolveError::Jump { host_id, .. } => hosts
                .iter()
                .find(|h| h.id == *host_id)
                .map(|h| h.name.clone()),
            _ => None,
        };
        (ErrorCode::from(&e), detail)
    })
}

struct StoreVerifier {
    store: Arc<Mutex<Store>>,
    prompter: Arc<dyn HostKeyPrompter>,
    mismatch: Mutex<Option<String>>,
    /// Servers whose host key the user trusted during this connection.
    first: Mutex<Vec<(String, u16)>>,
}

#[async_trait::async_trait]
impl HostKeyVerifier for StoreVerifier {
    async fn verify(&self, host_name: &str, address: &str, port: u16, fingerprint: &str) -> bool {
        let known = {
            let store = self.store.lock().expect("store lock");
            match store.known_host(address, port) {
                Ok(k) => k,
                Err(_) => return false,
            }
        };
        match check_host_key(known.as_deref(), fingerprint) {
            HostKeyStatus::Trusted => true,
            HostKeyStatus::Mismatch { expected } => {
                *self.mismatch.lock().expect("mismatch lock") =
                    Some(format!("expected {expected}, got {fingerprint}"));
                false
            }
            HostKeyStatus::Unknown => {
                let accepted = self
                    .prompter
                    .ask(host_name, address, port, fingerprint)
                    .await;
                if accepted {
                    let store = self.store.lock().expect("store lock");
                    if store.set_known_host(address, port, fingerprint).is_err() {
                        return false;
                    }
                    self.first
                        .lock()
                        .expect("first lock")
                        .push((address.to_owned(), port));
                }
                accepted
            }
        }
    }

    fn first_contact(&self, address: &str, port: u16) -> bool {
        self.first
            .lock()
            .expect("first lock")
            .iter()
            .any(|(a, p)| a == address && *p == port)
    }
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;

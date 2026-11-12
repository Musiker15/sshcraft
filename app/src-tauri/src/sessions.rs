//! Runs SSH sessions for the UI: resolves the host, loads the key, checks the host key
//! against the store (asking the user for unknown ones), and reconnects lost sessions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use stollen_core::model::{HostId, KeyRef, ResolvedHost, resolve};
use stollen_core::policy::{Disconnect, HostKeyStatus, check_host_key, next_reconnect_delay};
use stollen_core::store::Store;
use stollen_core::vault::Vault;
use stollen_ssh::{ConnectParams, HostKeyVerifier, ShellEvent, ShellWriter, SshError};
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;
use uuid::Uuid;

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
    /// Could not even start: host incomplete, key missing, vault locked.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionStatus {
    Connecting,
    Connected,
    Reconnecting {
        attempt: u32,
        delay_secs: u64,
    },
    Ended {
        reason: EndReason,
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

/// The UI may hold this many unacknowledged output bytes before Stollen stops reading from
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

struct Live {
    /// Keystrokes in call order. A per-connection task forwards them to the server.
    input: mpsc::UnboundedSender<Vec<u8>>,
    /// Cloned out before awaiting, so no lock is held while talking to the server.
    writer: Arc<Mutex<Option<ShellWriter>>>,
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
    agent: Arc<dyn AgentConnector>,
    gate: KeyGate,
    delay_scale: f32,
}

/// Asks Windows Hello once per app run before a key from the vault is used.
pub struct KeyGate {
    presence: Arc<dyn UserPresence>,
    verified: tokio::sync::Mutex<bool>,
    hello_missing: std::sync::atomic::AtomicBool,
}

const HELLO_REASON: &str = "Stollen möchte deine gespeicherten SSH-Schlüssel verwenden.";

impl KeyGate {
    fn new(presence: Arc<dyn UserPresence>) -> Self {
        Self {
            presence,
            verified: tokio::sync::Mutex::new(false),
            hello_missing: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Concurrent callers wait on the same lock, so one prompt serves them all.
    async fn ensure(&self) -> Result<(), String> {
        let mut verified = self.verified.lock().await;
        if *verified {
            return Ok(());
        }
        match self.presence.verify(HELLO_REASON).await {
            Presence::Verified => {
                *verified = true;
                Ok(())
            }
            Presence::Unavailable => {
                self.hello_missing
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                *verified = true;
                Ok(())
            }
            Presence::Declined => Err("Windows Hello wurde abgebrochen.".into()),
        }
    }
}

/// Opens a connection to the SSH agent (Proton Pass on Windows, an in-memory agent in tests).
#[async_trait::async_trait]
pub trait AgentConnector: Send + Sync + 'static {
    async fn connect(&self) -> Result<stollen_ssh::DynAgent, SshError>;
}

/// The Windows agent pipe served by the Proton Pass desktop app.
pub struct WindowsAgent;

#[async_trait::async_trait]
impl AgentConnector for WindowsAgent {
    async fn connect(&self) -> Result<stollen_ssh::DynAgent, SshError> {
        #[cfg(windows)]
        {
            stollen_ssh::agent::open_windows_agent().await
        }
        #[cfg(not(windows))]
        {
            Err(SshError::AgentUnavailable(
                "no agent support on this platform yet".into(),
            ))
        }
    }
}

const AGENT_UNAVAILABLE: &str = "Proton Pass ist nicht erreichbar. Läuft die Proton-Pass-App?";
const AGENT_KEY_MISSING: &str = "Der Schlüssel ist nicht mehr in Proton Pass.";
const AGENT_REFUSED: &str = "Proton Pass hat die Signatur abgelehnt.";

/// What `run` shares with the manager for one session.
struct Shared {
    sink: Arc<dyn SessionSink>,
    input: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
    writer: Arc<Mutex<Option<ShellWriter>>>,
    size: Arc<Mutex<(u32, u32)>>,
    flow: Arc<Flow>,
}

impl SessionManager {
    pub fn new(
        store: Arc<Mutex<Store>>,
        vault: Option<Arc<Vault>>,
        prompter: Arc<dyn HostKeyPrompter>,
        agent: Arc<dyn AgentConnector>,
        presence: Arc<dyn UserPresence>,
    ) -> Self {
        Self::with_delay_scale(store, vault, prompter, agent, presence, 1.0)
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
        agent: Arc<dyn AgentConnector>,
        presence: Arc<dyn UserPresence>,
        delay_scale: f32,
    ) -> Self {
        Self {
            ctx: Arc::new(Context {
                store,
                vault,
                prompter,
                agent,
                gate: KeyGate::new(presence),
                delay_scale,
            }),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn open(
        &self,
        host_id: HostId,
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
            task: tokio::spawn(run(self.ctx.clone(), host_id, shared)),
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
            live.task.abort();
        }
    }
}

fn end(sink: &dyn SessionSink, reason: EndReason, detail: Option<String>) {
    sink.status(SessionStatus::Ended { reason, detail });
}

/// Forwards queued input to one connection, in order. Aborted when the connection ends,
/// which also frees a write that is stuck on a dead connection.
async fn forward_input(
    input: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
    writer: ShellWriter,
) {
    let mut input = input.lock().await;
    while let Some(data) = input.recv().await {
        if writer.write(&data).await.is_err() {
            return;
        }
    }
}

/// The whole life of one tab: connect, pump output, reconnect after a drop, end.
async fn run(ctx: Arc<Context>, host_id: HostId, shared: Shared) {
    let sink = shared.sink.clone();
    let mut attempt = 0u32;
    let mut ever_connected = false;
    loop {
        sink.status(SessionStatus::Connecting);
        let reason = match connect_once(&ctx, host_id, &shared.size).await {
            Ok((shell_writer, mut events)) => {
                *shared.writer.lock().expect("writer lock") = Some(shell_writer.clone());
                let forwarder = tokio::spawn(forward_input(shared.input.clone(), shell_writer));
                shared.flow.reset();
                sink.status(SessionStatus::Connected);
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
            Err(Failure::End(reason, detail)) => return end(sink.as_ref(), reason, detail),
            Err(Failure::Lost(detail)) if !ever_connected => {
                return end(sink.as_ref(), EndReason::Lost, Some(detail));
            }
            Err(Failure::Lost(detail)) => Disconnect::Lost(detail),
        };

        attempt += 1;
        match next_reconnect_delay(&reason, attempt) {
            Some(delay) => {
                sink.status(SessionStatus::Reconnecting {
                    attempt,
                    delay_secs: delay.as_secs(),
                });
                tokio::time::sleep(delay.mul_f32(ctx.delay_scale)).await;
            }
            None => {
                let (reason, detail) = match reason {
                    Disconnect::Exited(_) => (EndReason::Exited, None),
                    Disconnect::Lost(d) => (EndReason::Lost, Some(d)),
                    Disconnect::HostKeyRejected => (EndReason::HostKeyRejected, None),
                    Disconnect::AuthFailed => (EndReason::AuthFailed, None),
                };
                return end(sink.as_ref(), reason, detail);
            }
        }
    }
}

enum Failure {
    /// Do not retry.
    End(EndReason, Option<String>),
    /// Network trouble, may retry.
    Lost(String),
}

async fn connect_once(
    ctx: &Context,
    host_id: HostId,
    size: &Mutex<(u32, u32)>,
) -> Result<(ShellWriter, tokio::sync::mpsc::Receiver<ShellEvent>), Failure> {
    let error = |msg: String| Failure::End(EndReason::Error, Some(msg));
    let resolved = {
        let store = ctx.store.lock().expect("store lock");
        lookup(&store, host_id).map_err(error)?
    };
    let credential = match &resolved.key {
        KeyRef::Stored { id } => {
            let vault = ctx
                .vault
                .as_ref()
                .ok_or_else(|| error("the key vault is locked".into()))?;
            ctx.gate.ensure().await.map_err(error)?;
            let store = ctx.store.lock().expect("store lock");
            let key = vault
                .private_key(&store, *id)
                .map_err(|e| error(e.to_string()))?;
            stollen_ssh::Credential::Key(key)
        }
        KeyRef::Agent { fingerprint } => stollen_ssh::Credential::Agent {
            fingerprint: fingerprint.clone(),
            agent: ctx
                .agent
                .connect()
                .await
                .map_err(|_| error(AGENT_UNAVAILABLE.into()))?,
        },
    };

    let verifier = Arc::new(StoreVerifier {
        store: ctx.store.clone(),
        prompter: ctx.prompter.clone(),
        host_name: resolved.name.clone(),
        mismatch: Mutex::new(None),
    });
    let params = ConnectParams {
        address: resolved.address.clone(),
        port: resolved.port,
        username: resolved.username.clone(),
        credential,
    };
    let session = match stollen_ssh::connect(params, verifier.clone()).await {
        Ok(s) => s,
        Err(SshError::HostKeyRejected) => {
            let mismatch = verifier.mismatch.lock().expect("mismatch lock").take();
            return Err(match mismatch {
                Some(detail) => Failure::End(EndReason::HostKeyMismatch, Some(detail)),
                None => Failure::End(EndReason::HostKeyRejected, None),
            });
        }
        Err(SshError::AuthFailed) => return Err(Failure::End(EndReason::AuthFailed, None)),
        Err(SshError::AgentUnavailable(_)) => return Err(error(AGENT_UNAVAILABLE.into())),
        Err(SshError::AgentKeyMissing) => return Err(error(AGENT_KEY_MISSING.into())),
        Err(SshError::AgentRefused) => {
            return Err(Failure::End(
                EndReason::AuthFailed,
                Some(AGENT_REFUSED.into()),
            ));
        }
        Err(e) => return Err(Failure::Lost(e.to_string())),
    };
    let (cols, rows) = *size.lock().expect("size lock");
    let shell = session
        .open_shell(cols, rows)
        .await
        .map_err(|e| Failure::Lost(e.to_string()))?;
    Ok(shell.split())
}

fn lookup(store: &Store, host_id: HostId) -> Result<ResolvedHost, String> {
    let host = store
        .list_hosts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|h| h.id == host_id)
        .ok_or("host not found")?;
    let group = match host.group_id {
        Some(gid) => store
            .list_groups()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|g| g.id == gid),
        None => None,
    };
    resolve(&host, group.as_ref()).map_err(|e| e.to_string())
}

struct StoreVerifier {
    store: Arc<Mutex<Store>>,
    prompter: Arc<dyn HostKeyPrompter>,
    host_name: String,
    mismatch: Mutex<Option<String>>,
}

#[async_trait::async_trait]
impl HostKeyVerifier for StoreVerifier {
    async fn verify(&self, address: &str, port: u16, fingerprint: &str) -> bool {
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
                    .ask(&self.host_name, address, port, fingerprint)
                    .await;
                if accepted {
                    let store = self.store.lock().expect("store lock");
                    if store.set_known_host(address, port, fingerprint).is_err() {
                        return false;
                    }
                }
                accepted
            }
        }
    }
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;

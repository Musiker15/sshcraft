//! Runs SSH sessions for the UI: resolves the host, loads the key, checks the host key
//! against the store (asking the user for unknown ones), and reconnects lost sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use stollen_core::model::{HostId, KeyRef, ResolvedHost, resolve};
use stollen_core::policy::{Disconnect, HostKeyStatus, check_host_key, next_reconnect_delay};
use stollen_core::store::Store;
use stollen_core::vault::Vault;
use stollen_ssh::{ConnectParams, HostKeyVerifier, ShellEvent, ShellWriter, SshError};
use tokio::task::JoinHandle;
use uuid::Uuid;

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

struct Live {
    writer: Arc<tokio::sync::Mutex<Option<ShellWriter>>>,
    size: Arc<Mutex<(u32, u32)>>,
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
    delay_scale: f32,
}

impl SessionManager {
    pub fn new(
        store: Arc<Mutex<Store>>,
        vault: Option<Arc<Vault>>,
        prompter: Arc<dyn HostKeyPrompter>,
    ) -> Self {
        Self::with_delay_scale(store, vault, prompter, 1.0)
    }

    /// `delay_scale` shortens reconnect delays in tests.
    pub fn with_delay_scale(
        store: Arc<Mutex<Store>>,
        vault: Option<Arc<Vault>>,
        prompter: Arc<dyn HostKeyPrompter>,
        delay_scale: f32,
    ) -> Self {
        Self {
            ctx: Arc::new(Context {
                store,
                vault,
                prompter,
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
        let writer = Arc::new(tokio::sync::Mutex::new(None));
        let size = Arc::new(Mutex::new((cols, rows)));
        let task = tokio::spawn(run(
            self.ctx.clone(),
            host_id,
            sink,
            writer.clone(),
            size.clone(),
        ));
        self.sessions
            .lock()
            .expect("sessions lock")
            .insert(id, Live { writer, size, task });
        id
    }

    pub async fn write(&self, id: SessionId, data: &[u8]) -> Result<(), String> {
        let writer = self.writer(id)?;
        let guard = writer.lock().await;
        match guard.as_ref() {
            Some(w) => w.write(data).await.map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    pub async fn resize(&self, id: SessionId, cols: u32, rows: u32) -> Result<(), String> {
        let (writer, size) = {
            let sessions = self.sessions.lock().expect("sessions lock");
            let live = sessions.get(&id).ok_or("unknown session")?;
            (live.writer.clone(), live.size.clone())
        };
        *size.lock().expect("size lock") = (cols, rows);
        let guard = writer.lock().await;
        match guard.as_ref() {
            Some(w) => w.resize(cols, rows).await.map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    pub fn close(&self, id: SessionId) {
        if let Some(live) = self.sessions.lock().expect("sessions lock").remove(&id) {
            live.task.abort();
        }
    }

    fn writer(
        &self,
        id: SessionId,
    ) -> Result<Arc<tokio::sync::Mutex<Option<ShellWriter>>>, String> {
        self.sessions
            .lock()
            .expect("sessions lock")
            .get(&id)
            .map(|l| l.writer.clone())
            .ok_or_else(|| "unknown session".to_owned())
    }
}

fn end(sink: &dyn SessionSink, reason: EndReason, detail: Option<String>) {
    sink.status(SessionStatus::Ended { reason, detail });
}

/// The whole life of one tab: connect, pump output, reconnect after a drop, end.
async fn run(
    ctx: Arc<Context>,
    host_id: HostId,
    sink: Arc<dyn SessionSink>,
    writer: Arc<tokio::sync::Mutex<Option<ShellWriter>>>,
    size: Arc<Mutex<(u32, u32)>>,
) {
    let mut attempt = 0u32;
    let mut ever_connected = false;
    loop {
        sink.status(SessionStatus::Connecting);
        let reason = match connect_once(&ctx, host_id, &size).await {
            Ok((shell_writer, mut events)) => {
                *writer.lock().await = Some(shell_writer);
                sink.status(SessionStatus::Connected);
                ever_connected = true;
                attempt = 0;
                let mut reason = Disconnect::Lost("connection closed".into());
                while let Some(event) = events.recv().await {
                    match event {
                        ShellEvent::Data(bytes) => sink.data(bytes),
                        ShellEvent::Ended(r) => {
                            reason = r;
                            break;
                        }
                    }
                }
                *writer.lock().await = None;
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
    let vault = ctx
        .vault
        .as_ref()
        .ok_or_else(|| error("the key vault is locked".into()))?;
    let (resolved, key) = {
        let store = ctx.store.lock().expect("store lock");
        let resolved = lookup(&store, host_id).map_err(error)?;
        let KeyRef::Stored { id } = resolved.key;
        let key = vault
            .private_key(&store, id)
            .map_err(|e| error(e.to_string()))?;
        (resolved, key)
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
        key,
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

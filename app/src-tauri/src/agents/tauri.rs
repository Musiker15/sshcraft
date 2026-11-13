//! The gate's backend in the app: the same commands the window uses, and questions shown in
//! SSHCraft's own window.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use sshcraft_agent::{
    AgentError, ErrorKind, ExecResult, KeyType, KeyView, MAX_OUTPUT, SessionOutput,
};
use sshcraft_core::agent::{AgentLogEntry, AgentPolicy, log_append};
use sshcraft_core::keytypes::{EcdsaCurveName, KeySpec};
use sshcraft_core::model::{Group, GroupId, Host, HostId, KeyId};
use sshcraft_core::vault::{self, KeyInfo};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::oneshot;
use uuid::Uuid;

use super::backend::{AgentBackend, AgentPrompt, Confirmer};
use crate::agent_sessions::AgentSessions;
use crate::errors::CommandError;
use crate::state::AppState;

fn failed(e: CommandError) -> AgentError {
    let mut message = format!("{:?}", e.code);
    if let Some(detail) = e.detail {
        message.push_str(": ");
        message.push_str(&detail);
    }
    AgentError::new(ErrorKind::Failed, message)
}

fn store_failed(e: impl std::fmt::Display) -> AgentError {
    AgentError::new(ErrorKind::Failed, e.to_string())
}

fn key_view(info: KeyInfo) -> KeyView {
    KeyView {
        id: info.id.0,
        name: info.name,
        kind: info.algorithm,
        fingerprint: info.fingerprint,
    }
}

fn session_id(session: &str) -> Option<Uuid> {
    Uuid::parse_str(session).ok()
}

pub struct TauriBackend {
    pub app: AppHandle,
}

impl TauriBackend {
    fn state(&self) -> State<'_, AppState> {
        self.app.state::<AppState>()
    }
}

#[async_trait::async_trait]
impl AgentBackend for TauriBackend {
    fn policy(&self) -> AgentPolicy {
        AgentPolicy::load(&self.state().store.lock().expect("store lock"))
    }

    fn hosts(&self) -> Result<Vec<Host>, AgentError> {
        self.state()
            .store
            .lock()
            .expect("store lock")
            .list_hosts()
            .map_err(store_failed)
    }

    fn groups(&self) -> Result<Vec<Group>, AgentError> {
        self.state()
            .store
            .lock()
            .expect("store lock")
            .list_groups()
            .map_err(store_failed)
    }

    fn keys(&self) -> Result<Vec<KeyView>, AgentError> {
        let keys = vault::list_keys(&self.state().store.lock().expect("store lock"))
            .map_err(store_failed)?;
        Ok(keys.into_iter().map(key_view).collect())
    }

    fn log(&self, entry: AgentLogEntry) {
        let state = self.state();
        if let Ok(vault) = &state.vault {
            let _ = log_append(vault, &state.store.lock().expect("store lock"), &entry);
            let _ = self.app.emit("agent-log", ());
        }
    }

    async fn exec(
        &self,
        host: HostId,
        command: &str,
        timeout: Duration,
    ) -> Result<ExecResult, AgentError> {
        let state = self.state();
        let (output, truncated) =
            AgentSessions::exec(&state.sessions, host, command, MAX_OUTPUT, timeout)
                .await
                .map_err(|(code, detail)| failed(CommandError { code, detail }))?;
        Ok(ExecResult {
            output: String::from_utf8_lossy(&output).into_owned(),
            truncated,
        })
    }

    fn session_open(&self, host: HostId) -> Result<String, AgentError> {
        let state = self.state();
        Ok(state.agent_sessions.open(&state.sessions, host).to_string())
    }

    fn session_owned(&self, session: &str) -> bool {
        session_id(session).is_some_and(|id| self.state().agent_sessions.owns(id))
    }

    fn session_write(&self, session: &str, data: &[u8]) -> Result<(), AgentError> {
        let state = self.state();
        let id = session_id(session)
            .ok_or_else(|| AgentError::new(ErrorKind::NotFound, "no such session"))?;
        state
            .agent_sessions
            .write(&state.sessions, id, data)
            .map_err(|e| AgentError::new(ErrorKind::Failed, e))
    }

    async fn session_read(
        &self,
        session: &str,
        wait: Duration,
    ) -> Result<SessionOutput, AgentError> {
        let state = self.state();
        let id = session_id(session)
            .ok_or_else(|| AgentError::new(ErrorKind::NotFound, "no such session"))?;
        let read = state
            .agent_sessions
            .read(&state.sessions, id, MAX_OUTPUT, wait)
            .await
            .map_err(|e| AgentError::new(ErrorKind::NotFound, e))?;
        Ok(SessionOutput {
            output: String::from_utf8_lossy(&read.output).into_owned(),
            ended: read.ended,
        })
    }

    fn session_close(&self, session: &str) -> Result<(), AgentError> {
        let state = self.state();
        let id = session_id(session)
            .ok_or_else(|| AgentError::new(ErrorKind::NotFound, "no such session"))?;
        if state.agent_sessions.close(&state.sessions, id) {
            Ok(())
        } else {
            Err(AgentError::new(ErrorKind::NotFound, "no such session"))
        }
    }

    fn save_host(&self, host: Host) -> Result<(), AgentError> {
        crate::commands::data::save_host(self.state(), host).map_err(failed)?;
        let _ = self.app.emit("data-changed", ());
        Ok(())
    }

    fn save_group(&self, group: Group) -> Result<(), AgentError> {
        crate::commands::data::save_group(self.state(), group).map_err(failed)?;
        let _ = self.app.emit("data-changed", ());
        Ok(())
    }

    fn delete_host(&self, host: HostId) -> Result<(), AgentError> {
        crate::commands::data::delete_host(self.state(), host).map_err(failed)?;
        let _ = self.app.emit("data-changed", ());
        Ok(())
    }

    fn delete_group(&self, group: GroupId) -> Result<(), AgentError> {
        crate::commands::data::delete_group(self.state(), group).map_err(failed)?;
        let _ = self.app.emit("data-changed", ());
        Ok(())
    }

    fn delete_key(&self, key: KeyId) -> Result<(), AgentError> {
        crate::commands::keys::delete_key(self.state(), key).map_err(failed)?;
        let _ = self.app.emit("data-changed", ());
        Ok(())
    }

    async fn generate_key(&self, name: &str, key_type: KeyType) -> Result<KeyView, AgentError> {
        let spec = match key_type {
            KeyType::Ed25519 => KeySpec::Ed25519,
            KeyType::Ecdsa => KeySpec::Ecdsa {
                curve: EcdsaCurveName::Nistp256,
            },
            KeyType::Rsa => KeySpec::Rsa { bits: 4096 },
        };
        let info = crate::commands::keys::generate_key(self.state(), name.to_owned(), spec, None)
            .await
            .map_err(failed)?;
        let _ = self.app.emit("data-changed", ());
        Ok(key_view(info))
    }

    async fn import_key(
        &self,
        name: &str,
        text: &str,
        passphrase: Option<&str>,
    ) -> Result<KeyView, AgentError> {
        let info = crate::commands::keys::import_key(
            self.state(),
            name.to_owned(),
            text.to_owned(),
            passphrase.map(str::to_owned),
            Some(true),
        )
        .await
        .map_err(failed)?;
        let _ = self.app.emit("data-changed", ());
        Ok(key_view(info))
    }
}

/// A question as the window gets it.
#[derive(Debug, Clone, Serialize)]
pub struct AgentQuestion {
    pub prompt_id: u64,
    #[serde(flatten)]
    pub prompt: AgentPrompt,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<bool>>>>;

/// Shows questions in the main window and waits for the answer from it.
pub struct TauriConfirmer {
    app: AppHandle,
    next: AtomicU64,
    pending: Pending,
}

impl TauriConfirmer {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            next: AtomicU64::new(1),
            pending: Arc::default(),
        }
    }

    /// The window's answer; a question nobody waits for any more is ignored.
    pub fn answer(&self, prompt_id: u64, allow: bool) {
        if let Some(tx) = self
            .pending
            .lock()
            .expect("agent prompts")
            .remove(&prompt_id)
        {
            let _ = tx.send(allow);
        }
    }
}

/// Takes a question back when its request goes away (answered, timed out, agent gone).
struct Withdraw {
    id: u64,
    pending: Pending,
    app: AppHandle,
}

impl Drop for Withdraw {
    fn drop(&mut self) {
        self.pending.lock().expect("agent prompts").remove(&self.id);
        let _ = self.app.emit("agent-confirm-withdrawn", self.id);
    }
}

#[async_trait::async_trait]
impl Confirmer for TauriConfirmer {
    async fn confirm(&self, prompt: AgentPrompt) -> bool {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().expect("agent prompts").insert(id, tx);
        let _withdraw = Withdraw {
            id,
            pending: self.pending.clone(),
            app: self.app.clone(),
        };
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        let question = AgentQuestion {
            prompt_id: id,
            prompt,
        };
        if self.app.emit("agent-confirm", &question).is_err() {
            return false;
        }
        rx.await.unwrap_or(false)
    }
}

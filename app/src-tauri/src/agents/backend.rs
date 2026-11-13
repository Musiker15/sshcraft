//! What the gate needs from the app: the stored hosts, groups and keys, the agent policy and
//! log, and the actions themselves. The app's version uses the same commands the window uses;
//! tests use a fake.

use std::time::Duration;

use sshcraft_agent::{AgentError, ExecResult, KeyType, KeyView, SessionOutput};
use sshcraft_core::agent::{AgentLogEntry, AgentPolicy};
use sshcraft_core::model::{Group, GroupId, Host, HostId, KeyId};

#[async_trait::async_trait]
pub trait AgentBackend: Send + Sync + 'static {
    fn policy(&self) -> AgentPolicy;
    fn hosts(&self) -> Result<Vec<Host>, AgentError>;
    fn groups(&self) -> Result<Vec<Group>, AgentError>;
    /// Stored keys by name, type and fingerprint only.
    fn keys(&self) -> Result<Vec<KeyView>, AgentError>;
    fn log(&self, entry: AgentLogEntry);

    async fn exec(
        &self,
        host: HostId,
        command: &str,
        timeout: Duration,
    ) -> Result<ExecResult, AgentError>;
    fn session_open(&self, host: HostId) -> Result<String, AgentError>;
    /// Whether `session` is one an agent opened.
    fn session_owned(&self, session: &str) -> bool;
    fn session_write(&self, session: &str, data: &[u8]) -> Result<(), AgentError>;
    async fn session_read(
        &self,
        session: &str,
        wait: Duration,
    ) -> Result<SessionOutput, AgentError>;
    fn session_close(&self, session: &str) -> Result<(), AgentError>;

    fn save_host(&self, host: Host) -> Result<(), AgentError>;
    fn save_group(&self, group: Group) -> Result<(), AgentError>;
    fn delete_host(&self, host: HostId) -> Result<(), AgentError>;
    fn delete_group(&self, group: GroupId) -> Result<(), AgentError>;
    fn delete_key(&self, key: KeyId) -> Result<(), AgentError>;
    async fn generate_key(&self, name: &str, key_type: KeyType) -> Result<KeyView, AgentError>;
    async fn import_key(
        &self,
        name: &str,
        text: &str,
        passphrase: Option<&str>,
    ) -> Result<KeyView, AgentError>;
}

/// What the user is asked before an action runs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AgentPrompt {
    pub agent: String,
    pub action: String,
    pub target: String,
    pub detail: String,
}

/// Asks the user in SSHCraft's own window; `false` for no. Dropping the future withdraws the
/// question.
#[async_trait::async_trait]
pub trait Confirmer: Send + Sync + 'static {
    async fn confirm(&self, prompt: AgentPrompt) -> bool;
}

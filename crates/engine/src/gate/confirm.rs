//! Asking the user before an agent's action runs. The engine puts the question into the prompt
//! registry (one id count for every kind of question), sends it on the event bus and waits for
//! `prompt.agent`. Without a window to ask in, the answer is no.

use serde::Serialize;

/// A question about an agent's action, as the window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentPrompt {
    pub prompt_id: u64,
    pub agent: String,
    pub action: String,
    pub target: String,
    pub detail: String,
}

/// Answers questions about agents' actions; `true` allows. The engine's own answers through the
/// window; tests give a fixed answer.
#[async_trait::async_trait]
pub trait Confirmer: Send + Sync {
    async fn confirm(&self, prompt: AgentPrompt) -> bool;
}

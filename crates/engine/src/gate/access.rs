//! What a command needs to be reachable for agents.

use serde_json::Value;
use sshcraft_store::agent::{AgentCategory, AgentOutcome};

use super::scope::Connection;
use super::view::GateView;
use crate::error::EngineError;

/// When an agent's request needs the user's answer in the SSHCraft window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskRule {
    /// The level of the category decides (allow, ask, deny).
    Level,
    /// Acting on a session the same connection opened needs no new question.
    OwnSession,
    /// Cleaning up what the agent itself opened (closing its session) is always allowed.
    Cleanup,
}

/// The agent's parameters, checked and turned into the command's parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub params: Value,
    /// What the request is about, for the question and the log (a host name, a key name).
    pub target: String,
    /// What it does, for the question and the log (the command line of `host.exec`).
    pub detail: String,
}

/// Why a request was refused, and how it goes into the agent log.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub error: EngineError,
    pub outcome: AgentOutcome,
}

pub type Resolve = fn(&GateView, &Connection, Value) -> Result<Resolved, Refusal>;
pub type Present = fn(&GateView, Value) -> Value;

/// How a command is reachable for agents. A command without one is never reachable for them.
#[derive(Debug)]
pub struct AgentAccess {
    pub category: AgentCategory,
    pub ask: AskRule,
    pub resolve: Resolve,
    /// What the agent gets back: never secrets, never hidden hosts.
    pub present: Present,
    /// The command only reads the agent's own data (its sessions), never the user's.
    pub own_data: bool,
    /// Whether the request goes into the agent log (lists do not).
    pub logged: bool,
}

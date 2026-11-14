//! Who calls a command. Everything except the user goes through the agent gate.

use serde::Serialize;

use crate::gate::wire::valid_agent_name;

/// The checked name of an agent (the MCP client's name or what `--agent` says).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentName(String);

impl AgentName {
    /// `None` for a name that is empty, too long or has control or bidi characters.
    pub fn new(name: &str) -> Option<Self> {
        valid_agent_name(name).then(|| Self(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How an agent reached SSHCraft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    Mcp,
    Launcher,
    Cli,
}

/// One connection of an agent or of the control channel; its sessions end with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct ConnId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "caller", rename_all = "snake_case")]
pub enum Caller {
    /// The user in the interface: menus, shortcuts, the command palette.
    User,
    Agent {
        name: AgentName,
        via: Via,
        conn: ConnId,
    },
    /// The control channel (`--control`): checked like an agent named `control`.
    Control {
        conn: ConnId,
    },
}

impl Caller {
    /// How the caller appears in the journal and in the agent log.
    pub fn label(&self) -> String {
        match self {
            Self::User => "user".to_string(),
            Self::Agent { name, .. } => format!("agent:{}", name.as_str()),
            Self::Control { .. } => "control".to_string(),
        }
    }

    pub fn is_user(&self) -> bool {
        matches!(self, Self::User)
    }
}

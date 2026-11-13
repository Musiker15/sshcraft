//! What AI agents send to SSHCraft and what they get back, one JSON object per line over a
//! local connection only the user's own processes can open. Nothing here ever carries private
//! key material: keys appear with name, type and fingerprint only.

pub mod framing;
pub mod transport;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Longest agent name accepted (the MCP client name or `--agent`).
pub const MAX_AGENT_NAME: usize = 64;
/// Most output one `exec` or `session_read` returns.
pub const MAX_OUTPUT: usize = 1024 * 1024;
pub const DEFAULT_EXEC_SECS: u64 = 60;
pub const MAX_EXEC_SECS: u64 = 600;
pub const MAX_WAIT_MS: u64 = 10_000;

/// Key types an agent may generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyType {
    Ed25519,
    Ecdsa,
    Rsa,
}

/// A host as an agent sees and writes it: no passwords, no identities' secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostView {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub group: Option<Uuid>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub username: Option<String>,
    /// The id of a stored key the host uses, if any.
    #[serde(default)]
    pub key: Option<Uuid>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A host to create (no `id`) or change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub group: Option<Uuid>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub key: Option<Uuid>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupView {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub parent: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub parent: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyView {
    pub id: Uuid,
    pub name: String,
    /// `ed25519`, `ecdsa-p256`, `rsa-4096` and the like.
    pub kind: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecResult {
    pub output: String,
    /// The output went past [`MAX_OUTPUT`] and was cut.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOutput {
    pub output: String,
    /// The session is over; nothing more will come.
    pub ended: bool,
}

/// Everything an agent can ask for. Hosts, groups and keys are named by id or by a name that
/// is unique among them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    ListHosts,
    ListGroups,
    ListKeys,
    HostInfo {
        host: String,
    },
    Exec {
        host: String,
        command: String,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    SessionOpen {
        host: String,
    },
    SessionWrite {
        session: String,
        data: String,
    },
    SessionRead {
        session: String,
        #[serde(default)]
        wait_ms: Option<u64>,
    },
    SessionClose {
        session: String,
    },
    SaveHost {
        host: HostInput,
    },
    SaveGroup {
        group: GroupInput,
    },
    DeleteHost {
        host: String,
    },
    DeleteGroup {
        group: String,
    },
    DeleteKey {
        key: String,
    },
    GenerateKey {
        name: String,
        #[serde(rename = "type")]
        key_type: KeyType,
    },
    ImportKey {
        name: String,
        /// The private key in OpenSSH or PEM form; it goes in, never out.
        text: String,
        #[serde(default)]
        passphrase: Option<String>,
    },
}

impl Action {
    /// The action's name as in the protocol (`list_hosts`, `exec`, …).
    pub fn name(&self) -> &'static str {
        match self {
            Self::ListHosts => "list_hosts",
            Self::ListGroups => "list_groups",
            Self::ListKeys => "list_keys",
            Self::HostInfo { .. } => "host_info",
            Self::Exec { .. } => "exec",
            Self::SessionOpen { .. } => "session_open",
            Self::SessionWrite { .. } => "session_write",
            Self::SessionRead { .. } => "session_read",
            Self::SessionClose { .. } => "session_close",
            Self::SaveHost { .. } => "save_host",
            Self::SaveGroup { .. } => "save_group",
            Self::DeleteHost { .. } => "delete_host",
            Self::DeleteGroup { .. } => "delete_group",
            Self::DeleteKey { .. } => "delete_key",
            Self::GenerateKey { .. } => "generate_key",
            Self::ImportKey { .. } => "import_key",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    /// Who asks: the MCP client's name or what `--agent` says.
    pub agent: String,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// No such host, group, key or session (or one hidden from agents).
    NotFound,
    /// Not allowed, or declined in SSHCraft.
    Denied,
    /// The request does not fit (arguments, name, size).
    Invalid,
    /// Allowed, but it did not work (connection, timeout, store).
    Failed,
    /// SSHCraft is not running.
    NotRunning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct AgentError {
    pub kind: ErrorKind,
    pub message: String,
}

impl AgentError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AgentError>,
}

impl Response {
    pub fn ok(id: u64, value: serde_json::Value) -> Self {
        Self {
            id,
            ok: Some(value),
            error: None,
        }
    }

    pub fn error(id: u64, error: AgentError) -> Self {
        Self {
            id,
            ok: None,
            error: Some(error),
        }
    }
}

/// Whether `name` may name an agent: 1 to [`MAX_AGENT_NAME`] characters, none of them control
/// characters.
pub fn valid_agent_name(name: &str) -> bool {
    let len = name.chars().count();
    (1..=MAX_AGENT_NAME).contains(&len)
        && !name.chars().any(char::is_control)
        && !name.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_goes_through_json_and_back() {
        let actions = vec![
            Action::ListHosts,
            Action::ListGroups,
            Action::ListKeys,
            Action::HostInfo { host: "web".into() },
            Action::Exec {
                host: "web".into(),
                command: "uptime".into(),
                timeout_secs: Some(5),
            },
            Action::SessionOpen { host: "web".into() },
            Action::SessionWrite {
                session: "s".into(),
                data: "ls\n".into(),
            },
            Action::SessionRead {
                session: "s".into(),
                wait_ms: None,
            },
            Action::SessionClose {
                session: "s".into(),
            },
            Action::SaveHost {
                host: HostInput {
                    id: None,
                    name: "db".into(),
                    group: None,
                    address: Some("10.0.0.5".into()),
                    port: Some(22),
                    username: Some("root".into()),
                    key: None,
                    tags: vec!["prod".into()],
                },
            },
            Action::SaveGroup {
                group: GroupInput {
                    id: None,
                    name: "eu".into(),
                    parent: None,
                },
            },
            Action::DeleteHost { host: "db".into() },
            Action::DeleteGroup { group: "eu".into() },
            Action::DeleteKey { key: "old".into() },
            Action::GenerateKey {
                name: "deploy".into(),
                key_type: KeyType::Ed25519,
            },
            Action::ImportKey {
                name: "k".into(),
                text: "-----BEGIN OPENSSH PRIVATE KEY-----".into(),
                passphrase: None,
            },
        ];
        for action in actions {
            let request = Request {
                id: 7,
                agent: "claude-code".into(),
                action: action.clone(),
            };
            let line = serde_json::to_string(&request).unwrap();
            assert!(
                line.contains(&format!("\"action\":\"{}\"", action.name())),
                "{line}"
            );
            let back: Request = serde_json::from_str(&line).unwrap();
            assert_eq!(back, request);
        }
    }

    #[test]
    fn agent_names_are_checked() {
        assert!(valid_agent_name("claude-code"));
        assert!(!valid_agent_name(""));
        assert!(!valid_agent_name("   "));
        assert!(!valid_agent_name("a\nb"));
        assert!(!valid_agent_name(&"x".repeat(MAX_AGENT_NAME + 1)));
    }

    #[test]
    fn responses_carry_either_a_value_or_an_error() {
        let ok = serde_json::to_string(&Response::ok(1, serde_json::json!([1]))).unwrap();
        assert_eq!(ok, r#"{"id":1,"ok":[1]}"#);
        let err = Response::error(2, AgentError::new(ErrorKind::Denied, "declined"));
        let text = serde_json::to_string(&err).unwrap();
        assert_eq!(serde_json::from_str::<Response>(&text).unwrap(), err);
    }
}

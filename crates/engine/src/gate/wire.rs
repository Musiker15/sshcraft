//! What agents see and write: hosts, groups and keys without secrets. Nothing here ever carries
//! private key material; keys appear with name, type and fingerprint only.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Longest agent name accepted (the MCP client name or `--agent`).
pub const MAX_AGENT_NAME: usize = 64;
/// Most output one `host.exec` or `session.read` returns.
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

/// For a field of a change: missing keeps the stored value, `null` clears it.
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// A host to create (no `id`) or change. On a change only the fields that are there change: a
/// missing field keeps its value, `null` clears it.
/// Any other field (jump hosts, identity, password, proxy, agent forwarding) is refused: an
/// agent changes only what is listed here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub group: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub address: Option<Option<String>>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub port: Option<Option<u16>>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub username: Option<Option<String>>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub key: Option<Option<Uuid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupView {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub parent: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// The command's exit status, if the server reported one (0 is success).
    #[serde(default)]
    pub exit_status: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOutput {
    pub output: String,
    /// The session is over; nothing more will come.
    pub ended: bool,
}

/// Characters that reorder text on screen (Trojan source); never allowed in names.
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Whether `name` may name an agent: 1 to [`MAX_AGENT_NAME`] characters, none of them control
/// characters.
pub fn valid_agent_name(name: &str) -> bool {
    let len = name.chars().count();
    (1..=MAX_AGENT_NAME).contains(&len) && !name.chars().any(|c| c.is_control() || is_bidi_control(c)) && !name.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::{HostInput, MAX_AGENT_NAME, valid_agent_name};

    #[test]
    fn missing_fields_keep_null_clears() {
        let input: HostInput = serde_json::from_str(r#"{"name": "x", "key": null}"#).unwrap();
        assert_eq!(input.key, Some(None));
        assert_eq!(input.address, None);
        let input: HostInput = serde_json::from_str(r#"{"address": "1.2.3.4"}"#).unwrap();
        assert_eq!(input.address, Some(Some("1.2.3.4".into())));
        assert_eq!(input.name, None);
    }

    #[test]
    fn agent_names_are_checked() {
        assert!(!valid_agent_name("claude\u{202E}edoc"));
        assert!(valid_agent_name("claude-code"));
        assert!(!valid_agent_name(""));
        assert!(!valid_agent_name("   "));
        assert!(!valid_agent_name("a\nb"));
        assert!(!valid_agent_name(&"x".repeat(MAX_AGENT_NAME + 1)));
    }
}

//! What AI agents may do through SSHCraft: a level per kind of action, hosts and groups hidden
//! from them, and a log of everything they asked for. All of it stays on this device; the log
//! is sealed with the vault (it holds commands).

use std::collections::{BTreeMap, BTreeSet, HashSet};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{Group, GroupId, Host, HostId, group_chain};
use crate::store::{Store, StoreError};
use crate::vault::{Vault, VaultError};

/// Kinds of actions, each with one level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCategory {
    /// Hosts, groups and key names.
    List,
    /// Connect, run commands, sessions.
    Connect,
    /// Create and change hosts and groups.
    Edit,
    /// Delete hosts, groups and keys.
    Delete,
    /// Import or generate keys.
    Keys,
    /// Share and change or end shares.
    Share,
    /// Revoke devices, manage sync servers and accounts.
    Sync,
}

impl AgentCategory {
    pub const ALL: [Self; 7] = [
        Self::List,
        Self::Connect,
        Self::Edit,
        Self::Delete,
        Self::Keys,
        Self::Share,
        Self::Sync,
    ];

    /// The level on a fresh install (design, section 10).
    pub fn default_level(self) -> AgentLevel {
        match self {
            Self::List => AgentLevel::Allow,
            Self::Connect | Self::Edit | Self::Keys => AgentLevel::Ask,
            Self::Delete | Self::Share | Self::Sync => AgentLevel::Deny,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLevel {
    Allow,
    Ask,
    Deny,
}

/// Levels that differ from the defaults, and the hidden hosts and groups.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPolicy {
    #[serde(default)]
    pub levels: BTreeMap<AgentCategory, AgentLevel>,
    /// Ids of hosts and groups agents never see.
    #[serde(default)]
    pub hidden: BTreeSet<Uuid>,
}

const POLICY_KEY: &str = "agents.policy";

impl AgentPolicy {
    pub fn level(&self, category: AgentCategory) -> AgentLevel {
        self.levels
            .get(&category)
            .copied()
            .unwrap_or_else(|| category.default_level())
    }

    /// The stored policy; a damaged one or unknown categories in it fall back to the defaults.
    pub fn load(store: &Store) -> Self {
        let Ok(Some(text)) = store.get_setting(POLICY_KEY) else {
            return Self::default();
        };
        let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&text)
        else {
            return Self::default();
        };
        let levels = map
            .get("levels")
            .and_then(|v| v.as_object())
            .map(|levels| {
                levels
                    .iter()
                    .filter_map(|(k, v)| {
                        let category =
                            serde_json::from_value(serde_json::Value::String(k.clone())).ok()?;
                        let level = serde_json::from_value(v.clone()).ok()?;
                        Some((category, level))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let hidden = map
            .get("hidden")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        Self { levels, hidden }
    }

    pub fn save(&self, store: &Store) -> Result<(), StoreError> {
        let json = serde_json::to_string(self).map_err(|e| StoreError::Malformed(e.to_string()))?;
        store.put_setting(POLICY_KEY, &json)
    }

    /// Whether `group` or a group above it is hidden.
    pub fn group_hidden(&self, groups: &[Group], group: GroupId) -> bool {
        group_chain(groups, group)
            .iter()
            .any(|g| self.hidden.contains(&g.id.0))
    }

    /// Whether `host` is hidden itself or through one of its groups.
    pub fn host_hidden(&self, groups: &[Group], host: &Host) -> bool {
        self.hidden.contains(&host.id.0)
            || host.group_id.is_some_and(|g| self.group_hidden(groups, g))
    }

    /// Every hidden host among `hosts`.
    pub fn hidden_hosts(&self, hosts: &[Host], groups: &[Group]) -> HashSet<HostId> {
        hosts
            .iter()
            .filter(|h| self.host_hidden(groups, h))
            .map(|h| h.id)
            .collect()
    }
}

/// How an agent's request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOutcome {
    /// Allowed without asking.
    Allowed,
    /// Asked, and the user allowed it.
    AskedAllowed,
    /// Asked, and the user declined (or did not answer in time).
    AskedDenied,
    /// The level is deny.
    Denied,
    /// About a hidden host or group: answered as unknown.
    Hidden,
    /// Allowed, but it failed.
    Failed,
}

pub const MAX_LOG: usize = 1000;
pub const MAX_DETAIL: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLogEntry {
    pub id: Uuid,
    /// Unix milliseconds.
    pub at: i64,
    pub agent: String,
    pub action: String,
    /// The host, group or key the action was about, by name; empty if none.
    pub target: String,
    /// The command for commands, else empty.
    pub detail: String,
    pub outcome: AgentOutcome,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentLogError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for AgentLogError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(e.into())
    }
}

/// Version 11: the log of agent actions.
pub(crate) const SCHEMA_V11: &str = "
CREATE TABLE agent_log (
    id TEXT PRIMARY KEY,
    at INTEGER NOT NULL,
    sealed BLOB NOT NULL
);
CREATE INDEX agent_log_at ON agent_log (at);
";

fn label(id: Uuid) -> String {
    format!("agent-log:{id}")
}

/// Keeps an entry (its detail cut to [`MAX_DETAIL`] characters); the oldest beyond
/// [`MAX_LOG`] go.
pub fn log_append(
    vault: &Vault,
    store: &Store,
    entry: &AgentLogEntry,
) -> Result<(), AgentLogError> {
    let mut entry = entry.clone();
    entry.detail = entry.detail.chars().take(MAX_DETAIL).collect();
    let json = serde_json::to_vec(&entry).map_err(|e| StoreError::Malformed(e.to_string()))?;
    let sealed = vault.seal_secret(&label(entry.id), &json)?;
    store.atomic(|| {
        store.conn.execute(
            "INSERT OR REPLACE INTO agent_log (id, at, sealed) VALUES (?1, ?2, ?3)",
            params![entry.id.to_string(), entry.at, sealed],
        )?;
        store.conn.execute(
            "DELETE FROM agent_log WHERE id NOT IN
                 (SELECT id FROM agent_log ORDER BY at DESC, id DESC LIMIT ?1)",
            [i64::try_from(MAX_LOG).unwrap_or(i64::MAX)],
        )?;
        Ok::<(), AgentLogError>(())
    })
}

/// The log, newest first; an entry that does not open is left out.
pub fn log_list(vault: &Vault, store: &Store) -> Result<Vec<AgentLogEntry>, AgentLogError> {
    let mut stmt = store
        .conn
        .prepare("SELECT id, sealed FROM agent_log ORDER BY at DESC, id DESC")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, sealed)| {
            let id = Uuid::parse_str(&id).ok()?;
            let plain = vault.open_secret(&label(id), &sealed).ok()?;
            let entry: AgentLogEntry = serde_json::from_slice(&plain).ok()?;
            (entry.id == id).then_some(entry)
        })
        .collect())
}

pub fn log_clear(store: &Store) -> Result<(), AgentLogError> {
    store.conn.execute("DELETE FROM agent_log", [])?;
    Ok(())
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;

//! Importing hosts from other programs: every source turns its data into
//! [`ImportCandidate`]s, the user confirms them in a preview, [`apply_import`] creates them.
//!
//! Import files are untrusted input: a broken line is skipped and reported, never the whole
//! import. Nothing from an import file is logged.

use serde::{Deserialize, Serialize};

use crate::model::{
    ConnectionSettings, Group, GroupId, Host, HostId, HostOs, ServerId, group_chain,
};
use crate::store::{Store, StoreError};

/// Longest name kept from an import file, in characters.
pub const MAX_NAME: usize = 200;
/// Largest import file read, in bytes.
pub const MAX_FILE: u64 = 10 * 1024 * 1024;

/// One host the user can take over.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportCandidate {
    pub name: String,
    /// Groups from the top down; empty for a host without a group.
    #[serde(default)]
    pub group_path: Vec<String>,
    /// Defaults for the innermost group when the import creates it (an `ssh/config` host
    /// shared by several aliases becomes a group with that address).
    #[serde(default)]
    pub group_defaults: ConnectionSettings,
    pub settings: ConnectionSettings,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub os: Option<HostOs>,
    pub key_note: Option<KeyNote>,
    /// Things the preview should point out; candidates with warnings start unchecked.
    #[serde(default)]
    pub warnings: Vec<ImportWarning>,
}

/// Why a candidate has no key yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeyNote {
    /// The source names no key.
    NoIdentityFile,
    /// The agent is reachable but holds none of the named keys.
    NotInAgent,
    /// The agent could not be asked, so it is unknown whether it holds the key.
    AgentUnavailable,
    /// The source names a key file; it can be imported on its own.
    File { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportWarning {
    /// `ProxyJump` or `ProxyCommand` is not followed; the host would connect directly.
    ProxyIgnored { directive: String },
    /// A `%` token in `HostName` other than `%h` and `%%`, left as written.
    UnsupportedToken { token: String },
    /// The source stores a password; it is not taken over.
    PasswordNotImported,
    /// The port was not a number from 1 to 65535 and was left empty.
    BadPort,
    /// Something the source sets that SSHCraft does not take over.
    Unsupported { what: String },
}

/// What a source offers, for the preview.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPlan {
    pub candidates: Vec<ImportCandidate>,
    /// Entries that could not be used, with the reason (line numbers, not contents).
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOutcome {
    pub created: usize,
    pub duplicates: usize,
    /// What was created, so it can be queued for its server.
    pub groups: Vec<GroupId>,
    pub hosts: Vec<HostId>,
}

/// A name from an import file: control characters removed, trimmed, at most [`MAX_NAME`].
pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME)
        .collect()
}

/// A port from an import file; anything but 1 to 65535 is dropped with a warning (an empty
/// value is no port, without warning).
pub fn port_from(text: &str, warnings: &mut Vec<ImportWarning>) -> Option<u16> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    match text.parse::<u32>() {
        Ok(port @ 1..=65535) => Some(port as u16),
        _ => {
            warnings.push(ImportWarning::BadPort);
            None
        }
    }
}

/// Finds or creates the group for `path` (from the top down) at `location`; returns `None`
/// for an empty path.
fn group_for(
    store: &Store,
    groups: &mut Vec<Group>,
    path: &[String],
    innermost: &ConnectionSettings,
    location: Option<ServerId>,
    created: &mut Vec<GroupId>,
) -> Result<Option<GroupId>, StoreError> {
    let mut parent: Option<GroupId> = None;
    for (level, name) in path.iter().enumerate() {
        let name = clean(name);
        if name.is_empty() {
            continue;
        }
        let writable = |g: &Group| -> bool {
            // A group that came in someone else's share is only written into where that share
            // allows adding (write, all hosts).
            match location {
                Some(server) => match store.received_share_of(
                    server,
                    crate::store::ItemKind::Group,
                    &g.id.0.to_string(),
                ) {
                    Ok(Some(share)) => {
                        share.role == Some(crate::store::ShareRole::Write)
                            && share.kind == crate::store::ShareKind::GroupAll
                    }
                    Ok(None) => true,
                    Err(_) => false,
                },
                None => true,
            }
        };
        let found = groups.iter().find(|g| {
            g.name == name
                && g.parent_id == parent
                && (parent.is_some() || g.location == location)
                && writable(g)
        });
        let id = match found {
            Some(group) => group.id,
            None => {
                let group = Group {
                    id: GroupId::new(),
                    name,
                    defaults: if level + 1 == path.len() {
                        innermost.clone()
                    } else {
                        ConnectionSettings::default()
                    },
                    location,
                    parent_id: parent,
                    tags: Vec::new(),
                };
                store.upsert_group(&group)?;
                created.push(group.id);
                let id = group.id;
                groups.push(group);
                id
            }
        };
        parent = Some(id);
    }
    Ok(parent)
}

/// Creates exactly the given candidates (as confirmed in the preview) and the groups on their
/// paths, at `location` (`None`: this device only). A host that already exists in the same
/// group with the same name, address, port and user is skipped; groups with the same name on
/// the same level are reused.
pub fn apply_import(
    store: &Store,
    candidates: &[ImportCandidate],
    location: Option<ServerId>,
) -> Result<ImportOutcome, StoreError> {
    let mut groups = store.list_groups()?;
    let mut hosts = store.list_hosts()?;
    let before = hosts.len();
    let mut outcome = ImportOutcome::default();
    for candidate in candidates {
        let group_id = group_for(
            store,
            &mut groups,
            &candidate.group_path,
            &candidate.group_defaults,
            location,
            &mut outcome.groups,
        )?;
        let host = Host {
            id: HostId::new(),
            name: clean(&candidate.name),
            group_id,
            settings: candidate.settings.clone(),
            location,
            tags: candidate.tags.clone(),
            os: candidate.os,
        };
        let key = |h: &Host| {
            let chain = h
                .group_id
                .map(|g| group_chain(&groups, g))
                .unwrap_or_default();
            let pick = |own: &Option<String>, get: fn(&ConnectionSettings) -> &Option<String>| {
                own.clone()
                    .or_else(|| chain.iter().find_map(|g| get(&g.defaults).clone()))
            };
            (
                h.name.to_lowercase(),
                h.location,
                pick(&h.settings.address, |d| &d.address),
                h.settings
                    .port
                    .or_else(|| chain.iter().find_map(|g| g.defaults.port))
                    .unwrap_or(crate::model::DEFAULT_PORT),
                pick(&h.settings.username, |d| &d.username),
            )
        };
        // The same machine (address and port at this place): with the same name and user, or
        // from a source that knows no user (known_hosts) when it was there before this import.
        let new = key(&host);
        let duplicate = hosts.iter().enumerate().any(|(i, h)| {
            let old = key(h);
            let machine = old.1 == new.1 && old.2 == new.2 && old.3 == new.3;
            machine && ((old.0 == new.0 && old.4 == new.4) || (new.4.is_none() && i < before))
        });
        if duplicate {
            outcome.duplicates += 1;
            continue;
        }
        store.upsert_host(&host)?;
        outcome.created += 1;
        outcome.hosts.push(host.id);
        hosts.push(host);
    }
    Ok(outcome)
}

pub mod csv;
pub mod keyfiles;
pub mod knownhosts;
pub mod mobaxterm;
pub mod putty;
pub mod securecrt;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_sessions;

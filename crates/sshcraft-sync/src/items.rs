//! What a record holds: one group, host or key, or the note that one was deleted, plus when it
//! was changed. The location is never part of it: every device puts synced items at the server
//! they came from.

use crate::SyncError;
use serde::{Deserialize, Serialize};
use sshcraft_core::model::{Group, GroupId, Host, HostId, KeyId, ServerId};
use sshcraft_core::store::{ItemKind, Store};
use sshcraft_core::vault::{Vault, parse_private_key};
use sshcraft_proto::ProtoError;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemBody {
    Group {
        group: Group,
    },
    Host {
        host: Host,
    },
    Key {
        id: KeyId,
        name: String,
        openssh: Zeroizing<String>,
    },
    Deleted {
        item_kind: ItemKind,
        item_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncItem {
    /// Unix milliseconds of the change, on the device that made it.
    pub modified_at: i64,
    pub body: ItemBody,
}

impl SyncItem {
    /// Kind and id of the item this body is about.
    pub fn item(&self) -> (ItemKind, String) {
        match &self.body {
            ItemBody::Group { group } => (ItemKind::Group, group.id.0.to_string()),
            ItemBody::Host { host } => (ItemKind::Host, host.id.0.to_string()),
            ItemBody::Key { id, .. } => (ItemKind::Key, id.0.to_string()),
            ItemBody::Deleted { item_kind, item_id } => (*item_kind, item_id.clone()),
        }
    }
}

pub fn encode(item: &SyncItem) -> Result<Zeroizing<Vec<u8>>, SyncError> {
    serde_json::to_vec(item)
        .map(Zeroizing::new)
        .map_err(|e| SyncError::Proto(ProtoError::Malformed(e.to_string())))
}

pub fn decode(bytes: &[u8]) -> Result<SyncItem, SyncError> {
    serde_json::from_slice(bytes)
        .map_err(|e| SyncError::Proto(ProtoError::Malformed(e.to_string())))
}

fn parse_id(id: &str) -> Result<Uuid, SyncError> {
    Uuid::parse_str(id).map_err(|e| SyncError::Store(e.to_string()))
}

/// The current local state of an item, ready to seal; `Deleted` if it no longer exists here.
pub fn local_item(
    store: &Store,
    vault: &Vault,
    kind: ItemKind,
    id: &str,
    now_ms: i64,
) -> Result<SyncItem, SyncError> {
    let uuid = parse_id(id)?;
    let deleted = || ItemBody::Deleted {
        item_kind: kind,
        item_id: id.to_owned(),
    };
    let body = match kind {
        ItemKind::Group => store
            .list_groups()?
            .into_iter()
            .find(|g| g.id == GroupId(uuid))
            .map(|mut group| {
                group.location = None;
                ItemBody::Group { group }
            })
            .unwrap_or_else(deleted),
        ItemKind::Host => store
            .list_hosts()?
            .into_iter()
            .find(|h| h.id == HostId(uuid))
            .map(|mut host| {
                host.location = None;
                ItemBody::Host { host }
            })
            .unwrap_or_else(deleted),
        ItemKind::Key => match store.key_record(KeyId(uuid))? {
            Some(record) => {
                let key = vault.private_key(store, record.id)?;
                let openssh = key
                    .to_openssh(russh::keys::ssh_key::LineEnding::LF)
                    .map_err(|e| SyncError::Vault(e.to_string()))?;
                ItemBody::Key {
                    id: record.id,
                    name: record.name,
                    openssh: Zeroizing::new(openssh.to_string()),
                }
            }
            None => deleted(),
        },
    };
    Ok(SyncItem {
        modified_at: now_ms,
        body,
    })
}

/// Writes an item that came from `server` into the local store, at that server's location.
pub fn apply(
    store: &Store,
    vault: &Vault,
    server: ServerId,
    item: &SyncItem,
) -> Result<(), SyncError> {
    match &item.body {
        ItemBody::Group { group } => {
            let mut group = group.clone();
            group.location = Some(server);
            store.upsert_group(&group)?;
            if store.location_of(ItemKind::Group, &group.id.0.to_string())? != Some(server) {
                store.set_location(ItemKind::Group, &group.id.0.to_string(), Some(server))?;
            }
        }
        ItemBody::Host { host } => {
            let mut host = host.clone();
            host.location = Some(server);
            store.upsert_host(&host)?;
            let id = host.id.0.to_string();
            if host.group_id.is_none() && store.location_of(ItemKind::Host, &id)? != Some(server) {
                store.set_location(ItemKind::Host, &id, Some(server))?;
            }
        }
        ItemBody::Key { id, name, openssh } => {
            let key = parse_private_key(openssh, None)?;
            vault.store_key_with_id(store, *id, name, &key, Some(server))?;
            if store.location_of(ItemKind::Key, &id.0.to_string())? != Some(server) {
                store.set_location(ItemKind::Key, &id.0.to_string(), Some(server))?;
            }
        }
        ItemBody::Deleted { item_kind, item_id } => {
            let uuid = parse_id(item_id)?;
            match item_kind {
                ItemKind::Group => store.delete_group(GroupId(uuid))?,
                ItemKind::Host => store.delete_host(HostId(uuid))?,
                ItemKind::Key => store.delete_key_record(KeyId(uuid))?,
            }
        }
    }
    Ok(())
}

/// "Last change wins": whether the local change is newer than the one on the server. On a tie
/// the server's state wins, so two devices always end up with the same state.
pub fn newer(local_modified: i64, remote_modified: i64) -> bool {
    local_modified > remote_modified
}

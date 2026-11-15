//! What a record holds: one group, host or key, or the note that one was deleted, plus when it
//! was changed. The location is never part of it: every device puts synced items at the server
//! they came from.

use crate::SyncError;
use crate::creds::{CredentialRefs, drop_unreferenced, keep_local, same_proxy_endpoint};
use serde::{Deserialize, Serialize};
use sshcraft_model::keytypes::KeyKind;
use sshcraft_model::model::{Group, GroupId, Host, HostId, Identity, IdentityId, KeyId, LogId, PasswordId, ServerId};
use sshcraft_proto::ProtoError;
use sshcraft_store::session_log::{self, LogError, LogMeta};
use sshcraft_store::store::{ItemKind, Store};
use sshcraft_store::vault::Vault;
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
        /// The stored OpenSSH text; encrypted when the passphrase is not saved.
        openssh: Zeroizing<String>,
        /// An OpenSSH user certificate for the key.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        certificate: Option<String>,
    },
    Identity {
        identity: Identity,
    },
    Password {
        id: PasswordId,
        secret: Zeroizing<String>,
    },
    /// The header of a finished session log: title, times, bookmarks and comments.
    Log {
        meta: LogMeta,
    },
    /// One part of a session log's output; written once, never changed.
    LogChunk {
        id: Uuid,
        log_id: LogId,
        seq: u32,
        #[serde(with = "sshcraft_proto::encoding::b64")]
        data: Vec<u8>,
        written_at: i64,
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
            ItemBody::Identity { identity } => (ItemKind::Identity, identity.id.0.to_string()),
            ItemBody::Password { id, .. } => (ItemKind::Password, id.0.to_string()),
            ItemBody::Log { meta } => (ItemKind::Log, meta.id.0.to_string()),
            ItemBody::LogChunk { id, .. } => (ItemKind::LogChunk, id.to_string()),
            ItemBody::Deleted { item_kind, item_id } => (*item_kind, item_id.clone()),
        }
    }
}

pub fn encode(item: &SyncItem) -> Result<Zeroizing<Vec<u8>>, SyncError> {
    serde_json::to_vec(item).map(Zeroizing::new).map_err(|e| SyncError::Proto(ProtoError::Malformed(e.to_string())))
}

pub fn decode(bytes: &[u8]) -> Result<SyncItem, SyncError> {
    serde_json::from_slice(bytes).map_err(|e| SyncError::Proto(ProtoError::Malformed(e.to_string())))
}

fn parse_id(id: &str) -> Result<Uuid, SyncError> {
    Uuid::parse_str(id).map_err(|e| SyncError::Store(e.to_string()))
}

/// The current local state of an item, ready to seal; `Deleted` if it no longer exists here.
pub fn local_item(store: &Store, vault: &Vault, kind: ItemKind, id: &str, now_ms: i64) -> Result<SyncItem, SyncError> {
    let uuid = parse_id(id)?;
    let deleted = || ItemBody::Deleted { item_kind: kind, item_id: id.to_owned() };
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
        // The stored text goes as it is: a key whose passphrase is not saved stays encrypted
        // with it. A key bound to this device never leaves it.
        ItemKind::Key => match store.key_record(KeyId(uuid))? {
            Some(record) if record.kind != KeyKind::Device => {
                ItemBody::Key { id: record.id, name: record.name, openssh: vault.stored_openssh(store, record.id)?, certificate: record.certificate }
            }
            _ => deleted(),
        },
        ItemKind::Identity => store
            .list_identities()?
            .into_iter()
            .find(|i| i.id == IdentityId(uuid))
            .map(|mut identity| {
                identity.location = None;
                ItemBody::Identity { identity }
            })
            .unwrap_or_else(deleted),
        ItemKind::Password => {
            let id = PasswordId(uuid);
            match store.password_record(id)? {
                Some(_) => ItemBody::Password { id, secret: vault.password(store, id)? },
                None => deleted(),
            }
        }
        // Only a finished log goes; a running one has no place yet.
        ItemKind::Log => match session_log::meta(vault, store, LogId(uuid)).map_err(log_error)? {
            Some(meta) if meta.ended_at.is_some() => ItemBody::Log { meta },
            _ => deleted(),
        },
        ItemKind::LogChunk => match session_log::chunk(vault, store, id).map_err(log_error)? {
            Some(chunk) => ItemBody::LogChunk { id: uuid, log_id: chunk.log, seq: chunk.seq, data: chunk.bytes, written_at: chunk.written_at },
            None => deleted(),
        },
    };
    Ok(SyncItem { modified_at: now_ms, body })
}

/// Writes an item that came from `server` into the local store, at that server's location.
pub fn apply(store: &Store, vault: &Vault, server: ServerId, item: &SyncItem) -> Result<(), SyncError> {
    apply_in(store, vault, server, item, None, None)
}

/// [`apply`] for a record of `area` (`personal`: this account's personal area). A record
/// carries no key for a key that is not in its area; the local assignment then stays.
pub fn apply_in(
    store: &Store,
    vault: &Vault,
    server: ServerId,
    item: &SyncItem,
    area: Option<Uuid>,
    personal: Option<Uuid>,
) -> Result<(), SyncError> {
    let keep = |local: Option<CredentialRefs>, incoming: &mut CredentialRefs, same_proxy: bool| {
        keep_local(store, server, &local.unwrap_or_default(), incoming, area, personal, same_proxy)
    };
    match &item.body {
        ItemBody::Group { group } => {
            let mut group = group.clone();
            group.location = Some(server);
            let mut refs = CredentialRefs::of_settings(&group.defaults);
            let before = store.list_groups()?.into_iter().find(|g| g.id == group.id);
            let same = before.as_ref().is_some_and(|b| same_proxy_endpoint(&b.defaults.proxy, &group.defaults.proxy));
            let before_refs = before.map(|g| CredentialRefs::of_settings(&g.defaults));
            keep(before_refs.clone(), &mut refs, same)?;
            refs.apply_to_settings(&mut group.defaults);
            store.upsert_group(&group)?;
            dropped_proxy(store, before_refs, &refs)?;
            // A subgroup takes its parent's location by itself.
            if group.parent_id.is_none() && store.location_of(ItemKind::Group, &group.id.0.to_string())? != Some(server) {
                store.set_location(ItemKind::Group, &group.id.0.to_string(), Some(server))?;
            }
        }
        ItemBody::Host { host } => {
            let mut host = host.clone();
            host.location = Some(server);
            let mut refs = CredentialRefs::of_settings(&host.settings);
            let before = store.list_hosts()?.into_iter().find(|h| h.id == host.id);
            let same = before.as_ref().is_some_and(|b| same_proxy_endpoint(&b.settings.proxy, &host.settings.proxy));
            let before_refs = before.map(|h| CredentialRefs::of_settings(&h.settings));
            keep(before_refs.clone(), &mut refs, same)?;
            refs.apply_to_settings(&mut host.settings);
            store.upsert_host(&host)?;
            dropped_proxy(store, before_refs, &refs)?;
            let id = host.id.0.to_string();
            if host.group_id.is_none() && store.location_of(ItemKind::Host, &id)? != Some(server) {
                store.set_location(ItemKind::Host, &id, Some(server))?;
            }
        }
        ItemBody::Key { id, name, openssh, certificate } => {
            // A key bound to this device is never replaced from outside.
            if store.key_record(*id)?.is_some_and(|r| r.kind == KeyKind::Device) {
                return Ok(());
            }
            vault.store_openssh_with_id(store, *id, name, openssh, certificate.as_deref(), Some(server))?;
            if store.location_of(ItemKind::Key, &id.0.to_string())? != Some(server) {
                store.set_location(ItemKind::Key, &id.0.to_string(), Some(server))?;
            }
        }
        ItemBody::Identity { identity } => {
            let mut identity = identity.clone();
            identity.location = Some(server);
            let mut refs = CredentialRefs { key: identity.key.clone(), password: identity.password, ..Default::default() };
            keep(
                store.list_identities()?.into_iter().find(|i| i.id == identity.id).map(|i| CredentialRefs {
                    key: i.key,
                    password: i.password,
                    ..Default::default()
                }),
                &mut refs,
                true,
            )?;
            identity.key = refs.key;
            identity.password = refs.password;
            store.upsert_identity(&identity)?;
            let id = identity.id.0.to_string();
            if store.location_of(ItemKind::Identity, &id)? != Some(server) {
                store.set_location(ItemKind::Identity, &id, Some(server))?;
            }
        }
        ItemBody::Password { id, secret } => {
            vault.store_password(store, *id, secret, Some(server))?;
        }
        ItemBody::Log { meta } => {
            session_log::put_meta(vault, store, meta, Some(server)).map_err(log_error)?;
        }
        ItemBody::LogChunk { id, log_id, seq, data, written_at } => {
            session_log::put_chunk(vault, store, *id, *log_id, *seq, data, *written_at, Some(server)).map_err(log_error)?;
        }
        ItemBody::Deleted { item_kind, item_id } => {
            let uuid = parse_id(item_id)?;
            match item_kind {
                // Local data stays authoritative: what another device deleted goes to the trash
                // here, not away.
                ItemKind::Group => {
                    store.trash_group(GroupId(uuid))?;
                    store.delete_group(GroupId(uuid))?;
                }
                ItemKind::Host => {
                    store.trash_host(HostId(uuid))?;
                    store.delete_host(HostId(uuid))?;
                }
                // A key bound to this device was never sent; no record may remove it.
                ItemKind::Key => {
                    if !store.key_record(KeyId(uuid))?.is_some_and(|r| r.kind == KeyKind::Device) {
                        store.trash_key(KeyId(uuid))?;
                        store.delete_key_record(KeyId(uuid))?;
                    }
                }
                ItemKind::Identity => store.delete_identity(IdentityId(uuid))?,
                ItemKind::Password => store.delete_password(PasswordId(uuid))?,
                ItemKind::Log => session_log::delete(store, LogId(uuid)).map_err(log_error)?,
                ItemKind::LogChunk => session_log::delete_chunk(store, item_id).map_err(log_error)?,
            }
        }
    }
    Ok(())
}

/// A log that does not fit (from an older or broken client) is a record to skip, not a failure
/// of the whole sync.
pub(crate) fn log_error(e: LogError) -> SyncError {
    match e {
        LogError::Store(e) => SyncError::Store(e.to_string()),
        other => SyncError::Proto(ProtoError::Malformed(other.to_string())),
    }
}

/// A proxy password this device had before a record came in and that is now gone (the proxy was
/// pointed elsewhere or removed) is deleted, unless something else still uses it.
fn dropped_proxy(store: &Store, before: Option<CredentialRefs>, after: &CredentialRefs) -> Result<(), SyncError> {
    match before.and_then(|b| b.proxy_password) {
        Some(old) if after.proxy_password != Some(old) => drop_unreferenced(store, &[old]),
        _ => Ok(()),
    }
}

/// "Last change wins": whether the local change is newer than the one on the server. On a tie
/// the server's state wins, so two devices always end up with the same state.
pub fn newer(local_modified: i64, remote_modified: i64) -> bool {
    local_modified > remote_modified
}

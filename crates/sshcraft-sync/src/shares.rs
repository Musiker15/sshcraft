//! Shares: an area of this account with members, holding a group with all its hosts, or copies
//! of a selection of hosts or of one host (design, section 8).

use crate::SyncError;
use crate::areas::{create_area, set_members};
use crate::contacts::require_confirmed;
use crate::engine::{Engine, from_json, lock, record_local_change};
use crate::items::{ItemBody, SyncItem, encode, local_item};
use serde::{Deserialize, Serialize};
use sshcraft_core::model::{Group, GroupId, Host, HostId, KeyId, KeyRef, ServerId};
use sshcraft_core::store::{ItemKind, RecordEntry, ShareKind, ShareRole, ShareRow, Store, copy_id};
use sshcraft_core::vault::Vault;
use sshcraft_proto::area::{AreaId, AreaKey, AreaManifest, Member, Role};
use sshcraft_proto::identity::AccountPublic;
use sshcraft_proto::record::{RecordHeader, SealedRecord};
use std::sync::Mutex;
use uuid::Uuid;

/// What a share holds, written by the owner into the share's area, so members and the owner's
/// other devices know which rules apply (a selection may only be changed, not added to).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareInfo {
    pub kind: ShareKind,
    /// The shared group on the owner's devices.
    pub group: Option<String>,
    pub with_keys: bool,
}

#[derive(Serialize, Deserialize)]
struct ShareInfoBody {
    share_info: ShareInfo,
}

/// The record id of a share's info record.
pub fn share_info_record_id(area: Uuid) -> Uuid {
    copy_id(area, area)
}

pub fn encode_share_info(info: &ShareInfo) -> Vec<u8> {
    serde_json::to_vec(&ShareInfoBody {
        share_info: info.clone(),
    })
    .expect("share info serializes")
}

/// The share info a record body holds, if it is one.
pub(crate) fn decode_share_info(body: &[u8]) -> Option<ShareInfo> {
    serde_json::from_slice::<ShareInfoBody>(body)
        .ok()
        .map(|b| b.share_info)
}

/// What to share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareWhat {
    /// The group with all its hosts, later ones included.
    GroupAll(GroupId),
    /// Copies of some hosts of a group, in a copy of the group.
    Selection { group: GroupId, hosts: Vec<HostId> },
    /// A copy of one host.
    Host(HostId),
}

/// Who gets a share, and whether they may change it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub account: AccountPublic,
    pub role: Role,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShareOutcome {
    pub area: Uuid,
    /// Hosts or groups whose key sits in the SSH agent: shared without it.
    pub agent_keys_skipped: usize,
}

fn not_allowed<T>() -> Result<T, SyncError> {
    Err(SyncError::NotAllowed)
}

/// The group and hosts a share starts with, checked: all at this server, none received from
/// someone else, a group shared "with all hosts" only once.
fn plan(
    store: &Store,
    server: ServerId,
    what: &ShareWhat,
) -> Result<(ShareKind, Option<Group>, Vec<Host>), SyncError> {
    let groups = store.list_groups()?;
    let hosts = store.list_hosts()?;
    let group_of = |id: GroupId| groups.iter().find(|g| g.id == id).cloned();
    let host_of = |id: HostId| hosts.iter().find(|h| h.id == id).cloned();
    let (kind, group, picked) = match what {
        ShareWhat::GroupAll(id) => {
            let Some(group) = group_of(*id) else {
                return not_allowed();
            };
            let members = hosts
                .iter()
                .filter(|h| h.group_id == Some(*id))
                .cloned()
                .collect();
            if let Some(area) = store.item_area(server, ItemKind::Group, &id.0.to_string())?
                && store.share(server, area)?.is_some_and(|s| s.owned)
            {
                return Err(SyncError::AlreadyShared);
            }
            (ShareKind::GroupAll, Some(group), members)
        }
        ShareWhat::Selection { group, hosts: ids } => {
            let Some(group) = group_of(*group) else {
                return not_allowed();
            };
            let mut picked = Vec::new();
            for id in ids {
                match host_of(*id) {
                    Some(host) if host.group_id == Some(group.id) => picked.push(host),
                    _ => return not_allowed(),
                }
            }
            if picked.is_empty() {
                return not_allowed();
            }
            (ShareKind::Selection, Some(group), picked)
        }
        ShareWhat::Host(id) => match host_of(*id) {
            Some(host) => (ShareKind::Host, None, vec![host]),
            None => return not_allowed(),
        },
    };
    let mut items: Vec<(ItemKind, String)> = picked
        .iter()
        .map(|h| (ItemKind::Host, h.id.0.to_string()))
        .collect();
    if let Some(group) = &group {
        items.push((ItemKind::Group, group.id.0.to_string()));
    }
    for (kind, id) in items {
        if store.location_of(kind, &id)? != Some(server)
            || store.received_share_of(server, kind, &id)?.is_some()
        {
            return not_allowed();
        }
    }
    Ok((kind, group, picked))
}

/// The stored keys the shared items use (at this server), and how many use an agent key.
fn used_keys(
    server: ServerId,
    store: &Store,
    group: &Option<Group>,
    hosts: &[Host],
) -> Result<(Vec<KeyId>, usize), SyncError> {
    let mut refs: Vec<&Option<KeyRef>> = hosts.iter().map(|h| &h.settings.key).collect();
    if let Some(group) = group {
        refs.push(&group.defaults.key);
    }
    let mut keys = Vec::new();
    let mut agent = 0;
    for key in refs.into_iter().flatten() {
        match key {
            KeyRef::Agent { .. } => agent += 1,
            KeyRef::Stored { id } => {
                // A key that came in someone else's share is theirs to share, not this account's.
                let key = id.0.to_string();
                if store.location_of(ItemKind::Key, &key)? == Some(server)
                    && store
                        .received_share_of(server, ItemKind::Key, &key)?
                        .is_none()
                    && !keys.contains(id)
                {
                    keys.push(*id);
                }
            }
        }
    }
    Ok((keys, agent))
}

/// Seals a body into an area and sends it as `record_id` at `version`.
pub(crate) async fn write_record(
    engine: &Engine,
    store: &Mutex<Store>,
    manifest: &AreaManifest,
    key: &AreaKey,
    record_id: Uuid,
    version: u64,
    body: &[u8],
) -> Result<(), SyncError> {
    let header = RecordHeader {
        record_id,
        area_id: manifest.area_id,
        version,
        key_version: manifest.key_version,
        author_account: engine.account,
        author_device: engine.device_id(),
    };
    let record = SealedRecord::seal(header, body, key, &engine.keys);
    let put = engine.client().put_record(&record).await;
    engine.revoked_check(store, put)?;
    Ok(())
}

/// The key a copy points to: the copy of a shared stored key, nothing otherwise (an agent key
/// or a key that was not shared).
fn copy_key(
    store: &Store,
    server: ServerId,
    area: Uuid,
    key: Option<KeyRef>,
) -> Result<Option<KeyRef>, SyncError> {
    let Some(KeyRef::Stored { id }) = key else {
        return Ok(None);
    };
    Ok(store
        .copies_of(server, ItemKind::Key, &id.0.to_string())?
        .into_iter()
        .find(|(a, _)| *a == area)
        .and_then(|(_, copy)| Uuid::parse_str(&copy).ok())
        .map(|copy| KeyRef::Stored { id: KeyId(copy) }))
}

/// The current content of the copy of an original in share `area`, built from the original
/// (or `Deleted` when the original is gone).
pub(crate) fn copy_item(
    store: &Store,
    vault: &Vault,
    server: ServerId,
    area: Uuid,
    kind: ItemKind,
    original: &str,
    modified_at: i64,
) -> Result<SyncItem, SyncError> {
    let original_uuid = Uuid::parse_str(original).map_err(|e| SyncError::Store(e.to_string()))?;
    let copy_uuid = copy_id(area, original_uuid);
    let copy = copy_uuid.to_string();
    let mut item = local_item(store, vault, kind, original, modified_at)?;
    match &mut item.body {
        ItemBody::Group { group } => {
            group.id = GroupId(copy_uuid);
            group.defaults.key = copy_key(store, server, area, group.defaults.key.take())?;
        }
        ItemBody::Host { host } => {
            host.id = HostId(copy_uuid);
            host.settings.key = copy_key(store, server, area, host.settings.key.take())?;
            host.group_id = match host.group_id {
                Some(group) => store
                    .copies_of(server, ItemKind::Group, &group.0.to_string())?
                    .into_iter()
                    .find(|(a, _)| *a == area)
                    .and_then(|(_, c)| Uuid::parse_str(&c).ok())
                    .map(GroupId),
                None => None,
            };
        }
        ItemBody::Key { id, .. } => *id = KeyId(copy_uuid),
        ItemBody::Deleted { item_id, .. } => *item_id = copy,
    }
    Ok(item)
}

/// Shares a group or hosts with other accounts of this server: a new area with the grants as
/// members. "All hosts" moves the group, its hosts and (with keys) their stored keys into the
/// area; a selection or one host goes as copies the owner's devices keep in step with the
/// originals. Keys from the SSH agent never go along.
pub async fn share(
    engine: &mut Engine,
    store: &Mutex<Store>,
    vault: &Vault,
    what: ShareWhat,
    grants: Vec<Grant>,
    with_keys: bool,
    now_ms: i64,
) -> Result<ShareOutcome, SyncError> {
    if grants.is_empty() {
        return not_allowed();
    }
    for grant in &grants {
        require_confirmed(store, engine.server, &grant.account)?;
    }
    let (kind, group, hosts) = plan(&*lock(store)?, engine.server, &what)?;
    let (keys, agent_keys_skipped) = used_keys(engine.server, &*lock(store)?, &group, &hosts)?;
    let members = grants
        .iter()
        .map(|g| Member {
            account: g.account,
            role: g.role,
        })
        .collect();
    let (manifest, key) = create_area(engine, store, members).await?;
    let area = manifest.area_id.0;
    let info = ShareInfo {
        kind,
        group: group.as_ref().map(|g| g.id.0.to_string()),
        with_keys,
    };
    write_record(
        engine,
        store,
        &manifest,
        &key,
        share_info_record_id(area),
        1,
        &encode_share_info(&info),
    )
    .await?;
    lock(store)?.put_share(
        engine.server,
        &ShareRow {
            area_id: area,
            kind,
            group_id: info.group.clone(),
            with_keys,
            owned: true,
            role: None,
            owner: engine.account.0,
        },
    )?;
    let server = engine.server;
    if kind == ShareKind::GroupAll {
        let group = group.expect("a group share has a group");
        {
            let guard = lock(store)?;
            let group_id = group.id.0.to_string();
            guard.set_item_area(server, ItemKind::Group, &group_id, area)?;
            record_local_change(&guard, ItemKind::Group, &group_id, now_ms)?;
            for host in &hosts {
                let id = host.id.0.to_string();
                guard.clear_item_area(server, ItemKind::Host, &id)?;
                record_local_change(&guard, ItemKind::Host, &id, now_ms)?;
            }
            if with_keys {
                for key_id in &keys {
                    let id = key_id.0.to_string();
                    guard.set_item_area(server, ItemKind::Key, &id, area)?;
                    record_local_change(&guard, ItemKind::Key, &id, now_ms)?;
                }
            }
        }
        engine.push(store, vault, now_ms).await?;
    } else {
        // The copies: keys first, then the group, then the hosts, so a member can apply each
        // as it comes.
        let mut originals: Vec<(ItemKind, String)> = Vec::new();
        if with_keys {
            originals.extend(keys.iter().map(|k| (ItemKind::Key, k.0.to_string())));
        }
        if kind == ShareKind::Selection
            && let Some(group) = &group
        {
            originals.push((ItemKind::Group, group.id.0.to_string()));
        }
        originals.extend(hosts.iter().map(|h| (ItemKind::Host, h.id.0.to_string())));
        for (item_kind, original) in &originals {
            let original_uuid =
                Uuid::parse_str(original).map_err(|e| SyncError::Store(e.to_string()))?;
            let copy = copy_id(area, original_uuid).to_string();
            lock(store)?.put_copy(server, area, *item_kind, original, &copy)?;
        }
        for (item_kind, original) in &originals {
            let original_uuid =
                Uuid::parse_str(original).map_err(|e| SyncError::Store(e.to_string()))?;
            let copy = copy_id(area, original_uuid).to_string();
            let item = {
                let guard = lock(store)?;
                copy_item(&guard, vault, server, area, *item_kind, original, now_ms)?
            };
            let record_id = Uuid::new_v4();
            write_record(
                engine,
                store,
                &manifest,
                &key,
                record_id,
                1,
                &encode(&item)?,
            )
            .await?;
            lock(store)?.put_record_entry(
                server,
                &RecordEntry {
                    record_id,
                    area_id: area,
                    version: 1,
                    modified_at: now_ms,
                },
                *item_kind,
                &copy,
            )?;
        }
    }
    Ok(ShareOutcome {
        area,
        agent_keys_skipped,
    })
}

/// After taking someone out: whether keys went along, so the user can be told to change them
/// (they may have been copied, design section 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RevokeOutcome {
    pub keys_were_shared: bool,
}

/// One share as the settings show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShareView {
    pub area: Uuid,
    pub kind: ShareKind,
    /// The shared group (own shares: the original's id).
    pub group: Option<String>,
    /// The shared hosts (own shares: the originals' ids).
    pub hosts: Vec<String>,
    pub grants: Vec<GrantView>,
    pub with_keys: bool,
    pub owned: bool,
    pub owner: AccountPublic,
    /// This account's role in a received share.
    pub role: Option<ShareRole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GrantView {
    pub account: AccountPublic,
    pub role: Role,
    /// From the contacts, when this device looked the account up.
    pub email: Option<String>,
    pub name: Option<String>,
}

fn owned_share(store: &Mutex<Store>, server: ServerId, area: Uuid) -> Result<ShareRow, SyncError> {
    match lock(store)?.share(server, area)? {
        Some(share) if share.owned => Ok(share),
        _ => not_allowed(),
    }
}

/// Changes who a share goes to and with which role. Someone taken out gets no key for what
/// follows (rotation); the others need not do anything. No grants left ends the share.
pub async fn set_grants(
    engine: &mut Engine,
    store: &Mutex<Store>,
    vault: &Vault,
    area: Uuid,
    grants: Vec<Grant>,
    now_ms: i64,
) -> Result<RevokeOutcome, SyncError> {
    let share = owned_share(store, engine.server, area)?;
    if grants.is_empty() {
        return unshare(engine, store, vault, area, now_ms).await;
    }
    let (manifest, _) = engine.stored_area(store, area)?;
    for grant in &grants {
        if !manifest.members.iter().any(|m| m.account == grant.account) {
            require_confirmed(store, engine.server, &grant.account)?;
        }
    }
    let removed = manifest
        .members
        .iter()
        .any(|m| !grants.iter().any(|g| g.account == m.account));
    let members = grants
        .iter()
        .map(|g| Member {
            account: g.account,
            role: g.role,
        })
        .collect();
    set_members(engine, store, vault, AreaId(area), members, now_ms / 1000).await?;
    Ok(RevokeOutcome {
        keys_were_shared: removed && share.with_keys,
    })
}

/// Ends a share. A group shared with all its hosts goes back into the personal area; copies
/// are deleted with the share's area.
pub async fn unshare(
    engine: &mut Engine,
    store: &Mutex<Store>,
    vault: &Vault,
    area: Uuid,
    now_ms: i64,
) -> Result<RevokeOutcome, SyncError> {
    let share = owned_share(store, engine.server, area)?;
    let server = engine.server;
    if share.kind == ShareKind::GroupAll {
        {
            let guard = lock(store)?;
            let mut items = guard.items_in_area(server, area)?;
            for host in guard.list_hosts()? {
                if host
                    .group_id
                    .is_some_and(|g| Some(g.0.to_string()) == share.group_id)
                {
                    items.push((ItemKind::Host, host.id.0.to_string()));
                }
            }
            for (kind, id) in &items {
                guard.clear_item_area(server, *kind, id)?;
            }
            for (kind, id) in &items {
                record_local_change(&guard, *kind, id, now_ms)?;
            }
        }
        engine.push(store, vault, now_ms).await?;
    }
    let deleted = engine.client().delete_area(AreaId(area)).await;
    match deleted {
        Ok(()) | Err(SyncError::Server { status: 404, .. }) => {}
        Err(err) => return engine.revoked_check(store, Err(err)),
    }
    lock(store)?.forget_area(server, area)?;
    Ok(RevokeOutcome {
        keys_were_shared: share.with_keys,
    })
}

/// Every share this device knows on `server`, own and received.
pub fn list(store: &Mutex<Store>, server: ServerId) -> Result<Vec<ShareView>, SyncError> {
    let guard = lock(store)?;
    let areas = guard.areas(server)?;
    let mut views = Vec::new();
    for share in guard.shares(server)? {
        let Some(state) = areas.iter().find(|a| a.area_id == share.area_id) else {
            continue;
        };
        let manifest: AreaManifest = from_json(&state.manifest)?;
        let hosts = if share.owned && share.kind != ShareKind::GroupAll {
            guard
                .copies_in(server, share.area_id)?
                .into_iter()
                .filter(|(kind, _, _)| *kind == ItemKind::Host)
                .map(|(_, original, _)| original)
                .collect()
        } else if share.kind == ShareKind::GroupAll && share.owned {
            guard
                .list_hosts()?
                .into_iter()
                .filter(|h| {
                    h.group_id
                        .is_some_and(|g| Some(g.0.to_string()) == share.group_id)
                })
                .map(|h| h.id.0.to_string())
                .collect()
        } else {
            guard
                .list_hosts()?
                .into_iter()
                .map(|h| h.id.0.to_string())
                .filter(|id| {
                    guard
                        .effective_area(server, ItemKind::Host, id)
                        .ok()
                        .flatten()
                        == Some(share.area_id)
                })
                .collect()
        };
        let grants = manifest
            .members
            .iter()
            .map(|m| {
                let contact = guard
                    .contact_by_account(server, &m.account.0)
                    .ok()
                    .flatten();
                GrantView {
                    account: m.account,
                    role: m.role,
                    email: contact.as_ref().map(|c| c.email.clone()),
                    name: contact.map(|c| c.name),
                }
            })
            .collect();
        views.push(ShareView {
            area: share.area_id,
            kind: share.kind,
            group: share.group_id.clone(),
            hosts,
            grants,
            with_keys: share.with_keys,
            owned: share.owned,
            owner: AccountPublic(share.owner),
            role: share.role,
        });
    }
    Ok(views)
}

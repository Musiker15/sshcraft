//! Shares: an area of this account with members, holding a group with all its hosts, or copies
//! of a selection of hosts or of one host (design, section 8).

use crate::SyncError;
use crate::areas::{create_area, set_members};
use crate::contacts::require_confirmed;
use crate::engine::{Engine, from_json, lock, record_local_change};
use crate::items::{ItemBody, SyncItem, encode, local_item};
use serde::{Deserialize, Serialize};
use sshcraft_core::model::{
    ConnectionSettings, Group, GroupId, Host, HostId, IdentityId, KeyId, KeyRef, PasswordId, Proxy,
    ServerId, flatten, group_chain,
};
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

/// Which key the owner uses for an item in one of its shares when that key is not shared: kept
/// in the owner's personal area, so the owner's devices agree while members never see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyAssignment {
    pub kind: ItemKind,
    pub item: String,
    pub key: Option<KeyRef>,
    #[serde(default)]
    pub password: Option<sshcraft_core::model::PasswordId>,
    #[serde(default)]
    pub identity: Option<sshcraft_core::model::IdentityId>,
    #[serde(default)]
    pub proxy_password: Option<sshcraft_core::model::PasswordId>,
    pub modified_at: i64,
}

impl KeyAssignment {
    pub fn refs(&self) -> crate::creds::CredentialRefs {
        crate::creds::CredentialRefs {
            key: self.key.clone(),
            password: self.password,
            identity: self.identity,
            proxy_password: self.proxy_password,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct KeyAssignmentBody {
    key_assignment: KeyAssignment,
}

pub(crate) fn encode_key_assignment(assignment: &KeyAssignment) -> Vec<u8> {
    serde_json::to_vec(&KeyAssignmentBody {
        key_assignment: assignment.clone(),
    })
    .expect("key assignment serializes")
}

pub(crate) fn decode_key_assignment(body: &[u8]) -> Option<KeyAssignment> {
    serde_json::from_slice::<KeyAssignmentBody>(body)
        .ok()
        .map(|b| b.key_assignment)
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
            // Members would not know the groups above it, nor what they pass down.
            if group.parent_id.is_some() {
                return Err(SyncError::NestedGroupAll);
            }
            // The group's hosts at every level.
            let mut tree = store.subgroups(*id)?;
            tree.push(*id);
            let members = hosts
                .iter()
                .filter(|h| h.group_id.is_some_and(|g| tree.contains(&g)))
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
            let mut tree = store.subgroups(group.id)?;
            tree.push(group.id);
            let mut picked = Vec::new();
            for id in ids {
                match host_of(*id) {
                    Some(host) if host.group_id.is_some_and(|g| tree.contains(&g)) => {
                        picked.push(host)
                    }
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

/// What the shared items use and this account may share along (at this server, not from
/// someone else's share): stored keys, passwords (also of proxies) and identities, the keys and
/// passwords of those identities included; and how many references point to an agent key.
#[derive(Default)]
struct Credentials {
    keys: Vec<KeyId>,
    passwords: Vec<PasswordId>,
    identities: Vec<IdentityId>,
    agent: usize,
}

/// The credentials of the hosts' own settings and of every group a shared host takes values
/// from (all groups of a tree shared with all hosts; for copies every group above the host, as
/// those values are written into the copies).
fn used_credentials(
    server: ServerId,
    store: &Store,
    kind: ShareKind,
    group: &Option<Group>,
    hosts: &[Host],
) -> Result<Credentials, SyncError> {
    let all_groups = store.list_groups()?;
    let mut groups: Vec<Group> = Vec::new();
    match (kind, group) {
        (ShareKind::GroupAll, Some(root)) => {
            let mut tree = store.subgroups(root.id)?;
            tree.push(root.id);
            groups.extend(all_groups.iter().filter(|g| tree.contains(&g.id)).cloned());
        }
        _ => {
            for host in hosts {
                if let Some(start) = host.group_id {
                    for g in group_chain(&all_groups, start) {
                        if !groups.iter().any(|known| known.id == g.id) {
                            groups.push(g);
                        }
                    }
                }
            }
            if let Some(root) = group {
                for g in group_chain(&all_groups, root.id) {
                    if !groups.iter().any(|known| known.id == g.id) {
                        groups.push(g);
                    }
                }
            }
        }
    }
    // An item that came in someone else's share is theirs to share, not this account's.
    let own = |kind: ItemKind, id: &str| -> Result<bool, SyncError> {
        Ok(store.location_of(kind, id)? == Some(server)
            && store.received_share_of(server, kind, id)?.is_none())
    };
    let identities = store.list_identities()?;
    let mut out = Credentials::default();
    let mut keys: Vec<KeyRef> = Vec::new();
    let mut passwords: Vec<PasswordId> = Vec::new();
    let settings = hosts
        .iter()
        .map(|h| &h.settings)
        .chain(groups.iter().map(|g| &g.defaults));
    for s in settings {
        keys.extend(s.key.clone());
        passwords.extend(s.password_ids());
        if let Some(id) = s.identity
            && own(ItemKind::Identity, &id.0.to_string())?
            && !out.identities.contains(&id)
        {
            out.identities.push(id);
            if let Some(identity) = identities.iter().find(|i| i.id == id) {
                keys.extend(identity.key.clone());
                passwords.extend(identity.password);
            }
        }
    }
    for key in keys {
        match key {
            KeyRef::Agent { .. } => out.agent += 1,
            KeyRef::Stored { id } => {
                if own(ItemKind::Key, &id.0.to_string())? && !out.keys.contains(&id) {
                    out.keys.push(id);
                }
            }
        }
    }
    for id in passwords {
        if own(ItemKind::Password, &id.0.to_string())? && !out.passwords.contains(&id) {
            out.passwords.push(id);
        }
    }
    Ok(out)
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

/// The copy in share `area` of the item `id`, if there is one.
fn copy_in(
    store: &Store,
    server: ServerId,
    area: Uuid,
    kind: ItemKind,
    id: Uuid,
) -> Result<Option<Uuid>, SyncError> {
    Ok(store
        .copies_of(server, kind, &id.to_string())?
        .into_iter()
        .find(|(a, _)| *a == area)
        .and_then(|(_, copy)| Uuid::parse_str(&copy).ok()))
}

/// The references a copy carries: the copies of shared stored keys and passwords, nothing for
/// what was not shared (an agent key, a key or password shared without keys), no identity (the
/// copy carries its values), and jump hosts pointing to their copies where there are some.
fn to_copy_refs(
    store: &Store,
    server: ServerId,
    area: Uuid,
    settings: &mut ConnectionSettings,
) -> Result<(), SyncError> {
    let password = |p: Option<PasswordId>| -> Result<Option<PasswordId>, SyncError> {
        Ok(match p {
            Some(p) => copy_in(store, server, area, ItemKind::Password, p.0)?.map(PasswordId),
            None => None,
        })
    };
    settings.key = match settings.key.take() {
        Some(KeyRef::Stored { id }) => copy_in(store, server, area, ItemKind::Key, id.0)?
            .map(|copy| KeyRef::Stored { id: KeyId(copy) }),
        _ => None,
    };
    settings.password = password(settings.password)?;
    if let Some(Proxy::Http(e) | Proxy::Socks5(e)) = &mut settings.proxy {
        e.password = password(e.password)?;
    }
    settings.identity = None;
    if let Some(jumps) = &mut settings.jump_hosts {
        for jump in jumps.iter_mut() {
            if let Some(copy) = copy_in(store, server, area, ItemKind::Host, jump.0)? {
                *jump = HostId(copy);
            }
        }
    }
    Ok(())
}

/// The settings a host's copy starts from: its own and those of the groups between its group
/// and the shared group `shared` (its own group counts, the shared group does not; with nothing
/// shared as a group, the whole chain), identities resolved.
fn host_copy_settings(
    store: &Store,
    host: &Host,
    shared: Option<&str>,
) -> Result<ConnectionSettings, SyncError> {
    let groups = store.list_groups()?;
    let chain = match host.group_id {
        Some(start) => group_chain(&groups, start),
        None => Vec::new(),
    };
    let mut levels = vec![&host.settings];
    for group in &chain {
        if shared.is_some_and(|shared| group.id.0.to_string() == shared) {
            break;
        }
        levels.push(&group.defaults);
    }
    Ok(flatten(&levels, &store.list_identities()?))
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
            // The copy is a top-level group for members: it carries what the original takes
            // from the groups above it and from identities.
            let groups = store.list_groups()?;
            let chain = group_chain(&groups, group.id);
            let levels: Vec<&ConnectionSettings> = chain.iter().map(|g| &g.defaults).collect();
            group.defaults = flatten(&levels, &store.list_identities()?);
            group.parent_id = None;
            group.id = GroupId(copy_uuid);
            to_copy_refs(store, server, area, &mut group.defaults)?;
        }
        ItemBody::Host { host } => {
            // A host from a subgroup lands flat in the copy of the shared group, with what it
            // inherits from the groups in between written into it.
            let shared = store.share(server, area)?.and_then(|s| s.group_id);
            host.settings = host_copy_settings(store, host, shared.as_deref())?;
            to_copy_refs(store, server, area, &mut host.settings)?;
            host.id = HostId(copy_uuid);
            host.group_id = match (host.group_id, shared) {
                (Some(_), Some(group)) => store
                    .copies_of(server, ItemKind::Group, &group)?
                    .into_iter()
                    .find(|(a, _)| *a == area)
                    .and_then(|(_, c)| Uuid::parse_str(&c).ok())
                    .map(GroupId),
                _ => None,
            };
        }
        ItemBody::Key { id, .. } => *id = KeyId(copy_uuid),
        ItemBody::Identity { identity } => identity.id = IdentityId(copy_uuid),
        ItemBody::Password { id, .. } => *id = PasswordId(copy_uuid),
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
    let credentials = used_credentials(engine.server, &*lock(store)?, kind, &group, &hosts)?;
    let agent_keys_skipped = credentials.agent;
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
            // Subgroups and all hosts below follow the group into the share.
            for sub in guard.subgroups(group.id)? {
                guard.clear_item_area(server, ItemKind::Group, &sub.0.to_string())?;
            }
            for host in &hosts {
                guard.clear_item_area(server, ItemKind::Host, &host.id.0.to_string())?;
            }
            crate::engine::record_group_hosts(&guard, &group_id, now_ms)?;
            // Identities always go along (without them members lack the user); keys and
            // passwords only with keys. Without, the identities go without theirs.
            let mut along: Vec<(ItemKind, String)> = credentials
                .identities
                .iter()
                .map(|i| (ItemKind::Identity, i.0.to_string()))
                .collect();
            if with_keys {
                along.extend(
                    credentials
                        .keys
                        .iter()
                        .map(|k| (ItemKind::Key, k.0.to_string())),
                );
                along.extend(
                    credentials
                        .passwords
                        .iter()
                        .map(|p| (ItemKind::Password, p.0.to_string())),
                );
            }
            for (item_kind, id) in &along {
                guard.set_item_area(server, *item_kind, id, area)?;
                record_local_change(&guard, *item_kind, id, now_ms)?;
            }
        }
        engine.push(store, vault, now_ms).await?;
    } else {
        // The copies: keys and passwords first, then the group, then the hosts, so a member
        // can apply each as it comes. Identities are not copied: the copies carry their values.
        let mut originals: Vec<(ItemKind, String)> = Vec::new();
        if with_keys {
            originals.extend(
                credentials
                    .keys
                    .iter()
                    .map(|k| (ItemKind::Key, k.0.to_string())),
            );
            originals.extend(
                credentials
                    .passwords
                    .iter()
                    .map(|p| (ItemKind::Password, p.0.to_string())),
            );
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
            let items = guard.items_in_area(server, area)?;
            for (kind, id) in &items {
                guard.clear_item_area(server, *kind, id)?;
            }
            for (kind, id) in &items {
                record_local_change(&guard, *kind, id, now_ms)?;
            }
            // Everything below the group goes back with it.
            if let Some(group) = &share.group_id {
                crate::engine::record_group_hosts(&guard, group, now_ms)?;
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

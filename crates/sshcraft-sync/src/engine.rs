//! Syncing one server: fetch what changed (pull) and send what changed here (push).
//!
//! Everything fetched is verified first: the account's device log must extend the head this
//! device saw before, manifests and records go through the `VersionTracker`, envelopes are
//! opened against the verified manifest. What fails is reported and skipped, never applied.

use crate::SyncError;
use crate::account::device_keys;
use crate::client::ServerClient;
use crate::items::{ItemBody, SyncItem, apply_in, decode, encode, local_item, newer};
use crate::shares::{
    KeyAssignment, ShareInfo, copy_item, decode_key_assignment, decode_share_info,
    encode_key_assignment, share_info_record_id,
};
use sshcraft_core::model::KeyRef;
use sshcraft_core::model::ServerId;
use sshcraft_core::store::{
    ItemKind, RecordEntry, ServerRow, ShareKind, ShareRole, ShareRow, Store,
};
use sshcraft_core::vault::Vault;
use sshcraft_proto::area::{AreaId, AreaKey, AreaManifest, KeyEnvelope, Role, open_area_key};
use sshcraft_proto::devicelog::{DeviceLog, DeviceSet, LogHead};
use sshcraft_proto::identity::{AccountPublic, DeviceKeys};
use sshcraft_proto::record::{Freshness, RecordHeader, SealedRecord, VersionTracker};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

/// Records fetched per request.
pub const PAGE: u32 = 500;
/// How often one item is retried after a conflict within one push.
pub const CONFLICT_ROUNDS: usize = 3;

/// Queues a local change for the server the item lives at (nothing for a local item). Call it
/// before deleting an item: afterwards its location is gone. A group's hosts are not queued with
/// it: their own last change must stay what "last change wins" compares (see
/// `record_group_hosts` for moving or deleting a group).
pub fn record_local_change(
    store: &Store,
    kind: ItemKind,
    id: &str,
    now_ms: i64,
) -> Result<(), SyncError> {
    let Some(server) = store.location_of(kind, id)? else {
        return Ok(());
    };
    store.queue(server, kind, id, now_ms)?;
    // A group that changed area (hung into or out of a share) takes everything below along.
    if kind == ItemKind::Group
        && let Some(personal) = store
            .get_setting(&crate::account::personal_setting(server))?
            .and_then(|t| Uuid::parse_str(&t).ok())
        && let Some(entry) = store.record_entry(server, ItemKind::Group, id)?
        && entry.area_id != store.area_for(server, ItemKind::Group, id, personal)?
    {
        record_group_hosts(store, id, now_ms)?;
    }
    // Its copies in shares follow it.
    for (_, copy) in store.copies_of(server, kind, id)? {
        store.queue(server, kind, &copy, now_ms)?;
    }
    // Copies below a group carry values from it: they follow too.
    if kind == ItemKind::Group
        && let Ok(uuid) = Uuid::parse_str(id)
    {
        let mut tree: Vec<String> = store
            .subgroups(sshcraft_core::model::GroupId(uuid))?
            .iter()
            .map(|g| g.0.to_string())
            .collect();
        for group in &tree {
            for (_, copy) in store.copies_of(server, ItemKind::Group, group)? {
                store.queue(server, ItemKind::Group, &copy, now_ms)?;
            }
        }
        tree.push(id.to_owned());
        for host in store.list_hosts()? {
            if host
                .group_id
                .is_some_and(|g| tree.contains(&g.0.to_string()))
            {
                for (_, copy) in store.copies_of(server, ItemKind::Host, &host.id.0.to_string())? {
                    store.queue(server, ItemKind::Host, &copy, now_ms)?;
                }
            }
        }
    }
    Ok(())
}

/// Queues every subgroup and every host below a group, for when the group moves or is deleted
/// (all of it goes with it).
pub fn record_group_hosts(store: &Store, group_id: &str, now_ms: i64) -> Result<(), SyncError> {
    let id = Uuid::parse_str(group_id).map_err(|e| SyncError::Store(e.to_string()))?;
    let mut groups = vec![group_id.to_owned()];
    for sub in store.subgroups(sshcraft_core::model::GroupId(id))? {
        let sub = sub.0.to_string();
        record_local_change(store, ItemKind::Group, &sub, now_ms)?;
        groups.push(sub);
    }
    for host in store.list_hosts()? {
        if host
            .group_id
            .is_some_and(|g| groups.contains(&g.0.to_string()))
        {
            record_local_change(store, ItemKind::Host, &host.id.0.to_string(), now_ms)?;
        }
    }
    Ok(())
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PullReport {
    pub applied: usize,
    /// Records or areas that did not verify: id and reason.
    pub rejected: Vec<String>,
}

/// One lock per server: a sync pass and a share or device command on the same server run one
/// after the other, so they never work on the same rotation or listing at once.
pub fn server_lock(server: ServerId) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    static LOCKS: std::sync::OnceLock<
        Mutex<HashMap<ServerId, std::sync::Arc<tokio::sync::Mutex<()>>>>,
    > = std::sync::OnceLock::new();
    let locks = LOCKS.get_or_init(Mutex::default);
    let mut map = locks.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(server).or_default().clone()
}

/// A logged-in connection to one server, with this device's keys.
pub struct Engine {
    pub server: ServerId,
    pub(crate) client: ServerClient,
    pub(crate) device_id: Uuid,
    pub(crate) keys: DeviceKeys,
    pub(crate) account: AccountPublic,
    warnings: Vec<String>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

pub(crate) fn lock(store: &Mutex<Store>) -> Result<MutexGuard<'_, Store>, SyncError> {
    store.lock().map_err(|e| SyncError::Store(e.to_string()))
}

pub(crate) fn to_json(value: &impl serde::Serialize) -> Result<String, SyncError> {
    serde_json::to_string(value).map_err(|e| SyncError::Store(e.to_string()))
}

pub(crate) fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, SyncError> {
    serde_json::from_str(text).map_err(|e| SyncError::Store(e.to_string()))
}

impl Engine {
    /// Opens the device keys and logs in. A device the server reports as revoked is marked so
    /// locally and stops here.
    pub async fn load(
        store: &Mutex<Store>,
        vault: &Vault,
        server: ServerId,
    ) -> Result<Engine, SyncError> {
        let row: ServerRow = lock(store)?.server(server)?.ok_or(SyncError::NotSetUp)?;
        if row.revoked {
            return Err(SyncError::Revoked);
        }
        let keys = device_keys(vault, &row)?;
        let mut client = ServerClient::new(&row.origin)?;
        let mut engine = Engine {
            server,
            client: client.clone(),
            device_id: row.device_id,
            keys,
            account: AccountPublic(row.account),
            warnings: Vec::new(),
        };
        let login = client.login(row.device_id, &engine.keys).await;
        engine.revoked_check(store, login)?;
        engine.client = client;
        Ok(engine)
    }

    pub fn client(&self) -> &ServerClient {
        &self.client
    }

    pub fn device_id(&self) -> Uuid {
        self.device_id
    }

    /// Turns "this device is inactive" from the server into `Revoked` and remembers it.
    pub(crate) fn revoked_check<T>(
        &self,
        store: &Mutex<Store>,
        result: Result<T, SyncError>,
    ) -> Result<T, SyncError> {
        match result {
            Err(SyncError::Server { code, .. }) if code == "device_inactive" => {
                lock(store)?.mark_server_revoked(self.server)?;
                Err(SyncError::Revoked)
            }
            Err(SyncError::Server { code, .. }) if code == "device_blocked" => {
                Err(SyncError::Blocked)
            }
            other => other,
        }
    }

    /// The account's device log, verified against the head this device accepted before.
    pub(crate) async fn own_devices(
        &self,
        store: &Mutex<Store>,
    ) -> Result<(DeviceSet, DeviceLog), SyncError> {
        let entries = self.revoked_check(store, self.client.log(&self.account, 0).await)?;
        let log = DeviceLog { entries };
        let known: Option<LogHead> = lock(store)?
            .server(self.server)?
            .and_then(|row| row.log_head)
            .map(|text| from_json(&text))
            .transpose()?;
        let set = log.verify_extends(&self.account, known.as_ref())?;
        if set.get_active(self.device_id).is_none() {
            lock(store)?.mark_server_revoked(self.server)?;
            return Err(SyncError::Revoked);
        }
        Ok((set, log))
    }

    /// Another account's device log, verified against the head this device accepted before
    /// for that account (a server cannot roll it back); this account's own goes through
    /// `own_devices`.
    pub async fn account_devices(
        &self,
        store: &Mutex<Store>,
        account: &AccountPublic,
    ) -> Result<DeviceSet, SyncError> {
        Ok(self.account_log(store, account).await?.0)
    }

    fn head_setting(&self, account: &AccountPublic) -> String {
        format!(
            "sync.head.{}.{}",
            self.server.0,
            data_encoding::BASE64URL_NOPAD.encode(&account.0)
        )
    }

    /// The head of another account's log this device accepted last, if any.
    pub(crate) fn known_head(
        &self,
        store: &Mutex<Store>,
        account: &AccountPublic,
    ) -> Result<Option<LogHead>, SyncError> {
        match lock(store)?.get_setting(&self.head_setting(account))? {
            Some(text) if !text.is_empty() => Ok(Some(from_json(&text)?)),
            _ => Ok(None),
        }
    }

    /// [`Engine::account_devices`] with the log itself.
    pub(crate) async fn account_log(
        &self,
        store: &Mutex<Store>,
        account: &AccountPublic,
    ) -> Result<(DeviceSet, DeviceLog), SyncError> {
        if *account == self.account {
            return self.own_devices(store).await;
        }
        let entries = self.revoked_check(store, self.client.log(account, 0).await)?;
        let log = DeviceLog { entries };
        let known = self.known_head(store, account)?;
        let set = log.verify_extends(account, known.as_ref())?;
        if let Some(head) = set.head() {
            lock(store)?.put_setting(&self.head_setting(account), &to_json(&head)?)?;
        }
        Ok((set, log))
    }

    /// The stored, verified manifest of an area and its key, opened with this device's envelope.
    pub(crate) fn stored_area(
        &self,
        store: &Mutex<Store>,
        area: Uuid,
    ) -> Result<(AreaManifest, AreaKey), SyncError> {
        let state = lock(store)?
            .areas(self.server)?
            .into_iter()
            .find(|a| a.area_id == area)
            .ok_or(SyncError::NotSetUp)?;
        let manifest: AreaManifest = from_json(&state.manifest)?;
        let envelope: KeyEnvelope = from_json(&state.envelope)?;
        let key = open_area_key(&envelope, self.device_id, &self.keys, &manifest)?;
        Ok((manifest, key))
    }

    /// Every stored area this account owns, with its key.
    pub(crate) fn owned_areas(
        &self,
        store: &Mutex<Store>,
    ) -> Result<Vec<(AreaManifest, AreaKey)>, SyncError> {
        let ids: Vec<Uuid> = lock(store)?
            .areas(self.server)?
            .into_iter()
            .map(|a| a.area_id)
            .collect();
        let mut owned = Vec::new();
        for id in ids {
            let (manifest, key) = self.stored_area(store, id)?;
            if manifest.owner == self.account {
                owned.push((manifest, key));
            }
        }
        Ok(owned)
    }

    pub(crate) fn tracker(&self, store: &Mutex<Store>) -> Result<VersionTracker, SyncError> {
        Ok(lock(store)?
            .server(self.server)?
            .and_then(|row| row.tracker)
            .map(|text| from_json(&text))
            .transpose()?
            .unwrap_or_default())
    }

    pub(crate) fn save_state(
        &self,
        store: &Mutex<Store>,
        devices: &DeviceSet,
        tracker: &VersionTracker,
    ) -> Result<(), SyncError> {
        let head = devices
            .head()
            .map(|h| to_json(&h))
            .transpose()?
            .unwrap_or_default();
        lock(store)?.save_server_state(self.server, &head, &to_json(tracker)?)?;
        Ok(())
    }

    /// Fetches and verifies an area's manifest and this device's key for it, and stores both.
    pub(crate) async fn open_area(
        &self,
        store: &Mutex<Store>,
        manifest: AreaManifest,
        devices: &DeviceSet,
        tracker: &mut VersionTracker,
    ) -> Result<Option<(AreaManifest, AreaKey)>, SyncError> {
        tracker.accept_manifest(&manifest, devices)?;
        let envelope =
            self.revoked_check(store, self.client.my_envelope(manifest.area_id).await)?;
        let Some(envelope) = envelope else {
            return Ok(None);
        };
        let key = open_area_key(&envelope, self.device_id, &self.keys, &manifest)?;
        lock(store)?.put_area(
            self.server,
            manifest.area_id.0,
            &to_json(&manifest)?,
            &to_json(&envelope)?,
        )?;
        Ok(Some((manifest, key)))
    }

    /// The verified devices of `account`, fetched once per pull.
    async fn devices_cached(
        &self,
        store: &Mutex<Store>,
        cache: &mut HashMap<AccountPublic, DeviceSet>,
        account: &AccountPublic,
    ) -> Result<DeviceSet, SyncError> {
        if let Some(set) = cache.get(account) {
            return Ok(set.clone());
        }
        let set = self.account_devices(store, account).await?;
        cache.insert(*account, set.clone());
        Ok(set)
    }

    /// Shares the server no longer lists. A received one means this account was removed or the
    /// share ended: its items go (design, section 8, the recipient loses access). An own one
    /// was ended on another device and only loses its bookkeeping here.
    fn drop_lost_shares(
        &self,
        store: &Mutex<Store>,
        listed: &HashSet<Uuid>,
    ) -> Result<(), SyncError> {
        let guard = lock(store)?;
        for share in guard.shares(self.server)? {
            if listed.contains(&share.area_id) {
                continue;
            }
            if !share.owned {
                for (kind, id) in guard.items_in_area(self.server, share.area_id)? {
                    remove_item(&guard, kind, &id)?;
                }
            }
            guard.forget_area(self.server, share.area_id)?;
        }
        Ok(())
    }

    /// Remembers what this device learns about a share from its verified manifest.
    fn note_share(&self, store: &Mutex<Store>, manifest: &AreaManifest) -> Result<(), SyncError> {
        let personal = self.personal_id(store)?;
        let guard = lock(store)?;
        let owned = manifest.owner == self.account;
        if owned && Some(manifest.area_id.0) == personal {
            return Ok(());
        }
        let role = if owned {
            None
        } else {
            match manifest.members.iter().find(|m| m.account == self.account) {
                Some(m) => Some(match m.role {
                    Role::Read => ShareRole::Read,
                    Role::Write => ShareRole::Write,
                }),
                None => return Ok(()),
            }
        };
        let existing = guard.share(self.server, manifest.area_id.0)?;
        guard.put_share(
            self.server,
            &ShareRow {
                area_id: manifest.area_id.0,
                // Until the owner's share info arrives, the strictest rules apply.
                kind: existing.as_ref().map_or(ShareKind::Selection, |s| s.kind),
                group_id: existing.as_ref().and_then(|s| s.group_id.clone()),
                with_keys: existing.as_ref().is_some_and(|s| s.with_keys),
                owned,
                role,
                owner: manifest.owner.0,
            },
        )?;
        Ok(())
    }

    fn note_share_info(
        &self,
        store: &Mutex<Store>,
        manifest: &AreaManifest,
        info: ShareInfo,
    ) -> Result<(), SyncError> {
        self.note_share(store, manifest)?;
        let guard = lock(store)?;
        if let Some(share) = guard.share(self.server, manifest.area_id.0)? {
            guard.put_share(
                self.server,
                &ShareRow {
                    kind: info.kind,
                    group_id: info.group,
                    with_keys: info.with_keys,
                    ..share
                },
            )?;
        }
        Ok(())
    }

    /// Fetches everything that changed on the server and applies what verifies: the personal
    /// area, own shares and shares of other accounts this account is a member of.
    pub async fn pull(
        &mut self,
        store: &Mutex<Store>,
        vault: &Vault,
    ) -> Result<PullReport, SyncError> {
        let mut report = PullReport::default();
        // A key rotation that was interrupted is finished before anything else.
        crate::areas::resume_rotation(self, store, vault, now_secs()).await?;
        let (devices, _) = self.own_devices(store).await?;
        let mut tracker = self.tracker(store)?;
        let mut cache = HashMap::from([(self.account, devices.clone())]);
        let mut listings = self.revoked_check(store, self.client.areas().await)?;
        // Areas the server handed to this account because their owner was deleted (design,
        // section 12): this device signs them anew as their owner.
        let mut taken = false;
        for listing in &listings {
            if listing.owner == self.account && listing.manifest.owner != self.account {
                if self.take_over(store, vault, listing).await? {
                    taken = true;
                } else {
                    report.rejected.push(format!(
                        "area {}: not this account's to take over",
                        listing.manifest.area_id.0
                    ));
                }
            }
        }
        if taken {
            listings = self.revoked_check(store, self.client.areas().await)?;
        }
        let listed: HashSet<Uuid> = listings.iter().map(|l| l.manifest.area_id.0).collect();
        self.drop_lost_shares(store, &listed)?;
        let personal = self.settle_personal(store).await?;
        let sync_keys = lock(store)?
            .server(self.server)?
            .is_some_and(|row| row.sync_keys);
        // Key assignments of own shared items, applied once their items are here.
        let mut assignments = Vec::new();
        // The personal area first: originals are there before their copies in own shares.
        listings.sort_by_key(|l| Some(l.manifest.area_id.0) != personal);
        for listing in listings {
            let area_id = listing.manifest.area_id;
            let owner = listing.manifest.owner;
            // An area of this account that the server now gives someone else, while this account
            // exists: a server taking it away. Refused and reported.
            if let Ok((old, _)) = self.stored_area(store, area_id.0)
                && old.owner == self.account
                && owner != self.account
            {
                report.rejected.push(format!(
                    "area {}: handed to another account while this one exists",
                    area_id.0
                ));
                continue;
            }
            // A new owner must have been a member who may write (the only one the server may
            // hand an area to); anything else is a server making someone else owner.
            let known = self.stored_area(store, area_id.0).ok().map(|(m, _)| m);
            if let Some(old) = known
                && old.owner != owner
                && !old
                    .members
                    .iter()
                    .any(|m| m.account == owner && m.role == Role::Write)
            {
                report
                    .rejected
                    .push(format!("area {}: owner change not allowed", area_id.0));
                continue;
            }
            let opened = match self.devices_cached(store, &mut cache, &owner).await {
                Ok(owner_devices) => {
                    self.open_area(store, listing.manifest, &owner_devices, &mut tracker)
                        .await
                }
                Err(err) => Err(err),
            };
            let (manifest, key) = match opened {
                Ok(Some(opened)) => opened,
                Ok(None) => {
                    report
                        .rejected
                        .push(format!("area {}: no key for this device", area_id.0));
                    continue;
                }
                Err(err) if skippable(&err) => {
                    report.rejected.push(format!("area {}: {err}", area_id.0));
                    continue;
                }
                Err(err) => return Err(err),
            };
            self.note_share(store, &manifest)?;
            let mut cursor = lock(store)?
                .areas(self.server)?
                .into_iter()
                .find(|a| a.area_id == area_id.0)
                .map_or(0, |a| a.cursor);
            // A record that verified but could not be applied yet (its group not here yet, for
            // example) is fetched again next time: the stored cursor stays before it.
            let mut retry_from: Option<u64> = None;
            loop {
                let page = self.client.records(area_id, cursor, PAGE).await;
                let (records, more) = self.revoked_check(store, page)?;
                for (seq, record) in records {
                    cursor = cursor.max(seq);
                    let record_id = record.header.record_id;
                    let author = record.header.author_account;
                    let author_devices = match self.devices_cached(store, &mut cache, &author).await
                    {
                        Ok(set) => set,
                        Err(err) if skippable(&err) => {
                            report.rejected.push(format!("record {record_id}: {err}"));
                            continue;
                        }
                        Err(err) => return Err(err),
                    };
                    // Keys are skipped uncounted while key sync is off, so turning it on (which
                    // fetches from the start) still brings them.
                    if !sync_keys && is_key_record(&record, &key, &manifest, &author_devices) {
                        continue;
                    }
                    let before = tracker.clone();
                    match tracker.accept_record(&record, &key, &manifest, &author_devices) {
                        Ok((Freshness::Unchanged, _)) => {}
                        Ok((Freshness::New, body)) => {
                            if let Some(assignment) = decode_key_assignment(&body) {
                                if author == self.account && Some(area_id.0) == personal {
                                    assignments.push((assignment, record.header.clone()));
                                } else {
                                    report
                                        .rejected
                                        .push(format!("record {record_id}: not allowed"));
                                }
                                continue;
                            }
                            if let Some(info) = decode_share_info(&body) {
                                if author == manifest.owner {
                                    self.note_share_info(store, &manifest, info)?;
                                } else {
                                    report
                                        .rejected
                                        .push(format!("record {record_id}: not allowed"));
                                }
                                continue;
                            }
                            match self.take(store, vault, &record.header, &body, personal) {
                                Ok(true) => report.applied += 1,
                                Ok(false) => {}
                                // Refused for good, not fetched again.
                                Err(err @ SyncError::GroupCycle) => {
                                    report.rejected.push(format!("record {record_id}: {err}"))
                                }
                                Err(err) => {
                                    tracker = before;
                                    retry_from =
                                        Some(retry_from.map_or(seq - 1, |r| r.min(seq - 1)));
                                    report.rejected.push(format!("record {record_id}: {err}"))
                                }
                            }
                        }
                        Err(err) => report.rejected.push(format!("record {record_id}: {err}")),
                    }
                }
                let keep = retry_from.map_or(cursor, |r| r.min(cursor));
                lock(store)?.set_cursor(self.server, area_id.0, keep)?;
                self.save_state(store, &devices, &tracker)?;
                if !more {
                    break;
                }
            }
        }
        self.save_state(store, &devices, &tracker)?;
        for (assignment, header) in assignments {
            self.take_assignment(store, &assignment, &header)?;
        }
        // Members' devices come and go; own shares follow.
        let problems = crate::areas::check_member_devices(self, store, vault, now_secs()).await?;
        report.rejected.extend(problems);
        Ok(report)
    }

    /// The personal area's id as this device stored it, if it did.
    pub(crate) fn personal_id(&self, store: &Mutex<Store>) -> Result<Option<Uuid>, SyncError> {
        let setting = crate::account::personal_setting(self.server);
        Ok(lock(store)?
            .get_setting(&setting)?
            .and_then(|text| Uuid::parse_str(&text).ok()))
    }

    /// Own areas that could be the personal one: no members and no known share.
    fn personal_candidates(&self, store: &Mutex<Store>) -> Result<Vec<Uuid>, SyncError> {
        let shares: HashSet<Uuid> = lock(store)?
            .shares(self.server)?
            .into_iter()
            .map(|s| s.area_id)
            .collect();
        Ok(self
            .owned_areas(store)?
            .into_iter()
            .filter(|(m, _)| m.members.is_empty() && !shares.contains(&m.area_id.0))
            .map(|(m, _)| m.area_id.0)
            .collect())
    }

    /// Finds and stores the personal area's id once (a device set up before it was stored, or
    /// one that joined): an own area without members whose records hold no share info (a
    /// share taken over from a deleted owner can have no members left).
    async fn settle_personal(&self, store: &Mutex<Store>) -> Result<Option<Uuid>, SyncError> {
        if let Some(id) = self.personal_id(store)? {
            return Ok(Some(id));
        }
        let candidates = self.personal_candidates(store)?;
        let mut found = None;
        for area in candidates {
            let mut since = 0;
            let mut share = false;
            loop {
                let page = self.client.records(AreaId(area), since, PAGE).await;
                let (records, more) = self.revoked_check(store, page)?;
                for (seq, record) in records {
                    since = since.max(seq);
                    share |= record.header.record_id == share_info_record_id(area);
                }
                if share || !more {
                    break;
                }
            }
            if !share {
                found = Some(area);
                break;
            }
        }
        if let Some(id) = found {
            lock(store)?.put_setting(
                &crate::account::personal_setting(self.server),
                &id.to_string(),
            )?;
        }
        Ok(found)
    }

    /// This account's personal area with its key.
    pub(crate) fn personal_area(
        &self,
        store: &Mutex<Store>,
    ) -> Result<(AreaManifest, AreaKey), SyncError> {
        let id = match self.personal_id(store)? {
            Some(id) => id,
            None => *self
                .personal_candidates(store)?
                .first()
                .ok_or(SyncError::NotSetUp)?,
        };
        self.stored_area(store, id)
    }

    /// Takes over an area whose owner was deleted, if this account may: the manifest this device
    /// verified before must name it as a member who may write. A new key keeps the deleted
    /// owner's devices out; the other members stay. Returns whether it took over.
    async fn take_over(
        &self,
        store: &Mutex<Store>,
        vault: &Vault,
        listing: &crate::client::AreaListing,
    ) -> Result<bool, SyncError> {
        let area = listing.manifest.area_id.0;
        let Ok((old, key)) = self.stored_area(store, area) else {
            return Ok(false);
        };
        if old != listing.manifest
            || !old
                .members
                .iter()
                .any(|m| m.account == self.account && m.role == Role::Write)
        {
            return Ok(false);
        }
        // Only an owner the server has deleted is replaced: its log must be gone.
        match self.client.log(&old.owner, 0).await {
            Err(SyncError::Server { status: 404, .. }) => {}
            Err(err @ SyncError::Network(_)) => return Err(err),
            _ => return Ok(false),
        }
        let members = old
            .members
            .iter()
            .filter(|m| m.account != self.account)
            .cloned()
            .collect();
        crate::areas::rotate(
            self,
            store,
            vault,
            vec![(old, key, members)],
            None,
            now_secs(),
        )
        .await?;
        // The items stay where they are; they are this account's own now, not copies.
        let (group, with_keys) = {
            let guard = lock(store)?;
            let group = guard
                .items_in_area(self.server, area)?
                .into_iter()
                .find(|(kind, _)| *kind == ItemKind::Group)
                .map(|(_, id)| id);
            let with_keys = guard.share(self.server, area)?.is_some_and(|s| s.with_keys);
            guard.put_share(
                self.server,
                &ShareRow {
                    area_id: area,
                    kind: ShareKind::GroupAll,
                    group_id: group.clone(),
                    with_keys,
                    owned: true,
                    role: None,
                    owner: self.account.0,
                },
            )?;
            (group, with_keys)
        };
        // The new owner's word on what the share holds, for its other devices.
        let info = ShareInfo {
            kind: ShareKind::GroupAll,
            group,
            with_keys,
        };
        let record_id = share_info_record_id(area);
        let mut version = 1;
        let mut since = 0;
        loop {
            let page = self.client.records(AreaId(area), since, PAGE).await;
            let (records, more) = self.revoked_check(store, page)?;
            for (seq, record) in records {
                since = since.max(seq);
                if record.header.record_id == record_id {
                    version = record.header.version + 1;
                }
            }
            if !more {
                break;
            }
        }
        let (manifest, key) = self.stored_area(store, area)?;
        crate::shares::write_record(
            self,
            store,
            &manifest,
            &key,
            record_id,
            version,
            &crate::shares::encode_share_info(&info),
        )
        .await?;
        Ok(true)
    }

    fn assignment_setting(&self, record_id: Uuid) -> String {
        format!("sync.assign.{}.{}", self.server.0, record_id)
    }

    /// Writes this account's key for an item of an own share into the personal area.
    async fn write_assignment(
        &self,
        store: &Mutex<Store>,
        personal: Uuid,
        kind: ItemKind,
        id: &str,
        key: Option<KeyRef>,
        modified_at: i64,
    ) -> Result<(), SyncError> {
        let item = Uuid::parse_str(id).map_err(|e| SyncError::Store(e.to_string()))?;
        let record_id = sshcraft_core::store::copy_id(personal, item);
        let setting = self.assignment_setting(record_id);
        let mut version = lock(store)?
            .get_setting(&setting)?
            .and_then(|t| t.parse::<u64>().ok())
            .unwrap_or(0)
            + 1;
        let (manifest, area_key) = self.stored_area(store, personal)?;
        let body = encode_key_assignment(&KeyAssignment {
            kind,
            item: id.to_owned(),
            key,
            modified_at,
        });
        for _ in 0..CONFLICT_ROUNDS {
            let written = crate::shares::write_record(
                self, store, &manifest, &area_key, record_id, version, &body,
            )
            .await;
            match written {
                Ok(()) => {
                    lock(store)?.put_setting(&setting, &version.to_string())?;
                    return Ok(());
                }
                Err(SyncError::Server {
                    status: 409,
                    current: Some(current),
                    ..
                }) => version = current + 1,
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    /// Applies the owner's key for an item of an own share, written by another own device,
    /// unless a newer local change of that item waits to be sent. Stays on this device.
    fn take_assignment(
        &self,
        store: &Mutex<Store>,
        assignment: &KeyAssignment,
        header: &RecordHeader,
    ) -> Result<(), SyncError> {
        let guard = lock(store)?;
        guard.put_setting(
            &self.assignment_setting(header.record_id),
            &header.version.to_string(),
        )?;
        if guard
            .queued_at(self.server, assignment.kind, &assignment.item)?
            .is_some_and(|queued| newer(queued, assignment.modified_at))
        {
            return Ok(());
        }
        let uuid =
            Uuid::parse_str(&assignment.item).map_err(|e| SyncError::Store(e.to_string()))?;
        match assignment.kind {
            ItemKind::Host => {
                if let Some(mut host) = guard.list_hosts()?.into_iter().find(|h| h.id.0 == uuid) {
                    host.settings.key = assignment.key.clone();
                    guard.upsert_host(&host)?;
                }
            }
            ItemKind::Group => {
                if let Some(mut group) = guard.list_groups()?.into_iter().find(|g| g.id.0 == uuid) {
                    group.defaults.key = assignment.key.clone();
                    guard.upsert_group(&group)?;
                }
            }
            ItemKind::Key => {}
        }
        Ok(())
    }

    /// Warnings of the last push: changes it refused to send (`share_read_only`,
    /// `share_no_add_remove`) and reset to the server's state.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Sends every queued local change into the area its item lives in. A conflict (someone
    /// wrote first) is settled by fetching and "last change wins"; a network error leaves the
    /// outbox as it is. A change this account may not make in a share it received is not sent
    /// and the item goes back to the server's state.
    pub async fn push(
        &mut self,
        store: &Mutex<Store>,
        vault: &Vault,
        _now_ms: i64,
    ) -> Result<usize, SyncError> {
        self.warnings.clear();
        let sync_keys = lock(store)?
            .server(self.server)?
            .ok_or(SyncError::NotSetUp)?
            .sync_keys;
        let mut outbox = lock(store)?.outbox(self.server)?;
        // Keys before what uses them, a group before its subgroups, groups before hosts, so a
        // device fetching in order can apply each record as it comes.
        {
            let groups = lock(store)?.list_groups()?;
            let rank = |(kind, id): &(ItemKind, String)| match kind {
                ItemKind::Key => (0, 0),
                ItemKind::Group => (
                    1,
                    Uuid::parse_str(id).map_or(0, |u| {
                        sshcraft_core::model::group_chain(&groups, sshcraft_core::model::GroupId(u))
                            .len()
                    }),
                ),
                ItemKind::Host => (2, 0),
            };
            outbox.sort_by_key(rank);
        }
        let mut sent = 0;
        for (kind, id) in outbox {
            if kind == ItemKind::Key && !sync_keys {
                lock(store)?.dequeue(self.server, kind, &id)?;
                continue;
            }
            let mut conflicts = 0;
            while conflicts < CONFLICT_ROUNDS {
                let personal = self.personal_area(store)?.0.area_id.0;
                let step = {
                    let guard = lock(store)?;
                    let Some(queued) = guard.queued_at(self.server, kind, &id)? else {
                        break;
                    };
                    // A copy in an own share is built from its original and goes to its share.
                    if let Some((area, original)) = guard.copy_entry(self.server, kind, &id)? {
                        let mut item =
                            copy_item(&guard, vault, self.server, area, kind, &original, queued)?;
                        if guard.location_of(kind, &original)? != Some(self.server) {
                            item.body = ItemBody::Deleted {
                                item_kind: kind,
                                item_id: id.clone(),
                            };
                        }
                        let known = guard.record_entry(self.server, kind, &id)?;
                        if matches!(item.body, ItemBody::Deleted { .. }) && known.is_none() {
                            guard.dequeue(self.server, kind, &id)?;
                            guard.remove_copy(self.server, kind, &id)?;
                            break;
                        }
                        Step::Write(Box::new(item), known, area, queued)
                    } else {
                        let mut item = local_item(&guard, vault, kind, &id, queued)?;
                        // Moved to another place (this device only, or another server): for this
                        // server it is gone.
                        if guard.location_of(kind, &id)? != Some(self.server) {
                            item.body = ItemBody::Deleted {
                                item_kind: kind,
                                item_id: id.clone(),
                            };
                        }
                        let known = guard.record_entry(self.server, kind, &id)?;
                        let deleted = matches!(item.body, ItemBody::Deleted { .. });
                        if deleted && known.is_none() {
                            // Never reached the server: nothing to delete there.
                            guard.dequeue(self.server, kind, &id)?;
                            break;
                        }
                        // A deleted item is removed where its record is.
                        let target = match (&known, deleted) {
                            (Some(entry), true) => entry.area_id,
                            _ => guard.area_for(self.server, kind, &id, personal)?,
                        };
                        strip_unsynced_keys(&guard, self.server, &mut item, target, personal)?;
                        // Moving between areas is a deletion in the old one and an addition in the
                        // new one; both are checked before anything is sent.
                        let moving = known.as_ref().is_some_and(|e| e.area_id != target);
                        let refused = match refusal(
                            &guard,
                            self.server,
                            target,
                            kind,
                            deleted,
                            known.is_none() || moving,
                        )? {
                            Some(warning) => Some(warning),
                            None => match (&known, moving) {
                                (Some(entry), true) => {
                                    refusal(&guard, self.server, entry.area_id, kind, true, false)?
                                }
                                _ => None,
                            },
                        };
                        if let Some(warning) = refused {
                            guard.dequeue(self.server, kind, &id)?;
                            Step::Refuse(warning, known)
                        } else if let Some(entry) = known.clone().filter(|e| e.area_id != target) {
                            Step::Leave(entry, item.modified_at)
                        } else {
                            Step::Write(Box::new(item), known, target, queued)
                        }
                    }
                };
                let put = match step {
                    Step::Refuse(warning, known) => {
                        self.warnings.push(warning.to_owned());
                        // An item that cannot be put back is reported and skipped; the rest of
                        // the outbox still goes.
                        match self.restore(store, vault, kind, &id, known).await {
                            Ok(()) => {}
                            Err(
                                err @ (SyncError::Network(_)
                                | SyncError::Revoked
                                | SyncError::Blocked),
                            ) => return Err(err),
                            Err(err) => self.warnings.push(format!("restore_failed: {err}")),
                        }
                        break;
                    }
                    Step::Leave(entry, modified_at) => {
                        // Moved into or out of a share: first gone from the old area.
                        let Ok((manifest, key)) = self.stored_area(store, entry.area_id) else {
                            lock(store)?.remove_record_entry(self.server, kind, &id)?;
                            continue;
                        };
                        let item = SyncItem {
                            modified_at,
                            body: ItemBody::Deleted {
                                item_kind: kind,
                                item_id: id.clone(),
                            },
                        };
                        let header = RecordHeader {
                            record_id: entry.record_id,
                            area_id: manifest.area_id,
                            version: entry.version + 1,
                            key_version: manifest.key_version,
                            author_account: self.account,
                            author_device: self.device_id,
                        };
                        let record = SealedRecord::seal(header, &encode(&item)?, &key, &self.keys);
                        match self.client.put_record(&record).await {
                            Ok(_) => {
                                lock(store)?.remove_record_entry(self.server, kind, &id)?;
                                continue;
                            }
                            Err(err) => Err(err),
                        }
                    }
                    Step::Write(item, known, target, queued) => {
                        let (manifest, key) = self.stored_area(store, target)?;
                        let (record_id, version) = known
                            .as_ref()
                            .map_or_else(|| (Uuid::new_v4(), 1), |e| (e.record_id, e.version + 1));
                        let header = RecordHeader {
                            record_id,
                            area_id: manifest.area_id,
                            version,
                            key_version: manifest.key_version,
                            author_account: self.account,
                            author_device: self.device_id,
                        };
                        let record = SealedRecord::seal(header, &encode(&item)?, &key, &self.keys);
                        let entry = RecordEntry {
                            record_id,
                            area_id: manifest.area_id.0,
                            version,
                            modified_at: item.modified_at,
                        };
                        match self.client.put_record(&record).await {
                            Ok(_) => {
                                let assignment = {
                                    let guard = lock(store)?;
                                    guard.put_record_entry(self.server, &entry, kind, &id)?;
                                    guard.dequeue_if(self.server, kind, &id, queued)?;
                                    if matches!(item.body, ItemBody::Deleted { .. }) {
                                        // A deleted copy is done with.
                                        guard.remove_copy(self.server, kind, &id)?;
                                    }
                                    own_assignment(
                                        &guard,
                                        self.server,
                                        kind,
                                        &id,
                                        target,
                                        personal,
                                    )?
                                };
                                if let Some(key) = assignment {
                                    self.write_assignment(
                                        store,
                                        personal,
                                        kind,
                                        &id,
                                        key,
                                        item.modified_at,
                                    )
                                    .await?;
                                }
                                sent += 1;
                                break;
                            }
                            Err(err) => Err(err),
                        }
                    }
                };
                match put {
                    // 409: someone wrote first. 400: the area moved on to a new key (a
                    // rotation) or the stored manifest is stale. Either way fetch first, then
                    // try again (the item may be gone from the outbox if the server's state won).
                    Err(SyncError::Server {
                        status: 409 | 400, ..
                    }) => {
                        conflicts += 1;
                        self.pull(store, vault).await?;
                    }
                    Err(err) => return self.revoked_check(store, Err(err)),
                    Ok(()) => {}
                }
            }
        }
        Ok(sent)
    }

    /// Puts an item back to the server's state after a refused change: a new item goes, a
    /// changed or deleted one gets the server's record again.
    async fn restore(
        &self,
        store: &Mutex<Store>,
        vault: &Vault,
        kind: ItemKind,
        id: &str,
        known: Option<RecordEntry>,
    ) -> Result<(), SyncError> {
        let Some(entry) = known else {
            let guard = lock(store)?;
            remove_item(&guard, kind, id)?;
            guard.clear_item_area(self.server, kind, id)?;
            return Ok(());
        };
        let (manifest, key) = self.stored_area(store, entry.area_id)?;
        let personal = self.personal_area(store).ok().map(|(m, _)| m.area_id.0);
        let mut since = 0;
        loop {
            let page = self.client.records(manifest.area_id, since, PAGE).await;
            let (records, more) = self.revoked_check(store, page)?;
            for (seq, record) in records {
                since = since.max(seq);
                if record.header.record_id != entry.record_id {
                    continue;
                }
                let devices = self
                    .account_devices(store, &record.header.author_account)
                    .await?;
                let body = record.open(&key, &manifest, &devices)?;
                self.take(store, vault, &record.header, &body, personal)?;
                return Ok(());
            }
            if !more {
                return Ok(());
            }
        }
    }

    /// Applies a verified record unless a newer local change is waiting to be sent; either way
    /// remembers its version. Returns whether the local store changed.
    fn take(
        &self,
        store: &Mutex<Store>,
        vault: &Vault,
        header: &RecordHeader,
        body: &[u8],
        personal: Option<Uuid>,
    ) -> Result<bool, SyncError> {
        let mut item = decode(body)?;
        let (kind, item_id) = item.item();
        let store = lock(store)?;
        let area = header.area_id.0;
        let entry = RecordEntry {
            record_id: header.record_id,
            area_id: area,
            version: header.version,
            modified_at: item.modified_at,
        };
        let deleted = matches!(item.body, ItemBody::Deleted { .. });
        // In an own share with all hosts, a member neither moves nor deletes the shared group,
        // nor hangs anything into groups outside the share.
        if header.author_account != self.account
            && let Some(share) = store
                .share(self.server, area)?
                .filter(|s| s.owned && s.kind == ShareKind::GroupAll)
        {
            let is_root = |id: &str| share.group_id.as_deref() == Some(id);
            let outside =
                |group: Option<sshcraft_core::model::GroupId>| -> Result<bool, SyncError> {
                    Ok(match group {
                        Some(g) => {
                            store.effective_area(self.server, ItemKind::Group, &g.0.to_string())?
                                != Some(area)
                        }
                        None => false,
                    })
                };
            let refuse = match &mut item.body {
                ItemBody::Deleted { item_kind, item_id } => {
                    *item_kind == ItemKind::Group && is_root(item_id)
                }
                ItemBody::Group { group } if is_root(&group.id.0.to_string()) => {
                    group.parent_id = store
                        .list_groups()?
                        .into_iter()
                        .find(|g| g.id == group.id)
                        .and_then(|g| g.parent_id);
                    false
                }
                ItemBody::Group { group } => outside(group.parent_id)?,
                ItemBody::Host { host } => outside(host.group_id)?,
                ItemBody::Key { .. } => false,
            };
            if refuse {
                store.put_record_entry(self.server, &entry, kind, &item_id)?;
                return Ok(false);
            }
        }
        // On the owner's devices a copy in an own share is not an item of its own: a member's
        // change goes into the original (and from there into every other copy).
        if store
            .share(self.server, area)?
            .is_some_and(|s| s.owned && s.kind != ShareKind::GroupAll)
            && find_original(&store, self.server, area, kind, &item_id)?.is_some()
        {
            store.put_record_entry(self.server, &entry, kind, &item_id)?;
            if header.author_account == self.account || deleted {
                return Ok(false);
            }
            return take_into_original(&store, self.server, area, &item);
        }
        // In a received selection only the owner adds or removes items; other members only
        // change existing ones (a first version is a new record).
        if let Some(share) = store
            .share(self.server, area)?
            .filter(|s| !s.owned && s.kind != ShareKind::GroupAll)
            && header.author_account.0 != share.owner
            && (deleted || header.version == 1)
        {
            return Ok(false);
        }
        // A deletion from an area the item has left (it moved into or out of a share) is not
        // about the item any more.
        if deleted
            && store.item_location(kind, &item_id)?.is_some()
            && store
                .effective_area(self.server, kind, &item_id)?
                .or(personal)
                != Some(area)
        {
            return Ok(false);
        }
        let local_wins = store
            .queued_at(self.server, kind, &item_id)?
            .is_some_and(|queued| newer(queued, item.modified_at));
        let sync_keys = store.server(self.server)?.is_some_and(|row| row.sync_keys);
        // Where the item is here decides whether this server may touch it: an item this device
        // keeps only locally is not this server's business; one at another server is taken
        // over by a live record (it was moved here) but not removed by a deletion (it was moved
        // away from here).
        let elsewhere = match store.item_location(kind, &item_id)? {
            Some(None) => true,
            Some(Some(server)) if server != self.server => deleted,
            _ => false,
        };
        if local_wins || elsewhere || (kind == ItemKind::Key && !sync_keys) {
            store.put_record_entry(self.server, &entry, kind, &item_id)?;
            return Ok(false);
        }
        if let ItemBody::Group { group } = &item.body
            && !settle_cycle(&store, self.server, group, item.modified_at)?
        {
            store.put_record_entry(self.server, &entry, kind, &item_id)?;
            return Err(SyncError::GroupCycle);
        }
        apply_in(&store, vault, self.server, &item, Some(area), personal)?;
        if deleted || Some(area) == personal {
            store.clear_item_area(self.server, kind, &item_id)?;
        } else if store.effective_area(self.server, kind, &item_id)? != Some(area) {
            store.set_item_area(self.server, kind, &item_id, area)?;
        }
        store.put_record_entry(self.server, &entry, kind, &item_id)?;
        store.dequeue(self.server, kind, &item_id)?;
        Ok(true)
    }
}

/// The original of a copy in own share `area`; another device of the owner learns the pair
/// here, from the copy id.
fn find_original(
    store: &Store,
    server: ServerId,
    area: Uuid,
    kind: ItemKind,
    copy: &str,
) -> Result<Option<String>, SyncError> {
    if let Some(original) = store.original_of(server, kind, copy)? {
        return Ok(Some(original));
    }
    let ids: Vec<Uuid> = match kind {
        ItemKind::Group => store.list_groups()?.into_iter().map(|g| g.id.0).collect(),
        ItemKind::Host => store.list_hosts()?.into_iter().map(|h| h.id.0).collect(),
        ItemKind::Key => store
            .list_key_records()?
            .into_iter()
            .map(|k| k.id.0)
            .collect(),
    };
    let Some(original) = ids
        .into_iter()
        .find(|id| sshcraft_core::store::copy_id(area, *id).to_string() == copy)
    else {
        return Ok(None);
    };
    store.put_copy(server, area, kind, &original.to_string(), copy)?;
    Ok(Some(original.to_string()))
}

/// A member's change to a copy, written into the original unless a newer local change of the
/// original waits to be sent. The original keeps its id, group and (unless the member picked
/// another shared key) its key; then it and all its copies are sent again.
fn take_into_original(
    store: &Store,
    server: ServerId,
    area: Uuid,
    item: &SyncItem,
) -> Result<bool, SyncError> {
    use sshcraft_core::model::KeyRef;
    let (kind, copy) = item.item();
    let Some(original) = find_original(store, server, area, kind, &copy)? else {
        return Ok(false);
    };
    if store
        .queued_at(server, kind, &original)?
        .is_some_and(|queued| newer(queued, item.modified_at))
    {
        return Ok(false);
    }
    let original_key = |key: &Option<KeyRef>| -> Result<Option<KeyRef>, SyncError> {
        match key {
            Some(KeyRef::Stored { id }) => Ok(store
                .original_of(server, ItemKind::Key, &id.0.to_string())?
                .and_then(|o| Uuid::parse_str(&o).ok())
                .map(|o| KeyRef::Stored {
                    id: sshcraft_core::model::KeyId(o),
                })),
            _ => Ok(None),
        }
    };
    let uuid = Uuid::parse_str(&original).map_err(|e| SyncError::Store(e.to_string()))?;
    match &item.body {
        ItemBody::Host { host } => {
            let Some(mine) = store.list_hosts()?.into_iter().find(|h| h.id.0 == uuid) else {
                return Ok(false);
            };
            let mut next = host.clone();
            next.id = mine.id;
            next.group_id = mine.group_id;
            next.location = mine.location;
            next.settings.key = original_key(&host.settings.key)?.or(mine.settings.key.clone());
            // What the copy only carried because the original inherits it stays inherited.
            let shared = store.share(server, area)?.and_then(|s| s.group_id);
            let baked =
                crate::shares::intermediate_defaults(store, mine.group_id, shared.as_deref())?;
            let own = &mine.settings;
            let settings = &mut next.settings;
            if own.address.is_none() && settings.address == baked.address {
                settings.address = None;
            }
            if own.port.is_none() && settings.port == baked.port {
                settings.port = None;
            }
            if own.username.is_none() && settings.username == baked.username {
                settings.username = None;
            }
            if own.key.is_none() && settings.key == baked.key {
                settings.key = None;
            }
            store.upsert_host(&next)?;
        }
        // The group, keys and deletions of copies are the owner's alone (a selection only lets
        // members change hosts).
        _ => return Ok(false),
    }
    record_local_change(store, kind, &original, item.modified_at)?;
    Ok(true)
}

/// The key to remember privately after sending an item into an own share: its local key when
/// the item is a host or group in a share this account owns.
fn own_assignment(
    store: &Store,
    server: ServerId,
    kind: ItemKind,
    id: &str,
    target: Uuid,
    personal: Uuid,
) -> Result<Option<Option<KeyRef>>, SyncError> {
    if target == personal || !store.share(server, target)?.is_some_and(|s| s.owned) {
        return Ok(None);
    }
    let key = match kind {
        ItemKind::Host => store
            .list_hosts()?
            .into_iter()
            .find(|h| h.id.0.to_string() == id)
            .map(|h| h.settings.key),
        ItemKind::Group => store
            .list_groups()?
            .into_iter()
            .find(|g| g.id.0.to_string() == id)
            .map(|g| g.defaults.key),
        ItemKind::Key => None,
    };
    Ok(key)
}

/// What `push` does with one queued item.
enum Step {
    /// Not this account's to change: back to the server's state.
    Refuse(&'static str, Option<RecordEntry>),
    /// The item moved to another area: its old record says it is gone first.
    Leave(RecordEntry, i64),
    Write(Box<crate::items::SyncItem>, Option<RecordEntry>, Uuid, i64),
}

/// Why this account may not send a change into `area`, if it may not: a received share it may
/// only read; the owner's keys, which members never change; a received selection (or single
/// host), where only hosts may be changed, none added or removed.
fn refusal(
    store: &Store,
    server: ServerId,
    area: Uuid,
    kind: ItemKind,
    deleted: bool,
    new: bool,
) -> Result<Option<&'static str>, SyncError> {
    let Some(share) = store.share(server, area)?.filter(|s| !s.owned) else {
        return Ok(None);
    };
    if share.role != Some(ShareRole::Write) || kind == ItemKind::Key {
        return Ok(Some("share_read_only"));
    }
    if share.kind != ShareKind::GroupAll && (deleted || new) {
        return Ok(Some("share_no_add_remove"));
    }
    if share.kind != ShareKind::GroupAll && kind == ItemKind::Group {
        return Ok(Some("share_read_only"));
    }
    Ok(None)
}

/// A group from the server that would end up inside itself here (two devices nested two groups
/// into each other at the same time): the later change wins. If it is the incoming one, the
/// local group that closes the loop moves to the top level and is sent as a change of its own;
/// otherwise the incoming one is refused. Returns whether the group may be applied.
fn settle_cycle(
    store: &Store,
    server: ServerId,
    group: &sshcraft_core::model::Group,
    modified_at: i64,
) -> Result<bool, SyncError> {
    let Some(parent) = group.parent_id else {
        return Ok(true);
    };
    if parent == group.id {
        return Ok(false);
    }
    let groups = store.list_groups()?;
    let above = sshcraft_core::model::group_chain(&groups, parent);
    let Some(closing) = above.iter().find(|g| g.parent_id == Some(group.id)) else {
        return Ok(true);
    };
    let id = closing.id.0.to_string();
    let local = store
        .queued_at(server, ItemKind::Group, &id)?
        .into_iter()
        .chain(
            store
                .record_entry(server, ItemKind::Group, &id)?
                .map(|e| e.modified_at),
        )
        .max()
        .unwrap_or(i64::MIN);
    if newer(local, modified_at) {
        return Ok(false);
    }
    let mut detached = closing.clone();
    detached.parent_id = None;
    store.upsert_group(&detached)?;
    record_local_change(store, ItemKind::Group, &id, modified_at)?;
    // Moved to the top level, it may have left a share: everything below follows.
    record_group_hosts(store, &id, modified_at)?;
    Ok(true)
}

/// Errors that make one area or record unusable but not the whole pull: it did not verify, or
/// its account is gone from the server.
fn skippable(err: &SyncError) -> bool {
    matches!(
        err,
        SyncError::Proto(_) | SyncError::Server { status: 404, .. }
    )
}

/// Removes an item that came with a share this account lost.
fn remove_item(store: &Store, kind: ItemKind, id: &str) -> Result<(), SyncError> {
    let uuid = Uuid::parse_str(id).map_err(|e| SyncError::Store(e.to_string()))?;
    match kind {
        ItemKind::Group => store.delete_group(sshcraft_core::model::GroupId(uuid))?,
        ItemKind::Host => store.delete_host(sshcraft_core::model::HostId(uuid))?,
        ItemKind::Key => store.delete_key_record(sshcraft_core::model::KeyId(uuid))?,
    }
    Ok(())
}

/// Whether a record holds a key, read without counting it in the tracker.
fn is_key_record(
    record: &SealedRecord,
    key: &AreaKey,
    manifest: &AreaManifest,
    devices: &DeviceSet,
) -> bool {
    record
        .open(key, manifest, devices)
        .ok()
        .and_then(|body| decode(&body).ok())
        .is_some_and(|item| item.item().0 == ItemKind::Key)
}

/// A host or group that points to a stored key which does not live at this server, or not in
/// the area the record goes to, sends no key: each device (each member) assigns its own
/// (design, sections 7 and 8). Agent keys never go into a share. `apply_in` keeps the local
/// assignment.
pub(crate) fn strip_unsynced_keys(
    store: &Store,
    server: ServerId,
    item: &mut crate::items::SyncItem,
    target: Uuid,
    personal: Uuid,
) -> Result<(), SyncError> {
    let key = match &mut item.body {
        ItemBody::Host { host } => &mut host.settings.key,
        ItemBody::Group { group } => &mut group.defaults.key,
        _ => return Ok(()),
    };
    let keep = match key {
        Some(sshcraft_core::model::KeyRef::Stored { id }) => {
            let id = id.0.to_string();
            store.location_of(ItemKind::Key, &id)? == Some(server)
                && store.area_for(server, ItemKind::Key, &id, personal)? == target
        }
        Some(sshcraft_core::model::KeyRef::Agent { .. }) => target == personal,
        None => true,
    };
    if !keep {
        *key = None;
    }
    Ok(())
}

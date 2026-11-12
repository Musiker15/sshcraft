//! Syncing one server: fetch what changed (pull) and send what changed here (push).
//!
//! Everything fetched is verified first: the account's device log must extend the head this
//! device saw before, manifests and records go through the `VersionTracker`, envelopes are
//! opened against the verified manifest. What fails is reported and skipped, never applied.

use crate::SyncError;
use crate::account::device_keys;
use crate::client::ServerClient;
use crate::items::{ItemBody, apply, decode, encode, local_item, newer};
use sshcraft_core::model::ServerId;
use sshcraft_core::store::{ItemKind, RecordEntry, ServerRow, Store};
use sshcraft_core::vault::Vault;
use sshcraft_proto::area::{AreaKey, AreaManifest, KeyEnvelope, open_area_key};
use sshcraft_proto::devicelog::{DeviceLog, DeviceSet, LogHead};
use sshcraft_proto::identity::{AccountPublic, DeviceKeys};
use sshcraft_proto::record::{Freshness, RecordHeader, SealedRecord, VersionTracker};
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
    Ok(())
}

/// Queues every host of a group, for when the group moves or is deleted (its hosts go with it).
pub fn record_group_hosts(store: &Store, group_id: &str, now_ms: i64) -> Result<(), SyncError> {
    for host in store.list_hosts()? {
        if host.group_id.is_some_and(|g| g.0.to_string() == group_id) {
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

/// A logged-in connection to one server, with this device's keys.
pub struct Engine {
    pub server: ServerId,
    pub(crate) client: ServerClient,
    pub(crate) device_id: Uuid,
    pub(crate) keys: DeviceKeys,
    pub(crate) account: AccountPublic,
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
        if *account == self.account {
            return Ok(self.own_devices(store).await?.0);
        }
        let entries = self.revoked_check(store, self.client.log(account, 0).await)?;
        let log = DeviceLog { entries };
        let setting = format!(
            "sync.head.{}.{}",
            self.server.0,
            data_encoding::BASE64URL_NOPAD.encode(&account.0)
        );
        let known: Option<LogHead> = match lock(store)?.get_setting(&setting)? {
            Some(text) if !text.is_empty() => Some(from_json(&text)?),
            _ => None,
        };
        let set = log.verify_extends(account, known.as_ref())?;
        if let Some(head) = set.head() {
            lock(store)?.put_setting(&setting, &to_json(&head)?)?;
        }
        Ok(set)
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

    /// Fetches everything that changed on the server and applies what verifies.
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
        let listings = self.revoked_check(store, self.client.areas().await)?;
        for listing in listings {
            // Shares from other accounts come with Teilplan 7.
            if listing.owner != self.account {
                continue;
            }
            let area_id = listing.manifest.area_id;
            let (manifest, key) = match self
                .open_area(store, listing.manifest, &devices, &mut tracker)
                .await
            {
                Ok(Some(opened)) => opened,
                Ok(None) => {
                    report
                        .rejected
                        .push(format!("area {}: no key for this device", area_id.0));
                    continue;
                }
                Err(err @ SyncError::Proto(_)) => {
                    report.rejected.push(format!("area {}: {err}", area_id.0));
                    continue;
                }
                Err(err) => return Err(err),
            };
            let mut cursor = lock(store)?
                .areas(self.server)?
                .into_iter()
                .find(|a| a.area_id == area_id.0)
                .map_or(0, |a| a.cursor);
            let sync_keys = lock(store)?
                .server(self.server)?
                .is_some_and(|row| row.sync_keys);
            // A record that verified but could not be applied yet (its group not here yet, for
            // example) is fetched again next time: the stored cursor stays before it.
            let mut retry_from: Option<u64> = None;
            loop {
                let page = self.client.records(area_id, cursor, PAGE).await;
                let (records, more) = self.revoked_check(store, page)?;
                for (seq, record) in records {
                    cursor = cursor.max(seq);
                    let record_id = record.header.record_id;
                    // Records of other accounts (members) come with Teilplan 7.
                    if record.header.author_account != self.account {
                        report
                            .rejected
                            .push(format!("record {record_id}: not allowed"));
                        continue;
                    }
                    // Keys are skipped uncounted while key sync is off, so turning it on (which
                    // fetches from the start) still brings them.
                    if !sync_keys && is_key_record(&record, &key, &manifest, &devices) {
                        continue;
                    }
                    let before = tracker.clone();
                    match tracker.accept_record(&record, &key, &manifest, &devices) {
                        Ok((Freshness::Unchanged, _)) => {}
                        Ok((Freshness::New, body)) => {
                            match self.take(store, vault, &record.header, &body) {
                                Ok(true) => report.applied += 1,
                                Ok(false) => {}
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
        Ok(report)
    }

    /// This account's personal area with its key: the own area without members (a share always
    /// has some).
    pub(crate) fn personal_area(
        &self,
        store: &Mutex<Store>,
    ) -> Result<(AreaManifest, AreaKey), SyncError> {
        self.owned_areas(store)?
            .into_iter()
            .find(|(manifest, _)| manifest.members.is_empty())
            .ok_or(SyncError::NotSetUp)
    }

    /// Sends every queued local change. A conflict (someone wrote first) is settled by
    /// fetching and "last change wins"; a network error leaves the outbox as it is.
    pub async fn push(
        &mut self,
        store: &Mutex<Store>,
        vault: &Vault,
        _now_ms: i64,
    ) -> Result<usize, SyncError> {
        let sync_keys = lock(store)?
            .server(self.server)?
            .ok_or(SyncError::NotSetUp)?
            .sync_keys;
        let outbox = lock(store)?.outbox(self.server)?;
        let mut sent = 0;
        for (kind, id) in outbox {
            if kind == ItemKind::Key && !sync_keys {
                lock(store)?.dequeue(self.server, kind, &id)?;
                continue;
            }
            for _ in 0..CONFLICT_ROUNDS {
                let (manifest, key) = self.personal_area(store)?;
                let (record, entry, sent_at) = {
                    let guard = lock(store)?;
                    let Some(queued) = guard.queued_at(self.server, kind, &id)? else {
                        break;
                    };
                    let mut item = local_item(&guard, vault, kind, &id, queued)?;
                    strip_unsynced_keys(&guard, self.server, &mut item)?;
                    // Moved to another place (this device only, or another server): for this
                    // server it is gone.
                    if guard.location_of(kind, &id)? != Some(self.server) {
                        item.body = ItemBody::Deleted {
                            item_kind: kind,
                            item_id: id.clone(),
                        };
                    }
                    let known = guard.record_entry(self.server, kind, &id)?;
                    if matches!(item.body, ItemBody::Deleted { .. }) && known.is_none() {
                        // Never reached the server: nothing to delete there.
                        guard.dequeue(self.server, kind, &id)?;
                        break;
                    }
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
                    (record, entry, queued)
                };
                match self.client.put_record(&record).await {
                    Ok(_) => {
                        let guard = lock(store)?;
                        guard.put_record_entry(self.server, &entry, kind, &id)?;
                        guard.dequeue_if(self.server, kind, &id, sent_at)?;
                        sent += 1;
                        break;
                    }
                    // 409: someone wrote first. 400: the area moved on to a new key (a rotation)
                    // or the stored manifest is stale. Either way fetch first, then try again.
                    Err(SyncError::Server {
                        status: 409 | 400, ..
                    }) => {
                        // Someone wrote first: fetch, then decide again (the item may be gone
                        // from the outbox if the server's state won).
                        self.pull(store, vault).await?;
                    }
                    Err(err) => return self.revoked_check(store, Err(err)),
                }
            }
        }
        Ok(sent)
    }

    /// Applies a verified record unless a newer local change is waiting to be sent; either way
    /// remembers its version. Returns whether the local store changed.
    fn take(
        &self,
        store: &Mutex<Store>,
        vault: &Vault,
        header: &RecordHeader,
        body: &[u8],
    ) -> Result<bool, SyncError> {
        let item = decode(body)?;
        let (kind, item_id) = item.item();
        let store = lock(store)?;
        let entry = RecordEntry {
            record_id: header.record_id,
            area_id: header.area_id.0,
            version: header.version,
            modified_at: item.modified_at,
        };
        let local_wins = store
            .queued_at(self.server, kind, &item_id)?
            .is_some_and(|queued| newer(queued, item.modified_at));
        let sync_keys = store.server(self.server)?.is_some_and(|row| row.sync_keys);
        let deleted = matches!(item.body, ItemBody::Deleted { .. });
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
        apply(&store, vault, self.server, &item)?;
        store.put_record_entry(self.server, &entry, kind, &item_id)?;
        store.dequeue(self.server, kind, &item_id)?;
        Ok(true)
    }
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

/// A host or group that points to a stored key which does not live at this server sends no key:
/// each device assigns its own (design, section 7). `apply` keeps the local assignment.
fn strip_unsynced_keys(
    store: &Store,
    server: ServerId,
    item: &mut crate::items::SyncItem,
) -> Result<(), SyncError> {
    let key = match &mut item.body {
        ItemBody::Host { host } => &mut host.settings.key,
        ItemBody::Group { group } => &mut group.defaults.key,
        _ => return Ok(()),
    };
    if let Some(sshcraft_core::model::KeyRef::Stored { id }) = key
        && store.location_of(ItemKind::Key, &id.0.to_string())? != Some(server)
    {
        *key = None;
    }
    Ok(())
}

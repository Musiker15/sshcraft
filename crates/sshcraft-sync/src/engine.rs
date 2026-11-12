//! Syncing one server: fetch what changed (pull) and send what changed here (push).
//!
//! Everything fetched is verified first: the account's device log must extend the head this
//! device saw before, manifests and records go through the `VersionTracker`, envelopes are
//! opened against the verified manifest. What fails is reported and skipped, never applied.

use crate::SyncError;
use crate::account::device_keys;
use crate::client::ServerClient;
use crate::items::{apply, decode, newer};
use sshcraft_core::model::ServerId;
use sshcraft_core::store::{RecordEntry, ServerRow, Store};
use sshcraft_core::vault::Vault;
use sshcraft_proto::area::{AreaKey, AreaManifest, open_area_key};
use sshcraft_proto::devicelog::{DeviceLog, DeviceSet, LogHead};
use sshcraft_proto::identity::{AccountPublic, DeviceKeys};
use sshcraft_proto::record::{Freshness, VersionTracker};
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

/// Records fetched per request.
pub const PAGE: u32 = 500;

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

pub(crate) fn lock(store: &Mutex<Store>) -> Result<MutexGuard<'_, Store>, SyncError> {
    store.lock().map_err(|e| SyncError::Store(e.to_string()))
}

fn to_json(value: &impl serde::Serialize) -> Result<String, SyncError> {
    serde_json::to_string(value).map_err(|e| SyncError::Store(e.to_string()))
}

fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, SyncError> {
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
                Err(SyncError::Revoked) => return Err(SyncError::Revoked),
                Err(err) => {
                    report.rejected.push(format!("area {}: {err}", area_id.0));
                    continue;
                }
            };
            let mut cursor = lock(store)?
                .areas(self.server)?
                .into_iter()
                .find(|a| a.area_id == area_id.0)
                .map_or(0, |a| a.cursor);
            loop {
                let page = self.client.records(area_id, cursor, PAGE).await;
                let (records, more) = self.revoked_check(store, page)?;
                for (seq, record) in records {
                    cursor = cursor.max(seq);
                    let record_id = record.header.record_id;
                    // Records of other accounts (members) come with Teilplan 7.
                    let opened = if record.header.author_account == self.account {
                        tracker.accept_record(&record, &key, &manifest, &devices)
                    } else {
                        Err(sshcraft_proto::ProtoError::NotAllowed)
                    };
                    match opened {
                        Ok((Freshness::Unchanged, _)) => {}
                        Ok((Freshness::New, body)) => {
                            match self.take(store, vault, &record.header, &body) {
                                Ok(true) => report.applied += 1,
                                Ok(false) => {}
                                Err(err) => {
                                    report.rejected.push(format!("record {record_id}: {err}"))
                                }
                            }
                        }
                        Err(err) => report.rejected.push(format!("record {record_id}: {err}")),
                    }
                }
                lock(store)?.set_cursor(self.server, area_id.0, cursor)?;
                self.save_state(store, &devices, &tracker)?;
                if !more {
                    break;
                }
            }
        }
        self.save_state(store, &devices, &tracker)?;
        Ok(report)
    }

    /// Applies a verified record unless a newer local change is waiting to be sent; either way
    /// remembers its version. Returns whether the local store changed.
    fn take(
        &self,
        store: &Mutex<Store>,
        vault: &Vault,
        header: &sshcraft_proto::record::RecordHeader,
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
        store.put_record_entry(self.server, &entry, kind, &item_id)?;
        if local_wins {
            return Ok(false);
        }
        apply(&store, vault, self.server, &item)?;
        store.dequeue(self.server, kind, &item_id)?;
        Ok(true)
    }
}

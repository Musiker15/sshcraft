//! The devices of this account, and revoking one with a fresh key for every area it could read.

use crate::SyncError;
use crate::engine::{Engine, PAGE, lock};
use serde::Serialize;
use sshcraft_core::store::{RecordEntry, Store};
use sshcraft_core::vault::Vault;
use sshcraft_proto::area::{AreaKey, AreaManifest, seal_area_key};
use sshcraft_proto::devicelog::LogAction;
use sshcraft_proto::identity::is_recovery;
use sshcraft_proto::record::{RecordHeader, SealedRecord};
use std::sync::Mutex;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceInfo {
    pub device_id: Uuid,
    pub name: String,
    pub platform: String,
    pub active: bool,
    pub admin_blocked: bool,
    pub last_seen: Option<i64>,
    pub this_device: bool,
    pub recovery: bool,
}

/// Every device ever added to the account, in log order, with its current state.
pub async fn list_devices(
    engine: &mut Engine,
    store: &Mutex<Store>,
) -> Result<Vec<DeviceInfo>, SyncError> {
    let (set, log) = engine.own_devices(store).await?;
    let states = engine.revoked_check(store, engine.client().my_devices().await)?;
    Ok(log
        .entries
        .iter()
        .filter_map(|entry| match &entry.action {
            LogAction::Add { certificate } => Some(certificate),
            LogAction::Revoke { .. } => None,
        })
        .map(|cert| {
            let state = states.iter().find(|s| s.device_id == cert.device_id);
            DeviceInfo {
                device_id: cert.device_id,
                name: cert.name.clone(),
                platform: cert.platform.clone(),
                active: set.get_active(cert.device_id).is_some(),
                admin_blocked: state.is_some_and(|s| s.admin_blocked),
                last_seen: state.and_then(|s| s.last_seen),
                this_device: cert.device_id == engine.device_id(),
                recovery: is_recovery(cert),
            }
        })
        .collect())
}

/// Revokes `target`, signed by this device, then rotates the personal area: new key, new key
/// version, envelopes only for the devices still active, every record sealed again. The
/// records are read before the revocation, while their author devices still count as active.
pub async fn revoke_device(
    engine: &mut Engine,
    store: &Mutex<Store>,
    _vault: &Vault,
    target: Uuid,
    now: i64,
) -> Result<(), SyncError> {
    let (set, mut log) = engine.own_devices(store).await?;
    let known = log.entries.iter().find_map(|entry| match &entry.action {
        LogAction::Add { certificate } if certificate.device_id == target => Some(certificate),
        _ => None,
    });
    let Some(target_cert) = known else {
        return Err(SyncError::NotAllowed);
    };
    if target == engine.device_id() || is_recovery(target_cert) {
        return Err(SyncError::NotAllowed);
    }
    // Already revoked (a revocation whose rotation failed): rotate again without a new entry,
    // so a retry always ends with a key the revoked device does not know.
    let already_revoked = set.get_active(target).is_none();

    let (manifest, key) = engine.personal_area(store)?;
    let mut bodies: Vec<(SealedRecord, Zeroizing<Vec<u8>>)> = Vec::new();
    let mut since = 0;
    loop {
        let (page, more) = engine.revoked_check(
            store,
            engine.client().records(manifest.area_id, since, PAGE).await,
        )?;
        for (seq, record) in page {
            since = since.max(seq);
            // A record that no longer opens (an author revoked earlier) is not carried over.
            if let Ok(body) = record.open(&key, &manifest, &set) {
                bodies.push((record, body));
            }
        }
        if !more {
            break;
        }
    }

    if !already_revoked {
        log.append_revoke_by_device(engine.device_id(), &engine.keys, target, now);
        let entry = log.entries.last().cloned().expect("just appended");
        let appended = engine
            .client()
            .append_log(&engine.account, &[entry], false)
            .await;
        engine.revoked_check(store, appended)?;
    }
    let remaining = log.verify(&engine.account)?;

    let new_key = AreaKey::generate();
    let key_version = manifest.key_version + 1;
    let rotated = AreaManifest::sign(
        manifest.area_id,
        manifest.version + 1,
        key_version,
        &new_key,
        engine.account,
        manifest.members.clone(),
        engine.device_id(),
        &engine.keys,
    );
    engine.client().put_manifest(&rotated).await?;
    let envelopes = remaining
        .active()
        .map(|cert| seal_area_key(&new_key, manifest.area_id, key_version, cert))
        .collect::<Result<Vec<_>, _>>()?;
    engine
        .client()
        .put_envelopes(manifest.area_id, &envelopes)
        .await?;
    let own = envelopes
        .iter()
        .find(|e| e.device_id == engine.device_id())
        .cloned()
        .ok_or(SyncError::NotSetUp)?;
    {
        let guard = lock(store)?;
        guard.put_area(
            engine.server,
            manifest.area_id.0,
            &serde_json::to_string(&rotated).map_err(|e| SyncError::Store(e.to_string()))?,
            &serde_json::to_string(&own).map_err(|e| SyncError::Store(e.to_string()))?,
        )?;
    }

    for (record, body) in bodies {
        let header = RecordHeader {
            record_id: record.header.record_id,
            area_id: manifest.area_id,
            version: record.header.version + 1,
            key_version,
            author_account: engine.account,
            author_device: engine.device_id(),
        };
        let sealed = SealedRecord::seal(header.clone(), &body, &new_key, &engine.keys);
        engine.client().put_record(&sealed).await?;
        let guard = lock(store)?;
        if let Some((kind, item_id)) = guard.item_for_record(engine.server, header.record_id)? {
            let modified_at = guard
                .record_entry(engine.server, kind, &item_id)?
                .map_or(0, |e| e.modified_at);
            guard.put_record_entry(
                engine.server,
                &RecordEntry {
                    record_id: header.record_id,
                    area_id: manifest.area_id.0,
                    version: header.version,
                    modified_at,
                },
                kind,
                &item_id,
            )?;
        }
    }
    Ok(())
}

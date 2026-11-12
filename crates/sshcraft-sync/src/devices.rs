//! The devices of this account, and revoking one with a fresh key for every area it could read.

use crate::SyncError;
use crate::engine::{Engine, PAGE, lock};
use serde::Serialize;
use sshcraft_core::store::{RecordEntry, Store};
use sshcraft_core::vault::Vault;
use sshcraft_proto::area::{AreaKey, AreaManifest, key_commitment, seal_area_key};
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

/// An unfinished key rotation, stored sealed until every step on the server is done, so a
/// failure at any step (network, app closed) is finished by the next sync.
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingRotation {
    target: Uuid,
    old_manifest: AreaManifest,
    #[serde(with = "sshcraft_proto::encoding::b64")]
    old_key: Vec<u8>,
    #[serde(with = "sshcraft_proto::encoding::b64")]
    new_key: Vec<u8>,
}

fn setting_key(engine: &Engine) -> String {
    format!("sync.rotation.{}", engine.server.0)
}

fn seal_label(engine: &Engine) -> String {
    format!("sync-rotation:{}", engine.server.0)
}

fn area_key(bytes: &[u8]) -> Result<AreaKey, SyncError> {
    let array: &[u8; 32] = bytes
        .try_into()
        .map_err(|_| SyncError::Store("area key length".into()))?;
    Ok(AreaKey::from_bytes(array))
}

fn save_pending(
    engine: &Engine,
    store: &Mutex<Store>,
    vault: &Vault,
    pending: &PendingRotation,
) -> Result<(), SyncError> {
    let json =
        Zeroizing::new(serde_json::to_vec(pending).map_err(|e| SyncError::Store(e.to_string()))?);
    let sealed = vault.seal_secret(&seal_label(engine), &json)?;
    lock(store)?.put_setting(
        &setting_key(engine),
        &data_encoding::BASE64URL_NOPAD.encode(&sealed),
    )?;
    Ok(())
}

fn load_pending(
    engine: &Engine,
    store: &Mutex<Store>,
    vault: &Vault,
) -> Result<Option<PendingRotation>, SyncError> {
    let Some(text) = lock(store)?.get_setting(&setting_key(engine))? else {
        return Ok(None);
    };
    if text.is_empty() {
        return Ok(None);
    }
    let sealed = data_encoding::BASE64URL_NOPAD
        .decode(text.as_bytes())
        .map_err(|e| SyncError::Store(e.to_string()))?;
    let json = vault.open_secret(&seal_label(engine), &sealed)?;
    Ok(Some(
        serde_json::from_slice(&json).map_err(|e| SyncError::Store(e.to_string()))?,
    ))
}

fn clear_pending(engine: &Engine, store: &Mutex<Store>) -> Result<(), SyncError> {
    lock(store)?.put_setting(&setting_key(engine), "")?;
    Ok(())
}

/// Revokes `target`, signed by this device, then rotates the personal area: new key, new key
/// version, envelopes only for the devices still active, every record sealed again. The plan is
/// stored (sealed) before the first step, and every step can be repeated, so an interruption is
/// finished by the next sync instead of leaving the account without a usable key.
pub async fn revoke_device(
    engine: &mut Engine,
    store: &Mutex<Store>,
    vault: &Vault,
    target: Uuid,
    now: i64,
) -> Result<(), SyncError> {
    // An earlier rotation that did not finish goes first.
    resume_rotation(engine, store, vault, now).await?;
    let (_, log) = engine.own_devices(store).await?;
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
    let (old_manifest, old_key) = engine.personal_area(store)?;
    let pending = PendingRotation {
        target,
        old_manifest,
        old_key: old_key.to_bytes().to_vec(),
        new_key: AreaKey::generate().to_bytes().to_vec(),
    };
    save_pending(engine, store, vault, &pending)?;
    finish(engine, store, pending, now).await
}

/// Finishes a stored, unfinished rotation; does nothing if there is none.
pub async fn resume_rotation(
    engine: &mut Engine,
    store: &Mutex<Store>,
    vault: &Vault,
    now: i64,
) -> Result<(), SyncError> {
    match load_pending(engine, store, vault)? {
        Some(pending) => finish(engine, store, pending, now).await,
        None => Ok(()),
    }
}

async fn finish(
    engine: &mut Engine,
    store: &Mutex<Store>,
    pending: PendingRotation,
    now: i64,
) -> Result<(), SyncError> {
    let old = &pending.old_manifest;
    let area = old.area_id;
    let old_key = area_key(&pending.old_key)?;
    let new_key = area_key(&pending.new_key)?;
    let new_version = old.key_version + 1;

    // 1. The revocation in the log (skipped when it is already there).
    let (set, mut log) = engine.own_devices(store).await?;
    let readers = if set.get_active(pending.target).is_some() {
        log.append_revoke_by_device(engine.device_id(), &engine.keys, pending.target, now);
        let entry = log.entries.last().cloned().expect("just appended");
        let appended = engine
            .client()
            .append_log(&engine.account, &[entry], false)
            .await;
        engine.revoked_check(store, appended)?;
        log.verify(&engine.account)?
    } else {
        set
    };

    // 2. The new manifest, unless the server already has it.
    let listings = engine.revoked_check(store, engine.client().areas().await)?;
    let current = listings
        .into_iter()
        .find(|l| l.manifest.area_id == area)
        .ok_or(SyncError::NotSetUp)?
        .manifest;
    let committed = key_commitment(area, new_version, &new_key);
    let manifest = if current.key_version == new_version && current.key_commitment == committed {
        current
    } else if current.key_version == old.key_version {
        let rotated = AreaManifest::sign(
            area,
            current.version + 1,
            new_version,
            &new_key,
            engine.account,
            current.members.clone(),
            engine.device_id(),
            &engine.keys,
        );
        engine.client().put_manifest(&rotated).await?;
        rotated
    } else {
        // Someone else rotated in between; this plan is out of date.
        clear_pending(engine, store)?;
        return Err(SyncError::NotAllowed);
    };

    // 3. Envelopes for every device still active (the upload replaces, so repeating is fine).
    let envelopes = readers
        .active()
        .map(|cert| seal_area_key(&new_key, area, new_version, cert))
        .collect::<Result<Vec<_>, _>>()?;
    engine.client().put_envelopes(area, &envelopes).await?;
    let own = envelopes
        .iter()
        .find(|e| e.device_id == engine.device_id())
        .cloned()
        .ok_or(SyncError::NotSetUp)?;
    lock(store)?.put_area(
        engine.server,
        area.0,
        &serde_json::to_string(&manifest).map_err(|e| SyncError::Store(e.to_string()))?,
        &serde_json::to_string(&own).map_err(|e| SyncError::Store(e.to_string()))?,
    )?;

    // 4. Every record still under the old key, sealed again under the new one.
    let mut since = 0;
    loop {
        let page = engine.client().records(area, since, PAGE).await;
        let (records, more) = engine.revoked_check(store, page)?;
        for (seq, record) in records {
            since = since.max(seq);
            if record.header.key_version != old.key_version {
                continue;
            }
            // Records of devices revoked before do not open any more and are not carried over.
            let Ok(body) = record.open(&old_key, old, &readers) else {
                continue;
            };
            let known = {
                let guard = lock(store)?;
                match guard.item_for_record(engine.server, record.header.record_id)? {
                    Some((kind, item)) => guard
                        .record_entry(engine.server, kind, &item)?
                        .map(|entry| (kind, item, entry)),
                    None => None,
                }
            };
            // An older version than this device saw is a rollback: do not sign it again.
            if known
                .as_ref()
                .is_some_and(|(_, _, entry)| record.header.version < entry.version)
            {
                continue;
            }
            let header = RecordHeader {
                record_id: record.header.record_id,
                area_id: area,
                version: record.header.version + 1,
                key_version: new_version,
                author_account: engine.account,
                author_device: engine.device_id(),
            };
            let sealed = SealedRecord::seal(header.clone(), &body, &new_key, &engine.keys);
            match engine.client().put_record(&sealed).await {
                Ok(_) => {}
                // Someone with the new key wrote a newer version first.
                Err(SyncError::Server { status: 409, .. }) => continue,
                Err(err) => return Err(err),
            }
            if let Some((kind, item, entry)) = known {
                lock(store)?.put_record_entry(
                    engine.server,
                    &RecordEntry {
                        version: header.version,
                        ..entry
                    },
                    kind,
                    &item,
                )?;
            }
        }
        if !more {
            break;
        }
    }
    clear_pending(engine, store)
}

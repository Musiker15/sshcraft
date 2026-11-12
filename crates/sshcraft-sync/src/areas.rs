//! Areas this account owns: creating one, changing its members, keeping an envelope for every
//! device that may read it, and rotating its key when a device or a member must no longer read
//! new states (design, sections 6 and 8).

use crate::SyncError;
use crate::engine::{Engine, PAGE, from_json, lock, to_json};
use crate::items::{encode, local_item};
use serde::{Deserialize, Serialize};
use sshcraft_core::store::{RecordEntry, Store};
use sshcraft_core::vault::Vault;
use sshcraft_proto::area::{
    AreaId, AreaKey, AreaManifest, KeyEnvelope, Member, key_commitment, seal_area_key,
};
use sshcraft_proto::devicelog::DeviceSet;
use sshcraft_proto::identity::{AccountPublic, DeviceCertificate};
use sshcraft_proto::record::{RecordHeader, SealedRecord};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use uuid::Uuid;
use zeroize::Zeroizing;

/// Envelopes per upload (the server's limit).
const ENVELOPES_PER_REQUEST: usize = 1000;

/// Which devices got an envelope for the current key version of an area.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct SealedSet {
    pub key_version: u32,
    pub devices: Vec<Uuid>,
}

fn sealed_setting(engine: &Engine, area: AreaId) -> String {
    format!("sync.sealed.{}.{}", engine.server.0, area.0)
}

pub(crate) fn load_sealed(
    engine: &Engine,
    store: &Mutex<Store>,
    area: AreaId,
) -> Result<SealedSet, SyncError> {
    match lock(store)?.get_setting(&sealed_setting(engine, area))? {
        Some(text) if !text.is_empty() => from_json(&text),
        _ => Ok(SealedSet::default()),
    }
}

fn save_sealed(
    engine: &Engine,
    store: &Mutex<Store>,
    area: AreaId,
    sealed: &SealedSet,
) -> Result<(), SyncError> {
    lock(store)?.put_setting(&sealed_setting(engine, area), &to_json(sealed)?)?;
    Ok(())
}

/// Every active device that may read an area of this account: its own (with the recovery
/// device) and every member's.
pub(crate) async fn readers(
    engine: &Engine,
    store: &Mutex<Store>,
    members: &[Member],
) -> Result<Vec<DeviceCertificate>, SyncError> {
    let mut certs: Vec<DeviceCertificate> = engine
        .own_devices(store)
        .await?
        .0
        .active()
        .cloned()
        .collect();
    for member in members {
        let devices = engine.account_devices(store, &member.account).await?;
        certs.extend(devices.active().cloned());
    }
    Ok(certs)
}

fn seal_all(
    key: &AreaKey,
    area: AreaId,
    key_version: u32,
    readers: &[&DeviceCertificate],
) -> Result<Vec<KeyEnvelope>, SyncError> {
    Ok(readers
        .iter()
        .map(|cert| seal_area_key(key, area, key_version, cert))
        .collect::<Result<Vec<_>, _>>()?)
}

async fn upload(
    engine: &Engine,
    store: &Mutex<Store>,
    area: AreaId,
    envelopes: &[KeyEnvelope],
) -> Result<(), SyncError> {
    for chunk in envelopes.chunks(ENVELOPES_PER_REQUEST) {
        let put = engine.client().put_envelopes(area, chunk).await;
        engine.revoked_check(store, put)?;
    }
    Ok(())
}

/// Stores a manifest this device signed, with its own envelope, and which devices have one.
fn remember(
    engine: &Engine,
    store: &Mutex<Store>,
    manifest: &AreaManifest,
    envelopes: &[KeyEnvelope],
) -> Result<(), SyncError> {
    let own = envelopes
        .iter()
        .find(|e| e.device_id == engine.device_id())
        .ok_or(SyncError::NotSetUp)?;
    lock(store)?.put_area(
        engine.server,
        manifest.area_id.0,
        &to_json(manifest)?,
        &to_json(own)?,
    )?;
    save_sealed(
        engine,
        store,
        manifest.area_id,
        &SealedSet {
            key_version: manifest.key_version,
            devices: envelopes.iter().map(|e| e.device_id).collect(),
        },
    )
}

/// A new area owned by this account, with envelopes for its devices and every member's.
pub async fn create_area(
    engine: &Engine,
    store: &Mutex<Store>,
    members: Vec<Member>,
) -> Result<(AreaManifest, AreaKey), SyncError> {
    let area = AreaId(Uuid::new_v4());
    let key = AreaKey::generate();
    // The readers first: a member whose log does not verify stops it before anything is sent.
    let readers = readers(engine, store, &members).await?;
    let manifest = AreaManifest::sign(
        area,
        1,
        1,
        &key,
        engine.account,
        members,
        engine.device_id(),
        &engine.keys,
    );
    let put = engine.client().put_manifest(&manifest).await;
    engine.revoked_check(store, put)?;
    let envelopes = seal_all(&key, area, 1, &readers.iter().collect::<Vec<_>>())?;
    upload(engine, store, area, &envelopes).await?;
    remember(engine, store, &manifest, &envelopes)?;
    Ok((manifest, key))
}

/// Seals the area key for every device of the owner and the members that has none yet;
/// returns how many envelopes were added.
pub async fn reseal_envelopes(
    engine: &Engine,
    store: &Mutex<Store>,
    area: AreaId,
) -> Result<usize, SyncError> {
    let (manifest, key) = engine.stored_area(store, area.0)?;
    let readers = readers(engine, store, &manifest.members).await?;
    let mut sealed = load_sealed(engine, store, area)?;
    if sealed.key_version != manifest.key_version {
        sealed = SealedSet {
            key_version: manifest.key_version,
            devices: vec![],
        };
    }
    let known: HashSet<Uuid> = sealed.devices.iter().copied().collect();
    let new: Vec<&DeviceCertificate> = readers
        .iter()
        .filter(|cert| !known.contains(&cert.device_id))
        .collect();
    if new.is_empty() {
        return Ok(0);
    }
    let envelopes = seal_all(&key, area, manifest.key_version, &new)?;
    upload(engine, store, area, &envelopes).await?;
    sealed.devices.extend(envelopes.iter().map(|e| e.device_id));
    save_sealed(engine, store, area, &sealed)?;
    Ok(envelopes.len())
}

/// Changes who may read or write an area of this account. Someone who leaves makes the key
/// change (rotation), so they cannot read what comes after; the others need not do anything.
pub async fn set_members(
    engine: &Engine,
    store: &Mutex<Store>,
    vault: &Vault,
    area: AreaId,
    members: Vec<Member>,
    now: i64,
) -> Result<(), SyncError> {
    resume_rotation(engine, store, vault, now).await?;
    let (old, key) = engine.stored_area(store, area.0)?;
    if old.owner != engine.account {
        return Err(SyncError::NotAllowed);
    }
    let removed = old
        .members
        .iter()
        .any(|m| !members.iter().any(|n| n.account == m.account));
    if removed {
        return rotate(engine, store, vault, vec![(old, key, members)], None, now).await;
    }
    let manifest = AreaManifest::sign(
        area,
        old.version + 1,
        old.key_version,
        &key,
        engine.account,
        members,
        engine.device_id(),
        &engine.keys,
    );
    let put = engine.client().put_manifest(&manifest).await;
    engine.revoked_check(store, put)?;
    let envelope = lock(store)?
        .areas(engine.server)?
        .into_iter()
        .find(|a| a.area_id == area.0)
        .ok_or(SyncError::NotSetUp)?
        .envelope;
    lock(store)?.put_area(engine.server, area.0, &to_json(&manifest)?, &envelope)?;
    reseal_envelopes(engine, store, area).await?;
    Ok(())
}

/// One area of an unfinished rotation.
#[derive(Serialize, Deserialize)]
struct PendingArea {
    old_manifest: AreaManifest,
    #[serde(with = "sshcraft_proto::encoding::b64")]
    old_key: Vec<u8>,
    #[serde(with = "sshcraft_proto::encoding::b64")]
    new_key: Vec<u8>,
    /// The members after the rotation.
    members: Vec<Member>,
}

/// An unfinished key rotation, stored sealed until every step on the server is done, so a
/// failure at any step (network, app closed) is finished by the next sync.
#[derive(Serialize, Deserialize)]
struct PendingRotation {
    /// A device of this account to revoke first.
    revoke: Option<Uuid>,
    areas: Vec<PendingArea>,
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
    if pending.revoke.is_none() && pending.areas.is_empty() {
        lock(store)?.put_setting(&setting_key(engine), "")?;
        return Ok(());
    }
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

/// Rotates the given areas (old manifest, old key, members afterwards): new key, new key
/// version, envelopes only for the devices that may still read, every record sealed again.
/// With `revoke`, that device of this account is revoked first. The plan is stored (sealed)
/// before the first step and every step can be repeated.
pub(crate) async fn rotate(
    engine: &Engine,
    store: &Mutex<Store>,
    vault: &Vault,
    areas: Vec<(AreaManifest, AreaKey, Vec<Member>)>,
    revoke: Option<Uuid>,
    now: i64,
) -> Result<(), SyncError> {
    let pending = PendingRotation {
        revoke,
        areas: areas
            .into_iter()
            .map(|(old_manifest, old_key, members)| PendingArea {
                old_manifest,
                old_key: old_key.to_bytes().to_vec(),
                new_key: AreaKey::generate().to_bytes().to_vec(),
                members,
            })
            .collect(),
    };
    save_pending(engine, store, vault, &pending)?;
    finish(engine, store, vault, pending, now).await
}

/// Finishes a stored, unfinished rotation; does nothing if there is none.
pub async fn resume_rotation(
    engine: &Engine,
    store: &Mutex<Store>,
    vault: &Vault,
    now: i64,
) -> Result<(), SyncError> {
    match load_pending(engine, store, vault)? {
        Some(pending) => finish(engine, store, vault, pending, now).await,
        None => Ok(()),
    }
}

async fn finish(
    engine: &Engine,
    store: &Mutex<Store>,
    vault: &Vault,
    mut pending: PendingRotation,
    now: i64,
) -> Result<(), SyncError> {
    if let Some(target) = pending.revoke {
        let (set, mut log) = engine.own_devices(store).await?;
        if set.get_active(target).is_some() {
            log.append_revoke_by_device(engine.device_id(), &engine.keys, target, now);
            let entry = log.entries.last().cloned().expect("just appended");
            let appended = engine
                .client()
                .append_log(&engine.account, &[entry], false)
                .await;
            engine.revoked_check(store, appended)?;
        }
        pending.revoke = None;
        save_pending(engine, store, vault, &pending)?;
    }
    while let Some(area) = pending.areas.first() {
        finish_area(engine, store, vault, area).await?;
        pending.areas.remove(0);
        save_pending(engine, store, vault, &pending)?;
    }
    Ok(())
}

async fn finish_area(
    engine: &Engine,
    store: &Mutex<Store>,
    vault: &Vault,
    pending: &PendingArea,
) -> Result<(), SyncError> {
    let old = &pending.old_manifest;
    let area = old.area_id;
    let old_key = area_key(&pending.old_key)?;
    let new_key = area_key(&pending.new_key)?;
    let new_version = old.key_version + 1;

    // 1. The new manifest, unless the server already has it.
    let listings = engine.revoked_check(store, engine.client().areas().await)?;
    let Some(current) = listings.into_iter().find(|l| l.manifest.area_id == area) else {
        // Deleted in between: nothing left to protect.
        return Ok(());
    };
    let current = current.manifest;
    let committed = key_commitment(area, new_version, &new_key);
    let manifest = if current.key_version == new_version
        && current.key_commitment == committed
        && current.owner == engine.account
    {
        current
    } else if current.key_version == old.key_version {
        let rotated = AreaManifest::sign(
            area,
            current.version + 1,
            new_version,
            &new_key,
            engine.account,
            pending.members.clone(),
            engine.device_id(),
            &engine.keys,
        );
        let put = engine.client().put_manifest(&rotated).await;
        engine.revoked_check(store, put)?;
        rotated
    } else {
        // Rotated by another device of this account in between; this plan is out of date.
        return Ok(());
    };

    // 2. Envelopes for every device that may still read (the upload replaces, so repeating
    // is fine).
    let readers = readers(engine, store, &pending.members).await?;
    let envelopes = seal_all(
        &new_key,
        area,
        new_version,
        &readers.iter().collect::<Vec<_>>(),
    )?;
    upload(engine, store, area, &envelopes).await?;
    remember(engine, store, &manifest, &envelopes)?;

    // 3. Every record still under the old key, sealed again under the new one.
    let own = engine.own_devices(store).await?.0;
    let mut authors: HashMap<AccountPublic, Option<DeviceSet>> = HashMap::new();
    let mut since = 0;
    loop {
        let page = engine.client().records(area, since, PAGE).await;
        let (records, more) = engine.revoked_check(store, page)?;
        for (seq, record) in records {
            since = since.max(seq);
            if record.header.key_version != old.key_version {
                continue;
            }
            let author = record.header.author_account;
            if author != engine.account && !authors.contains_key(&author) {
                // An account deleted on the server has no log any more.
                let devices = engine.account_devices(store, &author).await.ok();
                authors.insert(author, devices);
            }
            let devices = if author == engine.account {
                Some(&own)
            } else {
                authors.get(&author).and_then(Option::as_ref)
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
            let body = match devices {
                // Records of devices revoked before do not open any more and are not carried
                // over.
                Some(devices) => match record.open(&old_key, old, devices) {
                    Ok(body) => body,
                    Err(_) => continue,
                },
                // The author's account is gone: what this device verified before stands in.
                None => match &known {
                    Some((kind, item, entry)) => {
                        let guard = lock(store)?;
                        encode(&local_item(&guard, vault, *kind, item, entry.modified_at)?)?
                    }
                    None => continue,
                },
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
                Err(err) => return engine.revoked_check(store, Err(err)),
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
    Ok(())
}

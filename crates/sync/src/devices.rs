//! The devices of this account, and revoking one with a fresh key for every area it could read.

use crate::SyncError;
use crate::engine::Engine;
use serde::Serialize;
use sshcraft_core::store::Store;
use sshcraft_core::vault::Vault;
use sshcraft_proto::devicelog::LogAction;
use sshcraft_proto::identity::is_recovery;
use std::sync::Mutex;
use uuid::Uuid;

pub use crate::areas::resume_rotation;

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

/// Revokes `target`, signed by this device, then rotates every area this account owns (the
/// personal one and its shares): new key, envelopes only for devices that may still read, every
/// record sealed again. The plan is stored (sealed) before the first step, and every step can be
/// repeated, so an interruption is finished by the next sync.
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
    let areas = engine
        .owned_areas(store)?
        .into_iter()
        .map(|(manifest, key)| {
            let members = manifest.members.clone();
            (manifest, key, members)
        })
        .collect();
    crate::areas::rotate(engine, store, vault, areas, Some(target), now).await
}

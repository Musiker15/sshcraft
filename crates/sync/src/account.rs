//! Setting a server up on this device: a new account, or joining an existing one with the
//! account key. Nothing is stored locally until every step on the server has worked.

use crate::SyncError;
use crate::client::ServerClient;
use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};
use sshcraft_model::model::ServerId;
use sshcraft_proto::area::{AreaId, AreaKey, AreaManifest, KeyEnvelope, open_area_key, seal_area_key};
use sshcraft_proto::devicelog::DeviceLog;
use sshcraft_proto::identity::{AccountKey, DeviceCertificate, DeviceKeys, RECOVERY_PLATFORM};
use sshcraft_store::store::{ServerRow, Store};
use sshcraft_store::vault::Vault;
use std::sync::Mutex;
use uuid::Uuid;
use zeroize::Zeroizing;

/// Name of the recovery device in the device log.
pub const RECOVERY_NAME: &str = "Account key";

pub struct NewAccount {
    pub server_name: String,
    pub origin: String,
    pub api_key: String,
    pub name: String,
    pub email: String,
    pub device_name: String,
    pub platform: String,
}

pub struct JoinAccount {
    pub server_name: String,
    pub origin: String,
    pub api_key: String,
    /// The exported account key as the user pasted it.
    pub account_key: String,
    pub device_name: String,
    pub platform: String,
}

/// Vault label of a server's device keys.
/// The setting that holds the id of the personal area on a server.
pub fn personal_setting(server: ServerId) -> String {
    format!("sync.personal.{}", server.0)
}

pub fn device_label(server: ServerId) -> String {
    format!("sync-device:{}", server.0)
}

/// Opens this device's keys for a server from the vault.
pub fn device_keys(vault: &Vault, row: &ServerRow) -> Result<DeviceKeys, SyncError> {
    let bytes = vault.open_secret(&device_label(row.id), &row.sealed_device)?;
    let array: &[u8; 64] = bytes.as_slice().try_into().map_err(|_| SyncError::Vault("device keys have the wrong length".into()))?;
    Ok(DeviceKeys::from_bytes(array)?)
}

/// Logins are signed for the server's own address, so the entered one must be the same.
async fn check_origin(client: &ServerClient) -> Result<(), SyncError> {
    let info = client.server_info().await?;
    if info.origin != client.origin() {
        return Err(SyncError::OriginMismatch { server: info.origin });
    }
    Ok(())
}

fn to_json(value: &impl serde::Serialize) -> Result<String, SyncError> {
    serde_json::to_string(value).map_err(|e| SyncError::Store(e.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn save_server(
    store: &Mutex<Store>,
    vault: &Vault,
    name: String,
    origin: &str,
    account: [u8; 32],
    device_id: Uuid,
    keys: &DeviceKeys,
    areas: &[(AreaManifest, KeyEnvelope)],
    now: i64,
) -> Result<ServerId, SyncError> {
    let id = ServerId::new();
    let sealed_device = vault.seal_secret(&device_label(id), &keys.to_bytes()[..])?;
    let store = store.lock().map_err(|e| SyncError::Store(e.to_string()))?;
    store.add_server(&ServerRow {
        id,
        name,
        origin: origin.to_owned(),
        account,
        device_id,
        sealed_device,
        sync_enabled: true,
        sync_keys: false,
        revoked: false,
        log_head: None,
        tracker: None,
        created_at: now,
    })?;
    for (manifest, envelope) in areas {
        store.put_area(id, manifest.area_id.0, &to_json(manifest)?, &to_json(envelope)?)?;
    }
    // With a single area that is the personal one; with more, the first sync finds it.
    if let [(manifest, _)] = areas {
        store.put_setting(&personal_setting(id), &manifest.area_id.0.to_string())?;
    }
    Ok(id)
}

/// How far setting up an account got.
#[derive(Debug)]
pub enum Setup {
    /// Ready to sync on this device.
    Ready(ServerId),
    /// The account exists on the server, but a later step failed. Nothing is stored on this
    /// device; joining with the account key finishes the setup (here or on another device).
    Unfinished(SyncError),
}

/// A personal area with envelopes for this device and the recovery device.
async fn personal_area(
    client: &ServerClient,
    account: &AccountKey,
    device_id: Uuid,
    keys: &DeviceKeys,
    own: &DeviceCertificate,
    recovery: &DeviceCertificate,
) -> Result<(AreaManifest, KeyEnvelope), SyncError> {
    let area = AreaId(Uuid::new_v4());
    let area_key = AreaKey::generate();
    let manifest = AreaManifest::sign(area, 1, 1, &area_key, account.public(), vec![], device_id, keys);
    client.put_manifest(&manifest).await?;
    let mine = seal_area_key(&area_key, area, 1, own)?;
    let for_recovery = seal_area_key(&area_key, area, 1, recovery)?;
    client.put_envelopes(area, &[mine.clone(), for_recovery]).await?;
    Ok((manifest, mine))
}

/// Creates an account on the server. Returns how far it got and the account key export, which
/// is shown to the user once and never stored. An error means nothing exists on the server.
pub async fn create_account(store: &Mutex<Store>, vault: &Vault, input: NewAccount, now: i64) -> Result<(Setup, Zeroizing<String>), SyncError> {
    let client = ServerClient::new(&input.origin)?.with_api_key(input.api_key.clone());
    check_origin(&client).await?;
    let account = AccountKey::generate();
    let keys = DeviceKeys::generate();
    let device_id = Uuid::new_v4();
    let cert = DeviceCertificate::issue(&account, device_id, keys.public(), input.device_name.trim(), &input.platform, now);
    let (recovery_id, recovery_keys) = account.recovery_device();
    let recovery = DeviceCertificate::issue(&account, recovery_id, recovery_keys.public(), RECOVERY_NAME, RECOVERY_PLATFORM, now);
    let mut log = DeviceLog::default();
    log.append_add(&account, cert.clone(), now);
    log.append_add(&account, recovery.clone(), now);
    // From here on the account exists: whatever fails later, the user gets the key.
    client.register(&account.public(), &input.name, &input.email, &log).await?;
    let export = account.export();

    let finish = async {
        let mut client = client;
        client.login(device_id, &keys).await?;
        let area = personal_area(&client, &account, device_id, &keys, &cert, &recovery).await?;
        save_server(store, vault, input.server_name, client.origin(), account.public().0, device_id, &keys, &[area], now)
    };
    let setup = match finish.await {
        Ok(id) => Setup::Ready(id),
        Err(err) => Setup::Unfinished(err),
    };
    Ok((setup, export))
}

/// Sets this device up on an existing account with its account key. Works without any other
/// device online: it logs in as the account's recovery device to read the log and the area
/// keys, adds itself, and seals the area keys to itself. If a later step fails, the half-added
/// device is revoked again, so retries leave no stray devices.
pub async fn join_account(store: &Mutex<Store>, vault: &Vault, input: JoinAccount, now: i64) -> Result<ServerId, SyncError> {
    let account = AccountKey::import(&input.account_key)?;
    let (recovery_id, recovery_keys) = account.recovery_device();
    check_origin(&ServerClient::new(&input.origin)?).await?;
    let mut recovery_client = ServerClient::new(&input.origin)?.with_api_key(input.api_key.clone());
    match recovery_client.login(recovery_id, &recovery_keys).await {
        Ok(_) => {}
        Err(SyncError::Server { status: 401, code, .. }) if code == "unauthorized" => {
            return Err(SyncError::AccountNotFound);
        }
        Err(err) => return Err(err),
    }

    let mut log = DeviceLog { entries: recovery_client.log(&account.public(), 0).await? };
    log.verify(&account.public())?;
    let recovery = log
        .entries
        .iter()
        .find_map(|entry| match &entry.action {
            sshcraft_proto::devicelog::LogAction::Add { certificate } if certificate.device_id == recovery_id => Some((**certificate).clone()),
            _ => None,
        })
        .ok_or(SyncError::AccountNotFound)?;
    let keys = DeviceKeys::generate();
    let device_id = Uuid::new_v4();
    let cert = DeviceCertificate::issue(&account, device_id, keys.public(), input.device_name.trim(), &input.platform, now);
    log.append_add(&account, cert.clone(), now);
    let entry = log.entries.last().cloned().expect("just appended");
    recovery_client.append_log(&account.public(), &[entry], false).await?;

    let finish = async {
        let owner_devices = log.verify(&account.public())?;
        let mut client = ServerClient::new(&input.origin)?.with_api_key(input.api_key.clone());
        client.login(device_id, &keys).await?;
        let mut areas = Vec::new();
        for listing in client.areas().await? {
            if listing.owner != account.public() {
                continue;
            }
            let manifest = listing.manifest;
            manifest.verify(&owner_devices)?;
            let Some(envelope) = recovery_client.my_envelope(manifest.area_id).await? else {
                continue;
            };
            let area_key = open_area_key(&envelope, recovery_id, &recovery_keys, &manifest)?;
            let own = seal_area_key(&area_key, manifest.area_id, manifest.key_version, &cert)?;
            client.put_envelopes(manifest.area_id, std::slice::from_ref(&own)).await?;
            areas.push((manifest, own));
        }
        // An account whose setup never got that far has no personal area yet.
        if areas.is_empty() {
            areas.push(personal_area(&client, &account, device_id, &keys, &cert, &recovery).await?);
        }
        save_server(store, vault, input.server_name.clone(), client.origin(), account.public().0, device_id, &keys, &areas, now)
    };
    match finish.await {
        Ok(id) => Ok(id),
        Err(err) => {
            // Best effort: take the half-added device out again (signed by the recovery device).
            log.append_revoke_by_device(recovery_id, &recovery_keys, device_id, now);
            if let Some(entry) = log.entries.last().cloned() {
                let _ = recovery_client.append_log(&account.public(), &[entry], false).await;
            }
            Err(err)
        }
    }
}

/// The account key as a QR code (SVG), for a phone to scan.
pub fn account_key_qr_svg(export: &str) -> String {
    match QrCode::with_error_correction_level(export.as_bytes(), EcLevel::M) {
        Ok(code) => code.render::<svg::Color<'_>>().min_dimensions(240, 240).quiet_zone(true).build(),
        Err(_) => String::new(),
    }
}

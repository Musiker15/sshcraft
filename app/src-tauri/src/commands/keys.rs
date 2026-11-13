//! Key management. Only public facts (`KeyInfo`) cross into the UI.

use serde::{Deserialize, Serialize};
use sshcraft_core::authorized_keys::InstallOutcome;
use sshcraft_core::keytypes::{KeyKind, KeySpec};
use sshcraft_core::model::{HostId, KeyId, KeyRef};
use sshcraft_core::store::ItemKind;
use sshcraft_fido::{Attachment, CreateOptions, SkAlgorithm};
use std::sync::Arc;

use sshcraft_core::vault::{self, KeyInfo, Vault};
use tauri::State;
use zeroize::Zeroizing;

use super::{CommandError, CommandResult, ErrorCode};
use crate::passcli::{self, PassCli, PassCliError, PassCliProcess, PassCliStatus, PassKey};
use crate::state::AppState;

#[derive(Serialize)]
pub struct VaultStatus {
    ok: bool,
    code: Option<ErrorCode>,
    detail: Option<String>,
}

#[tauri::command]
pub fn vault_status(state: State<'_, AppState>) -> VaultStatus {
    match &state.vault {
        Ok(_) => VaultStatus {
            ok: true,
            code: None,
            detail: None,
        },
        Err(e) => VaultStatus {
            ok: false,
            code: Some(e.code),
            detail: e.detail.clone(),
        },
    }
}

fn unlocked(state: &AppState) -> CommandResult<Arc<Vault>> {
    state.vault.clone().map_err(|e| CommandError {
        code: ErrorCode::VaultLocked,
        detail: e.detail,
    })
}

/// Runs blocking key work off the main thread, so the window and terminals stay responsive.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| CommandError::new(ErrorCode::Internal, e))?
}

#[derive(Serialize)]
pub struct HelloStatus {
    missing: bool,
}

#[tauri::command]
pub fn hello_status(state: State<'_, AppState>) -> HelloStatus {
    HelloStatus {
        missing: state.sessions.hello_missing(),
    }
}

#[tauri::command]
pub fn list_keys(state: State<'_, AppState>) -> CommandResult<Vec<KeyInfo>> {
    Ok(vault::list_keys(&state.store.lock().expect("store lock"))?)
}

/// Imports a key. With `save_passphrase == false` an encrypted key stays encrypted with its
/// passphrase, which is then asked on each use.
#[tauri::command]
pub async fn import_key(
    state: State<'_, AppState>,
    name: String,
    text: String,
    passphrase: Option<String>,
    save_passphrase: Option<bool>,
) -> CommandResult<KeyInfo> {
    let vault = unlocked(&state)?;
    let store = state.store.clone();
    let text = Zeroizing::new(text);
    let passphrase = passphrase.map(Zeroizing::new).filter(|p| !p.is_empty());
    blocking(move || {
        let key = vault::parse_private_key(&text, passphrase.as_deref().map(String::as_str))?;
        let store = store.lock().expect("store lock");
        match passphrase {
            Some(p) if save_passphrase == Some(false) => {
                Ok(vault.store_protected(&store, &name, &key, &p)?)
            }
            _ => Ok(vault.store_key(&store, &name, &key)?),
        }
    })
    .await
}

#[tauri::command]
pub async fn generate_key(
    state: State<'_, AppState>,
    name: String,
    spec: KeySpec,
    passphrase: Option<String>,
) -> CommandResult<KeyInfo> {
    let vault = unlocked(&state)?;
    let store = state.store.clone();
    let passphrase = passphrase.map(Zeroizing::new);
    blocking(move || {
        let store = store.lock().expect("store lock");
        Ok(vault.generate(
            &store,
            &name,
            &spec,
            passphrase.as_deref().map(String::as_str),
        )?)
    })
    .await
}

#[derive(Serialize)]
pub struct SecuritySupport {
    fido2: bool,
    hello: bool,
}

/// Which security keys this device can create.
#[tauri::command]
pub async fn security_key_support(state: State<'_, AppState>) -> CommandResult<SecuritySupport> {
    let fido2 = state.security.is_some();
    #[cfg(windows)]
    let hello = fido2 && blocking(|| Ok(sshcraft_fido::webauthn::platform_available())).await?;
    #[cfg(not(windows))]
    let hello = false;
    Ok(SecuritySupport { fido2, hello })
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityKeyKind {
    /// A token you plug in (YubiKey and the like). Synced like any key.
    Fido2,
    /// Windows Hello: bound to this device, never synced.
    Hello,
}

/// Creates a key on a FIDO2 token or in Windows Hello. Windows shows its own dialog.
#[tauri::command]
pub async fn create_security_key(
    state: State<'_, AppState>,
    name: String,
    kind: SecurityKeyKind,
    algorithm: SkAlgorithm,
    verify_user: bool,
) -> CommandResult<KeyInfo> {
    let vault = unlocked(&state)?;
    let auth = state
        .security
        .clone()
        .ok_or_else(|| CommandError::code(ErrorCode::NotSupported))?;
    let options = match kind {
        SecurityKeyKind::Fido2 => CreateOptions {
            algorithm,
            attachment: Attachment::CrossPlatform,
            verify_user,
            user_name: name.clone(),
        },
        // Windows Hello holds only P-256 keys and always checks the user.
        SecurityKeyKind::Hello => CreateOptions {
            algorithm: SkAlgorithm::EcdsaP256,
            attachment: Attachment::Platform,
            verify_user: true,
            user_name: name.clone(),
        },
    };
    let key = sshcraft_fido::create_key(auth.as_ref(), &options, &name).await?;
    let store = state.store.lock().expect("store lock");
    Ok(match kind {
        SecurityKeyKind::Fido2 => vault.store_key(&store, &name, &key)?,
        SecurityKeyKind::Hello => vault.store_device_key(&store, &name, &key)?,
    })
}

/// A key changed: queue it for its server, if it has one.
pub(crate) fn key_changed(
    store: &sshcraft_core::store::Store,
    id: KeyId,
) -> CommandResult<Vec<sshcraft_core::model::ServerId>> {
    let key = id.0.to_string();
    let location = store.location_of(ItemKind::Key, &key)?;
    crate::sync::note_change(store, ItemKind::Key, &key, location, crate::sync::now_ms())
}

/// Runs `change` on a key the user owns, then queues it for sync.
/// Runs off the async threads: a passphrase change runs bcrypt, which can take a second.
async fn change_key(
    state: &AppState,
    id: KeyId,
    change: impl FnOnce(&Vault, &sshcraft_core::store::Store) -> Result<KeyInfo, vault::VaultError>
    + Send
    + 'static,
) -> CommandResult<KeyInfo> {
    let vault = unlocked(state)?;
    let store = state.store.clone();
    let (info, servers) = blocking(move || {
        let store = store.lock().expect("store lock");
        if let Some(server) = store.location_of(ItemKind::Key, &id.0.to_string())? {
            crate::sync::share_rights_for_key(&store, server, id)?;
        }
        let info = change(&vault, &store)?;
        let servers = key_changed(&store, id)?;
        Ok((info, servers))
    })
    .await?;
    state.sync.nudge(&servers);
    Ok(info)
}

#[tauri::command]
pub async fn attach_certificate(
    state: State<'_, AppState>,
    id: KeyId,
    text: String,
) -> CommandResult<KeyInfo> {
    change_key(&state, id, move |vault, store| {
        vault.attach_certificate(store, id, &text)
    })
    .await
}

#[tauri::command]
pub async fn detach_certificate(state: State<'_, AppState>, id: KeyId) -> CommandResult<KeyInfo> {
    change_key(&state, id, move |vault, store| {
        vault.detach_certificate(store, id)
    })
    .await
}

/// Saves the passphrase from now on (the key is kept decrypted inside the vault).
#[tauri::command]
pub async fn remember_passphrase(
    state: State<'_, AppState>,
    id: KeyId,
    passphrase: String,
) -> CommandResult<KeyInfo> {
    let passphrase = Zeroizing::new(passphrase);
    change_key(&state, id, move |vault, store| {
        vault.remember_passphrase(store, id, &passphrase)
    })
    .await
}

/// Stops saving the passphrase: the key is kept encrypted with `passphrase`.
#[tauri::command]
pub async fn protect_key(
    state: State<'_, AppState>,
    id: KeyId,
    passphrase: String,
) -> CommandResult<KeyInfo> {
    let passphrase = Zeroizing::new(passphrase);
    change_key(&state, id, move |vault, store| {
        vault.protect_key(store, id, &passphrase)
    })
    .await
}

/// Puts the public half of `key` into `authorized_keys` on the host.
#[tauri::command]
pub async fn install_key(
    state: State<'_, AppState>,
    host_id: HostId,
    key: KeyRef,
) -> CommandResult<InstallOutcome> {
    state
        .sessions
        .install_key(host_id, key)
        .await
        .map_err(|(code, detail)| CommandError { code, detail })
}

#[tauri::command]
pub async fn pass_cli_status() -> CommandResult<PassCliStatus> {
    blocking(|| Ok(passcli::local_status())).await
}

#[tauri::command]
pub async fn pass_cli_list_keys() -> CommandResult<Vec<PassKey>> {
    blocking(|| {
        let cli = PassCliProcess::locate().ok_or(PassCliError::NotInstalled)?;
        Ok(cli.list_ssh_keys()?)
    })
    .await
}

#[tauri::command]
pub async fn import_from_pass_cli(
    state: State<'_, AppState>,
    share_id: String,
    item_id: String,
    name: String,
) -> CommandResult<KeyInfo> {
    let vault = unlocked(&state)?;
    let store = state.store.clone();
    blocking(move || {
        let cli = PassCliProcess::locate().ok_or(PassCliError::NotInstalled)?;
        Ok(passcli::import_from_pass_cli(
            &cli, &vault, &store, &share_id, &item_id, &name,
        )?)
    })
    .await
}

#[tauri::command]
pub fn delete_key(state: State<'_, AppState>, id: KeyId) -> CommandResult<()> {
    let servers = {
        let store = state.store.lock().expect("store lock");
        let kind = sshcraft_core::store::ItemKind::Key;
        let key = id.0.to_string();
        let before = store.location_of(kind, &key)?;
        if let Some(server) = before {
            crate::sync::share_rights_for_key(&store, server, id)?;
        }
        let servers = crate::sync::note_change(&store, kind, &key, before, crate::sync::now_ms())?;
        // A Windows Hello key also goes from Windows Hello; if that fails, it goes from here.
        #[cfg(windows)]
        if let Some(record) = store.key_record(id)?
            && record.kind == KeyKind::Device
            && let Ok(vault) = unlocked(&state)
            && let Ok(text) = vault.stored_openssh(&store, id)
            && let Ok(stored) = russh::keys::PrivateKey::from_openssh(text.as_str())
        {
            let handle = match stored.key_data() {
                russh::keys::ssh_key::private::KeypairData::SkEcdsaSha2NistP256(sk) => {
                    sk.key_handle().to_vec()
                }
                russh::keys::ssh_key::private::KeypairData::SkEd25519(sk) => {
                    sk.key_handle().to_vec()
                }
                _ => Vec::new(),
            };
            let _ = sshcraft_fido::webauthn::delete_platform_credential(&handle);
        }
        vault::delete_key(&store, id)?;
        servers
    };
    state.sync.nudge(&servers);
    Ok(())
}

/// Keys the Proton Pass agent offers. Only public facts, never key material.
#[tauri::command]
pub async fn list_agent_keys() -> CommandResult<Vec<sshcraft_ssh::AgentKey>> {
    use crate::sessions::{AgentConnector, WindowsAgent};
    let mut agent = WindowsAgent
        .connect()
        .await
        .map_err(|e| CommandError::new(ErrorCode::AgentUnavailable, e))?;
    sshcraft_ssh::list_agent_keys(&mut agent)
        .await
        .map_err(|e| CommandError::new(ErrorCode::AgentUnavailable, e))
}

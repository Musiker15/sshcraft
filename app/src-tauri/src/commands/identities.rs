//! Identities: reusable user and credentials for hosts and groups.

use serde::{Deserialize, Serialize};
use sshcraft_core::model::{Identity, IdentityId, KeyRef, ServerId};
use sshcraft_core::store::{ItemKind, Store};
use sshcraft_core::vault::Vault;
use tauri::State;

use super::passwords::{
    PasswordChange, PasswordOwner, Touched, clear_password, drop_passwords, queue, set_password,
};
use super::{CommandError, CommandResult, ErrorCode};
use crate::state::AppState;

/// An identity as the UI sees it: whether it has a password, never which.
#[derive(Debug, Clone, Serialize)]
pub struct IdentityView {
    pub id: IdentityId,
    pub name: String,
    pub username: Option<String>,
    pub key: Option<KeyRef>,
    pub location: Option<ServerId>,
    pub has_password: bool,
}

impl From<Identity> for IdentityView {
    fn from(i: Identity) -> Self {
        Self {
            id: i.id,
            name: i.name,
            username: i.username,
            key: i.key,
            location: i.location,
            has_password: i.password.is_some(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdentityInput {
    pub id: Option<IdentityId>,
    pub name: String,
    pub username: Option<String>,
    pub key: Option<KeyRef>,
    /// Where a new identity lives; an existing one stays where it is.
    pub location: Option<ServerId>,
    pub password: PasswordChange,
}

fn find(store: &Store, id: IdentityId) -> Result<Option<Identity>, CommandError> {
    Ok(store.list_identities()?.into_iter().find(|i| i.id == id))
}

/// An identity that came in someone else's share is theirs.
fn check_rights(store: &Store, id: IdentityId) -> Result<(), CommandError> {
    let key = id.0.to_string();
    if let Some(server) = store.location_of(ItemKind::Identity, &key)?
        && store
            .received_share_of(server, ItemKind::Identity, &key)?
            .is_some()
    {
        return Err(CommandError::code(ErrorCode::ShareReadOnly));
    }
    Ok(())
}

pub(crate) fn save_identity(
    store: &Store,
    vault: Option<&Vault>,
    input: IdentityInput,
) -> Result<(IdentityView, Touched), CommandError> {
    let id = input.id.unwrap_or_default();
    let existing = find(store, id)?;
    if existing.is_some() {
        check_rights(store, id)?;
    }
    let key = id.0.to_string();
    let mut touched = vec![(
        ItemKind::Identity,
        key.clone(),
        store.location_of(ItemKind::Identity, &key)?,
    )];
    let identity = Identity {
        id,
        name: input.name.trim().to_owned(),
        username: input
            .username
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty()),
        key: input.key,
        password: existing.as_ref().and_then(|e| e.password),
        location: existing.as_ref().map_or(input.location, |e| e.location),
    };
    store.upsert_identity(&identity)?;
    let owner = PasswordOwner::Identity { id };
    match input.password {
        PasswordChange::Keep => {}
        PasswordChange::Set { secret } => {
            let secret = zeroize::Zeroizing::new(secret);
            let vault = vault.ok_or_else(|| CommandError::code(ErrorCode::VaultLocked))?;
            touched.extend(set_password(store, vault, owner, &secret)?);
        }
        PasswordChange::Clear => touched.extend(clear_password(store, owner)?),
    }
    let saved = find(store, id)?.ok_or_else(|| CommandError::code(ErrorCode::NotFound))?;
    Ok((saved.into(), touched))
}

pub(crate) fn delete_identity(store: &Store, id: IdentityId) -> Result<Touched, CommandError> {
    let Some(identity) = find(store, id)? else {
        return Ok(Vec::new());
    };
    check_rights(store, id)?;
    let key = id.0.to_string();
    let mut touched = vec![(
        ItemKind::Identity,
        key.clone(),
        store.location_of(ItemKind::Identity, &key)?,
    )];
    store.delete_identity(id)?;
    touched.extend(drop_passwords(
        store,
        &identity.password.into_iter().collect::<Vec<_>>(),
    )?);
    Ok(touched)
}

fn store(state: &AppState) -> std::sync::MutexGuard<'_, Store> {
    state.store.lock().expect("store lock")
}

#[tauri::command]
pub fn identities_list(state: State<'_, AppState>) -> CommandResult<Vec<IdentityView>> {
    Ok(store(&state)
        .list_identities()?
        .into_iter()
        .map(IdentityView::from)
        .collect())
}

#[tauri::command]
pub fn identity_save(
    state: State<'_, AppState>,
    identity: IdentityInput,
) -> CommandResult<IdentityView> {
    let vault = state.vault.clone().ok();
    let (view, servers) = {
        let store = store(&state);
        let (view, touched) = save_identity(&store, vault.as_deref(), identity)?;
        (view, queue(&store, touched)?)
    };
    state.sync.nudge(&servers);
    Ok(view)
}

#[tauri::command]
pub fn identity_delete(state: State<'_, AppState>, id: IdentityId) -> CommandResult<()> {
    let servers = {
        let store = store(&state);
        let touched = delete_identity(&store, id)?;
        queue(&store, touched)?
    };
    state.sync.nudge(&servers);
    Ok(())
}

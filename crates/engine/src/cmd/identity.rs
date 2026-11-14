//! Identities: reusable user and credentials for hosts and groups.

use serde::{Deserialize, Serialize};
use sshcraft_model::model::{Identity, IdentityId, KeyRef, ServerId};
use sshcraft_store::store::{ItemKind, Store};
use sshcraft_store::vault::Vault;

use super::password::{PasswordChange, PasswordOwner, Touched, clear_password, drop_passwords, queue, set_password};
use crate::cmd::CommandError;
use crate::error::ErrorCode;

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
        Self { id: i.id, name: i.name, username: i.username, key: i.key, location: i.location, has_password: i.password.is_some() }
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
        && store.received_share_of(server, ItemKind::Identity, &key)?.is_some()
    {
        return Err(CommandError::code(ErrorCode::ShareReadOnly));
    }
    Ok(())
}

pub(crate) fn save_identity(store: &Store, vault: Option<&Vault>, input: IdentityInput) -> Result<(IdentityView, Touched), CommandError> {
    let id = input.id.unwrap_or_default();
    // Nothing is written when the password cannot be stored.
    if matches!(input.password, PasswordChange::Set { .. }) && vault.is_none() {
        return Err(CommandError::code(ErrorCode::VaultLocked));
    }
    let existing = find(store, id)?;
    if existing.is_some() {
        check_rights(store, id)?;
    }
    let key = id.0.to_string();
    let mut touched = vec![(ItemKind::Identity, key.clone(), store.location_of(ItemKind::Identity, &key)?)];
    let identity = Identity {
        id,
        name: input.name.trim().to_owned(),
        username: input.username.map(|u| u.trim().to_owned()).filter(|u| !u.is_empty()),
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
    let mut touched = vec![(ItemKind::Identity, key.clone(), store.location_of(ItemKind::Identity, &key)?)];
    store.delete_identity(id)?;
    touched.extend(drop_passwords(store, &identity.password.into_iter().collect::<Vec<_>>())?);
    Ok(touched)
}

// --- Commands ---

use serde_json::Value;

use super::{CommandSpec, command, parse, query, sync_run, to_value};
use crate::Ctx;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query("identity.list", "Identities", "none", run_list),
        command(
            "identity.save",
            "Save an identity",
            "{ identity: { id?, name, username?, key?, location?, password: { kind: keep | set | clear, secret? } } }",
            run_save,
        ),
        command("identity.delete", "Delete an identity", "{ id }", run_delete),
    ]
}

#[derive(Deserialize)]
struct SaveParams {
    identity: IdentityInput,
}

#[derive(Deserialize)]
struct DeleteParams {
    id: IdentityId,
}

fn run_list(ctx: Ctx, _: Value) -> Out {
    sync_run(move || {
        let list: Vec<IdentityView> = ctx.session.store().list_identities()?.into_iter().map(IdentityView::from).collect();
        to_value(&list)
    })
}

fn run_save(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let p: SaveParams = parse("identity.save", params)?;
        let vault = ctx.session.vault().ok();
        let (view, servers) = {
            let store = ctx.session.store();
            let (view, touched) = save_identity(&store, vault.as_deref(), p.identity)?;
            (view, queue(&store, touched)?)
        };
        ctx.session.sync().nudge(&servers);
        ctx.session.announce(&servers);
        to_value(&view)
    })
}

fn run_delete(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let p: DeleteParams = parse("identity.delete", params)?;
        let servers = {
            let store = ctx.session.store();
            let touched = delete_identity(&store, p.id)?;
            queue(&store, touched)?
        };
        ctx.session.sync().nudge(&servers);
        ctx.session.announce(&servers);
        Ok(Value::Null)
    })
}

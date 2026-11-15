//! Stored passwords: set or cleared for a host, a group, their proxies or an identity. No
//! command ever returns a password, nor the id it is stored under: views only say whether one
//! is set.

use serde::{Deserialize, Serialize};
use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId, IdentityId, PasswordId, Proxy, ServerId};
use sshcraft_store::store::{ItemKind, Store};
use sshcraft_store::vault::Vault;

use crate::cmd::{CommandError, CommandResult};
use crate::error::ErrorCode;
use crate::sync::{note_change, now_ms};

/// Whose password it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PasswordOwner {
    Host {
        id: HostId,
    },
    Group {
        id: GroupId,
    },
    HostProxy {
        id: HostId,
    },
    /// The TOTP secret of a host.
    HostTotp {
        id: HostId,
    },
    GroupProxy {
        id: GroupId,
    },
    Identity {
        id: IdentityId,
    },
}

/// What a form wants done with a password.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PasswordChange {
    Keep,
    Set { secret: String },
    Clear,
}

/// Items a change touched, with where each lived before, to queue for sync afterwards.
pub(crate) type Touched = Vec<(ItemKind, String, Option<ServerId>)>;

fn not_found() -> CommandError {
    CommandError::code(ErrorCode::NotFound)
}

fn owner_item(owner: PasswordOwner) -> (ItemKind, String) {
    match owner {
        PasswordOwner::Host { id } | PasswordOwner::HostProxy { id } | PasswordOwner::HostTotp { id } => (ItemKind::Host, id.0.to_string()),
        PasswordOwner::Group { id } | PasswordOwner::GroupProxy { id } => (ItemKind::Group, id.0.to_string()),
        PasswordOwner::Identity { id } => (ItemKind::Identity, id.0.to_string()),
    }
}

fn proxy_password(settings: &ConnectionSettings) -> Result<Option<PasswordId>, CommandError> {
    match &settings.proxy {
        Some(Proxy::Http(e) | Proxy::Socks5(e)) => Ok(e.password),
        _ => Err(not_found()),
    }
}

fn set_proxy_password(settings: &mut ConnectionSettings, id: Option<PasswordId>) {
    if let Some(Proxy::Http(e) | Proxy::Socks5(e)) = &mut settings.proxy {
        e.password = id;
    }
}

fn host(store: &Store, id: HostId) -> Result<Host, CommandError> {
    store.list_hosts()?.into_iter().find(|h| h.id == id).ok_or_else(not_found)
}

fn group(store: &Store, id: GroupId) -> Result<Group, CommandError> {
    store.list_groups()?.into_iter().find(|g| g.id == id).ok_or_else(not_found)
}

/// The owner's current password and where the owner lives.
fn current(store: &Store, owner: PasswordOwner) -> Result<(Option<PasswordId>, Option<ServerId>), CommandError> {
    Ok(match owner {
        PasswordOwner::Host { id } => {
            let h = host(store, id)?;
            (h.settings.password, h.location)
        }
        PasswordOwner::HostProxy { id } => {
            let h = host(store, id)?;
            (proxy_password(&h.settings)?, h.location)
        }
        PasswordOwner::HostTotp { id } => {
            let h = host(store, id)?;
            (h.settings.totp, h.location)
        }
        PasswordOwner::Group { id } => {
            let g = group(store, id)?;
            (g.defaults.password, g.location)
        }
        PasswordOwner::GroupProxy { id } => {
            let g = group(store, id)?;
            (proxy_password(&g.defaults)?, g.location)
        }
        PasswordOwner::Identity { id } => {
            let i = store.list_identities()?.into_iter().find(|i| i.id == id).ok_or_else(not_found)?;
            (i.password, i.location)
        }
    })
}

fn point_to(store: &Store, owner: PasswordOwner, password: Option<PasswordId>) -> Result<(), CommandError> {
    match owner {
        PasswordOwner::Host { id } => {
            let mut h = host(store, id)?;
            h.settings.password = password;
            store.upsert_host(&h)?;
        }
        PasswordOwner::HostProxy { id } => {
            let mut h = host(store, id)?;
            set_proxy_password(&mut h.settings, password);
            store.upsert_host(&h)?;
        }
        PasswordOwner::HostTotp { id } => {
            let mut h = host(store, id)?;
            h.settings.totp = password;
            store.upsert_host(&h)?;
        }
        PasswordOwner::Group { id } => {
            let mut g = group(store, id)?;
            g.defaults.password = password;
            store.upsert_group(&g)?;
        }
        PasswordOwner::GroupProxy { id } => {
            let mut g = group(store, id)?;
            set_proxy_password(&mut g.defaults, password);
            store.upsert_group(&g)?;
        }
        PasswordOwner::Identity { id } => {
            let mut i = store.list_identities()?.into_iter().find(|i| i.id == id).ok_or_else(not_found)?;
            i.password = password;
            store.upsert_identity(&i)?;
        }
    }
    Ok(())
}

/// A password that came in someone else's share, or belongs to an item that did, is theirs.
fn check_rights(store: &Store, owner: PasswordOwner, password: Option<PasswordId>) -> Result<(), CommandError> {
    use sshcraft_store::store::ShareRole;
    let refused = || CommandError::code(ErrorCode::ShareReadOnly);
    // An item of a share with read only, or a password that came with a share: not this
    // account's. With write, an own password for a shared item stays on this account's side.
    let (kind, id) = owner_item(owner);
    if let Some(server) = store.location_of(kind, &id)?
        && let Some(share) = store.received_share_of(server, kind, &id)?
        && share.role != Some(ShareRole::Write)
    {
        return Err(refused());
    }
    if let Some(p) = password {
        let id = p.0.to_string();
        if let Some(server) = store.location_of(ItemKind::Password, &id)?
            && store.received_share_of(server, ItemKind::Password, &id)?.is_some()
        {
            return Err(refused());
        }
    }
    Ok(())
}

fn before(store: &Store, kind: ItemKind, id: &str) -> Result<Touched, CommandError> {
    Ok(vec![(kind, id.to_owned(), store.location_of(kind, id)?)])
}

/// Stores `secret` as the owner's password (under its existing id, or a new one) where the owner
/// lives.
pub(crate) fn set_password(store: &Store, vault: &Vault, owner: PasswordOwner, secret: &str) -> Result<Touched, CommandError> {
    let (existing, location) = current(store, owner)?;
    check_rights(store, owner, existing)?;
    let id = existing.unwrap_or_default();
    let (kind, item) = owner_item(owner);
    let mut touched = before(store, ItemKind::Password, &id.0.to_string())?;
    touched.extend(before(store, kind, &item)?);
    vault.store_password(store, id, secret, location)?;
    if existing.is_none() {
        point_to(store, owner, Some(id))?;
    }
    Ok(touched)
}

/// Removes the owner's password.
pub(crate) fn clear_password(store: &Store, owner: PasswordOwner) -> Result<Touched, CommandError> {
    let (existing, _) = current(store, owner)?;
    check_rights(store, owner, existing)?;
    let Some(id) = existing else {
        return Ok(Vec::new());
    };
    let (kind, item) = owner_item(owner);
    let mut touched = before(store, ItemKind::Password, &id.0.to_string())?;
    touched.extend(before(store, kind, &item)?);
    point_to(store, owner, None)?;
    store.delete_password(id)?;
    Ok(touched)
}

/// Removes passwords nothing refers to any more (a host or group deleted, a proxy switched off);
/// returns them to queue.
pub(crate) fn drop_passwords(store: &Store, ids: &[PasswordId]) -> Result<Touched, CommandError> {
    let mut touched = Vec::new();
    for id in ids {
        touched.extend(before(store, ItemKind::Password, &id.0.to_string())?);
        store.delete_password(*id)?;
    }
    Ok(touched)
}

/// Queues what a change touched; returns the servers to wake (after the store is unlocked).
pub(crate) fn queue(store: &Store, touched: Touched) -> CommandResult<Vec<ServerId>> {
    let mut servers = Vec::new();
    for (kind, id, was) in touched {
        servers.extend(note_change(store, kind, &id, was, now_ms())?);
    }
    Ok(servers)
}

/// Settings as the UI sees them: without password ids.
pub(crate) fn hide_passwords(settings: &mut ConnectionSettings) {
    settings.password = None;
    settings.totp = None;
    set_proxy_password(settings, None);
}

/// A form sends settings without password ids: the stored ones stay. A proxy password whose
/// proxy is gone is returned to be dropped.
pub(crate) fn keep_passwords(stored: Option<&ConnectionSettings>, incoming: &mut ConnectionSettings) -> Vec<PasswordId> {
    incoming.password = stored.and_then(|s| s.password);
    incoming.totp = stored.and_then(|s| s.totp);
    let old_proxy = stored.and_then(|s| proxy_password(s).ok().flatten());
    let has_proxy = matches!(incoming.proxy, Some(Proxy::Http(_) | Proxy::Socks5(_)));
    // A proxy that now leads elsewhere does not take the old proxy's password along.
    let same = stored.is_some_and(|s| sshcraft_sync::creds::same_proxy_endpoint(&s.proxy, &incoming.proxy));
    if has_proxy && same {
        set_proxy_password(incoming, old_proxy);
        Vec::new()
    } else {
        old_proxy.into_iter().collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HostView {
    #[serde(flatten)]
    pub host: Host,
    pub has_password: bool,
    pub proxy_has_password: bool,
    pub has_totp: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupView {
    #[serde(flatten)]
    pub group: Group,
    pub has_password: bool,
    pub proxy_has_password: bool,
}

fn flags(settings: &ConnectionSettings) -> (bool, bool) {
    (settings.password.is_some(), proxy_password(settings).ok().flatten().is_some())
}

pub(crate) fn host_view(mut host: Host) -> HostView {
    let (has_password, proxy_has_password) = flags(&host.settings);
    let has_totp = host.settings.totp.is_some();
    hide_passwords(&mut host.settings);
    HostView { host, has_password, proxy_has_password, has_totp }
}

pub(crate) fn group_view(mut group: Group) -> GroupView {
    let (has_password, proxy_has_password) = flags(&group.defaults);
    hide_passwords(&mut group.defaults);
    GroupView { group, has_password, proxy_has_password }
}

// --- Commands ---

use serde_json::Value;

use super::{CommandSpec, command, needs_vault, parse, sync_run};
use crate::Ctx;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            enabled: needs_vault,
            ..command(
                "password.set",
                "Set a password",
                "{ owner: { kind: host | group | host_proxy | group_proxy | identity, id }, secret }",
                run_set,
            )
        },
        command("password.clear", "Clear a password", "{ owner: { kind: host | group | host_proxy | group_proxy | identity, id } }", run_clear),
    ]
}

#[derive(Deserialize)]
struct SetParams {
    owner: PasswordOwner,
    secret: String,
}

#[derive(Deserialize)]
struct ClearParams {
    owner: PasswordOwner,
}

fn run_set(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let p: SetParams = parse("password.set", params)?;
        let secret = zeroize::Zeroizing::new(p.secret);
        // A TOTP secret that is not Base32 would only fail when connecting.
        if matches!(p.owner, PasswordOwner::HostTotp { .. }) && sshcraft_model::totp::decode_secret(&secret).is_none() {
            return Err(CommandError::bad_params("password.set", "the TOTP secret must be Base32 or an otpauth:// link"));
        }
        let vault = ctx.session.vault()?;
        let servers = {
            let store = ctx.session.store();
            let touched = set_password(&store, &vault, p.owner, &secret)?;
            queue(&store, touched)?
        };
        ctx.session.sync().nudge(&servers);
        ctx.session.announce(&servers);
        Ok(Value::Null)
    })
}

fn run_clear(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let p: ClearParams = parse("password.clear", params)?;
        let servers = {
            let store = ctx.session.store();
            let touched = clear_password(&store, p.owner)?;
            queue(&store, touched)?
        };
        ctx.session.sync().nudge(&servers);
        ctx.session.announce(&servers);
        Ok(Value::Null)
    })
}

#[cfg(test)]
#[path = "password_tests.rs"]
mod tests;

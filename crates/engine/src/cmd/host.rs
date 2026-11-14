//! Hosts and stored host keys, and the bookkeeping that hosts and groups share: queueing a change
//! for the sync server the item lives at, checking received shares, dropping passwords nothing
//! refers to any more.

use serde::Deserialize;
use serde_json::Value;
use sshcraft_model::model::{Group, Host, HostId, PasswordId, effective_endpoint, group_chain};
use sshcraft_store::store::{ItemKind, Store};

use super::password::{HostView, drop_passwords, host_view, keep_passwords, queue};
use super::{CommandError, CommandResult, CommandSpec, command, parse, query, sync_run, to_value};
use crate::error::ErrorCode;
use crate::sync::{Change, note_change, now_ms, share_rights};
use crate::{Ctx, Session};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query("host.list", "Hosts", "none", run_list),
        command("host.save", "Save a host", "{ host: the host as host.list returns it, without password flags }", run_save),
        command("host.delete", "Delete a host", "{ id }", run_delete),
        query("host.recent", "Recent hosts", "{ limit: at most 100 }", run_recent),
        command("host.forget_key", "Forget the host key", "{ host_id }", run_forget_key),
        query("host.info", "Host details", "{ host: id or unique name }", run_info),
    ]
}

/// Removes passwords nothing refers to any more and queues their removal; returns the servers.
pub(crate) fn drop_orphans(session: &Session, ids: &[PasswordId]) -> CommandResult<Vec<sshcraft_model::model::ServerId>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let servers = {
        let store = session.store();
        let touched = drop_passwords(&store, ids)?;
        queue(&store, touched)?
    };
    session.sync().nudge(&servers);
    Ok(servers)
}

/// Writes to the store, then queues the item for the server it lives at (or left).
pub(crate) fn changed(
    session: &Session,
    kind: ItemKind,
    id: String,
    write: impl FnOnce(&Store) -> Result<(), CommandError>,
) -> CommandResult<Vec<sshcraft_model::model::ServerId>> {
    let servers = {
        let store = session.store();
        let before = store.location_of(kind, &id)?;
        // A delete is queued before it happens: afterwards the item's location is gone.
        let early = note_change(&store, kind, &id, before, now_ms())?;
        write(&store)?;
        let mut servers = note_change(&store, kind, &id, before, now_ms())?;
        servers.extend(early);
        servers
    };
    session.sync().nudge(&servers);
    Ok(servers)
}

/// Checks the change against shares this account received; a change that stays on this device
/// (a key assigned to a read-only item) is written without queueing.
pub(crate) fn checked(
    session: &Session,
    change: Change<'_>,
    kind: ItemKind,
    id: String,
    write: impl FnOnce(&Store) -> Result<(), CommandError>,
) -> CommandResult<Vec<sshcraft_model::model::ServerId>> {
    let send = share_rights(&session.store(), change)?;
    if send {
        changed(session, kind, id, write)
    } else {
        write(&session.store())?;
        Ok(Vec::new())
    }
}

fn run_list(ctx: Ctx, _: Value) -> Out {
    sync_run(move || {
        let list: Vec<HostView> = ctx.session.store().list_hosts()?.into_iter().map(host_view).collect();
        to_value(&list)
    })
}

#[derive(Deserialize)]
struct SaveParams {
    host: Host,
}

fn run_save(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let SaveParams { mut host } = parse("host.save", params)?;
        let id = host.id.0.to_string();
        let orphans = {
            let store = ctx.session.store();
            let stored = store.list_hosts()?.into_iter().find(|h| h.id == host.id);
            keep_passwords(stored.as_ref().map(|h| &h.settings), &mut host.settings)
        };
        let mut servers = checked(&ctx.session, Change::SaveHost(&host), ItemKind::Host, id, |s| Ok(s.upsert_host(&host)?))?;
        servers.extend(drop_orphans(&ctx.session, &orphans)?);
        ctx.session.announce(&servers);
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct IdParams {
    id: HostId,
}

fn run_delete(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let IdParams { id } = parse("host.delete", params)?;
        share_rights(&ctx.session.store(), Change::DeleteHost(id))?;
        let passwords = ctx.session.store().list_hosts()?.into_iter().find(|h| h.id == id).map(|h| h.settings.password_ids()).unwrap_or_default();
        let mut servers = changed(&ctx.session, ItemKind::Host, id.0.to_string(), |s| Ok(s.delete_host(id)?))?;
        // The host's commands go with it.
        let _ = sshcraft_store::history::clear(&ctx.session.store(), Some(&format!("host:{}", id.0)));
        servers.extend(drop_orphans(&ctx.session, &passwords)?);
        ctx.session.announce(&servers);
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct RecentParams {
    limit: u32,
}

fn run_recent(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let RecentParams { limit } = parse("host.recent", params)?;
        to_value(&ctx.session.store().recent_hosts(limit.min(100))?)
    })
}

#[derive(Deserialize)]
struct ForgetParams {
    host_id: HostId,
}

/// Removes the stored host key for the host's effective address and port.
fn run_forget_key(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let ForgetParams { host_id } = parse("host.forget_key", params)?;
        let store = ctx.session.store();
        let host = store.list_hosts()?.into_iter().find(|h| h.id == host_id).ok_or_else(|| CommandError::code(ErrorCode::NotFound))?;
        let chain = match host.group_id {
            Some(gid) => group_chain(&store.list_groups()?, gid),
            None => Vec::new(),
        };
        let chain: Vec<&Group> = chain.iter().collect();
        let (address, port) = effective_endpoint(&host, &chain)?;
        store.forget_known_host(&address, port)?;
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct InfoParams {
    host: String,
}

/// A host by id or by a name only one host has.
pub(crate) fn find_host(hosts: &[Host], wanted: &str, command: &str) -> CommandResult<Host> {
    if let Ok(id) = uuid::Uuid::parse_str(wanted)
        && let Some(host) = hosts.iter().find(|h| h.id.0 == id)
    {
        return Ok(host.clone());
    }
    let mut named = hosts.iter().filter(|h| h.name == wanted);
    match (named.next(), named.next()) {
        (Some(host), None) => Ok(host.clone()),
        (Some(_), Some(_)) => Err(CommandError::bad_params(command, format!("more than one host is called {wanted:?}; use its id"))),
        _ => Err(CommandError::NotFound(format!("host {wanted:?}"))),
    }
}

fn run_info(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let InfoParams { host } = parse("host.info", params)?;
        let hosts = ctx.session.store().list_hosts()?;
        to_value(&host_view(find_host(&hosts, &host, "host.info")?))
    })
}

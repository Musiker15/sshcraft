//! Hosts and stored host keys, and the bookkeeping that hosts and groups share: queueing a change
//! for the sync server the item lives at, checking received shares, dropping passwords nothing
//! refers to any more.

use serde::Deserialize;
use serde_json::Value;
use sshcraft_model::model::{ConnectionSettings, Group, Host, HostId, PasswordId, Proxy, effective_endpoint, group_chain};
use sshcraft_store::store::{ItemKind, Store};

use super::password::{HostView, drop_passwords, host_view, keep_passwords, queue};
use super::{CommandError, CommandResult, CommandSpec, command, parse, query, sync_run, to_value};
use std::time::Duration;

use sshcraft_store::agent::MAX_DETAIL;

use crate::agent_sessions::AgentSessions;
use crate::error::ErrorCode;
use crate::gate::wire::{DEFAULT_EXEC_SECS, ExecResult, MAX_EXEC_SECS, MAX_OUTPUT};
use crate::sync::{Change, note_change, now_ms, share_rights};
use crate::{Ctx, Session};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query("host.list", "Hosts", "none", run_list),
        command(
            "host.save",
            "Save a host",
            "{ host: the host as host.list returns it, without password flags, approve_command?: the user entered its proxy command }",
            run_save,
        ),
        command("host.delete", "Delete a host", "{ id }", run_delete),
        query("host.recent", "Recent hosts", "{ limit: at most 100 }", run_recent),
        command("host.forget_key", "Forget the host key", "{ host_id }", run_forget_key),
        query("host.info", "Host details", "{ host: id or unique name }", run_info),
        command("host.exec", "Run a command on a host", "{ host: id or unique name, command, timeout_secs?: 1 to 600, 60 if missing }", run_exec),
    ]
}

#[derive(Deserialize)]
struct ExecParams {
    host: String,
    command: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// Runs one command on a connection of its own (every check of a tab applies): output and
/// errors as they came, at most [`MAX_OUTPUT`] bytes, and the exit status.
fn run_exec(ctx: Ctx, params: Value) -> Out {
    use futures::FutureExt;
    async move {
        let ExecParams { host, command, timeout_secs } = parse("host.exec", params)?;
        let length = command.chars().count();
        if command.trim().is_empty() || length > MAX_DETAIL {
            return Err(CommandError::bad_params("host.exec", format!("the command must have 1 to {MAX_DETAIL} characters")));
        }
        let secs = timeout_secs.unwrap_or(DEFAULT_EXEC_SECS);
        if !(1..=MAX_EXEC_SECS).contains(&secs) {
            return Err(CommandError::bad_params("host.exec", format!("timeout_secs must be 1 to {MAX_EXEC_SECS}")));
        }
        let session = ctx.session;
        let host = {
            let hosts = session.store().list_hosts()?;
            find_host(&hosts, &host, "host.exec")?
        };
        let run = AgentSessions::exec(session.sessions(), host.id, &command, MAX_OUTPUT, Duration::from_secs(secs))
            .await
            .map_err(|(code, detail)| CommandError::Failed { code, detail })?;
        to_value(&ExecResult { output: String::from_utf8_lossy(&run.output).into_owned(), truncated: run.truncated, exit_status: run.exit_status })
    }
    .boxed()
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
    /// The user entered the proxy command in this save (the host form sets it only when the
    /// command changed there); anything else never approves one.
    #[serde(default)]
    approve_command: bool,
}

fn run_save(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let SaveParams { mut host, approve_command } = parse("host.save", params)?;
        let id = host.id.0.to_string();
        let orphans = {
            let store = ctx.session.store();
            let stored = store.list_hosts()?.into_iter().find(|h| h.id == host.id);
            keep_passwords(stored.as_ref().map(|h| &h.settings), &mut host.settings)
        };
        let mut servers = checked(&ctx.session, Change::SaveHost(&host), ItemKind::Host, id, |s| Ok(s.upsert_host(&host)?))?;
        if approve_command {
            approve_commands(&ctx, [&host.settings])?;
        }
        servers.extend(drop_orphans(&ctx.session, &orphans)?);
        // Rules removed from the host stop forwarding.
        let rules: Vec<_> = host.forwards.iter().map(|r| r.id).collect();
        ctx.session.forwards().keep_only(host.id, &rules);
        ctx.session.announce(&servers);
        Ok(Value::Null)
    })
}

/// A proxy command the user entered in the window runs on this device without asking again.
/// One that an agent, sync, a share or an import brings is asked about when it is first used;
/// saving such a host for another reason (a rename, a rule, a move) does not approve it.
pub(crate) fn approve_commands<'a>(ctx: &Ctx, settings: impl IntoIterator<Item = &'a ConnectionSettings>) -> Result<(), CommandError> {
    if !ctx.caller.is_user() {
        return Ok(());
    }
    let store = ctx.session.store();
    for s in settings {
        if let Some(Proxy::Command { argv }) = &s.proxy {
            store.approve_command(&sshcraft_store::store::command_digest(argv))?;
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct IdParams {
    id: HostId,
}

fn run_delete(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let IdParams { id } = parse("host.delete", params)?;
        share_rights(&ctx.session.store(), Change::DeleteHost(id))?;
        // Into the trash first: a delete can be taken back.
        ctx.session.store().trash_host(id)?;
        let passwords = ctx.session.store().list_hosts()?.into_iter().find(|h| h.id == id).map(|h| h.settings.password_ids()).unwrap_or_default();
        let mut servers = changed(&ctx.session, ItemKind::Host, id.0.to_string(), |s| Ok(s.delete_host(id)?))?;
        // The host's commands go with it.
        let _ = sshcraft_store::history::clear(&ctx.session.store(), Some(&format!("host:{}", id.0)));
        servers.extend(drop_orphans(&ctx.session, &passwords)?);
        ctx.session.forwards().stop_host(id);
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

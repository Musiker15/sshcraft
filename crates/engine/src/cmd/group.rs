//! Groups: list, save, delete. A deleted group takes its subgroups and hosts along.

use serde::Deserialize;
use serde_json::Value;
use sshcraft_model::model::{Group, GroupId, PasswordId};
use sshcraft_store::store::{ItemKind, Store};
use sshcraft_sync::engine::record_group_hosts;

use super::host::{changed, checked, drop_orphans};
use super::password::{GroupView, group_view, keep_passwords};
use super::{CommandError, CommandResult, CommandSpec, command, parse, query, sync_run, to_value};
use crate::Ctx;
use crate::sync::{Change, now_ms, share_rights};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query("group.list", "Groups", "none", run_list),
        command("group.save", "Save a group", "{ group: the group as group.list returns it, without password flags }", run_save),
        command("group.delete", "Delete a group", "{ id }", run_delete),
    ]
}

/// Every password the settings of a group, its subgroups and their hosts refer to.
fn tree_passwords(store: &Store, id: GroupId) -> CommandResult<Vec<PasswordId>> {
    let mut tree = store.subgroups(id)?;
    tree.push(id);
    let mut out: Vec<PasswordId> = Vec::new();
    for group in store.list_groups()? {
        if tree.contains(&group.id) {
            out.extend(group.defaults.password_ids());
        }
    }
    for host in store.list_hosts()? {
        if host.group_id.is_some_and(|g| tree.contains(&g)) {
            out.extend(host.settings.password_ids());
        }
    }
    Ok(out)
}

fn run_list(ctx: Ctx, _: Value) -> Out {
    sync_run(move || {
        let list: Vec<GroupView> = ctx.session.store().list_groups()?.into_iter().map(group_view).collect();
        to_value(&list)
    })
}

#[derive(Deserialize)]
struct SaveParams {
    group: Group,
    /// As for `host.save`: the user entered the proxy command in this save.
    #[serde(default)]
    approve_command: bool,
}

fn run_save(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let SaveParams { mut group, approve_command } = parse("group.save", params)?;
        let id = group.id.0.to_string();
        let orphans = {
            let store = ctx.session.store();
            let stored = store.list_groups()?.into_iter().find(|g| g.id == group.id);
            keep_passwords(stored.as_ref().map(|g| &g.defaults), &mut group.defaults)
        };
        let mut servers = checked(&ctx.session, Change::SaveGroup(&group), ItemKind::Group, id, |s| Ok(s.upsert_group(&group)?))?;
        if approve_command {
            super::host::approve_commands(&ctx, [&group.defaults])?;
        }
        servers.extend(drop_orphans(&ctx.session, &orphans)?);
        ctx.session.announce(&servers);
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct IdParams {
    id: GroupId,
}

fn run_delete(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let IdParams { id } = parse("group.delete", params)?;
        share_rights(&ctx.session.store(), Change::DeleteGroup(id))?;
        ctx.session.store().trash_group(id)?;
        let passwords = tree_passwords(&ctx.session.store(), id)?;
        let mut servers = changed(&ctx.session, ItemKind::Group, id.0.to_string(), |s| {
            // Its hosts go with it; queue them while they still exist.
            record_group_hosts(s, &id.0.to_string(), now_ms()).map_err(crate::sync::sync_error)?;
            Ok(s.delete_group(id)?)
        })?;
        servers.extend(drop_orphans(&ctx.session, &passwords)?);
        // The group's hosts are gone: their rules stop forwarding.
        ctx.session.forwards().reconcile();
        ctx.session.announce(&servers);
        Ok(Value::Null)
    })
}

//! The trash and earlier versions (see `sshcraft_store::trash`). Restoring puts an item back and
//! sends it to the sync server it lives at again. None of these is open to agents.

use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_store::store::ItemKind;
use sshcraft_store::trash::{Restored, TrashKind};

use super::{CommandError, CommandSpec, command, parse, query, sync_run, to_value};
use crate::Ctx;
use crate::sync::{note_change, now_ms};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query("trash.list", "Trash", "none", run_list),
        command("trash.restore", "Restore from the trash", "{ kind: host|group|key|snippet, id }", run_restore),
        command("trash.purge", "Empty the trash", "{ kind?, id? }: one entry, or all without them", run_purge),
        query("trash.versions", "Earlier versions", "{ kind: host|group|snippet, id }", run_versions),
        command("trash.restore_version", "Restore an earlier version", "{ kind, id, version }", run_restore_version),
    ]
}

fn run_list(ctx: Ctx, _params: Value) -> Out {
    sync_run(move || {
        let days = ctx.session.settings().get().general.trash_days;
        let store = ctx.session.store();
        store.purge_old_trash(days)?;
        to_value(&store.list_trash()?)
    })
}

#[derive(Deserialize)]
struct EntryParams {
    kind: TrashKind,
    id: String,
}

/// Queues what came back for the servers it lives at; returns them.
pub(crate) fn send_again(ctx: &Ctx, restored: &Restored) -> Result<Vec<sshcraft_model::model::ServerId>, CommandError> {
    let store = ctx.session.store();
    let now = now_ms();
    let mut servers = Vec::new();
    let items = restored
        .groups
        .iter()
        .map(|g| (ItemKind::Group, g.0.to_string()))
        .chain(restored.hosts.iter().map(|h| (ItemKind::Host, h.0.to_string())))
        .chain(restored.keys.iter().map(|k| (ItemKind::Key, k.0.to_string())))
        .chain(restored.passwords.iter().map(|p| (ItemKind::Password, p.0.to_string())));
    for (kind, id) in items {
        for server in note_change(&store, kind, &id, None, now)? {
            if !servers.contains(&server) {
                servers.push(server);
            }
        }
    }
    Ok(servers)
}

fn run_restore(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let EntryParams { kind, id } = parse("trash.restore", params)?;
        let payload =
            ctx.session.store().trash_payload(kind, &id)?.ok_or_else(|| CommandError::NotFound(format!("{} {id} in the trash", kind.as_str())))?;
        let restored = ctx.session.store().restore_payload(&payload)?;
        ctx.session.store().remove_trash(kind, &id)?;
        let servers = send_again(&ctx, &restored)?;
        ctx.session.sync().nudge(&servers);
        ctx.session.announce(&servers);
        Ok(Value::Null)
    })
}

fn run_versions(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let EntryParams { kind, id } = parse("trash.versions", params)?;
        let list: Vec<Value> = ctx
            .session
            .store()
            .list_versions(kind, &id)?
            .into_iter()
            .map(|(version, saved_at, name)| json!({ "version": version, "saved_at": saved_at, "name": name }))
            .collect();
        Ok(Value::Array(list))
    })
}

#[derive(Deserialize)]
struct VersionParams {
    kind: TrashKind,
    id: String,
    version: i64,
}

/// Saves an earlier stand like an edit: it syncs and becomes a version itself.
fn run_restore_version(ctx: Ctx, params: Value) -> Out {
    use futures::FutureExt;
    async move {
        let VersionParams { kind, id, version } = parse("trash.restore_version", params)?;
        let payload = ctx.session.store().version(kind, &id, version)?.ok_or_else(|| CommandError::NotFound(format!("version {version} of {id}")))?;
        let engine = |e: crate::error::EngineError| CommandError::failed(crate::error::ErrorCode::Internal, e);
        match payload {
            sshcraft_store::trash::TrashPayload::Host { host, .. } => {
                ctx.session.execute(crate::caller::Caller::User, "host.save", json!({ "host": host })).await.map_err(engine)?;
            }
            sshcraft_store::trash::TrashPayload::Group { groups, .. } => {
                let group = groups.into_iter().next().ok_or_else(|| CommandError::NotFound(format!("version {version} of {id}")))?;
                ctx.session.execute(crate::caller::Caller::User, "group.save", json!({ "group": group })).await.map_err(engine)?;
            }
            sshcraft_store::trash::TrashPayload::Snippet { id: snippet, sealed, .. } => {
                ctx.session.store().put_snippet_sealed(snippet, &sealed)?;
                ctx.session.announce(&[]);
            }
            sshcraft_store::trash::TrashPayload::Key { .. } => {
                return Err(CommandError::bad_params("trash.restore_version", "keys have no versions"));
            }
        }
        Ok(Value::Null)
    }
    .boxed()
}

#[derive(Deserialize, Default)]
struct PurgeParams {
    #[serde(default)]
    kind: Option<TrashKind>,
    #[serde(default)]
    id: Option<String>,
}

fn run_purge(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let PurgeParams { kind, id } = parse("trash.purge", params)?;
        let removed = match (kind, id) {
            (Some(kind), Some(id)) => usize::from(ctx.session.store().remove_trash(kind, &id)?),
            (None, None) => ctx.session.store().purge_trash(None)?,
            _ => return Err(CommandError::bad_params("trash.purge", "both kind and id, or neither")),
        };
        Ok(json!({ "removed": removed }))
    })
}

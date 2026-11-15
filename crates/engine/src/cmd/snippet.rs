//! Snippets, sealed with the vault, and the command history of terminals.

use serde::Deserialize;
use serde_json::Value;
use sshcraft_model::model::SnippetId;
use sshcraft_store::history::{self, HistoryError};
use sshcraft_store::snippets::{self, Snippet};
use sshcraft_store::store::Store;
use sshcraft_store::vault::Vault;

use super::{CommandError, CommandResult, CommandSpec, blocking_run, command, needs_vault, parse, query, sync_run, to_value};
use crate::error::ErrorCode;
use crate::{Ctx, now_ms};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec { enabled: needs_vault, ..query("snippet.list", "Snippets", "none", run_list) },
        CommandSpec {
            enabled: needs_vault,
            ..command("snippet.save", "Save a snippet", "{ snippet: { id, name, package?, content, default_targets? } }", run_save)
        },
        command("snippet.delete", "Delete a snippet", "{ id }", run_delete),
        // The command can be a password typed by mistake: never in the journal.
        CommandSpec { journal: false, ..command("history.record", "Remember a command", "{ scope: host:<id>|local|serial, command }", run_record) },
        query("history.list", "Command history", "{ scope }", run_history),
        command("history.clear", "Clear the command history", "{ scope?: none clears all }", run_clear),
    ]
}

fn history_error(e: HistoryError) -> CommandError {
    match e {
        HistoryError::Scope | HistoryError::Command => CommandError::failed(ErrorCode::Internal, e),
        HistoryError::Vault(e) => CommandError::failed(ErrorCode::HistoryUnavailable, e),
        HistoryError::Store(e) => e.into(),
    }
}

fn run_list(ctx: Ctx, _: Value) -> Out {
    blocking_run("snippet.list", move || {
        let vault = ctx.session.vault()?;
        to_value(&snippets::list(&vault, &ctx.session.store())?)
    })
}

#[derive(Deserialize)]
struct SaveParams {
    snippet: Snippet,
}

fn run_save(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let SaveParams { snippet } = parse("snippet.save", params)?;
        let vault = ctx.session.vault()?;
        snippets::save(&vault, &ctx.session.store(), &snippet)?;
        to_value(&snippet)
    })
}

#[derive(Deserialize)]
struct IdParams {
    id: SnippetId,
}

fn run_delete(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let IdParams { id } = parse("snippet.delete", params)?;
        let name = ctx
            .session
            .vault()
            .ok()
            .and_then(|vault| snippets::list(&vault, &ctx.session.store()).ok())
            .and_then(|all| all.into_iter().find(|s| s.id == id).map(|s| s.name))
            .unwrap_or_default();
        ctx.session.store().trash_snippet(id, &name)?;
        snippets::delete(&ctx.session.store(), id)?;
        Ok(Value::Null)
    })
}

/// Keeps `command` in the history of `scope`, unless the history is switched off.
pub(crate) fn record_history(store: &Store, vault: Option<&Vault>, history_on: bool, scope: &str, command: &str, now: i64) -> CommandResult<()> {
    if !history_on {
        return Ok(());
    }
    let vault = vault.ok_or_else(|| CommandError::code(ErrorCode::HistoryUnavailable))?;
    history::record(vault, store, scope, command, now).map_err(history_error)
}

#[derive(Deserialize)]
struct RecordParams {
    scope: String,
    command: String,
}

/// Off the async threads: it opens and seals up to 1000 entries.
fn run_record(ctx: Ctx, params: Value) -> Out {
    blocking_run("history.record", move || {
        let RecordParams { scope, command } = parse("history.record", params)?;
        let history_on = ctx.session.settings().get().terminal.history;
        let vault = ctx.session.vault().ok();
        record_history(&ctx.session.store(), vault.as_deref(), history_on, &scope, &command, now_ms())?;
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct ScopeParams {
    scope: String,
}

fn run_history(ctx: Ctx, params: Value) -> Out {
    blocking_run("history.list", move || {
        let ScopeParams { scope } = parse("history.list", params)?;
        let vault = ctx.session.vault().map_err(|_| CommandError::code(ErrorCode::HistoryUnavailable))?;
        to_value(&history::list(&vault, &ctx.session.store(), &scope).map_err(history_error)?)
    })
}

#[derive(Deserialize)]
struct ClearParams {
    #[serde(default)]
    scope: Option<String>,
}

fn run_clear(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let ClearParams { scope } = parse("history.clear", params)?;
        history::clear(&ctx.session.store(), scope.as_deref()).map_err(history_error)?;
        Ok(Value::Null)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshcraft_model::model::HostId;
    use sshcraft_store::vault::MemoryMasterKey;

    #[test]
    fn history_off_records_nothing() {
        let store = Store::open_in_memory().unwrap();
        let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
        let scope = format!("host:{}", HostId::new().0);
        record_history(&store, Some(&vault), false, &scope, "ls", 1).unwrap();
        assert!(history::list(&vault, &store, &scope).unwrap().is_empty());
        record_history(&store, Some(&vault), true, &scope, "ls", 1).unwrap();
        assert_eq!(history::list(&vault, &store, &scope).unwrap().len(), 1);
        // Without an unlocked vault nothing is stored and that is said.
        assert!(record_history(&store, None, true, &scope, "pwd", 2).is_err());
    }
}

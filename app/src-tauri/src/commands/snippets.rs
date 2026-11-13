//! Snippets, the command history and directory listings for completion. All of it stays on
//! this device.

use sshcraft_core::history::{self, HistoryEntry, HistoryError};
use sshcraft_core::model::SnippetId;
use sshcraft_core::snippets::Snippet;
use sshcraft_core::store::Store;
use sshcraft_core::vault::Vault;
use tauri::State;

use super::{CommandError, CommandResult, ErrorCode};
use crate::sessions::SessionId;
use crate::state::AppState;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn history_error(e: HistoryError) -> CommandError {
    match e {
        HistoryError::Scope | HistoryError::Command => CommandError::new(ErrorCode::Internal, e),
        HistoryError::Vault(e) => CommandError::new(ErrorCode::HistoryUnavailable, e),
        HistoryError::Store(e) => e.into(),
    }
}

#[tauri::command]
pub fn snippets_list(state: State<'_, AppState>) -> CommandResult<Vec<Snippet>> {
    Ok(state.store.lock().expect("store lock").list_snippets()?)
}

#[tauri::command]
pub fn snippet_save(state: State<'_, AppState>, snippet: Snippet) -> CommandResult<Snippet> {
    state
        .store
        .lock()
        .expect("store lock")
        .upsert_snippet(&snippet)?;
    Ok(snippet)
}

#[tauri::command]
pub fn snippet_delete(state: State<'_, AppState>, id: SnippetId) -> CommandResult<()> {
    Ok(state.store.lock().expect("store lock").delete_snippet(id)?)
}

/// Keeps `command` in the history of `scope`, unless the history is switched off.
pub(crate) fn record_history(
    store: &Store,
    vault: Option<&Vault>,
    scope: &str,
    command: &str,
    now: i64,
) -> CommandResult<()> {
    if !crate::settings::load(store).history {
        return Ok(());
    }
    let vault = vault.ok_or_else(|| CommandError::code(ErrorCode::HistoryUnavailable))?;
    history::record(vault, store, scope, command, now).map_err(history_error)
}

#[tauri::command]
pub fn history_record(
    state: State<'_, AppState>,
    scope: String,
    command: String,
) -> CommandResult<()> {
    let vault = state.vault.as_ref().ok().cloned();
    let store = state.store.lock().expect("store lock");
    record_history(&store, vault.as_deref(), &scope, &command, now_ms())
}

#[tauri::command]
pub fn history_list(state: State<'_, AppState>, scope: String) -> CommandResult<Vec<HistoryEntry>> {
    let vault = state
        .vault
        .as_ref()
        .map_err(|_| CommandError::code(ErrorCode::HistoryUnavailable))?;
    let store = state.store.lock().expect("store lock");
    history::list(vault, &store, &scope).map_err(history_error)
}

#[tauri::command]
pub fn history_clear(state: State<'_, AppState>, scope: Option<String>) -> CommandResult<()> {
    let store = state.store.lock().expect("store lock");
    history::clear(&store, scope.as_deref()).map_err(history_error)
}

/// Names in `dir` of the session's machine, for path completion.
#[tauri::command]
pub async fn session_list_dir(
    state: State<'_, AppState>,
    id: SessionId,
    dir: String,
) -> CommandResult<Vec<String>> {
    state
        .sessions
        .list_dir(id, &dir)
        .await
        .map_err(|e| CommandError::new(ErrorCode::Session, e))
}

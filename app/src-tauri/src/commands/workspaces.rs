//! Workspace templates and opening links from terminal output.

use tauri::State;
#[cfg(desktop)]
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

use super::CommandResult;
#[cfg(desktop)]
use super::{CommandError, ErrorCode};
use crate::state::AppState;
use crate::workspaces::{self, Template};

#[tauri::command]
pub fn workspace_templates(state: State<'_, AppState>) -> Vec<Template> {
    workspaces::list(&state.store.lock().expect("store lock"))
}

#[tauri::command]
pub fn save_workspace_template(
    state: State<'_, AppState>,
    template: Template,
) -> CommandResult<Template> {
    Ok(workspaces::save(
        &state.store.lock().expect("store lock"),
        template,
    )?)
}

#[tauri::command]
pub fn delete_workspace_template(state: State<'_, AppState>, id: Uuid) -> CommandResult<()> {
    Ok(workspaces::delete(
        &state.store.lock().expect("store lock"),
        id,
    )?)
}

/// Opens a web link in the default browser; anything else is refused.
#[cfg(desktop)]
#[tauri::command]
pub fn open_link(app: tauri::AppHandle, url: String) -> CommandResult<()> {
    let url = crate::links::validate_link(&url)
        .ok_or_else(|| CommandError::code(ErrorCode::InvalidLink))?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| CommandError::new(ErrorCode::Io, e))
}

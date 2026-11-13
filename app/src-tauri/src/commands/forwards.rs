//! Starting and stopping port forwarding rules. Async, so the work runs on the runtime.

use sshcraft_core::forward::ForwardId;
use sshcraft_core::model::HostId;
use tauri::State;

use super::{CommandError, CommandResult};
use crate::forwards::ForwardStatus;
use crate::state::AppState;

#[tauri::command]
pub async fn forward_start(
    state: State<'_, AppState>,
    host_id: HostId,
    rule_id: ForwardId,
) -> CommandResult<()> {
    state
        .forwards
        .start(host_id, rule_id)
        .map_err(CommandError::code)
}

#[tauri::command]
pub async fn forward_stop(state: State<'_, AppState>, rule_id: ForwardId) -> CommandResult<()> {
    state.forwards.stop(rule_id);
    Ok(())
}

#[tauri::command]
pub fn forward_statuses(state: State<'_, AppState>) -> Vec<ForwardStatus> {
    state.forwards.statuses()
}

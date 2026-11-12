//! Groups, hosts and stored host keys.

use sshcraft_core::model::{Group, GroupId, Host, HostId, effective_endpoint};
use tauri::State;

use super::{CommandError, CommandResult, ErrorCode};
use crate::state::AppState;

fn store(state: &AppState) -> std::sync::MutexGuard<'_, sshcraft_core::store::Store> {
    state.store.lock().expect("store lock")
}

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> CommandResult<Vec<Group>> {
    Ok(store(&state).list_groups()?)
}

#[tauri::command]
pub fn save_group(state: State<'_, AppState>, group: Group) -> CommandResult<()> {
    Ok(store(&state).upsert_group(&group)?)
}

#[tauri::command]
pub fn delete_group(state: State<'_, AppState>, id: GroupId) -> CommandResult<()> {
    Ok(store(&state).delete_group(id)?)
}

#[tauri::command]
pub fn list_hosts(state: State<'_, AppState>) -> CommandResult<Vec<Host>> {
    Ok(store(&state).list_hosts()?)
}

#[tauri::command]
pub fn save_host(state: State<'_, AppState>, host: Host) -> CommandResult<()> {
    Ok(store(&state).upsert_host(&host)?)
}

#[tauri::command]
pub fn delete_host(state: State<'_, AppState>, id: HostId) -> CommandResult<()> {
    Ok(store(&state).delete_host(id)?)
}

/// Removes the stored host key for the host's effective address and port.
#[tauri::command]
pub fn forget_host_key(state: State<'_, AppState>, host_id: HostId) -> CommandResult<()> {
    let store = store(&state);
    let host = store
        .list_hosts()?
        .into_iter()
        .find(|h| h.id == host_id)
        .ok_or_else(|| CommandError::code(ErrorCode::NotFound))?;
    let group = match host.group_id {
        Some(gid) => store.list_groups()?.into_iter().find(|g| g.id == gid),
        None => None,
    };
    let (address, port) = effective_endpoint(&host, group.as_ref())?;
    Ok(store.forget_known_host(&address, port)?)
}

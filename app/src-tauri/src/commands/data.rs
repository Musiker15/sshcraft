//! Groups, hosts and stored host keys.

use sshcraft_core::model::{Group, GroupId, Host, HostId, effective_endpoint};
use tauri::State;

use super::{CommandError, CommandResult, ErrorCode};
use crate::settings::{self, AppSettings};
use crate::state::AppState;
use crate::sync::{Change, note_change, now_ms, share_rights};
use sshcraft_core::store::ItemKind;
use sshcraft_sync::engine::record_group_hosts;

fn store(state: &AppState) -> std::sync::MutexGuard<'_, sshcraft_core::store::Store> {
    state.store.lock().expect("store lock")
}

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> CommandResult<Vec<Group>> {
    Ok(store(&state).list_groups()?)
}

/// Writes to the store, then queues the item for the server it lives at (or left).
fn changed(
    state: &AppState,
    kind: ItemKind,
    id: String,
    write: impl FnOnce(&sshcraft_core::store::Store) -> Result<(), CommandError>,
) -> CommandResult<()> {
    let servers = {
        let store = store(state);
        let before = store.location_of(kind, &id)?;
        // A delete is queued before it happens: afterwards the item's location is gone.
        let early = note_change(&store, kind, &id, before, now_ms())?;
        write(&store)?;
        let mut servers = note_change(&store, kind, &id, before, now_ms())?;
        servers.extend(early);
        servers
    };
    state.sync.nudge(&servers);
    Ok(())
}

/// Checks the change against shares this account received; a change that stays on this device
/// (a key assigned to a read-only item) is written without queueing.
fn checked(
    state: &AppState,
    change: Change<'_>,
    kind: ItemKind,
    id: String,
    write: impl FnOnce(&sshcraft_core::store::Store) -> Result<(), CommandError>,
) -> CommandResult<()> {
    let send = share_rights(&store(state), change)?;
    if send {
        changed(state, kind, id, write)
    } else {
        write(&store(state))
    }
}

#[tauri::command]
pub fn save_group(state: State<'_, AppState>, group: Group) -> CommandResult<()> {
    let id = group.id.0.to_string();
    checked(
        &state,
        Change::SaveGroup(&group),
        ItemKind::Group,
        id,
        |s| Ok(s.upsert_group(&group)?),
    )
}

#[tauri::command]
pub fn delete_group(state: State<'_, AppState>, id: GroupId) -> CommandResult<()> {
    share_rights(&store(&state), Change::DeleteGroup(id))?;
    changed(&state, ItemKind::Group, id.0.to_string(), |s| {
        // Its hosts go with it; queue them while they still exist.
        record_group_hosts(s, &id.0.to_string(), now_ms()).map_err(crate::sync::sync_error)?;
        Ok(s.delete_group(id)?)
    })
}

#[tauri::command]
pub fn list_hosts(state: State<'_, AppState>) -> CommandResult<Vec<Host>> {
    Ok(store(&state).list_hosts()?)
}

#[tauri::command]
pub fn save_host(state: State<'_, AppState>, host: Host) -> CommandResult<()> {
    let id = host.id.0.to_string();
    checked(&state, Change::SaveHost(&host), ItemKind::Host, id, |s| {
        Ok(s.upsert_host(&host)?)
    })
}

#[tauri::command]
pub fn delete_host(state: State<'_, AppState>, id: HostId) -> CommandResult<()> {
    share_rights(&store(&state), Change::DeleteHost(id))?;
    changed(&state, ItemKind::Host, id.0.to_string(), |s| {
        Ok(s.delete_host(id)?)
    })
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppSettings {
    settings::load(&store(&state))
}

#[tauri::command]
pub fn save_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> CommandResult<AppSettings> {
    Ok(settings::save(&store(&state), settings)?)
}

/// The operating system's language, for the AUTO language setting.
#[tauri::command]
pub fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}

/// Ids of the hosts connected to most recently, newest first.
#[tauri::command]
pub fn list_recent(state: State<'_, AppState>, limit: u32) -> CommandResult<Vec<HostId>> {
    Ok(store(&state).recent_hosts(limit.min(100))?)
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

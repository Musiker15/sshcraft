//! Groups, hosts and stored host keys.

use sshcraft_core::model::{
    Group, GroupId, Host, HostId, PasswordId, effective_endpoint, group_chain,
};
use tauri::State;

use super::passwords::{
    GroupView, HostView, drop_passwords, group_view, host_view, keep_passwords, queue,
};
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
pub fn list_groups(state: State<'_, AppState>) -> CommandResult<Vec<GroupView>> {
    Ok(store(&state)
        .list_groups()?
        .into_iter()
        .map(group_view)
        .collect())
}

/// Removes passwords nothing refers to any more and queues their removal.
fn drop_orphans(state: &AppState, ids: &[PasswordId]) -> CommandResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let servers = {
        let store = store(state);
        let touched = drop_passwords(&store, ids)?;
        queue(&store, touched)?
    };
    state.sync.nudge(&servers);
    Ok(())
}

/// Every password the settings of a group, its subgroups and their hosts refer to.
fn tree_passwords(
    store: &sshcraft_core::store::Store,
    id: GroupId,
) -> CommandResult<Vec<PasswordId>> {
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
pub fn save_group(state: State<'_, AppState>, mut group: Group) -> CommandResult<()> {
    let id = group.id.0.to_string();
    let orphans = {
        let store = store(&state);
        let stored = store.list_groups()?.into_iter().find(|g| g.id == group.id);
        keep_passwords(stored.as_ref().map(|g| &g.defaults), &mut group.defaults)
    };
    checked(
        &state,
        Change::SaveGroup(&group),
        ItemKind::Group,
        id,
        |s| Ok(s.upsert_group(&group)?),
    )?;
    drop_orphans(&state, &orphans)
}

#[tauri::command]
pub fn delete_group(state: State<'_, AppState>, id: GroupId) -> CommandResult<()> {
    share_rights(&store(&state), Change::DeleteGroup(id))?;
    let passwords = tree_passwords(&store(&state), id)?;
    changed(&state, ItemKind::Group, id.0.to_string(), |s| {
        // Its hosts go with it; queue them while they still exist.
        record_group_hosts(s, &id.0.to_string(), now_ms()).map_err(crate::sync::sync_error)?;
        Ok(s.delete_group(id)?)
    })?;
    // The group's hosts are gone, and their rules stop.
    state.forwards.reconcile();
    drop_orphans(&state, &passwords)
}

#[tauri::command]
pub fn list_hosts(state: State<'_, AppState>) -> CommandResult<Vec<HostView>> {
    Ok(store(&state)
        .list_hosts()?
        .into_iter()
        .map(host_view)
        .collect())
}

#[tauri::command]
pub fn save_host(state: State<'_, AppState>, mut host: Host) -> CommandResult<()> {
    let id = host.id.0.to_string();
    let orphans = {
        let store = store(&state);
        let stored = store.list_hosts()?.into_iter().find(|h| h.id == host.id);
        keep_passwords(stored.as_ref().map(|h| &h.settings), &mut host.settings)
    };
    checked(&state, Change::SaveHost(&host), ItemKind::Host, id, |s| {
        Ok(s.upsert_host(&host)?)
    })?;
    // A rule taken off the host stops; the others keep running.
    let keep: Vec<_> = host.forwards.iter().map(|r| r.id).collect();
    state.forwards.keep_only(host.id, &keep);
    drop_orphans(&state, &orphans)
}

#[tauri::command]
pub fn delete_host(state: State<'_, AppState>, id: HostId) -> CommandResult<()> {
    share_rights(&store(&state), Change::DeleteHost(id))?;
    let passwords = store(&state)
        .list_hosts()?
        .into_iter()
        .find(|h| h.id == id)
        .map(|h| h.settings.password_ids())
        .unwrap_or_default();
    changed(&state, ItemKind::Host, id.0.to_string(), |s| {
        Ok(s.delete_host(id)?)
    })?;
    state.forwards.stop_host(id);
    drop_orphans(&state, &passwords)
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
    let chain = match host.group_id {
        Some(gid) => group_chain(&store.list_groups()?, gid),
        None => Vec::new(),
    };
    let chain: Vec<&Group> = chain.iter().collect();
    let (address, port) = effective_endpoint(&host, &chain)?;
    Ok(store.forget_known_host(&address, port)?)
}

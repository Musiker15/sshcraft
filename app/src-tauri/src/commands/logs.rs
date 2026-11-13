//! Session logs: list, read, search, bookmarks and comments, export and delete. Deleting and
//! changing a log that lives at a sync server queues the change for it.

use std::sync::Arc;

use serde::Deserialize;
use sshcraft_core::logtext::plain_text;
use sshcraft_core::model::{LogId, ServerId};
use sshcraft_core::session_log::{self, LogHit, LogMark, LogMeta, LogSummary};
use sshcraft_core::store::{ItemKind, Store};
use sshcraft_core::vault::Vault;
use tauri::State;

use super::{CommandResult, ErrorCode};
use crate::errors::CommandError;
use crate::recording::now_ms;
use crate::sessions::SessionId;
use crate::state::AppState;
use crate::sync::sync_error;

/// Session logs are sealed: they need the unlocked vault.
fn vault(state: &AppState) -> CommandResult<Arc<Vault>> {
    state.vault.clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// Without escape sequences, as a reader sees it.
    Text,
    /// The bytes as recorded; `cat` plays them back.
    Raw,
}

/// The file content of an export.
pub fn export_bytes(
    vault: &Vault,
    store: &Store,
    id: LogId,
    format: ExportFormat,
) -> CommandResult<Vec<u8>> {
    let bytes = session_log::read(vault, store, id)?;
    Ok(match format {
        ExportFormat::Raw => bytes,
        ExportFormat::Text => plain_text(&bytes).into_bytes(),
    })
}

/// Replaces the marks of a log and queues its header where it lives.
pub fn save_marks(
    vault: &Vault,
    store: &Store,
    id: LogId,
    marks: Vec<LogMark>,
) -> CommandResult<(LogMeta, Option<ServerId>)> {
    let meta = session_log::save_marks(vault, store, id, marks)?;
    let at = session_log::location(store, id)?.flatten();
    if let Some(server) = at {
        store.queue(server, ItemKind::Log, &id.0.to_string(), now_ms())?;
    }
    Ok((meta, at))
}

/// Deletes logs here and at their servers; returns the servers to nudge.
pub fn delete_logs(store: &Store, ids: &[LogId]) -> CommandResult<Vec<ServerId>> {
    let mut servers = Vec::new();
    for id in ids {
        if let Some(server) =
            sshcraft_sync::logs::delete_log(store, *id, now_ms()).map_err(sync_error)?
            && !servers.contains(&server)
        {
            servers.push(server);
        }
    }
    Ok(servers)
}

#[tauri::command]
pub fn logs_list(state: State<'_, AppState>) -> CommandResult<Vec<LogSummary>> {
    let vault = vault(&state)?;
    Ok(session_log::list(
        &vault,
        &state.store.lock().expect("store lock"),
    )?)
}

/// The recorded bytes, sent as they are (not as a JSON array).
#[tauri::command]
pub fn log_read(state: State<'_, AppState>, id: LogId) -> CommandResult<tauri::ipc::Response> {
    let vault = vault(&state)?;
    let bytes = session_log::read(&vault, &state.store.lock().expect("store lock"), id)?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub fn log_save_marks(
    state: State<'_, AppState>,
    id: LogId,
    marks: Vec<LogMark>,
) -> CommandResult<LogMeta> {
    let vault = vault(&state)?;
    let (meta, at) = save_marks(&vault, &state.store.lock().expect("store lock"), id, marks)?;
    state.sync.nudge(&at.into_iter().collect::<Vec<_>>());
    Ok(meta)
}

#[tauri::command]
pub fn logs_delete(state: State<'_, AppState>, ids: Vec<LogId>) -> CommandResult<()> {
    let servers = delete_logs(&state.store.lock().expect("store lock"), &ids)?;
    state.sync.nudge(&servers);
    Ok(())
}

#[tauri::command]
pub async fn logs_search(state: State<'_, AppState>, query: String) -> CommandResult<Vec<LogHit>> {
    let vault = vault(&state)?;
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        session_log::search(&vault, &store.lock().expect("store lock"), &query)
    })
    .await
    .map_err(|e| CommandError::new(ErrorCode::Internal, e))?
    .map_err(Into::into)
}

#[tauri::command]
pub fn log_export(
    state: State<'_, AppState>,
    id: LogId,
    path: String,
    format: ExportFormat,
) -> CommandResult<()> {
    let vault = vault(&state)?;
    let bytes = export_bytes(&vault, &state.store.lock().expect("store lock"), id, format)?;
    std::fs::write(&path, bytes).map_err(|e| CommandError::new(ErrorCode::Io, e))
}

/// A bookmark in the running recording of a session.
#[tauri::command]
pub fn session_bookmark(
    state: State<'_, AppState>,
    id: SessionId,
    text: String,
) -> CommandResult<LogMark> {
    state.sessions.bookmark(id, &text).map_err(|e| match e {
        session_log::LogError::NotFound => CommandError::code(ErrorCode::NotRecording),
        e => e.into(),
    })
}

#[tauri::command]
pub fn session_recording(state: State<'_, AppState>, id: SessionId) -> bool {
    state.sessions.is_recording(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshcraft_core::model::HostId;
    use sshcraft_core::session_log::{LogKind, MarkKind};
    use sshcraft_core::vault::MemoryMasterKey;
    use uuid::Uuid;

    fn setup() -> (Store, Vault) {
        let store = Store::open_in_memory().unwrap();
        let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
        (store, vault)
    }

    fn recorded(store: &Store, vault: &Vault, bytes: &[u8]) -> LogId {
        let meta = LogMeta {
            id: LogId(Uuid::new_v4()),
            title: "web".into(),
            kind: LogKind::Ssh,
            host_id: Some(HostId(Uuid::new_v4())),
            started_at: 1,
            ended_at: None,
            size: 0,
            truncated: false,
            marks: Vec::new(),
        };
        session_log::create(vault, store, &meta).unwrap();
        session_log::append_chunk(vault, store, meta.id, 0, bytes, 2).unwrap();
        session_log::finish(vault, store, meta.id, 3, bytes.len() as u64, false).unwrap();
        meta.id
    }

    /// A server row so that queueing at it has a target.
    fn at_server(store: &Store, id: LogId) -> ServerId {
        let server = ServerId(Uuid::new_v4());
        store
            .add_server(&sshcraft_core::store::ServerRow {
                id: server,
                name: "home".into(),
                origin: "https://sync.example.org".into(),
                account: [7; 32],
                device_id: Uuid::new_v4(),
                sealed_device: Vec::new(),
                sync_enabled: true,
                sync_keys: false,
                revoked: false,
                log_head: None,
                tracker: None,
                created_at: 1,
            })
            .unwrap();
        session_log::set_location(store, id, Some(server)).unwrap();
        server
    }

    #[test]
    fn export_text_drops_escape_sequences_raw_keeps_them() {
        let (store, vault) = setup();
        let id = recorded(&store, &vault, b"\x1b[32mok\x1b[0m\r\n");
        assert_eq!(
            export_bytes(&vault, &store, id, ExportFormat::Text).unwrap(),
            b"ok\n"
        );
        assert_eq!(
            export_bytes(&vault, &store, id, ExportFormat::Raw).unwrap(),
            b"\x1b[32mok\x1b[0m\r\n"
        );
    }

    #[test]
    fn deleting_a_synced_log_queues_header_and_parts() {
        let (store, vault) = setup();
        let id = recorded(&store, &vault, b"abc");
        let server = at_server(&store, id);
        let parts = session_log::chunk_ids(&store, id).unwrap();
        assert_eq!(delete_logs(&store, &[id]).unwrap(), vec![server]);
        let outbox = store.outbox(server).unwrap();
        assert!(outbox.contains(&(ItemKind::Log, id.0.to_string())));
        assert!(outbox.contains(&(ItemKind::LogChunk, parts[0].clone())));
        assert!(session_log::list(&vault, &store).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_local_log_queues_nothing() {
        let (store, vault) = setup();
        let id = recorded(&store, &vault, b"abc");
        assert!(delete_logs(&store, &[id]).unwrap().is_empty());
    }

    #[test]
    fn saving_marks_queues_the_header_where_the_log_lives() {
        let (store, vault) = setup();
        let id = recorded(&store, &vault, b"abcdef");
        let server = at_server(&store, id);
        let mark = LogMark {
            id: Uuid::new_v4(),
            offset: 3,
            kind: MarkKind::Comment,
            text: "here".into(),
            created_at: 4,
        };
        let (meta, at) = save_marks(&vault, &store, id, vec![mark.clone()]).unwrap();
        assert_eq!(at, Some(server));
        assert_eq!(meta.marks, vec![mark]);
        assert_eq!(
            store.outbox(server).unwrap(),
            vec![(ItemKind::Log, id.0.to_string())]
        );
    }
}

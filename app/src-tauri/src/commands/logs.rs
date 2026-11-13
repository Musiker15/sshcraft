//! Session logs: list, read, search, bookmarks and comments, export and delete. Deleting and
//! changing a log that lives at a sync server queues the change for it.
//!
//! Every command runs off the main thread, and the store is held only for short reads and
//! writes: opening a log (up to 64 MiB) happens without it, so recording and sync go on.

use std::sync::{Arc, Mutex};

use serde::Deserialize;
use sshcraft_core::logtext::plain_text;
use sshcraft_core::model::{LogId, ServerId};
use sshcraft_core::session_log::{self, LogHit, LogMark, LogMeta, LogSummary, MAX_HITS};
use sshcraft_core::store::{ItemKind, Store};
use sshcraft_core::vault::Vault;
use tauri::State;
use uuid::Uuid;

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

/// Runs `work` on a blocking thread.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| CommandError::new(ErrorCode::Internal, e))?
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// Without escape sequences, as a reader sees it.
    Text,
    /// The bytes as recorded; `cat` plays them back.
    Raw,
}

/// A log's output, the store held only while the sealed parts are read.
pub fn read_unlocked(store: &Mutex<Store>, vault: &Vault, id: LogId) -> CommandResult<Vec<u8>> {
    let sealed = session_log::sealed_log(vault, &store.lock().expect("store lock"), id)?;
    Ok(session_log::open_parts(vault, sealed)?)
}

/// The file content of an export.
pub fn export_bytes(
    store: &Mutex<Store>,
    vault: &Vault,
    id: LogId,
    format: ExportFormat,
) -> CommandResult<Vec<u8>> {
    let bytes = read_unlocked(store, vault, id)?;
    Ok(match format {
        ExportFormat::Raw => bytes,
        ExportFormat::Text => plain_text(&bytes).into_bytes(),
    })
}

/// Searches all logs, one at a time, holding the store only to read each one's sealed parts.
pub fn search_unlocked(
    store: &Mutex<Store>,
    vault: &Vault,
    query: &str,
) -> CommandResult<Vec<LogHit>> {
    let logs = session_log::list(vault, &store.lock().expect("store lock"))?;
    let mut hits = Vec::new();
    for summary in logs {
        let Ok(bytes) = read_unlocked(store, vault, summary.meta.id) else {
            continue;
        };
        session_log::search_bytes(summary.meta.id, &bytes, query, &mut hits);
        if hits.len() >= MAX_HITS {
            break;
        }
    }
    Ok(hits)
}

/// Queues a changed header where the log lives; returns that server.
fn queue_header(store: &Store, id: LogId) -> CommandResult<Option<ServerId>> {
    let at = session_log::location(store, id)?.flatten();
    if let Some(server) = at
        && !session_log::running(store, id)?
    {
        store.queue(server, ItemKind::Log, &id.0.to_string(), now_ms())?;
    }
    Ok(at)
}

/// Adds a bookmark or comment to what the log has now and queues its header.
pub fn add_mark(
    vault: &Vault,
    store: &Store,
    id: LogId,
    mark: LogMark,
) -> CommandResult<(LogMeta, Option<ServerId>)> {
    let meta = session_log::add_mark(vault, store, id, mark)?;
    Ok((meta, queue_header(store, id)?))
}

/// Removes one bookmark or comment and queues the header.
pub fn remove_mark(
    vault: &Vault,
    store: &Store,
    id: LogId,
    mark: Uuid,
) -> CommandResult<(LogMeta, Option<ServerId>)> {
    let meta = session_log::remove_mark(vault, store, id, mark)?;
    Ok((meta, queue_header(store, id)?))
}

/// Deletes ended logs here and at their servers; a log still being recorded stays. Returns the
/// servers to nudge.
pub fn delete_logs(store: &Store, ids: &[LogId]) -> CommandResult<Vec<ServerId>> {
    let mut servers = Vec::new();
    for id in ids {
        if session_log::running(store, *id)? {
            continue;
        }
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
pub async fn logs_list(state: State<'_, AppState>) -> CommandResult<Vec<LogSummary>> {
    let vault = vault(&state)?;
    let store = state.store.clone();
    blocking(move || {
        Ok(session_log::list(
            &vault,
            &store.lock().expect("store lock"),
        )?)
    })
    .await
}

/// The recorded bytes, sent as they are (not as a JSON array).
#[tauri::command]
pub async fn log_read(
    state: State<'_, AppState>,
    id: LogId,
) -> CommandResult<tauri::ipc::Response> {
    let vault = vault(&state)?;
    let store = state.store.clone();
    let bytes = blocking(move || read_unlocked(&store, &vault, id)).await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn log_add_mark(
    state: State<'_, AppState>,
    id: LogId,
    mark: LogMark,
) -> CommandResult<LogMeta> {
    let vault = vault(&state)?;
    let store = state.store.clone();
    let (meta, at) =
        blocking(move || add_mark(&vault, &store.lock().expect("store lock"), id, mark)).await?;
    state.sync.nudge(&at.into_iter().collect::<Vec<_>>());
    Ok(meta)
}

#[tauri::command]
pub async fn log_remove_mark(
    state: State<'_, AppState>,
    id: LogId,
    mark: Uuid,
) -> CommandResult<LogMeta> {
    let vault = vault(&state)?;
    let store = state.store.clone();
    let (meta, at) =
        blocking(move || remove_mark(&vault, &store.lock().expect("store lock"), id, mark)).await?;
    state.sync.nudge(&at.into_iter().collect::<Vec<_>>());
    Ok(meta)
}

#[tauri::command]
pub async fn logs_delete(state: State<'_, AppState>, ids: Vec<LogId>) -> CommandResult<()> {
    let store = state.store.clone();
    let servers = blocking(move || delete_logs(&store.lock().expect("store lock"), &ids)).await?;
    state.sync.nudge(&servers);
    Ok(())
}

#[tauri::command]
pub async fn logs_search(state: State<'_, AppState>, query: String) -> CommandResult<Vec<LogHit>> {
    let vault = vault(&state)?;
    let store = state.store.clone();
    blocking(move || search_unlocked(&store, &vault, &query)).await
}

/// Asks where to save (the dialog belongs to the app, so the window cannot pick a path itself)
/// and writes the log there. `false` when the dialog was cancelled.
#[cfg(desktop)]
#[tauri::command]
pub async fn log_export(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: LogId,
    format: ExportFormat,
) -> CommandResult<bool> {
    use tauri_plugin_dialog::DialogExt;
    let vault = vault(&state)?;
    let store = state.store.clone();
    let name = {
        let meta = session_log::meta(&vault, &store.lock().expect("store lock"), id)?
            .ok_or(CommandError::code(ErrorCode::NotFound))?;
        export_name(&meta, format)
    };
    blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name(&name)
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = path
            .into_path()
            .map_err(|e| CommandError::new(ErrorCode::Io, e))?;
        let bytes = export_bytes(&store, &vault, id, format)?;
        std::fs::write(path, bytes).map_err(|e| CommandError::new(ErrorCode::Io, e))?;
        Ok(true)
    })
    .await
}

#[cfg(not(desktop))]
#[tauri::command]
pub async fn log_export(_id: LogId, _format: ExportFormat) -> CommandResult<bool> {
    Err(CommandError::code(ErrorCode::Internal))
}

/// A file name for an export: title and start day, without characters Windows refuses.
pub fn export_name(meta: &LogMeta, format: ExportFormat) -> String {
    let day = chrono_day(meta.started_at);
    let title: String = meta
        .title
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let ext = match format {
        ExportFormat::Text => "txt",
        ExportFormat::Raw => "log",
    };
    format!("{}-{day}.{ext}", title.trim())
}

/// `YYYY-MM-DD` (UTC) of Unix milliseconds.
fn chrono_day(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
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

    fn setup() -> (Store, Vault) {
        let store = Store::open_in_memory().unwrap();
        let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
        (store, vault)
    }

    fn started(store: &Store, vault: &Vault, bytes: &[u8]) -> LogId {
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
        meta.id
    }

    fn recorded(store: &Store, vault: &Vault, bytes: &[u8]) -> LogId {
        let id = started(store, vault, bytes);
        session_log::finish(vault, store, id, 3, bytes.len() as u64, false).unwrap();
        id
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

    fn comment(offset: u64, text: &str) -> LogMark {
        LogMark {
            id: Uuid::new_v4(),
            offset,
            kind: MarkKind::Comment,
            text: text.into(),
            created_at: 4,
        }
    }

    #[test]
    fn export_text_drops_escape_sequences_raw_keeps_them() {
        let (store, vault) = setup();
        let id = recorded(&store, &vault, b"\x1b[32mok\x1b[0m\r\n");
        let store = Mutex::new(store);
        assert_eq!(
            export_bytes(&store, &vault, id, ExportFormat::Text).unwrap(),
            b"ok\n"
        );
        assert_eq!(
            export_bytes(&store, &vault, id, ExportFormat::Raw).unwrap(),
            b"\x1b[32mok\x1b[0m\r\n"
        );
    }

    #[test]
    fn export_names_are_safe_file_names() {
        let meta = LogMeta {
            id: LogId(Uuid::new_v4()),
            title: "db: prod/eu?".into(),
            kind: LogKind::Ssh,
            host_id: None,
            started_at: 1_794_528_000_000, // 2026-11-13
            ended_at: None,
            size: 0,
            truncated: false,
            marks: Vec::new(),
        };
        assert_eq!(
            export_name(&meta, ExportFormat::Text),
            "db_ prod_eu_-2026-11-13.txt"
        );
        assert_eq!(chrono_day(0), "1970-01-01");
        assert_eq!(chrono_day(951_782_400_000), "2000-02-29");
    }

    #[test]
    fn search_finds_hits_in_several_logs() {
        let (store, vault) = setup();
        recorded(&store, &vault, b"error one\r\n");
        recorded(&store, &vault, b"fine\r\nERROR two\r\n");
        let store = Mutex::new(store);
        let hits = search_unlocked(&store, &vault, "error").unwrap();
        assert_eq!(hits.len(), 2);
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
    fn a_log_being_recorded_is_not_deleted() {
        let (store, vault) = setup();
        let id = started(&store, &vault, b"abc");
        assert!(delete_logs(&store, &[id]).unwrap().is_empty());
        assert_eq!(session_log::list(&vault, &store).unwrap().len(), 1);
    }

    #[test]
    fn a_new_mark_keeps_marks_set_meanwhile_and_queues_the_header() {
        let (store, vault) = setup();
        let id = recorded(&store, &vault, b"abcdef");
        let server = at_server(&store, id);
        // A bookmark from elsewhere (terminal, other device) lands first.
        session_log::add_mark(&vault, &store, id, comment(1, "first")).unwrap();
        let (meta, at) = add_mark(&vault, &store, id, comment(3, "here")).unwrap();
        assert_eq!(at, Some(server));
        let texts: Vec<&str> = meta.marks.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "here"]);
        assert_eq!(
            store.outbox(server).unwrap(),
            vec![(ItemKind::Log, id.0.to_string())]
        );
        let (meta, _) = remove_mark(&vault, &store, id, meta.marks[0].id).unwrap();
        assert_eq!(meta.marks.len(), 1);
    }
}

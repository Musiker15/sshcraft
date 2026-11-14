//! Session logs: list, read, search, bookmarks and comments, export and delete. Deleting and
//! changing a log that lives at a sync server queues the change for it.
//!
//! Every command runs off the async threads, and the store is held only for short reads and
//! writes: opening a log (up to 64 MiB) happens without it, so recording and sync go on.

use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_model::logtext::plain_text;
use sshcraft_model::model::{LogId, ServerId};
use sshcraft_store::session_log::{self, LogHit, LogMark, LogMeta, MAX_HITS};
use sshcraft_store::store::{ItemKind, Store};
use sshcraft_store::vault::Vault;
use uuid::Uuid;

use super::{CommandError, CommandResult, CommandSpec, blocking_run, command, dialog_or_error, needs_vault, parse, query};
use crate::error::ErrorCode;
use crate::sync::sync_error;
use crate::{Ctx, now_ms};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    let sealed = |spec: CommandSpec| CommandSpec { enabled: needs_vault, ..spec };
    vec![
        sealed(query("log.list", "Session logs", "none", run_list)),
        sealed(query("log.read", "Read a session log", "{ id }; answers { data_base64 }", run_read)),
        sealed(command(
            "log.mark_add",
            "Add a bookmark or comment",
            "{ id, mark: { id, offset, kind: bookmark|comment, text, created_at } }",
            run_mark_add,
        )),
        sealed(command("log.mark_remove", "Remove a bookmark or comment", "{ id, mark: mark id }", run_mark_remove)),
        command("log.delete", "Delete session logs", "{ ids }", run_delete),
        sealed(query("log.search", "Search session logs", "{ query }", run_search)),
        sealed(command(
            "log.export",
            "Export a session log",
            "{ id, format: text|raw, path? }; without path the user is asked where (programmatic calls never open dialogs)",
            run_export,
        )),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// Without escape sequences, as a reader sees it.
    Text,
    /// The bytes as recorded; `cat` plays them back.
    Raw,
}

fn lock(store: &Mutex<Store>) -> std::sync::MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A log's output, the store held only while the sealed parts are read.
pub fn read_unlocked(store: &Mutex<Store>, vault: &Vault, id: LogId) -> CommandResult<Vec<u8>> {
    let sealed = session_log::sealed_log(vault, &lock(store), id)?;
    Ok(session_log::open_parts(vault, sealed)?)
}

/// The file content of an export.
pub fn export_bytes(store: &Mutex<Store>, vault: &Vault, id: LogId, format: ExportFormat) -> CommandResult<Vec<u8>> {
    let bytes = read_unlocked(store, vault, id)?;
    Ok(match format {
        ExportFormat::Raw => bytes,
        ExportFormat::Text => plain_text(&bytes).into_bytes(),
    })
}

/// Searches all logs, one at a time, holding the store only to read each one's sealed parts.
pub fn search_unlocked(store: &Mutex<Store>, vault: &Vault, query: &str) -> CommandResult<Vec<LogHit>> {
    search_with(store, vault, query, || {})
}

/// The search; `opening` runs while a log's parts are opened (a test looks at the lock there).
fn search_with(store: &Mutex<Store>, vault: &Vault, query: &str, opening: impl Fn()) -> CommandResult<Vec<LogHit>> {
    let logs = session_log::list(vault, &lock(store))?;
    let mut hits = Vec::new();
    for summary in logs {
        let Ok(sealed) = session_log::sealed_log(vault, &lock(store), summary.meta.id) else {
            continue;
        };
        opening();
        let Ok(bytes) = session_log::open_parts(vault, sealed) else {
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
pub fn add_mark(vault: &Vault, store: &Store, id: LogId, mark: LogMark) -> CommandResult<(LogMeta, Option<ServerId>)> {
    let meta = session_log::add_mark(vault, store, id, mark)?;
    Ok((meta, queue_header(store, id)?))
}

/// Removes one bookmark or comment and queues the header.
pub fn remove_mark(vault: &Vault, store: &Store, id: LogId, mark: Uuid) -> CommandResult<(LogMeta, Option<ServerId>)> {
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
        if let Some(server) = sshcraft_sync::logs::delete_log(store, *id, now_ms()).map_err(sync_error)?
            && !servers.contains(&server)
        {
            servers.push(server);
        }
    }
    Ok(servers)
}

/// A file name for an export: title and start day (UTC), without characters Windows refuses.
pub fn export_name(meta: &LogMeta, format: ExportFormat) -> String {
    let day = chrono::DateTime::from_timestamp_millis(meta.started_at).map_or_else(|| "unknown".to_owned(), |t| t.format("%Y-%m-%d").to_string());
    let title: String = meta.title.chars().map(|c| if c.is_control() || "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let ext = match format {
        ExportFormat::Text => "txt",
        ExportFormat::Raw => "log",
    };
    format!("{}-{day}.{ext}", title.trim())
}

fn run_list(ctx: Ctx, _: Value) -> Out {
    blocking_run("log.list", move || {
        let vault = ctx.session.vault()?;
        super::to_value(&session_log::list(&vault, &ctx.session.store())?)
    })
}

#[derive(Deserialize)]
struct IdParams {
    id: LogId,
}

/// The recorded bytes, as Base64.
fn run_read(ctx: Ctx, params: Value) -> Out {
    blocking_run("log.read", move || {
        let IdParams { id } = parse("log.read", params)?;
        let vault = ctx.session.vault()?;
        let bytes = read_unlocked(&ctx.session.shared_store(), &vault, id)?;
        Ok(json!({ "data_base64": data_encoding::BASE64.encode(&bytes) }))
    })
}

#[derive(Deserialize)]
struct MarkAddParams {
    id: LogId,
    mark: LogMark,
}

fn run_mark_add(ctx: Ctx, params: Value) -> Out {
    blocking_run("log.mark_add", move || {
        let MarkAddParams { id, mark } = parse("log.mark_add", params)?;
        let vault = ctx.session.vault()?;
        let (meta, at) = add_mark(&vault, &ctx.session.store(), id, mark)?;
        ctx.session.sync().nudge(&at.into_iter().collect::<Vec<_>>());
        super::to_value(&meta)
    })
}

#[derive(Deserialize)]
struct MarkRemoveParams {
    id: LogId,
    mark: Uuid,
}

fn run_mark_remove(ctx: Ctx, params: Value) -> Out {
    blocking_run("log.mark_remove", move || {
        let MarkRemoveParams { id, mark } = parse("log.mark_remove", params)?;
        let vault = ctx.session.vault()?;
        let (meta, at) = remove_mark(&vault, &ctx.session.store(), id, mark)?;
        ctx.session.sync().nudge(&at.into_iter().collect::<Vec<_>>());
        super::to_value(&meta)
    })
}

#[derive(Deserialize)]
struct DeleteParams {
    ids: Vec<LogId>,
}

fn run_delete(ctx: Ctx, params: Value) -> Out {
    blocking_run("log.delete", move || {
        let DeleteParams { ids } = parse("log.delete", params)?;
        let servers = delete_logs(&ctx.session.store(), &ids)?;
        ctx.session.sync().nudge(&servers);
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct SearchParams {
    query: String,
}

fn run_search(ctx: Ctx, params: Value) -> Out {
    blocking_run("log.search", move || {
        let SearchParams { query } = parse("log.search", params)?;
        let vault = ctx.session.vault()?;
        super::to_value(&search_unlocked(&ctx.session.shared_store(), &vault, &query)?)
    })
}

#[derive(Deserialize)]
struct ExportParams {
    id: LogId,
    format: ExportFormat,
    #[serde(default)]
    path: Option<std::path::PathBuf>,
}

/// Writes the log to `path`. Without one the user is asked where (a dialog in the interface);
/// a program that calls without a path gets an error, never a dialog.
fn run_export(ctx: Ctx, params: Value) -> Out {
    blocking_run("log.export", move || {
        let ExportParams { id, format, path } = parse("log.export", params.clone())?;
        let vault = ctx.session.vault()?;
        let Some(path) = path else {
            let meta = session_log::meta(&vault, &ctx.session.store(), id)?.ok_or(CommandError::code(ErrorCode::NotFound))?;
            let mut asked = params;
            if let Value::Object(map) = &mut asked {
                map.insert("name".into(), Value::String(export_name(&meta, format)));
            }
            return dialog_or_error(&ctx, "saveLog", &asked);
        };
        let bytes = export_bytes(&ctx.session.shared_store(), &vault, id, format)?;
        std::fs::write(&path, bytes).map_err(|e| CommandError::failed(ErrorCode::Io, e))?;
        Ok(json!({ "path": path }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshcraft_model::model::HostId;
    use sshcraft_store::session_log::{LogKind, MarkKind};
    use sshcraft_store::vault::MemoryMasterKey;

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
            .add_server(&sshcraft_store::store::ServerRow {
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
        LogMark { id: Uuid::new_v4(), offset, kind: MarkKind::Comment, text: text.into(), created_at: 4 }
    }

    #[test]
    fn export_text_drops_escape_sequences_raw_keeps_them() {
        let (store, vault) = setup();
        let id = recorded(&store, &vault, b"\x1b[32mok\x1b[0m\r\n");
        let store = Mutex::new(store);
        assert_eq!(export_bytes(&store, &vault, id, ExportFormat::Text).unwrap(), b"ok\n");
        assert_eq!(export_bytes(&store, &vault, id, ExportFormat::Raw).unwrap(), b"\x1b[32mok\x1b[0m\r\n");
    }

    #[test]
    fn export_names_are_safe_file_names() {
        let mut meta = LogMeta {
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
        assert_eq!(export_name(&meta, ExportFormat::Text), "db_ prod_eu_-2026-11-13.txt");
        meta.title = "web".into();
        meta.started_at = 0;
        assert_eq!(export_name(&meta, ExportFormat::Raw), "web-1970-01-01.log");
        meta.started_at = 951_782_400_000;
        assert_eq!(export_name(&meta, ExportFormat::Raw), "web-2000-02-29.log");
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
    fn search_does_not_hold_the_store_lock() {
        let (store, vault) = setup();
        recorded(&store, &vault, b"error one\r\n");
        recorded(&store, &vault, b"ERROR two\r\n");
        let store = Mutex::new(store);
        let free = std::cell::Cell::new(0);
        let hits = search_with(&store, &vault, "error", || {
            if store.try_lock().is_ok() {
                free.set(free.get() + 1);
            }
        })
        .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(free.get(), 2, "the store was locked while a log was opened");
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
        assert_eq!(store.outbox(server).unwrap(), vec![(ItemKind::Log, id.0.to_string())]);
        let (meta, _) = remove_mark(&vault, &store, id, meta.marks[0].id).unwrap();
        assert_eq!(meta.marks.len(), 1);
    }
}

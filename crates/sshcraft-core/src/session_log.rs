//! Session logs: what a terminal showed, recorded in parts, with bookmarks and comments. Every
//! part and the header are sealed with the vault, so the disk holds only ids, times and where a
//! log lives.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::logtext::plain_text;
use crate::model::{HostId, LogId, ServerId};
use crate::store::{Store, StoreError};
use crate::vault::{Vault, VaultError};

/// Bytes per stored part; one part plus Base64 stays below the 64 KiB a sync record may hold.
pub const CHUNK_SIZE: usize = 32 * 1024;
/// Recording stops at this size and the log is marked as truncated.
pub const MAX_LOG_SIZE: u64 = 64 * 1024 * 1024;
pub const MAX_TITLE: usize = 200;
pub const MAX_MARKS: usize = 500;
pub const MAX_BOOKMARK: usize = 100;
pub const MAX_COMMENT: usize = 2000;
pub const MAX_HITS: usize = 200;
pub const MAX_SNIPPET: usize = 200;
pub const MIN_QUERY: usize = 2;
pub const MAX_QUERY: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Ssh,
    Serial,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkKind {
    Bookmark,
    Comment,
}

/// A bookmark or a comment at a byte position of the recorded output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogMark {
    pub id: Uuid,
    pub offset: u64,
    pub kind: MarkKind,
    pub text: String,
    pub created_at: i64,
}

/// What is known about a log besides its output. Times in Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogMeta {
    pub id: LogId,
    pub title: String,
    pub kind: LogKind,
    #[serde(default)]
    pub host_id: Option<HostId>,
    pub started_at: i64,
    #[serde(default)]
    pub ended_at: Option<i64>,
    /// Recorded bytes; final once `ended_at` is set.
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub marks: Vec<LogMark>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogSummary {
    #[serde(flatten)]
    pub meta: LogMeta,
    /// The sync server it lives at; `None` for this device only.
    pub location: Option<ServerId>,
}

/// A line of a log that contains the searched text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogHit {
    pub log_id: LogId,
    pub line: u32,
    pub snippet: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("invalid session log: {0}")]
    Invalid(&'static str),
    #[error("session log not found")]
    NotFound,
    /// A part is missing, out of place or does not open.
    #[error("session log damaged")]
    Damaged,
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for LogError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(e.into())
    }
}

/// Version 10: session logs and their parts; log sync per server.
pub(crate) const SCHEMA_V10: &str = "
CREATE TABLE session_logs (
    id TEXT PRIMARY KEY,
    host_id TEXT,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    location TEXT,
    sealed BLOB NOT NULL
);
CREATE TABLE session_log_chunks (
    id TEXT PRIMARY KEY,
    log_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    len INTEGER NOT NULL,
    written_at INTEGER NOT NULL,
    location TEXT,
    sealed BLOB NOT NULL,
    UNIQUE (log_id, seq)
);
CREATE INDEX session_log_chunks_log ON session_log_chunks (log_id, seq);
ALTER TABLE sync_servers ADD COLUMN sync_logs INTEGER NOT NULL DEFAULT 0;
";

fn meta_label(id: LogId) -> String {
    format!("log:{}", id.0)
}

/// Sealed under its log and position: a part copied to another place does not open.
fn chunk_label(log: LogId, seq: u32) -> String {
    format!("log-chunk:{}:{seq}", log.0)
}

fn text_ok(text: &str, max: usize, lines: bool) -> bool {
    text.chars().count() <= max
        && !text
            .chars()
            .any(|c| c.is_control() && !(lines && c == '\n'))
}

pub fn validate_marks(marks: &[LogMark], size: u64) -> Result<(), LogError> {
    if marks.len() > MAX_MARKS {
        return Err(LogError::Invalid("marks"));
    }
    for mark in marks {
        let text_valid = match mark.kind {
            MarkKind::Bookmark => text_ok(&mark.text, MAX_BOOKMARK, false),
            MarkKind::Comment => {
                !mark.text.trim().is_empty() && text_ok(&mark.text, MAX_COMMENT, true)
            }
        };
        if !text_valid || mark.offset > size {
            return Err(LogError::Invalid("mark"));
        }
    }
    Ok(())
}

fn validate_meta(meta: &LogMeta, stored: u64) -> Result<(), LogError> {
    if meta.title.trim().is_empty() || !text_ok(&meta.title, MAX_TITLE, false) {
        return Err(LogError::Invalid("title"));
    }
    if meta.size > MAX_LOG_SIZE {
        return Err(LogError::Invalid("size"));
    }
    validate_marks(&meta.marks, meta.size.max(stored))
}

/// Bytes stored for `id` so far.
pub fn stored_size(store: &Store, id: LogId) -> Result<u64, LogError> {
    let size: i64 = store.conn.query_row(
        "SELECT COALESCE(SUM(len), 0) FROM session_log_chunks WHERE log_id = ?1",
        [id.0.to_string()],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(size).unwrap_or(0))
}

/// Stores the header of a log at `location`, new or replacing the one there was.
pub fn put_meta(
    vault: &Vault,
    store: &Store,
    meta: &LogMeta,
    location: Option<ServerId>,
) -> Result<(), LogError> {
    validate_meta(meta, stored_size(store, meta.id)?)?;
    let json = serde_json::to_vec(meta).map_err(|e| StoreError::Malformed(e.to_string()))?;
    let sealed = vault.seal_secret(&meta_label(meta.id), &json)?;
    store.conn.execute(
        "INSERT INTO session_logs (id, host_id, started_at, ended_at, location, sealed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (id) DO UPDATE SET host_id = ?2, started_at = ?3, ended_at = ?4,
             location = ?5, sealed = ?6",
        params![
            meta.id.0.to_string(),
            meta.host_id.map(|h| h.0.to_string()),
            meta.started_at,
            meta.ended_at,
            location.map(|s| s.0.to_string()),
            sealed,
        ],
    )?;
    Ok(())
}

/// Starts a log on this device.
pub fn create(vault: &Vault, store: &Store, meta: &LogMeta) -> Result<(), LogError> {
    put_meta(vault, store, meta, None)
}

/// Stores part `seq` of a log under the part id `chunk_id` (a part that came by sync keeps its
/// id). A part already stored at that position is replaced.
#[allow(clippy::too_many_arguments)]
pub fn put_chunk(
    vault: &Vault,
    store: &Store,
    chunk_id: Uuid,
    log: LogId,
    seq: u32,
    bytes: &[u8],
    written_at: i64,
    location: Option<ServerId>,
) -> Result<(), LogError> {
    if bytes.is_empty() || bytes.len() > CHUNK_SIZE {
        return Err(LogError::Invalid("chunk"));
    }
    let sealed = vault.seal_secret(&chunk_label(log, seq), bytes)?;
    store.conn.execute(
        "INSERT OR REPLACE INTO session_log_chunks
             (id, log_id, seq, len, written_at, location, sealed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            chunk_id.to_string(),
            log.0.to_string(),
            seq,
            i64::try_from(bytes.len()).unwrap_or(i64::MAX),
            written_at,
            location.map(|s| s.0.to_string()),
            sealed,
        ],
    )?;
    Ok(())
}

/// Appends the next part of a log that is being recorded here.
pub fn append_chunk(
    vault: &Vault,
    store: &Store,
    log: LogId,
    seq: u32,
    bytes: &[u8],
    written_at: i64,
) -> Result<(), LogError> {
    put_chunk(
        vault,
        store,
        Uuid::new_v4(),
        log,
        seq,
        bytes,
        written_at,
        None,
    )
}

/// One stored part of a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredChunk {
    pub log: LogId,
    pub seq: u32,
    pub bytes: Vec<u8>,
    pub written_at: i64,
}

/// A part as stored, by its id.
pub fn chunk(
    vault: &Vault,
    store: &Store,
    chunk_id: &str,
) -> Result<Option<StoredChunk>, LogError> {
    let row: Option<(String, u32, i64, Vec<u8>)> = store
        .conn
        .query_row(
            "SELECT log_id, seq, written_at, sealed FROM session_log_chunks WHERE id = ?1",
            [chunk_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((log, seq, written_at, sealed)) = row else {
        return Ok(None);
    };
    let log = LogId(Uuid::parse_str(&log).map_err(|_| LogError::Damaged)?);
    let plain = vault
        .open_secret(&chunk_label(log, seq), &sealed)
        .map_err(|_| LogError::Damaged)?;
    Ok(Some(StoredChunk {
        log,
        seq,
        bytes: plain.to_vec(),
        written_at,
    }))
}

/// The header of a log, if it is here and opens.
pub fn meta(vault: &Vault, store: &Store, id: LogId) -> Result<Option<LogMeta>, LogError> {
    let sealed: Option<Vec<u8>> = store
        .conn
        .query_row(
            "SELECT sealed FROM session_logs WHERE id = ?1",
            [id.0.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    let Some(sealed) = sealed else {
        return Ok(None);
    };
    let plain = vault
        .open_secret(&meta_label(id), &sealed)
        .map_err(|_| LogError::Damaged)?;
    let meta: LogMeta = serde_json::from_slice(&plain).map_err(|_| LogError::Damaged)?;
    if meta.id != id {
        return Err(LogError::Damaged);
    }
    Ok(Some(meta))
}

/// Where a log lives; `None` if it is not here.
pub fn location(store: &Store, id: LogId) -> Result<Option<Option<ServerId>>, LogError> {
    let row: Option<Option<String>> = store
        .conn
        .query_row(
            "SELECT location FROM session_logs WHERE id = ?1",
            [id.0.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(row.map(|loc| loc.and_then(|t| Uuid::parse_str(&t).ok()).map(ServerId)))
}

/// Ends a log: its final size, whether it was cut, when. Bookmarks and comments stay.
pub fn finish(
    vault: &Vault,
    store: &Store,
    id: LogId,
    ended_at: i64,
    size: u64,
    truncated: bool,
) -> Result<LogMeta, LogError> {
    let mut meta = meta(vault, store, id)?.ok_or(LogError::NotFound)?;
    let location = location(store, id)?.flatten();
    meta.ended_at = Some(ended_at);
    meta.size = size;
    meta.truncated = truncated;
    put_meta(vault, store, &meta, location)?;
    Ok(meta)
}

/// Replaces the bookmarks and comments of a log.
pub fn save_marks(
    vault: &Vault,
    store: &Store,
    id: LogId,
    marks: Vec<LogMark>,
) -> Result<LogMeta, LogError> {
    let mut meta = meta(vault, store, id)?.ok_or(LogError::NotFound)?;
    let location = location(store, id)?.flatten();
    meta.marks = marks;
    put_meta(vault, store, &meta, location)?;
    Ok(meta)
}

/// Every log with a header that opens, newest first.
pub fn list(vault: &Vault, store: &Store) -> Result<Vec<LogSummary>, LogError> {
    let mut stmt = store
        .conn
        .prepare("SELECT id, location FROM session_logs ORDER BY started_at DESC, id")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, location)| {
            let id = LogId(Uuid::parse_str(&id).ok()?);
            let meta = meta(vault, store, id).ok()??;
            let location = location
                .and_then(|t| Uuid::parse_str(&t).ok())
                .map(ServerId);
            Some(LogSummary { meta, location })
        })
        .collect())
}

/// The recorded output of a log, all parts in order.
pub fn read(vault: &Vault, store: &Store, id: LogId) -> Result<Vec<u8>, LogError> {
    let meta = meta(vault, store, id)?.ok_or(LogError::NotFound)?;
    let mut stmt = store
        .conn
        .prepare("SELECT seq, sealed FROM session_log_chunks WHERE log_id = ?1 ORDER BY seq")?;
    let rows = stmt
        .query_map([id.0.to_string()], |r| {
            Ok((r.get::<_, u32>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (expected, (seq, sealed)) in rows.into_iter().enumerate() {
        if usize::try_from(seq).ok() != Some(expected) {
            return Err(LogError::Damaged);
        }
        let plain = vault
            .open_secret(&chunk_label(id, seq), &sealed)
            .map_err(|_| LogError::Damaged)?;
        out.extend_from_slice(&plain);
    }
    if meta.ended_at.is_some() && u64::try_from(out.len()).ok() != Some(meta.size) {
        return Err(LogError::Damaged);
    }
    Ok(out)
}

/// Removes a log and its parts. Parts that came without their header go too.
pub fn delete(store: &Store, id: LogId) -> Result<(), LogError> {
    let tx = store.conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM session_log_chunks WHERE log_id = ?1",
        [id.0.to_string()],
    )?;
    tx.execute("DELETE FROM session_logs WHERE id = ?1", [id.0.to_string()])?;
    tx.commit()?;
    Ok(())
}

/// Removes one part (a deletion that came by sync).
pub fn delete_chunk(store: &Store, chunk_id: &str) -> Result<(), LogError> {
    store
        .conn
        .execute("DELETE FROM session_log_chunks WHERE id = ?1", [chunk_id])?;
    Ok(())
}

/// The ids of a log's parts, in order.
pub fn chunk_ids(store: &Store, id: LogId) -> Result<Vec<String>, LogError> {
    let mut stmt = store
        .conn
        .prepare("SELECT id FROM session_log_chunks WHERE log_id = ?1 ORDER BY seq")?;
    let ids = stmt
        .query_map([id.0.to_string()], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(ids)
}

/// Moves a log with its parts to a server or back to this device.
pub fn set_location(store: &Store, id: LogId, location: Option<ServerId>) -> Result<(), LogError> {
    let target = location.map(|s| s.0.to_string());
    let tx = store.conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE session_logs SET location = ?2 WHERE id = ?1",
        params![id.0.to_string(), target],
    )?;
    tx.execute(
        "UPDATE session_log_chunks SET location = ?2 WHERE log_id = ?1",
        params![id.0.to_string(), target],
    )?;
    tx.commit()?;
    Ok(())
}

/// The ended logs of a host.
pub fn logs_of_host(store: &Store, host: HostId) -> Result<Vec<LogId>, LogError> {
    ids(
        store,
        "SELECT id FROM session_logs WHERE host_id = ?1 AND ended_at IS NOT NULL",
        params![host.0.to_string()],
    )
}

/// A log whose place differs from its host's: `(log, where it is, where its host is)`.
pub type Misplaced = (LogId, Option<ServerId>, Option<ServerId>);

/// Ended logs of hosts on this device that live elsewhere than their host.
pub fn misplaced(store: &Store) -> Result<Vec<Misplaced>, LogError> {
    let mut stmt = store.conn.prepare(
        "SELECT l.id, l.location, h.location FROM session_logs l JOIN hosts h ON h.id = l.host_id
         WHERE l.ended_at IS NOT NULL AND l.location IS NOT h.location",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let server = |t: Option<String>| t.and_then(|t| Uuid::parse_str(&t).ok()).map(ServerId);
    Ok(rows
        .into_iter()
        .filter_map(|(id, at, host)| {
            Some((LogId(Uuid::parse_str(&id).ok()?), server(at), server(host)))
        })
        .collect())
}

/// Ended logs that live at `server`.
pub fn logs_at(store: &Store, server: ServerId) -> Result<Vec<LogId>, LogError> {
    ids(
        store,
        "SELECT id FROM session_logs WHERE location = ?1 AND ended_at IS NOT NULL",
        params![server.0.to_string()],
    )
}

/// Ended logs that ended before `before_ms`.
pub fn expired(store: &Store, before_ms: i64) -> Result<Vec<LogId>, LogError> {
    ids(
        store,
        "SELECT id FROM session_logs WHERE ended_at IS NOT NULL AND ended_at < ?1",
        params![before_ms],
    )
}

fn ids(store: &Store, sql: &str, params: impl rusqlite::Params) -> Result<Vec<LogId>, LogError> {
    let mut stmt = store.conn.prepare(sql)?;
    let rows = stmt
        .query_map(params, |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|id| Uuid::parse_str(&id).ok().map(LogId))
        .collect())
}

/// Ends the logs whose recording never ended (the app stopped while recording), at the time of
/// their last part. Call it only while nothing records.
pub fn finish_abandoned(vault: &Vault, store: &Store) -> Result<Vec<LogId>, LogError> {
    let open = ids(
        store,
        "SELECT id FROM session_logs WHERE ended_at IS NULL",
        [],
    )?;
    let mut done = Vec::new();
    for id in open {
        let Ok(Some(meta)) = meta(vault, store, id) else {
            continue;
        };
        let last: Option<i64> = store.conn.query_row(
            "SELECT MAX(written_at) FROM session_log_chunks WHERE log_id = ?1",
            [id.0.to_string()],
            |r| r.get(0),
        )?;
        let size = stored_size(store, id)?;
        let ended = last.unwrap_or(meta.started_at).max(meta.started_at);
        finish(vault, store, id, ended, size, meta.truncated)?;
        done.push(id);
    }
    Ok(done)
}

/// Lines of all logs containing `query`, case-insensitive, newest log first. A log that does
/// not open is skipped.
pub fn search(vault: &Vault, store: &Store, query: &str) -> Result<Vec<LogHit>, LogError> {
    let needle = query.trim().to_lowercase();
    let len = needle.chars().count();
    if !(MIN_QUERY..=MAX_QUERY).contains(&len) {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    for summary in list(vault, store)? {
        let Ok(bytes) = read(vault, store, summary.meta.id) else {
            continue;
        };
        for (number, line) in plain_text(&bytes).lines().enumerate() {
            let lower = line.to_lowercase();
            let Some(at) = lower.find(&needle) else {
                continue;
            };
            hits.push(LogHit {
                log_id: summary.meta.id,
                line: u32::try_from(number).unwrap_or(u32::MAX),
                snippet: snippet(line, lower[..at].chars().count()),
            });
            if hits.len() == MAX_HITS {
                return Ok(hits);
            }
        }
    }
    Ok(hits)
}

/// Up to [`MAX_SNIPPET`] characters of `line` around the character position `at`.
fn snippet(line: &str, at: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let start = at
        .saturating_sub(MAX_SNIPPET / 3)
        .min(chars.len().saturating_sub(MAX_SNIPPET));
    chars[start..chars.len().min(start + MAX_SNIPPET)]
        .iter()
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
#[path = "session_log_tests.rs"]
mod tests;

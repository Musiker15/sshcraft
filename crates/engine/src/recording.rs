//! Records what a terminal shows into a session log: a sink around the session's sink that
//! keeps the output in parts, sealed with the vault. Recording never ends a session; when
//! storing fails, only the recording stops.

use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use sshcraft_model::model::{LogId, ServerId};
use sshcraft_store::session_log::{self, LogError, LogMark, LogMeta, MarkKind};
use sshcraft_store::store::Store;
use sshcraft_store::vault::Vault;
use tokio::time::Instant;
use uuid::Uuid;

use crate::sessions::{SessionSink, SessionStatus};

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Sizes and times of a recording; smaller in tests.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub chunk: usize,
    pub max: u64,
    /// Output waits at most this long before it is stored.
    pub flush_after: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self { chunk: session_log::CHUNK_SIZE, max: session_log::MAX_LOG_SIZE, flush_after: Duration::from_secs(5) }
    }
}

#[derive(Default)]
struct State {
    buffer: Vec<u8>,
    seq: u32,
    /// Bytes taken into the log, stored or still in the buffer.
    size: u64,
    truncated: bool,
    /// Storing failed: nothing more is recorded.
    failed: bool,
    finished: bool,
    /// When the oldest byte in the buffer came.
    pending_since: Option<Instant>,
}

/// Told the id of each log that ended (to place it and send it to its sync server).
pub type OnFinish = Arc<dyn Fn(LogId) + Send + Sync>;

pub struct Recorder {
    store: Arc<Mutex<Store>>,
    vault: Arc<Vault>,
    id: LogId,
    limits: Limits,
    state: Mutex<State>,
    on_finish: Option<OnFinish>,
}

impl Recorder {
    /// Starts a log; `None` if it cannot even be created (the session runs unrecorded).
    pub fn start(store: Arc<Mutex<Store>>, vault: Arc<Vault>, meta: &LogMeta, limits: Limits, on_finish: Option<OnFinish>) -> Option<Arc<Self>> {
        session_log::create(&vault, &store.lock().unwrap_or_else(PoisonError::into_inner), meta).ok()?;
        let recorder = Arc::new(Self { store, vault, id: meta.id, limits, state: Mutex::new(State::default()), on_finish });
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(flush_regularly(Arc::downgrade(&recorder)));
        }
        Some(recorder)
    }

    pub fn id(&self) -> LogId {
        self.id
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes output into the log, up to the size limit.
    pub fn record(&self, bytes: &[u8]) {
        let mut state = self.state();
        if state.finished || state.failed || state.truncated || bytes.is_empty() {
            return;
        }
        let room = usize::try_from(self.limits.max.saturating_sub(state.size)).unwrap_or(usize::MAX);
        let take = bytes.len().min(room);
        if take < bytes.len() {
            state.truncated = true;
        }
        state.buffer.extend_from_slice(&bytes[..take]);
        state.size += take as u64;
        if state.pending_since.is_none() && !state.buffer.is_empty() {
            state.pending_since = Some(Instant::now());
        }
        while state.buffer.len() >= self.limits.chunk {
            let part: Vec<u8> = state.buffer.drain(..self.limits.chunk).collect();
            self.write(&mut state, &part);
        }
        if state.buffer.is_empty() {
            state.pending_since = None;
        }
    }

    fn write(&self, state: &mut State, part: &[u8]) {
        if state.failed || part.is_empty() {
            return;
        }
        let stored =
            session_log::append_chunk(&self.vault, &self.store.lock().unwrap_or_else(PoisonError::into_inner), self.id, state.seq, part, now_ms());
        match stored {
            Ok(()) => state.seq += 1,
            Err(_) => state.failed = true,
        }
    }

    fn flush(&self, state: &mut State) {
        let rest = std::mem::take(&mut state.buffer);
        for part in rest.chunks(self.limits.chunk) {
            self.write(state, part);
        }
        state.pending_since = None;
    }

    /// Stores what waited longer than `flush_after`.
    pub fn flush_if_due(&self) {
        let mut state = self.state();
        if state.pending_since.is_some_and(|since| since.elapsed() >= self.limits.flush_after) {
            self.flush(&mut state);
        }
    }

    pub fn is_finished(&self) -> bool {
        self.state().finished
    }

    /// Still taking output: not ended, not cut at the size limit, storing works.
    pub fn is_active(&self) -> bool {
        let state = self.state();
        !state.finished && !state.failed && !state.truncated
    }

    /// Bytes stored so far: what a bookmark points at and what the log ends with.
    fn stored(&self, state: &State) -> u64 {
        if state.failed {
            session_log::stored_size(&self.store.lock().unwrap_or_else(PoisonError::into_inner), self.id).unwrap_or(0)
        } else {
            state.size
        }
    }

    /// A bookmark at the output recorded so far, with `text` (the time when empty).
    pub fn bookmark(&self, text: &str) -> Result<LogMark, LogError> {
        let mut state = self.state();
        // An ended log is placed and maybe synced already; the viewer edits it from then on.
        if state.finished || state.failed {
            return Err(LogError::NotFound);
        }
        self.flush(&mut state);
        let offset = self.stored(&state);
        let mark = LogMark { id: Uuid::new_v4(), offset, kind: MarkKind::Bookmark, text: text.to_owned(), created_at: now_ms() };
        let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        session_log::add_mark(&self.vault, &store, self.id, mark.clone())?;
        Ok(mark)
    }

    /// Stores the rest and ends the log; later calls do nothing.
    pub fn finish(&self) -> Option<LogMeta> {
        let done = {
            let mut state = self.state();
            if state.finished {
                return None;
            }
            self.flush(&mut state);
            state.finished = true;
            let size = self.stored(&state);
            let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
            let marked = session_log::meta(&self.vault, &store, self.id).ok().flatten().is_some_and(|m| !m.marks.is_empty());
            if size == 0 && !marked {
                // Nothing came (the connection failed, or the tab closed at once): no log.
                let _ = session_log::delete(&store, self.id);
                None
            } else {
                session_log::finish(&self.vault, &store, self.id, now_ms(), size, state.truncated).ok()
            }
        };
        if done.is_some()
            && let Some(on_finish) = &self.on_finish
        {
            on_finish(self.id);
        }
        done
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.finish();
    }
}

async fn flush_regularly(recorder: Weak<Recorder>) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tick.tick().await;
        let Some(recorder) = recorder.upgrade() else {
            return;
        };
        if recorder.is_finished() {
            return;
        }
        recorder.flush_if_due();
    }
}

/// Passes output and status on to the session's sink and records the output.
pub struct LogSink {
    pub inner: Arc<dyn SessionSink>,
    pub recorder: Arc<Recorder>,
}

impl SessionSink for LogSink {
    fn data(&self, bytes: Vec<u8>) {
        self.recorder.record(&bytes);
        self.inner.data(bytes);
    }

    fn status(&self, status: SessionStatus) {
        if matches!(status, SessionStatus::Ended { .. }) {
            self.recorder.finish();
        }
        self.inner.status(status);
    }
}

/// Ends logs left open by a stopped app and removes their stray parts (at start only), and
/// deletes those older than the retention allows, here and at their sync servers. Returns the
/// servers that have deletions to send.
pub fn housekeeping(store: &Mutex<Store>, vault: &Vault, retention_days: u32, now_ms: i64, at_start: bool) -> Result<Vec<ServerId>, String> {
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    if at_start {
        session_log::finish_abandoned(vault, &store).map_err(|e| e.to_string())?;
        session_log::purge_orphans(&store).map_err(|e| e.to_string())?;
    }
    if retention_days == 0 {
        return Ok(Vec::new());
    }
    let before = now_ms.saturating_sub(i64::from(retention_days) * 24 * 60 * 60 * 1000);
    let expired = session_log::expired(&store, before).map_err(|e| e.to_string())?;
    let mut servers = Vec::new();
    for id in expired {
        if let Some(server) = sshcraft_sync::logs::delete_log(&store, id, now_ms).map_err(|e| e.to_string())?
            && !servers.contains(&server)
        {
            servers.push(server);
        }
    }
    Ok(servers)
}

/// Deletes expired logs every hour (the first run is at start, see `housekeeping`) and tells
/// `nudge` the servers that have deletions to send.
pub async fn tidy_regularly(
    store: Arc<Mutex<Store>>,
    vault: Arc<Vault>,
    settings: crate::sessions::ReadSettings,
    nudge: impl Fn(&[ServerId]) + Send + 'static,
) {
    loop {
        tokio::time::sleep(Duration::from_secs(60 * 60)).await;
        let days = settings().logs.log_retention_days;
        if let Ok(servers) = housekeeping(&store, &vault, days, now_ms(), false) {
            nudge(&servers);
        }
    }
}

#[cfg(test)]
#[path = "recording_tests.rs"]
mod tests;

//! Session logs in sync: they live where their host lives, in the personal area of that
//! server, and go out part by part, the header last.

use sshcraft_core::model::{LogId, ServerId};
use sshcraft_core::session_log;
use sshcraft_core::store::{ItemKind, Store};

use crate::SyncError;
use crate::items::log_error;

/// Queues a log and its parts at `server`: the parts first, the header after them.
fn queue_log(store: &Store, server: ServerId, id: LogId, now_ms: i64) -> Result<(), SyncError> {
    for chunk in session_log::chunk_ids(store, id).map_err(log_error)? {
        store.queue(server, ItemKind::LogChunk, &chunk, now_ms)?;
    }
    store.queue(server, ItemKind::Log, &id.0.to_string(), now_ms)?;
    Ok(())
}

/// Moves every ended log to where its host lives now and queues it at the old and the new
/// place. Returns the servers that have something to send.
pub fn place_logs(store: &Store, now_ms: i64) -> Result<Vec<ServerId>, SyncError> {
    let mut servers = Vec::new();
    for (id, at, wanted) in session_log::misplaced(store).map_err(log_error)? {
        if let Some(old) = at {
            queue_log(store, old, id, now_ms)?;
            servers.push(old);
        }
        session_log::set_location(store, id, wanted).map_err(log_error)?;
        if let Some(new) = wanted {
            queue_log(store, new, id, now_ms)?;
            servers.push(new);
        }
    }
    servers.sort_by_key(|s| s.0);
    servers.dedup();
    Ok(servers)
}

/// Deletes a log here and, if it lives at a server, there too. Returns that server.
pub fn delete_log(store: &Store, id: LogId, now_ms: i64) -> Result<Option<ServerId>, SyncError> {
    let at = session_log::location(store, id)
        .map_err(log_error)?
        .flatten();
    if let Some(server) = at {
        queue_log(store, server, id, now_ms)?;
    }
    session_log::delete(store, id).map_err(log_error)?;
    Ok(at)
}

/// Switches log sync for a server; turning it on queues the logs already there.
pub fn set_sync_logs(
    store: &Store,
    server: ServerId,
    on: bool,
    now_ms: i64,
) -> Result<(), SyncError> {
    store.set_sync_logs(server, on)?;
    if on {
        place_logs(store, now_ms)?;
        for id in session_log::logs_at(store, server).map_err(log_error)? {
            queue_log(store, server, id, now_ms)?;
        }
    }
    Ok(())
}

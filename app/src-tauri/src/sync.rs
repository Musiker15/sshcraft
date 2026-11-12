//! Sync in the app: one runtime per server, status for the window, and the bookkeeping that
//! keeps local changes queued for the server their item lives at.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sshcraft_core::model::ServerId;
use sshcraft_core::store::{ItemKind, Store};
use sshcraft_core::vault::Vault;
use sshcraft_proto::ProtoError;
use sshcraft_sync::SyncError;
use sshcraft_sync::engine::{record_group_hosts, record_local_change};
use sshcraft_sync::runtime::{SyncObserver, SyncRuntime, SyncStatus};

use crate::errors::{CommandError, ErrorCode};

/// How the window sees a server's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StatusView {
    Idle,
    Syncing,
    Offline { retry_in_secs: u64 },
    Revoked,
    Blocked,
    Error { detail: String },
    Off,
}

impl From<&SyncStatus> for StatusView {
    fn from(status: &SyncStatus) -> Self {
        match status {
            SyncStatus::Idle => Self::Idle,
            SyncStatus::Syncing => Self::Syncing,
            SyncStatus::Offline { retry_in_secs } => Self::Offline {
                retry_in_secs: *retry_in_secs,
            },
            SyncStatus::Revoked => Self::Revoked,
            SyncStatus::Blocked => Self::Blocked,
            SyncStatus::Error(detail) => Self::Error {
                detail: detail.clone(),
            },
        }
    }
}

/// Remembers the last status per server and passes everything on to the window.
struct StatusRecorder {
    inner: Arc<dyn SyncObserver>,
    statuses: Mutex<HashMap<ServerId, SyncStatus>>,
}

impl SyncObserver for StatusRecorder {
    fn status(&self, server: ServerId, status: SyncStatus) {
        if let Ok(mut map) = self.statuses.lock() {
            map.insert(server, status.clone());
        }
        self.inner.status(server, status);
    }

    fn data_changed(&self, server: ServerId) {
        self.inner.data_changed(server);
    }

    fn warning(&self, server: ServerId, message: String) {
        self.inner.warning(server, message);
    }
}

pub struct SyncManager {
    store: Arc<Mutex<Store>>,
    vault: Option<Arc<Vault>>,
    observer: Arc<StatusRecorder>,
    runtimes: Mutex<HashMap<ServerId, SyncRuntime>>,
}

impl SyncManager {
    pub fn new(
        store: Arc<Mutex<Store>>,
        vault: Option<Arc<Vault>>,
        events: Arc<dyn SyncObserver>,
    ) -> Self {
        Self {
            store,
            vault,
            observer: Arc::new(StatusRecorder {
                inner: events,
                statuses: Mutex::default(),
            }),
            runtimes: Mutex::default(),
        }
    }

    /// Starts a runtime for every server with sync on. Must run inside the async runtime.
    pub fn start_all(&self) {
        let servers = self
            .store
            .lock()
            .ok()
            .and_then(|s| s.servers().ok())
            .unwrap_or_default();
        for row in servers {
            self.start(row.id);
        }
    }

    /// Starts (or restarts) the runtime of one server, if sync is on and the vault is open.
    pub fn start(&self, server: ServerId) {
        self.stop(server);
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let wanted = self
            .store
            .lock()
            .ok()
            .and_then(|s| s.server(server).ok().flatten())
            .is_some_and(|row| row.sync_enabled && !row.revoked);
        if !wanted {
            return;
        }
        let runtime = SyncRuntime::start(
            self.store.clone(),
            vault,
            server,
            self.observer.clone() as Arc<dyn SyncObserver>,
        );
        if let Ok(mut map) = self.runtimes.lock() {
            map.insert(server, runtime);
        }
    }

    pub fn stop(&self, server: ServerId) {
        if let Ok(mut map) = self.runtimes.lock()
            && let Some(runtime) = map.remove(&server)
        {
            runtime.stop();
        }
        if let Ok(mut map) = self.observer.statuses.lock() {
            map.remove(&server);
        }
    }

    /// A local change is queued for these servers: send it now.
    pub fn nudge(&self, servers: &[ServerId]) {
        if let Ok(map) = self.runtimes.lock() {
            for server in servers {
                if let Some(runtime) = map.get(server) {
                    runtime.nudge();
                }
            }
        }
    }

    pub fn status(&self, server: ServerId) -> StatusView {
        let running = self
            .runtimes
            .lock()
            .is_ok_and(|map| map.contains_key(&server));
        self.observer
            .statuses
            .lock()
            .ok()
            .and_then(|map| map.get(&server).map(StatusView::from))
            .unwrap_or(if running {
                StatusView::Syncing
            } else {
                StatusView::Off
            })
    }
}

/// What the window shows for a server: a revoked device stays "revoked" after a restart, when no
/// runtime runs for it any more.
pub fn server_status(revoked: bool, live: StatusView) -> StatusView {
    if revoked { StatusView::Revoked } else { live }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Queues an item that changed. `before` is where it lived before the change: if it left a
/// server, that server learns it is gone there. Returns the servers to nudge.
pub fn note_change(
    store: &Store,
    kind: ItemKind,
    id: &str,
    before: Option<ServerId>,
    now: i64,
) -> Result<Vec<ServerId>, CommandError> {
    let after = store.location_of(kind, id)?;
    let mut servers = Vec::new();
    if let Some(old) = before
        && Some(old) != after
    {
        store.queue(old, kind, id, now)?;
        if kind == ItemKind::Group {
            for host in store.list_hosts()? {
                if host.group_id.is_some_and(|g| g.0.to_string() == id) {
                    store.queue(old, ItemKind::Host, &host.id.0.to_string(), now)?;
                }
            }
        }
        servers.push(old);
    }
    record_local_change(store, kind, id, now).map_err(sync_error)?;
    // A moved group takes its hosts along to the new place.
    if kind == ItemKind::Group && before != after {
        record_group_hosts(store, id, now).map_err(sync_error)?;
    }
    if let Some(new) = after {
        servers.push(new);
    }
    Ok(servers)
}

/// Error codes of the sync server (docs/server-api.md).
const KNOWN_SERVER_CODES: &[&str] = &[
    "bad_api_key",
    "unauthorized",
    "forbidden",
    "account_locked",
    "device_inactive",
    "device_blocked",
    "not_found",
    "method_not_allowed",
    "conflict",
    "email_taken",
    "account_exists",
    "device_id_taken",
    "invalid",
    "too_large",
    "busy",
    "internal",
];

/// The window's error code for a sync error.
pub fn sync_error(err: SyncError) -> CommandError {
    let code = match &err {
        SyncError::BadOrigin => ErrorCode::SyncBadOrigin,
        SyncError::OriginMismatch { .. } => ErrorCode::SyncOriginMismatch,
        SyncError::Network(_) => ErrorCode::SyncNetwork,
        SyncError::Server { code, .. } => match code.as_str() {
            "bad_api_key" => ErrorCode::SyncBadApiKey,
            "email_taken" => ErrorCode::SyncEmailTaken,
            "device_inactive" => ErrorCode::SyncRevoked,
            "device_blocked" => ErrorCode::SyncBlocked,
            _ => ErrorCode::SyncServer,
        },
        SyncError::Proto(ProtoError::BadChecksum) => ErrorCode::SyncChecksum,
        SyncError::Proto(ProtoError::BadAccountKeyFormat) => ErrorCode::SyncBadAccountKey,
        SyncError::Proto(_) => ErrorCode::SyncProtocol,
        SyncError::Store(_) => ErrorCode::Store,
        SyncError::Vault(_) => ErrorCode::Vault,
        SyncError::Revoked => ErrorCode::SyncRevoked,
        SyncError::Blocked => ErrorCode::SyncBlocked,
        SyncError::NotSetUp => ErrorCode::NotFound,
        SyncError::NotAllowed => ErrorCode::SyncNotAllowed,
        SyncError::AccountNotFound => ErrorCode::SyncAccountNotFound,
    };
    let detail = match err {
        // The code comes from the server: only ones this app knows reach the window.
        SyncError::Server { code, .. } if KNOWN_SERVER_CODES.contains(&code.as_str()) => code,
        SyncError::Server { .. } => "unknown".to_owned(),
        other => other.to_string(),
    };
    CommandError::new(code, detail)
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;

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
            // Everything below goes away from the old server with the group.
            let gid = sshcraft_core::model::GroupId(
                uuid::Uuid::parse_str(id).map_err(|_| CommandError::code(ErrorCode::Store))?,
            );
            let mut groups = vec![id.to_owned()];
            for sub in store.subgroups(gid)? {
                store.queue(old, ItemKind::Group, &sub.0.to_string(), now)?;
                groups.push(sub.0.to_string());
            }
            for host in store.list_hosts()? {
                if host
                    .group_id
                    .is_some_and(|g| groups.contains(&g.0.to_string()))
                {
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

/// A change the window wants to make, checked against shares this account received.
pub enum Change<'a> {
    SaveHost(&'a sshcraft_core::model::Host),
    SaveGroup(&'a sshcraft_core::model::Group),
    DeleteHost(sshcraft_core::model::HostId),
    DeleteGroup(sshcraft_core::model::GroupId),
    Move(ItemKind, &'a str),
}

/// The received share an item is in, if any.
fn received(
    store: &Store,
    kind: ItemKind,
    id: &str,
) -> Result<Option<sshcraft_core::store::ShareRow>, CommandError> {
    let Some(server) = store.location_of(kind, id)? else {
        return Ok(None);
    };
    Ok(store.received_share_of(server, kind, id)?)
}

/// Whether the change may be made to an item that came in a share (design, section 8): only
/// with write, a selection or one host only changed, never deleted, moved or shared on. `Ok`
/// says whether to send it: a key assigned to a read-only item stays on this device.
pub fn share_rights(store: &Store, change: Change<'_>) -> Result<bool, CommandError> {
    use sshcraft_core::store::{ShareKind, ShareRole};
    let read_only = || CommandError::code(ErrorCode::ShareReadOnly);
    match change {
        Change::SaveHost(host) => {
            let id = host.id.0.to_string();
            let existing = store.list_hosts()?.into_iter().find(|h| h.id == host.id);
            let share = match (&existing, host.group_id) {
                (Some(_), _) => received(store, ItemKind::Host, &id)?,
                (None, Some(group)) => received(store, ItemKind::Group, &group.0.to_string())?,
                (None, None) => None,
            };
            // Putting a host into a group that came in a share adds to that share: only a
            // group shared with all hosts and the right to change allow it.
            let moved = existing
                .as_ref()
                .is_some_and(|old| old.group_id != host.group_id);
            if moved && let Some(group) = host.group_id {
                let target = received(store, ItemKind::Group, &group.0.to_string())?;
                if target.is_some_and(|t| {
                    t.role != Some(ShareRole::Write) || t.kind != ShareKind::GroupAll
                }) {
                    return Err(read_only());
                }
            }
            let Some(share) = share else {
                return Ok(true);
            };
            if moved {
                return Err(CommandError::code(ErrorCode::ShareNotOwner));
            }
            match (share.role, existing) {
                (Some(ShareRole::Write), Some(_)) => Ok(true),
                (Some(ShareRole::Write), None) if share.kind == ShareKind::GroupAll => Ok(true),
                (Some(ShareRole::Read), Some(mut old)) => {
                    old.settings.key = host.settings.key.clone();
                    old.location = host.location;
                    if old == *host {
                        Ok(false)
                    } else {
                        Err(read_only())
                    }
                }
                _ => Err(read_only()),
            }
        }
        Change::SaveGroup(group) => {
            let id = group.id.0.to_string();
            let existing = store.list_groups()?.into_iter().find(|g| g.id == group.id);
            let moved = existing.as_ref().map_or(group.parent_id.is_some(), |old| {
                old.parent_id != group.parent_id
            });
            if moved {
                // Nesting into a group that came in a share adds to that share.
                if let Some(parent) = group.parent_id {
                    let target = received(store, ItemKind::Group, &parent.0.to_string())?;
                    if target.is_some_and(|t| {
                        t.role != Some(ShareRole::Write) || t.kind != ShareKind::GroupAll
                    }) {
                        return Err(read_only());
                    }
                }
                if existing.is_some() && received(store, ItemKind::Group, &id)?.is_some() {
                    return Err(CommandError::code(ErrorCode::ShareNotOwner));
                }
                // A group that is itself an own share stays where it is until the share ends.
                if let Some(server) = store.location_of(ItemKind::Group, &id)?
                    && let Some(area) = store.item_area(server, ItemKind::Group, &id)?
                    && store.share(server, area)?.is_some_and(|s| s.owned)
                {
                    return Err(CommandError::code(ErrorCode::ShareEndFirst));
                }
            }
            let Some(share) = received(store, ItemKind::Group, &id)? else {
                return Ok(true);
            };
            if share.role == Some(ShareRole::Write) {
                return Ok(true);
            }
            let old = store.list_groups()?.into_iter().find(|g| g.id == group.id);
            match old {
                Some(mut old) => {
                    old.defaults.key = group.defaults.key.clone();
                    old.location = group.location;
                    if old == *group {
                        Ok(false)
                    } else {
                        Err(read_only())
                    }
                }
                None => Err(read_only()),
            }
        }
        Change::DeleteHost(id) => match received(store, ItemKind::Host, &id.0.to_string())? {
            None => Ok(true),
            Some(share)
                if share.role == Some(ShareRole::Write) && share.kind == ShareKind::GroupAll =>
            {
                Ok(true)
            }
            Some(_) => Err(read_only()),
        },
        Change::DeleteGroup(id) => {
            let group = id.0.to_string();
            if received(store, ItemKind::Group, &group)?.is_some() {
                return Err(CommandError::code(ErrorCode::ShareNotOwner));
            }
            // A group of an own share goes with its share: end that first.
            if let Some(server) = store.location_of(ItemKind::Group, &group)?
                && let Some(area) = store.item_area(server, ItemKind::Group, &group)?
                && store.share(server, area)?.is_some_and(|s| s.owned)
            {
                return Err(CommandError::code(ErrorCode::ShareEndFirst));
            }
            Ok(true)
        }
        Change::Move(kind, id) => match received(store, kind, id)? {
            None => Ok(true),
            Some(_) => Err(CommandError::code(ErrorCode::ShareNotOwner)),
        },
    }
}

/// A key that came in someone else's share is theirs: it is neither deleted nor moved here.
pub fn share_rights_for_key(
    store: &Store,
    server: ServerId,
    key: sshcraft_core::model::KeyId,
) -> Result<(), CommandError> {
    match store.received_share_of(server, ItemKind::Key, &key.0.to_string())? {
        Some(_) => Err(CommandError::code(ErrorCode::ShareNotOwner)),
        None => Ok(()),
    }
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
        SyncError::ContactUnconfirmed => ErrorCode::ShareContactUnconfirmed,
        SyncError::AlreadyShared => ErrorCode::ShareAlreadyShared,
        SyncError::GroupCycle => ErrorCode::GroupCycle,
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

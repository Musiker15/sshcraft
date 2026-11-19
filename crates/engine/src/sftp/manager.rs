//! Open SFTP sessions, by id: one per SFTP tab. A session rides on the host's shared connection
//! when "reuse connections" is on, else on one of its own (the same host key check, sign-in and
//! jump hosts as a terminal).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use sshcraft_model::model::HostId;
use sshcraft_ssh::sftp::{EntryKind, RemoteAttrs, SftpClient, SftpError};

use super::names::{self, NameError};
use crate::error::ErrorCode;
use crate::sessions::{Context, OpenedKeys, connect_session, failure_code};

/// The most entries one listing gives.
pub const MAX_LIST: usize = 100_000;
/// How deep a recursive walk (delete, count, transfer) goes.
pub const MAX_DEPTH: usize = 64;
/// The most files and folders one recursive operation touches.
pub const MAX_ITEMS: usize = 1_000_000;

pub type SftpId = u64;
pub type SftpFailure = (ErrorCode, Option<String>);

pub(crate) fn sftp_failure(e: SftpError) -> SftpFailure {
    let code = match &e {
        SftpError::NotFound => ErrorCode::NotFound,
        SftpError::PermissionDenied => ErrorCode::PermissionDenied,
        SftpError::Unsupported => ErrorCode::NotSupported,
        SftpError::Timeout => ErrorCode::Timeout,
        SftpError::Lost(_) => ErrorCode::Network,
        SftpError::Failure(_) | SftpError::Protocol(_) => ErrorCode::SftpFailed,
    };
    (code, Some(e.to_string()))
}

pub(crate) fn name_failure(e: NameError) -> SftpFailure {
    (ErrorCode::NameInvalid, Some(e.to_string()))
}

fn io_failure(e: &std::io::Error) -> SftpFailure {
    let code = match e.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::NotFound,
        std::io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
        std::io::ErrorKind::AlreadyExists => ErrorCode::AlreadyExists,
        _ => ErrorCode::Io,
    };
    (code, Some(e.to_string()))
}

/// One entry of a listing, remote or local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryView {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: Option<u64>,
    /// Permission bits, when known.
    pub permissions: Option<u32>,
}

impl EntryView {
    fn remote(name: String, attrs: &RemoteAttrs) -> Self {
        EntryView { name, kind: attrs.kind, size: attrs.size, modified: attrs.modified, permissions: attrs.permissions }
    }
}

/// A folder's entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListingView {
    /// The folder, canonical.
    pub path: String,
    pub entries: Vec<EntryView>,
    /// There were more than [`MAX_LIST`]; the rest is not shown.
    pub truncated: bool,
}

/// How many files and folders a recursive operation would touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Count {
    pub files: usize,
    pub dirs: usize,
    pub bytes: u64,
}

pub(crate) struct Open {
    pub(crate) host: HostId,
    /// Keeps the connection alive while the session is open.
    pub(crate) connection: Arc<sshcraft_ssh::Session>,
    pub(crate) client: SftpClient,
}

/// The open SFTP sessions.
pub struct SftpManager {
    ctx: Arc<Context>,
    open: Mutex<HashMap<SftpId, Arc<Open>>>,
    next: AtomicU64,
    transfers: Arc<super::transfers::Transfers>,
}

impl SftpManager {
    pub(crate) fn new(ctx: Arc<Context>, events: crate::events::EventBus) -> Self {
        SftpManager { ctx, open: Mutex::new(HashMap::new()), next: AtomicU64::new(1), transfers: Arc::new(super::transfers::Transfers::new(events)) }
    }

    /// The uploads and downloads of every session.
    pub fn transfers(&self) -> &super::transfers::Transfers {
        &self.transfers
    }

    /// Starts uploading local files and folders into a remote folder.
    pub fn upload(
        &self,
        id: SftpId,
        local: Vec<PathBuf>,
        remote_dir: String,
        conflict: super::transfers::Conflict,
        name: Option<String>,
    ) -> Result<super::transfers::TransferId, SftpFailure> {
        let open = self.get(id)?;
        self.transfers.upload(id, open, local, remote_dir, conflict, name)
    }

    /// Starts downloading one remote file into an existing local file (a document the system's
    /// picker created).
    pub fn download_file(&self, id: SftpId, remote: String, local_file: PathBuf) -> Result<super::transfers::TransferId, SftpFailure> {
        let open = self.get(id)?;
        self.transfers.download_file(id, open, remote, local_file)
    }

    /// Starts downloading remote files and folders into a local folder.
    pub fn download(
        &self,
        id: SftpId,
        remote: Vec<String>,
        local_dir: PathBuf,
        conflict: super::transfers::Conflict,
    ) -> Result<super::transfers::TransferId, SftpFailure> {
        let open = self.get(id)?;
        self.transfers.download(id, open, remote, local_dir, conflict)
    }

    /// Starts copying files and folders of session `from` into a folder of session `to`.
    pub fn copy(
        &self,
        from: SftpId,
        remote: Vec<String>,
        to: SftpId,
        remote_dir: String,
        conflict: super::transfers::Conflict,
    ) -> Result<super::transfers::TransferId, SftpFailure> {
        let source = self.get(from)?;
        let open = self.get(to)?;
        self.transfers.copy((from, source), (to, open), remote, remote_dir, conflict)
    }

    /// Connects to `host` and starts SFTP; gives the id and the home folder.
    pub async fn open(&self, host: HostId) -> Result<(SftpId, String), SftpFailure> {
        let (open, home) = self.connect(host).await?;
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        self.lock().insert(id, Arc::new(open));
        Ok((id, home))
    }

    /// Connects session `id` again, after its connection went away; its id, its transfers'
    /// parts and what the pane shows stay. Gives the home folder.
    pub async fn reconnect(&self, id: SftpId) -> Result<String, SftpFailure> {
        let host = self.get(id)?.host;
        // A closed connection is not taken from the pool again, so this one is fresh.
        let (open, home) = self.connect(host).await?;
        let old = self.lock().insert(id, Arc::new(open));
        if let Some(old) = old {
            old.client.close();
            if let Ok(old) = Arc::try_unwrap(old) {
                crate::pool::release(old.connection).await;
            }
        }
        Ok(home)
    }

    /// Starts transfer `transfer` again on its session's current connection.
    pub fn retry(&self, transfer: super::transfers::TransferId) -> Result<super::transfers::TransferId, SftpFailure> {
        let (id, source) = self.transfers.sftp_of(transfer).ok_or((ErrorCode::NotFound, Some(format!("transfer {transfer}"))))?;
        let open = self.get(id)?;
        let source = source.map(|s| self.get(s)).transpose()?;
        self.transfers.retry(transfer, open, source)
    }

    async fn connect(&self, host: HostId) -> Result<(Open, String), SftpFailure> {
        let (connection, _) = connect_session(&self.ctx, host, &OpenedKeys::default(), true).await.map_err(failure_code)?;
        let client = match connection.sftp().await {
            Ok(client) => client,
            Err(e) => {
                crate::pool::release(connection).await;
                return Err((ErrorCode::SftpUnavailable, Some(e.to_string())));
            }
        };
        let home = match client.canonicalize(".").await.map_err(sftp_failure).and_then(|h| names::normalize_remote(&h).map_err(name_failure)) {
            Ok(home) => home,
            Err(e) => {
                client.close();
                crate::pool::release(connection).await;
                return Err(e);
            }
        };
        Ok((Open { host, connection, client }, home))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SftpId, Arc<Open>>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn get(&self, id: SftpId) -> Result<Arc<Open>, SftpFailure> {
        self.lock().get(&id).cloned().ok_or((ErrorCode::NotFound, Some(format!("SFTP session {id}"))))
    }

    /// The host of an open session.
    pub fn host(&self, id: SftpId) -> Option<HostId> {
        self.lock().get(&id).map(|o| o.host)
    }

    /// Ends a session; the connection closes when nothing else uses it.
    pub async fn close(&self, id: SftpId) -> bool {
        let Some(open) = self.lock().remove(&id) else {
            return false;
        };
        self.transfers.cancel_all(id);
        open.client.close();
        if let Ok(open) = Arc::try_unwrap(open) {
            crate::pool::release(open.connection).await;
        }
        true
    }

    /// The entries of the remote folder `path`.
    pub async fn list(&self, id: SftpId, path: &str) -> Result<ListingView, SftpFailure> {
        let path = names::normalize_remote(path).map_err(name_failure)?;
        let open = self.get(id)?;
        let listing = open.client.list(&path, MAX_LIST).await.map_err(sftp_failure)?;
        let entries = listing.entries.into_iter().map(|e| EntryView::remote(e.name, &e.attrs)).collect();
        Ok(ListingView { path, entries, truncated: listing.truncated })
    }

    /// Creates the folder `name` in `dir`.
    pub async fn mkdir(&self, id: SftpId, dir: &str, name: &str) -> Result<String, SftpFailure> {
        let path = names::remote_join(dir, name).map_err(name_failure)?;
        self.get(id)?.client.mkdir(&path).await.map_err(sftp_failure)?;
        Ok(path)
    }

    /// Renames `path` to `name` in the same folder; refuses when that name is taken.
    pub async fn rename(&self, id: SftpId, path: &str, name: &str) -> Result<String, SftpFailure> {
        let from = names::normalize_remote(path).map_err(name_failure)?;
        let dir = names::remote_parent(&from).ok_or((ErrorCode::NameInvalid, Some("/ has no name".into())))?;
        let to = names::remote_join(&dir, name).map_err(name_failure)?;
        let open = self.get(id)?;
        if from != to && open.client.lstat(&to).await.is_ok() {
            return Err((ErrorCode::AlreadyExists, Some(to)));
        }
        open.client.rename(&from, &to).await.map_err(sftp_failure)?;
        Ok(to)
    }

    /// Sets the permission bits of `path`.
    pub async fn chmod(&self, id: SftpId, path: &str, mode: u32) -> Result<(), SftpFailure> {
        let path = names::normalize_remote(path).map_err(name_failure)?;
        if mode > 0o7777 {
            return Err((ErrorCode::NameInvalid, Some(format!("mode {mode:o}"))));
        }
        self.get(id)?.client.chmod(&path, mode).await.map_err(sftp_failure)
    }

    /// How many files and folders `paths` hold, counted without following links to folders.
    pub async fn count(&self, id: SftpId, paths: &[String]) -> Result<Count, SftpFailure> {
        let open = self.get(id)?;
        let mut count = Count::default();
        for path in paths {
            let path = names::normalize_remote(path).map_err(name_failure)?;
            count_tree(&open.client, path, 0, &mut count).await?;
        }
        Ok(count)
    }

    /// Deletes `paths`, folders with everything in them; a link is removed, never followed.
    /// Returns how many files and folders went.
    pub async fn delete(&self, id: SftpId, paths: &[String]) -> Result<usize, SftpFailure> {
        let open = self.get(id)?;
        let mut deleted = 0usize;
        for path in paths {
            let path = names::normalize_remote(path).map_err(name_failure)?;
            if path == "/" {
                return Err((ErrorCode::NameInvalid, Some("/ is not deleted".into())));
            }
            remove_tree(&open.client, path, 0, &mut deleted).await?;
        }
        Ok(deleted)
    }
}

type Walk<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, SftpFailure>> + Send + 'a>>;

fn too_many(what: &str) -> SftpFailure {
    (ErrorCode::TooManyFiles, Some(what.to_owned()))
}

/// The entries of a folder for a walk: every one, or an error when there are more than a
/// listing gives (half a walk would be worse than none).
pub(crate) async fn walk_list(client: &SftpClient, dir: &str) -> Result<Vec<sshcraft_ssh::sftp::RemoteEntry>, SftpFailure> {
    let listing = client.list(dir, MAX_LIST).await.map_err(sftp_failure)?;
    if listing.truncated {
        return Err(too_many(&format!("{dir} has more than {MAX_LIST} entries")));
    }
    Ok(listing.entries)
}

fn count_tree<'a>(client: &'a SftpClient, path: String, depth: usize, count: &'a mut Count) -> Walk<'a, ()> {
    Box::pin(async move {
        if depth > MAX_DEPTH {
            return Err(too_many(&format!("deeper than {MAX_DEPTH} folders")));
        }
        let attrs = client.lstat(&path).await.map_err(sftp_failure)?;
        if attrs.kind != EntryKind::Dir {
            count.files += 1;
            count.bytes = count.bytes.saturating_add(attrs.size);
        } else {
            count.dirs += 1;
            for entry in walk_list(client, &path).await? {
                let child = names::remote_join(&path, &entry.name).map_err(name_failure)?;
                count_tree(client, child, depth + 1, count).await?;
            }
        }
        if count.files.saturating_add(count.dirs) > MAX_ITEMS {
            return Err(too_many(&format!("more than {MAX_ITEMS} files and folders")));
        }
        Ok(())
    })
}

fn remove_tree<'a>(client: &'a SftpClient, path: String, depth: usize, deleted: &'a mut usize) -> Walk<'a, ()> {
    Box::pin(async move {
        if depth > MAX_DEPTH {
            return Err(too_many(&format!("deeper than {MAX_DEPTH} folders")));
        }
        if *deleted >= MAX_ITEMS {
            return Err(too_many(&format!("more than {MAX_ITEMS} files and folders")));
        }
        let attrs = client.lstat(&path).await.map_err(sftp_failure)?;
        if attrs.kind == EntryKind::Dir {
            for entry in walk_list(client, &path).await? {
                let child = names::remote_join(&path, &entry.name).map_err(name_failure)?;
                remove_tree(client, child, depth + 1, deleted).await?;
            }
            client.rmdir(&path).await.map_err(sftp_failure)?;
        } else {
            client.remove(&path).await.map_err(sftp_failure)?;
        }
        *deleted += 1;
        Ok(())
    })
}

/// The entries of the local folder `path`.
pub async fn local_list(path: PathBuf) -> Result<ListingView, SftpFailure> {
    tokio::task::spawn_blocking(move || local_list_blocking(&path)).await.map_err(|e| (ErrorCode::Internal, Some(e.to_string())))?
}

fn local_list_blocking(path: &Path) -> Result<ListingView, SftpFailure> {
    if !path.is_absolute() {
        return Err(name_failure(NameError::NotAbsolute));
    }
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in std::fs::read_dir(path).map_err(|e| io_failure(&e))? {
        if entries.len() >= MAX_LIST {
            truncated = true;
            break;
        }
        let Ok(entry) = entry else { continue };
        let Ok(meta) = entry.path().symlink_metadata() else { continue };
        entries.push(local_entry(entry.file_name().to_string_lossy().into_owned(), &meta));
    }
    Ok(ListingView { path: path.to_string_lossy().into_owned(), entries, truncated })
}

pub(crate) fn local_entry(name: String, meta: &std::fs::Metadata) -> EntryView {
    let kind = if meta.file_type().is_symlink() {
        EntryKind::Symlink
    } else if meta.is_dir() {
        EntryKind::Dir
    } else if meta.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
    #[cfg(unix)]
    let permissions = Some(std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o7777);
    #[cfg(not(unix))]
    let permissions = None;
    EntryView { name, kind, size: if kind == EntryKind::File { meta.len() } else { 0 }, modified, permissions }
}

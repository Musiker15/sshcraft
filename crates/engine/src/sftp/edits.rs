//! Editing a remote file in the user's text editor. Files open only there, never in the program the
//! system links to a type, since opening a downloaded file can run it. The copy lives in the data
//! folder; a save goes back under the part name and is renamed over the file, unless the file
//! changed on the server since it was opened. Stopping, closing the SFTP session or quitting
//! deletes the copy, and what a crash left goes at the next start.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use serde::Serialize;
use sshcraft_ssh::sftp::{CHUNK, EntryKind, SftpClient, SftpError};

use super::manager::{SftpFailure, SftpId, SftpManager, name_failure, sftp_failure};
use super::names;
use crate::error::ErrorCode;

/// The largest file that opens in the editor, and the largest that goes back.
pub const MAX_EDIT_BYTES: u64 = 16 << 20;
/// Files edited at once.
pub const MAX_EDITS: usize = 64;

pub type EditId = u64;

/// Starts the editor `.0` on the file `.1`.
pub type Launcher = Arc<dyn Fn(&Path, &Path) -> std::io::Result<()> + Send + Sync>;

/// A file open in the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditView {
    pub edit: EditId,
    pub sftp: SftpId,
    pub remote: String,
    pub name: String,
    pub local: PathBuf,
}

/// What a look at the open files did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    /// Saved here and sent to the server.
    pub uploaded: Vec<EditId>,
    /// Saved here, but the file changed on the server since: nothing was sent.
    pub conflicts: Vec<EditId>,
    /// Saved here, but sending failed.
    pub failed: Vec<EditFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditFailure {
    pub edit: EditId,
    pub code: ErrorCode,
    pub message: Option<String>,
}

/// The size and time of a file, what a change is told by.
type Stamp = (u64, Option<u64>);

struct Edit {
    view: EditView,
    /// The file on the server when it was opened or last sent.
    remote_seen: Stamp,
    /// The copy here after it was written or last sent.
    local_seen: (u64, Option<SystemTime>),
    permissions: Option<u32>,
}

pub(crate) struct Edits {
    root: PathBuf,
    next: AtomicU64,
    list: Mutex<Vec<Edit>>,
    launcher: Mutex<Launcher>,
}

/// Starts `editor` with `file` as its only argument, never through a shell, and reaps it later.
fn spawn_editor(editor: &Path, file: &Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(editor).arg(file).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn local_stamp(path: &Path) -> std::io::Result<(u64, Option<SystemTime>)> {
    let meta = std::fs::metadata(path)?;
    Ok((meta.len(), meta.modified().ok()))
}

fn too_large() -> SftpFailure {
    (ErrorCode::FileTooLarge, Some(format!("larger than {} MiB", MAX_EDIT_BYTES >> 20)))
}

fn io_failure(e: &std::io::Error) -> SftpFailure {
    (ErrorCode::Io, Some(e.to_string()))
}

impl Edits {
    /// The edits under `root`, which starts empty: copies a crash left go.
    pub(crate) fn new(root: PathBuf) -> Self {
        let _ = std::fs::remove_dir_all(&root);
        Edits { root, next: AtomicU64::new(1), list: Mutex::new(Vec::new()), launcher: Mutex::new(Arc::new(spawn_editor)) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Edit>> {
        self.list.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn launch(&self, editor: &Path, file: &Path) -> std::io::Result<()> {
        let launcher = self.launcher.lock().unwrap_or_else(PoisonError::into_inner).clone();
        launcher(editor, file)
    }

    fn remove_copy(&self, view: &EditView) {
        if let Some(dir) = view.local.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// Stops every edit of `sftp` (its session closed).
    pub(crate) fn stop_all(&self, sftp: SftpId) {
        let gone: Vec<EditView> = {
            let mut list = self.lock();
            let (gone, kept): (Vec<Edit>, Vec<Edit>) = std::mem::take(&mut *list).into_iter().partition(|e| e.view.sftp == sftp);
            *list = kept;
            gone.into_iter().map(|e| e.view).collect()
        };
        for view in &gone {
            self.remove_copy(view);
        }
    }
}

/// The whole of the remote file `path`, at most [`MAX_EDIT_BYTES`].
async fn read_all(client: &SftpClient, path: &str) -> Result<Vec<u8>, SftpFailure> {
    let file = client.open_read(path).await.map_err(sftp_failure)?;
    let mut data = Vec::new();
    let result = loop {
        let offset = u64::try_from(data.len()).unwrap_or(u64::MAX);
        match file.read_at(offset, CHUNK).await {
            Ok(chunk) if chunk.is_empty() => break Ok(()),
            Ok(chunk) => {
                if offset.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX)) > MAX_EDIT_BYTES {
                    break Err(too_large());
                }
                data.extend_from_slice(&chunk);
            }
            Err(e) => break Err(sftp_failure(e)),
        }
    };
    let _ = file.close().await;
    result.map(|()| data)
}

/// Writes `data` to the part of `target`, keeps `permissions` and renames it over `target`. The
/// part goes when anything fails.
async fn write_over(client: &SftpClient, target: &str, data: &[u8], permissions: Option<u32>) -> Result<(), SftpFailure> {
    let dir = names::remote_parent(target).unwrap_or_else(|| "/".into());
    let name = names::remote_name(target).ok_or((ErrorCode::NameInvalid, None))?;
    let part = names::remote_join(&dir, &names::part_name(&name)).map_err(name_failure)?;
    // A part an earlier try left is ours and goes.
    if client.lstat(&part).await.is_ok() {
        let _ = client.remove(&part).await;
    }
    let file = client.create_new(&part).await.map_err(sftp_failure)?;
    let written = async {
        let mut offset = 0u64;
        for chunk in data.chunks(CHUNK as usize) {
            file.write_at(offset, chunk.to_vec()).await?;
            offset = offset.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        }
        Ok::<(), SftpError>(())
    }
    .await;
    let closed = file.close().await;
    let result = async {
        written.map_err(sftp_failure)?;
        closed.map_err(sftp_failure)?;
        if let Some(mode) = permissions {
            client.chmod(&part, mode).await.map_err(sftp_failure)?;
        }
        match client.remove(target).await {
            Ok(()) | Err(SftpError::NotFound) => {}
            Err(e) => return Err(sftp_failure(e)),
        }
        client.rename(&part, target).await.map_err(sftp_failure)
    }
    .await;
    if result.is_err() {
        let _ = client.remove(&part).await;
    }
    result
}

impl SftpManager {
    /// Starts the editor that runs files for the tests instead of a program.
    #[cfg(any(test, feature = "testing"))]
    pub fn set_editor_launcher(&self, launcher: Launcher) {
        *self.edits().launcher.lock().unwrap_or_else(PoisonError::into_inner) = launcher;
    }

    /// Copies the remote file `remote` of session `id` here and opens it in `editor`. A file
    /// already open opens in the editor again.
    pub async fn edit(&self, id: SftpId, remote: &str, editor: &Path) -> Result<EditView, SftpFailure> {
        if !editor.is_absolute() || !editor.is_file() {
            return Err((ErrorCode::EditorMissing, Some(editor.display().to_string())));
        }
        let remote = names::normalize_remote(remote).map_err(name_failure)?;
        let name = names::remote_name(&remote).ok_or((ErrorCode::NameInvalid, None))?;
        names::check_local_name_here(&name).map_err(name_failure)?;
        let open = self.get(id)?;
        let edits = self.edits();
        let already = edits.lock().iter().find(|e| e.view.sftp == id && e.view.remote == remote).map(|e| e.view.clone());
        if let Some(view) = already {
            edits.launch(editor, &view.local).map_err(|e| io_failure(&e))?;
            return Ok(view);
        }
        if edits.lock().len() >= MAX_EDITS {
            return Err((ErrorCode::TooManyFiles, Some(format!("at most {MAX_EDITS} files are edited at once"))));
        }
        let attrs = open.client.stat(&remote).await.map_err(sftp_failure)?;
        if attrs.kind != EntryKind::File {
            return Err((ErrorCode::NotSupported, Some("only a plain file opens in the editor".into())));
        }
        if attrs.size > MAX_EDIT_BYTES {
            return Err(too_large());
        }
        let data = read_all(&open.client, &remote).await?;
        let edit = edits.next.fetch_add(1, Ordering::Relaxed);
        let dir = edits.root.join(edit.to_string());
        let local = dir.join(&name);
        let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&local, &data)).and_then(|()| local_stamp(&local));
        let local_seen = match written {
            Ok(stamp) => stamp,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(io_failure(&e));
            }
        };
        if let Err(e) = edits.launch(editor, &local) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err((ErrorCode::EditorMissing, Some(e.to_string())));
        }
        let view = EditView { edit, sftp: id, remote, name, local };
        let size = u64::try_from(data.len()).unwrap_or(u64::MAX);
        edits.lock().push(Edit { view: view.clone(), remote_seen: (size, attrs.modified), local_seen, permissions: attrs.permissions });
        Ok(view)
    }

    /// The files open in the editor.
    pub fn edits_open(&self) -> Vec<EditView> {
        self.edits().lock().iter().map(|e| e.view.clone()).collect()
    }

    /// Stops editing: the copy here goes. Returns whether it was open.
    pub fn stop_edit(&self, edit: EditId) -> bool {
        let edits = self.edits();
        let gone = {
            let mut list = edits.lock();
            list.iter().position(|e| e.view.edit == edit).map(|i| list.remove(i))
        };
        match gone {
            Some(e) => {
                edits.remove_copy(&e.view);
                true
            }
            None => false,
        }
    }

    /// Sends the copy of `edit` to the server. Without `force`, a file that changed on the server
    /// since it was opened or last sent is left alone ([`ErrorCode::ChangedElsewhere`]).
    pub async fn save_edit(&self, edit: EditId, force: bool) -> Result<(), SftpFailure> {
        let edits = self.edits();
        let (view, remote_seen, permissions) = {
            let list = edits.lock();
            let e = list.iter().find(|e| e.view.edit == edit).ok_or((ErrorCode::NotFound, Some(format!("edit {edit}"))))?;
            (e.view.clone(), e.remote_seen, e.permissions)
        };
        let local_seen = local_stamp(&view.local).map_err(|e| io_failure(&e))?;
        if local_seen.0 > MAX_EDIT_BYTES {
            return Err(too_large());
        }
        let data = std::fs::read(&view.local).map_err(|e| io_failure(&e))?;
        let open = self.get(view.sftp)?;
        let now = match open.client.stat(&view.remote).await {
            Ok(attrs) => Some((attrs.size, attrs.modified)),
            Err(SftpError::NotFound) => None,
            Err(e) => return Err(sftp_failure(e)),
        };
        // Whatever the answer, this save of the copy was looked at.
        let mark_local = |seen| {
            if let Some(e) = edits.lock().iter_mut().find(|e| e.view.edit == edit) {
                e.local_seen = seen;
            }
        };
        if !force && now != Some(remote_seen) {
            mark_local(local_seen);
            return Err((ErrorCode::ChangedElsewhere, Some(view.remote.clone())));
        }
        write_over(&open.client, &view.remote, &data, permissions).await?;
        let sent =
            open.client.stat(&view.remote).await.map(|a| (a.size, a.modified)).unwrap_or((u64::try_from(data.len()).unwrap_or(u64::MAX), None));
        if let Some(e) = edits.lock().iter_mut().find(|e| e.view.edit == edit) {
            e.remote_seen = sent;
            e.local_seen = local_seen;
        }
        Ok(())
    }

    /// Looks at every copy and sends what was saved since.
    pub async fn sync_edits(&self) -> SyncReport {
        // Each save is looked at once: a failure is reported, not tried again every second.
        let changed: Vec<EditId> = self
            .edits()
            .lock()
            .iter_mut()
            .filter_map(|e| {
                let now = local_stamp(&e.view.local).ok().filter(|now| *now != e.local_seen)?;
                e.local_seen = now;
                Some(e.view.edit)
            })
            .collect();
        let mut report = SyncReport::default();
        for edit in changed {
            match self.save_edit(edit, false).await {
                Ok(()) => report.uploaded.push(edit),
                Err((ErrorCode::ChangedElsewhere, _)) => report.conflicts.push(edit),
                Err((code, message)) => report.failed.push(EditFailure { edit, code, message }),
            }
        }
        report
    }
}

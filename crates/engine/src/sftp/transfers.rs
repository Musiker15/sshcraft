//! Uploads and downloads: one job per request, files one after another, each file read or
//! written in [`WINDOW`] requests at once. A file is written under its part name and renamed when
//! complete; a cancelled or failed file's part is deleted, so nothing half-written stays behind.
//! Only when the connection was lost does the part stay: [`Transfers::retry`] goes on from it.
//! A part is continued only if this run of SSHCraft left it, so a stale or foreign one never is.
//! Progress goes out as [`crate::EngineEvent::TransferStatus`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sshcraft_ssh::sftp::{CHUNK, EntryKind, RemoteFile, SftpClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::manager::{MAX_DEPTH, MAX_ITEMS, Open, SftpFailure, SftpId, name_failure, sftp_failure, walk_list};
use super::names;
use crate::error::ErrorCode;
use crate::events::{EngineEvent, EventBus};

/// Requests in flight per file.
const WINDOW: u64 = 16;
/// The most errors a job keeps to show.
const MAX_ERRORS: usize = 20;
/// The most jobs kept after they ended.
const MAX_ENDED: usize = 50;
/// How often a running job reports.
const REPORT_EVERY: Duration = Duration::from_millis(200);
/// How many jobs run at once; the rest wait their turn.
const RUNNING: usize = 2;

pub type TransferId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Upload,
    Download,
}

/// What to do when the target name is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    /// Leave the file there and go on (the default: nothing is lost).
    #[default]
    Skip,
    Overwrite,
    /// Keep both: the new one gets a free name, `name (2).ext`.
    Rename,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TransferState {
    Queued,
    Running,
    Done,
    Cancelled,
    Failed { code: ErrorCode, detail: Option<String> },
}

/// A job as the interface shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransferView {
    pub id: TransferId,
    pub sftp: SftpId,
    pub direction: Direction,
    /// The first item's name, for the list.
    pub label: String,
    pub state: TransferState,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// Files left alone because their name was taken.
    pub skipped: usize,
    /// What went wrong with single files (the job went on), at most [`MAX_ERRORS`].
    pub errors: Vec<String>,
    /// Bytes taken over from parts an earlier try left, counted in `bytes_done`.
    pub resumed: u64,
}

/// What a job was asked to do, kept so it can be tried again.
#[derive(Debug, Clone)]
enum Request {
    Upload { local: Vec<PathBuf>, remote_dir: String, name: Option<String> },
    Download { remote: Vec<String>, local_dir: PathBuf },
    DownloadFile { remote: String, local_file: PathBuf },
}

struct Job {
    view: Mutex<TransferView>,
    cancel: AtomicBool,
    bytes: AtomicU64,
    request: Request,
    conflict: Conflict,
    /// The sources (local paths of an upload, remote paths of a download) that are done, so a
    /// retry leaves them alone instead of sending them again (or beside, as `name (2)`).
    completed: Mutex<HashSet<String>>,
}

impl Job {
    fn view(&self) -> TransferView {
        let mut view = self.view.lock().unwrap_or_else(PoisonError::into_inner).clone();
        view.bytes_done = self.bytes.load(Ordering::Relaxed);
        view
    }

    fn update(&self, f: impl FnOnce(&mut TransferView)) {
        f(&mut self.view.lock().unwrap_or_else(PoisonError::into_inner));
    }

    fn error(&self, what: String) {
        self.update(|v| {
            if v.errors.len() < MAX_ERRORS {
                v.errors.push(what);
            }
        });
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    fn is_completed(&self, source: &str) -> bool {
        self.completed.lock().unwrap_or_else(PoisonError::into_inner).contains(source)
    }

    fn complete(&self, source: String) {
        self.completed.lock().unwrap_or_else(PoisonError::into_inner).insert(source);
    }

    fn resumed(&self, bytes: u64) {
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        self.update(|v| v.resumed = v.resumed.saturating_add(bytes));
    }
}

/// Whether a file that stopped this way keeps its part: only when the connection went away
/// (the user did not ask to stop, and the part can be continued). A cancel or any other failure
/// deletes it.
fn keeps_part(stop: &Stop) -> bool {
    matches!(stop, Stop::Failed((ErrorCode::Network | ErrorCode::Timeout, _)))
}

/// The jobs of all SFTP sessions.
pub struct Transfers {
    jobs: Mutex<Vec<Arc<Job>>>,
    next: AtomicU64,
    events: EventBus,
    running: Arc<tokio::sync::Semaphore>,
    /// Parts a lost connection left, which a retry may continue: `r:<remote path>` for an
    /// upload's, `l:<local path>` for a download's.
    resumable: Mutex<HashSet<String>>,
}

/// Why a job or a file stopped.
enum Stop {
    Cancelled,
    Failed(SftpFailure),
}

impl From<SftpFailure> for Stop {
    fn from(f: SftpFailure) -> Self {
        Stop::Failed(f)
    }
}

impl Transfers {
    pub(crate) fn new(events: EventBus) -> Self {
        Transfers {
            jobs: Mutex::new(Vec::new()),
            next: AtomicU64::new(1),
            events,
            running: Arc::new(tokio::sync::Semaphore::new(RUNNING)),
            resumable: Mutex::new(HashSet::new()),
        }
    }

    /// Marks a part as left by a lost connection, so a retry may go on from it.
    pub(crate) fn mark_resumable(&self, key: String) {
        self.resumable.lock().unwrap_or_else(PoisonError::into_inner).insert(key);
    }

    /// Whether the part `key` may be continued; it may only once.
    fn take_resumable(&self, key: &str) -> bool {
        self.resumable.lock().unwrap_or_else(PoisonError::into_inner).remove(key)
    }

    /// The session a job belongs to.
    pub fn sftp_of(&self, id: TransferId) -> Option<SftpId> {
        self.lock().iter().map(|j| j.view()).find(|v| v.id == id).map(|v| v.sftp)
    }

    /// Starts job `id` again: one that failed, was cancelled, or ended with files that failed.
    /// Files it finished stay as they are; a part a lost connection left is continued.
    pub(crate) fn retry(self: &Arc<Self>, id: TransferId, open: Arc<Open>) -> Result<TransferId, SftpFailure> {
        let job = self.lock().iter().find(|j| j.view().id == id).cloned().ok_or((ErrorCode::NotFound, Some(format!("transfer {id}"))))?;
        let view = job.view();
        let again = match &view.state {
            TransferState::Failed { .. } | TransferState::Cancelled => true,
            TransferState::Done => !view.errors.is_empty(),
            TransferState::Queued | TransferState::Running => false,
        };
        if !again {
            return Err((ErrorCode::NotSupported, Some("only a job that did not finish is tried again".into())));
        }
        let completed = job.completed.lock().unwrap_or_else(PoisonError::into_inner).clone();
        Ok(self.start(view.sftp, open, job.request.clone(), job.conflict, completed))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Arc<Job>>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every job, oldest first.
    pub fn list(&self) -> Vec<TransferView> {
        self.lock().iter().map(|j| j.view()).collect()
    }

    /// Asks a job to stop; false when there is no such job or it already ended.
    pub fn cancel(&self, id: TransferId) -> bool {
        let jobs = self.lock();
        let Some(job) = jobs.iter().find(|j| j.view().id == id) else {
            return false;
        };
        let open = matches!(job.view().state, TransferState::Queued | TransferState::Running);
        if open {
            job.cancel.store(true, Ordering::SeqCst);
        }
        open
    }

    /// Stops every job of a session (it is closing).
    pub(crate) fn cancel_all(&self, sftp: SftpId) {
        for job in self.lock().iter().filter(|j| j.view().sftp == sftp) {
            job.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Removes the jobs that ended.
    pub fn clear_ended(&self) {
        self.lock().retain(|j| matches!(j.view().state, TransferState::Queued | TransferState::Running));
    }

    fn add(&self, sftp: SftpId, request: Request, conflict: Conflict, completed: HashSet<String>) -> Arc<Job> {
        let (direction, label) = match &request {
            Request::Upload { local, .. } => {
                (Direction::Upload, label_of(local.first().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()), local.len()))
            }
            Request::Download { remote, .. } => (Direction::Download, label_of(remote.first().and_then(|p| names::remote_name(p)), remote.len())),
            Request::DownloadFile { remote, .. } => (Direction::Download, label_of(names::remote_name(remote), 1)),
        };
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let view = TransferView {
            id,
            sftp,
            direction,
            label,
            state: TransferState::Queued,
            files_done: 0,
            files_total: 0,
            bytes_done: 0,
            bytes_total: 0,
            skipped: 0,
            errors: Vec::new(),
            resumed: 0,
        };
        let job = Arc::new(Job {
            view: Mutex::new(view),
            cancel: AtomicBool::new(false),
            bytes: AtomicU64::new(0),
            request,
            conflict,
            completed: Mutex::new(completed),
        });
        let mut jobs = self.lock();
        // Old ended jobs make room.
        while jobs.len() >= MAX_ENDED {
            match jobs.iter().position(|j| !matches!(j.view().state, TransferState::Queued | TransferState::Running)) {
                Some(i) => {
                    jobs.remove(i);
                }
                None => break,
            }
        }
        jobs.push(job.clone());
        job
    }

    fn report(&self, job: &Job) {
        self.events.send(EngineEvent::TransferStatus(job.view()));
    }

    /// Starts uploading `local` (files and folders) into the remote folder `remote_dir`.
    pub(crate) fn upload(
        self: &Arc<Self>,
        sftp: SftpId,
        open: Arc<Open>,
        local: Vec<PathBuf>,
        remote_dir: String,
        conflict: Conflict,
        name: Option<String>,
    ) -> Result<TransferId, SftpFailure> {
        let remote_dir = names::normalize_remote(&remote_dir).map_err(name_failure)?;
        if local.iter().any(|p| !p.is_absolute()) {
            return Err(name_failure(names::NameError::NotAbsolute));
        }
        if let Some(name) = &name {
            // A name of its own goes with one file only (the picker's file has a number for a
            // name on Android), and it is checked like any name sent to the server.
            if local.len() != 1 {
                return Err((ErrorCode::NameInvalid, Some("a name goes with one file only".into())));
            }
            names::check_name(name).map_err(name_failure)?;
        }
        Ok(self.start(sftp, open, Request::Upload { local, remote_dir, name }, conflict, HashSet::new()))
    }

    /// Starts downloading the remote file `remote` into the existing local file `local_file`,
    /// which it overwrites: a document the system's picker created (Android). There is no part
    /// file, since such a file cannot be renamed; on failure it is cut back to nothing.
    pub(crate) fn download_file(
        self: &Arc<Self>,
        sftp: SftpId,
        open: Arc<Open>,
        remote: String,
        local_file: PathBuf,
    ) -> Result<TransferId, SftpFailure> {
        if !local_file.is_absolute() {
            return Err(name_failure(names::NameError::NotAbsolute));
        }
        let remote = names::normalize_remote(&remote).map_err(name_failure)?;
        Ok(self.start(sftp, open, Request::DownloadFile { remote, local_file }, Conflict::Overwrite, HashSet::new()))
    }

    /// Starts downloading `remote` (files and folders) into the local folder `local_dir`.
    pub(crate) fn download(
        self: &Arc<Self>,
        sftp: SftpId,
        open: Arc<Open>,
        remote: Vec<String>,
        local_dir: PathBuf,
        conflict: Conflict,
    ) -> Result<TransferId, SftpFailure> {
        if !local_dir.is_absolute() {
            return Err(name_failure(names::NameError::NotAbsolute));
        }
        if !local_dir.is_dir() {
            return Err((ErrorCode::NotFound, Some(local_dir.to_string_lossy().into_owned())));
        }
        let remote = remote.iter().map(|p| names::normalize_remote(p)).collect::<Result<Vec<_>, _>>().map_err(name_failure)?;
        Ok(self.start(sftp, open, Request::Download { remote, local_dir }, conflict, HashSet::new()))
    }

    fn start(self: &Arc<Self>, sftp: SftpId, open: Arc<Open>, request: Request, conflict: Conflict, completed: HashSet<String>) -> TransferId {
        let job = self.add(sftp, request.clone(), conflict, completed);
        let id = job.view().id;
        let this = self.clone();
        tokio::spawn(async move {
            let result = match request {
                Request::Upload { local, remote_dir, name } => {
                    this.run(&job, || upload(&job, &open.client, local, remote_dir, conflict, name, &this)).await
                }
                Request::Download { remote, local_dir } => this.run(&job, || download(&job, &open.client, remote, local_dir, conflict, &this)).await,
                Request::DownloadFile { remote, local_file } => {
                    this.run(&job, || download_to_file(&job, &open.client, remote, local_file, &this)).await
                }
            };
            this.finish(&job, result);
        });
        id
    }

    /// Waits for a free slot, then runs the job.
    async fn run<F, Fut>(&self, job: &Job, work: F) -> Result<(), Stop>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), Stop>>,
    {
        self.report(job);
        let _slot = self.running.clone().acquire_owned().await.map_err(|_| Stop::Cancelled)?;
        if job.cancelled() {
            return Err(Stop::Cancelled);
        }
        job.update(|v| v.state = TransferState::Running);
        self.report(job);
        work().await
    }

    fn finish(&self, job: &Job, result: Result<(), Stop>) {
        job.update(|v| {
            v.state = match result {
                Ok(()) => TransferState::Done,
                Err(Stop::Cancelled) => TransferState::Cancelled,
                Err(Stop::Failed((code, detail))) => TransferState::Failed { code, detail },
            }
        });
        self.report(job);
    }
}

fn label_of(first: Option<String>, count: usize) -> String {
    let first: String = first.unwrap_or_default().chars().filter(|c| !c.is_control()).take(120).collect();
    if count > 1 { format!("{first} (+{})", count - 1) } else { first }
}

/// Reports a running job at most every [`REPORT_EVERY`].
struct Reporter<'a> {
    transfers: &'a Transfers,
    job: &'a Job,
    last: Instant,
}

impl Reporter<'_> {
    fn tick(&mut self) {
        if self.last.elapsed() >= REPORT_EVERY {
            self.last = Instant::now();
            self.transfers.report(self.job);
        }
    }
}

// ---- Upload ---------------------------------------------------------------------------------

/// A planned upload: folders to create (remote paths, parents first) and files.
#[derive(Default)]
struct UploadPlan {
    dirs: Vec<String>,
    files: Vec<(PathBuf, String, u64)>,
}

fn plan_upload(errors: &mut Vec<String>, local: &[PathBuf], remote_dir: &str, name: Option<&str>) -> Result<UploadPlan, SftpFailure> {
    let mut plan = UploadPlan::default();
    for path in local {
        plan_local(errors, path, remote_dir, name, 0, &mut plan)?;
    }
    Ok(plan)
}

/// Plans `path` into `remote_dir`, under `as_name` when given (the top item only).
fn plan_local(
    errors: &mut Vec<String>,
    path: &Path,
    remote_dir: &str,
    as_name: Option<&str>,
    depth: usize,
    plan: &mut UploadPlan,
) -> Result<(), SftpFailure> {
    if depth > MAX_DEPTH {
        return Err((ErrorCode::TooManyFiles, Some(format!("deeper than {MAX_DEPTH} folders"))));
    }
    if plan.dirs.len().saturating_add(plan.files.len()) >= MAX_ITEMS {
        return Err((ErrorCode::TooManyFiles, Some(format!("more than {MAX_ITEMS} files and folders"))));
    }
    let shown = path.to_string_lossy().into_owned();
    let Some(name) = as_name.or_else(|| path.file_name().and_then(|n| n.to_str())) else {
        errors.push(format!("{shown}: the name is not UTF-8"));
        return Ok(());
    };
    let target = match names::remote_join(remote_dir, name) {
        Ok(t) => t,
        Err(e) => {
            errors.push(format!("{shown}: {e}"));
            return Ok(());
        }
    };
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        errors.push(format!("{shown}: cannot be read"));
        return Ok(());
    };
    let meta = if meta.file_type().is_symlink() {
        // A link to a file sends the file; a link to a folder is not followed (no loops).
        match std::fs::metadata(path) {
            Ok(m) if m.is_file() => m,
            _ => {
                errors.push(format!("{shown}: a link to a folder is not followed"));
                return Ok(());
            }
        }
    } else {
        meta
    };
    if meta.is_dir() {
        plan.dirs.push(target.clone());
        let mut children: Vec<PathBuf> =
            std::fs::read_dir(path).map_err(|e| (ErrorCode::Io, Some(format!("{shown}: {e}"))))?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        children.sort();
        for child in children {
            plan_local(errors, &child, &target, None, depth + 1, plan)?;
        }
    } else if meta.is_file() {
        plan.files.push((path.to_path_buf(), target, meta.len()));
    } else {
        errors.push(format!("{shown}: not a file or folder"));
    }
    Ok(())
}

async fn upload(
    job: &Job,
    client: &SftpClient,
    local: Vec<PathBuf>,
    remote_dir: String,
    conflict: Conflict,
    name: Option<String>,
    transfers: &Transfers,
) -> Result<(), Stop> {
    let plan = {
        let local = local.clone();
        let remote_dir = remote_dir.clone();
        let (planned, errors) = tokio::task::spawn_blocking(move || {
            let mut errors = Vec::new();
            let planned = plan_upload(&mut errors, &local, &remote_dir, name.as_deref());
            (planned, errors)
        })
        .await
        .map_err(|e| Stop::Failed((ErrorCode::Internal, Some(e.to_string()))))?;
        for e in errors {
            job.error(e);
        }
        planned?
    };
    // A retry leaves the files an earlier try finished.
    let mut plan = plan;
    plan.files.retain(|(local, _, _)| !job.is_completed(&local.to_string_lossy()));
    let total: u64 = plan.files.iter().map(|(_, _, size)| *size).fold(0, u64::saturating_add);
    job.update(|v| {
        v.files_total = plan.files.len();
        v.bytes_total = total;
    });
    let mut reporter = Reporter { transfers, job, last: Instant::now() };
    let mut failed_dirs: Vec<String> = Vec::new();
    for dir in &plan.dirs {
        if job.cancelled() {
            return Err(Stop::Cancelled);
        }
        match client.lstat(dir).await {
            Ok(a) if a.kind == EntryKind::Dir => {}
            Ok(_) => {
                job.error(format!("{dir}: a file of that name is there"));
                failed_dirs.push(dir.clone());
            }
            Err(_) => {
                if let Err(e) = client.mkdir(dir).await {
                    job.error(format!("{dir}: {e}"));
                    failed_dirs.push(dir.clone());
                }
            }
        }
    }
    let mut taken: HashMap<String, HashSet<String>> = HashMap::new();
    for (local, target, _) in &plan.files {
        if job.cancelled() {
            return Err(Stop::Cancelled);
        }
        if failed_dirs.iter().any(|d| target.starts_with(&format!("{d}/"))) {
            continue;
        }
        let source = local.to_string_lossy().into_owned();
        match upload_file(job, client, local, target, conflict, &mut taken, &mut reporter).await {
            Ok(true) => {
                job.update(|v| v.files_done += 1);
                job.complete(source);
            }
            Ok(false) => {
                job.update(|v| v.skipped += 1);
                job.complete(source);
            }
            Err(Stop::Cancelled) => return Err(Stop::Cancelled),
            Err(stop @ Stop::Failed(_)) if keeps_part(&stop) => return Err(stop),
            Err(Stop::Failed((_, detail))) => job.error(format!("{target}: {}", detail.unwrap_or_default())),
        }
        reporter.tick();
    }
    Ok(())
}

/// The names in a remote folder, read once per folder for "keep both".
async fn remote_names<'a>(
    client: &SftpClient,
    dir: &str,
    taken: &'a mut HashMap<String, HashSet<String>>,
) -> Result<&'a mut HashSet<String>, SftpFailure> {
    if !taken.contains_key(dir) {
        let names = walk_list(client, dir).await?.into_iter().map(|e| e.name).collect();
        taken.insert(dir.to_owned(), names);
    }
    taken.get_mut(dir).ok_or((ErrorCode::Internal, None))
}

/// Uploads one file; `Ok(false)` when it was skipped because the name is taken.
async fn upload_file(
    job: &Job,
    client: &SftpClient,
    local: &Path,
    target: &str,
    conflict: Conflict,
    taken: &mut HashMap<String, HashSet<String>>,
    reporter: &mut Reporter<'_>,
) -> Result<bool, Stop> {
    let dir = names::remote_parent(target).ok_or((ErrorCode::NameInvalid, None))?;
    let name = names::remote_name(target).ok_or((ErrorCode::NameInvalid, None))?;
    let exists = client.lstat(target).await.is_ok();
    let (target, replace) = match (exists, conflict) {
        (false, _) => (target.to_owned(), false),
        (true, Conflict::Skip) => return Ok(false),
        (true, Conflict::Overwrite) => (target.to_owned(), true),
        (true, Conflict::Rename) => {
            let names = remote_names(client, &dir, taken).await?;
            names.insert(name.clone());
            let free = names::free_name(&name, |n| names.contains(n)).ok_or((ErrorCode::AlreadyExists, Some(target.to_owned())))?;
            names.insert(free.clone());
            (names::remote_join(&dir, &free).map_err(name_failure)?, false)
        }
    };
    let part = names::remote_join(&dir, &names::part_name(&name)).map_err(name_failure)?;
    let key = format!("r:{part}");
    let local_size = tokio::fs::metadata(local).await.map(|m| m.len()).map_err(|e| (ErrorCode::Io, Some(e.to_string())))?;
    // A part this run left when the connection went away goes on where it stopped, if it is not
    // longer than the file; any other part is an earlier try's and goes.
    let resume = if reporter.transfers.take_resumable(&key) {
        match client.lstat(&part).await {
            Ok(a) if a.kind == EntryKind::File && a.size <= local_size => Some(a.size),
            _ => None,
        }
    } else {
        None
    };
    let (file, offset) = match resume {
        Some(offset) => (client.open_write(&part).await.map_err(sftp_failure)?, offset),
        None => {
            let _ = client.remove(&part).await;
            (client.create_new(&part).await.map_err(sftp_failure)?, 0)
        }
    };
    job.resumed(offset);
    let sent = send(job, local, &file, reporter, offset).await;
    let closed = file.close().await.map_err(sftp_failure);
    let result = match (sent, closed) {
        (Ok(()), Ok(())) => async {
            if replace {
                client.remove(&target).await.map_err(sftp_failure)?;
            }
            client.rename(&part, &target).await.map_err(sftp_failure)?;
            Ok::<(), SftpFailure>(())
        }
        .await
        .map_err(Stop::from),
        (Err(stop), _) => Err(stop),
        (Ok(()), Err(e)) => Err(Stop::Failed(e)),
    };
    match &result {
        Err(stop) if keeps_part(stop) => reporter.transfers.mark_resumable(key),
        Err(_) => {
            let _ = client.remove(&part).await;
        }
        Ok(()) => {}
    }
    result.map(|()| true)
}

async fn send(job: &Job, local: &Path, file: &RemoteFile, reporter: &mut Reporter<'_>, start: u64) -> Result<(), Stop> {
    let mut source = tokio::fs::File::open(local).await.map_err(|e| (ErrorCode::Io, Some(e.to_string())))?;
    tokio::io::AsyncSeekExt::seek(&mut source, std::io::SeekFrom::Start(start)).await.map_err(|e| (ErrorCode::Io, Some(e.to_string())))?;
    let mut offset = start;
    loop {
        if job.cancelled() {
            return Err(Stop::Cancelled);
        }
        let mut chunks: Vec<(u64, Vec<u8>)> = Vec::new();
        for _ in 0..WINDOW {
            let mut buf = vec![0u8; CHUNK as usize];
            let mut filled = 0;
            while filled < buf.len() {
                let n = source.read(&mut buf[filled..]).await.map_err(|e| (ErrorCode::Io, Some(e.to_string())))?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            if filled == 0 {
                break;
            }
            buf.truncate(filled);
            chunks.push((offset, buf));
            offset = offset.saturating_add(filled as u64);
        }
        if chunks.is_empty() {
            return Ok(());
        }
        let sizes: u64 = chunks.iter().map(|(_, c)| c.len() as u64).sum();
        let writes = chunks.into_iter().map(|(at, data)| file.write_at(at, data));
        for result in futures::future::join_all(writes).await {
            result.map_err(sftp_failure)?;
        }
        job.bytes.fetch_add(sizes, Ordering::Relaxed);
        reporter.tick();
    }
}

// ---- Download -------------------------------------------------------------------------------

/// A planned download: local folders to create (parents first) and files (remote path, local
/// path, stated size).
#[derive(Default)]
struct DownloadPlan {
    dirs: Vec<PathBuf>,
    files: Vec<(String, PathBuf, u64)>,
}

type Plan<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), SftpFailure>> + Send + 'a>>;

fn plan_remote<'a>(job: &'a Job, client: &'a SftpClient, path: String, local_dir: PathBuf, depth: usize, plan: &'a mut DownloadPlan) -> Plan<'a> {
    Box::pin(async move {
        if depth > MAX_DEPTH {
            return Err((ErrorCode::TooManyFiles, Some(format!("deeper than {MAX_DEPTH} folders"))));
        }
        if plan.dirs.len().saturating_add(plan.files.len()) >= MAX_ITEMS {
            return Err((ErrorCode::TooManyFiles, Some(format!("more than {MAX_ITEMS} files and folders"))));
        }
        let Some(name) = names::remote_name(&path) else {
            job.error(format!("{path}: has no name"));
            return Ok(());
        };
        // The name becomes part of a local path: it must be a plain name here.
        if let Err(e) = names::check_local_name_here(&name) {
            job.error(format!("{path}: {e}"));
            return Ok(());
        }
        let local = local_dir.join(&name);
        let attrs = match client.lstat(&path).await {
            Ok(a) => a,
            Err(e) => {
                job.error(format!("{path}: {e}"));
                return Ok(());
            }
        };
        let attrs = if attrs.kind == EntryKind::Symlink {
            match client.stat(&path).await {
                Ok(a) if a.kind == EntryKind::File => a,
                _ => {
                    job.error(format!("{path}: a link to a folder is not followed"));
                    return Ok(());
                }
            }
        } else {
            attrs
        };
        match attrs.kind {
            EntryKind::Dir => {
                plan.dirs.push(local.clone());
                for entry in walk_list(client, &path).await? {
                    match names::remote_join(&path, &entry.name) {
                        Ok(child) => plan_remote(job, client, child, local.clone(), depth + 1, plan).await?,
                        Err(e) => job.error(format!("{path}/{}: {e}", entry.name.chars().filter(|c| !c.is_control()).collect::<String>())),
                    }
                }
            }
            EntryKind::File => plan.files.push((path, local, attrs.size)),
            _ => job.error(format!("{path}: not a file or folder")),
        }
        Ok(())
    })
}

async fn download(
    job: &Job,
    client: &SftpClient,
    remote: Vec<String>,
    local_dir: PathBuf,
    conflict: Conflict,
    transfers: &Transfers,
) -> Result<(), Stop> {
    let mut plan = DownloadPlan::default();
    for path in remote {
        if job.cancelled() {
            return Err(Stop::Cancelled);
        }
        plan_remote(job, client, path, local_dir.clone(), 0, &mut plan).await?;
    }
    plan.files.retain(|(remote, _, _)| !job.is_completed(remote));
    let total: u64 = plan.files.iter().map(|(_, _, size)| *size).fold(0, u64::saturating_add);
    job.update(|v| {
        v.files_total = plan.files.len();
        v.bytes_total = total;
    });
    let mut reporter = Reporter { transfers, job, last: Instant::now() };
    let mut failed_dirs: Vec<PathBuf> = Vec::new();
    for dir in &plan.dirs {
        match std::fs::symlink_metadata(dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                job.error(format!("{}: a file of that name is there", dir.display()));
                failed_dirs.push(dir.clone());
            }
            Err(_) => {
                if let Err(e) = std::fs::create_dir(dir) {
                    job.error(format!("{}: {e}", dir.display()));
                    failed_dirs.push(dir.clone());
                }
            }
        }
    }
    for (remote, local, _) in &plan.files {
        if job.cancelled() {
            return Err(Stop::Cancelled);
        }
        if failed_dirs.iter().any(|d| local.starts_with(d)) {
            continue;
        }
        match download_file(job, client, remote, local, conflict, &mut reporter).await {
            Ok(true) => {
                job.update(|v| v.files_done += 1);
                job.complete(remote.clone());
            }
            Ok(false) => {
                job.update(|v| v.skipped += 1);
                job.complete(remote.clone());
            }
            Err(Stop::Cancelled) => return Err(Stop::Cancelled),
            Err(stop @ Stop::Failed(_)) if keeps_part(&stop) => return Err(stop),
            Err(Stop::Failed((_, detail))) => job.error(format!("{remote}: {}", detail.unwrap_or_default())),
        }
        reporter.tick();
    }
    Ok(())
}

async fn download_file(
    job: &Job,
    client: &SftpClient,
    remote: &str,
    local: &Path,
    conflict: Conflict,
    reporter: &mut Reporter<'_>,
) -> Result<bool, Stop> {
    let dir = local.parent().ok_or((ErrorCode::NameInvalid, None))?;
    let name = local.file_name().and_then(|n| n.to_str()).ok_or((ErrorCode::NameInvalid, None))?.to_owned();
    let exists = std::fs::symlink_metadata(local).is_ok();
    let target = match (exists, conflict) {
        (false, _) | (true, Conflict::Overwrite) => local.to_path_buf(),
        (true, Conflict::Skip) => return Ok(false),
        (true, Conflict::Rename) => {
            let free = names::free_name(&name, |n| dir.join(n).symlink_metadata().is_ok())
                .ok_or((ErrorCode::AlreadyExists, Some(local.display().to_string())))?;
            dir.join(free)
        }
    };
    let part = dir.join(names::part_name(&name));
    let key = format!("l:{}", part.display());
    let remote_size = client.stat(remote).await.map(|a| a.size).map_err(sftp_failure)?;
    // As for uploads: only a part this run left is continued, and only if it is not longer.
    let resume = if reporter.transfers.take_resumable(&key) {
        match std::fs::symlink_metadata(&part) {
            Ok(m) if m.is_file() && m.len() <= remote_size => Some(m.len()),
            _ => None,
        }
    } else {
        None
    };
    let io = |e: std::io::Error| (ErrorCode::Io, Some(e.to_string()));
    let (mut out, offset) = match resume {
        Some(offset) => (tokio::fs::OpenOptions::new().append(true).open(&part).await.map_err(io)?, offset),
        None => {
            let _ = std::fs::remove_file(&part);
            (tokio::fs::OpenOptions::new().write(true).create_new(true).open(&part).await.map_err(io)?, 0)
        }
    };
    job.resumed(offset);
    let file = client.open_read(remote).await.map_err(sftp_failure)?;
    let received = receive(job, &file, &mut out, reporter, offset).await;
    let _ = file.close().await;
    let flushed = out.flush().await.map_err(|e| Stop::Failed((ErrorCode::Io, Some(e.to_string()))));
    drop(out);
    let result = received.and(flushed).and_then(|()| std::fs::rename(&part, &target).map_err(|e| Stop::Failed((ErrorCode::Io, Some(e.to_string())))));
    match &result {
        Err(stop) if keeps_part(stop) => reporter.transfers.mark_resumable(key),
        Err(_) => {
            let _ = std::fs::remove_file(&part);
        }
        Ok(()) => {}
    }
    result.map(|()| true)
}

/// One remote file into an existing local file, overwritten from the start; see
/// [`Transfers::download_file`].
async fn download_to_file(job: &Job, client: &SftpClient, remote: String, local_file: PathBuf, transfers: &Transfers) -> Result<(), Stop> {
    let attrs = client.stat(&remote).await.map_err(sftp_failure)?;
    if attrs.kind != EntryKind::File {
        return Err(Stop::Failed((ErrorCode::NotSupported, Some(format!("{remote}: not a file")))));
    }
    job.update(|v| {
        v.files_total = 1;
        v.bytes_total = attrs.size;
    });
    let io = |e: std::io::Error| (ErrorCode::Io, Some(e.to_string()));
    // The file is there already (the picker made it); it is neither created nor followed anew.
    let mut out = tokio::fs::OpenOptions::new().write(true).truncate(true).open(&local_file).await.map_err(io)?;
    let file = client.open_read(&remote).await.map_err(sftp_failure)?;
    let mut reporter = Reporter { transfers, job, last: Instant::now() };
    let received = receive(job, &file, &mut out, &mut reporter, 0).await;
    let _ = file.close().await;
    let result = match received {
        Ok(()) => out.flush().await.map_err(|e| Stop::Failed(io(e))),
        Err(stop) => Err(stop),
    };
    if result.is_err() {
        // Half a file must not look like the whole one.
        let _ = out.set_len(0).await;
    } else {
        job.update(|v| v.files_done = 1);
        job.complete(remote);
    }
    result
}

async fn receive(job: &Job, file: &RemoteFile, out: &mut tokio::fs::File, reporter: &mut Reporter<'_>, start: u64) -> Result<(), Stop> {
    let mut pos = start;
    loop {
        if job.cancelled() {
            return Err(Stop::Cancelled);
        }
        let reads = (0..WINDOW).map(|i| {
            let at = pos.saturating_add(i * u64::from(CHUNK));
            async move { (at, file.read_at(at, CHUNK).await) }
        });
        let mut end = false;
        for (at, data) in futures::future::join_all(reads).await {
            // After a short read the rest of this window starts too far; the next one starts
            // where the data ends.
            if at != pos {
                break;
            }
            let data = data.map_err(sftp_failure)?;
            if data.is_empty() {
                end = true;
                break;
            }
            out.write_all(&data).await.map_err(|e| (ErrorCode::Io, Some(e.to_string())))?;
            pos = pos.saturating_add(data.len() as u64);
            job.bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
            if data.len() < CHUNK as usize {
                break;
            }
        }
        reporter.tick();
        if end {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a lost connection keeps a part; the user's cancel and every other failure delete it.
    #[test]
    fn only_a_lost_connection_keeps_a_part() {
        assert!(keeps_part(&Stop::Failed((ErrorCode::Network, None))));
        assert!(keeps_part(&Stop::Failed((ErrorCode::Timeout, None))));
        assert!(!keeps_part(&Stop::Cancelled));
        assert!(!keeps_part(&Stop::Failed((ErrorCode::SftpFailed, None))));
        assert!(!keeps_part(&Stop::Failed((ErrorCode::PermissionDenied, None))));
    }
}

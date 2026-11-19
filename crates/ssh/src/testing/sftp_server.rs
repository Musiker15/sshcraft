//! The `sftp` subsystem of the test server, on a folder of the test's choosing: `/` on the wire is
//! that folder, and `..` can never leave it. Blocking file calls are fine here; it serves tests.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use russh_sftp::protocol::{Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version};

/// What tests can look at afterwards: the modes `setstat` asked for, by path on the wire.
#[derive(Default)]
pub(crate) struct SftpRecords {
    pub(crate) modes: Mutex<HashMap<String, u32>>,
}

pub(crate) struct SftpServer {
    root: PathBuf,
    records: Arc<SftpRecords>,
    next: u64,
    files: HashMap<String, std::fs::File>,
    /// Open directories: their entries until read once, then nothing (end of directory).
    dirs: HashMap<String, Option<Vec<File>>>,
}

impl SftpServer {
    pub(crate) fn new(root: PathBuf, records: Arc<SftpRecords>) -> Self {
        SftpServer { root, records, next: 0, files: HashMap::new(), dirs: HashMap::new() }
    }

    fn handle(&mut self) -> String {
        self.next += 1;
        format!("h{}", self.next)
    }

    /// The wire path in canonical form: absolute, no `.`, `..` resolved without leaving `/`.
    fn canonical(path: &str) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                p => parts.push(p),
            }
        }
        format!("/{}", parts.join("/"))
    }

    fn local(&self, path: &str) -> PathBuf {
        let canonical = Self::canonical(path);
        let mut local = self.root.clone();
        for part in canonical.split('/').filter(|p| !p.is_empty()) {
            local.push(part);
        }
        local
    }

    fn ok(id: u32) -> Status {
        Status { id, status_code: StatusCode::Ok, error_message: "Ok".into(), language_tag: "en-US".into() }
    }
}

fn code(e: &std::io::Error) -> StatusCode {
    match e.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

fn attrs_of(path: &Path, follow: bool) -> Result<FileAttributes, StatusCode> {
    let meta = if follow { std::fs::metadata(path) } else { std::fs::symlink_metadata(path) }.map_err(|e| code(&e))?;
    let mut attrs = FileAttributes::from(&meta);
    if meta.file_type().is_symlink() {
        attrs.permissions = Some(0o120777);
    } else if cfg!(windows) {
        attrs.permissions = Some(if meta.is_dir() { 0o040755 } else { 0o100644 });
    }
    Ok(attrs)
}

impl russh_sftp::server::Handler for SftpServer {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(&mut self, _version: u32, _extensions: HashMap<String, String>) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        Ok(Name { id, files: vec![File::dummy(Self::canonical(&path))] })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        Ok(Attrs { id, attrs: attrs_of(&self.local(&path), true)? })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        Ok(Attrs { id, attrs: attrs_of(&self.local(&path), false)? })
    }

    async fn setstat(&mut self, id: u32, path: String, attrs: FileAttributes) -> Result<Status, Self::Error> {
        let local = self.local(&path);
        std::fs::symlink_metadata(&local).map_err(|e| code(&e))?;
        if let Some(mode) = attrs.permissions {
            self.records.modes.lock().unwrap().insert(Self::canonical(&path), mode);
        }
        if let Some(mtime) = attrs.mtime {
            let file = std::fs::File::options().write(true).open(&local).map_err(|e| code(&e))?;
            file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(u64::from(mtime))).map_err(|e| code(&e))?;
        }
        Ok(Self::ok(id))
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let local = self.local(&path);
        let mut files = vec![File::dummy("."), File::dummy("..")];
        for entry in std::fs::read_dir(&local).map_err(|e| code(&e))? {
            let entry = entry.map_err(|e| code(&e))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let attrs = attrs_of(&entry.path(), false)?;
            files.push(File::new(name, attrs));
        }
        let handle = self.handle();
        self.dirs.insert(handle.clone(), Some(files));
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        match self.dirs.get_mut(&handle) {
            Some(entries) => match entries.take() {
                Some(files) => Ok(Name { id, files }),
                None => Err(StatusCode::Eof),
            },
            None => Err(StatusCode::Failure),
        }
    }

    async fn open(&mut self, id: u32, filename: String, pflags: OpenFlags, _attrs: FileAttributes) -> Result<Handle, Self::Error> {
        let local = self.local(&filename);
        let mut options = std::fs::OpenOptions::new();
        options.read(pflags.contains(OpenFlags::READ)).write(pflags.contains(OpenFlags::WRITE));
        if pflags.contains(OpenFlags::CREATE) && pflags.contains(OpenFlags::EXCLUDE) {
            options.create_new(true);
        } else if pflags.contains(OpenFlags::CREATE) {
            options.create(true);
        }
        if pflags.contains(OpenFlags::TRUNCATE) {
            options.truncate(true);
        }
        let file = options.open(&local).map_err(|e| code(&e))?;
        let handle = self.handle();
        self.files.insert(handle.clone(), file);
        Ok(Handle { id, handle })
    }

    async fn read(&mut self, id: u32, handle: String, offset: u64, len: u32) -> Result<Data, Self::Error> {
        let file = self.files.get_mut(&handle).ok_or(StatusCode::Failure)?;
        file.seek(SeekFrom::Start(offset)).map_err(|e| code(&e))?;
        let mut data = vec![0u8; len as usize];
        let n = file.read(&mut data).map_err(|e| code(&e))?;
        if n == 0 {
            return Err(StatusCode::Eof);
        }
        data.truncate(n);
        Ok(Data { id, data })
    }

    async fn write(&mut self, id: u32, handle: String, offset: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
        let file = self.files.get_mut(&handle).ok_or(StatusCode::Failure)?;
        file.seek(SeekFrom::Start(offset)).map_err(|e| code(&e))?;
        file.write_all(&data).map_err(|e| code(&e))?;
        Ok(Self::ok(id))
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        if self.files.remove(&handle).is_none() && self.dirs.remove(&handle).is_none() {
            return Err(StatusCode::Failure);
        }
        Ok(Self::ok(id))
    }

    async fn mkdir(&mut self, id: u32, path: String, _attrs: FileAttributes) -> Result<Status, Self::Error> {
        std::fs::create_dir(self.local(&path)).map_err(|e| code(&e))?;
        Ok(Self::ok(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        std::fs::remove_dir(self.local(&path)).map_err(|e| code(&e))?;
        Ok(Self::ok(id))
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        std::fs::remove_file(self.local(&filename)).map_err(|e| code(&e))?;
        Ok(Self::ok(id))
    }

    async fn rename(&mut self, id: u32, oldpath: String, newpath: String) -> Result<Status, Self::Error> {
        let to = self.local(&newpath);
        if to.exists() {
            return Err(StatusCode::Failure);
        }
        std::fs::rename(self.local(&oldpath), to).map_err(|e| code(&e))?;
        Ok(Self::ok(id))
    }
}

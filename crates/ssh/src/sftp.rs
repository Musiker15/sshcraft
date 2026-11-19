//! SFTP over an SSH connection: the `sftp` subsystem on its own channel, spoken through
//! russh-sftp's raw session so every listing and every read stays bounded.
//!
//! Everything the server sends is hostile: names are passed on as they came (the engine checks
//! them before one becomes a local path), listings stop at a limit, and a read never asks for
//! more than [`CHUNK`] bytes.

use std::sync::Arc;
use std::time::Duration;

use russh_sftp::client::RawSftpSession;
use russh_sftp::client::error::Error as RawError;
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};

/// The most one read asks for, and the size writes are cut into.
pub const CHUNK: u32 = 32 * 1024;
/// How long one request may wait for its answer.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// What went wrong in an SFTP request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SftpError {
    #[error("no such file or folder")]
    NotFound,
    #[error("permission denied")]
    PermissionDenied,
    #[error("the server refused: {0}")]
    Failure(String),
    #[error("the server does not support this")]
    Unsupported,
    #[error("the server did not answer in time")]
    Timeout,
    #[error("the connection was lost: {0}")]
    Lost(String),
    #[error("the server sent something unexpected: {0}")]
    Protocol(String),
}

impl From<RawError> for SftpError {
    fn from(e: RawError) -> Self {
        match e {
            RawError::Status(status) => match status.status_code {
                StatusCode::NoSuchFile => SftpError::NotFound,
                StatusCode::PermissionDenied => SftpError::PermissionDenied,
                StatusCode::OpUnsupported => SftpError::Unsupported,
                StatusCode::Eof => SftpError::Protocol("unexpected end of file".into()),
                _ => SftpError::Failure(bounded(&status.error_message)),
            },
            RawError::IO(message) => SftpError::Lost(bounded(&message)),
            RawError::Timeout => SftpError::Timeout,
            // russh-sftp says these when the channel under the session is gone (or a request's
            // answer can no longer come): the connection went away, which a retry can go on
            // from, not a server that misbehaves.
            RawError::UnexpectedBehavior(message)
                if message == "session closed"
                    || message == "sender dropped"
                    || message.starts_with("SendError:")
                    || message.starts_with("RecvError:") =>
            {
                SftpError::Lost(bounded(&message))
            }
            RawError::Limited(message) | RawError::UnexpectedBehavior(message) => SftpError::Protocol(bounded(&message)),
            RawError::UnexpectedPacket => SftpError::Protocol("unexpected packet".into()),
        }
    }
}

/// A server message cut to a length fit for a dialog, at a character boundary.
fn bounded(text: &str) -> String {
    const MAX: usize = 300;
    let mut end = text.len().min(MAX);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].chars().filter(|c| !c.is_control()).collect()
}

/// What a directory entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

/// What the server says about a file, as far as it said it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAttrs {
    pub kind: EntryKind,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: Option<u64>,
    /// The permission bits (`0o7777`), when the server sent them.
    pub permissions: Option<u32>,
}

impl From<&FileAttributes> for RemoteAttrs {
    fn from(a: &FileAttributes) -> Self {
        let kind = match a.permissions {
            None => EntryKind::Other,
            Some(_) => {
                let t = a.file_type();
                if t.is_dir() {
                    EntryKind::Dir
                } else if t.is_symlink() {
                    EntryKind::Symlink
                } else if t.is_file() {
                    EntryKind::File
                } else {
                    EntryKind::Other
                }
            }
        };
        RemoteAttrs { kind, size: a.size.unwrap_or(0), modified: a.mtime.map(u64::from), permissions: a.permissions.map(|p| p & 0o7777) }
    }
}

/// One entry of a listing; the name exactly as the server sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    pub name: String,
    pub attrs: RemoteAttrs,
}

/// A directory's entries, without `.` and `..`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listing {
    pub entries: Vec<RemoteEntry>,
    /// The directory had more than the limit; the rest was not read.
    pub truncated: bool,
}

/// An SFTP session on one channel of a connection. Cheap to clone; requests may run at once.
#[derive(Clone)]
pub struct SftpClient {
    raw: Arc<RawSftpSession>,
}

impl SftpClient {
    /// Starts the protocol on `stream` (the channel of the `sftp` subsystem).
    pub async fn start<S>(stream: S) -> Result<SftpClient, SftpError>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let raw = RawSftpSession::new(stream);
        raw.set_timeout(REQUEST_TIMEOUT.as_secs());
        raw.init().await?;
        Ok(SftpClient { raw: Arc::new(raw) })
    }

    /// The absolute form of `path` as the server resolves it (`.` is the home folder).
    pub async fn canonicalize(&self, path: &str) -> Result<String, SftpError> {
        let name = self.raw.realpath(path).await?;
        name.files.into_iter().next().map(|f| f.filename).ok_or_else(|| SftpError::Protocol("realpath gave no name".into()))
    }

    /// The entries of `dir`, at most `max`; `.` and `..` are left out.
    pub async fn list(&self, dir: &str, max: usize) -> Result<Listing, SftpError> {
        let handle = self.raw.opendir(dir).await?.handle;
        let mut listing = Listing::default();
        let result = loop {
            match self.raw.readdir(handle.as_str()).await {
                Ok(name) => {
                    for file in name.files {
                        if file.filename == "." || file.filename == ".." {
                            continue;
                        }
                        if listing.entries.len() >= max {
                            listing.truncated = true;
                            break;
                        }
                        listing.entries.push(RemoteEntry { attrs: RemoteAttrs::from(&file.attrs), name: file.filename });
                    }
                    if listing.truncated {
                        break Ok(());
                    }
                }
                Err(RawError::Status(status)) if status.status_code == StatusCode::Eof => break Ok(()),
                Err(e) => break Err(SftpError::from(e)),
            }
        };
        let _ = self.raw.close(handle).await;
        result.map(|()| listing)
    }

    /// What `path` is, following a symbolic link.
    pub async fn stat(&self, path: &str) -> Result<RemoteAttrs, SftpError> {
        Ok(RemoteAttrs::from(&self.raw.stat(path).await?.attrs))
    }

    /// What `path` is, the link itself when it is one.
    pub async fn lstat(&self, path: &str) -> Result<RemoteAttrs, SftpError> {
        Ok(RemoteAttrs::from(&self.raw.lstat(path).await?.attrs))
    }

    pub async fn mkdir(&self, path: &str) -> Result<(), SftpError> {
        self.raw.mkdir(path, FileAttributes::empty()).await?;
        Ok(())
    }

    pub async fn rmdir(&self, path: &str) -> Result<(), SftpError> {
        self.raw.rmdir(path).await?;
        Ok(())
    }

    pub async fn remove(&self, path: &str) -> Result<(), SftpError> {
        self.raw.remove(path).await?;
        Ok(())
    }

    pub async fn rename(&self, from: &str, to: &str) -> Result<(), SftpError> {
        self.raw.rename(from, to).await?;
        Ok(())
    }

    /// Sets the permission bits of `path` to `mode & 0o7777`.
    pub async fn chmod(&self, path: &str, mode: u32) -> Result<(), SftpError> {
        let attrs = FileAttributes { permissions: Some(mode & 0o7777), ..FileAttributes::empty() };
        self.raw.setstat(path, attrs).await?;
        Ok(())
    }

    /// Sets the modification (and access) time of `path` to `mtime`, seconds since 1970; a time
    /// the protocol cannot carry (past 2106) is refused.
    pub async fn set_modified(&self, path: &str, mtime: u64) -> Result<(), SftpError> {
        let secs = u32::try_from(mtime).map_err(|_| SftpError::Protocol("a time past 2106".into()))?;
        let attrs = FileAttributes { atime: Some(secs), mtime: Some(secs), ..FileAttributes::empty() };
        self.raw.setstat(path, attrs).await?;
        Ok(())
    }

    /// Opens `path` for reading.
    pub async fn open_read(&self, path: &str) -> Result<RemoteFile, SftpError> {
        let handle = self.raw.open(path, OpenFlags::READ, FileAttributes::empty()).await?.handle;
        Ok(RemoteFile { raw: self.raw.clone(), handle })
    }

    /// Creates `path` for writing; fails when it exists, so nothing is overwritten by surprise.
    pub async fn create_new(&self, path: &str) -> Result<RemoteFile, SftpError> {
        let flags = OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE;
        let handle = self.raw.open(path, flags, FileAttributes::empty()).await?.handle;
        Ok(RemoteFile { raw: self.raw.clone(), handle })
    }

    /// Opens an existing `path` for writing at offsets, without cutting it short: to go on with a
    /// part a lost connection left.
    pub async fn open_write(&self, path: &str) -> Result<RemoteFile, SftpError> {
        let handle = self.raw.open(path, OpenFlags::WRITE, FileAttributes::empty()).await?.handle;
        Ok(RemoteFile { raw: self.raw.clone(), handle })
    }

    /// Ends the session; its channel closes.
    pub fn close(&self) {
        let _ = self.raw.close_session();
    }
}

/// An open remote file. Reads and writes at an offset may run at once.
pub struct RemoteFile {
    raw: Arc<RawSftpSession>,
    handle: String,
}

impl RemoteFile {
    /// Up to `len` (at most [`CHUNK`]) bytes from `offset`; empty at the end of the file.
    pub async fn read_at(&self, offset: u64, len: u32) -> Result<Vec<u8>, SftpError> {
        match self.raw.read(self.handle.as_str(), offset, len.min(CHUNK)).await {
            Ok(data) => {
                let mut data = data.data;
                // A server may not send more than was asked.
                data.truncate(len.min(CHUNK) as usize);
                Ok(data)
            }
            Err(RawError::Status(status)) if status.status_code == StatusCode::Eof => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes `data` (at most [`CHUNK`] bytes) at `offset`.
    pub async fn write_at(&self, offset: u64, data: Vec<u8>) -> Result<(), SftpError> {
        if data.len() > CHUNK as usize {
            return Err(SftpError::Protocol("write larger than a chunk".into()));
        }
        self.raw.write(self.handle.as_str(), offset, data).await?;
        Ok(())
    }

    /// Closes the file; for a written one, the server confirms it was stored.
    pub async fn close(self) -> Result<(), SftpError> {
        self.raw.close(self.handle.as_str()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_session_is_a_lost_connection() {
        for closed in ["session closed", "sender dropped", "SendError: channel closed", "RecvError: channel closed"] {
            assert!(matches!(SftpError::from(RawError::UnexpectedBehavior(closed.into())), SftpError::Lost(_)), "{closed}");
        }
        assert!(matches!(SftpError::from(RawError::UnexpectedBehavior("odd".into())), SftpError::Protocol(_)));
    }

    #[test]
    fn server_messages_are_cut_and_cleaned() {
        assert_eq!(bounded("a\u{1b}[31mb\n"), "a[31mb");
        let long = "ä".repeat(400);
        let cut = bounded(&long);
        assert!(cut.len() <= 300 && cut.chars().all(|c| c == 'ä'));
    }

    #[test]
    fn attributes_name_the_kind_and_keep_only_permission_bits() {
        let dir = FileAttributes { permissions: Some(0o040755), size: Some(4096), mtime: Some(7), ..FileAttributes::empty() };
        let a = RemoteAttrs::from(&dir);
        assert_eq!((a.kind, a.permissions, a.modified), (EntryKind::Dir, Some(0o755), Some(7)));
        let bare = RemoteAttrs::from(&FileAttributes::empty());
        assert_eq!((bare.kind, bare.size), (EntryKind::Other, 0));
    }
}

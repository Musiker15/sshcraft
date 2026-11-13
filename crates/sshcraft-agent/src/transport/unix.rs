//! A Unix socket in a folder only the user can enter, with the peer's user checked.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

pub type ServerStream = UnixStream;
pub type ClientStream = UnixStream;

/// `$XDG_RUNTIME_DIR/sshcraft/agent.sock`, else `~/.sshcraft/agent.sock`.
pub fn endpoint() -> io::Result<String> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|d| PathBuf::from(d).join("sshcraft"))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".sshcraft")))
        .ok_or_else(|| io::Error::other("no runtime or home folder"))?;
    Ok(base.join("agent.sock").to_string_lossy().into_owned())
}

fn my_uid() -> u32 {
    // SAFETY: getuid cannot fail.
    unsafe { libc::getuid() }
}

pub struct Listener(UnixListener);

pub fn listen_at(path: &str) -> io::Result<Listener> {
    let path = Path::new(path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    // A socket left by a stopped app is removed; a live one means another instance.
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "already listening",
            ));
        }
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(Listener(listener))
}

impl Listener {
    pub async fn accept(&mut self) -> io::Result<UnixStream> {
        loop {
            let (stream, _) = self.0.accept().await?;
            if stream.peer_cred()?.uid() == my_uid() {
                return Ok(stream);
            }
        }
    }
}

pub async fn connect_to(path: &str) -> io::Result<UnixStream> {
    let stream = UnixStream::connect(path).await?;
    if stream.peer_cred()?.uid() != my_uid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the program on the other end does not run as this user",
        ));
    }
    Ok(stream)
}

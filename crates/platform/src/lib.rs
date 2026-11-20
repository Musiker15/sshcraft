//! Platform folders for SSHCraft: where the app keeps its data.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod legacy;
mod lock;
pub mod shortcut;

pub use legacy::{current_user_sid, is_legacy_pipe_name, is_own_legacy_pipe, legacy_in_pipes, legacy_instance_running, parse_whoami_sid};
pub use lock::{DataLock, lock_data_dir};

use std::path::PathBuf;

/// Why a platform folder could not be found.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// The operating system reports no home folder for this user.
    #[error("no home folder for this user")]
    NoHome,
    /// Another process (or another engine in this one) holds the data folder.
    #[error("the data folder {0} is in use by another SSHCraft")]
    DataInUse(PathBuf),
    /// The file system refused.
    #[error("{0}")]
    Io(String),
}

/// The folder that holds `sshcraft.db`. It is the folder the Tauri version used, so a rebuilt
/// app finds the data an earlier version wrote (on Windows `%APPDATA%\musiker15\SSHCraft\data`).
/// In portable mode it is the `data` folder in [`portable_root`].
pub fn data_dir() -> Result<PathBuf, PlatformError> {
    if let Some(root) = portable_root() {
        return Ok(portable_data_dir(&root));
    }
    directories::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION).map(|dirs| dirs.data_dir().to_path_buf()).ok_or(PlatformError::NoHome)
}

/// The folder for SSHCraft's own settings files beside the data: the agent link's token and
/// connection file live here (on Windows `%APPDATA%\musiker15\SSHCraft\config`, in the user's
/// profile, which only the user, SYSTEM and administrators may read). `SSHCRAFT_LINK_DIR` moves it
/// (tests, portable use).
pub fn config_dir() -> Result<PathBuf, PlatformError> {
    if let Some(dir) = std::env::var_os("SSHCRAFT_LINK_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    directories::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION).map(|dirs| dirs.config_dir().to_path_buf()).ok_or(PlatformError::NoHome)
}

/// The folder beside the program that turns on portable mode.
pub const PORTABLE_FOLDER: &str = "sshcraft-data";

/// Portable mode's folder: `sshcraft-data` beside the running program, when it exists. The data
/// then lives in it and the vault's master key is sealed with a passphrase there instead of
/// being kept in the system's credential store. Never on Android, where the app has no folder of
/// its own beside it.
pub fn portable_root() -> Option<PathBuf> {
    if cfg!(target_os = "android") {
        return None;
    }
    portable_root_beside(&std::env::current_exe().ok()?)
}

/// The portable folder beside the program `exe`, when it is a folder.
pub fn portable_root_beside(exe: &std::path::Path) -> Option<PathBuf> {
    let root = exe.parent()?.join(PORTABLE_FOLDER);
    root.is_dir().then_some(root)
}

/// Where portable mode keeps the data inside `root`.
pub fn portable_data_dir(root: &std::path::Path) -> PathBuf {
    root.join("data")
}

/// The names the Tauri version used for its folders; changing one moves existing data away.
const QUALIFIER: &str = "de";
const ORGANIZATION: &str = "musiker15";
const APPLICATION: &str = "SSHCraft";

/// The addresses of this computer's network interfaces; empty when they cannot be read.
pub fn interface_addresses() -> Vec<std::net::IpAddr> {
    if_addrs::get_if_addrs().map(|list| list.into_iter().map(|i| i.ip()).collect()).unwrap_or_default()
}

/// The user's home folder, if the operating system reports one.
pub fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::data_dir;

    /// Only a folder named `sshcraft-data` beside the program turns portable mode on; a file of
    /// that name, or the folder anywhere else, does not.
    #[test]
    fn portable_mode_needs_the_folder_beside_the_program() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("sshcraft.exe");
        assert_eq!(super::portable_root_beside(&exe), None);
        std::fs::write(dir.path().join("sshcraft-data"), b"").unwrap();
        assert_eq!(super::portable_root_beside(&exe), None);
        std::fs::remove_file(dir.path().join("sshcraft-data")).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::create_dir(dir.path().join("sub").join("sshcraft-data")).unwrap();
        assert_eq!(super::portable_root_beside(&exe), None);
        std::fs::create_dir(dir.path().join("sshcraft-data")).unwrap();
        let root = super::portable_root_beside(&exe).unwrap();
        assert_eq!(root, dir.path().join("sshcraft-data"));
        assert_eq!(super::portable_data_dir(&root), dir.path().join("sshcraft-data").join("data"));
    }
    use directories::ProjectDirs;

    /// The rebuild must find the database the Tauri version wrote: same folder, same name.
    #[test]
    fn data_dir_matches_the_tauri_location() {
        let dir = data_dir().unwrap();
        let expected = ProjectDirs::from("de", "musiker15", "SSHCraft").unwrap();
        assert_eq!(dir, expected.data_dir());
        if cfg!(windows) {
            assert!(dir.ends_with(r"musiker15\SSHCraft\data"), "{}", dir.display());
        }
    }

    /// The three names behind the folder, pinned as literals: Windows ignores the qualifier, so
    /// the folder test alone would not notice a changed "de" that moves the macOS folder.
    #[test]
    fn folder_names_are_the_tauri_ones() {
        assert_eq!((super::QUALIFIER, super::ORGANIZATION, super::APPLICATION), ("de", "musiker15", "SSHCraft"));
    }

    /// The folder ends where the Tauri version kept it, on every desktop system.
    #[test]
    fn data_dir_ends_in_the_known_folder() {
        let dir = data_dir().unwrap();
        let end = if cfg!(windows) {
            std::path::Path::new(r"musiker15\SSHCraft\data")
        } else if cfg!(target_os = "macos") {
            std::path::Path::new("de.musiker15.SSHCraft")
        } else {
            std::path::Path::new("sshcraft")
        };
        assert!(dir.ends_with(end), "{}", dir.display());
    }
}

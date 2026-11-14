//! An exclusive lock on the data folder: only one process opens the database and the vault.

use std::fs::File;
use std::path::Path;

use crate::PlatformError;

/// Holds the lock on the data folder until it is dropped.
#[derive(Debug)]
pub struct DataLock {
    _file: File,
}

/// Creates `dir` if needed and takes the lock file `dir/sshcraft.lock`. A lock held by this or
/// another process is [`PlatformError::DataInUse`].
pub fn lock_data_dir(dir: &Path) -> Result<DataLock, PlatformError> {
    std::fs::create_dir_all(dir).map_err(|e| PlatformError::Io(e.to_string()))?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("sshcraft.lock"))
        .map_err(|e| PlatformError::Io(e.to_string()))?;
    // The lock belongs to this open file: the operating system releases it when the file closes,
    // also when the process ends without cleaning up.
    match file.try_lock() {
        Ok(()) => Ok(DataLock { _file: file }),
        Err(std::fs::TryLockError::WouldBlock) => Err(PlatformError::DataInUse(dir.to_path_buf())),
        Err(std::fs::TryLockError::Error(e)) => Err(PlatformError::Io(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::lock_data_dir;
    use crate::PlatformError;

    #[test]
    fn data_lock_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let _first = lock_data_dir(dir.path()).unwrap();
        assert!(matches!(lock_data_dir(dir.path()), Err(PlatformError::DataInUse(_))));
    }

    #[test]
    fn data_lock_is_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let first = lock_data_dir(dir.path()).unwrap();
        drop(first);
        assert!(lock_data_dir(dir.path()).is_ok());
    }

    #[test]
    fn a_missing_folder_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("a").join("b");
        assert!(lock_data_dir(&data).is_ok());
        assert!(data.join("sshcraft.lock").is_file());
    }
}

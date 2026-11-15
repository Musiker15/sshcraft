//! The connection file a running app publishes beside its token file: which port, which token
//! file. It holds no secret.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::LinkError;

pub const CONNECTION_FILE: &str = "agent-connection.json";
pub const TOKEN_FILE: &str = "agent-control.token";
const VERSION: u32 = 1;
/// A connection file larger than this is not read.
const MAX_FILE: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionFile {
    pub version: u32,
    pub pid: u32,
    pub port: u16,
    pub token_file: PathBuf,
    /// The app's version, for messages.
    pub app: String,
}

impl ConnectionFile {
    pub fn new(port: u16, token_file: PathBuf, app: String) -> Self {
        Self { version: VERSION, pid: std::process::id(), port, token_file, app }
    }

    /// Writes the file into `dir`, replacing it in one step.
    pub fn write(&self, dir: &Path) -> Result<(), LinkError> {
        let text = serde_json::to_string_pretty(self).map_err(|e| LinkError::Io(e.to_string()))?;
        std::fs::create_dir_all(dir).map_err(|e| LinkError::Io(format!("{}: {e}", dir.display())))?;
        let path = dir.join(CONNECTION_FILE);
        let tmp = dir.join(format!("{CONNECTION_FILE}.{}.tmp", std::process::id()));
        std::fs::write(&tmp, text).map_err(|e| LinkError::Io(format!("{}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            LinkError::Io(format!("{}: {e}", path.display()))
        })
    }

    /// The file in `dir`; `None` when it is missing, too large, damaged or of another version.
    pub fn read(dir: &Path) -> Option<Self> {
        let path = dir.join(CONNECTION_FILE);
        if std::fs::metadata(&path).ok()?.len() > MAX_FILE {
            return None;
        }
        let file: Self = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
        (file.version == VERSION).then_some(file)
    }

    /// Removes the file in `dir`; a missing file is fine.
    pub fn remove(dir: &Path) -> Result<(), LinkError> {
        match std::fs::remove_file(dir.join(CONNECTION_FILE)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(LinkError::Io(e.to_string())),
        }
    }
}

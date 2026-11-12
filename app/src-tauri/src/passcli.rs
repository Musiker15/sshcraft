//! Imports SSH keys from Proton Pass through its command line tool `pass-cli`.
//!
//! The output of `pass-cli item view` contains the private key in clear text. It is never
//! logged, never written to a file and never copied into an error message.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use stollen_core::store::Store;
use stollen_core::vault::{self, KeyInfo, Vault, VaultError};
use zeroize::Zeroizing;

const VIEW_REASON: &str = "Stollen: import an SSH key into the encrypted local key vault";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PassKey {
    pub share_id: String,
    pub item_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PassCliStatus {
    NotInstalled,
    LoggedOut,
    Ready,
}

#[derive(Debug, thiserror::Error)]
pub enum PassCliError {
    #[error("pass-cli ist nicht installiert.")]
    NotInstalled,
    #[error("pass-cli ist nicht angemeldet. Melde dich mit `pass-cli login` an.")]
    LoggedOut,
    #[error("Dieser Eintrag enthält keinen privaten Schlüssel.")]
    NoPrivateKey,
    #[error("Der Schlüssel in Proton Pass ist mit einer Passphrase geschützt.")]
    Encrypted,
    #[error("pass-cli ist fehlgeschlagen: {0}")]
    Failed(String),
    #[error(transparent)]
    Vault(VaultError),
}

pub trait PassCli: Send + Sync {
    fn status(&self) -> PassCliStatus;
    fn list_ssh_keys(&self) -> Result<Vec<PassKey>, PassCliError>;
    fn private_key(&self, share_id: &str, item_id: &str)
    -> Result<Zeroizing<String>, PassCliError>;
}

pub fn ensure_ready(status: PassCliStatus) -> Result<(), PassCliError> {
    match status {
        PassCliStatus::Ready => Ok(()),
        PassCliStatus::LoggedOut => Err(PassCliError::LoggedOut),
        PassCliStatus::NotInstalled => Err(PassCliError::NotInstalled),
    }
}

fn malformed() -> PassCliError {
    PassCliError::Failed("unerwartete Ausgabe".into())
}

pub fn parse_key_list(json: &str) -> Result<Vec<PassKey>, PassCliError> {
    let value: Value = serde_json::from_str(json).map_err(|_| malformed())?;
    let items = value["items"].as_array().ok_or_else(malformed)?;
    Ok(items
        .iter()
        .filter(|item| item["item_type"] == "ssh_key")
        .filter_map(|item| {
            Some(PassKey {
                share_id: item["share_id"].as_str()?.to_owned(),
                item_id: item["id"].as_str()?.to_owned(),
                title: item["title"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect())
}

// Typed view of `pass-cli item view --output json`: only the private key is kept, and only
// inside a `Zeroizing` buffer. serde_json's internal scratch buffer for escaped strings is not
// zeroed; that copy and the pipe buffer inside `Command::output` are the remaining residue.
#[derive(serde::Deserialize)]
struct ItemView {
    item: ItemViewItem,
}

#[derive(serde::Deserialize)]
struct ItemViewItem {
    content: ItemViewOuter,
}

#[derive(serde::Deserialize)]
struct ItemViewOuter {
    content: ItemViewInner,
}

#[derive(serde::Deserialize)]
struct ItemViewInner {
    #[serde(rename = "SshKey")]
    ssh_key: Option<SshKeyContent>,
}

#[derive(serde::Deserialize)]
struct SshKeyContent {
    private_key: Option<Zeroizing<String>>,
}

pub fn parse_private_key(json: &[u8]) -> Result<Zeroizing<String>, PassCliError> {
    let view: ItemView = serde_json::from_slice(json).map_err(|_| malformed())?;
    match view
        .item
        .content
        .content
        .ssh_key
        .and_then(|k| k.private_key)
    {
        Some(key) if !key.trim().is_empty() => Ok(key),
        _ => Err(PassCliError::NoPrivateKey),
    }
}

/// Builds the error for a failed `pass-cli` run from its exit code and the first line of
/// stderr. stdout may hold secrets and is deliberately ignored.
pub fn failure(code: Option<i32>, _stdout: &[u8], stderr: &[u8]) -> PassCliError {
    let first_line = String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_owned();
    let code = code.map_or_else(|| "?".to_owned(), |c| c.to_string());
    PassCliError::Failed(format!("Exit-Code {code}: {first_line}"))
}

/// Fetches the key from Proton Pass and seals it into the vault. The store is only locked for
/// the final write, after the slow parts (process call, parsing) are done.
pub fn import_from_pass_cli(
    cli: &dyn PassCli,
    vault: &Vault,
    store: &Mutex<Store>,
    share_id: &str,
    item_id: &str,
    name: &str,
) -> Result<KeyInfo, PassCliError> {
    let text = cli.private_key(share_id, item_id)?;
    let key = vault::parse_private_key(&text, None).map_err(|e| match e {
        VaultError::WrongPassphrase => PassCliError::Encrypted,
        other => PassCliError::Vault(other),
    })?;
    let store = store.lock().expect("store lock");
    vault
        .store_key(&store, name, &key)
        .map_err(PassCliError::Vault)
}

/// The real `pass-cli`, run as a hidden child process.
pub struct PassCliProcess {
    exe: PathBuf,
}

impl PassCliProcess {
    pub fn locate() -> Option<Self> {
        let installed = std::env::var_os("LOCALAPPDATA")
            .map(|d| PathBuf::from(d).join(r"Programs\ProtonPass\pass-cli.exe"));
        let on_path = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|p| {
                    p.join(if cfg!(windows) {
                        "pass-cli.exe"
                    } else {
                        "pass-cli"
                    })
                })
                .find(|p| p.is_file())
        });
        installed
            .filter(|p| p.is_file())
            .or(on_path)
            .map(|exe| Self { exe })
    }

    fn run(&self, args: &[&str], reason: Option<&str>) -> Result<Zeroizing<Vec<u8>>, PassCliError> {
        let mut command = Command::new(&self.exe);
        command.args(args);
        if let Some(reason) = reason {
            command.env("PROTON_PASS_AGENT_REASON", reason);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let output = command
            .output()
            .map_err(|e| PassCliError::Failed(e.kind().to_string()))?;
        let stdout = Zeroizing::new(output.stdout);
        if output.status.success() {
            Ok(stdout)
        } else {
            Err(failure(output.status.code(), &stdout, &output.stderr))
        }
    }
}

impl PassCli for PassCliProcess {
    fn status(&self) -> PassCliStatus {
        match self.run(&["info"], None) {
            Ok(_) => PassCliStatus::Ready,
            Err(_) => PassCliStatus::LoggedOut,
        }
    }

    fn list_ssh_keys(&self) -> Result<Vec<PassKey>, PassCliError> {
        ensure_ready(self.status())?;
        let out = self.run(&["item", "list", "--output", "json"], None)?;
        parse_key_list(&String::from_utf8_lossy(&out))
    }

    fn private_key(
        &self,
        share_id: &str,
        item_id: &str,
    ) -> Result<Zeroizing<String>, PassCliError> {
        let uri = format!("pass://{share_id}/{item_id}");
        let out = self.run(
            &["item", "view", &uri, "--output", "json"],
            Some(VIEW_REASON),
        )?;
        parse_private_key(&out)
    }
}

/// Status of the installed `pass-cli`, `NotInstalled` if there is none.
pub fn local_status() -> PassCliStatus {
    PassCliProcess::locate().map_or(PassCliStatus::NotInstalled, |cli| cli.status())
}

#[cfg(test)]
#[path = "passcli_tests.rs"]
mod tests;

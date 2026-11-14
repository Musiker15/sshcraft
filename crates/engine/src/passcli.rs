//! Imports SSH keys from Proton Pass through its command line tool `pass-cli`.
//!
//! The output of `pass-cli item view` contains the private key in clear text. It is never
//! logged, never written to a file and never copied into an error message. `pass-cli` starts
//! with an argument list, never through a shell.

use std::ffi::OsStr;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use sshcraft_store::store::Store;
use sshcraft_store::vault::{self, KeyInfo, Vault, VaultError};
use zeroize::Zeroizing;

use crate::error::{EngineError, ErrorCode};

const VIEW_REASON: &str = "SSHCraft: import an SSH key into the encrypted local key vault";

/// `info` and `item list` answer quickly; `item view` may wait for a confirmation in Proton Pass.
const QUICK_LIMIT: Duration = Duration::from_secs(15);
const VIEW_LIMIT: Duration = Duration::from_secs(120);

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
    /// Installed, but the status check failed for another reason (network, crash, timeout).
    Failed,
}

#[derive(Debug, thiserror::Error)]
pub enum PassCliError {
    #[error("pass-cli is not installed")]
    NotInstalled,
    #[error("pass-cli is not logged in")]
    LoggedOut,
    #[error("the item has no private key")]
    NoPrivateKey,
    #[error("the key in Proton Pass is protected by a passphrase")]
    Encrypted,
    #[error("pass-cli failed: {0}")]
    Failed(String),
    #[error("pass-cli did not answer in time")]
    Timeout,
    #[error(transparent)]
    Vault(VaultError),
}

impl From<PassCliError> for EngineError {
    fn from(e: PassCliError) -> Self {
        let code = match e {
            PassCliError::NotInstalled => ErrorCode::PassCliMissing,
            PassCliError::LoggedOut => ErrorCode::PassCliLoggedOut,
            PassCliError::NoPrivateKey => ErrorCode::NoPrivateKey,
            PassCliError::Encrypted => ErrorCode::Encrypted,
            PassCliError::Timeout => ErrorCode::PassCliTimeout,
            PassCliError::Failed(_) | PassCliError::Vault(_) => ErrorCode::PassCliFailed,
        };
        Self::failed(code, e)
    }
}

pub trait PassCli: Send + Sync {
    fn status(&self) -> PassCliStatus;
    fn list_ssh_keys(&self) -> Result<Vec<PassKey>, PassCliError>;
    fn private_key(&self, share_id: &str, item_id: &str) -> Result<Zeroizing<String>, PassCliError>;
}

/// Finds `pass-cli` on each use, because it may be installed while SSHCraft runs. `None`: not
/// installed.
pub type LocatePassCli = Arc<dyn Fn() -> Option<Arc<dyn PassCli>> + Send + Sync>;

/// The installed `pass-cli`, looked up on each call.
pub fn locate_installed() -> LocatePassCli {
    Arc::new(|| PassCliProcess::locate().map(|cli| Arc::new(cli) as Arc<dyn PassCli>))
}

pub fn ensure_ready(status: PassCliStatus) -> Result<(), PassCliError> {
    match status {
        PassCliStatus::Ready => Ok(()),
        PassCliStatus::LoggedOut => Err(PassCliError::LoggedOut),
        PassCliStatus::NotInstalled => Err(PassCliError::NotInstalled),
        PassCliStatus::Failed => Err(PassCliError::Failed("status check failed".into())),
    }
}

fn malformed() -> PassCliError {
    PassCliError::Failed("unexpected output".into())
}

/// The share ids of `pass-cli vault list --output json`.
pub fn parse_vault_list(json: &str) -> Result<Vec<String>, PassCliError> {
    let value: Value = serde_json::from_str(json).map_err(|_| malformed())?;
    let vaults = value["vaults"].as_array().ok_or_else(malformed)?;
    Ok(vaults.iter().filter_map(|v| v["share_id"].as_str().map(str::to_owned)).collect())
}

/// Lists the active SSH keys of one vault. Since pass-cli 2.7, `item list` needs a vault.
pub fn item_list_args(share_id: &str) -> Vec<&str> {
    vec!["item", "list", "--share-id", share_id, "--filter-type", "ssh-key", "--filter-state", "active", "--output", "json"]
}

/// The keys of every vault that could be listed, by title; a vault that fails (no rights, for
/// example) is skipped. Only when every vault fails is that an error.
pub fn merge_key_lists(lists: Vec<Result<Vec<PassKey>, PassCliError>>) -> Result<Vec<PassKey>, PassCliError> {
    let mut keys = Vec::new();
    let mut first_error = None;
    let mut any_ok = lists.is_empty();
    for list in lists {
        match list {
            Ok(list) => {
                any_ok = true;
                keys.extend(list);
            }
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    if !any_ok && let Some(e) = first_error {
        return Err(e);
    }
    keys.sort_by_key(|k| k.title.to_lowercase());
    Ok(keys)
}

pub fn parse_key_list(json: &str) -> Result<Vec<PassKey>, PassCliError> {
    let value: Value = serde_json::from_str(json).map_err(|_| malformed())?;
    let items = value["items"].as_array().ok_or_else(malformed)?;
    Ok(items
        .iter()
        .filter(|item| item["item_type"] == "ssh_key")
        // Older pass-cli versions have no state; only an explicit "trashed" is left out.
        .filter(|item| item["state"] != "trashed")
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
    match view.item.content.content.ssh_key.and_then(|k| k.private_key) {
        Some(key) if !key.trim().is_empty() => Ok(key),
        _ => Err(PassCliError::NoPrivateKey),
    }
}

/// Builds the error for a failed `pass-cli` run from its exit code and the first line of
/// stderr. stdout may hold secrets and is never looked at. Only messages that clearly talk
/// about logging in count as "logged out"; everything else is a plain failure.
pub fn classify_failure(code: Option<i32>, stderr: &[u8]) -> PassCliError {
    let first_line = String::from_utf8_lossy(stderr).lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_owned();
    let lower = first_line.to_lowercase();
    let login = ["not logged in", "logged out", "login required", "pass-cli login"];
    if login.iter().any(|m| lower.contains(m)) {
        return PassCliError::LoggedOut;
    }
    let code = code.map_or_else(|| "?".to_owned(), |c| c.to_string());
    PassCliError::Failed(format!("exit code {code}: {first_line}"))
}

/// Runs a command and kills it once `limit` has passed. stdout and stderr are drained on
/// their own threads, so a full pipe cannot stall the wait; the whole call, reading included,
/// stays within `limit` even if a left-over grandchild keeps a pipe open. Partial output that
/// is thrown away is zeroed, because stdout may hold a private key.
pub fn run_with_timeout(mut command: Command, limit: Duration) -> Result<Output, PassCliError> {
    use std::sync::mpsc;
    use std::time::Instant;
    use wait_timeout::ChildExt;

    let started = Instant::now();
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| PassCliError::Failed(e.kind().to_string()))?;
    fn drain(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<Zeroizing<Vec<u8>>> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = Zeroizing::new(Vec::new());
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buffer);
            }
            let _ = tx.send(buffer);
        });
        rx
    }
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let status = match child.wait_timeout(limit) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PassCliError::Timeout);
        }
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PassCliError::Failed(e.kind().to_string()));
        }
    };
    let collect = |rx: mpsc::Receiver<Zeroizing<Vec<u8>>>| {
        let left = limit.saturating_sub(started.elapsed());
        rx.recv_timeout(left).map_err(|_| PassCliError::Timeout)
    };
    let mut stdout = collect(stdout)?;
    let mut stderr = collect(stderr)?;
    Ok(Output { status, stdout: std::mem::take(&mut *stdout), stderr: std::mem::take(&mut *stderr) })
}

/// Looks for `exe_name` in the given `PATH` value. Relative entries are skipped: they would
/// resolve against whatever the current directory happens to be.
pub fn find_on_path(path_var: &OsStr, exe_name: &str) -> Option<PathBuf> {
    std::env::split_paths(path_var).filter(|dir| dir.is_absolute()).map(|dir| dir.join(exe_name)).find(|candidate| candidate.is_file())
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
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    vault.store_key(&store, name, &key).map_err(PassCliError::Vault)
}

/// The real `pass-cli`, run as a hidden child process.
pub struct PassCliProcess {
    exe: PathBuf,
}

impl PassCliProcess {
    pub fn locate() -> Option<Self> {
        let installed = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join(r"Programs\ProtonPass\pass-cli.exe"));
        let exe_name = if cfg!(windows) { "pass-cli.exe" } else { "pass-cli" };
        let on_path = std::env::var_os("PATH").and_then(|paths| find_on_path(&paths, exe_name));
        installed.filter(|p| p.is_file()).or(on_path).map(|exe| Self { exe })
    }

    fn run(&self, args: &[&str], reason: Option<&str>, limit: Duration) -> Result<Zeroizing<Vec<u8>>, PassCliError> {
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
        command.stdin(Stdio::null());
        let output = run_with_timeout(command, limit)?;
        let stdout = Zeroizing::new(output.stdout);
        if output.status.success() { Ok(stdout) } else { Err(classify_failure(output.status.code(), &output.stderr)) }
    }
}

impl PassCli for PassCliProcess {
    fn status(&self) -> PassCliStatus {
        match self.run(&["info"], None, QUICK_LIMIT) {
            Ok(_) => PassCliStatus::Ready,
            Err(PassCliError::LoggedOut) => PassCliStatus::LoggedOut,
            Err(_) => PassCliStatus::Failed,
        }
    }

    /// Every vault's active SSH keys; the vaults are asked at the same time, each call of
    /// pass-cli takes a second or more.
    fn list_ssh_keys(&self) -> Result<Vec<PassKey>, PassCliError> {
        ensure_ready(self.status())?;
        let vaults = self.run(&["vault", "list", "--output", "json"], None, QUICK_LIMIT)?;
        let shares = parse_vault_list(&String::from_utf8_lossy(&vaults))?;
        let lists: Vec<Result<Vec<PassKey>, PassCliError>> = std::thread::scope(|scope| {
            let handles: Vec<_> = shares
                .iter()
                .map(|share| {
                    scope.spawn(move || {
                        let out = self.run(&item_list_args(share), None, QUICK_LIMIT)?;
                        parse_key_list(&String::from_utf8_lossy(&out))
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(malformed()))).collect()
        });
        merge_key_lists(lists)
    }

    fn private_key(&self, share_id: &str, item_id: &str) -> Result<Zeroizing<String>, PassCliError> {
        let uri = format!("pass://{share_id}/{item_id}");
        let out = self.run(&["item", "view", &uri, "--output", "json"], Some(VIEW_REASON), VIEW_LIMIT)?;
        parse_private_key(&out)
    }
}

#[cfg(test)]
#[path = "passcli_tests.rs"]
mod tests;

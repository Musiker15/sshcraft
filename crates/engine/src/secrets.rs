//! Passwords from a password manager (see `sshcraft_model::secret_ref`): fetched when connecting
//! by the manager's own program, with an argument list and never a shell. What it prints is the
//! password; what it writes to standard error (bounded) is the reason when it fails.

use std::time::Duration;

use sshcraft_model::secret_ref::{Manager, SecretRef};
use zeroize::Zeroizing;

/// How long a manager may take (it may wait for the user to unlock or approve it).
const FETCH_TIMEOUT: Duration = Duration::from_secs(120);
/// The most output read from a manager.
const MAX_OUTPUT: usize = 64 * 1024;

/// Fetches a password for a reference.
#[async_trait::async_trait]
pub trait SecretSource: Send + Sync {
    /// `database_password` is set for KeePassXC.
    async fn fetch(&self, reference: &SecretRef, database_password: Option<&str>) -> Result<Zeroizing<String>, String>;
}

/// The managers' command line programs.
pub struct CliSecrets;

/// Where a manager's program is: Proton Pass installs `pass-cli` next to the app on Windows;
/// the others are looked up on `PATH`.
fn program(manager: Manager) -> std::path::PathBuf {
    if manager == Manager::ProtonPass
        && let Some(local) = std::env::var_os("LOCALAPPDATA")
    {
        let installed = std::path::PathBuf::from(local).join(r"Programs\ProtonPass\pass-cli.exe");
        if installed.is_file() {
            return installed;
        }
    }
    std::path::PathBuf::from(manager.program())
}

#[async_trait::async_trait]
impl SecretSource for CliSecrets {
    async fn fetch(&self, reference: &SecretRef, database_password: Option<&str>) -> Result<Zeroizing<String>, String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let args = reference.arguments().map_err(|e| e.to_string())?;
        let program = program(reference.manager);
        let mut command = tokio::process::Command::new(&program);
        command
            .args(&args)
            .stdin(if database_password.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() })
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|e| format!("{} could not start: {e}", reference.manager.program()))?;
        if let (Some(password), Some(mut stdin)) = (database_password, child.stdin.take()) {
            let line = Zeroizing::new(format!("{password}\n"));
            stdin.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
            drop(stdin);
        }
        let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err("no output".into());
        };
        let run = async {
            let mut out = Zeroizing::new(Vec::new());
            let mut err = Vec::new();
            let mut stdout = (&mut stdout).take(u64::try_from(MAX_OUTPUT).unwrap_or(u64::MAX));
            let mut stderr = (&mut stderr).take(4096);
            let (a, b) = tokio::join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err));
            a.map_err(|e| e.to_string())?;
            let _ = b;
            let status = child.wait().await.map_err(|e| e.to_string())?;
            Ok::<_, String>((status, out, err))
        };
        let (status, out, err) =
            tokio::time::timeout(FETCH_TIMEOUT, run).await.map_err(|_| format!("{} did not answer in time", reference.manager.program()))??;
        if !status.success() {
            let said = String::from_utf8_lossy(&err).trim().to_owned();
            return Err(if said.is_empty() { format!("{} failed ({status})", reference.manager.program()) } else { said });
        }
        let text = std::str::from_utf8(&out).map_err(|_| "the password is not text".to_owned())?;
        let password = text.trim_end_matches(['\r', '\n']);
        if password.is_empty() {
            return Err(format!("{} printed no password", reference.manager.program()));
        }
        Ok(Zeroizing::new(password.to_owned()))
    }
}

/// The digest a reference is allowed by on this device: for this manager, reference and the
/// address it goes to, so a host pointed elsewhere asks again.
pub fn approval_digest(reference: &SecretRef, address: &str, port: u16) -> String {
    let manager = serde_json::to_string(&reference.manager).unwrap_or_default();
    sshcraft_store::store::command_digest(&["secretref".into(), manager, reference.reference.clone(), address.to_owned(), port.to_string()])
}

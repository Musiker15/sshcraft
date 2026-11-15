//! `--when-closed launch`: start the app as a double click would (its program, no arguments,
//! never through a shell) and wait for it to let agents connect.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use sshcraft_mcp::{Backend, BackendError, Remote};
use tokio::sync::Mutex;

/// How long a started app has to let agents connect.
pub const LAUNCH_WAIT: Duration = Duration::from_secs(45);

/// The app's program: `SSHCRAFT_APP_PATH`, else `sshcraft` beside this program.
pub fn app_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SSHCRAFT_APP_PATH").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let here = std::env::current_exe().ok()?;
    Some(here.parent()?.join(format!("sshcraft{}", std::env::consts::EXE_SUFFIX)))
}

/// Starts `program` with no arguments, its output going nowhere.
pub fn start(program: &Path) -> Result<(), String> {
    Command::new(program)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not start {}: {e}", program.display()))
}

fn not_allowed() -> BackendError {
    BackendError::Failed("SSHCraft is running but does not let agents connect. Allow agent control in Settings › AI agents, then try again.".into())
}

/// Whether an SSHCraft holds the data folder `dir` (it runs). The lock is let go at once; a
/// folder that is not there yet is not made.
fn app_running(dir: &Path) -> bool {
    dir.is_dir() && matches!(sshcraft_platform::lock_data_dir(dir), Err(sshcraft_platform::PlatformError::DataInUse(_)))
}

/// The app's data folder, to see whether it runs: `SSHCRAFT_DATA_DIR` (tests), else the app's own.
pub fn data_dir() -> Option<PathBuf> {
    match std::env::var_os("SSHCRAFT_DATA_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => sshcraft_platform::data_dir().ok(),
    }
}

/// The app, started on the first call that finds it closed.
pub struct Launching {
    remote: Remote,
    program: Option<PathBuf>,
    /// The app's data folder: held by a running app.
    data_dir: Option<PathBuf>,
    /// Started once; a second call waits for the same start.
    started: Mutex<bool>,
    wait: Duration,
}

impl Launching {
    pub fn new(remote: Remote, program: Option<PathBuf>, data_dir: Option<PathBuf>, wait: Duration) -> Self {
        Self { remote, program, data_dir, started: Mutex::new(false), wait }
    }

    async fn ensure(&self, agent: &str) -> Result<(), BackendError> {
        if self.remote.reachable(agent).await {
            return Ok(());
        }
        let mut started = self.started.lock().await;
        if !*started && self.data_dir.as_deref().is_some_and(app_running) {
            // It runs, it only does not let agents in: a second start would only show that it is
            // already open.
            return Err(not_allowed());
        }
        if !*started {
            let program = self.program.as_deref().ok_or_else(|| BackendError::Failed("cannot find the SSHCraft app".into()))?;
            start(program).map_err(BackendError::Failed)?;
            *started = true;
        }
        let deadline = Instant::now() + self.wait;
        while Instant::now() < deadline {
            if self.remote.reachable(agent).await {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(not_allowed())
    }
}

#[async_trait::async_trait]
impl Backend for Launching {
    async fn execute(&self, agent: &str, command: &str, params: Value) -> Result<Value, BackendError> {
        self.ensure(agent).await?;
        self.remote.execute(agent, command, params).await
    }

    async fn commands(&self, agent: &str) -> Result<Value, BackendError> {
        self.ensure(agent).await?;
        self.remote.commands(agent).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    /// SSHCraft runs but does not let agents connect: nothing else is started, the agent learns why.
    #[tokio::test]
    async fn a_running_app_is_not_started_again() {
        let data = tempfile::tempdir().unwrap();
        let _running = sshcraft_platform::lock_data_dir(data.path()).unwrap();
        let link = tempfile::tempdir().unwrap();
        let launching = Launching::new(
            Remote::window(link.path().to_path_buf(), "mcp"),
            Some(PathBuf::from("no-such-program")),
            Some(data.path().to_path_buf()),
            Duration::from_millis(100),
        );
        match launching.ensure("claude").await {
            Err(BackendError::Failed(message)) => assert!(message.contains("does not let agents connect"), "{message}"),
            other => panic!("{other:?}"),
        }
    }
}

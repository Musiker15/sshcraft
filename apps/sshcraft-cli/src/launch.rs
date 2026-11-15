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

/// The app, started on the first call that finds it closed.
pub struct Launching {
    remote: Remote,
    program: Option<PathBuf>,
    /// Started once; a second call waits for the same start.
    started: Mutex<bool>,
    wait: Duration,
}

impl Launching {
    pub fn new(remote: Remote, program: Option<PathBuf>, wait: Duration) -> Self {
        Self { remote, program, started: Mutex::new(false), wait }
    }

    async fn ensure(&self, agent: &str) -> Result<(), BackendError> {
        if self.remote.reachable(agent).await {
            return Ok(());
        }
        let mut started = self.started.lock().await;
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
        Err(BackendError::Failed(
            "SSHCraft started but does not let agents connect. Allow agent control in Settings › AI agents, then try again.".into(),
        ))
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

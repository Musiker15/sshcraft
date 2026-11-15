//! Where tool calls go: the running app. `Remote` finds it through `agent-connection.json` (or a
//! `--control` app through `--bridge`), signs in, says hello as the agent, and keeps that one
//! connection: the sessions an agent opens live as long as it.

use std::net::SocketAddr;
use std::path::PathBuf;

use serde_json::{Value, json};
use sshcraft_link::{Client, ConnectionFile, LinkError, Token};

/// Why a call did not reach the app or did not succeed there.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    #[error("SSHCraft is not running. Start the SSHCraft app (and allow agent control in Settings › AI agents), then try again.")]
    NotRunning,
    #[error("{0}")]
    Failed(String),
}

impl From<LinkError> for BackendError {
    fn from(e: LinkError) -> Self {
        match e {
            LinkError::NotRunning => Self::NotRunning,
            LinkError::Remote(message) => Self::Failed(message),
            other => Self::Failed(other.to_string()),
        }
    }
}

/// Runs engine commands for the MCP server.
#[async_trait::async_trait]
pub trait Backend: Send + Sync + 'static {
    /// Runs `command` with `params` as agent `agent`.
    async fn execute(&self, agent: &str, command: &str, params: Value) -> Result<Value, BackendError>;
    /// The commands agents may use.
    async fn commands(&self, agent: &str) -> Result<Value, BackendError>;
}

enum Target {
    /// The open window, through the connection file in this folder.
    Window { dir: PathBuf, via: &'static str },
    /// A `--control` app: its port and token (no hello; it calls the engine as the control
    /// channel).
    Bridge { addr: SocketAddr, token: Token },
}

/// The running app.
pub struct Remote {
    target: Target,
    client: tokio::sync::Mutex<Option<Client>>,
}

impl Remote {
    /// The app whose connection file is in `dir`; calls go as an agent reached `via` (`mcp`,
    /// `cli`).
    pub fn window(dir: PathBuf, via: &'static str) -> Self {
        Self { target: Target::Window { dir, via }, client: tokio::sync::Mutex::new(None) }
    }

    /// A `--control` app on `addr` with `token`.
    pub fn bridge(addr: SocketAddr, token: Token) -> Self {
        Self { target: Target::Bridge { addr, token }, client: tokio::sync::Mutex::new(None) }
    }

    /// Whether the app answers (connects once if needed).
    pub async fn reachable(&self, agent: &str) -> bool {
        self.client(agent).await.is_ok()
    }

    async fn client(&self, agent: &str) -> Result<Client, BackendError> {
        let mut slot = self.client.lock().await;
        if let Some(client) = slot.as_ref().filter(|c| c.is_alive()) {
            return Ok(client.clone());
        }
        let client = match &self.target {
            Target::Window { dir, via } => {
                let file = ConnectionFile::read(dir).ok_or(BackendError::NotRunning)?;
                let token = Token::load(&file.token_file).ok_or(BackendError::NotRunning)?;
                let client = Client::connect(SocketAddr::from(([127, 0, 0, 1], file.port)), &token).await.map_err(|e| match e {
                    LinkError::Refused(_) => BackendError::NotRunning,
                    other => other.into(),
                })?;
                client.call("hello", json!({ "agent": agent, "via": via })).await?;
                client
            }
            Target::Bridge { addr, token } => Client::connect(*addr, token).await?,
        };
        *slot = Some(client.clone());
        Ok(client)
    }
}

#[async_trait::async_trait]
impl Backend for Remote {
    async fn execute(&self, agent: &str, command: &str, params: Value) -> Result<Value, BackendError> {
        let client = self.client(agent).await?;
        Ok(client.call("engine.execute", json!({ "command": command, "params": params })).await?)
    }

    async fn commands(&self, agent: &str) -> Result<Value, BackendError> {
        let client = self.client(agent).await?;
        Ok(client.call("engine.commands", json!({})).await?)
    }
}

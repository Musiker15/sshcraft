//! Settings for `sshcraft-server serve`.

use sshcraft_proto::identity::normalize_origin;
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub listen: SocketAddr,
    pub database_url: String,
    /// The address clients use to reach this server, e.g. `https://sync.example.org`.
    /// Login signatures are bound to it, so it must match what clients type in.
    pub public_origin: String,
    /// How often an open WebSocket is pinged and its session checked again.
    pub ws_recheck: Duration,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 8470)),
            database_url: "sqlite://sshcraft.db".to_owned(),
            public_origin: String::new(),
            ws_recheck: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("the public origin must be an http:// or https:// address with a host, got {0:?}")]
    BadOrigin(String),
}

impl ServeConfig {
    /// Checks the settings and returns the public origin in normal form.
    pub fn validate(&self) -> Result<String, ConfigError> {
        let bad = || ConfigError::BadOrigin(self.public_origin.clone());
        let origin = normalize_origin(&self.public_origin);
        let host = origin
            .strip_prefix("https://")
            .or_else(|| origin.strip_prefix("http://"))
            .ok_or_else(bad)?;
        if host.is_empty() || host.contains(['/', '@', ' ']) {
            return Err(bad());
        }
        Ok(origin)
    }
}

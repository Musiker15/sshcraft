//! The Windows SSH agent pipe, which the Proton Pass desktop app serves.

use std::time::Duration;

use russh::keys::agent::client::AgentClient;

use crate::{AgentOpener, DynAgent, Io, SshError};

pub const WINDOWS_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn open_windows_agent() -> Result<DynAgent, SshError> {
    let client = tokio::time::timeout(
        CONNECT_TIMEOUT,
        AgentClient::connect_named_pipe(WINDOWS_AGENT_PIPE),
    )
    .await
    .map_err(|_| SshError::AgentUnavailable("timed out".into()))?
    .map_err(|e| SshError::AgentUnavailable(e.to_string()))?;
    Ok(client.dynamic())
}

/// Opens the agent pipe for forwarding: a raw stream, one per forwarded channel.
pub struct WindowsAgentOpener;

#[async_trait::async_trait]
impl AgentOpener for WindowsAgentOpener {
    async fn open(&self) -> Result<Box<dyn Io>, SshError> {
        let open = async {
            tokio::net::windows::named_pipe::ClientOptions::new().open(WINDOWS_AGENT_PIPE)
        };
        let pipe = tokio::time::timeout(CONNECT_TIMEOUT, open)
            .await
            .map_err(|_| SshError::AgentUnavailable("timed out".into()))?
            .map_err(|e| SshError::AgentUnavailable(e.to_string()))?;
        Ok(Box::new(pipe))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pipe_name_is_the_openssh_agent_pipe() {
        assert_eq!(super::WINDOWS_AGENT_PIPE.as_bytes()[..4], *b"\\\\.\\");
        assert!(super::WINDOWS_AGENT_PIPE.ends_with("\\pipe\\openssh-ssh-agent"));
    }
}

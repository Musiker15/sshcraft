//! The Windows SSH agent pipe, which the Proton Pass desktop app serves.

use std::time::Duration;

use russh::keys::agent::client::AgentClient;

use crate::{DynAgent, SshError};

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

#[cfg(test)]
mod tests {
    #[test]
    fn pipe_name_is_the_openssh_agent_pipe() {
        assert_eq!(super::WINDOWS_AGENT_PIPE.as_bytes()[..4], *b"\\\\.\\");
        assert!(super::WINDOWS_AGENT_PIPE.ends_with("\\pipe\\openssh-ssh-agent"));
    }
}

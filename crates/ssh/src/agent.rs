//! The SSH agent: on Windows the OpenSSH agent pipe, which the Proton Pass desktop app serves;
//! on Linux, FreeBSD and macOS the socket that `SSH_AUTH_SOCK` names (ssh-agent, gpg-agent,
//! the desktop's keyring, Proton Pass or 1Password).

use std::time::Duration;

use russh::keys::agent::client::AgentClient;

use crate::{AgentOpener, DynAgent, Io, SshError};

#[cfg(windows)]
pub const WINDOWS_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(windows)]
pub async fn open_windows_agent() -> Result<DynAgent, SshError> {
    let client = tokio::time::timeout(CONNECT_TIMEOUT, AgentClient::connect_named_pipe(WINDOWS_AGENT_PIPE))
        .await
        .map_err(|_| SshError::AgentUnavailable("timed out".into()))?
        .map_err(|e| SshError::AgentUnavailable(e.to_string()))?;
    Ok(client.dynamic())
}

/// Opens the agent pipe for forwarding: a raw stream, one per forwarded channel.
#[cfg(windows)]
pub struct WindowsAgentOpener;

#[cfg(windows)]
#[async_trait::async_trait]
impl AgentOpener for WindowsAgentOpener {
    async fn open(&self) -> Result<Box<dyn Io>, SshError> {
        let open = async { tokio::net::windows::named_pipe::ClientOptions::new().open(WINDOWS_AGENT_PIPE) };
        let pipe = tokio::time::timeout(CONNECT_TIMEOUT, open)
            .await
            .map_err(|_| SshError::AgentUnavailable("timed out".into()))?
            .map_err(|e| SshError::AgentUnavailable(e.to_string()))?;
        Ok(Box::new(pipe))
    }
}

/// The variable that names the agent's socket.
#[cfg(unix)]
pub const AUTH_SOCK: &str = "SSH_AUTH_SOCK";

/// The socket a value of `SSH_AUTH_SOCK` names; unset or empty names none.
#[cfg(unix)]
pub fn socket_from(value: Option<std::ffi::OsString>) -> Option<std::path::PathBuf> {
    value.filter(|v| !v.is_empty()).map(std::path::PathBuf::from)
}

/// The socket of this process's `SSH_AUTH_SOCK`.
#[cfg(unix)]
pub fn env_socket() -> Option<std::path::PathBuf> {
    socket_from(std::env::var_os(AUTH_SOCK))
}

#[cfg(unix)]
async fn connect_socket(path: &std::path::Path) -> Result<tokio::net::UnixStream, SshError> {
    tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::UnixStream::connect(path))
        .await
        .map_err(|_| SshError::AgentUnavailable("timed out".into()))?
        .map_err(|e| SshError::AgentUnavailable(e.to_string()))
}

/// The agent listening on the socket `path`.
#[cfg(unix)]
pub async fn open_unix_agent(path: &std::path::Path) -> Result<DynAgent, SshError> {
    Ok(AgentClient::connect(connect_socket(path).await?).dynamic())
}

/// Opens the agent's socket for forwarding: a raw stream, one per forwarded channel.
#[cfg(unix)]
pub struct UnixAgentOpener {
    path: std::path::PathBuf,
}

#[cfg(unix)]
impl UnixAgentOpener {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

#[cfg(unix)]
#[async_trait::async_trait]
impl AgentOpener for UnixAgentOpener {
    async fn open(&self) -> Result<Box<dyn Io>, SshError> {
        Ok(Box::new(connect_socket(&self.path).await?))
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn pipe_name_is_the_openssh_agent_pipe() {
        assert_eq!(super::WINDOWS_AGENT_PIPE.as_bytes()[..4], *b"\\\\.\\");
        assert!(super::WINDOWS_AGENT_PIPE.ends_with("\\pipe\\openssh-ssh-agent"));
    }
}

//! SSH session engine for Stollen: connect with a private key, verify the server's host key
//! through the caller, run an interactive shell and stream its raw output bytes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use russh::client::{self, Msg};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, ChannelWriteHalf};
use stollen_core::policy::Disconnect;
use tokio::sync::mpsc;

#[cfg(feature = "testing")]
pub mod testing;

const TERM: &str = "xterm-256color";
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
const KEEPALIVE_MAX: usize = 3;
const EVENT_BUFFER: usize = 1024;

pub struct ConnectParams {
    pub address: String,
    pub port: u16,
    pub username: String,
    pub key: PrivateKey,
}

/// Decides whether to trust the server's host key. Fingerprints look like `SHA256:…`.
#[async_trait::async_trait]
pub trait HostKeyVerifier: Send + Sync {
    async fn verify(&self, address: &str, port: u16, fingerprint: &str) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SshError {
    #[error("the server's host key was rejected")]
    HostKeyRejected,
    #[error("the server rejected the key")]
    AuthFailed,
    #[error("could not connect: {0}")]
    Connect(String),
    #[error("ssh protocol error: {0}")]
    Protocol(String),
}

impl From<&SshError> for Disconnect {
    fn from(e: &SshError) -> Self {
        match e {
            SshError::HostKeyRejected => Disconnect::HostKeyRejected,
            SshError::AuthFailed => Disconnect::AuthFailed,
            SshError::Connect(m) | SshError::Protocol(m) => Disconnect::Lost(m.clone()),
        }
    }
}

fn protocol(e: impl std::fmt::Display) -> SshError {
    SshError::Protocol(e.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    /// Raw bytes from the remote terminal, not decoded.
    Data(Vec<u8>),
    Ended(Disconnect),
}

struct ClientHandler {
    address: String,
    port: u16,
    verifier: Arc<dyn HostKeyVerifier>,
    rejected: Arc<AtomicBool>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key_data = match server_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.key_data(),
            PublicKeyOrCertificate::Certificate(cert) => cert.public_key(),
        };
        let fingerprint = key_data.fingerprint(HashAlg::Sha256).to_string();
        let trusted = self
            .verifier
            .verify(&self.address, self.port, &fingerprint)
            .await;
        if !trusted {
            self.rejected.store(true, Ordering::SeqCst);
        }
        Ok(trusted)
    }
}

/// An authenticated SSH connection. Shells opened from it keep it alive.
pub struct Session {
    handle: Arc<client::Handle<ClientHandler>>,
}

pub struct Shell {
    pub events: mpsc::Receiver<ShellEvent>,
    writer: ShellWriter,
}

/// The sending half of a shell, separate from its event stream.
#[derive(Clone)]
pub struct ShellWriter {
    channel: Arc<ChannelWriteHalf<Msg>>,
    _session: Arc<client::Handle<ClientHandler>>,
}

pub async fn connect(
    params: ConnectParams,
    verifier: Arc<dyn HostKeyVerifier>,
) -> Result<Session, SshError> {
    let config = Arc::new(client::Config {
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        keepalive_max: KEEPALIVE_MAX,
        ..Default::default()
    });
    let rejected = Arc::new(AtomicBool::new(false));
    let handler = ClientHandler {
        address: params.address.clone(),
        port: params.port,
        verifier,
        rejected: rejected.clone(),
    };
    let mut handle = client::connect(config, (params.address.as_str(), params.port), handler)
        .await
        .map_err(|e| {
            if rejected.load(Ordering::SeqCst) {
                SshError::HostKeyRejected
            } else {
                SshError::Connect(e.to_string())
            }
        })?;
    let hash = handle
        .best_supported_rsa_hash()
        .await
        .map_err(protocol)?
        .flatten();
    let auth = handle
        .authenticate_publickey(
            params.username,
            PrivateKeyWithHashAlg::new(Arc::new(params.key), hash),
        )
        .await
        .map_err(protocol)?;
    if !auth.success() {
        return Err(SshError::AuthFailed);
    }
    Ok(Session {
        handle: Arc::new(handle),
    })
}

impl Session {
    pub async fn open_shell(&self, cols: u32, rows: u32) -> Result<Shell, SshError> {
        let channel = self.handle.channel_open_session().await.map_err(protocol)?;
        channel
            .request_pty(false, TERM, cols, rows, 0, 0, &[])
            .await
            .map_err(protocol)?;
        channel.request_shell(false).await.map_err(protocol)?;
        let (mut read, write) = channel.split();
        let (tx, rx) = mpsc::channel(EVENT_BUFFER);

        tokio::spawn(async move {
            let mut exit_status = None;
            let mut closed = false;
            while let Some(msg) = read.wait().await {
                match msg {
                    ChannelMsg::Data { data } => {
                        if tx.send(ShellEvent::Data(data.to_vec())).await.is_err() {
                            return;
                        }
                    }
                    ChannelMsg::ExitStatus { exit_status: code } => exit_status = Some(code),
                    ChannelMsg::Close => {
                        closed = true;
                        break;
                    }
                    _ => {}
                }
            }
            let reason = if exit_status.is_some() || closed {
                Disconnect::Exited(exit_status)
            } else {
                Disconnect::Lost("connection closed".into())
            };
            let _ = tx.send(ShellEvent::Ended(reason)).await;
        });

        Ok(Shell {
            events: rx,
            writer: ShellWriter {
                channel: Arc::new(write),
                _session: self.handle.clone(),
            },
        })
    }

    pub async fn disconnect(self) {
        let _ = self
            .handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
    }
}

impl Shell {
    pub async fn write(&self, data: &[u8]) -> Result<(), SshError> {
        self.writer.write(data).await
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> Result<(), SshError> {
        self.writer.resize(cols, rows).await
    }

    pub fn split(self) -> (ShellWriter, mpsc::Receiver<ShellEvent>) {
        (self.writer, self.events)
    }
}

impl ShellWriter {
    pub async fn write(&self, data: &[u8]) -> Result<(), SshError> {
        self.channel.data(data).await.map_err(protocol)
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> Result<(), SshError> {
        self.channel
            .window_change(cols, rows, 0, 0)
            .await
            .map_err(protocol)
    }
}

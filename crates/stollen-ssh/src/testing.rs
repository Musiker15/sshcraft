//! An in-process SSH server for tests. Accepts exactly one client key.
//!
//! The shell echoes every input chunk, except two commands sent as their own chunk:
//! `exit\n` ends the shell with exit status 0, `flood\n` sends [`flood_pattern`].

use std::sync::Arc;

use russh::keys::ssh_encoding::bytes::Bytes;
use russh::keys::{Algorithm, HashAlg, PrivateKey, PublicKey};
use russh::server::{self, Auth, Msg, Server as _, Session, VerifiedPublicKey};
use russh::{Channel, ChannelId};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const FLOOD_LEN: usize = 5 * 1024 * 1024;
const FLOOD_CHUNK: usize = 32 * 1024;

/// 5 MiB of bytes 0..=255 interleaved with multi-byte UTF-8, so chunk borders split characters.
pub fn flood_pattern() -> Vec<u8> {
    let mut block: Vec<u8> = (0..=255u8).collect();
    block.extend_from_slice("äöü🙂".as_bytes());
    block.iter().copied().cycle().take(FLOOD_LEN).collect()
}

pub struct TestServer {
    pub port: u16,
    pub host_fingerprint: String,
    authorized: PublicKey,
    host_key: PrivateKey,
    running: Option<(server::RunningServerHandle, JoinHandle<()>)>,
}

impl TestServer {
    pub async fn start(authorized: PublicKey) -> TestServer {
        let host_key = random_host_key();
        let mut server = TestServer {
            port: 0,
            host_fingerprint: fingerprint(&host_key),
            authorized,
            host_key,
            running: None,
        };
        server.listen().await;
        server
    }

    /// Drops every connection and stops listening.
    pub fn stop(&mut self) {
        let _ = self.stop_task();
    }

    fn stop_task(&mut self) -> Option<JoinHandle<()>> {
        let (handle, task) = self.running.take()?;
        handle.shutdown("test server stopped".into());
        task.abort();
        Some(task)
    }

    /// Stops and listens again on the same port, optionally with a new host key.
    pub async fn restart(&mut self, new_host_key: bool) {
        if let Some(task) = self.stop_task() {
            // The listener is only released once the aborted task has been dropped.
            let _ = task.await;
        }
        if new_host_key {
            self.host_key = random_host_key();
            self.host_fingerprint = fingerprint(&self.host_key);
        }
        self.listen().await;
    }

    async fn listen(&mut self) {
        let socket = TcpListener::bind(("127.0.0.1", self.port))
            .await
            .expect("bind test server");
        self.port = socket.local_addr().expect("local addr").port();
        let config = Arc::new(server::Config {
            keys: vec![self.host_key.clone()],
            auth_rejection_time: std::time::Duration::from_millis(10),
            auth_rejection_time_initial: Some(std::time::Duration::ZERO),
            ..Default::default()
        });
        let authorized = self.authorized.clone();
        let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut factory = Factory { authorized };
            let running = factory.run_on_socket(config, &socket);
            let _ = handle_tx.send(running.handle());
            let _ = running.await;
        });
        let handle = handle_rx.await.expect("test server handle");
        self.running = Some((handle, task));
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn random_host_key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("host key")
}

fn fingerprint(key: &PrivateKey) -> String {
    key.public_key().fingerprint(HashAlg::Sha256).to_string()
}

#[derive(Clone)]
struct Factory {
    authorized: PublicKey,
}

impl server::Server for Factory {
    type Handler = Handler;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Handler {
        Handler {
            authorized: self.authorized.clone(),
        }
    }
}

struct Handler {
    authorized: PublicKey,
}

impl server::Handler for Handler {
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self,
        _user: &str,
        key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        if key.public_key.key_data() == self.authorized.key_data() {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _cols: u32,
        _rows: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        _cols: u32,
        _rows: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let handle = session.handle();
        match data {
            b"exit\n" => {
                tokio::spawn(async move {
                    let _ = handle.exit_status_request(channel, 0).await;
                    let _ = handle.eof(channel).await;
                    let _ = handle.close(channel).await;
                });
            }
            b"flood\n" => {
                tokio::spawn(async move {
                    for chunk in flood_pattern().chunks(FLOOD_CHUNK) {
                        if handle
                            .data(channel, Bytes::from(chunk.to_vec()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                });
            }
            other => session.data(channel, other.to_vec())?,
        }
        Ok(())
    }
}

/// An in-memory SSH agent holding `keys`. With `confirm == false` it refuses every signature.
pub async fn spawn_agent(keys: &[PrivateKey], confirm: bool) -> crate::DynAgent {
    use russh::keys::agent::client::AgentClient;

    let (client_side, server_side) = tokio::io::duplex(64 * 1024);
    let listener = futures::stream::iter(vec![Ok::<_, std::io::Error>(server_side)]);
    tokio::spawn(async move {
        let _ = russh::keys::agent::server::serve(listener, TestAgent { confirm }).await;
    });
    let mut client = AgentClient::connect(client_side).dynamic();
    for key in keys {
        client.add_identity(key, &[]).await.expect("add identity");
    }
    client
}

#[derive(Clone)]
struct TestAgent {
    confirm: bool,
}

impl russh::keys::agent::server::Agent for TestAgent {
    fn confirm_request(
        &self,
        msg: russh::keys::agent::server::MessageType,
    ) -> impl std::future::Future<Output = bool> + Send {
        let allowed = self.confirm || !matches!(msg, russh::keys::agent::server::MessageType::Sign);
        async move { allowed }
    }
}

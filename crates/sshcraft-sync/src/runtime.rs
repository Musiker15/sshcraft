//! One background task per server: sync once, then keep a WebSocket open and sync again on
//! every change notice, every local change (`nudge`) and every few minutes. Reconnects with a
//! growing pause when the server is away; stops for good when this device was revoked.

use crate::SyncError;
use crate::engine::Engine;
use futures::StreamExt;
use sshcraft_core::model::ServerId;
use sshcraft_core::store::Store;
use sshcraft_core::vault::Vault;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// A full sync runs at least this often, even without notices.
pub const PERIODIC: Duration = Duration::from_secs(300);
/// Longest pause between reconnect attempts.
pub const MAX_BACKOFF_SECS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    Idle,
    Syncing,
    Offline { retry_in_secs: u64 },
    Revoked,
    Error(String),
}

pub trait SyncObserver: Send + Sync {
    fn status(&self, server: ServerId, status: SyncStatus);
    fn data_changed(&self, server: ServerId);
    fn warning(&self, server: ServerId, message: String);
}

pub struct SyncRuntime {
    nudge: Arc<Notify>,
    task: JoinHandle<()>,
}

impl SyncRuntime {
    pub fn start(
        store: Arc<Mutex<Store>>,
        vault: Arc<Vault>,
        server: ServerId,
        observer: Arc<dyn SyncObserver>,
    ) -> SyncRuntime {
        let nudge = Arc::new(Notify::new());
        let worker = Worker {
            store,
            vault,
            server,
            observer,
            nudge: nudge.clone(),
        };
        let task = tokio::spawn(worker.run());
        SyncRuntime { nudge, task }
    }

    /// A local change is waiting: sync now (several nudges in a row make one run).
    pub fn nudge(&self) {
        self.nudge.notify_one();
    }

    pub fn stop(self) {
        self.task.abort();
    }
}

impl Drop for SyncRuntime {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Worker {
    store: Arc<Mutex<Store>>,
    vault: Arc<Vault>,
    server: ServerId,
    observer: Arc<dyn SyncObserver>,
    nudge: Arc<Notify>,
}

/// What ended a connected phase.
enum Ended {
    Revoked,
    Lost,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

impl Worker {
    async fn run(self) {
        let mut backoff = 1;
        loop {
            match self.connected().await {
                Ended::Revoked => {
                    self.observer.status(self.server, SyncStatus::Revoked);
                    return;
                }
                Ended::Lost => {
                    self.observer.status(
                        self.server,
                        SyncStatus::Offline {
                            retry_in_secs: backoff,
                        },
                    );
                    // A local change does not shorten the pause; the outbox keeps it.
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
                }
            }
        }
    }

    /// Logs in, syncs, then follows the event stream until it breaks.
    async fn connected(&self) -> Ended {
        let mut engine = match Engine::load(&self.store, &self.vault, self.server).await {
            Ok(engine) => engine,
            Err(SyncError::Revoked) => return Ended::Revoked,
            Err(SyncError::Network(_)) => return Ended::Lost,
            Err(err) => {
                self.observer
                    .status(self.server, SyncStatus::Error(err.to_string()));
                return Ended::Lost;
            }
        };
        if let Err(ended) = self.pass(&mut engine).await {
            return ended;
        }
        let mut socket = match self.open_events(&engine).await {
            Some(socket) => socket,
            None => return Ended::Lost,
        };
        loop {
            tokio::select! {
                message = socket.next() => match message {
                    Some(Ok(Message::Text(text))) => {
                        if text.contains("\"session_ended\"") {
                            // The next login tells whether this device was revoked.
                            return Ended::Lost;
                        }
                        if let Err(ended) = self.pass(&mut engine).await {
                            return ended;
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Ended::Lost,
                    Some(Ok(_)) => {}
                },
                _ = self.nudge.notified() => {
                    if let Err(ended) = self.pass(&mut engine).await {
                        return ended;
                    }
                }
                _ = tokio::time::sleep(PERIODIC) => {
                    if let Err(ended) = self.pass(&mut engine).await {
                        return ended;
                    }
                }
            }
        }
    }

    /// One pull and one push; reports changes and rejected items.
    async fn pass(&self, engine: &mut Engine) -> Result<(), Ended> {
        self.observer.status(self.server, SyncStatus::Syncing);
        let outcome = async {
            let report = engine.pull(&self.store, &self.vault).await?;
            for message in &report.rejected {
                self.observer.warning(self.server, message.clone());
            }
            if report.applied > 0 {
                self.observer.data_changed(self.server);
            }
            engine.push(&self.store, &self.vault, now_ms()).await?;
            Ok::<(), SyncError>(())
        }
        .await;
        match outcome {
            Ok(()) => {
                self.observer.status(self.server, SyncStatus::Idle);
                Ok(())
            }
            Err(SyncError::Revoked) => Err(Ended::Revoked),
            Err(SyncError::Network(_)) => Err(Ended::Lost),
            Err(SyncError::Server { code, .. }) if code == "unauthorized" => Err(Ended::Lost),
            Err(err) => {
                self.observer
                    .status(self.server, SyncStatus::Error(err.to_string()));
                Ok(())
            }
        }
    }

    async fn open_events(
        &self,
        engine: &Engine,
    ) -> Option<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    > {
        let mut request = engine.client().events_url().into_client_request().ok()?;
        let token = engine.client().token()?;
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().ok()?);
        tokio_tungstenite::connect_async(request)
            .await
            .ok()
            .map(|(socket, _)| socket)
    }
}

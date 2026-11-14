//! One background task per server: sync once, then keep a WebSocket open and sync again on
//! every change notice, every local change (`nudge`) and every few minutes. Reconnects with a
//! growing pause when the server is away; stops for good when this device was revoked.

use crate::SyncError;
use crate::engine::Engine;
use futures::StreamExt;
use sshcraft_model::model::ServerId;
use sshcraft_store::store::Store;
use sshcraft_store::vault::Vault;
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
    Offline {
        retry_in_secs: u64,
    },
    Revoked,
    /// Blocked by the server's operator; retried, the block may be lifted.
    Blocked,
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
    /// Connection lost or not possible; try again after a pause.
    Lost,
    /// The operator blocked this device; try again after a pause, it may be lifted.
    Blocked,
    /// Something else failed (a locked account, a broken vault); shown as an error, retried.
    Failed(String),
}

/// The pause before the next connection attempt: 1, 2, 4 ... up to `MAX_BACKOFF_SECS`, and back
/// to 1 once a connection worked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backoff(u64);

impl Default for Backoff {
    fn default() -> Self {
        Self(1)
    }
}

impl Backoff {
    /// The pause to wait now; the next one is twice as long.
    pub fn pause(&mut self) -> u64 {
        let now = self.0;
        self.0 = (self.0 * 2).min(MAX_BACKOFF_SECS);
        now
    }

    pub fn reset(&mut self) {
        self.0 = 1;
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

impl Worker {
    async fn run(self) {
        let mut backoff = Backoff::default();
        loop {
            let ended = self.connected(&mut backoff).await;
            let pause = backoff.pause();
            match ended {
                Ended::Revoked => {
                    self.observer.status(self.server, SyncStatus::Revoked);
                    return;
                }
                Ended::Lost => self.observer.status(
                    self.server,
                    SyncStatus::Offline {
                        retry_in_secs: pause,
                    },
                ),
                Ended::Blocked => self.observer.status(self.server, SyncStatus::Blocked),
                Ended::Failed(detail) => {
                    self.observer.status(self.server, SyncStatus::Error(detail))
                }
            }
            // A local change does not shorten the pause; the outbox keeps it.
            tokio::time::sleep(Duration::from_secs(pause)).await;
        }
    }

    fn ended(err: SyncError) -> Ended {
        match err {
            SyncError::Revoked => Ended::Revoked,
            SyncError::Blocked => Ended::Blocked,
            SyncError::Network(_) => Ended::Lost,
            SyncError::Server { code, .. } if code == "unauthorized" => Ended::Lost,
            other => Ended::Failed(other.to_string()),
        }
    }

    /// Logs in, syncs, then follows the event stream until it breaks.
    async fn connected(&self, backoff: &mut Backoff) -> Ended {
        let mut engine = match Engine::load(&self.store, &self.vault, self.server).await {
            Ok(engine) => engine,
            Err(err) => return Self::ended(err),
        };
        if let Err(ended) = self.pass(&mut engine).await {
            return ended;
        }
        let Some(mut socket) = self.open_events(&engine).await else {
            return Ended::Lost;
        };
        // Connected and in step: the next outage starts with a short pause again.
        backoff.reset();
        let mut next_periodic = tokio::time::Instant::now() + PERIODIC;
        loop {
            let run = tokio::select! {
                message = socket.next() => match message {
                    Some(Ok(Message::Text(text))) => {
                        if text.contains("\"session_ended\"") {
                            // The next login tells whether this device was revoked or blocked.
                            return Ended::Lost;
                        }
                        true
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Ended::Lost,
                    Some(Ok(_)) => false,
                },
                _ = self.nudge.notified() => true,
                _ = tokio::time::sleep_until(next_periodic) => true,
            };
            if run {
                if let Err(ended) = self.pass(&mut engine).await {
                    return ended;
                }
                // Pings do not count: the periodic run is measured from the last real one.
                next_periodic = tokio::time::Instant::now() + PERIODIC;
            }
        }
    }

    /// One pull and one push; reports changes and rejected items.
    async fn pass(&self, engine: &mut Engine) -> Result<(), Ended> {
        let _turn = crate::engine::server_lock(self.server).lock_owned().await;
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
            Err(err) => match Self::ended(err) {
                Ended::Failed(detail) => {
                    // Shown, but the connection stays; the next notice or nudge tries again.
                    self.observer.status(self.server, SyncStatus::Error(detail));
                    Ok(())
                }
                ended => Err(ended),
            },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_to_the_limit_and_resets() {
        let mut b = Backoff::default();
        let waits: Vec<u64> = (0..8).map(|_| b.pause()).collect();
        assert_eq!(waits, vec![1, 2, 4, 8, 16, 32, 60, 60]);
        b.reset();
        assert_eq!(b.pause(), 1);
    }
}

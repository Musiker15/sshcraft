//! Requests to the running app over the local connection. Several may be under way at once; a
//! request whose caller stops waiting is cancelled in the app (its question goes away).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use sshcraft_agent::framing::{read_message, write_message};
use sshcraft_agent::transport::ClientStream;
use sshcraft_agent::{Action, AgentError, ErrorKind, Request, Response};
use tokio::io::{BufReader, WriteHalf};
use tokio::sync::oneshot;

/// Sends actions somewhere and gets their results (the app, or a fake in tests).
#[async_trait::async_trait]
pub trait Caller: Send + Sync + 'static {
    async fn call(&self, agent: &str, action: Action) -> Result<Value, AgentError>;
}

pub fn not_running() -> AgentError {
    AgentError::new(
        ErrorKind::NotRunning,
        "SSHCraft is not running. Start the SSHCraft app and try again.",
    )
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Response>>>>;

struct Conn {
    write: tokio::sync::Mutex<WriteHalf<ClientStream>>,
    pending: Pending,
    alive: Arc<AtomicBool>,
}

/// A connection to the app, opened on first use and again after it broke.
#[derive(Default)]
pub struct AppClient {
    conn: tokio::sync::Mutex<Option<Arc<Conn>>>,
    next_id: AtomicU64,
}

impl AppClient {
    async fn connection(&self) -> Result<Arc<Conn>, AgentError> {
        let mut slot = self.conn.lock().await;
        if let Some(conn) = slot.as_ref().filter(|c| c.alive.load(Ordering::SeqCst)) {
            return Ok(conn.clone());
        }
        let stream = sshcraft_agent::transport::connect().await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                AgentError::new(ErrorKind::Denied, e.to_string())
            } else {
                not_running()
            }
        })?;
        let (read, write) = tokio::io::split(stream);
        let pending: Pending = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        // Answers come in any order; each goes to the request with its id.
        tokio::spawn({
            let pending = pending.clone();
            let alive = alive.clone();
            async move {
                let mut read = BufReader::new(read);
                while let Ok(Some(response)) = read_message::<Response, _>(&mut read).await {
                    if let Some(tx) = pending.lock().expect("pending").remove(&response.id) {
                        let _ = tx.send(response);
                    }
                }
                alive.store(false, Ordering::SeqCst);
                // Waiting requests learn that the app went away.
                pending.lock().expect("pending").clear();
            }
        });
        let conn = Arc::new(Conn {
            write: tokio::sync::Mutex::new(write),
            pending,
            alive,
        });
        *slot = Some(conn.clone());
        Ok(conn)
    }
}

/// Cancels the request in the app when the caller stops waiting before the answer.
struct CancelOnDrop {
    conn: Arc<Conn>,
    id: u64,
    armed: bool,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.conn.pending.lock().expect("pending").remove(&self.id);
        let conn = self.conn.clone();
        let id = self.id;
        tokio::spawn(async move {
            let cancel = Request {
                id: 0,
                agent: "cancel".into(),
                action: Action::Cancel { request: id },
            };
            let _ = write_message(&mut *conn.write.lock().await, &cancel).await;
        });
    }
}

#[async_trait::async_trait]
impl Caller for AppClient {
    async fn call(&self, agent: &str, action: Action) -> Result<Value, AgentError> {
        let conn = self.connection().await?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = oneshot::channel();
        conn.pending.lock().expect("pending").insert(id, tx);
        let mut guard = CancelOnDrop {
            conn: conn.clone(),
            id,
            armed: true,
        };
        let request = Request {
            id,
            agent: agent.to_owned(),
            action,
        };
        if write_message(&mut *conn.write.lock().await, &request)
            .await
            .is_err()
        {
            guard.armed = false;
            conn.alive.store(false, Ordering::SeqCst);
            return Err(not_running());
        }
        let response = rx.await;
        guard.armed = false;
        let response = response.map_err(|_| not_running())?;
        match (response.ok, response.error) {
            (_, Some(error)) => Err(error),
            (Some(value), None) => Ok(value),
            (None, None) => Ok(Value::Null),
        }
    }
}

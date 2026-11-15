//! The calling end: signs in by challenge (and stops before sending anything derived from the
//! token when the other end cannot prove it holds it), then sends requests side by side. A call
//! whose future is dropped before its answer sends a `cancel` for it.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::oneshot;

use crate::auth::{client_proof, nonce, same, server_proof};
use crate::wire::{Line, Request, Response, read_line, write_line};
use crate::{LinkError, MAX_REPLY, MAX_REQUEST, Token};

/// How long connecting and signing in may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Response>>>>;

struct Inner {
    write: tokio::sync::Mutex<OwnedWriteHalf>,
    pending: Pending,
    alive: Arc<AtomicBool>,
    next: AtomicU64,
    reader: tokio::task::JoinHandle<()>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

/// A signed-in connection to a running app.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("alive", &self.is_alive()).finish()
    }
}

async fn answer(read: &mut BufReader<OwnedReadHalf>) -> Result<Response, LinkError> {
    match read_line(read, MAX_REPLY).await {
        Ok(Line::Text(bytes)) => {
            serde_json::from_slice(&bytes).map_err(|_| LinkError::Refused("the other end does not speak the SSHCraft link".into()))
        }
        Ok(Line::End) => Err(LinkError::Refused("the other end closed the connection".into())),
        Ok(Line::TooLong) => Err(LinkError::Refused("the other end sent too much".into())),
        Err(e) => Err(LinkError::Io(e.to_string())),
    }
}

async fn sign_in(read: &mut BufReader<OwnedReadHalf>, write: &mut OwnedWriteHalf, token: &Token) -> Result<(), LinkError> {
    let client_nonce = nonce();
    let challenge = Request { id: 1, method: "auth.challenge".into(), params: json!({ "clientNonce": client_nonce }) };
    write_line(write, &challenge, MAX_REQUEST).await.map_err(|e| LinkError::Io(e.to_string()))?;
    let reply = answer(read).await?;
    let result = reply.result.unwrap_or_default();
    let server_nonce = result.get("serverNonce").and_then(Value::as_str).unwrap_or_default().to_string();
    let given = result.get("serverProof").and_then(Value::as_str).unwrap_or_default();
    // The other end must prove it holds the token before anything derived from it goes out.
    if !reply.ok || !same(given, &server_proof(token, &client_nonce, &server_nonce)) {
        return Err(LinkError::Refused("the program on that port is not this SSHCraft; the token was not sent".into()));
    }
    let auth = Request { id: 2, method: "auth".into(), params: json!({ "proof": client_proof(token, &client_nonce, &server_nonce) }) };
    write_line(write, &auth, MAX_REQUEST).await.map_err(|e| LinkError::Io(e.to_string()))?;
    let reply = answer(read).await?;
    if !reply.ok {
        return Err(LinkError::Refused(reply.error.unwrap_or_else(|| "refused".into())));
    }
    Ok(())
}

impl Client {
    /// Connects to `addr` (a loopback address only) and signs in with `token`.
    pub async fn connect(addr: SocketAddr, token: &Token) -> Result<Self, LinkError> {
        if !addr.ip().is_loopback() {
            return Err(LinkError::Refused(format!("{addr} is not on this computer")));
        }
        let stream = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(_)) | Err(_) => return Err(LinkError::NotRunning),
        };
        let (read, mut write) = stream.into_split();
        let mut read = BufReader::new(read);
        tokio::time::timeout(CONNECT_TIMEOUT, sign_in(&mut read, &mut write, token))
            .await
            .map_err(|_| LinkError::Refused("signing in took too long".into()))??;
        let pending: Pending = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        let reader = tokio::spawn(read_answers(read, pending.clone(), alive.clone()));
        Ok(Self { inner: Arc::new(Inner { write: tokio::sync::Mutex::new(write), pending, alive, next: AtomicU64::new(10), reader }) })
    }

    pub fn is_alive(&self) -> bool {
        self.inner.alive.load(Ordering::SeqCst)
    }

    /// Calls `method`; dropping the future before the answer cancels the request in the app.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, LinkError> {
        if !self.is_alive() {
            return Err(LinkError::Closed);
        }
        let id = self.inner.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().unwrap_or_else(PoisonError::into_inner).insert(id, tx);
        // The connection may have ended between the check above and the insert: its reader has
        // cleared the waiting calls already, and this one would wait for ever.
        if !self.is_alive() {
            self.inner.pending.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
            return Err(LinkError::Closed);
        }
        let mut guard = CancelOnDrop { client: self.clone(), id, armed: true };
        let request = Request { id, method: method.to_string(), params };
        let sent = write_line(&mut *self.inner.write.lock().await, &request, MAX_REQUEST).await;
        if let Err(e) = sent {
            guard.armed = false;
            self.inner.pending.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
            return Err(if e.kind() == std::io::ErrorKind::InvalidData {
                LinkError::Remote("the request is too large".into())
            } else {
                LinkError::Closed
            });
        }
        let response = rx.await;
        guard.armed = false;
        let response = response.map_err(|_| LinkError::Closed)?;
        if response.ok { Ok(response.result.unwrap_or(Value::Null)) } else { Err(LinkError::Remote(response.error.unwrap_or_default())) }
    }
}

/// Sends a `cancel` for a call whose caller stopped waiting.
struct CancelOnDrop {
    client: Client,
    id: u64,
    armed: bool,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.client.inner.pending.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.id);
        let client = self.client.clone();
        let id = self.id;
        // Outside a runtime there is nobody to send it; the connection's end cancels it then.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let cancel = Request { id: 0, method: "cancel".into(), params: json!({ "request": id }) };
                let _ = write_line(&mut *client.inner.write.lock().await, &cancel, MAX_REQUEST).await;
            });
        }
    }
}

async fn read_answers(mut read: BufReader<OwnedReadHalf>, pending: Pending, alive: Arc<AtomicBool>) {
    while let Ok(Line::Text(bytes)) = read_line(&mut read, MAX_REPLY).await {
        let Ok(response) = serde_json::from_slice::<Response>(&bytes) else { continue };
        if let Some(tx) = pending.lock().unwrap_or_else(PoisonError::into_inner).remove(&response.id) {
            let _ = tx.send(response);
        }
    }
    alive.store(false, Ordering::SeqCst);
    // Waiting calls learn that the connection ended.
    pending.lock().unwrap_or_else(PoisonError::into_inner).clear();
}

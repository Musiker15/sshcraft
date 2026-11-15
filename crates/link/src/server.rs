//! The serving end: listens on 127.0.0.1, signs each connection in, then hands its requests to a
//! [`Handler`], side by side. A connection that goes away (or a server that stops) drops its
//! running requests, and the handler hears that the connection closed.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::io::BufReader;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::{AbortHandle, JoinSet};

use crate::auth::{client_proof, nonce, same, server_proof, valid_nonce};
use crate::wire::{Line, Request, Response, read_line, write_line};
use crate::{AUTH_TIMEOUT, LinkError, MAX_CONNECTIONS, MAX_REPLY, MAX_REQUEST, MAX_RUNNING, Token};

/// One connection, for the handler to tell its callers apart.
pub type ConnId = u64;

/// What the server does with the requests of signed-in connections.
pub trait Handler: Send + Sync + 'static {
    /// Answers one request; dropping the future cancels it.
    fn call(&self, conn: ConnId, method: String, params: Value) -> BoxFuture<'static, Result<Value, String>>;
    /// Connection `conn` ended (it closed, or the server stopped).
    fn closed(&self, conn: ConnId);
}

#[derive(Debug, Clone, Copy)]
pub struct ServerOptions {
    /// How long a new connection has to sign in.
    pub auth_timeout: Duration,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self { auth_timeout: AUTH_TIMEOUT }
    }
}

/// A listening server; dropping it (or [`Server::stop`]) ends every connection.
pub struct Server {
    port: u16,
    stop: watch::Sender<bool>,
}

impl Server {
    /// Listens on `127.0.0.1:port` (0: any free port).
    pub async fn bind(port: u16, token: Token, handler: Arc<dyn Handler>) -> Result<Self, LinkError> {
        Self::bind_with(port, token, handler, ServerOptions::default()).await
    }

    pub async fn bind_with(port: u16, token: Token, handler: Arc<dyn Handler>, options: ServerOptions) -> Result<Self, LinkError> {
        let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| LinkError::Io(format!("127.0.0.1:{port}: {e}")))?;
        let port = listener.local_addr().map_err(|e| LinkError::Io(e.to_string()))?.port();
        let (stop, stopped) = watch::channel(false);
        tokio::spawn(accept(listener, token, handler, options, stopped));
        Ok(Self { port, stop })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stops listening and ends every connection.
    pub fn stop(&self) {
        let _ = self.stop.send(true);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Frees a connection's place when it ends.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Tells the handler that a connection ended, also when its task is aborted.
struct Closed(Arc<dyn Handler>, ConnId, bool);

impl Drop for Closed {
    fn drop(&mut self) {
        if self.2 {
            self.0.closed(self.1);
        }
    }
}

async fn accept(listener: TcpListener, token: Token, handler: Arc<dyn Handler>, options: ServerOptions, mut stopped: watch::Receiver<bool>) {
    let open = Arc::new(AtomicUsize::new(0));
    let next = AtomicU64::new(1);
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { continue };
                while connections.try_join_next().is_some() {}
                if open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    // No place: the connection is closed at once.
                    drop(stream);
                    continue;
                }
                open.fetch_add(1, Ordering::SeqCst);
                let slot = Slot(open.clone());
                let conn = next.fetch_add(1, Ordering::SeqCst);
                connections.spawn(serve(stream, token.clone(), handler.clone(), conn, options, slot));
            }
        }
    }
    // Dropping the set aborts every connection, and with them their requests.
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

/// An answer as it goes out: one too large for a line becomes an error.
fn fit(response: Response) -> Response {
    let size = serde_json::to_vec(&response).map_or(usize::MAX, |v| v.len());
    if size >= MAX_REPLY { Response::error(response.id, "the answer is too large to send") } else { response }
}

async fn request(read: &mut BufReader<OwnedReadHalf>) -> Option<Request> {
    match read_line(read, MAX_REQUEST).await {
        Ok(Line::Text(bytes)) => serde_json::from_slice(&bytes).ok(),
        _ => None,
    }
}

/// The challenge sign-in; whether it succeeded. A failure has been answered.
async fn sign_in(read: &mut BufReader<OwnedReadHalf>, write: &mut OwnedWriteHalf, token: &Token) -> bool {
    let refuse = |id: u64| Response::error(id, "authentication required");
    let Some(first) = request(read).await else { return false };
    let client_nonce = first.params.get("clientNonce").and_then(Value::as_str).unwrap_or_default().to_string();
    if first.method != "auth.challenge" || !valid_nonce(&client_nonce) {
        let _ = write_line(write, &refuse(first.id), MAX_REPLY).await;
        return false;
    }
    let server_nonce = nonce();
    let proof = server_proof(token, &client_nonce, &server_nonce);
    if write_line(write, &Response::ok(first.id, json!({ "serverNonce": server_nonce, "serverProof": proof })), MAX_REPLY).await.is_err() {
        return false;
    }
    let Some(second) = request(read).await else { return false };
    let given = second.params.get("proof").and_then(Value::as_str).unwrap_or_default();
    if second.method != "auth" || !same(given, &client_proof(token, &client_nonce, &server_nonce)) {
        let _ = write_line(write, &refuse(second.id), MAX_REPLY).await;
        return false;
    }
    write_line(write, &Response::ok(second.id, json!({ "authenticated": true })), MAX_REPLY).await.is_ok()
}

async fn serve(stream: TcpStream, token: Token, handler: Arc<dyn Handler>, conn: ConnId, options: ServerOptions, _slot: Slot) {
    let (read, mut write) = stream.into_split();
    let mut read = BufReader::new(read);
    match tokio::time::timeout(options.auth_timeout, sign_in(&mut read, &mut write, &token)).await {
        Ok(true) => {}
        _ => return,
    }
    // Signed in: from here on the handler hears when the connection ends.
    let _closed = Closed(handler.clone(), conn, true);
    let (answers, mut outgoing) = mpsc::channel::<Response>(256);
    let mut tasks = JoinSet::new();
    tasks.spawn(async move {
        while let Some(response) = outgoing.recv().await {
            if write_line(&mut write, &fit(response), MAX_REPLY).await.is_err() {
                return;
            }
        }
    });
    let mut running: HashMap<u64, AbortHandle> = HashMap::new();
    loop {
        let line = match read_line(&mut read, MAX_REQUEST).await {
            Ok(Line::Text(bytes)) => bytes,
            // The end, an error, or a line too long: the connection is done.
            _ => break,
        };
        running.retain(|_, task| !task.is_finished());
        while tasks.try_join_next().is_some() {}
        let Ok(Request { id, method, params }) = serde_json::from_slice::<Request>(&line) else {
            let _ = answers.send(Response::error(0, "not a request")).await;
            continue;
        };
        match method.as_str() {
            "cancel" => {
                if let Some(task) = params.get("request").and_then(Value::as_u64).and_then(|r| running.remove(&r)) {
                    task.abort();
                }
            }
            "auth" | "auth.challenge" => {
                let _ = answers.send(Response::error(id, "already authenticated")).await;
            }
            _ if running.len() >= MAX_RUNNING => {
                let _ = answers.send(Response::error(id, "too many requests at once")).await;
            }
            _ => {
                let call = handler.call(conn, method, params);
                let answers = answers.clone();
                let task = tasks.spawn(async move {
                    let response = match call.await {
                        Ok(value) => Response::ok(id, value),
                        Err(error) => Response::error(id, error),
                    };
                    let _ = answers.send(response).await;
                });
                running.insert(id, task);
            }
        }
    }
    // Dropping the tasks aborts the requests still running (and their questions).
    tasks.abort_all();
}

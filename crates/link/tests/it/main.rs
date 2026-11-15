//! The link: sign-in by challenge, the wire's limits, the token and connection files.

mod auth;
mod files;
mod limits;
mod wire;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use sshcraft_link::{ConnId, Handler, Server, Token};

/// A handler that answers `ping`, `echo` and `wait` (never answers until cancelled) and counts.
#[derive(Default)]
pub struct Echo {
    pub calls: AtomicUsize,
    pub closed: AtomicUsize,
    pub cancelled: Arc<AtomicUsize>,
}

/// Counts when dropped: a request whose future was dropped before it answered.
struct OnDrop(Arc<AtomicUsize>, bool);

impl Drop for OnDrop {
    fn drop(&mut self) {
        if !self.1 {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Handler for Echo {
    fn call(&self, _conn: ConnId, method: String, params: Value) -> BoxFuture<'static, Result<Value, String>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let cancelled = self.cancelled.clone();
        async move {
            match method.as_str() {
                "ping" => Ok(json!("pong")),
                "echo" => Ok(params),
                "big" => Ok(json!("x".repeat(9 << 20))),
                "wait" => {
                    let mut guard = OnDrop(cancelled, false);
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    guard.1 = true;
                    Ok(Value::Null)
                }
                other => Err(format!("unknown method {other}")),
            }
        }
        .boxed()
    }

    fn closed(&self, _conn: ConnId) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}

pub async fn echo_server() -> (Server, Arc<Echo>, Token) {
    let token = Token::generate();
    let handler = Arc::new(Echo::default());
    let server = Server::bind(0, token.clone(), handler.clone()).await.unwrap();
    (server, handler, token)
}

pub fn addr(server: &Server) -> std::net::SocketAddr {
    std::net::SocketAddr::from(([127, 0, 0, 1], server.port()))
}

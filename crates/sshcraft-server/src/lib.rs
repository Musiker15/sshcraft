//! Self-hosted sync server for SSHCraft.
//!
//! The server stores accounts, device logs, areas, key envelopes and sealed records and pushes
//! change notices to connected devices. It never sees a private key or a plaintext record.

pub mod admin;
pub mod auth;
pub mod config;
pub mod error;
pub mod hub;
pub mod routes;
pub mod state;
pub mod store;

pub use routes::app;
pub use state::AppState;

use config::{ConfigError, ServeConfig};
use std::future::Future;
use std::time::Duration;
use store::StoreError;
use tokio::net::TcpListener;

/// How often expired sessions are removed.
const SESSION_SWEEP: Duration = Duration::from_secs(3600);

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Binds `config.listen` and serves until `shutdown` completes.
pub async fn serve(
    config: ServeConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServeError> {
    config.validate()?;
    let listener = TcpListener::bind(config.listen).await?;
    serve_on(listener, config, shutdown).await
}

/// Serves on an already bound listener until `shutdown` completes.
pub async fn serve_on(
    listener: TcpListener,
    config: ServeConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServeError> {
    let origin = config.validate()?;
    let store = store::open(&config.database_url).await?;
    let (key, created) = store.api_key().await?;
    if created {
        tracing::info!(
            "created the API key {key}; give it only to people who should use this server (show it again with `sshcraft-server api-key show`)"
        );
    }
    let state = AppState::new(store, origin.clone(), config.ws_recheck);
    let sweeper = {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(SESSION_SWEEP);
            loop {
                tick.tick().await;
                if let Err(err) = state.store.delete_expired_sessions(state.now()).await {
                    tracing::warn!("removing expired sessions failed: {err}");
                }
            }
        })
    };
    tracing::info!(
        "listening on {}, public origin {origin}",
        listener.local_addr()?
    );
    let result = axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown)
        .await;
    sweeper.abort();
    result.map_err(ServeError::Io)
}

/// Asks a running server at `addr` for `GET /v1/server`; `Ok` when it answers 200 within five
/// seconds. Used by `sshcraft-server health` (the Docker health check), so it needs no HTTP client.
pub async fn health(addr: std::net::SocketAddr) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let check = async {
        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .map_err(|e| e.to_string())?;
        stream
            .write_all(
                b"GET /v1/server HTTP/1.1
Host: localhost
Connection: close

",
            )
            .await
            .map_err(|e| e.to_string())?;
        let mut head = [0u8; 12];
        stream
            .read_exact(&mut head)
            .await
            .map_err(|e| e.to_string())?;
        if head.starts_with(b"HTTP/1.1 200") {
            Ok(())
        } else {
            Err(format!(
                "unexpected answer: {}",
                String::from_utf8_lossy(&head)
            ))
        }
    };
    tokio::time::timeout(Duration::from_secs(5), check)
        .await
        .map_err(|_| "no answer within 5 seconds".to_owned())?
}

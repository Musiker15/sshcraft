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

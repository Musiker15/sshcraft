//! The HTTP routes under `/v1`.

mod server;

use crate::error::ApiError;
use crate::state::AppState;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;

/// Largest accepted request body.
pub const BODY_LIMIT: usize = 1 << 20;

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/v1/server", get(server::info))
        .fallback(|| async { ApiError::NotFound })
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(state)
}

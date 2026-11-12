//! The HTTP routes under `/v1`.

pub(crate) mod accounts;
mod devicelog;
mod login;
mod server;

use crate::error::ApiError;
use crate::state::AppState;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};

/// Largest accepted request body.
pub const BODY_LIMIT: usize = 1 << 20;

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/v1/server", get(server::info))
        .route("/v1/accounts", post(accounts::register))
        .route("/v1/accounts/me", get(accounts::me))
        .route("/v1/accounts/me/devices", get(accounts::my_devices))
        .route("/v1/accounts/lookup", get(accounts::lookup))
        .route("/v1/accounts/{account}", get(accounts::public_info))
        .route(
            "/v1/accounts/{account}/log",
            get(devicelog::read).post(devicelog::append),
        )
        .route("/v1/auth/challenge", post(login::challenge))
        .route("/v1/auth/login", post(login::login))
        .fallback(|| async { ApiError::NotFound })
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(state)
}

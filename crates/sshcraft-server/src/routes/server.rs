use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

/// `GET /v1/server`: what this server is and which origin logins are bound to.
pub async fn info(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "name": "sshcraft-server",
        "version": env!("CARGO_PKG_VERSION"),
        "api": 1,
        "origin": state.origin,
    }))
}

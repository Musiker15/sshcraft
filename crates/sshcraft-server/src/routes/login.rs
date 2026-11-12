//! Login: the server sends a random challenge, the device signs it for this server's origin.

use crate::auth::{has_api_key, token_hash};
use crate::error::{ApiError, ApiJson};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use data_encoding::BASE64URL_NOPAD;
use rand::RngExt;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

/// Lifetime of a session, in seconds (30 days).
pub const SESSION_TTL: i64 = 30 * 86_400;

#[derive(Deserialize)]
pub struct Login {
    device_id: Uuid,
    challenge: String,
    signature: String,
}

fn decode<const N: usize>(text: &str) -> Result<[u8; N], ApiError> {
    BASE64URL_NOPAD
        .decode(text.as_bytes())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| ApiError::Invalid("bad base64 field".into()))
}

/// `POST /v1/auth/challenge`
pub async fn challenge(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let challenge = state.challenges.issue(state.now())?;
    Ok(Json(
        json!({ "challenge": BASE64URL_NOPAD.encode(&challenge) }),
    ))
}

/// `POST /v1/auth/login`. The API key is required only for a device's first login, so devices
/// that are already in keep working after the operator rotates the key.
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<Login>,
) -> Result<Json<Value>, ApiError> {
    let challenge = BASE64URL_NOPAD
        .decode(body.challenge.as_bytes())
        .map_err(|_| ApiError::Invalid("bad base64 field".into()))?;
    let signature: [u8; 64] = decode(&body.signature)?;
    let now = state.now();
    if !state.challenges.valid(&challenge, now) {
        return Err(ApiError::Unauthorized);
    }
    let device = state
        .store
        .device(body.device_id)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let account = state
        .store
        .account(&device.account)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if account.locked {
        return Err(ApiError::AccountLocked);
    }
    if !device.active {
        return Err(ApiError::DeviceInactive);
    }
    if device.admin_blocked {
        return Err(ApiError::DeviceBlocked);
    }
    if !device.has_logged_in && !has_api_key(&state, &headers).await? {
        return Err(ApiError::BadApiKey);
    }
    device
        .cert
        .verify_auth(&challenge, &state.origin, &signature)
        .map_err(|_| ApiError::Unauthorized)?;
    if !state.challenges.redeem(&challenge, now) {
        return Err(ApiError::Unauthorized);
    }

    state.store.mark_logged_in(device.device_id, now).await?;
    let mut token = [0u8; 32];
    rand::rng().fill(&mut token[..]);
    let expires_at = now + SESSION_TTL;
    state
        .store
        .create_session(
            token_hash(&token),
            device.device_id,
            &device.account,
            expires_at,
        )
        .await?;
    Ok(Json(json!({
        "token": BASE64URL_NOPAD.encode(&token),
        "expires_at": expires_at,
        "account": device.account,
    })))
}

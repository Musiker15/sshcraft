//! Who is asking: the API key (registration, first login of a device) or a session token.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::request::Parts;
use data_encoding::BASE64URL_NOPAD;
use sha2::{Digest, Sha256};
use sshcraft_proto::identity::AccountPublic;
use subtle::ConstantTimeEq;
use uuid::Uuid;

/// `last_seen` is written at most this often per device, in seconds.
const TOUCH_EVERY: i64 = 300;

/// SHA-256 of a session token; only the hash is stored.
pub fn token_hash(token: &[u8]) -> [u8; 32] {
    Sha256::digest(token).into()
}

/// Whether the request carries the current API key in `X-Api-Key` (constant-time compare).
pub async fn has_api_key(state: &AppState, headers: &HeaderMap) -> Result<bool, ApiError> {
    let Some(given) = headers.get("x-api-key") else {
        return Ok(false);
    };
    let (key, _) = state.store.api_key().await?;
    Ok(bool::from(given.as_bytes().ct_eq(key.as_bytes())))
}

/// The request carries the current API key.
pub struct ApiKey;

impl FromRequestParts<AppState> for ApiKey {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        if has_api_key(state, &parts.headers).await? {
            Ok(Self)
        } else {
            Err(ApiError::BadApiKey)
        }
    }
}

/// A valid session of an active device of an unlocked account, checked against the database
/// on every request.
#[derive(Debug, Clone, Copy)]
pub struct Session {
    pub device_id: Uuid,
    pub account: AccountPublic,
}

impl Session {
    /// Checks a bearer token as it stands now; also used for open WebSockets.
    pub async fn check(state: &AppState, token: &[u8]) -> Result<Self, ApiError> {
        let now = state.now();
        let session = state
            .store
            .session(token_hash(token))
            .await?
            .filter(|s| s.expires_at > now)
            .ok_or(ApiError::Unauthorized)?;
        let account = state
            .store
            .account(&session.account)
            .await?
            .ok_or(ApiError::Unauthorized)?;
        if account.locked {
            return Err(ApiError::AccountLocked);
        }
        let device = state
            .store
            .device(session.device_id)
            .await?
            .ok_or(ApiError::Unauthorized)?;
        if !device.active || device.admin_blocked || device.account != session.account {
            return Err(ApiError::DeviceInactive);
        }
        if device
            .last_seen
            .is_none_or(|seen| now - seen >= TOUCH_EVERY)
        {
            state.store.touch_device(device.device_id, now).await?;
        }
        Ok(Self {
            device_id: session.device_id,
            account: session.account,
        })
    }
}

/// The token bytes from `Authorization: Bearer <base64>`.
pub fn bearer_token(headers: &HeaderMap) -> Result<Vec<u8>, ApiError> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let token = value
        .strip_prefix("Bearer ")
        .ok_or(ApiError::Unauthorized)?;
    BASE64URL_NOPAD
        .decode(token.trim().as_bytes())
        .map_err(|_| ApiError::Unauthorized)
}

impl FromRequestParts<AppState> for Session {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = bearer_token(&parts.headers)?;
        Self::check(state, &token).await
    }
}

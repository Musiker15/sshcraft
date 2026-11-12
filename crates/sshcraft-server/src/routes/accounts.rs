//! Registration and account information.

use crate::auth::{ApiKey, Session};
use crate::error::{ApiError, ApiJson};
use crate::state::AppState;
use crate::store::AccountRow;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_proto::devicelog::{DeviceLog, LogAction};
use sshcraft_proto::identity::{AccountPublic, DeviceCertificate};

pub const MAX_NAME: usize = 100;
pub const MAX_EMAIL: usize = 254;
pub const MAX_DEVICE_NAME: usize = 100;
pub const MAX_PLATFORM: usize = 32;

#[derive(Deserialize)]
pub struct Register {
    account: AccountPublic,
    name: String,
    email: String,
    log: DeviceLog,
}

fn invalid(why: &str) -> ApiError {
    ApiError::Invalid(why.to_owned())
}

/// Trimmed name, 1 to [`MAX_NAME`] characters.
pub fn check_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    let len = name.chars().count();
    if len == 0 || len > MAX_NAME {
        return Err(invalid("name must have 1 to 100 characters"));
    }
    Ok(name.to_owned())
}

/// Trimmed email and its normal form (lower case), or `invalid`.
pub fn check_email(email: &str) -> Result<(String, String), ApiError> {
    let email = email.trim();
    let mut parts = email.split('@');
    let well_formed = matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(local), Some(domain), None) if !local.is_empty() && !domain.is_empty()
    );
    if !well_formed || email.chars().count() > MAX_EMAIL {
        return Err(invalid("not an email address"));
    }
    Ok((email.to_owned(), email.to_lowercase()))
}

/// Device name and platform within their limits.
pub fn check_certificate(cert: &DeviceCertificate) -> Result<(), ApiError> {
    let name = cert.name.chars().count();
    let platform = cert.platform.chars().count();
    if name == 0 || name > MAX_DEVICE_NAME || platform == 0 || platform > MAX_PLATFORM {
        return Err(invalid("device name or platform out of range"));
    }
    Ok(())
}

/// `POST /v1/accounts`: a new account with its first device.
pub async fn register(
    State(state): State<AppState>,
    _key: ApiKey,
    ApiJson(body): ApiJson<Register>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let name = check_name(&body.name)?;
    let (email, email_norm) = check_email(&body.email)?;
    let [first] = body.log.entries.as_slice() else {
        return Err(invalid("a new account starts with exactly one device"));
    };
    body.log.verify(&body.account)?;
    let LogAction::Add { certificate } = &first.action else {
        return Err(invalid("a new account starts with adding a device"));
    };
    check_certificate(certificate)?;
    let row = AccountRow {
        account: body.account,
        name,
        email,
        created_at: state.now(),
        locked: false,
    };
    state
        .store
        .create_account(&row, &email_norm, first, certificate)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "account": body.account })),
    ))
}

/// `GET /v1/accounts/me`
pub async fn me(State(state): State<AppState>, session: Session) -> Result<Json<Value>, ApiError> {
    let row = state
        .store
        .account(&session.account)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(json!({
        "account": row.account,
        "name": row.name,
        "email": row.email,
        "created_at": row.created_at,
    })))
}

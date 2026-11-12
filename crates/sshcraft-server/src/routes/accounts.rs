//! Registration and account information.

use crate::auth::{ApiKey, Session};
use crate::error::{ApiError, ApiJson, ApiPath, ApiQuery};
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

/// An account's public key from a URL path segment (Base64 URL, no padding).
pub fn parse_account(text: &str) -> Result<AccountPublic, ApiError> {
    data_encoding::BASE64URL_NOPAD
        .decode(text.as_bytes())
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .map(AccountPublic)
        .ok_or_else(|| invalid("not an account key"))
}

/// `GET /v1/accounts/me/devices`: state per device; names and platforms are in the log.
pub async fn my_devices(
    State(state): State<AppState>,
    session: Session,
) -> Result<Json<Value>, ApiError> {
    let devices: Vec<Value> = state
        .store
        .devices_of(&session.account)
        .await?
        .into_iter()
        .map(|d| {
            json!({
                "device_id": d.device_id,
                "active": d.active,
                "admin_blocked": d.admin_blocked,
                "last_seen": d.last_seen,
            })
        })
        .collect();
    Ok(Json(json!({ "devices": devices })))
}

#[derive(Deserialize)]
pub struct Lookup {
    email: String,
}

/// `GET /v1/accounts/lookup?email=...`: find someone to share with.
pub async fn lookup(
    State(state): State<AppState>,
    _session: Session,
    ApiQuery(query): ApiQuery<Lookup>,
) -> Result<Json<Value>, ApiError> {
    let (_, email_norm) = check_email(&query.email)?;
    let row = state
        .store
        .account_by_email(&email_norm)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(json!({ "account": row.account, "name": row.name })))
}

/// `GET /v1/accounts/{account}`: public key and name, no email.
pub async fn public_info(
    State(state): State<AppState>,
    _session: Session,
    ApiPath(account): ApiPath<String>,
) -> Result<Json<Value>, ApiError> {
    let account = parse_account(&account)?;
    let row = state
        .store
        .account(&account)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(json!({ "account": row.account, "name": row.name })))
}

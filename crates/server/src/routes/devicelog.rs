//! The device log of an account: read by any logged-in account, appended by the account's own
//! devices or by a new device with the API key. The signatures in the log decide what is valid.

use crate::auth::{Session, has_api_key};
use crate::error::{ApiError, ApiJson, ApiPath, ApiQuery};
use crate::hub::Event;
use crate::routes::accounts::{check_certificate, parse_account};
use crate::state::AppState;
use crate::store::DeviceChanges;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_proto::devicelog::{DeviceLog, LogAction, LogEntry, LogSigner};
use sshcraft_proto::identity::AccountPublic;

/// Most entries in one append.
pub const MAX_APPEND: usize = 100;
/// Most entries in one account's log. Every append verifies the whole log, so this also bounds
/// the work one request can cause.
pub const MAX_LOG: usize = 1_000;

#[derive(Deserialize)]
pub struct From {
    #[serde(default)]
    from: u64,
}

#[derive(Deserialize)]
pub struct Append {
    entries: Vec<LogEntry>,
}

/// `GET /v1/accounts/{account}/log?from=<seq>`
pub async fn read(
    State(state): State<AppState>,
    _session: Session,
    ApiPath(account): ApiPath<String>,
    ApiQuery(query): ApiQuery<From>,
) -> Result<Json<Value>, ApiError> {
    let account = parse_account(&account)?;
    state.store.account(&account).await?.ok_or(ApiError::NotFound)?;
    let log = state.store.device_log(&account).await?;
    let from = usize::try_from(query.from).unwrap_or(usize::MAX);
    let entries = log.entries.get(from..).unwrap_or_default();
    Ok(Json(json!({ "entries": entries })))
}

/// `POST /v1/accounts/{account}/log`
pub async fn append(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiPath(account): ApiPath<String>,
    ApiJson(body): ApiJson<Append>,
) -> Result<Json<Value>, ApiError> {
    let account = parse_account(&account)?;
    if headers.contains_key(axum::http::header::AUTHORIZATION) {
        let token = crate::auth::bearer_token(&headers)?;
        let session = Session::check(&state, &token).await?;
        if session.account != account {
            return Err(ApiError::Forbidden);
        }
    } else if has_api_key(&state, &headers).await? {
        // Without a session only the account key may sign: that is all a new device needs to
        // enroll, and it keeps a device the operator blocked from revoking the others.
        if body.entries.iter().any(|e| e.signer != LogSigner::Account) {
            return Err(ApiError::Forbidden);
        }
    } else {
        return Err(ApiError::Unauthorized);
    }
    let row = state.store.account(&account).await?.ok_or(ApiError::NotFound)?;
    if row.locked {
        return Err(ApiError::AccountLocked);
    }
    if body.entries.is_empty() || body.entries.len() > MAX_APPEND {
        return Err(ApiError::Invalid("1 to 100 entries per append".into()));
    }

    let stored = state.store.device_log(&account).await?;
    let len = stored.entries.len() as u64;
    if body.entries[0].seq != len {
        return Err(ApiError::Conflict { current: len });
    }
    if stored.entries.len() + body.entries.len() > MAX_LOG {
        return Err(ApiError::Invalid("the device log is full".into()));
    }
    let mut extended = DeviceLog { entries: stored.entries };
    extended.entries.extend(body.entries.iter().cloned());
    let extended = verify_off_runtime(extended, account).await?;

    let mut changes = DeviceChanges::default();
    for entry in &body.entries {
        match &entry.action {
            LogAction::Add { certificate } => {
                check_certificate(certificate)?;
                changes.added.push((**certificate).clone());
            }
            LogAction::Revoke { device_id } => changes.revoked.push(*device_id),
        }
    }
    state.store.append_log(&account, len, &body.entries, &changes).await?;
    for device_id in &changes.revoked {
        state.hub.end_device(*device_id);
    }
    let new_len = extended.entries.len() as u64;
    let mut accounts = state.store.area_peers(&account).await?;
    accounts.push(account);
    state.hub.publish(&accounts, Event::DeviceLog { account, len: new_len });
    Ok(Json(json!({ "len": new_len })))
}

/// Runs the full log verification on the blocking pool, so a long log does not hold up a
/// runtime thread that serves other requests.
pub async fn verify_off_runtime(log: DeviceLog, account: AccountPublic) -> Result<DeviceLog, ApiError> {
    tokio::task::spawn_blocking(move || log.verify(&account).map(|_| log)).await.map_err(ApiError::internal)?.map_err(ApiError::from)
}

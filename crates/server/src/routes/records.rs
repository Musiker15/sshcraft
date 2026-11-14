//! Sealed records. The server checks who writes, under which key version and with which
//! version number; it cannot read the body.

use crate::auth::Session;
use crate::error::{ApiError, ApiJson, ApiPath, ApiQuery};
use crate::hub::Event;
use crate::routes::areas::{audience, can_read, load};
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_proto::area::AreaId;
use sshcraft_proto::record::SealedRecord;
use uuid::Uuid;

/// Largest accepted ciphertext of one record.
pub const MAX_CIPHERTEXT: usize = 64 * 1024;
const DEFAULT_LIMIT: u32 = 500;
/// A page stops growing once its records reach this many bytes of JSON; `more` says so.
pub const MAX_PAGE_BYTES: usize = 4 << 20;
const MAX_LIMIT: u32 = 1000;

/// `PUT /v1/areas/{area_id}/records/{record_id}`
pub async fn put(
    State(state): State<AppState>,
    session: Session,
    ApiPath((area, record_id)): ApiPath<(Uuid, Uuid)>,
    ApiJson(record): ApiJson<SealedRecord>,
) -> Result<Json<Value>, ApiError> {
    let header = &record.header;
    if header.area_id.0 != area || header.record_id != record_id {
        return Err(ApiError::Invalid(
            "record header differs from the path".into(),
        ));
    }
    if header.author_account != session.account || header.author_device != session.device_id {
        return Err(ApiError::Forbidden);
    }
    let row = load(&state, area).await?;
    if !row.manifest.can_write(&session.account) {
        return Err(ApiError::Forbidden);
    }
    let owner = state.store.account(&row.owner).await?;
    if owner.is_none_or(|o| o.locked) {
        return Err(ApiError::AccountLocked);
    }
    if header.key_version != row.manifest.key_version {
        return Err(ApiError::Invalid("record under another key version".into()));
    }
    if record.ciphertext.len() > MAX_CIPHERTEXT {
        return Err(ApiError::TooLarge);
    }
    let seq = state.store.put_record(&record).await?;
    state.hub.publish(
        &audience(&row),
        Event::Record {
            area_id: AreaId(area),
            record_id,
            version: record.header.version,
            seq,
        },
    );
    Ok(Json(json!({ "seq": seq })))
}

#[derive(Deserialize)]
pub struct Since {
    #[serde(default)]
    since: u64,
    limit: Option<u32>,
}

/// `GET /v1/areas/{area_id}/records?since=<seq>&limit=<n>`
pub async fn list(
    State(state): State<AppState>,
    session: Session,
    ApiPath(area): ApiPath<Uuid>,
    ApiQuery(query): ApiQuery<Since>,
) -> Result<Json<Value>, ApiError> {
    let row = load(&state, area).await?;
    if !can_read(&row, &session.account) {
        return Err(ApiError::Forbidden);
    }
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let mut records = state
        .store
        .records_since(AreaId(area), query.since, limit + 1)
        .await?;
    let mut more = records.len() > limit as usize;
    records.truncate(limit as usize);
    let mut page = Vec::with_capacity(records.len());
    let mut bytes = 0;
    for r in records {
        let entry = json!({ "seq": r.seq, "record": r.record });
        bytes += entry.to_string().len();
        if bytes > MAX_PAGE_BYTES && !page.is_empty() {
            more = true;
            break;
        }
        page.push(entry);
    }
    Ok(Json(json!({ "records": page, "more": more })))
}

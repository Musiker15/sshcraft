//! Areas: the owner writes the signed manifest and the key envelopes; owner and members read.

use crate::auth::Session;
use crate::error::{ApiError, ApiJson, ApiPath};
use crate::hub::Event;
use crate::state::AppState;
use crate::store::AreaRow;
use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_proto::area::{AreaId, AreaManifest, KeyEnvelope};
use sshcraft_proto::devicelog::DeviceSet;
use sshcraft_proto::identity::AccountPublic;
use std::collections::HashSet;
use uuid::Uuid;

/// Most members of one area.
pub const MAX_MEMBERS: usize = 1000;
/// Most envelopes in one upload.
pub const MAX_ENVELOPES: usize = 1000;

fn invalid(why: &str) -> ApiError {
    ApiError::Invalid(why.to_owned())
}

/// The verified device set of `account`, from its stored log.
pub async fn devices_of(state: &AppState, account: &AccountPublic) -> Result<DeviceSet, ApiError> {
    let log = state.store.device_log(account).await?;
    let account = *account;
    tokio::task::spawn_blocking(move || log.verify(&account))
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::internal)
}

/// The area, or `not_found`.
pub async fn load(state: &AppState, area: Uuid) -> Result<AreaRow, ApiError> {
    state
        .store
        .area(AreaId(area))
        .await?
        .ok_or(ApiError::NotFound)
}

/// Owner and members of the area, who get its change notices.
pub fn audience(row: &AreaRow) -> Vec<AccountPublic> {
    std::iter::once(row.owner)
        .chain(row.manifest.members.iter().map(|m| m.account))
        .collect()
}

/// Whether `account` owns the area or is listed in its manifest.
pub fn can_read(row: &AreaRow, account: &AccountPublic) -> bool {
    row.owner == *account || row.manifest.members.iter().any(|m| m.account == *account)
}

/// `GET /v1/areas`
pub async fn list(
    State(state): State<AppState>,
    session: Session,
) -> Result<Json<Value>, ApiError> {
    let areas: Vec<Value> = state
        .store
        .areas_of(&session.account)
        .await?
        .into_iter()
        .map(|row| json!({ "owner": row.owner, "manifest": row.manifest }))
        .collect();
    Ok(Json(json!({ "areas": areas })))
}

/// `PUT /v1/areas/{area_id}/manifest`
pub async fn put_manifest(
    State(state): State<AppState>,
    session: Session,
    ApiPath(area): ApiPath<Uuid>,
    ApiJson(manifest): ApiJson<AreaManifest>,
) -> Result<Json<Value>, ApiError> {
    if manifest.area_id.0 != area {
        return Err(invalid("area id differs from the path"));
    }
    if manifest.owner != session.account {
        return Err(ApiError::Forbidden);
    }
    let existing = state.store.area(manifest.area_id).await?;
    if existing
        .as_ref()
        .is_some_and(|row| row.owner != session.account)
    {
        return Err(ApiError::Forbidden);
    }
    let owner_devices = devices_of(&state, &session.account).await?;
    manifest.verify(&owner_devices)?;

    if manifest.members.len() > MAX_MEMBERS {
        return Err(invalid("too many members"));
    }
    let mut seen = HashSet::new();
    for member in &manifest.members {
        if member.account == manifest.owner || !seen.insert(member.account) {
            return Err(invalid("a member is the owner or listed twice"));
        }
        if state.store.account(&member.account).await?.is_none() {
            return Err(invalid("a member has no account on this server"));
        }
    }
    let expected = existing.as_ref().map(|row| row.manifest.version);
    state
        .store
        .put_manifest(&session.account, &manifest, expected)
        .await?;
    // Members who were just removed hear about it too, so they notice.
    let mut accounts = audience(&AreaRow {
        owner: session.account,
        manifest: manifest.clone(),
    });
    if let Some(old) = &existing {
        accounts.extend(audience(old));
    }
    state.hub.publish(
        &accounts,
        Event::Manifest {
            area_id: manifest.area_id,
            version: manifest.version,
        },
    );
    Ok(Json(json!({ "version": manifest.version })))
}

/// `DELETE /v1/areas/{area_id}`, owner only.
pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    ApiPath(area): ApiPath<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = load(&state, area).await?;
    if row.owner != session.account {
        return Err(ApiError::Forbidden);
    }
    state.store.delete_area(AreaId(area)).await?;
    state.hub.publish(
        &audience(&row),
        Event::AreaDeleted {
            area_id: AreaId(area),
        },
    );
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
pub struct Envelopes {
    envelopes: Vec<KeyEnvelope>,
}

/// `PUT /v1/areas/{area_id}/envelopes`, owner only. Envelopes cannot be checked (they are not
/// authenticated by design); the server only makes sure each goes to an active device of someone
/// who may read the area, under the current key version.
pub async fn put_envelopes(
    State(state): State<AppState>,
    session: Session,
    ApiPath(area): ApiPath<Uuid>,
    ApiJson(body): ApiJson<Envelopes>,
) -> Result<Json<Value>, ApiError> {
    let row = load(&state, area).await?;
    if row.owner != session.account {
        return Err(ApiError::Forbidden);
    }
    if body.envelopes.len() > MAX_ENVELOPES {
        return Err(invalid("too many envelopes"));
    }
    for envelope in &body.envelopes {
        if envelope.area_id.0 != area || envelope.key_version != row.manifest.key_version {
            return Err(invalid("envelope for another area or key version"));
        }
        let device = state.store.device(envelope.device_id).await?;
        let allowed = device.is_some_and(|d| d.active && can_read(&row, &d.account));
        if !allowed {
            return Err(invalid("envelope for a device that may not read this area"));
        }
    }
    state
        .store
        .put_envelopes(AreaId(area), &body.envelopes)
        .await?;
    state.hub.publish(
        &audience(&row),
        Event::Envelopes {
            area_id: AreaId(area),
            key_version: row.manifest.key_version,
        },
    );
    Ok(Json(json!({ "count": body.envelopes.len() })))
}

/// `GET /v1/areas/{area_id}/envelope`: the envelope for the calling device.
pub async fn my_envelope(
    State(state): State<AppState>,
    session: Session,
    ApiPath(area): ApiPath<Uuid>,
) -> Result<Json<KeyEnvelope>, ApiError> {
    let row = load(&state, area).await?;
    if !can_read(&row, &session.account) {
        return Err(ApiError::Forbidden);
    }
    state
        .store
        .envelope_for(AreaId(area), session.device_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

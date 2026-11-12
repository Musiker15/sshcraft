//! Storage behind the HTTP layer. Both backends (SQLite, PostgreSQL) fulfil the same contract,
//! checked by `tests/store_contract.rs`. Protocol objects are stored as their JSON; only the
//! header fields the server needs are columns.

pub mod postgres;
pub mod sqlite;

use sshcraft_proto::area::{AreaId, AreaManifest, KeyEnvelope, Role};
use sshcraft_proto::devicelog::{DeviceLog, LogEntry};
use sshcraft_proto::identity::{AccountPublic, DeviceCertificate};
use sshcraft_proto::record::SealedRecord;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    pub account: AccountPublic,
    pub name: String,
    pub email: String,
    pub created_at: i64,
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRow {
    pub device_id: Uuid,
    pub account: AccountPublic,
    pub cert: DeviceCertificate,
    pub active: bool,
    pub admin_blocked: bool,
    pub has_logged_in: bool,
    pub last_seen: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub device_id: Uuid,
    pub account: AccountPublic,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaRow {
    pub owner: AccountPublic,
    pub manifest: AreaManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRecord {
    pub seq: u64,
    pub record: SealedRecord,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceChanges {
    pub added: Vec<DeviceCertificate>,
    pub revoked: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AreaOutcome {
    TransferTo(AccountPublic),
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaFate {
    pub area_id: AreaId,
    pub members: usize,
    pub outcome: AreaOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("version conflict, current {current}")]
    Conflict { current: u64 },
    #[error("email taken")]
    EmailTaken,
    #[error("account exists")]
    AccountExists,
    #[error("device id taken")]
    DeviceIdTaken,
    #[error("not found")]
    NotFound,
    /// The account may not do this to the area (re-checked inside the write transaction).
    #[error("not allowed")]
    NotAllowed,
    /// The area moved on to another key version while the write was on its way.
    #[error("the area is at key version {current}")]
    WrongKeyVersion { current: u32 },
    #[error("storage: {0}")]
    Backend(String),
}

#[async_trait::async_trait]
pub trait Store: Send + Sync {
    /// Returns the API key, creating one on first use; `true` when it was created now.
    async fn api_key(&self) -> Result<(String, bool), StoreError>;
    async fn rotate_api_key(&self) -> Result<String, StoreError>;

    async fn create_account(
        &self,
        row: &AccountRow,
        email_norm: &str,
        first: &LogEntry,
        device: &DeviceCertificate,
    ) -> Result<(), StoreError>;
    async fn account(&self, account: &AccountPublic) -> Result<Option<AccountRow>, StoreError>;
    async fn account_by_email(&self, email_norm: &str) -> Result<Option<AccountRow>, StoreError>;
    async fn list_accounts(&self) -> Result<Vec<AccountRow>, StoreError>;
    /// Locking also deletes the account's sessions.
    async fn set_locked(&self, account: &AccountPublic, locked: bool) -> Result<(), StoreError>;

    async fn device_log(&self, account: &AccountPublic) -> Result<DeviceLog, StoreError>;
    /// Appends only if the stored log has exactly `expected_len` entries, else
    /// `Conflict { current }`. Adds `changes.added` as active devices, marks `changes.revoked`
    /// inactive and deletes their sessions.
    async fn append_log(
        &self,
        account: &AccountPublic,
        expected_len: u64,
        entries: &[LogEntry],
        changes: &DeviceChanges,
    ) -> Result<(), StoreError>;
    async fn device(&self, device_id: Uuid) -> Result<Option<DeviceRow>, StoreError>;
    async fn devices_of(&self, account: &AccountPublic) -> Result<Vec<DeviceRow>, StoreError>;
    async fn mark_logged_in(&self, device_id: Uuid, now: i64) -> Result<(), StoreError>;
    async fn touch_device(&self, device_id: Uuid, now: i64) -> Result<(), StoreError>;
    /// Blocking also deletes the device's sessions.
    async fn set_admin_blocked(&self, device_id: Uuid, blocked: bool) -> Result<(), StoreError>;

    async fn create_session(
        &self,
        token_hash: [u8; 32],
        device_id: Uuid,
        account: &AccountPublic,
        expires_at: i64,
    ) -> Result<(), StoreError>;
    async fn session(&self, token_hash: [u8; 32]) -> Result<Option<SessionRow>, StoreError>;
    async fn delete_expired_sessions(&self, now: i64) -> Result<u64, StoreError>;

    async fn area(&self, area_id: AreaId) -> Result<Option<AreaRow>, StoreError>;
    /// Areas the account owns or is a member of.
    async fn areas_of(&self, account: &AccountPublic) -> Result<Vec<AreaRow>, StoreError>;
    /// Accounts that share at least one area with `account` (owner or member), without itself.
    async fn area_peers(&self, account: &AccountPublic) -> Result<Vec<AccountPublic>, StoreError>;
    /// `expected` = `None` for a new area (version must be 1), else the stored version (new =
    /// stored + 1). Rewrites the member index from `manifest.members`; a higher `key_version`
    /// drops older envelopes.
    async fn put_manifest(
        &self,
        owner: &AccountPublic,
        manifest: &AreaManifest,
        expected: Option<u64>,
    ) -> Result<(), StoreError>;
    async fn delete_area(&self, area_id: AreaId) -> Result<(), StoreError>;
    /// Replaces the envelope per (area, device); all must carry the area's current key version.
    async fn put_envelopes(
        &self,
        area_id: AreaId,
        envelopes: &[KeyEnvelope],
    ) -> Result<(), StoreError>;
    async fn envelope_for(
        &self,
        area_id: AreaId,
        device_id: Uuid,
    ) -> Result<Option<KeyEnvelope>, StoreError>;

    /// Compare and swap: version 1 for a new record id, else stored + 1. Returns the area-local
    /// change number (`seq`), which grows by one per accepted write in this area.
    async fn put_record(&self, record: &SealedRecord) -> Result<u64, StoreError>;
    async fn records_since(
        &self,
        area_id: AreaId,
        since: u64,
        limit: u32,
    ) -> Result<Vec<StoredRecord>, StoreError>;

    async fn plan_account_deletion(
        &self,
        account: &AccountPublic,
    ) -> Result<Vec<AreaFate>, StoreError>;
    /// Runs exactly the plan of `plan_account_deletion` in one transaction and returns it.
    async fn delete_account(&self, account: &AccountPublic) -> Result<Vec<AreaFate>, StoreError>;
}

/// Opens the store named by `url`: `sqlite://<path>` or `postgres://...`.
pub async fn open(url: &str) -> Result<Arc<dyn Store>, StoreError> {
    if let Some(path) = url.strip_prefix("sqlite://") {
        return Ok(Arc::new(sqlite::SqliteStore::open(path).await?));
    }
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        return Ok(Arc::new(postgres::PostgresStore::connect(url).await?));
    }
    // Never echo the URL: it may carry a password.
    Err(StoreError::Backend(
        "unsupported database url, expected sqlite://... or postgres://...".into(),
    ))
}

/// What happens to the areas of an account that is deleted (design, section 12): an area with a
/// member who may write goes to the first such member in manifest order whose account still
/// exists and is not locked (`open_accounts`); every other area goes.
pub(crate) fn area_fate(
    manifest: &AreaManifest,
    open_accounts: &std::collections::HashSet<AccountPublic>,
) -> AreaFate {
    let outcome = manifest
        .members
        .iter()
        .find(|m| m.role == Role::Write && open_accounts.contains(&m.account))
        .map(|m| AreaOutcome::TransferTo(m.account))
        .unwrap_or(AreaOutcome::Delete);
    AreaFate {
        area_id: manifest.area_id,
        members: manifest.members.len(),
        outcome,
    }
}

pub(crate) fn role_name(role: Role) -> &'static str {
    match role {
        Role::Read => "read",
        Role::Write => "write",
    }
}

pub(crate) fn backend(err: impl std::fmt::Display) -> StoreError {
    StoreError::Backend(err.to_string())
}

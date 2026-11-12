//! SQLite backend: one connection behind a mutex, every call on the blocking pool, every write
//! in an `IMMEDIATE` transaction. That makes each compare and swap atomic.

use super::{
    AccountRow, AreaFate, AreaOutcome, AreaRow, DeviceChanges, DeviceRow, SessionRow, Store,
    StoreError, StoredRecord, area_fate, backend, role_name,
};
use data_encoding::BASE64URL_NOPAD;
use rand::RngExt;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sshcraft_proto::area::{AreaId, AreaManifest, KeyEnvelope};
use sshcraft_proto::devicelog::{DeviceLog, LogEntry};
use sshcraft_proto::identity::{AccountPublic, DeviceCertificate};
use sshcraft_proto::record::SealedRecord;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE server_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE deleted_accounts (account BLOB PRIMARY KEY);
CREATE TABLE accounts (
    account BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    email_norm TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    locked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE log_entries (
    account BLOB NOT NULL REFERENCES accounts(account) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    entry TEXT NOT NULL,
    PRIMARY KEY (account, seq)
);
CREATE TABLE devices (
    device_id TEXT PRIMARY KEY,
    account BLOB NOT NULL REFERENCES accounts(account) ON DELETE CASCADE,
    cert TEXT NOT NULL,
    active INTEGER NOT NULL,
    admin_blocked INTEGER NOT NULL DEFAULT 0,
    has_logged_in INTEGER NOT NULL DEFAULT 0,
    last_seen INTEGER
);
CREATE INDEX devices_account ON devices(account);
CREATE TABLE sessions (
    token_hash BLOB PRIMARY KEY,
    device_id TEXT NOT NULL REFERENCES devices(device_id) ON DELETE CASCADE,
    account BLOB NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX sessions_device ON sessions(device_id);
CREATE TABLE areas (
    area_id TEXT PRIMARY KEY,
    owner BLOB NOT NULL,
    version INTEGER NOT NULL,
    key_version INTEGER NOT NULL,
    manifest TEXT NOT NULL
);
CREATE INDEX areas_owner ON areas(owner);
CREATE TABLE area_members (
    area_id TEXT NOT NULL REFERENCES areas(area_id) ON DELETE CASCADE,
    account BLOB NOT NULL,
    role TEXT NOT NULL,
    PRIMARY KEY (area_id, account)
);
CREATE INDEX area_members_account ON area_members(account);
CREATE TABLE envelopes (
    area_id TEXT NOT NULL REFERENCES areas(area_id) ON DELETE CASCADE,
    device_id TEXT NOT NULL,
    key_version INTEGER NOT NULL,
    envelope TEXT NOT NULL,
    PRIMARY KEY (area_id, device_id)
);
CREATE TABLE records (
    area_id TEXT NOT NULL REFERENCES areas(area_id) ON DELETE CASCADE,
    record_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    seq INTEGER NOT NULL,
    record TEXT NOT NULL,
    PRIMARY KEY (area_id, record_id)
);
CREATE INDEX records_seq ON records(area_id, seq);
";

pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    /// Opens or creates the database file and brings the schema up to date.
    pub async fn open(path: &str) -> Result<Self, StoreError> {
        let path = path.to_owned();
        let conn = tokio::task::spawn_blocking(move || -> Result<Connection, StoreError> {
            let mut conn = Connection::open(&path).map_err(backend)?;
            conn.pragma_update(None, "journal_mode", "WAL")
                .map_err(backend)?;
            conn.pragma_update(None, "foreign_keys", true)
                .map_err(backend)?;
            migrate(&mut conn)?;
            Ok(conn)
        })
        .await
        .map_err(backend)??;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(backend)?;
            f(&conn)
        })
        .await
        .map_err(backend)?
    }

    async fn write<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.lock().map_err(backend)?;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(backend)?;
            let value = f(&tx)?;
            tx.commit().map_err(backend)?;
            Ok(value)
        })
        .await
        .map_err(backend)?
    }
}

fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(backend)?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::Backend("newer schema".into()));
    }
    if version == 0 {
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend)?;
        tx.execute_batch(SCHEMA).map_err(backend)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(backend)?;
        tx.commit().map_err(backend)?;
    }
    Ok(())
}

fn to_json(value: &impl Serialize) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(backend)
}

fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, StoreError> {
    serde_json::from_str(text).map_err(backend)
}

fn account_from(bytes: Vec<u8>) -> Result<AccountPublic, StoreError> {
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| StoreError::Backend("account key length".into()))?;
    Ok(AccountPublic(array))
}

fn uuid_from(text: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(text).map_err(backend)
}

fn random_key() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes[..]);
    BASE64URL_NOPAD.encode(&bytes)
}

const ACCOUNT_COLUMNS: &str = "account, name, email, created_at, locked";

fn account_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(Vec<u8>, String, String, i64, bool)> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
}

fn to_account(raw: (Vec<u8>, String, String, i64, bool)) -> Result<AccountRow, StoreError> {
    let (account, name, email, created_at, locked) = raw;
    Ok(AccountRow {
        account: account_from(account)?,
        name,
        email,
        created_at,
        locked,
    })
}

const DEVICE_COLUMNS: &str =
    "device_id, account, cert, active, admin_blocked, has_logged_in, last_seen";

type RawDevice = (String, Vec<u8>, String, bool, bool, bool, Option<i64>);

fn device_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawDevice> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
    ))
}

fn to_device(raw: RawDevice) -> Result<DeviceRow, StoreError> {
    let (id, account, cert, active, admin_blocked, has_logged_in, last_seen) = raw;
    Ok(DeviceRow {
        device_id: uuid_from(&id)?,
        account: account_from(account)?,
        cert: from_json(&cert)?,
        active,
        admin_blocked,
        has_logged_in,
        last_seen,
    })
}

fn to_area(owner: Vec<u8>, manifest: String) -> Result<AreaRow, StoreError> {
    Ok(AreaRow {
        owner: account_from(owner)?,
        manifest: from_json(&manifest)?,
    })
}

fn insert_device(tx: &Transaction<'_>, cert: &DeviceCertificate) -> Result<(), StoreError> {
    let taken: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM devices WHERE device_id = ?1)",
            [cert.device_id.to_string()],
            |r| r.get(0),
        )
        .map_err(backend)?;
    if taken {
        return Err(StoreError::DeviceIdTaken);
    }
    tx.execute(
        "INSERT INTO devices (device_id, account, cert, active) VALUES (?1, ?2, ?3, 1)",
        params![
            cert.device_id.to_string(),
            cert.account.0.to_vec(),
            to_json(cert)?
        ],
    )
    .map_err(backend)?;
    Ok(())
}

fn owned_area_fates(tx: &Connection, account: &AccountPublic) -> Result<Vec<AreaFate>, StoreError> {
    let mut stmt = tx
        .prepare("SELECT manifest FROM areas WHERE owner = ?1 ORDER BY area_id")
        .map_err(backend)?;
    let manifests = stmt
        .query_map([account.0.to_vec()], |r| r.get::<_, String>(0))
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?;
    let mut stmt = tx
        .prepare("SELECT account FROM accounts WHERE locked = 0 AND account <> ?1")
        .map_err(backend)?;
    let open = stmt
        .query_map([account.0.to_vec()], |r| r.get::<_, Vec<u8>>(0))
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?
        .into_iter()
        .map(account_from)
        .collect::<Result<std::collections::HashSet<_>, _>>()?;
    manifests
        .iter()
        .map(|m| from_json::<AreaManifest>(m).map(|m| area_fate(&m, &open)))
        .collect()
}

#[async_trait::async_trait]
impl Store for SqliteStore {
    async fn api_key(&self) -> Result<(String, bool), StoreError> {
        self.write(|tx| {
            let existing: Option<String> = tx
                .query_row(
                    "SELECT value FROM server_config WHERE key = 'api_key'",
                    [],
                    |r| r.get(0),
                )
                .optional()
                .map_err(backend)?;
            if let Some(key) = existing {
                return Ok((key, false));
            }
            let key = random_key();
            tx.execute(
                "INSERT INTO server_config (key, value) VALUES ('api_key', ?1)",
                [&key],
            )
            .map_err(backend)?;
            Ok((key, true))
        })
        .await
    }

    async fn rotate_api_key(&self) -> Result<String, StoreError> {
        self.write(|tx| {
            let key = random_key();
            tx.execute(
                "INSERT INTO server_config (key, value) VALUES ('api_key', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [&key],
            )
            .map_err(backend)?;
            Ok(key)
        })
        .await
    }

    async fn create_account(
        &self,
        row: &AccountRow,
        email_norm: &str,
        first: &LogEntry,
        device: &DeviceCertificate,
    ) -> Result<(), StoreError> {
        let (row, email_norm, first, device) = (
            row.clone(),
            email_norm.to_owned(),
            first.clone(),
            device.clone(),
        );
        self.write(move |tx| {
            let key = row.account.0.to_vec();
            // A deleted account key stays taken, so deleting a user also keeps them out of the
            // areas whose manifests still name them.
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM accounts WHERE account = ?1)
                     OR EXISTS(SELECT 1 FROM deleted_accounts WHERE account = ?1)",
                    [&key],
                    |r| r.get(0),
                )
                .map_err(backend)?;
            if exists {
                return Err(StoreError::AccountExists);
            }
            let taken: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM accounts WHERE email_norm = ?1)",
                    [&email_norm],
                    |r| r.get(0),
                )
                .map_err(backend)?;
            if taken {
                return Err(StoreError::EmailTaken);
            }
            tx.execute(
                "INSERT INTO accounts (account, name, email, email_norm, created_at, locked)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    key,
                    row.name,
                    row.email,
                    email_norm,
                    row.created_at,
                    row.locked
                ],
            )
            .map_err(backend)?;
            tx.execute(
                "INSERT INTO log_entries (account, seq, entry) VALUES (?1, 0, ?2)",
                params![key, to_json(&first)?],
            )
            .map_err(backend)?;
            insert_device(tx, &device)
        })
        .await
    }

    async fn account(&self, account: &AccountPublic) -> Result<Option<AccountRow>, StoreError> {
        let key = account.0.to_vec();
        self.read(move |c| {
            c.query_row(
                &format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE account = ?1"),
                [key],
                account_row,
            )
            .optional()
            .map_err(backend)?
            .map(to_account)
            .transpose()
        })
        .await
    }

    async fn account_by_email(&self, email_norm: &str) -> Result<Option<AccountRow>, StoreError> {
        let email = email_norm.to_owned();
        self.read(move |c| {
            c.query_row(
                &format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE email_norm = ?1"),
                [email],
                account_row,
            )
            .optional()
            .map_err(backend)?
            .map(to_account)
            .transpose()
        })
        .await
    }

    async fn list_accounts(&self) -> Result<Vec<AccountRow>, StoreError> {
        self.read(|c| {
            let mut stmt = c
                .prepare(&format!(
                    "SELECT {ACCOUNT_COLUMNS} FROM accounts ORDER BY email_norm"
                ))
                .map_err(backend)?;
            let rows = stmt
                .query_map([], account_row)
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)?;
            rows.into_iter().map(to_account).collect()
        })
        .await
    }

    async fn set_locked(&self, account: &AccountPublic, locked: bool) -> Result<(), StoreError> {
        let key = account.0.to_vec();
        self.write(move |tx| {
            let changed = tx
                .execute(
                    "UPDATE accounts SET locked = ?2 WHERE account = ?1",
                    params![key, locked],
                )
                .map_err(backend)?;
            if changed == 0 {
                return Err(StoreError::NotFound);
            }
            if locked {
                tx.execute("DELETE FROM sessions WHERE account = ?1", [key])
                    .map_err(backend)?;
            }
            Ok(())
        })
        .await
    }

    async fn device_log(&self, account: &AccountPublic) -> Result<DeviceLog, StoreError> {
        let key = account.0.to_vec();
        self.read(move |c| {
            let mut stmt = c
                .prepare("SELECT entry FROM log_entries WHERE account = ?1 ORDER BY seq")
                .map_err(backend)?;
            let entries = stmt
                .query_map([key], |r| r.get::<_, String>(0))
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)?;
            Ok(DeviceLog {
                entries: entries
                    .iter()
                    .map(|e| from_json(e))
                    .collect::<Result<_, _>>()?,
            })
        })
        .await
    }

    async fn append_log(
        &self,
        account: &AccountPublic,
        expected_len: u64,
        entries: &[LogEntry],
        changes: &DeviceChanges,
    ) -> Result<(), StoreError> {
        let (key, entries, changes) = (account.0.to_vec(), entries.to_vec(), changes.clone());
        self.write(move |tx| {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM accounts WHERE account = ?1)",
                    [&key],
                    |r| r.get(0),
                )
                .map_err(backend)?;
            if !exists {
                return Err(StoreError::NotFound);
            }
            let len: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM log_entries WHERE account = ?1",
                    [&key],
                    |r| r.get(0),
                )
                .map_err(backend)?;
            if len as u64 != expected_len {
                return Err(StoreError::Conflict {
                    current: len as u64,
                });
            }
            for (offset, entry) in entries.iter().enumerate() {
                tx.execute(
                    "INSERT INTO log_entries (account, seq, entry) VALUES (?1, ?2, ?3)",
                    params![key, len + offset as i64, to_json(entry)?],
                )
                .map_err(backend)?;
            }
            for cert in &changes.added {
                insert_device(tx, cert)?;
            }
            for id in &changes.revoked {
                tx.execute(
                    "UPDATE devices SET active = 0 WHERE device_id = ?1 AND account = ?2",
                    params![id.to_string(), key],
                )
                .map_err(backend)?;
                tx.execute(
                    "DELETE FROM sessions WHERE device_id = ?1",
                    [id.to_string()],
                )
                .map_err(backend)?;
            }
            Ok(())
        })
        .await
    }

    async fn device(&self, device_id: Uuid) -> Result<Option<DeviceRow>, StoreError> {
        self.read(move |c| {
            c.query_row(
                &format!("SELECT {DEVICE_COLUMNS} FROM devices WHERE device_id = ?1"),
                [device_id.to_string()],
                device_row,
            )
            .optional()
            .map_err(backend)?
            .map(to_device)
            .transpose()
        })
        .await
    }

    async fn devices_of(&self, account: &AccountPublic) -> Result<Vec<DeviceRow>, StoreError> {
        let key = account.0.to_vec();
        self.read(move |c| {
            let mut stmt = c
                .prepare(&format!(
                    "SELECT {DEVICE_COLUMNS} FROM devices WHERE account = ?1 ORDER BY rowid"
                ))
                .map_err(backend)?;
            let rows = stmt
                .query_map([key], device_row)
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)?;
            rows.into_iter().map(to_device).collect()
        })
        .await
    }

    async fn mark_logged_in(&self, device_id: Uuid, now: i64) -> Result<(), StoreError> {
        self.write(move |tx| {
            tx.execute(
                "UPDATE devices SET has_logged_in = 1, last_seen = ?2 WHERE device_id = ?1",
                params![device_id.to_string(), now],
            )
            .map_err(backend)?;
            Ok(())
        })
        .await
    }

    async fn touch_device(&self, device_id: Uuid, now: i64) -> Result<(), StoreError> {
        self.write(move |tx| {
            tx.execute(
                "UPDATE devices SET last_seen = ?2 WHERE device_id = ?1",
                params![device_id.to_string(), now],
            )
            .map_err(backend)?;
            Ok(())
        })
        .await
    }

    async fn set_admin_blocked(&self, device_id: Uuid, blocked: bool) -> Result<(), StoreError> {
        self.write(move |tx| {
            let id = device_id.to_string();
            let changed = tx
                .execute(
                    "UPDATE devices SET admin_blocked = ?2 WHERE device_id = ?1",
                    params![id, blocked],
                )
                .map_err(backend)?;
            if changed == 0 {
                return Err(StoreError::NotFound);
            }
            if blocked {
                tx.execute("DELETE FROM sessions WHERE device_id = ?1", [id])
                    .map_err(backend)?;
            }
            Ok(())
        })
        .await
    }

    async fn create_session(
        &self,
        token_hash: [u8; 32],
        device_id: Uuid,
        account: &AccountPublic,
        expires_at: i64,
    ) -> Result<(), StoreError> {
        let key = account.0.to_vec();
        self.write(move |tx| {
            tx.execute(
                "INSERT INTO sessions (token_hash, device_id, account, expires_at) VALUES (?1, ?2, ?3, ?4)",
                params![token_hash.to_vec(), device_id.to_string(), key, expires_at],
            )
            .map_err(backend)?;
            Ok(())
        })
        .await
    }

    async fn session(&self, token_hash: [u8; 32]) -> Result<Option<SessionRow>, StoreError> {
        self.read(move |c| {
            let raw: Option<(String, Vec<u8>, i64)> = c
                .query_row(
                    "SELECT device_id, account, expires_at FROM sessions WHERE token_hash = ?1",
                    [token_hash.to_vec()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(backend)?;
            raw.map(|(device, account, expires_at)| {
                Ok(SessionRow {
                    device_id: uuid_from(&device)?,
                    account: account_from(account)?,
                    expires_at,
                })
            })
            .transpose()
        })
        .await
    }

    async fn delete_expired_sessions(&self, now: i64) -> Result<u64, StoreError> {
        self.write(move |tx| {
            let n = tx
                .execute("DELETE FROM sessions WHERE expires_at <= ?1", [now])
                .map_err(backend)?;
            Ok(n as u64)
        })
        .await
    }

    async fn area(&self, area_id: AreaId) -> Result<Option<AreaRow>, StoreError> {
        self.read(move |c| {
            let raw: Option<(Vec<u8>, String)> = c
                .query_row(
                    "SELECT owner, manifest FROM areas WHERE area_id = ?1",
                    [area_id.0.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(backend)?;
            raw.map(|(owner, manifest)| to_area(owner, manifest))
                .transpose()
        })
        .await
    }

    async fn areas_of(&self, account: &AccountPublic) -> Result<Vec<AreaRow>, StoreError> {
        let key = account.0.to_vec();
        self.read(move |c| {
            let mut stmt = c
                .prepare(
                    "SELECT owner, manifest FROM areas WHERE owner = ?1
                     OR area_id IN (SELECT area_id FROM area_members WHERE account = ?1)
                     ORDER BY area_id",
                )
                .map_err(backend)?;
            let rows = stmt
                .query_map([key], |r| {
                    Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)?;
            rows.into_iter().map(|(o, m)| to_area(o, m)).collect()
        })
        .await
    }

    async fn area_peers(&self, account: &AccountPublic) -> Result<Vec<AccountPublic>, StoreError> {
        let key = account.0.to_vec();
        self.read(move |c| {
            let mut stmt = c
                .prepare(
                    "WITH mine AS (
                         SELECT area_id FROM areas WHERE owner = ?1
                         UNION SELECT area_id FROM area_members WHERE account = ?1
                     )
                     SELECT owner FROM areas WHERE area_id IN (SELECT area_id FROM mine)
                     UNION SELECT account FROM area_members WHERE area_id IN (SELECT area_id FROM mine)",
                )
                .map_err(backend)?;
            let rows = stmt
                .query_map([&key], |r| r.get::<_, Vec<u8>>(0))
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)?;
            rows.into_iter()
                .filter(|peer| *peer != key)
                .map(account_from)
                .collect()
        })
        .await
    }

    async fn put_manifest(
        &self,
        owner: &AccountPublic,
        manifest: &AreaManifest,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        let (owner, manifest) = (owner.0.to_vec(), manifest.clone());
        self.write(move |tx| {
            let id = manifest.area_id.0.to_string();
            let stored: Option<(i64, i64)> = tx
                .query_row(
                    "SELECT version, key_version FROM areas WHERE area_id = ?1",
                    [&id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(backend)?;
            let current = stored.map(|(v, _)| v as u64);
            let required = current.map_or(1, |v| v + 1);
            if current != expected || manifest.version != required {
                return Err(StoreError::Conflict {
                    current: current.unwrap_or(0),
                });
            }
            let json = to_json(&manifest)?;
            tx.execute(
                "INSERT INTO areas (area_id, owner, version, key_version, manifest)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(area_id) DO UPDATE SET owner = excluded.owner, version = excluded.version,
                     key_version = excluded.key_version, manifest = excluded.manifest",
                params![id, owner, manifest.version as i64, manifest.key_version, json],
            )
            .map_err(backend)?;
            tx.execute(
                "DELETE FROM envelopes WHERE area_id = ?1 AND key_version < ?2",
                params![id, manifest.key_version],
            )
            .map_err(backend)?;
            tx.execute("DELETE FROM area_members WHERE area_id = ?1", [&id])
                .map_err(backend)?;
            for member in &manifest.members {
                tx.execute(
                    "INSERT OR REPLACE INTO area_members (area_id, account, role) VALUES (?1, ?2, ?3)",
                    params![id, member.account.0.to_vec(), role_name(member.role)],
                )
                .map_err(backend)?;
            }
            Ok(())
        })
        .await
    }

    async fn delete_area(&self, area_id: AreaId) -> Result<(), StoreError> {
        self.write(move |tx| {
            let n = tx
                .execute(
                    "DELETE FROM areas WHERE area_id = ?1",
                    [area_id.0.to_string()],
                )
                .map_err(backend)?;
            if n == 0 {
                return Err(StoreError::NotFound);
            }
            Ok(())
        })
        .await
    }

    async fn put_envelopes(
        &self,
        area_id: AreaId,
        envelopes: &[KeyEnvelope],
    ) -> Result<(), StoreError> {
        let envelopes = envelopes.to_vec();
        self.write(move |tx| {
            let id = area_id.0.to_string();
            let exists: bool = tx
                .query_row("SELECT EXISTS(SELECT 1 FROM areas WHERE area_id = ?1)", [&id], |r| r.get(0))
                .map_err(backend)?;
            if !exists {
                return Err(StoreError::NotFound);
            }
            for envelope in &envelopes {
                tx.execute(
                    "INSERT INTO envelopes (area_id, device_id, key_version, envelope) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(area_id, device_id) DO UPDATE SET key_version = excluded.key_version,
                         envelope = excluded.envelope",
                    params![id, envelope.device_id.to_string(), envelope.key_version, to_json(envelope)?],
                )
                .map_err(backend)?;
            }
            Ok(())
        })
        .await
    }

    async fn envelope_for(
        &self,
        area_id: AreaId,
        device_id: Uuid,
    ) -> Result<Option<KeyEnvelope>, StoreError> {
        self.read(move |c| {
            let raw: Option<String> = c
                .query_row(
                    "SELECT envelope FROM envelopes WHERE area_id = ?1 AND device_id = ?2",
                    params![area_id.0.to_string(), device_id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(backend)?;
            raw.map(|e| from_json(&e)).transpose()
        })
        .await
    }

    async fn put_record(&self, record: &SealedRecord) -> Result<u64, StoreError> {
        let record = record.clone();
        self.write(move |tx| {
            let area = record.header.area_id.0.to_string();
            let id = record.header.record_id.to_string();
            let exists: bool = tx
                .query_row("SELECT EXISTS(SELECT 1 FROM areas WHERE area_id = ?1)", [&area], |r| r.get(0))
                .map_err(backend)?;
            if !exists {
                return Err(StoreError::NotFound);
            }
            let stored: Option<i64> = tx
                .query_row(
                    "SELECT version FROM records WHERE area_id = ?1 AND record_id = ?2",
                    params![area, id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(backend)?;
            let current = stored.map(|v| v as u64);
            if record.header.version != current.map_or(1, |v| v + 1) {
                return Err(StoreError::Conflict {
                    current: current.unwrap_or(0),
                });
            }
            let seq: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(seq), 0) + 1 FROM records WHERE area_id = ?1",
                    [&area],
                    |r| r.get(0),
                )
                .map_err(backend)?;
            tx.execute(
                "INSERT INTO records (area_id, record_id, version, seq, record) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(area_id, record_id) DO UPDATE SET version = excluded.version,
                     seq = excluded.seq, record = excluded.record",
                params![area, id, record.header.version as i64, seq, to_json(&record)?],
            )
            .map_err(backend)?;
            Ok(seq as u64)
        })
        .await
    }

    async fn records_since(
        &self,
        area_id: AreaId,
        since: u64,
        limit: u32,
    ) -> Result<Vec<StoredRecord>, StoreError> {
        self.read(move |c| {
            let mut stmt = c
                .prepare(
                    "SELECT seq, record FROM records WHERE area_id = ?1 AND seq > ?2
                     ORDER BY seq LIMIT ?3",
                )
                .map_err(backend)?;
            let rows = stmt
                .query_map(params![area_id.0.to_string(), since as i64, limit], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(backend)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(backend)?;
            rows.into_iter()
                .map(|(seq, record)| {
                    Ok(StoredRecord {
                        seq: seq as u64,
                        record: from_json(&record)?,
                    })
                })
                .collect()
        })
        .await
    }

    async fn plan_account_deletion(
        &self,
        account: &AccountPublic,
    ) -> Result<Vec<AreaFate>, StoreError> {
        let account = *account;
        self.read(move |c| owned_area_fates(c, &account)).await
    }

    async fn delete_account(&self, account: &AccountPublic) -> Result<Vec<AreaFate>, StoreError> {
        let account = *account;
        self.write(move |tx| {
            let key = account.0.to_vec();
            let exists: bool = tx
                .query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE account = ?1)", [&key], |r| r.get(0))
                .map_err(backend)?;
            if !exists {
                return Err(StoreError::NotFound);
            }
            let plan = owned_area_fates(tx, &account)?;
            for fate in &plan {
                let id = fate.area_id.0.to_string();
                match &fate.outcome {
                    AreaOutcome::TransferTo(heir) => {
                        tx.execute(
                            "UPDATE areas SET owner = ?2 WHERE area_id = ?1",
                            params![id, heir.0.to_vec()],
                        )
                        .map_err(backend)?;
                        tx.execute(
                            "DELETE FROM area_members WHERE area_id = ?1 AND account = ?2",
                            params![id, heir.0.to_vec()],
                        )
                        .map_err(backend)?;
                    }
                    AreaOutcome::Delete => {
                        tx.execute("DELETE FROM areas WHERE area_id = ?1", [id])
                            .map_err(backend)?;
                    }
                }
            }
            tx.execute("DELETE FROM area_members WHERE account = ?1", [&key])
                .map_err(backend)?;
            tx.execute(
                "DELETE FROM envelopes WHERE device_id IN (SELECT device_id FROM devices WHERE account = ?1)",
                [&key],
            )
            .map_err(backend)?;
            tx.execute("INSERT OR IGNORE INTO deleted_accounts (account) VALUES (?1)", [&key])
                .map_err(backend)?;
            tx.execute("DELETE FROM accounts WHERE account = ?1", [&key])
                .map_err(backend)?;
            Ok(plan)
        })
        .await
    }
}

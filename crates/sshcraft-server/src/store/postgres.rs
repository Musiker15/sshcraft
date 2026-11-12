//! PostgreSQL backend. Compare and swap works by locking the account or area row
//! (`SELECT ... FOR UPDATE`) inside the transaction; new rows are inserted with
//! `ON CONFLICT DO NOTHING` and the affected row count decides who won.
//!
//! The connection uses no TLS: the database is expected on the same host (see docs/server.md).

use super::{
    AccountRow, AreaFate, AreaOutcome, AreaRow, DeviceChanges, DeviceRow, SessionRow, Store,
    StoreError, StoredRecord, area_fate, backend, role_name,
};
use data_encoding::BASE64URL_NOPAD;
use deadpool_postgres::{GenericClient, Manager, ManagerConfig, Pool, RecyclingMethod};
use rand::RngExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sshcraft_proto::area::{AreaId, AreaManifest, KeyEnvelope};
use sshcraft_proto::devicelog::{DeviceLog, LogEntry};
use sshcraft_proto::identity::{AccountPublic, DeviceCertificate};
use sshcraft_proto::record::SealedRecord;
use tokio_postgres::error::SqlState;
use tokio_postgres::{NoTls, Row};
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS schema_version (version BIGINT NOT NULL);
CREATE TABLE IF NOT EXISTS server_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS deleted_accounts (account BYTEA PRIMARY KEY);
CREATE TABLE IF NOT EXISTS accounts (
    account BYTEA PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    email_norm TEXT NOT NULL CONSTRAINT accounts_email_norm_key UNIQUE,
    created_at BIGINT NOT NULL,
    locked BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE TABLE IF NOT EXISTS log_entries (
    account BYTEA NOT NULL REFERENCES accounts(account) ON DELETE CASCADE,
    seq BIGINT NOT NULL,
    entry TEXT NOT NULL,
    PRIMARY KEY (account, seq)
);
CREATE TABLE IF NOT EXISTS devices (
    device_id UUID CONSTRAINT devices_pkey PRIMARY KEY,
    account BYTEA NOT NULL REFERENCES accounts(account) ON DELETE CASCADE,
    cert TEXT NOT NULL,
    active BOOLEAN NOT NULL,
    admin_blocked BOOLEAN NOT NULL DEFAULT FALSE,
    has_logged_in BOOLEAN NOT NULL DEFAULT FALSE,
    last_seen BIGINT,
    added BIGSERIAL
);
CREATE INDEX IF NOT EXISTS devices_account ON devices(account);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash BYTEA PRIMARY KEY,
    device_id UUID NOT NULL REFERENCES devices(device_id) ON DELETE CASCADE,
    account BYTEA NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_device ON sessions(device_id);
CREATE TABLE IF NOT EXISTS areas (
    area_id UUID PRIMARY KEY,
    owner BYTEA NOT NULL,
    version BIGINT NOT NULL,
    key_version BIGINT NOT NULL,
    manifest TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS areas_owner ON areas(owner);
CREATE TABLE IF NOT EXISTS area_members (
    area_id UUID NOT NULL REFERENCES areas(area_id) ON DELETE CASCADE,
    account BYTEA NOT NULL,
    role TEXT NOT NULL,
    PRIMARY KEY (area_id, account)
);
CREATE INDEX IF NOT EXISTS area_members_account ON area_members(account);
CREATE TABLE IF NOT EXISTS envelopes (
    area_id UUID NOT NULL REFERENCES areas(area_id) ON DELETE CASCADE,
    device_id UUID NOT NULL,
    key_version BIGINT NOT NULL,
    envelope TEXT NOT NULL,
    PRIMARY KEY (area_id, device_id)
);
CREATE TABLE IF NOT EXISTS records (
    area_id UUID NOT NULL REFERENCES areas(area_id) ON DELETE CASCADE,
    record_id UUID NOT NULL,
    version BIGINT NOT NULL,
    seq BIGINT NOT NULL,
    record TEXT NOT NULL,
    PRIMARY KEY (area_id, record_id)
);
CREATE INDEX IF NOT EXISTS records_seq ON records(area_id, seq);
";

pub struct PostgresStore {
    pool: Pool,
}

impl PostgresStore {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let config: tokio_postgres::Config = url.parse().map_err(backend)?;
        let manager = Manager::from_config(
            config,
            NoTls,
            ManagerConfig {
                recycling_method: RecyclingMethod::Fast,
            },
        );
        let pool = Pool::builder(manager)
            .max_size(16)
            .build()
            .map_err(backend)?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    async fn client(&self) -> Result<deadpool_postgres::Client, StoreError> {
        self.pool.get().await.map_err(backend)
    }

    async fn migrate(&self) -> Result<(), StoreError> {
        let mut client = self.client().await?;
        let tx = client.transaction().await.map_err(pg)?;
        tx.batch_execute(SCHEMA).await.map_err(pg)?;
        let version: Option<i64> = tx
            .query_opt("SELECT version FROM schema_version", &[])
            .await
            .map_err(pg)?
            .map(|r| r.get(0));
        match version {
            Some(v) if v > SCHEMA_VERSION => {
                return Err(StoreError::Backend("newer schema".into()));
            }
            Some(_) => {}
            None => {
                tx.execute(
                    "INSERT INTO schema_version (version) VALUES ($1)",
                    &[&SCHEMA_VERSION],
                )
                .await
                .map_err(pg)?;
            }
        }
        tx.commit().await.map_err(pg)
    }
}

fn pg(err: tokio_postgres::Error) -> StoreError {
    if err.code() == Some(&SqlState::UNIQUE_VIOLATION) {
        let constraint = err
            .as_db_error()
            .and_then(|e| e.constraint())
            .unwrap_or_default()
            .to_owned();
        return match constraint.as_str() {
            "accounts_email_norm_key" => StoreError::EmailTaken,
            "accounts_pkey" => StoreError::AccountExists,
            "devices_pkey" => StoreError::DeviceIdTaken,
            _ => StoreError::Conflict { current: 0 },
        };
    }
    backend(err)
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

fn random_key() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes[..]);
    BASE64URL_NOPAD.encode(&bytes)
}

const ACCOUNT_COLUMNS: &str = "account, name, email, created_at, locked";

fn to_account(r: &Row) -> Result<AccountRow, StoreError> {
    Ok(AccountRow {
        account: account_from(r.get(0))?,
        name: r.get(1),
        email: r.get(2),
        created_at: r.get(3),
        locked: r.get(4),
    })
}

const DEVICE_COLUMNS: &str =
    "device_id, account, cert, active, admin_blocked, has_logged_in, last_seen";

fn to_device(r: &Row) -> Result<DeviceRow, StoreError> {
    Ok(DeviceRow {
        device_id: r.get(0),
        account: account_from(r.get(1))?,
        cert: from_json(r.get::<_, &str>(2))?,
        active: r.get(3),
        admin_blocked: r.get(4),
        has_logged_in: r.get(5),
        last_seen: r.get(6),
    })
}

fn to_area(r: &Row) -> Result<AreaRow, StoreError> {
    Ok(AreaRow {
        owner: account_from(r.get(0))?,
        manifest: from_json(r.get::<_, &str>(1))?,
    })
}

async fn exists(
    c: &impl GenericClient,
    sql: &str,
    param: &(dyn tokio_postgres::types::ToSql + Sync),
) -> Result<bool, StoreError> {
    Ok(c.query_opt(sql, &[param]).await.map_err(pg)?.is_some())
}

async fn insert_device(c: &impl GenericClient, cert: &DeviceCertificate) -> Result<(), StoreError> {
    if exists(
        c,
        "SELECT 1 FROM devices WHERE device_id = $1",
        &cert.device_id,
    )
    .await?
    {
        return Err(StoreError::DeviceIdTaken);
    }
    c.execute(
        "INSERT INTO devices (device_id, account, cert, active) VALUES ($1, $2, $3, TRUE)",
        &[&cert.device_id, &cert.account.0.as_slice(), &to_json(cert)?],
    )
    .await
    .map_err(pg)?;
    Ok(())
}

async fn owned_area_fates(
    c: &impl GenericClient,
    account: &AccountPublic,
) -> Result<Vec<AreaFate>, StoreError> {
    let rows = c
        .query(
            "SELECT manifest FROM areas WHERE owner = $1 ORDER BY area_id",
            &[&account.0.as_slice()],
        )
        .await
        .map_err(pg)?;
    let open = c
        .query(
            "SELECT account FROM accounts WHERE NOT locked AND account <> $1",
            &[&account.0.as_slice()],
        )
        .await
        .map_err(pg)?
        .iter()
        .map(|r| account_from(r.get(0)))
        .collect::<Result<std::collections::HashSet<_>, _>>()?;
    rows.iter()
        .map(|r| from_json::<AreaManifest>(r.get::<_, &str>(0)).map(|m| area_fate(&m, &open)))
        .collect()
}

#[async_trait::async_trait]
impl Store for PostgresStore {
    async fn api_key(&self) -> Result<(String, bool), StoreError> {
        let c = self.client().await?;
        let created = c
            .execute(
                "INSERT INTO server_config (key, value) VALUES ('api_key', $1) ON CONFLICT (key) DO NOTHING",
                &[&random_key()],
            )
            .await
            .map_err(pg)?
            == 1;
        let row = c
            .query_one("SELECT value FROM server_config WHERE key = 'api_key'", &[])
            .await
            .map_err(pg)?;
        Ok((row.get(0), created))
    }

    async fn rotate_api_key(&self) -> Result<String, StoreError> {
        let key = random_key();
        self.client()
            .await?
            .execute(
                "INSERT INTO server_config (key, value) VALUES ('api_key', $1)
                 ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
                &[&key],
            )
            .await
            .map_err(pg)?;
        Ok(key)
    }

    async fn create_account(
        &self,
        row: &AccountRow,
        email_norm: &str,
        first: &LogEntry,
        device: &DeviceCertificate,
    ) -> Result<(), StoreError> {
        let mut c = self.client().await?;
        let tx = c.transaction().await.map_err(pg)?;
        let key = row.account.0.as_slice();
        // A deleted account key stays taken, so deleting a user also keeps them out of the areas
        // whose manifests still name them.
        if exists(&tx, "SELECT 1 FROM accounts WHERE account = $1", &key).await?
            || exists(
                &tx,
                "SELECT 1 FROM deleted_accounts WHERE account = $1",
                &key,
            )
            .await?
        {
            return Err(StoreError::AccountExists);
        }
        if exists(
            &tx,
            "SELECT 1 FROM accounts WHERE email_norm = $1",
            &email_norm,
        )
        .await?
        {
            return Err(StoreError::EmailTaken);
        }
        tx.execute(
            "INSERT INTO accounts (account, name, email, email_norm, created_at, locked)
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &key,
                &row.name,
                &row.email,
                &email_norm,
                &row.created_at,
                &row.locked,
            ],
        )
        .await
        .map_err(pg)?;
        tx.execute(
            "INSERT INTO log_entries (account, seq, entry) VALUES ($1, 0, $2)",
            &[&key, &to_json(first)?],
        )
        .await
        .map_err(pg)?;
        insert_device(&tx, device).await?;
        tx.commit().await.map_err(pg)
    }

    async fn account(&self, account: &AccountPublic) -> Result<Option<AccountRow>, StoreError> {
        self.client()
            .await?
            .query_opt(
                &format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE account = $1"),
                &[&account.0.as_slice()],
            )
            .await
            .map_err(pg)?
            .as_ref()
            .map(to_account)
            .transpose()
    }

    async fn account_by_email(&self, email_norm: &str) -> Result<Option<AccountRow>, StoreError> {
        self.client()
            .await?
            .query_opt(
                &format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE email_norm = $1"),
                &[&email_norm],
            )
            .await
            .map_err(pg)?
            .as_ref()
            .map(to_account)
            .transpose()
    }

    async fn list_accounts(&self) -> Result<Vec<AccountRow>, StoreError> {
        self.client()
            .await?
            .query(
                &format!("SELECT {ACCOUNT_COLUMNS} FROM accounts ORDER BY email_norm"),
                &[],
            )
            .await
            .map_err(pg)?
            .iter()
            .map(to_account)
            .collect()
    }

    async fn set_locked(&self, account: &AccountPublic, locked: bool) -> Result<(), StoreError> {
        let mut c = self.client().await?;
        let tx = c.transaction().await.map_err(pg)?;
        let key = account.0.as_slice();
        let n = tx
            .execute(
                "UPDATE accounts SET locked = $2 WHERE account = $1",
                &[&key, &locked],
            )
            .await
            .map_err(pg)?;
        if n == 0 {
            return Err(StoreError::NotFound);
        }
        if locked {
            tx.execute("DELETE FROM sessions WHERE account = $1", &[&key])
                .await
                .map_err(pg)?;
        }
        tx.commit().await.map_err(pg)
    }

    async fn device_log(&self, account: &AccountPublic) -> Result<DeviceLog, StoreError> {
        let rows = self
            .client()
            .await?
            .query(
                "SELECT entry FROM log_entries WHERE account = $1 ORDER BY seq",
                &[&account.0.as_slice()],
            )
            .await
            .map_err(pg)?;
        Ok(DeviceLog {
            entries: rows
                .iter()
                .map(|r| from_json(r.get::<_, &str>(0)))
                .collect::<Result<_, _>>()?,
        })
    }

    async fn append_log(
        &self,
        account: &AccountPublic,
        expected_len: u64,
        entries: &[LogEntry],
        changes: &DeviceChanges,
    ) -> Result<(), StoreError> {
        let mut c = self.client().await?;
        let tx = c.transaction().await.map_err(pg)?;
        let key = account.0.as_slice();
        if !exists(
            &tx,
            "SELECT 1 FROM accounts WHERE account = $1 FOR UPDATE",
            &key,
        )
        .await?
        {
            return Err(StoreError::NotFound);
        }
        let len: i64 = tx
            .query_one(
                "SELECT COUNT(*) FROM log_entries WHERE account = $1",
                &[&key],
            )
            .await
            .map_err(pg)?
            .get(0);
        if len as u64 != expected_len {
            return Err(StoreError::Conflict {
                current: len as u64,
            });
        }
        for (offset, entry) in entries.iter().enumerate() {
            tx.execute(
                "INSERT INTO log_entries (account, seq, entry) VALUES ($1, $2, $3)",
                &[&key, &(len + offset as i64), &to_json(entry)?],
            )
            .await
            .map_err(pg)?;
        }
        for cert in &changes.added {
            insert_device(&tx, cert).await?;
        }
        for id in &changes.revoked {
            tx.execute(
                "UPDATE devices SET active = FALSE WHERE device_id = $1 AND account = $2",
                &[id, &key],
            )
            .await
            .map_err(pg)?;
            tx.execute("DELETE FROM sessions WHERE device_id = $1", &[id])
                .await
                .map_err(pg)?;
        }
        tx.commit().await.map_err(pg)
    }

    async fn device(&self, device_id: Uuid) -> Result<Option<DeviceRow>, StoreError> {
        self.client()
            .await?
            .query_opt(
                &format!("SELECT {DEVICE_COLUMNS} FROM devices WHERE device_id = $1"),
                &[&device_id],
            )
            .await
            .map_err(pg)?
            .as_ref()
            .map(to_device)
            .transpose()
    }

    async fn devices_of(&self, account: &AccountPublic) -> Result<Vec<DeviceRow>, StoreError> {
        self.client()
            .await?
            .query(
                &format!("SELECT {DEVICE_COLUMNS} FROM devices WHERE account = $1 ORDER BY added"),
                &[&account.0.as_slice()],
            )
            .await
            .map_err(pg)?
            .iter()
            .map(to_device)
            .collect()
    }

    async fn mark_logged_in(&self, device_id: Uuid, now: i64) -> Result<(), StoreError> {
        self.client()
            .await?
            .execute(
                "UPDATE devices SET has_logged_in = TRUE, last_seen = $2 WHERE device_id = $1",
                &[&device_id, &now],
            )
            .await
            .map_err(pg)?;
        Ok(())
    }

    async fn touch_device(&self, device_id: Uuid, now: i64) -> Result<(), StoreError> {
        self.client()
            .await?
            .execute(
                "UPDATE devices SET last_seen = $2 WHERE device_id = $1",
                &[&device_id, &now],
            )
            .await
            .map_err(pg)?;
        Ok(())
    }

    async fn set_admin_blocked(&self, device_id: Uuid, blocked: bool) -> Result<(), StoreError> {
        let mut c = self.client().await?;
        let tx = c.transaction().await.map_err(pg)?;
        let n = tx
            .execute(
                "UPDATE devices SET admin_blocked = $2 WHERE device_id = $1",
                &[&device_id, &blocked],
            )
            .await
            .map_err(pg)?;
        if n == 0 {
            return Err(StoreError::NotFound);
        }
        if blocked {
            tx.execute("DELETE FROM sessions WHERE device_id = $1", &[&device_id])
                .await
                .map_err(pg)?;
        }
        tx.commit().await.map_err(pg)
    }

    async fn create_session(
        &self,
        token_hash: [u8; 32],
        device_id: Uuid,
        account: &AccountPublic,
        expires_at: i64,
    ) -> Result<(), StoreError> {
        self.client()
            .await?
            .execute(
                "INSERT INTO sessions (token_hash, device_id, account, expires_at) VALUES ($1, $2, $3, $4)",
                &[&token_hash.as_slice(), &device_id, &account.0.as_slice(), &expires_at],
            )
            .await
            .map_err(pg)?;
        Ok(())
    }

    async fn session(&self, token_hash: [u8; 32]) -> Result<Option<SessionRow>, StoreError> {
        let row = self
            .client()
            .await?
            .query_opt(
                "SELECT device_id, account, expires_at FROM sessions WHERE token_hash = $1",
                &[&token_hash.as_slice()],
            )
            .await
            .map_err(pg)?;
        row.map(|r| {
            Ok(SessionRow {
                device_id: r.get(0),
                account: account_from(r.get(1))?,
                expires_at: r.get(2),
            })
        })
        .transpose()
    }

    async fn delete_expired_sessions(&self, now: i64) -> Result<u64, StoreError> {
        self.client()
            .await?
            .execute("DELETE FROM sessions WHERE expires_at <= $1", &[&now])
            .await
            .map_err(pg)
    }

    async fn area(&self, area_id: AreaId) -> Result<Option<AreaRow>, StoreError> {
        self.client()
            .await?
            .query_opt(
                "SELECT owner, manifest FROM areas WHERE area_id = $1",
                &[&area_id.0],
            )
            .await
            .map_err(pg)?
            .as_ref()
            .map(to_area)
            .transpose()
    }

    async fn areas_of(&self, account: &AccountPublic) -> Result<Vec<AreaRow>, StoreError> {
        self.client()
            .await?
            .query(
                "SELECT owner, manifest FROM areas WHERE owner = $1
                 OR area_id IN (SELECT area_id FROM area_members WHERE account = $1)
                 ORDER BY area_id",
                &[&account.0.as_slice()],
            )
            .await
            .map_err(pg)?
            .iter()
            .map(to_area)
            .collect()
    }

    async fn area_peers(&self, account: &AccountPublic) -> Result<Vec<AccountPublic>, StoreError> {
        let key = account.0.as_slice();
        let rows = self
            .client()
            .await?
            .query(
                "WITH mine AS (
                     SELECT area_id FROM areas WHERE owner = $1
                     UNION SELECT area_id FROM area_members WHERE account = $1
                 )
                 SELECT owner FROM areas WHERE area_id IN (SELECT area_id FROM mine)
                 UNION SELECT account FROM area_members WHERE area_id IN (SELECT area_id FROM mine)",
                &[&key],
            )
            .await
            .map_err(pg)?;
        rows.iter()
            .map(|r| r.get::<_, Vec<u8>>(0))
            .filter(|peer| peer.as_slice() != key)
            .map(account_from)
            .collect()
    }

    async fn put_manifest(
        &self,
        owner: &AccountPublic,
        manifest: &AreaManifest,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        let mut c = self.client().await?;
        let tx = c.transaction().await.map_err(pg)?;
        let id = manifest.area_id.0;
        let current: Option<u64> = tx
            .query_opt(
                "SELECT version FROM areas WHERE area_id = $1 FOR UPDATE",
                &[&id],
            )
            .await
            .map_err(pg)?
            .map(|r| r.get::<_, i64>(0) as u64);
        if current != expected || manifest.version != current.map_or(1, |v| v + 1) {
            return Err(StoreError::Conflict {
                current: current.unwrap_or(0),
            });
        }
        let (version, key_version) = (manifest.version as i64, i64::from(manifest.key_version));
        let json = to_json(manifest)?;
        let owner = owner.0.as_slice();
        if current.is_none() {
            let inserted = tx
                .execute(
                    "INSERT INTO areas (area_id, owner, version, key_version, manifest)
                     VALUES ($1, $2, $3, $4, $5) ON CONFLICT (area_id) DO NOTHING",
                    &[&id, &owner, &version, &key_version, &json],
                )
                .await
                .map_err(pg)?;
            if inserted == 0 {
                // Someone created the area between our read and our insert.
                return Err(StoreError::Conflict { current: 1 });
            }
        } else {
            tx.execute(
                "UPDATE areas SET owner = $2, version = $3, key_version = $4, manifest = $5
                 WHERE area_id = $1",
                &[&id, &owner, &version, &key_version, &json],
            )
            .await
            .map_err(pg)?;
        }
        tx.execute(
            "DELETE FROM envelopes WHERE area_id = $1 AND key_version < $2",
            &[&id, &key_version],
        )
        .await
        .map_err(pg)?;
        tx.execute("DELETE FROM area_members WHERE area_id = $1", &[&id])
            .await
            .map_err(pg)?;
        for member in &manifest.members {
            tx.execute(
                "INSERT INTO area_members (area_id, account, role) VALUES ($1, $2, $3)
                 ON CONFLICT (area_id, account) DO UPDATE SET role = EXCLUDED.role",
                &[&id, &member.account.0.as_slice(), &role_name(member.role)],
            )
            .await
            .map_err(pg)?;
        }
        tx.commit().await.map_err(pg)
    }

    async fn delete_area(&self, area_id: AreaId) -> Result<(), StoreError> {
        let n = self
            .client()
            .await?
            .execute("DELETE FROM areas WHERE area_id = $1", &[&area_id.0])
            .await
            .map_err(pg)?;
        if n == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn put_envelopes(
        &self,
        area_id: AreaId,
        envelopes: &[KeyEnvelope],
    ) -> Result<(), StoreError> {
        let mut c = self.client().await?;
        let tx = c.transaction().await.map_err(pg)?;
        if !exists(
            &tx,
            "SELECT 1 FROM areas WHERE area_id = $1 FOR UPDATE",
            &area_id.0,
        )
        .await?
        {
            return Err(StoreError::NotFound);
        }
        for envelope in envelopes {
            tx.execute(
                "INSERT INTO envelopes (area_id, device_id, key_version, envelope) VALUES ($1, $2, $3, $4)
                 ON CONFLICT (area_id, device_id) DO UPDATE SET key_version = EXCLUDED.key_version,
                     envelope = EXCLUDED.envelope",
                &[
                    &area_id.0,
                    &envelope.device_id,
                    &i64::from(envelope.key_version),
                    &to_json(envelope)?,
                ],
            )
            .await
            .map_err(pg)?;
        }
        tx.commit().await.map_err(pg)
    }

    async fn envelope_for(
        &self,
        area_id: AreaId,
        device_id: Uuid,
    ) -> Result<Option<KeyEnvelope>, StoreError> {
        self.client()
            .await?
            .query_opt(
                "SELECT envelope FROM envelopes WHERE area_id = $1 AND device_id = $2",
                &[&area_id.0, &device_id],
            )
            .await
            .map_err(pg)?
            .map(|r| from_json(r.get::<_, &str>(0)))
            .transpose()
    }

    async fn put_record(&self, record: &SealedRecord) -> Result<u64, StoreError> {
        let mut c = self.client().await?;
        let tx = c.transaction().await.map_err(pg)?;
        let (area, id) = (record.header.area_id.0, record.header.record_id);
        // Locking the area row serializes all writes in this area, which also keeps `seq` gapless.
        if !exists(
            &tx,
            "SELECT 1 FROM areas WHERE area_id = $1 FOR UPDATE",
            &area,
        )
        .await?
        {
            return Err(StoreError::NotFound);
        }
        let current: Option<u64> = tx
            .query_opt(
                "SELECT version FROM records WHERE area_id = $1 AND record_id = $2",
                &[&area, &id],
            )
            .await
            .map_err(pg)?
            .map(|r| r.get::<_, i64>(0) as u64);
        if record.header.version != current.map_or(1, |v| v + 1) {
            return Err(StoreError::Conflict {
                current: current.unwrap_or(0),
            });
        }
        let seq: i64 = tx
            .query_one(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM records WHERE area_id = $1",
                &[&area],
            )
            .await
            .map_err(pg)?
            .get(0);
        tx.execute(
            "INSERT INTO records (area_id, record_id, version, seq, record) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (area_id, record_id) DO UPDATE SET version = EXCLUDED.version,
                 seq = EXCLUDED.seq, record = EXCLUDED.record",
            &[&area, &id, &(record.header.version as i64), &seq, &to_json(record)?],
        )
        .await
        .map_err(pg)?;
        tx.commit().await.map_err(pg)?;
        Ok(seq as u64)
    }

    async fn records_since(
        &self,
        area_id: AreaId,
        since: u64,
        limit: u32,
    ) -> Result<Vec<StoredRecord>, StoreError> {
        self.client()
            .await?
            .query(
                "SELECT seq, record FROM records WHERE area_id = $1 AND seq > $2
                 ORDER BY seq LIMIT $3",
                &[&area_id.0, &(since as i64), &i64::from(limit)],
            )
            .await
            .map_err(pg)?
            .iter()
            .map(|r| {
                Ok(StoredRecord {
                    seq: r.get::<_, i64>(0) as u64,
                    record: from_json(r.get::<_, &str>(1))?,
                })
            })
            .collect()
    }

    async fn plan_account_deletion(
        &self,
        account: &AccountPublic,
    ) -> Result<Vec<AreaFate>, StoreError> {
        owned_area_fates(&self.client().await?, account).await
    }

    async fn delete_account(&self, account: &AccountPublic) -> Result<Vec<AreaFate>, StoreError> {
        let mut c = self.client().await?;
        let tx = c.transaction().await.map_err(pg)?;
        let key = account.0.as_slice();
        if !exists(
            &tx,
            "SELECT 1 FROM accounts WHERE account = $1 FOR UPDATE",
            &key,
        )
        .await?
        {
            return Err(StoreError::NotFound);
        }
        let plan = owned_area_fates(&tx, account).await?;
        for fate in &plan {
            let id = fate.area_id.0;
            match &fate.outcome {
                AreaOutcome::TransferTo(heir) => {
                    let heir = heir.0.as_slice();
                    tx.execute(
                        "UPDATE areas SET owner = $2 WHERE area_id = $1",
                        &[&id, &heir],
                    )
                    .await
                    .map_err(pg)?;
                    tx.execute(
                        "DELETE FROM area_members WHERE area_id = $1 AND account = $2",
                        &[&id, &heir],
                    )
                    .await
                    .map_err(pg)?;
                }
                AreaOutcome::Delete => {
                    tx.execute("DELETE FROM areas WHERE area_id = $1", &[&id])
                        .await
                        .map_err(pg)?;
                }
            }
        }
        tx.execute("DELETE FROM area_members WHERE account = $1", &[&key])
            .await
            .map_err(pg)?;
        tx.execute(
            "DELETE FROM envelopes WHERE device_id IN (SELECT device_id FROM devices WHERE account = $1)",
            &[&key],
        )
        .await
        .map_err(pg)?;
        tx.execute(
            "INSERT INTO deleted_accounts (account) VALUES ($1) ON CONFLICT DO NOTHING",
            &[&key],
        )
        .await
        .map_err(pg)?;
        tx.execute("DELETE FROM accounts WHERE account = $1", &[&key])
            .await
            .map_err(pg)?;
        tx.commit().await.map_err(pg)?;
        Ok(plan)
    }
}

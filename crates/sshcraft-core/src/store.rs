//! SQLite store for groups, hosts, known host keys and sealed key records.

use std::path::Path;

use rusqlite::OptionalExtension;

use crate::model::{Group, GroupId, Host, HostId, KeyId};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("stored data is malformed: {0}")]
    Malformed(String),
}

/// A key as stored on disk. `sealed_secret` is the vault's ciphertext, never plaintext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRecord {
    pub id: KeyId,
    pub name: String,
    pub algorithm: String,
    pub fingerprint: String,
    pub public_key: String,
    pub sealed_secret: Vec<u8>,
}

pub struct Store {
    conn: rusqlite::Connection,
}

const SCHEMA_VERSION: i64 = 2;

pub(crate) const SCHEMA_V1: &str = "
CREATE TABLE groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    defaults TEXT NOT NULL
);
CREATE TABLE hosts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    group_id TEXT REFERENCES groups(id) ON DELETE CASCADE,
    settings TEXT NOT NULL
);
CREATE TABLE known_hosts (
    address TEXT NOT NULL,
    port INTEGER NOT NULL,
    fingerprint TEXT NOT NULL,
    PRIMARY KEY (address, port)
);
CREATE TABLE keys (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    algorithm TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    public_key TEXT NOT NULL,
    sealed_secret BLOB NOT NULL
);
";

/// Version 2: app settings and the hosts connected to most recently.
const SCHEMA_V2: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS recent_hosts (
    host_id TEXT PRIMARY KEY REFERENCES hosts(id) ON DELETE CASCADE,
    connected_at INTEGER NOT NULL
);
";

type KeyRow = (String, String, String, String, String, Vec<u8>);

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::init(rusqlite::Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(rusqlite::Connection::open_in_memory()?)
    }

    fn init(conn: rusqlite::Connection) -> Result<Self, StoreError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        // Each step runs in one transaction with its version bump, so a crash in between
        // leaves the database at the previous version instead of half upgraded.
        if version < 1 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(SCHEMA_V1)?;
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        }
        if version < 2 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(SCHEMA_V2)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    pub fn upsert_group(&self, group: &Group) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO groups (id, name, defaults) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, defaults = excluded.defaults",
            rusqlite::params![
                group.id.0.to_string(),
                group.name,
                to_json(&group.defaults)?
            ],
        )?;
        Ok(())
    }

    pub fn delete_group(&self, id: GroupId) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM groups WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }

    pub fn list_groups(&self) -> Result<Vec<Group>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, defaults FROM groups ORDER BY name COLLATE NOCASE, id")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, defaults) = row?;
            Ok(Group {
                id: GroupId(parse_uuid(&id)?),
                name,
                defaults: from_json(&defaults)?,
            })
        })
        .collect()
    }

    pub fn upsert_host(&self, host: &Host) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO hosts (id, name, group_id, settings) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, group_id = excluded.group_id,
             settings = excluded.settings",
            rusqlite::params![
                host.id.0.to_string(),
                host.name,
                host.group_id.map(|g| g.0.to_string()),
                to_json(&host.settings)?
            ],
        )?;
        Ok(())
    }

    pub fn delete_host(&self, id: HostId) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM hosts WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }

    pub fn list_hosts(&self) -> Result<Vec<Host>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, group_id, settings FROM hosts ORDER BY name COLLATE NOCASE, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, group_id, settings) = row?;
            Ok(Host {
                id: HostId(parse_uuid(&id)?),
                name,
                group_id: group_id.map(|g| parse_uuid(&g).map(GroupId)).transpose()?,
                settings: from_json(&settings)?,
            })
        })
        .collect()
    }

    pub fn known_host(&self, address: &str, port: u16) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT fingerprint FROM known_hosts WHERE address = ?1 AND port = ?2",
                rusqlite::params![address, port],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_known_host(
        &self,
        address: &str,
        port: u16,
        fingerprint: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO known_hosts (address, port, fingerprint) VALUES (?1, ?2, ?3)
             ON CONFLICT(address, port) DO UPDATE SET fingerprint = excluded.fingerprint",
            rusqlite::params![address, port, fingerprint],
        )?;
        Ok(())
    }

    pub fn forget_known_host(&self, address: &str, port: u16) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM known_hosts WHERE address = ?1 AND port = ?2",
            rusqlite::params![address, port],
        )?;
        Ok(())
    }

    pub fn put_key_record(&self, record: &KeyRecord) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO keys (id, name, algorithm, fingerprint, public_key, sealed_secret)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, algorithm = excluded.algorithm,
             fingerprint = excluded.fingerprint, public_key = excluded.public_key,
             sealed_secret = excluded.sealed_secret",
            rusqlite::params![
                record.id.0.to_string(),
                record.name,
                record.algorithm,
                record.fingerprint,
                record.public_key,
                record.sealed_secret
            ],
        )?;
        Ok(())
    }

    pub fn key_record(&self, id: KeyId) -> Result<Option<KeyRecord>, StoreError> {
        Ok(self
            .query_keys("WHERE id = ?1", &[&id.0.to_string()])?
            .into_iter()
            .next())
    }

    pub fn list_key_records(&self) -> Result<Vec<KeyRecord>, StoreError> {
        self.query_keys("ORDER BY name COLLATE NOCASE, id", &[])
    }

    pub fn delete_key_record(&self, id: KeyId) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM keys WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn put_setting(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// Notes a successful connection; `at_ms` is milliseconds since the Unix epoch.
    pub fn touch_recent(&self, host: HostId, at_ms: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO recent_hosts (host_id, connected_at) VALUES (?1, ?2)
             ON CONFLICT(host_id) DO UPDATE SET connected_at = excluded.connected_at",
            rusqlite::params![host.0.to_string(), at_ms],
        )?;
        Ok(())
    }

    /// Hosts connected to most recently, newest first.
    pub fn recent_hosts(&self, limit: u32) -> Result<Vec<HostId>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT host_id FROM recent_hosts ORDER BY connected_at DESC, host_id LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| r.get::<_, String>(0))?;
        rows.map(|row| Ok(HostId(parse_uuid(&row?)?))).collect()
    }

    pub fn has_key_records(&self) -> Result<bool, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT EXISTS(SELECT 1 FROM keys)", [], |r| r.get(0))?)
    }

    fn query_keys(
        &self,
        tail: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<KeyRecord>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, name, algorithm, fingerprint, public_key, sealed_secret FROM keys {tail}"
        ))?;
        let rows = stmt.query_map(params, |r| -> rusqlite::Result<KeyRow> {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, algorithm, fingerprint, public_key, sealed_secret) = row?;
            Ok(KeyRecord {
                id: KeyId(parse_uuid(&id)?),
                name,
                algorithm,
                fingerprint,
                public_key,
                sealed_secret,
            })
        })
        .collect()
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(|e| StoreError::Malformed(e.to_string()))
}

fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, StoreError> {
    serde_json::from_str(text).map_err(|e| StoreError::Malformed(e.to_string()))
}

fn parse_uuid(text: &str) -> Result<uuid::Uuid, StoreError> {
    uuid::Uuid::parse_str(text).map_err(|e| StoreError::Malformed(e.to_string()))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

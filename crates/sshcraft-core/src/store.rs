//! SQLite store for groups, hosts, known host keys and sealed key records.

use std::path::Path;

use rusqlite::OptionalExtension;

use crate::keytypes::KeyKind;
use crate::model::{Group, GroupId, Host, HostId, KeyId, ServerId};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("stored data is malformed: {0}")]
    Malformed(String),
    /// A group would end up inside itself.
    #[error("a group cannot be inside itself")]
    GroupCycle,
    /// Groups would nest deeper than `MAX_GROUP_DEPTH`.
    #[error("groups nest too deep")]
    TooDeep,
    /// A key bound to this device cannot move to a sync server.
    #[error("this key is bound to this device")]
    DeviceBound,
    #[error(transparent)]
    InvalidForward(#[from] crate::forward::ForwardError),
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
    /// The sync server this key belongs to; `None` means "only on this device".
    pub location: Option<ServerId>,
    pub kind: KeyKind,
    /// The sealed OpenSSH text is itself encrypted with a passphrase the user types.
    pub protected: bool,
    /// An OpenSSH user certificate for this key (public data).
    pub certificate: Option<String>,
}

pub struct Store {
    pub(crate) conn: rusqlite::Connection,
}

pub(crate) const SCHEMA_VERSION: i64 = 9;

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
pub(crate) const SCHEMA_V2: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS recent_hosts (
    host_id TEXT PRIMARY KEY REFERENCES hosts(id) ON DELETE CASCADE,
    connected_at INTEGER NOT NULL
);
";

/// Version 9: snippets, and the sealed command history per terminal target.
pub(crate) const SCHEMA_V9: &str = "
CREATE TABLE snippets (
    id TEXT PRIMARY KEY,
    sealed BLOB NOT NULL
);
CREATE TABLE command_history (
    scope TEXT PRIMARY KEY,
    sealed BLOB NOT NULL
);
";

/// Version 8: port forwarding rules of a host.
pub(crate) const SCHEMA_V8: &str = "
ALTER TABLE hosts ADD COLUMN forwards TEXT NOT NULL DEFAULT '[]';
";

/// Version 7: what kind of key, whether its passphrase is asked, and its certificate.
pub(crate) const SCHEMA_V7: &str = "
ALTER TABLE keys ADD COLUMN kind TEXT NOT NULL DEFAULT 'vault';
ALTER TABLE keys ADD COLUMN protected INTEGER NOT NULL DEFAULT 0;
ALTER TABLE keys ADD COLUMN certificate TEXT;
";

/// Version 5: nested groups, tags, and the operating system of a host.
pub(crate) const SCHEMA_V5: &str = "
ALTER TABLE groups ADD COLUMN parent_id TEXT REFERENCES groups(id) ON DELETE CASCADE;
ALTER TABLE groups ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';
ALTER TABLE hosts ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';
ALTER TABLE hosts ADD COLUMN os TEXT;
";

/// Version 3: where each item lives (a sync server or only this device) and the sync state.
pub(crate) const SCHEMA_V3: &str = "
ALTER TABLE groups ADD COLUMN location TEXT;
ALTER TABLE hosts ADD COLUMN location TEXT;
ALTER TABLE keys ADD COLUMN location TEXT;
CREATE TABLE sync_servers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    origin TEXT NOT NULL,
    account BLOB NOT NULL,
    device_id TEXT NOT NULL,
    sealed_device BLOB NOT NULL,
    sync_enabled INTEGER NOT NULL,
    sync_keys INTEGER NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0,
    log_head TEXT,
    tracker TEXT,
    created_at INTEGER NOT NULL
);
CREATE TABLE sync_areas (
    server_id TEXT NOT NULL REFERENCES sync_servers(id) ON DELETE CASCADE,
    area_id TEXT NOT NULL,
    manifest TEXT NOT NULL,
    envelope TEXT NOT NULL,
    cursor INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, area_id)
);
CREATE TABLE sync_records (
    server_id TEXT NOT NULL REFERENCES sync_servers(id) ON DELETE CASCADE,
    record_id TEXT NOT NULL,
    area_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    item_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    modified_at INTEGER NOT NULL,
    PRIMARY KEY (server_id, record_id)
);
CREATE UNIQUE INDEX sync_records_item ON sync_records(server_id, kind, item_id);
CREATE TABLE sync_outbox (
    server_id TEXT NOT NULL REFERENCES sync_servers(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    item_id TEXT NOT NULL,
    queued_at INTEGER NOT NULL,
    PRIMARY KEY (server_id, kind, item_id)
);
";

type KeyRow = (
    String,
    String,
    String,
    String,
    String,
    Vec<u8>,
    Option<String>,
    String,
    bool,
    Option<String>,
);

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
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
        }
        if version < 3 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(SCHEMA_V3)?;
            tx.pragma_update(None, "user_version", 3)?;
            tx.commit()?;
        }
        if version < 4 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(share_store::SCHEMA_V4)?;
            tx.pragma_update(None, "user_version", 4)?;
            tx.commit()?;
        }
        if version < 5 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(SCHEMA_V5)?;
            tx.pragma_update(None, "user_version", 5)?;
            tx.commit()?;
        }
        if version < 6 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(credential_store::SCHEMA_V6)?;
            tx.pragma_update(None, "user_version", 6)?;
            tx.commit()?;
        }
        if version < 7 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(SCHEMA_V7)?;
            tx.pragma_update(None, "user_version", 7)?;
            tx.commit()?;
        }
        if version < 8 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(SCHEMA_V8)?;
            tx.pragma_update(None, "user_version", 8)?;
            tx.commit()?;
        }
        if version < 9 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(SCHEMA_V9)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    /// Saves a group. A subgroup takes its parent's location, and so do its own subgroups and
    /// hosts; the parent must exist and must not be the group or one of its subgroups.
    pub fn upsert_group(&self, group: &Group) -> Result<(), StoreError> {
        let id = group.id.0.to_string();
        let tx = self.conn.unchecked_transaction()?;
        if let Some(parent) = group.parent_id {
            if parent == group.id || subgroups_of(&tx, &id)?.contains(&parent.0.to_string()) {
                return Err(StoreError::GroupCycle);
            }
            // Levels above the parent, the parent, this group and the deepest level below it.
            let above: i64 = tx.query_row(
                "WITH RECURSIVE up(id, depth) AS (
                     SELECT ?1, 1
                     UNION ALL
                     SELECT g.parent_id, up.depth + 1 FROM groups g JOIN up ON g.id = up.id
                     WHERE g.parent_id IS NOT NULL AND up.depth <= ?2
                 )
                 SELECT MAX(depth) FROM up",
                rusqlite::params![parent.0.to_string(), crate::model::MAX_GROUP_DEPTH as i64],
                |r| r.get(0),
            )?;
            let below: i64 = tx.query_row(
                "WITH RECURSIVE down(id, depth) AS (
                     SELECT ?1, 0
                     UNION ALL
                     SELECT g.id, down.depth + 1 FROM groups g JOIN down ON g.parent_id = down.id
                     WHERE down.depth <= ?2
                 )
                 SELECT MAX(depth) FROM down",
                rusqlite::params![id, crate::model::MAX_GROUP_DEPTH as i64],
                |r| r.get(0),
            )?;
            if above + 1 + below > crate::model::MAX_GROUP_DEPTH as i64 {
                return Err(StoreError::TooDeep);
            }
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM groups WHERE id = ?1)",
                [parent.0.to_string()],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(StoreError::Malformed("no such parent group".into()));
            }
        }
        tx.execute(
            // The location is set on insert only (or taken from the parent); moving an item goes
            // through `set_location`, so a form that sends an item back without its location
            // cannot move it.
            "INSERT INTO groups (id, name, defaults, location, parent_id, tags)
             VALUES (?1, ?2, ?3,
                 CASE WHEN ?5 IS NOT NULL THEN (SELECT location FROM groups WHERE id = ?5) ELSE ?4 END,
                 ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, defaults = excluded.defaults,
                 parent_id = excluded.parent_id, tags = excluded.tags,
                 location = CASE WHEN excluded.parent_id IS NOT NULL
                     THEN (SELECT location FROM groups WHERE id = excluded.parent_id)
                     ELSE groups.location END",
            rusqlite::params![
                id,
                group.name,
                to_json(&normalized(&group.defaults))?,
                group.location.map(|s| s.0.to_string()),
                group.parent_id.map(|p| p.0.to_string()),
                to_json(&crate::model::normalize_tags(&group.tags))?
            ],
        )?;
        // Whatever is below follows the group's (possibly new) location.
        let location: Option<String> =
            tx.query_row("SELECT location FROM groups WHERE id = ?1", [&id], |r| {
                r.get(0)
            })?;
        for sub in subgroups_of(&tx, &id)? {
            tx.execute(
                "UPDATE groups SET location = ?2 WHERE id = ?1",
                rusqlite::params![sub, location],
            )?;
            tx.execute(
                "UPDATE hosts SET location = ?2 WHERE group_id = ?1",
                rusqlite::params![sub, location],
            )?;
        }
        tx.execute(
            "UPDATE hosts SET location = ?2 WHERE group_id = ?1",
            rusqlite::params![id, location],
        )?;
        carry_passwords(&tx, ItemKind::Group, &id)?;
        tx.commit()?;
        Ok(())
    }

    /// Every group below `id`, at any depth.
    pub fn subgroups(&self, id: GroupId) -> Result<Vec<GroupId>, StoreError> {
        subgroups_of(&self.conn, &id.0.to_string())?
            .iter()
            .map(|s| parse_uuid(s).map(GroupId))
            .collect()
    }

    /// Deletes a group with its subgroups and all their hosts.
    pub fn delete_group(&self, id: GroupId) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM groups WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }

    pub fn list_groups(&self) -> Result<Vec<Group>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, defaults, location, parent_id, tags FROM groups
             ORDER BY name COLLATE NOCASE, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, defaults, location, parent, tags) = row?;
            Ok(Group {
                id: GroupId(parse_uuid(&id)?),
                name,
                defaults: from_json(&defaults)?,
                location: parse_location(location)?,
                parent_id: parent.map(|p| parse_uuid(&p).map(GroupId)).transpose()?,
                tags: from_json(&tags)?,
            })
        })
        .collect()
    }

    pub fn upsert_host(&self, host: &Host) -> Result<(), StoreError> {
        crate::forward::validate_all(&host.forwards)?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            // A host in a group always lives where its group lives.
            "INSERT INTO hosts (id, name, group_id, settings, location, tags, os, forwards)
             VALUES (?1, ?2, ?3, ?4,
                 CASE WHEN ?3 IS NOT NULL THEN (SELECT location FROM groups WHERE id = ?3) ELSE ?5 END,
                 ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, group_id = excluded.group_id,
             settings = excluded.settings, tags = excluded.tags, os = excluded.os,
             forwards = excluded.forwards,
             location = CASE WHEN excluded.group_id IS NOT NULL
                 THEN (SELECT location FROM groups WHERE id = excluded.group_id)
                 ELSE hosts.location END",
            rusqlite::params![
                host.id.0.to_string(),
                host.name,
                host.group_id.map(|g| g.0.to_string()),
                to_json(&normalized(&host.settings))?,
                host.location.map(|s| s.0.to_string()),
                to_json(&crate::model::normalize_tags(&host.tags))?,
                host.os.map(|os| to_json(&os)).transpose()?,
                to_json(&host.forwards)?
            ],
        )?;
        carry_passwords(&tx, ItemKind::Host, &host.id.0.to_string())?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_host(&self, id: HostId) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM hosts WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }

    pub fn list_hosts(&self) -> Result<Vec<Host>, StoreError> {
        /// Rules one by one: a damaged value or rule never hides the host.
        fn read_forwards(text: &str) -> Vec<crate::forward::ForwardRule> {
            serde_json::from_str::<Vec<serde_json::Value>>(text)
                .map(crate::forward::parse_rules)
                .unwrap_or_default()
        }
        let mut stmt = self.conn.prepare(
            "SELECT id, name, group_id, settings, location, tags, os, forwards FROM hosts
             ORDER BY name COLLATE NOCASE, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, group_id, settings, location, tags, os, forwards) = row?;
            Ok(Host {
                forwards: read_forwards(&forwards),
                id: HostId(parse_uuid(&id)?),
                name,
                group_id: group_id.map(|g| parse_uuid(&g).map(GroupId)).transpose()?,
                settings: from_json(&settings)?,
                location: parse_location(location)?,
                tags: from_json(&tags)?,
                os: os.map(|o| from_json(&o)).transpose()?,
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
            "INSERT INTO keys (id, name, algorithm, fingerprint, public_key, sealed_secret, location,
                 kind, protected, certificate)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, algorithm = excluded.algorithm,
             fingerprint = excluded.fingerprint, public_key = excluded.public_key,
             sealed_secret = excluded.sealed_secret, kind = excluded.kind,
             protected = excluded.protected, certificate = excluded.certificate",
            rusqlite::params![
                record.id.0.to_string(),
                record.name,
                record.algorithm,
                record.fingerprint,
                record.public_key,
                record.sealed_secret,
                record.location.map(|s| s.0.to_string()),
                record.kind.as_str(),
                record.protected,
                record.certificate,
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
            "SELECT id, name, algorithm, fingerprint, public_key, sealed_secret, location,
                 kind, protected, certificate
             FROM keys {tail}"
        ))?;
        let rows = stmt.query_map(params, |r| -> rusqlite::Result<KeyRow> {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
            ))
        })?;
        rows.map(|row| {
            let (
                id,
                name,
                algorithm,
                fingerprint,
                public_key,
                sealed_secret,
                location,
                kind,
                protected,
                certificate,
            ) = row?;
            Ok(KeyRecord {
                id: KeyId(parse_uuid(&id)?),
                name,
                algorithm,
                fingerprint,
                public_key,
                sealed_secret,
                location: parse_location(location)?,
                kind: KeyKind::parse(&kind)
                    .ok_or_else(|| StoreError::Malformed(format!("key kind {kind}")))?,
                protected,
                certificate,
            })
        })
        .collect()
    }
}

/// Every group below `id`, at any depth (at most `MAX_GROUP_DEPTH` levels).
fn subgroups_of(conn: &rusqlite::Connection, id: &str) -> Result<Vec<String>, StoreError> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE below(id, depth) AS (
             SELECT id, 1 FROM groups WHERE parent_id = ?1
             UNION
             SELECT g.id, below.depth + 1 FROM groups g JOIN below ON g.parent_id = below.id
             WHERE below.depth < ?2
         )
         SELECT DISTINCT id FROM below WHERE id != ?1",
    )?;
    let rows = stmt
        .query_map(
            rusqlite::params![id, crate::model::MAX_GROUP_DEPTH as i64],
            |r| r.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Settings as they are stored: environment variables normalized.
fn normalized(settings: &crate::model::ConnectionSettings) -> crate::model::ConnectionSettings {
    crate::model::ConnectionSettings {
        env: crate::model::normalize_env(&settings.env),
        ..settings.clone()
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(|e| StoreError::Malformed(e.to_string()))
}

fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, StoreError> {
    serde_json::from_str(text).map_err(|e| StoreError::Malformed(e.to_string()))
}

fn parse_location(text: Option<String>) -> Result<Option<ServerId>, StoreError> {
    text.map(|t| parse_uuid(&t).map(ServerId)).transpose()
}

pub(crate) fn parse_uuid(text: &str) -> Result<uuid::Uuid, StoreError> {
    uuid::Uuid::parse_str(text).map_err(|e| StoreError::Malformed(e.to_string()))
}

#[path = "credential_store.rs"]
mod credential_store;
#[path = "share_store.rs"]
mod share_store;
#[path = "sync_store.rs"]
mod sync_store;
pub use credential_store::PasswordRecord;
use credential_store::carry_passwords;
pub use share_store::{Contact, ShareKind, ShareRole, ShareRow, copy_id};
pub use sync_store::{AreaState, ItemKind, RecordEntry, ServerRow};

#[cfg(test)]
#[path = "store_tree_tests.rs"]
mod tree_tests;

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod credentials_tests;

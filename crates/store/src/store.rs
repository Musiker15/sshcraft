//! SQLite store for groups, hosts, known host keys and sealed key records.

use std::path::Path;

use rusqlite::OptionalExtension;

use sshcraft_model::keytypes::KeyKind;
use sshcraft_model::model::{Group, GroupId, Host, HostId, KeyId, ServerId};

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
    InvalidForward(#[from] sshcraft_model::forward::ForwardError),
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

/// The tables whose rows are sealed with the vault's master key.
pub const SEALED_TABLES: [&str; 7] = ["keys", "passwords", "command_history", "snippets", "session_logs", "session_log_chunks", "agent_log"];

pub struct Store {
    pub(crate) conn: rusqlite::Connection,
}

pub(crate) const SCHEMA_VERSION: i64 = 14;

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

/// Version 12: several host keys per address with the key itself (for `UpdateHostKeys` and
/// export), the proxy commands approved on this device, and the hosts kept in step with
/// `~/.ssh/config`. The last two are device-local and never sync.
pub(crate) const SCHEMA_V12: &str = "
CREATE TABLE host_keys (
    address TEXT NOT NULL,
    port INTEGER NOT NULL,
    fingerprint TEXT NOT NULL,
    algorithm TEXT,
    public_key TEXT,
    added_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (address, port, fingerprint)
);
INSERT INTO host_keys (address, port, fingerprint) SELECT address, port, fingerprint FROM known_hosts;
DROP TABLE known_hosts;
CREATE TABLE approved_commands (
    digest TEXT PRIMARY KEY
);
CREATE TABLE ssh_config_links (
    host_id TEXT PRIMARY KEY REFERENCES hosts(id) ON DELETE CASCADE,
    alias TEXT NOT NULL,
    file TEXT NOT NULL,
    digest TEXT NOT NULL
);
";

/// The digest a proxy command is approved by: SHA-256 over its arguments as JSON, so arguments
/// that differ only in how they were split never share one.
pub fn command_digest(argv: &[String]) -> String {
    use sha2::Digest;
    let json = serde_json::to_vec(argv).unwrap_or_default();
    sha2::Sha256::digest(&json).iter().map(|b| format!("{b:02x}")).collect()
}

/// A host key known for an address and port. Keys from schema 11 and from fingerprint-only
/// sources have no algorithm and no key; they still match by fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostKey {
    pub address: String,
    pub port: u16,
    pub algorithm: Option<String>,
    /// The key in OpenSSH base64, as in a `known_hosts` line.
    pub public_key: Option<String>,
    pub fingerprint: String,
    /// Unix seconds; 0 when not known.
    pub added_at: i64,
}

/// A host kept in step with an entry of `~/.ssh/config` on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigLink {
    pub host_id: HostId,
    pub alias: String,
    /// The file the entry was read from, as a display path.
    pub file: String,
    /// The digest of the entry when it was last applied, so an unchanged entry is not applied
    /// again over edits made in SSHCraft.
    pub digest: String,
}

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

type KeyRow = (String, String, String, String, String, Vec<u8>, Option<String>, String, bool, Option<String>);

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let conn = rusqlite::Connection::open(path)?;
        // Many small writes (recorded output, the sync outbox): WAL with NORMAL sync keeps each
        // commit from waiting for the disk; a crash loses no transaction that reached the log.
        let _: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    /// Runs `f` as one unit: all its writes or none. Nests (savepoints), unlike a transaction.
    pub fn atomic<T, E: From<StoreError>>(&self, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        self.conn.execute_batch("SAVEPOINT atomic").map_err(|e| E::from(e.into()))?;
        match f() {
            Ok(value) => {
                self.conn.execute_batch("RELEASE atomic").map_err(|e| E::from(e.into()))?;
                Ok(value)
            }
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK TO atomic; RELEASE atomic");
                Err(err)
            }
        }
    }

    /// Runs SQL on the database as it is, for tests that need a broken or odd state.
    #[cfg(feature = "testing")]
    pub fn execute_raw(&self, sql: &str) -> Result<(), StoreError> {
        self.conn.execute_batch(sql)?;
        Ok(())
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
            tx.pragma_update(None, "user_version", 9)?;
            tx.commit()?;
        }
        if version < 10 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(crate::session_log::SCHEMA_V10)?;
            tx.pragma_update(None, "user_version", 10)?;
            tx.commit()?;
        }
        if version < 11 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(crate::agent::SCHEMA_V11)?;
            tx.pragma_update(None, "user_version", 11)?;
            tx.commit()?;
        }
        if version < 12 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(SCHEMA_V12)?;
            tx.pragma_update(None, "user_version", 12)?;
            tx.commit()?;
        }
        if version < 13 {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(crate::trash::SCHEMA_V13)?;
            tx.pragma_update(None, "user_version", 13)?;
            tx.commit()?;
        }
        if version < 14 {
            let tx = conn.unchecked_transaction()?;
            // Only when missing: a database rebuilt from older steps may have it already.
            let has: bool =
                tx.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('sync_servers') WHERE name = 'sync_all')", [], |r| r.get(0))?;
            if !has {
                tx.execute_batch(sync_store::SCHEMA_V14)?;
            }
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
                rusqlite::params![parent.0.to_string(), sshcraft_model::model::MAX_GROUP_DEPTH as i64],
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
                rusqlite::params![id, sshcraft_model::model::MAX_GROUP_DEPTH as i64],
                |r| r.get(0),
            )?;
            if above + 1 + below > sshcraft_model::model::MAX_GROUP_DEPTH as i64 {
                return Err(StoreError::TooDeep);
            }
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM groups WHERE id = ?1)", [parent.0.to_string()], |r| r.get(0))?;
            if !exists {
                return Err(StoreError::Malformed("no such parent group".into()));
            }
        }
        tx.execute(
            // The location is set on insert only (or taken from the parent); moving an item goes
            // through `set_location`, so a form that sends an item back without its location
            // cannot move it. A new item without one goes to the server that syncs everything.
            "INSERT INTO groups (id, name, defaults, location, parent_id, tags)
             VALUES (?1, ?2, ?3,
                 CASE WHEN ?5 IS NOT NULL THEN (SELECT location FROM groups WHERE id = ?5)
                     ELSE COALESCE(?4, (SELECT id FROM sync_servers WHERE sync_all = 1 AND sync_enabled = 1 AND revoked = 0 LIMIT 1)) END,
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
                to_json(&sshcraft_model::model::normalize_tags(&group.tags))?
            ],
        )?;
        // Whatever is below follows the group's (possibly new) location.
        let location: Option<String> = tx.query_row("SELECT location FROM groups WHERE id = ?1", [&id], |r| r.get(0))?;
        for sub in subgroups_of(&tx, &id)? {
            tx.execute("UPDATE groups SET location = ?2 WHERE id = ?1", rusqlite::params![sub, location])?;
            tx.execute("UPDATE hosts SET location = ?2 WHERE group_id = ?1", rusqlite::params![sub, location])?;
        }
        tx.execute("UPDATE hosts SET location = ?2 WHERE group_id = ?1", rusqlite::params![id, location])?;
        carry_passwords(&tx, ItemKind::Group, &id)?;
        tx.commit()?;
        Ok(())
    }

    /// Every group below `id`, at any depth.
    pub fn subgroups(&self, id: GroupId) -> Result<Vec<GroupId>, StoreError> {
        subgroups_of(&self.conn, &id.0.to_string())?.iter().map(|s| parse_uuid(s).map(GroupId)).collect()
    }

    /// Deletes a group with its subgroups and all their hosts.
    pub fn delete_group(&self, id: GroupId) -> Result<(), StoreError> {
        self.conn.execute("DELETE FROM groups WHERE id = ?1", [id.0.to_string()])?;
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
        sshcraft_model::forward::validate_all(&host.forwards)?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            // A host in a group always lives where its group lives.
            "INSERT INTO hosts (id, name, group_id, settings, location, tags, os, forwards)
             VALUES (?1, ?2, ?3, ?4,
                 CASE WHEN ?3 IS NOT NULL THEN (SELECT location FROM groups WHERE id = ?3)
                     ELSE COALESCE(?5, (SELECT id FROM sync_servers WHERE sync_all = 1 AND sync_enabled = 1 AND revoked = 0 LIMIT 1)) END,
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
                to_json(&sshcraft_model::model::normalize_tags(&host.tags))?,
                host.os.map(|os| to_json(&os)).transpose()?,
                to_json(&host.forwards)?
            ],
        )?;
        carry_passwords(&tx, ItemKind::Host, &host.id.0.to_string())?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_host(&self, id: HostId) -> Result<(), StoreError> {
        self.conn.execute("DELETE FROM hosts WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }

    pub fn list_hosts(&self) -> Result<Vec<Host>, StoreError> {
        /// Rules one by one: a damaged value or rule never hides the host.
        fn read_forwards(text: &str) -> Vec<sshcraft_model::forward::ForwardRule> {
            serde_json::from_str::<Vec<serde_json::Value>>(text).map(sshcraft_model::forward::parse_rules).unwrap_or_default()
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

    /// The first fingerprint known for an address, for callers that need only one.
    pub fn known_host(&self, address: &str, port: u16) -> Result<Option<String>, StoreError> {
        Ok(self.host_keys(address, port)?.into_iter().next().map(|k| k.fingerprint))
    }

    /// Trusts a fingerprint alone, keeping any key already stored for it.
    pub fn set_known_host(&self, address: &str, port: u16, fingerprint: &str) -> Result<(), StoreError> {
        self.add_host_key(&HostKey {
            address: address.to_owned(),
            port,
            algorithm: None,
            public_key: None,
            fingerprint: fingerprint.to_owned(),
            added_at: 0,
        })
    }

    pub fn forget_known_host(&self, address: &str, port: u16) -> Result<(), StoreError> {
        self.forget_host_keys(address, port)
    }

    pub fn host_keys(&self, address: &str, port: u16) -> Result<Vec<HostKey>, StoreError> {
        self.read_host_keys("WHERE address = ?1 AND port = ?2", rusqlite::params![address, port])
    }

    pub fn all_host_keys(&self) -> Result<Vec<HostKey>, StoreError> {
        self.read_host_keys("", [])
    }

    fn read_host_keys(&self, filter: &str, params: impl rusqlite::Params) -> Result<Vec<HostKey>, StoreError> {
        let sql = format!(
            "SELECT address, port, fingerprint, algorithm, public_key, added_at FROM host_keys {filter}
             ORDER BY address, port, added_at, fingerprint"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params, |r| {
            Ok(HostKey {
                address: r.get(0)?,
                port: r.get(1)?,
                fingerprint: r.get(2)?,
                algorithm: r.get(3)?,
                public_key: r.get(4)?,
                added_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Adds a trusted key; for a fingerprint already known, fills in a missing algorithm, key or
    /// time and keeps what is there.
    pub fn add_host_key(&self, key: &HostKey) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO host_keys (address, port, fingerprint, algorithm, public_key, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(address, port, fingerprint) DO UPDATE SET
                 algorithm = coalesce(host_keys.algorithm, excluded.algorithm),
                 public_key = coalesce(host_keys.public_key, excluded.public_key),
                 added_at = CASE WHEN host_keys.added_at = 0 THEN excluded.added_at ELSE host_keys.added_at END",
            rusqlite::params![key.address, key.port, key.fingerprint, key.algorithm, key.public_key, key.added_at],
        )?;
        Ok(())
    }

    /// Whether a key was there to remove.
    pub fn remove_host_key(&self, address: &str, port: u16, fingerprint: &str) -> Result<bool, StoreError> {
        let n = self
            .conn
            .execute("DELETE FROM host_keys WHERE address = ?1 AND port = ?2 AND fingerprint = ?3", rusqlite::params![address, port, fingerprint])?;
        Ok(n > 0)
    }

    pub fn forget_host_keys(&self, address: &str, port: u16) -> Result<(), StoreError> {
        self.conn.execute("DELETE FROM host_keys WHERE address = ?1 AND port = ?2", rusqlite::params![address, port])?;
        Ok(())
    }

    /// Marks a proxy command line (by [`command_digest`], before its tokens are replaced) as
    /// allowed to run on this device.
    pub fn approve_command(&self, digest: &str) -> Result<(), StoreError> {
        self.conn.execute("INSERT OR IGNORE INTO approved_commands (digest) VALUES (?1)", [digest])?;
        Ok(())
    }

    pub fn command_approved(&self, digest: &str) -> Result<bool, StoreError> {
        Ok(self.conn.query_row("SELECT 1 FROM approved_commands WHERE digest = ?1", [digest], |_| Ok(())).optional()?.is_some())
    }

    pub fn config_links(&self) -> Result<Vec<ConfigLink>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT host_id, alias, file, digest FROM ssh_config_links ORDER BY file, alias")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.map(|row| {
            let (id, alias, file, digest) = row?;
            let id = uuid::Uuid::parse_str(&id).map_err(|e| StoreError::Malformed(e.to_string()))?;
            Ok(ConfigLink { host_id: HostId(id), alias, file, digest })
        })
        .collect()
    }

    pub fn set_config_link(&self, link: &ConfigLink) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO ssh_config_links (host_id, alias, file, digest) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(host_id) DO UPDATE SET alias = excluded.alias, file = excluded.file, digest = excluded.digest",
            rusqlite::params![link.host_id.0.to_string(), link.alias, link.file, link.digest],
        )?;
        Ok(())
    }

    pub fn remove_config_link(&self, host: HostId) -> Result<(), StoreError> {
        self.conn.execute("DELETE FROM ssh_config_links WHERE host_id = ?1", [host.0.to_string()])?;
        Ok(())
    }

    pub fn put_key_record(&self, record: &KeyRecord) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO keys (id, name, algorithm, fingerprint, public_key, sealed_secret, location,
                 kind, protected, certificate)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6,
                 COALESCE(?7, CASE WHEN ?8 = 'vault' THEN
                     (SELECT id FROM sync_servers WHERE sync_all = 1 AND sync_enabled = 1 AND sync_keys = 1
                      AND revoked = 0 LIMIT 1) END),
                 ?8, ?9, ?10)
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
        Ok(self.query_keys("WHERE id = ?1", &[&id.0.to_string()])?.into_iter().next())
    }

    pub fn list_key_records(&self) -> Result<Vec<KeyRecord>, StoreError> {
        self.query_keys("ORDER BY name COLLATE NOCASE, id", &[])
    }

    pub fn delete_key_record(&self, id: KeyId) -> Result<(), StoreError> {
        self.conn.execute("DELETE FROM keys WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
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
        Ok(self.recent_hosts_with_times(limit)?.into_iter().map(|(id, _)| id).collect())
    }

    /// [`Store::recent_hosts`] with when each was connected to last (milliseconds since 1970).
    pub fn recent_hosts_with_times(&self, limit: u32) -> Result<Vec<(HostId, i64)>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT host_id, connected_at FROM recent_hosts ORDER BY connected_at DESC, host_id LIMIT ?1")?;
        let rows = stmt.query_map([limit], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        rows.map(|row| {
            let (id, at) = row?;
            Ok((HostId(parse_uuid(&id)?), at))
        })
        .collect()
    }

    pub fn has_key_records(&self) -> Result<bool, StoreError> {
        Ok(self.conn.query_row("SELECT EXISTS(SELECT 1 FROM keys)", [], |r| r.get(0))?)
    }

    /// Whether anything sealed with the vault's master key is stored: keys, passwords, command
    /// history, snippets, session logs or the agent log. Without the master key all of it would
    /// be unreadable, so a new master key must never be made while any of it exists.
    pub fn has_sealed_records(&self) -> Result<bool, StoreError> {
        let mut any = false;
        for table in SEALED_TABLES {
            any |= self.conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table})"), [], |r| r.get::<_, bool>(0))?;
        }
        Ok(any)
    }

    fn query_keys(&self, tail: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<KeyRecord>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, name, algorithm, fingerprint, public_key, sealed_secret, location,
                 kind, protected, certificate
             FROM keys {tail}"
        ))?;
        let rows = stmt.query_map(params, |r| -> rusqlite::Result<KeyRow> {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?))
        })?;
        rows.map(|row| {
            let (id, name, algorithm, fingerprint, public_key, sealed_secret, location, kind, protected, certificate) = row?;
            Ok(KeyRecord {
                id: KeyId(parse_uuid(&id)?),
                name,
                algorithm,
                fingerprint,
                public_key,
                sealed_secret,
                location: parse_location(location)?,
                kind: KeyKind::parse(&kind).ok_or_else(|| StoreError::Malformed(format!("key kind {kind}")))?,
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
        .query_map(rusqlite::params![id, sshcraft_model::model::MAX_GROUP_DEPTH as i64], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Settings as they are stored: environment variables normalized.
fn normalized(settings: &sshcraft_model::model::ConnectionSettings) -> sshcraft_model::model::ConnectionSettings {
    sshcraft_model::model::ConnectionSettings { env: sshcraft_model::model::normalize_env(&settings.env), ..settings.clone() }
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

//! Sync state on this device: the servers it syncs with, their areas, which record holds which
//! item, and the outbox of local changes still to send. Also where items live (`location`).

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Store, StoreError, parse_uuid};
use crate::model::ServerId;

/// The kinds of items that sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Group,
    Host,
    Key,
    Identity,
    Password,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::Host => "host",
            Self::Key => "key",
            Self::Identity => "identity",
            Self::Password => "password",
        }
    }

    pub fn parse(text: &str) -> Result<Self, StoreError> {
        match text {
            "group" => Ok(Self::Group),
            "host" => Ok(Self::Host),
            "key" => Ok(Self::Key),
            "identity" => Ok(Self::Identity),
            "password" => Ok(Self::Password),
            other => Err(StoreError::Malformed(format!("unknown item kind {other}"))),
        }
    }

    fn table(self) -> &'static str {
        match self {
            Self::Group => "groups",
            Self::Host => "hosts",
            Self::Key => "keys",
            Self::Identity => "identities",
            Self::Password => "passwords",
        }
    }
}

/// One sync server this device has an account on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRow {
    pub id: ServerId,
    pub name: String,
    pub origin: String,
    pub account: [u8; 32],
    pub device_id: Uuid,
    /// This device's keys, sealed by the vault.
    pub sealed_device: Vec<u8>,
    pub sync_enabled: bool,
    pub sync_keys: bool,
    pub revoked: bool,
    /// Last accepted head of the account's device log (JSON).
    pub log_head: Option<String>,
    /// The `VersionTracker` (JSON).
    pub tracker: Option<String>,
    pub created_at: i64,
}

/// An area this device can read on a server: its last verified manifest, this device's
/// envelope and how far its records have been fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaState {
    pub area_id: Uuid,
    pub manifest: String,
    pub envelope: String,
    pub cursor: u64,
}

/// Which record holds an item, at which version and how recent its content is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordEntry {
    pub record_id: Uuid,
    pub area_id: Uuid,
    pub version: u64,
    /// Unix milliseconds of the change on the device that made it.
    pub modified_at: i64,
}

type RawServer = (
    String,
    String,
    String,
    Vec<u8>,
    String,
    Vec<u8>,
    bool,
    bool,
    bool,
    Option<String>,
    Option<String>,
    i64,
);

fn to_server(raw: RawServer) -> Result<ServerRow, StoreError> {
    let (id, name, origin, account, device_id, sealed_device, en, keys, rev, head, tracker, at) =
        raw;
    Ok(ServerRow {
        id: ServerId(parse_uuid(&id)?),
        name,
        origin,
        account: account
            .try_into()
            .map_err(|_| StoreError::Malformed("account key length".into()))?,
        device_id: parse_uuid(&device_id)?,
        sealed_device,
        sync_enabled: en,
        sync_keys: keys,
        revoked: rev,
        log_head: head,
        tracker,
        created_at: at,
    })
}

const SERVER_COLUMNS: &str = "id, name, origin, account, device_id, sealed_device, sync_enabled, \
     sync_keys, revoked, log_head, tracker, created_at";

fn server_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawServer> {
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
        r.get(10)?,
        r.get(11)?,
    ))
}

impl Store {
    pub fn add_server(&self, row: &ServerRow) -> Result<(), StoreError> {
        self.conn.execute(
            &format!("INSERT INTO sync_servers ({SERVER_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"),
            params![
                row.id.0.to_string(),
                row.name,
                row.origin,
                row.account.to_vec(),
                row.device_id.to_string(),
                row.sealed_device,
                row.sync_enabled,
                row.sync_keys,
                row.revoked,
                row.log_head,
                row.tracker,
                row.created_at
            ],
        )?;
        Ok(())
    }

    pub fn servers(&self) -> Result<Vec<ServerRow>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SERVER_COLUMNS} FROM sync_servers ORDER BY created_at, id"
        ))?;
        let rows = stmt
            .query_map([], server_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(to_server).collect()
    }

    pub fn server(&self, id: ServerId) -> Result<Option<ServerRow>, StoreError> {
        self.conn
            .query_row(
                &format!("SELECT {SERVER_COLUMNS} FROM sync_servers WHERE id = ?1"),
                [id.0.to_string()],
                server_row,
            )
            .optional()?
            .map(to_server)
            .transpose()
    }

    /// Turning key sync on fetches every area from the start again: key records skipped while
    /// it was off were never counted, so they arrive now.
    pub fn update_server_flags(
        &self,
        id: ServerId,
        sync_enabled: bool,
        sync_keys: bool,
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let was: Option<bool> = tx
            .query_row(
                "SELECT sync_keys FROM sync_servers WHERE id = ?1",
                [id.0.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute(
            "UPDATE sync_servers SET sync_enabled = ?2, sync_keys = ?3 WHERE id = ?1",
            params![id.0.to_string(), sync_enabled, sync_keys],
        )?;
        if sync_keys && was == Some(false) {
            tx.execute(
                "UPDATE sync_areas SET cursor = 0 WHERE server_id = ?1",
                [id.0.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn mark_server_revoked(&self, id: ServerId) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sync_servers SET revoked = 1 WHERE id = ?1",
            [id.0.to_string()],
        )?;
        Ok(())
    }

    pub fn save_server_state(
        &self,
        id: ServerId,
        log_head: &str,
        tracker: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sync_servers SET log_head = ?2, tracker = ?3 WHERE id = ?1",
            params![id.0.to_string(), log_head, tracker],
        )?;
        Ok(())
    }

    /// Forgets a server on this device. Its groups, hosts and keys stay, as local items.
    pub fn remove_server(&self, id: ServerId) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let key = id.0.to_string();
        for table in ["groups", "hosts", "keys"] {
            tx.execute(
                &format!("UPDATE {table} SET location = NULL WHERE location = ?1"),
                [&key],
            )?;
        }
        tx.execute("DELETE FROM sync_servers WHERE id = ?1", [&key])?;
        tx.commit()?;
        Ok(())
    }

    /// Moves an item to a server or back to this device. A group takes its hosts along; a
    /// host inside a group cannot move on its own.
    pub fn set_location(
        &self,
        kind: ItemKind,
        id: &str,
        location: Option<ServerId>,
    ) -> Result<(), StoreError> {
        let target = location.map(|s| s.0.to_string());
        let tx = self.conn.unchecked_transaction()?;
        if kind == ItemKind::Key && location.is_some() {
            let key_kind: Option<String> = tx
                .query_row("SELECT kind FROM keys WHERE id = ?1", [id], |r| r.get(0))
                .optional()?;
            if key_kind.as_deref() == Some(crate::keytypes::KeyKind::Device.as_str()) {
                return Err(StoreError::DeviceBound);
            }
        }
        if kind == ItemKind::Host {
            let group: Option<Option<String>> = tx
                .query_row("SELECT group_id FROM hosts WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            if matches!(group, Some(Some(_))) {
                return Err(StoreError::Malformed("host follows its group".into()));
            }
        }
        if kind == ItemKind::Group {
            let parent: Option<Option<String>> = tx
                .query_row("SELECT parent_id FROM groups WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            if matches!(parent, Some(Some(_))) {
                return Err(StoreError::Malformed("group follows its parent".into()));
            }
        }
        let changed = tx.execute(
            &format!("UPDATE {} SET location = ?2 WHERE id = ?1", kind.table()),
            params![id, target],
        )?;
        if changed == 0 {
            return Err(StoreError::Malformed("no such item".into()));
        }
        if kind == ItemKind::Group {
            let mut groups = super::subgroups_of(&tx, id)?;
            groups.push(id.to_owned());
            for group in groups {
                tx.execute(
                    "UPDATE groups SET location = ?2 WHERE id = ?1",
                    params![group, target],
                )?;
                tx.execute(
                    "UPDATE hosts SET location = ?2 WHERE group_id = ?1",
                    params![group, target],
                )?;
            }
        }
        super::carry_passwords(&tx, kind, id)?;
        tx.commit()?;
        Ok(())
    }

    /// Stores an area's verified manifest and this device's envelope; keeps the cursor.
    pub fn put_area(
        &self,
        server: ServerId,
        area_id: Uuid,
        manifest: &str,
        envelope: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO sync_areas (server_id, area_id, manifest, envelope) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(server_id, area_id) DO UPDATE SET manifest = excluded.manifest,
                 envelope = excluded.envelope",
            params![server.0.to_string(), area_id.to_string(), manifest, envelope],
        )?;
        Ok(())
    }

    pub fn areas(&self, server: ServerId) -> Result<Vec<AreaState>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT area_id, manifest, envelope, cursor FROM sync_areas WHERE server_id = ?1
             ORDER BY area_id",
        )?;
        let rows = stmt
            .query_map([server.0.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(area, manifest, envelope, cursor)| {
                Ok(AreaState {
                    area_id: parse_uuid(&area)?,
                    manifest,
                    envelope,
                    cursor: cursor as u64,
                })
            })
            .collect()
    }

    pub fn set_cursor(
        &self,
        server: ServerId,
        area_id: Uuid,
        cursor: u64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sync_areas SET cursor = ?3 WHERE server_id = ?1 AND area_id = ?2",
            params![server.0.to_string(), area_id.to_string(), cursor as i64],
        )?;
        Ok(())
    }

    pub fn record_entry(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
    ) -> Result<Option<RecordEntry>, StoreError> {
        let raw: Option<(String, String, i64, i64)> = self
            .conn
            .query_row(
                "SELECT record_id, area_id, version, modified_at FROM sync_records
                 WHERE server_id = ?1 AND kind = ?2 AND item_id = ?3",
                params![server.0.to_string(), kind.as_str(), item_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        raw.map(|(record, area, version, modified_at)| {
            Ok(RecordEntry {
                record_id: parse_uuid(&record)?,
                area_id: parse_uuid(&area)?,
                version: version as u64,
                modified_at,
            })
        })
        .transpose()
    }

    /// The item a record holds on this server, if this device has seen it before.
    pub fn item_for_record(
        &self,
        server: ServerId,
        record_id: Uuid,
    ) -> Result<Option<(ItemKind, String)>, StoreError> {
        let raw: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT kind, item_id FROM sync_records WHERE server_id = ?1 AND record_id = ?2",
                params![server.0.to_string(), record_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        raw.map(|(kind, item)| Ok((ItemKind::parse(&kind)?, item)))
            .transpose()
    }

    pub fn put_record_entry(
        &self,
        server: ServerId,
        entry: &RecordEntry,
        kind: ItemKind,
        item_id: &str,
    ) -> Result<(), StoreError> {
        // An item that moved to another area has a new record: the old one no longer holds it.
        self.conn.execute(
            "DELETE FROM sync_records WHERE server_id = ?1 AND kind = ?2 AND item_id = ?3
             AND record_id != ?4",
            params![
                server.0.to_string(),
                kind.as_str(),
                item_id,
                entry.record_id.to_string()
            ],
        )?;
        self.conn.execute(
            "INSERT INTO sync_records (server_id, record_id, area_id, kind, item_id, version, modified_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(server_id, record_id) DO UPDATE SET area_id = excluded.area_id,
                 kind = excluded.kind, item_id = excluded.item_id, version = excluded.version,
                 modified_at = excluded.modified_at",
            params![
                server.0.to_string(),
                entry.record_id.to_string(),
                entry.area_id.to_string(),
                kind.as_str(),
                item_id,
                entry.version as i64,
                entry.modified_at
            ],
        )?;
        Ok(())
    }

    pub fn remove_record_entry(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM sync_records WHERE server_id = ?1 AND kind = ?2 AND item_id = ?3",
            params![server.0.to_string(), kind.as_str(), item_id],
        )?;
        Ok(())
    }

    /// Notes that an item changed locally and still has to go to `server`; the time is the
    /// latest change, which "last change wins" compares against the server's state.
    pub fn queue(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
        at: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO sync_outbox (server_id, kind, item_id, queued_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(server_id, kind, item_id) DO UPDATE SET queued_at = excluded.queued_at",
            params![server.0.to_string(), kind.as_str(), item_id, at],
        )?;
        Ok(())
    }

    /// Items waiting to be sent, oldest first.
    pub fn outbox(&self, server: ServerId) -> Result<Vec<(ItemKind, String)>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, item_id FROM sync_outbox WHERE server_id = ?1
             ORDER BY queued_at, kind, item_id",
        )?;
        let rows = stmt
            .query_map([server.0.to_string()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(kind, item)| Ok((ItemKind::parse(&kind)?, item)))
            .collect()
    }

    /// Takes an item out of the outbox only if it was not changed again since `queued_at`, so a
    /// change made while the previous one was being sent is not lost.
    pub fn dequeue_if(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
        queued_at: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM sync_outbox WHERE server_id = ?1 AND kind = ?2 AND item_id = ?3
             AND queued_at = ?4",
            params![server.0.to_string(), kind.as_str(), item_id, queued_at],
        )?;
        Ok(())
    }

    pub fn dequeue(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM sync_outbox WHERE server_id = ?1 AND kind = ?2 AND item_id = ?3",
            params![server.0.to_string(), kind.as_str(), item_id],
        )?;
        Ok(())
    }

    /// When a still unsent local change to an item was queued (Unix milliseconds).
    pub fn queued_at(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
    ) -> Result<Option<i64>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT queued_at FROM sync_outbox WHERE server_id = ?1 AND kind = ?2 AND item_id = ?3",
                params![server.0.to_string(), kind.as_str(), item_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// `None` if the item does not exist; otherwise where it lives (`Some(None)`: this device only).
    pub fn item_location(
        &self,
        kind: ItemKind,
        id: &str,
    ) -> Result<Option<Option<ServerId>>, StoreError> {
        let raw: Option<Option<String>> = self
            .conn
            .query_row(
                &format!("SELECT location FROM {} WHERE id = ?1", kind.table()),
                [id],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|loc| loc.map(|t| parse_uuid(&t).map(ServerId)).transpose())
            .transpose()
    }

    /// Where an item lives, or `None` for a local item (or one that does not exist).
    pub fn location_of(&self, kind: ItemKind, id: &str) -> Result<Option<ServerId>, StoreError> {
        let raw: Option<Option<String>> = self
            .conn
            .query_row(
                &format!("SELECT location FROM {} WHERE id = ?1", kind.table()),
                [id],
                |r| r.get(0),
            )
            .optional()?;
        raw.flatten()
            .map(|t| parse_uuid(&t).map(ServerId))
            .transpose()
    }
}

#[cfg(test)]
#[path = "sync_store_tests.rs"]
mod tests;

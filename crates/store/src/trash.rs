//! The trash and earlier versions. A deleted host, group (with its subgroups and their hosts),
//! key or snippet is kept here with the rows it had, sealed parts (passwords, the key's secret,
//! the snippet) staying sealed as they were. Restoring puts the rows back under their ids. Every
//! save of a host, group or snippet keeps the stand before, at most [`MAX_VERSIONS`] of them.

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sshcraft_model::model::{Group, GroupId, Host, HostId, KeyId, PasswordId, ServerId, SnippetId};

use crate::store::{KeyRecord, Store, StoreError};

/// Earlier versions kept per item.
pub const MAX_VERSIONS: usize = 20;

/// Version 13: the trash and earlier versions.
pub(crate) const SCHEMA_V13: &str = "
CREATE TABLE trash (
    kind TEXT NOT NULL,
    id TEXT NOT NULL,
    name TEXT NOT NULL,
    payload TEXT NOT NULL,
    deleted_at INTEGER NOT NULL,
    PRIMARY KEY (kind, id)
);
CREATE TABLE versions (
    kind TEXT NOT NULL,
    id TEXT NOT NULL,
    saved_at INTEGER NOT NULL,
    name TEXT NOT NULL,
    payload TEXT NOT NULL
);
CREATE INDEX versions_item ON versions (kind, id, saved_at);
";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrashKind {
    Host,
    Group,
    Key,
    Snippet,
}

impl TrashKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Group => "group",
            Self::Key => "key",
            Self::Snippet => "snippet",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Host, Self::Group, Self::Key, Self::Snippet].into_iter().find(|k| k.as_str() == text)
    }
}

/// A password row as it was: still sealed with the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedPassword {
    pub id: PasswordId,
    /// The sealed bytes, Base64.
    pub sealed: String,
    pub location: Option<ServerId>,
}

/// A key row as it was; the secret stays sealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyRow {
    pub id: KeyId,
    pub name: String,
    pub algorithm: String,
    pub fingerprint: String,
    pub public_key: String,
    pub sealed_secret: String,
    pub location: Option<ServerId>,
    pub kind: sshcraft_model::keytypes::KeyKind,
    pub protected: bool,
    pub certificate: Option<String>,
}

/// What a trash entry or a version holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrashPayload {
    Host {
        host: Box<Host>,
        passwords: Vec<SealedPassword>,
    },
    /// The group first, then its subgroups (parents before children), their hosts.
    Group {
        groups: Vec<Group>,
        hosts: Vec<Host>,
        passwords: Vec<SealedPassword>,
    },
    Key {
        record: KeyRow,
    },
    /// The sealed snippet; its seal names the id, so it comes back under the same one.
    Snippet {
        id: SnippetId,
        name: String,
        sealed: String,
    },
}

/// One entry of the trash, without its rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrashEntry {
    pub kind: TrashKind,
    pub id: String,
    pub name: String,
    /// Unix seconds.
    pub deleted_at: i64,
}

/// What a restore put back, for sync.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restored {
    pub groups: Vec<GroupId>,
    pub hosts: Vec<HostId>,
    pub keys: Vec<KeyId>,
    pub passwords: Vec<PasswordId>,
}

fn b64(bytes: &[u8]) -> String {
    data_encoding::BASE64.encode(bytes)
}

fn unb64(text: &str) -> Result<Vec<u8>, StoreError> {
    data_encoding::BASE64.decode(text.as_bytes()).map_err(|e| StoreError::Malformed(e.to_string()))
}

/// Unix seconds now; the trash only compares them.
fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl TrashPayload {
    /// Everything in it stays on this device only (it comes back where its server or share
    /// would not take it).
    pub fn make_local(&mut self) {
        match self {
            Self::Host { host, passwords } => {
                host.location = None;
                passwords.iter_mut().for_each(|p| p.location = None);
            }
            Self::Group { groups, hosts, passwords } => {
                groups.iter_mut().for_each(|g| g.location = None);
                hosts.iter_mut().for_each(|h| h.location = None);
                passwords.iter_mut().for_each(|p| p.location = None);
            }
            Self::Key { record } => record.location = None,
            Self::Snippet { .. } => {}
        }
    }
}

impl Store {
    /// Keeps a host in the trash before it is deleted; nothing when it is not there.
    pub fn trash_host(&self, id: HostId) -> Result<(), StoreError> {
        if let Some(payload @ TrashPayload::Host { .. }) = self.snapshot_host(id)? {
            let name = match &payload {
                TrashPayload::Host { host, .. } => host.name.clone(),
                _ => String::new(),
            };
            self.put_trash(TrashKind::Host, &id.0.to_string(), &name, &payload, now_secs())?;
        }
        Ok(())
    }

    /// Keeps a group with everything below it in the trash before it is deleted.
    pub fn trash_group(&self, id: GroupId) -> Result<(), StoreError> {
        if let Some(payload) = self.snapshot_group(id)? {
            let name = match &payload {
                TrashPayload::Group { groups, .. } => groups.first().map(|g| g.name.clone()).unwrap_or_default(),
                _ => String::new(),
            };
            self.put_trash(TrashKind::Group, &id.0.to_string(), &name, &payload, now_secs())?;
        }
        Ok(())
    }

    /// Keys bound to this device are not kept: their Windows Hello credential goes when they
    /// are deleted, so a restored record could not sign anyway.
    pub fn trash_key(&self, id: KeyId) -> Result<(), StoreError> {
        if self.key_record(id)?.is_some_and(|r| r.kind == sshcraft_model::keytypes::KeyKind::Device) {
            return Ok(());
        }
        if let Some(payload) = self.snapshot_key(id)? {
            let name = match &payload {
                TrashPayload::Key { record } => record.name.clone(),
                _ => String::new(),
            };
            self.put_trash(TrashKind::Key, &id.0.to_string(), &name, &payload, now_secs())?;
        }
        Ok(())
    }

    pub fn trash_snippet(&self, id: SnippetId, name: &str) -> Result<(), StoreError> {
        if let Some(payload) = self.snapshot_snippet(id, name)? {
            self.put_trash(TrashKind::Snippet, &id.0.to_string(), name, &payload, now_secs())?;
        }
        Ok(())
    }

    /// Removes what was deleted more than `days` ago.
    pub fn purge_old_trash(&self, days: u32) -> Result<usize, StoreError> {
        self.purge_trash(Some(now_secs().saturating_sub(i64::from(days).saturating_mul(86_400))))
    }

    fn sealed_passwords(&self, ids: impl IntoIterator<Item = PasswordId>) -> Result<Vec<SealedPassword>, StoreError> {
        let mut out = Vec::new();
        for id in ids {
            if out.iter().any(|p: &SealedPassword| p.id == id) {
                continue;
            }
            if let Some((sealed, location)) = self.password_record(id)? {
                out.push(SealedPassword { id, sealed: b64(&sealed), location });
            }
        }
        Ok(out)
    }

    pub fn snapshot_host(&self, id: HostId) -> Result<Option<TrashPayload>, StoreError> {
        let Some(host) = self.list_hosts()?.into_iter().find(|h| h.id == id) else {
            return Ok(None);
        };
        let passwords = self.sealed_passwords(host.settings.password_ids())?;
        Ok(Some(TrashPayload::Host { host: Box::new(host), passwords }))
    }

    /// The group, every group below it and all their hosts.
    pub fn snapshot_group(&self, id: GroupId) -> Result<Option<TrashPayload>, StoreError> {
        let all = self.list_groups()?;
        let Some(top) = all.iter().find(|g| g.id == id).cloned() else {
            return Ok(None);
        };
        let mut groups = vec![top];
        let mut i = 0;
        while let Some(parent) = groups.get(i).map(|g| g.id) {
            groups.extend(all.iter().filter(|g| g.parent_id == Some(parent) && !groups.iter().any(|k| k.id == g.id)).cloned().collect::<Vec<_>>());
            i += 1;
        }
        let hosts: Vec<Host> = self.list_hosts()?.into_iter().filter(|h| h.group_id.is_some_and(|g| groups.iter().any(|k| k.id == g))).collect();
        let ids =
            groups.iter().flat_map(|g| g.defaults.password_ids()).chain(hosts.iter().flat_map(|h| h.settings.password_ids())).collect::<Vec<_>>();
        let passwords = self.sealed_passwords(ids)?;
        Ok(Some(TrashPayload::Group { groups, hosts, passwords }))
    }

    pub fn snapshot_key(&self, id: KeyId) -> Result<Option<TrashPayload>, StoreError> {
        Ok(self.key_record(id)?.map(|r| TrashPayload::Key {
            record: KeyRow {
                id: r.id,
                name: r.name,
                algorithm: r.algorithm,
                fingerprint: r.fingerprint,
                public_key: r.public_key,
                sealed_secret: b64(&r.sealed_secret),
                location: r.location,
                kind: r.kind,
                protected: r.protected,
                certificate: r.certificate,
            },
        }))
    }

    /// The sealed snippet; `name` is shown in the trash (the store cannot read the snippet).
    pub fn snapshot_snippet(&self, id: SnippetId, name: &str) -> Result<Option<TrashPayload>, StoreError> {
        let sealed: Option<Vec<u8>> =
            self.conn.query_row("SELECT sealed FROM snippets WHERE id = ?1", [id.0.to_string()], |r| r.get(0)).optional()?;
        Ok(sealed.map(|s| TrashPayload::Snippet { id, name: name.to_owned(), sealed: b64(&s) }))
    }

    pub fn put_trash(&self, kind: TrashKind, id: &str, name: &str, payload: &TrashPayload, now: i64) -> Result<(), StoreError> {
        let json = serde_json::to_string(payload).map_err(|e| StoreError::Malformed(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO trash (kind, id, name, payload, deleted_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(kind, id) DO UPDATE SET name = excluded.name, payload = excluded.payload, deleted_at = excluded.deleted_at",
            rusqlite::params![kind.as_str(), id, name, json, now],
        )?;
        Ok(())
    }

    /// Newest first.
    pub fn list_trash(&self) -> Result<Vec<TrashEntry>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT kind, id, name, deleted_at FROM trash ORDER BY deleted_at DESC, name")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, id, name, deleted_at) = row?;
            if let Some(kind) = TrashKind::parse(&kind) {
                out.push(TrashEntry { kind, id, name, deleted_at });
            }
        }
        Ok(out)
    }

    pub fn trash_payload(&self, kind: TrashKind, id: &str) -> Result<Option<TrashPayload>, StoreError> {
        let json: Option<String> = self
            .conn
            .query_row("SELECT payload FROM trash WHERE kind = ?1 AND id = ?2", rusqlite::params![kind.as_str(), id], |r| r.get(0))
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(|e| StoreError::Malformed(e.to_string()))).transpose()
    }

    /// Takes one entry out after it was restored; its versions stay with the item.
    pub fn remove_trash(&self, kind: TrashKind, id: &str) -> Result<bool, StoreError> {
        Ok(self.conn.execute("DELETE FROM trash WHERE kind = ?1 AND id = ?2", rusqlite::params![kind.as_str(), id])? > 0)
    }

    /// Deletes one entry for good, with the earlier versions of what it held.
    pub fn purge_one(&self, kind: TrashKind, id: &str) -> Result<bool, StoreError> {
        let gone = self.forget_trashed_versions(&format!("WHERE kind = '{}' AND id = ?1", kind.as_str()), [id])?;
        Ok(self.remove_trash(kind, id)? || gone)
    }

    /// Deletes the entries deleted before `before` (Unix seconds), or all with `None`, with the
    /// earlier versions of what they held.
    pub fn purge_trash(&self, before: Option<i64>) -> Result<usize, StoreError> {
        Ok(match before {
            Some(t) => {
                self.forget_trashed_versions("WHERE deleted_at < ?1", [t])?;
                self.conn.execute("DELETE FROM trash WHERE deleted_at < ?1", [t])?
            }
            None => {
                self.forget_trashed_versions("", [])?;
                self.conn.execute("DELETE FROM trash", [])?
            }
        })
    }

    /// Forgets the versions of the items held by the trash entries `filter` picks (a group's
    /// subgroups and hosts too); whether there were entries.
    fn forget_trashed_versions(&self, filter: &str, params: impl rusqlite::Params) -> Result<bool, StoreError> {
        let mut stmt = self.conn.prepare(&format!("SELECT payload FROM trash {filter}"))?;
        let payloads: Vec<String> = stmt.query_map(params, |r| r.get(0))?.collect::<Result<_, _>>()?;
        for json in &payloads {
            let Ok(payload) = serde_json::from_str::<TrashPayload>(json) else { continue };
            match payload {
                TrashPayload::Host { host, .. } => self.forget_versions(TrashKind::Host, &host.id.0.to_string())?,
                TrashPayload::Group { groups, hosts, .. } => {
                    for g in groups {
                        self.forget_versions(TrashKind::Group, &g.id.0.to_string())?;
                    }
                    for h in hosts {
                        self.forget_versions(TrashKind::Host, &h.id.0.to_string())?;
                    }
                }
                TrashPayload::Snippet { id, .. } => self.forget_versions(TrashKind::Snippet, &id.0.to_string())?,
                TrashPayload::Key { .. } => {}
            }
        }
        Ok(!payloads.is_empty())
    }

    /// Puts the rows of an entry back under their ids. Refused when one of the ids is in use
    /// again (sync brought it back, say); that is checked before anything is written.
    pub fn restore_payload(&self, payload: &TrashPayload) -> Result<Restored, StoreError> {
        let mut restored = Restored::default();
        let hosts_now = self.list_hosts()?;
        let groups_now = self.list_groups()?;
        let taken = |what: &str| StoreError::Malformed(format!("{what} exists again; nothing was restored"));
        let put_passwords = |passwords: &[SealedPassword], restored: &mut Restored| -> Result<(), StoreError> {
            for p in passwords {
                if self.password_record(p.id)?.is_none() {
                    self.put_password_record(p.id, &unb64(&p.sealed)?, p.location)?;
                    restored.passwords.push(p.id);
                }
            }
            Ok(())
        };
        match payload {
            TrashPayload::Host { host, passwords } => {
                if hosts_now.iter().any(|h| h.id == host.id) {
                    return Err(taken("the host"));
                }
                let mut host = (**host).clone();
                let mut passwords = passwords.clone();
                if host.group_id.is_some_and(|g| !groups_now.iter().any(|k| k.id == g)) {
                    // At the top, on this device: with its passwords.
                    host.group_id = None;
                    host.location = None;
                    passwords.iter_mut().for_each(|p| p.location = None);
                }
                put_passwords(&passwords, &mut restored)?;
                self.upsert_host(&host)?;
                restored.hosts.push(host.id);
            }
            TrashPayload::Group { groups, hosts, passwords } => {
                if groups.iter().any(|g| groups_now.iter().any(|k| k.id == g.id)) || hosts.iter().any(|h| hosts_now.iter().any(|k| k.id == h.id)) {
                    return Err(taken("a group or host"));
                }
                // The top group's parent may be gone: then the whole tree comes back at the
                // top, on this device.
                let orphan = groups.first().is_some_and(|g| g.parent_id.is_some_and(|p| !groups_now.iter().any(|k| k.id == p)));
                let mut tree = TrashPayload::Group { groups: groups.clone(), hosts: hosts.clone(), passwords: passwords.clone() };
                if orphan {
                    tree.make_local();
                }
                let TrashPayload::Group { groups, hosts, passwords } = tree else {
                    return Err(StoreError::Malformed("not a group".into()));
                };
                put_passwords(&passwords, &mut restored)?;
                for (i, group) in groups.iter().enumerate() {
                    let mut group = group.clone();
                    if i == 0 && orphan {
                        group.parent_id = None;
                    }
                    self.upsert_group(&group)?;
                    restored.groups.push(group.id);
                }
                for host in &hosts {
                    self.upsert_host(host)?;
                    restored.hosts.push(host.id);
                }
            }
            TrashPayload::Key { record } => {
                if self.key_record(record.id)?.is_some() {
                    return Err(taken("the key"));
                }
                self.put_key_record(&KeyRecord {
                    id: record.id,
                    name: record.name.clone(),
                    algorithm: record.algorithm.clone(),
                    fingerprint: record.fingerprint.clone(),
                    public_key: record.public_key.clone(),
                    sealed_secret: unb64(&record.sealed_secret)?,
                    location: record.location,
                    kind: record.kind,
                    protected: record.protected,
                    certificate: record.certificate.clone(),
                })?;
                restored.keys.push(record.id);
            }
            TrashPayload::Snippet { id, sealed, .. } => {
                self.conn
                    .execute("INSERT INTO snippets (id, sealed) VALUES (?1, ?2)", rusqlite::params![id.0.to_string(), unb64(sealed)?])
                    .map_err(|_| taken("the snippet"))?;
            }
        }
        Ok(restored)
    }

    /// Keeps the stand of an item before a save; the oldest beyond [`MAX_VERSIONS`] go.
    pub fn put_version(&self, kind: TrashKind, id: &str, name: &str, payload: &TrashPayload, now: i64) -> Result<(), StoreError> {
        let json = serde_json::to_string(payload).map_err(|e| StoreError::Malformed(e.to_string()))?;
        let last: Option<String> = self
            .conn
            .query_row(
                "SELECT payload FROM versions WHERE kind = ?1 AND id = ?2 ORDER BY saved_at DESC, rowid DESC LIMIT 1",
                rusqlite::params![kind.as_str(), id],
                |r| r.get(0),
            )
            .optional()?;
        if last.as_deref() == Some(json.as_str()) {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO versions (kind, id, saved_at, name, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![kind.as_str(), id, now, name, json],
        )?;
        self.conn.execute(
            "DELETE FROM versions WHERE kind = ?1 AND id = ?2 AND rowid NOT IN
                 (SELECT rowid FROM versions WHERE kind = ?1 AND id = ?2 ORDER BY saved_at DESC, rowid DESC LIMIT ?3)",
            rusqlite::params![kind.as_str(), id, i64::try_from(MAX_VERSIONS).unwrap_or(20)],
        )?;
        Ok(())
    }

    /// `(version id, saved_at, name)`, newest first.
    pub fn list_versions(&self, kind: TrashKind, id: &str) -> Result<Vec<(i64, i64, String)>, StoreError> {
        let mut stmt =
            self.conn.prepare("SELECT rowid, saved_at, name FROM versions WHERE kind = ?1 AND id = ?2 ORDER BY saved_at DESC, rowid DESC")?;
        let rows = stmt.query_map(rusqlite::params![kind.as_str(), id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn version(&self, kind: TrashKind, id: &str, version: i64) -> Result<Option<TrashPayload>, StoreError> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT payload FROM versions WHERE kind = ?1 AND id = ?2 AND rowid = ?3",
                rusqlite::params![kind.as_str(), id, version],
                |r| r.get(0),
            )
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(|e| StoreError::Malformed(e.to_string()))).transpose()
    }

    /// Puts an earlier sealed stand of a snippet back.
    pub fn put_snippet_sealed(&self, id: SnippetId, sealed: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO snippets (id, sealed) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET sealed = excluded.sealed",
            rusqlite::params![id.0.to_string(), unb64(sealed)?],
        )?;
        Ok(())
    }

    /// Drops the versions of an item (it is deleted for good).
    pub fn forget_versions(&self, kind: TrashKind, id: &str) -> Result<(), StoreError> {
        self.conn.execute("DELETE FROM versions WHERE kind = ?1 AND id = ?2", rusqlite::params![kind.as_str(), id])?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "trash_tests.rs"]
mod tests;

//! Shares on this device: which area an item lives in, the accounts this device has looked up
//! (contacts), the shares it owns or received, and which copy belongs to which original.

use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use super::{ItemKind, Store, StoreError, parse_uuid};
use crate::model::ServerId;

/// Version 4: areas of items, contacts, shares and copies.
pub(crate) const SCHEMA_V4: &str = "
CREATE TABLE sync_item_area (
    server_id TEXT NOT NULL REFERENCES sync_servers(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    item_id TEXT NOT NULL,
    area_id TEXT NOT NULL,
    PRIMARY KEY (server_id, kind, item_id)
);
CREATE TABLE sync_contacts (
    server_id TEXT NOT NULL REFERENCES sync_servers(id) ON DELETE CASCADE,
    email_norm TEXT NOT NULL,
    account BLOB NOT NULL,
    name TEXT NOT NULL,
    first_seen INTEGER NOT NULL,
    confirmed INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, email_norm)
);
CREATE TABLE sync_shares (
    server_id TEXT NOT NULL REFERENCES sync_servers(id) ON DELETE CASCADE,
    area_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    group_id TEXT,
    with_keys INTEGER NOT NULL,
    owned INTEGER NOT NULL,
    role TEXT,
    owner BLOB NOT NULL,
    PRIMARY KEY (server_id, area_id)
);
CREATE TABLE sync_share_copies (
    server_id TEXT NOT NULL REFERENCES sync_servers(id) ON DELETE CASCADE,
    area_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    original_id TEXT NOT NULL,
    copy_id TEXT NOT NULL,
    PRIMARY KEY (server_id, area_id, kind, original_id)
);
CREATE UNIQUE INDEX sync_share_copies_copy ON sync_share_copies(server_id, kind, copy_id);
";

/// How much of a group a share holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareKind {
    /// The group itself with every host, later ones included.
    GroupAll,
    /// Copies of some hosts of a group (and of the group).
    Selection,
    /// A copy of one host.
    Host,
}

impl ShareKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GroupAll => "group_all",
            Self::Selection => "selection",
            Self::Host => "host",
        }
    }

    pub fn parse(text: &str) -> Result<Self, StoreError> {
        match text {
            "group_all" => Ok(Self::GroupAll),
            "selection" => Ok(Self::Selection),
            "host" => Ok(Self::Host),
            other => Err(StoreError::Malformed(format!("unknown share kind {other}"))),
        }
    }
}

/// This account's role in a share it received.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareRole {
    Read,
    Write,
}

impl ShareRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }

    fn parse(text: &str) -> Result<Self, StoreError> {
        match text {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            other => Err(StoreError::Malformed(format!("unknown role {other}"))),
        }
    }
}

/// A share this device knows: one this account owns, or one it received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareRow {
    pub area_id: Uuid,
    pub kind: ShareKind,
    /// The shared group (the original's id on the owner's devices).
    pub group_id: Option<String>,
    pub with_keys: bool,
    pub owned: bool,
    /// This account's role in a received share; `None` for an own share.
    pub role: Option<ShareRole>,
    pub owner: [u8; 32],
}

/// An account looked up by e-mail, and whether the user compared its safety number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    /// Normalized (trimmed, lower case).
    pub email: String,
    pub account: [u8; 32],
    pub name: String,
    pub first_seen: i64,
    pub confirmed: bool,
}

/// The id of the copy of `original` in the share `area`: the same on every device of the owner,
/// so each of them can tell which copy belongs to which original.
pub fn copy_id(area: Uuid, original: Uuid) -> Uuid {
    use sha2::{Digest, Sha256};
    let digest = Sha256::new()
        .chain_update(b"sshcraft-copy")
        .chain_update(area.as_bytes())
        .chain_update(original.as_bytes())
        .finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_random_bytes(bytes).into_uuid()
}

type RawShare = (
    String,
    String,
    Option<String>,
    bool,
    bool,
    Option<String>,
    Vec<u8>,
);

fn to_share(raw: RawShare) -> Result<ShareRow, StoreError> {
    let (area, kind, group_id, with_keys, owned, role, owner) = raw;
    Ok(ShareRow {
        area_id: parse_uuid(&area)?,
        kind: ShareKind::parse(&kind)?,
        group_id,
        with_keys,
        owned,
        role: role.as_deref().map(ShareRole::parse).transpose()?,
        owner: owner
            .try_into()
            .map_err(|_| StoreError::Malformed("owner key length".into()))?,
    })
}

const SHARE_COLUMNS: &str = "area_id, kind, group_id, with_keys, owned, role, owner";

fn share_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawShare> {
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

type RawContact = (String, Vec<u8>, String, i64, bool);

fn to_contact(raw: RawContact) -> Result<Contact, StoreError> {
    let (email, account, name, first_seen, confirmed) = raw;
    Ok(Contact {
        email,
        account: account
            .try_into()
            .map_err(|_| StoreError::Malformed("account key length".into()))?,
        name,
        first_seen,
        confirmed,
    })
}

fn contact_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawContact> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
}

impl Store {
    /// Puts an item into an area of `server` (a share); without a row it is in the personal area.
    pub fn set_item_area(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
        area: Uuid,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO sync_item_area (server_id, kind, item_id, area_id) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(server_id, kind, item_id) DO UPDATE SET area_id = excluded.area_id",
            params![
                server.0.to_string(),
                kind.as_str(),
                item_id,
                area.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn clear_item_area(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM sync_item_area WHERE server_id = ?1 AND kind = ?2 AND item_id = ?3",
            params![server.0.to_string(), kind.as_str(), item_id],
        )?;
        Ok(())
    }

    /// The area set for this item itself, if any.
    pub fn item_area(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
    ) -> Result<Option<Uuid>, StoreError> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT area_id FROM sync_item_area WHERE server_id = ?1 AND kind = ?2 AND item_id = ?3",
                params![server.0.to_string(), kind.as_str(), item_id],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|t| parse_uuid(&t)).transpose()
    }

    /// The area an item is in, if it is not the personal one: its own, or a host's group's.
    pub fn effective_area(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
    ) -> Result<Option<Uuid>, StoreError> {
        if let Some(area) = self.item_area(server, kind, item_id)? {
            return Ok(Some(area));
        }
        let above = |sql: &str, id: &str| -> Result<Option<String>, StoreError> {
            Ok(self
                .conn
                .query_row(sql, [id], |r| r.get::<_, Option<String>>(0))
                .optional()?
                .flatten())
        };
        const PARENT: &str = "SELECT parent_id FROM groups WHERE id = ?1";
        // A host follows its group, a group its parent, at any depth.
        let mut next = match kind {
            ItemKind::Host => above("SELECT group_id FROM hosts WHERE id = ?1", item_id)?,
            ItemKind::Group => above(PARENT, item_id)?,
            ItemKind::Key => None,
        };
        for _ in 0..crate::model::MAX_GROUP_DEPTH {
            let Some(group) = next else {
                return Ok(None);
            };
            if let Some(area) = self.item_area(server, ItemKind::Group, &group)? {
                return Ok(Some(area));
            }
            next = above(PARENT, &group)?;
        }
        Ok(None)
    }

    /// The area an item goes to on `server`: [`Store::effective_area`], else `personal`.
    pub fn area_for(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
        personal: Uuid,
    ) -> Result<Uuid, StoreError> {
        Ok(self
            .effective_area(server, kind, item_id)?
            .unwrap_or(personal))
    }

    /// Every item with its own area row in `area`.
    pub fn items_in_area(
        &self,
        server: ServerId,
        area: Uuid,
    ) -> Result<Vec<(ItemKind, String)>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, item_id FROM sync_item_area WHERE server_id = ?1 AND area_id = ?2
             ORDER BY kind, item_id",
        )?;
        let rows = stmt
            .query_map(params![server.0.to_string(), area.to_string()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(kind, item)| Ok((ItemKind::parse(&kind)?, item)))
            .collect()
    }

    pub fn contact(&self, server: ServerId, email: &str) -> Result<Option<Contact>, StoreError> {
        self.conn
            .query_row(
                "SELECT email_norm, account, name, first_seen, confirmed FROM sync_contacts
                 WHERE server_id = ?1 AND email_norm = ?2",
                params![server.0.to_string(), email],
                contact_row,
            )
            .optional()?
            .map(to_contact)
            .transpose()
    }

    pub fn contact_by_account(
        &self,
        server: ServerId,
        account: &[u8; 32],
    ) -> Result<Option<Contact>, StoreError> {
        self.conn
            .query_row(
                "SELECT email_norm, account, name, first_seen, confirmed FROM sync_contacts
                 WHERE server_id = ?1 AND account = ?2",
                params![server.0.to_string(), account.to_vec()],
                contact_row,
            )
            .optional()?
            .map(to_contact)
            .transpose()
    }

    /// Remembers a contact. A different account for a known e-mail is stored unconfirmed; the
    /// first time this e-mail was seen stays.
    pub fn put_contact(&self, server: ServerId, contact: &Contact) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO sync_contacts (server_id, email_norm, account, name, first_seen, confirmed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(server_id, email_norm) DO UPDATE SET name = excluded.name,
                 confirmed = CASE WHEN sync_contacts.account = excluded.account
                     THEN sync_contacts.confirmed ELSE 0 END,
                 account = excluded.account",
            params![
                server.0.to_string(),
                contact.email,
                contact.account.to_vec(),
                contact.name,
                contact.first_seen,
                contact.confirmed
            ],
        )?;
        Ok(())
    }

    /// The user compared the safety number of `account` for this e-mail.
    pub fn confirm_contact(
        &self,
        server: ServerId,
        email: &str,
        account: &[u8; 32],
    ) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE sync_contacts SET account = ?3, confirmed = 1
             WHERE server_id = ?1 AND email_norm = ?2",
            params![server.0.to_string(), email, account.to_vec()],
        )?;
        if changed == 0 {
            return Err(StoreError::Malformed("no such contact".into()));
        }
        Ok(())
    }

    pub fn put_share(&self, server: ServerId, share: &ShareRow) -> Result<(), StoreError> {
        self.conn.execute(
            &format!(
                "INSERT INTO sync_shares (server_id, {SHARE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(server_id, area_id) DO UPDATE SET kind = excluded.kind,
                     group_id = excluded.group_id, with_keys = excluded.with_keys,
                     owned = excluded.owned, role = excluded.role, owner = excluded.owner"
            ),
            params![
                server.0.to_string(),
                share.area_id.to_string(),
                share.kind.as_str(),
                share.group_id,
                share.with_keys,
                share.owned,
                share.role.map(ShareRole::as_str),
                share.owner.to_vec()
            ],
        )?;
        Ok(())
    }

    pub fn shares(&self, server: ServerId) -> Result<Vec<ShareRow>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SHARE_COLUMNS} FROM sync_shares WHERE server_id = ?1 ORDER BY area_id"
        ))?;
        let rows = stmt
            .query_map([server.0.to_string()], share_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(to_share).collect()
    }

    pub fn share(&self, server: ServerId, area: Uuid) -> Result<Option<ShareRow>, StoreError> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {SHARE_COLUMNS} FROM sync_shares WHERE server_id = ?1 AND area_id = ?2"
                ),
                params![server.0.to_string(), area.to_string()],
                share_row,
            )
            .optional()?
            .map(to_share)
            .transpose()
    }

    /// Forgets a share with its copies and the area rows of its items.
    pub fn remove_share(&self, server: ServerId, area: Uuid) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let (s, a) = (server.0.to_string(), area.to_string());
        for table in ["sync_shares", "sync_share_copies", "sync_item_area"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE server_id = ?1 AND area_id = ?2"),
                params![s, a],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Forgets an area on this device: the share, its copies, item areas, record entries and
    /// the stored manifest and key.
    pub fn forget_area(&self, server: ServerId, area: Uuid) -> Result<(), StoreError> {
        self.remove_share(server, area)?;
        let (s, a) = (server.0.to_string(), area.to_string());
        self.conn.execute(
            "DELETE FROM sync_records WHERE server_id = ?1 AND area_id = ?2",
            params![s, a],
        )?;
        self.conn.execute(
            "DELETE FROM sync_areas WHERE server_id = ?1 AND area_id = ?2",
            params![s, a],
        )?;
        Ok(())
    }

    pub fn put_copy(
        &self,
        server: ServerId,
        area: Uuid,
        kind: ItemKind,
        original_id: &str,
        copy_id: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO sync_share_copies (server_id, area_id, kind, original_id, copy_id)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(server_id, area_id, kind, original_id) DO UPDATE SET copy_id = excluded.copy_id",
            params![
                server.0.to_string(),
                area.to_string(),
                kind.as_str(),
                original_id,
                copy_id
            ],
        )?;
        Ok(())
    }

    pub fn remove_copy(
        &self,
        server: ServerId,
        kind: ItemKind,
        copy_id: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM sync_share_copies WHERE server_id = ?1 AND kind = ?2 AND copy_id = ?3",
            params![server.0.to_string(), kind.as_str(), copy_id],
        )?;
        Ok(())
    }

    /// The copies of an original: share area and copy id.
    pub fn copies_of(
        &self,
        server: ServerId,
        kind: ItemKind,
        original_id: &str,
    ) -> Result<Vec<(Uuid, String)>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT area_id, copy_id FROM sync_share_copies
             WHERE server_id = ?1 AND kind = ?2 AND original_id = ?3 ORDER BY area_id",
        )?;
        let rows = stmt
            .query_map(
                params![server.0.to_string(), kind.as_str(), original_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(area, copy)| Ok((parse_uuid(&area)?, copy)))
            .collect()
    }

    pub fn original_of(
        &self,
        server: ServerId,
        kind: ItemKind,
        copy_id: &str,
    ) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT original_id FROM sync_share_copies
                 WHERE server_id = ?1 AND kind = ?2 AND copy_id = ?3",
                params![server.0.to_string(), kind.as_str(), copy_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The share area and original of a copy.
    pub fn copy_entry(
        &self,
        server: ServerId,
        kind: ItemKind,
        copy_id: &str,
    ) -> Result<Option<(Uuid, String)>, StoreError> {
        let raw: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT area_id, original_id FROM sync_share_copies
                 WHERE server_id = ?1 AND kind = ?2 AND copy_id = ?3",
                params![server.0.to_string(), kind.as_str(), copy_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        raw.map(|(area, original)| Ok((parse_uuid(&area)?, original)))
            .transpose()
    }

    /// The copies in one share: kind, original and copy id.
    pub fn copies_in(
        &self,
        server: ServerId,
        area: Uuid,
    ) -> Result<Vec<(ItemKind, String, String)>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, original_id, copy_id FROM sync_share_copies
             WHERE server_id = ?1 AND area_id = ?2 ORDER BY kind, original_id",
        )?;
        let rows = stmt
            .query_map(params![server.0.to_string(), area.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(kind, original, copy)| Ok((ItemKind::parse(&kind)?, original, copy)))
            .collect()
    }

    /// The received share an item is in, if any.
    pub fn received_share_of(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
    ) -> Result<Option<ShareRow>, StoreError> {
        let Some(area) = self.effective_area(server, kind, item_id)? else {
            return Ok(None);
        };
        Ok(self.share(server, area)?.filter(|share| !share.owned))
    }

    /// Whether this account may only read the item (it came in a share with role `read`).
    pub fn read_only(
        &self,
        server: ServerId,
        kind: ItemKind,
        item_id: &str,
    ) -> Result<bool, StoreError> {
        Ok(self
            .received_share_of(server, kind, item_id)?
            .is_some_and(|share| share.role == Some(ShareRole::Read)))
    }
}

#[cfg(test)]
#[path = "share_store_tests.rs"]
mod tests;

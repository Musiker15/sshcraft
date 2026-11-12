//! Identities and sealed passwords. A password lives where its owner lives (the host, group or
//! identity that refers to it), so moving the owner takes the password along.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{ItemKind, Store, StoreError, from_json, parse_location, parse_uuid, to_json};
use crate::model::{ConnectionSettings, Identity, IdentityId, KeyRef, PasswordId, ServerId};

/// Version 6: identities and passwords sealed by the vault.
pub(crate) const SCHEMA_V6: &str = "
CREATE TABLE identities (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    body TEXT NOT NULL,
    location TEXT
);
CREATE TABLE passwords (
    id TEXT PRIMARY KEY,
    sealed BLOB NOT NULL,
    location TEXT
);
";

/// What an identity holds besides its id, name and location.
#[derive(Serialize, Deserialize)]
struct IdentityBody {
    username: Option<String>,
    key: Option<KeyRef>,
    password: Option<PasswordId>,
}

impl Store {
    /// Saves an identity. The location is set on insert only; moving goes through
    /// `set_location`.
    pub fn upsert_identity(&self, identity: &Identity) -> Result<(), StoreError> {
        let id = identity.id.0.to_string();
        let body = IdentityBody {
            username: identity.username.clone(),
            key: identity.key.clone(),
            password: identity.password,
        };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO identities (id, name, body, location) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, body = excluded.body",
            params![
                id,
                identity.name.trim(),
                to_json(&body)?,
                identity.location.map(|s| s.0.to_string())
            ],
        )?;
        carry_passwords(&tx, ItemKind::Identity, &id)?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_identities(&self) -> Result<Vec<Identity>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, body, location FROM identities ORDER BY name COLLATE NOCASE, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, body, location) = row?;
            let body: IdentityBody = from_json(&body)?;
            Ok(Identity {
                id: IdentityId(parse_uuid(&id)?),
                name,
                username: body.username,
                key: body.key,
                password: body.password,
                location: parse_location(location)?,
            })
        })
        .collect()
    }

    pub fn delete_identity(&self, id: IdentityId) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM identities WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }

    /// Stores a sealed password; `sealed` is the vault's ciphertext, never plaintext.
    pub fn put_password_record(
        &self,
        id: PasswordId,
        sealed: &[u8],
        location: Option<ServerId>,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO passwords (id, sealed, location) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET sealed = excluded.sealed, location = excluded.location",
            params![id.0.to_string(), sealed, location.map(|s| s.0.to_string())],
        )?;
        Ok(())
    }

    /// The sealed password and where it lives.
    pub fn password_record(
        &self,
        id: PasswordId,
    ) -> Result<Option<(Vec<u8>, Option<ServerId>)>, StoreError> {
        let row: Option<(Vec<u8>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT sealed, location FROM passwords WHERE id = ?1",
                [id.0.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(sealed, location)| Ok((sealed, parse_location(location)?)))
            .transpose()
    }

    pub fn delete_password(&self, id: PasswordId) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM passwords WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }

    pub fn list_password_ids(&self) -> Result<Vec<PasswordId>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT id FROM passwords ORDER BY id")?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0))?;
        ids.map(|id| Ok(PasswordId(parse_uuid(&id?)?))).collect()
    }
}

fn place(
    conn: &rusqlite::Connection,
    ids: &[PasswordId],
    location: &Option<String>,
) -> Result<(), StoreError> {
    for id in ids {
        conn.execute(
            "UPDATE passwords SET location = ?2 WHERE id = ?1",
            params![id.0.to_string(), location],
        )?;
    }
    Ok(())
}

fn settings_passwords(json: &str) -> Result<Vec<PasswordId>, StoreError> {
    Ok(from_json::<ConnectionSettings>(json)?.password_ids())
}

/// Puts the passwords an item refers to where the item lives; for a group also those of its
/// subgroups and their hosts.
pub(crate) fn carry_passwords(
    conn: &rusqlite::Connection,
    kind: ItemKind,
    id: &str,
) -> Result<(), StoreError> {
    match kind {
        ItemKind::Host => {
            let row: Option<(String, Option<String>)> = conn
                .query_row(
                    "SELECT settings, location FROM hosts WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((settings, location)) = row {
                place(conn, &settings_passwords(&settings)?, &location)?;
            }
        }
        ItemKind::Identity => {
            let row: Option<(String, Option<String>)> = conn
                .query_row(
                    "SELECT body, location FROM identities WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((body, location)) = row {
                let body: IdentityBody = from_json(&body)?;
                place(
                    conn,
                    &body.password.into_iter().collect::<Vec<_>>(),
                    &location,
                )?;
            }
        }
        ItemKind::Group => {
            let mut groups = super::subgroups_of(conn, id)?;
            groups.push(id.to_owned());
            for group in groups {
                let row: Option<(String, Option<String>)> = conn
                    .query_row(
                        "SELECT defaults, location FROM groups WHERE id = ?1",
                        [&group],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some((defaults, location)) = row {
                    place(conn, &settings_passwords(&defaults)?, &location)?;
                }
                let mut stmt =
                    conn.prepare("SELECT settings, location FROM hosts WHERE group_id = ?1")?;
                let hosts = stmt
                    .query_map([&group], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                for (settings, location) in hosts {
                    place(conn, &settings_passwords(&settings)?, &location)?;
                }
            }
        }
        ItemKind::Key | ItemKind::Password => {}
    }
    Ok(())
}

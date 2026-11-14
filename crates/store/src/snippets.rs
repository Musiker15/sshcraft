//! Snippets: commands or short scripts, sorted into packages, with hosts they usually run on.
//! Kept on this device only, sealed with the vault (a command can hold a password or token).

use serde::{Deserialize, Serialize};

use crate::store::{Store, StoreError};
use crate::vault::{Vault, VaultError};
use sshcraft_model::model::{HostId, SnippetId};

pub const MAX_SNIPPETS: usize = 1000;
pub const MAX_NAME: usize = 100;
pub const MAX_PACKAGE: usize = 100;
pub const MAX_CONTENT: usize = 10_000;
pub const MAX_TARGETS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub id: SnippetId,
    pub name: String,
    /// The package (folder) it is in; empty for none.
    #[serde(default)]
    pub package: String,
    pub content: String,
    /// Hosts the snippet runs on unless others are picked.
    #[serde(default)]
    pub default_targets: Vec<HostId>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SnippetError {
    #[error("invalid snippet: {0}")]
    Invalid(&'static str),
}

#[derive(Debug, thiserror::Error)]
pub enum SnippetStoreError {
    #[error(transparent)]
    Invalid(#[from] SnippetError),
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<rusqlite::Error> for SnippetStoreError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Store(e.into())
    }
}

fn text_ok(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(char::is_control)
}

pub fn validate(snippet: &Snippet) -> Result<(), SnippetError> {
    if snippet.name.trim().is_empty() || !text_ok(&snippet.name, MAX_NAME) {
        return Err(SnippetError::Invalid("name"));
    }
    if !text_ok(&snippet.package, MAX_PACKAGE) {
        return Err(SnippetError::Invalid("package"));
    }
    // Lines and tabs are what a script is made of; no other control characters.
    let content_ok = !snippet.content.trim().is_empty()
        && snippet.content.chars().count() <= MAX_CONTENT
        && !snippet
            .content
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'));
    if !content_ok {
        return Err(SnippetError::Invalid("content"));
    }
    if snippet.default_targets.len() > MAX_TARGETS {
        return Err(SnippetError::Invalid("targets"));
    }
    Ok(())
}

/// Sealed under the snippet's id: a block moved to another id does not open.
fn label(id: SnippetId) -> String {
    format!("snippet:{}", id.0)
}

/// All snippets by name; one that cannot be opened or read is left out.
pub fn list(vault: &Vault, store: &Store) -> Result<Vec<Snippet>, SnippetStoreError> {
    let mut stmt = store.conn.prepare("SELECT id, sealed FROM snippets")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut snippets: Vec<Snippet> = rows
        .into_iter()
        .filter_map(|(id, sealed)| {
            let id = SnippetId(uuid::Uuid::parse_str(&id).ok()?);
            let plain = vault.open_secret(&label(id), &sealed).ok()?;
            let snippet: Snippet = serde_json::from_slice(&plain).ok()?;
            (snippet.id == id && validate(&snippet).is_ok()).then_some(snippet)
        })
        .collect();
    snippets.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.0.cmp(&b.id.0))
    });
    Ok(snippets)
}

/// Stores a valid snippet; a new one only while there are fewer than [`MAX_SNIPPETS`].
pub fn save(vault: &Vault, store: &Store, snippet: &Snippet) -> Result<(), SnippetStoreError> {
    validate(snippet)?;
    let id = snippet.id.0.to_string();
    let exists: bool = store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM snippets WHERE id = ?1)",
        [&id],
        |r| r.get(0),
    )?;
    let count: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM snippets", [], |r| r.get(0))?;
    if !exists && usize::try_from(count).unwrap_or(usize::MAX) >= MAX_SNIPPETS {
        return Err(SnippetError::Invalid("too many").into());
    }
    let clean = Snippet {
        name: snippet.name.trim().to_owned(),
        package: snippet.package.trim().to_owned(),
        ..snippet.clone()
    };
    let plain = zeroize::Zeroizing::new(
        serde_json::to_vec(&clean).map_err(|e| StoreError::Malformed(e.to_string()))?,
    );
    let sealed = vault.seal_secret(&label(snippet.id), &plain)?;
    store.conn.execute(
        "INSERT INTO snippets (id, sealed) VALUES (?1, ?2)
         ON CONFLICT(id) DO UPDATE SET sealed = excluded.sealed",
        rusqlite::params![id, sealed],
    )?;
    Ok(())
}

pub fn delete(store: &Store, id: SnippetId) -> Result<(), SnippetStoreError> {
    store
        .conn
        .execute("DELETE FROM snippets WHERE id = ?1", [id.0.to_string()])?;
    Ok(())
}

impl Store {
    pub(crate) fn history_block(&self, scope: &str) -> Result<Option<Vec<u8>>, StoreError> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row(
                "SELECT sealed FROM command_history WHERE scope = ?1",
                [scope],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub(crate) fn put_history_block(&self, scope: &str, sealed: &[u8]) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO command_history (scope, sealed) VALUES (?1, ?2)
             ON CONFLICT(scope) DO UPDATE SET sealed = excluded.sealed",
            rusqlite::params![scope, sealed],
        )?;
        Ok(())
    }

    pub(crate) fn delete_history_blocks(&self, scope: Option<&str>) -> Result<(), StoreError> {
        match scope {
            Some(scope) => self
                .conn
                .execute("DELETE FROM command_history WHERE scope = ?1", [scope])?,
            None => self.conn.execute("DELETE FROM command_history", [])?,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::MemoryMasterKey;

    fn setup() -> (Store, Vault) {
        let store = Store::open_in_memory().unwrap();
        let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
        (store, vault)
    }

    fn snippet(name: &str) -> Snippet {
        Snippet {
            id: SnippetId::new(),
            name: name.into(),
            package: "Wartung".into(),
            content: "sudo apt update\n\tsudo apt upgrade -y # Größe prüfen".into(),
            default_targets: vec![HostId::new()],
        }
    }

    fn invalid(s: Snippet) -> bool {
        matches!(validate(&s), Err(SnippetError::Invalid(_)))
    }

    #[test]
    fn multi_line_content_with_tabs_and_umlauts_is_fine() {
        assert_eq!(validate(&snippet("Updates")), Ok(()));
    }

    #[test]
    fn bad_snippets_are_refused() {
        assert!(invalid(snippet("")));
        assert!(invalid(snippet("   ")));
        assert!(invalid(snippet(&"n".repeat(101))));
        let mut s = snippet("x");
        s.package = "p".repeat(101);
        assert!(invalid(s));
        let mut s = snippet("x");
        s.content = String::new();
        assert!(invalid(s));
        let mut s = snippet("x");
        s.content = "echo \u{1b}[31m".into();
        assert!(invalid(s));
        let mut s = snippet("x");
        s.content = "a\0b".into();
        assert!(invalid(s));
        let mut s = snippet("x");
        s.content = "x".repeat(10_001);
        assert!(invalid(s));
        let mut s = snippet("x");
        s.default_targets = (0..51).map(|_| HostId::new()).collect();
        assert!(invalid(s));
    }

    #[test]
    fn save_list_change_delete() {
        let (store, vault) = setup();
        let mut a = snippet("B second");
        let b = snippet("a first");
        save(&vault, &store, &a).unwrap();
        save(&vault, &store, &b).unwrap();
        assert_eq!(list(&vault, &store).unwrap(), vec![b.clone(), a.clone()]);
        a.content = "uptime".into();
        save(&vault, &store, &a).unwrap();
        assert_eq!(list(&vault, &store).unwrap()[1].content, "uptime");
        delete(&store, b.id).unwrap();
        assert_eq!(list(&vault, &store).unwrap(), vec![a]);
    }

    #[test]
    fn invalid_snippets_are_not_saved() {
        let (store, vault) = setup();
        assert!(matches!(
            save(&vault, &store, &snippet("")),
            Err(SnippetStoreError::Invalid(_))
        ));
        assert!(list(&vault, &store).unwrap().is_empty());
    }

    #[test]
    fn the_1001st_snippet_is_refused() {
        let (store, vault) = setup();
        for i in 0..MAX_SNIPPETS {
            save(&vault, &store, &snippet(&format!("s{i}"))).unwrap();
        }
        assert!(matches!(
            save(&vault, &store, &snippet("one more")),
            Err(SnippetStoreError::Invalid(_))
        ));
        let mut first = list(&vault, &store).unwrap().remove(0);
        first.content = "changed".into();
        save(&vault, &store, &first).unwrap();
    }

    #[test]
    fn nothing_in_plain_text() {
        let (store, vault) = setup();
        let mut s = snippet("Datenbank");
        s.content = "mysql -uroot -pGeheim42".into();
        save(&vault, &store, &s).unwrap();
        let blob: Vec<u8> = store
            .conn
            .query_row("SELECT sealed FROM snippets", [], |r| r.get(0))
            .unwrap();
        assert!(!blob.windows(8).any(|w| w == b"Geheim42"));
        assert!(!blob.windows(9).any(|w| w == b"Datenbank"));
    }

    #[test]
    fn a_broken_or_moved_block_is_left_out() {
        let (store, vault) = setup();
        let good = snippet("good");
        save(&vault, &store, &good).unwrap();
        let other = snippet("other");
        save(&vault, &store, &other).unwrap();
        // The block of "other" under a new id does not open.
        store
            .conn
            .execute(
                "UPDATE snippets SET id = ?1 WHERE id = ?2",
                [uuid::Uuid::new_v4().to_string(), other.id.0.to_string()],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO snippets (id, sealed) VALUES ('not-a-uuid', x'00')",
                [],
            )
            .unwrap();
        assert_eq!(list(&vault, &store).unwrap(), vec![good]);
    }
}

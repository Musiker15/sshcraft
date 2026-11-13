//! Snippets: commands or short scripts, sorted into packages, with hosts they usually run on.
//! Kept on this device only.

use serde::{Deserialize, Serialize};

use crate::model::{HostId, SnippetId};
use crate::store::{Store, StoreError};

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

/// What the `body` column holds: everything but id and name.
#[derive(Serialize, Deserialize)]
struct Body {
    package: String,
    content: String,
    default_targets: Vec<HostId>,
}

impl Store {
    /// All snippets by name; a row that cannot be read is left out.
    pub fn list_snippets(&self) -> Result<Vec<Snippet>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, body FROM snippets ORDER BY name COLLATE NOCASE, id")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut snippets = Vec::new();
        for row in rows {
            let (id, name, body) = row?;
            let (Ok(id), Ok(body)) = (
                uuid::Uuid::parse_str(&id),
                serde_json::from_str::<Body>(&body),
            ) else {
                continue;
            };
            let snippet = Snippet {
                id: SnippetId(id),
                name,
                package: body.package,
                content: body.content,
                default_targets: body.default_targets,
            };
            if validate(&snippet).is_ok() {
                snippets.push(snippet);
            }
        }
        Ok(snippets)
    }

    /// Stores a valid snippet; a new one only while there are fewer than [`MAX_SNIPPETS`].
    pub fn upsert_snippet(&self, snippet: &Snippet) -> Result<(), StoreError> {
        validate(snippet)?;
        let id = snippet.id.0.to_string();
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM snippets WHERE id = ?1)",
            [&id],
            |r| r.get(0),
        )?;
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM snippets", [], |r| r.get(0))?;
        if !exists && usize::try_from(count).unwrap_or(usize::MAX) >= MAX_SNIPPETS {
            return Err(SnippetError::Invalid("too many").into());
        }
        let body = serde_json::to_string(&Body {
            package: snippet.package.trim().to_owned(),
            content: snippet.content.clone(),
            default_targets: snippet.default_targets.clone(),
        })
        .map_err(|e| StoreError::Malformed(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO snippets (id, name, body) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, body = excluded.body",
            rusqlite::params![id, snippet.name.trim(), body],
        )?;
        Ok(())
    }

    pub fn delete_snippet(&self, id: SnippetId) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM snippets WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }

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
        let store = Store::open_in_memory().unwrap();
        let mut a = snippet("B second");
        let b = snippet("A first");
        store.upsert_snippet(&a).unwrap();
        store.upsert_snippet(&b).unwrap();
        assert_eq!(store.list_snippets().unwrap(), vec![b.clone(), a.clone()]);
        a.content = "uptime".into();
        store.upsert_snippet(&a).unwrap();
        assert_eq!(store.list_snippets().unwrap()[1].content, "uptime");
        store.delete_snippet(b.id).unwrap();
        assert_eq!(store.list_snippets().unwrap(), vec![a]);
    }

    #[test]
    fn invalid_snippets_are_not_saved() {
        let store = Store::open_in_memory().unwrap();
        assert!(matches!(
            store.upsert_snippet(&snippet("")),
            Err(StoreError::InvalidSnippet(_))
        ));
        assert!(store.list_snippets().unwrap().is_empty());
    }

    #[test]
    fn the_1001st_snippet_is_refused() {
        let store = Store::open_in_memory().unwrap();
        for i in 0..MAX_SNIPPETS {
            store.upsert_snippet(&snippet(&format!("s{i}"))).unwrap();
        }
        assert!(matches!(
            store.upsert_snippet(&snippet("one more")),
            Err(StoreError::InvalidSnippet(_))
        ));
        // Changing an existing one still works.
        let mut first = store.list_snippets().unwrap().remove(0);
        first.content = "changed".into();
        store.upsert_snippet(&first).unwrap();
    }

    #[test]
    fn a_broken_row_is_left_out() {
        let store = Store::open_in_memory().unwrap();
        let good = snippet("good");
        store.upsert_snippet(&good).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO snippets (id, name, body) VALUES ('not-a-uuid', 'bad', '{')",
                [],
            )
            .unwrap();
        assert_eq!(store.list_snippets().unwrap(), vec![good]);
    }
}

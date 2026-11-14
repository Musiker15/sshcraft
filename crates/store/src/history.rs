//! Command history per terminal target (`host:<uuid>`, `local:<label>`). Commands can hold
//! secrets, so each target's list is sealed with the vault and stored as one block. It never
//! syncs.

use serde::{Deserialize, Serialize};

use crate::store::{Store, StoreError};
use crate::vault::{Vault, VaultError};

pub const MAX_ENTRIES: usize = 1000;
pub const MAX_COMMAND: usize = 2000;
const MAX_SCOPE: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub command: String,
    pub uses: u32,
    /// Milliseconds since the epoch.
    pub last_used: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("invalid history scope")]
    Scope,
    #[error("invalid command for the history")]
    Command,
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// `host:<uuid>` or `local:<label>`.
pub fn valid_scope(scope: &str) -> bool {
    if scope.len() > MAX_SCOPE || scope.chars().any(char::is_control) {
        return false;
    }
    if let Some(id) = scope.strip_prefix("host:") {
        return uuid::Uuid::parse_str(id).is_ok();
    }
    scope.strip_prefix("local:").is_some_and(|label| !label.trim().is_empty())
}

fn label(scope: &str) -> String {
    format!("history:{scope}")
}

/// Records `command` as just used: a known one moves to the front and counts once more.
pub fn record(vault: &Vault, store: &Store, scope: &str, command: &str, now_ms: i64) -> Result<(), HistoryError> {
    if !valid_scope(scope) {
        return Err(HistoryError::Scope);
    }
    let command_ok = !command.trim().is_empty() && command.chars().count() <= MAX_COMMAND && !command.chars().any(char::is_control);
    if !command_ok {
        return Err(HistoryError::Command);
    }
    let mut entries = list(vault, store, scope)?;
    let uses = match entries.iter().position(|e| e.command == command) {
        Some(index) => entries.remove(index).uses.saturating_add(1),
        None => 1,
    };
    entries.insert(0, HistoryEntry { command: command.to_owned(), uses, last_used: now_ms });
    entries.truncate(MAX_ENTRIES);
    let plain = zeroize::Zeroizing::new(serde_json::to_vec(&entries).map_err(|e| StoreError::Malformed(e.to_string()))?);
    let sealed = vault.seal_secret(&label(scope), &plain)?;
    store.put_history_block(scope, &sealed)?;
    Ok(())
}

/// The target's history, most recently used first.
pub fn list(vault: &Vault, store: &Store, scope: &str) -> Result<Vec<HistoryEntry>, HistoryError> {
    if !valid_scope(scope) {
        return Err(HistoryError::Scope);
    }
    let Some(sealed) = store.history_block(scope)? else {
        return Ok(Vec::new());
    };
    // Sealed under its own scope: a block moved to another scope does not open.
    let plain = vault.open_secret(&label(scope), &sealed)?;
    serde_json::from_slice(&plain).map_err(|e| StoreError::Malformed(e.to_string()).into())
}

/// Forgets one target's history, or with `None` all of it.
pub fn clear(store: &Store, scope: Option<&str>) -> Result<(), HistoryError> {
    if scope.is_some_and(|s| !valid_scope(s)) {
        return Err(HistoryError::Scope);
    }
    store.delete_history_blocks(scope)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::MemoryMasterKey;

    const HOST: &str = "host:6f0c5c55-7d36-4c29-9d37-0e7c7a1a7b4e";
    const OTHER: &str = "local:PowerShell";

    fn setup() -> (Store, Vault) {
        let store = Store::open_in_memory().unwrap();
        let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
        (store, vault)
    }

    fn commands(vault: &Vault, store: &Store, scope: &str) -> Vec<String> {
        list(vault, store, scope).unwrap().into_iter().map(|e| e.command).collect()
    }

    #[test]
    fn record_and_list_newest_first() {
        let (store, vault) = setup();
        record(&vault, &store, HOST, "ls -la", 1).unwrap();
        record(&vault, &store, HOST, "uptime", 2).unwrap();
        assert_eq!(commands(&vault, &store, HOST), vec!["uptime", "ls -la"]);
    }

    #[test]
    fn same_command_moves_up_and_counts() {
        let (store, vault) = setup();
        record(&vault, &store, HOST, "ls", 1).unwrap();
        record(&vault, &store, HOST, "df -h", 2).unwrap();
        record(&vault, &store, HOST, "ls", 3).unwrap();
        let entries = list(&vault, &store, HOST).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], HistoryEntry { command: "ls".into(), uses: 2, last_used: 3 });
    }

    #[test]
    fn keeps_at_most_1000() {
        let (store, vault) = setup();
        for i in 0..1005 {
            record(&vault, &store, HOST, &format!("echo {i}"), i).unwrap();
        }
        let all = commands(&vault, &store, HOST);
        assert_eq!(all.len(), MAX_ENTRIES);
        assert_eq!(all[0], "echo 1004");
        assert!(!all.contains(&"echo 0".to_owned()));
    }

    #[test]
    fn rejects_long_and_control_commands() {
        let (store, vault) = setup();
        for bad in ["x".repeat(2001), "a\nb".into(), "a\u{1b}b".into(), String::new(), "   ".into()] {
            assert!(matches!(record(&vault, &store, HOST, &bad, 1), Err(HistoryError::Command)), "{bad:?}");
        }
        assert!(commands(&vault, &store, HOST).is_empty());
    }

    #[test]
    fn scopes_are_separate() {
        let (store, vault) = setup();
        record(&vault, &store, HOST, "on the host", 1).unwrap();
        record(&vault, &store, OTHER, "dir", 2).unwrap();
        assert_eq!(commands(&vault, &store, HOST), vec!["on the host"]);
        assert_eq!(commands(&vault, &store, OTHER), vec!["dir"]);
    }

    #[test]
    fn clear_one_and_all() {
        let (store, vault) = setup();
        record(&vault, &store, HOST, "a", 1).unwrap();
        record(&vault, &store, OTHER, "b", 1).unwrap();
        clear(&store, Some(HOST)).unwrap();
        assert!(commands(&vault, &store, HOST).is_empty());
        assert_eq!(commands(&vault, &store, OTHER), vec!["b"]);
        clear(&store, None).unwrap();
        assert!(commands(&vault, &store, OTHER).is_empty());
    }

    #[test]
    fn nothing_in_plain_text() {
        let (store, vault) = setup();
        record(&vault, &store, HOST, "export TOKEN=geheim42", 1).unwrap();
        let blob: Vec<u8> = store.conn.query_row("SELECT sealed FROM command_history", [], |r| r.get(0)).unwrap();
        assert!(!blob.windows(8).any(|w| w == b"geheim42"));
        assert!(!blob.is_empty());
    }

    #[test]
    fn a_block_moved_to_another_scope_does_not_open() {
        let (store, vault) = setup();
        record(&vault, &store, HOST, "secret of the host", 1).unwrap();
        store.conn.execute("UPDATE command_history SET scope = ?1", [OTHER]).unwrap();
        assert!(list(&vault, &store, OTHER).is_err());
    }

    #[test]
    fn invalid_scope_is_refused() {
        let (store, vault) = setup();
        for bad in ["", "x", "host:", "local:", "host:not-a-uuid", "serial:COM3"] {
            assert!(!valid_scope(bad), "{bad}");
            assert!(matches!(record(&vault, &store, bad, "ls", 1), Err(HistoryError::Scope)));
        }
        assert!(valid_scope(HOST));
        assert!(valid_scope(OTHER));
    }
}

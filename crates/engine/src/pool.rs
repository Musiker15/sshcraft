//! Connection reuse, like OpenSSH's ControlMaster: tabs, port forwards and commands to one host
//! share its SSH connection instead of signing in again. The pool holds connections weakly, so a
//! connection ends when its last user lets go of it; one that ended is never handed out.
//!
//! A connection is shared only between users that ask for the same route with the same options
//! (the key covers the whole resolved route), and remote forwards keep a connection of their own:
//! the server's channels for them are routed per connection.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use sshcraft_model::model::{HostId, Route};
use sshcraft_ssh::Session;

/// Connections remembered at most; dead ones are dropped first.
const MAX_ENTRIES: usize = 256;

/// What decides whether two users may share a connection: the host and the whole resolved route
/// with every option that shapes the connection, compared in full (no hash that could collide).
pub(crate) type Key = (HostId, String);

pub(crate) fn key(host: HostId, route: &Route) -> Key {
    (host, format!("{route:?}"))
}

#[derive(Default)]
pub(crate) struct Pool {
    entries: Mutex<HashMap<Key, Weak<Session>>>,
    /// One lock per key while a connection for it is being made, so users that start at once
    /// wait for it instead of each signing in (and waking the computer) on their own.
    connecting: Mutex<HashMap<Key, Arc<tokio::sync::Mutex<()>>>>,
}

impl Pool {
    /// The live connection for `key`, if one is in use.
    pub(crate) fn get(&self, key: &Key) -> Option<Arc<Session>> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let session = entries.get(key).and_then(Weak::upgrade);
        match session {
            Some(s) if !s.is_closed() => Some(s),
            _ => {
                entries.remove(key);
                None
            }
        }
    }

    /// The lock to hold while making the connection for `key`.
    pub(crate) fn gate(&self, key: &Key) -> Arc<tokio::sync::Mutex<()>> {
        let mut gates = self.connecting.lock().unwrap_or_else(PoisonError::into_inner);
        // Locks nobody waits on any more go.
        if gates.len() >= MAX_ENTRIES {
            gates.retain(|_, g| Arc::strong_count(g) > 1);
        }
        gates.entry(key.clone()).or_default().clone()
    }

    /// Hands out none of the connections in use any more (after the app lock); their users keep
    /// them.
    pub(crate) fn clear(&self) {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    /// Remembers `session` for `key`.
    pub(crate) fn put(&self, key: Key, session: &Arc<Session>) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() >= MAX_ENTRIES {
            entries.retain(|_, s| s.upgrade().is_some_and(|s| !s.is_closed()));
        }
        if entries.len() < MAX_ENTRIES {
            entries.insert(key, Arc::downgrade(session));
        }
    }
}

/// Ends `session` when this was its last user; a shared one stays for the others.
pub(crate) async fn release(session: Arc<Session>) {
    if let Ok(session) = Arc::try_unwrap(session) {
        session.disconnect().await;
    }
}

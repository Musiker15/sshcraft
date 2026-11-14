//! What belongs to one connection of an agent or of the control channel: the sessions it opened
//! end with it, and no other connection reaches them.

use std::collections::BTreeMap;
use std::sync::{PoisonError, Weak};

use sshcraft_model::model::HostId;

use crate::caller::{AgentName, Caller, ConnId, Via};
use crate::sessions::SessionId;

/// The sessions one connection opened, and on which host. A snapshot is what `resolve` sees.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Connection {
    pub conn: Option<ConnId>,
    pub sessions: BTreeMap<SessionId, HostId>,
}

impl Connection {
    pub fn owns_session(&self, id: SessionId) -> bool {
        self.sessions.contains_key(&id)
    }

    pub fn host_of(&self, id: SessionId) -> Option<HostId> {
        self.sessions.get(&id).copied()
    }
}

/// An open connection of an agent or of the control channel. Calls go out with
/// [`AgentConnection::caller`]; dropping it closes the sessions it opened.
pub struct AgentConnection {
    caller: Caller,
    conn: ConnId,
    engine: Weak<crate::Inner>,
}

impl AgentConnection {
    pub(crate) fn new(conn: ConnId, name: Option<(AgentName, Via)>, engine: Weak<crate::Inner>) -> Self {
        let caller = match name {
            Some((name, via)) => Caller::Agent { name, via, conn },
            None => Caller::Control { conn },
        };
        Self { caller, conn, engine }
    }

    pub fn caller(&self) -> Caller {
        self.caller.clone()
    }

    /// The id of this connection (tests reach a connection after it is gone through it).
    #[cfg(test)]
    pub(crate) fn id(&self) -> ConnId {
        self.conn
    }
}

impl Drop for AgentConnection {
    fn drop(&mut self) {
        let Some(inner) = self.engine.upgrade() else {
            return;
        };
        let gone = inner.connections.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.conn);
        for id in gone.map(|c| c.sessions.into_keys().collect::<Vec<_>>()).unwrap_or_default() {
            inner.agent_sessions.close(&inner.sessions, id);
        }
    }
}

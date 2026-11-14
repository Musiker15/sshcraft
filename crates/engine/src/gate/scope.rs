//! What belongs to one connection of an agent or of the control channel: the sessions it opened
//! end with it.

use crate::caller::ConnId;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Connection {
    pub conn: Option<ConnId>,
    /// The ids of the sessions this connection opened.
    pub sessions: Vec<String>,
}

impl Connection {
    pub fn owns_session(&self, id: &str) -> bool {
        self.sessions.iter().any(|s| s == id)
    }
}

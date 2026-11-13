//! Commands and terminal sessions run for AI agents. They go through the same connection code as
//! a tab (keys stay in the vault, host keys are checked, Windows Hello is asked), and an agent
//! reaches only the sessions it opened itself.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sshcraft_core::model::HostId;
use tokio::sync::Notify;

use crate::errors::ErrorCode;
use crate::sessions::{
    OpenedKeys, SessionId, SessionManager, SessionSink, SessionStatus, SessionTarget,
    connect_session, failure_code,
};

/// Output of an agent's session, kept until the agent reads it. Not acknowledged output stops
/// the session from reading further (flow control), so this stays near
/// [`crate::sessions::OUTPUT_HIGH_WATERMARK`].
#[derive(Default)]
pub struct AgentSink {
    buffer: Mutex<Vec<u8>>,
    ended: AtomicBool,
    arrived: Notify,
}

impl AgentSink {
    pub fn buffered(&self) -> usize {
        self.buffer.lock().expect("agent buffer").len()
    }
}

impl SessionSink for AgentSink {
    fn data(&self, bytes: Vec<u8>) {
        self.buffer
            .lock()
            .expect("agent buffer")
            .extend_from_slice(&bytes);
        self.arrived.notify_waiters();
    }

    fn status(&self, status: SessionStatus) {
        if matches!(status, SessionStatus::Ended { .. }) {
            self.ended.store(true, Ordering::SeqCst);
            self.arrived.notify_waiters();
        }
    }
}

/// What an agent read: output, and whether the session is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRead {
    pub output: Vec<u8>,
    pub ended: bool,
}

/// The sessions agents opened, by id.
#[derive(Default)]
pub struct AgentSessions {
    sinks: Mutex<HashMap<SessionId, Arc<AgentSink>>>,
}

pub type ExecError = (ErrorCode, Option<String>);

impl AgentSessions {
    /// Runs `command` on a host on a connection of its own; at most `limit` bytes of output,
    /// `true` when it was cut.
    pub async fn exec(
        manager: &SessionManager,
        host: HostId,
        command: &str,
        limit: usize,
        timeout: Duration,
    ) -> Result<(Vec<u8>, bool), ExecError> {
        let ctx = manager.context();
        let run = async {
            let (session, _) = connect_session(&ctx, host, &OpenedKeys::default())
                .await
                .map_err(failure_code)?;
            let output = session.exec(command, limit.saturating_add(1)).await;
            session.disconnect().await;
            output.map_err(|e| (ErrorCode::Session, Some(e.to_string())))
        };
        let mut output = tokio::time::timeout(timeout, run)
            .await
            .map_err(|_| (ErrorCode::Timeout, None))??;
        let cut = output.len() > limit;
        output.truncate(limit);
        Ok((output, cut))
    }

    pub fn open(&self, manager: &SessionManager, host: HostId) -> SessionId {
        let sink = Arc::new(AgentSink::default());
        let id = manager.open(SessionTarget::Host { host_id: host }, 120, 40, sink.clone());
        self.sinks.lock().expect("agent sessions").insert(id, sink);
        id
    }

    fn sink(&self, id: SessionId) -> Option<Arc<AgentSink>> {
        self.sinks.lock().expect("agent sessions").get(&id).cloned()
    }

    /// Whether `id` is a session an agent opened (only those are an agent's business).
    pub fn owns(&self, id: SessionId) -> bool {
        self.sink(id).is_some()
    }

    pub fn write(
        &self,
        manager: &SessionManager,
        id: SessionId,
        data: &[u8],
    ) -> Result<(), String> {
        if !self.owns(id) {
            return Err("unknown session".into());
        }
        manager.write(id, data)
    }

    /// Up to `limit` bytes of output; waits up to `wait` when there is none yet.
    pub async fn read(
        &self,
        manager: &SessionManager,
        id: SessionId,
        limit: usize,
        wait: Duration,
    ) -> Result<AgentRead, String> {
        let sink = self.sink(id).ok_or("unknown session")?;
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let arrived = sink.arrived.notified();
            {
                let mut buffer = sink.buffer.lock().expect("agent buffer");
                let ended = sink.ended.load(Ordering::SeqCst);
                if !buffer.is_empty() || ended {
                    let take = buffer.len().min(limit);
                    let output: Vec<u8> = buffer.drain(..take).collect();
                    drop(buffer);
                    manager.ack(id, output.len());
                    return Ok(AgentRead { output, ended });
                }
            }
            if tokio::time::timeout_at(deadline, arrived).await.is_err() {
                return Ok(AgentRead {
                    output: Vec::new(),
                    ended: false,
                });
            }
        }
    }

    pub fn close(&self, manager: &SessionManager, id: SessionId) -> bool {
        let known = self
            .sinks
            .lock()
            .expect("agent sessions")
            .remove(&id)
            .is_some();
        if known {
            manager.close(id);
        }
        known
    }

    /// Buffered output of a session (for tests and the size check).
    pub fn buffered(&self, id: SessionId) -> usize {
        self.sink(id).map_or(0, |s| s.buffered())
    }
}

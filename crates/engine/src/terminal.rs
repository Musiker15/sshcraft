//! The terminal stream: how the interface gets the output and the state of a session it opened.
//! Output waits in the stream until the interface takes it; bytes it has not acknowledged count
//! against the session's high watermark, so a tab that is never read does not let the engine read
//! the server without end.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::sessions::{Flow, SessionId, SessionSink, SessionStatus};

/// What a session tells its terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEvent {
    Data(Vec<u8>),
    Status(SessionStatus),
}

/// Wakes whoever reads the terminal streams (the window's repaint); called from any thread.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Puts a session's output and state into its stream.
pub(crate) struct StreamSink {
    pub(crate) sender: mpsc::UnboundedSender<TermEvent>,
    pub(crate) waker: Option<Waker>,
}

impl StreamSink {
    fn wake(&self) {
        if let Some(waker) = &self.waker {
            waker();
        }
    }
}

impl SessionSink for StreamSink {
    fn data(&self, bytes: Vec<u8>) {
        // The receiver is gone once the session is closed: nothing to deliver to.
        let _ = self.sender.send(TermEvent::Data(bytes));
        self.wake();
    }

    fn status(&self, status: SessionStatus) {
        let _ = self.sender.send(TermEvent::Status(status));
        self.wake();
    }
}

/// The receiving end of one session's terminal. Taken once with
/// [`crate::Session::attach_terminal`].
pub struct TerminalStream {
    id: SessionId,
    receiver: mpsc::UnboundedReceiver<TermEvent>,
    flow: Arc<Flow>,
}

impl TerminalStream {
    pub(crate) fn new(id: SessionId, receiver: mpsc::UnboundedReceiver<TermEvent>, flow: Arc<Flow>) -> Self {
        Self { id, receiver, flow }
    }

    pub fn id(&self) -> SessionId {
        self.id
    }

    /// The next event; `None` once the session is closed and everything was taken.
    pub async fn recv(&mut self) -> Option<TermEvent> {
        self.receiver.recv().await
    }

    /// The next event if there is one, without waiting (for the interface's frame loop).
    pub fn try_recv(&mut self) -> Option<TermEvent> {
        self.receiver.try_recv().ok()
    }

    /// Nothing is waiting in the stream right now.
    pub fn is_empty(&self) -> bool {
        self.receiver.is_empty()
    }

    /// The session is closed and everything it sent was taken.
    pub fn is_closed(&self) -> bool {
        self.receiver.is_closed() && self.receiver.is_empty()
    }

    /// The interface has shown `bytes` more output bytes: the session may read on.
    pub fn ack(&self, bytes: usize) {
        self.flow.ack(bytes);
    }
}

#[cfg(test)]
#[path = "terminal_tests.rs"]
mod tests;

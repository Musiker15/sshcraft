//! Moves a session's output into its terminal, a frame's worth at a time.
//!
//! A frame feeds slices of [`FEED_SLICE`] bytes until it has fed [`FrameBudget::max_bytes`] or
//! spent [`FrameBudget::max_time`], so a flood (1 GB through `cat`) never stalls the window, and
//! acknowledges exactly what it fed. What it took from the stream but did not feed yet waits in
//! the pump and still counts against the session's high watermark: the engine reads the server
//! on only as the terminal catches up.

use std::time::{Duration, Instant};

use sshcraft_engine::terminal::{TermEvent, TerminalStream};
use sshcraft_engine::{SessionId, SessionStatus};
use sshcraft_term::Terminal;

/// Most bytes one frame feeds.
pub const FRAME_BYTE_BUDGET: usize = 4 * 1024 * 1024;
/// Most time one frame feeds for (measured in R3.1: 1 MiB takes up to 8.8 ms for CJK output).
pub const FRAME_TIME_BUDGET: Duration = Duration::from_millis(6);
/// The bytes fed between two looks at the clock.
pub const FEED_SLICE: usize = 32 * 1024;

/// How much one frame may feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameBudget {
    pub max_bytes: usize,
    pub max_time: Duration,
}

impl FrameBudget {
    pub const DEFAULT: Self = Self { max_bytes: FRAME_BYTE_BUDGET, max_time: FRAME_TIME_BUDGET };
}

/// What one frame did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PumpOutcome {
    /// Bytes fed into the terminal (and acknowledged).
    pub fed: usize,
    /// The newest status the session reported in this frame.
    pub status: Option<SessionStatus>,
    /// Output is waiting: ask for the next frame.
    pub more: bool,
    /// The session is closed and everything was fed.
    pub closed: bool,
}

pub struct Pump {
    stream: TerminalStream,
    pending: Vec<u8>,
    offset: usize,
}

impl Pump {
    pub fn new(stream: TerminalStream) -> Self {
        Self { stream, pending: Vec::new(), offset: 0 }
    }

    pub fn id(&self) -> SessionId {
        self.stream.id()
    }

    /// Bytes taken from the stream and not fed yet.
    pub fn held(&self) -> usize {
        self.pending.len().saturating_sub(self.offset)
    }

    /// Feeds up to `budget` of output into `term`.
    pub fn frame(&mut self, term: &mut Terminal, budget: FrameBudget) -> PumpOutcome {
        self.frame_into(&mut |slice| term.feed(slice), budget)
    }

    /// [`Pump::frame`] into `feed` instead of a terminal (the tmux control stream).
    pub fn frame_into(&mut self, feed: &mut dyn FnMut(&[u8]), budget: FrameBudget) -> PumpOutcome {
        let start = Instant::now();
        let mut out = PumpOutcome::default();
        loop {
            if self.held() == 0 {
                self.pending.clear();
                self.offset = 0;
                match self.stream.try_recv() {
                    Some(TermEvent::Data(bytes)) => self.pending = bytes,
                    Some(TermEvent::Status(status)) => {
                        out.status = Some(status);
                        continue;
                    }
                    None => break,
                }
            }
            let room = budget.max_bytes.saturating_sub(out.fed);
            if room == 0 || (out.fed > 0 && start.elapsed() >= budget.max_time) {
                break;
            }
            let take = FEED_SLICE.min(room).min(self.held());
            let end = self.offset + take;
            if let Some(slice) = self.pending.get(self.offset..end) {
                feed(slice);
            }
            self.offset = end;
            out.fed += take;
            // Acknowledged only once it is in the terminal.
            self.stream.ack(take);
        }
        out.more = self.held() > 0 || !self.stream.is_empty();
        out.closed = self.held() == 0 && self.stream.is_closed();
        out
    }
}

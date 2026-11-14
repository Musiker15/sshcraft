//! Collects what the emulator reports while it parses.

use std::sync::{Arc, Mutex, PoisonError};

use alacritty_terminal::event::{Event, EventListener};

/// Events kept until the terminal's owner takes them; more are dropped, so a server that floods
/// bells or answers cannot grow the queue without end.
const MAX_EVENTS: usize = 4096;

#[derive(Clone, Default)]
pub(crate) struct Listener {
    events: Arc<Mutex<Vec<Event>>>,
}

impl Listener {
    pub(crate) fn take(&self) -> Vec<Event> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        if events.len() < MAX_EVENTS {
            events.push(event);
        }
    }
}

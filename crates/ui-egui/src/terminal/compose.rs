//! The paste history and the compose field. Pastes are kept in memory only, never saved: they
//! may hold passwords. The compose field is a multi-line editor under the terminal; Ctrl+Enter
//! sends its text, as a paste followed by Enter, to the tab or the tabs it broadcasts to.

use std::collections::VecDeque;

use sshcraft_term::TermModes;

/// Pastes kept.
pub const MAX_PASTES: usize = 20;
/// The largest paste kept, in bytes.
pub const MAX_PASTE_BYTES: usize = 64 * 1024;
/// The height of the compose field, in points.
pub const COMPOSE_HEIGHT: f32 = 96.0;

/// The last pastes, newest first.
#[derive(Debug, Default, Clone)]
pub struct PasteHistory {
    entries: VecDeque<String>,
}

impl PasteHistory {
    /// Keeps `text` as the newest paste; a blank or too large one is not kept, a repeated one moves
    /// to the front.
    pub fn push(&mut self, text: &str) {
        if text.trim().is_empty() || text.len() > MAX_PASTE_BYTES {
            return;
        }
        self.entries.retain(|e| e != text);
        self.entries.push_front(text.to_string());
        self.entries.truncate(MAX_PASTES);
    }

    pub fn entries(&self) -> &VecDeque<String> {
        &self.entries
    }
}

/// What the compose field sends for `text`: the text as a paste, then Enter.
pub fn compose_bytes(text: &str, modes: TermModes) -> Vec<u8> {
    let mut bytes = sshcraft_term::input::encode_paste(text.trim_end_matches(['\r', '\n']), modes);
    bytes.push(b'\r');
    bytes
}

/// The first line of a paste, cut for the list.
pub fn preview(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default();
    let mut shown: String = line.chars().filter(|c| !c.is_control()).take(80).collect();
    if line.chars().count() > 80 || text.lines().filter(|l| !l.trim().is_empty()).count() > 1 {
        shown.push_str(" …");
    }
    shown
}

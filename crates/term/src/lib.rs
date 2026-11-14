#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
//! The terminal of SSHCraft without a toolkit: escape sequences in, a screen out.
//!
//! It wraps `alacritty_terminal` completely; nothing outside this crate sees the emulator's
//! types, so the renderer and the interface stay the same if the emulator changes. Everything
//! passed to [`Terminal::feed`] is hostile: a size is clamped, the scrollback is bounded by
//! [`limits::MAX_CELLS`], and the queue of answers and notices has a limit.

mod extras;
mod filter;
mod guard;
pub mod input;
pub mod limits;
mod listener;
mod marks;
mod screen;
mod search;
mod selection;

use alacritty_terminal::event::Event;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, Osc52, Term};
use alacritty_terminal::vte::ansi::Processor;

pub use screen::{CellFlags, CellView, CursorShape, CursorView, Screen, SelectionView, TermColor, TermModes};
pub use search::{SearchError, SearchMatch, SearchQuery};
pub use selection::{GridPoint, SelectionKind};

/// Towards the history (`Up`) or towards the newest output (`Down`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

use filter::Prefilter;
use limits::{DEFAULT_SCROLLBACK, MAX_CELLS, MAX_CLIPBOARD_BYTES, MAX_COLS, MAX_ROWS, MAX_SCROLLBACK, MAX_TITLE_CHARS, MIN_COLS, MIN_ROWS};
use listener::Listener;

/// How a terminal is set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermConfig {
    /// Scrollback lines wanted; the terminal keeps fewer when the grid would pass the budget.
    pub scrollback: usize,
    /// Programs may write the clipboard (OSC 52). They can never read it.
    pub clipboard_write: bool,
}

impl Default for TermConfig {
    fn default() -> Self {
        Self { scrollback: DEFAULT_SCROLLBACK, clipboard_write: true }
    }
}

/// Columns and rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
}

impl TermSize {
    /// `cols` by `rows`, clamped to [`limits::MIN_COLS`]..=[`limits::MAX_COLS`] and
    /// [`limits::MIN_ROWS`]..=[`limits::MAX_ROWS`].
    pub fn clamped(cols: u16, rows: u16) -> Self {
        Self { cols: cols.clamp(MIN_COLS, MAX_COLS), rows: rows.clamp(MIN_ROWS, MAX_ROWS) }
    }
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

/// What the terminal reports besides its screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// Bytes the terminal answers to its own session (device attributes, cursor position).
    Reply(Vec<u8>),
    Title(String),
    ResetTitle,
    Bell,
    /// Text a program wants in the clipboard.
    ClipboardWrite(String),
}

/// The size in bytes of one grid cell of the emulator, for memory budgets.
pub fn cell_size() -> usize {
    std::mem::size_of::<alacritty_terminal::term::cell::Cell>()
}

/// Scrollback lines a `size` terminal keeps when `wanted` are asked for.
fn scrollback_for(size: TermSize, wanted: usize) -> usize {
    let lines = MAX_CELLS / usize::from(size.cols.max(1));
    wanted.min(MAX_SCROLLBACK).min(lines.saturating_sub(usize::from(size.rows)))
}

fn options(size: TermSize, config: &TermConfig) -> Config {
    // Programs may copy into the clipboard, never paste from it: a read is never answered.
    let osc52 = if config.clipboard_write { Osc52::OnlyCopy } else { Osc52::Disabled };
    Config { scrolling_history: scrollback_for(size, config.scrollback), osc52, ..Config::default() }
}

/// Characters that hide or reorder text in a title.
fn hides_text(c: char) -> bool {
    c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// A title as it may be shown: without control and bidi characters, at most
/// [`MAX_TITLE_CHARS`] characters.
fn clean_title(title: &str) -> String {
    title.chars().filter(|c| !hides_text(*c)).take(MAX_TITLE_CHARS).collect()
}

/// One terminal.
pub struct Terminal {
    term: Term<Listener>,
    parser: Processor,
    prefilter: Prefilter,
    /// What the prefilter lets through, reused between feeds.
    filtered: Vec<u8>,
    listener: Listener,
    size: TermSize,
    config: TermConfig,
    /// The last match and the query it belongs to, where "next" goes on from.
    last_match: Option<(u64, SearchMatch)>,
    /// Changes that may have added per-cell extras since the budget was last checked.
    extras_changes: usize,
}

impl Terminal {
    pub fn new(size: TermSize, config: &TermConfig) -> Self {
        let size = TermSize::clamped(size.cols, size.rows);
        let listener = Listener::default();
        let term = Term::new(options(size, config), &size, listener.clone());
        Self {
            term,
            parser: Processor::new(),
            prefilter: Prefilter::default(),
            filtered: Vec::new(),
            listener,
            size,
            config: config.clone(),
            last_match: None,
            extras_changes: 0,
        }
    }

    /// Output from the session.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut filtered = std::mem::take(&mut self.filtered);
        filtered.clear();
        self.prefilter.filter(bytes, &mut filtered);
        // In slices, so the extras budget is checked while a large feed is under way.
        for slice in filtered.chunks(64 * 1024) {
            let mut guard = guard::Guard { term: &mut self.term, extras: &mut self.extras_changes };
            self.parser.advance(&mut guard, slice);
            if self.extras_changes >= extras::CHECK_EVERY {
                self.extras_changes = 0;
                self.enforce_extras_budget();
            }
        }
        // A large feed must not keep its buffer for the life of the terminal.
        if filtered.capacity() <= 1 << 20 {
            self.filtered = filtered;
        }
    }

    /// Changes the size, clamped; the scrollback shrinks when the grid would pass the budget.
    pub fn resize(&mut self, size: TermSize) {
        let size = TermSize::clamped(size.cols, size.rows);
        if size == self.size {
            return;
        }
        // The history shrinks before a wider grid is allocated.
        self.term.set_options(options(size, &self.config));
        self.term.resize(size);
        self.size = size;
    }

    pub fn size(&self) -> TermSize {
        self.size
    }

    /// Scrollback lines this terminal keeps at its current size.
    pub fn effective_scrollback(&self) -> usize {
        scrollback_for(self.size, self.config.scrollback)
    }

    /// The input modes programs switched on (cursor keys, paste, mouse, focus).
    pub fn modes(&self) -> TermModes {
        screen::modes(*self.term.mode())
    }

    pub fn screen(&self) -> Screen {
        screen::screen(&self.term)
    }

    /// The visible rows as text, one line each, for screen readers.
    pub fn visible_text(&self) -> String {
        self.screen().text()
    }

    /// Answers and notices since the last call.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        self.listener
            .take()
            .into_iter()
            .filter_map(|event| match event {
                Event::PtyWrite(text) => Some(Notice::Reply(text.into_bytes())),
                Event::Title(title) => Some(Notice::Title(clean_title(&title))),
                Event::ResetTitle => Some(Notice::ResetTitle),
                Event::Bell => Some(Notice::Bell),
                Event::ClipboardStore(_, text) if self.config.clipboard_write && text.len() <= MAX_CLIPBOARD_BYTES => {
                    Some(Notice::ClipboardWrite(text))
                }
                _ => None,
            })
            .collect()
    }
}

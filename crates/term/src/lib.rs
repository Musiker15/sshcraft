#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
//! The terminal of SSHCraft without a toolkit: escape sequences in, a screen out.
//!
//! It wraps `alacritty_terminal` completely; nothing outside this crate sees the emulator's
//! types, so the renderer and the interface stay the same if the emulator changes. Everything
//! passed to [`Terminal::feed`] is hostile: a size is clamped, the scrollback is bounded by
//! [`limits::MAX_CELLS`], and the queue of answers and notices has a limit.

pub mod limits;
mod listener;
mod screen;

use alacritty_terminal::event::Event;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;

pub use screen::{CellFlags, CellView, CursorShape, CursorView, Screen, SelectionView, TermColor, TermModes};

use limits::{DEFAULT_SCROLLBACK, MAX_CELLS, MAX_COLS, MAX_ROWS, MAX_SCROLLBACK, MIN_COLS, MIN_ROWS};
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
    Config { scrolling_history: scrollback_for(size, config.scrollback), ..Config::default() }
}

/// One terminal.
pub struct Terminal {
    term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    size: TermSize,
    config: TermConfig,
}

impl Terminal {
    pub fn new(size: TermSize, config: &TermConfig) -> Self {
        let size = TermSize::clamped(size.cols, size.rows);
        let listener = Listener::default();
        let term = Term::new(options(size, config), &size, listener.clone());
        Self { term, parser: Processor::new(), listener, size, config: config.clone() }
    }

    /// Output from the session.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
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

    pub fn screen(&self) -> Screen {
        screen::screen(&self.term)
    }

    /// The visible rows as text, one line each, for screen readers.
    pub fn visible_text(&self) -> String {
        let s = self.screen();
        s.cells
            .chunks(usize::from(s.cols).max(1))
            .map(|row| row.iter().map(|c| c.text.as_str()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Answers and notices since the last call.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        self.listener
            .take()
            .into_iter()
            .filter_map(|event| match event {
                Event::PtyWrite(text) => Some(Notice::Reply(text.into_bytes())),
                Event::Title(title) => Some(Notice::Title(title)),
                Event::ResetTitle => Some(Notice::ResetTitle),
                Event::Bell => Some(Notice::Bell),
                Event::ClipboardStore(_, text) => Some(Notice::ClipboardWrite(text)),
                _ => None,
            })
            .collect()
    }
}

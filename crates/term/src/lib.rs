#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
//! The terminal of SSHCraft without a toolkit: escape sequences in, a screen out.
//!
//! It wraps `alacritty_terminal` completely; nothing outside this crate sees the emulator's
//! types, so the renderer and the interface stay the same if the emulator changes. Everything
//! passed to [`Terminal::feed`] is hostile: a size is clamped, the scrollback is bounded by
//! [`limits::MAX_CELLS`], and the queue of answers and notices has a limit.

mod commands;
mod extras;
mod filter;
mod guard;
pub mod input;
pub mod limits;
mod listener;
mod marks;
mod progstatus;
mod screen;
mod search;
mod selection;

use alacritty_terminal::event::Event;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, Osc52, Term};
use alacritty_terminal::vte::ansi::Processor;

pub use commands::{CommandRecord, CommandRow, CommandState, MAX_COMMANDS, MAX_OUTPUT_BYTES};
pub use progstatus::{BlockedKind, ProgramRecord, ProgramState};
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

const ALL_PROGRAM_STATES: [ProgramState; 5] =
    [ProgramState::Idle, ProgramState::Working, ProgramState::Done, ProgramState::Blocked, ProgramState::Error];

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
    /// The shell's working directory (OSC 7), a plain absolute path.
    WorkingDirectory(String),
    /// A command started running (OSC 133 C).
    CommandStarted,
    /// A command ended (OSC 133 D), with its exit code when the shell gave one.
    CommandFinished {
        exit: Option<i32>,
    },
    /// A message for the desktop (OSC 9, OSC 777): plain, bounded text.
    Notification {
        title: Option<String>,
        body: String,
    },
    /// The program status records changed (OSC 7501).
    ProgramStatusChanged,
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

/// Text from a program as it may be shown: without control and bidi characters, at most `max`
/// characters.
pub(crate) fn clean_text(text: &str, max: usize) -> String {
    text.chars().filter(|c| !hides_text(*c)).take(max).collect()
}

/// A title as it may be shown: at most [`MAX_TITLE_CHARS`] characters.
fn clean_title(title: &str) -> String {
    clean_text(title, MAX_TITLE_CHARS)
}

/// Finished lines kept for [`Terminal::take_completed_lines`]; older ones are dropped.
pub const MAX_COMPLETED_LINES: usize = 1000;
/// The longest finished line kept, in characters.
pub const MAX_LINE_CHARS: usize = 4096;

/// What cached command rows are for: the version of the terminal, the display offset, the rows.
type RowsKey = (u64, usize, usize);

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
    /// What the shell reported about its commands (OSC 133).
    commands: commands::Commands,
    /// What programs reported about themselves (OSC 7501).
    programs: progstatus::Programs,
    /// Whether the program status changed since the notices were last taken.
    programs_changed: bool,
    /// Finished output lines, kept only while [`Terminal::set_line_capture`] is on.
    completed: Option<std::collections::VecDeque<String>>,
    /// Counts every change that may move prompt lines or their records.
    version: u64,
    /// The command rows of the last screen, with the version, offset and height they are for:
    /// the interface asks for the screen several times a frame.
    rows_cache: std::cell::Cell<Option<(RowsKey, Vec<CommandRow>)>>,
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
            commands: commands::Commands::default(),
            programs: progstatus::Programs::default(),
            programs_changed: false,
            completed: None,
            version: 0,
            rows_cache: std::cell::Cell::new(None),
        }
    }

    /// Output from the session.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.changed();
        let mut filtered = std::mem::take(&mut self.filtered);
        filtered.clear();
        self.prefilter.filter(bytes, &mut filtered);
        // A shell mark applies to the line the cursor is on when the parser reaches it.
        let events = std::mem::take(&mut self.prefilter.events);
        let mut done = 0;
        for (offset, event) in events {
            self.advance(filtered.get(done..offset).unwrap_or_default());
            done = offset.max(done);
            match event {
                filter::FilterEvent::Shell(mark) => {
                    if mark == filter::ShellMark::PromptStart {
                        // A prompt means the programs of the last command are gone.
                        self.programs_changed |= self.programs.drop_states(&[ProgramState::Idle, ProgramState::Working, ProgramState::Blocked]);
                    }
                    self.apply_shell_mark(mark);
                }
                filter::FilterEvent::Status(report) => self.programs_changed |= self.programs.apply(report),
                filter::FilterEvent::Reset => self.programs_changed |= self.programs.drop_states(&ALL_PROGRAM_STATES),
            }
        }
        self.advance(filtered.get(done..).unwrap_or_default());
        // A large feed must not keep its buffer for the life of the terminal.
        if filtered.capacity() <= 1 << 20 {
            self.filtered = filtered;
        }
    }

    /// Runs `bytes` through the parser, in slices, so the extras budget is checked while a large
    /// feed is under way.
    fn advance(&mut self, bytes: &[u8]) {
        for slice in bytes.chunks(64 * 1024) {
            let mut guard = guard::Guard { term: &mut self.term, extras: &mut self.extras_changes, lines: self.completed.as_mut() };
            self.parser.advance(&mut guard, slice);
            if self.extras_changes >= extras::CHECK_EVERY {
                self.extras_changes = 0;
                self.enforce_extras_budget();
            }
        }
    }

    /// Changes the size, clamped; the scrollback shrinks when the grid would pass the budget.
    pub fn resize(&mut self, size: TermSize) {
        let size = TermSize::clamped(size.cols, size.rows);
        if size == self.size {
            return;
        }
        self.changed();
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
        let mut screen = screen::screen(&self.term);
        let key = (self.version, screen.display_offset, usize::from(screen.rows));
        let rows = match self.rows_cache.take() {
            Some((cached, rows)) if cached == key => rows,
            _ => self.command_rows(screen.display_offset, usize::from(screen.rows)),
        };
        self.rows_cache.set(Some((key, rows.clone())));
        screen.commands = rows;
        screen
    }

    /// Prompt lines or their records may have changed: the cached command rows are stale.
    pub(crate) fn changed(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    /// The visible rows as text, one line each, for screen readers.
    pub fn visible_text(&self) -> String {
        self.screen().text()
    }

    /// Keeps finished output lines (for triggers) or stops keeping them.
    pub fn set_line_capture(&mut self, on: bool) {
        if on {
            self.completed.get_or_insert_with(Default::default);
        } else {
            self.completed = None;
        }
    }

    /// The output lines that ended since the last call, oldest first; full-screen programs are
    /// left out.
    pub fn take_completed_lines(&mut self) -> Vec<String> {
        self.completed.as_mut().map(|lines| lines.drain(..).collect()).unwrap_or_default()
    }

    /// What programs reported about themselves (OSC 7501), the one updated longest ago first.
    pub fn program_status(&self) -> Vec<ProgramRecord> {
        self.programs.records()
    }

    /// The most urgent state any program reported: blocked, error, done, working, idle.
    pub fn status_summary(&self) -> Option<ProgramState> {
        self.programs.summary()
    }

    /// Drops the programs that are done or failed: the user has seen them (they typed).
    pub fn drop_finished(&mut self) {
        self.programs_changed |= self.programs.drop_states(&[ProgramState::Done, ProgramState::Error]);
    }

    /// Answers and notices since the last call.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        let cwd = self.prefilter.cwd.take().map(Notice::WorkingDirectory);
        let commands = self.take_command_notices();
        let replies = std::mem::take(&mut self.prefilter.replies).into_iter().map(Notice::Reply);
        let notifications = std::mem::take(&mut self.prefilter.notifications).into_iter().map(|(title, body)| Notice::Notification { title, body });
        let changed = std::mem::take(&mut self.programs_changed).then_some(Notice::ProgramStatusChanged);
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
            .chain(cwd)
            .chain(commands)
            .chain(replies)
            .chain(notifications)
            .chain(changed)
            .collect()
    }
}

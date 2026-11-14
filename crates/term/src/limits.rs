//! Bounds on everything a server can make the terminal hold. A size is clamped, never refused.

/// Narrowest terminal.
pub const MIN_COLS: u16 = 2;
/// Widest terminal.
pub const MAX_COLS: u16 = 1000;
/// Lowest terminal.
pub const MIN_ROWS: u16 = 1;
/// Highest terminal.
pub const MAX_ROWS: u16 = 500;

/// Scrollback lines when nothing else is asked for.
pub const DEFAULT_SCROLLBACK: usize = 10_000;
/// The most scrollback lines that can be asked for.
pub const MAX_SCROLLBACK: usize = 100_000;
/// Cells of screen and scrollback together; a wide terminal gets fewer scrollback lines.
pub const MAX_CELLS: usize = 4_000_000;
/// Memory for the grid and scrollback of one session.
pub const SESSION_MEMORY_BUDGET: usize = 128 * 1024 * 1024;

/// Longest OSC sequence: OSC 52 with 1 MiB of text in base64, plus its head.
pub const MAX_OSC_BYTES: usize = 1_572_864;
/// Most text a program may put into the clipboard.
pub const MAX_CLIPBOARD_BYTES: usize = 1_048_576;
/// Longest window title shown.
pub const MAX_TITLE_CHARS: usize = 256;
/// Longest link target kept.
pub const MAX_LINK_BYTES: usize = 2048;
/// Longest search pattern.
pub const MAX_SEARCH_PATTERN: usize = 1024;

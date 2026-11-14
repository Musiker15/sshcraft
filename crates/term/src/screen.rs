//! What the terminal shows, in SSHCraft's own types: the renderer never sees the emulator.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{self, NamedColor};
use bitflags::bitflags;

use crate::limits::MAX_LINK_BYTES;
use crate::listener::Listener;

/// The visible part of the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub cols: u16,
    pub rows: u16,
    /// Row by row, `cols * rows` cells.
    pub cells: Vec<CellView>,
    pub cursor: CursorView,
    /// Lines scrolled back into the history; 0 shows the newest output.
    pub display_offset: usize,
    pub selection: Option<SelectionView>,
    pub modes: TermModes,
    /// Link targets; [`CellView::link`] indexes into this list.
    pub links: Vec<String>,
}

/// One cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellView {
    /// The character and the zero-width characters that join it; empty behind a wide character.
    pub text: String,
    pub fg: TermColor,
    pub bg: TermColor,
    pub flags: CellFlags,
    pub link: Option<u32>,
}

impl Default for CellView {
    fn default() -> Self {
        Self { text: " ".into(), fg: TermColor::Default, bg: TermColor::DefaultBg, flags: CellFlags::empty(), link: None }
    }
}

/// A colour as the server asked for it; the renderer's palette decides what it looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermColor {
    Default,
    DefaultBg,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CellFlags: u16 {
        const BOLD = 1;
        const DIM = 1 << 1;
        const ITALIC = 1 << 2;
        const UNDERLINE = 1 << 3;
        const DOUBLE_UNDERLINE = 1 << 4;
        const UNDERCURL = 1 << 5;
        const DOTTED_UNDERLINE = 1 << 6;
        const DASHED_UNDERLINE = 1 << 7;
        const STRIKEOUT = 1 << 8;
        const INVERSE = 1 << 9;
        const HIDDEN = 1 << 10;
        const WIDE = 1 << 11;
        const WIDE_SPACER = 1 << 12;
        const SELECTED = 1 << 13;
    }
}

bitflags! {
    /// Modes a program switched on that change what input the terminal sends.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct TermModes: u16 {
        const APP_CURSOR = 1;
        const APP_KEYPAD = 1 << 1;
        const BRACKETED_PASTE = 1 << 2;
        const MOUSE_REPORT = 1 << 3;
        const MOUSE_DRAG = 1 << 4;
        const MOUSE_MOTION = 1 << 5;
        const SGR_MOUSE = 1 << 6;
        const FOCUS_REPORT = 1 << 7;
        const ALT_SCREEN = 1 << 8;
        const ALTERNATE_SCROLL = 1 << 9;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorView {
    pub col: u16,
    pub row: u16,
    pub shape: CursorShape,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Beam,
    Underline,
}

/// A selection in screen coordinates (column, row), clipped to the screen; `start` comes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionView {
    pub start: (u16, u16),
    pub end: (u16, u16),
    pub block: bool,
}

fn color(c: ansi::Color) -> TermColor {
    match c {
        ansi::Color::Spec(rgb) => TermColor::Rgb(rgb.r, rgb.g, rgb.b),
        ansi::Color::Indexed(i) => TermColor::Indexed(i),
        ansi::Color::Named(named) => named_color(named),
    }
}

fn named_color(named: NamedColor) -> TermColor {
    use NamedColor as N;
    let base = match named {
        N::Background => return TermColor::DefaultBg,
        N::Foreground | N::BrightForeground | N::DimForeground | N::Cursor => return TermColor::Default,
        N::DimBlack => N::Black,
        N::DimRed => N::Red,
        N::DimGreen => N::Green,
        N::DimYellow => N::Yellow,
        N::DimBlue => N::Blue,
        N::DimMagenta => N::Magenta,
        N::DimCyan => N::Cyan,
        N::DimWhite => N::White,
        other => other,
    };
    // The sixteen named colours are 0..=15.
    u8::try_from(base as usize).map_or(TermColor::Default, TermColor::Indexed)
}

fn flags(f: Flags) -> CellFlags {
    const MAP: [(Flags, CellFlags); 14] = [
        (Flags::BOLD, CellFlags::BOLD),
        (Flags::DIM, CellFlags::DIM),
        (Flags::ITALIC, CellFlags::ITALIC),
        (Flags::UNDERLINE, CellFlags::UNDERLINE),
        (Flags::DOUBLE_UNDERLINE, CellFlags::DOUBLE_UNDERLINE),
        (Flags::UNDERCURL, CellFlags::UNDERCURL),
        (Flags::DOTTED_UNDERLINE, CellFlags::DOTTED_UNDERLINE),
        (Flags::DASHED_UNDERLINE, CellFlags::DASHED_UNDERLINE),
        (Flags::STRIKEOUT, CellFlags::STRIKEOUT),
        (Flags::INVERSE, CellFlags::INVERSE),
        (Flags::HIDDEN, CellFlags::HIDDEN),
        (Flags::WIDE_CHAR, CellFlags::WIDE),
        (Flags::WIDE_CHAR_SPACER, CellFlags::WIDE_SPACER),
        (Flags::LEADING_WIDE_CHAR_SPACER, CellFlags::WIDE_SPACER),
    ];
    MAP.iter().filter(|(a, _)| f.contains(*a)).fold(CellFlags::empty(), |acc, (_, b)| acc | *b)
}

pub(crate) fn modes(m: TermMode) -> TermModes {
    const MAP: [(TermMode, TermModes); 10] = [
        (TermMode::APP_CURSOR, TermModes::APP_CURSOR),
        (TermMode::APP_KEYPAD, TermModes::APP_KEYPAD),
        (TermMode::BRACKETED_PASTE, TermModes::BRACKETED_PASTE),
        (TermMode::MOUSE_REPORT_CLICK, TermModes::MOUSE_REPORT),
        (TermMode::MOUSE_DRAG, TermModes::MOUSE_DRAG),
        (TermMode::MOUSE_MOTION, TermModes::MOUSE_MOTION),
        (TermMode::SGR_MOUSE, TermModes::SGR_MOUSE),
        (TermMode::FOCUS_IN_OUT, TermModes::FOCUS_REPORT),
        (TermMode::ALT_SCREEN, TermModes::ALT_SCREEN),
        (TermMode::ALTERNATE_SCROLL, TermModes::ALTERNATE_SCROLL),
    ];
    MAP.iter().filter(|(a, _)| m.contains(*a)).fold(TermModes::empty(), |acc, (_, b)| acc | *b)
}

fn cell_view(cell: &Cell, links: &mut Vec<String>) -> CellView {
    let f = flags(cell.flags);
    let text = if f.contains(CellFlags::WIDE_SPACER) {
        String::new()
    } else {
        let mut text = String::from(cell.c);
        if let Some(zero) = cell.zerowidth() {
            text.extend(zero.iter());
        }
        text
    };
    let link = cell.hyperlink().filter(|h| h.uri().len() <= MAX_LINK_BYTES).and_then(|h| {
        let uri = h.uri();
        let index = match links.iter().position(|l| l == uri) {
            Some(i) => i,
            None => {
                links.push(uri.to_string());
                links.len() - 1
            }
        };
        u32::try_from(index).ok()
    });
    CellView { text, fg: color(cell.fg), bg: color(cell.bg), flags: f, link }
}

pub(crate) fn screen(term: &Term<Listener>) -> Screen {
    let content = term.renderable_content();
    let cols = term.columns();
    let rows = term.screen_lines();
    let offset = i32::try_from(content.display_offset).unwrap_or(i32::MAX);
    let mut cells = vec![CellView::default(); cols.saturating_mul(rows)];
    let mut links = Vec::new();
    let range = content.selection;
    for indexed in content.display_iter {
        let Ok(row) = usize::try_from(indexed.point.line.0.saturating_add(offset)) else { continue };
        let col = indexed.point.column.0;
        if row < rows
            && col < cols
            && let Some(slot) = cells.get_mut(row * cols + col)
        {
            *slot = cell_view(indexed.cell, &mut links);
            if range.is_some_and(|r| r.contains(indexed.point)) {
                slot.flags |= CellFlags::SELECTED;
            }
        }
    }
    let selection = range.and_then(|r| {
        let last_row = i32::try_from(rows.saturating_sub(1)).unwrap_or(0);
        let start_row = r.start.line.0.saturating_add(offset);
        let end_row = r.end.line.0.saturating_add(offset);
        if end_row < 0 || start_row > last_row {
            return None;
        }
        let last_col = cols.saturating_sub(1);
        let start = if start_row < 0 { (0, 0) } else { (r.start.column.0.min(last_col), start_row) };
        let end = if end_row > last_row { (last_col, last_row) } else { (r.end.column.0.min(last_col), end_row) };
        let point = |(c, row): (usize, i32)| (u16::try_from(c).unwrap_or(0), u16::try_from(row).unwrap_or(0));
        Some(SelectionView { start: point(start), end: point(end), block: r.is_block })
    });
    let cursor_row = content.cursor.point.line.0.saturating_add(offset);
    let shape = match content.cursor.shape {
        ansi::CursorShape::Beam => Some(CursorShape::Beam),
        ansi::CursorShape::Underline => Some(CursorShape::Underline),
        ansi::CursorShape::Block | ansi::CursorShape::HollowBlock => Some(CursorShape::Block),
        ansi::CursorShape::Hidden => None,
    };
    let last_row = i32::try_from(rows.saturating_sub(1)).unwrap_or(0);
    let cursor = CursorView {
        col: u16::try_from(content.cursor.point.column.0.min(cols.saturating_sub(1))).unwrap_or(0),
        row: u16::try_from(cursor_row.clamp(0, last_row)).unwrap_or(0),
        shape: shape.unwrap_or(CursorShape::Block),
        visible: shape.is_some() && (0..=last_row).contains(&cursor_row) && content.mode.contains(TermMode::SHOW_CURSOR),
    };
    Screen {
        cols: u16::try_from(cols).unwrap_or(u16::MAX),
        rows: u16::try_from(rows).unwrap_or(u16::MAX),
        cells,
        cursor,
        display_offset: content.display_offset,
        selection,
        modes: modes(content.mode),
        links,
    }
}

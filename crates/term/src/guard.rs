//! Sits between the parser and the emulator and bounds what the emulator would store without a
//! limit: joining characters on one cell ([`MAX_ZEROWIDTH`]), link targets ([`MAX_LINK_BYTES`]),
//! and it counts every change that may add per-cell extras, so the terminal can hold them to
//! their budget. Everything else is forwarded unchanged (`guard/forward.rs`).

mod forward;

use alacritty_terminal::index::Column;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Attr, Handler, Hyperlink};
use unicode_width::UnicodeWidthChar;

use crate::limits::{MAX_LINK_BYTES, MAX_ZEROWIDTH};
use crate::listener::Listener;

pub(crate) struct Guard<'a> {
    pub(crate) term: &'a mut Term<Listener>,
    /// Changes that may have added extras since the budget was last checked.
    pub(crate) extras: &'a mut usize,
    /// Where finished lines go, when the terminal keeps them (for triggers).
    pub(crate) lines: Option<&'a mut std::collections::VecDeque<String>>,
}

impl Guard<'_> {
    /// Keeps the cursor's line before a line feed ends it, outside full-screen programs.
    pub(crate) fn line_ends(&mut self) {
        let alt = self.term.mode().contains(alacritty_terminal::term::TermMode::ALT_SCREEN);
        let Some(lines) = self.lines.as_deref_mut() else { return };
        if alt {
            return;
        }
        let grid = self.term.grid();
        let row = &grid[grid.cursor.point.line];
        let mut text = String::new();
        for col in 0..alacritty_terminal::grid::Dimensions::columns(grid) {
            let cell = &row[Column(col)];
            if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                text.push(cell.c);
            }
        }
        let text: String = text.trim_end().chars().take(crate::MAX_LINE_CHARS).collect();
        if lines.len() >= crate::MAX_COMPLETED_LINES {
            lines.pop_front();
        }
        lines.push_back(text);
    }

    /// The cell a joining character would go on, the way the emulator picks it.
    fn joins_full(&self) -> bool {
        let grid = self.term.grid();
        let cursor = &grid.cursor;
        let mut column = cursor.point.column;
        if !cursor.input_needs_wrap {
            column = Column(column.0.saturating_sub(1));
        }
        let row = &grid[cursor.point.line];
        if row[column].flags.contains(Flags::WIDE_CHAR_SPACER) {
            column = Column(column.0.saturating_sub(1));
        }
        row[column].zerowidth().is_some_and(|z| z.len() >= MAX_ZEROWIDTH)
    }

    pub(crate) fn input_char(&mut self, c: char) {
        if c.width() == Some(0) {
            if self.joins_full() {
                return;
            }
            *self.extras += 1;
        }
        Handler::input(self.term, c);
    }

    pub(crate) fn hyperlink(&mut self, link: Option<Hyperlink>) {
        let link = link.filter(|l| l.uri.len() <= MAX_LINK_BYTES && l.id.as_ref().is_none_or(|id| id.len() <= MAX_LINK_BYTES));
        if link.is_some() {
            *self.extras += 1;
        }
        Handler::set_hyperlink(self.term, link);
    }

    pub(crate) fn attribute(&mut self, attr: Attr) {
        if matches!(attr, Attr::UnderlineColor(Some(_))) {
            *self.extras += 1;
        }
        Handler::terminal_attribute(self.term, attr);
    }
}

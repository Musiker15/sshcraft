//! Scrolling, points in the grid and the selection. Every point that comes from outside (a mouse
//! position, a remembered match) is clamped to the grid before the emulator sees it: its index
//! operators panic on a line outside the grid.

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};

use crate::Terminal;

/// A point in the grid: `line` 0 is the top of the newest screen, the history is negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GridPoint {
    pub line: i32,
    pub col: u16,
}

/// How a selection grows from where it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    Simple,
    /// The word under the point (double click).
    Word,
    /// Whole lines, wrapped lines as one (triple click).
    Line,
    Block,
}

impl Terminal {
    /// `point` clamped to the grid, as the emulator's point.
    pub(crate) fn clamp(&self, point: GridPoint) -> Point {
        let grid = self.term.grid();
        let top = grid.topmost_line().0;
        let bottom = grid.bottommost_line().0;
        let last_col = grid.columns().saturating_sub(1);
        Point::new(Line(point.line.clamp(top, bottom)), Column(usize::from(point.col).min(last_col)))
    }

    pub(crate) fn grid_point(point: Point) -> GridPoint {
        GridPoint { line: point.line.0, col: u16::try_from(point.column.0).unwrap_or(u16::MAX) }
    }

    /// Scrolls `lines` towards the history (negative: towards the newest output).
    pub fn scroll(&mut self, lines: i32) {
        if self.scroll_folded(lines) {
            return;
        }
        // The emulator adds this to its offset as i32: keep it within what can be scrolled.
        let most = i32::try_from(self.term.grid().history_size().saturating_add(self.term.grid().screen_lines())).unwrap_or(i32::MAX);
        self.term.scroll_display(Scroll::Delta(lines.clamp(-most, most)));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// The grid point under screen cell `col`, `row`, taking the scroll position into account.
    pub fn point_at(&self, col: u16, row: u16) -> GridPoint {
        let size = self.size;
        let line = self.view_line(usize::from(row.min(size.rows.saturating_sub(1))));
        GridPoint { line, col: col.min(size.cols.saturating_sub(1)) }
    }

    /// The text of grid line `line` (clamped), without trailing blanks.
    pub fn line_text(&self, line: i32) -> String {
        let point = self.clamp(GridPoint { line, col: 0 });
        let row = &self.term.grid()[point.line];
        let mut text = String::new();
        for col in 0..self.term.grid().columns() {
            let cell = &row[Column(col)];
            if cell
                .flags
                .intersects(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER | alacritty_terminal::term::cell::Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            text.push(cell.c);
            if let Some(zero) = cell.zerowidth() {
                text.extend(zero.iter());
            }
        }
        text.trim_end().to_string()
    }

    pub fn select_start(&mut self, at: GridPoint, kind: SelectionKind) {
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
            SelectionKind::Block => SelectionType::Block,
        };
        let point = self.clamp(at);
        self.term.selection = Some(Selection::new(ty, point, Side::Left));
    }

    pub fn select_update(&mut self, to: GridPoint) {
        let point = self.clamp(to);
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, Side::Right);
        }
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    /// The selected text; `None` without a selection.
    pub fn selection_text(&self) -> Option<String> {
        // A line selection ends with a line break; pasted into a shell it would run at once.
        self.term.selection_to_string().map(|text| text.strip_suffix('\n').map(str::to_string).unwrap_or(text)).filter(|text| !text.is_empty())
    }
}

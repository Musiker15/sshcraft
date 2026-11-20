//! Folding the output of a command (shell integration): the lines of a folded output show as one
//! row, so the view is no longer a plain window on the grid. The view is built from the line at
//! its bottom upwards, a fold counting as one row; clicks, the selection, the cursor, scrolling,
//! jumps and search go through it. A fold refers to its command's id, so it moves with the output
//! and goes when the command leaves the history. A full-screen program is never folded.

use std::collections::HashSet;

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::TermMode;

use crate::Terminal;
use crate::commands::CommandRow;

/// A folded output: grid lines `first..=last`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FoldRange {
    pub(crate) id: u64,
    pub(crate) first: i32,
    pub(crate) last: i32,
}

/// What one row of the view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewRow {
    Line(i32),
    Folded(FoldRange),
}

impl ViewRow {
    /// The topmost grid line of the row.
    pub(crate) fn low(self) -> i32 {
        match self {
            Self::Line(line) => line,
            Self::Folded(range) => range.first,
        }
    }

    /// The bottommost grid line of the row.
    pub(crate) fn high(self) -> i32 {
        match self {
            Self::Line(line) => line,
            Self::Folded(range) => range.last,
        }
    }
}

/// A folded output on the screen: the row standing for it, its command and how many lines it
/// hides. The row has no text; the interface draws what it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldRow {
    pub row: usize,
    pub id: u64,
    pub lines: usize,
}

/// The row of `view` that shows grid line `line` itself, not inside a fold.
pub(crate) fn row_of_line(view: &[ViewRow], line: i32) -> Option<usize> {
    let row = view.partition_point(|r| r.high() < line);
    matches!(view.get(row), Some(ViewRow::Line(l)) if *l == line).then_some(row)
}

/// The fold among `ranges` (top to bottom) that hides `line`.
fn containing(ranges: &[FoldRange], line: i32) -> Option<FoldRange> {
    let index = ranges.partition_point(|r| r.first <= line).checked_sub(1)?;
    ranges.get(index).copied().filter(|r| r.last >= line)
}

/// What the cached view is for: the version of the terminal, the display offset, the rows.
pub(crate) type ViewKey = (u64, usize, usize);

/// A view and the key it is for.
type CachedView = (ViewKey, Vec<ViewRow>, Vec<CommandRow>);

/// The folds of a terminal and what was worked out from them.
#[derive(Default)]
pub(crate) struct Folds {
    /// The ids of the folded commands; never more than the records kept.
    pub(crate) ids: HashSet<u64>,
    /// The folded outputs for a version of the terminal.
    ranges: std::cell::Cell<Option<(u64, Vec<FoldRange>)>>,
    /// The rows and command rows of the last view: the interface asks for the screen several
    /// times a frame.
    view: std::cell::Cell<Option<CachedView>>,
}

impl Terminal {
    /// Whether anything is folded where folds apply.
    fn folding(&self) -> bool {
        !self.folds.ids.is_empty() && !self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    /// The folded outputs, top to bottom.
    fn fold_ranges(&self) -> Vec<FoldRange> {
        if !self.folding() {
            return Vec::new();
        }
        if let Some((version, ranges)) = self.folds.ranges.take()
            && version == self.version
        {
            self.folds.ranges.set(Some((version, ranges.clone())));
            return ranges;
        }
        let mut ranges: Vec<FoldRange> = self
            .fold_ranges_all()
            .into_iter()
            .filter(|(id, _)| self.folds.ids.contains(id))
            .map(|(id, (first, last))| FoldRange { id, first, last })
            .collect();
        // Newest first is bottom first.
        ranges.reverse();
        self.folds.ranges.set(Some((self.version, ranges.clone())));
        ranges
    }

    /// The grid line at the bottom of the view.
    fn bottom_line(&self) -> i32 {
        let grid = self.term.grid();
        let offset = i32::try_from(grid.display_offset()).unwrap_or(i32::MAX);
        grid.bottommost_line().0.saturating_sub(offset)
    }

    /// The rows of a view whose bottom row shows `bottom`, at most `rows`, top to bottom. When
    /// the history ends first, what fits below `bottom` follows.
    fn view_from(&self, bottom: i32, rows: usize, ranges: &[FoldRange]) -> Vec<ViewRow> {
        let grid = self.term.grid();
        let (top, last) = (grid.topmost_line().0, grid.bottommost_line().0);
        let mut out = Vec::with_capacity(rows);
        let mut line = bottom;
        while out.len() < rows && line >= top {
            match containing(ranges, line) {
                Some(range) => {
                    out.push(ViewRow::Folded(range));
                    line = range.first.saturating_sub(1);
                }
                None => {
                    out.push(ViewRow::Line(line));
                    line = line.saturating_sub(1);
                }
            }
        }
        out.reverse();
        let mut line = bottom.saturating_add(1);
        while out.len() < rows && line <= last {
            match containing(ranges, line) {
                Some(range) => {
                    out.push(ViewRow::Folded(range));
                    line = range.last.saturating_add(1);
                }
                None => {
                    out.push(ViewRow::Line(line));
                    line = line.saturating_add(1);
                }
            }
        }
        out
    }

    /// Whether `rows` rows fit above `bottom`, `bottom` included.
    fn fills_up(&self, bottom: i32, rows: usize, ranges: &[FoldRange]) -> bool {
        let top = self.term.grid().topmost_line().0;
        let mut line = bottom;
        for _ in 0..rows {
            if line < top {
                return false;
            }
            line = containing(ranges, line).map_or(line, |r| r.first).saturating_sub(1);
        }
        true
    }

    /// The rows of the view and its prompt lines.
    pub(crate) fn view(&self) -> (Vec<ViewRow>, Vec<CommandRow>) {
        let grid = self.term.grid();
        let rows = grid.screen_lines();
        let key = (self.version, grid.display_offset(), rows);
        if let Some((cached, view, commands)) = self.folds.view.take()
            && cached == key
        {
            self.folds.view.set(Some((cached, view.clone(), commands.clone())));
            return (view, commands);
        }
        let view = self.view_from(self.bottom_line(), rows, &self.fold_ranges());
        let commands = self.command_rows(&view);
        self.folds.view.set(Some((key, view.clone(), commands.clone())));
        (view, commands)
    }

    /// The grid line shown by row `row` of the view; a fold stands for its first line.
    pub(crate) fn view_line(&self, row: usize) -> i32 {
        let (view, _) = self.view();
        match view.get(row).or(view.last()) {
            Some(r) => r.low(),
            None => self.term.grid().bottommost_line().0,
        }
    }

    /// Scrolls `rows` rows towards the history (negative: towards the newest output), a fold
    /// counting as one row. Returns false when nothing is folded, for the plain way.
    pub(crate) fn scroll_folded(&mut self, rows: i32) -> bool {
        let ranges = self.fold_ranges();
        if ranges.is_empty() {
            return false;
        }
        let screen = self.term.grid().screen_lines();
        let last = self.term.grid().bottommost_line().0;
        let start = self.bottom_line();
        let mut bottom = start;
        if rows > 0 {
            for _ in 0..rows {
                let up = containing(&ranges, bottom).map_or(bottom, |r| r.first).saturating_sub(1);
                if !self.fills_up(up, screen, &ranges) {
                    break;
                }
                bottom = up;
            }
        } else {
            for _ in 0..rows.unsigned_abs() {
                if bottom >= last {
                    break;
                }
                let next = bottom.saturating_add(1);
                bottom = containing(&ranges, next).map_or(next, |r| r.last);
            }
        }
        self.term.scroll_display(Scroll::Delta(start.saturating_sub(bottom)));
        true
    }

    /// Scrolls so that grid line `line` is the top row; a fold hiding it opens first.
    pub(crate) fn scroll_line_to_top(&mut self, line: i32) {
        self.unfold_line(line);
        let ranges = self.fold_ranges();
        let grid = self.term.grid();
        let (rows, last) = (grid.screen_lines(), grid.bottommost_line().0);
        let mut bottom = line;
        for _ in 1..rows {
            if bottom >= last {
                break;
            }
            let next = bottom.saturating_add(1);
            bottom = containing(&ranges, next).map_or(next, |r| r.last);
        }
        let wanted = last.saturating_sub(bottom).max(0);
        let offset = i32::try_from(grid.display_offset()).unwrap_or(i32::MAX);
        self.term.scroll_display(Scroll::Delta(wanted.saturating_sub(offset)));
    }

    /// Opens the fold that hides grid line `line`, if one does.
    pub(crate) fn unfold_line(&mut self, line: i32) {
        if let Some(range) = containing(&self.fold_ranges(), line) {
            self.folds.ids.remove(&range.id);
            self.changed();
        }
    }

    /// Folds the output of command `id` (see [`crate::CommandRecord::id`]), or shows it again
    /// when it is folded. Returns whether it is folded now: a command that runs, printed nothing
    /// or left the history does not fold.
    pub fn toggle_fold(&mut self, id: u64) -> bool {
        if self.folds.ids.remove(&id) {
            self.changed();
            return false;
        }
        if self.term.mode().contains(TermMode::ALT_SCREEN) || !self.fold_ranges_all().iter().any(|(i, _)| *i == id) {
            return false;
        }
        // Ids count up, so the folds of commands that left the records are the smaller ones.
        let oldest = self.commands.records.front().map_or(0, |r| r.id());
        self.folds.ids.retain(|f| *f >= oldest);
        self.folds.ids.insert(id);
        self.changed();
        true
    }

    /// Folds the output of the newest command that has one, or shows it again. Returns whether
    /// it is folded now.
    pub fn fold_last(&mut self) -> bool {
        match self.fold_ranges_all().first() {
            Some((id, _)) => self.toggle_fold(*id),
            None => false,
        }
    }

    /// Folds the output of every command that has one.
    pub fn fold_all(&mut self) {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        self.folds.ids = self.fold_ranges_all().into_iter().map(|(id, _)| id).collect();
        self.changed();
    }

    /// Shows every folded output again.
    pub fn unfold_all(&mut self) {
        self.folds.ids.clear();
        self.changed();
    }
}

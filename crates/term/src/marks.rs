//! Marks on lines, to jump back to them. A mark is a bit on the cells of its line, so it moves
//! with the line as output scrolls and goes with it when the line leaves the history or is
//! erased: the emulator resets a cell completely when it reuses or clears it.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;

use crate::selection::GridPoint;
use crate::{Direction, Terminal};

/// The flag bit `alacritty_terminal` 0.26 leaves unused; it survives scrolling and reflow and is
/// cleared with the cell.
pub(crate) const MARK: Flags = Flags::from_bits_retain(0x8000);

impl Terminal {
    fn set_mark(&mut self, line: Line, on: bool) {
        let columns = self.term.grid().columns();
        let row = &mut self.term.grid_mut()[line];
        for col in 0..columns {
            row[Column(col)].flags.set(MARK, on);
        }
    }

    /// Marks the line of the cursor.
    pub fn add_mark(&mut self) {
        self.changed();
        let line = self.term.grid().cursor.point.line;
        self.set_mark(line, true);
    }

    pub fn remove_mark(&mut self, at: GridPoint) {
        self.changed();
        let line = self.clamp(at).line;
        self.set_mark(line, false);
    }

    /// The marked lines, oldest first, each at column 0.
    pub fn marks(&self) -> Vec<GridPoint> {
        let grid = self.term.grid();
        let columns = grid.columns();
        (grid.topmost_line().0..=grid.bottommost_line().0)
            .filter(|line| {
                let row = &grid[Line(*line)];
                (0..columns).any(|col| row[Column(col)].flags.contains(MARK))
            })
            .map(|line| GridPoint { line, col: 0 })
            .collect()
    }

    /// Scrolls the next mark above (`Up`) or below (`Down`) the top of the view to the top.
    /// Returns whether there was one.
    pub fn jump_to_mark(&mut self, direction: Direction) -> bool {
        // The top row of the view, which folds below it can move above the grid's.
        let top = self.view_line(0);
        let marks = self.marks();
        let target = match direction {
            Direction::Up => marks.iter().rev().find(|m| m.line < top),
            Direction::Down => marks.iter().find(|m| m.line > top),
        };
        let Some(target) = target else { return false };
        self.scroll_line_to_top(target.line);
        true
    }
}

//! What cells carry besides their character: joining characters, a link, an underline colour.
//! The emulator allocates them per cell without a bound, so the terminal counts them and, past
//! [`EXTRAS_BUDGET`], takes them off the oldest lines first: those lines keep their text and
//! lose links and decorations.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, CellExtra};

use crate::Terminal;
use crate::limits::EXTRAS_BUDGET;

/// Changes that may add extras between two checks of the budget.
pub(crate) const CHECK_EVERY: usize = 4096;

/// The bytes of one cell's extras.
fn cost(cell: &Cell) -> usize {
    if cell.extra.is_none() {
        return 0;
    }
    let zero = cell.zerowidth().map_or(0, std::mem::size_of_val);
    let link = cell.hyperlink().map_or(0, |h| h.uri().len() + h.id().len());
    std::mem::size_of::<CellExtra>() + zero + link
}

impl Terminal {
    /// The bytes the cells' extras take, counting extras shared by neighbouring cells once.
    pub fn extras_bytes(&self) -> usize {
        let grid = self.term.grid();
        let columns = grid.columns();
        let mut total = 0usize;
        for line in grid.topmost_line().0..=grid.bottommost_line().0 {
            let row = &grid[Line(line)];
            let mut previous = std::ptr::null();
            for col in 0..columns {
                let cell = &row[Column(col)];
                if let Some(extra) = &cell.extra {
                    let ptr = std::sync::Arc::as_ptr(extra);
                    if ptr != previous {
                        total = total.saturating_add(cost(cell));
                    }
                    previous = ptr;
                }
            }
        }
        total
    }

    /// Takes extras off the oldest lines until the rest fits the budget.
    pub(crate) fn enforce_extras_budget(&mut self) {
        let mut over = self.extras_bytes().saturating_sub(EXTRAS_BUDGET);
        if over == 0 {
            return;
        }
        // Clear a quarter of the budget more than needed, so the next check is not at once.
        over = over.saturating_add(EXTRAS_BUDGET / 4);
        let grid = self.term.grid_mut();
        let columns = grid.columns();
        let (top, bottom) = (grid.topmost_line().0, grid.bottommost_line().0);
        for line in top..=bottom {
            let row = &mut grid[Line(line)];
            for col in 0..columns {
                let cell = &mut row[Column(col)];
                if cell.extra.is_some() {
                    over = over.saturating_sub(cost(cell));
                    cell.extra = None;
                }
            }
            if over == 0 {
                return;
            }
        }
    }
}

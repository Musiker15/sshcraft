//! Integration tests of the terminal: what a server sends and what the terminal shows.

mod commands;
mod fold;
mod fuzz;
mod history;
mod hostile;
mod input;
mod lines;
mod notify;
mod rules;
mod screen;
mod search;

use sshcraft_term::{Screen, TermConfig, TermSize, Terminal};

/// A terminal of `cols` by `rows` with the default configuration.
pub fn term(cols: u16, rows: u16) -> Terminal {
    Terminal::new(TermSize::clamped(cols, rows), &TermConfig::default())
}

/// The text of visible row `row`, without trailing blanks.
pub fn row_text(screen: &Screen, row: u16) -> String {
    let cols = usize::from(screen.cols);
    let start = usize::from(row) * cols;
    let text: String = screen.cells[start..start + cols].iter().map(|c| if c.text.is_empty() { "" } else { c.text.as_str() }).collect();
    text.trim_end().to_string()
}

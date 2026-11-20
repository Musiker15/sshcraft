//! Searching the screen and the history, as plain text or as a regular expression. The regex
//! engine of `alacritty_terminal` (`regex-automata`, lazy DFA) never backtracks, so no pattern
//! makes a search run away; the pattern length is bounded all the same.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Boundary, Column, Direction as Way, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::search::RegexSearch;

use crate::limits::MAX_SEARCH_PATTERN;
use crate::selection::GridPoint;
use crate::{Direction, Terminal};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub pattern: String,
    /// The pattern is a regular expression; otherwise every character means itself.
    pub regex: bool,
    pub case_sensitive: bool,
}

/// A match, inclusive at both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    pub start: GridPoint,
    pub end: GridPoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    TooLong,
    Invalid(String),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong => write!(f, "the search is longer than {MAX_SEARCH_PATTERN} characters"),
            Self::Invalid(reason) => write!(f, "not a valid regular expression: {reason}"),
        }
    }
}

impl std::error::Error for SearchError {}

/// `text` with every character that means something in a regular expression escaped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl Terminal {
    /// Finds the next match of `query` in `direction`, after the previous match of the same
    /// query, wrapping around. The match is selected and scrolled into view.
    pub fn search(&mut self, query: &SearchQuery, direction: Direction) -> Result<Option<SearchMatch>, SearchError> {
        if query.pattern.chars().count() > MAX_SEARCH_PATTERN {
            return Err(SearchError::TooLong);
        }
        if query.pattern.is_empty() {
            return Ok(None);
        }
        let body = if query.regex { query.pattern.clone() } else { escape(&query.pattern) };
        let flags = if query.case_sensitive { "(?-i)" } else { "(?i)" };
        let mut regex = RegexSearch::new(&format!("{flags}{body}")).map_err(|e| SearchError::Invalid(e.to_string()))?;

        let previous = self.last_match.filter(|(q, _)| *q == query_key(query)).map(|(_, m)| m);
        let grid = self.term.grid();
        let (origin, way, side) = match (direction, previous) {
            (Direction::Down, Some(m)) => (self.clamp(m.end).add(grid, Boundary::None, 1), Way::Right, Side::Left),
            (Direction::Down, None) => (Point::new(grid.topmost_line(), Column(0)), Way::Right, Side::Left),
            (Direction::Up, Some(m)) => (self.clamp(m.start).sub(grid, Boundary::None, 1), Way::Left, Side::Right),
            (Direction::Up, None) => (Point::new(grid.bottommost_line(), grid.last_column()), Way::Left, Side::Right),
        };
        let Some(found) = self.term.search_next(&mut regex, origin, way, side, None) else {
            self.last_match = None;
            return Ok(None);
        };
        let (start, end) = (*found.start(), *found.end());
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        self.term.selection = Some(selection);
        // A match in folded output shows it again.
        self.unfold_line(start.line.0);
        self.term.scroll_to_point(start);
        let m = SearchMatch { start: Self::grid_point(start), end: Self::grid_point(end) };
        self.last_match = Some((query_key(query), m));
        Ok(Some(m))
    }
}

/// What identifies a query for "the next match": its pattern and options.
pub(crate) fn query_key(query: &SearchQuery) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    query.pattern.hash(&mut hasher);
    query.regex.hash(&mut hasher);
    query.case_sensitive.hash(&mut hasher);
    hasher.finish()
}

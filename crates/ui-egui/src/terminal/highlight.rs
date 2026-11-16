//! Keyword highlights and output triggers (settings `terminal.highlights` and
//! `terminal.triggers`). Patterns are compiled once per change of the settings, within the
//! engine's limits; highlights touch only the visible rows of a frame, triggers only lines that
//! ended.

use egui::Color32;
use regex::Regex;
use sshcraft_engine::settings::{Highlight, Trigger, TriggerAction, compile_pattern};
use sshcraft_term::{Screen, TermColor};

/// The compiled highlights.
#[derive(Debug, Default, Clone)]
pub struct Highlighter {
    patterns: Vec<(Regex, [u8; 3])>,
}

impl Highlighter {
    pub fn new(highlights: &[Highlight]) -> Self {
        Self { patterns: highlights.iter().filter_map(|h| compile_pattern(&h.pattern).map(|r| (r, h.color))).collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Colours the cells of `screen` that a pattern matches.
    pub fn apply(&self, screen: &mut Screen) {
        if self.patterns.is_empty() {
            return;
        }
        let cols = usize::from(screen.cols).max(1);
        for row in screen.cells.chunks_mut(cols) {
            // The row's text, and for each byte the cell it belongs to.
            let mut text = String::new();
            let mut cell_of = Vec::new();
            for (col, cell) in row.iter().enumerate() {
                let piece = cell.text.as_str();
                text.push_str(piece);
                cell_of.extend(std::iter::repeat_n(col, piece.len()));
            }
            for (regex, [r, g, b]) in &self.patterns {
                for found in regex.find_iter(&text) {
                    if found.is_empty() {
                        continue;
                    }
                    let first = cell_of.get(found.start()).copied();
                    let last = cell_of.get(found.end().saturating_sub(1)).copied();
                    if let (Some(first), Some(last)) = (first, last) {
                        for cell in row.iter_mut().take(last + 1).skip(first) {
                            cell.fg = TermColor::Rgb(*r, *g, *b);
                        }
                    }
                }
            }
        }
    }
}

/// The compiled triggers.
#[derive(Debug, Default, Clone)]
pub struct Triggers {
    patterns: Vec<(Regex, TriggerAction)>,
}

impl Triggers {
    pub fn new(triggers: &[Trigger]) -> Self {
        Self { patterns: triggers.iter().filter_map(|t| compile_pattern(&t.pattern).map(|r| (r, t.action))).collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// The actions `line` sets off, one per matching trigger.
    pub fn actions(&self, line: &str) -> Vec<TriggerAction> {
        self.patterns.iter().filter(|(regex, _)| regex.is_match(line)).map(|(_, action)| *action).collect()
    }
}

/// Highlights and triggers with the settings they were made from.
#[derive(Debug, Default)]
pub struct Patterns {
    from: Option<(Vec<Highlight>, Vec<Trigger>)>,
    pub highlighter: Highlighter,
    pub triggers: Triggers,
}

impl Patterns {
    /// Compiles again when the settings changed.
    pub fn refresh(&mut self, highlights: &[Highlight], triggers: &[Trigger]) {
        if self.from.as_ref().is_some_and(|(h, t)| h.as_slice() == highlights && t.as_slice() == triggers) {
            return;
        }
        self.highlighter = Highlighter::new(highlights);
        self.triggers = Triggers::new(triggers);
        self.from = Some((highlights.to_vec(), triggers.to_vec()));
    }
}

/// The colour of a highlight in the settings page.
pub fn swatch([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

//! Hint mode: every URL, path, IP address and hash on the screen gets a short label; typing a
//! label copies the text (in capitals: types it into the terminal). Nothing found is ever opened
//! or run.

use std::ops::Range;
use std::sync::LazyLock;

use egui::{Color32, FontId, Painter, Rect, pos2, vec2};
use regex::Regex;
use sshcraft_term::Screen;

use super::CellMetrics;

/// Hints shown at most; more would not fit as labels anyway.
pub const MAX_HINTS: usize = 200;

/// What a hint is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintKind {
    Url,
    Path,
    Ip,
    Hash,
}

/// A text on the screen that can be picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub row: usize,
    pub cols: Range<usize>,
    pub text: String,
    pub kind: HintKind,
}

fn compiled(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

static URL: LazyLock<Option<Regex>> = LazyLock::new(|| compiled(r#"\b(?:https?|ftp)://[^\s<>"'`]+"#));
static PATH: LazyLock<Option<Regex>> = LazyLock::new(|| compiled(r"(?:~|\.\.?)?/[A-Za-z0-9._\-/]*[A-Za-z0-9_\-]"));
static IPV4: LazyLock<Option<Regex>> = LazyLock::new(|| compiled(r"\b(?:\d{1,3}\.){3}\d{1,3}\b"));
static IPV6: LazyLock<Option<Regex>> =
    LazyLock::new(|| compiled(r"\b(?:[0-9A-Fa-f]{1,4}:){1,7}(?::?[0-9A-Fa-f]{1,4})+\b|\b(?:[0-9A-Fa-f]{1,4}:){1,7}:"));
static HASH: LazyLock<Option<Regex>> = LazyLock::new(|| compiled(r"\b[0-9a-f]{7,64}\b"));

/// The labels for `count` hints: `a` to `z`, then two letters.
pub fn labels(count: usize) -> Vec<String> {
    let letters: Vec<char> = ('a'..='z').collect();
    let mut out: Vec<String> = Vec::with_capacity(count);
    if count <= letters.len() {
        out.extend(letters.iter().take(count).map(|c| c.to_string()));
        return out;
    }
    // With more than 26, every label has two letters, so none is the start of another.
    'outer: for a in &letters {
        for b in &letters {
            if out.len() == count {
                break 'outer;
            }
            out.push(format!("{a}{b}"));
        }
    }
    out
}

/// The hints of the visible rows, top to bottom, left to right, at most [`MAX_HINTS`].
pub fn find_hints(screen: &Screen) -> Vec<Hint> {
    let cols = usize::from(screen.cols).max(1);
    let mut hints = Vec::new();
    for (row, cells) in screen.cells.chunks(cols).enumerate() {
        let mut text = String::new();
        let mut cell_of = Vec::new();
        for (col, cell) in cells.iter().enumerate() {
            text.push_str(&cell.text);
            cell_of.extend(std::iter::repeat_n(col, cell.text.len()));
        }
        let mut taken: Vec<Range<usize>> = Vec::new();
        let mut add = |range: Range<usize>, kind: HintKind, hints: &mut Vec<Hint>| {
            if taken.iter().any(|t| t.start < range.end && range.start < t.end) {
                return;
            }
            let (Some(first), Some(last)) = (cell_of.get(range.start), cell_of.get(range.end.saturating_sub(1))) else { return };
            if let Some(found) = text.get(range.clone()) {
                hints.push(Hint { row, cols: *first..last + 1, text: found.to_string(), kind });
                taken.push(range);
            }
        };
        for (pattern, kind) in [(&URL, HintKind::Url), (&IPV4, HintKind::Ip), (&IPV6, HintKind::Ip), (&PATH, HintKind::Path), (&HASH, HintKind::Hash)]
        {
            let Some(regex) = pattern.as_ref() else { continue };
            for m in regex.find_iter(&text) {
                let mut end = m.end();
                if kind == HintKind::Url {
                    // Punctuation after a URL belongs to the sentence.
                    let trimmed = m.as_str().trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '!', '?']);
                    end = m.start() + trimmed.len();
                }
                if kind == HintKind::Hash {
                    let s = m.as_str();
                    // A hash has letters and digits; a plain number or word is none.
                    if !(s.bytes().any(|b| b.is_ascii_digit()) && s.bytes().any(|b| b.is_ascii_lowercase())) {
                        continue;
                    }
                }
                if kind == HintKind::Path && m.as_str().len() < 2 {
                    continue;
                }
                add(m.start()..end, kind, &mut hints);
            }
        }
    }
    hints.sort_by_key(|h| (h.row, h.cols.start));
    hints.truncate(MAX_HINTS);
    hints
}

/// The labels over the hints, on the terminal at `rect`.
pub fn paint_labels(painter: &Painter, rect: Rect, metrics: &CellMetrics, hints: &[(String, Hint)], typed: &str) {
    for (label, hint) in hints {
        if !label.starts_with(typed) {
            continue;
        }
        #[allow(clippy::cast_precision_loss)] // Rows and columns are far below 2^24.
        let at = pos2(rect.min.x + hint.cols.start as f32 * metrics.width, rect.min.y + hint.row as f32 * metrics.height);
        let galley = painter.layout_no_wrap(label.clone(), FontId::monospace(metrics.height * 0.75), Color32::BLACK);
        let area = Rect::from_min_size(at, galley.size() + vec2(4.0, 0.0));
        painter.rect_filled(area, 2.0, Color32::from_rgb(0xf5, 0xc5, 0x42));
        painter.galley(at + vec2(2.0, 0.0), galley, Color32::BLACK);
    }
}

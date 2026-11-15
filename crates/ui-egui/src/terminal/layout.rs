//! A screen as primitives: rectangles, text and lines, positioned on the cell grid.
//!
//! Text is drawn in runs: neighbouring narrow cells of one row with the same look become one
//! text, so a font with ligatures can join them (each narrow glyph of a monospaced font is one
//! cell wide). A wide character and a cell with joined characters (accents, emoji sequences) are
//! a text of their own, centred on their cells, because their glyphs are not exactly one or two
//! cells wide and would push the rest of a run off the grid.

use egui::{Color32, FontId, Pos2, Rect, pos2, vec2};
use sshcraft_term::{CellFlags, CellView, CursorShape, Screen};

use crate::terminal::TermPalette;

/// The size of one cell in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
    /// Distance from the top of a cell to the text's baseline.
    pub baseline: f32,
}

impl CellMetrics {
    /// The metrics of `font`: the advance of `M` and the row height.
    pub fn measure(ctx: &egui::Context, font: &FontId) -> Self {
        ctx.fonts_mut(|fonts| {
            let width = fonts.glyph_width(font, 'M').max(1.0);
            let height = fonts.row_height(font).max(1.0);
            Self { width, height, baseline: (height * 0.8).round() }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    Solid,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    Rect {
        rect: Rect,
        color: Color32,
    },
    /// Text whose cells start at `pos` and are `width` wide; it is centred in that width.
    Text {
        pos: Pos2,
        width: f32,
        text: String,
        color: Color32,
        font: FontId,
        italic: bool,
        bold: bool,
    },
    Line {
        from: Pos2,
        to: Pos2,
        color: Color32,
        style: LineStyle,
    },
}

/// How one cell is drawn, after inverse, selection, cursor and links are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Look {
    fg: Color32,
    bg: Color32,
    italic: bool,
    bold: bool,
    hidden: bool,
    underline: Option<LineStyle>,
    strike: bool,
}

fn underline(flags: CellFlags) -> Option<LineStyle> {
    if flags.contains(CellFlags::DOUBLE_UNDERLINE) {
        Some(LineStyle::Double)
    } else if flags.contains(CellFlags::UNDERCURL) {
        Some(LineStyle::Curly)
    } else if flags.contains(CellFlags::DOTTED_UNDERLINE) {
        Some(LineStyle::Dotted)
    } else if flags.contains(CellFlags::DASHED_UNDERLINE) {
        Some(LineStyle::Dashed)
    } else if flags.contains(CellFlags::UNDERLINE) {
        Some(LineStyle::Solid)
    } else {
        None
    }
}

fn look(cell: &CellView, palette: &TermPalette, under_block_cursor: bool) -> Look {
    let f = cell.flags;
    let mut fg = palette.resolve(cell.fg, f);
    let mut bg = palette.resolve(cell.bg, f - CellFlags::DIM);
    if cell.link.is_some() && cell.fg == sshcraft_term::TermColor::Default {
        fg = palette.link;
    }
    if f.contains(CellFlags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if f.contains(CellFlags::SELECTED) {
        fg = palette.selection_fg;
        bg = palette.selection_bg;
    }
    if under_block_cursor {
        fg = palette.bg;
        bg = palette.cursor;
    }
    let underline = underline(f).or(cell.link.map(|_| LineStyle::Solid));
    Look {
        fg,
        bg,
        italic: f.contains(CellFlags::ITALIC),
        bold: f.contains(CellFlags::BOLD),
        hidden: f.contains(CellFlags::HIDDEN),
        underline,
        strike: f.contains(CellFlags::STRIKEOUT),
    }
}

/// The emoji modifiers for skin tone, U+1F3FB to U+1F3FF.
const SKIN_TONES: [char; 5] = ['\u{1f3fb}', '\u{1f3fc}', '\u{1f3fd}', '\u{1f3fe}', '\u{1f3ff}'];

/// A cell that is drawn on its own: wide, or with joined characters.
fn stands_alone(cell: &CellView) -> bool {
    cell.flags.contains(CellFlags::WIDE) || cell.text.chars().nth(1).is_some()
}

/// `ligatures` false draws every narrow cell on its own, so no font joins neighbouring characters.
pub fn layout(
    screen: &Screen,
    metrics: &CellMetrics,
    palette: &TermPalette,
    font: &FontId,
    origin: Pos2,
    focused: bool,
    ligatures: bool,
) -> Vec<Primitive> {
    let cols = usize::from(screen.cols);
    let cursor = screen.cursor;
    let block_cursor = focused && cursor.visible && cursor.shape == CursorShape::Block;
    let cell_rect = |col: usize, row: usize, span: usize| {
        Rect::from_min_size(
            pos2(origin.x + col as f32 * metrics.width, origin.y + row as f32 * metrics.height),
            vec2(span as f32 * metrics.width, metrics.height),
        )
    };

    let (mut backgrounds, mut texts, mut lines) = (Vec::new(), Vec::new(), Vec::new());
    for (row, cells) in screen.cells.chunks(cols.max(1)).enumerate() {
        let looks: Vec<Look> = cells
            .iter()
            .enumerate()
            .map(|(col, cell)| look(cell, palette, block_cursor && usize::from(cursor.row) == row && usize::from(cursor.col) == col))
            .collect();

        // Backgrounds: runs of one colour that is not the terminal's own.
        let mut col = 0;
        while col < cols {
            let bg = looks[col].bg;
            let start = col;
            while col < cols && looks[col].bg == bg {
                col += 1;
            }
            if bg != palette.bg {
                backgrounds.push(Primitive::Rect { rect: cell_rect(start, row, col - start), color: bg });
            }
        }

        // Text: runs of narrow cells that look the same; wide cells and clusters alone.
        let mut col = 0;
        while col < cols {
            let cell = &cells[col];
            if cell.flags.contains(CellFlags::WIDE_SPACER) {
                col += 1;
                continue;
            }
            let start = col;
            let mut text = cell.text.clone();
            let span = if cell.flags.contains(CellFlags::WIDE) { 2 } else { 1 };
            col += span;
            // The emulator keeps a zero width joiner with the character before it and puts the
            // next character of an emoji sequence (👩‍💻), or a skin tone (👍🏽), in cells of its
            // own: join them again.
            let first = text.clone();
            while col < cols && (text.ends_with('\u{200d}') || cells[col].text.starts_with(SKIN_TONES)) {
                let next = &cells[col];
                text.push_str(&next.text);
                col += if next.flags.contains(CellFlags::WIDE) { 2 } else { 1 };
            }
            col = col.min(cols);
            let mut cells_drawn = col - start;
            if ligatures && !stands_alone(cell) && text == first {
                while col < cols && !stands_alone(&cells[col]) && !cells[col].flags.contains(CellFlags::WIDE_SPACER) && looks[col] == looks[start] {
                    text.push_str(&cells[col].text);
                    col += 1;
                }
                // Blanks at the end of a run of narrow cells are one cell each: leave them out.
                let kept = text.trim_end_matches(' ').len();
                cells_drawn = (col - start).saturating_sub(text.len() - kept);
                text.truncate(kept);
            }
            let l = looks[start];
            let width = cells_drawn as f32 * metrics.width;
            if !l.hidden && !text.trim().is_empty() {
                let rect = cell_rect(start, row, col - start);
                texts.push(Primitive::Text { pos: rect.min, width, text, color: l.fg, font: font.clone(), italic: l.italic, bold: l.bold });
            }
        }

        // Lines: underline and strikeout, merged over cells that look the same.
        let mut col = 0;
        while col < cols {
            let l = looks[col];
            let start = col;
            while col < cols && looks[col].underline == l.underline && looks[col].strike == l.strike && looks[col].fg == l.fg {
                col += 1;
            }
            let rect = cell_rect(start, row, col - start);
            if let Some(style) = l.underline {
                let y = (rect.min.y + metrics.baseline + 2.0).min(rect.max.y - 1.0);
                lines.push(Primitive::Line { from: pos2(rect.min.x, y), to: pos2(rect.max.x, y), color: l.fg, style });
            }
            if l.strike {
                let y = rect.min.y + metrics.height * 0.55;
                lines.push(Primitive::Line { from: pos2(rect.min.x, y), to: pos2(rect.max.x, y), color: l.fg, style: LineStyle::Solid });
            }
        }
    }

    let mut out = backgrounds;
    out.append(&mut texts);
    out.append(&mut lines);
    if cursor.visible {
        let span = screen
            .cells
            .get(usize::from(cursor.row) * cols + usize::from(cursor.col))
            .map_or(1, |c| if c.flags.contains(CellFlags::WIDE) { 2 } else { 1 });
        let rect = cell_rect(usize::from(cursor.col), usize::from(cursor.row), span);
        let color = palette.cursor;
        if !focused {
            let [a, b, c, d] = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
            for (from, to) in [(a, b), (b, c), (c, d), (d, a)] {
                out.push(Primitive::Line { from, to, color, style: LineStyle::Solid });
            }
        } else {
            match cursor.shape {
                // The block is the cursor cell's background, drawn with the backgrounds.
                CursorShape::Block => {}
                CursorShape::Beam => {
                    out.push(Primitive::Rect { rect: Rect::from_min_size(rect.min, vec2((metrics.width * 0.12).max(1.0), rect.height())), color })
                }
                CursorShape::Underline => {
                    out.push(Primitive::Rect { rect: Rect::from_min_max(pos2(rect.min.x, rect.max.y - 2.0), rect.max), color });
                }
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;

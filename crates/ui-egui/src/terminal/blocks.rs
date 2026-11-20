//! Command blocks from shell integration: a thin bar left of each visible prompt line shows how
//! its command went (green for exit 0, red for any other end, grey while it runs), and hovering it
//! says the exit code and how long the command took. Clicking the bar folds the output of a
//! finished command into one row; clicking that row shows it again.

use egui::{Color32, CursorIcon, FontId, Painter, Pos2, Rect, Response, pos2, vec2};
use sshcraft_term::{CommandRecord, CommandRow, CommandState, FoldRow};

use super::{CellMetrics, TermPalette};
use crate::i18n::{tr, trf};

/// The width of the bar, in points.
const BAR_WIDTH: f32 = 3.0;
/// How far right of its left edge a click still hits the bar, in points.
const BAR_HIT: f32 = BAR_WIDTH + 4.0;

const OK: Color32 = Color32::from_rgb(0x3f, 0xb9, 0x50);
const FAILED: Color32 = Color32::from_rgb(0xe5, 0x48, 0x4d);
const RUNNING: Color32 = Color32::from_rgb(0x8b, 0x8f, 0x98);

/// The bar colour of a command; a prompt without a command has none.
pub fn gutter_color(record: &CommandRecord) -> Option<Color32> {
    match record.state {
        CommandState::Prompt => None,
        CommandState::Running => Some(RUNNING),
        CommandState::Finished if record.exit == Some(0) => Some(OK),
        CommandState::Finished => Some(FAILED),
    }
}

/// What hovering the bar says.
pub fn describe(record: &CommandRecord) -> String {
    let took = match (record.started, record.finished) {
        (Some(start), Some(end)) => {
            let ms = end.signed_duration_since(start).num_milliseconds().max(0);
            format!("{}.{} s", ms / 1000, (ms % 1000) / 100)
        }
        _ => String::new(),
    };
    let started = record.started.map(|s| s.with_timezone(&chrono::Local).format("%H:%M:%S").to_string()).unwrap_or_default();
    match (record.state, record.exit) {
        (CommandState::Running, _) => trf("Running since {time}", &[("time", &started)]),
        (CommandState::Finished, Some(code)) => {
            trf("Exit code {code}, {took}, started {time}", &[("code", &code.to_string()), ("took", &took), ("time", &started)])
        }
        (CommandState::Finished, None) => trf("Finished, {took}, started {time}", &[("took", &took), ("time", &started)]),
        (CommandState::Prompt, _) => tr("Waiting for a command").to_string(),
    }
}

/// What a folded row says.
pub fn fold_label(lines: usize) -> String {
    if lines == 1 { tr("One line folded, click to show").to_string() } else { trf("{n} lines folded, click to show", &[("n", &lines.to_string())]) }
}

/// The top of screen row `row` in `rect`.
fn row_top(rect: Rect, metrics: &CellMetrics, row: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)] // A row index is far below 2^24.
    let row = row as f32;
    rect.min.y + row * metrics.height
}

/// The command whose bar is under `pos`, when its output can fold.
pub fn bar_at(pos: Pos2, rect: Rect, metrics: &CellMetrics, rows: &[CommandRow]) -> Option<u64> {
    if pos.x < rect.min.x || pos.x > rect.min.x + BAR_HIT {
        return None;
    }
    rows.iter().find(|r| r.foldable && (row_top(rect, metrics, r.row)..row_top(rect, metrics, r.row + 1)).contains(&pos.y)).map(|r| r.record.id())
}

/// Draws the bars of `rows` and shows the description of the one under the pointer.
pub fn paint_gutter(painter: &Painter, response: &Response, rect: Rect, metrics: &CellMetrics, rows: &[CommandRow]) {
    let pointer = response.hover_pos();
    for row in rows {
        let Some(color) = gutter_color(&row.record) else { continue };
        let bar = Rect::from_min_size(pos2(rect.min.x, row_top(rect, metrics, row.row)), vec2(BAR_WIDTH, metrics.height));
        painter.rect_filled(bar, 0.0, color);
        let hit = Rect::from_min_size(bar.min, vec2(BAR_HIT, metrics.height));
        if pointer.is_some_and(|p| hit.contains(p)) {
            let text = match (row.foldable, row.folded) {
                (true, true) => format!("{}\n{}", describe(&row.record), tr("Click to show the output")),
                (true, false) => format!("{}\n{}", describe(&row.record), tr("Click to fold the output")),
                _ => describe(&row.record),
            };
            if row.foldable {
                painter.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            response.clone().on_hover_text_at_pointer(text);
        }
    }
}

/// Draws the rows that stand for folded output: a faint band with how many lines it hides.
pub fn paint_folds(
    painter: &Painter,
    response: &Response,
    rect: Rect,
    metrics: &CellMetrics,
    folds: &[FoldRow],
    palette: &TermPalette,
    font: &FontId,
) {
    let pointer = response.hover_pos();
    for fold in folds {
        let top = row_top(rect, metrics, fold.row);
        let band = Rect::from_min_max(pos2(rect.min.x, top), pos2(rect.max.x, top + metrics.height));
        let hovered = pointer.is_some_and(|p| band.contains(p));
        let fill = palette.fg.gamma_multiply(if hovered { 0.12 } else { 0.06 });
        painter.rect_filled(band.shrink2(vec2(0.0, 1.0)), 3.0, fill);
        let text = format!("\u{22ef} {}", fold_label(fold.lines));
        painter.text(
            pos2(band.min.x + metrics.width, band.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            font.clone(),
            palette.fg.gamma_multiply(0.65),
        );
        if hovered {
            painter.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
    }
}

/// The folded output whose row is `row`.
pub fn fold_at(row: u16, folds: &[FoldRow]) -> Option<u64> {
    folds.iter().find(|f| f.row == usize::from(row)).map(|f| f.id)
}

//! Command blocks from shell integration: a thin bar left of each visible prompt line shows how
//! its command went (green for exit 0, red for any other end, grey while it runs), and hovering it
//! says the exit code and how long the command took.

use egui::{Color32, Painter, Rect, Response, pos2, vec2};
use sshcraft_term::{CommandRecord, CommandRow, CommandState};

use super::CellMetrics;
use crate::i18n::{tr, trf};

/// The width of the bar, in points.
const BAR_WIDTH: f32 = 3.0;

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

/// Draws the bars of `rows` and shows the description of the one under the pointer.
pub fn paint_gutter(painter: &Painter, response: &Response, rect: Rect, metrics: &CellMetrics, rows: &[CommandRow]) {
    let pointer = response.hover_pos();
    for row in rows {
        let Some(color) = gutter_color(&row.record) else { continue };
        #[allow(clippy::cast_precision_loss)] // A row index is far below 2^24.
        let top = rect.min.y + row.row as f32 * metrics.height;
        let bar = Rect::from_min_size(pos2(rect.min.x, top), vec2(BAR_WIDTH, metrics.height));
        painter.rect_filled(bar, 0.0, color);
        if pointer.is_some_and(|p| bar.expand2(vec2(2.0, 0.0)).contains(p)) {
            response.clone().on_hover_text_at_pointer(describe(&row.record));
        }
    }
}

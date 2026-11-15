//! A QR code drawn with the painter (dark modules on a light square), for a phone to scan.

use egui::{Color32, Rect, Sense, Ui, vec2};
use qrcode::{Color, EcLevel, QrCode};

/// Draws `text` as a QR code `size` points wide, named `label` for screen readers. Nothing is
/// drawn when the text does not fit a QR code.
pub fn show(ui: &mut Ui, text: &str, size: f32, label: &str) {
    let Ok(code) = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M) else { return };
    let width = code.width();
    let colors = code.to_colors();
    // A quiet zone of four modules around the code, as scanners expect.
    let modules = width + 8;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, label));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    let cell = size / modules as f32;
    for (i, color) in colors.iter().enumerate() {
        if *color == Color::Dark {
            let (x, y) = ((i % width + 4) as f32, (i / width + 4) as f32);
            painter.rect_filled(Rect::from_min_size(rect.min + vec2(x * cell, y * cell), vec2(cell.ceil(), cell.ceil())), 0.0, Color32::BLACK);
        }
    }
}

//! A QR code drawn with the painter (dark modules on a light square), for a phone to scan.
//! The modules are encoded once; they hold the text as much as the text does, so they are
//! wiped when dropped.

use egui::{Color32, Rect, Sense, Ui, vec2};
use qrcode::{Color, EcLevel, QrCode};
use zeroize::Zeroizing;

/// The modules of a QR code: 1 for dark, row by row.
#[derive(Clone)]
pub struct QrModules {
    width: usize,
    dark: Zeroizing<Vec<u8>>,
}

impl QrModules {
    /// The code for `text`; `None` when the text does not fit a QR code.
    pub fn new(text: &str) -> Option<Self> {
        let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
        let dark = code.to_colors().iter().map(|c| u8::from(*c == Color::Dark)).collect();
        Some(Self { width: code.width(), dark: Zeroizing::new(dark) })
    }
}

/// Draws `modules` `size` points wide, named `label` for screen readers.
pub fn show(ui: &mut Ui, modules: &QrModules, size: f32, label: &str) {
    let width = modules.width.max(1);
    // A quiet zone of four modules around the code, as scanners expect.
    let count = width + 8;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, label));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    let cell = size / count as f32;
    for (i, dark) in modules.dark.iter().enumerate() {
        if *dark == 1 {
            let (x, y) = ((i % width + 4) as f32, (i / width + 4) as f32);
            painter.rect_filled(Rect::from_min_size(rect.min + vec2(x * cell, y * cell), vec2(cell.ceil(), cell.ceil())), 0.0, Color32::BLACK);
        }
    }
}

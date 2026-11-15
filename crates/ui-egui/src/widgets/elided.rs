//! Text from hosts, imports and servers shown safely: characters that hide or reorder text made
//! visible, and never wider than its space (cut with "…", the full text in the tooltip and for
//! screen readers).

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, FontId, Response, Sense, Ui, WidgetInfo};

/// Characters shown at most of a name; the rest is cut before layout so a huge name costs nothing.
const MAX_CHARS: usize = 512;

/// `text` with control characters dropped and bidi and invisible formatting characters shown as
/// `⟨U+202E⟩`, so a name cannot reorder or hide what follows it.
pub fn visible(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().take(MAX_CHARS) {
        match c {
            '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}' => {
                out.push_str(&format!("⟨U+{:04X}⟩", c as u32));
            }
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// A one-line label at most `max_width` wide; returns its response (click sense).
pub fn elided_label(ui: &mut Ui, text: &str, max_width: f32, color: Color32, font: FontId) -> Response {
    let shown = visible(text);
    let mut job = LayoutJob::single_section(shown.clone(), TextFormat::simple(font, color));
    job.wrap = TextWrapping { max_width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let cut = galley.elided;
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::click());
    ui.painter().galley(rect.min, galley, color);
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Label, true, text));
    if cut { response.on_hover_text(shown) } else { response }
}

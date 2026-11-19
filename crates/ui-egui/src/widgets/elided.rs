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
            '\u{61c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}' => {
                out.push_str(&format!("⟨U+{:04X}⟩", c as u32));
            }
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// A field for a secret (a password, a passphrase, a sign-in answer), masked unless `masked` is
/// false. Its undo history is emptied after every frame, so egui's memory never keeps an earlier
/// copy of what was typed.
pub fn secret_field(ui: &mut Ui, value: &mut String, id: egui::Id, masked: bool, hint: &str, width: f32) -> Response {
    let response = ui.add(egui::TextEdit::singleline(value).id(id).password(masked).hint_text(hint).desired_width(width));
    forget_undo(ui.ctx(), id);
    response
}

/// Empties the undo history of the text field `id` (for fields that hold a private key).
pub fn forget_undo(ctx: &egui::Context, id: egui::Id) {
    if let Some(mut state) = egui::text_edit::TextEditState::load(ctx, id) {
        state.clear_undoer();
        state.store(ctx, id);
    }
}

/// [`visible`] for text of several lines (a server's sign-in instructions): line breaks stay.
pub fn visible_lines(text: &str) -> String {
    text.lines().map(visible).collect::<Vec<_>>().join(
        "
",
    )
}

/// `text` made [`visible`] and laid out on one line at most `max_width` wide, cut with "…".
pub fn one_line(ui: &Ui, text: &str, max_width: f32, color: Color32, font: FontId) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(visible(text), TextFormat::simple(font, color));
    job.wrap = TextWrapping { max_width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.fonts_mut(|f| f.layout_job(job))
}

/// A one-line label at most `max_width` wide; returns its response (click sense).
pub fn elided_label(ui: &mut Ui, text: &str, max_width: f32, color: Color32, font: FontId) -> Response {
    let shown = visible(text);
    let galley = one_line(ui, text, max_width, color, font);
    let cut = galley.elided;
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::click());
    ui.painter().galley(rect.min, galley, color);
    response.widget_info(|| WidgetInfo::labeled(egui::Role::Label, true, text));
    if cut { response.on_hover_text(shown) } else { response }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_line_is_cut_with_an_ellipsis() {
        let seen = std::rc::Rc::new(std::cell::Cell::new(None));
        let out = seen.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            let galley = one_line(ui, "backup-2026-11-19-mit-langem-namen.tar.gz", 120.0, Color32::WHITE, FontId::proportional(14.0));
            out.set(Some((galley.elided, galley.size().x)));
        });
        harness.run();
        let (cut, width) = seen.get().unwrap();
        assert!(cut);
        assert!(width <= 120.0);
    }
}

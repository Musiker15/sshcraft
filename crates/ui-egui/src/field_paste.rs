//! Text fields on a tablet: a field with the keyboard scrolls into the part the on-screen
//! keyboard leaves free, and gets a Paste button.
//!
//! Paste for text fields on a tablet. Without a hardware keyboard there is no Ctrl+V, and egui
//! has no long-press menu, so a field that has the keyboard gets a Paste button above the
//! on-screen keyboard. A tap reads the system clipboard (the app's paste source) and hands the
//! text to the field as a paste on the next frame; the field keeps the keyboard throughout.

use crate::app::{Form, SshcraftApp};
use crate::i18n::tr;

/// The button's place on the last frame and a paste on its way to a field.
#[derive(Default)]
pub struct FieldPaste {
    rect: Option<egui::Rect>,
    pending: Option<(egui::Id, String)>,
    /// Where the field with the keyboard was on the last frame.
    field: Option<egui::Rect>,
    /// That field, when the keyboard covers it (or it went above the top): the scroll area
    /// around it scrolls it back into view (`scroll_into_view`).
    pub scroll: Option<egui::Rect>,
}

/// Scrolls the field with the keyboard into view, when `target` (from `FieldPaste::scroll`) is
/// set. Called first thing inside a scroll area's content: egui hands a scroll target only to the
/// scroll areas it was set inside of.
pub fn scroll_into_view(ui: &egui::Ui, target: Option<egui::Rect>) {
    if let Some(field) = target {
        ui.scroll_to_rect(field.expand2(egui::vec2(0.0, 4.0)), None);
    }
}

/// The widget with the keyboard when it is a text field: not a terminal, which has its own
/// key bar.
fn focused_field(app: &SshcraftApp, ctx: &egui::Context) -> Option<egui::Id> {
    let focused = ctx.memory(|m| m.focused())?;
    let terminal = app.terminals.values().any(|t| t.view == Some(focused));
    (!terminal && ctx.egui_wants_keyboard_input()).then_some(focused)
}

/// Before the frame is drawn: delivers a pending paste to its field, and keeps the field's
/// focus when the press is on the button (egui would take it away for a press elsewhere, and
/// Android would close the keyboard). Returns what to put back after the frame.
pub fn before(app: &mut SshcraftApp, ui: &egui::Ui) -> Option<egui::SurrenderFocusOn> {
    if app.form != Form::Tablet {
        return None;
    }
    let ctx = ui.ctx();
    // A field the keyboard covers (or that went above the top) scrolls back into view.
    let visible = ctx.content_rect();
    app.field_paste.scroll = app.field_paste.field.filter(|field| field.bottom() > visible.bottom() || field.top() < visible.top());
    if let Some((field, text)) = app.field_paste.pending.take() {
        ctx.memory_mut(|m| m.request_focus(field));
        ctx.input_mut(|i| i.events.push(egui::Event::Paste(text)));
    }
    let on_button = app.field_paste.rect.is_some_and(|r| ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| r.contains(p)));
    on_button.then(|| {
        let before = ctx.options(|o| o.input_options.surrender_focus_on);
        ctx.options_mut(|o| o.input_options.surrender_focus_on = egui::SurrenderFocusOn::Never);
        before
    })
}

/// After the frame is drawn: the button while a text field has the keyboard.
pub fn after(app: &mut SshcraftApp, ctx: &egui::Context, restore: Option<egui::SurrenderFocusOn>) {
    if let Some(surrender) = restore {
        ctx.options_mut(|o| o.input_options.surrender_focus_on = surrender);
    }
    app.field_paste.rect = None;
    app.field_paste.field = None;
    if app.form != Form::Tablet {
        return;
    }
    let Some(field) = focused_field(app, ctx) else { return };
    app.field_paste.field = ctx.output(|o| o.ime.as_ref().filter(|ime| ime.owner == field).map(|ime| ime.rect));
    // Above the keyboard and the navigation bar, which the content rect leaves out.
    let corner = ctx.content_rect().right_bottom() - egui::vec2(16.0, 16.0);
    let area =
        egui::Area::new(egui::Id::unique("sshcraft-field-paste")).order(egui::Order::Foreground).pivot(egui::Align2::RIGHT_BOTTOM).fixed_pos(corner);
    let shown = area.show(ctx, |ui| ui.add(egui::Button::new(tr("Paste")).sense(egui::Sense::CLICK).min_size(egui::vec2(96.0, 44.0))));
    app.field_paste.rect = Some(shown.response.rect);
    if shown.inner.clicked()
        && let Some(text) = app.paste_source.as_ref().and_then(|source| source())
    {
        app.field_paste.pending = Some((field, text));
        ctx.request_repaint();
    }
}

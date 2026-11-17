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
pub fn before(app: &mut SshcraftApp, ctx: &egui::Context) -> Option<egui::SurrenderFocusOn> {
    if app.form != Form::Tablet {
        return None;
    }
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
    if app.form != Form::Tablet {
        return;
    }
    let Some(field) = focused_field(app, ctx) else { return };
    let corner = ctx.content_rect().right_bottom() - egui::vec2(12.0, 12.0);
    let area =
        egui::Area::new(egui::Id::unique("sshcraft-field-paste")).order(egui::Order::Foreground).pivot(egui::Align2::RIGHT_BOTTOM).fixed_pos(corner);
    let shown = area.show(ctx, |ui| ui.add(egui::Button::new(tr("Paste")).sense(egui::Sense::CLICK).min_size(egui::vec2(72.0, 32.0))));
    app.field_paste.rect = Some(shown.response.rect);
    if shown.inner.clicked()
        && let Some(text) = app.paste_source.as_ref().and_then(|source| source())
    {
        app.field_paste.pending = Some((field, text));
        ctx.request_repaint();
    }
}

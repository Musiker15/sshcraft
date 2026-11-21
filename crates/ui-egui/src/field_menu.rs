//! Cut, copy and paste for text fields with the right mouse button. egui gives its text fields no
//! menu of their own, so this one opens over whichever field was right-clicked. It focuses the
//! field and hands the action to egui as Ctrl+X, Ctrl+C or Ctrl+V would: the system clipboard is
//! used, and a password field copies nothing, as with the keys.

use crate::app::SshcraftApp;
use crate::i18n::tr;

/// The menu that is open and a paste on its way to a field.
#[derive(Default)]
pub struct FieldMenu {
    /// The field the menu belongs to and where it opened.
    open: Option<(egui::Id, egui::Pos2)>,
}

/// What the menu does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Cut,
    Copy,
    Paste,
}

/// A paste read from the app's paste source (Android), handed to `target` on the next frame.
#[derive(Clone, Default)]
struct Pending(Option<(egui::Id, String)>);

/// Where [`Pending`] waits in egui's memory.
fn pending_key() -> egui::Id {
    egui::Id::unique("sshcraft-pending-paste")
}

/// Hands `text` to `target` as a paste at the start of the next frame (the frame that reads it
/// has already begun). For the app's paste source, which egui does not know.
pub fn paste_next_frame(ctx: &egui::Context, target: egui::Id, text: String) {
    ctx.data_mut(|d| d.insert_temp(pending_key(), Pending(Some((target, text)))));
    ctx.request_repaint();
}

/// Before the frame is drawn: delivers a paste [`paste_next_frame`] queued.
pub fn before(ctx: &egui::Context) {
    if let Some(Pending(Some((target, text)))) = ctx.data_mut(|d| d.remove_temp::<Pending>(pending_key())) {
        ctx.memory_mut(|m| m.request_focus(target));
        ctx.input_mut(|i| i.events.push(egui::Event::Paste(text)));
    }
}

/// The text field under the pointer: a text field keeps its state under its id.
fn field_under_pointer(ctx: &egui::Context) -> Option<egui::Id> {
    let hovered = ctx.interaction_snapshot(|s| s.hovered.clone());
    hovered.into_iter().find(|id| egui::text_edit::TextEditState::load(ctx, *id).is_some())
}

/// Does `action` on `field`: focuses it and asks egui for the cut, copy or paste.
pub fn run(app: &SshcraftApp, ctx: &egui::Context, field: egui::Id, action: Action) {
    ctx.memory_mut(|m| m.request_focus(field));
    match action {
        Action::Cut => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut),
        Action::Copy => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy),
        Action::Paste => match app.paste_source.as_ref().and_then(|source| source()) {
            Some(text) => paste_next_frame(ctx, field, text),
            None => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
        },
    }
}

/// After the frame is drawn: opens the menu on a right click into a text field and shows it.
pub fn after(app: &mut SshcraftApp, ctx: &egui::Context) {
    if ctx.input(|i| i.pointer.secondary_clicked())
        && let Some(field) = field_under_pointer(ctx)
        && let Some(at) = ctx.input(|i| i.pointer.interact_pos())
    {
        app.field_menu.open = Some((field, at));
    }
    let Some((field, at)) = app.field_menu.open else { return };
    let shown =
        egui::Area::new(egui::Id::unique("sshcraft-field-menu")).order(egui::Order::Foreground).fixed_pos(at).constrain(true).show(ctx, |ui| {
            egui::Frame::menu(ui.style())
                .show(ui, |ui| {
                    ui.set_min_width(140.0);
                    let mut chosen = None;
                    for (action, label) in [(Action::Cut, "Cut"), (Action::Copy, "Copy"), (Action::Paste, "Paste")] {
                        if ui.add(egui::Button::new(tr(label)).frame(false)).clicked() {
                            chosen = Some(action);
                        }
                    }
                    chosen
                })
                .inner
        });
    if let Some(action) = shown.inner {
        app.field_menu.open = None;
        run(app, ctx, field, action);
        return;
    }
    // A press anywhere else or Escape closes it.
    let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !shown.response.rect.contains(p)));
    if outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.field_menu.open = None;
    }
}

//! Asks before an action that cannot be undone: what goes, what it means, and a button that
//! names the action. Nothing happens until that button is clicked.

use sshcraft_model::forward::ForwardRule;
use sshcraft_model::model::HostId;

use crate::app::SshcraftApp;
use crate::i18n::tr;

/// The action waiting for the user's yes.
#[derive(Debug, Clone)]
pub enum Pending {
    RemoveServer { id: String },
    EndShare { server: String, area: String },
    DeleteSnippet { id: String },
    DeleteRule { host: HostId, rule: ForwardRule },
}

#[derive(Debug, Clone)]
pub struct Confirm {
    pub title: String,
    pub body: String,
    /// The button's text: the action, never a bare "OK".
    pub button: &'static str,
    pub pending: Pending,
}

/// Asks before `pending` runs.
pub fn ask(app: &mut SshcraftApp, title: String, body: &str, button: &'static str, pending: Pending) {
    app.ui.ask = Some(Confirm { title, body: body.to_string(), button, pending });
}

pub fn show(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(confirm) = app.ui.ask.clone() else { return };
    let (mut close, mut go) = (false, false);
    let modal = crate::widgets::modal(ctx, egui::Id::unique("sshcraft-confirm")).show(ctx, |ui| {
        crate::widgets::fit_dialog(ui);
        ui.set_width(crate::widgets::fit(ui, 440.0));
        crate::widgets::buttons::title(ui, &confirm.title);
        ui.add_space(6.0);
        ui.label(&confirm.body);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(tr("Cancel")).clicked() {
                close = true;
            }
            if ui.button(confirm.button).clicked() {
                go = true;
            }
        });
    });
    if modal.should_close() {
        close = true;
    }
    if go {
        app.ui.ask = None;
        run(app, confirm.pending);
    } else if close {
        app.ui.ask = None;
    }
}

fn run(app: &mut SshcraftApp, pending: Pending) {
    match pending {
        Pending::RemoveServer { id } => crate::pages::sync::remove_server(app, &id),
        Pending::EndShare { server, area } => crate::pages::sharing::end_share(app, server, area),
        Pending::DeleteSnippet { id } => crate::pages::snippets::delete(app, id),
        Pending::DeleteRule { host, rule } => crate::pages::forwarding::delete_rule(app, host, &rule),
    }
}

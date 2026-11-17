//! Quick connect: `user@host:port` typed, or filled from an `ssh://` link. A host that matches
//! is opened; otherwise one is saved under that name and opened, so host keys, questions and
//! history work as for every host. Nothing connects before the user presses Connect.

use egui::Ui;
use serde_json::json;
use sshcraft_model::model::HostId;
use sshcraft_model::ssh_url::{Target, TargetError, parse_quick};

use crate::app::SshcraftApp;
use crate::i18n::tr;
use crate::state::ConnectRequest;
use crate::theme::Tokens;

/// The dialog's state.
#[derive(Debug, Clone, Default)]
pub struct QuickConnect {
    pub text: String,
    pub error: Option<String>,
    /// Set while the new host is being saved.
    pub busy: bool,
}

/// Opens the dialog with `text` in its field.
pub fn open(app: &mut SshcraftApp, text: String) {
    app.ui.quick_connect = Some(QuickConnect { text, error: None, busy: false });
}

fn problem(e: &TargetError) -> &'static str {
    match e {
        TargetError::NotSsh | TargetError::Extra => tr("Only user@host:port or an ssh:// link without a path."),
        TargetError::Empty => tr("Type a host to connect to."),
        TargetError::TooLong => tr("That is too long for a host."),
        TargetError::Password => tr("A link carries no password; SSHCraft asks for it when connecting."),
        TargetError::User => tr("The user name has spaces, control characters or starts with -."),
        TargetError::Host => tr("The host has spaces or characters a host name cannot have."),
        TargetError::Port => tr("The port must be 1 to 65535."),
    }
}

/// A saved host that uses exactly this address, port and user, also where its groups give them.
fn matching(app: &SshcraftApp, target: &Target) -> Option<HostId> {
    let port = target.port.unwrap_or(sshcraft_model::model::DEFAULT_PORT);
    let groups: Vec<sshcraft_model::model::Group> = app.data.groups.iter().map(|g| g.group.clone()).collect();
    app.data
        .hosts
        .iter()
        .find(|row| {
            let s = &row.host.settings;
            let chain = crate::pages::host_form::chain(row.host.group_id, &groups);
            let address = s.address.clone().or_else(|| chain.iter().find_map(|g| g.defaults.address.clone()));
            let used_port = s.port.or_else(|| chain.iter().find_map(|g| g.defaults.port)).unwrap_or(sshcraft_model::model::DEFAULT_PORT);
            let user = s.username.clone().or_else(|| chain.iter().find_map(|g| g.defaults.username.clone()));
            address.as_deref() == Some(target.host.as_str()) && used_port == port && user == target.user
        })
        .map(|row| row.host.id)
}

/// Opens the host `text` names, saving it first when there is none.
pub fn connect(app: &mut SshcraftApp, text: &str) -> Result<(), String> {
    let target = parse_quick(text).map_err(|e| problem(&e).to_string())?;
    if let Some(id) = matching(app, &target) {
        app.ui.connect_request = Some(ConnectRequest::Host(id));
        app.ui.quick_connect = None;
        return Ok(());
    }
    let id = HostId::new();
    let host = json!({
        "id": id,
        "name": target.name(),
        "settings": { "address": target.host, "port": target.port, "username": target.user },
    });
    if let Some(dialog) = &mut app.ui.quick_connect {
        dialog.busy = true;
    }
    app.runner.run(
        "host.save",
        json!({ "host": host }),
        Box::new(move |app, result| match result {
            Ok(_) => {
                app.data.dirty |= crate::data::Refresh::HOSTS;
                app.ui.connect_request = Some(ConnectRequest::Host(id));
                app.ui.quick_connect = None;
            }
            Err(e) => match &mut app.ui.quick_connect {
                Some(dialog) => {
                    dialog.busy = false;
                    dialog.error = Some(e.to_string());
                }
                None => app.toasts.error(&e.to_string()),
            },
        }),
    );
    Ok(())
}

/// The field and its button, on the new-tab page and in the dialog.
pub fn field(app: &mut SshcraftApp, ui: &mut Ui, text: &mut String) -> Option<String> {
    let mut go = false;
    ui.horizontal(|ui| {
        let response = ui.add(egui::TextEdit::singleline(text).hint_text("user@host:port").desired_width(260.0).char_limit(256));
        go = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        go |= ui.add_enabled(!app.forms.busy(), crate::widgets::buttons::main_action(tr("Connect"))).clicked();
    });
    if !go || text.trim().is_empty() {
        return None;
    }
    connect(app, text).err()
}

/// Draws the dialog while it is open.
pub fn show(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.ui.quick_connect.take() else { return };
    let tokens = Tokens::get(ctx);
    let mut keep = true;
    egui::Modal::new(egui::Id::unique("sshcraft-quick-connect")).show(ctx, |ui| {
        ui.set_width(420.0);
        crate::widgets::buttons::title(ui, tr("Quick connect"));
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("A new host is saved under this name; you can delete it later.")).color(tokens.text_dim));
        ui.add_space(6.0);
        // `connect` may close the dialog; it works on the app, so put the dialog back first.
        let mut text = std::mem::take(&mut dialog.text);
        app.ui.quick_connect = Some(dialog.clone());
        let error = if dialog.busy { None } else { field(app, ui, &mut text) };
        match &mut app.ui.quick_connect {
            Some(open) => {
                open.text = text;
                if error.is_some() {
                    open.error = error;
                }
                if let Some(problem) = &open.error {
                    ui.label(egui::RichText::new(problem).color(tokens.danger));
                }
            }
            None => keep = false,
        }
        ui.add_space(6.0);
        if ui.button(tr("Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
    });
    if !keep {
        app.ui.quick_connect = None;
    }
}

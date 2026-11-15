//! Settings > Sync: the servers this device syncs with and their state, an account created or
//! joined, the account key shown once (with a QR code) and never kept, the devices of an
//! account, items moved between this device and a server, and the connections SSHCraft makes
//! by itself (rule 17.6).

use egui::Ui;
use serde_json::{Value, json};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::theme::Tokens;
use crate::widgets::{secret_field, visible};

#[derive(Clone, Default)]
pub struct CreateForm {
    pub server_name: String,
    pub origin: String,
    pub api_key: String,
    pub name: String,
    pub email: String,
    pub device_name: String,
}

#[derive(Clone, Default)]
pub struct JoinForm {
    pub server_name: String,
    pub origin: String,
    pub api_key: String,
    pub account_key: String,
    pub device_name: String,
}

// Secrets never show in a debug print of the window's state.
impl std::fmt::Debug for CreateForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateForm").field("server_name", &self.server_name).field("origin", &self.origin).finish_non_exhaustive()
    }
}

impl std::fmt::Debug for JoinForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoinForm").field("server_name", &self.server_name).field("origin", &self.origin).finish_non_exhaustive()
    }
}

/// The account key of a new account, shown once.
#[derive(Clone)]
struct Created {
    key: zeroize::Zeroizing<String>,
}

impl std::fmt::Debug for Created {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Created { .. }")
    }
}

/// Moving a host, group or key between this device and a server.
#[derive(Debug, Clone)]
pub struct MoveForm {
    kind: String,
    id: String,
    name: String,
    /// The server, or `None` for this device only.
    location: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SyncView {
    pub create: Option<CreateForm>,
    pub join: Option<JoinForm>,
    created: Option<Created>,
    /// The devices of the account of `devices_of`.
    pub devices: Option<Vec<Value>>,
    devices_of: Option<String>,
    /// A device waiting for its revocation to be confirmed: server, device id, name.
    revoke: Option<(String, String, String)>,
    moving: Option<MoveForm>,
}

impl SyncView {
    /// The account key shown now, if one is.
    pub fn account_key(&self) -> Option<&str> {
        self.created.as_ref().map(|c| c.key.as_str())
    }
}

/// Opens the dialog that moves `kind` (`host`, `group`, `key`) `id` named `name`.
pub fn ask_move(app: &mut SshcraftApp, kind: &str, id: &str, name: &str) {
    app.ui.sync.moving = Some(MoveForm { kind: kind.into(), id: id.into(), name: name.into(), location: None });
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// A server's state, in words.
fn status_text(status: Option<&Value>) -> String {
    let Some(s) = status else { return tr("Sync off").to_string() };
    match s.get("state").and_then(Value::as_str).unwrap_or_default() {
        "idle" => tr("Up to date").to_string(),
        "syncing" => tr("Syncing…").to_string(),
        "offline" => trf("Offline, retry in {seconds} s", &[("seconds", &s.get("retry_in_secs").and_then(Value::as_u64).unwrap_or(0).to_string())]),
        "revoked" => tr("This device was revoked").to_string(),
        "blocked" => tr("Blocked by the server's operator").to_string(),
        "error" => trf("Error: {detail}", &[("detail", &visible(&text(s, "detail")))]),
        _ => tr("Sync off").to_string(),
    }
}

fn servers(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    if app.data.servers.is_empty() {
        ui.label(egui::RichText::new(tr("No sync server yet. Everything stays on this device.")).color(tokens.text_dim));
    }
    for server in app.data.servers.clone() {
        let id = text(&server, "id");
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(visible(&text(&server, "name"))).strong());
                ui.label(egui::RichText::new(visible(&text(&server, "origin"))).color(tokens.text_dim));
                ui.label(status_text(app.data.sync_status.get(&id).or_else(|| server.get("status"))));
            });
            ui.horizontal(|ui| {
                let mut on = server.get("sync_enabled").and_then(Value::as_bool).unwrap_or(false);
                let mut keys = server.get("sync_keys").and_then(Value::as_bool).unwrap_or(false);
                let mut logs = server.get("sync_logs").and_then(Value::as_bool).unwrap_or(false);
                let changed = ui.checkbox(&mut on, tr("Sync")).changed() | ui.checkbox(&mut keys, tr("Keys and passwords")).changed();
                if changed {
                    app.run_reported("sync.set_server", json!({ "id": id, "sync_enabled": on, "sync_keys": keys }));
                    app.data.dirty |= crate::data::Refresh::SERVERS;
                }
                if ui.checkbox(&mut logs, tr("Session logs")).changed() {
                    app.run_reported("sync.server_logs", json!({ "id": id, "on": logs }));
                    app.data.dirty |= crate::data::Refresh::SERVERS;
                }
                if ui.button(tr("Devices")).clicked() {
                    load_devices(app, &id);
                }
                if ui.button(tr("Remove server")).clicked() {
                    app.run_reported("sync.remove_server", json!({ "id": id }));
                    app.data.dirty |= crate::data::Refresh::DATA;
                }
            });
        });
    }
}

fn load_devices(app: &mut SshcraftApp, server: &str) {
    app.ui.sync.devices_of = Some(server.to_string());
    let server = server.to_string();
    app.runner.run(
        "device.list",
        json!({ "id": server }),
        Box::new(move |app, result| match result {
            Ok(Value::Array(list)) if app.ui.sync.devices_of.as_deref() == Some(server.as_str()) => app.ui.sync.devices = Some(list),
            Ok(_) => {}
            Err(e) => app.toasts.error(&e.to_string()),
        }),
    );
}

pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        if ui.button(tr("Create account")).clicked() {
            app.ui.sync.create = Some(CreateForm { device_name: device_name(), ..CreateForm::default() });
        }
        if ui.button(tr("Join an account")).clicked() {
            app.ui.sync.join = Some(JoinForm { device_name: device_name(), ..JoinForm::default() });
        }
    });
    ui.add_space(8.0);
    servers(app, ui);
    ui.add_space(12.0);
    ui.label(egui::RichText::new(tr("Connections SSHCraft makes by itself")).strong());
    let origins: Vec<String> = app.data.servers.iter().map(|s| text(s, "origin")).collect();
    if origins.is_empty() {
        ui.label(egui::RichText::new(tr("None. SSHCraft connects only where you tell it to.")).color(tokens.text_dim));
    }
    for origin in origins {
        ui.label(trf("Sync server {origin}", &[("origin", &visible(&origin))]));
    }
}

/// This computer's name, as a suggestion for the device name.
fn device_name() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default().chars().filter(|c| !c.is_control()).take(60).collect()
}

/// The account dialogs, the devices, the move dialog.
pub fn dialogs(app: &mut SshcraftApp, ctx: &egui::Context) {
    create_dialog(app, ctx);
    join_dialog(app, ctx);
    key_dialog(app, ctx);
    devices_dialog(app, ctx);
    move_dialog(app, ctx);
}

fn field(ui: &mut Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(300.0).char_limit(500));
    ui.end_row();
}

fn create_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.sync.create.take() else { return };
    let busy = app.forms.busy();
    let (mut keep, mut go) = (true, false);
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-create-account")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.heading(tr("Create account"));
        egui::Grid::new("sshcraft-create").num_columns(2).min_col_width(130.0).show(ui, |ui| {
            field(ui, tr("Server name"), &mut form.server_name);
            field(ui, tr("Server address"), &mut form.origin);
            ui.label(tr("API key"));
            secret_field(ui, &mut form.api_key, egui::Id::unique("sshcraft-api-key"), true, "", 300.0);
            ui.end_row();
            field(ui, tr("Your name"), &mut form.name);
            field(ui, tr("Email"), &mut form.email);
            field(ui, tr("This device's name"), &mut form.device_name);
        });
        ui.horizontal(|ui| {
            let ready =
                [&form.server_name, &form.origin, &form.api_key, &form.name, &form.email, &form.device_name].iter().all(|s| !s.trim().is_empty());
            go = ui.add_enabled(ready && !busy, egui::Button::new(tr("Create"))).clicked();
            if ui.button(tr("Cancel")).clicked() {
                keep = false;
            }
            if busy {
                ui.spinner();
            }
        });
    });
    if modal.should_close() {
        keep = false;
    }
    if go && let Some(generation) = app.forms.start() {
        let input = json!({ "server_name": form.server_name.trim(), "origin": form.origin.trim(), "api_key": form.api_key.trim(), "name": form.name.trim(), "email": form.email.trim(), "device_name": form.device_name.trim() });
        app.runner.run(
            "sync.create_account",
            json!({ "input": input }),
            Box::new(move |app, result| {
                let current = app.forms.done(generation);
                match result {
                    Ok(v) => {
                        // The key is shown even if a later step failed: the account exists.
                        let key = zeroize::Zeroizing::new(text(&v, "account_key"));
                        if current {
                            app.ui.sync.create = None;
                        }
                        app.ui.sync.created = Some(Created { key });
                        app.data.dirty |= crate::data::Refresh::DATA;
                    }
                    Err(e) => app.toasts.error(&e.to_string()),
                }
            }),
        );
    }
    if keep {
        app.ui.sync.create = Some(form);
    }
}

fn join_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.sync.join.take() else { return };
    let busy = app.forms.busy();
    let (mut keep, mut go) = (true, false);
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-join-account")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.heading(tr("Join an account"));
        egui::Grid::new("sshcraft-join").num_columns(2).min_col_width(130.0).show(ui, |ui| {
            field(ui, tr("Server name"), &mut form.server_name);
            field(ui, tr("Server address"), &mut form.origin);
            ui.label(tr("API key"));
            secret_field(ui, &mut form.api_key, egui::Id::unique("sshcraft-join-api-key"), true, "", 300.0);
            ui.end_row();
            ui.label(tr("Account key"));
            secret_field(ui, &mut form.account_key, egui::Id::unique("sshcraft-account-key"), true, "", 300.0);
            ui.end_row();
            field(ui, tr("This device's name"), &mut form.device_name);
        });
        ui.horizontal(|ui| {
            let ready = [&form.server_name, &form.origin, &form.api_key, &form.account_key, &form.device_name].iter().all(|s| !s.trim().is_empty());
            go = ui.add_enabled(ready && !busy, egui::Button::new(tr("Join"))).clicked();
            if ui.button(tr("Cancel")).clicked() {
                keep = false;
            }
        });
    });
    if modal.should_close() {
        keep = false;
    }
    if go && let Some(generation) = app.forms.start() {
        let input = json!({ "server_name": form.server_name.trim(), "origin": form.origin.trim(), "api_key": form.api_key.trim(), "account_key": form.account_key.trim(), "device_name": form.device_name.trim() });
        app.runner.run(
            "sync.join_account",
            json!({ "input": input }),
            Box::new(move |app, result| {
                let current = app.forms.done(generation);
                match result {
                    Ok(_) => {
                        if current {
                            app.ui.sync.join = None;
                        }
                        app.data.dirty |= crate::data::Refresh::DATA;
                    }
                    Err(e) => app.toasts.error(&e.to_string()),
                }
            }),
        );
    }
    if keep {
        app.ui.sync.join = Some(form);
    }
}

fn key_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(key) = app.ui.sync.account_key().map(str::to_string) else { return };
    let tokens = Tokens::get(ctx);
    let mut close = false;
    egui::Modal::new(egui::Id::unique("sshcraft-account-key-shown")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.heading(tr("Your account key"));
        ui.label(tr("Keep it in your password manager now. SSHCraft does not store it and shows it only this once. Every new device needs it."));
        ui.add_space(6.0);
        ui.label(egui::RichText::new(&key).monospace().color(tokens.text_strong));
        crate::widgets::qr::show(ui, &key, 220.0, tr("Account key as QR code"));
        if ui.button(tr("I have kept the key")).clicked() {
            close = true;
        }
    });
    if close {
        app.ui.sync.created = None;
    }
}

fn devices_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let (Some(devices), Some(server)) = (app.ui.sync.devices.clone(), app.ui.sync.devices_of.clone()) else { return };
    let tokens = Tokens::get(ctx);
    let (mut close, mut revoke) = (false, None);
    egui::Modal::new(egui::Id::unique("sshcraft-devices")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading(tr("Devices"));
        egui::Grid::new("sshcraft-device-list").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
            for d in &devices {
                let mut name = visible(&text(d, "name"));
                if d.get("recovery") == Some(&Value::Bool(true)) {
                    name = tr("Account key (recovery)").to_string();
                }
                if d.get("this_device") == Some(&Value::Bool(true)) {
                    name = trf("{name} (this device)", &[("name", &name)]);
                }
                ui.label(name);
                ui.label(visible(&text(d, "platform")));
                let state = if d.get("admin_blocked") == Some(&Value::Bool(true)) {
                    tr("Blocked by the server's operator")
                } else if d.get("active") == Some(&Value::Bool(false)) {
                    tr("Revoked")
                } else {
                    tr("Active")
                };
                ui.label(egui::RichText::new(state).color(tokens.text_dim));
                let can = d.get("recovery") != Some(&Value::Bool(true)) && d.get("this_device") != Some(&Value::Bool(true)) && d.get("active") != Some(&Value::Bool(false));
                if ui.add_enabled(can, egui::Button::new(tr("Revoke"))).clicked() {
                    revoke = Some((text(d, "device_id"), text(d, "name")));
                }
                ui.end_row();
            }
        });
        if let Some((device, name)) = app.ui.sync.revoke.as_ref().map(|(_, d, n)| (d.clone(), n.clone())) {
            ui.separator();
            ui.label(
                egui::RichText::new(trf(
                    "Revoke {name}? The account's keys change; SSH keys and passwords that device synced may have been copied: change them on the servers.",
                    &[("name", &visible(&name))],
                ))
                .color(tokens.warning),
            );
            if ui.button(tr("Revoke the device")).clicked() {
                let server = server.clone();
                app.ui.sync.revoke = None;
                app.runner.run(
                    "device.revoke",
                    json!({ "id": server, "device_id": device }),
                    Box::new(move |app, result| match result {
                        Ok(_) => load_devices(app, &server),
                        Err(e) => app.toasts.error(&e.to_string()),
                    }),
                );
            }
        }
        if ui.button(tr("Close")).clicked() {
            close = true;
        }
    });
    if let Some((device, name)) = revoke {
        app.ui.sync.revoke = Some((server, device, name));
    }
    if close {
        app.ui.sync.devices = None;
        app.ui.sync.devices_of = None;
        app.ui.sync.revoke = None;
    }
}

fn move_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.sync.moving.take() else { return };
    let servers: Vec<(String, String)> = app.data.servers.iter().map(|s| (text(s, "id"), text(s, "name"))).collect();
    let (mut keep, mut go) = (true, false);
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-move")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading(trf("Move {name}", &[("name", &visible(&form.name))]));
        ui.radio_value(&mut form.location, None, tr("This device only"));
        for (id, name) in &servers {
            ui.radio_value(&mut form.location, Some(id.clone()), visible(name));
        }
        ui.horizontal(|ui| {
            go = ui.button(tr("Move")).clicked();
            if ui.button(tr("Cancel")).clicked() {
                keep = false;
            }
        });
    });
    if modal.should_close() {
        keep = false;
    }
    if go {
        let params = json!({ "kind": form.kind, "item_id": form.id, "location": form.location });
        app.runner.run(
            "item.move",
            params,
            Box::new(|app, result| match result {
                Ok(_) => app.data.dirty |= crate::data::Refresh::DATA,
                Err(e) => app.toasts.error(&e.to_string()),
            }),
        );
        return;
    }
    if keep {
        app.ui.sync.moving = Some(form);
    }
}

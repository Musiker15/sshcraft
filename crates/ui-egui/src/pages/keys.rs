//! The keys page: keys of every kind (vault, security keys, Windows Hello, agent keys) with name,
//! type and fingerprint, and identities. Only the public part of a key is ever shown or copied;
//! a private key pasted for an import leaves the screen with the dialog.

use egui::{FontId, Sense, Ui, vec2};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::SshcraftApp;
use crate::data::KeyRow;
use crate::i18n::{tr, trf};
use crate::theme::Tokens;
use crate::widgets::icons::{Icon, draw};
use crate::widgets::{elided_label, visible};

/// What this device can make besides vault keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct SecuritySupport {
    pub fido2: bool,
    pub hello: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyType {
    Ed25519,
    Ecdsa(&'static str),
    Rsa(u16),
}

impl KeyType {
    fn spec(&self) -> Value {
        match self {
            Self::Ed25519 => json!({ "type": "ed25519" }),
            Self::Ecdsa(curve) => json!({ "type": "ecdsa", "curve": curve }),
            Self::Rsa(bits) => json!({ "type": "rsa", "bits": bits }),
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Ed25519 => "Ed25519".into(),
            Self::Ecdsa(curve) => format!("ECDSA {curve}"),
            Self::Rsa(bits) => format!("RSA {bits}"),
        }
    }
}

const TYPES: [KeyType; 6] =
    [KeyType::Ed25519, KeyType::Ecdsa("nistp256"), KeyType::Ecdsa("nistp384"), KeyType::Ecdsa("nistp521"), KeyType::Rsa(3072), KeyType::Rsa(4096)];

#[derive(Debug, Clone)]
pub struct GenerateForm {
    pub name: String,
    pub kind: KeyType,
    pub passphrase: String,
}

#[derive(Debug, Clone)]
pub struct ImportForm {
    pub name: String,
    /// The private key as pasted; dropped with the dialog.
    pub text: String,
    pub passphrase: String,
    pub save_passphrase: bool,
}

#[derive(Debug, Clone)]
pub struct SecurityForm {
    pub name: String,
    pub hello: bool,
    pub ed25519: bool,
    pub verify_user: bool,
}

/// The dialog open on the keys page.
#[derive(Debug, Clone)]
pub enum KeyDialog {
    Generate(GenerateForm),
    Import(ImportForm),
    Security(SecurityForm),
    /// Set the passphrase: `protect` asks on each use, otherwise it is saved.
    Passphrase {
        id: String,
        protect: bool,
        passphrase: String,
    },
    Certificate {
        id: String,
        text: String,
    },
}

impl KeyDialog {
    pub fn generate_mut(&mut self) -> Option<&mut GenerateForm> {
        match self {
            Self::Generate(form) => Some(form),
            _ => None,
        }
    }

    pub fn import_mut(&mut self) -> Option<&mut ImportForm> {
        match self {
            Self::Import(form) => Some(form),
            _ => None,
        }
    }
}

/// An identity being created or edited.
#[derive(Debug, Clone, Default)]
pub struct IdentityForm {
    pub id: Option<String>,
    pub name: String,
    pub username: String,
    /// The id of a stored key.
    pub key: Option<String>,
    pub password: String,
    pub has_password: bool,
    pub remove_password: bool,
    pub error: Option<String>,
}

/// Copies the public key of `id` (never anything else).
pub fn copy_public_key(app: &mut SshcraftApp, id: &str, ctx: egui::Context) {
    if let Some(key) = app.data.keys.iter().find(|k| k.id == id) {
        ctx.copy_text(key.public_key.clone());
        app.toasts.info(tr("Public key copied."));
    }
}

fn kind_label(key: &KeyRow) -> &'static str {
    if key.kind.as_str() == Some("device") {
        tr("Windows Hello")
    } else if key.algorithm.starts_with("sk-") {
        tr("Security key")
    } else {
        tr("Vault")
    }
}

/// A pasted key or certificate scrolls inside a box of fixed height: a dialog that changes its
/// height while open loses clicks (egui discards the pass).
fn fixed_height(ui: &mut Ui, name: &str, add: impl FnOnce(&mut Ui)) {
    egui::ScrollArea::vertical().id_salt(name).max_height(120.0).min_scrolled_height(120.0).auto_shrink([false, false]).show(ui, add);
}

/// Closes the key dialog that sent the command, if it is still the one open.
fn close_on_success(generation: u64) -> crate::runner::Then {
    Box::new(move |app, result| {
        let current = app.forms.done(generation);
        match result {
            Ok(_) if current => app.ui.key_dialog = None,
            Ok(_) => {}
            Err(e) => app.toasts.error(&e.to_string()),
        }
    })
}

fn dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.ui.key_dialog.take() else { return };
    let busy = app.forms.busy();
    let mut keep = true;
    let mut submit = false;
    let title = match &dialog {
        KeyDialog::Generate(_) => tr("Generate key"),
        KeyDialog::Import(_) => tr("Import key"),
        KeyDialog::Security(_) => tr("Security key"),
        KeyDialog::Passphrase { protect: true, .. } => tr("Ask for the passphrase"),
        KeyDialog::Passphrase { .. } => tr("Save the passphrase"),
        KeyDialog::Certificate { .. } => tr("Attach a certificate"),
    };
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-key-dialog")).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.heading(title);
        ui.add_space(8.0);
        egui::Grid::new("sshcraft-key-fields").num_columns(2).spacing([12.0, 8.0]).min_col_width(120.0).show(ui, |ui| match &mut dialog {
            KeyDialog::Generate(form) => {
                ui.label(tr("Name"));
                ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(280.0));
                ui.end_row();
                ui.label(tr("Type"));
                egui::ComboBox::from_id_salt("sshcraft-key-type").selected_text(form.kind.label()).show_ui(ui, |ui| {
                    for t in TYPES {
                        let label = t.label();
                        ui.selectable_value(&mut form.kind, t, label);
                    }
                });
                ui.end_row();
                ui.label(tr("Passphrase"));
                crate::widgets::secret_field(ui, &mut form.passphrase, egui::Id::unique("sshcraft-key-passphrase"), true, tr("optional"), 280.0);
                ui.end_row();
            }
            KeyDialog::Import(form) => {
                ui.label(tr("Name"));
                ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(280.0));
                ui.end_row();
                ui.label(tr("Private key"));
                fixed_height(ui, "sshcraft-import-text", |ui| {
                    let id = egui::Id::unique("sshcraft-private-key");
                    ui.add(egui::TextEdit::multiline(&mut form.text).id(id).desired_rows(6).desired_width(280.0).font(egui::TextStyle::Monospace));
                    crate::widgets::forget_undo(ui.ctx(), id);
                });
                ui.end_row();
                ui.label(tr("Passphrase"));
                crate::widgets::secret_field(
                    ui,
                    &mut form.passphrase,
                    egui::Id::unique("sshcraft-key-passphrase"),
                    true,
                    tr("if the key has one"),
                    280.0,
                );
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut form.save_passphrase, tr("Save the passphrase"));
                ui.end_row();
            }
            KeyDialog::Security(form) => {
                ui.label(tr("Name"));
                ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(280.0));
                ui.end_row();
                ui.label(tr("Where"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut form.hello, false, tr("Security key (FIDO2)"));
                    ui.radio_value(&mut form.hello, true, tr("Windows Hello"));
                });
                ui.end_row();
                ui.label(tr("Type"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut form.ed25519, false, "ECDSA P-256");
                    ui.radio_value(&mut form.ed25519, true, "Ed25519");
                });
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut form.verify_user, tr("Ask for PIN or fingerprint on each use"));
                ui.end_row();
            }
            KeyDialog::Passphrase { passphrase, .. } => {
                ui.label(tr("Passphrase"));
                crate::widgets::secret_field(ui, passphrase, egui::Id::unique("sshcraft-key-passphrase"), true, "", 280.0);
                ui.end_row();
            }
            KeyDialog::Certificate { text, .. } => {
                ui.label(tr("Certificate"));
                fixed_height(ui, "sshcraft-certificate-text", |ui| {
                    ui.add(egui::TextEdit::multiline(text).desired_rows(4).desired_width(280.0).font(egui::TextStyle::Monospace));
                });
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let action = match &dialog {
                KeyDialog::Generate(_) => tr("Generate"),
                KeyDialog::Import(_) => tr("Import"),
                KeyDialog::Security(_) => tr("Create"),
                _ => tr("Save"),
            };
            submit = ui.add_enabled(!busy, egui::Button::new(action)).clicked();
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
    if submit {
        let (id, params) = match &dialog {
            KeyDialog::Generate(f) => {
                let passphrase = if f.passphrase.is_empty() { Value::Null } else { json!(f.passphrase) };
                ("key.generate", json!({ "name": f.name.trim(), "spec": f.kind.spec(), "passphrase": passphrase }))
            }
            KeyDialog::Import(f) => {
                let passphrase = if f.passphrase.is_empty() { Value::Null } else { json!(f.passphrase) };
                ("key.import", json!({ "name": f.name.trim(), "text": f.text, "passphrase": passphrase, "save_passphrase": f.save_passphrase }))
            }
            KeyDialog::Security(f) => {
                let kind = if f.hello { "hello" } else { "fido2" };
                let algorithm = if f.ed25519 { "ed25519" } else { "ecdsa_p256" };
                ("key.create_security", json!({ "name": f.name.trim(), "kind": kind, "algorithm": algorithm, "verify_user": f.verify_user }))
            }
            KeyDialog::Passphrase { id, protect: true, passphrase } => ("key.protect", json!({ "id": id, "passphrase": passphrase })),
            KeyDialog::Passphrase { id, passphrase, .. } => ("key.remember_passphrase", json!({ "id": id, "passphrase": passphrase })),
            KeyDialog::Certificate { id, text } => ("key.attach_certificate", json!({ "id": id, "text": text })),
        };
        if let Some(generation) = app.forms.start() {
            app.runner.run(id, params, close_on_success(generation));
        }
    }
    if keep {
        app.ui.key_dialog = Some(dialog);
    }
}

fn identity_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.identity_form.take() else { return };
    let busy = app.forms.busy();
    let mut keep = true;
    let mut submit = false;
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-identity-dialog")).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.heading(if form.id.is_some() { tr("Edit identity") } else { tr("New identity") });
        ui.add_space(8.0);
        egui::Grid::new("sshcraft-identity-fields").num_columns(2).spacing([12.0, 8.0]).min_col_width(120.0).show(ui, |ui| {
            ui.label(tr("Name"));
            ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(280.0));
            ui.end_row();
            ui.label(tr("User"));
            ui.add(egui::TextEdit::singleline(&mut form.username).desired_width(280.0));
            ui.end_row();
            ui.label(tr("Key"));
            let shown = form
                .key
                .as_ref()
                .and_then(|id| app.data.keys.iter().find(|k| &k.id == id))
                .map_or_else(|| tr("None").to_string(), |k| visible(&k.name));
            egui::ComboBox::from_id_salt("sshcraft-identity-key").selected_text(shown).width(280.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut form.key, None, tr("None"));
                for k in &app.data.keys {
                    ui.selectable_value(&mut form.key, Some(k.id.clone()), visible(&k.name));
                }
            });
            ui.end_row();
            ui.label(tr("Password"));
            ui.horizontal(|ui| {
                let hint = if form.has_password { tr("Stored; type to replace") } else { tr("None stored") };
                crate::widgets::secret_field(ui, &mut form.password, egui::Id::unique("sshcraft-identity-password"), true, hint, 200.0);
                if form.has_password {
                    ui.checkbox(&mut form.remove_password, tr("Remove"));
                }
            });
            ui.end_row();
        });
        if let Some(error) = &form.error {
            ui.colored_label(Tokens::get(ui.ctx()).danger, error);
        }
        ui.horizontal(|ui| {
            submit = ui.add_enabled(!busy, egui::Button::new(tr("Save"))).clicked();
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
    if submit {
        if form.name.trim().is_empty() {
            form.error = Some(tr("An identity needs a name.").to_string());
        } else {
            let password = if !form.password.is_empty() {
                json!({ "kind": "set", "secret": form.password })
            } else if form.remove_password {
                json!({ "kind": "clear" })
            } else {
                json!({ "kind": "keep" })
            };
            let username = if form.username.trim().is_empty() { Value::Null } else { json!(form.username.trim()) };
            let key = form.key.as_ref().map_or(Value::Null, |id| json!({ "kind": "stored", "id": id }));
            let identity = json!({ "id": form.id, "name": form.name.trim(), "username": username, "key": key, "password": password });
            if let Some(generation) = app.forms.start() {
                app.runner.run(
                    "identity.save",
                    json!({ "identity": identity }),
                    Box::new(move |app, result| {
                        let current = app.forms.done(generation);
                        match result {
                            Ok(_) if current => app.ui.identity_form = None,
                            Ok(_) => {}
                            Err(e) if current => {
                                if let Some(form) = app.ui.identity_form.as_mut() {
                                    form.error = Some(e.to_string());
                                }
                            }
                            Err(e) => app.toasts.error(&e.to_string()),
                        }
                    }),
                );
            }
        }
    }
    if keep {
        app.ui.identity_form = Some(form);
    }
}

fn key_row(app: &mut SshcraftApp, ui: &mut Ui, key: &KeyRow) {
    let tokens = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let (icon, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        draw(ui.painter(), Icon::Key, icon, tokens.accent);
        let response = elided_label(ui, &key.name, 220.0, tokens.text, FontId::proportional(14.0));
        ui.label(egui::RichText::new(visible(&key.algorithm)).color(tokens.text_dim));
        ui.label(egui::RichText::new(kind_label(key)).color(tokens.text_dim));
        ui.label(egui::RichText::new(visible(&key.fingerprint)).font(FontId::monospace(12.0)).color(tokens.text_dim));
        if key.certificate.is_some() {
            ui.label(egui::RichText::new(tr("certificate")).color(tokens.success));
        }
        if key.protected {
            ui.label(egui::RichText::new(tr("asks for passphrase")).color(tokens.warning));
        }
        let id = key.id.clone();
        let ctx = ui.ctx().clone();
        let has_certificate = key.certificate.is_some();
        response.context_menu(|ui| {
            if ui.button(tr("Copy public key")).clicked() {
                copy_public_key(app, &id, ctx.clone());
            }
            if ui.button(tr("Move to…")).clicked() {
                crate::pages::sync::ask_move(app, "key", &id, &key.name);
            }
            ui.menu_button(tr("Install on host"), |ui| {
                for row in &app.data.hosts {
                    if ui.button(visible(&row.host.name)).clicked() {
                        app.run_reported("key.install", json!({ "host_id": row.host.id, "key": { "kind": "stored", "id": id } }));
                    }
                }
            });
            if ui.button(tr("Save the passphrase")).clicked() {
                app.ui.key_dialog = Some(KeyDialog::Passphrase { id: id.clone(), protect: false, passphrase: String::new() });
            }
            if ui.button(tr("Ask for the passphrase")).clicked() {
                app.ui.key_dialog = Some(KeyDialog::Passphrase { id: id.clone(), protect: true, passphrase: String::new() });
            }
            if has_certificate {
                if ui.button(tr("Detach the certificate")).clicked() {
                    app.run_reported("key.detach_certificate", json!({ "id": id }));
                }
            } else if ui.button(tr("Attach a certificate")).clicked() {
                app.ui.key_dialog = Some(KeyDialog::Certificate { id: id.clone(), text: String::new() });
            }
            if ui.button(tr("Delete")).clicked() {
                app.ui.key_confirm = Some((id.clone(), key_name(app, &id)));
            }
        });
    });
}

fn key_name(app: &SshcraftApp, id: &str) -> String {
    app.data.keys.iter().find(|k| k.id == id).map(|k| k.name.clone()).unwrap_or_default()
}

fn confirm_delete(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some((id, name)) = app.ui.key_confirm.clone() else { return };
    egui::Modal::new(egui::Id::unique("sshcraft-key-confirm")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.label(trf("Delete the key {name}? Hosts that use it can no longer sign in with it.", &[("name", &visible(&name))]));
        ui.horizontal(|ui| {
            if ui.button(tr("Delete")).clicked() {
                app.run_reported("key.delete", json!({ "id": id }));
                app.ui.key_confirm = None;
            }
            if ui.button(tr("Cancel")).clicked() {
                app.ui.key_confirm = None;
            }
        });
    });
}

pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    if app.data.security.is_none() && !app.data.security_loading {
        app.data.security_loading = true;
        app.runner.run(
            "key.security_support",
            json!({}),
            Box::new(|app, result| {
                app.data.security_loading = false;
                app.data.security = Some(result.ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default());
            }),
        );
    }
    ui.horizontal(|ui| {
        ui.heading(tr("Keys"));
        ui.add_space(16.0);
        if ui.button(tr("Generate key")).clicked() {
            app.ui.key_dialog = Some(KeyDialog::Generate(GenerateForm { name: String::new(), kind: KeyType::Ed25519, passphrase: String::new() }));
        }
        if ui.button(tr("Import key")).clicked() {
            app.ui.key_dialog =
                Some(KeyDialog::Import(ImportForm { name: String::new(), text: String::new(), passphrase: String::new(), save_passphrase: true }));
        }
        // pass-cli is a desktop program.
        if app.form == crate::app::Form::Desktop && ui.button(tr("Import from Proton Pass")).clicked() {
            crate::pages::key_sources::open_passcli(app);
        }
        if let Some(support) = app.data.security
            && (support.fido2 || support.hello)
            && ui.button(tr("Security key")).clicked()
        {
            app.ui.key_dialog =
                Some(KeyDialog::Security(SecurityForm { name: String::new(), hello: !support.fido2, ed25519: false, verify_user: true }));
        }
    });
    ui.add_space(8.0);
    let keys = app.data.keys.clone();
    if keys.is_empty() {
        ui.label(egui::RichText::new(tr("No keys yet.")).color(tokens.text_dim));
    }
    for key in &keys {
        key_row(app, ui, key);
    }
    ui.add_space(20.0);
    crate::pages::key_sources::agent_section(app, ui);
    ui.add_space(20.0);
    ui.horizontal(|ui| {
        ui.heading(tr("Identities"));
        ui.add_space(16.0);
        if ui.button(tr("New identity")).clicked() {
            app.ui.identity_form = Some(IdentityForm::default());
        }
    });
    ui.add_space(8.0);
    let identities = app.data.identities.clone();
    for identity in &identities {
        ui.horizontal(|ui| {
            let (icon, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            draw(ui.painter(), Icon::Identity, icon, tokens.text_dim);
            let response = elided_label(ui, &identity.name, 220.0, tokens.text, FontId::proportional(14.0));
            if let Some(user) = &identity.username {
                ui.label(egui::RichText::new(visible(user)).color(tokens.text_dim));
            }
            let id = identity.id.clone();
            let identity = identity.clone();
            response.context_menu(|ui| {
                if ui.button(tr("Edit")).clicked() {
                    let key = identity.key.as_ref().and_then(|k| k.get("id")).and_then(Value::as_str).map(str::to_string);
                    app.ui.identity_form = Some(IdentityForm {
                        id: Some(id.clone()),
                        name: identity.name.clone(),
                        username: identity.username.clone().unwrap_or_default(),
                        key,
                        has_password: identity.has_password,
                        ..IdentityForm::default()
                    });
                }
                if ui.button(tr("Delete")).clicked() {
                    app.run_reported("identity.delete", json!({ "id": id }));
                }
            });
        });
    }
    ui.add_space(20.0);
    crate::pages::known_hosts::section(app, ui);
    let ctx = ui.ctx().clone();
    dialog(app, &ctx);
    identity_dialog(app, &ctx);
    confirm_delete(app, &ctx);
    crate::pages::key_sources::passcli_dialog(app, &ctx);
}

//! Keys from elsewhere: the SSH agent's keys, listed and never stored (`key.agent_list`), and
//! keys in Proton Pass, imported into the vault through `pass-cli` (`key.passcli_*`). Only names
//! and public facts reach the window; the private key goes from pass-cli to the vault.

use egui::{FontId, Sense, Ui, vec2};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::SshcraftApp;
use crate::i18n::tr;
use crate::theme::Tokens;
use crate::widgets::icons::{Icon, draw};
use crate::widgets::{elided_label, visible};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AgentKeyRow {
    pub fingerprint: String,
    pub algorithm: String,
    pub comment: String,
    pub public_key: String,
}

/// The agent's keys, asked for once when the keys page is first shown, and on "Refresh".
#[derive(Debug, Clone, Default)]
pub struct AgentKeys {
    loaded: Option<Result<Vec<AgentKeyRow>, String>>,
    loading: bool,
}

impl AgentKeys {
    pub fn keys(&self) -> Option<&[AgentKeyRow]> {
        match &self.loaded {
            Some(Ok(keys)) => Some(keys),
            _ => None,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PassItem {
    pub share_id: String,
    pub item_id: String,
    pub title: String,
}

/// The Proton Pass dialog.
#[derive(Debug, Clone)]
pub enum PassCliView {
    Loading,
    /// Why there is no list (not installed, signed out, no answer).
    Unavailable(String),
    Items(Vec<PassItem>),
}

impl PassCliView {
    pub fn items(&self) -> Option<&[PassItem]> {
        match self {
            Self::Items(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }
}

fn load_agent_keys(app: &mut SshcraftApp) {
    app.ui.agent_keys.loading = true;
    app.runner.run(
        "key.agent_list",
        json!({}),
        Box::new(|app, result| {
            app.ui.agent_keys.loading = false;
            app.ui.agent_keys.loaded = Some(match result {
                Ok(Value::Array(keys)) => Ok(keys.into_iter().filter_map(|k| serde_json::from_value(k).ok()).collect()),
                Ok(_) => Ok(Vec::new()),
                Err(e) => Err(e.to_string()),
            });
        }),
    );
}

/// The agent's keys, below the vault's. Left out where there is no agent (Android).
pub fn agent_section(app: &mut SshcraftApp, ui: &mut Ui) {
    if !app.agent_available {
        return;
    }
    if !app.ui.agent_keys.is_loaded() && !app.ui.agent_keys.loading {
        load_agent_keys(app);
    }
    let tokens = Tokens::get(ui.ctx());
    let loading = app.ui.agent_keys.loading;
    let mut refresh = false;
    crate::widgets::buttons::card(
        ui,
        Some(tr("In the SSH agent")),
        app,
        |ui, _| refresh = ui.add_enabled(!loading, egui::Button::new(tr("Refresh"))).clicked(),
        |ui, app| agent_keys(app, ui, &tokens),
    );
    if refresh {
        load_agent_keys(app);
    }
}

/// The keys the agent holds, or why there are none.
fn agent_keys(app: &mut SshcraftApp, ui: &mut Ui, tokens: &Tokens) {
    let mut copy = None;
    match &app.ui.agent_keys.loaded {
        None => {
            ui.spinner();
        }
        Some(Err(_)) => {
            ui.label(egui::RichText::new(tr("The SSH agent cannot be reached. Is Proton Pass or another agent running?")).color(tokens.text_dim));
        }
        Some(Ok(keys)) if keys.is_empty() => {
            ui.label(egui::RichText::new(tr("The SSH agent holds no key.")).color(tokens.text_dim));
        }
        Some(Ok(keys)) => {
            for key in keys {
                ui.horizontal(|ui| {
                    let (icon, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    draw(ui.painter(), Icon::Key, icon, tokens.text_dim);
                    let name = if key.comment.is_empty() { tr("Agent key").to_string() } else { key.comment.clone() };
                    let response = elided_label(ui, &name, 220.0, tokens.text, FontId::proportional(14.0));
                    ui.label(egui::RichText::new(visible(&key.algorithm)).color(tokens.text_dim));
                    ui.label(egui::RichText::new(visible(&key.fingerprint)).monospace().color(tokens.text_dim));
                    response.context_menu(|ui| {
                        if ui.button(tr("Copy public key")).clicked() {
                            copy = Some(key.public_key.clone());
                            ui.close();
                        }
                    });
                });
            }
        }
    }
    if let Some(text) = copy {
        ui.ctx().copy_text(text);
        app.toasts.info(tr("Public key copied."));
    }
}

/// Opens the Proton Pass dialog: the status first, then the keys.
pub fn open_passcli(app: &mut SshcraftApp) {
    app.ui.passcli = Some(PassCliView::Loading);
    app.runner.run(
        "key.passcli_status",
        json!({}),
        Box::new(|app, result| {
            // The dialog was closed meanwhile: it stays closed.
            if !matches!(app.ui.passcli, Some(PassCliView::Loading)) {
                return;
            }
            let status = result.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let why = match status.as_str() {
                "ready" => None,
                "not_installed" => Some(tr("pass-cli is not installed. Install Proton Pass's command line to import keys from it.")),
                "logged_out" => Some(tr("pass-cli is not signed in. Run pass-cli login, then try again.")),
                _ => Some(tr("pass-cli did not answer.")),
            };
            if let Some(why) = why {
                app.ui.passcli = Some(PassCliView::Unavailable(why.to_string()));
                return;
            }
            app.runner.run(
                "key.passcli_list",
                json!({}),
                Box::new(|app, result| {
                    if !matches!(app.ui.passcli, Some(PassCliView::Loading)) {
                        return;
                    }
                    app.ui.passcli = Some(match result {
                        Ok(Value::Array(items)) => PassCliView::Items(items.into_iter().filter_map(|i| serde_json::from_value(i).ok()).collect()),
                        Ok(_) => PassCliView::Items(Vec::new()),
                        Err(e) => PassCliView::Unavailable(e.to_string()),
                    });
                }),
            );
        }),
    );
}

pub fn passcli_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(view) = app.ui.passcli.clone() else { return };
    let tokens = Tokens::get(ctx);
    let busy = app.forms.busy();
    let (mut keep, mut import) = (true, None);
    let modal = crate::widgets::modal(ctx, egui::Id::unique("sshcraft-passcli")).show(ctx, |ui| {
        crate::widgets::fit_dialog(ui);
        ui.set_width(crate::widgets::fit(ui, 480.0));
        crate::widgets::buttons::title(ui, tr("Import from Proton Pass"));
        ui.add_space(8.0);
        egui::ScrollArea::vertical().max_height(300.0).min_scrolled_height(300.0).auto_shrink([false, false]).show(ui, |ui| match &view {
            PassCliView::Loading => {
                ui.spinner();
            }
            PassCliView::Unavailable(why) => {
                ui.label(egui::RichText::new(why).color(tokens.text_dim));
            }
            PassCliView::Items(items) if items.is_empty() => {
                ui.label(egui::RichText::new(tr("Proton Pass holds no SSH key.")).color(tokens.text_dim));
            }
            PassCliView::Items(items) => {
                for item in items {
                    ui.horizontal(|ui| {
                        elided_label(ui, &item.title, 300.0, tokens.text, FontId::proportional(14.0));
                        if ui.add_enabled(!busy, egui::Button::new(tr("Import"))).clicked() {
                            import = Some(item.clone());
                        }
                    });
                }
            }
        });
        ui.add_space(8.0);
        if ui.button(tr("Close")).clicked() {
            keep = false;
        }
    });
    if modal.should_close() {
        keep = false;
    }
    if !keep {
        app.ui.passcli = None;
    }
    if let Some(item) = import
        && let Some(generation) = app.forms.start()
    {
        app.runner.run(
            "key.passcli_import",
            json!({ "share_id": item.share_id, "item_id": item.item_id, "name": item.title }),
            Box::new(move |app, result| {
                app.forms.done(generation);
                match result {
                    Ok(_) => {
                        app.toasts.info(tr("Key imported."));
                        app.data.dirty |= crate::data::Refresh::KEYS;
                    }
                    Err(e) => app.toasts.error(&e.to_string()),
                }
            }),
        );
    }
}

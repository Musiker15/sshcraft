//! The host keys SSHCraft trusts, on the keys page: each with its address, algorithm and
//! fingerprint; removed one by one (the next connection asks again), read from and written to a
//! `known_hosts` file.

use egui::Ui;
use serde_json::{Value, json};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::pages::import_export::DialogFor;
use crate::theme::Tokens;
use crate::widgets::visible;

/// One known host key as `hostkey.list` returns it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct KnownKey {
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub algorithm: Option<String>,
    pub fingerprint: String,
}

/// Loads the list when it is not there (after start and after every change of data).
fn load(app: &mut SshcraftApp) {
    if app.data.host_keys.is_some() || app.data.host_keys_loading {
        return;
    }
    app.data.host_keys_loading = true;
    app.runner.run(
        "hostkey.list",
        json!({}),
        Box::new(|app, result| {
            app.data.host_keys_loading = false;
            let rows = result.ok().and_then(|v: Value| serde_json::from_value::<Vec<KnownKey>>(v).ok()).unwrap_or_default();
            app.data.host_keys = Some(rows);
        }),
    );
}

pub fn section(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    load(app);
    ui.horizontal_wrapped(|ui| {
        crate::widgets::buttons::title(ui, tr("Known hosts"));
        ui.add_space(16.0);
        let files = app.form == crate::app::Form::Desktop;
        if files && ui.button(tr("Import known_hosts")).clicked() {
            let dialog = crate::file_dialogs::FileDialog::open().title(tr("Import known_hosts"));
            app.dialogs.ask(dialog, DialogFor::KnownHostsImport);
        }
        if files && ui.button(tr("Export known_hosts")).clicked() {
            let dialog = crate::file_dialogs::FileDialog::save().title(tr("Export known_hosts")).file_name("known_hosts");
            app.dialogs.ask(dialog, DialogFor::KnownHostsExport);
        }
    });
    ui.label(
        egui::RichText::new(tr("Servers whose host key SSHCraft trusts. Remove a key and the next connection asks again.")).color(tokens.text_dim),
    );
    ui.add_space(8.0);
    let keys = app.data.host_keys.clone().unwrap_or_default();
    if keys.is_empty() {
        ui.label(egui::RichText::new(tr("No known hosts yet.")).color(tokens.text_dim));
    }
    let mut remove = None;
    egui::Grid::new("sshcraft-known-hosts").num_columns(4).striped(true).show(ui, |ui| {
        for key in &keys {
            let at = if key.port == 22 { visible(&key.address) } else { format!("{}:{}", visible(&key.address), key.port) };
            ui.label(at);
            ui.label(
                egui::RichText::new(key.algorithm.as_deref().map(visible).unwrap_or_else(|| tr("fingerprint only").to_owned()))
                    .color(tokens.text_dim),
            );
            ui.label(egui::RichText::new(visible(&key.fingerprint)).monospace().small());
            if ui.small_button(tr("Remove")).on_hover_text(tr("The next connection asks again")).clicked() {
                remove = Some(key.clone());
            }
            ui.end_row();
        }
    });
    if let Some(key) = remove {
        app.data.host_keys = None;
        app.run_reported("hostkey.remove", json!({ "address": key.address, "port": key.port, "fingerprint": key.fingerprint }));
    }
}

/// A file chosen for the known hosts.
pub fn dialog_answer(app: &mut SshcraftApp, what: &DialogFor, path: std::path::PathBuf) {
    match what {
        DialogFor::KnownHostsImport => app.runner.run(
            "import.known_hosts",
            json!({ "path": path.to_string_lossy() }),
            Box::new(|app, result| {
                app.data.host_keys = None;
                match result {
                    Ok(v) => {
                        let count = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
                        let conflicts = v.get("conflicts").and_then(Value::as_array).map_or(0, Vec::len).to_string();
                        app.toasts.info(&trf(
                            "{added} keys added, {same} already known, {conflicts} servers kept their known key.",
                            &[("added", &count("added")), ("same", &count("same")), ("conflicts", &conflicts)],
                        ));
                    }
                    Err(e) => app.toasts.error(&e.to_string()),
                }
            }),
        ),
        DialogFor::KnownHostsExport => app.runner.run(
            "hostkey.export",
            json!({ "path": path.to_string_lossy() }),
            Box::new(|app, result| match result {
                Ok(v) => {
                    let skipped = v.get("skipped").and_then(Value::as_u64).unwrap_or(0);
                    if skipped == 0 {
                        app.toasts.info(tr("Exported."));
                    } else {
                        app.toasts
                            .info(&trf("Exported. {count} keys known only by their fingerprint were left out.", &[("count", &skipped.to_string())]));
                    }
                }
                Err(e) => app.toasts.error(&e.to_string()),
            }),
        ),
        _ => {}
    }
}

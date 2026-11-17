//! Import with a preview, and export. A source is read only after the user chose it; the preview
//! creates nothing, the import creates exactly the entries left checked, and entries with a
//! warning start unchecked. Files are chosen through craft-dialogs, which never blocks a frame.

use std::path::PathBuf;

use crate::file_dialogs::{Answer, FileDialog};
use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_import::import::{ImportCandidate, ImportWarning};
use sshcraft_model::model::GroupId;

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::pages::host_form::group_targets;
use crate::theme::Tokens;
use crate::widgets::visible;

/// Where an import reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportSourceKind {
    #[default]
    SshConfig,
    KnownHosts,
    Csv,
    PuttyRegistry,
    PuttyReg,
    MobaXterm,
    SecureCrt,
}

impl ImportSourceKind {
    const ALL: [Self; 7] = [Self::SshConfig, Self::KnownHosts, Self::Csv, Self::PuttyRegistry, Self::PuttyReg, Self::MobaXterm, Self::SecureCrt];

    fn label(self) -> &'static str {
        match self {
            Self::SshConfig => tr("~/.ssh/config"),
            Self::KnownHosts => tr("known_hosts file"),
            Self::Csv => tr("CSV file"),
            Self::PuttyRegistry => tr("PuTTY (this computer)"),
            Self::PuttyReg => tr("PuTTY registry export (.reg)"),
            Self::MobaXterm => tr("MobaXterm (MobaXterm.ini)"),
            Self::SecureCrt => tr("SecureCRT sessions folder"),
        }
    }

    /// What the source needs chosen: a file, a folder, or nothing.
    fn needs(self) -> Option<bool> {
        match self {
            Self::SshConfig | Self::PuttyRegistry => None,
            Self::SecureCrt => Some(true),
            _ => Some(false),
        }
    }

    fn source(self, path: Option<&std::path::Path>) -> Value {
        let path = path.map(|p| p.to_string_lossy().to_string());
        match self {
            Self::SshConfig => json!({ "kind": "ssh_config" }),
            Self::PuttyRegistry => json!({ "kind": "putty_registry" }),
            Self::KnownHosts => json!({ "kind": "known_hosts", "path": path }),
            Self::Csv => json!({ "kind": "csv", "path": path }),
            Self::PuttyReg => json!({ "kind": "putty_reg", "path": path }),
            Self::MobaXterm => json!({ "kind": "moba_xterm", "path": path }),
            Self::SecureCrt => json!({ "kind": "secure_crt", "dir": path }),
        }
    }
}

/// A candidate and whether it will be created.
#[derive(Debug, Clone)]
pub struct Checked {
    pub candidate: ImportCandidate,
    pub checked: bool,
}

#[derive(Debug, Clone, Default)]
pub enum ImportStep {
    #[default]
    Choose,
    Loading,
    Preview {
        candidates: Vec<Checked>,
        skipped: Vec<String>,
        host_keys: Option<usize>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ImportState {
    pub source: ImportSourceKind,
    pub path: Option<PathBuf>,
    pub step: ImportStep,
    /// Also take over the host keys of a known_hosts file.
    pub host_keys: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ExportState {
    pub json: bool,
    pub group: Option<GroupId>,
}

/// What a file dialog was opened for.
#[derive(Debug, Clone)]
pub enum DialogFor {
    ImportSource,
    Export {
        json: bool,
        group: Option<GroupId>,
    },
    /// A session log to export: `.log` writes the raw output, anything else plain text.
    LogExport {
        id: String,
    },
    /// A `known_hosts` file to read host keys from.
    KnownHostsImport,
    /// Where to write the known host keys.
    KnownHostsExport,
    /// Where a backup goes, or which one is read.
    Backup {
        restore: bool,
    },
}

#[derive(Deserialize)]
struct Preview {
    plan: Plan,
    #[serde(default)]
    host_keys: Option<usize>,
}

#[derive(Deserialize)]
struct Plan {
    candidates: Vec<ImportCandidate>,
    #[serde(default)]
    skipped: Vec<String>,
}

fn warning_text(w: &ImportWarning) -> String {
    match w {
        ImportWarning::ProxyIgnored { directive } => trf("{directive} is not followed", &[("directive", &visible(directive))]),
        ImportWarning::UnsupportedToken { token } => trf("{token} left as written", &[("token", &visible(token))]),
        ImportWarning::PasswordNotImported => tr("the stored password is not taken over").to_string(),
        ImportWarning::BadPort => tr("the port was not valid and is left empty").to_string(),
        ImportWarning::Unsupported { what } => trf("{what} is not taken over", &[("what", &visible(what))]),
    }
}

/// The answers of file dialogs since the last frame.
pub fn take_dialog_answers(app: &mut SshcraftApp) {
    for (what, answer) in app.dialogs.take_answers() {
        let path = match answer {
            Answer::Paths(paths) => paths.into_iter().next(),
            Answer::Cancelled | Answer::Message(_) => None,
            Answer::Unavailable(why) => {
                app.toasts.error(&why);
                None
            }
        };
        let Some(path) = path else {
            // A backup that was not written or read does not keep its passphrase.
            if matches!(what, DialogFor::Backup { .. }) {
                app.ui.backup_passphrase = None;
            }
            continue;
        };
        match what {
            DialogFor::KnownHostsImport | DialogFor::KnownHostsExport => crate::pages::known_hosts::dialog_answer(app, &what, path),
            DialogFor::Backup { restore } => crate::pages::backup::dialog_answer(app, restore, path),
            DialogFor::ImportSource => {
                if let Some(import) = app.ui.import.as_mut() {
                    import.path = Some(path);
                    import.step = ImportStep::Choose;
                }
            }
            DialogFor::LogExport { id } => {
                let raw = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("log"));
                let params = json!({ "id": id, "format": if raw { "raw" } else { "text" }, "path": path.to_string_lossy() });
                app.runner.run(
                    "log.export",
                    params,
                    Box::new(|app, result| match result {
                        Ok(_) => app.toasts.info(tr("Exported.")),
                        Err(e) => app.toasts.error(&e.to_string()),
                    }),
                );
            }
            DialogFor::Export { json, group } => {
                let format = if json { "json" } else { "csv" };
                let params = json!({ "format": format, "filter": { "group": group }, "path": path.to_string_lossy() });
                app.runner.run(
                    "host.export",
                    params,
                    Box::new(|app, result| match result {
                        Ok(_) => app.toasts.info(tr("Exported.")),
                        Err(e) => app.toasts.error(&e.to_string()),
                    }),
                );
            }
        }
    }
}

fn preview(app: &mut SshcraftApp) {
    let Some(import) = app.ui.import.as_mut() else { return };
    let source = import.source.source(import.path.as_deref());
    import.step = ImportStep::Loading;
    app.runner.run(
        "import.preview",
        json!({ "source": source }),
        Box::new(|app, result| {
            let Some(import) = app.ui.import.as_mut() else { return };
            match result.and_then(|v| serde_json::from_value::<Preview>(v).map_err(|e| sshcraft_engine::EngineError::bad_params("import.preview", e)))
            {
                Ok(p) => {
                    let candidates = p.plan.candidates.into_iter().map(|c| Checked { checked: c.warnings.is_empty(), candidate: c }).collect();
                    import.step = ImportStep::Preview { candidates, skipped: p.plan.skipped, host_keys: p.host_keys };
                }
                Err(e) => {
                    import.step = ImportStep::Choose;
                    app.toasts.error(&e.to_string());
                }
            }
        }),
    );
}

fn apply(app: &mut SshcraftApp) {
    let Some(import) = app.ui.import.as_ref() else { return };
    let ImportStep::Preview { candidates, .. } = &import.step else { return };
    let chosen: Vec<&ImportCandidate> = candidates.iter().filter(|c| c.checked).map(|c| &c.candidate).collect();
    let keys = (import.source == ImportSourceKind::KnownHosts && import.host_keys).then(|| import.path.clone()).flatten();
    let params = json!({ "candidates": chosen });
    let Some(generation) = app.forms.start() else { return };
    app.runner.run(
        "import.apply",
        params,
        Box::new(move |app, result| {
            let current = app.forms.done(generation);
            match result {
                Ok(outcome) => {
                    let created = outcome.get("created").and_then(Value::as_u64).unwrap_or(0).to_string();
                    app.toasts.info(&trf("{count} hosts created.", &[("count", &created)]));
                    if let Some(path) = keys {
                        app.run_reported("import.known_hosts", json!({ "path": path.to_string_lossy() }));
                    }
                    if current {
                        app.ui.import = None;
                    }
                }
                Err(e) => app.toasts.error(&e.to_string()),
            }
        }),
    );
}

pub fn show_import(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut import) = app.ui.import.take() else { return };
    let tokens = Tokens::get(ctx);
    let busy = app.forms.busy();
    let (mut keep, mut choose, mut run_preview, mut run_apply) = (true, false, false, false);
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-import")).show(ctx, |ui| {
        ui.set_width(560.0);
        crate::widgets::buttons::title(ui, tr("Import from another program"));
        ui.add_space(8.0);
        egui::ComboBox::from_id_salt("sshcraft-import-source").selected_text(import.source.label()).width(320.0).show_ui(ui, |ui| {
            for kind in ImportSourceKind::ALL {
                if ui.selectable_value(&mut import.source, kind, kind.label()).changed() {
                    import.path = None;
                    import.step = ImportStep::Choose;
                }
            }
        });
        if let Some(folder) = import.source.needs() {
            ui.horizontal(|ui| {
                choose = ui.button(if folder { tr("Choose folder") } else { tr("Choose file") }).clicked();
                let shown = import.path.as_ref().map_or_else(|| tr("Nothing chosen").to_string(), |p| visible(&p.to_string_lossy()));
                ui.label(egui::RichText::new(shown).color(tokens.text_dim));
            });
        }
        if import.source == ImportSourceKind::KnownHosts {
            ui.checkbox(&mut import.host_keys, tr("Also take over the host keys (stored keys stay)"));
        }
        ui.add_space(8.0);
        egui::ScrollArea::vertical().max_height(320.0).min_scrolled_height(320.0).auto_shrink([false, false]).show(ui, |ui| match &mut import.step {
            ImportStep::Choose => {
                ui.label(egui::RichText::new(tr("Nothing is created before Import hosts.")).color(tokens.text_dim));
            }
            ImportStep::Loading => {
                ui.spinner();
            }
            ImportStep::Preview { candidates, skipped, .. } => {
                for c in candidates.iter_mut() {
                    ui.horizontal(|ui| {
                        let name = visible(&c.candidate.name);
                        let path = c.candidate.group_path.iter().map(|g| visible(g)).collect::<Vec<_>>().join(" / ");
                        let label = if path.is_empty() { name } else { format!("{path} / {name}") };
                        ui.checkbox(&mut c.checked, label);
                        if let Some(address) = &c.candidate.settings.address {
                            ui.label(egui::RichText::new(visible(address)).color(tokens.text_dim));
                        }
                    });
                    for w in &c.candidate.warnings {
                        ui.label(egui::RichText::new(format!("    {}", warning_text(w))).color(tokens.warning));
                    }
                }
                if !skipped.is_empty() {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(trf("{count} lines skipped:", &[("count", &skipped.len().to_string())])).color(tokens.text_dim));
                    for line in skipped.iter().take(50) {
                        ui.label(egui::RichText::new(visible(line)).color(tokens.text_dim));
                    }
                }
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let ready = import.source.needs().is_none() || import.path.is_some();
            run_preview = ui.add_enabled(ready, egui::Button::new(tr("Preview"))).clicked();
            let previewed = matches!(import.step, ImportStep::Preview { .. });
            run_apply = ui.add_enabled(previewed && !busy, egui::Button::new(tr("Import hosts"))).clicked();
            if ui.button(tr("Cancel")).clicked() {
                keep = false;
            }
        });
    });
    if modal.should_close() {
        keep = false;
    }
    if choose {
        let dialog = if import.source.needs() == Some(true) { FileDialog::folder() } else { FileDialog::open() };
        app.dialogs.ask(dialog.title(tr("Import hosts")), DialogFor::ImportSource);
    }
    if keep {
        app.ui.import = Some(import);
        if run_preview {
            preview(app);
        } else if run_apply {
            apply(app);
        }
    }
}

pub fn show_export(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut export) = app.ui.export.take() else { return };
    let groups: Vec<sshcraft_model::model::Group> = app.data.groups.iter().map(|g| g.group.clone()).collect();
    let (mut keep, mut go) = (true, false);
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-export")).show(ctx, |ui| {
        ui.set_width(440.0);
        crate::widgets::buttons::title(ui, tr("Export hosts"));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.radio_value(&mut export.json, false, tr("CSV (the values each host uses)"));
            ui.radio_value(&mut export.json, true, tr("JSON (groups and inheritance)"));
        });
        let targets = group_targets(&groups, None);
        let current = targets.iter().find(|(id, _)| *id == export.group).map_or_else(|| tr("Everything").to_string(), |(_, p)| p.clone());
        egui::ComboBox::from_id_salt("sshcraft-export-group").selected_text(current).width(320.0).show_ui(ui, |ui| {
            for (id, path) in &targets {
                let label = if id.is_none() { tr("Everything").to_string() } else { path.clone() };
                ui.selectable_value(&mut export.group, *id, label);
            }
        });
        ui.label(egui::RichText::new(tr("No password, passphrase or private key is exported.")).color(Tokens::get(ui.ctx()).text_dim));
        ui.horizontal(|ui| {
            go = ui.button(tr("Export to file")).clicked();
            if ui.button(tr("Cancel")).clicked() {
                keep = false;
            }
        });
    });
    if modal.should_close() {
        keep = false;
    }
    if go {
        let (name, filter) = if export.json { ("sshcraft-hosts.json", ("JSON", "json")) } else { ("sshcraft-hosts.csv", ("CSV", "csv")) };
        let dialog = FileDialog::save().title(tr("Export hosts")).file_name(name).filter(filter.0, &[filter.1]);
        app.dialogs.ask(dialog, DialogFor::Export { json: export.json, group: export.group });
        keep = false;
    }
    if keep {
        app.ui.export = Some(export);
    }
}

//! Settings › General › Backup: an encrypted backup file written and read back. The passphrase
//! is asked first (twice for a new file) and held only until the file is chosen.

use egui::Ui;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::pages::import_export::DialogFor;

/// The passphrase dialog.
#[derive(Default, Clone)]
pub struct BackupForm {
    pub restore: bool,
    pub passphrase: Zeroizing<String>,
    pub again: Zeroizing<String>,
    pub error: Option<String>,
}

impl std::fmt::Debug for BackupForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupForm").field("restore", &self.restore).finish_non_exhaustive()
    }
}

/// The passphrase between the dialog and the chosen file; never printed.
#[derive(Clone, Default)]
pub struct HeldPassphrase(pub Zeroizing<String>);

impl std::fmt::Debug for HeldPassphrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HeldPassphrase(..)")
    }
}

/// The shortest passphrase (as the engine checks).
const MIN: usize = 12;

pub fn section(app: &mut SshcraftApp, ui: &mut Ui) {
    // A backup is a file: offered where one can be picked.
    if !crate::file_dialogs::can_pick(app) {
        return;
    }
    ui.label(egui::RichText::new(tr("Backup")).strong());
    ui.label(
        egui::RichText::new(tr("One file with your hosts, groups, identities, keys, passwords, snippets and host keys, encrypted with a passphrase. Restoring adds what is missing and changes nothing that is here."))
            .weak(),
    );
    ui.horizontal(|ui| {
        if ui.button(tr("Create backup…")).clicked() {
            app.ui.backup = Some(BackupForm::default());
        }
        if ui.button(tr("Restore from backup…")).clicked() {
            app.ui.backup = Some(BackupForm { restore: true, ..BackupForm::default() });
        }
    });
}

pub fn dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.backup.take() else { return };
    let mut keep = true;
    let mut go = false;
    let modal = crate::widgets::modal(ctx, egui::Id::unique("sshcraft-backup")).show(ctx, |ui| {
        crate::widgets::fit_dialog(ui);
        ui.set_max_width(420.0);
        crate::widgets::buttons::title(ui, if form.restore { tr("Restore from backup") } else { tr("Create backup") });
        ui.add_space(6.0);
        let label = ui.label(tr("Passphrase"));
        ui.add(egui::TextEdit::singleline(&mut *form.passphrase).password(true)).labelled_by(label.id);
        if !form.restore {
            let label = ui.label(tr("Passphrase again"));
            ui.add(egui::TextEdit::singleline(&mut *form.again).password(true)).labelled_by(label.id);
            ui.label(egui::RichText::new(tr("Keep it in your password manager: without it the file cannot be opened.")).small());
        }
        if let Some(error) = &form.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(tr("Cancel")).clicked() {
                keep = false;
            }
            if ui.button(if form.restore { tr("Choose file…") } else { tr("Save as…") }).clicked() {
                go = true;
            }
        });
    });
    if modal.should_close() {
        keep = false;
    }
    if go {
        if form.passphrase.chars().count() < MIN {
            form.error = Some(trf("The passphrase needs at least {count} characters.", &[("count", &MIN.to_string())]));
        } else if !form.restore && *form.passphrase != *form.again {
            form.error = Some(tr("The two passphrases differ.").to_owned());
        } else {
            let dialog = if form.restore {
                crate::file_dialogs::FileDialog::open().title(tr("Restore from backup"))
            } else {
                crate::file_dialogs::FileDialog::save().title(tr("Create backup")).file_name("sshcraft.backup")
            };
            app.dialogs.ask(dialog, DialogFor::Backup { restore: form.restore });
            app.ui.backup_passphrase = Some(HeldPassphrase(std::mem::take(&mut form.passphrase)));
            keep = false;
        }
    }
    if keep {
        app.ui.backup = Some(form);
    }
}

/// A file was chosen: the backup is written or read with the passphrase held for it.
pub fn dialog_answer(app: &mut SshcraftApp, restore: bool, path: std::path::PathBuf) {
    let Some(passphrase) = app.ui.backup_passphrase.take() else { return };
    let params = json!({ "path": path.to_string_lossy(), "passphrase": passphrase.0.as_str() });
    let id = if restore { "backup.restore" } else { "backup.create" };
    app.runner.run(
        id,
        params,
        Box::new(move |app, result| match result {
            Ok(v) if restore => {
                let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
                app.toasts.info(&trf(
                    "Restored: {hosts} hosts, {groups} groups, {keys} keys, {passwords} passwords, {snippets} snippets.",
                    &[
                        ("hosts", &n("hosts")),
                        ("groups", &n("groups")),
                        ("keys", &n("keys")),
                        ("passwords", &n("passwords")),
                        ("snippets", &n("snippets")),
                    ],
                ));
            }
            Ok(_) => app.toasts.info(tr("The backup is written.")),
            Err(e) => app.toasts.error(&e.to_string()),
        }),
    );
}

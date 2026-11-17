//! The trash (Settings › Trash) and the earlier versions of a host, group or snippet.

use egui::Ui;
use serde_json::{Value, json};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::theme::Tokens;
use crate::widgets::visible;

/// One entry of the trash as `trash.list` returns it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct TrashRow {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub deleted_at: i64,
}

/// The versions dialog: which item, and its versions once loaded.
#[derive(Debug, Clone, Default)]
pub struct VersionsDialog {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub versions: Option<Vec<Value>>,
}

fn when(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0).map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "host" => tr("Host"),
        "group" => tr("Group"),
        "key" => tr("Key"),
        _ => tr("Snippet"),
    }
}

fn load(app: &mut SshcraftApp) {
    if app.data.trash.is_some() || app.data.trash_loading {
        return;
    }
    app.data.trash_loading = true;
    app.runner.run(
        "trash.list",
        json!({}),
        Box::new(|app, result| {
            app.data.trash_loading = false;
            app.data.trash = Some(result.ok().and_then(|v| serde_json::from_value::<Vec<TrashRow>>(v).ok()).unwrap_or_default());
        }),
    );
}

pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    load(app);
    let days = app.data.settings.general.trash_days;
    ui.label(
        egui::RichText::new(trf(
            "Deleted hosts, groups, keys and snippets stay here for {days} days, also when another device deleted them.",
            &[("days", &days.to_string())],
        ))
        .color(tokens.text_dim),
    );
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("Keep for"));
        let mut d = days;
        if ui.add(egui::DragValue::new(&mut d).range(sshcraft_engine::settings::TRASH_DAYS).suffix(tr(" days"))).changed() {
            crate::pages::settings::save(app, move |s| s.general.trash_days = d);
        }
    });
    ui.add_space(8.0);
    let rows = app.data.trash.clone().unwrap_or_default();
    if rows.is_empty() {
        ui.label(egui::RichText::new(tr("The trash is empty.")).color(tokens.text_dim));
        return;
    }
    let mut action: Option<(&'static str, Value)> = None;
    egui::Grid::new("sshcraft-trash").num_columns(4).striped(true).show(ui, |ui| {
        for row in &rows {
            ui.label(kind_label(&row.kind));
            ui.label(visible(&row.name));
            ui.label(egui::RichText::new(when(row.deleted_at)).color(tokens.text_dim));
            ui.horizontal_wrapped(|ui| {
                if ui.small_button(tr("Restore")).clicked() {
                    action = Some(("trash.restore", json!({ "kind": row.kind, "id": row.id })));
                }
                if ui.small_button(tr("Delete for good")).clicked() {
                    action = Some(("trash.purge", json!({ "kind": row.kind, "id": row.id })));
                }
            });
            ui.end_row();
        }
    });
    ui.add_space(8.0);
    if ui.button(tr("Empty the trash")).clicked() {
        action = Some(("trash.purge", json!({})));
    }
    if let Some((id, params)) = action {
        app.data.trash = None;
        app.run_reported(id, params);
    }
}

/// Opens the earlier versions of an item.
pub fn open_versions(app: &mut SshcraftApp, kind: &str, id: &str, name: &str) {
    app.ui.versions = Some(VersionsDialog { kind: kind.to_owned(), id: id.to_owned(), name: name.to_owned(), versions: None });
    let (kind, id) = (kind.to_owned(), id.to_owned());
    app.runner.run(
        "trash.versions",
        json!({ "kind": kind, "id": id }),
        Box::new(move |app, result| {
            if let Some(dialog) = app.ui.versions.as_mut().filter(|d| d.id == id) {
                dialog.versions = Some(result.ok().and_then(|v| v.as_array().cloned()).unwrap_or_default());
            }
        }),
    );
}

pub fn versions_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(dialog) = app.ui.versions.clone() else { return };
    let mut keep = true;
    let mut restore = None;
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-versions")).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.heading(trf("Earlier versions of {name}", &[("name", &visible(&dialog.name))]));
        ui.add_space(6.0);
        match &dialog.versions {
            None => {
                ui.spinner();
            }
            Some(list) if list.is_empty() => {
                ui.label(tr("No earlier versions."));
            }
            Some(list) => {
                egui::Grid::new("sshcraft-versions-list").num_columns(3).striped(true).show(ui, |ui| {
                    for v in list {
                        ui.label(when(v.get("saved_at").and_then(Value::as_i64).unwrap_or(0)));
                        ui.label(visible(v.get("name").and_then(Value::as_str).unwrap_or_default()));
                        if ui.small_button(tr("Restore")).clicked() {
                            restore = v.get("version").and_then(Value::as_i64);
                        }
                        ui.end_row();
                    }
                });
            }
        }
        ui.add_space(8.0);
        if ui.button(tr("Close")).clicked() {
            keep = false;
        }
    });
    if modal.should_close() {
        keep = false;
    }
    if let Some(version) = restore {
        app.run_reported("trash.restore_version", json!({ "kind": dialog.kind, "id": dialog.id, "version": version }));
        keep = false;
    }
    if !keep {
        app.ui.versions = None;
    }
}

//! Session logs: recorded output (never keys), listed with host, start, duration, size and place;
//! searched (from 2 characters, at most 200 hits), played back read-only, exported as text or raw,
//! deleted. A log still recording is never deleted.

use std::collections::HashSet;

use egui::{FontId, Ui};
use serde_json::{Value, json};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::theme::Tokens;
use crate::widgets::{elided_label, visible};

/// Search hits shown at most.
pub const MAX_HITS: usize = 200;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LogFilter {
    #[default]
    All,
    /// Local shells and serial ports.
    Local,
    Host(String),
}

/// The page's own state.
#[derive(Debug, Clone, Default)]
pub struct LogsView {
    pub search: String,
    /// The query the hits are for.
    searched: String,
    pub hits: Vec<Value>,
    pub filter: LogFilter,
    pub selected: HashSet<String>,
    /// The list was fetched for this visit of the page.
    pub fresh: bool,
    /// Logs waiting for the user to confirm their deletion.
    pub confirm_delete: Option<Vec<String>>,
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn recording(log: &Value) -> bool {
    log.get("ended_at").is_none_or(Value::is_null)
}

fn when(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

fn duration(log: &Value) -> String {
    let start = log.get("started_at").and_then(Value::as_i64).unwrap_or(0);
    let Some(end) = log.get("ended_at").and_then(Value::as_i64) else { return tr("recording").to_string() };
    let secs = (end - start).max(0) / 1000;
    if secs >= 3600 {
        format!("{} h {} min", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{} min", secs / 60)
    } else {
        format!("{secs} s")
    }
}

fn size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MiB", b as f64 / f64::from(1 << 20)),
        b if b >= 1 << 10 => format!("{:.1} KiB", b as f64 / f64::from(1 << 10)),
        b => format!("{b} B"),
    }
}

fn host_label(app: &SshcraftApp, log: &Value) -> String {
    match log.get("host_id").and_then(Value::as_str) {
        Some(host) => {
            app.data.hosts.iter().find(|r| r.host.id.0.to_string() == host).map_or_else(|| tr("Deleted host").to_string(), |r| visible(&r.host.name))
        }
        None => visible(&text(log, "title")),
    }
}

/// The logs the list shows: those of the filter, and while searching only those with a hit.
fn shown(app: &SshcraftApp) -> Vec<Value> {
    let searching = app.ui.logs.searched.chars().count() >= 2;
    let hits: HashSet<String> = app.ui.logs.hits.iter().map(|h| text(h, "log_id")).collect();
    app.data
        .logs
        .iter()
        .filter(|l| match &app.ui.logs.filter {
            LogFilter::All => true,
            LogFilter::Local => l.get("host_id").is_none_or(Value::is_null),
            LogFilter::Host(h) => l.get("host_id").and_then(Value::as_str) == Some(h.as_str()),
        })
        .filter(|l| !searching || hits.contains(&text(l, "id")))
        .cloned()
        .collect()
}

/// Runs the search when the query changed (from 2 characters).
fn search(app: &mut SshcraftApp) {
    let query = app.ui.logs.search.trim().to_string();
    if query == app.ui.logs.searched {
        return;
    }
    app.ui.logs.searched.clone_from(&query);
    app.ui.logs.hits.clear();
    if query.chars().count() < 2 {
        return;
    }
    app.runner.run(
        "log.search",
        json!({ "query": query }),
        Box::new(move |app, result| {
            if app.ui.logs.searched == query
                && let Ok(Value::Array(hits)) = result
            {
                app.ui.logs.hits = hits.into_iter().take(MAX_HITS).collect();
            }
        }),
    );
}

fn delete(app: &mut SshcraftApp, ids: Vec<String>) {
    if ids.is_empty() {
        app.toasts.info(tr("A log still recording is not deleted."));
        return;
    }
    app.runner.run(
        "log.delete",
        json!({ "ids": ids }),
        Box::new(|app, result| {
            app.ui.logs.selected.clear();
            app.data.dirty |= crate::data::Refresh::LOGS;
            if let Err(e) = result {
                app.toasts.error(&e.to_string());
            }
        }),
    );
}

fn filter_box(app: &mut SshcraftApp, ui: &mut Ui) {
    let hosts: Vec<(String, String)> = app.data.hosts.iter().map(|r| (r.host.id.0.to_string(), r.host.name.clone())).collect();
    let current = match &app.ui.logs.filter {
        LogFilter::All => tr("All logs").to_string(),
        LogFilter::Local => tr("Local and serial").to_string(),
        LogFilter::Host(h) => hosts.iter().find(|(id, _)| id == h).map_or_else(|| tr("Deleted host").to_string(), |(_, n)| visible(n)),
    };
    egui::ComboBox::from_id_salt("sshcraft-logs-filter").selected_text(current).show_ui(ui, |ui| {
        ui.selectable_value(&mut app.ui.logs.filter, LogFilter::All, tr("All logs"));
        ui.selectable_value(&mut app.ui.logs.filter, LogFilter::Local, tr("Local and serial"));
        for (id, name) in &hosts {
            ui.selectable_value(&mut app.ui.logs.filter, LogFilter::Host(id.clone()), visible(name));
        }
    });
}

/// Asks before logs go: they are gone for good, on the sync server and the other devices too.
fn confirm_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(ids) = app.ui.logs.confirm_delete.clone() else { return };
    let mut close = false;
    let mut confirmed = false;
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-logs-confirm")).show(ctx, |ui| {
        ui.set_width(420.0);
        crate::widgets::buttons::title(ui, trf("Delete {count} session logs?", &[("count", &ids.len().to_string())]));
        ui.add_space(6.0);
        ui.label(tr("They are deleted for good, on the sync server and your other devices too."));
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Cancel")).clicked() {
                close = true;
            }
            if ui.button(tr("Delete")).clicked() {
                confirmed = true;
            }
        });
    });
    if modal.should_close() {
        close = true;
    }
    if confirmed {
        app.ui.logs.confirm_delete = None;
        delete(app, ids);
    } else if close {
        app.ui.logs.confirm_delete = None;
    }
}

enum LogAction {
    Open(String),
    /// Opens the log at the first match of the page's search.
    Find(String),
    Export(String),
}

pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    if !app.ui.logs.fresh {
        app.ui.logs.fresh = true;
        app.data.dirty |= crate::data::Refresh::LOGS;
    }
    search(app);
    let tokens = Tokens::get(ui.ctx());
    let list = shown(app);
    let mut action = None;
    let mut delete_ids = None;
    ui.horizontal_wrapped(|ui| {
        crate::widgets::buttons::page_title(ui, tr("Logs"));
        ui.add_space(16.0);
        ui.add(egui::TextEdit::singleline(&mut app.ui.logs.search).hint_text(tr("Search in all logs")).desired_width(240.0));
        filter_box(app, ui);
        if ui.button(tr("Delete selected")).clicked() {
            let ids: Vec<String> =
                list.iter().filter(|l| !recording(l)).map(|l| text(l, "id")).filter(|id| app.ui.logs.selected.contains(id)).collect();
            delete_ids = Some(ids);
        }
        if ui.button(tr("Delete all shown")).clicked() {
            delete_ids = Some(list.iter().filter(|l| !recording(l)).map(|l| text(l, "id")).collect());
        }
    });
    ui.add_space(6.0);
    if !app.ui.logs.hits.is_empty() {
        ui.label(egui::RichText::new(trf("{count} hits", &[("count", &app.ui.logs.hits.len().to_string())])).strong());
        for hit in app.ui.logs.hits.clone() {
            let line = text(&hit, "snippet");
            if elided_label(ui, &line, ui.available_width(), tokens.text, FontId::monospace(12.0)).clicked() {
                action = Some(LogAction::Find(text(&hit, "log_id")));
            }
        }
        ui.separator();
    }
    if list.is_empty() {
        ui.label(egui::RichText::new(tr("No session logs. Recording is off unless a host or the settings turn it on.")).color(tokens.text_dim));
    }
    // Exporting writes a file, which a tablet cannot pick.
    let tablet = app.form == crate::app::Form::Tablet;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("sshcraft-logs").num_columns(7).min_col_width(60.0).spacing([12.0, 6.0]).show(ui, |ui| {
            for log in &list {
                let id = text(log, "id");
                let mut on = app.ui.logs.selected.contains(&id);
                if ui.add_enabled(!recording(log), egui::Checkbox::without_text(&mut on)).changed() {
                    if on {
                        app.ui.logs.selected.insert(id.clone());
                    } else {
                        app.ui.logs.selected.remove(&id);
                    }
                }
                elided_label(ui, &host_label(app, log), 180.0, tokens.text, FontId::proportional(14.0));
                ui.label(when(log.get("started_at").and_then(Value::as_i64).unwrap_or(0)));
                ui.label(duration(log));
                ui.label(size(log.get("size").and_then(Value::as_u64).unwrap_or(0)));
                ui.label(if log.get("location").is_none_or(Value::is_null) { tr("This device") } else { tr("Sync server") });
                ui.horizontal_wrapped(|ui| {
                    if ui.button(tr("Open")).clicked() {
                        action = Some(LogAction::Open(id.clone()));
                    }
                    if !tablet && ui.button(tr("Export")).clicked() {
                        action = Some(LogAction::Export(id.clone()));
                    }
                });
                ui.end_row();
            }
        });
    });
    match delete_ids {
        Some(ids) if ids.is_empty() => delete(app, ids),
        Some(ids) => app.ui.logs.confirm_delete = Some(ids),
        None => {}
    }
    confirm_dialog(app, ui.ctx());
    match action {
        Some(LogAction::Open(id)) => crate::log_viewer::open(app, id, None),
        Some(LogAction::Find(id)) => {
            let query = app.ui.logs.searched.clone();
            crate::log_viewer::open(app, id, Some(query));
        }
        Some(LogAction::Export(id)) => {
            let name = app.data.logs.iter().find(|l| text(l, "id") == id).map(|l| text(l, "title")).unwrap_or_default();
            let file: String = name.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').take(60).collect();
            let dialog = crate::file_dialogs::FileDialog::save()
                .title(tr("Export the log"))
                .file_name(format!("{}.txt", if file.is_empty() { "session-log" } else { &file }))
                .filter(tr("Text"), &["txt"])
                .filter(tr("Raw output"), &["log"]);
            app.dialogs.ask(dialog, crate::pages::import_export::DialogFor::LogExport { id });
        }
        None => {}
    }
}

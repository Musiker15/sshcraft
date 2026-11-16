//! Snippets: commands or short scripts in packages, kept on this device and sealed with the vault
//! (`snippet.*`). Running one types it into open panes at once, or opens a tab with one pane per
//! new host (16 per tab) that types it once after connecting. "Run" ends with Enter, "Paste only"
//! leaves it at the prompt.

use std::collections::BTreeMap;

use egui::{FontId, Ui};
use serde_json::{Value, json};
use sshcraft_engine::SessionId;
use sshcraft_model::model::HostId;
use sshcraft_term::TermModes;

use crate::app::SshcraftApp;
use crate::data::SnippetRow;
use crate::i18n::{tr, trf};
use crate::state::{Page, Panes, TabState};
use crate::theme::Tokens;
use crate::widgets::{elided_label, visible};

/// Most characters of a snippet's content.
pub const MAX_CONTENT: usize = 10_000;
/// Most characters of a name or a package.
pub const MAX_NAME: usize = 100;

/// The bytes that type `content`: inside a bracketed paste when the shell asked for one, else
/// line by line (each line runs as it arrives, tabs become spaces). `run` ends it with Enter.
/// No `ESC` ever goes along, so the text can neither end the paste nor type a sequence.
pub fn paste_bytes(content: &str, bracketed: bool, run: bool) -> Vec<u8> {
    let text: String = content.replace("\r\n", "\n").chars().filter(|c| *c != '\x1b').collect();
    let mut out = Vec::new();
    if bracketed {
        out.extend_from_slice(b"\x1b[200~");
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
    } else {
        out.extend_from_slice(text.replace('\t', " ").replace('\n', "\r").as_bytes());
    }
    if run {
        out.push(b'\r');
    }
    out
}

/// Why a snippet's fields cannot be saved, if they cannot.
fn problem(name: &str, package: &str, content: &str) -> Option<&'static str> {
    if name.trim().is_empty() || name.chars().count() > MAX_NAME || package.chars().count() > MAX_NAME {
        return Some(tr("A name is needed; name and package have at most 100 characters."));
    }
    if content.trim().is_empty() || content.chars().count() > MAX_CONTENT {
        return Some(tr("The content has 1 to 10 000 characters."));
    }
    content
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
        .then_some(tr("The content has no control characters but line breaks and tabs."))
}

/// A snippet being written or changed.
#[derive(Debug, Clone, Default)]
pub struct SnippetForm {
    pub id: Option<String>,
    pub name: String,
    pub package: String,
    pub content: String,
    pub targets: Vec<HostId>,
}

/// Where a run types: an open pane, or a new connection to a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Open(SessionId),
    New(HostId),
}

/// The run dialog.
#[derive(Debug, Clone)]
pub struct SnippetRun {
    pub snippet: String,
    pub picks: Vec<Pick>,
    pub paste_only: bool,
    /// Default hosts that were deleted, by count (named in the dialog).
    pub missing: usize,
    /// Values for the snippet's placeholders, by name.
    pub values: BTreeMap<String, String>,
}

/// The sidebar beside the terminal: its search and the history of the focused pane.
#[derive(Debug, Clone, Default)]
pub struct Sidebar {
    pub open: bool,
    pub search: String,
    scope: Option<String>,
    history: Vec<String>,
}

fn host_of(app: &SshcraftApp, session: SessionId) -> Option<HostId> {
    app.terminals.get(&session).and_then(|t| t.host)
}

/// Every open pane with a session, by tab order.
fn open_panes(app: &SshcraftApp) -> Vec<SessionId> {
    app.ui.tabs.iter().flat_map(|t| t.panes.sessions()).collect()
}

/// Opens the run dialog for `snippet`: each default host once, its open pane if it has one.
pub fn open_run(app: &mut SshcraftApp, snippet: &str) {
    let Some(row) = app.data.snippets.iter().find(|s| s.id == snippet).cloned() else { return };
    let open = open_panes(app);
    let mut picks = Vec::new();
    let mut missing = 0;
    for host in &row.default_targets {
        if !app.data.hosts.iter().any(|r| r.host.id == *host) {
            missing += 1;
            continue;
        }
        let pick = open.iter().find(|s| host_of(app, **s) == Some(*host)).map_or(Pick::New(*host), |s| Pick::Open(*s));
        if !picks.contains(&pick) {
            picks.push(pick);
        }
    }
    let values = sshcraft_model::placeholders::find(&row.content).into_iter().map(|p| (p.name, p.default.unwrap_or_default())).collect();
    app.ui.snippet_run = Some(SnippetRun { snippet: row.id, picks, paste_only: false, missing, values });
}

/// Runs `snippet` in session `id` from the bar above its terminal: at once, or through the run
/// dialog with this pane chosen when the snippet has placeholders to fill.
pub fn run_from_bar(app: &mut SshcraftApp, snippet: &str, id: SessionId) {
    let Some(row) = app.data.snippets.iter().find(|s| s.id == snippet).cloned() else { return };
    if sshcraft_model::placeholders::find(&row.content).is_empty() {
        type_into(app, id, &row.content, true);
        return;
    }
    open_run(app, snippet);
    if let Some(job) = app.ui.snippet_run.as_mut() {
        job.picks = vec![Pick::Open(id)];
    }
}

/// Whether `snippet` is on the bar above the terminals.
pub fn pinned(app: &SshcraftApp, snippet: &str) -> bool {
    app.data.settings.terminal.snippet_bar.iter().any(|s| s == snippet)
}

/// Types `content` into session `id` (in a bracketed paste if that terminal asked for one).
pub fn type_into(app: &SshcraftApp, id: SessionId, content: &str, run: bool) {
    let bracketed = app.terminals.get(&id).is_some_and(|t| t.term.modes().contains(TermModes::BRACKETED_PASTE));
    crate::session_tab::send_bytes(app, id, &paste_bytes(content, bracketed, run));
}

fn run(app: &mut SshcraftApp, job: &SnippetRun) {
    let Some(mut row) = app.data.snippets.iter().find(|s| s.id == job.snippet).cloned() else { return };
    // The run button stays off until every placeholder has a value.
    let Ok(filled) = sshcraft_model::placeholders::fill(&row.content, &job.values) else { return };
    row.content = filled;
    let new: Vec<HostId> = job.picks.iter().filter_map(|p| if let Pick::New(h) = p { Some(*h) } else { None }).collect();
    for pick in &job.picks {
        if let Pick::Open(session) = pick {
            type_into(app, *session, &row.content, !job.paste_only);
        }
    }
    // A new connection types the snippet once, line by line: its shell may not have asked for
    // bracketed paste yet.
    let start = paste_bytes(&row.content, false, !job.paste_only);
    for hosts in new.chunks(crate::panes::MAX_PANES) {
        let slots: Vec<_> = hosts.iter().map(|_| app.new_pane()).collect();
        let Some(first) = slots.first().copied() else { continue };
        let mut panes = Panes::single(first);
        panes.panes.clone_from(&slots);
        panes.reset_sizes();
        let title = trf("Snippet: {name}", &[("name", &visible(&row.name))]);
        app.ui.tabs.push(TabState { title, panes, named: true });
        app.ui.page = Page::Tab(app.ui.tabs.len() - 1);
        for (host, slot) in hosts.iter().zip(slots) {
            let name = app.data.hosts.iter().find(|r| r.host.id == *host).map(|r| r.host.name.clone()).unwrap_or_default();
            crate::session_tab::open_with(app, json!({ "kind": "host", "host_id": host }), name, Some(*host), Some(slot), Some(start.clone()));
        }
    }
}

enum RowAction {
    Run,
    Edit,
    Delete,
    Pin,
}

pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.heading(tr("Snippets"));
        ui.add_space(16.0);
        if ui.button(tr("New snippet")).clicked() {
            app.ui.snippet_form = Some(SnippetForm::default());
        }
    });
    ui.add_space(8.0);
    if app.data.snippets.is_empty() {
        ui.label(egui::RichText::new(tr("No snippets yet. They stay on this device and need the vault.")).color(tokens.text_dim));
    }
    let mut packages: BTreeMap<String, Vec<SnippetRow>> = BTreeMap::new();
    for s in &app.data.snippets {
        packages.entry(s.package.clone()).or_default().push(s.clone());
    }
    // Snippets without a package come last.
    let mut order: Vec<String> = packages.keys().filter(|k| !k.is_empty()).cloned().collect();
    order.sort_by_key(|k| k.to_lowercase());
    if packages.contains_key("") {
        order.push(String::new());
    }
    let mut action: Option<(SnippetRow, RowAction)> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for package in order {
            let title = if package.is_empty() { tr("Without package").to_string() } else { visible(&package) };
            ui.label(egui::RichText::new(title).strong());
            let mut rows = packages.remove(&package).unwrap_or_default();
            rows.sort_by_key(|s| s.name.to_lowercase());
            for row in rows {
                ui.horizontal(|ui| {
                    elided_label(ui, &row.name, 220.0, tokens.text, FontId::proportional(14.0));
                    let first = row.content.lines().next().unwrap_or_default();
                    elided_label(ui, first, 320.0, tokens.text_dim, FontId::monospace(12.0));
                    if ui.button(tr("Run")).clicked() {
                        action = Some((row.clone(), RowAction::Run));
                    }
                    if ui.button(tr("Edit")).clicked() {
                        action = Some((row.clone(), RowAction::Edit));
                    }
                    if ui.button(tr("Delete")).clicked() {
                        action = Some((row.clone(), RowAction::Delete));
                    }
                    let mut on_bar = pinned(app, &row.id);
                    if ui.checkbox(&mut on_bar, tr("On the bar")).changed() {
                        action = Some((row.clone(), RowAction::Pin));
                    }
                });
            }
            ui.add_space(8.0);
        }
    });
    match action {
        Some((row, RowAction::Run)) => open_run(app, &row.id),
        Some((row, RowAction::Pin)) => {
            let on = pinned(app, &row.id);
            crate::pages::settings::save(app, |s| {
                if on {
                    s.terminal.snippet_bar.retain(|id| *id != row.id);
                } else if s.terminal.snippet_bar.len() < sshcraft_engine::settings::MAX_SNIPPET_BAR {
                    s.terminal.snippet_bar.push(row.id.clone());
                }
            });
        }
        Some((row, RowAction::Edit)) => {
            app.ui.snippet_form =
                Some(SnippetForm { id: Some(row.id), name: row.name, package: row.package, content: row.content, targets: row.default_targets });
        }
        Some((row, RowAction::Delete)) => crate::confirm::ask(
            app,
            crate::i18n::trf("Delete {name}?", &[("name", &visible(&row.name))]),
            tr("The snippet is deleted for good, on your other devices too."),
            tr("Delete snippet"),
            crate::confirm::Pending::DeleteSnippet { id: row.id },
        ),
        None => {}
    }
}

/// Deletes snippet `id` (after the user confirmed it).
pub fn delete(app: &mut SshcraftApp, id: String) {
    app.runner.run(
        "snippet.delete",
        json!({ "id": id }),
        Box::new(|app, result| match result {
            Ok(_) => app.data.dirty |= crate::data::Refresh::SNIPPETS,
            Err(e) => app.toasts.error(&e.to_string()),
        }),
    );
}

/// The snippet form and the run dialog.
pub fn dialogs(app: &mut SshcraftApp, ctx: &egui::Context) {
    form_dialog(app, ctx);
    run_dialog(app, ctx);
}

fn form_body(ui: &mut Ui, form: &mut SnippetForm, hosts: &[(HostId, String)]) {
    egui::Grid::new("sshcraft-snippet-form").num_columns(2).min_col_width(110.0).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label(tr("Name"));
        ui.add(egui::TextEdit::singleline(&mut form.name).char_limit(MAX_NAME));
        ui.end_row();
        ui.label(tr("Package"));
        ui.add(egui::TextEdit::singleline(&mut form.package).hint_text(tr("optional")).char_limit(MAX_NAME));
        ui.end_row();
    });
    ui.label(tr("Content"));
    egui::ScrollArea::vertical().id_salt("sshcraft-snippet-content").max_height(160.0).min_scrolled_height(160.0).auto_shrink([false, false]).show(
        ui,
        |ui| {
            ui.add(egui::TextEdit::multiline(&mut form.content).desired_rows(7).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
        },
    );
    ui.label(tr("Default hosts"));
    egui::ScrollArea::vertical().id_salt("sshcraft-snippet-hosts").max_height(90.0).min_scrolled_height(90.0).auto_shrink([false, false]).show(
        ui,
        |ui| {
            for (id, name) in hosts {
                let mut on = form.targets.contains(id);
                if ui.checkbox(&mut on, visible(name)).changed() {
                    if on {
                        form.targets.push(*id);
                    } else {
                        form.targets.retain(|t| t != id);
                    }
                }
            }
        },
    );
}

fn form_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.snippet_form.take() else { return };
    let tokens = Tokens::get(ctx);
    let busy = app.forms.busy();
    let hosts: Vec<(HostId, String)> = app.data.hosts.iter().map(|r| (r.host.id, r.host.name.clone())).collect();
    let (mut keep, mut save) = (true, false);
    let problem = problem(&form.name, &form.package, &form.content);
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-snippet")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading(if form.id.is_some() { tr("Edit snippet") } else { tr("New snippet") });
        form_body(ui, &mut form, &hosts);
        if let Some(problem) = problem {
            ui.label(egui::RichText::new(problem).color(tokens.danger));
        }
        ui.horizontal(|ui| {
            save = ui.add_enabled(problem.is_none() && !busy, egui::Button::new(tr("Save"))).clicked();
            if ui.button(tr("Cancel")).clicked() {
                keep = false;
            }
        });
    });
    if modal.should_close() {
        keep = false;
    }
    if save && let Some(generation) = app.forms.start() {
        let package = if form.package.trim().is_empty() { Value::Null } else { json!(form.package.trim()) };
        let id = form.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let snippet = json!({ "id": id, "name": form.name.trim(), "package": package, "content": form.content, "default_targets": form.targets });
        app.runner.run(
            "snippet.save",
            json!({ "snippet": snippet }),
            Box::new(move |app, result| {
                let current = app.forms.done(generation);
                match result {
                    Ok(_) => {
                        app.data.dirty |= crate::data::Refresh::SNIPPETS;
                        if current {
                            app.ui.snippet_form = None;
                        }
                    }
                    Err(e) => app.toasts.error(&e.to_string()),
                }
            }),
        );
    }
    if keep {
        app.ui.snippet_form = Some(form);
    }
}

fn run_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut job) = app.ui.snippet_run.take() else { return };
    let tokens = Tokens::get(ctx);
    let name = app.data.snippets.iter().find(|s| s.id == job.snippet).map(|s| s.name.clone()).unwrap_or_default();
    let open: Vec<(SessionId, String)> =
        open_panes(app).into_iter().map(|s| (s, app.terminals.get(&s).map(|t| t.title.clone()).unwrap_or_default())).collect();
    let hosts: Vec<(HostId, String)> = app.data.hosts.iter().map(|r| (r.host.id, r.host.name.clone())).collect();
    let (mut keep, mut go) = (true, false);
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-snippet-run")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.heading(trf("Run {name}", &[("name", &visible(&name))]));
        if job.missing > 0 {
            let count = job.missing.to_string();
            ui.label(egui::RichText::new(trf("{count} default host was deleted and is left out.", &[("count", &count)])).color(tokens.warning));
        }
        egui::ScrollArea::vertical().max_height(260.0).min_scrolled_height(260.0).auto_shrink([false, false]).show(ui, |ui| {
            if !open.is_empty() {
                ui.label(egui::RichText::new(tr("Open panes")).strong());
            }
            for (session, title) in &open {
                toggle(ui, &mut job.picks, Pick::Open(*session), &visible(title));
            }
            ui.label(egui::RichText::new(tr("New connections")).strong());
            for (host, name) in &hosts {
                toggle(ui, &mut job.picks, Pick::New(*host), &visible(name));
            }
        });
        if !job.values.is_empty() {
            ui.label(egui::RichText::new(tr("Values")).strong());
            egui::Grid::new("sshcraft-snippet-values").num_columns(2).show(ui, |ui| {
                for (name, value) in &mut job.values {
                    ui.label(visible(name));
                    ui.add(egui::TextEdit::singleline(value).desired_width(320.0));
                    ui.end_row();
                }
            });
        }
        ui.checkbox(&mut job.paste_only, tr("Paste only"));
        let content = app.data.snippets.iter().find(|s| s.id == job.snippet).map(|s| s.content.clone()).unwrap_or_default();
        // A placeholder without a default needs a value; one with a default may be left empty.
        let filled = sshcraft_model::placeholders::find(&content)
            .iter()
            .filter(|p| p.default.is_none())
            .all(|p| job.values.get(&p.name).is_some_and(|v| !v.is_empty()));
        ui.horizontal(|ui| {
            let count = job.picks.len().to_string();
            go = ui.add_enabled(!job.picks.is_empty() && filled, egui::Button::new(trf("Run on {count}", &[("count", &count)]))).clicked();
            if ui.button(tr("Cancel")).clicked() {
                keep = false;
            }
        });
    });
    if modal.should_close() {
        keep = false;
    }
    if go {
        run(app, &job);
        return;
    }
    if keep {
        app.ui.snippet_run = Some(job);
    }
}

fn toggle(ui: &mut Ui, picks: &mut Vec<Pick>, pick: Pick, label: &str) {
    let mut on = picks.contains(&pick);
    if ui.checkbox(&mut on, label).changed() {
        if on {
            picks.push(pick);
        } else {
            picks.retain(|p| *p != pick);
        }
    }
}

/// The history scope of a session's target: `host:<id>` or `local` (serial lines keep none).
fn scope(target: &Value) -> Option<String> {
    match target.get("kind").and_then(Value::as_str)? {
        "host" => target.get("host_id").and_then(Value::as_str).map(|id| format!("host:{id}")),
        "local" => Some("local".into()),
        _ => None,
    }
}

/// A line the focused pane's history just kept shows in the sidebar at once.
pub fn sidebar_recorded(app: &mut SshcraftApp, scope: &str, line: &str) {
    if app.ui.sidebar.scope.as_deref() == Some(scope) {
        app.ui.sidebar.history.retain(|h| h != line);
        app.ui.sidebar.history.insert(0, line.to_string());
    }
}

/// The sidebar forgets a cleared history (`None`: all of it).
pub fn sidebar_forget(app: &mut SshcraftApp, scope: Option<&str>) {
    if scope.is_none() || app.ui.sidebar.scope.as_deref() == scope {
        app.ui.sidebar.history.clear();
    }
}

/// Fetches the history of `scope` when the focused pane changed.
fn load_history(app: &mut SshcraftApp, want: Option<String>) {
    if app.ui.sidebar.scope == want {
        return;
    }
    app.ui.sidebar.scope.clone_from(&want);
    app.ui.sidebar.history.clear();
    let Some(scope) = want else { return };
    app.runner.run(
        "history.list",
        json!({ "scope": scope }),
        Box::new(move |app, result| {
            if app.ui.sidebar.scope.as_deref() == Some(scope.as_str())
                && let Ok(Value::Array(rows)) = result
            {
                app.ui.sidebar.history = rows.iter().filter_map(|r| r.get("command").and_then(Value::as_str)).map(str::to_string).collect();
            }
        }),
    );
}

/// The sidebar beside the terminal: snippets and the focused pane's history, one search for
/// both. A click pastes into the focused pane (into every pane when broadcast is on); the play
/// button runs a snippet.
pub fn sidebar(app: &mut SshcraftApp, ui: &mut Ui, focused: Option<SessionId>) {
    let tokens = Tokens::get(ui.ctx());
    let want = focused.and_then(|s| app.terminals.get(&s)).and_then(|t| scope(&t.target));
    load_history(app, want.clone());
    ui.add(egui::TextEdit::singleline(&mut app.ui.sidebar.search).hint_text(tr("Search")).desired_width(f32::INFINITY));
    let query = app.ui.sidebar.search.to_lowercase();
    let mut paste: Option<(String, bool)> = None;
    let mut clear = false;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.label(egui::RichText::new(tr("Snippets")).strong());
        for s in app
            .data
            .snippets
            .iter()
            .filter(|s| query.is_empty() || s.name.to_lowercase().contains(&query) || s.content.to_lowercase().contains(&query))
        {
            ui.horizontal(|ui| {
                if crate::widgets::icons::icon_button(ui, crate::widgets::icons::Icon::Terminal, tr("Run snippet")) {
                    paste = Some((s.content.clone(), true));
                }
                if elided_label(ui, &s.name, ui.available_width(), tokens.text, FontId::proportional(13.0)).clicked() {
                    paste = Some((s.content.clone(), false));
                }
            });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("History")).strong());
            clear = want.is_some() && ui.small_button(tr("Clear")).clicked();
        });
        for line in app.ui.sidebar.history.iter().filter(|l| query.is_empty() || l.to_lowercase().contains(&query)) {
            if elided_label(ui, line, ui.available_width(), tokens.text_dim, FontId::monospace(12.0)).clicked() {
                paste = Some((line.clone(), false));
            }
        }
    });
    if clear && let Some(scope) = want {
        app.run_reported("history.clear", json!({ "scope": scope }));
        crate::session_tab::forget_history(app, Some(&scope));
    }
    if let (Some((text, run)), Some(session)) = (paste, focused) {
        for target in crate::session_tab::broadcast_targets(app, session) {
            type_into(app, target, &text, run);
        }
    }
}

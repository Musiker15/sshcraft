//! Workspace templates: a tab's panes with their connections, titles, working directories and a
//! command, kept on this device (`workspace.*`). Opening one connects every pane again; over SSH
//! it types one line once after the first connect: `cd -- '<dir>' && <command>`.

use serde_json::{Value, json};
use sshcraft_engine::SessionId;

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::state::{Page, PaneSlot, Panes, TabState};
use crate::theme::Tokens;
use crate::widgets::visible;

/// The line typed once after connecting: the directory as one quoted word, then the command.
/// `posix` is false for a serial line, which gets the command alone. `None` when there is
/// nothing to type, or when either part has a line break or another control character (it
/// would end the line and run what follows).
pub fn start_line(cwd: Option<&str>, command: Option<&str>, posix: bool) -> Option<String> {
    let plain = |s: &str| !s.chars().any(char::is_control);
    let cwd = cwd.map(str::trim).filter(|c| !c.is_empty() && posix);
    let command = command.map(str::trim).filter(|c| !c.is_empty());
    if !cwd.is_none_or(plain) || !command.is_none_or(plain) {
        return None;
    }
    let cd = cwd.map(|dir| format!("cd -- '{}'", dir.replace('\'', "'\\''")));
    match (cd, command) {
        (Some(cd), Some(command)) => Some(format!("{cd} && {command}")),
        (Some(cd), None) => Some(cd),
        (None, Some(command)) => Some(command.to_string()),
        (None, None) => None,
    }
}

/// A local shell's start directory: absolute and on this computer (not a UNC share).
pub fn local_cwd(cwd: &str) -> Option<String> {
    let unc = cwd.starts_with("\\\\") || cwd.starts_with("//");
    (std::path::Path::new(cwd).is_absolute() && !unc).then(|| cwd.to_string())
}

/// One pane in the save dialog.
#[derive(Debug, Clone)]
pub struct PaneForm {
    pub session: SessionId,
    pub title: String,
    pub cwd: String,
    pub command: String,
}

/// The "Save as workspace" dialog.
#[derive(Debug, Clone, Default)]
pub struct TemplateForm {
    pub name: String,
    pub panes: Vec<PaneForm>,
    tab: usize,
}

/// Opens the save dialog for tab `tab`, filled with its panes.
pub fn open_dialog(app: &mut SshcraftApp, tab: usize) {
    let Some(t) = app.ui.tabs.get(tab) else { return };
    let panes = t
        .panes
        .sessions()
        .filter_map(|s| app.terminals.get(&s))
        .map(|p| PaneForm { session: p.id, title: p.title.clone(), cwd: p.cwd.clone().unwrap_or_default(), command: String::new() })
        .collect();
    app.ui.template_form = Some(TemplateForm { name: String::new(), panes, tab });
}

/// The template's target for a session target: a local shell's directory is kept apart.
fn template_target(target: &Value) -> Value {
    let mut target = target.clone();
    if let Some(map) = target.as_object_mut() {
        map.remove("cwd");
    }
    target
}

fn save(app: &mut SshcraftApp, form: &TemplateForm) {
    let Some(tab) = app.ui.tabs.get(form.tab) else { return };
    let panes: Vec<Value> = form
        .panes
        .iter()
        .filter_map(|p| {
            let session = app.terminals.get(&p.session)?;
            let text = |s: &str| if s.trim().is_empty() { Value::Null } else { json!(s.trim()) };
            Some(json!({ "target": template_target(&session.target), "title": p.title.trim(), "cwd": text(&p.cwd), "command": text(&p.command) }))
        })
        .collect();
    let template = json!({
        "id": uuid::Uuid::new_v4(),
        "name": form.name.trim(),
        "layout": if tab.panes.focus_layout { "focus" } else { "split" },
        "col_sizes": tab.panes.col_sizes,
        "row_sizes": tab.panes.row_sizes,
        "panes": panes,
    });
    let Some(generation) = app.forms.start() else { return };
    app.runner.run(
        "workspace.save",
        json!({ "template": template }),
        Box::new(move |app, result| {
            let current = app.forms.done(generation);
            match result {
                Ok(_) => {
                    app.data.dirty |= crate::data::Refresh::TEMPLATES;
                    if current {
                        app.ui.template_form = None;
                    }
                    app.toasts.info(tr("Workspace saved."));
                }
                Err(e) => app.toasts.error(&e.to_string()),
            }
        }),
    );
}

pub fn show_dialog(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.template_form.take() else { return };
    let busy = app.forms.busy();
    let (mut keep, mut submit) = (true, false);
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-template")).show(ctx, |ui| {
        ui.set_width(620.0);
        ui.heading(tr("Save as workspace"));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(tr("Name"));
            ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(300.0).char_limit(100));
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height(260.0).min_scrolled_height(260.0).auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("sshcraft-template-panes").num_columns(3).min_col_width(150.0).show(ui, |ui| {
                ui.label(egui::RichText::new(tr("Title")).strong());
                ui.label(egui::RichText::new(tr("Directory")).strong());
                ui.label(egui::RichText::new(tr("Command")).strong());
                ui.end_row();
                for pane in &mut form.panes {
                    ui.add(egui::TextEdit::singleline(&mut pane.title).desired_width(150.0).char_limit(100));
                    ui.add(egui::TextEdit::singleline(&mut pane.cwd).desired_width(180.0).char_limit(1000));
                    ui.add(egui::TextEdit::singleline(&mut pane.command).desired_width(200.0).char_limit(1000));
                    ui.end_row();
                }
            });
        });
        ui.label(
            egui::RichText::new(tr("Over SSH the directory and the command are typed once after the first connect (a POSIX shell)."))
                .color(Tokens::get(ui.ctx()).text_dim),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            submit = ui.add_enabled(!busy && !form.name.trim().is_empty(), egui::Button::new(tr("Save"))).clicked();
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
        save(app, &form);
    }
    if keep {
        app.ui.template_form = Some(form);
    }
}

/// Opens `template` in a new tab: every pane connects again, a pane whose host was deleted is
/// left out (a notice gives the count), and broadcast starts off.
pub fn open(app: &mut SshcraftApp, template: &Value) {
    let panes = template.get("panes").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut missing = 0;
    let mut wanted = Vec::new();
    for pane in panes.iter().take(crate::panes::MAX_PANES) {
        let target = pane.get("target").cloned().unwrap_or_default();
        let host = target.get("host_id").and_then(|h| serde_json::from_value::<sshcraft_model::model::HostId>(h.clone()).ok());
        if let Some(host) = host
            && !app.data.hosts.iter().any(|r| r.host.id == host)
        {
            missing += 1;
            continue;
        }
        wanted.push((pane.clone(), target, host));
    }
    if missing > 0 {
        let count = missing.to_string();
        app.toasts.info(&trf("{count} pane left out: its host was deleted.", &[("count", &count)]));
    }
    if wanted.is_empty() {
        return;
    }
    let slots: Vec<PaneSlot> = wanted.iter().map(|_| app.new_pane()).collect();
    let mut panes_state = Panes::single(slots[0]);
    panes_state.panes = slots.clone();
    panes_state.reset_sizes();
    let sizes = |key: &str| template.get(key).and_then(|v| serde_json::from_value::<Vec<f32>>(v.clone()).ok());
    if let (Some(cols), Some(rows)) = (sizes("col_sizes"), sizes("row_sizes"))
        && cols.len() == panes_state.col_sizes.len()
        && rows.len() == panes_state.row_sizes.len()
    {
        panes_state.col_sizes = cols;
        panes_state.row_sizes = rows;
    }
    panes_state.focus_layout = template.get("layout").and_then(Value::as_str) == Some("focus");
    panes_state.broadcast = false;
    let name = template.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
    app.ui.tabs.push(TabState { title: visible(&name), panes: panes_state });
    app.ui.page = Page::Tab(app.ui.tabs.len() - 1);
    for ((pane, mut target, host), slot) in wanted.into_iter().zip(slots) {
        let text = |key: &str| pane.get(key).and_then(Value::as_str).map(str::to_string);
        let (cwd, command, title) = (text("cwd"), text("command"), text("title").unwrap_or_default());
        let kind = target.get("kind").and_then(Value::as_str).unwrap_or_default().to_string();
        let start = match kind.as_str() {
            "local" => {
                // A local shell starts in its directory; only the command is typed.
                if let (Some(map), Some(dir)) = (target.as_object_mut(), cwd.as_deref().and_then(local_cwd)) {
                    map.insert("cwd".into(), json!(dir));
                }
                start_line(None, command.as_deref(), true)
            }
            "serial" => start_line(None, command.as_deref(), false),
            _ => start_line(cwd.as_deref(), command.as_deref(), true),
        };
        crate::session_tab::open_with(app, target, title, host, Some(slot), start);
    }
}

/// The templates on the new-tab page: open with a click, delete with the bin.
pub fn list(app: &mut SshcraftApp, ui: &mut egui::Ui) {
    if app.data.templates.is_empty() {
        return;
    }
    let tokens = Tokens::get(ui.ctx());
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tr("Workspaces")).strong());
    let mut open_it = None;
    let mut delete = None;
    for template in &app.data.templates {
        let name = template.get("name").and_then(Value::as_str).unwrap_or_default();
        ui.horizontal(|ui| {
            if crate::widgets::elided_label(ui, name, 300.0, tokens.text, egui::FontId::proportional(14.0)).clicked() {
                open_it = Some(template.clone());
            }
            if crate::widgets::icons::icon_button(ui, crate::widgets::icons::Icon::Close, tr("Delete workspace")) {
                delete = template.get("id").cloned();
            }
        });
    }
    if let Some(template) = open_it {
        open(app, &template);
    }
    if let Some(id) = delete {
        app.runner.run(
            "workspace.delete",
            json!({ "id": id }),
            Box::new(|app, result| match result {
                Ok(_) => app.data.dirty |= crate::data::Refresh::TEMPLATES,
                Err(e) => app.toasts.error(&e.to_string()),
            }),
        );
    }
}

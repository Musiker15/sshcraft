//! Port forwarding: the rules of every host on one page, started and stopped here
//! (`forward.start`, `forward.stop`), created and changed with a wizard that saves them on their
//! host (`host.save`). A rule bound to anything but loopback is marked: other machines can
//! reach it.

use egui::{FontId, Ui};
use serde_json::{Value, json};
use sshcraft_model::forward::{ForwardId, ForwardKind, ForwardRule, MAX_FORWARDS_PER_HOST};
use sshcraft_model::model::{Host, HostId};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::theme::Tokens;
use crate::widgets::{elided_label, visible};

/// The rule as an `ssh` option: `-L 127.0.0.1:8080:db:5432`, `-D 127.0.0.1:1080`.
pub fn ssh_option(rule: &ForwardRule) -> String {
    let bracket = |a: &str| if a.contains(':') && !a.starts_with('[') { format!("[{a}]") } else { a.to_string() };
    let bind = format!("{}:{}", bracket(&rule.bind_address), rule.bind_port);
    match rule.kind {
        ForwardKind::Local => format!("-L {bind}:{}:{}", bracket(&rule.target_host), rule.target_port),
        ForwardKind::Remote => format!("-R {bind}:{}:{}", bracket(&rule.target_host), rule.target_port),
        ForwardKind::Dynamic => format!("-D {bind}"),
    }
}

/// Whether `address` is reachable from this machine (or the server) only.
fn loopback(address: &str) -> bool {
    let a = address.trim_start_matches('[').trim_end_matches(']').to_lowercase();
    a == "localhost" || a == "::1" || a.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
}

/// The wizard for a new or changed rule.
#[derive(Debug, Clone)]
pub struct ForwardForm {
    pub host: Option<HostId>,
    pub rule: ForwardRule,
    /// The rule exists on its host and is being changed.
    pub editing: bool,
}

impl ForwardForm {
    fn new() -> Self {
        let rule = ForwardRule {
            id: ForwardId::new(),
            name: String::new(),
            kind: ForwardKind::Local,
            bind_address: "127.0.0.1".into(),
            bind_port: 8080,
            target_host: String::new(),
            target_port: 80,
        };
        Self { host: None, rule, editing: false }
    }
}

fn state(app: &SshcraftApp, rule: ForwardId) -> Option<&Value> {
    app.data.forwards.get(&rule)
}

fn running_value(s: &Value) -> bool {
    s.get("state").and_then(Value::as_str).is_some_and(|s| matches!(s, "starting" | "running" | "reconnecting"))
}

fn running(app: &SshcraftApp, rule: ForwardId) -> bool {
    state(app, rule).is_some_and(running_value)
}

/// What a rule is doing, in words.
fn state_text(status: Option<&Value>) -> String {
    let Some(s) = status else { return tr("Stopped").to_string() };
    let number = |key: &str| s.get(key).and_then(Value::as_u64).unwrap_or(0).to_string();
    match s.get("state").and_then(Value::as_str).unwrap_or_default() {
        "starting" => tr("Starting…").to_string(),
        "running" => {
            let mut text = trf("Running on port {port}, {count} connections", &[("port", &number("bound_port")), ("count", &number("connections"))]);
            if s.get("refused").and_then(Value::as_u64).unwrap_or(0) > 0 {
                text.push_str(&trf(", {count} refused", &[("count", &number("refused"))]));
            }
            text
        }
        "reconnecting" => {
            trf("Reconnecting (attempt {attempt}) in {seconds} s", &[("attempt", &number("attempt")), ("seconds", &number("delay_secs"))])
        }
        "failed" => {
            let detail = s.get("detail").and_then(Value::as_str).or_else(|| s.get("code").and_then(Value::as_str)).unwrap_or_default();
            trf("Failed: {detail}", &[("detail", &visible(detail))])
        }
        _ => tr("Stopped").to_string(),
    }
}

fn toggle(app: &mut SshcraftApp, host: HostId, rule: ForwardId) {
    let id = if running(app, rule) { "forward.stop" } else { "forward.start" };
    app.run_reported(id, json!({ "host_id": host, "rule_id": rule }));
}

/// Deletes `rule` of `host`, stopping it first (after the user confirmed it).
pub fn delete_rule(app: &mut SshcraftApp, host: HostId, rule: &ForwardRule) {
    if running(app, rule.id) {
        app.run_reported("forward.stop", json!({ "host_id": host, "rule_id": rule.id }));
    }
    let id = rule.id;
    save_rules(app, host, |rules| rules.retain(|r| r.id != id), false);
}

/// Saves `host` with its rules changed by `change`.
fn save_rules(app: &mut SshcraftApp, host: HostId, change: impl FnOnce(&mut Vec<ForwardRule>), then_close: bool) {
    let Some(mut stored) = app.data.hosts.iter().find(|r| r.host.id == host).map(|r| r.host.clone()) else { return };
    change(&mut stored.forwards);
    let Some(generation) = app.forms.start() else {
        app.toasts.info(tr("The last change is still being saved: try again in a moment."));
        return;
    };
    app.runner.run(
        "host.save",
        json!({ "host": stored }),
        Box::new(move |app, result| {
            let current = app.forms.done(generation);
            match result {
                Ok(_) if then_close && current => app.ui.forward_form = None,
                Ok(_) => {}
                Err(e) => app.toasts.error(&e.to_string()),
            }
        }),
    );
}

/// Asks for the state of every running rule once, when the page is first shown.
fn load_states(app: &mut SshcraftApp) {
    if app.data.forwards_loaded {
        return;
    }
    app.data.forwards_loaded = true;
    app.runner.run(
        "forward.status",
        json!({}),
        Box::new(|app, result| {
            if let Ok(Value::Array(list)) = result {
                for status in list {
                    if let Some(id) = status.get("rule_id").and_then(|v| serde_json::from_value::<ForwardId>(v.clone()).ok()) {
                        app.data.forwards.insert(id, status);
                    }
                }
            }
        }),
    );
}

enum RuleAction {
    Toggle,
    Edit,
    Delete,
}

pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    load_states(app);
    let tokens = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.heading(tr("Port forwarding"));
        ui.add_space(16.0);
        if ui.button(tr("New rule")).clicked() {
            app.ui.forward_form = Some(ForwardForm::new());
        }
    });
    ui.add_space(8.0);
    let hosts: Vec<Host> = app.data.hosts.iter().map(|r| r.host.clone()).filter(|h| !h.forwards.is_empty()).collect();
    if hosts.is_empty() {
        ui.label(egui::RichText::new(tr("No forwarding rules yet. A rule belongs to a host and starts here.")).color(tokens.text_dim));
    }
    let mut action: Option<(HostId, ForwardRule, RuleAction)> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("sshcraft-forwards").num_columns(5).min_col_width(80.0).spacing([14.0, 8.0]).show(ui, |ui| {
            for host in &hosts {
                for rule in &host.forwards {
                    let label = if rule.name.is_empty() { ssh_option(rule) } else { rule.name.clone() };
                    if elided_label(ui, &label, 200.0, tokens.text, FontId::proportional(14.0)).double_clicked() {
                        action = Some((host.id, rule.clone(), RuleAction::Toggle));
                    }
                    ui.label(egui::RichText::new(visible(&host.name)).color(tokens.text_dim));
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(visible(&ssh_option(rule))).monospace());
                        if !loopback(&rule.bind_address) {
                            ui.label(egui::RichText::new(tr("Reachable from other machines")).color(tokens.warning));
                        }
                    });
                    ui.label(state_text(state(app, rule.id)));
                    ui.horizontal(|ui| {
                        let on = running(app, rule.id);
                        if ui.button(if on { tr("Stop") } else { tr("Start") }).clicked() {
                            action = Some((host.id, rule.clone(), RuleAction::Toggle));
                        }
                        if ui.add_enabled(!on, egui::Button::new(tr("Edit"))).on_disabled_hover_text(tr("Stop the rule to change it.")).clicked() {
                            action = Some((host.id, rule.clone(), RuleAction::Edit));
                        }
                        if ui.button(tr("Delete")).clicked() {
                            action = Some((host.id, rule.clone(), RuleAction::Delete));
                        }
                    });
                    ui.end_row();
                }
            }
        });
        orphans(app, ui, &hosts);
    });
    if let Some((host, rule, what)) = action {
        match what {
            RuleAction::Toggle => toggle(app, host, rule.id),
            RuleAction::Edit => app.ui.forward_form = Some(ForwardForm { host: Some(host), rule, editing: true }),
            RuleAction::Delete => crate::confirm::ask(
                app,
                crate::i18n::trf("Delete the rule {name}?", &[("name", &visible(&rule.name))]),
                tr("A running rule stops; the rule is gone from its host."),
                tr("Delete rule"),
                crate::confirm::Pending::DeleteRule { host, rule },
            ),
        }
    }
    wizard(app, ui.ctx());
}

/// Forwards that run without a rule (it was removed meanwhile): they can still be stopped.
fn orphans(app: &mut SshcraftApp, ui: &mut Ui, hosts: &[Host]) {
    let known: Vec<ForwardId> = hosts.iter().flat_map(|h| h.forwards.iter().map(|r| r.id)).collect();
    let found: Vec<(ForwardId, HostId)> = app
        .data
        .forwards
        .iter()
        .filter(|(id, s)| !known.contains(id) && running_value(s))
        .filter_map(|(id, s)| s.get("host_id").and_then(|h| serde_json::from_value(h.clone()).ok()).map(|h| (*id, h)))
        .collect();
    if found.is_empty() {
        return;
    }
    ui.add_space(12.0);
    ui.label(egui::RichText::new(tr("Without a rule")).strong());
    for (id, host) in found {
        ui.horizontal(|ui| {
            ui.label(state_text(state(app, id)));
            if ui.button(tr("Stop")).clicked() {
                app.run_reported("forward.stop", json!({ "host_id": host, "rule_id": id }));
            }
        });
    }
}

/// Why `form` cannot be saved, if it cannot.
fn problem(app: &SshcraftApp, form: &ForwardForm) -> Option<String> {
    let r = &form.rule;
    let Some(host) = form.host.and_then(|h| app.data.hosts.iter().find(|row| row.host.id == h)) else {
        return Some(tr("Choose a host.").to_string());
    };
    if !form.editing && host.host.forwards.len() >= MAX_FORWARDS_PER_HOST {
        return Some(tr("A host has at most 50 rules.").to_string());
    }
    let address_ok = |a: &str| !a.is_empty() && a.chars().count() <= 253 && !a.chars().any(|c| c.is_whitespace() || c.is_control());
    if !address_ok(&r.bind_address) {
        return Some(tr("The address has no spaces and at most 253 characters.").to_string());
    }
    if r.bind_port == 0 || (r.kind != ForwardKind::Dynamic && (r.target_port == 0 || !address_ok(&r.target_host))) {
        return Some(tr("Ports are 1 to 65535, and the target needs an address.").to_string());
    }
    (r.name.chars().count() > 100).then(|| tr("A name has at most 100 characters.").to_string())
}

fn form_grid(ui: &mut Ui, form: &mut ForwardForm, hosts: &[(HostId, String)]) {
    egui::Grid::new("sshcraft-forward-form").num_columns(2).min_col_width(130.0).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label(tr("Kind"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut form.rule.kind, ForwardKind::Local, tr("Local (-L)"));
            ui.radio_value(&mut form.rule.kind, ForwardKind::Remote, tr("Remote (-R)"));
            ui.radio_value(&mut form.rule.kind, ForwardKind::Dynamic, tr("Dynamic, SOCKS (-D)"));
        });
        ui.end_row();
        ui.label(tr("Host"));
        let chosen =
            form.host.and_then(|h| hosts.iter().find(|(id, _)| *id == h)).map_or_else(|| tr("Choose a host").to_string(), |(_, n)| visible(n));
        ui.add_enabled_ui(!form.editing, |ui| {
            egui::ComboBox::from_id_salt("sshcraft-forward-host").selected_text(chosen).show_ui(ui, |ui| {
                for (id, name) in hosts {
                    ui.selectable_value(&mut form.host, Some(*id), visible(name));
                }
            });
        });
        ui.end_row();
        ui.label(tr("Name"));
        ui.add(egui::TextEdit::singleline(&mut form.rule.name).hint_text(tr("optional")).char_limit(100));
        ui.end_row();
        ui.label(if form.rule.kind == ForwardKind::Remote { tr("Address on the server") } else { tr("Address here") });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut form.rule.bind_address).desired_width(200.0).char_limit(253));
            ui.label(tr("Port"));
            ui.add(egui::DragValue::new(&mut form.rule.bind_port).range(1..=65535));
        });
        ui.end_row();
        if form.rule.kind != ForwardKind::Dynamic {
            ui.label(if form.rule.kind == ForwardKind::Remote { tr("Target here") } else { tr("Target from the server") });
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut form.rule.target_host).desired_width(200.0).char_limit(253));
                ui.label(tr("Port"));
                ui.add(egui::DragValue::new(&mut form.rule.target_port).range(1..=65535));
            });
            ui.end_row();
        }
    });
}

fn wizard(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut form) = app.ui.forward_form.take() else { return };
    let tokens = Tokens::get(ctx);
    let busy = app.forms.busy();
    let hosts: Vec<(HostId, String)> = app.data.hosts.iter().map(|r| (r.host.id, r.host.name.clone())).collect();
    let problem = problem(app, &form);
    let (mut keep, mut save) = (true, false);
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-forward")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading(if form.editing { tr("Edit rule") } else { tr("New rule") });
        ui.add_space(6.0);
        let before = form.rule.kind;
        form_grid(ui, &mut form, &hosts);
        // A new kind brings its own default address.
        if form.rule.kind != before {
            form.rule.bind_address = if form.rule.kind == ForwardKind::Remote { "localhost" } else { "127.0.0.1" }.into();
            if form.rule.kind == ForwardKind::Dynamic {
                form.rule.target_host.clear();
                form.rule.target_port = 0;
            } else if form.rule.target_port == 0 {
                form.rule.target_port = 80;
            }
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(visible(&ssh_option(&form.rule))).monospace().color(tokens.text_strong));
        if !loopback(&form.rule.bind_address) {
            let text = trf("Other machines can reach this port through {address}.", &[("address", &visible(&form.rule.bind_address))]);
            ui.label(egui::RichText::new(text).color(tokens.warning));
        }
        if form.rule.kind == ForwardKind::Dynamic {
            ui.label(egui::RichText::new(tr("The SOCKS proxy asks no password: anyone who reaches the port can use it.")).color(tokens.text_dim));
        }
        if let Some(problem) = &problem {
            ui.label(egui::RichText::new(problem).color(tokens.danger));
        }
        ui.add_space(6.0);
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
    if save && let Some(host) = form.host {
        let rule = form.rule.clone();
        save_rules(
            app,
            host,
            move |rules| match rules.iter_mut().find(|r| r.id == rule.id) {
                Some(existing) => *existing = rule,
                None => rules.push(rule),
            },
            true,
        );
    }
    if keep {
        app.ui.forward_form = Some(form);
    }
}

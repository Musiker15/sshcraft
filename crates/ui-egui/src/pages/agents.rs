//! Settings > AI agents: the right of agents per category (allow, ask, block), the hosts and
//! groups hidden from them, and the log of what they did. None of this is reachable for an
//! agent: the engine keeps these commands to the window.

use egui::Ui;
use serde_json::{Value, json};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::theme::Tokens;
use crate::widgets::visible;

/// The categories in the order the page lists them: id and English label.
pub const CATEGORIES: [(&str, &str); 11] = [
    ("list", "View hosts, groups and key names"),
    ("connect", "Connect and run commands"),
    ("edit", "Create and change hosts and groups"),
    ("delete", "Delete"),
    ("keys", "Import and generate keys"),
    ("share", "Share"),
    ("sync", "Sync servers and devices"),
    ("forward", "Port forwarding"),
    ("logs", "Session logs"),
    ("workspaces", "Workspaces"),
    ("snippets", "Snippets"),
];

/// The level a category has without a stored one.
fn default_level(category: &str) -> &'static str {
    match category {
        "list" => "allow",
        "connect" | "edit" | "keys" => "ask",
        _ => "deny",
    }
}

#[derive(Debug, Clone, Default)]
pub struct AgentsView {
    pub policy: Option<Value>,
    pub log: Option<Vec<Value>>,
    /// Requests still on their way.
    loading: u8,
    /// Why the rights could not be read.
    failed: Option<String>,
    /// The port as it is being typed.
    port_text: Option<String>,
}

/// Fetches the policy and the log again.
pub fn reload(app: &mut SshcraftApp) {
    app.ui.agents.loading = 2;
    app.ui.agents.failed = None;
    app.runner.run(
        "agent.policy",
        json!({}),
        Box::new(|app, result| {
            app.ui.agents.loading = app.ui.agents.loading.saturating_sub(1);
            match result {
                Ok(policy) => app.ui.agents.policy = Some(policy),
                Err(e) => app.ui.agents.failed = Some(e.to_string()),
            }
        }),
    );
    app.runner.run(
        "agent.log",
        json!({}),
        Box::new(|app, result| {
            app.ui.agents.loading = app.ui.agents.loading.saturating_sub(1);
            // A log that cannot be read shows as empty rather than asking again every frame.
            app.ui.agents.log = Some(match result {
                Ok(Value::Array(log)) => log,
                _ => Vec::new(),
            });
        }),
    );
}

fn outcome(text: &str) -> &'static str {
    match text {
        "allowed" => tr("allowed"),
        "asked_allowed" => tr("asked and allowed"),
        "asked_denied" => tr("asked and denied"),
        "denied" => tr("blocked"),
        "hidden" => tr("hidden"),
        "failed" => tr("failed"),
        _ => tr("cancelled"),
    }
}

fn name_of(app: &SshcraftApp, id: &str) -> String {
    app.data
        .hosts
        .iter()
        .find(|r| r.host.id.0.to_string() == id)
        .map(|r| r.host.name.clone())
        .or_else(|| app.data.groups.iter().find(|r| r.group.id.0.to_string() == id).map(|r| r.group.name.clone()))
        .map_or_else(|| tr("Deleted host").to_string(), |n| visible(&n))
}

pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    let wanted = app.ui.agents.policy.is_none() || app.ui.agents.log.is_none();
    if wanted && app.ui.agents.loading == 0 && app.ui.agents.failed.is_none() {
        reload(app);
    }
    let tokens = Tokens::get(ui.ctx());
    if let Some(problem) = app.ui.agents.failed.clone() {
        ui.label(egui::RichText::new(visible(&problem)).color(tokens.danger));
        if ui.button(tr("Try again")).clicked() {
            reload(app);
        }
        return;
    }
    let Some(policy) = app.ui.agents.policy.clone() else {
        ui.spinner();
        return;
    };
    connection(app, ui, &tokens);
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(tr(
            "What AI agents may do through MCP or the command line. \"Ask\" asks in this window; with no window, the answer is no.",
        ))
        .color(tokens.text_dim),
    );
    ui.add_space(6.0);
    let mut set = None;
    egui::Grid::new("sshcraft-agent-levels").num_columns(2).min_col_width(220.0).spacing([16.0, 6.0]).show(ui, |ui| {
        for (category, label) in CATEGORIES {
            ui.label(tr(label));
            let current = policy.pointer(&format!("/levels/{category}")).and_then(Value::as_str).unwrap_or(default_level(category)).to_string();
            ui.horizontal(|ui| {
                for (level, text) in [("allow", tr("Allow")), ("ask", tr("Ask")), ("deny", tr("Block"))] {
                    if ui.radio(current == level, text).clicked() && current != level {
                        set = Some((category, level));
                    }
                }
            });
            ui.end_row();
        }
    });
    if let Some((category, level)) = set {
        app.runner.run(
            "agent.set_level",
            json!({ "category": category, "level": level }),
            Box::new(|app, result| {
                if let Err(e) = result {
                    app.toasts.error(&e.to_string());
                }
                reload(app);
            }),
        );
    }
    ui.add_space(12.0);
    ui.label(egui::RichText::new(tr("Hidden from agents")).strong());
    let hidden: Vec<String> =
        policy.get("hidden").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    if hidden.is_empty() {
        ui.label(egui::RichText::new(tr("Nothing. Hide a host or group from its context menu.")).color(tokens.text_dim));
    }
    let mut show_again = None;
    for id in &hidden {
        ui.horizontal(|ui| {
            ui.label(name_of(app, id));
            if ui.button(tr("Show again")).clicked() {
                show_again = Some(id.clone());
            }
        });
    }
    if let Some(id) = show_again {
        set_hidden(app, &id, false);
    }
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("Log")).strong());
        if ui.button(tr("Clear the log")).clicked() {
            app.runner.run(
                "agent.log_clear",
                json!({}),
                Box::new(|app, result| {
                    if let Err(e) = result {
                        app.toasts.error(&e.to_string());
                    }
                    reload(app);
                }),
            );
        }
    });
    for entry in app.ui.agents.log.clone().unwrap_or_default().iter().take(200) {
        let at = entry
            .get("at")
            .and_then(Value::as_i64)
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let text = |k: &str| visible(entry.get(k).and_then(Value::as_str).unwrap_or_default());
        let line = format!(
            "{at}  {}  {}  {}  {}",
            text("agent"),
            text("action"),
            text("target"),
            outcome(entry.get("outcome").and_then(Value::as_str).unwrap_or_default())
        );
        crate::widgets::elided_label(ui, &line, ui.available_width(), tokens.text, egui::FontId::monospace(12.0));
    }
}

/// The switch that lets agents connect, the port, and where they connect now.
fn connection(app: &mut SshcraftApp, ui: &mut Ui, tokens: &Tokens) {
    ui.label(egui::RichText::new(tr("Connection")).strong());
    let mut integrations = app.data.settings.integrations.clone();
    let mut changed = ui.checkbox(&mut integrations.allow_agent_control, tr("Allow agent control")).changed();
    // Typed, and taken when the field is left: every change restarts the link and cuts agents off.
    ui.horizontal(|ui| {
        let label = ui.label(tr("Port (0: any free port)"));
        let current = integrations.agent_port;
        let text = app.ui.agents.port_text.get_or_insert_with(|| current.to_string());
        let field = ui.add(egui::TextEdit::singleline(text).desired_width(70.0).char_limit(5)).labelled_by(label.id);
        if field.lost_focus() {
            match text.trim().parse::<u16>() {
                Ok(port) if port != current => {
                    integrations.agent_port = port;
                    changed = true;
                }
                Ok(_) => {}
                Err(_) => app.toasts.error(tr("The port is a number from 0 to 65535.")),
            }
        }
        if !field.has_focus() {
            app.ui.agents.port_text = None;
        }
    });
    if changed {
        crate::pages::settings::save(app, |s| s.integrations = integrations.clone());
        crate::link::check_soon(app);
    }
    let status = app.session.agent_link_status();
    let consent = app.link.consent;
    let (text, color) = match (status.port, &app.link.error, consent) {
        (Some(port), _, _) => (trf("Agents can connect on 127.0.0.1:{port}.", &[("port", &port.to_string())]), tokens.success),
        (None, Some(error), _) => (trf("Agents cannot connect: {error}", &[("error", &visible(error))]), tokens.danger),
        (None, None, Some(c)) if c.allow_setting && c.mcp_flag && !c.launcher_allows => {
            (tr("Turned off in the ArtCraft Launcher (Settings › AI agents).").to_string(), tokens.warning)
        }
        _ => (tr("Agents cannot connect.").to_string(), tokens.text_dim),
    };
    ui.label(egui::RichText::new(text).color(color));
    ui.horizontal(|ui| {
        let line = "claude mcp add sshcraft -- sshcraft-cli mcp";
        ui.label(egui::RichText::new(line).monospace());
        if ui.button(tr("Copy")).clicked() {
            ui.ctx().copy_text(line.to_string());
        }
    });
}

/// Hides host or group `id` from agents, or shows it again.
pub fn set_hidden(app: &mut SshcraftApp, id: &str, hidden: bool) {
    app.runner.run(
        "agent.set_hidden",
        json!({ "id": id, "hidden": hidden }),
        Box::new(|app, result| {
            if let Err(e) = result {
                app.toasts.error(&e.to_string());
            }
            reload(app);
        }),
    );
}

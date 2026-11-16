//! Settings › Connections: every connection SSHCraft makes, or offers, by itself.

use egui::Ui;
use serde_json::{Value, json};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::theme::Tokens;
use crate::widgets::visible;

fn load(app: &mut SshcraftApp) {
    if app.data.connections.is_some() || app.data.connections_loading {
        return;
    }
    app.data.connections_loading = true;
    app.runner.run(
        "app.connections",
        json!({}),
        Box::new(|app, result| {
            app.data.connections_loading = false;
            app.data.connections = Some(result.ok().and_then(|v| v.as_array().cloned()).unwrap_or_default());
        }),
    );
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

/// What a connection is for, in the interface's language.
fn purpose(v: &Value) -> String {
    let name = visible(&text(v, "name"));
    let count = v.get("count").and_then(Value::as_u64).unwrap_or(0).to_string();
    match text(v, "kind").as_str() {
        "sync" => trf("Sync with {name}: HTTPS, and a WebSocket for changes", &[("name", &name)]),
        "password_manager" => trf("{name}: passwords of {count} hosts or groups, fetched when connecting", &[("name", &name), ("count", &count)]),
        "wake_on_lan" => trf("Wake-on-LAN before connecting to {count} hosts or groups", &[("count", &count)]),
        "port_knock" => trf("Port knocking before connecting to {count} hosts or groups", &[("count", &count)]),
        "connect_program" => trf("Local programs before or after connecting to {count} hosts or groups, once allowed", &[("count", &count)]),
        "reachability" => tr("Whether the hosts in the list answer, every minute while it is shown").to_owned(),
        "ssh_agent" => tr("Keys from the SSH agent (Proton Pass), when a host uses one").to_owned(),
        "agent_port" => tr("AI agents connect here, from this computer only").to_owned(),
        "update_check" => tr("SSHCraft does not check for updates and sends no telemetry").to_owned(),
        _ => String::new(),
    }
}

pub fn show(app: &mut SshcraftApp, ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    load(app);
    ui.label(
        egui::RichText::new(tr(
            "Everything SSHCraft connects to or starts by itself. Your own SSH connections, port forwards and proxy commands are not listed.",
        ))
        .color(tokens.text_dim),
    );
    ui.add_space(8.0);
    let rows = app.data.connections.clone().unwrap_or_default();
    egui::Grid::new("sshcraft-connections").num_columns(3).striped(true).show(ui, |ui| {
        for row in &rows {
            let active = row.get("active").and_then(Value::as_bool).unwrap_or(false);
            ui.label(egui::RichText::new(if active { tr("on") } else { tr("off") }).color(if active { tokens.text } else { tokens.text_dim }));
            ui.label(egui::RichText::new(visible(&text(row, "target"))).monospace());
            ui.label(purpose(row));
            ui.end_row();
        }
    });
}

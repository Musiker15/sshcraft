//! The paste history (in memory only) and the compose field under the terminal.

use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers};
use egui_kittest::Harness;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{SessionId, SessionStatus};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::state::ConnectRequest;
use sshcraft_ui_egui::terminal::compose::{MAX_PASTE_BYTES, MAX_PASTES, PasteHistory};
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

#[test]
fn the_history_keeps_the_newest_pastes_once_each() {
    let mut history = PasteHistory::default();
    for i in 0..25 {
        history.push(&format!("paste {i}"));
    }
    assert_eq!(history.entries().len(), MAX_PASTES);
    assert_eq!(history.entries()[0], "paste 24");
    history.push("paste 10");
    assert_eq!(history.entries()[0], "paste 10");
    assert_eq!(history.entries().iter().filter(|e| *e == "paste 10").count(), 1);
    history.push(&"x".repeat(MAX_PASTE_BYTES + 1));
    assert_eq!(history.entries()[0], "paste 10", "a paste too large is not kept");
    history.push("   ");
    assert_eq!(history.entries()[0], "paste 10", "blank pastes are not kept");
}

fn until(h: &mut H, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn tab(h: &mut H) -> (SessionId, TestServer) {
    let session = h.state().session.clone();
    let (server, host) = session.runtime().block_on(test_host(&session, "compose", TestAuth::default(), true));
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the tab", |app| !app.terminals.is_empty());
    let id = *h.state().terminals.keys().next().unwrap();
    until(h, "connected", |app| app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. }))));
    (id, server)
}

#[test]
fn a_paste_lands_in_the_history() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (_id, _server) = tab(&mut h);
    h.input_mut().events.push(Event::Paste("kubectl get pods".into()));
    h.run_steps(2);
    assert_eq!(h.state().pastes.entries().front().map(String::as_str), Some("kubectl get pods"));
}

#[test]
fn the_compose_field_sends_its_text_with_ctrl_enter() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server) = tab(&mut h);
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.terminal.compose");
    h.run_steps(2);
    assert!(h.state().terminals[&id].compose_text().is_some());
    *h.state_mut().terminals.get_mut(&id).unwrap().compose_text_mut().unwrap() = "echo composed-line".into();
    h.run_steps(1);
    h.input_mut().events.push(Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
    h.run_steps(2);
    until(&mut h, "the echo", |app| app.terminals[&id].term.visible_text().contains("echo composed-line"));
    assert_eq!(h.state().terminals[&id].compose_text().map(String::as_str), Some(""), "sent text is cleared");
}

#[test]
fn the_resource_bar_asks_the_server_only_when_switched_on() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (id, server) = tab(&mut h);
    server.set_exec_output("0.25 0 0 1/1 1\n@@SSHCRAFT@@\nMemTotal: 1000 kB\nMemAvailable: 500 kB\n@@SSHCRAFT@@\n");
    h.run_steps(5);
    assert!(!server.received_commands().contains(&sshcraft_model::resources::PROBE.to_owned()), "off by default");
    let mut settings = session.runtime().block_on(session.execute(sshcraft_engine::Caller::User, "settings.get", serde_json::json!({}))).unwrap();
    settings["terminal"]["resource_bar"] = serde_json::json!(true);
    session.runtime().block_on(session.execute(sshcraft_engine::Caller::User, "settings.save", serde_json::json!({ "settings": settings }))).unwrap();
    until(&mut h, "the resources", |app| app.terminals.get(&id).is_some_and(|t| t.resources.is_some()));
    let r = h.state().terminals[&id].resources.clone().unwrap();
    assert_eq!(sshcraft_ui_egui::session_tab::resources_text(&r), "load 0.25  ·  memory 50 % / 0.0 GiB");
}

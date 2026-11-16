//! tmux in control mode: windows as tabs, panes as terminals, typing as `send-keys`.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use serde_json::json;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{Caller, SessionId};
use sshcraft_ssh::testing::TestAuth;
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::state::{ConnectRequest, PaneSlot};
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn until(h: &mut H, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
}

/// What tmux sends after `tmux -CC new-session`: a window with one pane, its name and output.
const CONTROL: &str = "\x1bP1000p%begin 1 1 0\n%end 1 1 0\n%window-add @1\n%layout-change @1 b25f,80x24,0,0,1 b25f,80x24,0,0,1 *\n%window-renamed @1 editor\n%output %1 hello-from-tmux\\015\\012\n";

fn pane_of(app: &SshcraftApp) -> Option<SessionId> {
    app.terminals.values().find(|t| t.tmux_pane.is_some()).map(|t| t.id)
}

#[test]
fn tmux_windows_become_tabs_and_typing_goes_as_keys() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (server, host) = session.runtime().block_on(test_host(&session, "tmux", TestAuth::default(), true));
    server.set_exec_output(CONTROL);
    server.set_exec_keep_open(true);
    // tmux answers every command, in order.
    server.set_exec_replies(vec![
        (
            "list-windows".to_owned(),
            "%begin 2 2 1
@1 b25f,80x24,0,0,1 editor
%end 2 2 1
"
            .to_owned(),
        ),
        (
            "capture-pane".to_owned(),
            "%begin 3 3 1
%end 3 3 1
"
            .to_owned(),
        ),
    ]);
    let mut row = session.runtime().block_on(session.execute(Caller::User, "host.list", json!({}))).unwrap()[0].clone();
    row["settings"]["persistence"] = json!({ "tool": "tmux_control", "name": "work" });
    session.runtime().block_on(session.execute(Caller::User, "host.save", json!({ "host": row }))).unwrap();
    until(&mut h, "the host", |app| app.data.hosts.iter().any(|r| r.host.settings.persistence.is_some()));
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(&mut h, "the pane", |app| pane_of(app).is_some_and(|p| app.terminals[&p].term.screen().text().contains("hello-from-tmux")));
    assert!(server.received_commands().contains(&"tmux -CC new-session -A -s work".to_owned()));
    let pane = pane_of(h.state()).unwrap();
    let tab = h.state().ui.tabs.iter().find(|t| t.panes.panes.contains(&PaneSlot::Session { session: pane })).expect("a tab for the window");
    assert_eq!(tab.title, "editor");
    // The control session itself is in no tab.
    let control = h.state().terminals.values().find(|t| t.tmux.is_some()).map(|t| t.id).unwrap();
    assert!(!h.state().ui.tabs.iter().any(|t| t.panes.sessions().any(|s| s == control)));
    sshcraft_ui_egui::session_tab::send_bytes(h.state(), pane, b"ls\r");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !String::from_utf8_lossy(&server.received_exec_input()).contains("send-keys -t %1 -H 6c 73 0d") {
        assert!(Instant::now() < deadline, "no send-keys: {}", String::from_utf8_lossy(&server.received_exec_input()));
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    // Closing the last tmux tab closes the control session: tmux detaches, the session stays.
    let index = h.state().ui.tabs.iter().position(|t| t.panes.panes.contains(&PaneSlot::Session { session: pane })).unwrap();
    h.state_mut().close_tab_at(index);
    h.run_steps(2);
    assert!(h.state().terminals.values().all(|t| t.tmux.is_none() && t.tmux_pane.is_none()));
}

/// A host running tmux in control mode against `server`, opened in a tab.
fn open_control(
    h: &mut H,
    session: &sshcraft_engine::Session,
    script: &str,
    replies: Vec<(String, String)>,
    keep_open: bool,
) -> sshcraft_ssh::testing::TestServer {
    let (server, host) = session.runtime().block_on(test_host(session, "tmux", TestAuth::default(), true));
    server.set_exec_output(script);
    server.set_exec_keep_open(keep_open);
    server.set_exec_replies(replies);
    let mut row = session.runtime().block_on(session.execute(Caller::User, "host.list", json!({}))).unwrap()[0].clone();
    row["settings"]["persistence"] = json!({ "tool": "tmux_control", "name": "work" });
    session.runtime().block_on(session.execute(Caller::User, "host.save", json!({ "host": row }))).unwrap();
    until(h, "the host", |app| app.data.hosts.iter().any(|r| r.host.settings.persistence.is_some()));
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    server
}

#[test]
fn attaching_to_a_running_session_lists_its_windows_and_history_comes_first() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let replies = vec![
        ("list-windows".to_owned(), "%begin 2 2 1\n@3 b25f,80x24,0,0,4 shell\n%end 2 2 1\n".to_owned()),
        ("capture-pane".to_owned(), "%begin 3 3 1\nold line\n%end 3 3 1\n".to_owned()),
    ];
    // Only tmux's answer to its own start: no %window-add for a session that is already there.
    let _server = open_control(&mut h, &session, "\x1bP1000p%begin 1 1 0\n%end 1 1 0\n%output %4 new-line\\015\\012\n", replies, true);
    until(&mut h, "the window", |app| app.ui.tabs.iter().any(|t| t.title == "shell"));
    until(&mut h, "the history", |app| pane_of(app).is_some_and(|p| app.terminals[&p].term.screen().text().contains("new-line")));
    let text = h.state().terminals[&pane_of(h.state()).unwrap()].term.screen().text();
    let (old, new) = (text.find("old line").expect("history"), text.find("new-line").unwrap());
    assert!(old < new, "the history before what came while it was on its way: {text}");
}

#[test]
fn a_pane_that_left_the_layout_goes_and_the_last_tab_detaches() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let script = "\x1bP1000p%begin 1 1 0\n%end 1 1 0\n%window-add @1\n%layout-change @1 a,80x24,0,0{40x24,0,0,1,39x24,41,0,2} a *\n%layout-change @1 b,80x24,0,0,1 b *\n";
    let _server = open_control(&mut h, &session, script, Vec::new(), true);
    until(&mut h, "one pane left", |app| {
        app.terminals.values().filter(|t| t.tmux_pane.is_some()).count() == 1
            && app.ui.tabs.iter().any(|t| t.panes.panes.len() == 1 && t.title.starts_with("tmux"))
    });
    let pane = pane_of(h.state()).unwrap();
    let index = h.state().ui.tabs.iter().position(|t| t.panes.panes.contains(&PaneSlot::Session { session: pane })).unwrap();
    h.state_mut().close_tab_at(index);
    h.run_steps(2);
    assert!(h.state().terminals.values().all(|t| t.tmux.is_none() && t.tmux_pane.is_none()), "nothing of tmux is left");
}

#[test]
fn tmux_missing_on_the_server_is_shown() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let _server = open_control(&mut h, &session, "bash: tmux: command not found\r\n", Vec::new(), false);
    until(&mut h, "the message", |app| {
        app.terminals.values().any(|t| t.tmux.is_some() && t.term.screen().text().contains("tmux: command not found"))
    });
}

#[test]
fn a_telnet_host_never_runs_in_control_mode() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let id = sshcraft_model::model::HostId(uuid::Uuid::new_v4());
    let host = json!({ "id": id, "name": "switch", "settings": { "address": "10.0.0.2", "protocol": "telnet", "persistence": { "tool": "tmux_control", "name": "x" } } });
    session.runtime().block_on(session.execute(Caller::User, "host.save", json!({ "host": host }))).unwrap();
    until(&mut h, "the host", |app| app.data.hosts.iter().any(|r| r.host.id == id));
    assert!(!sshcraft_ui_egui::pages::hosts::uses_tmux_control(h.state(), id));
}

#[test]
fn a_pane_leaves_terminal_queries_to_tmux() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let replies = vec![
        ("list-windows".to_owned(), "%begin 2 2 1\n@1 b25f,80x24,0,0,1 shell\n%end 2 2 1\n".to_owned()),
        ("capture-pane".to_owned(), "%begin 3 3 1\n%end 3 3 1\n".to_owned()),
    ];
    // A program asks for the device attributes (ESC [ c) and its position (ESC [ 6 n).
    let script = "\x1bP1000p%begin 1 1 0\n%end 1 1 0\n%output %1 \\033[c\\033[6nready\n";
    let server = open_control(&mut h, &session, script, replies, true);
    until(&mut h, "the pane", |app| pane_of(app).is_some_and(|p| app.terminals[&p].term.screen().text().contains("ready")));
    for _ in 0..25 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    }
    let typed = String::from_utf8_lossy(&server.received_exec_input()).into_owned();
    assert!(!typed.contains("send-keys"), "tmux answers queries itself, nothing is typed back: {typed}");
}

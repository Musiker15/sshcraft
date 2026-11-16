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

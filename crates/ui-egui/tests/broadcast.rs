//! Broadcast types into every connected pane of a tab, never the terminal's own answers; status
//! dots say which pane needs a look.

use std::time::{Duration, Instant};

use egui::Event;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{EndReason, Session, SessionId, SessionStatus};
use sshcraft_model::model::HostId;
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::state::{ConnectRequest, Page};
use sshcraft_ui_egui::status::{Dot, waiting_line};
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn until(h: &mut H, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}; messages: {:?}", h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn host(session: &Session, name: &str) -> (TestServer, HostId) {
    session.runtime().block_on(test_host(session, name, TestAuth::default(), true))
}

fn connected(app: &SshcraftApp, id: SessionId) -> bool {
    app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. })))
}

fn connect_here(h: &mut H, host: HostId) -> SessionId {
    let before: Vec<SessionId> = h.state().terminals.keys().copied().collect();
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the session", |app| app.terminals.keys().any(|id| !before.contains(id)));
    let id = *h.state().terminals.keys().find(|id| !before.contains(id)).unwrap();
    until(h, "the connection", |app| connected(app, id));
    id
}

fn screen(app: &SshcraftApp, id: SessionId) -> String {
    app.terminals.get(&id).map(|t| t.term.visible_text()).unwrap_or_default()
}

/// A tab with `names.len()` connected panes, the first focused.
fn tab_of(h: &mut H, session: &Session, names: &[&str]) -> (Vec<SessionId>, Vec<TestServer>) {
    let mut ids = Vec::new();
    let mut servers = Vec::new();
    h.state_mut().open_new_tab();
    h.run_steps(2);
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            h.state_mut().split();
            h.run_steps(2);
        }
        let (server, host) = host(session, name);
        servers.push(server);
        ids.push(connect_here(h, host));
    }
    h.state_mut().ui.tabs[0].panes.focused = 0;
    h.run_steps(3);
    (ids, servers)
}

#[test]
fn broadcast_skips_ended_panes_and_terminal_replies() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (ids, _servers) = tab_of(&mut h, &session, &["alpha", "beta", "gamma"]);
    // The third pane's session ended as far as the window knows.
    h.state_mut().terminals.get_mut(&ids[2]).unwrap().status = Some(SessionStatus::Ended { reason: EndReason::Lost, code: None, detail: None });
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.pane.broadcast");
    h.run_steps(3);
    assert!(h.state().ui.tabs[0].panes.broadcast);
    h.event(Event::Text("bc-text".into()));
    until(&mut h, "the echo in both connected panes", |app| screen(app, ids[0]).contains("bc-text") && screen(app, ids[1]).contains("bc-text"));
    std::thread::sleep(Duration::from_millis(200));
    h.run_steps(5);
    assert!(!screen(h.state(), ids[2]).contains("bc-text"), "an ended pane gets nothing");
}

#[test]
fn broadcast_keeps_commands_and_passwords_apart() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (ids, _servers) = tab_of(&mut h, &session, &["alpha", "beta"]);
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.pane.broadcast");
    h.run_steps(3);
    // The second pane asks for a password; the focused one is at its prompt.
    h.state_mut().terminals.get_mut(&ids[1]).unwrap().term.feed(b"\r\n[sudo] password for me: ");
    h.event(Event::Text("bc-command".into()));
    until(&mut h, "the echo in the focused pane", |app| screen(app, ids[0]).contains("bc-command"));
    std::thread::sleep(Duration::from_millis(200));
    h.run_steps(5);
    assert!(!screen(h.state(), ids[1]).contains("bc-command"), "a command never goes into another pane's password question");
    // Now the focused pane asks and the other is at its prompt.
    h.state_mut().terminals.get_mut(&ids[0]).unwrap().term.feed(b"\r\nPassword: ");
    h.state_mut().terminals.get_mut(&ids[1]).unwrap().term.feed(b"\r\n$ ");
    h.event(Event::Text("s3cret".into()));
    until(&mut h, "the echo in the focused pane", |app| screen(app, ids[0]).contains("s3cret"));
    std::thread::sleep(Duration::from_millis(200));
    h.run_steps(5);
    assert!(!screen(h.state(), ids[1]).contains("s3cret"), "a password never goes to another pane's prompt");
}

#[test]
fn focus_reports_go_only_to_their_own_terminal() {
    use egui::FontId;
    use sshcraft_term::{TermConfig, TermSize, Terminal};
    use sshcraft_ui_egui::terminal::{TermPalette, TerminalView};
    let mut term = Terminal::new(TermSize::clamped(40, 10), &TermConfig::default());
    // The program asks for focus reports.
    term.feed(b"\x1b[?1004h");
    let palette = TermPalette::dark();
    let mut seen = Vec::new();
    let mut h = egui_kittest::Harness::new_ui(|ui| {
        let r = TerminalView { term: &mut term, palette: &palette, font: FontId::monospace(14.0), ligatures: true, highlighter: None }.show(ui);
        r.response.request_focus();
        seen.push((r.input.clone(), r.reports.clone()));
    });
    h.run_steps(3);
    drop(h);
    assert!(seen.iter().all(|(input, _)| input.is_empty()), "nothing typed: {seen:?}");
    assert!(seen.iter().any(|(_, reports)| reports.as_slice() == b"\x1b[I"), "the focus report is a report: {seen:?}");
}

#[test]
fn broadcast_is_off_for_new_tabs_and_shows() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.state_mut().open_new_tab();
    h.run_steps(2);
    assert!(!h.state().ui.tabs[0].panes.broadcast);
    assert!(h.query_by_label("Broadcast on").is_none());
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.pane.broadcast");
    h.run_steps(2);
    assert!(h.query_by_label("Broadcast on").is_some(), "the tab shows it");
    h.state_mut().open_new_tab();
    h.run_steps(2);
    assert!(!h.state().ui.tabs[1].panes.broadcast, "a new tab starts without");
}

#[test]
fn dots_rank_error_waiting_unseen() {
    let order = [Dot::Error, Dot::Waiting, Dot::Unseen, Dot::Connected, Dot::Connecting, Dot::Ended];
    for (i, a) in order.iter().enumerate() {
        for b in &order[i + 1..] {
            assert_eq!(Dot::most_urgent([*b, *a]), Some(*a), "{a:?} before {b:?}");
        }
    }
    assert_eq!(Dot::most_urgent([]), None);
}

#[test]
fn waiting_line_knows_password_questions() {
    for line in [
        "Password:",
        "root@db's password: ",
        "Passwort für root:",
        "Enter passphrase for key '/home/a/.ssh/id_ed25519':",
        "Mot de passe :",
        "Contraseña:",
        "Senha:",
        "Are you sure you want to continue connecting (yes/no)?",
        "Verification code:",
        "PIN:",
    ] {
        assert!(waiting_line(line), "{line}");
    }
    for line in ["password reset done", "$ ls", "the passphrase was changed.", ""] {
        assert!(!waiting_line(line), "{line}");
    }
}

#[test]
fn looking_at_a_pane_clears_unseen_and_waiting_not_error() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (ids, _servers) = tab_of(&mut h, &session, &["alpha", "beta"]);
    h.run_steps(5);
    // The second pane gets output while the first has the focus.
    h.state_mut().terminals.get_mut(&ids[1]).unwrap().term.feed(b"new output\r\n");
    h.state_mut().terminals.get_mut(&ids[1]).unwrap().mark_output();
    h.run_steps(2);
    assert_eq!(h.state().terminals[&ids[1]].dot(), Dot::Unseen);
    h.state_mut().ui.tabs[0].panes.focused = 1;
    h.run_steps(2);
    assert_eq!(h.state().terminals[&ids[1]].dot(), Dot::Connected, "looked at");
    h.state_mut().terminals.get_mut(&ids[1]).unwrap().status = Some(SessionStatus::Ended { reason: EndReason::Lost, code: None, detail: None });
    h.run_steps(2);
    assert_eq!(h.state().terminals[&ids[1]].dot(), Dot::Error, "an error stays when looked at");
    assert_eq!(h.state().ui.page, Page::Tab(0));
}

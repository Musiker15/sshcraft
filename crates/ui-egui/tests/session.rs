//! Session tabs: connecting, the host key and sign-in questions with their delay, OSC 52 consent
//! per host, links only through the engine, closing, and the terminal's copy and paste.

use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, PointerButton, ViewportCommand};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use sshcraft_engine::testing::{known_host, set_known_host, test_host};
use sshcraft_engine::{Caller, Session, SessionId, SessionStatus};
use sshcraft_model::model::HostId;
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_term::{GridPoint, SelectionKind};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::state::{ConnectRequest, Page, PaneSlot};
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

/// A test server and a host for it; its host key is trusted when `trusted`. The shell echoes
/// what it gets.
fn host(session: &Session, name: &str, trusted: bool) -> (TestServer, HostId) {
    session.runtime().block_on(test_host(session, name, TestAuth::default(), trusted))
}

fn connect(h: &mut H, host: HostId) -> SessionId {
    let before: Vec<SessionId> = h.state().terminals.keys().copied().collect();
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the tab", |app| app.terminals.keys().any(|id| !before.contains(id)));
    *h.state().terminals.keys().find(|id| !before.contains(id)).unwrap()
}

fn connected(app: &SshcraftApp, id: SessionId) -> bool {
    app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. })))
}

fn copied(h: &H) -> Option<String> {
    h.output().platform_output.commands.iter().find_map(|c| match c {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    })
}

/// What `steps` frames copied, each frame looked at.
fn copied_in(h: &mut H, steps: usize) -> Option<String> {
    let mut text = None;
    for _ in 0..steps {
        h.run_steps(1);
        text = text.or_else(|| copied(h));
    }
    text
}

fn feed(h: &mut H, id: SessionId, bytes: &[u8]) {
    h.state_mut().terminals.get_mut(&id).unwrap().term.feed(bytes);
}

fn known(session: &Session, server: &TestServer) -> Option<String> {
    known_host(session, "127.0.0.1", server.port)
}

/// Lets the answer buttons of a new question become active (400 ms at 60 frames a second).
fn wait_out_the_delay(h: &mut H) {
    h.run_steps(30);
}

#[test]
fn connecting_opens_a_terminal_tab() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "web", true);
    h.run_steps(2);
    let tab = connect(&mut h, id);
    until(&mut h, "the connection", |app| connected(app, tab));
    assert!(h.state().in_terminal());
    let Page::Tab(i) = h.state().ui.page else { panic!("a tab is shown") };
    assert_eq!(h.state().ui.tabs[i].panes.panes, [PaneSlot::Session { session: tab }]);
    assert_eq!(h.state().ui.tabs[i].title, "web");
    assert!(h.query_by_role(egui::accesskit::Role::Terminal).is_some(), "the terminal is on screen");
    assert!(h.query_by_label("Connected").is_some(), "the status line says so");
}

#[test]
fn an_unknown_host_key_is_asked_and_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (server, id) = host(&session, "new", false);
    h.run_steps(2);
    let tab = connect(&mut h, id);
    until(&mut h, "the question", |app| !app.prompts.open.is_empty());
    let tree = format!("{:?}", h.root());
    assert!(tree.contains(&server.host_fingerprint), "the fingerprint is shown");
    wait_out_the_delay(&mut h);
    h.get_by_label("Accept").click();
    until(&mut h, "the connection", |app| connected(app, tab));
    assert_eq!(known(&session, &server).as_deref(), Some(server.host_fingerprint.as_str()));
}

#[test]
fn a_changed_host_key_is_never_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (server, id) = host(&session, "moved", false);
    let old = "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    set_known_host(&session, "127.0.0.1", server.port, old);
    h.run_steps(2);
    let tab = connect(&mut h, id);
    let deadline = Instant::now() + Duration::from_secs(15);
    while !h.state().terminals.get(&tab).is_some_and(|t| matches!(t.status, Some(SessionStatus::Ended { .. }))) {
        assert!(Instant::now() < deadline, "the session did not end");
        assert!(h.state().prompts.open.is_empty(), "a changed key is never asked about");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
    assert!(h.query_by_label_contains("The host key of this server changed").is_some(), "a warning is shown");
    assert!(h.query_by_label("Accept").is_none() && h.query_by_label("Reconnect").is_none(), "nothing to go on with");
    assert_eq!(known(&session, &server).as_deref(), Some(old), "the stored key stays");
}

#[test]
fn a_new_prompt_is_not_answered_by_the_click_for_the_old_one() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (first, a) = host(&session, "first", false);
    let (second, b) = host(&session, "second", false);
    h.run_steps(2);
    connect(&mut h, a);
    connect(&mut h, b);
    until(&mut h, "both questions", |app| app.prompts.open.len() == 2);
    wait_out_the_delay(&mut h);
    h.get_by_label("Accept").click();
    h.run_steps(1);
    // A second click right after lands on the next question, which does not take it yet.
    h.get_by_label("Accept").click();
    h.run_steps(3);
    assert_eq!(h.state().prompts.open.len(), 1, "the second question is still open");
    let answered = [known(&session, &first), known(&session, &second)].iter().filter(|k| k.is_some()).count();
    assert!(answered <= 1, "at most the first question was answered");
    wait_out_the_delay(&mut h);
    h.get_by_label("Accept").click();
    until(&mut h, "the second answer", |app| app.prompts.open.is_empty());
}

const OSC52_HELLO: &[u8] = b"\x1b]52;c;aGVsbG8=\x07";

#[test]
fn clipboard_writes_need_consent_per_host() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_a, host_a) = host(&session, "alpha", true);
    let (_b, host_b) = host(&session, "beta", true);
    h.run_steps(2);
    let tab = connect(&mut h, host_a);
    until(&mut h, "the connection", |app| connected(app, tab));

    feed(&mut h, tab, OSC52_HELLO);
    assert_eq!(copied_in(&mut h, 3), None, "nothing without consent");
    h.get_by_label("No").click();
    h.run_steps(1);
    assert_eq!(copied(&h), None);

    feed(&mut h, tab, OSC52_HELLO);
    h.run_steps(2);
    h.get_by_label("Once").click();
    h.run_steps(1);
    assert_eq!(copied(&h).as_deref(), Some("hello"));

    feed(&mut h, tab, OSC52_HELLO);
    h.run_steps(2);
    h.get_by_label("Always for this host").click();
    h.run_steps(1);
    assert_eq!(copied(&h).as_deref(), Some("hello"));
    until(&mut h, "the saved consent", |app| app.data.settings.terminal.clipboard_hosts.contains(&host_a));
    assert_eq!(h.state().data.settings.terminal.clipboard_hosts, vec![host_a], "only this host");

    feed(&mut h, tab, OSC52_HELLO);
    assert_eq!(copied_in(&mut h, 3).as_deref(), Some("hello"), "no question for a host that is always allowed");
    assert!(h.query_by_label("Always for this host").is_none());

    let other = connect(&mut h, host_b);
    until(&mut h, "the second connection", |app| connected(app, other));
    feed(&mut h, other, OSC52_HELLO);
    assert_eq!(copied_in(&mut h, 3), None, "another host asks again");
    assert!(h.query_by_label("Always for this host").is_some());
}

#[test]
fn clipboard_writes_are_dropped_when_turned_off() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let mut settings = session.runtime().block_on(session.execute(Caller::User, "settings.get", json!({}))).unwrap();
    settings["terminal"]["clipboard_osc52"] = json!("off");
    session.runtime().block_on(session.execute(Caller::User, "settings.save", json!({ "settings": settings }))).unwrap();
    let (_server, id) = host(&session, "web", true);
    h.run_steps(2);
    let tab = connect(&mut h, id);
    until(&mut h, "the connection", |app| connected(app, tab));
    feed(&mut h, tab, OSC52_HELLO);
    assert_eq!(copied_in(&mut h, 3), None);
    assert!(h.query_by_label("Once").is_none(), "not even a question");
}

#[test]
fn closing_the_tab_closes_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "web", true);
    h.run_steps(2);
    let tab = connect(&mut h, id);
    until(&mut h, "the connection", |app| connected(app, tab));
    assert!(session.unacknowledged(tab).is_some());
    h.get_by_label("Close tab").click();
    until(&mut h, "the closed tab", |app| app.ui.tabs.is_empty() && app.terminals.is_empty());
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.unacknowledged(tab).is_some() {
        assert!(Instant::now() < deadline, "the session is still open");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

/// Ctrl+clicks the first cell of the terminal.
fn ctrl_click_top_left(h: &mut H) {
    let rect = h.get_by_role(egui::accesskit::Role::Terminal).rect();
    let pos = rect.min + egui::vec2(3.0, 5.0);
    h.event(Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::CTRL });
        h.run_steps(1);
    }
}

/// Steps until a frame opens a URL, or `frames` pass.
fn opened_url(h: &mut H, frames: usize) -> Option<String> {
    for _ in 0..frames {
        let url = h.output().platform_output.commands.iter().find_map(|c| match c {
            egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
            _ => None,
        });
        if url.is_some() {
            return url;
        }
        std::thread::sleep(Duration::from_millis(10));
        h.run_steps(1);
    }
    None
}

#[test]
fn a_link_opens_only_through_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "web", true);
    h.run_steps(2);
    let tab = connect(&mut h, id);
    until(&mut h, "the connection", |app| connected(app, tab));
    feed(&mut h, tab, b"\x1b[2J\x1b[H\x1b]8;;https://example.com/docs\x1b\\LINK\x1b]8;;\x1b\\");
    h.run_steps(2);
    ctrl_click_top_left(&mut h);
    assert_eq!(opened_url(&mut h, 60).as_deref(), Some("https://example.com/docs"));

    feed(&mut h, tab, b"\x1b[2J\x1b[H\x1b]8;;file:///C:/Windows/System32/calc.exe\x1b\\FILE\x1b]8;;\x1b\\");
    h.run_steps(2);
    let before = h.state().toasts.texts().len();
    ctrl_click_top_left(&mut h);
    assert_eq!(opened_url(&mut h, 30), None, "a file link opens nothing");
    assert!(h.state().toasts.texts().len() > before, "it says why: {:?}", h.state().toasts.texts());
}

/// Ctrl+Shift+`key` in one frame of its own (kittest's own helpers spread a key over several
/// frames, and a step shows only the last frame's output).
fn press(h: &mut H, key: Key) {
    let both = Modifiers::CTRL | Modifiers::SHIFT;
    h.input_mut().events.push(Event::Key { key, pressed: true, modifiers: both, repeat: false, physical_key: None });
    h.step();
    h.input_mut().events.push(Event::Key { key, pressed: false, modifiers: both, repeat: false, physical_key: None });
}

#[test]
fn ctrl_shift_c_and_v_copy_and_paste() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "web", true);
    h.run_steps(2);
    let tab = connect(&mut h, id);
    until(&mut h, "the connection", |app| connected(app, tab));
    h.run_steps(10);
    feed(&mut h, tab, b"\x1b[2J\x1b[Hhello world");
    {
        let term = &mut h.state_mut().terminals.get_mut(&tab).unwrap().term;
        let start: GridPoint = term.point_at(0, 0);
        let end = term.point_at(4, 0);
        term.select_start(start, SelectionKind::Simple);
        term.select_update(end);
    }
    h.run_steps(1);
    press(&mut h, Key::C);
    assert_eq!(copied(&h).as_deref(), Some("hello"));

    press(&mut h, Key::V);
    let asked = h.output().viewport_output.values().any(|v| v.commands.contains(&ViewportCommand::RequestPaste));
    assert!(asked, "the paste asks the system for the clipboard");
    h.event(Event::Paste("pasted-text".into()));
    h.run_steps(1);
    until(&mut h, "the echo", |app| app.terminals.get(&tab).is_some_and(|t| t.term.visible_text().contains("pasted-text")));
}

#[test]
fn a_second_request_for_the_same_host_opens_no_second_session() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "web", true);
    h.state_mut().open_new_tab();
    h.run_steps(2);
    // A double click asks twice before the first answer is back.
    for _ in 0..2 {
        let app = h.state_mut();
        app.ui.connect_request = Some(ConnectRequest::Host(id));
        sshcraft_ui_egui::session_tab::take_request(app);
    }
    until(&mut h, "the session", |app| !app.terminals.is_empty());
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    assert_eq!(h.state().terminals.len(), 1, "one connection");
    assert_eq!(h.state().ui.tabs.len(), 1);
}

#[test]
fn reconnecting_closes_the_old_session() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "new", false);
    h.run_steps(2);
    let old = connect(&mut h, id);
    until(&mut h, "the question", |app| !app.prompts.open.is_empty());
    wait_out_the_delay(&mut h);
    h.get_by_label("Cancel").click();
    until(&mut h, "the end", |app| app.terminals.get(&old).is_some_and(|t| matches!(t.status, Some(SessionStatus::Ended { .. }))));
    h.get_by_label("Reconnect").click();
    until(&mut h, "the new session", |app| !app.terminals.contains_key(&old) && !app.terminals.is_empty());
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.unacknowledged(old).is_some() {
        assert!(Instant::now() < deadline, "the old session is still open in the engine");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

#[test]
fn a_long_end_detail_keeps_reconnect_on_screen() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "web", true);
    h.run_steps(2);
    let tab = connect(&mut h, id);
    until(&mut h, "the connection", |app| connected(app, tab));
    h.state_mut().terminals.get_mut(&tab).unwrap().status =
        Some(SessionStatus::Ended { reason: sshcraft_engine::EndReason::Lost, code: None, detail: Some("x".repeat(5000)) });
    h.run_steps(3);
    let button = h.get_by_label("Reconnect").rect();
    assert!(button.max.x <= 1200.0, "Reconnect at {button:?}");
}

#[test]
fn a_question_shows_which_tab_it_belongs_to() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_a, first) = host(&session, "web", false);
    let (_b, second) = host(&session, "web", false);
    h.run_steps(2);
    let one = connect(&mut h, first);
    until(&mut h, "the first question", |app| !app.prompts.open.is_empty());
    connect(&mut h, second);
    until(&mut h, "the second question", |app| app.prompts.open.len() == 2);
    assert_eq!(h.state().prompts.open[0].session, Some(one));
    assert_eq!(h.state().ui.page, Page::Tab(1), "the second tab is in front");
    assert!(h.query_by_label_contains("Asked by tab 1").is_some(), "the question names its tab");
    h.get_by_label("Show tab").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.page, Page::Tab(0));
}

#[test]
fn notices_beside_a_connection_are_named() {
    use sshcraft_engine::ErrorCode;
    use sshcraft_ui_egui::session_tab::notice_text;
    for code in [
        ErrorCode::AgentForwardingUnavailable,
        ErrorCode::X11Unavailable,
        ErrorCode::X11NotAllowed,
        ErrorCode::CertificateExpired,
        ErrorCode::TelnetUnencrypted,
    ] {
        assert!(notice_text(code).is_some(), "{code:?}");
    }
    assert_eq!(notice_text(ErrorCode::Network), None);
}

#[test]
fn the_hex_view_keeps_bytes_only_while_open() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, host) =
        session.runtime().block_on(sshcraft_engine::testing::test_host(&session, "hex", sshcraft_ssh::testing::TestAuth::default(), true));
    h.state_mut().ui.connect_request = Some(sshcraft_ui_egui::state::ConnectRequest::Host(host));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !h.state().terminals.values().any(|t| matches!(t.status, Some(sshcraft_engine::SessionStatus::Connected { .. }))) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    }
    let id = *h.state().terminals.keys().next().unwrap();
    sshcraft_ui_egui::session_tab::send_bytes(h.state(), id, b"before\n");
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    }
    assert!(h.state().terminals[&id].hex.is_none(), "closed: nothing kept");
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.terminal.hex");
    h.run_steps(2);
    sshcraft_ui_egui::session_tab::send_bytes(h.state(), id, b"after\n");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !h.state().terminals[&id].hex.as_ref().is_some_and(|x| x.lines().join("\n").contains("|after")) {
        assert!(std::time::Instant::now() < deadline, "the echo did not reach the hex view");
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    }
    let dump = h.state().terminals[&id].hex.as_ref().unwrap().lines().join("\n");
    assert!(!dump.contains("before"), "nothing from before it opened: {dump}");
}

#[test]
fn a_macro_waits_for_its_text_and_can_be_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = sshcraft_ui_egui::testing::app_harness(dir.path());
    let mut settings = session.runtime().block_on(session.execute(sshcraft_engine::Caller::User, "settings.get", serde_json::json!({}))).unwrap();
    settings["terminal"]["macros"] = serde_json::json!([
        { "name": "two", "steps": [{ "expect": "", "send": "echo step-one" }, { "expect": "step-one", "send": "echo step-two" }] },
        { "name": "waits", "steps": [{ "expect": "never-comes", "send": "echo nope" }] },
    ]);
    session.runtime().block_on(session.execute(sshcraft_engine::Caller::User, "settings.save", serde_json::json!({ "settings": settings }))).unwrap();
    let (_server, host) =
        session.runtime().block_on(sshcraft_engine::testing::test_host(&session, "macro", sshcraft_ssh::testing::TestAuth::default(), true));
    h.state_mut().ui.connect_request = Some(sshcraft_ui_egui::state::ConnectRequest::Host(host));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !h.state().terminals.values().any(|t| matches!(t.status, Some(sshcraft_engine::SessionStatus::Connected { .. })))
        || h.state().data.settings.terminal.macros.len() < 2
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    }
    let id = *h.state().terminals.keys().next().unwrap();
    sshcraft_ui_egui::macros::start(h.state_mut(), id, "two");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !h.state().terminals[&id].term.screen().text().contains("echo step-two") {
        assert!(std::time::Instant::now() < deadline, "{}", h.state().terminals[&id].term.screen().text());
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    }
    assert!(h.state().terminals[&id].macro_run.is_none(), "done after its last step");
    sshcraft_ui_egui::macros::start(h.state_mut(), id, "waits");
    h.run_steps(3);
    assert!(h.state().terminals[&id].macro_run.is_some(), "waiting for its text");
    sshcraft_ui_egui::macros::stop(h.state_mut(), id);
    assert!(h.state().terminals[&id].macro_run.is_none());
    assert!(!h.state().terminals[&id].term.screen().text().contains("echo nope"));
}

//! Session logs: recorded output listed, searched, played back read-only with its marks,
//! exported where the user chooses; a log still recording is never deleted.

use std::time::{Duration, Instant};

use craft_dialogs::Answer;
use egui::{Event, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{Caller, Session, SessionId, SessionStatus};
use sshcraft_ssh::testing::TestAuth;
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::log_viewer::LogViewer;
use sshcraft_ui_egui::state::{ConnectRequest, Page};
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

fn run(session: &Session, id: &str, params: Value) -> Value {
    session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap()
}

fn logs(session: &Session) -> Vec<Value> {
    run(session, "log.list", json!({})).as_array().cloned().unwrap_or_default()
}

/// Recording on, one connected pane that printed `logged-text`; the server stays alive.
fn recorded(h: &mut H, session: &Session) -> (SessionId, sshcraft_ssh::testing::TestServer) {
    let mut settings = run(session, "settings.get", json!({}));
    settings["logs"]["session_logs"] = json!(true);
    run(session, "settings.save", json!({ "settings": settings }));
    let (server, host) = session.runtime().block_on(test_host(session, "web", TestAuth::default(), true));
    h.state_mut().open_new_tab();
    h.run_steps(2);
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the session", |app| !app.terminals.is_empty());
    let id = *h.state().terminals.keys().next().unwrap();
    until(h, "the connection", |app| app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. }))));
    h.event(Event::Text("echo logged-text".into()));
    until(h, "the echo", |app| app.terminals.get(&id).is_some_and(|t| t.term.visible_text().contains("logged-text")));
    (id, server)
}

fn close_and_open_logs(h: &mut H, session: &Session) {
    h.state_mut().close_tab();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !logs(session).iter().any(|l| l["ended_at"].is_i64()) {
        assert!(Instant::now() < deadline, "the log did not end");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.state_mut().ui.page = Page::Logs;
    until(h, "the list", |app| !app.data.logs.is_empty());
}

#[test]
fn rec_shows_while_recording_and_a_recording_log_is_not_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_id, _server) = recorded(&mut h, &session);
    until(&mut h, "the Rec mark", |_| true);
    assert!(h.query_by_label("Rec").is_some(), "the pane says it records");
    h.state_mut().ui.page = Page::Logs;
    until(&mut h, "the list", |app| !app.data.logs.is_empty());
    h.get_by_label("Delete all shown").click();
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    assert_eq!(logs(&session).len(), 1, "a log still recording stays");
}

#[test]
fn ctrl_shift_l_adds_a_bookmark() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_id, _server) = recorded(&mut h, &session);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key {
            key: Key::L,
            pressed,
            modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            repeat: false,
            physical_key: None,
        });
        h.step();
    }
    until(&mut h, "the notice", |app| app.toasts.texts().iter().any(|t| t.contains("Bookmark")));
    close_and_open_logs(&mut h, &session);
    let marks = logs(&session)[0]["marks"].as_array().cloned().unwrap_or_default();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0]["kind"], "bookmark");
}

#[test]
fn a_recorded_session_plays_back_with_marks_added_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_id, _server) = recorded(&mut h, &session);
    close_and_open_logs(&mut h, &session);
    h.get_by_label("Open").click();
    until(&mut h, "the viewer", |app| app.log_viewer.as_ref().is_some_and(|v| v.text().contains("logged-text")));
    h.get_by_label("Bookmark").click();
    until(&mut h, "the mark", |_| logs(&session)[0]["marks"].as_array().is_some_and(|m| m.len() == 1));
    until(&mut h, "the mark in the list", |app| app.log_viewer.as_ref().is_some_and(|v| v.marks().len() == 1));
    h.get_by_label("Remove mark").click();
    until(&mut h, "no mark", |_| logs(&session)[0]["marks"].as_array().is_some_and(Vec::is_empty));
}

#[test]
fn search_opens_the_log_at_the_hit() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_id, _server) = recorded(&mut h, &session);
    close_and_open_logs(&mut h, &session);
    h.state_mut().ui.logs.search = "logged".into();
    until(&mut h, "the hits", |app| !app.ui.logs.hits.is_empty());
    h.get_by_label_contains("logged-text").click();
    until(&mut h, "the viewer", |app| app.log_viewer.is_some());
    assert!(h.state().log_viewer.as_ref().unwrap().text().contains("logged-text"));
}

#[test]
fn export_writes_text_where_the_user_chose() {
    let dir = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_id, _server) = recorded(&mut h, &session);
    close_and_open_logs(&mut h, &session);
    let out = files.path().join("web.txt");
    let answer = out.clone();
    h.state_mut().dialogs.set_override(Some(Box::new(move |_| Answer::Paths(vec![answer.clone()]))));
    h.get_by_label("Export").click();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !out.exists() {
        assert!(Instant::now() < deadline, "no file; messages: {:?}", h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    std::thread::sleep(Duration::from_millis(100));
    assert!(std::fs::read_to_string(&out).unwrap().contains("logged-text"));
}

#[test]
fn a_long_log_says_its_beginning_is_missing() {
    let short = LogViewer::from_bytes(String::new(), b"one\r\ntwo\r\n", &[]);
    assert!(!short.beginning_missing());
    let long: Vec<u8> = (0..100_050).flat_map(|i| format!("line {i}\r\n").into_bytes()).collect();
    let viewer = LogViewer::from_bytes(String::new(), &long, &[]);
    assert!(viewer.beginning_missing(), "more than 100 000 lines");
}

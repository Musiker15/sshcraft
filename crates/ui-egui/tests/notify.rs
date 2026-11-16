//! Desktop notifications: a long command that finished while the window was in the background,
//! and messages from the server (OSC 9, OSC 777), which always name their host and are limited.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{SessionId, SessionStatus};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::notify::{RateLimit, Recording};
use sshcraft_ui_egui::state::ConnectRequest;
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

fn tab(h: &mut H) -> (SessionId, TestServer, Recording) {
    let session = h.state().session.clone();
    let (server, host) = session.runtime().block_on(test_host(&session, "box1", TestAuth::default(), true));
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the tab", |app| !app.terminals.is_empty());
    let id = *h.state().terminals.keys().next().unwrap();
    until(h, "connected", |app| app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. }))));
    let recording = Recording::default();
    h.state_mut().notifier = Box::new(recording.clone());
    (id, server, recording)
}

/// Bytes as if the server sent them: through the terminal and the notices of the next frames.
fn arrive(h: &mut H, id: SessionId, bytes: &[u8]) {
    h.state_mut().terminals.get_mut(&id).unwrap().term.feed(bytes);
    h.run_steps(2);
}

fn window_focused(h: &mut H, focused: bool) {
    let input = h.input_mut();
    input.events.push(egui::Event::WindowFocused(focused));
    input.viewports.entry(egui::ViewportId::ROOT).or_default().focused = Some(focused);
    h.run_steps(2);
}

const START: &[u8] = b"\x1b]133;A\x07$ \x1b]133;B\x07make\r\n\x1b]133;C\x07";

#[test]
fn a_long_command_in_the_background_notifies_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server, recording) = tab(&mut h);
    h.state_mut().data.settings.terminal.notify_after_secs = 1;
    window_focused(&mut h, false);
    arrive(&mut h, id, START);
    std::thread::sleep(Duration::from_millis(1100));
    arrive(&mut h, id, b"\x1b]133;D;3\x07");
    h.run_steps(4);
    let got = recording.taken();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].0.contains("box1"), "{got:?}");
    assert!(got[0].1.contains('3'), "{got:?}");
}

#[test]
fn a_short_command_does_not_notify() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server, recording) = tab(&mut h);
    h.state_mut().data.settings.terminal.notify_after_secs = 10;
    window_focused(&mut h, false);
    arrive(&mut h, id, START);
    arrive(&mut h, id, b"\x1b]133;D;0\x07");
    assert!(recording.taken().is_empty());
}

#[test]
fn a_command_watched_in_the_window_does_not_notify() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server, recording) = tab(&mut h);
    h.state_mut().data.settings.terminal.notify_after_secs = 1;
    window_focused(&mut h, true);
    arrive(&mut h, id, START);
    std::thread::sleep(Duration::from_millis(1100));
    arrive(&mut h, id, b"\x1b]133;D;0\x07");
    assert!(recording.taken().is_empty());
}

#[test]
fn a_server_message_names_its_host() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server, recording) = tab(&mut h);
    window_focused(&mut h, false);
    arrive(&mut h, id, b"\x1b]777;notify;SSHCraft;Your session expired, sign in again\x07");
    let got = recording.taken();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].0.starts_with("box1"), "the host comes first: {got:?}");
}

#[test]
fn server_messages_are_limited() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server, recording) = tab(&mut h);
    window_focused(&mut h, false);
    for i in 0..10 {
        arrive(&mut h, id, format!("\x1b]9;message {i}\x07").as_bytes());
    }
    assert_eq!(recording.taken().len(), 1);
}

#[test]
fn server_messages_can_be_turned_off() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server, recording) = tab(&mut h);
    h.state_mut().data.settings.terminal.server_notifications = false;
    window_focused(&mut h, false);
    arrive(&mut h, id, b"\x1b]9;hello\x07");
    assert!(recording.taken().is_empty());
}

#[test]
fn the_rate_limit_lets_one_through_per_window() {
    let mut limit = RateLimit::default();
    let now = Instant::now();
    assert!(limit.allow(now));
    assert!(!limit.allow(now + Duration::from_secs(1)));
    assert!(limit.allow(now + Duration::from_secs(6)));
}

#[test]
fn markup_in_a_message_is_never_rendered_on_freedesktop() {
    let safe = sshcraft_ui_egui::notify::markup_safe("<a href=\"https://evil\">click</a> & more");
    if cfg!(all(unix, not(target_os = "macos"))) {
        assert_eq!(safe, "&lt;a href=\"https://evil\"&gt;click&lt;/a&gt; &amp; more");
    } else {
        assert_eq!(safe, "<a href=\"https://evil\">click</a> & more");
    }
}

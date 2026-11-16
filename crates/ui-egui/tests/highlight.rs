//! Keyword highlights in the terminal and triggers on output lines.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use sshcraft_engine::settings::{Highlight, Trigger, TriggerAction};
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{SessionId, SessionStatus};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_term::{TermColor, TermConfig, TermSize, Terminal};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::notify::Recording;
use sshcraft_ui_egui::state::{ConnectRequest, Page};
use sshcraft_ui_egui::terminal::highlight::Highlighter;
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

#[test]
fn a_highlight_colours_its_cells_only() {
    let mut t = Terminal::new(TermSize::clamped(40, 5), &TermConfig::default());
    t.feed("an Error here, ünd error".as_bytes());
    let mut screen = t.screen();
    Highlighter::new(&[Highlight { pattern: "error".into(), color: [255, 0, 0] }]).apply(&mut screen);
    let fg = |col: usize| screen.cells[col].fg;
    assert_eq!(fg(3), TermColor::Rgb(255, 0, 0));
    assert_eq!(fg(7), TermColor::Rgb(255, 0, 0));
    assert_ne!(fg(8), TermColor::Rgb(255, 0, 0));
    assert_ne!(fg(2), TermColor::Rgb(255, 0, 0));
    // After the two-byte "ü" the match still lands on the right cells.
    assert_eq!(fg(19), TermColor::Rgb(255, 0, 0));
    assert_ne!(fg(18), TermColor::Rgb(255, 0, 0));
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

fn tab(h: &mut H) -> (SessionId, TestServer, Recording) {
    let session = h.state().session.clone();
    let (server, host) = session.runtime().block_on(test_host(&session, "box2", TestAuth::default(), true));
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the tab", |app| !app.terminals.is_empty());
    let id = *h.state().terminals.keys().next().unwrap();
    until(h, "connected", |app| app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. }))));
    let recording = Recording::default();
    h.state_mut().notifier = Box::new(recording.clone());
    let input = h.input_mut();
    input.viewports.entry(egui::ViewportId::ROOT).or_default().focused = Some(false);
    h.run_steps(2);
    (id, server, recording)
}

#[test]
fn a_trigger_acts_once_for_a_matching_line() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server, recording) = tab(&mut h);
    h.state_mut().data.settings.terminal.triggers = vec![Trigger { pattern: "deploy (done|finished)".into(), action: TriggerAction::Notify }];
    h.run_steps(2);
    h.state_mut().terminals.get_mut(&id).unwrap().term.feed(b"step 1\r\nDeploy done in 3s\r\nstep 2\r\n");
    h.run_steps(6);
    let got = recording.taken();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].0.starts_with("box2"), "{got:?}");
    assert!(got[0].1.contains("Deploy done"), "{got:?}");
}

#[test]
fn a_bell_trigger_marks_the_tab() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server, _recording) = tab(&mut h);
    h.state_mut().data.settings.terminal.triggers = vec![Trigger { pattern: "password changed".into(), action: TriggerAction::Bell }];
    // The tab is in the background: a tab looked at loses its mark at once.
    h.state_mut().ui.page = Page::Hosts;
    h.run_steps(2);
    assert!(!h.state().terminals[&id].waiting());
    h.state_mut().terminals.get_mut(&id).unwrap().term.feed(b"Password changed\r\n");
    h.run_steps(4);
    assert!(h.state().terminals[&id].waiting());
}

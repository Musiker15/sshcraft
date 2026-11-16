//! Command blocks from shell integration (OSC 133): the gutter, jumping between commands and
//! copying the output of the last one.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{SessionId, SessionStatus};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_term::CommandState;
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::state::ConnectRequest;
use sshcraft_ui_egui::terminal::blocks::{describe, gutter_color};
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

/// A connected tab on a test server, which lives as long as the returned server.
fn tab(h: &mut H) -> (SessionId, TestServer) {
    let session = h.state().session.clone();
    let (server, host) = session.runtime().block_on(test_host(&session, "blocks", TestAuth::default(), true));
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the tab", |app| !app.terminals.is_empty());
    let id = *h.state().terminals.keys().next().unwrap();
    until(h, "connected", |app| app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. }))));
    (id, server)
}

fn feed(h: &mut H, id: SessionId, bytes: &[u8]) {
    h.state_mut().terminals.get_mut(&id).unwrap().term.feed(bytes);
    h.run_steps(2);
}

const PROMPT: &[u8] = b"\x1b]133;A\x07$ \x1b]133;B\x07";

fn command(output: &str, code: i32) -> Vec<u8> {
    format!("\x1b]133;A\x07$ \x1b]133;B\x07cmd\r\n\x1b]133;C\x07{output}\x1b]133;D;{code}\x07").into_bytes()
}

fn copied_in(h: &mut H, steps: usize) -> Option<String> {
    let mut text = None;
    for _ in 0..steps {
        h.run_steps(1);
        text = text.or_else(|| {
            h.output().platform_output.commands.iter().find_map(|c| match c {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
        });
    }
    text
}

#[test]
fn copy_output_copies_the_last_finished_command() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server) = tab(&mut h);
    feed(&mut h, id, &[command("one\r\ntwo\r\n", 0).as_slice(), PROMPT].concat());
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.terminal.copy_output");
    assert_eq!(copied_in(&mut h, 4).as_deref(), Some("one\ntwo"));
}

#[test]
fn the_previous_command_scrolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server) = tab(&mut h);
    let many: Vec<u8> = (0..40).flat_map(|i| command(&format!("line {i}\r\nmore\r\nand more\r\n"), 0)).collect();
    feed(&mut h, id, &[many.as_slice(), PROMPT].concat());
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.terminal.prev_command");
    h.run_steps(2);
    assert!(h.state().terminals[&id].term.screen().display_offset > 0);
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.terminal.next_command");
    h.run_steps(2);
}

#[test]
fn commands_get_their_times_while_shown() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server) = tab(&mut h);
    feed(&mut h, id, &command("x\r\n", 1));
    let last = h.state().terminals[&id].term.command_history().last().cloned().unwrap();
    assert_eq!(last.state, CommandState::Finished);
    assert!(last.started.is_some() && last.finished.is_some(), "{last:?}");
}

#[test]
fn the_gutter_shows_how_a_command_ended() {
    let mut t = sshcraft_term::Terminal::new(sshcraft_term::TermSize::clamped(40, 10), &sshcraft_term::TermConfig::default());
    t.feed(&command("ok\r\n", 0));
    t.feed(&command("bad\r\n", 2));
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07sleep 9\r\n\x1b]133;C\x07");
    let history = t.command_history();
    let colors: Vec<_> = history.iter().map(gutter_color).collect();
    assert_eq!(colors.len(), 3);
    assert!(colors.iter().all(Option::is_some));
    assert_ne!(colors[0], colors[1], "success and failure differ");
    assert_ne!(colors[1], colors[2], "failure and running differ");
    assert!(describe(&history[1]).contains('2'), "{}", describe(&history[1]));
}

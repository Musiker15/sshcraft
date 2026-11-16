//! Hint mode: labels on URLs, paths, addresses and hashes; a label copies its text.

use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers};
use egui_kittest::Harness;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{SessionId, SessionStatus};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_term::{TermConfig, TermSize, Terminal};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::state::ConnectRequest;
use sshcraft_ui_egui::terminal::hints::{HintKind, find_hints, labels};
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn screen_of(text: &str) -> sshcraft_term::Screen {
    let mut t = Terminal::new(TermSize::clamped(100, 6), &TermConfig::default());
    t.feed(text.as_bytes());
    t.screen()
}

#[test]
fn urls_paths_addresses_and_hashes_are_found() {
    let screen = screen_of(
        "see https://example.org/a?b=1, then /var/log/syslog\r\nhost 192.168.1.20 and fe80::1:2:3 at commit 3f2a9c1d\r\nplain words 12345 here",
    );
    let found: Vec<(HintKind, String)> = find_hints(&screen).into_iter().map(|h| (h.kind, h.text)).collect();
    assert!(found.contains(&(HintKind::Url, "https://example.org/a?b=1".into())), "{found:?}");
    assert!(found.contains(&(HintKind::Path, "/var/log/syslog".into())), "{found:?}");
    assert!(found.contains(&(HintKind::Ip, "192.168.1.20".into())), "{found:?}");
    assert!(found.contains(&(HintKind::Ip, "fe80::1:2:3".into())), "{found:?}");
    assert!(found.contains(&(HintKind::Hash, "3f2a9c1d".into())), "{found:?}");
    assert!(!found.iter().any(|(_, t)| t == "12345"), "a plain number is no hash: {found:?}");
    assert!(!found.iter().any(|(k, t)| *k == HintKind::Path && t.contains("example.org")), "no path inside a URL: {found:?}");
}

#[test]
fn labels_are_unique_and_short() {
    let all = labels(300);
    assert_eq!(all.len(), 300);
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 300);
    // With more than 26 hints every label has two letters, so none starts another.
    assert_eq!(all[0], "aa");
    assert!(all.iter().all(|l| l.len() == 2));
    assert_eq!(labels(3), ["a", "b", "c"]);
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
    let (server, host) = session.runtime().block_on(test_host(&session, "hints", TestAuth::default(), true));
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the tab", |app| !app.terminals.is_empty());
    let id = *h.state().terminals.keys().next().unwrap();
    until(h, "connected", |app| app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. }))));
    (id, server)
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
fn typing_a_label_copies_its_text_and_escape_leaves() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server) = tab(&mut h);
    h.state_mut().terminals.get_mut(&id).unwrap().term.feed(b"\x1b[2J\x1b[Hgo to https://example.org/x now\r\n");
    h.run_steps(2);
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.terminal.hints");
    h.run_steps(2);
    assert!(h.state().terminals[&id].hints_shown());
    h.input_mut().events.push(Event::Text("a".into()));
    assert_eq!(copied_in(&mut h, 4).as_deref(), Some("https://example.org/x"));
    assert!(!h.state().terminals[&id].hints_shown(), "choosing ends the hints");

    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.terminal.hints");
    h.run_steps(2);
    h.input_mut().events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(!h.state().terminals[&id].hints_shown());
}

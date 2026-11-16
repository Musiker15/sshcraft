//! Program status (OSC 7501) as the tab's dot: waiting, failed, done, working.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{SessionId, SessionStatus};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::state::{ConnectRequest, Page};
use sshcraft_ui_egui::status::Dot;
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

fn tab(h: &mut H) -> (SessionId, TestServer) {
    let session = h.state().session.clone();
    let (server, host) = session.runtime().block_on(test_host(&session, "agent-box", TestAuth::default(), true));
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the tab", |app| !app.terminals.is_empty());
    let id = *h.state().terminals.keys().next().unwrap();
    until(h, "connected", |app| app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. }))));
    // In the background, so looking at it does not clear anything.
    h.state_mut().ui.page = Page::Hosts;
    h.run_steps(2);
    (id, server)
}

fn report(h: &mut H, id: SessionId, body: &str) {
    h.state_mut().terminals.get_mut(&id).unwrap().term.feed(format!("\x1b]7501;{body}\x07").as_bytes());
    h.run_steps(2);
}

#[test]
fn the_dot_follows_the_most_urgent_program() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server) = tab(&mut h);
    report(&mut h, id, "state=working:id=a:app=claude");
    assert_eq!(h.state().terminals[&id].dot(), Dot::Working);
    // "Allow edit?" in base64.
    report(&mut h, id, "state=blocked:id=a:app=claude:msg=QWxsb3cgZWRpdD8=");
    assert_eq!(h.state().terminals[&id].dot(), Dot::Waiting);
    let text = h.state().terminals[&id].program_text().unwrap();
    assert!(text.contains("claude") && text.contains("Allow edit?"), "{text}");
    // Waiting for the user goes before a failure; once it is answered the failure shows.
    report(&mut h, id, "state=error:id=b");
    assert_eq!(h.state().terminals[&id].dot(), Dot::Waiting);
    report(&mut h, id, "state=clear:id=a");
    assert_eq!(h.state().terminals[&id].dot(), Dot::Error);
}

#[test]
fn a_prompt_clears_a_working_program() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    let (id, _server) = tab(&mut h);
    report(&mut h, id, "state=working:id=a");
    h.state_mut().terminals.get_mut(&id).unwrap().term.feed(b"\x1b]133;A\x07");
    h.run_steps(2);
    assert_ne!(h.state().terminals[&id].dot(), Dot::Working);
}

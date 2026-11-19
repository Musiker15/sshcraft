//! The SFTP pane on a test server: it opens from a host, and after a lost connection it offers to
//! connect again.

use std::time::{Duration, Instant};

use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use sshcraft_engine::Session;
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn wait(h: &mut H, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

/// The files of a test host, open in their tab, with the server's folder read.
fn opened(dir: &std::path::Path) -> (H, Session, sshcraft_ssh::testing::TestServer, tempfile::TempDir) {
    let (mut h, session) = app_harness(dir);
    h.set_size(vec2(1200.0, 844.0));
    let (server, _host) = session.runtime().block_on(sshcraft_engine::testing::trusted_test_host(&session));
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("on-the-server.txt"), b"hi").unwrap();
    server.serve_sftp(root.path());
    h.state_mut().ui.page = Page::Hosts;
    h.state_mut().data.dirty |= sshcraft_ui_egui::data::Refresh::DATA;
    wait(&mut h, "the host never showed", |app| !app.data.hosts.is_empty());
    h.run_steps(3);
    h.get_by_label("SFTP").click();
    wait(&mut h, "the server's folder was not read", |app| app.sftp.values().any(|v| v.remote.entries.iter().any(|e| e.name == "on-the-server.txt")));
    (h, session, server, root)
}

#[test]
fn after_a_lost_connection_the_pane_connects_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session, _server, _root) = opened(dir.path());
    assert!(h.query_by_label("Reconnect").is_none(), "no button while the connection is there");
    for view in h.state_mut().sftp.values_mut() {
        view.lost = true;
        view.remote.entries.clear();
    }
    h.run_steps(2);
    h.get_by_label("Reconnect").click();
    wait(&mut h, "the pane did not connect again", |app| app.sftp.values().all(|v| !v.lost && !v.remote.entries.is_empty()));
}

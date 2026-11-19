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

/// An open SFTP pane counts as an open session, so Android keeps the connection while the
/// system's file picker sends the app to the background.
#[test]
fn an_sftp_pane_counts_as_an_open_session() {
    let dir = tempfile::tempdir().unwrap();
    let (h, _session, _server, _root) = opened(dir.path());
    assert_eq!(sshcraft_ui_egui::notify::open_sessions(h.state()), 1);
}

/// Files go from one host's pane into the folder another pane shows, through "Copy to…".
#[test]
fn copy_to_sends_the_selection_into_another_panes_folder() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session, _server, root) = opened(dir.path());
    std::fs::create_dir(root.path().join("target")).unwrap();
    let first = *h.state().sftp.keys().next().unwrap();
    h.state_mut().ui.page = Page::Hosts;
    h.run_steps(2);
    h.get_by_label("SFTP").click();
    wait(&mut h, "a second pane did not open", |app| app.sftp.len() == 2);
    let second = *h.state().sftp.keys().find(|k| **k != first).unwrap();
    wait(&mut h, "the second pane was not read", |app| app.sftp.get(&second).is_some_and(|v| !v.remote.loading));
    // As if the second pane showed the empty folder "/target".
    let pane = h.state_mut().sftp.get_mut(&second).unwrap();
    pane.remote.path = "/target".into();
    pane.remote.entries.clear();
    h.state_mut().sftp.get_mut(&first).unwrap().remote.selected.insert("on-the-server.txt".into());
    h.state_mut().ui.page = Page::Tab(0);
    h.run_steps(2);
    h.get_by_label("Copy to…").click();
    h.run_steps(6);
    h.get_by_label("test host: /target").click();
    let copied = root.path().join("target/on-the-server.txt");
    wait(&mut h, "the file was not copied", |_| std::fs::read(&copied).is_ok_and(|d| d == b"hi"));
}

/// "Sync folders…" sends what is new in the folder shown here into the folder shown on the host.
#[test]
fn sync_sends_the_newer_files_of_this_folder_up() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session, _server, root) = opened(dir.path());
    let local = tempfile::tempdir().unwrap();
    std::fs::write(local.path().join("from-here.txt"), b"new").unwrap();
    let pane = h.state_mut().sftp.values_mut().next().unwrap();
    pane.local.path = local.path().to_string_lossy().into_owned();
    pane.local.selected.clear();
    h.run_steps(2);
    h.get_by_label("Sync folders…").click();
    h.run_steps(6);
    h.get_by_label("Upload newer files").click();
    let sent = root.path().join("from-here.txt");
    wait(&mut h, "the file was not sent", |_| std::fs::read(&sent).is_ok_and(|d| d == b"new"));
    assert_eq!(std::fs::read(root.path().join("on-the-server.txt")).unwrap(), b"hi", "nothing there is touched");
}

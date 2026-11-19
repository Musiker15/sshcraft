//! A host's row: name and address, cut to fit, and the buttons for SSH and SFTP.

use std::time::{Duration, Instant};

use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use sshcraft_engine::{Caller, Session};
use sshcraft_model::model::HostId;
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn with_host(dir: &std::path::Path, name: &str, address: &str, width: f32) -> (H, Session, HostId) {
    let (mut h, session) = app_harness(dir);
    h.set_size(vec2(width, 844.0));
    let id = HostId::new();
    session
        .runtime()
        .block_on(session.execute(Caller::User, "host.save", json!({ "host": { "id": id, "name": name, "settings": { "address": address } } })))
        .unwrap();
    h.state_mut().ui.page = Page::Hosts;
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.state().data.hosts.is_empty() {
        assert!(Instant::now() < deadline, "the host never showed");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(3);
    (h, session, id)
}

#[test]
fn the_ssh_button_connects() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session, _id) = with_host(dir.path(), "web-01", "192.0.2.1", 1200.0);
    assert!(h.state().ui.tabs.is_empty());
    h.get_by_label("SSH").click();
    // The request is taken in the next frame and opens a terminal for the host.
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.state().terminals.is_empty() {
        assert!(Instant::now() < deadline, "SSH opens a session");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

/// The SFTP button opens the host's files in a tab of their own, with the server's folder read.
#[test]
fn the_sftp_button_opens_the_files() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    h.set_size(vec2(1200.0, 844.0));
    let (server, _host) = session.runtime().block_on(sshcraft_engine::testing::trusted_test_host(&session));
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("from-the-server.txt"), b"hi").unwrap();
    server.serve_sftp(root.path());
    h.state_mut().ui.page = Page::Hosts;
    // The host went into the store directly, without the event a save sends.
    h.state_mut().data.dirty |= sshcraft_ui_egui::data::Refresh::DATA;
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.state().data.hosts.is_empty() {
        assert!(Instant::now() < deadline, "the host never showed");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(3);
    h.get_by_label("SFTP").click();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let listed = h.state().sftp.values().any(|v| v.remote.entries.iter().any(|e| e.name == "from-the-server.txt"));
        if listed {
            break;
        }
        assert!(Instant::now() < deadline, "the server's folder was not read");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    assert!(matches!(h.state().ui.page, Page::Tab(_)), "the files' tab is shown");
    h.run_steps(2);
    assert!(h.query_by_label("from-the-server.txt").is_some(), "the file is in the list");
}

#[test]
fn long_names_stay_inside_the_row() {
    let dir = tempfile::tempdir().unwrap();
    let name = "a".repeat(120);
    let address = format!("{}.example.org", "b".repeat(68));
    let (h, _session, _id) = with_host(dir.path(), &name, &address, 390.0);
    for label in ["SSH", "SFTP"] {
        let right = h.get_by_label(label).rect().max.x;
        assert!(right <= 390.5, "{label} ends at {right}, outside a 390 pt window");
    }
}

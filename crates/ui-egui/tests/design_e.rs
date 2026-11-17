//! The sizes and places design E fixes: settings at the bottom of the sidebar, a search field and
//! buttons 50 points high, SSH and SFTP as square symbols in host rows, and "New host" floating at
//! the bottom of a phone.

use std::time::{Duration, Instant};

use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use sshcraft_engine::{Caller, Session};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn run(session: &Session, id: &str, params: serde_json::Value) {
    session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap();
}

fn with_hosts(dir: &std::path::Path, width: f32) -> H {
    let (mut h, session) = app_harness(dir);
    h.set_size(vec2(width, 800.0));
    for name in ["alpha", "beta"] {
        run(
            &session,
            "host.save",
            json!({ "host": { "id": uuid::Uuid::new_v4(), "name": name, "group_id": null, "settings": { "address": "192.0.2.1" } } }),
        );
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.state().data.hosts.len() < 2 {
        assert!(Instant::now() < deadline, "timed out waiting for the hosts");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(4);
    h
}

#[test]
fn settings_sit_at_the_bottom_of_the_sidebar() {
    let dir = tempfile::tempdir().unwrap();
    let h = with_hosts(dir.path(), 1280.0);
    let logs = h.get_by_label("Logs").rect();
    let settings = h.get_by_label("Settings").rect();
    assert!(settings.top() > logs.bottom() + 300.0, "settings at {settings:?}, logs at {logs:?}");
    assert!(settings.bottom() > 800.0 - 80.0, "settings end at {} in an 800 pt window", settings.bottom());
}

#[test]
fn the_search_and_its_buttons_are_fifty_points_high() {
    let dir = tempfile::tempdir().unwrap();
    let h = with_hosts(dir.path(), 1280.0);
    let search = h.get_by_role(egui::accesskit::Role::TextInput).rect();
    let new_host = h.get_by_label("New host").rect();
    let new_group = h.get_by_label("New group").rect();
    for (what, rect) in [("New host", new_host), ("New group", new_group)] {
        assert!((rect.height() - 50.0).abs() < 0.5, "{what} is {} high", rect.height());
    }
    assert!(new_group.left() > search.right(), "the buttons sit right of the search");
}

#[test]
fn host_rows_offer_ssh_and_sftp_as_square_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let h = with_hosts(dir.path(), 1280.0);
    let ssh: Vec<_> = h.query_all_by_label("SSH").map(|n| n.rect()).collect();
    let sftp: Vec<_> = h.query_all_by_label("SFTP").map(|n| n.rect()).collect();
    assert_eq!((ssh.len(), sftp.len()), (2, 2), "one pair per host");
    for rect in ssh.iter().chain(&sftp) {
        assert!((rect.width() - 34.0).abs() < 0.5 && (rect.height() - 34.0).abs() < 0.5, "a 34 pt square, not {rect:?}");
    }
}

#[test]
fn a_phone_has_new_host_floating_at_the_bottom() {
    let dir = tempfile::tempdir().unwrap();
    let h = with_hosts(dir.path(), 390.0);
    let button = h.get_by_label("New host").rect();
    assert!(button.bottom() > 800.0 - 40.0 && button.width() > 300.0, "a wide button at the bottom, not {button:?}");
    assert!(h.query_by_label("New group").is_some(), "the bar holds New group");
}

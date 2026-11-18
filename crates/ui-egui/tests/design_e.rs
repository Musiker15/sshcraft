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

#[test]
fn the_sync_card_is_only_as_tall_as_its_two_lines() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = with_hosts(dir.path(), 1280.0);
    h.state_mut().data.servers = vec![json!({ "id": "s1", "name": "home", "status": { "state": "idle" } })];
    h.run_steps(3);
    let state = h.get_by_label("Up to date").rect();
    let settings = h.get_by_label("Settings").rect();
    assert!(settings.top() - state.top() < 90.0, "the card ends near the settings: state at {state:?}, settings at {settings:?}");
    assert!(state.top() > 800.0 - 200.0, "the card sits at the bottom, not at {state:?}");
}

#[test]
fn a_host_shows_the_address_it_takes_from_its_group() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    h.set_size(vec2(1280.0, 800.0));
    let group = uuid::Uuid::new_v4();
    run(&session, "group.save", json!({ "group": { "id": group, "name": "servers", "defaults": { "address": "198.51.100.77" } } }));
    run(&session, "host.save", json!({ "host": { "id": uuid::Uuid::new_v4(), "name": "inherits", "group_id": group, "settings": {} } }));
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.state().data.hosts.is_empty() || h.state().data.groups.is_empty() {
        assert!(Instant::now() < deadline, "timed out waiting for the host");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(4);
    assert!(h.query_by_label_contains("198.51.100.77").is_some(), "the group's address shows under the host");
}

#[test]
fn a_group_card_has_the_group_menu() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    h.set_size(vec2(1280.0, 800.0));
    let group = uuid::Uuid::new_v4();
    run(&session, "group.save", json!({ "group": { "id": group, "name": "servers", "defaults": {} } }));
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.state().data.groups.is_empty() {
        assert!(Instant::now() < deadline, "timed out waiting for the group");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(4);
    h.get_by_label("servers").click_secondary();
    h.run_steps(2);
    // The menu's "Edit", not the menu bar's.
    h.query_all_by_label("Edit").last().expect("the group menu").click();
    h.run_steps(2);
    assert!(h.state().ui.group_form.is_some(), "Edit opens the group's form");
}

#[test]
fn the_new_tab_names_each_host_with_its_group() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let group = uuid::Uuid::new_v4();
    run(&session, "group.save", json!({ "group": { "id": group, "name": "Masterblack", "defaults": {} } }));
    run(
        &session,
        "host.save",
        json!({ "host": { "id": uuid::Uuid::new_v4(), "name": "root", "group_id": group, "settings": { "address": "192.0.2.9" } } }),
    );
    run(
        &session,
        "host.save",
        json!({ "host": { "id": uuid::Uuid::new_v4(), "name": "loose", "group_id": null, "settings": { "address": "192.0.2.8" } } }),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.state().data.hosts.len() < 2 || h.state().data.groups.is_empty() {
        assert!(Instant::now() < deadline, "timed out waiting for the hosts");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.state_mut().open_new_tab();
    h.run_steps(4);
    assert!(h.query_by_label("Masterblack: root").is_some(), "the host in a group carries its group's name");
    assert!(h.query_by_label("loose").is_some(), "a host without a group keeps its name alone");
}

#[test]
fn a_hosts_tags_show_in_its_row() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    h.set_size(vec2(1280.0, 800.0));
    let group = uuid::Uuid::new_v4();
    run(&session, "group.save", json!({ "group": { "id": group, "name": "servers", "defaults": {} } }));
    run(
        &session,
        "host.save",
        json!({ "host": { "id": uuid::Uuid::new_v4(), "name": "tagged", "group_id": group, "tags": ["prod"], "settings": { "address": "192.0.2.4" } } }),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.state().data.hosts.is_empty() || h.state().data.groups.is_empty() {
        assert!(Instant::now() < deadline, "timed out waiting for the host");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(4);
    let row = h.get_by_label("tagged").rect();
    let tag = h.query_all_by_label("prod").map(|n| n.rect()).find(|r| (r.center().y - row.center().y).abs() < 12.0);
    assert!(tag.is_some_and(|t| t.left() > row.left()), "the tag sits in the host's row");
}

#[test]
fn the_overview_offers_the_tags_of_hosts_in_groups() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    h.set_size(vec2(1280.0, 800.0));
    let group = uuid::Uuid::new_v4();
    run(&session, "group.save", json!({ "group": { "id": group, "name": "servers", "defaults": {} } }));
    run(
        &session,
        "host.save",
        json!({ "host": { "id": uuid::Uuid::new_v4(), "name": "grouped", "group_id": group, "tags": ["zone-a"], "settings": { "address": "192.0.2.4" } } }),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.state().data.hosts.is_empty() || h.state().data.groups.is_empty() {
        assert!(Instant::now() < deadline, "timed out waiting for the host");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(4);
    // One pill in the host's row, one chip to filter by.
    assert_eq!(h.query_all_by_label("zone-a").count(), 2, "the tag is offered as a filter chip too");
}

#[test]
fn the_hello_switch_is_in_the_general_settings_and_saves() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.state_mut().ui.page = sshcraft_ui_egui::state::Page::Settings;
    h.run_steps(4);
    assert!(h.state().data.settings.general.hello_before_secrets, "on by default");
    h.get_by_label("Ask Windows Hello before the first stored key or password").click();
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.state().data.settings.general.hello_before_secrets {
        assert!(Instant::now() < deadline, "the switch was not saved");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

#[test]
fn the_key_list_says_where_each_key_lives() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.state_mut().ui.page = sshcraft_ui_egui::state::Page::Keys;
    h.run_steps(4);
    let app = h.state_mut();
    app.data.servers = vec![json!({ "id": "srv-1", "name": "home", "status": { "state": "idle" } })];
    app.data.keys = vec![
        sshcraft_ui_egui::data::KeyRow { id: "k1".into(), name: "synced".into(), location: Some("srv-1".into()), ..Default::default() },
        sshcraft_ui_egui::data::KeyRow { id: "k2".into(), name: "local".into(), ..Default::default() },
    ];
    h.run_steps(3);
    let synced = h.get_by_label("synced").rect();
    let local = h.get_by_label("local").rect();
    let at = |label: &str, row: egui::Rect| h.query_all_by_label(label).any(|n| (n.rect().center().y - row.center().y).abs() < 10.0);
    assert!(at("home", synced), "the synced key names its server");
    assert!(at("This device only", local), "the local key says it stays here");
}

//! The window around the pages: sidebar, tabs and their shortcuts, the new-tab page, labels that
//! never run over their space.

use std::time::{Duration, Instant};

use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use serde_json::json;
use sshcraft_engine::Caller;
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

#[test]
fn the_sidebar_switches_pages() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    h.get_by_label("Keys").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.page, Page::Keys);
    h.get_by_label("Settings").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.page, Page::Settings);
}

#[test]
fn tabs_open_and_close_by_shortcut() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::T);
    h.run_steps(2);
    assert_eq!(h.state().ui.tabs.len(), 1);
    assert_eq!(h.state().ui.page, Page::Tab(0));
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::W);
    h.run_steps(2);
    assert!(h.state().ui.tabs.is_empty());
    assert_eq!(h.state().ui.page, Page::Hosts);
}

#[test]
fn ctrl_tab_cycles_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::T);
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::T);
    h.run_steps(2);
    assert_eq!(h.state().ui.page, Page::Tab(1));
    h.key_press_modifiers(Modifiers::CTRL, Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().ui.page, Page::Tab(0));
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().ui.page, Page::Tab(1));
}

#[test]
fn the_new_tab_page_offers_the_hosts() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let host = json!({ "host": { "id": uuid::Uuid::new_v4(), "name": "db-primary", "group_id": null, "settings": { "address": "10.0.0.5" } } });
    session.runtime().block_on(session.execute(Caller::User, "host.save", host)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.state().data.hosts.is_empty() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::T);
    h.run_steps(2);
    assert!(h.query_by_label("db-primary").is_some(), "the new-tab page lists the host");
    assert!(h.query_by_label("Local terminal").is_some());
}

#[test]
fn labels_never_overflow() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::T);
    h.run_steps(2);
    let long = "x".repeat(400);
    h.state_mut().ui.tabs[0].title = long.clone();
    h.run_steps(2);
    let tab = h.get_by_label(&long);
    let width = tab.rect().width();
    assert!(width <= sshcraft_ui_egui::shell::MAX_TAB_WIDTH + 1.0, "the tab is {width} wide");
}

#[test]
fn a_held_middle_click_closes_one_tab() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(2);
    for _ in 0..3 {
        h.state_mut().open_new_tab();
    }
    h.run_steps(3);
    let rect = h.get_all_by_label("New tab").next().unwrap().rect();
    let at = rect.center();
    h.event(egui::Event::PointerMoved(at));
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Middle, pressed: true, modifiers: Modifiers::NONE });
    // A click of about 100 ms: several frames with the button down.
    h.run_steps(6);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Middle, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert_eq!(h.state().ui.tabs.len(), 2, "one tab closed, not several");
}

#[test]
fn closing_another_tab_keeps_the_view() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(2);
    for _ in 0..3 {
        h.state_mut().open_new_tab();
    }
    h.state_mut().ui.tabs[2].title = "third".into();
    h.state_mut().ui.page = Page::Tab(2);
    h.run_steps(3);
    h.get_all_by_label("Close tab").next().unwrap().click();
    h.run_steps(2);
    assert_eq!(h.state().ui.page, Page::Tab(1), "still on the same tab, one place to the left");
    assert_eq!(h.state().ui.tabs[1].title, "third");
    h.state_mut().ui.page = Page::Hosts;
    h.run_steps(2);
    h.get_all_by_label("Close tab").next().unwrap().click();
    h.run_steps(2);
    assert_eq!(h.state().ui.page, Page::Hosts, "the hosts page stays");
}

#[test]
fn the_sidebar_has_every_page() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    let pages = [
        ("Hosts", Page::Hosts),
        ("Keys", Page::Keys),
        ("Snippets", Page::Snippets),
        ("Port forwarding", Page::Forwarding),
        ("Logs", Page::Logs),
        ("Settings", Page::Settings),
    ];
    let mut tops = Vec::new();
    for (label, page) in pages {
        let entry = h.get_by_role_and_label(egui::accesskit::Role::Button, label);
        tops.push(entry.rect().top());
        entry.click();
        h.run_steps(2);
        assert_eq!(h.state().ui.page, page, "{label}");
    }
    assert!(tops.windows(2).all(|w| w[0] < w[1]), "in this order: {tops:?}");
}

#[test]
fn quick_connect_saves_a_host_and_opens_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = sshcraft_ui_egui::testing::app_harness(dir.path());
    sshcraft_ui_egui::quick_connect::open(h.state_mut(), "tester@127.0.0.1:2222".into());
    h.run_steps(2);
    h.get_by_label("Connect").click();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !h.state().data.hosts.iter().any(|r| r.host.name == "tester@127.0.0.1:2222") {
        assert!(std::time::Instant::now() < deadline, "the host was not saved");
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
    assert!(h.state().ui.quick_connect.is_none(), "the dialog closes");
    assert_eq!(h.state().ui.tabs.len(), 1, "the host opens in a tab");
    let row = h.state().data.hosts.iter().find(|r| r.host.name == "tester@127.0.0.1:2222").unwrap();
    assert_eq!(row.host.settings.username.as_deref(), Some("tester"));
    assert_eq!(row.host.settings.port, Some(2222));
}

#[test]
fn quick_connect_refuses_an_option_as_user_and_saves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = sshcraft_ui_egui::testing::app_harness(dir.path());
    sshcraft_ui_egui::quick_connect::open(h.state_mut(), "-oProxyCommand=calc@host".into());
    h.run_steps(2);
    h.get_by_label("Connect").click();
    h.run_steps(5);
    let dialog = h.state().ui.quick_connect.clone().expect("the dialog stays open");
    assert!(dialog.error.is_some());
    assert!(h.state().data.hosts.is_empty());
}

#[test]
fn quick_connect_matches_what_a_host_inherits() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let group = uuid::Uuid::new_v4();
    let host = uuid::Uuid::new_v4();
    let run = |id: &str, params: serde_json::Value| session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap();
    run("group.save", json!({ "group": { "id": group, "name": "lab", "defaults": { "port": 2222, "username": "ops" } } }));
    run("host.save", json!({ "host": { "id": host, "name": "db", "group_id": group, "settings": { "address": "db.lab" } } }));
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.state().data.hosts.is_empty() || h.state().data.groups.is_empty() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    // Port 22 is not what the saved host uses: a new host.
    sshcraft_ui_egui::quick_connect::connect(h.state_mut(), "ops@db.lab").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.state().data.hosts.len() < 2 {
        assert!(Instant::now() < deadline, "a new host for port 22");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    // The inherited port and user: the saved host opens.
    sshcraft_ui_egui::quick_connect::connect(h.state_mut(), "ops@db.lab:2222").unwrap();
    assert_eq!(
        h.state().ui.connect_request.as_ref().map(|r| format!("{r:?}")),
        Some(format!("{:?}", sshcraft_ui_egui::state::ConnectRequest::Host(sshcraft_model::model::HostId(host))))
    );
}

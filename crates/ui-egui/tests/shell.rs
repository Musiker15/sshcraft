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

//! The Hosts page at its root: the question over a large search field, then "Recently
//! connected", the hosts without a group, and the groups as cards with their hosts. A group, a
//! search or a filter shows the list instead.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use sshcraft_engine::{Caller, Session};
use sshcraft_model::model::{GroupId, HostId};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::app::Form;
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn run(session: &Session, id: &str, params: serde_json::Value) {
    session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap();
}

fn group(session: &Session, name: &str) -> GroupId {
    let id = GroupId::new();
    run(session, "group.save", json!({ "group": { "id": id, "name": name, "defaults": {} } }));
    id
}

fn host(session: &Session, name: &str, group: Option<GroupId>) -> HostId {
    let id = HostId::new();
    run(session, "host.save", json!({ "host": { "id": id, "name": name, "group_id": group, "settings": { "address": "192.0.2.1" } } }));
    id
}

fn until(h: &mut H, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(3);
}

fn hosts_page(dir: &std::path::Path) -> (H, Session) {
    let (mut h, session) = app_harness(dir);
    h.state_mut().ui.page = Page::Hosts;
    h.run_steps(2);
    (h, session)
}

fn y(h: &H, label: &str) -> f32 {
    h.get_by_label(label).rect().min.y
}

#[test]
fn hosts_without_a_group_come_before_the_groups() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = hosts_page(dir.path());
    let g = group(&session, "G");
    host(&session, "a", Some(g));
    host(&session, "b", None);
    until(&mut h, "the hosts", |app| app.data.hosts.len() == 2 && app.data.groups.len() == 1);
    assert!(h.query_by_label("Where do you want to connect?").is_some());
    assert!(y(&h, "Without a group") < y(&h, "Groups"), "the hosts without a group come first");
    assert!(h.query_by_label("b").is_some() && h.query_by_label("a").is_some() && h.query_by_label("G").is_some());
}

#[test]
fn a_group_card_shows_eight_hosts() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = hosts_page(dir.path());
    let g = group(&session, "G");
    for i in 0..10 {
        host(&session, &format!("h{i:02}"), Some(g));
    }
    until(&mut h, "the hosts", |app| app.data.hosts.len() == 10);
    let shown = (0..10).filter(|i| h.query_by_label(&format!("h{i:02}")).is_some()).count();
    assert_eq!(shown, 8);
    assert!(h.query_by_label("Show all 10").is_some());
}

#[test]
fn without_a_group_shows_twelve() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = hosts_page(dir.path());
    for i in 0..15 {
        host(&session, &format!("h{i:02}"), None);
    }
    until(&mut h, "the hosts", |app| app.data.hosts.len() == 15);
    let shown = (0..15).filter(|i| h.query_by_label(&format!("h{i:02}")).is_some()).count();
    assert_eq!(shown, 12);
    h.get_by_label("Show all 15").click();
    h.run_steps(3);
    assert!(h.query_by_label("Where do you want to connect?").is_none(), "the list replaces the overview");
}

#[test]
fn recent_hosts_come_first() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = hosts_page(dir.path());
    let b = host(&session, "b", None);
    sshcraft_engine::testing::touch_recent(&session, b, 1);
    host(&session, "c", None);
    until(&mut h, "the recent host", |app| app.data.recent.contains(&b) && app.data.hosts.len() == 2);
    assert!(y(&h, "Recently connected") < y(&h, "Without a group"));
}

#[test]
fn a_deleted_recent_host_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = hosts_page(dir.path());
    let b = host(&session, "b", None);
    host(&session, "c", None);
    sshcraft_engine::testing::touch_recent(&session, b, 1);
    run(&session, "host.delete", json!({ "id": b }));
    until(&mut h, "the hosts", |app| app.data.hosts.len() == 1);
    assert!(h.query_by_label("Recently connected").is_none());
}

#[test]
fn searching_leaves_the_overview() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = hosts_page(dir.path());
    let g = group(&session, "G");
    host(&session, "a", Some(g));
    until(&mut h, "the hosts", |app| app.data.hosts.len() == 1);
    assert!(h.query_by_label("Groups").is_some());
    h.state_mut().ui.hosts.search = "a".into();
    h.run_steps(3);
    assert!(h.query_by_label("Groups").is_none());
}

#[test]
fn the_tablet_header_has_no_import() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = hosts_page(dir.path());
    h.get_by_label("More").click();
    h.run_steps(2);
    assert!(h.query_by_label("Import").is_some(), "the desktop imports, behind More");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.state_mut().form = Form::Tablet;
    h.run_steps(3);
    if let Some(more) = h.query_by_label("More") {
        more.click();
        h.run_steps(2);
    }
    assert!(h.query_by_label("Import").is_none());
}

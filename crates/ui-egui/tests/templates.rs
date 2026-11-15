//! Workspace templates: a tab's panes saved with their connections, titles, directories and
//! commands, opened again from the new-tab page; the start line never runs foreign text.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{Caller, Session, SessionId, SessionStatus};
use sshcraft_model::model::HostId;
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::pages::templates::{local_cwd, start_line};
use sshcraft_ui_egui::state::{ConnectRequest, PaneSlot};
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn until(h: &mut H, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}; messages: {:?}", h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn host(session: &Session, name: &str) -> (TestServer, HostId) {
    session.runtime().block_on(test_host(session, name, TestAuth::default(), true))
}

fn connected(app: &SshcraftApp, id: SessionId) -> bool {
    app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. })))
}

fn connect_here(h: &mut H, host: HostId) -> SessionId {
    let before: Vec<SessionId> = h.state().terminals.keys().copied().collect();
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the session", |app| app.terminals.keys().any(|id| !before.contains(id)));
    let id = *h.state().terminals.keys().find(|id| !before.contains(id)).unwrap();
    until(h, "the connection", |app| connected(app, id));
    id
}

fn templates(session: &Session) -> Vec<serde_json::Value> {
    let list = session.runtime().block_on(session.execute(Caller::User, "workspace.list", json!({}))).unwrap();
    list.as_array().cloned().unwrap_or_default()
}

#[test]
fn a_template_directory_is_quoted_once() {
    assert_eq!(start_line(Some("/srv/app"), Some("make run"), true).as_deref(), Some("cd -- '/srv/app' && make run"));
    assert_eq!(start_line(Some("'; rm -rf ~ #"), None, true).as_deref(), Some("cd -- ''\\''; rm -rf ~ #'"), "one quoted word");
    assert_eq!(start_line(Some("/srv"), Some("top"), false).as_deref(), Some("top"), "a serial line gets the command only");
    assert_eq!(start_line(None, None, true), None);
    assert_eq!(start_line(Some("/a\nb"), None, true), None, "never a second line");
    assert_eq!(start_line(None, Some("ls\rrm -rf /"), true), None);
    // fish reads a backslash inside single quotes: the word could end early there.
    assert_eq!(start_line(Some("/tmp/a\\'; rm -rf ~ #"), Some("ls"), true), None, "no backslash in a directory");
}

#[test]
fn a_local_cwd_must_be_local_and_absolute() {
    assert_eq!(local_cwd("relative/dir"), None);
    assert_eq!(local_cwd(r"\\server\share"), None, "UNC");
    assert_eq!(local_cwd("//server/share"), None, "UNC with slashes");
    let home = if cfg!(windows) { r"C:\Users\me" } else { "/home/me" };
    assert_eq!(local_cwd(home).as_deref(), Some(home));
}

/// A tab with two connected panes and a template saved from it through the dialog.
fn saved(h: &mut H, session: &Session) -> (Vec<TestServer>, Vec<HostId>) {
    let (a_server, a) = host(session, "alpha");
    let (b_server, b) = host(session, "beta");
    h.state_mut().data.dirty |= sshcraft_ui_egui::data::Refresh::HOSTS;
    h.state_mut().open_new_tab();
    h.run_steps(2);
    let first = connect_here(h, a);
    h.state_mut().split();
    h.run_steps(2);
    connect_here(h, b);
    h.state_mut().ui.tabs[0].panes.col_sizes = vec![0.7, 0.3];
    h.state_mut().terminals.get_mut(&first).unwrap().cwd = Some("/srv/app".into());
    h.run_steps(2);
    h.get_by_label("Save as workspace").click();
    h.run_steps(3);
    {
        let form = h.state_mut().ui.template_form.as_mut().expect("the dialog opened");
        form.name = "deploy".into();
        form.panes[0].command = "make run".into();
    }
    h.get_by_label("Save").click();
    until(h, "the template", |_| !templates(session).is_empty());
    (vec![a_server, b_server], vec![a, b])
}

#[test]
fn a_template_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_servers, hosts) = saved(&mut h, &session);
    let template = templates(&session).remove(0);
    assert_eq!(template["name"], "deploy");
    assert_eq!(template["panes"][0]["cwd"], "/srv/app", "the directory OSC 7 reported");
    assert_eq!(template["panes"][0]["command"], "make run");
    h.state_mut().close_tab();
    h.state_mut().open_new_tab();
    until(&mut h, "the template list", |app| !app.data.templates.is_empty());
    h.get_by_label("deploy").click();
    until(&mut h, "both connections", |app| app.terminals.len() == 2 && app.terminals.keys().all(|id| connected(app, *id)));
    let tab = h.state().ui.tabs.iter().find(|t| t.panes.panes.len() == 2).expect("a tab with both panes").clone();
    assert!((tab.panes.col_sizes[0] - 0.7).abs() < 1e-3, "{:?}", tab.panes.col_sizes);
    let opened: Vec<HostId> = tab
        .panes
        .panes
        .iter()
        .filter_map(|p| match p {
            PaneSlot::Session { session } => h.state().terminals[session].host,
            PaneSlot::New { .. } => None,
        })
        .collect();
    assert_eq!(opened, hosts, "the same hosts in the same places");
    assert!(!tab.panes.broadcast, "broadcast is off in an opened template");
}

#[test]
fn a_deleted_host_is_left_out_with_a_notice() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_servers, hosts) = saved(&mut h, &session);
    h.state_mut().close_tab();
    session.runtime().block_on(session.execute(Caller::User, "host.delete", json!({ "id": hosts[1] }))).unwrap();
    h.state_mut().open_new_tab();
    until(&mut h, "the data", |app| app.data.hosts.len() == 1 && !app.data.templates.is_empty());
    h.get_by_label("deploy").click();
    until(&mut h, "the connection", |app| app.terminals.len() == 1);
    assert!(h.state().toasts.texts().iter().any(|t| t.contains("1 pane")), "{:?}", h.state().toasts.texts());
}

#[test]
fn templates_stop_at_100() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, host_id) = host(&session, "alpha");
    for i in 0..100 {
        let template = json!({ "id": uuid::Uuid::new_v4(), "name": format!("t{i}"), "layout": "split", "col_sizes": [1.0], "row_sizes": [1.0], "panes": [{ "target": { "kind": "host", "host_id": host_id }, "title": "alpha" }] });
        session.runtime().block_on(session.execute(Caller::User, "workspace.save", json!({ "template": template }))).unwrap();
    }
    h.state_mut().data.dirty |= sshcraft_ui_egui::data::Refresh::HOSTS;
    h.state_mut().open_new_tab();
    h.run_steps(2);
    let id = connect_here(&mut h, host_id);
    let _ = id;
    h.get_by_label("Save as workspace").click();
    h.run_steps(3);
    h.state_mut().ui.template_form.as_mut().unwrap().name = "one too many".into();
    h.get_by_label("Save").click();
    until(&mut h, "the refusal", |app| !app.toasts.texts().is_empty());
    assert_eq!(templates(&session).len(), 100);
}

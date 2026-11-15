//! The port forwarding page: the rules of every host, the wizard with its summary and warning,
//! start and stop, refused connections and rules reachable from other machines.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{Caller, Session};
use sshcraft_model::forward::{ForwardId, ForwardKind, ForwardRule};
use sshcraft_model::model::HostId;
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::pages::forwarding::{ForwardForm, ssh_option};
use sshcraft_ui_egui::state::Page;
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

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn rule(kind: ForwardKind, bind: &str, port: u16) -> ForwardRule {
    ForwardRule {
        id: ForwardId::new(),
        name: "db".into(),
        kind,
        bind_address: bind.into(),
        bind_port: port,
        target_host: "db.internal".into(),
        target_port: 5432,
    }
}

fn page(h: &mut H) {
    h.state_mut().data.dirty |= sshcraft_ui_egui::data::Refresh::HOSTS;
    h.state_mut().ui.page = Page::Forwarding;
    h.run_steps(3);
}

fn state_of(app: &SshcraftApp, rule: ForwardId) -> String {
    app.data.forwards.get(&rule).and_then(|s| s.get("state")).and_then(|s| s.as_str()).unwrap_or_default().to_string()
}

#[test]
fn the_summary_shows_the_ssh_option() {
    assert_eq!(ssh_option(&rule(ForwardKind::Local, "127.0.0.1", 8080)), "-L 127.0.0.1:8080:db.internal:5432");
    assert_eq!(ssh_option(&rule(ForwardKind::Remote, "localhost", 9000)), "-R localhost:9000:db.internal:5432");
    let mut socks = rule(ForwardKind::Dynamic, "127.0.0.1", 1080);
    socks.target_host.clear();
    socks.target_port = 0;
    assert_eq!(ssh_option(&socks), "-D 127.0.0.1:1080");
    assert_eq!(ssh_option(&rule(ForwardKind::Local, "::1", 8080)), "-L [::1]:8080:db.internal:5432", "IPv6 in brackets");
}

#[test]
fn a_local_rule_is_created_started_and_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, host_id) = host(&session, "web");
    page(&mut h);
    until(&mut h, "the host", |app| !app.data.hosts.is_empty());
    h.get_by_label("New rule").click();
    h.run_steps(3);
    let port = free_port();
    {
        let form: &mut ForwardForm = h.state_mut().ui.forward_form.as_mut().expect("the wizard opened");
        form.host = Some(host_id);
        form.rule = rule(ForwardKind::Local, "127.0.0.1", port);
    }
    h.run_steps(2);
    assert!(h.query_by_label_contains("-L 127.0.0.1:").is_some(), "the summary");
    h.get_by_label("Save").click();
    until(&mut h, "the rule", |app| app.data.hosts.iter().any(|r| r.host.forwards.len() == 1));
    let id = h.state().data.hosts[0].host.forwards[0].id;
    h.get_by_label("Start").click();
    until(&mut h, "the running rule", |app| state_of(app, id) == "running");
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok(), "the port listens");
    assert!(h.query_by_label("Edit").is_some_and(|n| n.accesskit_node().is_disabled()), "a running rule cannot be edited");
    h.get_by_label("Stop").click();
    until(&mut h, "the stopped rule", |app| state_of(app, id) == "stopped");
}

#[test]
fn the_wizard_warns_about_0_0_0_0() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, host_id) = host(&session, "web");
    page(&mut h);
    until(&mut h, "the host", |app| !app.data.hosts.is_empty());
    h.get_by_label("New rule").click();
    h.run_steps(3);
    h.state_mut().ui.forward_form.as_mut().unwrap().host = Some(host_id);
    h.run_steps(2);
    assert!(h.query_by_label_contains("Other machines can reach").is_none(), "no warning for 127.0.0.1");
    h.state_mut().ui.forward_form.as_mut().unwrap().rule.bind_address = "0.0.0.0".into();
    h.run_steps(2);
    assert!(h.query_by_label_contains("Other machines can reach").is_some());
}

#[test]
fn rules_stop_at_50_per_host() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, host_id) = host(&session, "web");
    let mut stored = session.runtime().block_on(session.execute(Caller::User, "host.list", json!({}))).unwrap()[0].clone();
    stored["forwards"] = json!((0..50).map(|i| rule(ForwardKind::Local, "127.0.0.1", 20000 + i)).collect::<Vec<_>>());
    session.runtime().block_on(session.execute(Caller::User, "host.save", json!({ "host": stored }))).unwrap();
    page(&mut h);
    until(&mut h, "the rules", |app| app.data.hosts.first().is_some_and(|r| r.host.forwards.len() == 50));
    h.get_by_label("New rule").click();
    h.run_steps(3);
    h.state_mut().ui.forward_form.as_mut().unwrap().host = Some(host_id);
    h.run_steps(2);
    assert!(h.query_by_label_contains("at most 50").is_some(), "it says why");
    assert!(h.get_by_label("Save").accesskit_node().is_disabled());
}

#[test]
fn refused_connections_and_reachable_rules_show_in_the_row() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, host_id) = host(&session, "web");
    let open = rule(ForwardKind::Local, "0.0.0.0", 8081);
    let mut stored = session.runtime().block_on(session.execute(Caller::User, "host.list", json!({}))).unwrap()[0].clone();
    stored["forwards"] = json!([open]);
    session.runtime().block_on(session.execute(Caller::User, "host.save", json!({ "host": stored }))).unwrap();
    page(&mut h);
    until(&mut h, "the rule", |app| app.data.hosts.first().is_some_and(|r| r.host.forwards.len() == 1));
    h.state_mut().data.forwards.insert(
        open.id,
        json!({ "rule_id": open.id, "host_id": host_id, "state": "running", "bound_port": 8081, "connections": 2, "refused": 3, "notice": null }),
    );
    h.run_steps(2);
    assert!(h.query_by_label_contains("3 refused").is_some(), "the refused count");
    assert!(h.query_by_label_contains("Reachable from other machines").is_some());
}

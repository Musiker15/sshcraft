//! Snippets: saved in packages, run on new connections (one pane each, typed once) or open panes,
//! pasted with or without Enter, from the page, the palette or the sidebar beside the terminal.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{Caller, Session, SessionId, SessionStatus};
use sshcraft_model::model::HostId;
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::pages::snippets::{Pick, paste_bytes};
use sshcraft_ui_egui::state::{ConnectRequest, Page};
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

fn screen(app: &SshcraftApp, id: SessionId) -> String {
    app.terminals.get(&id).map(|t| t.term.visible_text()).unwrap_or_default()
}

fn save_snippet(session: &Session, name: &str, package: &str, content: &str, targets: &[HostId]) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let snippet = json!({ "id": id, "name": name, "package": package, "content": content, "default_targets": targets });
    session.runtime().block_on(session.execute(Caller::User, "snippet.save", json!({ "snippet": snippet }))).unwrap();
    id
}

fn refresh(h: &mut H) {
    h.state_mut().data.dirty |= sshcraft_ui_egui::data::Refresh::DATA;
}

#[test]
fn paste_only_leaves_it_at_the_prompt() {
    assert_eq!(paste_bytes("ls -la", false, false), b"ls -la");
    assert_eq!(paste_bytes("ls -la", false, true), b"ls -la\r");
}

#[test]
fn multi_line_uses_bracketed_paste_when_the_shell_has_it() {
    assert_eq!(paste_bytes("a\nb", true, true), b"\x1b[200~a\nb\x1b[201~\r");
    assert_eq!(paste_bytes("a\r\nb", false, true), b"a\rb\r", "each line runs as it arrives");
    assert_eq!(paste_bytes("a\tb", false, false), b"a b", "tabs become spaces without bracketed paste");
    assert_eq!(paste_bytes("x\x1b[201~y", true, false), b"\x1b[200~x[201~y\x1b[201~", "no escape inside the paste");
}


/// Lets a few frames and answers pass.
fn settle(h: &mut H) {
    for _ in 0..15 {
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

#[test]
fn deleting_a_snippet_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    save_snippet(&session, "restart", "ops", "sudo systemctl restart app", &[]);
    h.state_mut().ui.page = Page::Snippets;
    refresh(&mut h);
    until(&mut h, "the snippet", |app| !app.data.snippets.is_empty());
    h.get_by_label("Delete").click();
    settle(&mut h);
    let listed = |session: &Session| session.runtime().block_on(session.execute(Caller::User, "snippet.list", json!({}))).unwrap();
    assert_eq!(listed(&session).as_array().map(Vec::len), Some(1), "nothing is deleted before the confirmation");
    h.get_by_label("Delete snippet").click();
    until(&mut h, "the deletion", |_| listed(&session).as_array().is_some_and(Vec::is_empty));
}

#[test]
fn a_snippet_is_saved_and_listed_by_package() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.state_mut().ui.page = Page::Snippets;
    h.run_steps(3);
    h.get_by_label("New snippet").click();
    h.run_steps(3);
    {
        let form = h.state_mut().ui.snippet_form.as_mut().expect("the form opened");
        form.name = "restart".into();
        form.package = "ops".into();
        form.content = "sudo systemctl restart app".into();
    }
    h.run_steps(2);
    h.get_by_label("Save").click();
    until(&mut h, "the snippet", |app| app.data.snippets.iter().any(|s| s.name == "restart"));
    assert!(h.query_by_label("ops").is_some(), "the package heading");
    assert!(h.query_by_label("restart").is_some());
}

#[test]
fn snippet_limits() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.state_mut().ui.page = Page::Snippets;
    h.run_steps(3);
    h.get_by_label("New snippet").click();
    h.run_steps(3);
    {
        let form = h.state_mut().ui.snippet_form.as_mut().unwrap();
        form.name = "bad".into();
        form.content = "echo \x07".into();
    }
    h.run_steps(2);
    assert!(h.get_by_label("Save").accesskit_node().is_disabled(), "no control characters but line breaks and tabs");
    h.state_mut().ui.snippet_form.as_mut().unwrap().content = "x".repeat(10_001);
    h.run_steps(2);
    assert!(h.get_by_label("Save").accesskit_node().is_disabled(), "at most 10 000 characters");
    h.state_mut().ui.snippet_form.as_mut().unwrap().content = "line one\n\tline two".into();
    h.run_steps(2);
    assert!(!h.get_by_label("Save").accesskit_node().is_disabled());
}

#[test]
fn running_on_new_hosts_opens_one_pane_each_and_types_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_a, a) = host(&session, "alpha");
    let (_b, b) = host(&session, "beta");
    let snippet = save_snippet(&session, "hello", "", "echo snip-run", &[a, b]);
    refresh(&mut h);
    h.state_mut().ui.page = Page::Snippets;
    until(&mut h, "the snippet", |app| !app.data.snippets.is_empty() && app.data.hosts.len() == 2);
    h.get_by_label("Run").click();
    h.run_steps(3);
    let picks = h.state().ui.snippet_run.as_ref().expect("the run dialog").picks.clone();
    assert_eq!(picks, [Pick::New(a), Pick::New(b)], "the default hosts, each once");
    assert_eq!(h.state().ui.snippet_run.as_ref().unwrap().snippet, snippet);
    h.get_by_label("Run on 2").click();
    until(&mut h, "both panes typed", |app| app.terminals.len() == 2 && app.terminals.keys().all(|id| screen(app, *id).contains("snip-run")));
    let tab = &h.state().ui.tabs[0];
    assert_eq!(tab.panes.panes.len(), 2, "one tab, one pane per host");
    assert_eq!(tab.title, "Snippet: hello");
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    for id in h.state().terminals.keys() {
        assert_eq!(screen(h.state(), *id).matches("snip-run").count(), 1, "typed once");
    }
}

#[test]
fn running_on_open_panes_types_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_a, a) = host(&session, "alpha");
    save_snippet(&session, "hello", "", "echo open-run", &[a]);
    refresh(&mut h);
    h.state_mut().open_new_tab();
    h.run_steps(2);
    let pane = connect_here(&mut h, a);
    h.state_mut().ui.page = Page::Snippets;
    until(&mut h, "the snippet", |app| !app.data.snippets.is_empty());
    h.get_by_label("Run").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.snippet_run.as_ref().unwrap().picks, [Pick::Open(pane)], "the open pane of a default host");
    h.get_by_label("Paste only").click();
    h.run_steps(2);
    h.get_by_label("Run on 1").click();
    until(&mut h, "the paste", |app| screen(app, pane).contains("open-run"));
    assert_eq!(h.state().terminals.len(), 1, "no new connection");
}

#[test]
fn the_sidebar_pastes_into_all_panes_when_broadcast_is_on() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_a, a) = host(&session, "alpha");
    let (_b, b) = host(&session, "beta");
    save_snippet(&session, "greet", "", "echo side-run", &[]);
    refresh(&mut h);
    h.state_mut().open_new_tab();
    h.run_steps(2);
    let first = connect_here(&mut h, a);
    h.state_mut().split();
    h.run_steps(2);
    let second = connect_here(&mut h, b);
    until(&mut h, "the snippet", |app| !app.data.snippets.is_empty());
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.pane.broadcast");
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.terminal.sidebar");
    h.run_steps(3);
    h.get_by_label("greet").click();
    until(&mut h, "both panes", |app| screen(app, first).contains("side-run") && screen(app, second).contains("side-run"));
}

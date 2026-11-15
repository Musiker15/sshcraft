//! AI agents: their question in the window (agent, action, target, the command with warnings),
//! answered only by a click after the delay; their rights per category, hidden hosts, their log.

use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use sshcraft_engine::{Caller, Session, Via};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::pages::settings::Section;
use sshcraft_ui_egui::prompts::PromptKind;
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

fn run(session: &Session, id: &str, params: Value) -> Value {
    session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap()
}

/// An agent asks to save a host (Create/change asks by default); the task waits for the answer.
/// The connection lives as long as the task.
fn agent_saves(session: &Session, name: &str) -> tokio::task::JoinHandle<Result<Value, sshcraft_engine::EngineError>> {
    let conn = session.agent_connection("claude", Via::Mcp).unwrap();
    let s = session.clone();
    // Agents write hosts in their own flat form.
    let host = json!({ "host": { "name": name, "address": "192.0.2.9" } });
    session.runtime().spawn(async move {
        let result = s.execute(conn.caller(), "host.save", host).await;
        drop(conn);
        result
    })
}

fn agent_question(app: &SshcraftApp) -> bool {
    app.prompts.open.iter().any(|p| matches!(p.kind, PromptKind::Agent { .. }))
}

#[test]
fn an_agent_question_names_agent_action_and_target_and_waits_for_a_click() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    h.run_steps(2);
    let task = agent_saves(&session, "db-02");
    until(&mut h, "the question", agent_question);
    let tree = format!("{:?}", h.root());
    assert!(tree.contains("claude") && tree.contains("db-02"), "who asks and about what");
    // Typing answers nothing: only a click does.
    h.event(Event::Text("y".into()));
    for pressed in [true, false] {
        h.event(Event::Key { key: Key::Enter, pressed, modifiers: Modifiers::NONE, repeat: false, physical_key: None });
        h.run_steps(1);
    }
    h.run_steps(30);
    assert!(agent_question(h.state()), "still open after typing");
    assert!(!task.is_finished());
    h.get_by_label("Allow").click();
    until(&mut h, "the answer", |app| !agent_question(app));
    let result = session.runtime().block_on(task).unwrap();
    assert!(result.is_ok(), "{result:?}");
}

fn press(h: &mut H, key: Key) {
    for pressed in [true, false] {
        h.event(Event::Key { key, pressed, modifiers: Modifiers::NONE, repeat: false, physical_key: None });
        h.run_steps(1);
    }
}

#[test]
fn tab_then_enter_or_space_never_answers_an_agent_question() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    h.run_steps(2);
    let task = agent_saves(&session, "db-03");
    until(&mut h, "the question", agent_question);
    // Long past the delay: the buttons take clicks now.
    std::thread::sleep(Duration::from_millis(600));
    h.run_steps(5);
    for _ in 0..4 {
        press(&mut h, Key::Tab);
        press(&mut h, Key::Enter);
        press(&mut h, Key::Space);
    }
    h.run_steps(10);
    assert!(agent_question(h.state()), "no key answers the question");
    assert!(!task.is_finished());
    h.get_by_label("Deny").click();
    until(&mut h, "the answer", |app| !agent_question(app));
    let result = session.runtime().block_on(task).unwrap();
    assert!(result.is_err(), "denied: {result:?}");
}

#[test]
fn a_long_command_carries_a_warning() {
    use sshcraft_ui_egui::prompts::command_warning;
    assert!(command_warning("ls -la").is_none());
    assert!(command_warning("echo a\nrm -rf /").is_some(), "more than one line");
    assert!(command_warning(&"x".repeat(301)).is_some(), "over 300 characters");
    assert!(command_warning("ls \u{202E}fdp.exe").is_some(), "hidden or reordering characters");
}

fn agents_page(h: &mut H) {
    h.state_mut().ui.page = Page::Settings;
    h.state_mut().ui.settings.section = Section::Agents;
    h.run_steps(3);
}

#[test]
fn levels_are_set_per_category() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    agents_page(&mut h);
    until(&mut h, "the policy", |app| app.ui.agents.policy.is_some());
    let delete = sshcraft_ui_egui::pages::agents::CATEGORIES.iter().position(|(c, _)| *c == "delete").unwrap();
    h.get_all_by_label("Ask").nth(delete).unwrap().click();
    let deadline = Instant::now() + Duration::from_secs(10);
    while run(&session, "agent.policy", json!({}))["levels"]["delete"] != "ask" {
        assert!(Instant::now() < deadline, "the level was not saved");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

#[test]
fn a_hidden_host_is_listed_and_shown_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let host = uuid::Uuid::new_v4();
    run(&session, "host.save", json!({ "host": { "id": host, "name": "secret-db", "group_id": null, "settings": { "address": "192.0.2.5" } } }));
    run(&session, "agent.set_hidden", json!({ "id": host, "hidden": true }));
    agents_page(&mut h);
    until(&mut h, "the hidden host", |app| {
        app.ui.agents.policy.as_ref().is_some_and(|p| p["hidden"].as_array().is_some_and(|a| !a.is_empty())) && !app.data.hosts.is_empty()
    });
    assert!(h.query_by_label("secret-db").is_some());
    h.get_by_label("Show again").click();
    let deadline = Instant::now() + Duration::from_secs(10);
    while run(&session, "agent.policy", json!({}))["hidden"].as_array().is_some_and(|a| !a.is_empty()) {
        assert!(Instant::now() < deadline, "still hidden");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

#[test]
fn the_open_log_takes_in_what_an_agent_just_did() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let conn = session.agent_connection("claude", Via::Mcp).unwrap();
    let _ = session.runtime().block_on(session.execute(conn.caller(), "host.delete", json!({ "id": uuid::Uuid::new_v4() })));
    agents_page(&mut h);
    until(&mut h, "the log", |app| app.ui.agents.log.as_ref().is_some_and(|l| l.len() == 1));
    let _ = session.runtime().block_on(session.execute(conn.caller(), "host.delete", json!({ "id": uuid::Uuid::new_v4() })));
    until(&mut h, "the second entry", |app| app.ui.agents.log.as_ref().is_some_and(|l| l.len() == 2));
}

#[test]
fn the_log_lists_outcomes_and_clears() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let conn = session.agent_connection("claude", Via::Mcp).unwrap();
    session.runtime().block_on(session.execute(conn.caller(), "host.list", json!({}))).unwrap();
    let _ = session.runtime().block_on(session.execute(conn.caller(), "host.delete", json!({ "host": "nowhere" })));
    let _ = session.runtime().block_on(session.execute(conn.caller(), "host.delete", json!({ "id": uuid::Uuid::new_v4() })));
    agents_page(&mut h);
    until(&mut h, "the log", |app| app.ui.agents.log.as_ref().is_some_and(|l| !l.is_empty()));
    assert!(h.query_all_by_label_contains("host.delete").next().is_some(), "the denied delete is in the log");
    h.get_by_label("Clear the log").click();
    let deadline = Instant::now() + Duration::from_secs(10);
    while run(&session, "agent.log", json!({})).as_array().is_some_and(|l| !l.is_empty()) {
        assert!(Instant::now() < deadline, "the log is still there");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

mod link {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use egui_kittest::kittest::Queryable;
    use serde_json::json;
    use sshcraft_engine::consent::Consent;
    use sshcraft_link::{Client, ConnectionFile, Token};

    use super::{H, agents_page};
    use sshcraft_ui_egui::testing::app_harness;

    /// The suite's flag and the launcher's switch, as the test sets them.
    #[derive(Default)]
    struct Suite {
        flag: AtomicBool,
        launcher: AtomicBool,
    }

    fn with_suite(h: &mut H) -> Arc<Suite> {
        let suite = Arc::new(Suite::default());
        let seen = suite.clone();
        h.state_mut().link.source = Arc::new(move |allow| Consent {
            allow_setting: allow,
            mcp_flag: seen.flag.load(Ordering::SeqCst),
            launcher_allows: seen.launcher.load(Ordering::SeqCst),
        });
        suite
    }

    fn wait(h: &mut H, what: &str, done: impl Fn(&H) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !done(h) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(50));
            h.run_steps(1);
        }
    }

    fn file(h: &H) -> Option<ConnectionFile> {
        h.state().link.dir.as_deref().and_then(ConnectionFile::read)
    }

    /// With the window minimized eframe runs no frame, only `App::logic`: the consent is still
    /// followed.
    #[test]
    fn the_link_follows_the_consent_while_the_window_is_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, session, ctx) = sshcraft_ui_egui::testing::app_on(dir.path());
        let mut settings = session.runtime().block_on(session.execute(sshcraft_engine::Caller::User, "settings.get", json!({}))).unwrap();
        settings["integrations"]["allow_agent_control"] = json!(true);
        session.runtime().block_on(session.execute(sshcraft_engine::Caller::User, "settings.save", json!({ "settings": settings }))).unwrap();
        let allowed = Arc::new(AtomicBool::new(true));
        let seen = allowed.clone();
        app.link.source = Arc::new(move |_| Consent { allow_setting: seen.load(Ordering::SeqCst), mcp_flag: false, launcher_allows: false });
        let dir_of = app.link.dir.clone().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while ConnectionFile::read(&dir_of).is_none() {
            assert!(Instant::now() < deadline, "the link did not start without a frame");
            app.logic(&ctx);
            std::thread::sleep(Duration::from_millis(50));
        }
        allowed.store(false, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(10);
        while ConnectionFile::read(&dir_of).is_some() {
            assert!(Instant::now() < deadline, "the link did not stop without a frame");
            app.logic(&ctx);
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Consent taken back while the link is starting: it stops as soon as the start is done, not
    /// two seconds later.
    #[test]
    fn consent_taken_back_during_the_start_stops_the_link_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _session, ctx) = sshcraft_ui_egui::testing::app_on(dir.path());
        let allowed = Arc::new(AtomicBool::new(true));
        let seen = allowed.clone();
        app.link.source = Arc::new(move |_| Consent { allow_setting: seen.load(Ordering::SeqCst), mcp_flag: false, launcher_allows: false });
        let dir_of = app.link.dir.clone().unwrap();
        // This look starts the link; the consent goes before the start is done.
        app.logic(&ctx);
        allowed.store(false, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(10);
        while ConnectionFile::read(&dir_of).is_none() {
            assert!(Instant::now() < deadline, "the start never finished");
            app.logic(&ctx);
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = Instant::now();
        while ConnectionFile::read(&dir_of).is_some() {
            assert!(started.elapsed() < Duration::from_millis(1000), "the link ran on without consent");
            app.logic(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// A drag over the port does not change it (each change would restart the link and cut the
    /// agents off); a typed port counts when the field is left.
    #[test]
    fn the_port_changes_only_when_typed_and_left() {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, session) = app_harness(dir.path());
        agents_page(&mut h);
        h.run_steps(3);
        let port = |session: &sshcraft_engine::Session| {
            session.runtime().block_on(session.execute(sshcraft_engine::Caller::User, "settings.get", json!({}))).unwrap()["integrations"]["agent_port"].clone()
        };
        let field = h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Port (0: any free port)").rect();
        let from = field.center();
        for (pos, pressed) in [(from, Some(true)), (from + egui::vec2(60.0, 0.0), None), (from + egui::vec2(120.0, 0.0), Some(false))] {
            h.event(egui::Event::PointerMoved(pos));
            if let Some(pressed) = pressed {
                h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            }
            h.run_steps(1);
        }
        h.run_steps(5);
        assert_eq!(port(&session), json!(0), "a drag changed nothing");
        h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Port (0: any free port)").click();
        h.run_steps(1);
        h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.event(egui::Event::Text("5123".into()));
        h.run_steps(2);
        assert_eq!(port(&session), json!(0), "not while typing");
        h.event(egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
        h.run_steps(3);
        let deadline = Instant::now() + Duration::from_secs(10);
        while port(&session) != json!(5123) {
            assert!(Instant::now() < deadline, "the typed port was not saved");
            std::thread::sleep(Duration::from_millis(20));
            h.run_steps(1);
        }
    }

    #[test]
    fn the_link_follows_the_consent() {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, session) = app_harness(dir.path());
        let suite = with_suite(&mut h);
        agents_page(&mut h);
        h.run_steps(3);
        assert!(file(&h).is_none(), "off until the user turns it on");
        h.get_by_label("Allow agent control").click();
        wait(&mut h, "the link", |h| file(h).is_some());
        let f = file(&h).unwrap();
        let token = Token::load_or_create(&f.token_file).unwrap();
        let client = session.runtime().block_on(Client::connect(std::net::SocketAddr::from(([127, 0, 0, 1], f.port)), &token)).unwrap();
        let shown = format!("127.0.0.1:{}", f.port);
        wait(&mut h, "the page to say where agents connect", |h| h.query_by_label_contains(&shown).is_some());
        h.get_by_label("Allow agent control").click();
        wait(&mut h, "the link to stop", |h| file(h).is_none());
        std::thread::sleep(Duration::from_millis(200));
        assert!(session.runtime().block_on(client.call("ping", json!({}))).is_err(), "the agent was cut off");
        // With the suite's flag, the launcher must name SSHCraft as well.
        suite.flag.store(true, Ordering::SeqCst);
        h.get_by_label("Allow agent control").click();
        std::thread::sleep(Duration::from_millis(2500));
        h.run_steps(5);
        assert!(file(&h).is_none(), "the launcher says no");
        assert!(h.query_by_label_contains("ArtCraft Launcher").is_some(), "the page says why");
        suite.launcher.store(true, Ordering::SeqCst);
        wait(&mut h, "the link with the launcher's yes", |h| file(h).is_some());
    }
}

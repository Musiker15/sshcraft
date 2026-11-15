//! Suggestions while typing: history, then snippets, then paths; from two characters; taking one
//! types only the missing rest and never runs it.

use std::time::{Duration, Instant};

use sshcraft_ui_egui::completion::{PathCache, Suggestion, path_word, resolve_dir, suggest};
use sshcraft_ui_egui::data::SnippetRow;

fn snippet(name: &str, content: &str) -> SnippetRow {
    SnippetRow { id: name.into(), name: name.into(), package: String::new(), content: content.into(), default_targets: Vec::new() }
}

#[test]
fn suggestions_start_at_two_characters() {
    let history = vec!["git status".to_string()];
    assert!(suggest("g", &history, &[], &[], false).is_empty());
    let found = suggest("gi", &history, &[], &[], false);
    assert_eq!(found.first().map(|s| s.insert.as_slice()), Some(b"t status".as_slice()), "only the missing rest");
}

#[test]
fn history_comes_before_snippets_before_paths() {
    let history = vec!["docker ps".to_string()];
    let snippets = [snippet("deploy", "docker compose up -d")];
    let paths = vec!["/docs".to_string()];
    let found = suggest("do", &history, &snippets, &paths, false);
    let kinds: Vec<&str> = found.iter().map(|s| s.kind).collect();
    assert_eq!(kinds, ["history", "snippet"], "{found:?}");
    let shown: Vec<&str> = found.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(shown, ["docker ps", "docker compose up -d"]);
}

#[test]
fn a_snippet_by_name_replaces_the_line() {
    let snippets = [snippet("restart-app", "sudo systemctl restart app")];
    let found: Vec<Suggestion> = suggest("resta", &[], &snippets, &[], false);
    let by_name = found.iter().find(|s| s.kind == "snippet").expect("found by its name");
    let mut expected = vec![0x7f; 5];
    expected.extend_from_slice(b"sudo systemctl restart app");
    assert_eq!(by_name.insert, expected, "the line is erased, then the snippet pasted");
}

#[test]
fn a_snippet_by_name_is_pasted_and_never_runs() {
    let snippets = [snippet("two-steps", "cd /srv\nrm -rf cache"), snippet("tabbed", "ls\t-la")];
    let multi = |bracketed| suggest("two", &[], &snippets, &[], bracketed).into_iter().find(|s| s.label == "two-steps");
    assert!(multi(false).is_none(), "several lines are not offered where the shell cannot take a paste");
    let pasted = multi(true).expect("offered as a paste").insert;
    let body: Vec<u8> = pasted.iter().copied().skip_while(|b| *b == 0x7f).collect();
    assert!(body.starts_with(b"\x1b[200~") && body.ends_with(b"\x1b[201~"), "{body:?}");
    assert!(!body.contains(&b'\r'), "no Enter runs a line");
    let tabbed = suggest("tab", &[], &snippets, &[], false).into_iter().find(|s| s.label == "tabbed").unwrap().insert;
    assert!(!tabbed.contains(&b'\t'), "a Tab would start the shell's completion");
}

#[test]
fn paths_need_a_slash() {
    assert_eq!(path_word("cd /us"), Some(("/".to_string(), "us".to_string())));
    assert_eq!(path_word("ls ~/pro"), Some(("~/".to_string(), "pro".to_string())));
    assert_eq!(path_word("cat ./src/ma"), Some(("./src/".to_string(), "ma".to_string())));
    assert_eq!(path_word("vim conf/app.y"), Some(("conf/".to_string(), "app.y".to_string())));
    assert_eq!(path_word("ls foo"), None);
    assert_eq!(path_word("ls "), None);
}

#[test]
fn relative_paths_start_at_the_shell_directory() {
    assert_eq!(resolve_dir("src/", Some("/srv/app")).as_deref(), Some("/srv/app/src/"));
    assert_eq!(resolve_dir("./", Some("/srv/app/")).as_deref(), Some("/srv/app/./"));
    assert_eq!(resolve_dir("/etc/", Some("/srv")).as_deref(), Some("/etc/"));
    assert_eq!(resolve_dir("~/x/", None).as_deref(), Some("~/x/"));
    assert_eq!(resolve_dir("src/", None), None, "without the shell's directory a relative path is not guessed");
}

#[test]
fn listings_are_cached_10_s() {
    let mut cache = PathCache::default();
    let now = Instant::now();
    cache.put("/srv/", vec!["app".into()], now);
    assert_eq!(cache.get("/srv/", now + Duration::from_secs(9)), Some(&["app".to_string()][..]));
    assert_eq!(cache.get("/srv/", now + Duration::from_secs(11)), None, "older than 10 s");
}

mod in_the_window {
    use std::time::{Duration, Instant};

    use egui::{Event, Key, Modifiers};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;
    use sshcraft_engine::testing::test_host;
    use sshcraft_engine::{Caller, Session, SessionId, SessionStatus};
    use sshcraft_model::model::HostId;
    use sshcraft_ssh::testing::TestAuth;
    use sshcraft_ui_egui::SshcraftApp;
    use sshcraft_ui_egui::state::ConnectRequest;
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

    fn connected_pane(h: &mut H, session: &Session) -> (SessionId, HostId, sshcraft_ssh::testing::TestServer) {
        let (server, host) = session.runtime().block_on(test_host(session, "web", TestAuth::default(), true));
        h.state_mut().open_new_tab();
        h.run_steps(2);
        h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
        until(h, "the session", |app| !app.terminals.is_empty());
        let id = *h.state().terminals.keys().next().unwrap();
        until(h, "the connection", |app| app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. }))));
        h.run_steps(5);
        // The test shell shows no prompt; history keeps only lines typed after one.
        h.state_mut().terminals.get_mut(&id).unwrap().term.feed(b"$ ");
        (id, host, server)
    }

    fn key(h: &mut H, key: Key) {
        for pressed in [true, false] {
            h.event(Event::Key { key, pressed, modifiers: Modifiers::NONE, repeat: false, physical_key: None });
            h.run_steps(1);
        }
    }

    fn typed(h: &mut H, text: &str) {
        h.event(Event::Text(text.into()));
        h.run_steps(2);
    }

    fn history(session: &Session, host: HostId) -> Vec<String> {
        let rows = session.runtime().block_on(session.execute(Caller::User, "history.list", json!({ "scope": format!("host:{}", host.0) }))).unwrap();
        rows.as_array().unwrap().iter().filter_map(|r| r["command"].as_str().map(str::to_string)).collect()
    }

    fn screen(app: &SshcraftApp, id: SessionId) -> String {
        app.terminals.get(&id).map(|t| t.term.visible_text()).unwrap_or_default()
    }

    #[test]
    fn a_typed_line_is_kept_and_suggested_and_down_tab_takes_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, session) = app_harness(dir.path());
        let (id, host, _server) = connected_pane(&mut h, &session);
        typed(&mut h, "echo kept-line");
        key(&mut h, Key::Enter);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !history(&session, host).contains(&"echo kept-line".to_string()) {
            assert!(Instant::now() < deadline, "the line was not kept");
            std::thread::sleep(Duration::from_millis(20));
            h.run_steps(1);
        }
        // The test shell echoes Enter as a carriage return: a clean screen shows what follows.
        h.state_mut().terminals.get_mut(&id).unwrap().term.feed(b"\x1b[2J\x1b[H$ ");
        typed(&mut h, "ec");
        until(&mut h, "the suggestion", |app| app.terminals.get(&id).is_some_and(|t| t.suggestions().iter().any(|s| s.label == "echo kept-line")));
        assert!(!screen(h.state(), id).contains("echo kept-line"), "only what was typed so far");
        key(&mut h, Key::ArrowDown);
        key(&mut h, Key::Tab);
        until(&mut h, "the taken rest", |app| screen(app, id).contains("echo kept-line"));
        assert!(h.state().terminals[&id].suggestions().is_empty() || h.state().terminals[&id].selected().is_none(), "the list closed");
    }

    #[test]
    fn enter_without_a_selection_goes_to_the_shell() {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, session) = app_harness(dir.path());
        let (id, host, _server) = connected_pane(&mut h, &session);
        session
            .runtime()
            .block_on(session.execute(Caller::User, "history.record", json!({ "scope": format!("host:{}", host.0), "command": "echo old" })))
            .unwrap();
        h.state_mut().terminals.get_mut(&id).unwrap().reload_history();
        h.run_steps(3);
        typed(&mut h, "ec");
        until(&mut h, "the suggestion", |app| app.terminals.get(&id).is_some_and(|t| !t.suggestions().is_empty()));
        key(&mut h, Key::Enter);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !history(&session, host).contains(&"ec".to_string()) {
            assert!(Instant::now() < deadline, "Enter reached the shell and ended the line 'ec'");
            std::thread::sleep(Duration::from_millis(20));
            h.run_steps(1);
        }
        assert!(!screen(h.state(), id).contains("echo old"), "nothing was taken");
    }

    /// Types `text` and Enter; waits until the history has it.
    fn kept_line(h: &mut H, session: &Session, host: HostId, text: &str) {
        typed(h, text);
        key(h, Key::Enter);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !history(session, host).contains(&text.to_string()) {
            assert!(Instant::now() < deadline, "{text} was not kept");
            std::thread::sleep(Duration::from_millis(20));
            h.run_steps(1);
        }
    }

    /// Types `ec` on a clean prompt and lets the suggestions come.
    fn type_ec(h: &mut H, id: SessionId) {
        h.state_mut().terminals.get_mut(&id).unwrap().term.feed(b"\x1b[2J\x1b[H$ ");
        typed(h, "ec");
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(20));
            h.run_steps(1);
        }
    }

    fn suggests(app: &SshcraftApp, id: SessionId, label: &str) -> bool {
        app.terminals.get(&id).is_some_and(|t| t.suggestions().iter().any(|s| s.label == label))
    }

    #[test]
    fn clearing_the_history_in_the_sidebar_takes_it_out_of_the_suggestions() {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, session) = app_harness(dir.path());
        let (id, host, _server) = connected_pane(&mut h, &session);
        kept_line(&mut h, &session, host, "echo secret-line");
        sshcraft_ui_egui::commands::run(h.state_mut(), "ui.terminal.sidebar");
        until(&mut h, "the sidebar", |_| true);
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.query_by_label("echo secret-line").is_none() {
            assert!(Instant::now() < deadline, "the sidebar lists the line");
            std::thread::sleep(Duration::from_millis(20));
            h.run_steps(1);
        }
        h.get_by_label("Clear").click();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !history(&session, host).is_empty() {
            assert!(Instant::now() < deadline, "not cleared");
            std::thread::sleep(Duration::from_millis(20));
            h.run_steps(1);
        }
        type_ec(&mut h, id);
        assert!(!suggests(h.state(), id, "echo secret-line"), "a cleared line is never suggested");
    }

    #[test]
    fn turning_the_history_off_takes_it_out_of_the_suggestions() {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, session) = app_harness(dir.path());
        let (id, host, _server) = connected_pane(&mut h, &session);
        kept_line(&mut h, &session, host, "echo secret-line");
        h.state_mut().ui.page = sshcraft_ui_egui::state::Page::Settings;
        h.state_mut().ui.settings.section = sshcraft_ui_egui::pages::settings::Section::Terminal;
        h.run_steps(3);
        h.get_by_label("Keep the commands typed in terminals").click();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !history(&session, host).is_empty() {
            assert!(Instant::now() < deadline, "not cleared");
            std::thread::sleep(Duration::from_millis(20));
            h.run_steps(1);
        }
        h.state_mut().ui.page = sshcraft_ui_egui::state::Page::Tab(0);
        h.run_steps(3);
        type_ec(&mut h, id);
        assert!(!suggests(h.state(), id, "echo secret-line"), "a cleared line is never suggested");
    }

    #[test]
    fn the_sidebar_shows_a_line_typed_while_it_is_open() {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, session) = app_harness(dir.path());
        let (_id, host, _server) = connected_pane(&mut h, &session);
        sshcraft_ui_egui::commands::run(h.state_mut(), "ui.terminal.sidebar");
        h.run_steps(5);
        kept_line(&mut h, &session, host, "echo later-line");
        h.run_steps(5);
        assert!(h.query_by_label("echo later-line").is_some(), "the sidebar has the new line");
    }

    #[test]
    fn the_list_leaves_keys_alone_while_the_keyboard_is_elsewhere() {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, session) = app_harness(dir.path());
        let (id, host, _server) = connected_pane(&mut h, &session);
        kept_line(&mut h, &session, host, "echo kept-line");
        sshcraft_ui_egui::commands::run(h.state_mut(), "ui.terminal.sidebar");
        h.run_steps(3);
        type_ec(&mut h, id);
        until(&mut h, "the suggestion", |app| suggests(app, id, "echo kept-line"));
        // The keyboard goes to the sidebar's search field.
        h.get_by_role(egui::accesskit::Role::TextInput).click();
        h.run_steps(2);
        key(&mut h, Key::ArrowDown);
        key(&mut h, Key::Enter);
        for _ in 0..15 {
            std::thread::sleep(Duration::from_millis(20));
            h.run_steps(1);
        }
        assert_eq!(h.state().terminals[&id].selected(), None, "Down went to the search field");
        assert!(!screen(h.state(), id).contains("echo kept-line"), "Enter took nothing into the terminal");
    }

    #[test]
    fn history_off_keeps_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, session) = app_harness(dir.path());
        let mut settings = session.runtime().block_on(session.execute(Caller::User, "settings.get", json!({}))).unwrap();
        settings["terminal"]["history"] = json!(false);
        session.runtime().block_on(session.execute(Caller::User, "settings.save", json!({ "settings": settings }))).unwrap();
        let (_id, host, _server) = connected_pane(&mut h, &session);
        typed(&mut h, "echo secret-ish");
        key(&mut h, Key::Enter);
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(20));
            h.run_steps(1);
        }
        assert!(history(&session, host).is_empty());
    }
}

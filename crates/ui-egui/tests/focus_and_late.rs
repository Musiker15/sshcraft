//! Where typing goes and where late answers land: a question over a terminal takes the keyboard,
//! the shown terminal gets it back, a slow command's answer never reaches a dialog opened after
//! it, and a form waits for its command before it can be sent again.

use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{Session, SessionId, SessionStatus};
use sshcraft_model::model::HostId;
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::pages::keys::{self, KeyType};
use sshcraft_ui_egui::state::{ConnectRequest, Page};
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn until(h: &mut H, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    until_within(h, what, Duration::from_secs(20), done);
}

/// An RSA key of 4096 bits, made in a debug build on a busy CI machine, can take a minute.
const SLOW_KEY: Duration = Duration::from_secs(180);

fn until_within(h: &mut H, what: &str, within: Duration, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + within;
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}; messages: {:?}", h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn host(session: &Session, name: &str, auth: TestAuth, trusted: bool) -> (TestServer, HostId) {
    session.runtime().block_on(test_host(session, name, auth, trusted))
}

fn connect(h: &mut H, host: HostId) -> SessionId {
    let before: Vec<SessionId> = h.state().terminals.keys().copied().collect();
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the tab", |app| app.terminals.keys().any(|id| !before.contains(id)));
    *h.state().terminals.keys().find(|id| !before.contains(id)).unwrap()
}

fn connected(app: &SshcraftApp, id: SessionId) -> bool {
    app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. })))
}

fn screen(app: &SshcraftApp, id: SessionId) -> String {
    app.terminals.get(&id).map(|t| t.term.visible_text()).unwrap_or_default()
}

/// Types `text` and Enter as a keyboard would, without clicking anywhere first.
fn type_line(h: &mut H, text: &str) {
    h.event(Event::Text(text.into()));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::Key { key: Key::Enter, pressed, modifiers: Modifiers::NONE, repeat: false, physical_key: None });
        h.run_steps(1);
    }
}

/// Lets the shell echo what reached it.
fn settle(h: &mut H) {
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

#[test]
fn typing_during_a_sign_in_question_never_reaches_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let auth = TestAuth { key_then_kbd: true, kbd: vec![vec![("Password: ".into(), false, "pw".into())]], ..Default::default() };
    let (_server, id) = host(&session, "web", auth, true);
    h.run_steps(2);
    let tab = connect(&mut h, id);
    until(&mut h, "the question", |app| !app.prompts.open.is_empty());
    type_line(&mut h, "typed-early");
    assert_eq!(h.state().prompts.open[0].answers[0], "typed-early", "the answer field has the keyboard");
    h.state_mut().prompts.open[0].answers[0] = "pw".into();
    h.run_steps(30);
    h.get_by_label("Sign in").click();
    until(&mut h, "the connection", |app| connected(app, tab));
    settle(&mut h);
    assert!(!screen(h.state(), tab).contains("typed-early"), "nothing typed during the question reached the shell");
    assert_eq!(h.state().prompts.open.len(), 0);
}

#[test]
fn a_sign_in_question_of_a_hidden_tab_does_not_take_the_keyboard() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let auth = TestAuth { key_then_kbd: true, kbd: vec![vec![("Password: ".into(), false, "pw".into())]], ..Default::default() };
    let (_server, id) = host(&session, "web", auth, true);
    h.run_steps(2);
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(id));
    h.run_steps(1);
    // The user went on to another page before the question came.
    h.state_mut().ui.page = Page::Keys;
    until(&mut h, "the question", |app| !app.prompts.open.is_empty());
    assert_eq!(h.state().ui.page, Page::Keys);
    type_line(&mut h, "meant-elsewhere");
    assert_eq!(
        h.state().prompts.open[0].answers[0],
        "",
        "typing meant for another place stays out of the answer; page {:?} session {:?}",
        h.state().ui.page,
        h.state().prompts.open[0].session
    );
}

#[test]
fn typing_during_a_host_key_question_never_reaches_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "new", TestAuth::default(), false);
    h.run_steps(2);
    let tab = connect(&mut h, id);
    until(&mut h, "the question", |app| !app.prompts.open.is_empty());
    type_line(&mut h, "typed-early");
    assert_eq!(h.ctx.memory(|m| m.focused()), None, "the terminal behind the question has no keyboard");
    h.run_steps(30);
    h.get_by_label("Accept").click();
    until(&mut h, "the connection", |app| connected(app, tab));
    settle(&mut h);
    assert!(!screen(h.state(), tab).contains("typed-early"));
}

#[test]
fn tab_then_enter_never_accepts_a_host_key() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "new", TestAuth::default(), false);
    h.run_steps(2);
    let tab = connect(&mut h, id);
    until(&mut h, "the question", |app| !app.prompts.open.is_empty());
    std::thread::sleep(Duration::from_millis(600));
    h.run_steps(5);
    for _ in 0..4 {
        for key in [Key::Tab, Key::Enter, Key::Space] {
            for pressed in [true, false] {
                h.event(Event::Key { key, pressed, modifiers: Modifiers::NONE, repeat: false, physical_key: None });
                h.run_steps(1);
            }
        }
    }
    settle(&mut h);
    assert_eq!(h.state().prompts.open.len(), 1, "no key accepts the host key");
    assert!(!connected(h.state(), tab));
}

#[test]
fn the_shown_terminal_gets_the_keyboard_after_switching_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_a, host_a) = host(&session, "alpha", TestAuth::default(), true);
    let (_b, host_b) = host(&session, "beta", TestAuth::default(), true);
    h.run_steps(2);
    let first = connect(&mut h, host_a);
    until(&mut h, "the first connection", |app| connected(app, first));
    let second = connect(&mut h, host_b);
    until(&mut h, "the second connection", |app| connected(app, second));
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.tab.previous");
    h.run_steps(3);
    h.event(Event::Text("to-the-first".into()));
    until(&mut h, "the echo", |app| screen(app, first).contains("to-the-first"));
    assert!(!screen(h.state(), second).contains("to-the-first"));
}

#[test]
fn a_connection_whose_tab_was_closed_while_connecting_does_not_come_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "web", TestAuth::default(), true);
    h.run_steps(2);
    h.state_mut().open_new_tab();
    h.run_steps(2);
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(id));
    // The request goes out in this frame; the user closes the tab before the answer.
    h.run_steps(1);
    h.state_mut().close_tab();
    settle(&mut h);
    settle(&mut h);
    assert!(h.state().ui.tabs.is_empty(), "the closed tab does not come back as a tab of its own");
    assert!(h.state().terminals.is_empty());
}

#[test]
fn a_late_connection_does_not_take_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "web", TestAuth::default(), true);
    h.run_steps(2);
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(id));
    // The request goes out in this frame; its answer comes on a later one.
    h.run_steps(1);
    h.state_mut().ui.page = Page::Keys;
    until(&mut h, "the tab", |app| !app.terminals.is_empty());
    assert_eq!(h.state().ui.page, Page::Keys, "the user stays where they went");
    assert_eq!(h.state().ui.tabs.len(), 1, "the tab is there for later");
}

fn keys_page(h: &mut H) {
    h.state_mut().ui.page = Page::Keys;
    h.run_steps(3);
}

fn open_generate(h: &mut H, name: &str) {
    h.get_by_label("Generate key").click();
    h.run_steps(3);
    let form = h.state_mut().ui.key_dialog.as_mut().and_then(keys::KeyDialog::generate_mut).unwrap();
    form.name = name.into();
    // RSA takes long enough to click again before it is done.
    form.kind = KeyType::Rsa(4096);
    h.run_steps(1);
}

#[test]
fn a_form_waits_for_its_command_before_it_can_be_sent_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    keys_page(&mut h);
    open_generate(&mut h, "slow");
    h.get_by_label("Generate").click();
    h.run_steps(2);
    assert!(h.get_by_label("Generate").accesskit_node().is_disabled(), "the button waits for the key");
    h.get_by_label("Generate").click();
    until_within(&mut h, "the key", SLOW_KEY, |app| !app.data.keys.is_empty() && app.runner.busy() == 0);
    h.run_steps(5);
    assert_eq!(h.state().data.keys.len(), 1, "one key, not two");
}

#[test]
fn a_late_answer_does_not_close_the_next_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    keys_page(&mut h);
    open_generate(&mut h, "slow");
    h.get_by_label("Generate").click();
    h.run_steps(1);
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    h.get_by_label("Import key").click();
    h.run_steps(3);
    h.state_mut().ui.key_dialog.as_mut().and_then(keys::KeyDialog::import_mut).unwrap().name = "typed".into();
    until_within(&mut h, "the slow key", SLOW_KEY, |app| !app.data.keys.is_empty());
    let import = h.state_mut().ui.key_dialog.as_mut().and_then(keys::KeyDialog::import_mut).map(|f| f.name.clone());
    assert_eq!(import.as_deref(), Some("typed"), "the import dialog and what was typed stay");
}

#[test]
fn a_closed_proton_pass_dialog_stays_closed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    keys_page(&mut h);
    h.get_by_label("Import from Proton Pass").click();
    h.run_steps(1);
    h.state_mut().ui.passcli = None;
    until(&mut h, "the status", |app| app.runner.busy() == 0);
    assert!(h.state().ui.passcli.is_none());
}

#[test]
fn a_secret_field_keeps_no_undo_history() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let auth = TestAuth { key_then_kbd: true, kbd: vec![vec![("Password: ".into(), false, "pw".into())]], ..Default::default() };
    let (_server, id) = host(&session, "web", auth, true);
    h.run_steps(2);
    connect(&mut h, id);
    until(&mut h, "the question", |app| !app.prompts.open.is_empty());
    let prompt = h.state().prompts.open[0].id;
    h.event(Event::Text("s3cret".into()));
    h.run_steps(3);
    let field = sshcraft_ui_egui::prompts::answer_id(prompt, 0);
    let state = egui::text_edit::TextEditState::load(&h.ctx, field).expect("the field was drawn");
    let current = (egui::text::CCursorRange::one(egui::text::CCursor::new(6)), "s3cret".to_string());
    assert!(!state.undoer().has_undo(&current), "nothing to undo back to");
}

#[test]
fn every_dialog_counts_for_late_answers() {
    use sshcraft_ui_egui::forms::shape;
    use sshcraft_ui_egui::state::UiState;
    let closed = shape(&UiState::default());
    type Open = fn(&mut UiState);
    let opened: [(&str, Open); 4] = [
        ("create account", |ui| ui.sync.create = Some(Default::default())),
        ("join account", |ui| ui.sync.join = Some(Default::default())),
        ("delete logs", |ui| ui.logs.confirm_delete = Some(vec!["a".into()])),
        ("a confirmation", |ui| {
            ui.ask = Some(sshcraft_ui_egui::confirm::Confirm {
                title: String::new(),
                body: String::new(),
                button: "Go",
                pending: sshcraft_ui_egui::confirm::Pending::DeleteSnippet { id: String::new() },
            })
        }),
    ];
    for (what, open) in opened {
        let mut ui = UiState::default();
        open(&mut ui);
        assert_ne!(shape(&ui), closed, "{what}");
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    sshcraft_ui_egui::pages::sync::ask_move(h.state_mut(), "host", "x", "web");
    assert_ne!(shape(&h.state().ui), closed, "move to a server");
}

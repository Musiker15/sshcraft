//! The menu bar, the window's own title bar and quitting: a menu item runs its command, a menu
//! runs nothing while a question is open, quitting with running sessions asks first, and the
//! system title bar is a setting.

use std::time::{Duration, Instant};

use craft_menu::{CommandRef, Target};
use egui::accesskit::Role;
use egui::{ViewportCommand, ViewportEvent, ViewportId};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sshcraft_engine::Session;
use sshcraft_engine::testing::test_host;
use sshcraft_model::model::HostId;
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ui_egui::SshcraftApp;
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

fn host(session: &Session, name: &str, trusted: bool) -> (TestServer, HostId) {
    session.runtime().block_on(test_host(session, name, TestAuth::default(), trusted))
}

/// Opens the menu titled `menu` and chooses its item `item`, through the accessibility tree as a
/// screen reader would (it reaches the bar also while a dialog's backdrop covers it).
fn choose(h: &mut H, menu: &str, item: &str) {
    h.get_by_role_and_label(Role::MenuItem, menu).click_accesskit();
    h.run_steps(2);
    h.get_by_role_and_label(Role::MenuItem, item).click_accesskit();
    h.run_steps(3);
}

/// The window asked to close, as its close button or Alt+F4 would.
fn request_close(h: &mut H) {
    h.input_mut().viewports.entry(ViewportId::ROOT).or_default().events.push(ViewportEvent::Close);
    h.step();
}

/// Whether the last frame told the window to stop closing.
fn cancelled_close(h: &H) -> bool {
    h.output().viewport_output.get(&ViewportId::ROOT).is_some_and(|v| v.commands.contains(&ViewportCommand::CancelClose))
}

/// Whether the last frame told the window to close.
fn sent_close(h: &H) -> bool {
    h.output().viewport_output.get(&ViewportId::ROOT).is_some_and(|v| v.commands.contains(&ViewportCommand::Close))
}

#[test]
fn the_menu_runs_its_command() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    assert!(h.state().ui.tabs.is_empty());
    choose(&mut h, "File", "New tab");
    assert_eq!(h.state().ui.tabs.len(), 1, "File › New tab opens a tab");
    assert_eq!(h.state().ui.page, Page::Tab(0));
    choose(&mut h, "View", "Keys");
    assert_eq!(h.state().ui.page, Page::Keys);
}

#[test]
fn the_menu_is_closed_while_a_question_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    // An unknown host key: the engine asks before it trusts it.
    let (_server, id) = host(&session, "web", false);
    let _window = session.subscribe();
    h.run_steps(2);
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(id));
    until(&mut h, "the question", |app| !app.prompts.open.is_empty());
    let tabs = h.state().ui.tabs.len();
    choose(&mut h, "File", "New tab");
    assert_eq!(h.state().ui.tabs.len(), tabs, "no tab opened behind the question");
    assert_eq!(h.state().prompts.open.len(), 1, "the question stays");
}

#[test]
fn quitting_without_sessions_just_quits() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    request_close(&mut h);
    assert!(!cancelled_close(&h), "nothing runs, nothing to ask");
    assert!(h.query_by_label("End sessions and quit").is_none());
}

#[test]
fn quitting_with_sessions_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (_server, id) = host(&session, "web", true);
    h.run_steps(2);
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(id));
    until(&mut h, "the session", |app| !app.terminals.is_empty());

    request_close(&mut h);
    assert!(cancelled_close(&h), "the window stays while the question is open");
    h.run_steps(3);
    assert!(h.query_by_label("End sessions and quit").is_some(), "the question is shown");
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert!(h.query_by_label("End sessions and quit").is_none());
    assert_eq!(h.state().terminals.len(), 1, "Cancel leaves the session running");

    // The menu's Quit asks the same question (File › Exit on Windows, File › Quit on Linux; on
    // macOS it sits in the app menu).
    if cfg!(target_os = "macos") {
        sshcraft_ui_egui::commands::run(h.state_mut(), "ui.app.quit");
    } else {
        choose(&mut h, "File", if cfg!(windows) { "Exit" } else { "Quit" });
    }
    h.run_steps(3);
    assert!(h.query_by_label("End sessions and quit").is_some(), "Quit from the menu asks too");
    h.get_by_label("End sessions and quit").click();
    h.step();
    assert!(sent_close(&h), "the answer closes the window");
    request_close(&mut h);
    assert!(!cancelled_close(&h), "a close after the yes goes through");
}

#[test]
fn the_system_title_bar_is_a_setting() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    assert_eq!(sshcraft_ui_egui::window::title_bar_pref(&h.state().data.settings), craft_window::policy::TitleBarPref::Integrated);
    h.state_mut().ui.page = Page::Settings;
    h.run_steps(3);
    h.get_by_label("General").click();
    h.run_steps(3);
    h.get_by_label("Use system title bar").click();
    until(&mut h, "the saved setting", |app| app.data.settings.general.system_title_bar);
    assert_eq!(sshcraft_ui_egui::window::title_bar_pref(&h.state().data.settings), craft_window::policy::TitleBarPref::System);
    assert!(h.query_by_label_contains("after a restart").is_some(), "the page says when it takes effect");
}

#[test]
fn menu_items_show_their_shortcuts() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    let shortcut = |app: &SshcraftApp, id: &str| {
        let bar = craft_menu::layout(&sshcraft_ui_egui::menus::menu_bar(app), Target::Linux, &sshcraft_ui_egui::menus::layout_options()).bar;
        bar.find(&CommandRef::new(id)).and_then(|item| item.shortcut)
    };
    assert_eq!(shortcut(h.state(), "ui.tab.new"), craft_keymap::Chord::parse("Ctrl+Shift+T").ok());
    assert_eq!(shortcut(h.state(), "ui.palette"), craft_keymap::Chord::parse("Ctrl+Shift+P").ok(), "the first of two");
    // A changed shortcut is what the menu shows.
    h.state_mut().bindings.assign("ui.tab.new", "Ctrl+Alt+N").unwrap();
    assert_eq!(shortcut(h.state(), "ui.tab.new"), craft_keymap::Chord::parse("Ctrl+Alt+N").ok());
    // And the open menu shows it next to the label.
    h.get_by_role_and_label(Role::MenuItem, "File").click_accesskit();
    h.run_steps(2);
    let shown = craft_keymap::Chord::parse("Ctrl+Alt+N").unwrap().display(craft_keymap::Platform::current());
    let row = h.get_by_role_and_label(Role::MenuItem, "New tab");
    assert_eq!(row.accesskit_node().data().keyboard_shortcut(), Some(shown.as_str()), "the menu shows {shown}");
}

#[test]
fn the_window_buttons_work_where_the_app_draws_the_frame() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    assert!(h.query_by_label("Minimize").is_none(), "the system draws the buttons by default in tests");
    let layout = craft_window::Layout {
        left: Vec::new(),
        right: vec![craft_window::Button::Minimize, craft_window::Button::Maximize, craft_window::Button::Close],
    };
    h.state_mut().window_frame =
        craft_window::policy::Frame::Integrated(craft_window::policy::Captions { family: craft_window::policy::CaptionFamily::Windows, layout });
    h.run_steps(2);
    let sent = |h: &H, command: &ViewportCommand| h.output().viewport_output.get(&ViewportId::ROOT).is_some_and(|v| v.commands.contains(command));
    h.get_by_role_and_label(Role::Button, "Minimize").click();
    h.step();
    assert!(sent(&h, &ViewportCommand::Minimized(true)));
    h.get_by_role_and_label(Role::Button, "Maximize").click();
    h.step();
    assert!(sent(&h, &ViewportCommand::Maximized(true)));
    h.get_by_role_and_label(Role::Button, "Close window").click();
    h.step();
    assert!(sent(&h, &ViewportCommand::Close));
}

/// Alt and `key` pressed and released as a keyboard sends them, Alt down first.
fn alt_key(h: &mut H, key: egui::Key) {
    let alt = egui::Modifiers::ALT;
    for (k, pressed, m) in [(egui::Key::AltLeft, true, alt), (key, true, alt), (key, false, alt), (egui::Key::AltLeft, false, egui::Modifiers::NONE)]
    {
        h.input_mut().events.push(egui::Event::Key { key: k, pressed, modifiers: m, repeat: false, physical_key: None });
    }
    h.run_steps(3);
}

#[test]
fn the_keyboard_stays_with_the_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    // Outside a terminal Alt+F opens File, as on Windows.
    h.run_steps(3);
    alt_key(&mut h, egui::Key::F);
    assert!(h.query_by_role_and_label(Role::MenuItem, "New tab").is_some(), "Alt+F opens File on a page");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::MenuItem, "New tab").is_none());

    let (_server, id) = host(&session, "web", true);
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(id));
    until(&mut h, "the session", |app| !app.terminals.is_empty() && app.in_terminal());
    // In a terminal Alt+F is the shell's (forward a word), F10 the program's, and a tap on Alt
    // must not take the arrow keys away from it.
    alt_key(&mut h, egui::Key::F);
    assert!(h.query_by_role_and_label(Role::MenuItem, "New tab").is_none(), "Alt+F stays with the terminal");
    h.key_press(egui::Key::F10);
    h.run_steps(2);
    for (pressed, m) in [(true, egui::Modifiers::ALT), (false, egui::Modifiers::NONE)] {
        h.input_mut().events.push(egui::Event::Key { key: egui::Key::AltLeft, pressed, modifiers: m, repeat: false, physical_key: None });
    }
    h.run_steps(2);
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::MenuItem, "New tab").is_none(), "no menu opened from the keyboard");
    assert!(!craft_menu::bar::is_focused(&h.ctx), "the bar did not take the keyboard");
}

//! The tablet form (Android): what the desktop offers and a tablet cannot do is left out, the
//! key bar under the terminal, and the finger on the terminal.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use egui::{Event, FontId, Key, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{SessionId, SessionStatus};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_term::{TermConfig, TermSize, Terminal};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::app::Form;
use sshcraft_ui_egui::pages::settings::Section;
use sshcraft_ui_egui::state::{ConnectRequest, Page};
use sshcraft_ui_egui::terminal::{TermPalette, TerminalView};
use sshcraft_ui_egui::testing::app_harness;

fn harness(dir: &std::path::Path, form: Form) -> Harness<'static, SshcraftApp> {
    let (mut h, _session) = app_harness(dir);
    h.state_mut().form = form;
    h.run_steps(3);
    h
}

fn settings(h: &mut Harness<'static, SshcraftApp>, section: Section) {
    h.state_mut().ui.page = Page::Settings;
    h.state_mut().ui.settings.section = section;
    h.run_steps(3);
}

#[test]
fn a_tablet_offers_no_local_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), Form::Tablet);
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::T);
    h.run_steps(2);
    assert!(h.query_by_label("Quick connect").is_some(), "the new-tab page is open");
    assert!(h.query_by_label("Local terminal").is_none());
    assert!(h.query_by_label("Serial port").is_none());
}

#[test]
fn a_tablet_offers_no_files_and_no_agents() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), Form::Tablet);
    settings(&mut h, Section::General);
    assert!(h.query_by_label("Create backup…").is_none());
    assert!(h.query_by_label("AI agents").is_none(), "no agents on a tablet");
    assert!(h.query_by_label("Use system title bar").is_none(), "no title bar on a tablet");
    settings(&mut h, Section::Appearance);
    assert!(h.query_by_label("Dropdown window").is_none());
    h.state_mut().ui.page = Page::Hosts;
    h.run_steps(3);
    assert!(h.query_by_label("New group").is_some(), "the hosts page is open");
    assert!(h.query_by_label("Import").is_none());
    assert!(h.query_by_label("Export").is_none());
}

/// The counter-check: the desktop keeps all of it.
#[test]
fn the_desktop_still_offers_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), Form::Desktop);
    settings(&mut h, Section::General);
    assert!(h.query_by_label("Create backup…").is_some());
    assert!(h.query_by_label("AI agents").is_some());
    settings(&mut h, Section::Appearance);
    assert!(h.query_by_label("Dropdown window").is_some());
    h.state_mut().ui.page = Page::Hosts;
    h.run_steps(3);
    h.get_by_label("More").click();
    h.run_steps(2);
    assert!(h.query_by_label("Import").is_some());
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::T);
    h.run_steps(2);
    assert!(h.query_by_label("Local terminal").is_some());
}

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

/// A tablet connected to a test server whose shell echoes what it gets.
fn connected_tablet(dir: &std::path::Path) -> (H, TestServer, SessionId) {
    let (mut h, session) = app_harness(dir);
    h.state_mut().form = Form::Tablet;
    let (server, host) = session.runtime().block_on(test_host(&session, "web", TestAuth::default(), true));
    h.run_steps(2);
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(&mut h, "the tab", |app| !app.terminals.is_empty());
    let tab = *h.state().terminals.keys().next().unwrap();
    until(&mut h, "the connection", |app| app.terminals.get(&tab).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. }))));
    h.run_steps(10);
    (h, server, tab)
}

fn shown(app: &SshcraftApp, tab: SessionId, text: &str) -> bool {
    app.terminals.get(&tab).is_some_and(|t| t.term.visible_text().contains(text))
}

/// Ctrl on the bar turns the next letter from the keyboard into a control key (Ctrl+H, a
/// backspace, takes back the `a`), lets go after it (the `Z` is plain), and tapping the bar
/// leaves the keyboard with the terminal (the letters typed after the taps arrive).
#[test]
fn the_key_bar_sends_ctrl_keys_and_keeps_the_terminal_focused() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _server, tab) = connected_tablet(dir.path());
    h.get_by_label("|").click();
    h.run_steps(2);
    h.event(Event::Text("a".into()));
    h.run_steps(2);
    until(&mut h, "the echo of |a", |app| shown(app, tab, "|a"));
    h.get_by_label("Ctrl").click();
    h.run_steps(2);
    assert!(h.state().key_bar.ctrl, "Ctrl is held");
    h.event(Event::Text("h".into()));
    h.run_steps(2);
    h.event(Event::Text("Z".into()));
    h.run_steps(2);
    until(&mut h, "the echo of |Z", |app| shown(app, tab, "|Z"));
    assert!(!h.state().key_bar.ctrl, "Ctrl let go after one key");
}

/// The on-screen keyboard is open while the terminal asks for text input (IME): a tap on the bar
/// must not drop that request for a single frame, or Android closes the keyboard.
#[test]
fn tapping_the_key_bar_keeps_the_keyboard_open() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _server, _tab) = connected_tablet(dir.path());
    assert!(h.output().platform_output.ime.is_some(), "the terminal asks for the keyboard");
    for label in ["Esc", "Ctrl", "←"] {
        h.get_by_label(label).click();
        for _ in 0..3 {
            h.step();
            assert!(h.output().platform_output.ime.is_some(), "the keyboard stayed open after tapping {label}");
        }
    }
}

#[test]
fn the_key_bar_pastes_from_the_system_clipboard() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _server, tab) = connected_tablet(dir.path());
    h.state_mut().paste_source = Some(std::sync::Arc::new(|| Some("pasted-text".to_string())));
    h.get_by_label("Paste").click();
    h.run_steps(2);
    until(&mut h, "the pasted text", |app| shown(app, tab, "pasted-text"));
}

/// The on-screen keyboard covers the lower part of the window; the Android app reports it as a
/// bottom safe-area inset. The key bar and the terminal stay above it.
#[test]
fn the_key_bar_stays_above_the_on_screen_keyboard() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _server, _tab) = connected_tablet(dir.path());
    let window = h.ctx.input(|i| i.viewport_rect());
    let keyboard = 300.0;
    let inset = egui::epaint::MarginF32 { bottom: keyboard, ..Default::default() };
    h.input_mut().safe_area_insets = Some(egui::SafeAreaInsets(inset));
    h.run_steps(3);
    let visible = window.bottom() - keyboard;
    let bar = h.get_by_label("PgUp").rect();
    assert!(bar.bottom() <= visible, "the key bar ends at {} above the keyboard at {visible}", bar.bottom());
    let terminal = h.get_by_label("Terminal").rect();
    assert!(terminal.bottom() <= bar.top() + 1.0, "the terminal is above the bar");
}

/// A text field on a tablet gets a Paste button while it has the keyboard, so a key or an
/// account key from a password manager can be pasted without a hardware keyboard. The field
/// keeps the keyboard through the tap.
#[test]
fn a_text_field_on_a_tablet_can_paste() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), Form::Tablet);
    h.state_mut().paste_source = Some(std::sync::Arc::new(|| Some("db-primary".to_string())));
    h.state_mut().ui.page = Page::Hosts;
    h.run_steps(3);
    assert!(h.query_by_label("Paste").is_none(), "no button without a field that has the keyboard");
    h.get_by_role(egui::accesskit::Role::TextInput).click();
    h.run_steps(3);
    h.get_by_label("Paste").click();
    for _ in 0..3 {
        h.step();
        assert!(h.output().platform_output.ime.is_some(), "the field kept the keyboard");
    }
    h.run_steps(2);
    assert_eq!(h.state().ui.hosts.search, "db-primary");
}

/// The counter-check: the desktop has no key bar.
#[test]
fn the_desktop_has_no_key_bar() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _server, _tab) = connected_tablet(dir.path());
    h.state_mut().form = Form::Desktop;
    h.run_steps(3);
    assert!(h.query_by_label("PgUp").is_none());
}

/// A terminal on a tablet with 100 lines of history, and the harness around it.
fn history_terminal() -> (Harness<'static>, Rc<RefCell<Terminal>>) {
    let term = Rc::new(RefCell::new(Terminal::new(TermSize::clamped(40, 10), &TermConfig::default())));
    for i in 0..100 {
        term.borrow_mut().feed(format!("line {i}\r\n").as_bytes());
    }
    let t = term.clone();
    let palette = TermPalette::dark();
    let mut harness = Harness::builder().with_size(vec2(400.0, 200.0)).build_ui(move |ui| {
        let mut term = t.borrow_mut();
        let _ = TerminalView { term: &mut term, palette: &palette, font: FontId::monospace(14.0), ligatures: true, highlighter: None, touch: true }
            .show(ui);
    });
    harness.run_steps(2);
    (harness, term)
}

/// A drag down the terminal, by a finger (`touch`, as egui-winit reports one: touch events and
/// the pointer it moves) or by a mouse.
fn drag_down(harness: &mut Harness<'static>, touch: bool) {
    let rect = harness.get_by_label("Terminal").rect();
    let start = rect.center() - vec2(0.0, rect.height() / 3.0);
    let finger = |phase, pos| Event::Touch { device_id: egui::TouchDeviceId(0), id: egui::TouchId(0), phase, pos, force: None };
    harness.event(Event::PointerMoved(start));
    if touch {
        harness.event(finger(egui::TouchPhase::Start, start));
    }
    harness.event(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    harness.run_steps(1);
    for step in 1..=10 {
        let at = start + vec2(0.0, step as f32 * 12.0);
        if touch {
            harness.event(finger(egui::TouchPhase::Move, at));
        }
        harness.event(Event::PointerMoved(at));
        harness.run_steps(1);
    }
    let end = start + vec2(0.0, 120.0);
    if touch {
        harness.event(finger(egui::TouchPhase::End, end));
    }
    harness.event(Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    harness.run_steps(2);
}

/// A finger dragged down a tablet's terminal goes back in the history.
#[test]
fn a_drag_scrolls_back_on_a_tablet() {
    let (mut harness, term) = history_terminal();
    assert!(!term.borrow().visible_text().contains("line 85"));
    drag_down(&mut harness, true);
    assert!(term.borrow().visible_text().contains("line 85"), "the drag went back in the history");
    assert!(term.borrow().selection_text().is_none(), "a finger's drag selects nothing");
}

/// A mouse or trackpad on a tablet selects as on the desktop.
#[test]
fn a_mouse_drag_on_a_tablet_still_selects() {
    let (mut harness, term) = history_terminal();
    drag_down(&mut harness, false);
    assert!(term.borrow().selection_text().is_some(), "the mouse selected");
    assert!(!term.borrow().visible_text().contains("line 85"), "and did not scroll");
}

/// Ctrl held for one session lets go when another session has the bar.
#[test]
fn held_keys_let_go_in_another_session() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _server, tab) = connected_tablet(dir.path());
    assert_eq!(h.state().key_bar_session, Some(tab));
    h.state_mut().key_bar.ctrl = true;
    h.state_mut().key_bar_session = None;
    h.run_steps(2);
    assert!(!h.state().key_bar.ctrl);
}

/// No agents on a tablet: the agent link stays off even with the setting and the consent on.
#[test]
fn a_tablet_never_starts_the_agent_link() {
    use sshcraft_engine::Caller;
    use sshcraft_engine::consent::Consent;
    use sshcraft_link::ConnectionFile;
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let (mut app, session, ctx) = sshcraft_ui_egui::testing::app_on(dir.path());
    app.form = Form::Tablet;
    let mut settings = session.runtime().block_on(session.execute(Caller::User, "settings.get", serde_json::json!({}))).unwrap();
    settings["integrations"]["allow_agent_control"] = serde_json::json!(true);
    session.runtime().block_on(session.execute(Caller::User, "settings.save", serde_json::json!({ "settings": settings }))).unwrap();
    app.link.source = Arc::new(|_| Consent { allow_setting: true, mcp_flag: false, launcher_allows: false });
    let link_dir = app.link.dir.clone().unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        app.logic(&ctx);
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(ConnectionFile::read(&link_dir).is_none(), "the agent link started on a tablet");
}

/// A field low on a long page scrolls into the part the on-screen keyboard leaves free.
#[test]
fn a_field_under_the_keyboard_scrolls_into_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), Form::Tablet);
    settings(&mut h, Section::Appearance);
    let lowest = h.get_all_by_role(egui::accesskit::Role::TextInput).map(|n| n.rect()).max_by(|a, b| a.bottom().total_cmp(&b.bottom())).unwrap();
    let window = h.ctx.input(|i| i.viewport_rect());
    // A keyboard that covers the lower part of the field.
    let keyboard = window.bottom() - lowest.bottom() + 8.0;
    assert!(keyboard < window.height() - 100.0, "the field is low enough for the test: {lowest:?}");
    let at = lowest.center();
    h.event(egui::Event::PointerMoved(at));
    h.event(egui::Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(egui::Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    h.input_mut().safe_area_insets = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 { bottom: keyboard, ..Default::default() }));
    h.run_steps(10);
    let field = h.get_all_by_role(egui::accesskit::Role::TextInput).find(|n| n.is_focused()).map(|n| n.rect()).expect("the field keeps the focus");
    assert!(
        field.bottom() <= window.bottom() - keyboard,
        "the field ends at {} under the keyboard at {}",
        field.bottom(),
        window.bottom() - keyboard
    );
}

/// The app lock follows what the app found on the device: offered and named after the
/// fingerprint on a tablet with a screen lock, never locking where nothing can unlock it.
#[test]
fn the_app_lock_follows_the_devices_screen_lock() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), Form::Tablet);
    h.state_mut().lock_available = true;
    settings(&mut h, Section::General);
    assert!(h.query_by_label_contains("fingerprint").is_some(), "the settings name the fingerprint");
    h.state_mut().lock.locked = true;
    h.run_steps(3);
    assert!(h.query_by_label_contains("fingerprint, face or screen lock").is_some(), "the lock screen says how to unlock");
}

#[test]
fn nothing_locks_where_nothing_can_unlock() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _session, ctx) = sshcraft_ui_egui::testing::app_on(dir.path());
    app.form = Form::Tablet;
    app.lock_available = false;
    app.data.settings.general.lock_after_minutes = 1;
    app.lock.last_input = Instant::now() - Duration::from_secs(120);
    app.logic(&ctx);
    assert!(!app.lock.locked, "locked without a way to unlock");
    app.lock_available = true;
    app.logic(&ctx);
    assert!(app.lock.locked, "the counter-check: with a screen lock it locks");
}

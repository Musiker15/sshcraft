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
    assert!(h.query_by_label("Import").is_some());
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

/// The counter-check: the desktop has no key bar.
#[test]
fn the_desktop_has_no_key_bar() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _server, _tab) = connected_tablet(dir.path());
    h.state_mut().form = Form::Desktop;
    h.run_steps(3);
    assert!(h.query_by_label("PgUp").is_none());
}

/// A finger dragged down a tablet's terminal goes back in the history.
#[test]
fn a_drag_scrolls_back_on_a_tablet() {
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
    assert!(!term.borrow().visible_text().contains("line 85"));
    let rect = harness.get_by_label("Terminal").rect();
    let start = rect.center() - vec2(0.0, rect.height() / 3.0);
    harness.event(Event::PointerMoved(start));
    harness.event(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    harness.run_steps(1);
    for step in 1..=10 {
        harness.event(Event::PointerMoved(start + vec2(0.0, step as f32 * 12.0)));
        harness.run_steps(1);
    }
    let end = start + vec2(0.0, 120.0);
    harness.event(Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    harness.run_steps(2);
    assert!(term.borrow().visible_text().contains("line 85"), "the drag went back in the history");
    assert!(term.borrow().selection_text().is_none(), "a drag on a tablet selects nothing");
}

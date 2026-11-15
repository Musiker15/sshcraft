//! The terminal widget in egui_kittest: what a person types, pastes, clicks and drags, and what a
//! screen reader reads. No pictures: every check is on bytes, state or the AccessKit tree.

use std::cell::RefCell;
use std::rc::Rc;

use egui::{Event, FontId, ImeEvent, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use sshcraft_term::{TermConfig, TermSize, Terminal};
use sshcraft_ui_egui::terminal::{TermPalette, TerminalResponse, TerminalView};

#[derive(Default)]
struct Seen {
    input: Vec<u8>,
    resize: Option<TermSize>,
    open_link: Option<String>,
    copy: Option<String>,
}

struct Fixture {
    harness: Harness<'static>,
    term: Rc<RefCell<Terminal>>,
    seen: Rc<RefCell<Seen>>,
}

fn fixture(output: &[u8]) -> Fixture {
    let term = Rc::new(RefCell::new(Terminal::new(TermSize::clamped(40, 10), &TermConfig::default())));
    term.borrow_mut().feed(output);
    let seen = Rc::new(RefCell::new(Seen::default()));
    let (t, s) = (term.clone(), seen.clone());
    let palette = TermPalette::dark();
    let harness = Harness::builder().with_size(vec2(400.0, 200.0)).build_ui(move |ui| {
        let mut term = t.borrow_mut();
        let TerminalResponse { input, resize, open_link, copy, .. } =
            TerminalView { term: &mut term, palette: &palette, font: FontId::monospace(14.0), ligatures: true }.show(ui);
        let mut seen = s.borrow_mut();
        seen.input.extend(input);
        seen.resize = resize.or(seen.resize);
        seen.open_link = open_link.or(seen.open_link.take());
        seen.copy = copy.or(seen.copy.take());
    });
    Fixture { harness, term, seen }
}

/// Clicks into the terminal so it has the keyboard focus.
fn focus(f: &mut Fixture) {
    f.harness.get_by_label("Terminal").click();
    f.harness.run_steps(2);
    f.seen.borrow_mut().input.clear();
}

/// The screen position of cell `col`, `row`, from the widget's top left corner.
fn cell(f: &Fixture, col: u16, row: u16) -> Pos2 {
    let rect = f.harness.get_by_label("Terminal").rect();
    let size = f.term.borrow().size();
    let w = rect.width() / f32::from(size.cols);
    let h = rect.height() / f32::from(size.rows);
    Pos2::new(rect.min.x + (f32::from(col) + 0.5) * w, rect.min.y + (f32::from(row) + 0.5) * h)
}

#[test]
fn typing_sends_bytes() {
    let mut f = fixture(b"");
    focus(&mut f);
    f.harness.event(Event::Text("ls".into()));
    f.harness.key_press(Key::Enter);
    f.harness.run_steps(2);
    assert_eq!(f.seen.borrow().input, b"ls\r");
}

#[test]
fn arrows_and_tab_stay_in_the_terminal() {
    let mut f = fixture(b"");
    focus(&mut f);
    f.harness.key_press(Key::Tab);
    f.harness.key_press(Key::ArrowUp);
    f.harness.key_press_modifiers(Modifiers::CTRL, Key::D);
    f.harness.run_steps(2);
    assert_eq!(f.seen.borrow().input, b"\t\x1b[A\x04");
}

#[test]
fn ctrl_c_with_a_selection_copies_without_one_interrupts() {
    let mut f = fixture(b"hello");
    focus(&mut f);
    f.harness.event(Event::Copy);
    f.harness.run_steps(2);
    assert_eq!(f.seen.borrow().input, [0x03]);
    assert_eq!(f.seen.borrow().copy, None);
    f.term.borrow_mut().select_start(sshcraft_term::GridPoint { line: 0, col: 0 }, sshcraft_term::SelectionKind::Word);
    f.seen.borrow_mut().input.clear();
    f.harness.event(Event::Copy);
    f.harness.run_steps(2);
    assert!(f.seen.borrow().input.is_empty());
    assert_eq!(f.seen.borrow().copy.as_deref(), Some("hello"));
}

#[test]
fn paste_goes_through_the_bracket() {
    let mut f = fixture(b"\x1b[?2004h");
    focus(&mut f);
    f.harness.event(Event::Paste("ls\x1b[201~rm".into()));
    f.harness.run_steps(2);
    // Every ESC of pasted text is dropped: what is left cannot end the bracket.
    assert_eq!(f.seen.borrow().input, b"\x1b[200~ls[201~rm\x1b[201~");
}

#[test]
fn ime_commit_sends_the_text_and_preedit_sends_nothing() {
    let mut f = fixture(b"");
    focus(&mut f);
    f.harness.event(Event::Ime(ImeEvent::Preedit { text: "にほ".into(), active_range_chars: None }));
    f.harness.run_steps(2);
    assert!(f.seen.borrow().input.is_empty());
    f.harness.event(Event::Ime(ImeEvent::Commit("日本".into())));
    f.harness.run_steps(2);
    assert_eq!(f.seen.borrow().input, "日本".as_bytes());
}

#[test]
fn ime_candidates_sit_at_the_cursor() {
    let mut f = fixture(b"abc");
    focus(&mut f);
    f.harness.step();
    let ime = f.harness.output().platform_output.ime.clone().expect("the terminal asks for the IME while focused");
    let cursor = cell(&f, 3, 0);
    assert!(ime.cursor_rect.expand(1.0).contains(Pos2::new(ime.cursor_rect.min.x, cursor.y)), "{:?} vs {cursor:?}", ime.cursor_rect);
    assert!((ime.cursor_rect.min.x - (cursor.x - ime.cursor_rect.width().max(1.0) / 2.0)).abs() < 12.0, "{:?} vs {cursor:?}", ime.cursor_rect);
    assert_eq!(ime.purpose, egui::IMEPurpose::Terminal);
}

#[test]
fn dragging_selects_and_the_wheel_scrolls() {
    let mut f = fixture(b"hello world");
    let (from, to) = (cell(&f, 0, 0), cell(&f, 4, 0));
    f.harness.drag_at(from);
    f.harness.run_steps(1);
    f.harness.hover_at(to);
    f.harness.run_steps(1);
    f.harness.drop_at(to);
    f.harness.run_steps(2);
    assert_eq!(f.term.borrow().selection_text().as_deref(), Some("hello"));

    let mut f = fixture(&b"line\r\n".repeat(60));
    f.harness.hover_at(cell(&f, 1, 1));
    f.harness.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, 3.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
        source: egui::MouseWheelSource::Wheel,
    });
    f.harness.run_steps(2);
    assert!(f.term.borrow().screen().display_offset > 0);

    let mut f = fixture(b"\x1b[?1049h\x1b[?1007h");
    focus(&mut f);
    f.harness.hover_at(cell(&f, 1, 1));
    f.harness.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
        source: egui::MouseWheelSource::Wheel,
    });
    f.harness.run_steps(2);
    assert!(f.seen.borrow().input.windows(3).any(|w| w == b"\x1b[A" || w == b"\x1bOA"), "{:?}", f.seen.borrow().input);
}

#[test]
fn ctrl_click_on_a_link_asks_to_open_it() {
    let mut f = fixture(b"\x1b]8;;https://example.com/\x1b\\link\x1b]8;;\x1b\\");
    let at = cell(&f, 1, 0);
    f.harness.hover_at(at);
    f.harness.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    f.harness.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    f.harness.run_steps(2);
    assert_eq!(f.seen.borrow().open_link, None);
    f.harness.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::CTRL });
    f.harness.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::CTRL });
    f.harness.run_steps(2);
    assert_eq!(f.seen.borrow().open_link.as_deref(), Some("https://example.com/"));
}

#[test]
fn a_new_size_is_reported() {
    let mut f = fixture(b"");
    f.harness.run_steps(2);
    let size = f.seen.borrow().resize.expect("the widget reports the size it has room for");
    assert_eq!(f.term.borrow().size(), size);
    assert!(size.cols > 10 && size.rows > 3, "{size:?}");
    f.harness.set_size(vec2(200.0, 100.0));
    f.seen.borrow_mut().resize = None;
    f.harness.run_steps(2);
    let smaller = f.seen.borrow().resize.expect("a smaller area is reported");
    assert!(smaller.cols < size.cols && smaller.rows < size.rows);
}

#[test]
fn screen_readers_read_the_screen_as_text() {
    let f = fixture(b"hello\r\nworld");
    let node = f.harness.get_by_label("Terminal");
    let value = node.value().unwrap_or_default();
    assert!(value.contains("hello\nworld"), "{value:?}");
}

/// egui-winit sends a Key and a Text event for Alt+letter; the terminal sends `ESC b` once.
#[test]
fn alt_letters_are_sent_once() {
    let mut f = fixture(b"");
    focus(&mut f);
    f.harness.event(Event::Key { key: Key::B, physical_key: Some(Key::B), pressed: true, repeat: false, modifiers: Modifiers::ALT });
    f.harness.event(Event::Text("b".into()));
    f.harness.run_steps(2);
    assert_eq!(f.seen.borrow().input, b"\x1bb");
}

/// A collapsed split or tab must not shrink the terminal to 2x1 and reflow its history away.
#[test]
fn a_collapsed_area_keeps_the_size() {
    let mut f = fixture(b"");
    f.harness.run_steps(2);
    let size = f.term.borrow().size();
    f.seen.borrow_mut().resize = None;
    f.harness.set_size(vec2(4.0, 4.0));
    f.harness.run_steps(2);
    assert_eq!(f.seen.borrow().resize, None);
    assert_eq!(f.term.borrow().size(), size);
}

/// Clicking into a terminal scrolled back sends a focus report but keeps its place.
#[test]
fn focusing_a_scrolled_terminal_keeps_its_place() {
    let mut f = fixture(&[b"\x1b[?1004h".as_slice(), &b"line\r\n".repeat(60)].concat());
    f.harness.run_steps(2);
    f.term.borrow_mut().scroll(10);
    f.harness.get_by_label("Terminal").click();
    f.harness.run_steps(2);
    assert!(f.seen.borrow().input.windows(3).any(|w| w == b"\x1b[I"), "{:?}", f.seen.borrow().input);
    assert_eq!(f.term.borrow().screen().display_offset, 10);
}

/// A drag with the right button is reported as one, not as a left drag.
#[test]
fn a_right_drag_is_reported_as_right() {
    let mut f = fixture(b"\x1b[?1002h\x1b[?1006h");
    f.harness.run_steps(2);
    let (from, to) = (cell(&f, 1, 1), cell(&f, 6, 1));
    f.harness.event(Event::PointerMoved(from));
    f.harness.event(Event::PointerButton { pos: from, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE });
    f.harness.run_steps(1);
    f.harness.event(Event::PointerMoved(to));
    f.harness.run_steps(1);
    f.harness.event(Event::PointerButton { pos: to, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE });
    f.harness.run_steps(2);
    let input = String::from_utf8_lossy(&f.seen.borrow().input).to_string();
    assert!(input.contains("\x1b[<34;"), "{input:?}");
    assert!(!input.contains("\x1b[<32;"), "{input:?}");
}

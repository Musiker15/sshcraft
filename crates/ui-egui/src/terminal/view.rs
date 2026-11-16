//! The terminal widget: draws a [`Terminal`] in the space it gets and turns keyboard, IME,
//! mouse and clipboard events into the bytes the session receives. It sends nothing itself: the
//! caller gets the bytes in [`TerminalResponse::input`] and passes them to the session.

use egui::{Event, FontId, ImeEvent, Pos2, Rect, Response, Sense, Ui, pos2, vec2};
use sshcraft_term::input::{Key as TermKey, Mods, MouseAction, MouseButton, encode_focus, encode_key, encode_mouse, encode_paste};
use sshcraft_term::limits::{MIN_COLS, MIN_ROWS};
use sshcraft_term::{Screen, SelectionKind, TermModes, TermSize, Terminal};

use super::keys::map_key;
use super::{CellMetrics, TermPalette, layout, paint};

/// The terminal as a widget.
pub struct TerminalView<'a> {
    pub term: &'a mut Terminal,
    pub palette: &'a TermPalette,
    pub font: FontId,
    /// Neighbouring characters may join where the font has ligatures (setting `appearance.ligatures`).
    pub ligatures: bool,
    /// Patterns coloured wherever they appear (setting `terminal.highlights`).
    pub highlighter: Option<&'a super::highlight::Highlighter>,
}

/// What happened in one frame.
pub struct TerminalResponse {
    pub response: Response,
    /// Bytes for the session that the user typed, pasted or composed.
    pub input: Vec<u8>,
    /// Bytes the terminal sends on its own (mouse, wheel and focus reports): only ever to its
    /// own session, never to the other panes of a broadcast.
    pub reports: Vec<u8>,
    /// The terminal was resized to fit; the session should be told.
    pub resize: Option<TermSize>,
    /// The user Ctrl+clicked a link; opening it goes through the engine's checks.
    pub open_link: Option<String>,
    /// Text for the clipboard (the selection, on copy).
    pub copy: Option<String>,
}

/// State kept between frames, per widget.
#[derive(Clone, Default)]
struct State {
    focused: bool,
    preedit: String,
    ime_generation: u64,
    /// Wheel movement not yet a whole line.
    wheel: f32,
    /// An Alt+key was sent as `ESC key`; the text event egui-winit sends for it is skipped.
    skip_alt_text: bool,
}

const MOUSE_ANY: TermModes = TermModes::MOUSE_REPORT.union(TermModes::MOUSE_DRAG).union(TermModes::MOUSE_MOTION);

/// Most lines one frame's wheel movement turns into.
const MAX_WHEEL_STEPS: u32 = 100;

fn term_mods(m: egui::Modifiers) -> Mods {
    let mut out = Mods::empty();
    out.set(Mods::SHIFT, m.shift);
    out.set(Mods::ALT, m.alt);
    out.set(Mods::CTRL, m.ctrl);
    out
}

/// What every step of a frame needs to know.
struct Frame {
    rect: Rect,
    metrics: CellMetrics,
    size: TermSize,
    modes: TermModes,
    modifiers: egui::Modifiers,
    /// The program asked for the mouse and Shift is not held to select instead.
    report_mouse: bool,
    focused: bool,
    ligatures: bool,
}

impl Frame {
    /// The screen cell under `pos`, clamped to the terminal.
    fn cell_at(&self, pos: Pos2) -> (u16, u16) {
        let col = ((pos.x - self.rect.min.x) / self.metrics.width).max(0.0) as u16;
        let row = ((pos.y - self.rect.min.y) / self.metrics.height).max(0.0) as u16;
        (col.min(self.size.cols.saturating_sub(1)), row.min(self.size.rows.saturating_sub(1)))
    }
}

/// Mouse buttons as reports for a program that asked for the mouse.
fn report_buttons(ui: &Ui, frame: &Frame, col: u16, row: u16, out: &mut Vec<u8>) {
    ui.input(|i| {
        for event in &i.events {
            let Event::PointerButton { button, pressed, modifiers, .. } = event else { continue };
            let button = match button {
                egui::PointerButton::Primary => MouseButton::Left,
                egui::PointerButton::Middle => MouseButton::Middle,
                egui::PointerButton::Secondary => MouseButton::Right,
                _ => continue,
            };
            let action = if *pressed { MouseAction::Press } else { MouseAction::Release };
            if let Some(bytes) = encode_mouse(button, action, col, row, term_mods(*modifiers), frame.modes) {
                out.extend(bytes);
            }
        }
    });
}

/// A click: Ctrl opens the link under it, two select a word, three a line, one clears.
fn click(ui: &Ui, response: &Response, frame: &Frame, term: &mut Terminal, (col, row): (u16, u16), out: &mut TerminalResponse) {
    // The modifiers of the click itself, not of the frame.
    let click_mods = ui.input(|i| {
        i.events.iter().rev().find_map(|e| match e {
            Event::PointerButton { pressed: false, modifiers, .. } => Some(*modifiers),
            _ => None,
        })
    });
    let point = term.point_at(col, row);
    if click_mods.unwrap_or(frame.modifiers).ctrl {
        let screen = term.screen();
        let link = screen.cells.get(usize::from(row) * usize::from(frame.size.cols) + usize::from(col)).and_then(|c| c.link);
        out.open_link = link.and_then(|i| screen.links.get(i as usize).cloned());
    } else if response.triple_clicked() {
        term.select_start(point, SelectionKind::Line);
    } else if response.double_clicked() {
        term.select_start(point, SelectionKind::Word);
    } else {
        term.clear_selection();
    }
}

/// Selection, links and reports from the mouse.
fn mouse(ui: &Ui, response: &Response, frame: &Frame, term: &mut Terminal, out: &mut TerminalResponse) {
    let Some(pos) = response.interact_pointer_pos() else { return };
    let (col, row) = frame.cell_at(pos);
    if frame.report_mouse {
        report_buttons(ui, frame, col, row, &mut out.reports);
        let (moved, held) = ui.input(|i| {
            let held = if i.pointer.button_down(egui::PointerButton::Secondary) {
                MouseButton::Right
            } else if i.pointer.button_down(egui::PointerButton::Middle) {
                MouseButton::Middle
            } else {
                MouseButton::Left
            };
            (i.pointer.delta() != egui::Vec2::ZERO, held)
        });
        if response.dragged()
            && moved
            && let Some(bytes) = encode_mouse(held, MouseAction::Drag, col, row, term_mods(frame.modifiers), frame.modes)
        {
            out.reports.extend(bytes);
        }
    } else if response.drag_started() || response.dragged() || response.drag_stopped() {
        if response.drag_started() {
            let (c, r) = frame.cell_at(ui.input(|i| i.pointer.press_origin()).unwrap_or(pos));
            term.select_start(term.point_at(c, r), SelectionKind::Simple);
        }
        term.select_update(term.point_at(col, row));
    } else if response.clicked() {
        click(ui, response, frame, term, (col, row), out);
    }
}

/// The wheel: the history, arrow keys in a full-screen program that asked for them, or reports.
fn wheel(ui: &Ui, frame: &Frame, state: &mut State, term: &mut Terminal, out: &mut Vec<u8>) {
    let lines: f32 = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                Event::MouseWheel { unit, delta, .. } => Some(match unit {
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Point => delta.y / frame.metrics.height,
                    egui::MouseWheelUnit::Page => delta.y * f32::from(frame.size.rows),
                }),
                _ => None,
            })
            .sum()
    });
    state.wheel += lines;
    let whole = state.wheel.trunc();
    state.wheel -= whole;
    let steps = whole as i32;
    if steps == 0 {
        return;
    }
    let count = steps.unsigned_abs().min(MAX_WHEEL_STEPS);
    let (col, row) = ui.input(|i| i.pointer.hover_pos()).map_or((0, 0), |p| frame.cell_at(p));
    if frame.report_mouse {
        let button = if steps > 0 { MouseButton::WheelUp } else { MouseButton::WheelDown };
        for _ in 0..count {
            out.extend(encode_mouse(button, MouseAction::Press, col, row, Mods::empty(), frame.modes).unwrap_or_default());
        }
    } else if frame.modes.contains(TermModes::ALT_SCREEN | TermModes::ALTERNATE_SCROLL) {
        let key = if steps > 0 { TermKey::Up } else { TermKey::Down };
        for _ in 0..count {
            out.extend(encode_key(key, Mods::empty(), frame.modes).unwrap_or_default());
        }
    } else {
        term.scroll(steps);
    }
}

/// Keys, clipboard and IME while the terminal has the focus.
fn keyboard(ui: &Ui, frame: &Frame, state: &mut State, term: &mut Terminal, out: &mut TerminalResponse) {
    let events = ui.input(|i| i.events.clone());
    let typed_from = out.input.len();
    // egui-winit sends Alt+letter as a Key and a Text event: the Key already became `ESC letter`.
    for event in events {
        match event {
            Event::Text(_) if state.skip_alt_text => state.skip_alt_text = false,
            Event::Text(text) => out.input.extend(text.into_bytes()),
            Event::Key { key, pressed: true, modifiers, .. } => {
                // A key that is not Alt+letter: its own text event is typed as usual.
                state.skip_alt_text = false;
                if let Some((k, m)) = map_key(key, modifiers)
                    && let Some(bytes) = encode_key(k, m, frame.modes)
                {
                    state.skip_alt_text = matches!(k, TermKey::Char(_)) && modifiers.alt && !modifiers.ctrl;
                    out.input.extend(bytes);
                }
            }
            Event::Copy => match term.selection_text() {
                Some(text) => out.copy = Some(text),
                None => out.input.push(0x03),
            },
            Event::Cut => out.input.push(0x18),
            Event::Paste(text) => out.input.extend(encode_paste(&text, frame.modes)),
            Event::Ime(ImeEvent::Preedit { text, .. }) => state.preedit = text,
            Event::Ime(ImeEvent::Commit(text)) => {
                state.preedit.clear();
                out.input.extend(text.into_bytes());
            }
            _ => {}
        }
    }
    // Typing goes back to the newest output; focus and mouse reports do not.
    if out.input.len() > typed_from {
        term.scroll_to_bottom();
    }
}

/// Draws the screen, the IME's text being composed, and tells the IME where the cursor is.
/// Returns the screen it drew, for the screen reader's text.
#[allow(clippy::too_many_arguments)] // The parts of one frame, all needed to paint it.
fn draw(
    ui: &Ui,
    response: &Response,
    frame: &Frame,
    state: &State,
    term: &Terminal,
    palette: &TermPalette,
    font: &FontId,
    highlighter: Option<&super::highlight::Highlighter>,
) -> Screen {
    let (rect, metrics, focused) = (frame.rect, frame.metrics, frame.focused);
    let mut screen = term.screen();
    if let Some(highlighter) = highlighter {
        highlighter.apply(&mut screen);
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.bg);
    paint(&painter, &layout(&screen, &metrics, palette, font, rect.min, focused, frame.ligatures));
    super::blocks::paint_gutter(&painter, response, rect, &metrics, &screen.commands);
    let cursor = Rect::from_min_size(
        pos2(rect.min.x + f32::from(screen.cursor.col) * metrics.width, rect.min.y + f32::from(screen.cursor.row) * metrics.height),
        vec2(2.0, metrics.height),
    );
    if !focused {
        return screen;
    }
    if !state.preedit.is_empty() {
        let galley = painter.layout_no_wrap(state.preedit.clone(), font.clone(), palette.fg);
        let area = Rect::from_min_size(cursor.min, vec2(galley.size().x, metrics.height));
        painter.rect_filled(area, 0.0, palette.bg);
        painter.galley(area.min, galley, palette.fg);
        painter.line_segment([area.left_bottom(), area.right_bottom()], egui::Stroke::new(1.0, palette.fg));
    }
    let ime = egui::output::IMEOutput::new(response.id, state.ime_generation, egui::IMEPurpose::Terminal, rect, cursor);
    ui.output_mut(|o| o.ime = Some(ime));
    screen
}

impl TerminalView<'_> {
    pub fn show(self, ui: &mut Ui) -> TerminalResponse {
        let Self { term, palette, font, ligatures, highlighter } = self;
        // A server may print any character: one no font has is a box, never a panic.
        ui.ctx().set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
        let metrics = CellMetrics::measure(ui.ctx(), &font);
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let id = response.id;
        let mut state: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        let mut out =
            TerminalResponse { response: response.clone(), input: Vec::new(), reports: Vec::new(), resize: None, open_link: None, copy: None };

        // Fit the terminal to the space. A collapsed area (a hidden split, a tab being laid out)
        // keeps the size: shrinking to 2x1 would reflow the history away and tell the server.
        let fits = rect.width() >= f32::from(MIN_COLS) * metrics.width && rect.height() >= f32::from(MIN_ROWS) * metrics.height;
        let size = if fits { TermSize::clamped((rect.width() / metrics.width) as u16, (rect.height() / metrics.height) as u16) } else { term.size() };
        if size != term.size() {
            term.resize(size);
            out.resize = Some(size);
        }
        let modes = term.modes();
        let modifiers = ui.input(|i| i.modifiers);

        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let focused = ui.memory(|m| m.has_focus(id));
        let frame =
            Frame { rect, metrics, size, modes, modifiers, report_mouse: modes.intersects(MOUSE_ANY) && !modifiers.shift, focused, ligatures };
        if focused {
            let filter = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true };
            ui.memory_mut(|m| m.set_focus_lock_filter(id, filter));
        }
        if focused != state.focused {
            out.reports.extend(encode_focus(focused, modes).unwrap_or_default());
            if focused {
                state.ime_generation = state.ime_generation.wrapping_add(1);
            } else {
                state.preedit.clear();
            }
            state.focused = focused;
        }

        mouse(ui, &response, &frame, term, &mut out);
        if response.hovered() {
            wheel(ui, &frame, &mut state, term, &mut out.reports);
        }
        if focused {
            keyboard(ui, &frame, &mut state, term, &mut out);
        }
        let screen = draw(ui, &response, &frame, &state, term, palette, &font, highlighter);

        // Screen readers read the screen as text.
        let text = screen.text();
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::Role::Terminal, true, "Terminal");
            info.current_text_value = Some(text.clone());
            info
        });
        ui.data_mut(|d| d.insert_temp::<State>(id, state));
        out
    }
}

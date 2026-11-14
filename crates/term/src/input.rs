//! What the terminal sends for keys, the mouse, focus and pasted text, after xterm's control
//! sequences (the public "ctlseqs" document). Pure functions over the terminal's modes.

use bitflags::bitflags;

use crate::TermModes;

/// A key, independent of any toolkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A character as typed, already with Shift applied.
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1 to F12.
    F(u8),
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Mods: u8 {
        const SHIFT = 1;
        const ALT = 1 << 1;
        const CTRL = 1 << 2;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    /// Moving with the button held.
    Drag,
}

const ESC: u8 = 0x1b;
const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";

/// The xterm modifier parameter: 1 plus Shift 1, Alt 2, Ctrl 4.
fn modifier(mods: Mods) -> u8 {
    1 + mods.bits()
}

/// The control character Ctrl gives with `c`, if any.
fn control(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 1),
        'A'..='Z' => Some(c as u8 - b'A' + 1),
        '@' | ' ' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

/// `CSI <code> ~`, with the modifier as second parameter.
fn tilde(code: u8, mods: Mods) -> Vec<u8> {
    if mods.is_empty() { format!("\x1b[{code}~").into_bytes() } else { format!("\x1b[{code};{}~", modifier(mods)).into_bytes() }
}

/// A cursor-type key ending in `last`: `SS3` in application mode, `CSI` otherwise,
/// `CSI 1;<mods>` with modifiers.
fn cursor(last: char, mods: Mods, app: bool) -> Vec<u8> {
    if !mods.is_empty() {
        format!("\x1b[1;{}{last}", modifier(mods)).into_bytes()
    } else if app {
        format!("\x1bO{last}").into_bytes()
    } else {
        format!("\x1b[{last}").into_bytes()
    }
}

fn with_alt(mods: Mods, bytes: Vec<u8>) -> Vec<u8> {
    if mods.contains(Mods::ALT) {
        let mut out = vec![ESC];
        out.extend(bytes);
        out
    } else {
        bytes
    }
}

/// The bytes for `key`, or `None` for a key the terminal does not send.
pub fn encode_key(key: Key, mods: Mods, modes: TermModes) -> Option<Vec<u8>> {
    let app = modes.contains(TermModes::APP_CURSOR);
    let bytes = match key {
        Key::Char(c) => {
            let plain = if mods.contains(Mods::CTRL) {
                control(c).map_or_else(|| c.to_string().into_bytes(), |b| vec![b])
            } else {
                c.to_string().into_bytes()
            };
            with_alt(mods, plain)
        }
        Key::Enter => with_alt(mods, vec![b'\r']),
        Key::Tab if mods.contains(Mods::SHIFT) => b"\x1b[Z".to_vec(),
        Key::Tab => with_alt(mods, vec![b'\t']),
        Key::Backspace if mods.contains(Mods::CTRL) => with_alt(mods, vec![0x08]),
        Key::Backspace => with_alt(mods, vec![0x7f]),
        Key::Escape => with_alt(mods, vec![ESC]),
        Key::Up => cursor('A', mods, app),
        Key::Down => cursor('B', mods, app),
        Key::Right => cursor('C', mods, app),
        Key::Left => cursor('D', mods, app),
        Key::Home => cursor('H', mods, app),
        Key::End => cursor('F', mods, app),
        Key::Insert => tilde(2, mods),
        Key::Delete => tilde(3, mods),
        Key::PageUp => tilde(5, mods),
        Key::PageDown => tilde(6, mods),
        Key::F(n @ 1..=4) => {
            let last = char::from(b'P' + (n - 1));
            if mods.is_empty() { format!("\x1bO{last}").into_bytes() } else { format!("\x1b[1;{}{last}", modifier(mods)).into_bytes() }
        }
        Key::F(n @ 5..=12) => {
            const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            tilde(*CODES.get(usize::from(n - 5))?, mods)
        }
        Key::F(_) => return None,
    };
    Some(bytes)
}

/// Pasted text as it is sent: line breaks become `CR`, and in bracketed paste mode the text is
/// wrapped in the brackets. The bracket sequences are removed from the text in every mode, again
/// and again until none is left, so pasted text can never end the bracket and run what follows.
pub fn encode_paste(text: &str, modes: TermModes) -> Vec<u8> {
    let mut clean = text.replace("\r\n", "\r").replace('\n', "\r");
    while clean.contains(PASTE_START) || clean.contains(PASTE_END) {
        clean = clean.replace(PASTE_START, "").replace(PASTE_END, "");
    }
    if modes.contains(TermModes::BRACKETED_PASTE) { format!("{PASTE_START}{clean}{PASTE_END}").into_bytes() } else { clean.into_bytes() }
}

/// The report for a mouse event at screen cell `col`, `row` (from 0), or `None` when the program
/// did not ask for it or the old encoding cannot express the position.
pub fn encode_mouse(button: MouseButton, action: MouseAction, col: u16, row: u16, mods: Mods, modes: TermModes) -> Option<Vec<u8>> {
    let any = TermModes::MOUSE_REPORT | TermModes::MOUSE_DRAG | TermModes::MOUSE_MOTION;
    if !modes.intersects(any) {
        return None;
    }
    let wheel = matches!(button, MouseButton::WheelUp | MouseButton::WheelDown);
    if action == MouseAction::Drag && !modes.intersects(TermModes::MOUSE_DRAG | TermModes::MOUSE_MOTION) {
        return None;
    }
    if wheel && action == MouseAction::Release {
        return None;
    }
    let mut code: u16 = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
    };
    if action == MouseAction::Drag {
        code += 32;
    }
    code += u16::from(mods.contains(Mods::SHIFT)) * 4 + u16::from(mods.contains(Mods::ALT)) * 8 + u16::from(mods.contains(Mods::CTRL)) * 16;
    let (x, y) = (u32::from(col) + 1, u32::from(row) + 1);
    if modes.contains(TermModes::SGR_MOUSE) {
        let end = if action == MouseAction::Release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes());
    }
    // The old encoding has no release per button and one byte per coordinate.
    if action == MouseAction::Release {
        code = (code & !3) | 3;
    }
    let byte = |v: u32| u8::try_from(v + 32).ok();
    Some(vec![ESC, b'[', b'M', byte(u32::from(code))?, byte(x)?, byte(y)?])
}

/// The report for gaining or losing focus, when the program asked for it.
pub fn encode_focus(focused: bool, modes: TermModes) -> Option<Vec<u8>> {
    modes.contains(TermModes::FOCUS_REPORT).then(|| if focused { b"\x1b[I".to_vec() } else { b"\x1b[O".to_vec() })
}

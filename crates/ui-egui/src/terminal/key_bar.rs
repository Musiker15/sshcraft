//! The bar of special keys under the terminal on a tablet: the keys an on-screen keyboard lacks
//! (Esc, Tab, arrows, Home, End, Page Up and Down, a few symbols) and Ctrl and Alt as keys that
//! stay down for the next key or character, typed on the bar or on the keyboard.

use egui::Event;
use sshcraft_term::TermModes;
use sshcraft_term::input::{Key as TermKey, Mods, encode_key, encode_paste};

/// The height of the bar, in points.
pub const KEY_BAR_HEIGHT: f32 = 34.0;

/// Ctrl and Alt as tapped on the bar: each applies to the next key or character, then lets go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyBar {
    pub ctrl: bool,
    pub alt: bool,
}

/// A button of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKey {
    Esc,
    Tab,
    Ctrl,
    Alt,
    Left,
    Up,
    Down,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Char(char),
    Paste,
}

/// The buttons in the order they are shown, with their labels.
pub const KEYS: [(BarKey, &str); 18] = [
    (BarKey::Esc, "Esc"),
    (BarKey::Tab, "Tab"),
    (BarKey::Ctrl, "Ctrl"),
    (BarKey::Alt, "Alt"),
    (BarKey::Left, "←"),
    (BarKey::Up, "↑"),
    (BarKey::Down, "↓"),
    (BarKey::Right, "→"),
    (BarKey::Home, "Home"),
    (BarKey::End, "End"),
    (BarKey::PageUp, "PgUp"),
    (BarKey::PageDown, "PgDn"),
    (BarKey::Char('|'), "|"),
    (BarKey::Char('~'), "~"),
    (BarKey::Char('/'), "/"),
    (BarKey::Char('-'), "-"),
    (BarKey::Char('_'), "_"),
    (BarKey::Paste, "Paste"),
];

/// `c` with Ctrl: the control character for letters and `@ [ \ ] ^ _ ?` and space, else `c`.
fn with_ctrl(c: char) -> char {
    match c {
        'a'..='z' | 'A'..='Z' => char::from((c.to_ascii_lowercase() as u8) & 0x1f),
        '@' | ' ' => '\0',
        '[' => '\u{1b}',
        '\\' => '\u{1c}',
        ']' => '\u{1d}',
        '^' => '\u{1e}',
        '_' => '\u{1f}',
        '?' => '\u{7f}',
        other => other,
    }
}

/// Keys that type a character: their own text event follows them.
fn types_text(key: egui::Key) -> bool {
    let name = key.name();
    name.chars().count() == 1 || matches!(key, egui::Key::Space)
}

impl KeyBar {
    fn active(self) -> bool {
        self.ctrl || self.alt
    }

    fn release(&mut self) {
        *self = Self::default();
    }

    /// The text `s` typed with the held keys: Ctrl turns its first character into a control
    /// character, Alt puts ESC before it.
    fn text(self, s: &str) -> String {
        let mut chars = s.chars();
        let Some(first) = chars.next() else { return String::new() };
        let first = if self.ctrl { with_ctrl(first) } else { first };
        let mut out = String::with_capacity(s.len() + 1);
        if self.alt {
            out.push('\u{1b}');
        }
        out.push(first);
        out.extend(chars);
        out
    }

    /// Applies held Ctrl and Alt to the next key or character among this frame's `events`, from
    /// the keyboard, and lets go of them after it. A character key's own text event is folded
    /// into one text event; a key without text (Enter, an arrow) gets the modifiers.
    pub fn apply(&mut self, events: &mut Vec<Event>) {
        if !self.active() {
            return;
        }
        let mut out = Vec::with_capacity(events.len());
        let mut held: Option<Event> = None;
        for event in events.drain(..) {
            if !self.active() {
                out.push(event);
                continue;
            }
            match event {
                Event::Key { key, pressed: true, .. } if types_text(key) => held = Some(event),
                Event::Text(s) => {
                    held = None;
                    out.push(Event::Text(self.text(&s)));
                    self.release();
                }
                Event::Key { key, physical_key, pressed: true, repeat, mut modifiers } => {
                    modifiers.ctrl |= self.ctrl;
                    modifiers.command |= self.ctrl;
                    modifiers.alt |= self.alt;
                    out.push(Event::Key { key, physical_key, pressed: true, repeat, modifiers });
                    self.release();
                }
                other => out.push(other),
            }
        }
        // A character key whose text never came (a hardware key egui had no text for).
        if let Some(Event::Key { key, physical_key, repeat, mut modifiers, .. }) = held {
            modifiers.ctrl |= self.ctrl;
            modifiers.command |= self.ctrl;
            modifiers.alt |= self.alt;
            out.push(Event::Key { key, physical_key, pressed: true, repeat, modifiers });
            self.release();
        }
        *events = out;
    }

    /// The bytes a tap on `key` sends, in the terminal's `modes`; `paste` is the clipboard's
    /// text for Paste. Ctrl and Alt only change what is held.
    pub fn press(&mut self, key: BarKey, modes: TermModes, paste: Option<&str>) -> Vec<u8> {
        let term_key = match key {
            BarKey::Ctrl => {
                self.ctrl = !self.ctrl;
                return Vec::new();
            }
            BarKey::Alt => {
                self.alt = !self.alt;
                return Vec::new();
            }
            // A paste is text as it is: held keys stay for the next key.
            BarKey::Paste => return paste.map(|text| encode_paste(text, modes)).unwrap_or_default(),
            BarKey::Char(c) => {
                let bytes = self.text(&c.to_string()).into_bytes();
                self.release();
                return bytes;
            }
            BarKey::Esc => TermKey::Escape,
            BarKey::Tab => TermKey::Tab,
            BarKey::Left => TermKey::Left,
            BarKey::Up => TermKey::Up,
            BarKey::Down => TermKey::Down,
            BarKey::Right => TermKey::Right,
            BarKey::Home => TermKey::Home,
            BarKey::End => TermKey::End,
            BarKey::PageUp => TermKey::PageUp,
            BarKey::PageDown => TermKey::PageDown,
        };
        let mut mods = Mods::empty();
        mods.set(Mods::CTRL, self.ctrl);
        mods.set(Mods::ALT, self.alt);
        self.release();
        encode_key(term_key, mods, modes).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};

    fn key(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    fn ctrl() -> KeyBar {
        KeyBar { ctrl: true, alt: false }
    }

    #[test]
    fn ctrl_then_a_letter_is_a_control_key() {
        let mut bar = ctrl();
        let mut events = vec![key(Key::C), Event::Text("c".into())];
        bar.apply(&mut events);
        assert_eq!(events, vec![Event::Text("\u{3}".into())]);
        let mut bar = ctrl();
        assert_eq!(bar.press(BarKey::Char('_'), TermModes::empty(), None), vec![0x1f]);
    }

    #[test]
    fn alt_then_a_letter_is_alt_modified() {
        let mut bar = KeyBar { ctrl: false, alt: true };
        let mut events = vec![Event::Text("x".into())];
        bar.apply(&mut events);
        assert_eq!(events, vec![Event::Text("\u{1b}x".into())]);
    }

    #[test]
    fn a_sticky_key_applies_once() {
        let mut bar = ctrl();
        let mut events = vec![Event::Text("c".into()), Event::Text("d".into())];
        bar.apply(&mut events);
        assert_eq!(events, vec![Event::Text("\u{3}".into()), Event::Text("d".into())]);
        assert_eq!(bar, KeyBar::default());
        let mut bar = ctrl();
        assert_eq!(bar.press(BarKey::Up, TermModes::empty(), None), b"\x1b[1;5A".to_vec());
        assert_eq!(bar.press(BarKey::Up, TermModes::empty(), None), b"\x1b[A".to_vec(), "Ctrl let go after one key");
    }

    #[test]
    fn a_second_tap_releases_a_sticky_key() {
        let mut bar = KeyBar::default();
        assert!(bar.press(BarKey::Ctrl, TermModes::empty(), None).is_empty());
        assert!(bar.ctrl);
        bar.press(BarKey::Ctrl, TermModes::empty(), None);
        assert!(!bar.ctrl);
        let mut events = vec![Event::Text("c".into())];
        bar.apply(&mut events);
        assert_eq!(events, vec![Event::Text("c".into())]);
    }

    #[test]
    fn arrows_are_keys_not_text() {
        let mut bar = KeyBar::default();
        assert_eq!(bar.press(BarKey::Left, TermModes::empty(), None), b"\x1b[D".to_vec());
        // A full-screen program asked for application cursor keys.
        assert_eq!(bar.press(BarKey::Left, TermModes::APP_CURSOR, None), b"\x1bOD".to_vec());
        let mut bar = ctrl();
        let mut events = vec![key(Key::ArrowRight)];
        bar.apply(&mut events);
        assert!(matches!(&events[..], [Event::Key { key: Key::ArrowRight, modifiers, .. }] if modifiers.ctrl));
    }

    #[test]
    fn paste_is_not_modified_by_ctrl() {
        let mut bar = ctrl();
        assert_eq!(bar.press(BarKey::Paste, TermModes::empty(), Some("ls")), b"ls".to_vec());
        assert!(bar.ctrl, "Ctrl stays for the next key");
        assert!(bar.press(BarKey::Paste, TermModes::empty(), None).is_empty(), "an empty clipboard sends nothing");
    }
}

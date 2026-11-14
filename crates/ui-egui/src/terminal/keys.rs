//! egui's keys as the terminal's keys. Printable characters come as `Event::Text` and are not
//! mapped here, except with Ctrl or Alt, which turn them into control bytes or an escape prefix.
//! Ctrl+C, Ctrl+X and Ctrl+V come as `Event::Copy`, `Cut` and `Paste` and are left to those.

use sshcraft_term::input::{Key as TermKey, Mods};

fn mods(m: egui::Modifiers) -> Mods {
    let mut out = Mods::empty();
    out.set(Mods::SHIFT, m.shift);
    out.set(Mods::ALT, m.alt);
    out.set(Mods::CTRL, m.ctrl);
    out
}

/// The terminal key for `key` with `modifiers`, or `None` when egui delivers it otherwise.
pub fn map_key(key: egui::Key, modifiers: egui::Modifiers) -> Option<(TermKey, Mods)> {
    use egui::Key as K;
    let m = mods(modifiers);
    let named = match key {
        K::Enter => Some(TermKey::Enter),
        K::Tab => Some(TermKey::Tab),
        K::Backspace => Some(TermKey::Backspace),
        K::Escape => Some(TermKey::Escape),
        K::ArrowUp => Some(TermKey::Up),
        K::ArrowDown => Some(TermKey::Down),
        K::ArrowLeft => Some(TermKey::Left),
        K::ArrowRight => Some(TermKey::Right),
        K::Home => Some(TermKey::Home),
        K::End => Some(TermKey::End),
        K::PageUp => Some(TermKey::PageUp),
        K::PageDown => Some(TermKey::PageDown),
        K::Insert => Some(TermKey::Insert),
        K::Delete => Some(TermKey::Delete),
        K::F1 => Some(TermKey::F(1)),
        K::F2 => Some(TermKey::F(2)),
        K::F3 => Some(TermKey::F(3)),
        K::F4 => Some(TermKey::F(4)),
        K::F5 => Some(TermKey::F(5)),
        K::F6 => Some(TermKey::F(6)),
        K::F7 => Some(TermKey::F(7)),
        K::F8 => Some(TermKey::F(8)),
        K::F9 => Some(TermKey::F(9)),
        K::F10 => Some(TermKey::F(10)),
        K::F11 => Some(TermKey::F(11)),
        K::F12 => Some(TermKey::F(12)),
        _ => None,
    };
    if let Some(named) = named {
        return Some((named, m));
    }
    if !(modifiers.ctrl || modifiers.alt) {
        return None;
    }
    // Copy, cut and paste arrive as their own events.
    if modifiers.ctrl && !modifiers.alt && !modifiers.shift && matches!(key, K::C | K::X | K::V) {
        return None;
    }
    let c = match key {
        K::Space => ' ',
        K::OpenBracket => '[',
        K::CloseBracket => ']',
        K::Backslash => '\\',
        K::Minus => '-',
        K::Slash => '/',
        other => {
            let name = other.name();
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => c.to_ascii_lowercase(),
                _ => return None,
            }
        }
    };
    Some((TermKey::Char(c), m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};

    #[test]
    fn letters_are_text_unless_ctrl_or_alt() {
        assert_eq!(map_key(Key::A, Modifiers::NONE), None);
        assert_eq!(map_key(Key::A, Modifiers::SHIFT), None);
        assert_eq!(map_key(Key::A, Modifiers::CTRL), Some((TermKey::Char('a'), Mods::CTRL)));
        assert_eq!(map_key(Key::Num1, Modifiers::ALT), Some((TermKey::Char('1'), Mods::ALT)));
        assert_eq!(map_key(Key::C, Modifiers::CTRL), None);
        assert_eq!(map_key(Key::ArrowUp, Modifiers::SHIFT), Some((TermKey::Up, Mods::SHIFT)));
    }
}

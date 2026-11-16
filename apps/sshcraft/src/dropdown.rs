//! The dropdown window: a global shortcut, set under *Settings › Appearance*, shows SSHCraft at
//! the top of the screen or hides it again, from any program. The shortcut only shows and hides
//! the window; it never types into a session. Off unless set.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

/// A shortcut in SSHCraft's notation (`Ctrl+Alt+T`, `Ctrl+``, `F12`) as the system wants it.
pub fn hotkey(combo: &str) -> Option<HotKey> {
    if !sshcraft_engine::settings::valid_combo(combo) {
        return None;
    }
    let parts: Vec<&str> = combo.split('+').collect();
    let (key, modifiers) = parts.split_last()?;
    let mut mods = Modifiers::empty();
    for m in modifiers {
        mods |= match *m {
            "Ctrl" => Modifiers::CONTROL,
            "Alt" => Modifiers::ALT,
            "Shift" => Modifiers::SHIFT,
            "Meta" => Modifiers::META,
            _ => return None,
        };
    }
    Some(HotKey::new((!mods.is_empty()).then_some(mods), code(key)?))
}

fn code(key: &str) -> Option<Code> {
    let upper = key.to_ascii_uppercase();
    let named = match upper.as_str() {
        "`" => Code::Backquote,
        "-" => Code::Minus,
        "=" => Code::Equal,
        "," => Code::Comma,
        "." => Code::Period,
        "/" => Code::Slash,
        ";" => Code::Semicolon,
        "SPACE" => Code::Space,
        "ENTER" => Code::Enter,
        "TAB" => Code::Tab,
        "ESCAPE" => Code::Escape,
        _ => {
            // Letters, digits and F-keys by the names the hotkey crate reads.
            let name = match upper.chars().next() {
                Some(c) if upper.len() == 1 && c.is_ascii_alphabetic() => format!("Key{c}"),
                Some(c) if upper.len() == 1 && c.is_ascii_digit() => format!("Digit{c}"),
                Some('F') if upper.len() > 1 => upper.clone(),
                _ => return None,
            };
            return name.parse::<Code>().ok();
        }
    };
    Some(named)
}

/// The registered shortcut and the presses it brought.
pub struct Dropdown {
    manager: Option<GlobalHotKeyManager>,
    current: Option<(String, HotKey)>,
    pressed: Arc<AtomicBool>,
    /// The last shortcut the system refused, so it is reported once.
    refused: Option<String>,
}

impl Dropdown {
    /// Starts listening; `ctx` is woken on a press, also while the window is hidden.
    pub fn new(ctx: &egui::Context) -> Self {
        let pressed = Arc::new(AtomicBool::new(false));
        let seen = pressed.clone();
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new().name("sshcraft-dropdown".into()).spawn(move || {
            while let Ok(event) = GlobalHotKeyEvent::receiver().recv() {
                if event.state == HotKeyState::Pressed {
                    seen.store(true, Ordering::SeqCst);
                    ctx.request_repaint();
                }
            }
        });
        Self { manager: GlobalHotKeyManager::new().ok(), current: None, pressed, refused: None }
    }

    /// Registers `wanted` (or nothing) when it changed; a shortcut the system refused comes back
    /// once as an error.
    pub fn sync(&mut self, wanted: Option<&str>) -> Result<(), String> {
        let wanted = wanted.filter(|w| !w.is_empty());
        if self.current.as_ref().map(|(c, _)| c.as_str()) == wanted || (wanted.is_some() && self.refused.as_deref() == wanted) {
            return Ok(());
        }
        let Some(manager) = &self.manager else { return Err("global shortcuts are not available here".into()) };
        if let Some((_, old)) = self.current.take() {
            let _ = manager.unregister(old);
        }
        let Some(combo) = wanted else {
            self.refused = None;
            return Ok(());
        };
        let Some(key) = hotkey(combo) else {
            self.refused = Some(combo.to_owned());
            return Err(format!("{combo} cannot be a global shortcut"));
        };
        match manager.register(key) {
            Ok(()) => {
                self.current = Some((combo.to_owned(), key));
                self.refused = None;
                Ok(())
            }
            Err(e) => {
                self.refused = Some(combo.to_owned());
                Err(e.to_string())
            }
        }
    }

    /// Whether the shortcut was pressed since the last call.
    pub fn take_pressed(&self) -> bool {
        self.pressed.swap(false, Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_become_system_hotkeys() {
        assert_eq!(hotkey("Ctrl+`"), Some(HotKey::new(Some(Modifiers::CONTROL), Code::Backquote)));
        assert_eq!(hotkey("Ctrl+Alt+T"), Some(HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyT)));
        assert_eq!(hotkey("F12"), Some(HotKey::new(None, Code::F12)));
        assert_eq!(hotkey("Meta+1"), Some(HotKey::new(Some(Modifiers::META), Code::Digit1)));
        for bad in ["", "T", "Shift+T", "Ctrl+", "Alt+Ctrl+T", "Ctrl+F99", "Ctrl+ä"] {
            assert_eq!(hotkey(bad), None, "{bad}");
        }
    }
}

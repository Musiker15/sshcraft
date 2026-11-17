//! SSHCraft on Android: opens the engine in the app's private storage and shows the interface in
//! its tablet form. android-activity starts the process and calls [`android_main`]; the APK is
//! put together by `cargo xtask android`.
//!
//! The helpers at the top are plain functions so they can be tested on the desktop.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// The part of the window, in points, that the system covers: the status bar at the top, the
/// navigation bar and the on-screen keyboard at the bottom. `window` is the window in points,
/// `content` the visible content rect `(left, top, right, bottom)` in pixels, `ppp` the pixels
/// per point. Never negative, also when the content rect lags behind a rotation.
pub fn covered(window: egui::Rect, content: (i32, i32, i32, i32), ppp: f32) -> egui::epaint::MarginF32 {
    if !(ppp.is_finite() && ppp > 0.0) {
        return egui::epaint::MarginF32::ZERO;
    }
    let (left, top, right, bottom) = content;
    let px = |v: i32| v as f32 / ppp;
    let inset = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    egui::epaint::MarginF32 {
        left: inset(px(left) - window.left()),
        top: inset(px(top) - window.top()),
        right: inset(window.right() - px(right)),
        bottom: inset(window.bottom() - px(bottom)),
    }
}

/// What a hardware keyboard's clipboard key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardKey {
    Copy,
    Cut,
    Paste,
}

fn clipboard_key(event: &egui::Event) -> Option<ClipboardKey> {
    let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { return None };
    match key {
        egui::Key::C if modifiers.command_only() => Some(ClipboardKey::Copy),
        egui::Key::X if modifiers.command_only() => Some(ClipboardKey::Cut),
        // Ctrl+V in a field; Ctrl+Shift+V and Shift+Insert in the terminal.
        egui::Key::V if modifiers.command_only() || (modifiers.ctrl && modifiers.shift && !modifiers.alt) => Some(ClipboardKey::Paste),
        egui::Key::Insert if modifiers.shift_only() => Some(ClipboardKey::Paste),
        _ => None,
    }
}

/// A hardware keyboard's clipboard keys, as egui-winit makes them on the desktop, but with
/// Android's clipboard: Ctrl+C and Ctrl+X become copy and cut, the paste keys a paste of
/// `clipboard` (read only when such a key came; nothing is pasted from an empty one). The app
/// turns egui-winit's own handling off, because its clipboard on Android reaches no other app.
pub fn clipboard_keys(events: &mut Vec<egui::Event>, clipboard: impl FnOnce() -> Option<String>) {
    if !events.iter().any(|e| clipboard_key(e).is_some()) {
        return;
    }
    // Read once per frame, however many paste keys came.
    let mut read = Some(clipboard);
    let mut text: Option<String> = None;
    let mut out = Vec::with_capacity(events.len());
    for event in events.drain(..) {
        match clipboard_key(&event) {
            None => out.push(event),
            Some(ClipboardKey::Copy) => out.push(egui::Event::Copy),
            Some(ClipboardKey::Cut) => out.push(egui::Event::Cut),
            Some(ClipboardKey::Paste) => {
                if let Some(read) = read.take() {
                    text = read().filter(|t| !t.is_empty()).map(|t| t.replace("\r\n", "\n"));
                }
                if let Some(text) = &text {
                    out.push(egui::Event::Paste(text.clone()));
                }
            }
        }
    }
    *events = out;
}

#[cfg(target_os = "android")]
mod android;

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, Rect, pos2};

    fn window() -> Rect {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1280.0, 800.0))
    }

    #[test]
    fn the_inset_is_what_the_keyboard_covers() {
        // 2 pixels per point: a keyboard covering the lower 600 pixels covers 300 points, the
        // status bar's 48 pixels 24 points.
        let covered = covered(window(), (0, 48, 2560, 1000), 2.0);
        assert_eq!(covered.bottom, 300.0);
        assert_eq!(covered.top, 24.0);
        assert_eq!((covered.left, covered.right), (0.0, 0.0));
    }

    #[test]
    fn a_content_rect_beyond_the_window_is_no_inset() {
        // Just after a rotation the content rect can still be the old, larger one.
        let covered = covered(window(), (-10, -10, 4000, 4000), 2.0);
        assert_eq!(covered, egui::epaint::MarginF32::ZERO);
        assert_eq!(super::covered(window(), (0, 0, 10, 10), 0.0), egui::epaint::MarginF32::ZERO, "no scale yet");
    }

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    #[test]
    fn the_paste_keys_paste_the_system_clipboard() {
        for paste in [key(Key::V, Modifiers::COMMAND), key(Key::V, Modifiers::CTRL | Modifiers::SHIFT), key(Key::Insert, Modifiers::SHIFT)] {
            let mut events = vec![Event::Text("a".into()), paste];
            clipboard_keys(&mut events, || Some("from\r\nandroid".into()));
            assert_eq!(events, vec![Event::Text("a".into()), Event::Paste("from\nandroid".into())]);
        }
        let mut events = vec![key(Key::Insert, Modifiers::SHIFT)];
        clipboard_keys(&mut events, || None);
        assert!(events.is_empty(), "an empty clipboard pastes nothing");
    }

    #[test]
    fn ctrl_c_and_x_copy_and_cut() {
        let mut events = vec![key(Key::C, Modifiers::COMMAND), key(Key::X, Modifiers::COMMAND)];
        clipboard_keys(&mut events, || panic!("copying reads no clipboard"));
        assert_eq!(events, vec![Event::Copy, Event::Cut]);
    }

    #[test]
    fn other_keys_never_read_the_clipboard() {
        // Ctrl+Shift+C is the terminal's own copy key; Alt+V is typed as usual.
        let mut events = vec![key(Key::C, Modifiers::CTRL | Modifiers::SHIFT), key(Key::V, Modifiers::ALT), Event::Text("v".into())];
        clipboard_keys(&mut events, || panic!("the clipboard was read"));
        assert_eq!(events.len(), 3);
    }
}

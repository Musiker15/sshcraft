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

/// Puts a hardware keyboard's paste keys (Ctrl+Shift+V, Shift+Insert) into a paste of `clipboard`
/// (Android's clipboard, read only when such a key came). egui's own clipboard on Android knows
/// only what was copied inside the app.
pub fn paste_keys(events: &mut Vec<egui::Event>, clipboard: impl FnOnce() -> Option<String>) {
    let is_paste = |e: &egui::Event| {
        matches!(e, egui::Event::Key { key: egui::Key::V, pressed: true, modifiers, .. } if modifiers.ctrl && modifiers.shift)
            || matches!(e, egui::Event::Key { key: egui::Key::Insert, pressed: true, modifiers, .. } if modifiers.shift_only())
    };
    let Some(at) = events.iter().position(is_paste) else { return };
    match clipboard() {
        Some(text) if !text.is_empty() => events[at] = egui::Event::Paste(text),
        _ => {
            events.remove(at);
        }
    }
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
    fn ctrl_shift_v_pastes_the_system_clipboard() {
        let mut events = vec![Event::Text("a".into()), key(Key::V, Modifiers::CTRL | Modifiers::SHIFT)];
        paste_keys(&mut events, || Some("from android".into()));
        assert_eq!(events, vec![Event::Text("a".into()), Event::Paste("from android".into())]);
        let mut events = vec![key(Key::Insert, Modifiers::SHIFT)];
        paste_keys(&mut events, || None);
        assert!(events.is_empty(), "an empty clipboard pastes nothing");
    }

    #[test]
    fn other_keys_never_read_the_clipboard() {
        let mut events = vec![key(Key::V, Modifiers::CTRL), Event::Text("v".into())];
        paste_keys(&mut events, || panic!("the clipboard was read"));
        assert_eq!(events.len(), 2);
    }
}

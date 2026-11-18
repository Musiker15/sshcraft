//! SSHCraft on Android: opens the engine in the app's private storage and shows the interface in
//! its tablet form. android-activity starts the process and calls [`android_main`]; the APK is
//! put together by `cargo xtask android`.
//!
//! The helpers at the top are plain functions so they can be tested on the desktop.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// The answer of the biometric prompt (`Biometric.poll` in `java/`, codes kept in step by a test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Pending,
    Verified,
    Declined,
}

/// `Biometric.poll`'s code as an answer: -1 open, 0 confirmed, anything else declined.
pub fn answer(code: i32) -> Answer {
    match code {
        -1 => Answer::Pending,
        0 => Answer::Verified,
        _ => Answer::Declined,
    }
}

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

/// `MainActivity.covered()` as `(left, top, right, bottom)` in pixels: four 16-bit values.
pub fn unpack_covered(packed: u64) -> (i32, i32, i32, i32) {
    let part = |shift: u32| ((packed >> shift) & 0xFFFF) as i32;
    (part(0), part(16), part(32), part(48))
}

/// The larger of two margins, side by side: what the system covers by either report.
pub fn widest(a: egui::epaint::MarginF32, b: egui::epaint::MarginF32) -> egui::epaint::MarginF32 {
    egui::epaint::MarginF32 { left: a.left.max(b.left), top: a.top.max(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) }
}

/// Pixels the system covers, from the window's edges, as points: never negative.
pub fn covered_edges((left, top, right, bottom): (i32, i32, i32, i32), ppp: f32) -> egui::epaint::MarginF32 {
    if !(ppp.is_finite() && ppp > 0.0) {
        return egui::epaint::MarginF32::ZERO;
    }
    let points = |v: i32| (v.max(0) as f32) / ppp;
    egui::epaint::MarginF32 { left: points(left), top: points(top), right: points(right), bottom: points(bottom) }
}

/// How long after another field took the keyboard it is shown again: after egui-winit's reset of
/// the text input, which hides it, went through.
pub const RESHOW_AFTER: std::time::Duration = std::time::Duration::from_millis(150);

/// How long the keyboard stays asked for after the last frame that wanted it.
pub const KEEP_KEYBOARD: std::time::Duration = std::time::Duration::from_millis(400);

/// Whether the keyboard stays open in a frame that does not ask for it, `since` the last frame
/// that did. Opening the keyboard moves the interface up (the window insets change), and a frame
/// in between may lose the field's request for text; closing then would close the keyboard the
/// user just opened.
pub fn keep_keyboard(since: std::time::Duration) -> bool {
    since < KEEP_KEYBOARD
}

/// Whether the on-screen keyboard is up: what the system covers at the bottom (`bottom_px`, the
/// keyboard and the navigation bar) is more than a fifth of the window. The navigation bar alone
/// is far less.
pub fn keyboard_up(bottom_px: i32, window_height_px: f32) -> bool {
    window_height_px > 0.0 && bottom_px as f32 > window_height_px * 0.2
}

/// Whether a tap opens the keyboard again: the user closed it (the back key, the keyboard's own
/// button) and tapped into the terminal or field that still wants text (`asked_now`; a tap on
/// the key bar beside it leaves the keyboard closed). Nothing else changed, so
/// without this the keyboard stayed closed until another field took the focus.
pub fn show_again(tapped: bool, asked_now: bool, up: bool) -> bool {
    tapped && asked_now && !up
}

/// When to draw the next frame without input: at once after the visible area changed (the
/// on-screen keyboard opened or closed, the tablet turned), often for a while after input (the
/// keyboard animates after a tap), and once a second otherwise, since Android tells the app
/// nothing about the keyboard.
pub fn repaint_after(content_changed: bool, since_input: std::time::Duration) -> std::time::Duration {
    if content_changed {
        std::time::Duration::ZERO
    } else if since_input < std::time::Duration::from_secs(2) {
        std::time::Duration::from_millis(100)
    } else {
        std::time::Duration::from_secs(1)
    }
}

/// Text the on-screen keyboard committed at once, as egui's event: one character is typed, more
/// (a paste from the keyboard's clipboard, a swiped word) are a paste, so the terminal sends
/// them as one bracketed paste and a field inserts them whole.
pub fn committed(text: String) -> egui::Event {
    if text.chars().nth(1).is_some() || text.contains(['\n', '\r']) {
        egui::Event::Paste(text.replace("\r\n", "\n"))
    } else {
        egui::Event::Text(text)
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
#[cfg(target_os = "android")]
mod background;
#[cfg(target_os = "android")]
mod biometric;
#[cfg(target_os = "android")]
mod keyboard;

/// Where `config.toml` lives: in the app's own data folder. Android has no config folder the
/// settings library could find, so it fell back to a folder the app cannot write, and every
/// setting (the language among them) was lost on the next start.
pub fn settings_location(data_dir: &std::path::Path) -> craft_settings::Location {
    craft_settings::Location::Dir(data_dir.join("config"))
}

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

    #[test]
    fn the_keyboard_and_the_bars_are_kept_free() {
        // As MainActivity packs them: 0 left, 96 px status bar, 0 right, 1200 px keyboard.
        let packed = 96u64 << 16 | 1200u64 << 48;
        assert_eq!(unpack_covered(packed), (0, 96, 0, 1200));
        let edges = covered_edges(unpack_covered(packed), 2.0);
        assert_eq!((edges.top, edges.bottom), (48.0, 600.0));
        assert_eq!(covered_edges((0, 0, 0, 10), 0.0), egui::epaint::MarginF32::ZERO, "no scale yet");
        let content = covered(window(), (0, 0, 2560, 1600), 2.0);
        assert_eq!(widest(edges, content).bottom, 600.0, "the window inset wins where the content rect sees nothing");
    }

    /// The packing in Rust is the one in Java.
    #[test]
    fn the_packing_matches_the_java_class() {
        let java = include_str!("../java/de/musiker15/sshcraft/MainActivity.java");
        assert!(java.contains("pack(used.left) | pack(used.top) << 16 | pack(used.right) << 32 | pack(used.bottom) << 48"));
        assert!(java.contains("Math.min(pixels, 0xFFFF)"));
    }

    #[test]
    fn a_single_frame_without_a_field_keeps_the_keyboard() {
        use std::time::Duration;
        assert!(keep_keyboard(Duration::from_millis(16)), "one frame of a layout change");
        assert!(keep_keyboard(Duration::from_millis(300)));
        assert!(!keep_keyboard(Duration::from_millis(500)), "the field was left");
    }

    #[test]
    fn the_keyboard_opening_is_drawn_soon() {
        use std::time::Duration;
        assert_eq!(repaint_after(true, Duration::from_secs(60)), Duration::ZERO);
        assert!(repaint_after(false, Duration::from_millis(300)) <= Duration::from_millis(100), "right after a tap");
        assert!(repaint_after(false, Duration::from_secs(60)) <= Duration::from_secs(1), "never stale for long");
    }

    #[test]
    fn a_keyboard_paste_is_a_paste_and_a_letter_is_typed() {
        assert_eq!(committed("a".into()), Event::Text("a".into()));
        assert_eq!(committed("ä".into()), Event::Text("ä".into()), "one character, however many bytes");
        assert_eq!(committed("ssh-ed25519 AAAA".into()), Event::Paste("ssh-ed25519 AAAA".into()));
        assert_eq!(committed("\r".into()), Event::Paste(String::from("\r")), "a line break is never typed as text");
        assert_eq!(committed("a\r\nb".into()), Event::Paste("a\nb".into()));
    }

    #[test]
    fn only_a_confirmation_unlocks() {
        assert_eq!(answer(0), Answer::Verified);
        assert_eq!(answer(-1), Answer::Pending);
        for declined in [1, 2, 7, i32::MIN, i32::MAX] {
            assert_eq!(answer(declined), Answer::Declined, "code {declined}");
        }
    }

    /// The codes `answer` reads are the ones the Java class writes.
    #[test]
    fn the_codes_match_the_java_class() {
        let java = include_str!("../java/de/musiker15/sshcraft/Biometric.java");
        for (name, code) in [("PENDING", -1), ("VERIFIED", 0), ("DECLINED", 1)] {
            assert!(java.contains(&format!("public static final int {name} = {code};")), "{name} is {code} in Java");
        }
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

    #[test]
    fn settings_live_in_the_app_data_folder() {
        let data = std::path::Path::new("/data/user/0/de.musiker15.sshcraft/files");
        match super::settings_location(data) {
            craft_settings::Location::Dir(dir) => assert!(dir.starts_with(data), "{dir:?}"),
            other => panic!("settings must be written in the data folder, not {other:?}"),
        }
    }

    #[test]
    fn the_navigation_bar_alone_is_no_keyboard() {
        assert!(!keyboard_up(72, 1600.0), "a navigation bar");
        assert!(keyboard_up(900, 1600.0), "a keyboard");
        assert!(!keyboard_up(900, 0.0), "no window yet");
    }

    #[test]
    fn a_tap_reopens_a_closed_keyboard() {
        assert!(show_again(true, true, false), "closed by the user, tapped again");
        assert!(!show_again(true, true, true), "already up");
        assert!(!show_again(false, true, false), "no tap: the user closed it on purpose");
        assert!(!show_again(true, false, false), "the tap went to something without text");
    }
}

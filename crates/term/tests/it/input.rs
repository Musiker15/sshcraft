//! Keys, mouse and paste as the bytes a terminal sends, after xterm's control sequences.

use sshcraft_term::TermModes;
use sshcraft_term::input::{Key, Mods, MouseAction, MouseButton, encode_focus, encode_key, encode_mouse, encode_paste};

fn key(k: Key, mods: Mods) -> Vec<u8> {
    encode_key(k, mods, TermModes::empty()).unwrap_or_default()
}

#[test]
fn arrows_follow_the_cursor_mode() {
    assert_eq!(key(Key::Up, Mods::empty()), b"\x1b[A");
    assert_eq!(encode_key(Key::Up, Mods::empty(), TermModes::APP_CURSOR).unwrap(), b"\x1bOA");
    assert_eq!(key(Key::Right, Mods::CTRL), b"\x1b[1;5C");
    assert_eq!(encode_key(Key::Right, Mods::SHIFT, TermModes::APP_CURSOR).unwrap(), b"\x1b[1;2C");
}

#[test]
fn control_letters_become_c0() {
    assert_eq!(key(Key::Char('c'), Mods::CTRL), [0x03]);
    assert_eq!(key(Key::Char('C'), Mods::CTRL), [0x03]);
    assert_eq!(key(Key::Char('['), Mods::CTRL), [0x1b]);
    assert_eq!(key(Key::Char(' '), Mods::CTRL), [0x00]);
    assert_eq!(key(Key::Char('?'), Mods::CTRL), [0x7f]);
}

#[test]
fn alt_prefixes_escape() {
    assert_eq!(key(Key::Char('x'), Mods::ALT), b"\x1bx");
    assert_eq!(key(Key::Char('c'), Mods::ALT | Mods::CTRL), b"\x1b\x03");
    assert_eq!(key(Key::Char('ä'), Mods::empty()), "ä".as_bytes());
}

#[test]
fn keys_match_xterm() {
    let table: &[(Key, Mods, &[u8])] = &[
        (Key::Enter, Mods::empty(), b"\r"),
        (Key::Backspace, Mods::empty(), b"\x7f"),
        (Key::Backspace, Mods::CTRL, b"\x08"),
        (Key::Tab, Mods::empty(), b"\t"),
        (Key::Tab, Mods::SHIFT, b"\x1b[Z"),
        (Key::Escape, Mods::empty(), b"\x1b"),
        (Key::Insert, Mods::empty(), b"\x1b[2~"),
        (Key::Delete, Mods::empty(), b"\x1b[3~"),
        (Key::Delete, Mods::CTRL, b"\x1b[3;5~"),
        (Key::PageUp, Mods::empty(), b"\x1b[5~"),
        (Key::PageDown, Mods::empty(), b"\x1b[6~"),
        (Key::Home, Mods::empty(), b"\x1b[H"),
        (Key::End, Mods::empty(), b"\x1b[F"),
        (Key::F(1), Mods::empty(), b"\x1bOP"),
        (Key::F(4), Mods::empty(), b"\x1bOS"),
        (Key::F(1), Mods::SHIFT, b"\x1b[1;2P"),
        (Key::F(5), Mods::empty(), b"\x1b[15~"),
        (Key::F(10), Mods::empty(), b"\x1b[21~"),
        (Key::F(11), Mods::empty(), b"\x1b[23~"),
        (Key::F(12), Mods::empty(), b"\x1b[24~"),
        (Key::F(12), Mods::ALT, b"\x1b[24;3~"),
    ];
    for (k, mods, expected) in table {
        assert_eq!(key(*k, *mods), *expected, "{k:?} {mods:?}");
    }
    assert_eq!(encode_key(Key::F(13), Mods::empty(), TermModes::empty()), None);
    assert_eq!(encode_key(Key::Home, Mods::empty(), TermModes::APP_CURSOR).unwrap(), b"\x1bOH");
}

#[test]
fn paste_is_bracketed_when_asked() {
    assert_eq!(encode_paste("ls\n", TermModes::BRACKETED_PASTE), b"\x1b[200~ls\r\x1b[201~");
    assert_eq!(encode_paste("a\r\nb\n", TermModes::empty()), b"a\rb\r");
}

#[test]
fn pasted_text_cannot_close_the_bracket() {
    let evil = "ls\x1b[201~rm -rf ~\r";
    let sent = encode_paste(evil, TermModes::BRACKETED_PASTE);
    assert!(sent.starts_with(b"\x1b[200~") && sent.ends_with(b"\x1b[201~"));
    let inner = &sent[6..sent.len() - 6];
    assert!(!inner.windows(6).any(|w| w == b"\x1b[201~" || w == b"\x1b[200~"), "{inner:?}");
    let nested = encode_paste("a\x1b[20\x1b[201~1~b", TermModes::BRACKETED_PASTE);
    let inner = &nested[6..nested.len() - 6];
    assert!(!inner.windows(6).any(|w| w == b"\x1b[201~"), "{inner:?}");
    let plain = encode_paste(evil, TermModes::empty());
    assert!(!plain.windows(6).any(|w| w == b"\x1b[201~"));
}

#[test]
fn mouse_is_reported_only_when_asked() {
    let sgr = TermModes::MOUSE_REPORT | TermModes::SGR_MOUSE;
    assert_eq!(encode_mouse(MouseButton::Left, MouseAction::Press, 0, 0, Mods::empty(), TermModes::empty()), None);
    assert_eq!(encode_mouse(MouseButton::Left, MouseAction::Press, 0, 0, Mods::empty(), sgr).unwrap(), b"\x1b[<0;1;1M");
    assert_eq!(encode_mouse(MouseButton::Left, MouseAction::Release, 0, 0, Mods::empty(), sgr).unwrap(), b"\x1b[<0;1;1m");
    assert_eq!(encode_mouse(MouseButton::Right, MouseAction::Press, 9, 4, Mods::CTRL, sgr).unwrap(), b"\x1b[<18;10;5M");
    assert_eq!(encode_mouse(MouseButton::WheelUp, MouseAction::Press, 0, 0, Mods::empty(), sgr).unwrap(), b"\x1b[<64;1;1M");
    assert_eq!(encode_mouse(MouseButton::Left, MouseAction::Drag, 1, 1, Mods::empty(), sgr), None);
    let drag = sgr | TermModes::MOUSE_DRAG;
    assert_eq!(encode_mouse(MouseButton::Left, MouseAction::Drag, 1, 1, Mods::empty(), drag).unwrap(), b"\x1b[<32;2;2M");
    let x10 = TermModes::MOUSE_REPORT;
    assert_eq!(encode_mouse(MouseButton::Left, MouseAction::Press, 0, 0, Mods::empty(), x10).unwrap(), b"\x1b[M !!");
    assert_eq!(encode_mouse(MouseButton::Left, MouseAction::Release, 0, 0, Mods::empty(), x10).unwrap(), b"\x1b[M#!!");
}

#[test]
fn old_mouse_encoding_caps_coordinates() {
    assert_eq!(encode_mouse(MouseButton::Left, MouseAction::Press, 300, 0, Mods::empty(), TermModes::MOUSE_REPORT), None);
    assert_eq!(encode_mouse(MouseButton::Left, MouseAction::Press, 222, 0, Mods::empty(), TermModes::MOUSE_REPORT).unwrap(), b"\x1b[M \xff!");
}

#[test]
fn focus_is_reported_only_when_asked() {
    assert_eq!(encode_focus(true, TermModes::empty()), None);
    assert_eq!(encode_focus(true, TermModes::FOCUS_REPORT).unwrap(), b"\x1b[I");
    assert_eq!(encode_focus(false, TermModes::FOCUS_REPORT).unwrap(), b"\x1b[O");
}

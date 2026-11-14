//! The escape sequence rules of `docs/security.md`: what a server prints stays in the terminal.

use sshcraft_term::limits::{MAX_CLIPBOARD_BYTES, MAX_LINK_BYTES, MAX_TITLE_CHARS};
use sshcraft_term::{Notice, TermConfig, TermSize, Terminal};

use crate::{row_text, term};

fn notices(bytes: &[u8]) -> Vec<Notice> {
    let mut t = term(80, 24);
    t.feed(bytes);
    t.take_notices()
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn osc52_writes_the_clipboard() {
    assert_eq!(notices(b"\x1b]52;c;aGVsbG8=\x07"), vec![Notice::ClipboardWrite("hello".into())]);
}

#[test]
fn osc52_never_reads() {
    let found = notices(b"\x1b]52;c;?\x07");
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn osc52_is_off_when_the_setting_is_off() {
    let mut t = Terminal::new(TermSize::clamped(80, 24), &TermConfig { clipboard_write: false, ..TermConfig::default() });
    t.feed(b"\x1b]52;c;aGVsbG8=\x07");
    assert!(t.take_notices().is_empty());
}

#[test]
fn osc52_over_the_limit_is_dropped() {
    let text = vec![b'x'; MAX_CLIPBOARD_BYTES + 1];
    let found = notices(format!("\x1b]52;c;{}\x07", base64(&text)).as_bytes());
    assert!(!found.iter().any(|n| matches!(n, Notice::ClipboardWrite(_))));
    let text = vec![b'x'; MAX_CLIPBOARD_BYTES];
    let found = notices(format!("\x1b]52;c;{}\x07", base64(&text)).as_bytes());
    assert!(found.iter().any(|n| matches!(n, Notice::ClipboardWrite(t) if t.len() == MAX_CLIPBOARD_BYTES)));
}

#[test]
fn an_endless_osc_is_dropped_and_output_goes_on() {
    let mut t = term(80, 24);
    t.feed(b"\x1b]0;");
    for _ in 0..64 {
        t.feed(&[b'A'; 65536]);
    }
    t.feed(b"\x07ok");
    assert!(!t.take_notices().iter().any(|n| matches!(n, Notice::Title(_))));
    assert_eq!(row_text(&t.screen(), 0), "ok");
}

#[test]
fn title_queries_get_no_answer() {
    let found = notices(b"\x1b]0;secret\x07\x1b[21t\x1b[20t");
    assert!(!found.iter().any(|n| matches!(n, Notice::Reply(_))), "{found:?}");
}

#[test]
fn titles_are_bounded_and_clean() {
    let title = format!("a\u{202e}b{}", "c".repeat(1000));
    let found = notices(format!("\x1b]0;{title}\x07").as_bytes());
    let Some(Notice::Title(shown)) = found.first() else { panic!("no title: {found:?}") };
    assert!(shown.chars().count() <= MAX_TITLE_CHARS);
    assert!(shown.starts_with("abc"));
    assert!(!shown.chars().any(|c| c.is_control() || ('\u{202a}'..='\u{202e}').contains(&c)));
}

#[test]
fn zmodem_and_trzsz_start_nothing() {
    let found = notices(b"**\x18B00000000000000\r\x8a\x11::TRZSZ:TRANSFER:R:1.1.0:0123456789\r\n");
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn device_queries_are_answered_to_the_own_terminal() {
    let mut a = term(80, 24);
    let mut b = term(80, 24);
    a.feed(b"\x1b[c\x1b[6n");
    let replies: Vec<_> = a.take_notices().into_iter().filter(|n| matches!(n, Notice::Reply(_))).collect();
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert_eq!(replies[1], Notice::Reply(b"\x1b[1;1R".to_vec()));
    assert!(b.take_notices().is_empty());
}

#[test]
fn long_links_are_dropped() {
    let mut t = term(80, 24);
    let long = format!("https://example.com/{}", "a".repeat(MAX_LINK_BYTES));
    t.feed(format!("\x1b]8;;{long}\x1b\\x\x1b]8;;\x1b\\ \x1b]8;;https://example.com/\x1b\\y\x1b]8;;\x1b\\").as_bytes());
    let s = t.screen();
    assert_eq!(s.cells[0].link, None);
    let ok = s.cells[2].link.and_then(|i| s.links.get(i as usize));
    assert_eq!(ok.map(String::as_str), Some("https://example.com/"));
}

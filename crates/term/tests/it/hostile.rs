//! Output written to exhaust memory or time (findings of the R3 review): runs of joining
//! characters, links that never repeat, links longer than kept, and scroll amounts at the ends of
//! the integer range.

use std::time::{Duration, Instant};

use sshcraft_term::input::encode_paste;
use sshcraft_term::limits::{MAX_LINK_BYTES, MAX_ZEROWIDTH};
use sshcraft_term::{TermModes, TermSize};

use crate::term;

#[test]
fn zero_width_runs_are_capped_per_cell() {
    let mut t = term(80, 24);
    t.feed(b"a");
    t.feed("\u{301}".repeat(100_000).as_bytes());
    t.feed(b"b");
    let s = t.screen();
    assert!(s.cells[0].text.chars().count() <= 1 + MAX_ZEROWIDTH, "{} chars", s.cells[0].text.chars().count());
    assert_eq!(s.cells[1].text, "b");
}

#[test]
fn the_zero_width_cap_holds_across_cursor_moves() {
    let mut t = term(80, 24);
    t.feed(b"a");
    let round = format!("{}\x1b[C\x1b[D", "\u{301}".repeat(8));
    for _ in 0..2_000 {
        t.feed(round.as_bytes());
    }
    let s = t.screen();
    assert!(s.cells[0].text.chars().count() <= 1 + MAX_ZEROWIDTH, "{} chars", s.cells[0].text.chars().count());
}

#[test]
fn overlong_links_are_never_stored() {
    let mut t = term(80, 24);
    let long = format!("https://example.com/{}", "a".repeat(MAX_LINK_BYTES * 2));
    t.feed(format!("\x1b]8;;{long}\x1b\\xyz\x1b]8;;\x1b\\").as_bytes());
    assert_eq!(t.extras_bytes(), 0);
    assert_eq!(crate::row_text(&t.screen(), 0), "xyz");
}

#[test]
fn links_that_never_repeat_stay_in_the_extras_budget() {
    let mut t = term(80, 24);
    // 50 000 links of about 1 KiB that never repeat: 50 MiB if all were kept.
    for i in 0..50_000u32 {
        t.feed(format!("\x1b]8;;https://example.com/{i:0>1000}\x1b\\x\x1b]8;;\x1b\\").as_bytes());
    }
    assert!(t.extras_bytes() <= sshcraft_term::limits::EXTRAS_BUDGET, "{} bytes", t.extras_bytes());
}

#[test]
fn screen_with_a_link_per_cell_is_fast() {
    let mut t = term(400, 100);
    for i in 0..(400 * 100) {
        t.feed(format!("\x1b]8;;https://example.com/{i}\x1b\\x\x1b]8;;\x1b\\").as_bytes());
    }
    let start = Instant::now();
    let s = t.screen();
    assert!(s.links.len() > 39_000);
    // Linear in the cells: well under a second even unoptimised (it was quadratic).
    assert!(start.elapsed() < Duration::from_millis(500), "{:?}", start.elapsed());
}

#[test]
fn scrolling_by_any_amount_is_safe() {
    let mut t = term(80, 24);
    for i in 0..100 {
        t.feed(format!("{i}\r\n").as_bytes());
    }
    t.scroll(i32::MAX);
    assert!(t.screen().display_offset <= 100);
    t.scroll(i32::MAX);
    t.scroll(i32::MIN);
    assert_eq!(t.screen().display_offset, 0);
}

#[test]
fn a_pathological_paste_is_fast() {
    let n = 40_000;
    let text = format!("{}\x1b[201~{}", "\x1b[20".repeat(n), "1~".repeat(n));
    let start = Instant::now();
    let sent = encode_paste(&text, TermModes::BRACKETED_PASTE);
    assert!(start.elapsed() < Duration::from_millis(500), "{:?}", start.elapsed());
    let inner = &sent[6..sent.len() - 6];
    assert!(!inner.contains(&0x1b));
    let _ = TermSize::clamped(1, 1);
}

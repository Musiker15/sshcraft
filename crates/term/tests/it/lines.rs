//! Completed output lines for triggers: each line once, when it ends, only when asked for.

use crate::term;

#[test]
fn nothing_is_kept_unless_asked() {
    let mut t = term(40, 10);
    t.feed(b"one\r\ntwo\r\n");
    assert!(t.take_completed_lines().is_empty());
}

#[test]
fn each_line_comes_once_when_it_ends() {
    let mut t = term(40, 10);
    t.set_line_capture(true);
    t.feed(b"one\r\ntw");
    assert_eq!(t.take_completed_lines(), ["one"]);
    t.feed(b"o\r\nthree");
    assert_eq!(t.take_completed_lines(), ["two"]);
    assert!(t.take_completed_lines().is_empty());
}

#[test]
fn full_screen_programs_are_left_out() {
    let mut t = term(40, 10);
    t.set_line_capture(true);
    t.feed(b"\x1b[?1049hERROR in vim\r\n\x1b[?1049l");
    assert!(t.take_completed_lines().is_empty());
}

#[test]
fn completed_lines_are_bounded() {
    let mut t = term(40, 10);
    t.set_line_capture(true);
    t.feed(&b"x\r\n".repeat(5000));
    assert!(t.take_completed_lines().len() <= sshcraft_term::MAX_COMPLETED_LINES);
}

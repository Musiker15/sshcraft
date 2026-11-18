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

/// A line wider than the terminal fills several rows but is one line of the output: counted
/// from the top row on screen, the lines are the output's, not the rows.
#[test]
fn wrapped_rows_count_as_one_line_from_the_top() {
    let mut t = term(10, 4);
    t.feed(b"aaaaaaaaaaaaaaaaaaaaaaaaa\r\nb\r\nc");
    // Five rows: the long line takes three, so the top row on screen is its second.
    assert_eq!(t.logical_lines_from_top(), 3, "the long line, b and c");
    let mut plain = term(10, 4);
    plain.feed(b"1\r\n2\r\n3\r\n4\r\n5");
    assert_eq!(plain.logical_lines_from_top(), 4, "without wraps, one line per row");
}

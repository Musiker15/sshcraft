use sshcraft_term::{Direction, GridPoint, SelectionKind, TermConfig, TermSize, Terminal};

use crate::term;

fn lines(t: &mut Terminal, from: usize, to: usize) {
    for i in from..to {
        t.feed(format!("line {i}\r\n").as_bytes());
    }
}

fn with_scrollback(scrollback: usize) -> Terminal {
    Terminal::new(TermSize::clamped(80, 24), &TermConfig { scrollback, ..TermConfig::default() })
}

#[test]
fn scrolling_shows_older_lines() {
    let mut t = term(80, 24);
    lines(&mut t, 0, 300);
    t.scroll(10);
    let s = t.screen();
    assert_eq!(s.display_offset, 10);
    // 300 lines and an empty prompt line: the newest screen starts at line 277, ten back at 267.
    assert_eq!(crate::row_text(&s, 0), "line 267");
    t.scroll_to_bottom();
    assert_eq!(t.screen().display_offset, 0);
}

#[test]
fn scrollback_drops_the_oldest_lines() {
    let mut t = with_scrollback(100);
    lines(&mut t, 0, 500);
    // 500 lines plus the empty last row: 24 on screen, 100 in the history.
    assert_eq!(t.line_text(-100), "line 377");
}

#[test]
fn word_selection_takes_the_word() {
    let mut t = term(80, 24);
    t.feed(b"foo bar baz");
    t.select_start(GridPoint { line: 0, col: 5 }, SelectionKind::Word);
    assert_eq!(t.selection_text().as_deref(), Some("bar"));
}

#[test]
fn dragging_selects_from_start_to_end() {
    let mut t = term(80, 24);
    t.feed(b"hello world");
    t.select_start(GridPoint { line: 0, col: 0 }, SelectionKind::Simple);
    t.select_update(GridPoint { line: 0, col: 4 });
    assert_eq!(t.selection_text().as_deref(), Some("hello"));
    assert!(t.screen().selection.is_some());
    t.clear_selection();
    assert_eq!(t.selection_text(), None);
}

#[test]
fn line_selection_takes_wrapped_lines_as_one() {
    let mut t = term(10, 5);
    t.feed(b"abcdefghijklmno");
    t.select_start(GridPoint { line: 0, col: 2 }, SelectionKind::Line);
    assert_eq!(t.selection_text().as_deref(), Some("abcdefghijklmno"), "one line, no break inside or after");
}

#[test]
fn selection_of_wide_characters_is_whole() {
    let mut t = term(80, 24);
    t.feed("日本 x".as_bytes());
    t.select_start(GridPoint { line: 0, col: 1 }, SelectionKind::Simple);
    t.select_update(GridPoint { line: 0, col: 2 });
    assert_eq!(t.selection_text().as_deref(), Some("日本"));
}

#[test]
fn points_outside_the_grid_are_clamped() {
    let mut t = term(80, 24);
    t.feed(b"x");
    t.select_start(GridPoint { line: -100_000, col: 60_000 }, SelectionKind::Word);
    t.select_update(GridPoint { line: 100_000, col: 60_000 });
    let _ = t.selection_text();
    assert_eq!(t.point_at(500, 500), GridPoint { line: 23, col: 79 });
    assert_eq!(t.line_text(-100_000), "x");
    assert_eq!(t.line_text(100_000), "");
    t.remove_mark(GridPoint { line: 100_000, col: 9 });
}

#[test]
fn a_mark_stays_on_its_line_while_output_scrolls() {
    let mut t = term(80, 24);
    lines(&mut t, 0, 10);
    t.feed(b"here");
    t.add_mark();
    t.feed(b"\r\n");
    lines(&mut t, 10, 110);
    let marks = t.marks();
    assert_eq!(marks.len(), 1);
    assert_eq!(t.line_text(marks[0].line), "here");
}

#[test]
fn a_mark_stays_when_the_history_is_full() {
    let mut t = with_scrollback(50);
    lines(&mut t, 0, 200);
    t.feed(b"here");
    t.add_mark();
    t.feed(b"\r\n");
    lines(&mut t, 200, 230);
    let marks = t.marks();
    assert_eq!(marks.len(), 1);
    assert_eq!(t.line_text(marks[0].line), "here");
}

#[test]
fn a_mark_leaves_with_its_line() {
    let mut t = with_scrollback(50);
    t.feed(b"here");
    t.add_mark();
    t.feed(b"\r\n");
    lines(&mut t, 0, 200);
    assert!(t.marks().is_empty());
}

#[test]
fn jump_to_mark_scrolls_there() {
    let mut t = term(80, 24);
    t.feed(b"first");
    t.add_mark();
    t.feed(b"\r\n");
    lines(&mut t, 0, 100);
    assert!(t.jump_to_mark(Direction::Up));
    let s = t.screen();
    assert!(s.display_offset > 0);
    assert!((0..s.rows).any(|r| crate::row_text(&s, r) == "first"));
    let mark = t.marks()[0];
    t.remove_mark(mark);
    assert!(t.marks().is_empty());
}

use sshcraft_term::limits::MAX_SEARCH_PATTERN;
use sshcraft_term::{Direction, SearchError, SearchQuery, Terminal};

use crate::term;

fn query(pattern: &str, regex: bool) -> SearchQuery {
    SearchQuery { pattern: pattern.into(), regex, case_sensitive: false }
}

fn filled() -> Terminal {
    let mut t = term(80, 24);
    for i in 0..200 {
        t.feed(format!("entry {i}\r\n").as_bytes());
    }
    t
}

#[test]
fn plain_search_finds_the_text_in_the_history() {
    let mut t = filled();
    t.feed(b"needle 1\r\n");
    let mut t2 = term(80, 24);
    t2.feed(b"needle\r\n");
    let found = t.search(&query("entry 5", false), Direction::Up).unwrap().unwrap();
    assert!(found.start.line < 0, "{found:?}");
    assert_eq!(t.selection_text().as_deref(), Some("entry 5"));
    assert!(t.screen().display_offset > 0);
}

#[test]
fn plain_search_treats_regex_characters_literally() {
    let mut t = term(80, 24);
    t.feed(b"abc");
    assert_eq!(t.search(&query("a.c", false), Direction::Down).unwrap(), None);
    t.feed(b" a.c");
    assert!(t.search(&query("a.c", false), Direction::Down).unwrap().is_some());
}

#[test]
fn regex_search_finds_a_pattern() {
    let mut t = term(80, 24);
    t.feed(b"ok\r\nerror   42\r\n");
    t.search(&query(r"err(or)?\s+\d+", true), Direction::Down).unwrap().unwrap();
    assert_eq!(t.selection_text().as_deref(), Some("error   42"));
}

#[test]
fn search_is_case_insensitive_by_default() {
    let mut t = term(80, 24);
    t.feed(b"Hello");
    assert!(t.search(&query("hello", false), Direction::Down).unwrap().is_some());
    let sensitive = SearchQuery { case_sensitive: true, ..query("hello", false) };
    assert_eq!(t.search(&sensitive, Direction::Down).unwrap(), None);
    let upper = SearchQuery { case_sensitive: false, ..query("HELLO", false) };
    assert!(t.search(&upper, Direction::Down).unwrap().is_some());
}

#[test]
fn search_wraps_around() {
    let mut t = term(80, 24);
    t.feed(b"one\r\ntwo\r\none");
    let first = t.search(&query("one", false), Direction::Down).unwrap().unwrap();
    let second = t.search(&query("one", false), Direction::Down).unwrap().unwrap();
    let third = t.search(&query("one", false), Direction::Down).unwrap().unwrap();
    assert_ne!(first, second);
    assert_eq!(first, third);
}

#[test]
fn an_overlong_pattern_is_refused() {
    let mut t = term(80, 24);
    let long = "a".repeat(MAX_SEARCH_PATTERN + 1);
    assert_eq!(t.search(&query(&long, false), Direction::Down), Err(SearchError::TooLong));
}

#[test]
fn an_invalid_regex_is_an_error_not_a_panic() {
    let mut t = term(80, 24);
    assert!(matches!(t.search(&query("(", true), Direction::Down), Err(SearchError::Invalid(_))));
}

#[test]
fn a_pathological_regex_finishes() {
    let mut t = term(200, 50);
    for _ in 0..10_000 {
        t.feed(&[b'a'; 120]);
        t.feed(b"\r\n");
    }
    assert_eq!(t.search(&query("(a*)*b", true), Direction::Up).unwrap(), None);
}

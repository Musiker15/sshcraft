//! Folding the output of a command (shell integration): the hidden lines become one row, the rows
//! around it, the cursor, clicks, scrolling, search and jumps follow.

use sshcraft_term::{Direction, FoldRow, SearchQuery, SelectionKind};

use crate::{row_text, term};

const A: &[u8] = b"\x1b]133;A\x07";
const B: &[u8] = b"\x1b]133;B\x07";
const C: &[u8] = b"\x1b]133;C\x07";

fn end(code: &str) -> Vec<u8> {
    format!("\x1b]133;D;{code}\x07").into_bytes()
}

/// A finished command `cmd` whose output is `lines` lines `<cmd> <n>`.
fn command(cmd: &str, lines: usize) -> Vec<u8> {
    let output: String = (1..=lines).map(|n| format!("{cmd} {n}\r\n")).collect();
    [A, b"$ ", B, cmd.as_bytes(), b"\r\n", C, output.as_bytes(), &end("0")].concat()
}

fn prompt() -> Vec<u8> {
    [A, b"$ ", B].concat()
}

/// The id of the command `back` places before the newest one (0: the newest).
fn id_from_end(t: &sshcraft_term::Terminal, back: usize) -> u64 {
    let history = t.command_history();
    history[history.len() - 1 - back].id()
}

fn texts(t: &sshcraft_term::Terminal) -> Vec<String> {
    let screen = t.screen();
    (0..screen.rows).map(|r| row_text(&screen, r)).collect()
}

#[test]
fn folding_leaves_one_row_for_the_output() {
    let mut t = term(40, 10);
    t.feed(&[command("ls", 2), prompt()].concat());
    let id = t.screen().commands[0].record.id();
    assert!(t.toggle_fold(id));
    let screen = t.screen();
    assert_eq!(screen.folds, vec![FoldRow { row: 1, id, lines: 2 }]);
    assert_eq!(row_text(&screen, 0), "$ ls");
    assert_eq!(row_text(&screen, 1), "", "the placeholder row has no text of its own");
    assert_eq!(row_text(&screen, 2), "$");
    assert!(screen.commands[0].folded);
    assert_eq!(screen.commands[1].row, 2);
    assert_eq!((screen.cursor.row, screen.cursor.visible), (2, true));
}

#[test]
fn unfolding_shows_the_same_screen_as_before() {
    let mut t = term(40, 10);
    t.feed(&[command("ls", 2), prompt()].concat());
    let before = t.screen();
    let id = before.commands[0].record.id();
    assert!(t.toggle_fold(id));
    assert!(!t.toggle_fold(id));
    assert_eq!(t.screen(), before);
}

#[test]
fn only_a_finished_command_with_output_folds() {
    let mut t = term(40, 10);
    t.feed(&[A, b"$ ", B, b"true\r\n", C, &end("0"), A, b"$ ", B, b"sleep\r\n", C, b"tick\r\n"].concat());
    let rows = t.screen().commands;
    assert!(!rows[0].foldable, "no output: {rows:?}");
    assert!(!rows[1].foldable, "still running: {rows:?}");
    assert!(!t.toggle_fold(rows[0].record.id()));
    assert!(!t.toggle_fold(rows[1].record.id()));
    assert!(!t.toggle_fold(999_999), "an id no command has");
    assert!(t.screen().folds.is_empty());
}

#[test]
fn a_fold_pulls_older_lines_into_view() {
    let mut t = term(40, 5);
    t.feed(&[command("x", 1), command("y", 6), prompt()].concat());
    assert_eq!(texts(&t)[0], "y 3");
    // The prompt of `y` is above the view; its record is the one before the newest.
    let id = id_from_end(&t, 1);
    assert!(t.toggle_fold(id));
    assert_eq!(texts(&t), ["$ x", "x 1", "$ y", "", "$"]);
    assert_eq!(t.screen().folds, vec![FoldRow { row: 3, id, lines: 6 }]);
}

#[test]
fn clicks_map_to_the_lines_around_a_fold() {
    let mut t = term(40, 10);
    t.feed(&[command("ls", 2), prompt()].concat());
    let id = t.screen().commands[0].record.id();
    t.toggle_fold(id);
    assert_eq!(t.line_text(t.point_at(0, 2).line), "$");
    assert_eq!(t.line_text(t.point_at(0, 1).line), "ls 1", "the placeholder stands for its first line");
}

#[test]
fn copying_across_a_fold_includes_the_hidden_lines() {
    let mut t = term(40, 10);
    t.feed(&[command("ls", 2), prompt()].concat());
    t.toggle_fold(t.screen().commands[0].record.id());
    t.select_start(t.point_at(0, 0), SelectionKind::Simple);
    t.select_update(t.point_at(1, 2));
    assert_eq!(t.selection_text().as_deref(), Some("$ ls\nls 1\nls 2\n$"));
    let selection = t.screen().selection.unwrap();
    assert_eq!((selection.start, selection.end), ((0, 0), (1, 2)));
}

#[test]
fn scrolling_steps_over_a_fold_one_row_at_a_time() {
    let mut t = term(40, 5);
    let older: Vec<u8> = (0..4).flat_map(|i| command(&format!("old{i}"), 2)).collect();
    t.feed(&[older, command("big", 40), prompt()].concat());
    // The prompt of `big` is far above the view; fold it through its record.
    assert!(t.toggle_fold(id_from_end(&t, 1)));
    let mut seen = vec![texts(&t)];
    for _ in 0..30 {
        t.scroll(1);
        let now = texts(&t);
        if now == *seen.last().unwrap() {
            break;
        }
        seen.push(now);
    }
    // Twelve lines of older commands, the prompt of `big`, its fold and the new prompt are 15
    // rows: a view of 5 moves 10 times, one row each.
    assert_eq!(seen.len(), 11, "{seen:#?}");
    assert_eq!(seen.last().unwrap()[0], "$ old0");
    // Back down one row at a time shows the same views in reverse.
    for wanted in seen.iter().rev().skip(1) {
        t.scroll(-1);
        assert_eq!(texts(&t), *wanted);
    }
    t.scroll(-1);
    assert_eq!(texts(&t), seen[0], "the bottom stays the bottom");
}

#[test]
fn scrolling_stops_where_a_fold_at_the_top_ends_the_history() {
    let mut t = term(40, 5);
    let newer: Vec<u8> = (0..4).flat_map(|i| command(&format!("new{i}"), 2)).collect();
    t.feed(&[command("first", 40), newer, prompt()].concat());
    assert!(t.toggle_fold(t.command_history()[0].id()));
    let mut steps = 0;
    let mut last = texts(&t);
    while steps < 50 {
        t.scroll(1);
        let now = texts(&t);
        if now == last {
            break;
        }
        last = now;
        steps += 1;
    }
    // The prompt of `first`, its fold, twelve lines of newer commands and the new prompt are 15
    // rows: the view moves 10 times and then shows the first five.
    assert_eq!(steps, 10);
    assert_eq!(last, ["$ first", "", "$ new0", "new0 1", "new0 2"]);
    // Scrolling past the top went nowhere, so one row down moves at once.
    t.scroll(-1);
    assert_eq!(texts(&t)[0], "");
}

#[test]
fn search_unfolds_a_match_in_folded_output() {
    let mut t = term(40, 10);
    t.feed(&[command("ls", 2), prompt()].concat());
    let id = t.screen().commands[0].record.id();
    t.toggle_fold(id);
    let query = SearchQuery { pattern: "ls 2".into(), regex: false, case_sensitive: true };
    assert!(t.search(&query, Direction::Up).unwrap().is_some());
    assert!(t.screen().folds.is_empty());
    assert!(!t.screen().commands[0].folded);
}

#[test]
fn jumping_to_a_prompt_puts_it_at_the_top_past_folds() {
    let mut t = term(40, 5);
    for i in 0..6 {
        t.feed(&command(&format!("c{i}"), 3));
    }
    t.feed(&prompt());
    t.fold_all();
    assert!(t.jump_to_prompt(Direction::Up));
    let screen = t.screen();
    assert_eq!(screen.commands.first().map(|c| c.row), Some(0), "{:?}", texts(&t));
}

#[test]
fn a_fold_stays_while_new_output_arrives() {
    let mut t = term(40, 10);
    t.feed(&[command("ls", 2), prompt()].concat());
    let id = t.screen().commands[0].record.id();
    t.toggle_fold(id);
    t.feed(&[b"pwd\r\n".as_slice(), C, b"/home\r\n", &end("0"), &prompt()].concat());
    let screen = t.screen();
    assert_eq!(screen.folds, vec![FoldRow { row: 1, id, lines: 2 }]);
    assert_eq!(row_text(&screen, 3), "/home");
}

#[test]
fn the_last_command_folds_and_everything_unfolds() {
    let mut t = term(40, 12);
    t.feed(&[command("a", 2), command("b", 2), prompt()].concat());
    assert!(t.fold_last());
    let folds = t.screen().folds;
    assert_eq!(folds.len(), 1);
    assert_eq!(texts(&t)[3], "$ b");
    assert!(!t.fold_last(), "the same command shows again");
    t.fold_all();
    assert_eq!(t.screen().folds.len(), 2);
    t.unfold_all();
    assert!(t.screen().folds.is_empty());
}

#[test]
fn a_full_screen_program_is_never_folded() {
    let mut t = term(40, 10);
    t.feed(&[command("ls", 2), prompt()].concat());
    t.fold_all();
    t.feed(b"\x1b[?1049h\x1b[Hfull");
    let screen = t.screen();
    assert!(screen.folds.is_empty());
    assert_eq!(row_text(&screen, 0), "full");
}

//! Shell integration (OSC 133): prompt lines, command records, exit codes and output.

use sshcraft_term::{CommandState, Direction, Notice};

use crate::term;

const A: &[u8] = b"\x1b]133;A\x07";
const B: &[u8] = b"\x1b]133;B\x07";
const C: &[u8] = b"\x1b]133;C\x07";

fn end(code: &str) -> Vec<u8> {
    format!("\x1b]133;D;{code}\x07").into_bytes()
}

/// One prompt, one command with two output lines that ends with `code`, then a new prompt.
fn session(code: &str) -> Vec<u8> {
    [A, b"$ ", B, b"ls\r\n", C, b"out1\r\nout2\r\n", &end(code), A, b"$ ", B].concat()
}

#[test]
fn prompt_lines_carry_their_command_and_exit_code() {
    let mut t = term(40, 10);
    t.feed(&session("2"));
    let commands = t.screen().commands;
    assert_eq!(commands.len(), 2, "{commands:?}");
    assert_eq!(commands[0].row, 0);
    assert_eq!(commands[0].record.state, CommandState::Finished);
    assert_eq!(commands[0].record.exit, Some(2));
    assert_eq!(commands[1].row, 3);
    assert_eq!(commands[1].record.state, CommandState::Prompt);
}

#[test]
fn marks_split_across_feeds_are_the_same() {
    let mut whole = term(40, 10);
    whole.feed(&session("0"));
    let mut bytewise = term(40, 10);
    for byte in session("0") {
        bytewise.feed(&[byte]);
    }
    assert_eq!(bytewise.screen().commands, whole.screen().commands);
}

#[test]
fn the_newest_rows_keep_their_records_after_a_clear() {
    let mut t = term(40, 10);
    t.feed(&session("1"));
    t.feed(b"\x1b[H\x1b[2J");
    t.feed(&[A, b"$ ", B, b"pwd\r\n", C, b"/root\r\n", &end("0"), A, b"$ ", B].concat());
    let commands = t.screen().commands;
    let newest = commands.last().expect("a prompt row");
    assert_eq!(newest.record.state, CommandState::Prompt);
    let finished = &commands[commands.len() - 2];
    assert_eq!(finished.record.exit, Some(0));
}

#[test]
fn records_are_bounded() {
    let mut t = term(40, 10);
    let one = [A, b"$ ", B, b"x\r\n", C, &end("0")].concat();
    for _ in 0..5000 {
        t.feed(&one);
    }
    assert!(t.command_history().len() <= 1000, "{}", t.command_history().len());
}

#[test]
fn an_exit_code_that_is_no_number_is_unknown() {
    let mut t = term(40, 10);
    t.feed(&[A, b"$ ", B, b"x\r\n", C, &end("abc")].concat());
    let last = t.command_history().last().cloned().expect("a record");
    assert_eq!(last.state, CommandState::Finished);
    assert_eq!(last.exit, None);
}

#[test]
fn the_output_of_the_last_finished_command_is_its_lines_only() {
    let mut t = term(40, 10);
    t.feed(&session("0"));
    assert_eq!(t.last_finished_output().as_deref(), Some("out1\nout2"));
}

#[test]
fn a_wrapped_output_line_is_one_line() {
    let mut t = term(10, 10);
    t.feed(&[A, b"$ ", B, b"x\r\n", C, b"0123456789abcdef\r\n", &end("0"), A, b"$ ", B].concat());
    assert_eq!(t.last_finished_output().as_deref(), Some("0123456789abcdef"));
}

#[test]
fn starting_and_finishing_a_command_are_reported() {
    let mut t = term(40, 10);
    t.feed(&session("3"));
    let notices = t.take_notices();
    assert!(notices.contains(&Notice::CommandStarted), "{notices:?}");
    assert!(notices.contains(&Notice::CommandFinished { exit: Some(3) }), "{notices:?}");
}

#[test]
fn stamping_fills_times_once() {
    let mut t = term(40, 10);
    t.feed(&[A, b"$ ", B, b"sleep 1\r\n", C].concat());
    let first = chrono::DateTime::from_timestamp(1_000, 0).expect("time");
    t.stamp_commands(first);
    t.feed(&end("0"));
    let second = chrono::DateTime::from_timestamp(1_005, 0).expect("time");
    t.stamp_commands(second);
    t.stamp_commands(chrono::DateTime::from_timestamp(2_000, 0).expect("time"));
    let last = t.command_history().last().cloned().expect("a record");
    assert_eq!(last.started, Some(first));
    assert_eq!(last.finished, Some(second));
}

#[test]
fn jumping_between_prompts_scrolls_the_view() {
    let mut t = term(40, 5);
    for _ in 0..6 {
        t.feed(&[A, b"$ ", B, b"x\r\n", C, b"1\r\n2\r\n3\r\n", &end("0")].concat());
    }
    t.feed(&[A, b"$ ", B].concat());
    assert!(t.jump_to_prompt(Direction::Up));
    assert!(t.screen().display_offset > 0);
    assert!(t.jump_to_prompt(Direction::Down));
}

#[test]
fn a_two_line_prompt_keeps_rows_and_output_aligned() {
    let mut t = term(40, 12);
    let prompt = [A, b"user@host ~\r\n$ ", B].concat();
    t.feed(&[prompt.as_slice(), b"false\r\n", C, b"err\r\n", &end("1"), &prompt].concat());
    let commands = t.screen().commands;
    assert_eq!(commands.len(), 2, "one row per command: {commands:?}");
    assert_eq!(commands[0].record.exit, Some(1));
    assert_eq!(commands[1].record.state, CommandState::Prompt);
    assert_eq!(t.last_finished_output().as_deref(), Some("err"));
}

#[test]
fn an_empty_enter_without_end_mark_keeps_rows_aligned() {
    let mut t = term(40, 12);
    t.feed(&[A, b"$ \r\n", A, b"$ ls\r\n", C, b"out\r\n", &end("0"), A, b"$ "].concat());
    let commands = t.screen().commands;
    assert_eq!(commands.len(), 2, "{commands:?}");
    assert_eq!(commands[0].record.exit, Some(0));
    assert_eq!(t.last_finished_output().as_deref(), Some("out"));
}

#[test]
fn an_end_mark_without_a_command_finishes_nothing() {
    let mut t = term(40, 12);
    t.feed(&[A, b"$ ", B, b"\r\n", &end("0"), A, b"$ ", B].concat());
    assert!(t.command_history().iter().all(|r| r.state == CommandState::Prompt), "{:?}", t.command_history());
}

#[test]
fn marks_inside_a_full_screen_program_are_left_out() {
    let mut t = term(40, 12);
    t.feed(&[b"\x1b[?1049h".as_slice(), A, b"$ ", B, b"x\r\n", C, &end("0"), b"\x1b[?1049l"].concat());
    assert!(t.command_history().is_empty());
}

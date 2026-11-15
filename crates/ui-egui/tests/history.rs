//! Which typed lines become history: only lines typed at a shell prompt, decided at their first
//! key; never secrets, REPLs, full-screen programs, edited lines, multi-line pastes or broadcast.

use sshcraft_ui_egui::history::{Capture, LineContext};

fn at(prompt: &str) -> LineContext {
    LineContext { prompt: prompt.into(), alt_screen: false, broadcast: false }
}

/// Feeds `input` key by key at `ctx`; the lines it keeps.
fn kept(ctx: &LineContext, input: &[u8]) -> Vec<String> {
    let mut capture = Capture::default();
    input.iter().filter_map(|b| capture.input(&[*b], ctx)).collect()
}

#[test]
fn a_line_at_a_shell_prompt_is_kept() {
    assert_eq!(kept(&at("me@box:~$ "), b"git status\r"), ["git status"]);
    assert_eq!(kept(&at("PS C:\\> "), b"dir\r"), ["dir"]);
    assert_eq!(kept(&at("$ "), b"lss\x7f -la\r"), ["ls -la"], "backspace is no edit");
    assert_eq!(kept(&at("$ "), b"\r"), Vec::<String>::new(), "an empty line is nothing");
}

#[test]
fn input_after_a_password_question_is_never_kept() {
    for prompt in [
        "Password: ",
        "[sudo] password for me: ",
        "Enter passphrase for key '/k': ",
        "PIN: ",
        "Token: ",
        "Verification code: ",
        "Passwort: ",
        "Mot de passe : ",
        "Contraseña: ",
        "Senha: ",
    ] {
        assert!(kept(&at(prompt), b"hunter2\r").is_empty(), "{prompt}");
    }
}

#[test]
fn lines_starting_with_a_space_are_not_kept() {
    assert!(kept(&at("$ "), b" export TOKEN=abc\r").is_empty());
}

#[test]
fn repl_and_fullscreen_input_is_not_kept() {
    for prompt in ["mysql> ", "postgres=# ", "sqlite> ", ">>> ", "irb(main):001:0> ", "> ", "... "] {
        assert!(kept(&at(prompt), b"select 1;\r").is_empty(), "{prompt}");
    }
    let full = LineContext { alt_screen: true, ..at("$ ") };
    assert!(kept(&full, b"dd\r").is_empty(), "vim and other full-screen programs");
}

#[test]
fn edited_lines_are_not_kept() {
    assert!(kept(&at("$ "), b"git sta\tus\r").is_empty(), "Tab completion");
    assert!(kept(&at("$ "), b"git\x1b[A\r").is_empty(), "an arrow");
    assert!(kept(&at("$ "), b"\x12ssh\r").is_empty(), "Ctrl+R");
    assert_eq!(kept(&at("$ "), b"abc\x03ls\r"), ["ls"], "Ctrl+C starts the line again");
}

#[test]
fn multi_line_pastes_and_broadcast_are_not_kept() {
    let mut capture = Capture::default();
    let ctx = at("$ ");
    assert_eq!(capture.input(b"\x1b[200~echo a\necho b\x1b[201~\r", &ctx), None, "a multi-line paste");
    assert_eq!(capture.input(b"\x1b[200~uptime\x1b[201~\r", &ctx).as_deref(), Some("uptime"), "a one-line paste counts");
    let broadcast = LineContext { broadcast: true, ..at("$ ") };
    assert!(kept(&broadcast, b"reboot\r").is_empty());
}

#[test]
fn the_line_so_far_is_known_at_a_prompt_only() {
    let mut capture = Capture::default();
    capture.input(b"gi", &at("$ "));
    assert_eq!(capture.line(), Some("gi"));
    capture.input(b"\t", &at("$ "));
    assert_eq!(capture.line(), None, "after Tab the shell changed the line");
    let mut secret = Capture::default();
    secret.input(b"hu", &at("Password: "));
    assert_eq!(secret.line(), None, "no suggestions for a secret");
}

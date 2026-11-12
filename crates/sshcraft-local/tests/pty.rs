use std::time::Duration;

use sshcraft_core::policy::{Disconnect, ShellEvent};
use sshcraft_local::pty::{ShellCommand, available_shells, spawn};

fn echo_command() -> ShellCommand {
    if cfg!(windows) {
        ShellCommand {
            program: "cmd.exe".into(),
            args: vec!["/c".into(), "echo sshcraft-local".into()],
            label: "echo".into(),
        }
    } else {
        ShellCommand {
            program: "sh".into(),
            args: vec!["-c".into(), "echo sshcraft-local".into()],
            label: "echo".into(),
        }
    }
}

#[tokio::test]
async fn spawned_shell_echoes() {
    let (writer, mut events) = spawn(&echo_command(), 80, 24).unwrap();
    let mut output = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let end = loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Some(ShellEvent::Data(d))) => {
                // ConPTY asks where the cursor is and waits; a terminal (xterm.js) answers.
                if d.windows(4).any(|w| w == b"\x1b[6n") {
                    writer.write(b"\x1b[1;1R").unwrap();
                }
                output.extend(d);
            }
            Ok(Some(ShellEvent::Ended(reason))) => break reason,
            other => panic!(
                "no end: {other:?}, got {:?}",
                String::from_utf8_lossy(&output)
            ),
        }
    };
    let text = String::from_utf8_lossy(&output);
    assert!(text.contains("sshcraft-local"), "{text:?}");
    assert_eq!(end, Disconnect::Exited(Some(0)));
}

#[tokio::test]
async fn resize_does_not_fail() {
    let interactive = if cfg!(windows) {
        ShellCommand {
            program: "cmd.exe".into(),
            args: Vec::new(),
            label: "cmd".into(),
        }
    } else {
        ShellCommand {
            program: "sh".into(),
            args: Vec::new(),
            label: "sh".into(),
        }
    };
    let (writer, _events) = spawn(&interactive, 80, 24).unwrap();
    writer.resize(120, 40).unwrap();
    writer.write(b"exit\r").unwrap();
}

#[test]
fn unknown_program_fails_to_spawn() {
    let missing = ShellCommand {
        program: "sshcraft-no-such-shell-xyz".into(),
        args: Vec::new(),
        label: "missing".into(),
    };
    assert!(spawn(&missing, 80, 24).is_err());
}

#[test]
fn available_shells_ends_with_custom() {
    let custom = ShellCommand {
        program: "my-shell".into(),
        args: vec!["--login".into()],
        label: "Mine".into(),
    };
    let shells = available_shells(Some(custom.clone()));
    assert!(shells.len() >= 2);
    assert_eq!(shells.last(), Some(&custom));
    if cfg!(windows) {
        assert!(
            shells
                .iter()
                .any(|s| s.program.eq_ignore_ascii_case("cmd.exe"))
        );
        assert!(
            shells
                .iter()
                .any(|s| s.program.eq_ignore_ascii_case("powershell.exe"))
        );
    }
}

#[tokio::test]
async fn close_ends_the_shell_even_while_a_clone_lives() {
    let interactive = if cfg!(windows) {
        ShellCommand {
            program: "cmd.exe".into(),
            args: Vec::new(),
            label: "cmd".into(),
        }
    } else {
        ShellCommand {
            program: "sh".into(),
            args: Vec::new(),
            label: "sh".into(),
        }
    };
    let (writer, mut events) = spawn(&interactive, 80, 24).unwrap();
    let _held = writer.clone();
    writer.close();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Some(ShellEvent::Data(d))) => {
                if d.windows(4).any(|w| w == b"\x1b[6n") {
                    let _ = writer.write(b"\x1b[1;1R");
                }
            }
            Ok(Some(ShellEvent::Ended(_))) | Ok(None) => break,
            Err(_) => panic!("the shell is still running"),
        }
    }
}

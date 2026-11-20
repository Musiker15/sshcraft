use super::*;
use std::path::Path;

fn have_sh() -> bool {
    std::process::Command::new("sh").args(["-c", "true"]).status().is_ok_and(|s| s.success())
}

/// Runs `command` in a real `sh` with `HOME` in `home` and `SHELL` set to `shell`.
fn run(home: &Path, shell: &str, command: &str) -> Vec<u8> {
    let home = home.to_string_lossy().replace('\\', "/");
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("HOME", home)
        .env("SHELL", shell)
        .env_remove("ZDOTDIR")
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    output.stdout
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn commands_carry_no_quote_of_their_own_and_no_plain_block() {
    for command in [install_command(), remove_command()] {
        assert!(command.starts_with("sh -c '"));
        assert_eq!(command.matches('\'').count(), 2, "only the outer quotes: {command}");
    }
    assert!(!install_command().contains("__sshcraft"), "the block travels encoded");
}

#[test]
fn parse_outcome_reads_marks_and_rejects_others() {
    assert_eq!(parse_outcome(b"motd\nSSHCRAFT_SHELL=zsh\nSSHCRAFT_ADDED\n"), Ok(Outcome::Added { shell: Shell::Zsh }));
    assert_eq!(parse_outcome(b"SSHCRAFT_SHELL=fish\r\nSSHCRAFT_PRESENT\r\n"), Ok(Outcome::AlreadyPresent { shell: Shell::Fish }));
    assert_eq!(parse_outcome(b"SSHCRAFT_SHELL_OTHER\n"), Ok(Outcome::OtherShell));
    assert_eq!(parse_outcome(b"SSHCRAFT_REMOVED\n"), Ok(Outcome::Removed));
    assert_eq!(parse_outcome(b"SSHCRAFT_ABSENT\n"), Ok(Outcome::Absent));
    assert_eq!(parse_outcome(b"SSHCRAFT_SHELL=bash\nSSHCRAFT_FAILED\n"), Err(ShellIntegrationError::Failed));
    // A server that prints a mark of its own does not name a shell SSHCraft did not detect.
    for output in [&b""[..], b"'sh' is not recognized", b"SSHCRAFT_ADDED\n", b"SSHCRAFT_SHELL=tcsh\nSSHCRAFT_ADDED\n"] {
        assert_eq!(parse_outcome(output), Err(ShellIntegrationError::Unsupported), "{:?}", String::from_utf8_lossy(output));
    }
}

#[test]
fn install_appends_the_block_once_and_remove_restores_the_file() {
    if !have_sh() {
        eprintln!("no sh on this machine: skipped");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let rc = home.path().join(".bashrc");
    let before = "alias ll='ls -l'\nexport EDITOR=vi";
    std::fs::write(&rc, before).unwrap();

    assert_eq!(parse_outcome(&run(home.path(), "/bin/bash", &install_command())), Ok(Outcome::Added { shell: Shell::Bash }));
    let text = read(&rc);
    assert!(text.starts_with(&format!("{before}\n{BEGIN}\n")), "{text}");
    assert!(text.ends_with(&format!("{END}\n")), "{text}");
    assert!(text.contains(BASH));

    assert_eq!(parse_outcome(&run(home.path(), "/bin/bash", &install_command())), Ok(Outcome::AlreadyPresent { shell: Shell::Bash }));
    assert_eq!(read(&rc).matches(BEGIN).count(), 1);

    std::fs::write(&rc, format!("{}after the block\n", read(&rc))).unwrap();
    assert_eq!(parse_outcome(&run(home.path(), "/bin/bash", &remove_command())), Ok(Outcome::Removed));
    assert_eq!(read(&rc), format!("{before}\nafter the block\n"));
    assert!(!home.path().join(".bashrc.sshcraft-part").exists());

    assert_eq!(parse_outcome(&run(home.path(), "/bin/bash", &remove_command())), Ok(Outcome::Absent));
}

#[test]
fn zsh_and_fish_get_their_own_start_files() {
    if !have_sh() {
        eprintln!("no sh on this machine: skipped");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    assert_eq!(parse_outcome(&run(home.path(), "/usr/bin/zsh", &install_command())), Ok(Outcome::Added { shell: Shell::Zsh }));
    assert!(read(&home.path().join(".zshrc")).contains(ZSH));
    assert_eq!(parse_outcome(&run(home.path(), "/usr/local/bin/fish", &install_command())), Ok(Outcome::Added { shell: Shell::Fish }));
    let fish = home.path().join(".config").join("fish").join("config.fish");
    assert!(read(&fish).contains(FISH));

    // One removal takes the block out of both.
    assert_eq!(parse_outcome(&run(home.path(), "/bin/sh", &remove_command())), Ok(Outcome::Removed));
    assert!(!read(&home.path().join(".zshrc")).contains(BEGIN));
    assert!(!read(&fish).contains(BEGIN));
}

#[test]
fn another_shell_changes_nothing() {
    if !have_sh() {
        eprintln!("no sh on this machine: skipped");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    assert_eq!(parse_outcome(&run(home.path(), "/bin/tcsh", &install_command())), Ok(Outcome::OtherShell));
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}

#[test]
fn a_block_without_its_last_line_is_left_alone() {
    if !have_sh() {
        eprintln!("no sh on this machine: skipped");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let rc = home.path().join(".bashrc");
    let text = format!("before\n{BEGIN}\nsomething the user wrote\nmore of it\n");
    std::fs::write(&rc, &text).unwrap();
    assert_eq!(parse_outcome(&run(home.path(), "/bin/bash", &remove_command())), Err(ShellIntegrationError::Failed));
    assert_eq!(read(&rc), text);
}

/// `shell -n` on the block of `kind`, where that shell is installed.
fn parses_in(shell: &str, kind: Shell) {
    // On Windows `bash` may be WSL's launcher without a distribution.
    if !std::process::Command::new(shell).args(["-c", "true"]).output().is_ok_and(|o| o.status.success()) {
        eprintln!("no {shell} on this machine: skipped");
        return;
    }
    let output = std::process::Command::new(shell).arg("-n").arg("-c").arg(kind.block()).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn the_bash_block_is_valid_bash() {
    parses_in("bash", Shell::Bash);
}

#[test]
fn the_zsh_block_is_valid_zsh() {
    parses_in("zsh", Shell::Zsh);
}

#[test]
fn the_fish_block_is_valid_fish() {
    parses_in("fish", Shell::Fish);
}

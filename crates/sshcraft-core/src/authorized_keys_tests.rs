use super::*;
use russh::keys::{Algorithm, PrivateKey};

fn public_with_comment(comment: &str) -> PublicKey {
    let mut key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .unwrap()
        .public_key()
        .clone();
    key.set_comment(comment);
    key
}

/// `type base64` of a key, without comment.
fn body(key: &PublicKey) -> String {
    let mut bare = key.clone();
    bare.set_comment("");
    bare.to_openssh().unwrap().trim().to_owned()
}

#[test]
fn comment_is_sanitized() {
    let key = public_with_comment("a'b;$(rm)");
    let command = install_command(&key, false);
    assert!(command.contains(&format!("{} abrm", body(&key))));
    assert!(!command.contains("$(rm)"));
    assert!(command.starts_with("sh -c '"));
    assert_eq!(command.matches('\'').count(), 2, "only the outer quotes");
}

#[test]
fn long_comment_is_cut_to_64() {
    let key = public_with_comment(&"x".repeat(200));
    let command = install_command(&key, false);
    assert!(command.contains(&format!("{} {}\"", body(&key), "x".repeat(64))));
}

#[test]
fn parse_outcome_reads_markers_and_rejects_others() {
    assert_eq!(
        parse_outcome(b"motd line\nSSHCRAFT_ADDED\n").unwrap(),
        InstallOutcome::Added
    );
    assert_eq!(
        parse_outcome(b"SSHCRAFT_PRESENT\r\n").unwrap(),
        InstallOutcome::AlreadyPresent
    );
    for output in [&b""[..], b"'sh' is not recognized", b"SSHCRAFT_ADDEDX\n"] {
        assert!(matches!(
            parse_outcome(output),
            Err(AuthorizedKeysError::Unsupported)
        ));
    }
}

/// Runs the install command in a real `sh` with `HOME` in a temporary folder; `None` when this
/// machine has no `sh`.
fn run_in_sh(home: &std::path::Path, command: &str) -> Option<Vec<u8>> {
    let home = home.to_string_lossy().replace('\\', "/");
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("HOME", home)
        .output()
        .ok()?;
    assert!(output.status.success(), "{output:?}");
    Some(output.stdout)
}

#[test]
fn script_runs_in_sh() {
    if std::process::Command::new("sh")
        .args(["-c", "true"])
        .status()
        .is_err()
    {
        eprintln!("no sh on this machine: script_runs_in_sh skipped");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join(".ssh").join("authorized_keys");
    let key = public_with_comment("deploy@laptop");
    let command = install_command(&key, false);

    let first = run_in_sh(home.path(), &command).unwrap();
    assert_eq!(parse_outcome(&first).unwrap(), InstallOutcome::Added);
    let second = run_in_sh(home.path(), &command).unwrap();
    assert_eq!(
        parse_outcome(&second).unwrap(),
        InstallOutcome::AlreadyPresent
    );
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text, format!("{} deploy@laptop\n", body(&key)));

    // A file without a newline at its end gets the key on a line of its own.
    let other = public_with_comment("other");
    std::fs::write(&file, "ssh-ed25519 AAAAexisting old").unwrap();
    run_in_sh(home.path(), &install_command(&other, false)).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        text,
        format!("ssh-ed25519 AAAAexisting old\n{} other\n", body(&other))
    );

    // A line with options in front counts as present.
    std::fs::write(&file, format!("no-pty {} alt\n", body(&key))).unwrap();
    let again = run_in_sh(home.path(), &command).unwrap();
    assert_eq!(
        parse_outcome(&again).unwrap(),
        InstallOutcome::AlreadyPresent
    );

    // `verify-required` goes in front.
    std::fs::remove_file(&file).unwrap();
    run_in_sh(home.path(), &install_command(&key, true)).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with(&format!("verify-required {}", body(&key))));
}

#[test]
fn failure_marker_is_its_own_outcome() {
    assert!(matches!(
        parse_outcome(b"SSHCRAFT_FAILED\n"),
        Err(AuthorizedKeysError::Failed)
    ));
}

#[test]
fn script_reports_failure_and_ignores_commented_keys() {
    if std::process::Command::new("sh")
        .args(["-c", "true"])
        .status()
        .is_err()
    {
        eprintln!("no sh on this machine: skipped");
        return;
    }
    let key = public_with_comment("deploy");
    let command = install_command(&key, false);

    // A commented-out line does not count as present.
    let home = tempfile::tempdir().unwrap();
    let ssh = home.path().join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(
        ssh.join("authorized_keys"),
        format!("# {} old\n", body(&key)),
    )
    .unwrap();
    let out = run_in_sh(home.path(), &command).unwrap();
    assert_eq!(parse_outcome(&out).unwrap(), InstallOutcome::Added);

    // `.ssh` is a file: mkdir fails, and the script says so.
    let broken = tempfile::tempdir().unwrap();
    std::fs::write(broken.path().join(".ssh"), "not a folder").unwrap();
    let home = broken.path().to_string_lossy().replace('\\', "/");
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .env("HOME", home)
        .output()
        .unwrap();
    assert!(matches!(
        parse_outcome(&output.stdout),
        Err(AuthorizedKeysError::Failed)
    ));
}

//! Command ids are an interface (the interface, the CLI, MCP and saved shortcuts name them): once
//! shipped, an id is never renamed or removed. Adding a command means adding its id to
//! `command_ids.txt` in the same change.

use sshcraft_engine::cmd::command_specs;

const KNOWN: &str = include_str!("command_ids.txt");

#[test]
fn command_ids_are_stable() {
    let mut ids: Vec<&str> = command_specs().iter().map(|s| s.id).collect();
    ids.sort_unstable();
    // `SSHCRAFT_WRITE_COMMAND_IDS=1` writes the list after a command was added on purpose.
    if std::env::var_os("SSHCRAFT_WRITE_COMMAND_IDS").is_some() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/command_ids.txt");
        std::fs::write(path, format!("{}\n", ids.join("\n"))).unwrap();
        return;
    }
    let known: Vec<&str> = KNOWN.lines().filter(|l| !l.is_empty()).collect();
    let missing: Vec<&&str> = known.iter().filter(|id| !ids.contains(id)).collect();
    let new: Vec<&&str> = ids.iter().filter(|id| !known.contains(id)).collect();
    assert!(missing.is_empty(), "command ids removed or renamed (never allowed once shipped): {missing:?}");
    assert!(new.is_empty(), "new command ids: add them to tests/it/command_ids.txt: {new:?}");
    assert_eq!(ids, known);
}

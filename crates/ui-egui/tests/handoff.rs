//! `ssh://` links from a second start: read once, deleted, never connecting by themselves.

use sshcraft_ui_egui::handoff;
use sshcraft_ui_egui::testing::app_harness;

#[test]
fn a_link_fills_the_quick_connect_dialog_and_its_file_goes() {
    let dir = tempfile::tempdir().unwrap();
    let drop = tempfile::tempdir().unwrap();
    handoff::write(drop.path(), "ssh://root@db.example.org:2222").unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.state_mut().handoff_dir = Some(drop.path().to_path_buf());
    h.run_steps(3);
    let dialog = h.state().ui.quick_connect.clone().expect("the dialog opens");
    assert_eq!(dialog.text, "root@db.example.org:2222");
    assert!(h.state().ui.tabs.is_empty(), "nothing connects by itself");
    assert!(h.state().data.hosts.is_empty(), "nothing is saved by itself");
    assert_eq!(std::fs::read_dir(drop.path()).unwrap().count(), 0);
}

#[test]
fn hostile_and_oversized_files_are_dropped_unread() {
    let drop = tempfile::tempdir().unwrap();
    handoff::write(drop.path(), "ssh://-oProxyCommand=calc@host").unwrap();
    std::fs::write(drop.path().join("big.json"), format!("{{\"link\":\"ssh://{}\"}}", "a".repeat(5000))).unwrap();
    std::fs::write(drop.path().join("broken.json"), "not json").unwrap();
    std::fs::write(drop.path().join("other.txt"), "ssh://host").unwrap();
    assert!(handoff::take(drop.path()).is_empty());
    let left: Vec<String> = std::fs::read_dir(drop.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(left, vec!["other.txt".to_string()], "every handoff file is removed, other files stay");
}

#[test]
fn at_most_eight_files_are_taken_at_once() {
    let drop = tempfile::tempdir().unwrap();
    for i in 0..10 {
        handoff::write(drop.path(), &format!("ssh://host{i}")).unwrap();
    }
    assert_eq!(handoff::take(drop.path()).len(), handoff::MAX_FILES);
    assert_eq!(handoff::take(drop.path()).len(), 2);
}

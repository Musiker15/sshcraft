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

#[test]
fn a_shortcut_opens_only_a_saved_host() {
    let dir = tempfile::tempdir().unwrap();
    let drop = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let id = sshcraft_model::model::HostId(uuid::Uuid::new_v4());
    let host = serde_json::json!({ "id": id, "name": "web", "settings": { "address": "127.0.0.1", "port": 1 } });
    session.runtime().block_on(session.execute(sshcraft_engine::Caller::User, "host.save", serde_json::json!({ "host": host }))).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !h.state().data.hosts.iter().any(|r| r.host.id == id) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    }
    h.state_mut().handoff_dir = Some(drop.path().to_path_buf());
    handoff::write_host(drop.path(), sshcraft_model::model::HostId(uuid::Uuid::new_v4())).unwrap();
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(25));
        h.run_steps(1);
    }
    assert!(!drop.path().read_dir().unwrap().any(|e| e.unwrap().path().extension().is_some_and(|x| x == "json")), "the file was taken");
    assert!(h.state().ui.tabs.is_empty(), "an unknown id opens nothing");
    h.state_mut().handoff_next = 0.0;
    handoff::write_host(drop.path(), id).unwrap();
    h.run_steps(3);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while h.state().ui.tabs.is_empty() {
        assert!(std::time::Instant::now() < deadline, "the saved host opens");
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    }
}

#[test]
fn a_plain_second_start_brings_a_hidden_window_back() {
    let dir = tempfile::tempdir().unwrap();
    let drop = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.state_mut().handoff_dir = Some(drop.path().to_path_buf());
    h.state_mut().dropdown_hidden = true;
    handoff::write_reveal(drop.path()).unwrap();
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run_steps(1);
    }
    assert!(!h.state().dropdown_hidden, "the window is shown again");
    assert!(handoff::take(drop.path()).is_empty(), "the file was taken");
}

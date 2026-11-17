//! Import with a preview (only checked entries, warnings unchecked) and export without secrets;
//! file dialogs answered through craft-dialogs' test override.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use craft_dialogs::Answer;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use sshcraft_engine::Caller;
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::pages::import_export::{ImportSourceKind, ImportStep};
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

fn until(h: &mut Harness<'static, SshcraftApp>, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}; messages: {:?}", h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
}

/// Every file dialog answers `path`.
fn answer_with(h: &mut Harness<'static, SshcraftApp>, path: PathBuf) {
    h.state_mut().dialogs.set_override(Some(Box::new(move |_| Answer::Paths(vec![path.clone()]))));
}

fn preview_csv(h: &mut Harness<'static, SshcraftApp>, csv: &str, dir: &std::path::Path) {
    let file = dir.join("hosts.csv");
    std::fs::write(&file, csv).unwrap();
    answer_with(h, file);
    h.get_by_label("More").click();
    h.run_steps(2);
    h.get_by_label("Import").click();
    h.run_steps(3);
    h.state_mut().ui.import.as_mut().expect("the import opened").source = ImportSourceKind::Csv;
    h.run_steps(2);
    h.get_by_label("Choose file").click();
    until(h, "the chosen file", |app| app.ui.import.as_ref().is_some_and(|i| i.path.is_some()));
    h.get_by_label("Preview").click();
    until(h, "the preview", |app| matches!(app.ui.import.as_ref().map(|i| &i.step), Some(ImportStep::Preview { .. })));
}

#[test]
fn a_csv_import_creates_only_checked_entries() {
    let dir = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    preview_csv(&mut h, "name,address,port,username\nweb,192.0.2.1,22,root\ndb,192.0.2.2,22,root\n", files.path());
    {
        let import = h.state_mut().ui.import.as_mut().unwrap();
        let ImportStep::Preview { candidates, .. } = &mut import.step else { unreachable!() };
        assert_eq!(candidates.len(), 2);
        let db = candidates.iter_mut().find(|c| c.candidate.name == "db").unwrap();
        db.checked = false;
    }
    h.get_by_label("Import hosts").click();
    until(&mut h, "the import", |app| !app.data.hosts.is_empty() && app.ui.import.is_none());
    let names: Vec<&str> = h.state().data.hosts.iter().map(|r| r.host.name.as_str()).collect();
    assert_eq!(names, ["web"]);
}

#[test]
fn entries_with_a_warning_start_unchecked() {
    let dir = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    preview_csv(&mut h, "name,address,port\nok,192.0.2.1,22\nbad,192.0.2.2,99999\n", files.path());
    let import = h.state().ui.import.as_ref().unwrap();
    let ImportStep::Preview { candidates, .. } = &import.step else { unreachable!() };
    let checked = |name: &str| candidates.iter().find(|c| c.candidate.name == name).unwrap().checked;
    assert!(checked("ok"));
    assert!(!checked("bad"), "the entry with a bad port starts unchecked");
}

#[test]
fn an_export_writes_no_secret() {
    let dir = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let host = uuid::Uuid::new_v4();
    let run = |id: &str, params| session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap();
    run("host.save", json!({ "host": { "id": host, "name": "db", "group_id": null, "settings": { "address": "192.0.2.5" } } }));
    run("password.set", json!({ "owner": { "kind": "host", "id": host }, "secret": "S3cret-pw" }));
    until(&mut h, "the host", |app| !app.data.hosts.is_empty());
    let out = files.path().join("hosts.json");
    answer_with(&mut h, out.clone());
    h.get_by_label("More").click();
    h.run_steps(2);
    h.get_by_label("Export").click();
    h.run_steps(3);
    h.state_mut().ui.export.as_mut().expect("the export opened").json = true;
    h.run_steps(2);
    h.get_by_label("Export to file").click();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !out.exists() {
        assert!(Instant::now() < deadline, "no file; messages: {:?}", h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    std::thread::sleep(Duration::from_millis(100));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("\"db\""), "{text}");
    assert!(!text.contains("S3cret"), "no secret in the export");
    assert!(text.contains("has_password"), "it says a password exists: {text}");
}

#[test]
fn a_dialog_answered_later_still_reaches_its_page() {
    let dir = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    let file = files.path().join("hosts.csv");
    std::fs::write(&file, "name,address\nweb,192.0.2.1\n").unwrap();
    answer_with(&mut h, file.clone());
    h.get_by_label("More").click();
    h.run_steps(2);
    h.get_by_label("Import").click();
    h.run_steps(3);
    h.state_mut().ui.import.as_mut().unwrap().source = ImportSourceKind::Csv;
    h.run_steps(2);
    h.get_by_label("Choose file").click();
    // The click lands in this frame; the answer is taken while another page is shown.
    h.run_steps(1);
    assert!(h.state().ui.import.as_ref().unwrap().path.is_none(), "not answered yet");
    h.state_mut().ui.page = Page::Keys;
    h.run_steps(3);
    h.state_mut().ui.page = Page::Hosts;
    until(&mut h, "the chosen file", |app| app.ui.import.as_ref().is_some_and(|i| i.path.is_some()));
    assert_eq!(h.state().ui.import.as_ref().unwrap().path.as_deref(), Some(file.as_path()));
}

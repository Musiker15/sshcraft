//! The app shell: start page, commands that never block a frame, errors as messages, data that
//! follows the engine.

use std::time::{Duration, Instant};

use egui_kittest::kittest::Queryable;
use serde_json::json;
use sshcraft_engine::Caller;
use sshcraft_ui_egui::testing::app_harness;

#[test]
fn the_app_starts_on_the_hosts_page() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    assert_eq!(h.state().ui.page, sshcraft_ui_egui::state::Page::Hosts);
    assert!(h.query_all_by_label("Hosts").next().is_some());
}

#[test]
fn a_slow_command_does_not_block_a_frame() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(2);
    h.state().runner.run_future(
        async {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            Ok(json!("late"))
        },
        Box::new(|app, result| app.toasts.info(&format!("{}", result.unwrap_or_default()))),
    );
    let start = Instant::now();
    h.run_steps(10);
    assert!(start.elapsed() < Duration::from_millis(1000), "ten frames took {:?}", start.elapsed());
    assert_eq!(h.state().runner.busy(), 1);
    std::thread::sleep(Duration::from_millis(1700));
    h.run_steps(2);
    assert_eq!(h.state().runner.busy(), 0);
    assert!(h.state().toasts.texts().iter().any(|t| t.contains("late")), "{:?}", h.state().toasts.texts());
}

#[test]
fn an_engine_error_becomes_a_message() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(2);
    h.state().run_reported("host.save", json!({}));
    std::thread::sleep(Duration::from_millis(300));
    h.run_steps(3);
    assert!(!h.state().toasts.texts().is_empty(), "the failure is shown");
}

#[test]
fn data_follows_data_changed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    h.run_steps(2);
    let runtime = session.runtime();
    runtime
        .block_on(session.execute(
            Caller::User,
            "host.save",
            json!({ "host": { "id": uuid::Uuid::new_v4(), "name": "db", "group_id": null, "settings": { "address": "10.0.0.5" } } }),
        ))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !h.state().data.hosts.iter().any(|row| row.host.name == "db") {
        assert!(Instant::now() < deadline, "the host never arrived");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

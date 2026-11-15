//! Budgets of the window (rule 9 of the design), in a release build only (`cargo xtask perf`): a
//! debug build measures the compiler, not the program.

#![cfg(not(debug_assertions))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::json;
use sshcraft_engine::Caller;
use sshcraft_import::import::ImportCandidate;
use sshcraft_model::model::ConnectionSettings;
use sshcraft_ui_egui::testing::{app_harness, app_on};

/// Memory the process uses now (working set), in bytes.
fn in_use() -> usize {
    memory_stats::memory_stats().map_or(0, |m| m.physical_mem)
}

/// The tests measure time and memory; one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const MIB: usize = 1024 * 1024;
const FRAME: Duration = Duration::from_millis(8);

#[test]
fn the_empty_app_stays_in_its_memory_budget() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let before = in_use();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(120);
    let grown = in_use().saturating_sub(before);
    assert!(grown <= 200 * MIB, "the empty app grew by {} MiB", grown / MIB);
}

#[test]
fn a_frame_of_the_hosts_page_with_5000_hosts_takes_under_8_ms() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let (mut app, session, ctx) = app_on(dir.path());
    let candidates: Vec<ImportCandidate> = (0..5000)
        .map(|i| ImportCandidate {
            name: format!("server-{i:04}.example.internal"),
            settings: ConnectionSettings { address: Some(format!("10.{}.{}.{}", i / 65536, (i / 256) % 256, i % 256)), ..Default::default() },
            tags: vec![format!("rack-{}", i % 40)],
            ..Default::default()
        })
        .collect();
    session.runtime().block_on(session.execute(Caller::User, "import.apply", json!({ "candidates": candidates }))).unwrap();
    let frame = |app: &mut sshcraft_ui_egui::SshcraftApp| {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let start = Instant::now();
        let _ = ctx.run_ui(input, |ui| app.frame(ui));
        start.elapsed()
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.data.hosts.len() < 5000 {
        assert!(Instant::now() < deadline, "the hosts did not load: {}", app.data.hosts.len());
        frame(&mut app);
        std::thread::sleep(Duration::from_millis(10));
    }
    for _ in 0..10 {
        frame(&mut app);
    }
    let mut times: Vec<Duration> = (0..31).map(|_| frame(&mut app)).collect();
    times.sort();
    let median = times[times.len() / 2];
    eprintln!("hosts page, 5000 hosts: median {median:?}, slowest {:?}", times[times.len() - 1]);
    assert!(median < FRAME, "a frame with 5000 hosts took {median:?} (median of 31)");
}

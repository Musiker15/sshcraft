//! The app lock: it locks after the idle time, then shows nothing of the sessions and hosts,
//! and unlocks only after the user's presence.

use std::time::{Duration, Instant};

use egui_kittest::kittest::Queryable;
use serde_json::json;
use sshcraft_engine::{Caller, Session};
use sshcraft_ui_egui::testing::{app_harness, app_harness_with};

fn add_host(session: &Session, name: &str) {
    let host = json!({ "id": uuid::Uuid::new_v4(), "name": name, "settings": { "address": "10.0.0.5" } });
    session.runtime().block_on(session.execute(Caller::User, "host.save", json!({ "host": host }))).unwrap();
}

fn set_minutes(h: &mut egui_kittest::Harness<'static, sshcraft_ui_egui::SshcraftApp>, minutes: u32) {
    h.state_mut().data.settings.general.lock_after_minutes = minutes;
    h.state_mut().lock.last_input = Instant::now().checked_sub(Duration::from_secs(u64::from(minutes) * 60 + 1)).unwrap();
}

#[test]
fn the_app_locks_after_idle_and_hides_everything() {
    if !sshcraft_ui_egui::lock::supported() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    add_host(&session, "secret-host");
    for _ in 0..20 {
        h.run_steps(1);
        if h.query_by_label("secret-host").is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(h.query_by_label("secret-host").is_some());
    set_minutes(&mut h, 5);
    // A frame without input.
    h.step();
    assert!(h.state().lock.locked);
    h.run_steps(2);
    assert!(h.query_by_label("secret-host").is_none(), "nothing of the hosts under the lock");
    h.get_by_label("SSHCraft is locked");
    h.get_by_label("Unlock").click();
    for _ in 0..50 {
        h.run_steps(1);
        if !h.state().lock.locked {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!h.state().lock.locked, "unlocked with the presence");
}

#[test]
fn a_declined_unlock_stays_locked() {
    if !sshcraft_ui_egui::lock::supported() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness_with(dir.path(), |c| c.presence = std::sync::Arc::new(sshcraft_engine::testing::NeverPresent));
    h.run_steps(2);
    set_minutes(&mut h, 1);
    h.step();
    assert!(h.state().lock.locked);
    h.run_steps(1);
    h.get_by_label("Unlock").click();
    for _ in 0..50 {
        h.run_steps(1);
        if h.state().lock.error.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(h.state().lock.locked);
    assert!(h.state().lock.error.is_some());
}

#[test]
fn zero_minutes_never_locks() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(2);
    h.state_mut().lock.last_input = Instant::now().checked_sub(Duration::from_secs(100_000)).unwrap();
    h.step();
    assert!(!h.state().lock.locked);
}

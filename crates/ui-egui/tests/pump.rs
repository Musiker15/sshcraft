//! The pump between a session's terminal stream and the terminal: at most a frame's budget per
//! frame, every byte fed acknowledged once, nothing held without counting against the session.

use std::time::Duration;

use serde_json::json;
use sshcraft_engine::testing::trusted_test_host;
use sshcraft_engine::{Caller, Session, SessionConfig, SessionId, SessionStatus};
use sshcraft_term::{TermConfig, TermSize, Terminal};
use sshcraft_ui_egui::terminal::pump::{FrameBudget, Pump};

/// The flood of the test server's shell.
fn flood_len() -> usize {
    sshcraft_ssh::testing::flood_pattern().len()
}

/// A frame budget by bytes only, so a slow test machine does not change the outcome.
fn bytes(max_bytes: usize) -> FrameBudget {
    FrameBudget { max_bytes, max_time: Duration::from_secs(3600) }
}

async fn open() -> (Session, SessionId, Pump, sshcraft_ssh::testing::TestServer, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    let (server, host) = trusted_test_host(&session).await;
    let opened = session
        .execute(Caller::User, "session.open", json!({ "target": { "kind": "host", "host_id": host }, "cols": 80, "rows": 24 }))
        .await
        .unwrap();
    let id: SessionId = serde_json::from_value(opened["session"].clone()).unwrap();
    let stream = session.attach_terminal(id).unwrap();
    (session, id, Pump::new(stream), server, dir)
}

async fn flood(session: &Session, id: SessionId) {
    session.execute(Caller::User, "session.send", json!({ "id": id, "data": "flood\n" })).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_frame_takes_at_most_the_budget_and_acks_what_it_took() {
    let (session, id, mut pump, _server, _dir) = open().await;
    let mut term = Terminal::new(TermSize::clamped(80, 24), &TermConfig::default());
    flood(&session, id).await;
    let budget = 100 * 1024;
    let mut total = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while total < flood_len() {
        assert!(tokio::time::Instant::now() < deadline, "only {total} of {} bytes arrived", flood_len());
        let outcome = pump.frame(&mut term, bytes(budget));
        assert!(outcome.fed <= budget, "a frame fed {} bytes", outcome.fed);
        // What the pump holds and has not fed still counts against the session.
        assert!(pump.held() <= session.unacknowledged(id).unwrap_or(0), "held {} > unacknowledged", pump.held());
        total += outcome.fed;
        if outcome.fed == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    assert_eq!(total, flood_len(), "nothing doubled, nothing lost");
}

#[tokio::test(flavor = "multi_thread")]
async fn leftover_output_waits_for_the_next_frame() {
    let (session, id, mut pump, _server, _dir) = open().await;
    let mut term = Terminal::new(TermSize::clamped(80, 24), &TermConfig::default());
    flood(&session, id).await;
    // Let the session read up to its watermark, then take a little.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let first = pump.frame(&mut term, bytes(1000));
    assert_eq!(first.fed, 1000);
    assert!(first.more, "there is more to take");
    let mut total = first.fed;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while total < flood_len() {
        assert!(tokio::time::Instant::now() < deadline);
        let outcome = pump.frame(&mut term, bytes(512 * 1024));
        total += outcome.fed;
        if outcome.fed == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    assert_eq!(total, flood_len());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(session.unacknowledged(id), Some(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn status_and_close_come_through() {
    let (session, id, mut pump, _server, _dir) = open().await;
    let mut term = Terminal::new(TermSize::clamped(80, 24), &TermConfig::default());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut statuses = Vec::new();
    while !statuses.iter().any(|s| matches!(s, SessionStatus::Connected { .. })) {
        assert!(tokio::time::Instant::now() < deadline, "never connected: {statuses:?}");
        statuses.extend(pump.frame(&mut term, FrameBudget::DEFAULT).status);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    session.execute(Caller::User, "session.close", json!({ "id": id })).await.unwrap();
    loop {
        let outcome = pump.frame(&mut term, FrameBudget::DEFAULT);
        statuses.extend(outcome.status);
        if outcome.closed {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "the stream never closed: {statuses:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(statuses.iter().any(|s| matches!(s, SessionStatus::Connected { .. })), "{statuses:?}");
}

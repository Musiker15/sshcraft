//! Sessions through `execute` and the terminal stream: flow control without a reader, prompts a
//! lagging subscriber missed, input order, and that typed input never reaches a recording.

use std::time::Duration;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use serde_json::{Value, json};
use sshcraft_model::model::{ConnectionSettings, Host, HostId, KeyRef};
use sshcraft_ssh::testing::{TestAuth, TestServer, flood_pattern};

use crate::caller::Caller;
use crate::events::EngineEvent;
use crate::sessions::{OUTPUT_HIGH_WATERMARK, SessionId, SessionStatus};
use crate::terminal::{TermEvent, TerminalStream};
use crate::{Session, SessionConfig};

/// One SSH data message at most; what may come on top of the watermark.
const BLOCK: usize = 64 * 1024;

struct World {
    session: Session,
    server: TestServer,
    host: HostId,
    _dir: tempfile::TempDir,
}

/// An engine with one host on a test server that takes the host's stored key.
async fn world(quiet: bool, trusted: bool, event_capacity: usize) -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::for_tests(dir.path());
    config.event_capacity = event_capacity;
    let (session, _) = Session::open(config).unwrap();
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server = TestServer::start_with(TestAuth { key: Some(key.public_key().clone()), quiet, ..Default::default() }).await;
    let host = {
        let store = session.store();
        let info = session.vault().unwrap().import_key(&store, "test", &key.to_openssh(LineEnding::LF).unwrap(), None).unwrap();
        let host = Host {
            forwards: Vec::new(),
            location: None,
            id: HostId::new(),
            name: "test host".into(),
            group_id: None,
            settings: ConnectionSettings {
                address: Some("127.0.0.1".into()),
                port: Some(server.port),
                username: Some("tester".into()),
                key: Some(KeyRef::Stored { id: info.id }),
                ..Default::default()
            },
            tags: Vec::new(),
            os: None,
        };
        store.upsert_host(&host).unwrap();
        if trusted {
            store.set_known_host("127.0.0.1", server.port, &server.host_fingerprint).unwrap();
        }
        host.id
    };
    World { session, server, host, _dir: dir }
}

async fn open(w: &World) -> SessionId {
    let opened = w
        .session
        .execute(Caller::User, "session.open", json!({ "target": { "kind": "host", "host_id": w.host }, "cols": 80, "rows": 24 }))
        .await
        .unwrap();
    serde_json::from_value(opened["session"].clone()).unwrap()
}

async fn send(w: &World, id: SessionId, text: &str) {
    w.session.execute(Caller::User, "session.send", json!({ "id": id, "data": text })).await.unwrap();
}

async fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Takes output from the stream, acknowledging it, until `enough` says so.
async fn drain(stream: &mut TerminalStream, enough: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let mut seen = Vec::new();
    while !enough(&seen) {
        let event = tokio::time::timeout(Duration::from_secs(20), stream.recv()).await.expect("no output in time").expect("the stream ended");
        if let TermEvent::Data(bytes) = event {
            stream.ack(bytes.len());
            seen.extend(bytes);
        }
    }
    seen
}

#[tokio::test]
async fn an_unattached_terminal_stays_bounded() {
    let w = world(true, true, crate::EVENT_CAPACITY).await;
    let id = open(&w).await;
    send(&w, id, "flood\n").await;
    let held = || w.session.sessions().unacknowledged(id).unwrap_or(0);
    wait_until("the watermark", || held() >= OUTPUT_HIGH_WATERMARK).await;
    // The session stopped reading: nothing more piles up.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(held() <= OUTPUT_HIGH_WATERMARK + BLOCK, "{} bytes held", held());

    let mut stream = w.session.attach_terminal(id).unwrap();
    assert!(w.session.attach_terminal(id).is_none(), "a terminal is attached once");
    let flood = flood_pattern();
    let seen = drain(&mut stream, |seen| seen.len() >= flood.len()).await;
    assert_eq!(seen, flood);
}

#[tokio::test]
async fn a_lagging_subscriber_still_finds_open_prompts() {
    let w = world(false, false, 1).await;
    let mut events = w.session.subscribe();
    for _ in 0..10 {
        w.session.events().send(EngineEvent::SettingsChanged);
    }
    let id = open(&w).await;
    let list = || async { w.session.execute(Caller::User, "prompt.list", Value::Null).await.unwrap() };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let prompts = loop {
        let prompts = list().await;
        if prompts.as_array().is_some_and(|p| !p.is_empty()) {
            break prompts;
        }
        assert!(tokio::time::Instant::now() < deadline, "no open prompt");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(matches!(events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_))));
    assert_eq!(prompts[0]["kind"], "host_key");
    assert_eq!(prompts[0]["fingerprint"], w.server.host_fingerprint.as_str());

    let prompt_id = prompts[0]["prompt_id"].clone();
    w.session.execute(Caller::User, "prompt.host_key", json!({ "prompt_id": prompt_id, "accept": true })).await.unwrap();
    let mut stream = w.session.attach_terminal(id).unwrap();
    loop {
        match tokio::time::timeout(Duration::from_secs(20), stream.recv()).await.unwrap().unwrap() {
            TermEvent::Status(SessionStatus::Connected { .. }) => break,
            TermEvent::Status(SessionStatus::Ended { .. }) => panic!("the session ended"),
            _ => {}
        }
    }
    assert!(list().await.as_array().unwrap().is_empty());
    let known = w.session.store().known_host("127.0.0.1", w.server.port).unwrap();
    assert_eq!(known.as_deref(), Some(w.server.host_fingerprint.as_str()));
}

#[tokio::test]
async fn session_send_keeps_order_through_execute() {
    let w = world(false, true, crate::EVENT_CAPACITY).await;
    let id = open(&w).await;
    let mut stream = w.session.attach_terminal(id).unwrap();
    let mut expected = Vec::new();
    for i in 0..1000 {
        let chunk = format!("{i:04};");
        send(&w, id, &chunk).await;
        expected.extend_from_slice(chunk.as_bytes());
    }
    let seen = drain(&mut stream, |seen| seen.len() >= expected.len()).await;
    assert_eq!(String::from_utf8_lossy(&seen), String::from_utf8_lossy(&expected));
}

#[tokio::test]
async fn typed_input_is_never_recorded() {
    let w = world(true, true, crate::EVENT_CAPACITY).await;
    w.session.settings().update(|s| s.logs.session_logs = true);
    let id = open(&w).await;
    assert_eq!(w.session.execute(Caller::User, "session.recording", json!({ "id": id })).await.unwrap(), json!(true));
    send(&w, id, "S3CRET-INPUT\n").await;
    send(&w, id, "flood\n").await;
    let mut stream = w.session.attach_terminal(id).unwrap();
    drain(&mut stream, |seen| seen.len() >= BLOCK).await;
    w.session.execute(Caller::User, "session.close", json!({ "id": id })).await.unwrap();

    let vault = w.session.vault().unwrap();
    let store = w.session.store();
    let logs = sshcraft_store::session_log::list(&vault, &store).unwrap();
    assert_eq!(logs.len(), 1);
    let recorded = sshcraft_store::session_log::read(&vault, &store, logs[0].meta.id).unwrap();
    assert!(recorded.len() >= BLOCK, "the output is recorded");
    assert!(!recorded.windows(12).any(|w| w == b"S3CRET-INPUT"), "typed input is in the log");
}

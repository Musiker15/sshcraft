use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use stollen_core::model::{ConnectionSettings, Host, KeyRef};
use stollen_core::vault::MemoryMasterKey;
use stollen_ssh::testing::TestServer;

use super::*;

#[derive(Default)]
struct RecordingSink {
    statuses: Mutex<Vec<SessionStatus>>,
    data: Mutex<Vec<u8>>,
}

impl SessionSink for RecordingSink {
    fn data(&self, bytes: Vec<u8>) {
        self.data.lock().unwrap().extend(bytes);
    }
    fn status(&self, status: SessionStatus) {
        self.statuses.lock().unwrap().push(status);
    }
}

impl RecordingSink {
    fn statuses(&self) -> Vec<SessionStatus> {
        self.statuses.lock().unwrap().clone()
    }

    async fn wait_for(&self, pred: impl Fn(&[SessionStatus]) -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        while !pred(&self.statuses()) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out, statuses: {:?}",
                self.statuses()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

struct FixedPrompter {
    answer: bool,
    asked: AtomicUsize,
}

#[async_trait::async_trait]
impl HostKeyPrompter for FixedPrompter {
    async fn ask(&self, _: &str, _: &str, _: u16, _: &str) -> bool {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.answer
    }
}

struct Fixture {
    server: TestServer,
    store: Arc<Mutex<Store>>,
    manager: SessionManager,
    prompter: Arc<FixedPrompter>,
    host_id: HostId,
}

async fn fixture(answer: bool) -> Fixture {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server = TestServer::start(key.public_key().clone()).await;
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    let info = vault
        .import_key(
            &store,
            "test",
            &key.to_openssh(LineEnding::LF).unwrap(),
            None,
        )
        .unwrap();
    let host = Host {
        id: HostId::new(),
        name: "test host".into(),
        group_id: None,
        settings: ConnectionSettings {
            address: Some("127.0.0.1".into()),
            port: Some(server.port),
            username: Some("tester".into()),
            key: Some(KeyRef::Stored { id: info.id }),
        },
    };
    store.upsert_host(&host).unwrap();
    let store = Arc::new(Mutex::new(store));
    let prompter = Arc::new(FixedPrompter {
        answer,
        asked: AtomicUsize::new(0),
    });
    let manager = SessionManager::with_delay_scale(
        store.clone(),
        Some(Arc::new(vault)),
        prompter.clone(),
        0.01,
    );
    Fixture {
        server,
        store,
        manager,
        prompter,
        host_id: host.id,
    }
}

fn ended(statuses: &[SessionStatus]) -> Option<EndReason> {
    statuses.iter().find_map(|s| match s {
        SessionStatus::Ended { reason, .. } => Some(*reason),
        _ => None,
    })
}

fn connected_count(statuses: &[SessionStatus]) -> usize {
    statuses
        .iter()
        .filter(|s| **s == SessionStatus::Connected)
        .count()
}

#[tokio::test]
async fn unknown_host_key_asks_and_stores_on_accept() {
    let f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    assert_eq!(f.prompter.asked.load(Ordering::SeqCst), 1);
    let stored = f
        .store
        .lock()
        .unwrap()
        .known_host("127.0.0.1", f.server.port)
        .unwrap();
    assert_eq!(stored, Some(f.server.host_fingerprint.clone()));
}

#[tokio::test]
async fn known_host_key_does_not_ask() {
    let f = fixture(true).await;
    f.store
        .lock()
        .unwrap()
        .set_known_host("127.0.0.1", f.server.port, &f.server.host_fingerprint)
        .unwrap();
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    assert_eq!(f.prompter.asked.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unknown_host_key_declined_ends_rejected() {
    let f = fixture(false).await;
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::HostKeyRejected));
    let stored = f
        .store
        .lock()
        .unwrap()
        .known_host("127.0.0.1", f.server.port)
        .unwrap();
    assert_eq!(stored, None);
}

#[tokio::test]
async fn mismatch_ends_without_prompt() {
    let f = fixture(true).await;
    f.store
        .lock()
        .unwrap()
        .set_known_host("127.0.0.1", f.server.port, "SHA256:other")
        .unwrap();
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::HostKeyMismatch));
    assert_eq!(f.prompter.asked.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn lost_connection_reconnects() {
    let mut f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    f.server.restart(false).await;
    sink.wait_for(|s| connected_count(s) == 2).await;
    assert!(
        sink.statuses()
            .iter()
            .any(|s| matches!(s, SessionStatus::Reconnecting { attempt: 1, .. }))
    );
    assert_eq!(ended(&sink.statuses()), None);
}

#[tokio::test]
async fn reconnect_into_changed_host_key_stops() {
    let mut f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    f.server.restart(true).await;
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::HostKeyMismatch));
    assert_eq!(connected_count(&sink.statuses()), 1);
    assert_eq!(f.prompter.asked.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn exit_does_not_reconnect() {
    let f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    f.manager.write(id, b"exit\n").await.unwrap();
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::Exited));
    assert!(
        !sink
            .statuses()
            .iter()
            .any(|s| matches!(s, SessionStatus::Reconnecting { .. }))
    );
}

#[tokio::test]
async fn output_reaches_sink() {
    let f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    f.manager.write(id, b"ping\n").await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !sink.data.lock().unwrap().windows(4).any(|w| w == b"ping") {
        assert!(tokio::time::Instant::now() < deadline, "no echo");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn missing_vault_ends_with_error() {
    let f = fixture(true).await;
    let manager = SessionManager::new(f.store.clone(), None, f.prompter.clone());
    let sink = Arc::new(RecordingSink::default());
    manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::Error));
}

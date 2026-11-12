use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use sshcraft_core::model::{ConnectionSettings, Host, KeyRef};
use sshcraft_core::vault::MemoryMasterKey;
use sshcraft_ssh::testing::TestServer;

use super::*;
use crate::hello::{Presence, UserPresence};

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

/// Hands out in-memory agents holding `keys`, or fails like a missing Proton Pass.
struct TestAgents {
    keys: Vec<PrivateKey>,
    reachable: bool,
}

#[async_trait::async_trait]
impl AgentConnector for TestAgents {
    async fn connect(&self) -> Result<sshcraft_ssh::DynAgent, SshError> {
        if !self.reachable {
            return Err(SshError::AgentUnavailable("pipe not found".into()));
        }
        Ok(sshcraft_ssh::testing::spawn_agent(&self.keys, true).await)
    }
}

struct Fixture {
    server: TestServer,
    store: Arc<Mutex<Store>>,
    manager: SessionManager,
    prompter: Arc<FixedPrompter>,
    host_id: HostId,
    key: PrivateKey,
    vault: Arc<Vault>,
}

/// Answers Windows Hello requests from a script; the last answer repeats.
struct FakePresence {
    answers: Mutex<Vec<Presence>>,
    asked: AtomicUsize,
}

impl FakePresence {
    fn new(answers: &[Presence]) -> Self {
        Self {
            answers: Mutex::new(answers.to_vec()),
            asked: AtomicUsize::new(0),
        }
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl UserPresence for FakePresence {
    async fn verify(&self, _reason: &str) -> Presence {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let mut answers = self.answers.lock().unwrap();
        if answers.len() > 1 {
            answers.remove(0)
        } else {
            answers[0]
        }
    }
}

impl Fixture {
    /// Adds a host that uses the fixture's key through the agent instead of the vault.
    fn agent_host(&self) -> HostId {
        let host = Host {
            id: HostId::new(),
            name: "agent host".into(),
            group_id: None,
            settings: ConnectionSettings {
                address: Some("127.0.0.1".into()),
                port: Some(self.server.port),
                username: Some("tester".into()),
                key: Some(KeyRef::Agent {
                    fingerprint: self
                        .key
                        .fingerprint(russh::keys::HashAlg::Sha256)
                        .to_string(),
                }),
            },
        };
        self.store.lock().unwrap().upsert_host(&host).unwrap();
        host.id
    }

    fn manager_with_presence(&self, presence: Arc<FakePresence>) -> SessionManager {
        SessionManager::with_delay_scale(
            self.store.clone(),
            Some(self.vault.clone()),
            self.prompter.clone(),
            Arc::new(TestAgents {
                keys: vec![self.key.clone()],
                reachable: true,
            }),
            presence,
            0.01,
        )
    }

    fn manager_with(&self, vault: Option<Arc<Vault>>, agent_reachable: bool) -> SessionManager {
        SessionManager::with_delay_scale(
            self.store.clone(),
            vault,
            self.prompter.clone(),
            Arc::new(TestAgents {
                keys: vec![self.key.clone()],
                reachable: agent_reachable,
            }),
            Arc::new(FakePresence::new(&[Presence::Verified])),
            0.01,
        )
    }
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
    let vault = Arc::new(vault);
    let manager = SessionManager::with_delay_scale(
        store.clone(),
        Some(vault.clone()),
        prompter.clone(),
        Arc::new(TestAgents {
            keys: vec![key.clone()],
            reachable: true,
        }),
        Arc::new(FakePresence::new(&[Presence::Verified])),
        0.01,
    );
    Fixture {
        server,
        store,
        manager,
        prompter,
        host_id: host.id,
        key,
        vault,
    }
}

fn connected(statuses: &[SessionStatus]) -> bool {
    statuses.contains(&SessionStatus::Connected)
}

#[tokio::test]
async fn stored_key_asks_hello_once_per_run() {
    let f = fixture(true).await;
    let presence = Arc::new(FakePresence::new(&[Presence::Verified]));
    let manager = f.manager_with_presence(presence.clone());
    let (a, b) = (
        Arc::new(RecordingSink::default()),
        Arc::new(RecordingSink::default()),
    );
    manager.open(f.host_id, 80, 24, a.clone());
    a.wait_for(connected).await;
    manager.open(f.host_id, 80, 24, b.clone());
    b.wait_for(connected).await;
    assert_eq!(presence.asked(), 1);
}

#[tokio::test]
async fn declined_hello_ends_without_connecting() {
    let f = fixture(true).await;
    let presence = Arc::new(FakePresence::new(&[Presence::Declined]));
    let manager = f.manager_with_presence(presence.clone());
    let sink = Arc::new(RecordingSink::default());
    manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    let statuses = sink.statuses();
    assert_eq!(ended(&statuses), Some(EndReason::Error));
    assert!(!connected(&statuses));
    assert!(
        !statuses
            .iter()
            .any(|s| matches!(s, SessionStatus::Reconnecting { .. }))
    );
}

#[tokio::test]
async fn declined_hello_asks_again_next_time() {
    let f = fixture(true).await;
    let presence = Arc::new(FakePresence::new(&[Presence::Declined, Presence::Verified]));
    let manager = f.manager_with_presence(presence.clone());
    let first = Arc::new(RecordingSink::default());
    manager.open(f.host_id, 80, 24, first.clone());
    first.wait_for(|s| ended(s).is_some()).await;
    let second = Arc::new(RecordingSink::default());
    manager.open(f.host_id, 80, 24, second.clone());
    second.wait_for(connected).await;
    assert_eq!(presence.asked(), 2);
}

#[tokio::test]
async fn unavailable_hello_connects_and_flags() {
    let f = fixture(true).await;
    let presence = Arc::new(FakePresence::new(&[Presence::Unavailable]));
    let manager = f.manager_with_presence(presence.clone());
    let sink = Arc::new(RecordingSink::default());
    manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(connected).await;
    assert!(manager.hello_missing());
}

#[tokio::test]
async fn agent_key_does_not_ask_hello() {
    let f = fixture(true).await;
    let host = f.agent_host();
    let presence = Arc::new(FakePresence::new(&[Presence::Declined]));
    let manager = f.manager_with_presence(presence.clone());
    let sink = Arc::new(RecordingSink::default());
    manager.open(host, 80, 24, sink.clone());
    sink.wait_for(connected).await;
    assert_eq!(presence.asked(), 0);
}

#[tokio::test]
async fn agent_key_connects() {
    let f = fixture(true).await;
    let host = f.agent_host();
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(host, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
}

#[tokio::test]
async fn agent_unavailable_ends_with_error_and_no_retry() {
    let f = fixture(true).await;
    let host = f.agent_host();
    let manager = f.manager_with(None, false);
    let sink = Arc::new(RecordingSink::default());
    manager.open(host, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    let statuses = sink.statuses();
    assert_eq!(ended(&statuses), Some(EndReason::Error));
    let detail = statuses.iter().find_map(|s| match s {
        SessionStatus::Ended { detail, .. } => detail.clone(),
        _ => None,
    });
    assert!(detail.unwrap_or_default().contains("Proton Pass"));
    assert!(
        !statuses
            .iter()
            .any(|s| matches!(s, SessionStatus::Reconnecting { .. }))
    );
}

#[tokio::test]
async fn agent_key_needs_no_vault() {
    let f = fixture(true).await;
    let host = f.agent_host();
    let manager = f.manager_with(None, true);
    let sink = Arc::new(RecordingSink::default());
    manager.open(host, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
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
    f.manager.write(id, b"exit\n").unwrap();
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
    f.manager.write(id, b"ping\n").unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !sink.data.lock().unwrap().windows(4).any(|w| w == b"ping") {
        assert!(tokio::time::Instant::now() < deadline, "no echo");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn missing_vault_ends_with_error() {
    let f = fixture(true).await;
    let manager = f.manager_with(None, true);
    let sink = Arc::new(RecordingSink::default());
    manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::Error));
}

#[tokio::test]
async fn writes_are_delivered_in_order() {
    let f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    let mut expected = Vec::new();
    for i in 0..300 {
        let chunk = format!("{i:04};");
        expected.extend_from_slice(chunk.as_bytes());
        f.manager.write(id, chunk.as_bytes()).unwrap();
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while sink.data.lock().unwrap().len() < expected.len() {
        assert!(tokio::time::Instant::now() < deadline, "echo incomplete");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(*sink.data.lock().unwrap(), expected);
}

#[tokio::test]
async fn output_waits_for_acknowledgement() {
    let f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(f.host_id, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    f.manager.write(id, b"flood\n").unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let unacked = sink.data.lock().unwrap().len();
    assert!(unacked > 0, "some output must arrive");
    assert!(
        unacked <= OUTPUT_HIGH_WATERMARK + 64 * 1024,
        "{unacked} bytes delivered without acknowledgement"
    );
    let total = sshcraft_ssh::testing::flood_pattern().len();
    let mut acked = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let received = sink.data.lock().unwrap().len();
        if received >= total {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "stalled at {received}"
        );
        f.manager.ack(id, received - acked);
        acked = received;
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(sink.data.lock().unwrap().len(), total);
}

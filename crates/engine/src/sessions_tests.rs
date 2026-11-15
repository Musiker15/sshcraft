use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use sshcraft_model::model::{ConnectionSettings, Host, KeyRef};
use sshcraft_ssh::testing::TestServer;
use sshcraft_store::vault::MemoryMasterKey;

use super::*;
use crate::error::ErrorCode;
use crate::testing::TestAgents;
use sshcraft_fido::presence::{Presence, UserPresence};

/// A manager as the Tauri version built it; the settings are read from the Tauri row `app` in the
/// store, so a test sets them with [`put_app_settings`].
fn test_manager(
    store: Arc<Mutex<Store>>,
    vault: Option<Arc<Vault>>,
    prompter: Arc<dyn HostKeyPrompter>,
    auth: Arc<dyn AuthPrompter>,
    agent: Arc<dyn AgentConnector>,
    presence: Arc<dyn UserPresence>,
    delay_scale: f32,
) -> SessionManager {
    let settings_store = store.clone();
    SessionManager::new(ManagerParts {
        store,
        vault,
        prompter,
        auth,
        agent,
        presence,
        security: None,
        settings: Arc::new(move || crate::settings::read_tauri_settings(&settings_store.lock().unwrap())),
        runtime: tokio::runtime::Handle::current(),
        delay_scale,
    })
}

/// Writes the settings the way the Tauri version kept them (`app` row, JSON).
fn put_app_settings(store: &Mutex<Store>, settings: serde_json::Value) {
    store.lock().unwrap().put_setting("app", &settings.to_string()).unwrap();
}

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
            assert!(tokio::time::Instant::now() < deadline, "timed out, statuses: {:?}", self.statuses());
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

    async fn ask_command(&self, _: &str, _: &str) -> bool {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.answer
    }

    async fn ask_secret_ref(&self, _: &str, _: &str) -> bool {
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
        Self { answers: Mutex::new(answers.to_vec()), asked: AtomicUsize::new(0) }
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
        if answers.len() > 1 { answers.remove(0) } else { answers[0] }
    }
}

impl Fixture {
    /// Adds a host that uses the fixture's key through the agent instead of the vault.
    fn agent_host(&self) -> HostId {
        let host = Host {
            forwards: Vec::new(),
            location: None,
            id: HostId::new(),
            name: "agent host".into(),
            group_id: None,
            settings: ConnectionSettings {
                address: Some("127.0.0.1".into()),
                port: Some(self.server.port),
                username: Some("tester".into()),
                key: Some(KeyRef::Agent { fingerprint: self.key.fingerprint(russh::keys::HashAlg::Sha256).to_string() }),
                ..Default::default()
            },
            tags: Vec::new(),
            os: None,
        };
        self.store.lock().unwrap().upsert_host(&host).unwrap();
        host.id
    }

    fn manager_with_presence(&self, presence: Arc<FakePresence>) -> SessionManager {
        self.manager_with_any_presence(presence)
    }

    fn manager_with_any_presence(&self, presence: Arc<dyn UserPresence>) -> SessionManager {
        test_manager(
            self.store.clone(),
            Some(self.vault.clone()),
            self.prompter.clone(),
            Arc::new(sshcraft_ssh::NoPrompter),
            Arc::new(TestAgents { keys: vec![self.key.clone()], reachable: true }),
            presence,
            0.01,
        )
    }

    fn manager_with(&self, vault: Option<Arc<Vault>>, agent_reachable: bool) -> SessionManager {
        test_manager(
            self.store.clone(),
            vault,
            self.prompter.clone(),
            Arc::new(sshcraft_ssh::NoPrompter),
            Arc::new(TestAgents { keys: vec![self.key.clone()], reachable: agent_reachable }),
            Arc::new(FakePresence::new(&[Presence::Verified])),
            0.01,
        )
    }
}

async fn fixture(answer: bool) -> Fixture {
    fixture_with(answer, TestAuth::default()).await
}

/// [`fixture`] with a test server for `auth` (its key is the fixture's).
async fn fixture_with(answer: bool, auth: TestAuth) -> Fixture {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server = TestServer::start_with(TestAuth { key: Some(key.public_key().clone()), ..auth }).await;
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    let info = vault.import_key(&store, "test", &key.to_openssh(LineEnding::LF).unwrap(), None).unwrap();
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
    let store = Arc::new(Mutex::new(store));
    let prompter = Arc::new(FixedPrompter { answer, asked: AtomicUsize::new(0) });
    let vault = Arc::new(vault);
    let manager = test_manager(
        store.clone(),
        Some(vault.clone()),
        prompter.clone(),
        Arc::new(sshcraft_ssh::NoPrompter),
        Arc::new(TestAgents { keys: vec![key.clone()], reachable: true }),
        Arc::new(FakePresence::new(&[Presence::Verified])),
        0.01,
    );
    Fixture { server, store, manager, prompter, host_id: host.id, key, vault }
}

fn connected(statuses: &[SessionStatus]) -> bool {
    statuses.iter().any(|s| matches!(s, SessionStatus::Connected { .. }))
}

#[tokio::test]
async fn stored_key_asks_hello_once_per_run() {
    let f = fixture(true).await;
    let presence = Arc::new(FakePresence::new(&[Presence::Verified]));
    let manager = f.manager_with_presence(presence.clone());
    let (a, b) = (Arc::new(RecordingSink::default()), Arc::new(RecordingSink::default()));
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, a.clone());
    a.wait_for(connected).await;
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, b.clone());
    b.wait_for(connected).await;
    assert_eq!(presence.asked(), 1);
}

#[tokio::test]
async fn declined_hello_ends_without_connecting() {
    let f = fixture(true).await;
    let presence = Arc::new(FakePresence::new(&[Presence::Declined]));
    let manager = f.manager_with_presence(presence.clone());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    let statuses = sink.statuses();
    assert_eq!(ended(&statuses), Some(EndReason::Error));
    assert_eq!(ended_code(&statuses), Some(ErrorCode::HelloDeclined));
    assert!(!connected(&statuses));
    assert!(!statuses.iter().any(|s| matches!(s, SessionStatus::Reconnecting { .. })));
}

#[tokio::test]
async fn declined_hello_asks_again_next_time() {
    let f = fixture(true).await;
    let presence = Arc::new(FakePresence::new(&[Presence::Declined, Presence::Verified]));
    let manager = f.manager_with_presence(presence.clone());
    let first = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, first.clone());
    first.wait_for(|s| ended(s).is_some()).await;
    let second = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, second.clone());
    second.wait_for(connected).await;
    assert_eq!(presence.asked(), 2);
}

#[tokio::test]
async fn unavailable_hello_connects_and_flags() {
    let f = fixture(true).await;
    let presence = Arc::new(FakePresence::new(&[Presence::Unavailable]));
    let manager = f.manager_with_presence(presence.clone());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
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
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(connected).await;
    assert_eq!(presence.asked(), 0);
}

#[tokio::test]
async fn agent_key_connects() {
    let f = fixture(true).await;
    let host = f.agent_host();
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
}

#[tokio::test]
async fn agent_unavailable_ends_with_error_and_no_retry() {
    let f = fixture(true).await;
    let host = f.agent_host();
    let manager = f.manager_with(None, false);
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    let statuses = sink.statuses();
    assert_eq!(ended(&statuses), Some(EndReason::Error));
    assert_eq!(ended_code(&statuses), Some(ErrorCode::AgentUnavailable));
    assert!(!statuses.iter().any(|s| matches!(s, SessionStatus::Reconnecting { .. })));
}

#[tokio::test]
async fn agent_key_needs_no_vault() {
    let f = fixture(true).await;
    let host = f.agent_host();
    let manager = f.manager_with(None, true);
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
}

fn ended(statuses: &[SessionStatus]) -> Option<EndReason> {
    statuses.iter().find_map(|s| match s {
        SessionStatus::Ended { reason, .. } => Some(*reason),
        _ => None,
    })
}

fn ended_code(statuses: &[SessionStatus]) -> Option<ErrorCode> {
    statuses.iter().find_map(|s| match s {
        SessionStatus::Ended { code, .. } => *code,
        _ => None,
    })
}

#[tokio::test]
async fn first_failed_attempt_is_connect_failed() {
    let f = fixture(true).await;
    let closed_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let host_id = f.agent_host();
    {
        let store = f.store.lock().unwrap();
        let mut host = store.list_hosts().unwrap().into_iter().find(|h| h.id == host_id).unwrap();
        host.settings.port = Some(closed_port);
        store.upsert_host(&host).unwrap();
    }
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    let statuses = sink.statuses();
    assert_eq!(ended(&statuses), Some(EndReason::ConnectFailed));
    assert_eq!(ended_code(&statuses), Some(ErrorCode::Network));
    assert!(!statuses.iter().any(|s| matches!(s, SessionStatus::Reconnecting { .. })));
}

fn connected_count(statuses: &[SessionStatus]) -> usize {
    statuses.iter().filter(|s| matches!(s, SessionStatus::Connected { .. })).count()
}

#[tokio::test]
async fn unknown_host_key_asks_and_stores_on_accept() {
    let f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    assert_eq!(f.prompter.asked.load(Ordering::SeqCst), 1);
    let stored = f.store.lock().unwrap().known_host("127.0.0.1", f.server.port).unwrap();
    assert_eq!(stored, Some(f.server.host_fingerprint.clone()));
}

#[tokio::test]
async fn known_host_key_does_not_ask() {
    let f = fixture(true).await;
    f.store.lock().unwrap().set_known_host("127.0.0.1", f.server.port, &f.server.host_fingerprint).unwrap();
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    assert_eq!(f.prompter.asked.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unknown_host_key_declined_ends_rejected() {
    let f = fixture(false).await;
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::HostKeyRejected));
    let stored = f.store.lock().unwrap().known_host("127.0.0.1", f.server.port).unwrap();
    assert_eq!(stored, None);
}

#[tokio::test]
async fn mismatch_ends_without_prompt() {
    let f = fixture(true).await;
    f.store.lock().unwrap().set_known_host("127.0.0.1", f.server.port, "SHA256:other").unwrap();
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::HostKeyMismatch));
    assert_eq!(f.prompter.asked.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn lost_connection_reconnects() {
    let mut f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    f.server.restart(false).await;
    sink.wait_for(|s| connected_count(s) == 2).await;
    assert!(sink.statuses().iter().any(|s| matches!(s, SessionStatus::Reconnecting { attempt: 1, .. })));
    assert_eq!(ended(&sink.statuses()), None);
}

#[tokio::test]
async fn reconnect_into_changed_host_key_stops() {
    let mut f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
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
    let id = f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    f.manager.write(id, b"exit\n").unwrap();
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::Exited));
    assert!(!sink.statuses().iter().any(|s| matches!(s, SessionStatus::Reconnecting { .. })));
}

#[tokio::test]
async fn output_reaches_sink() {
    let f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
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
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::Error));
    assert_eq!(ended_code(&sink.statuses()), Some(ErrorCode::VaultLocked));
}

#[tokio::test]
async fn writes_are_delivered_in_order() {
    let f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
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
    let id = f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    f.manager.write(id, b"flood\n").unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let unacked = sink.data.lock().unwrap().len();
    assert!(unacked > 0, "some output must arrive");
    assert!(unacked <= OUTPUT_HIGH_WATERMARK + 64 * 1024, "{unacked} bytes delivered without acknowledgement");
    let total = sshcraft_ssh::testing::flood_pattern().len();
    let mut acked = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let received = sink.data.lock().unwrap().len();
        if received >= total {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "stalled at {received}");
        f.manager.ack(id, received - acked);
        acked = received;
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(sink.data.lock().unwrap().len(), total);
}

/// Never answers; counts how often a pending check was dropped (the tab was closed).
#[derive(Default)]
struct PendingThenVerified {
    asked: AtomicUsize,
    dropped: Arc<AtomicUsize>,
}

struct CountDrop(Arc<AtomicUsize>);

impl Drop for CountDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl UserPresence for PendingThenVerified {
    async fn verify(&self, _reason: &str) -> Presence {
        if self.asked.fetch_add(1, Ordering::SeqCst) == 0 {
            let _guard = CountDrop(self.dropped.clone());
            std::future::pending::<()>().await;
        }
        Presence::Verified
    }
}

async fn wait_until(pred: impl Fn() -> bool) {
    for _ in 0..400 {
        if pred() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition not reached");
}

#[tokio::test]
async fn closing_tab_cancels_hello() {
    let f = fixture(true).await;
    let presence = Arc::new(PendingThenVerified::default());
    let manager = f.manager_with_any_presence(presence.clone());
    let id = manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, Arc::new(RecordingSink::default()));
    wait_until(|| presence.asked.load(Ordering::SeqCst) == 1).await;
    manager.close(id);
    wait_until(|| presence.dropped.load(Ordering::SeqCst) == 1).await;
}

#[tokio::test]
async fn waiting_tab_asks_again_after_other_tab_closed() {
    let f = fixture(true).await;
    let presence = Arc::new(PendingThenVerified::default());
    let manager = f.manager_with_any_presence(presence.clone());
    let first = manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, Arc::new(RecordingSink::default()));
    wait_until(|| presence.asked.load(Ordering::SeqCst) == 1).await;
    let second = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, second.clone());
    manager.close(first);
    second.wait_for(connected).await;
    assert_eq!(presence.asked.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn hello_unavailable_is_reported_once() {
    let f = fixture(true).await;
    let manager = f.manager_with_presence(Arc::new(FakePresence::new(&[Presence::Unavailable])));
    let reported = Arc::new(AtomicUsize::new(0));
    let counter = reported.clone();
    manager.on_hello_unavailable(Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    let (a, b) = (Arc::new(RecordingSink::default()), Arc::new(RecordingSink::default()));
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, a.clone());
    a.wait_for(connected).await;
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, b.clone());
    b.wait_for(connected).await;
    assert_eq!(reported.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn connected_session_is_recent() {
    let f = fixture(true).await;
    assert!(f.store.lock().unwrap().recent_hosts(10).unwrap().is_empty());
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(connected).await;
    assert_eq!(f.store.lock().unwrap().recent_hosts(10).unwrap(), vec![f.host_id]);
}

#[tokio::test]
async fn host_session_uses_terminal_type_setting() {
    let f = fixture(true).await;
    put_app_settings(&f.store, serde_json::json!({ "terminal_type": "linux" }));
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(connected).await;
    // The client reports "connected" once its requests are sent; the server may record the pty
    // request a moment later.
    for _ in 0..200 {
        if !f.server.requested_terms().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(f.server.requested_terms(), vec!["linux".to_owned()]);
}

#[test]
fn hello_reason_follows_the_app_language() {
    use crate::settings::LanguageSetting;
    let english = hello_reason(LanguageSetting::En, Some("de-DE"));
    let german = hello_reason(LanguageSetting::De, Some("en-US"));
    assert!(english.contains("wants to use"), "{english}");
    assert!(german.contains("möchte"), "{german}");
    assert_eq!(hello_reason(LanguageSetting::Auto, Some("de-AT")), german);
    assert_eq!(hello_reason(LanguageSetting::Auto, Some("fr-FR")), english);
    assert_eq!(hello_reason(LanguageSetting::Auto, None), english);
}

#[tokio::test]
async fn a_key_not_on_this_device_ends_before_hello() {
    let f = fixture(true).await;
    {
        let store = f.store.lock().unwrap();
        let mut host = store.list_hosts().unwrap().pop().unwrap();
        host.settings.key = Some(KeyRef::Stored { id: sshcraft_model::model::KeyId::new() });
        store.upsert_host(&host).unwrap();
    }
    let presence = Arc::new(FakePresence::new(&[Presence::Verified]));
    let manager = f.manager_with_presence(presence.clone());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended_code(&sink.statuses()), Some(ErrorCode::KeyNotOnDevice));
    assert_eq!(presence.asked(), 0, "no Windows Hello prompt for a key that is not here");
}

#[test]
fn os_probe_runs_once() {
    let store = std::sync::Mutex::new(sshcraft_store::store::Store::open_in_memory().unwrap());
    let host = sshcraft_model::model::Host {
        forwards: Vec::new(),
        id: sshcraft_model::model::HostId::new(),
        name: "h".into(),
        group_id: None,
        settings: Default::default(),
        location: None,
        tags: Vec::new(),
        os: None,
    };
    store.lock().unwrap().upsert_host(&host).unwrap();
    assert!(needs_os(&store, host.id));
    mark_probed(&store.lock().unwrap(), host.id);
    assert!(!needs_os(&store, host.id), "a host whose system was not recognized is asked once");
}

// ---- Passwords, routes and sign-in questions ----

use crate::prompts::PromptRegistry;
use crate::prompts::tests::{FakeEmitter, first_auth_question};
use sshcraft_model::model::PasswordId;
use sshcraft_ssh::testing::TestAuth;

/// A store with a vault and hosts that sign in with passwords.
struct PasswordWorld {
    store: Arc<Mutex<Store>>,
    vault: Arc<Vault>,
}

impl PasswordWorld {
    fn new() -> Self {
        let store = Store::open_in_memory().unwrap();
        let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
        Self { store: Arc::new(Mutex::new(store)), vault: Arc::new(vault) }
    }

    /// A host at `port` with a stored password (if given) and jump hosts.
    fn host(&self, port: u16, password: Option<&str>, jumps: Vec<HostId>) -> HostId {
        let store = self.store.lock().unwrap();
        let password_id = password.map(|p| {
            let id = PasswordId::new();
            self.vault.store_password(&store, id, p, None).unwrap();
            id
        });
        let host = Host {
            forwards: Vec::new(),
            location: None,
            id: HostId::new(),
            name: format!("host-{port}"),
            group_id: None,
            settings: ConnectionSettings {
                address: Some("127.0.0.1".into()),
                port: Some(port),
                username: Some("tester".into()),
                password: password_id,
                jump_hosts: (!jumps.is_empty()).then_some(jumps),
                ..Default::default()
            },
            tags: Vec::new(),
            os: None,
        };
        store.upsert_host(&host).unwrap();
        host.id
    }

    /// Trusts the server's host key beforehand, as after an earlier connection.
    fn trust(&self, server: &TestServer) {
        self.store.lock().unwrap().set_known_host("127.0.0.1", server.port, &server.host_fingerprint).unwrap();
    }

    fn set(&self, host: HostId, change: impl FnOnce(&mut ConnectionSettings)) {
        let store = self.store.lock().unwrap();
        let mut h = store.list_hosts().unwrap().into_iter().find(|h| h.id == host).unwrap();
        change(&mut h.settings);
        store.upsert_host(&h).unwrap();
    }

    fn manager(
        &self,
        vault: Option<Arc<Vault>>,
        host_keys: Arc<FixedPrompter>,
        auth: Arc<dyn sshcraft_ssh::AuthPrompter>,
        presence: Arc<FakePresence>,
    ) -> SessionManager {
        test_manager(self.store.clone(), vault, host_keys, auth, Arc::new(TestAgents { keys: Vec::new(), reachable: false }), presence, 0.01)
    }
}

fn trusting() -> Arc<FixedPrompter> {
    Arc::new(FixedPrompter { answer: true, asked: AtomicUsize::new(0) })
}

fn verified() -> Arc<FakePresence> {
    Arc::new(FakePresence::new(&[Presence::Verified]))
}

async fn password_server(password: &str) -> TestServer {
    TestServer::start_with(TestAuth { password: Some(password.into()), ..Default::default() }).await
}

async fn kbd_server(answer: &str) -> TestServer {
    TestServer::start_with(TestAuth { kbd: vec![vec![("Code: ".into(), true, answer.into())]], ..Default::default() }).await
}

#[tokio::test]
async fn password_host_connects() {
    let server = password_server("pw").await;
    let w = PasswordWorld::new();
    w.trust(&server);
    let host = w.host(server.port, Some("pw"), Vec::new());
    let manager = w.manager(Some(w.vault.clone()), trusting(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(connected).await;
    assert_eq!(server.received_passwords(), vec!["pw".to_owned()]);
}

#[tokio::test]
async fn jump_route_connects() {
    let jump = password_server("a").await;
    let target = password_server("b").await;
    let w = PasswordWorld::new();
    w.trust(&jump);
    w.trust(&target);
    let via = w.host(jump.port, Some("a"), Vec::new());
    let host = w.host(target.port, Some("b"), vec![via]);
    let host_keys = trusting();
    let manager = w.manager(Some(w.vault.clone()), host_keys.clone(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(connected).await;
    assert_eq!(host_keys.asked.load(Ordering::SeqCst), 0);
    assert_eq!(jump.received_passwords(), vec!["a".to_owned()]);
    assert_eq!(target.received_passwords(), vec!["b".to_owned()]);
}

#[tokio::test]
async fn kbd_prompt_reaches_registry_and_answer_logs_in() {
    let server = kbd_server("42").await;
    let w = PasswordWorld::new();
    let host = w.host(server.port, None, Vec::new());
    let registry = Arc::new(PromptRegistry::new(FakeEmitter::default()));
    let manager = w.manager(Some(w.vault.clone()), trusting(), registry.clone(), verified());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    let prompt = first_auth_question(&registry).await;
    assert_eq!(prompt.prompts[0].text, "Code: ");
    assert_eq!(prompt.username, "tester");
    registry.answer_auth(prompt.prompt_id, Some(vec!["42".into()]));
    sink.wait_for(connected).await;
}

#[tokio::test]
async fn closing_tab_withdraws_auth_prompt() {
    let server = kbd_server("42").await;
    let w = PasswordWorld::new();
    let host = w.host(server.port, None, Vec::new());
    let registry = Arc::new(PromptRegistry::new(FakeEmitter::default()));
    let manager = w.manager(Some(w.vault.clone()), trusting(), registry.clone(), verified());
    let sink = Arc::new(RecordingSink::default());
    let id = manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    let prompt = first_auth_question(&registry).await;
    manager.close(id);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !registry.emitter().withdrawn.lock().unwrap().contains(&prompt.prompt_id) {
        assert!(tokio::time::Instant::now() < deadline, "not withdrawn");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn cancel_ends_without_reconnect() {
    let server = kbd_server("42").await;
    let w = PasswordWorld::new();
    let host = w.host(server.port, None, Vec::new());
    let registry = Arc::new(PromptRegistry::new(FakeEmitter::default()));
    let manager = w.manager(Some(w.vault.clone()), trusting(), registry.clone(), verified());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    let prompt = first_auth_question(&registry).await;
    registry.answer_auth(prompt.prompt_id, None);
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::AuthCancelled));
    assert!(!sink.statuses().iter().any(|s| matches!(s, SessionStatus::Reconnecting { .. })));
}

#[tokio::test]
async fn vault_locked_with_password_reports_vault_locked() {
    let server = password_server("pw").await;
    let w = PasswordWorld::new();
    let host = w.host(server.port, Some("pw"), Vec::new());
    let manager = w.manager(None, trusting(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended_code(&sink.statuses()), Some(ErrorCode::VaultLocked));
    assert!(server.received_passwords().is_empty());
}

#[tokio::test]
async fn hello_asked_before_password_use() {
    let server = password_server("pw").await;
    let w = PasswordWorld::new();
    w.trust(&server);
    let host = w.host(server.port, Some("pw"), Vec::new());
    let presence = verified();
    let manager = w.manager(Some(w.vault.clone()), trusting(), Arc::new(sshcraft_ssh::NoPrompter), presence.clone());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(connected).await;
    assert_eq!(presence.asked(), 1);
}

#[tokio::test]
async fn declined_hello_keeps_password_back() {
    let server = password_server("pw").await;
    let w = PasswordWorld::new();
    let host = w.host(server.port, Some("pw"), Vec::new());
    let manager =
        w.manager(Some(w.vault.clone()), trusting(), Arc::new(sshcraft_ssh::NoPrompter), Arc::new(FakePresence::new(&[Presence::Declined])));
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended_code(&sink.statuses()), Some(ErrorCode::HelloDeclined));
    assert!(server.received_passwords().is_empty());
}

// ---- Local shells and serial ports ----

impl RecordingSink {
    fn data_text(&self) -> String {
        String::from_utf8_lossy(&self.data.lock().unwrap()).into_owned()
    }
}

#[tokio::test]
async fn local_target_runs_and_ends() {
    let w = PasswordWorld::new();
    let manager = w.manager(None, trusting(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let shell = if cfg!(windows) {
        sshcraft_local::pty::ShellCommand { program: "cmd.exe".into(), args: vec!["/c".into(), "echo sshcraft-tab".into()], label: "echo".into() }
    } else {
        sshcraft_local::pty::ShellCommand { program: "sh".into(), args: vec!["-c".into(), "echo sshcraft-tab".into()], label: "echo".into() }
    };
    let sink = Arc::new(RecordingSink::default());
    let id = manager.open(SessionTarget::Local { shell, cwd: None }, 80, 24, sink.clone());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut answered = false;
    while ended(&sink.statuses()).is_none() {
        assert!(tokio::time::Instant::now() < deadline, "{:?}", sink.statuses());
        // ConPTY waits for the cursor position; xterm.js answers it in the app.
        if !answered && sink.data_text().contains("\u{1b}[6n") {
            manager.write(id, b"\x1b[1;1R").unwrap();
            answered = true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(ended(&sink.statuses()), Some(EndReason::Exited));
    assert!(sink.data_text().contains("sshcraft-tab"));
    assert!(!sink.statuses().iter().any(|s| matches!(s, SessionStatus::Reconnecting { .. })));
    assert!(w.store.lock().unwrap().recent_hosts(10).unwrap().is_empty());
}

#[tokio::test]
async fn serial_with_invalid_settings_ends_with_error() {
    let w = PasswordWorld::new();
    let manager = w.manager(None, trusting(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let sink = Arc::new(RecordingSink::default());
    let settings = sshcraft_local::serial::SerialSettings { port: "COM1".into(), data_bits: 9, ..Default::default() };
    manager.open(SessionTarget::Serial { settings }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::Error));
    assert_eq!(ended_code(&sink.statuses()), Some(ErrorCode::SerialFailed));
}

#[tokio::test]
async fn missing_serial_port_fails_without_retrying() {
    let w = PasswordWorld::new();
    let manager = w.manager(None, trusting(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let sink = Arc::new(RecordingSink::default());
    let settings = sshcraft_local::serial::SerialSettings { port: "sshcraft-no-such-port".into(), ..Default::default() };
    manager.open(SessionTarget::Serial { settings }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::ConnectFailed));
    assert_eq!(ended_code(&sink.statuses()), Some(ErrorCode::SerialFailed));
}

// ---- Review fixes ----

fn ended_detail(statuses: &[SessionStatus]) -> Option<String> {
    statuses.iter().find_map(|s| match s {
        SessionStatus::Ended { detail, .. } => detail.clone(),
        _ => None,
    })
}

#[tokio::test]
async fn first_contact_withholds_stored_password() {
    let server = password_server("pw").await;
    let w = PasswordWorld::new();
    let host = w.host(server.port, Some("pw"), Vec::new());
    let manager = w.manager(Some(w.vault.clone()), trusting(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::AuthCancelled));
    assert!(server.received_passwords().is_empty());
}

#[tokio::test]
async fn jump_failure_keeps_its_code_and_names_the_hop() {
    let jump = password_server("a").await;
    let target = password_server("b").await;
    let w = PasswordWorld::new();
    w.trust(&jump);
    w.trust(&target);
    let via = w.host(jump.port, Some("wrong"), Vec::new());
    let host = w.host(target.port, Some("b"), vec![via]);
    let manager = w.manager(Some(w.vault.clone()), trusting(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended_code(&sink.statuses()), Some(ErrorCode::PasswordRejected));
    assert_eq!(ended_detail(&sink.statuses()), Some(format!("host-{}", jump.port)));
}

#[tokio::test]
async fn missing_key_of_a_jump_host_names_the_hop() {
    let jump = password_server("a").await;
    let target = password_server("b").await;
    let w = PasswordWorld::new();
    let via = w.host(jump.port, None, Vec::new());
    w.set(via, |s| s.key = Some(KeyRef::Stored { id: sshcraft_model::model::KeyId::new() }));
    let host = w.host(target.port, Some("b"), vec![via]);
    let manager = w.manager(Some(w.vault.clone()), trusting(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended_code(&sink.statuses()), Some(ErrorCode::KeyNotOnDevice));
    assert_eq!(ended_detail(&sink.statuses()), Some(format!("host-{}", jump.port)));
}

#[tokio::test]
async fn forwarding_without_an_agent_is_noted() {
    let server = password_server("pw").await;
    let w = PasswordWorld::new();
    w.trust(&server);
    let host = w.host(server.port, Some("pw"), Vec::new());
    w.set(host, |s| s.agent_forwarding = Some(true));
    let manager = w.manager(Some(w.vault.clone()), trusting(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(connected).await;
    assert!(sink.statuses().contains(&SessionStatus::Connected { notice: Some(ErrorCode::AgentForwardingUnavailable) }));
}

// ---- Teilplan 10: keys with passphrase, security keys, certificates, authorized_keys ----

/// Answers each question with the next prepared answer (`None`: cancel) and keeps them.
struct ScriptedAuth {
    answers: Mutex<Vec<Option<String>>>,
    seen: Mutex<Vec<sshcraft_ssh::AuthRequest>>,
}

impl ScriptedAuth {
    fn new(answers: &[Option<&str>]) -> Arc<Self> {
        Arc::new(Self { answers: Mutex::new(answers.iter().map(|a| a.map(str::to_owned)).collect()), seen: Mutex::new(Vec::new()) })
    }

    fn seen(&self) -> Vec<sshcraft_ssh::AuthRequest> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl sshcraft_ssh::AuthPrompter for ScriptedAuth {
    async fn answer(&self, request: sshcraft_ssh::AuthRequest) -> Option<Vec<Zeroizing<String>>> {
        self.seen.lock().unwrap().push(request);
        let mut answers = self.answers.lock().unwrap();
        let next = if answers.is_empty() { None } else { answers.remove(0) };
        next.map(|a| vec![Zeroizing::new(a)])
    }
}

impl Fixture {
    fn key_id(&self) -> KeyId {
        self.store.lock().unwrap().list_key_records().unwrap()[0].id
    }

    fn manager_with_auth(&self, auth: Arc<dyn sshcraft_ssh::AuthPrompter>, presence: Arc<FakePresence>) -> SessionManager {
        test_manager(
            self.store.clone(),
            Some(self.vault.clone()),
            self.prompter.clone(),
            auth,
            Arc::new(TestAgents { keys: vec![self.key.clone()], reachable: true }),
            presence,
            0.01,
        )
    }

    /// A host on `port` as user `tester` with `key`.
    fn host_on(&self, port: u16, key: KeyRef, jumps: Option<Vec<HostId>>) -> HostId {
        let host = Host {
            forwards: Vec::new(),
            location: None,
            id: HostId::new(),
            name: "other".into(),
            group_id: None,
            settings: ConnectionSettings {
                address: Some("127.0.0.1".into()),
                port: Some(port),
                username: Some("tester".into()),
                key: Some(key),
                jump_hosts: jumps,
                ..Default::default()
            },
            tags: Vec::new(),
            os: None,
        };
        self.store.lock().unwrap().upsert_host(&host).unwrap();
        host.id
    }
}

async fn open_and_wait(manager: &SessionManager, host_id: HostId) -> Vec<SessionStatus> {
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected(s) || ended(s).is_some()).await;
    sink.statuses()
}

#[tokio::test]
async fn protected_key_asks_for_passphrase_once_and_connects() {
    let f = fixture(true).await;
    {
        let id = f.key_id();
        let store = f.store.lock().unwrap();
        f.vault.protect_key(&store, id, "pw").unwrap();
    }
    let auth = ScriptedAuth::new(&[Some("pw")]);
    let presence = Arc::new(FakePresence::new(&[Presence::Verified]));
    let manager = f.manager_with_auth(auth.clone(), presence.clone());
    let statuses = open_and_wait(&manager, f.host_id).await;
    assert!(connected(&statuses), "{statuses:?}");
    let seen = auth.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].passphrase_for.as_deref(), Some("test"));
    assert_eq!(presence.asked(), 0, "no Windows Hello for a key with a passphrase");
}

#[tokio::test]
async fn wrong_passphrase_three_times_ends_with_wrong_passphrase() {
    let f = fixture(true).await;
    {
        let id = f.key_id();
        let store = f.store.lock().unwrap();
        f.vault.protect_key(&store, id, "pw").unwrap();
    }
    let auth = ScriptedAuth::new(&[Some("a"), Some("b"), Some("c"), Some("pw")]);
    let manager = f.manager_with_auth(auth.clone(), Arc::new(FakePresence::new(&[Presence::Verified])));
    let statuses = open_and_wait(&manager, f.host_id).await;
    assert_eq!(ended_code(&statuses), Some(ErrorCode::WrongPassphrase));
    assert_eq!(auth.seen().len(), 3);
    let id = f.key_id();
    let record = f.store.lock().unwrap().key_record(id).unwrap().unwrap();
    assert!(record.protected, "a wrong passphrase changes nothing stored");
}

#[tokio::test]
async fn cancelled_passphrase_ends_with_auth_cancelled() {
    let f = fixture(true).await;
    {
        let id = f.key_id();
        let store = f.store.lock().unwrap();
        f.vault.protect_key(&store, id, "pw").unwrap();
    }
    let auth = ScriptedAuth::new(&[None]);
    let manager = f.manager_with_auth(auth, Arc::new(FakePresence::new(&[Presence::Verified])));
    let statuses = open_and_wait(&manager, f.host_id).await;
    assert_eq!(ended(&statuses), Some(EndReason::AuthCancelled));
}

#[tokio::test]
async fn passphrase_prompt_names_the_jump_host_key() {
    let f = fixture(true).await;
    let target_key = f.key_id();
    let jump_key = {
        let store = f.store.lock().unwrap();
        f.vault.store_protected(&store, "jump-key", &f.key, "jpw").unwrap().id
    };
    let jump = f.host_on(f.server.port, KeyRef::Stored { id: jump_key }, Some(Vec::new()));
    let target = f.host_on(f.server.port, KeyRef::Stored { id: target_key }, Some(vec![jump]));
    let auth = ScriptedAuth::new(&[Some("jpw")]);
    let manager = f.manager_with_auth(auth.clone(), Arc::new(FakePresence::new(&[Presence::Verified])));
    let statuses = open_and_wait(&manager, target).await;
    assert!(connected(&statuses), "{statuses:?}");
    let seen = auth.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].passphrase_for.as_deref(), Some("jump-key"));
}

#[tokio::test]
async fn security_key_signs_without_key_gate() {
    use sshcraft_fido::soft::SoftAuthenticator;
    use sshcraft_fido::{Attachment, CreateOptions, SkAlgorithm, create_key};
    let f = fixture(true).await;
    let token = Arc::new(SoftAuthenticator::new(Attachment::CrossPlatform));
    let options =
        CreateOptions { algorithm: SkAlgorithm::EcdsaP256, attachment: Attachment::CrossPlatform, verify_user: true, user_name: "tester".into() };
    let sk = create_key(token.as_ref(), &options, "yubikey").await.unwrap();
    let server = TestServer::start(sk.public_key().clone()).await;
    let id = {
        let store = f.store.lock().unwrap();
        f.vault.store_key(&store, "yubikey", &sk).unwrap().id
    };
    let host = f.host_on(server.port, KeyRef::Stored { id }, None);
    let presence = Arc::new(FakePresence::new(&[Presence::Verified]));
    let manager = f.manager_with_presence(presence.clone()).with_security_keys(token.clone());
    let statuses = open_and_wait(&manager, host).await;
    assert!(connected(&statuses), "{statuses:?}");
    assert_eq!(presence.asked(), 0);
    assert_eq!(token.assertions(), 1);
}

#[tokio::test]
async fn cancelled_security_key_ends_without_reconnecting() {
    use sshcraft_fido::soft::SoftAuthenticator;
    use sshcraft_fido::{Attachment, CreateOptions, SkAlgorithm, create_key};
    let f = fixture(true).await;
    let token = Arc::new(SoftAuthenticator::new(Attachment::CrossPlatform));
    let options =
        CreateOptions { algorithm: SkAlgorithm::Ed25519, attachment: Attachment::CrossPlatform, verify_user: false, user_name: "tester".into() };
    let sk = create_key(token.as_ref(), &options, "yubikey").await.unwrap();
    let server = TestServer::start(sk.public_key().clone()).await;
    let id = {
        let store = f.store.lock().unwrap();
        f.vault.store_key(&store, "yubikey", &sk).unwrap().id
    };
    token.set_cancel(true);
    let host = f.host_on(server.port, KeyRef::Stored { id }, None);
    let manager = f.manager_with_presence(Arc::new(FakePresence::new(&[Presence::Verified]))).with_security_keys(token);
    let statuses = open_and_wait(&manager, host).await;
    assert_eq!(ended(&statuses), Some(EndReason::AuthCancelled));
    assert_eq!(ended_code(&statuses), Some(ErrorCode::SecurityKeyCancelled));
}

#[tokio::test]
async fn expired_certificate_notice_reaches_status() {
    use sshcraft_ssh::testing::{TestAuth, user_certificate};
    let f = fixture(true).await;
    let ca = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server =
        TestServer::start_with(TestAuth { key: Some(f.key.public_key().clone()), ca: Some(ca.public_key().clone()), ..Default::default() }).await;
    let cert = user_certificate(f.key.public_key(), &ca, "tester", 0, 1);
    {
        let id = f.key_id();
        let store = f.store.lock().unwrap();
        f.vault.attach_certificate(&store, id, &cert.to_openssh().unwrap()).unwrap();
    }
    let host = f.host_on(server.port, KeyRef::Stored { id: f.key_id() }, None);
    let statuses = open_and_wait(&f.manager, host).await;
    assert!(statuses.iter().any(|s| matches!(s, SessionStatus::Connected { notice: Some(ErrorCode::CertificateExpired) })), "{statuses:?}");
}

#[tokio::test]
async fn install_key_adds_line_via_exec() {
    let f = fixture(true).await;
    f.server.set_exec_output("SSHCRAFT_ADDED\n");
    let outcome = f.manager.install_key(f.host_id, KeyRef::Stored { id: f.key_id() }).await.unwrap();
    assert_eq!(outcome, sshcraft_ssh::authorized_keys::InstallOutcome::Added);
    let commands = f.server.received_commands();
    assert_eq!(commands.len(), 1);
    let line = {
        let mut public = f.key.public_key().clone();
        public.set_comment("");
        public.to_openssh().unwrap()
    };
    assert!(commands[0].contains(line.trim()));
}

#[tokio::test]
async fn install_key_reports_unsupported_on_unknown_output() {
    let f = fixture(true).await;
    let err = f.manager.install_key(f.host_id, KeyRef::Stored { id: f.key_id() }).await.unwrap_err();
    assert_eq!(err.0, ErrorCode::InstallKeyUnsupported);
}

#[tokio::test]
async fn key_from_other_device_reports_key_not_on_device() {
    let f = fixture(true).await;
    let host = f.host_on(f.server.port, KeyRef::Stored { id: KeyId::new() }, None);
    let statuses = open_and_wait(&f.manager, host).await;
    assert_eq!(ended_code(&statuses), Some(ErrorCode::KeyNotOnDevice));
}

#[tokio::test]
async fn encrypted_security_key_asks_passphrase_then_signs() {
    use sshcraft_fido::soft::SoftAuthenticator;
    use sshcraft_fido::{Attachment, CreateOptions, SkAlgorithm, create_key};
    let f = fixture(true).await;
    let token = Arc::new(SoftAuthenticator::new(Attachment::CrossPlatform));
    let options =
        CreateOptions { algorithm: SkAlgorithm::EcdsaP256, attachment: Attachment::CrossPlatform, verify_user: false, user_name: "tester".into() };
    let sk = create_key(token.as_ref(), &options, "yubikey").await.unwrap();
    let server = TestServer::start(sk.public_key().clone()).await;
    let encrypted = sk.encrypt(&mut rand::rng(), "pw").unwrap().to_openssh(LineEnding::LF).unwrap();
    let id = KeyId::new();
    {
        let store = f.store.lock().unwrap();
        // As a synced record would bring it.
        f.vault.store_openssh_with_id(&store, id, "yubikey", &encrypted, None, None).unwrap();
    }
    let host = f.host_on(server.port, KeyRef::Stored { id }, None);
    let auth = ScriptedAuth::new(&[Some("pw")]);
    let manager = f.manager_with_auth(auth.clone(), Arc::new(FakePresence::new(&[Presence::Verified]))).with_security_keys(token.clone());
    let statuses = open_and_wait(&manager, host).await;
    assert!(connected(&statuses), "{statuses:?}");
    assert_eq!(auth.seen().len(), 1);
    assert_eq!(token.assertions(), 1);
}

#[tokio::test]
async fn reconnect_does_not_ask_the_passphrase_again() {
    let mut f = fixture(true).await;
    {
        let id = f.key_id();
        let store = f.store.lock().unwrap();
        f.vault.protect_key(&store, id, "pw").unwrap();
    }
    let auth = ScriptedAuth::new(&[Some("pw"), Some("pw"), Some("pw")]);
    let manager = f.manager_with_auth(auth.clone(), Arc::new(FakePresence::new(&[Presence::Verified])));
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    f.server.restart(false).await;
    sink.wait_for(|s| connected_count(s) == 2).await;
    assert_eq!(auth.seen().len(), 1, "asked once per tab, not per attempt");
}

#[tokio::test]
async fn one_key_on_jump_host_and_target_is_asked_once() {
    let f = fixture(true).await;
    let id = f.key_id();
    {
        let store = f.store.lock().unwrap();
        f.vault.protect_key(&store, id, "pw").unwrap();
    }
    let jump = f.host_on(f.server.port, KeyRef::Stored { id }, Some(Vec::new()));
    let target = f.host_on(f.server.port, KeyRef::Stored { id }, Some(vec![jump]));
    let auth = ScriptedAuth::new(&[Some("pw"), Some("pw")]);
    let manager = f.manager_with_auth(auth.clone(), Arc::new(FakePresence::new(&[Presence::Verified])));
    let statuses = open_and_wait(&manager, target).await;
    assert!(connected(&statuses), "{statuses:?}");
    assert_eq!(auth.seen().len(), 1);
}

#[tokio::test]
async fn install_key_reports_write_failure_and_cancelled_sign_in() {
    let f = fixture(true).await;
    let id = f.key_id();
    f.server.set_exec_output("SSHCRAFT_FAILED\n");
    let err = f.manager.install_key(f.host_id, KeyRef::Stored { id }).await.unwrap_err();
    assert_eq!(err.0, ErrorCode::InstallKeyFailed);

    {
        let store = f.store.lock().unwrap();
        f.vault.protect_key(&store, id, "pw").unwrap();
    }
    let manager = f.manager_with_auth(ScriptedAuth::new(&[None]), Arc::new(FakePresence::new(&[Presence::Verified])));
    let err = manager.install_key(f.host_id, KeyRef::Stored { id }).await.unwrap_err();
    assert_eq!(err.0, ErrorCode::SignInCancelled);
}

#[path = "forwards_tests.rs"]
mod forwards;

#[tokio::test]
async fn host_session_lists_a_directory() {
    let f = fixture(true).await;
    f.server.set_exec_output("motd\nSSHCRAFT-LS\na/\nb\n");
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(connected).await;
    assert_eq!(f.manager.list_dir(id, "/tmp", "").await.unwrap(), vec!["a/".to_owned(), "b".to_owned()]);
    assert!(f.server.received_commands().contains(&crate::listing::LIST_COMMAND.to_owned()));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !f.server.received_exec_input().ends_with(b"/tmp\n") {
        assert!(tokio::time::Instant::now() < deadline, "no directory on stdin");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn local_session_lists_a_directory() {
    let w = PasswordWorld::new();
    let manager = w.manager(None, trusting(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let shell = if cfg!(windows) {
        sshcraft_local::pty::ShellCommand { program: "cmd.exe".into(), args: Vec::new(), label: "cmd".into() }
    } else {
        sshcraft_local::pty::ShellCommand { program: "sh".into(), args: Vec::new(), label: "sh".into() }
    };
    let sink = Arc::new(RecordingSink::default());
    let id = manager.open(SessionTarget::Local { shell, cwd: None }, 80, 24, sink.clone());
    sink.wait_for(connected).await;
    let dir = std::env::temp_dir().join(format!("sshcraft-ls-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("inner")).unwrap();
    let names = manager.list_dir(id, &dir.to_string_lossy(), "").await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    manager.close(id);
    assert_eq!(names, vec!["inner/".to_owned()]);
}

/// Sets whether the fixture's host records and what the app setting says.
fn set_recording(f: &Fixture, host: Option<bool>, app: bool) {
    let store = f.store.lock().unwrap();
    let mut h = store.list_hosts().unwrap().into_iter().find(|h| h.id == f.host_id).unwrap();
    h.settings.session_log = host;
    store.upsert_host(&h).unwrap();
    drop(store);
    put_app_settings(&f.store, serde_json::json!({ "session_logs": app }));
}

async fn echo(f: &Fixture, sink: &RecordingSink, id: SessionId, text: &[u8]) {
    f.manager.write(id, text).unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !sink.data.lock().unwrap().windows(text.len()).any(|w| w == text) {
        assert!(tokio::time::Instant::now() < deadline, "no echo");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn logs(f: &Fixture) -> Vec<sshcraft_store::session_log::LogSummary> {
    sshcraft_store::session_log::list(&f.vault, &f.store.lock().unwrap()).unwrap()
}

#[tokio::test]
async fn a_host_that_asks_for_it_is_recorded() {
    let f = fixture(true).await;
    set_recording(&f, Some(true), false);
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    assert!(f.manager.is_recording(id));
    echo(&f, &sink, id, b"first-line").await;
    let mark = f.manager.bookmark(id, "here").unwrap();
    echo(&f, &sink, id, b"second-line").await;
    f.manager.close(id);
    let logs = logs(&f);
    assert_eq!(logs.len(), 1);
    let meta = &logs[0].meta;
    assert_eq!(meta.title, "test host");
    assert_eq!(meta.host_id, Some(f.host_id));
    assert!(meta.ended_at.is_some());
    assert_eq!(meta.marks, vec![mark.clone()]);
    let bytes = sshcraft_store::session_log::read(&f.vault, &f.store.lock().unwrap(), meta.id).unwrap();
    assert_eq!(bytes, *sink.data.lock().unwrap());
    assert!(bytes[..mark.offset as usize].ends_with(b"first-line"));
}

/// Spec 10 (T14): a recording holds what the server sent, never what was typed. A password
/// typed at a prompt that shows nothing must not appear in the log.
#[tokio::test]
async fn typed_input_never_reaches_the_log() {
    let f = fixture_with(true, TestAuth { quiet: true, ..TestAuth::default() }).await;
    set_recording(&f, Some(true), false);
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    assert!(f.manager.is_recording(id));
    f.manager.write(id, b"typed-secret-hunter2\n").unwrap();
    // Some output, so there is a log at all (`flood` must come as a chunk of its own).
    tokio::time::sleep(Duration::from_millis(300)).await;
    f.manager.write(id, b"flood\n").unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while sink.data.lock().unwrap().len() < 4096 {
        assert!(tokio::time::Instant::now() < deadline, "no output");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    f.manager.close(id);
    let logs = logs(&f);
    assert_eq!(logs.len(), 1);
    let bytes = sshcraft_store::session_log::read(&f.vault, &f.store.lock().unwrap(), logs[0].meta.id).unwrap();
    assert!(!bytes.is_empty());
    assert!(!bytes.windows(12).any(|w| w == b"typed-secret"), "what was typed is not in the log");
}

#[tokio::test]
async fn a_host_that_declines_is_not_recorded() {
    let f = fixture(true).await;
    set_recording(&f, Some(false), true);
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    assert!(!f.manager.is_recording(id));
    echo(&f, &sink, id, b"unrecorded").await;
    f.manager.close(id);
    assert!(logs(&f).is_empty());
    assert!(f.manager.bookmark(id, "x").is_err());
}

#[tokio::test]
async fn the_app_setting_applies_when_the_host_does_not_decide() {
    let f = fixture(true).await;
    set_recording(&f, None, true);
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    echo(&f, &sink, id, b"recorded").await;
    f.manager.close(id);
    assert_eq!(logs(&f).len(), 1);
}

#[tokio::test]
async fn nothing_is_recorded_without_the_vault() {
    let f = fixture(true).await;
    set_recording(&f, Some(true), true);
    let manager = f.manager_with(None, true);
    let host = f.agent_host();
    let sink = Arc::new(RecordingSink::default());
    let id = manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    assert!(!manager.is_recording(id));
    manager.close(id);
}

#[tokio::test]
async fn quitting_ends_running_recordings() {
    let f = fixture(true).await;
    set_recording(&f, Some(true), false);
    let sink = Arc::new(RecordingSink::default());
    let id = f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    echo(&f, &sink, id, b"before quit").await;
    f.manager.finish_recordings();
    assert!(!f.manager.is_recording(id));
    let logs = logs(&f);
    assert!(logs[0].meta.ended_at.is_some());
    f.manager.close(id);
}

mod agents {
    use super::*;
    use crate::agent_sessions::AgentSessions;

    #[tokio::test]
    async fn exec_returns_the_output() {
        let f = fixture(true).await;
        let run = AgentSessions::exec(&f.manager, f.host_id, "uptime", 1024, Duration::from_secs(10)).await.unwrap();
        assert_eq!(run.output, sshcraft_ssh::testing::TEST_EXEC_OUTPUT.as_bytes());
        assert!(!run.truncated);
        assert_eq!(run.exit_status, Some(0));
        assert!(f.server.received_commands().contains(&"uptime".to_owned()));
    }

    #[tokio::test]
    async fn exec_output_is_cut_at_the_limit() {
        let f = fixture(true).await;
        f.server.set_exec_output(&"y".repeat(5000));
        let run = AgentSessions::exec(&f.manager, f.host_id, "yes", 1000, Duration::from_secs(10)).await.unwrap();
        assert_eq!(run.output.len(), 1000);
        assert!(run.truncated);
    }

    #[tokio::test]
    async fn exec_waits_as_long_as_asked_and_reports_errors_and_status() {
        let f = fixture(true).await;
        f.server.set_exec_output("");
        f.server.set_exec_result("ls: /nope: No such file\n", 2, Duration::from_secs(6));
        let run = AgentSessions::exec(&f.manager, f.host_id, "ls /nope", 1024, Duration::from_secs(20)).await.unwrap();
        assert_eq!(run.exit_status, Some(2));
        assert!(String::from_utf8_lossy(&run.output).contains("No such file"));
        // A shorter limit than the command needs ends in a timeout.
        let short = AgentSessions::exec(&f.manager, f.host_id, "ls /nope", 1024, Duration::from_secs(3)).await;
        assert_eq!(short.unwrap_err().0, ErrorCode::Timeout);
    }

    #[tokio::test]
    async fn exec_on_an_unknown_host_fails() {
        let f = fixture(true).await;
        assert!(AgentSessions::exec(&f.manager, HostId::new(), "ls", 10, Duration::from_secs(5)).await.is_err());
    }

    #[tokio::test]
    async fn a_session_is_written_read_and_closed() {
        let f = fixture(true).await;
        let agents = AgentSessions::default();
        let id = agents.open(&f.manager, f.host_id);
        agents.write(&f.manager, id, b"hello-agent").unwrap();
        let mut seen = Vec::new();
        for _ in 0..50 {
            let read = agents.read(&f.manager, id, 1 << 20, Duration::from_millis(200)).await.unwrap();
            seen.extend(read.output);
            if seen.windows(11).any(|w| w == b"hello-agent") {
                break;
            }
        }
        assert!(seen.windows(11).any(|w| w == b"hello-agent"));
        assert!(agents.close(&f.manager, id));
        assert!(agents.read(&f.manager, id, 10, Duration::ZERO).await.is_err());
    }

    #[tokio::test]
    async fn agents_reach_only_their_own_sessions() {
        let f = fixture(true).await;
        let agents = AgentSessions::default();
        let sink = Arc::new(RecordingSink::default());
        let tab = f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink);
        assert!(agents.write(&f.manager, tab, b"rm -rf /\n").is_err());
        assert!(agents.read(&f.manager, tab, 10, Duration::ZERO).await.is_err());
        assert!(!agents.close(&f.manager, tab));
        f.manager.close(tab);
    }

    #[tokio::test]
    async fn unread_output_stays_bounded() {
        let f = fixture(true).await;
        let agents = AgentSessions::default();
        let id = agents.open(&f.manager, f.host_id);
        // Wait until connected: the first echo arrives.
        agents.write(&f.manager, id, b"x").unwrap();
        while agents.buffered(id) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let chunk = vec![b'z'; 64 * 1024];
        for _ in 0..40 {
            agents.write(&f.manager, id, &chunk).unwrap();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        let held = agents.buffered(id);
        assert!(held <= crate::sessions::OUTPUT_HIGH_WATERMARK + 2 * chunk.len(), "{held}");
        // Reading lets the rest through.
        let read = agents.read(&f.manager, id, 1 << 20, Duration::from_millis(100)).await.unwrap();
        assert!(!read.output.is_empty());
        agents.close(&f.manager, id);
    }
}

// ---------- proxy commands run only after approval on this device

/// A program that does not exist: starting it fails, which shows it was started.
fn missing_program() -> Vec<String> {
    vec!["sshcraft-no-such-program".into(), "%h".into(), "%p".into()]
}

async fn command_session(w: &PasswordWorld, server: &TestServer, pw: &str, argv: Vec<String>, answer: bool) -> (Vec<SessionStatus>, usize) {
    let host = w.host(server.port, Some(pw), Vec::new());
    w.set(host, |s| s.proxy = Some(sshcraft_model::model::Proxy::Command { argv }));
    let prompter = Arc::new(FixedPrompter { answer, asked: AtomicUsize::new(0) });
    let manager = w.manager(Some(w.vault.clone()), prompter.clone(), Arc::new(sshcraft_ssh::NoPrompter), verified());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(|s| ended(s).is_some()).await;
    (sink.statuses(), prompter.asked.load(Ordering::SeqCst))
}

#[tokio::test]
async fn a_synced_proxy_command_asks_before_it_runs() {
    let pw = format!("pw-{:016x}", rand::random::<u64>());
    let server = password_server(&pw).await;
    let w = PasswordWorld::new();
    w.trust(&server);
    let digest = sshcraft_store::store::command_digest(&missing_program());
    // Said no: nothing starts, nothing is kept.
    let (statuses, asked) = command_session(&w, &server, &pw, missing_program(), false).await;
    assert_eq!(asked, 1);
    assert_eq!(ended_code(&statuses), Some(ErrorCode::ProxyCommandNotAllowed));
    assert!(!w.store.lock().unwrap().command_approved(&digest).unwrap());
    // Said yes: the program is started (and fails, it does not exist) and the answer is kept.
    let (statuses, asked) = command_session(&w, &server, &pw, missing_program(), true).await;
    assert_eq!(asked, 1);
    assert_eq!(ended_code(&statuses), Some(ErrorCode::ProxyFailed));
    assert!(w.store.lock().unwrap().command_approved(&digest).unwrap());
}

#[tokio::test]
async fn an_approved_proxy_command_runs_without_asking() {
    let pw = format!("pw-{:016x}", rand::random::<u64>());
    let server = password_server(&pw).await;
    let w = PasswordWorld::new();
    w.trust(&server);
    w.store.lock().unwrap().approve_command(&sshcraft_store::store::command_digest(&missing_program())).unwrap();
    let (statuses, asked) = command_session(&w, &server, &pw, missing_program(), false).await;
    assert_eq!(asked, 0);
    assert_eq!(ended_code(&statuses), Some(ErrorCode::ProxyFailed));
}

#[tokio::test]
async fn a_changed_command_asks_again() {
    let pw = format!("pw-{:016x}", rand::random::<u64>());
    let server = password_server(&pw).await;
    let w = PasswordWorld::new();
    w.trust(&server);
    w.store.lock().unwrap().approve_command(&sshcraft_store::store::command_digest(&missing_program())).unwrap();
    let mut changed = missing_program();
    changed.push("--extra".into());
    let (statuses, asked) = command_session(&w, &server, &pw, changed, false).await;
    assert_eq!(asked, 1);
    assert_eq!(ended_code(&statuses), Some(ErrorCode::ProxyCommandNotAllowed));
}

// ---------- reconnect after sleep and network changes

impl Fixture {
    fn manager_scaled(&self, delay_scale: f32) -> SessionManager {
        test_manager(
            self.store.clone(),
            Some(self.vault.clone()),
            self.prompter.clone(),
            Arc::new(sshcraft_ssh::NoPrompter),
            Arc::new(TestAgents { keys: vec![self.key.clone()], reachable: true }),
            Arc::new(FakePresence::new(&[Presence::Verified])),
            delay_scale,
        )
    }

    fn set_port(&self, port: u16) {
        let store = self.store.lock().unwrap();
        let mut h = store.list_hosts().unwrap().into_iter().find(|h| h.id == self.host_id).unwrap();
        h.settings.port = Some(port);
        store.upsert_host(&h).unwrap();
    }
}

/// A TCP relay in front of the test server whose open connections can be frozen: they stay
/// open but nothing passes any more, like a connection after sleep or on a network that is gone.
struct FreezingRelay {
    port: u16,
    /// Connections numbered below this are frozen.
    frozen_below: Arc<std::sync::atomic::AtomicU64>,
    opened: Arc<std::sync::atomic::AtomicU64>,
}

impl FreezingRelay {
    async fn start(target: u16) -> Self {
        use std::sync::atomic::AtomicU64;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let frozen_below = Arc::new(AtomicU64::new(0));
        let opened = Arc::new(AtomicU64::new(0));
        let (frozen, count) = (frozen_below.clone(), opened.clone());
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let number = count.fetch_add(1, Ordering::SeqCst);
                let server = tokio::net::TcpStream::connect(("127.0.0.1", target)).await.unwrap();
                let (cr, cw) = client.into_split();
                let (sr, sw) = server.into_split();
                for (mut from, mut to) in [
                    (Box::new(cr) as Box<dyn tokio::io::AsyncRead + Send + Unpin>, Box::new(sw) as Box<dyn tokio::io::AsyncWrite + Send + Unpin>),
                    (Box::new(sr), Box::new(cw)),
                ] {
                    let frozen = frozen.clone();
                    tokio::spawn(async move {
                        use tokio::io::{AsyncReadExt, AsyncWriteExt};
                        let mut buf = [0u8; 16 * 1024];
                        loop {
                            let Ok(n) = from.read(&mut buf).await else { return };
                            if n == 0 {
                                return;
                            }
                            if number < frozen.load(Ordering::SeqCst) {
                                // Swallow everything, keep the socket open.
                                std::future::pending::<()>().await;
                            }
                            if to.write_all(&buf[..n]).await.is_err() {
                                return;
                            }
                        }
                    });
                }
            }
        });
        Self { port, frozen_below, opened }
    }

    fn freeze_open_connections(&self) {
        self.frozen_below.store(self.opened.load(Ordering::SeqCst), Ordering::SeqCst);
    }
}

#[tokio::test]
async fn a_dead_connection_is_found_after_wake() {
    let f = fixture(true).await;
    let relay = FreezingRelay::start(f.server.port).await;
    f.set_port(relay.port);
    let manager = f.manager_scaled(0.01);
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    relay.freeze_open_connections();
    manager.wake(crate::waker::WakeEvent::Resumed);
    // Found dead within the ping time (50 ms here), not after the 45 s of keep-alive, and
    // connected again through a new connection.
    tokio::time::timeout(Duration::from_secs(10), sink.wait_for(|s| connected_count(s) == 2)).await.expect("reconnected after the wake event");
}

#[tokio::test]
async fn a_live_connection_stays_after_wake() {
    let f = fixture(true).await;
    let manager = f.manager_scaled(0.01);
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    manager.wake(crate::waker::WakeEvent::NetworkChanged);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(connected_count(&sink.statuses()), 1);
    assert!(!sink.statuses().iter().any(|s| matches!(s, SessionStatus::Reconnecting { .. })));
}

#[tokio::test]
async fn a_wake_event_retries_a_waiting_reconnect_at_once() {
    let mut f = fixture(true).await;
    // Unscaled: the first reconnect would wait a full second, the second two.
    let manager = f.manager_scaled(100.0);
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    f.server.restart(false).await;
    sink.wait_for(|s| s.iter().any(|s| matches!(s, SessionStatus::Reconnecting { .. }))).await;
    manager.wake(crate::waker::WakeEvent::Resumed);
    tokio::time::timeout(Duration::from_secs(10), sink.wait_for(|s| connected_count(s) == 2)).await.expect("retried at once");
}

#[tokio::test]
async fn zero_reconnect_attempts_end_at_once() {
    let mut f = fixture(true).await;
    let manager = SessionManager::new(ManagerParts {
        store: f.store.clone(),
        vault: Some(f.vault.clone()),
        prompter: f.prompter.clone(),
        auth: Arc::new(sshcraft_ssh::NoPrompter),
        agent: Arc::new(TestAgents { keys: vec![f.key.clone()], reachable: true }),
        presence: Arc::new(FakePresence::new(&[Presence::Verified])),
        security: None,
        settings: Arc::new(|| {
            let mut s = crate::settings::EngineSettings::default();
            s.terminal.reconnect_attempts = 0;
            s
        }),
        runtime: tokio::runtime::Handle::current(),
        delay_scale: 0.01,
    });
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    f.server.restart(false).await;
    sink.wait_for(|s| ended(s).is_some()).await;
    assert_eq!(ended(&sink.statuses()), Some(EndReason::Lost));
    assert!(!sink.statuses().iter().any(|s| matches!(s, SessionStatus::Reconnecting { .. })));
}

// ---------- host keys kept whole, and UpdateHostKeys

#[tokio::test]
async fn the_first_key_is_stored_whole() {
    let f = fixture(true).await;
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    let keys = f.store.lock().unwrap().host_keys("127.0.0.1", f.server.port).unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].fingerprint, f.server.host_fingerprint);
    assert_eq!(keys[0].algorithm.as_deref(), Some("ssh-ed25519"));
    assert!(keys[0].public_key.as_deref().is_some_and(|k| k.starts_with("AAAA")));
    assert!(keys[0].added_at > 0);
}

#[tokio::test]
async fn a_key_known_by_fingerprint_gains_its_key_on_connect() {
    let f = fixture(true).await;
    f.store.lock().unwrap().set_known_host("127.0.0.1", f.server.port, &f.server.host_fingerprint).unwrap();
    let sink = Arc::new(RecordingSink::default());
    f.manager.open(SessionTarget::Host { host_id: f.host_id }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1).await;
    assert_eq!(f.prompter.asked.load(Ordering::SeqCst), 0);
    let keys = f.store.lock().unwrap().host_keys("127.0.0.1", f.server.port).unwrap();
    assert_eq!(keys[0].algorithm.as_deref(), Some("ssh-ed25519"));
}

fn verifier_on(store: Arc<Mutex<Store>>, update_keys: bool) -> StoreVerifier {
    let (_alive, abandoned) = tokio::sync::watch::channel(());
    StoreVerifier {
        session: None,
        store,
        prompter: Arc::new(FixedPrompter { answer: false, asked: AtomicUsize::new(0) }),
        mismatch: Mutex::new(None),
        first: Mutex::new(Vec::new()),
        abandoned,
        update_keys,
    }
}

fn presented(name: &str) -> sshcraft_ssh::hostkeys::PresentedKey {
    sshcraft_ssh::hostkeys::PresentedKey { algorithm: "ssh-ed25519".into(), public_key: format!("AAAA{name}"), fingerprint: format!("SHA256:{name}") }
}

fn fingerprints(store: &Mutex<Store>) -> Vec<String> {
    store.lock().unwrap().host_keys("h", 22).unwrap().into_iter().map(|k| k.fingerprint).collect()
}

#[test]
fn keys_no_longer_announced_are_removed_and_new_ones_proven_first() {
    use sshcraft_ssh::hostkeys::AnnounceAction;
    let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
    store.lock().unwrap().set_known_host("h", 22, "SHA256:used").unwrap();
    store.lock().unwrap().set_known_host("h", 22, "SHA256:old").unwrap();
    let v = verifier_on(store.clone(), true);
    let keys = [presented("used"), presented("new")];
    assert_eq!(v.announced("h", 22, "SHA256:used", &keys), AnnounceAction::Prove(vec![presented("new")]));
    // Not proven: nothing is added (the old key still goes, the server no longer names it).
    v.apply("h", 22, "SHA256:used", &keys, &[]);
    assert!(!fingerprints(&store).contains(&"SHA256:new".to_string()));
    store.lock().unwrap().set_known_host("h", 22, "SHA256:old").unwrap();
    v.apply("h", 22, "SHA256:used", &keys, &[presented("new")]);
    let mut now = fingerprints(&store);
    now.sort();
    assert_eq!(now, vec!["SHA256:new", "SHA256:used"]);
    // Only going away: applied without proof.
    assert_eq!(v.announced("h", 22, "SHA256:used", &[presented("used")]), AnnounceAction::Apply);
}

#[test]
fn update_host_keys_off_changes_nothing() {
    use sshcraft_ssh::hostkeys::AnnounceAction;
    let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
    store.lock().unwrap().set_known_host("h", 22, "SHA256:used").unwrap();
    store.lock().unwrap().set_known_host("h", 22, "SHA256:old").unwrap();
    let v = verifier_on(store.clone(), false);
    let keys = [presented("used"), presented("new")];
    assert_eq!(v.announced("h", 22, "SHA256:used", &keys), AnnounceAction::Ignore);
    v.apply("h", 22, "SHA256:used", &keys, &[presented("new")]);
    assert_eq!(fingerprints(&store).len(), 2);
    assert!(!fingerprints(&store).contains(&"SHA256:new".to_string()));
}

// ---------- passwords from a password manager

/// A password manager that answers with a fixed password (or an error) and notes each call.
struct FakeManager {
    answer: Result<String, String>,
    calls: Mutex<Vec<(sshcraft_model::secret_ref::SecretRef, Option<String>)>>,
}

#[async_trait::async_trait]
impl crate::secrets::SecretSource for FakeManager {
    async fn fetch(&self, reference: &sshcraft_model::secret_ref::SecretRef, database_password: Option<&str>) -> Result<Zeroizing<String>, String> {
        self.calls.lock().unwrap().push((reference.clone(), database_password.map(str::to_owned)));
        self.answer.clone().map(Zeroizing::new)
    }
}

fn op_ref() -> sshcraft_model::secret_ref::SecretRef {
    sshcraft_model::secret_ref::SecretRef { manager: sshcraft_model::secret_ref::Manager::OnePassword, reference: "op://Servers/web/password".into() }
}

async fn ref_session(
    w: &PasswordWorld,
    server: &TestServer,
    reference: sshcraft_model::secret_ref::SecretRef,
    answer: Result<String, String>,
    allow: bool,
    auth: Arc<dyn sshcraft_ssh::AuthPrompter>,
) -> (Vec<SessionStatus>, Arc<FakeManager>, usize) {
    let host = w.host(server.port, None, Vec::new());
    w.set(host, |s| s.password_ref = Some(reference));
    let prompter = Arc::new(FixedPrompter { answer: allow, asked: AtomicUsize::new(0) });
    let manager = w.manager(Some(w.vault.clone()), prompter.clone(), auth, verified());
    let fake = Arc::new(FakeManager { answer, calls: Mutex::new(Vec::new()) });
    manager.set_secrets(fake.clone());
    let sink = Arc::new(RecordingSink::default());
    manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
    sink.wait_for(|s| connected_count(s) == 1 || ended(s).is_some()).await;
    (sink.statuses(), fake, prompter.asked.load(Ordering::SeqCst))
}

#[tokio::test]
async fn the_reference_is_fetched_when_connecting() {
    let pw = format!("pw-{:016x}", rand::random::<u64>());
    let server = password_server(&pw).await;
    let w = PasswordWorld::new();
    w.trust(&server);
    w.store.lock().unwrap().approve_command(&crate::secrets::approval_digest(&op_ref(), "127.0.0.1", server.port)).unwrap();
    let (statuses, fake, asked) = ref_session(&w, &server, op_ref(), Ok(pw), false, Arc::new(sshcraft_ssh::NoPrompter)).await;
    assert_eq!(connected_count(&statuses), 1, "{statuses:?}");
    assert_eq!(asked, 0);
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
}

/// A shared or synced host with a reference: the manager is not asked before the user allowed
/// it for this address (it could send another of the user's passwords to a server of the
/// sharer's choosing).
#[tokio::test]
async fn a_reference_from_elsewhere_asks_first() {
    let pw = format!("pw-{:016x}", rand::random::<u64>());
    let server = password_server(&pw).await;
    let w = PasswordWorld::new();
    w.trust(&server);
    let (statuses, fake, asked) = ref_session(&w, &server, op_ref(), Ok(pw.clone()), false, Arc::new(sshcraft_ssh::NoPrompter)).await;
    assert_eq!(asked, 1);
    assert_eq!(ended_code(&statuses), Some(ErrorCode::SecretRefNotAllowed));
    assert!(fake.calls.lock().unwrap().is_empty(), "nothing was fetched");
    // Counter-check: allowed, it is fetched and connects, and the answer is kept.
    let (statuses, fake, _) = ref_session(&w, &server, op_ref(), Ok(pw), true, Arc::new(sshcraft_ssh::NoPrompter)).await;
    assert_eq!(connected_count(&statuses), 1);
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
    assert!(w.store.lock().unwrap().command_approved(&crate::secrets::approval_digest(&op_ref(), "127.0.0.1", server.port)).unwrap());
}

#[tokio::test]
async fn a_failing_manager_ends_with_its_message() {
    let server = password_server("unused").await;
    let w = PasswordWorld::new();
    w.trust(&server);
    w.store.lock().unwrap().approve_command(&crate::secrets::approval_digest(&op_ref(), "127.0.0.1", server.port)).unwrap();
    let (statuses, _, _) = ref_session(&w, &server, op_ref(), Err("[ERROR] not signed in".into()), false, Arc::new(sshcraft_ssh::NoPrompter)).await;
    assert_eq!(ended_code(&statuses), Some(ErrorCode::SecretRefFailed));
    assert!(statuses.iter().any(|s| matches!(s, SessionStatus::Ended { detail: Some(d), .. } if d.contains("not signed in"))));
}

#[tokio::test]
async fn keepassxc_asks_the_database_password_once() {
    let pw = format!("pw-{:016x}", rand::random::<u64>());
    let server = password_server(&pw).await;
    let w = PasswordWorld::new();
    w.trust(&server);
    let kdbx = sshcraft_model::secret_ref::SecretRef {
        manager: sshcraft_model::secret_ref::Manager::KeePassXc,
        reference: "/home/me/s.kdbx#Servers/web".into(),
    };
    w.store.lock().unwrap().approve_command(&crate::secrets::approval_digest(&kdbx, "127.0.0.1", server.port)).unwrap();
    let auth = ScriptedAuth::new(&[Some("db-password")]);
    let host = w.host(server.port, None, Vec::new());
    w.set(host, |s| s.password_ref = Some(kdbx.clone()));
    let manager = w.manager(Some(w.vault.clone()), trusting(), auth.clone(), verified());
    let fake = Arc::new(FakeManager { answer: Ok(pw), calls: Mutex::new(Vec::new()) });
    manager.set_secrets(fake.clone());
    for _ in 0..2 {
        let sink = Arc::new(RecordingSink::default());
        manager.open(SessionTarget::Host { host_id: host }, 80, 24, sink.clone());
        sink.wait_for(|s| connected_count(s) == 1 || ended(s).is_some()).await;
        assert_eq!(connected_count(&sink.statuses()), 1);
    }
    assert_eq!(auth.seen().len(), 1, "asked once per run");
    let calls = fake.calls.lock().unwrap();
    assert!(calls.iter().all(|(_, db)| db.as_deref() == Some("db-password")));
}

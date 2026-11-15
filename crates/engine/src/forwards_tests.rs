//! Port forwarding through the app: start, stop, reconnect, prompts. A child of the session
//! tests, so it shares their fixtures.

use std::sync::Mutex;
use std::time::Duration;

use sshcraft_model::forward::{ForwardId, ForwardKind, ForwardRule};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::*;
use crate::forwards::{ForwardManager, ForwardSink, ForwardState, ForwardStatus};

#[derive(Default)]
struct Recorder(Mutex<Vec<ForwardStatus>>);

impl ForwardSink for Recorder {
    fn status(&self, status: ForwardStatus) {
        self.0.lock().unwrap().push(status);
    }
}

impl Recorder {
    fn states(&self) -> Vec<ForwardState> {
        self.0.lock().unwrap().iter().map(|s| s.state.clone()).collect()
    }

    async fn wait_for(&self, pred: impl Fn(&[ForwardState]) -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        while !pred(&self.states()) {
            assert!(tokio::time::Instant::now() < deadline, "timed out, states: {:?}", self.states());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

fn running_count(states: &[ForwardState]) -> usize {
    states.iter().filter(|s| matches!(s, ForwardState::Running { .. })).count()
}

async fn echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = stream.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    port
}

async fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port()
}

fn local_rule(bind_port: u16, target_port: u16) -> ForwardRule {
    ForwardRule {
        id: ForwardId::new(),
        name: "db".into(),
        kind: ForwardKind::Local,
        bind_address: "127.0.0.1".into(),
        bind_port,
        target_host: "127.0.0.1".into(),
        target_port,
    }
}

/// Puts `rule` on the host and returns its id.
fn add_rule(store: &Arc<Mutex<Store>>, host_id: HostId, rule: ForwardRule) -> ForwardId {
    let store = store.lock().unwrap();
    let mut host = store.list_hosts().unwrap().into_iter().find(|h| h.id == host_id).unwrap();
    let id = rule.id;
    host.forwards.push(rule);
    store.upsert_host(&host).unwrap();
    id
}

async fn echoes(port: u16) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)).await else {
        return false;
    };
    if stream.write_all(b"ping").await.is_err() {
        return false;
    }
    let mut back = [0u8; 4];
    matches!(tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut back)).await, Ok(Ok(_))) && &back == b"ping"
}

async fn port_free(port: u16) -> bool {
    for _ in 0..100 {
        if TcpListener::bind(("127.0.0.1", port)).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[tokio::test]
async fn local_rule_runs_and_stops() {
    let f = fixture(true).await;
    let port = free_port().await;
    let rule = add_rule(&f.store, f.host_id, local_rule(port, echo_server().await));
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(f.manager.context(), sink.clone());
    forwards.start(f.host_id, rule).unwrap();
    sink.wait_for(|s| running_count(s) == 1).await;
    assert!(matches!(sink.states()[0], ForwardState::Starting));
    assert!(echoes(port).await);
    forwards.stop(f.host_id, rule);
    sink.wait_for(|s| matches!(s.last(), Some(ForwardState::Stopped))).await;
    assert!(port_free(port).await);
    assert!(forwards.statuses().is_empty());
}

#[tokio::test]
async fn port_in_use_fails_without_retry() {
    let f = fixture(true).await;
    let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = taken.local_addr().unwrap().port();
    let rule = add_rule(&f.store, f.host_id, local_rule(port, 9));
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(f.manager.context(), sink.clone());
    forwards.start(f.host_id, rule).unwrap();
    sink.wait_for(|s| matches!(s.last(), Some(ForwardState::Failed { .. }))).await;
    match sink.states().last().unwrap() {
        ForwardState::Failed { code, .. } => assert_eq!(*code, Some(ErrorCode::PortInUse)),
        other => panic!("{other:?}"),
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!sink.states().iter().any(|s| matches!(s, ForwardState::Reconnecting { .. })));
    // The failure stays visible until the rule is started again or stopped.
    assert!(matches!(forwards.statuses()[0].state, ForwardState::Failed { .. }));
}

#[tokio::test]
async fn lost_connection_reconnects_and_listens_again() {
    let mut f = fixture(true).await;
    let port = free_port().await;
    let rule = add_rule(&f.store, f.host_id, local_rule(port, echo_server().await));
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(f.manager.context(), sink.clone());
    forwards.start(f.host_id, rule).unwrap();
    sink.wait_for(|s| running_count(s) == 1).await;
    f.server.restart(false).await;
    sink.wait_for(|s| running_count(s) == 2).await;
    assert!(sink.states().iter().any(|s| matches!(s, ForwardState::Reconnecting { .. })));
    assert!(echoes(port).await);
}

#[tokio::test]
async fn stop_while_signing_in_withdraws_the_question() {
    let server = kbd_server("42").await;
    let w = PasswordWorld::new();
    let host = w.host(server.port, None, Vec::new());
    let registry = Arc::new(PromptRegistry::new(FakeEmitter::default()));
    let manager = w.manager(Some(w.vault.clone()), trusting(), registry.clone(), verified());
    let rule = add_rule(&w.store, host, local_rule(free_port().await, 9));
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(manager.context(), sink.clone());
    forwards.start(host, rule).unwrap();
    let prompt = first_auth_question(&registry).await;
    forwards.stop(host, rule);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !registry.emitter().withdrawn.lock().unwrap().contains(&prompt.prompt_id) {
        assert!(tokio::time::Instant::now() < deadline, "not withdrawn");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn stopping_a_host_stops_its_rules() {
    let f = fixture(true).await;
    let port = free_port().await;
    let rule = add_rule(&f.store, f.host_id, local_rule(port, echo_server().await));
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(f.manager.context(), sink.clone());
    forwards.start(f.host_id, rule).unwrap();
    sink.wait_for(|s| running_count(s) == 1).await;
    forwards.stop_host(f.host_id);
    sink.wait_for(|s| matches!(s.last(), Some(ForwardState::Stopped))).await;
    assert!(port_free(port).await);
}

#[tokio::test]
async fn unknown_rule_is_not_found() {
    let f = fixture(true).await;
    let forwards = ForwardManager::new(f.manager.context(), Arc::new(Recorder::default()));
    assert_eq!(forwards.start(f.host_id, ForwardId::new()), Err(ErrorCode::NotFound));
    assert_eq!(forwards.start(HostId::new(), ForwardId::new()), Err(ErrorCode::NotFound));
}

#[tokio::test]
async fn starting_twice_keeps_one() {
    let f = fixture(true).await;
    let port = free_port().await;
    let rule = add_rule(&f.store, f.host_id, local_rule(port, echo_server().await));
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(f.manager.context(), sink.clone());
    forwards.start(f.host_id, rule).unwrap();
    forwards.start(f.host_id, rule).unwrap();
    sink.wait_for(|s| running_count(s) == 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(running_count(&sink.states()), 1);
    assert!(!sink.states().iter().any(|s| matches!(s, ForwardState::Failed { .. })));
    assert_eq!(forwards.statuses().len(), 1);
}

#[tokio::test]
async fn a_rule_removed_from_its_host_stops() {
    let f = fixture(true).await;
    let target = echo_server().await;
    let (port_a, port_b) = (free_port().await, free_port().await);
    let a = add_rule(&f.store, f.host_id, local_rule(port_a, target));
    let b = add_rule(&f.store, f.host_id, local_rule(port_b, target));
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(f.manager.context(), sink.clone());
    forwards.start(f.host_id, a).unwrap();
    forwards.start(f.host_id, b).unwrap();
    sink.wait_for(|s| running_count(s) == 2).await;
    forwards.keep_only(f.host_id, &[b]);
    assert!(port_free(port_a).await);
    assert!(echoes(port_b).await);
    assert_eq!(forwards.statuses().len(), 1);
}

/// A manager whose host key and sign-in questions both go through `registry`.
fn asking_manager(w: &PasswordWorld, registry: &Arc<PromptRegistry<FakeEmitter>>) -> SessionManager {
    test_manager(
        w.store.clone(),
        Some(w.vault.clone()),
        registry.clone(),
        registry.clone(),
        Arc::new(TestAgents { keys: Vec::new(), reachable: false }),
        verified(),
        0.01,
    )
}

async fn wait_withdrawn(registry: &PromptRegistry<FakeEmitter>, prompt_id: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !registry.emitter().withdrawn.lock().unwrap().contains(&prompt_id) {
        assert!(tokio::time::Instant::now() < deadline, "not withdrawn");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn stop_while_asking_for_the_host_key_withdraws_it() {
    let server = kbd_server("42").await;
    let w = PasswordWorld::new();
    let host = w.host(server.port, None, Vec::new());
    let registry = Arc::new(PromptRegistry::new(FakeEmitter::default()));
    let manager = asking_manager(&w, &registry);
    let rule = add_rule(&w.store, host, local_rule(free_port().await, 9));
    let forwards = ForwardManager::new(manager.context(), Arc::new(Recorder::default()));
    forwards.start(host, rule).unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let asked = loop {
        if let Some(id) = registry.emitter().asked.lock().unwrap().first() {
            break *id;
        }
        assert!(tokio::time::Instant::now() < deadline, "no host key question");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    forwards.stop(host, rule);
    wait_withdrawn(&registry, asked).await;
    // Nothing was trusted for the stopped rule.
    assert!(w.store.lock().unwrap().known_host("127.0.0.1", server.port).unwrap().is_none());
}

#[tokio::test]
async fn cancelled_sign_in_ends_as_stopped() {
    let server = kbd_server("42").await;
    let w = PasswordWorld::new();
    let host = w.host(server.port, None, Vec::new());
    let registry = Arc::new(PromptRegistry::new(FakeEmitter::default()));
    let manager = w.manager(Some(w.vault.clone()), trusting(), registry.clone(), verified());
    let rule = add_rule(&w.store, host, local_rule(free_port().await, 9));
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(manager.context(), sink.clone());
    forwards.start(host, rule).unwrap();
    let prompt = first_auth_question(&registry).await;
    registry.answer_auth(prompt.prompt_id, None);
    sink.wait_for(|s| matches!(s.last(), Some(ForwardState::Stopped))).await;
    assert!(!sink.states().iter().any(|s| matches!(s, ForwardState::Failed { .. })));
    assert!(forwards.statuses().is_empty());
}

#[test]
fn a_refused_tunnel_is_retried_once_the_rule_ran() {
    use crate::forwards::tunnel_failure;
    use crate::sessions::Failure;
    for error in [SshError::ForwardRefused, SshError::PortInUse] {
        let first = tunnel_failure(clone_error(&error), false);
        assert!(matches!(first, Failure::End(..)), "{error:?} at the first start");
        let later = tunnel_failure(error, true);
        assert!(matches!(later, Failure::Lost(..)), "after it ran");
    }
}

fn clone_error(e: &SshError) -> SshError {
    match e {
        SshError::ForwardRefused => SshError::ForwardRefused,
        _ => SshError::PortInUse,
    }
}

#[tokio::test]
async fn remote_rule_comes_back_after_the_server_restarts() {
    let mut f = fixture(true).await;
    let port = free_port().await;
    let rule = ForwardRule { kind: ForwardKind::Remote, ..local_rule(port, echo_server().await) };
    let rule = add_rule(&f.store, f.host_id, rule);
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(f.manager.context(), sink.clone());
    forwards.start(f.host_id, rule).unwrap();
    sink.wait_for(|s| running_count(s) == 1).await;
    assert!(echoes(port).await);
    f.server.restart(false).await;
    sink.wait_for(|s| running_count(s) == 2).await;
    // Right after the restart a connection may still reach the old in-process server's listener,
    // which is going away: the rule counts as back once it echoes within a few seconds.
    assert!(echoes_soon(port).await);
}

async fn echoes_soon(port: u16) -> bool {
    for _ in 0..25 {
        if echoes(port).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

#[tokio::test]
async fn reconcile_stops_rules_whose_host_or_rule_is_gone() {
    let f = fixture(true).await;
    let target = echo_server().await;
    let (port_a, port_b) = (free_port().await, free_port().await);
    let a = add_rule(&f.store, f.host_id, local_rule(port_a, target));
    let b = add_rule(&f.store, f.host_id, local_rule(port_b, target));
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(f.manager.context(), sink.clone());
    forwards.start(f.host_id, a).unwrap();
    forwards.start(f.host_id, b).unwrap();
    sink.wait_for(|s| running_count(s) == 2).await;
    // Rule a goes (as by sync), then the whole host (as by deleting its group).
    {
        let store = f.store.lock().unwrap();
        let mut host = store.list_hosts().unwrap().remove(0);
        host.forwards.retain(|r| r.id != a);
        store.upsert_host(&host).unwrap();
    }
    forwards.reconcile();
    assert!(port_free(port_a).await);
    assert!(echoes(port_b).await);
    f.store.lock().unwrap().delete_host(f.host_id).unwrap();
    forwards.reconcile();
    assert!(port_free(port_b).await);
    assert!(forwards.statuses().is_empty());
}

#[tokio::test]
async fn same_rule_id_on_two_hosts_runs_twice() {
    let f = fixture(true).await;
    let target = echo_server().await;
    let rule = local_rule(free_port().await, target);
    let shared_id = add_rule(&f.store, f.host_id, rule.clone());
    let other = {
        let store = f.store.lock().unwrap();
        let mut copy = store.list_hosts().unwrap().remove(0);
        copy.id = HostId::new();
        copy.name = "copy".into();
        copy.forwards = vec![ForwardRule { bind_port: free_port_blocking(), ..rule }];
        store.upsert_host(&copy).unwrap();
        copy.id
    };
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(f.manager.context(), sink.clone());
    forwards.start(f.host_id, shared_id).unwrap();
    forwards.start(other, shared_id).unwrap();
    sink.wait_for(|s| running_count(s) == 2).await;
    assert_eq!(forwards.statuses().len(), 2);
    forwards.stop(other, shared_id);
    assert_eq!(forwards.statuses().len(), 1);
}

fn free_port_blocking() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[tokio::test]
async fn stopping_a_failed_rule_reports_stopped() {
    let f = fixture(true).await;
    let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = taken.local_addr().unwrap().port();
    let rule = add_rule(&f.store, f.host_id, local_rule(port, 9));
    let sink = Arc::new(Recorder::default());
    let forwards = ForwardManager::new(f.manager.context(), sink.clone());
    forwards.start(f.host_id, rule).unwrap();
    sink.wait_for(|s| matches!(s.last(), Some(ForwardState::Failed { .. }))).await;
    forwards.stop(f.host_id, rule);
    assert!(matches!(sink.states().last(), Some(ForwardState::Stopped)));
    assert!(forwards.statuses().is_empty());
}

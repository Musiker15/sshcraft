mod common;

use common::{Dev, TestServer, create, join, sample_host};
use sshcraft_core::model::ServerId;
use sshcraft_core::store::{ItemKind, Store};
use sshcraft_core::vault::Vault;
use sshcraft_sync::devices::revoke_device;
use sshcraft_sync::engine::{Engine, record_local_change};
use sshcraft_sync::runtime::{SyncObserver, SyncRuntime, SyncStatus};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Status(SyncStatus),
    Changed,
    Warning(String),
}

struct Watcher(mpsc::UnboundedSender<Seen>);

impl SyncObserver for Watcher {
    fn status(&self, _server: ServerId, status: SyncStatus) {
        let _ = self.0.send(Seen::Status(status));
    }
    fn data_changed(&self, _server: ServerId) {
        let _ = self.0.send(Seen::Changed);
    }
    fn warning(&self, _server: ServerId, message: String) {
        let _ = self.0.send(Seen::Warning(message));
    }
}

/// A device whose store and vault can be shared with a runtime.
struct Shared {
    store: Arc<Mutex<Store>>,
    vault: Arc<Vault>,
}

impl Shared {
    fn from(dev: Dev) -> Self {
        Self {
            store: Arc::new(dev.store),
            vault: Arc::new(dev.vault),
        }
    }

    fn server(&self) -> ServerId {
        self.store.lock().unwrap().servers().unwrap()[0].id
    }

    fn start(&self) -> (SyncRuntime, mpsc::UnboundedReceiver<Seen>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let runtime = SyncRuntime::start(
            self.store.clone(),
            self.vault.clone(),
            self.server(),
            Arc::new(Watcher(tx)),
        );
        (runtime, rx)
    }

    fn add_host(&self, name: &str) {
        let server = self.server();
        let store = self.store.lock().unwrap();
        let mut host = sample_host(name);
        host.location = Some(server);
        store.upsert_host(&host).unwrap();
        record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 1000).unwrap();
    }

    fn host_names(&self) -> Vec<String> {
        self.store
            .lock()
            .unwrap()
            .list_hosts()
            .unwrap()
            .into_iter()
            .map(|h| h.name)
            .collect()
    }
}

async fn wait_for(rx: &mut mpsc::UnboundedReceiver<Seen>, want: impl Fn(&Seen) -> bool, secs: u64) {
    tokio::time::timeout(Duration::from_secs(secs), async {
        while let Some(seen) = rx.recv().await {
            if want(&seen) {
                return;
            }
        }
        panic!("observer closed");
    })
    .await
    .expect("expected event did not come");
}

async fn pair(server: &TestServer) -> (Shared, Shared) {
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(server, &a, "a@example.org").await;
    join(server, &b, &export).await;
    (Shared::from(a), Shared::from(b))
}

#[tokio::test]
async fn change_on_a_reaches_b_live() {
    let server = TestServer::start().await;
    let (a, b) = pair(&server).await;
    let (ra, mut seen_a) = a.start();
    let (rb, mut seen_b) = b.start();
    wait_for(&mut seen_a, |s| *s == Seen::Status(SyncStatus::Idle), 5).await;
    wait_for(&mut seen_b, |s| *s == Seen::Status(SyncStatus::Idle), 5).await;
    a.add_host("live");
    ra.nudge();
    wait_for(&mut seen_b, |s| *s == Seen::Changed, 5).await;
    assert_eq!(b.host_names(), vec!["live"]);
    ra.stop();
    rb.stop();
}

#[tokio::test]
async fn server_restart_reconnects() {
    let mut server = TestServer::start().await;
    let (a, b) = pair(&server).await;
    let (rb, mut seen_b) = b.start();
    wait_for(&mut seen_b, |s| *s == Seen::Status(SyncStatus::Idle), 5).await;
    server.stop().await;
    wait_for(
        &mut seen_b,
        |s| matches!(s, Seen::Status(SyncStatus::Offline { .. })),
        5,
    )
    .await;
    server.restart().await;
    wait_for(&mut seen_b, |s| *s == Seen::Status(SyncStatus::Idle), 15).await;

    a.add_host("after-restart");
    let mut engine = Engine::load(&a.store, &a.vault, a.server()).await.unwrap();
    engine.push(&a.store, &a.vault, 0).await.unwrap();
    wait_for(&mut seen_b, |s| *s == Seen::Changed, 5).await;
    assert_eq!(b.host_names(), vec!["after-restart"]);
    rb.stop();
}

#[tokio::test]
async fn revocation_ends_the_runtime() {
    let server = TestServer::start().await;
    let (a, b) = pair(&server).await;
    let (rb, mut seen_b) = b.start();
    wait_for(&mut seen_b, |s| *s == Seen::Status(SyncStatus::Idle), 5).await;
    let target = b.store.lock().unwrap().servers().unwrap()[0].device_id;
    let mut engine = Engine::load(&a.store, &a.vault, a.server()).await.unwrap();
    revoke_device(&mut engine, &a.store, &a.vault, target, 10)
        .await
        .unwrap();
    wait_for(&mut seen_b, |s| *s == Seen::Status(SyncStatus::Revoked), 5).await;
    assert!(b.store.lock().unwrap().servers().unwrap()[0].revoked);
    rb.stop();
}

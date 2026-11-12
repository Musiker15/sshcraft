mod common;

use common::{Dev, TestServer, create, join, sample_host};
use sshcraft_core::model::{Host, ServerId};
use sshcraft_core::store::ItemKind;
use sshcraft_sync::SyncError;
use sshcraft_sync::engine::{Engine, record_local_change};

async fn engine(dev: &Dev) -> Engine {
    Engine::load(&dev.store, &dev.vault, dev.row().id)
        .await
        .unwrap()
}

async fn sync(dev: &Dev, now: i64) {
    let mut e = engine(dev).await;
    e.pull(&dev.store, &dev.vault).await.unwrap();
    e.push(&dev.store, &dev.vault, now).await.unwrap();
}

/// Saves a host at the server's location and queues it, as the app does.
fn save(dev: &Dev, host: &Host, at: i64) {
    let server: ServerId = dev.row().id;
    let store = dev.store.lock().unwrap();
    let mut host = host.clone();
    host.location = Some(server);
    store.upsert_host(&host).unwrap();
    record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), at).unwrap();
}

fn rename(dev: &Dev, host: &Host, name: &str, at: i64) {
    let mut h = host.clone();
    h.name = name.into();
    save(dev, &h, at);
}

fn name_on(dev: &Dev) -> Vec<String> {
    dev.hosts().into_iter().map(|h| h.name).collect()
}

/// Two devices that both have host `web`.
async fn pair(server: &TestServer) -> (Dev, Dev, Host) {
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(server, &a, "a@example.org").await;
    join(server, &b, &export).await;
    let host = sample_host("web");
    save(&a, &host, 100);
    sync(&a, 100).await;
    sync(&b, 100).await;
    assert_eq!(name_on(&b), vec!["web"]);
    (a, b, host)
}

#[tokio::test]
async fn local_change_reaches_the_other_device() {
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    rename(&a, &host, "renamed", 200);
    let mut e = engine(&a).await;
    assert_eq!(e.push(&a.store, &a.vault, 200).await.unwrap(), 1);
    assert!(outbox_len(&a) == 0);
    sync(&b, 300).await;
    assert_eq!(name_on(&b), vec!["renamed"]);
}

#[tokio::test]
async fn offline_change_stays_queued() {
    let mut server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    let mut e = engine(&a).await;
    server.stop().await;
    rename(&a, &host, "offline", 200);
    assert!(matches!(
        e.push(&a.store, &a.vault, 200).await,
        Err(SyncError::Network(_))
    ));
    assert_eq!(outbox_len(&a), 1);
    server.restart().await;
    sync(&a, 300).await;
    assert!(outbox_len(&a) == 0);
    sync(&b, 300).await;
    assert_eq!(name_on(&b), vec!["offline"]);
}

#[tokio::test]
async fn concurrent_edits_converge_on_the_newer() {
    for a_first in [true, false] {
        let server = TestServer::start().await;
        let (a, b, host) = pair(&server).await;
        rename(&a, &host, "from-a", 1000);
        rename(&b, &host, "from-b", 2000);
        let (first, second) = if a_first { (&a, &b) } else { (&b, &a) };
        sync(first, 3000).await;
        sync(second, 3000).await;
        sync(first, 3000).await;
        sync(second, 3000).await;
        assert_eq!(name_on(&a), vec!["from-b"], "a_first = {a_first}");
        assert_eq!(name_on(&b), vec!["from-b"], "a_first = {a_first}");
    }
}

#[tokio::test]
async fn delete_wins_over_older_edit() {
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    rename(&b, &host, "edited", 2000);
    {
        let store = a.store.lock().unwrap();
        record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 3000).unwrap();
        store.delete_host(host.id).unwrap();
    }
    sync(&b, 4000).await;
    sync(&a, 4000).await;
    sync(&b, 4000).await;
    assert!(a.hosts().is_empty());
    assert!(b.hosts().is_empty());
}

#[tokio::test]
async fn keys_only_sync_when_enabled() {
    let server = TestServer::start().await;
    let (a, b, _) = pair(&server).await;
    let server_id = a.row().id;
    let key = {
        let store = a.store.lock().unwrap();
        let info = a.vault.generate_ed25519(&store, "deploy").unwrap();
        store
            .set_location(ItemKind::Key, &info.id.0.to_string(), Some(server_id))
            .unwrap();
        record_local_change(&store, ItemKind::Key, &info.id.0.to_string(), 500).unwrap();
        info
    };
    let mut e = engine(&a).await;
    assert_eq!(
        e.push(&a.store, &a.vault, 500).await.unwrap(),
        0,
        "key sync is off"
    );
    assert!(
        a.store
            .lock()
            .unwrap()
            .outbox(server_id)
            .unwrap()
            .is_empty()
    );

    for dev in [&a, &b] {
        let id = dev.row().id;
        dev.store
            .lock()
            .unwrap()
            .update_server_flags(id, true, true)
            .unwrap();
    }
    record_local_change(
        &a.store.lock().unwrap(),
        ItemKind::Key,
        &key.id.0.to_string(),
        600,
    )
    .unwrap();
    assert_eq!(e.push(&a.store, &a.vault, 600).await.unwrap(), 1);
    sync(&b, 700).await;
    let got = b.store.lock().unwrap().key_record(key.id).unwrap().unwrap();
    assert_eq!(got.fingerprint, key.fingerprint);
}

/// Number of queued changes (takes the server id before locking the store).
fn outbox_len(dev: &Dev) -> usize {
    let id = dev.row().id;
    dev.store.lock().unwrap().outbox(id).unwrap().len()
}

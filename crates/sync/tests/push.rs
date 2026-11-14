mod common;

use common::{Dev, TestServer, create, join, sample_host};
use sshcraft_model::model::{Host, ServerId};
use sshcraft_store::store::ItemKind;
use sshcraft_sync::SyncError;
use sshcraft_sync::engine::{Engine, record_local_change};

async fn engine(dev: &Dev) -> Engine {
    Engine::load(&dev.store, &dev.vault, dev.row().id).await.unwrap()
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
    assert!(matches!(e.push(&a.store, &a.vault, 200).await, Err(SyncError::Network(_))));
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
        store.set_location(ItemKind::Key, &info.id.0.to_string(), Some(server_id)).unwrap();
        record_local_change(&store, ItemKind::Key, &info.id.0.to_string(), 500).unwrap();
        info
    };
    let mut e = engine(&a).await;
    assert_eq!(e.push(&a.store, &a.vault, 500).await.unwrap(), 0, "key sync is off");
    assert!(a.store.lock().unwrap().outbox(server_id).unwrap().is_empty());

    for dev in [&a, &b] {
        let id = dev.row().id;
        dev.store.lock().unwrap().update_server_flags(id, true, true).unwrap();
    }
    record_local_change(&a.store.lock().unwrap(), ItemKind::Key, &key.id.0.to_string(), 600).unwrap();
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

#[tokio::test]
async fn moving_an_item_off_the_server_deletes_it_there() {
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    {
        let store = a.store.lock().unwrap();
        record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 500).unwrap();
        store.set_location(ItemKind::Host, &host.id.0.to_string(), None).unwrap();
    }
    sync(&a, 600).await;
    sync(&b, 600).await;
    sync(&a, 700).await;
    assert!(b.hosts().is_empty(), "gone from the server");
    assert_eq!(name_on(&a), vec!["web"], "still on the device that moved it");
}

#[tokio::test]
async fn renaming_a_group_keeps_a_newer_host_edit() {
    use sshcraft_model::model::{ConnectionSettings, Group, GroupId};
    let server = TestServer::start().await;
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &b, &export).await;
    let server_id = a.row().id;
    let group = Group {
        id: GroupId::new(),
        name: "srv".into(),
        defaults: ConnectionSettings::default(),
        location: Some(server_id),
        parent_id: None,
        tags: Vec::new(),
    };
    let mut host = sample_host("web");
    host.group_id = Some(group.id);
    {
        let store = a.store.lock().unwrap();
        store.upsert_group(&group).unwrap();
        record_local_change(&store, ItemKind::Group, &group.id.0.to_string(), 100).unwrap();
    }
    save(&a, &host, 100);
    sync(&a, 100).await;
    sync(&b, 100).await;
    assert_eq!(name_on(&b), vec!["web"]);

    // B edits the host (offline, at 150); A renames the group later (200).
    rename(&b, &host, "edited-on-b", 150);
    {
        let store = a.store.lock().unwrap();
        let mut renamed = group.clone();
        renamed.name = "renamed".into();
        store.upsert_group(&renamed).unwrap();
        record_local_change(&store, ItemKind::Group, &group.id.0.to_string(), 200).unwrap();
    }
    sync(&a, 300).await;
    sync(&b, 300).await;
    sync(&a, 300).await;
    assert_eq!(name_on(&a), vec!["edited-on-b"]);
    assert_eq!(name_on(&b), vec!["edited-on-b"]);
}

#[tokio::test]
async fn enabling_key_sync_fetches_keys_skipped_before() {
    let server = TestServer::start().await;
    let (a, b, _) = pair(&server).await;
    let a_id = a.row().id;
    a.store.lock().unwrap().update_server_flags(a_id, true, true).unwrap();
    let key = {
        let store = a.store.lock().unwrap();
        let info = a.vault.generate_ed25519(&store, "deploy").unwrap();
        store.set_location(ItemKind::Key, &info.id.0.to_string(), Some(a_id)).unwrap();
        record_local_change(&store, ItemKind::Key, &info.id.0.to_string(), 500).unwrap();
        info
    };
    sync(&a, 500).await;
    sync(&b, 500).await;
    assert!(b.store.lock().unwrap().key_record(key.id).unwrap().is_none(), "key sync is off on B");
    let b_id = b.row().id;
    b.store.lock().unwrap().update_server_flags(b_id, true, true).unwrap();
    sync(&b, 600).await;
    assert!(b.store.lock().unwrap().key_record(key.id).unwrap().is_some());
}

fn server_of(dev: &Dev, server: &TestServer) -> ServerId {
    dev.store.lock().unwrap().servers().unwrap().into_iter().find(|r| r.origin == server.origin).unwrap().id
}

async fn sync_on(dev: &Dev, server: ServerId) {
    let mut e = Engine::load(&dev.store, &dev.vault, server).await.unwrap();
    e.pull(&dev.store, &dev.vault).await.unwrap();
    e.push(&dev.store, &dev.vault, 0).await.unwrap();
}

#[tokio::test]
async fn moving_between_two_servers_keeps_the_item_on_a_device_with_both() {
    for y_first in [true, false] {
        let (x, y) = (TestServer::start().await, TestServer::start().await);
        let (a, b) = (Dev::new(), Dev::new());
        let ex = create(&x, &a, "a@example.org").await;
        let ey = create(&y, &a, "a@example.org").await;
        join(&x, &b, &ex).await;
        join(&y, &b, &ey).await;
        let (ax, ay) = (server_of(&a, &x), server_of(&a, &y));
        let (bx, by) = (server_of(&b, &x), server_of(&b, &y));

        let mut host = sample_host("web");
        host.location = Some(ax);
        let id = host.id.0.to_string();
        {
            let store = a.store.lock().unwrap();
            store.upsert_host(&host).unwrap();
            record_local_change(&store, ItemKind::Host, &id, 100).unwrap();
        }
        sync_on(&a, ax).await;
        sync_on(&b, bx).await;
        assert_eq!(name_on(&b), vec!["web"]);

        // A moves the host from X to Y: X learns it is gone, Y gets it.
        {
            let store = a.store.lock().unwrap();
            record_local_change(&store, ItemKind::Host, &id, 500).unwrap();
            store.set_location(ItemKind::Host, &id, Some(ay)).unwrap();
            record_local_change(&store, ItemKind::Host, &id, 500).unwrap();
        }
        sync_on(&a, ax).await;
        sync_on(&a, ay).await;
        if y_first {
            sync_on(&b, by).await;
            sync_on(&b, bx).await;
        } else {
            sync_on(&b, bx).await;
            sync_on(&b, by).await;
        }
        let hosts = b.hosts();
        assert_eq!(hosts.len(), 1, "y_first = {y_first}");
        assert_eq!(hosts[0].location, Some(by), "y_first = {y_first}");
    }
}

#[tokio::test]
async fn unsynced_key_assignments_stay_per_device() {
    use sshcraft_model::model::KeyRef;
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    // Key sync is off: each device assigns a key of its own to the same host.
    let assign = |dev: &Dev, name: &str, at: i64| {
        let id = dev.store.lock().unwrap().list_hosts().unwrap()[0].id;
        let key = {
            let store = dev.store.lock().unwrap();
            dev.vault.generate_ed25519(&store, name).unwrap().id
        };
        let mut h = host.clone();
        h.id = id;
        h.settings.key = Some(KeyRef::Stored { id: key });
        save(dev, &h, at);
        key
    };
    let ka = assign(&a, "a-key", 200);
    sync(&a, 200).await;
    let kb = assign(&b, "b-key", 300);
    sync(&b, 300).await;
    sync(&a, 400).await;
    rename(&a, &a.hosts()[0], "renamed", 500);
    sync(&a, 500).await;
    sync(&b, 600).await;
    let key_on = |dev: &Dev| dev.hosts()[0].settings.key.clone();
    assert_eq!(key_on(&a), Some(KeyRef::Stored { id: ka }));
    assert_eq!(key_on(&b), Some(KeyRef::Stored { id: kb }));
    assert_eq!(name_on(&b), vec!["renamed"]);
}

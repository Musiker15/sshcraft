mod common;

use common::{Dev, TestServer, create, engine, inject_record, member, sample_host, seal_in, sync};
use sshcraft_core::store::ItemKind;
use sshcraft_proto::area::Role;
use sshcraft_sync::areas::{create_area, set_members};
use sshcraft_sync::engine::record_local_change;
use sshcraft_sync::items::ItemBody;
use uuid::Uuid;

/// A, B and C on one server; A owns an area shared with B and C.
async fn three(server: &TestServer) -> (Dev, Dev, Dev) {
    let (a, b, c) = (Dev::new(), Dev::new(), Dev::new());
    create(server, &a, "a@example.org").await;
    create(server, &b, "b@example.org").await;
    create(server, &c, "c@example.org").await;
    (a, b, c)
}

#[tokio::test]
async fn member_receives_a_shared_area() {
    let server = TestServer::start().await;
    let (a, b, c) = three(&server).await;
    let e = engine(&a).await;
    let (manifest, _) = create_area(
        &e,
        &a.store,
        vec![member(&b, Role::Read), member(&c, Role::Write)],
    )
    .await
    .unwrap();
    let host = sample_host("web");
    let record = seal_in(
        &a,
        manifest.area_id.0,
        ItemBody::Host { host: host.clone() },
        Uuid::new_v4(),
        1,
        10,
    );
    a.client(&server).await.put_record(&record).await.unwrap();
    sync(&b).await;
    let hosts = b.hosts();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "web");
    let server_b = b.row().id;
    let store = b.store.lock().unwrap();
    let id = host.id.0.to_string();
    assert!(store.read_only(server_b, ItemKind::Host, &id).unwrap());
    assert_eq!(
        store.effective_area(server_b, ItemKind::Host, &id).unwrap(),
        Some(manifest.area_id.0)
    );
}

#[tokio::test]
async fn record_from_a_reader_is_refused() {
    let server = TestServer::start().await;
    let (a, b, c) = three(&server).await;
    let e = engine(&a).await;
    let (manifest, _) = create_area(
        &e,
        &a.store,
        vec![member(&b, Role::Read), member(&c, Role::Write)],
    )
    .await
    .unwrap();
    sync(&b).await;
    let forged = seal_in(
        &b,
        manifest.area_id.0,
        ItemBody::Host {
            host: sample_host("evil"),
        },
        Uuid::new_v4(),
        1,
        10,
    );
    // The server refuses it ...
    let refused = b.client(&server).await.put_record(&forged).await;
    assert_eq!(refused.unwrap_err().server_code(), Some("forbidden"));
    // ... and a server that serves it anyway does not get it applied.
    inject_record(&server, &forged);
    let mut ec = engine(&c).await;
    let report = ec.pull(&c.store, &c.vault).await.unwrap();
    assert!(c.hosts().is_empty());
    assert!(
        report
            .rejected
            .iter()
            .any(|r| r.contains(&forged.header.record_id.to_string())),
        "{report:?}"
    );
}

#[tokio::test]
async fn removed_member_loses_the_items() {
    let server = TestServer::start().await;
    let (a, b, c) = three(&server).await;
    let e = engine(&a).await;
    let (manifest, _) = create_area(
        &e,
        &a.store,
        vec![member(&b, Role::Read), member(&c, Role::Read)],
    )
    .await
    .unwrap();
    let record = seal_in(
        &a,
        manifest.area_id.0,
        ItemBody::Host {
            host: sample_host("web"),
        },
        Uuid::new_v4(),
        1,
        10,
    );
    a.client(&server).await.put_record(&record).await.unwrap();
    sync(&b).await;
    assert_eq!(b.hosts().len(), 1);
    set_members(
        &e,
        &a.store,
        &a.vault,
        manifest.area_id,
        vec![member(&c, Role::Read)],
        20,
    )
    .await
    .unwrap();
    sync(&b).await;
    assert!(b.hosts().is_empty());
    let server_b = b.row().id;
    assert!(b.store.lock().unwrap().shares(server_b).unwrap().is_empty());
    // C keeps reading.
    sync(&c).await;
    assert_eq!(c.hosts().len(), 1);
}

fn sample_group(name: &str) -> sshcraft_core::model::Group {
    sshcraft_core::model::Group {
        id: sshcraft_core::model::GroupId::new(),
        name: name.into(),
        defaults: Default::default(),
        location: None,
    }
}

/// A shares group G with host H with B (write) and C (read); everyone has synced.
async fn shared_group(server: &TestServer) -> (Dev, Dev, Dev, Uuid, sshcraft_core::model::Host) {
    let (a, b, c) = three(server).await;
    let e = engine(&a).await;
    let (manifest, _) = create_area(
        &e,
        &a.store,
        vec![member(&b, Role::Write), member(&c, Role::Read)],
    )
    .await
    .unwrap();
    let area = manifest.area_id.0;
    let group = sample_group("team");
    let mut host = sample_host("web");
    host.group_id = Some(group.id);
    let client = a.client(server).await;
    let g = seal_in(&a, area, ItemBody::Group { group }, Uuid::new_v4(), 1, 10);
    client.put_record(&g).await.unwrap();
    let h = seal_in(
        &a,
        area,
        ItemBody::Host { host: host.clone() },
        Uuid::new_v4(),
        1,
        10,
    );
    client.put_record(&h).await.unwrap();
    for dev in [&a, &b, &c] {
        sync(dev).await;
    }
    assert_eq!(b.hosts().len(), 1);
    (a, b, c, area, host)
}

fn edit_host(dev: &Dev, host: &sshcraft_core::model::Host, name: &str, at: i64) {
    let server = dev.row().id;
    let store = dev.store.lock().unwrap();
    let mut host = store
        .list_hosts()
        .unwrap()
        .into_iter()
        .find(|h| h.id == host.id)
        .unwrap();
    host.name = name.into();
    store.upsert_host(&host).unwrap();
    let _ = server;
    record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), at).unwrap();
}

#[tokio::test]
async fn writer_changes_reach_the_owner() {
    let server = TestServer::start().await;
    let (a, b, _c, _, host) = shared_group(&server).await;
    edit_host(&b, &host, "web-renamed", 100);
    sync(&b).await;
    sync(&a).await;
    assert_eq!(a.hosts()[0].name, "web-renamed");
}

#[tokio::test]
async fn reader_edit_is_rolled_back() {
    let server = TestServer::start().await;
    let (_a, _b, c, _, host) = shared_group(&server).await;
    edit_host(&c, &host, "mine-now", 100);
    let mut e = engine(&c).await;
    assert_eq!(e.push(&c.store, &c.vault, 100).await.unwrap(), 0);
    assert_eq!(c.hosts()[0].name, "web");
    assert!(
        e.warnings().iter().any(|w| w == "share_read_only"),
        "{:?}",
        e.warnings()
    );
    let server_c = c.row().id;
    assert!(c.store.lock().unwrap().outbox(server_c).unwrap().is_empty());
}

#[tokio::test]
async fn selection_member_cannot_add_or_delete() {
    let server = TestServer::start().await;
    let (_a, b, _c, _, host) = shared_group(&server).await;
    let server_b = b.row().id;
    // Deleting the shared host.
    {
        let store = b.store.lock().unwrap();
        record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 100).unwrap();
        store.delete_host(host.id).unwrap();
    }
    // Adding a host to the shared group.
    let mut extra = sample_host("extra");
    extra.group_id = host.group_id;
    extra.location = Some(server_b);
    {
        let store = b.store.lock().unwrap();
        store.upsert_host(&extra).unwrap();
        record_local_change(&store, ItemKind::Host, &extra.id.0.to_string(), 101).unwrap();
    }
    let mut e = engine(&b).await;
    assert_eq!(e.push(&b.store, &b.vault, 101).await.unwrap(), 0);
    let names: Vec<String> = b.hosts().into_iter().map(|h| h.name).collect();
    assert_eq!(names, vec!["web".to_owned()]);
    assert!(
        e.warnings().iter().any(|w| w == "share_no_add_remove"),
        "{:?}",
        e.warnings()
    );
}

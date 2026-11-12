mod common;

use common::{Dev, TestServer, create, engine, inject_record, member, sample_host, seal_in, sync};
use sshcraft_core::store::ItemKind;
use sshcraft_proto::area::Role;
use sshcraft_sync::areas::{create_area, set_members};
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

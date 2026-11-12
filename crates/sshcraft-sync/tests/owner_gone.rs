mod common;

use common::{Dev, TestServer, account, create, engine, server_sql, stored_area, sync};
use sshcraft_core::model::{HostId, KeyRef};
use sshcraft_core::store::ItemKind;
use sshcraft_proto::area::{AreaId, AreaManifest, Member, Role};
use sshcraft_sync::contacts::{confirm, find};
use sshcraft_sync::engine::record_local_change;
use sshcraft_sync::shares::{Grant, ShareWhat, share};
use uuid::Uuid;

async fn befriend(a: &Dev, email: &str) {
    let e = engine(a).await;
    let found = find(&e, &a.store, email).await.unwrap();
    confirm(&a.store, a.row().id, email, &found.account).unwrap();
}

fn rename(dev: &Dev, id: HostId, name: &str, at: i64) {
    let store = dev.store.lock().unwrap();
    let mut host = store
        .list_hosts()
        .unwrap()
        .into_iter()
        .find(|h| h.id == id)
        .unwrap();
    host.name = name.into();
    store.upsert_host(&host).unwrap();
    record_local_change(&store, ItemKind::Host, &id.0.to_string(), at).unwrap();
}

/// A shares host "one" with B (write) and C (read); everyone synced.
async fn setup(server: &TestServer) -> (Dev, Dev, Dev, Uuid, String) {
    let (a, b, c) = (Dev::new(), Dev::new(), Dev::new());
    create(server, &a, "a@example.org").await;
    let export_b = create(server, &b, "b@example.org").await;
    create(server, &c, "c@example.org").await;
    let server_a = a.row().id;
    let mut one = common::sample_host("one");
    one.location = Some(server_a);
    one.settings.key = None::<KeyRef>;
    {
        let store = a.store.lock().unwrap();
        store.upsert_host(&one).unwrap();
        record_local_change(&store, ItemKind::Host, &one.id.0.to_string(), 1).unwrap();
    }
    sync(&a).await;
    befriend(&a, "b@example.org").await;
    befriend(&a, "c@example.org").await;
    let mut e = engine(&a).await;
    let grants = vec![
        Grant {
            account: account(&b),
            role: Role::Write,
        },
        Grant {
            account: account(&c),
            role: Role::Read,
        },
    ];
    let outcome = share(
        &mut e,
        &a.store,
        &a.vault,
        ShareWhat::Host(one.id),
        grants,
        false,
        5,
    )
    .await
    .unwrap();
    sync(&b).await;
    sync(&c).await;
    assert_eq!(b.hosts().len(), 1);
    assert_eq!(c.hosts().len(), 1);
    (a, b, c, outcome.area, export_b)
}

#[tokio::test]
async fn writer_takes_over_when_the_owner_is_deleted() {
    let server = TestServer::start().await;
    let (_a, b, c, area, _) = setup(&server).await;
    let store = server.store().await;
    sshcraft_server::admin::users_delete(&*store, "a@example.org", true, &mut Vec::new())
        .await
        .unwrap();
    sync(&b).await;
    let (manifest, _) = stored_area(&b, area);
    assert_eq!(manifest.owner, account(&b));
    assert_eq!(
        manifest.members,
        vec![Member {
            account: account(&c),
            role: Role::Read
        }]
    );
    let id = b.hosts()[0].id;
    rename(&b, id, "kept-alive", 100);
    sync(&b).await;
    sync(&c).await;
    let hosts = c.hosts();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "kept-alive");
}

#[tokio::test]
async fn takeover_by_a_reader_is_refused() {
    let server = TestServer::start().await;
    let (a, b, c, area, _) = setup(&server).await;
    // The server hands the area to C, who may only read.
    server_sql(
        &server,
        "UPDATE areas SET owner = ?1 WHERE area_id = ?2",
        rusqlite::params![account(&c).0.to_vec(), area.to_string()],
    );
    // An honest client does not take over what it may only read.
    sync(&c).await;
    let (still, _) = stored_area(&c, area);
    assert_eq!(still.owner, account(&a));
    // A modified one does; the other members refuse its manifest.
    let (old, key) = stored_area(&c, area);
    let forged = AreaManifest::sign(
        AreaId(area),
        old.version + 1,
        old.key_version,
        &key,
        account(&c),
        vec![Member {
            account: account(&b),
            role: Role::Write,
        }],
        c.row().device_id,
        &c.keys(),
    );
    c.client(&server).await.put_manifest(&forged).await.unwrap();
    let mut e = engine(&b).await;
    let report = e.pull(&b.store, &b.vault).await.unwrap();
    assert!(
        report
            .rejected
            .iter()
            .any(|r| r.contains(&area.to_string())),
        "{report:?}"
    );
    let (kept, _) = stored_area(&b, area);
    assert_eq!(kept.owner, account(&a));
}

#[tokio::test]
async fn the_heirs_other_devices_follow() {
    let server = TestServer::start().await;
    let (a, b, c, area, export_b) = setup(&server).await;
    let b2 = Dev::new();
    common::join(&server, &b2, &export_b).await;
    // The owner seals the share for B's new device on its next sync.
    sync(&a).await;
    sync(&b2).await;
    assert_eq!(b2.hosts().len(), 1);
    let store = server.store().await;
    sshcraft_server::admin::users_delete(&*store, "a@example.org", true, &mut Vec::new())
        .await
        .unwrap();
    sync(&b).await;
    sync(&b2).await;
    let id = b.hosts()[0].id;
    rename(&b, id, "from-b", 100);
    sync(&b).await;
    sync(&b2).await;
    assert_eq!(b2.hosts()[0].name, "from-b");
    // A new own host on the second device goes into the personal area, not the share.
    let mut own = common::sample_host("own");
    own.location = Some(b2.row().id);
    {
        let s = b2.store.lock().unwrap();
        s.upsert_host(&own).unwrap();
        record_local_change(&s, ItemKind::Host, &own.id.0.to_string(), 200).unwrap();
    }
    sync(&b2).await;
    let server_b2 = b2.row().id;
    let entry = b2
        .store
        .lock()
        .unwrap()
        .record_entry(server_b2, ItemKind::Host, &own.id.0.to_string())
        .unwrap()
        .unwrap();
    assert_ne!(entry.area_id, area);
    sync(&c).await;
    assert!(c.hosts().iter().all(|h| h.id != own.id));
    let _ = a;
}

#[tokio::test]
async fn a_live_owner_keeps_the_share() {
    let server = TestServer::start().await;
    let (a, b, _c, area, _) = setup(&server).await;
    // A taken-over server hands A's share to B while A still exists.
    server_sql(
        &server,
        "UPDATE areas SET owner = ?1 WHERE area_id = ?2",
        rusqlite::params![account(&b).0.to_vec(), area.to_string()],
    );
    let mut e = engine(&b).await;
    let report = e.pull(&b.store, &b.vault).await.unwrap();
    assert!(
        report
            .rejected
            .iter()
            .any(|r| r.contains(&area.to_string())),
        "{report:?}"
    );
    assert_eq!(stored_area(&b, area).0.owner, account(&a));
    // A's own sync goes on.
    let mut ea = engine(&a).await;
    ea.pull(&a.store, &a.vault).await.unwrap();
}

#[tokio::test]
async fn the_heirs_own_keys_stay_out_of_the_share() {
    let server = TestServer::start().await;
    let (_a, b, c, _area, _) = setup(&server).await;
    let server_b = b.row().id;
    // B assigns an own key (in B's personal area) to the shared host, on B's device only.
    {
        let store = b.store.lock().unwrap();
        let info = b.vault.generate_ed25519(&store, "b-own").unwrap();
        store
            .set_location(ItemKind::Key, &info.id.0.to_string(), Some(server_b))
            .unwrap();
        let mut host = store.list_hosts().unwrap().pop().unwrap();
        host.settings.key = Some(KeyRef::Stored { id: info.id });
        store.upsert_host(&host).unwrap();
    }
    let store = server.store().await;
    sshcraft_server::admin::users_delete(&*store, "a@example.org", true, &mut Vec::new())
        .await
        .unwrap();
    sync(&b).await;
    sync(&c).await;
    assert_eq!(
        c.hosts()[0].settings.key,
        None,
        "B's own key does not reach C"
    );
}

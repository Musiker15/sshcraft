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
async fn setup(server: &TestServer) -> (Dev, Dev, Dev, Uuid) {
    let (a, b, c) = (Dev::new(), Dev::new(), Dev::new());
    create(server, &a, "a@example.org").await;
    create(server, &b, "b@example.org").await;
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
    (a, b, c, outcome.area)
}

#[tokio::test]
async fn writer_takes_over_when_the_owner_is_deleted() {
    let server = TestServer::start().await;
    let (_a, b, c, area) = setup(&server).await;
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
    let (a, b, c, area) = setup(&server).await;
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

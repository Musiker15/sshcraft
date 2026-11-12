mod common;

use common::{Dev, TestServer, account, create, engine, stored_area, sync};
use sshcraft_core::model::{Group, GroupId, Host, HostId};
use sshcraft_core::store::ItemKind;
use sshcraft_proto::area::{AreaId, Role};
use sshcraft_sync::SyncError;
use sshcraft_sync::contacts::{confirm, find};
use sshcraft_sync::engine::record_local_change;
use sshcraft_sync::shares::{Grant, ShareWhat, list, set_grants, share, unshare};
use uuid::Uuid;

async fn people(server: &TestServer) -> (Dev, Dev, Dev) {
    let (a, b, c) = (Dev::new(), Dev::new(), Dev::new());
    create(server, &a, "a@example.org").await;
    create(server, &b, "b@example.org").await;
    create(server, &c, "c@example.org").await;
    (a, b, c)
}

async fn befriend(a: &Dev, email: &str) {
    let e = engine(a).await;
    let found = find(&e, &a.store, email).await.unwrap();
    confirm(&a.store, a.row().id, email, &found.account).unwrap();
}

fn grant(dev: &Dev, role: Role) -> Grant {
    Grant {
        account: account(dev),
        role,
    }
}

/// Group "team" with host "one" at the server, synced.
async fn group_with_host(a: &Dev) -> (Group, Host) {
    let server = a.row().id;
    let group = Group {
        id: GroupId::new(),
        name: "team".into(),
        defaults: Default::default(),
        location: Some(server),
    };
    let mut one = common::sample_host("one");
    one.group_id = Some(group.id);
    one.location = Some(server);
    {
        let store = a.store.lock().unwrap();
        store.upsert_group(&group).unwrap();
        store.upsert_host(&one).unwrap();
        record_local_change(&store, ItemKind::Group, &group.id.0.to_string(), 1).unwrap();
        record_local_change(&store, ItemKind::Host, &one.id.0.to_string(), 1).unwrap();
    }
    sync(a).await;
    (group, one)
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

/// A shares host "one" with B and C (both read); everyone synced.
async fn shared_with_two(server: &TestServer) -> (Dev, Dev, Dev, Host, Uuid) {
    let (a, b, c) = people(server).await;
    let (_, one) = group_with_host(&a).await;
    befriend(&a, "b@example.org").await;
    befriend(&a, "c@example.org").await;
    let mut e = engine(&a).await;
    let outcome = share(
        &mut e,
        &a.store,
        &a.vault,
        ShareWhat::Host(one.id),
        vec![grant(&b, Role::Read), grant(&c, Role::Read)],
        false,
        5,
    )
    .await
    .unwrap();
    sync(&b).await;
    sync(&c).await;
    assert_eq!(b.hosts().len(), 1);
    (a, b, c, one, outcome.area)
}

#[tokio::test]
async fn revoked_recipient_cannot_read_new_states() {
    let server = TestServer::start().await;
    let (a, b, c, one, area) = shared_with_two(&server).await;
    let mut e = engine(&a).await;
    let outcome = set_grants(
        &mut e,
        &a.store,
        &a.vault,
        area,
        vec![grant(&c, Role::Read)],
        10,
    )
    .await
    .unwrap();
    assert!(!outcome.keys_were_shared);
    assert_eq!(
        stored_area(&a, area).0.key_version,
        2,
        "taking someone out changes the key at once"
    );
    rename(&a, one.id, "after", 20);
    sync(&a).await;
    sync(&b).await;
    assert!(b.hosts().is_empty(), "B lost the shared host");
    assert_eq!(stored_area(&a, area).0.key_version, 2);
    let store = server.store().await;
    assert!(
        store
            .envelope_for(AreaId(area), b.row().device_id)
            .await
            .unwrap()
            .is_none(),
        "no key for B's device any more"
    );
    let listed = b.client(&server).await.areas().await.unwrap();
    assert!(listed.iter().all(|l| l.manifest.area_id.0 != area));
    let _ = c;
}

#[tokio::test]
async fn remaining_recipients_keep_reading_without_doing_anything() {
    let server = TestServer::start().await;
    let (a, _b, c, one, area) = shared_with_two(&server).await;
    let mut e = engine(&a).await;
    set_grants(
        &mut e,
        &a.store,
        &a.vault,
        area,
        vec![grant(&c, Role::Read)],
        10,
    )
    .await
    .unwrap();
    rename(&a, one.id, "after", 20);
    sync(&a).await;
    sync(&c).await;
    assert_eq!(c.hosts()[0].name, "after");
}

#[tokio::test]
async fn recipient_cannot_reshare_or_unshare() {
    let server = TestServer::start().await;
    let (a, b, c, _one, area) = shared_with_two(&server).await;
    befriend(&b, "c@example.org").await;
    let copy = b.hosts()[0].id;
    let mut e = engine(&b).await;
    let reshare = share(
        &mut e,
        &b.store,
        &b.vault,
        ShareWhat::Host(copy),
        vec![grant(&c, Role::Read)],
        false,
        5,
    )
    .await;
    assert_eq!(reshare.unwrap_err(), SyncError::NotAllowed);
    let regrant = set_grants(
        &mut e,
        &b.store,
        &b.vault,
        area,
        vec![grant(&c, Role::Write)],
        5,
    )
    .await;
    assert_eq!(regrant.unwrap_err(), SyncError::NotAllowed);
    assert_eq!(
        unshare(&mut e, &b.store, &b.vault, area, 5)
            .await
            .unwrap_err(),
        SyncError::NotAllowed
    );
    let _ = a;
}

#[tokio::test]
async fn role_change_takes_effect() {
    let server = TestServer::start().await;
    let (a, b, c, one, area) = shared_with_two(&server).await;
    let copy = b.hosts()[0].id;
    let mut e = engine(&a).await;
    // Read: B's change is not sent.
    rename(&b, copy, "b-1", 100);
    sync(&b).await;
    sync(&a).await;
    assert_eq!(a.hosts()[0].name, "one");
    // Write: it is.
    set_grants(
        &mut e,
        &a.store,
        &a.vault,
        area,
        vec![grant(&b, Role::Write), grant(&c, Role::Read)],
        110,
    )
    .await
    .unwrap();
    sync(&b).await;
    rename(&b, copy, "b-2", 200);
    sync(&b).await;
    sync(&a).await;
    assert_eq!(
        a.hosts().into_iter().find(|h| h.id == one.id).unwrap().name,
        "b-2"
    );
    // Read again: no longer.
    set_grants(
        &mut e,
        &a.store,
        &a.vault,
        area,
        vec![grant(&b, Role::Read), grant(&c, Role::Read)],
        210,
    )
    .await
    .unwrap();
    sync(&b).await;
    rename(&b, copy, "b-3", 300);
    sync(&b).await;
    sync(&a).await;
    assert_eq!(
        a.hosts().into_iter().find(|h| h.id == one.id).unwrap().name,
        "b-2"
    );
}

#[tokio::test]
async fn unshare_group_returns_it_to_the_owner() {
    let server = TestServer::start().await;
    let (a, b, _c) = people(&server).await;
    let (group, one) = group_with_host(&a).await;
    befriend(&a, "b@example.org").await;
    let mut e = engine(&a).await;
    let outcome = share(
        &mut e,
        &a.store,
        &a.vault,
        ShareWhat::GroupAll(group.id),
        vec![grant(&b, Role::Write)],
        false,
        5,
    )
    .await
    .unwrap();
    sync(&b).await;
    assert_eq!(b.hosts().len(), 1);
    assert_eq!(list(&a.store, a.row().id).unwrap().len(), 1);
    let mut e = engine(&a).await;
    unshare(&mut e, &a.store, &a.vault, outcome.area, 10)
        .await
        .unwrap();
    sync(&a).await;
    sync(&b).await;
    assert!(b.hosts().is_empty());
    assert_eq!(a.hosts().len(), 1);
    let server_a = a.row().id;
    {
        let store = a.store.lock().unwrap();
        assert!(
            store
                .effective_area(server_a, ItemKind::Host, &one.id.0.to_string())
                .unwrap()
                .is_none()
        );
        assert!(store.shares(server_a).unwrap().is_empty());
    }
    assert!(list(&a.store, server_a).unwrap().is_empty());
    // Back in the personal area: later changes stay with A.
    rename(&a, one.id, "mine", 30);
    sync(&a).await;
    sync(&b).await;
    assert!(b.hosts().is_empty());
    assert_eq!(a.hosts()[0].name, "mine");
}

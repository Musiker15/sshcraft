mod common;

use common::{Dev, TestServer, create, engine, inject_record, member, sample_host, seal_in, sync};
use sshcraft_core::store::{ItemKind, ShareKind};
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
async fn shared_group(
    server: &TestServer,
    kind: sshcraft_core::store::ShareKind,
) -> (Dev, Dev, Dev, Uuid, sshcraft_core::model::Host) {
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
    let (m, key) = common::stored_area(&a, area);
    let info = sshcraft_sync::shares::ShareInfo {
        kind,
        group: Some(group.id.0.to_string()),
        with_keys: false,
    };
    let header = sshcraft_proto::record::RecordHeader {
        record_id: sshcraft_sync::shares::share_info_record_id(area),
        area_id: m.area_id,
        version: 1,
        key_version: m.key_version,
        author_account: common::account(&a),
        author_device: a.row().device_id,
    };
    let sealed = sshcraft_proto::record::SealedRecord::seal(
        header,
        &sshcraft_sync::shares::encode_share_info(&info),
        &key,
        &a.keys(),
    );
    client.put_record(&sealed).await.unwrap();
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
    let (a, b, _c, _, host) = shared_group(&server, ShareKind::GroupAll).await;
    edit_host(&b, &host, "web-renamed", 100);
    sync(&b).await;
    sync(&a).await;
    assert_eq!(a.hosts()[0].name, "web-renamed");
}

#[tokio::test]
async fn reader_edit_is_rolled_back() {
    let server = TestServer::start().await;
    let (_a, _b, c, _, host) = shared_group(&server, ShareKind::GroupAll).await;
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
    let (_a, b, _c, _, host) = shared_group(&server, ShareKind::Selection).await;
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

// ---------- Task 6: creating shares

use sshcraft_core::model::{Group, Host, KeyRef};
use sshcraft_sync::SyncError;
use sshcraft_sync::contacts::{confirm, find};
use sshcraft_sync::shares::{Grant, ShareWhat, share};

/// A puts group "team" with hosts "one" and "two" on the server and syncs.
fn local_group(a: &Dev) -> (Group, Host, Host) {
    let server = a.row().id;
    let mut group = sample_group("team");
    group.location = Some(server);
    let mut one = sample_host("one");
    one.group_id = Some(group.id);
    one.location = Some(server);
    let mut two = sample_host("two");
    two.group_id = Some(group.id);
    two.location = Some(server);
    let store = a.store.lock().unwrap();
    store.upsert_group(&group).unwrap();
    store.upsert_host(&one).unwrap();
    store.upsert_host(&two).unwrap();
    record_local_change(&store, ItemKind::Group, &group.id.0.to_string(), 1).unwrap();
    record_local_change(&store, ItemKind::Host, &one.id.0.to_string(), 1).unwrap();
    record_local_change(&store, ItemKind::Host, &two.id.0.to_string(), 1).unwrap();
    (group, one, two)
}

async fn befriend(a: &Dev, b: &Dev, email: &str) {
    let e = engine(a).await;
    let found = find(&e, &a.store, email).await.unwrap();
    confirm(&a.store, a.row().id, email, &found.account).unwrap();
    let _ = b;
}

fn grant(dev: &Dev, role: Role) -> Grant {
    Grant {
        account: common::account(dev),
        role,
    }
}

#[tokio::test]
async fn group_all_share_includes_later_hosts() {
    let server = TestServer::start().await;
    let (a, b, _c) = three(&server).await;
    let (group, _, _) = local_group(&a);
    sync(&a).await;
    befriend(&a, &b, "b@example.org").await;
    let mut e = engine(&a).await;
    share(
        &mut e,
        &a.store,
        &a.vault,
        ShareWhat::GroupAll(group.id),
        vec![grant(&b, Role::Read)],
        false,
        5,
    )
    .await
    .unwrap();
    sync(&b).await;
    assert_eq!(b.hosts().len(), 2);
    // A later host of the group goes along.
    let server_a = a.row().id;
    let mut three_host = sample_host("three");
    three_host.group_id = Some(group.id);
    three_host.location = Some(server_a);
    {
        let store = a.store.lock().unwrap();
        store.upsert_host(&three_host).unwrap();
        record_local_change(&store, ItemKind::Host, &three_host.id.0.to_string(), 10).unwrap();
    }
    sync(&a).await;
    sync(&b).await;
    let mut names: Vec<String> = b.hosts().into_iter().map(|h| h.name).collect();
    names.sort();
    assert_eq!(names, vec!["one", "three", "two"]);
    // A's own devices still have everything once.
    assert_eq!(a.hosts().len(), 3);
}

#[tokio::test]
async fn selection_shares_copies_only() {
    let server = TestServer::start().await;
    let (a, b, _c) = three(&server).await;
    let (group, one, _) = local_group(&a);
    sync(&a).await;
    befriend(&a, &b, "b@example.org").await;
    let mut e = engine(&a).await;
    let what = ShareWhat::Selection {
        group: group.id,
        hosts: vec![one.id],
    };
    share(
        &mut e,
        &a.store,
        &a.vault,
        what,
        vec![grant(&b, Role::Write)],
        false,
        5,
    )
    .await
    .unwrap();
    sync(&b).await;
    let hosts = b.hosts();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "one");
    assert_ne!(hosts[0].id, one.id);
    let groups = b.store.lock().unwrap().list_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_ne!(groups[0].id, group.id);
    assert_eq!(hosts[0].group_id, Some(groups[0].id));
    // The owner does not see the copies as items of its own.
    sync(&a).await;
    assert_eq!(a.hosts().len(), 2);
    assert_eq!(a.store.lock().unwrap().list_groups().unwrap().len(), 1);
}

#[tokio::test]
async fn share_needs_a_confirmed_contact() {
    let server = TestServer::start().await;
    let (a, b, _c) = three(&server).await;
    let (group, _, _) = local_group(&a);
    sync(&a).await;
    let mut e = engine(&a).await;
    let refused = share(
        &mut e,
        &a.store,
        &a.vault,
        ShareWhat::GroupAll(group.id),
        vec![grant(&b, Role::Read)],
        false,
        5,
    )
    .await;
    assert_eq!(refused.unwrap_err(), SyncError::ContactUnconfirmed);
    // Looked up but not confirmed is not enough either.
    find(&e, &a.store, "b@example.org").await.unwrap();
    let refused = share(
        &mut e,
        &a.store,
        &a.vault,
        ShareWhat::GroupAll(group.id),
        vec![grant(&b, Role::Read)],
        false,
        5,
    )
    .await;
    assert_eq!(refused.unwrap_err(), SyncError::ContactUnconfirmed);
}

#[tokio::test]
async fn agent_keys_are_not_shared() {
    let server = TestServer::start().await;
    let (a, b, _c) = three(&server).await;
    let (_, mut one, _) = local_group(&a);
    one.settings.key = Some(KeyRef::Agent {
        fingerprint: "SHA256:abc".into(),
    });
    a.store.lock().unwrap().upsert_host(&one).unwrap();
    sync(&a).await;
    befriend(&a, &b, "b@example.org").await;
    let mut e = engine(&a).await;
    let outcome = share(
        &mut e,
        &a.store,
        &a.vault,
        ShareWhat::Host(one.id),
        vec![grant(&b, Role::Read)],
        true,
        5,
    )
    .await
    .unwrap();
    assert_eq!(outcome.agent_keys_skipped, 1);
    sync(&b).await;
    assert_eq!(b.hosts()[0].settings.key, None);
}

#[tokio::test]
async fn keys_go_along_only_when_asked() {
    let server = TestServer::start().await;
    let (a, b, c) = three(&server).await;
    for dev in [&a, &b, &c] {
        let id = dev.row().id;
        dev.store
            .lock()
            .unwrap()
            .update_server_flags(id, true, true)
            .unwrap();
    }
    let (group, mut one, _) = local_group(&a);
    let server_a = a.row().id;
    {
        let store = a.store.lock().unwrap();
        let info = a.vault.generate_ed25519(&store, "deploy").unwrap();
        store
            .set_location(ItemKind::Key, &info.id.0.to_string(), Some(server_a))
            .unwrap();
        record_local_change(&store, ItemKind::Key, &info.id.0.to_string(), 1).unwrap();
        one.settings.key = Some(KeyRef::Stored { id: info.id });
        store.upsert_host(&one).unwrap();
    }
    sync(&a).await;
    befriend(&a, &b, "b@example.org").await;
    befriend(&a, &c, "c@example.org").await;
    let mut e = engine(&a).await;
    let with = ShareWhat::Selection {
        group: group.id,
        hosts: vec![one.id],
    };
    share(
        &mut e,
        &a.store,
        &a.vault,
        with,
        vec![grant(&b, Role::Read)],
        true,
        5,
    )
    .await
    .unwrap();
    share(
        &mut e,
        &a.store,
        &a.vault,
        ShareWhat::Host(one.id),
        vec![grant(&c, Role::Read)],
        false,
        6,
    )
    .await
    .unwrap();
    sync(&b).await;
    sync(&c).await;
    let b_keys = b.store.lock().unwrap().list_key_records().unwrap();
    assert_eq!(b_keys.len(), 1);
    assert!(matches!(b.hosts()[0].settings.key, Some(KeyRef::Stored { id }) if id == b_keys[0].id));
    assert!(
        c.store
            .lock()
            .unwrap()
            .list_key_records()
            .unwrap()
            .is_empty()
    );
    assert_eq!(c.hosts()[0].settings.key, None);
    // The owner keeps its key on the host.
    sync(&a).await;
    let mine = a.hosts().into_iter().find(|h| h.id == one.id).unwrap();
    assert!(matches!(mine.settings.key, Some(KeyRef::Stored { .. })));
}

// ---------- Task 7: copies follow their original

fn rename(dev: &Dev, id: sshcraft_core::model::HostId, name: &str, at: i64) {
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

/// A shares host "one" of its group as a selection with B (write); everyone synced.
async fn selection_of_one(server: &TestServer) -> (Dev, Dev, Dev, Host) {
    let (a, b, c) = three(server).await;
    let (group, one, _) = local_group(&a);
    sync(&a).await;
    befriend(&a, &b, "b@example.org").await;
    let mut e = engine(&a).await;
    let what = ShareWhat::Selection {
        group: group.id,
        hosts: vec![one.id],
    };
    share(
        &mut e,
        &a.store,
        &a.vault,
        what,
        vec![grant(&b, Role::Write)],
        false,
        5,
    )
    .await
    .unwrap();
    sync(&b).await;
    (a, b, c, one)
}

#[tokio::test]
async fn owner_edit_reaches_the_copy() {
    let server = TestServer::start().await;
    let (a, b, _c, one) = selection_of_one(&server).await;
    rename(&a, one.id, "one-new", 100);
    sync(&a).await;
    sync(&b).await;
    assert_eq!(b.hosts()[0].name, "one-new");
}

#[tokio::test]
async fn writer_edit_on_a_copy_reaches_the_original() {
    let server = TestServer::start().await;
    let (a, b, c, one) = selection_of_one(&server).await;
    befriend(&a, &c, "c@example.org").await;
    let mut e = engine(&a).await;
    share(
        &mut e,
        &a.store,
        &a.vault,
        ShareWhat::Host(one.id),
        vec![grant(&c, Role::Read)],
        false,
        6,
    )
    .await
    .unwrap();
    sync(&c).await;
    let copy = b.hosts()[0].id;
    rename(&b, copy, "from-b", 100);
    sync(&b).await;
    sync(&a).await;
    let original = a.hosts().into_iter().find(|h| h.id == one.id).unwrap();
    assert_eq!(original.name, "from-b");
    assert_eq!(
        original.group_id, one.group_id,
        "the original stays in its group"
    );
    sync(&c).await;
    assert_eq!(c.hosts()[0].name, "from-b");
}

#[tokio::test]
async fn concurrent_owner_and_writer_converge() {
    let server = TestServer::start().await;
    let (a, b, _c, one) = selection_of_one(&server).await;
    let copy = b.hosts()[0].id;
    rename(&a, one.id, "from-a", 1000);
    rename(&b, copy, "from-b", 2000);
    for _ in 0..2 {
        sync(&a).await;
        sync(&b).await;
    }
    let original = a.hosts().into_iter().find(|h| h.id == one.id).unwrap();
    assert_eq!(original.name, "from-b");
    assert_eq!(b.hosts()[0].name, "from-b");
}

#[tokio::test]
async fn deleting_the_original_removes_the_copy() {
    let server = TestServer::start().await;
    let (a, b, _c, one) = selection_of_one(&server).await;
    {
        let store = a.store.lock().unwrap();
        record_local_change(&store, ItemKind::Host, &one.id.0.to_string(), 100).unwrap();
        store.delete_host(one.id).unwrap();
    }
    sync(&a).await;
    sync(&b).await;
    assert!(b.hosts().is_empty());
}

#[tokio::test]
async fn newer_unsent_owner_change_beats_an_older_copy_change() {
    let server = TestServer::start().await;
    let (a, b, _c, one) = selection_of_one(&server).await;
    let copy = b.hosts()[0].id;
    rename(&b, copy, "from-b", 1000);
    sync(&b).await;
    // A changed the original later but has not sent it yet when B's change arrives.
    rename(&a, one.id, "from-a", 2000);
    for _ in 0..2 {
        sync(&a).await;
        sync(&b).await;
    }
    let original = a.hosts().into_iter().find(|h| h.id == one.id).unwrap();
    assert_eq!(original.name, "from-a");
    assert_eq!(b.hosts()[0].name, "from-a");
}

// ---------- Final review fixes

#[tokio::test]
async fn own_host_moved_into_a_received_selection_stays() {
    let server = TestServer::start().await;
    let (_a, b, _c, _, host) = shared_group(&server, ShareKind::Selection).await;
    let server_b = b.row().id;
    let mut mine = sample_host("mine");
    mine.location = Some(server_b);
    {
        let store = b.store.lock().unwrap();
        store.upsert_host(&mine).unwrap();
        record_local_change(&store, ItemKind::Host, &mine.id.0.to_string(), 50).unwrap();
    }
    sync(&b).await;
    // B puts the own host into the received group: an addition the selection does not allow.
    {
        let store = b.store.lock().unwrap();
        let mut moved = mine.clone();
        moved.group_id = host.group_id;
        store.upsert_host(&moved).unwrap();
        record_local_change(&store, ItemKind::Host, &mine.id.0.to_string(), 60).unwrap();
    }
    let mut e = engine(&b).await;
    e.push(&b.store, &b.vault, 60).await.unwrap();
    assert!(
        e.warnings().iter().any(|w| w == "share_no_add_remove"),
        "{:?}",
        e.warnings()
    );
    let kept = b
        .hosts()
        .into_iter()
        .find(|h| h.id == mine.id)
        .expect("the own host stays");
    assert_eq!(kept.group_id, None);
    // And it is still in the personal area on the server: another sync keeps it.
    sync(&b).await;
    assert!(b.hosts().iter().any(|h| h.id == mine.id));
}

/// A shares host "one" with key "deploy" as a selection with B (write, key sync on); returns
/// the key B received.
async fn received_key(
    server: &TestServer,
    all_hosts: bool,
) -> (Dev, Dev, Dev, sshcraft_core::model::KeyId) {
    let (a, b, c) = three(server).await;
    for dev in [&a, &b, &c] {
        let id = dev.row().id;
        dev.store
            .lock()
            .unwrap()
            .update_server_flags(id, true, true)
            .unwrap();
    }
    let (group, mut one, _) = local_group(&a);
    let server_a = a.row().id;
    {
        let store = a.store.lock().unwrap();
        let info = a.vault.generate_ed25519(&store, "deploy").unwrap();
        store
            .set_location(ItemKind::Key, &info.id.0.to_string(), Some(server_a))
            .unwrap();
        record_local_change(&store, ItemKind::Key, &info.id.0.to_string(), 1).unwrap();
        one.settings.key = Some(KeyRef::Stored { id: info.id });
        store.upsert_host(&one).unwrap();
    }
    sync(&a).await;
    befriend(&a, &b, "b@example.org").await;
    let mut e = engine(&a).await;
    let what = if all_hosts {
        ShareWhat::GroupAll(group.id)
    } else {
        ShareWhat::Selection {
            group: group.id,
            hosts: vec![one.id],
        }
    };
    share(
        &mut e,
        &a.store,
        &a.vault,
        what,
        vec![grant(&b, Role::Write)],
        true,
        5,
    )
    .await
    .unwrap();
    sync(&b).await;
    let key = b.store.lock().unwrap().list_key_records().unwrap()[0].id;
    (a, b, c, key)
}

#[tokio::test]
async fn received_keys_are_not_shared_on() {
    let server = TestServer::start().await;
    let (a, b, c, key) = received_key(&server, false).await;
    let server_b = b.row().id;
    let mut mine = sample_host("mine");
    mine.location = Some(server_b);
    mine.settings.key = Some(KeyRef::Stored { id: key });
    let mut group = sample_group("b-team");
    group.location = Some(server_b);
    let mut grouped = sample_host("grouped");
    grouped.location = Some(server_b);
    grouped.group_id = Some(group.id);
    grouped.settings.key = Some(KeyRef::Stored { id: key });
    {
        let store = b.store.lock().unwrap();
        store.upsert_host(&mine).unwrap();
        store.upsert_group(&group).unwrap();
        store.upsert_host(&grouped).unwrap();
        record_local_change(&store, ItemKind::Host, &mine.id.0.to_string(), 50).unwrap();
        record_local_change(&store, ItemKind::Group, &group.id.0.to_string(), 50).unwrap();
        record_local_change(&store, ItemKind::Host, &grouped.id.0.to_string(), 50).unwrap();
    }
    sync(&b).await;
    befriend(&b, &c, "c@example.org").await;
    let mut e = engine(&b).await;
    share(
        &mut e,
        &b.store,
        &b.vault,
        ShareWhat::Host(mine.id),
        vec![grant(&c, Role::Read)],
        true,
        60,
    )
    .await
    .unwrap();
    share(
        &mut e,
        &b.store,
        &b.vault,
        ShareWhat::GroupAll(group.id),
        vec![grant(&c, Role::Read)],
        true,
        61,
    )
    .await
    .unwrap();
    sync(&c).await;
    assert!(
        c.store
            .lock()
            .unwrap()
            .list_key_records()
            .unwrap()
            .is_empty(),
        "A's key never reaches C"
    );
    // The received key stays in A's share, and A keeps it.
    let area_of_key = b
        .store
        .lock()
        .unwrap()
        .effective_area(server_b, ItemKind::Key, &key.0.to_string())
        .unwrap();
    assert!(area_of_key.is_some());
    sync(&a).await;
    assert_eq!(a.store.lock().unwrap().list_key_records().unwrap().len(), 1);
}

#[tokio::test]
async fn members_cannot_delete_a_shared_key() {
    let server = TestServer::start().await;
    let (a, b, _c, key) = received_key(&server, true).await;
    {
        let store = b.store.lock().unwrap();
        record_local_change(&store, ItemKind::Key, &key.0.to_string(), 70).unwrap();
        store.delete_key_record(key).unwrap();
    }
    let mut e = engine(&b).await;
    e.push(&b.store, &b.vault, 70).await.unwrap();
    assert!(!e.warnings().is_empty());
    assert_eq!(
        b.store.lock().unwrap().list_key_records().unwrap().len(),
        1,
        "restored"
    );
    sync(&a).await;
    assert_eq!(a.store.lock().unwrap().list_key_records().unwrap().len(), 1);
}

#[tokio::test]
async fn selection_writer_cannot_change_the_group() {
    let server = TestServer::start().await;
    let (a, b, _c, _one) = selection_of_one(&server).await;
    let server_b = b.row().id;
    let copy = b.store.lock().unwrap().list_groups().unwrap()[0].clone();
    {
        let store = b.store.lock().unwrap();
        let mut renamed = copy.clone();
        renamed.name = "renamed".into();
        store.upsert_group(&renamed).unwrap();
        record_local_change(&store, ItemKind::Group, &copy.id.0.to_string(), 80).unwrap();
    }
    let mut e = engine(&b).await;
    e.push(&b.store, &b.vault, 80).await.unwrap();
    assert!(
        e.warnings().iter().any(|w| w == "share_read_only"),
        "{:?}",
        e.warnings()
    );
    sync(&a).await;
    assert_eq!(
        a.store.lock().unwrap().list_groups().unwrap()[0].name,
        "team"
    );
    let _ = server_b;
}

#[tokio::test]
async fn owner_key_assignments_reach_the_owners_other_devices() {
    let server = TestServer::start().await;
    let (a, b, a2) = (Dev::new(), Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    create(&server, &b, "b@example.org").await;
    let (group, one, _) = local_group(&a);
    sync(&a).await;
    befriend(&a, &b, "b@example.org").await;
    let mut e = engine(&a).await;
    share(
        &mut e,
        &a.store,
        &a.vault,
        ShareWhat::GroupAll(group.id),
        vec![grant(&b, Role::Read)],
        false,
        5,
    )
    .await
    .unwrap();
    common::join(&server, &a2, &export).await;
    for dev in [&a, &a2] {
        let id = dev.row().id;
        dev.store
            .lock()
            .unwrap()
            .update_server_flags(id, true, true)
            .unwrap();
    }
    sync(&a2).await;
    let server_a = a.row().id;
    let key = {
        let store = a.store.lock().unwrap();
        let info = a.vault.generate_ed25519(&store, "deploy").unwrap();
        store
            .set_location(ItemKind::Key, &info.id.0.to_string(), Some(server_a))
            .unwrap();
        record_local_change(&store, ItemKind::Key, &info.id.0.to_string(), 10).unwrap();
        let mut host = store
            .list_hosts()
            .unwrap()
            .into_iter()
            .find(|h| h.id == one.id)
            .unwrap();
        host.settings.key = Some(KeyRef::Stored { id: info.id });
        store.upsert_host(&host).unwrap();
        record_local_change(&store, ItemKind::Host, &one.id.0.to_string(), 11).unwrap();
        info.id
    };
    sync(&a).await;
    sync(&a2).await;
    let on_a2 = a2.hosts().into_iter().find(|h| h.id == one.id).unwrap();
    assert_eq!(on_a2.settings.key, Some(KeyRef::Stored { id: key }));
    // B, who got the group without keys, sees none.
    sync(&b).await;
    assert!(b.hosts().iter().all(|h| h.settings.key.is_none()));
}

#[tokio::test]
async fn members_ignore_additions_and_deletions_of_other_members_in_a_selection() {
    let server = TestServer::start().await;
    let (_a, b, c, area, host) = shared_group(&server, ShareKind::Selection).await;
    let server_b = b.row().id;
    let entry = b
        .store
        .lock()
        .unwrap()
        .record_entry(server_b, ItemKind::Host, &host.id.0.to_string())
        .unwrap()
        .unwrap();
    // A modified client of B deletes the host and adds another, straight on the server.
    let client = b.client(&server).await;
    let deleted = seal_in(
        &b,
        area,
        ItemBody::Deleted {
            item_kind: ItemKind::Host,
            item_id: host.id.0.to_string(),
        },
        entry.record_id,
        entry.version + 1,
        500,
    );
    client.put_record(&deleted).await.unwrap();
    let mut extra = sample_host("extra");
    extra.group_id = host.group_id;
    let added = seal_in(
        &b,
        area,
        ItemBody::Host { host: extra },
        Uuid::new_v4(),
        1,
        501,
    );
    client.put_record(&added).await.unwrap();
    sync(&c).await;
    let names: Vec<String> = c.hosts().into_iter().map(|h| h.name).collect();
    assert_eq!(names, vec!["web".to_owned()]);
}

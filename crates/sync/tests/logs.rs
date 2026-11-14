mod common;

use common::{Dev, TestServer, create, engine, join, sample_host, sync};
use sshcraft_model::model::{Group, GroupId, Host, LogId, ServerId};
use sshcraft_proto::area::Role;
use sshcraft_store::session_log::{self, LogKind, LogMark, LogMeta, MarkKind};
use sshcraft_store::store::ItemKind;
use sshcraft_sync::contacts::{confirm, find};
use sshcraft_sync::engine::record_local_change;
use sshcraft_sync::logs::{delete_log, place_logs, set_sync_logs};
use sshcraft_sync::shares::{Grant, ShareWhat, share};
use uuid::Uuid;

fn server_of(dev: &Dev) -> ServerId {
    dev.row().id
}

/// Puts `host` at the device's server and queues it.
fn save_host(dev: &Dev, host: &Host) {
    let server = server_of(dev);
    let store = dev.store.lock().unwrap();
    let mut host = host.clone();
    host.location = Some(server);
    store.upsert_host(&host).unwrap();
    record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 1).unwrap();
}

/// Records a finished log of `host` with two parts and a comment, as the app does.
fn record(dev: &Dev, host: &Host, title: &str) -> LogId {
    let store = dev.store.lock().unwrap();
    let meta = LogMeta {
        id: LogId(Uuid::new_v4()),
        title: title.into(),
        kind: LogKind::Ssh,
        host_id: Some(host.id),
        started_at: 10,
        ended_at: None,
        size: 0,
        truncated: false,
        marks: Vec::new(),
    };
    session_log::create(&dev.vault, &store, &meta).unwrap();
    session_log::append_chunk(&dev.vault, &store, meta.id, 0, b"$ uptime\r\n", 11).unwrap();
    session_log::append_chunk(&dev.vault, &store, meta.id, 1, b" up 3 days\r\n", 12).unwrap();
    session_log::save_marks(
        &dev.vault,
        &store,
        meta.id,
        vec![LogMark {
            id: Uuid::new_v4(),
            offset: 10,
            kind: MarkKind::Comment,
            text: "after uptime".into(),
            created_at: 12,
        }],
    )
    .unwrap();
    session_log::finish(&dev.vault, &store, meta.id, 20, 22, false).unwrap();
    place_logs(&store, 20).unwrap();
    meta.id
}

fn logs_on(dev: &Dev) -> Vec<(String, Vec<u8>)> {
    let store = dev.store.lock().unwrap();
    session_log::list(&dev.vault, &store)
        .unwrap()
        .into_iter()
        .map(|s| {
            let bytes = session_log::read(&dev.vault, &store, s.meta.id).unwrap();
            (s.meta.title, bytes)
        })
        .collect()
}

fn logs_sync(dev: &Dev, on: bool) {
    let server = server_of(dev);
    set_sync_logs(&dev.store.lock().unwrap(), server, on, 5).unwrap();
}

/// Two devices of one account, both with host `web`.
async fn pair(server: &TestServer) -> (Dev, Dev, Host) {
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(server, &a, "a@example.org").await;
    join(server, &b, &export).await;
    let host = sample_host("web");
    save_host(&a, &host);
    sync(&a).await;
    sync(&b).await;
    (a, b, host)
}

#[tokio::test]
async fn a_log_reaches_the_other_device_whole() {
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    logs_sync(&a, true);
    logs_sync(&b, true);
    let id = record(&a, &host, "web");
    sync(&a).await;
    sync(&b).await;
    assert_eq!(
        logs_on(&b),
        vec![("web".to_owned(), b"$ uptime\r\n up 3 days\r\n".to_vec())]
    );
    let meta = session_log::meta(&b.vault, &b.store.lock().unwrap(), id)
        .unwrap()
        .unwrap();
    assert_eq!(meta.marks.len(), 1);
    assert_eq!(meta.marks[0].text, "after uptime");
}

#[tokio::test]
async fn logs_are_not_sent_while_log_sync_is_off() {
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    logs_sync(&b, true);
    record(&a, &host, "web");
    sync(&a).await;
    sync(&b).await;
    assert!(logs_on(&b).is_empty());
    // Switched on at A, the logs already there go.
    logs_sync(&a, true);
    sync(&a).await;
    sync(&b).await;
    assert_eq!(logs_on(&b).len(), 1);
}

#[tokio::test]
async fn a_device_with_log_sync_off_skips_logs_quietly() {
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    logs_sync(&a, true);
    record(&a, &host, "web");
    sync(&a).await;
    let mut e = engine(&b).await;
    let report = e.pull(&b.store, &b.vault).await.unwrap();
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    assert!(logs_on(&b).is_empty());
    // Switched on, B fetches from the start and gets it.
    logs_sync(&b, true);
    sync(&b).await;
    assert_eq!(logs_on(&b).len(), 1);
}

#[tokio::test]
async fn deleting_a_log_removes_it_everywhere() {
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    logs_sync(&a, true);
    logs_sync(&b, true);
    let id = record(&a, &host, "web");
    sync(&a).await;
    sync(&b).await;
    assert_eq!(logs_on(&b).len(), 1);
    let server_a = server_of(&a);
    assert_eq!(
        delete_log(&a.store.lock().unwrap(), id, 30).unwrap(),
        Some(server_a)
    );
    sync(&a).await;
    sync(&b).await;
    assert!(logs_on(&b).is_empty());
    let left: i64 = session_log::chunk_ids(&b.store.lock().unwrap(), id)
        .unwrap()
        .len() as i64;
    assert_eq!(left, 0);
}

#[tokio::test]
async fn logs_follow_their_host_off_the_server() {
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    logs_sync(&a, true);
    logs_sync(&b, true);
    let id = record(&a, &host, "web");
    sync(&a).await;
    sync(&b).await;
    assert_eq!(logs_on(&b).len(), 1);
    // A moves the host to this device only; the log goes with it.
    {
        let store = a.store.lock().unwrap();
        record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 40).unwrap();
        store
            .set_location(ItemKind::Host, &host.id.0.to_string(), None)
            .unwrap();
    }
    sync(&a).await;
    assert_eq!(
        session_log::location(&a.store.lock().unwrap(), id).unwrap(),
        Some(None)
    );
    assert_eq!(logs_on(&a).len(), 1);
    sync(&b).await;
    assert!(logs_on(&b).is_empty());
}

#[tokio::test]
async fn removing_the_server_keeps_logs_here() {
    let server = TestServer::start().await;
    let (a, _b, host) = pair(&server).await;
    logs_sync(&a, true);
    let id = record(&a, &host, "web");
    let server_a = server_of(&a);
    let store = a.store.lock().unwrap();
    assert_eq!(
        session_log::location(&store, id).unwrap(),
        Some(Some(server_a))
    );
    store.remove_server(server_a).unwrap();
    assert_eq!(session_log::location(&store, id).unwrap(), Some(None));
    assert_eq!(session_log::list(&a.vault, &store).unwrap().len(), 1);
}

#[tokio::test]
async fn a_log_of_a_shared_host_stays_personal() {
    let server = TestServer::start().await;
    let (a, b) = (Dev::new(), Dev::new());
    create(&server, &a, "a@example.org").await;
    create(&server, &b, "b@example.org").await;
    let mut group = Group {
        id: GroupId::new(),
        name: "team".into(),
        defaults: Default::default(),
        location: Some(server_of(&a)),
        parent_id: None,
        tags: Vec::new(),
    };
    group.location = Some(server_of(&a));
    let mut host = sample_host("one");
    host.group_id = Some(group.id);
    host.location = Some(server_of(&a));
    {
        let store = a.store.lock().unwrap();
        store.upsert_group(&group).unwrap();
        store.upsert_host(&host).unwrap();
        record_local_change(&store, ItemKind::Group, &group.id.0.to_string(), 1).unwrap();
        record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 1).unwrap();
    }
    sync(&a).await;
    let mut e = engine(&a).await;
    let found = find(&e, &a.store, "b@example.org").await.unwrap();
    let server_a = server_of(&a);
    confirm(&a.store, server_a, "b@example.org", &found.account).unwrap();
    share(
        &mut e,
        &a.store,
        &a.vault,
        ShareWhat::GroupAll(group.id),
        vec![Grant {
            account: common::account(&b),
            role: Role::Write,
        }],
        false,
        5,
    )
    .await
    .unwrap();
    logs_sync(&a, true);
    logs_sync(&b, true);
    record(&a, &host, "shared host");
    sync(&a).await;
    sync(&b).await;
    assert_eq!(b.hosts().len(), 1);
    // B could not read A's personal area anyway; what counts are the area of the record below
    // and that a log record put into the share is not taken.
    assert!(logs_on(&b).is_empty());
    // On A's own devices it is there, in the personal area.
    let server_a = server_of(&a);
    let id = {
        let store = a.store.lock().unwrap();
        let personal = store
            .get_setting(&sshcraft_sync::account::personal_setting(server_a))
            .unwrap()
            .unwrap();
        let id = session_log::list(&a.vault, &store).unwrap()[0].meta.id;
        let entry = store
            .record_entry(server_a, ItemKind::Log, &id.0.to_string())
            .unwrap()
            .unwrap();
        assert_eq!(entry.area_id.to_string(), personal);
        id
    };
    // A log record put into the share is not taken by its members.
    let area = a.store.lock().unwrap().shares(server_a).unwrap()[0].area_id;
    let meta = session_log::meta(&a.vault, &a.store.lock().unwrap(), id)
        .unwrap()
        .unwrap();
    let record = common::seal_in(
        &a,
        area,
        sshcraft_sync::items::ItemBody::Log { meta },
        Uuid::new_v4(),
        1,
        50,
    );
    a.client(&server).await.put_record(&record).await.unwrap();
    sync(&b).await;
    assert!(logs_on(&b).is_empty());
}

#[tokio::test]
async fn a_part_put_in_another_place_does_not_open() {
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    logs_sync(&a, true);
    logs_sync(&b, true);
    let id = record(&a, &host, "web");
    sync(&a).await;
    sync(&b).await;
    let store = b.store.lock().unwrap();
    // Someone swapped the two parts on the disk of B: no gap, but each sits at a foreign place.
    let chunks = session_log::chunk_ids(&store, id).unwrap();
    assert_eq!(chunks.len(), 2);
    store
        .execute_raw(&format!(
            "UPDATE session_log_chunks SET seq = 99 WHERE id = '{a}';
             UPDATE session_log_chunks SET seq = 0 WHERE id = '{b}';
             UPDATE session_log_chunks SET seq = 1 WHERE id = '{a}';",
            a = chunks[0],
            b = chunks[1]
        ))
        .unwrap();
    assert!(session_log::read(&b.vault, &store, id).is_err());
}

#[tokio::test]
async fn switching_log_sync_off_and_on_sends_nothing_again() {
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    logs_sync(&a, true);
    logs_sync(&b, true);
    record(&a, &host, "web");
    sync(&a).await;
    sync(&b).await;
    // B has it from A; switching off and on queues nothing on either device.
    for dev in [&a, &b] {
        logs_sync(dev, false);
        logs_sync(dev, true);
        let server_id = server_of(dev);
        assert!(
            dev.store
                .lock()
                .unwrap()
                .outbox(server_id)
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn a_broken_log_record_is_refused_for_good() {
    let server = TestServer::start().await;
    let (a, b, _host) = pair(&server).await;
    logs_sync(&b, true);
    let personal = {
        let server_a = server_of(&a);
        let store = a.store.lock().unwrap();
        Uuid::parse_str(
            &store
                .get_setting(&sshcraft_sync::account::personal_setting(server_a))
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    };
    let broken = LogMeta {
        id: LogId(Uuid::new_v4()),
        title: String::new(),
        kind: LogKind::Ssh,
        host_id: None,
        started_at: 1,
        ended_at: Some(2),
        size: 0,
        truncated: false,
        marks: Vec::new(),
    };
    let record = common::seal_in(
        &a,
        personal,
        sshcraft_sync::items::ItemBody::Log {
            meta: broken.clone(),
        },
        Uuid::new_v4(),
        1,
        5,
    );
    a.client(&server).await.put_record(&record).await.unwrap();
    sync(&b).await;
    assert!(logs_on(&b).is_empty());
    let server_b = server_of(&b);
    assert!(
        b.store
            .lock()
            .unwrap()
            .record_entry(server_b, ItemKind::Log, &broken.id.0.to_string())
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn a_damaged_log_does_not_stop_the_other_changes() {
    let server = TestServer::start().await;
    let (a, b, host) = pair(&server).await;
    logs_sync(&a, true);
    let id = record(&a, &host, "web");
    a.store
        .lock()
        .unwrap()
        .execute_raw(&format!(
            "UPDATE session_log_chunks SET sealed = x'00' WHERE log_id = '{}'",
            id.0
        ))
        .unwrap();
    let mut renamed = host.clone();
    renamed.name = "renamed".into();
    renamed.location = Some(server_of(&a));
    {
        let store = a.store.lock().unwrap();
        store.upsert_host(&renamed).unwrap();
        record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 60).unwrap();
    }
    let mut e = engine(&a).await;
    e.pull(&a.store, &a.vault).await.unwrap();
    let sent = e.push(&a.store, &a.vault, 50).await.unwrap();
    assert!(sent >= 1);
    assert!(e.warnings().iter().any(|w| w.starts_with("log_skipped")));
    sync(&b).await;
    assert_eq!(b.hosts()[0].name, "renamed");
}

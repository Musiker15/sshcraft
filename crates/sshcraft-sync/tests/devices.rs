mod common;

use common::{Dev, TestServer, create, join, sample_host};
use sshcraft_core::model::Host;
use sshcraft_core::store::ItemKind;
use sshcraft_proto::area::AreaId;
use sshcraft_proto::identity::{AccountPublic, recovery_device_id};
use sshcraft_sync::SyncError;
use sshcraft_sync::devices::{list_devices, revoke_device};
use sshcraft_sync::engine::{Engine, record_local_change};

async fn engine(dev: &Dev) -> Result<Engine, SyncError> {
    Engine::load(&dev.store, &dev.vault, dev.row().id).await
}

async fn sync(dev: &Dev) {
    let mut e = engine(dev).await.unwrap();
    e.pull(&dev.store, &dev.vault).await.unwrap();
    e.push(&dev.store, &dev.vault, 0).await.unwrap();
}

fn save(dev: &Dev, host: &Host, at: i64) {
    let server = dev.row().id;
    let store = dev.store.lock().unwrap();
    let mut host = host.clone();
    host.location = Some(server);
    store.upsert_host(&host).unwrap();
    record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), at).unwrap();
}

#[tokio::test]
async fn revoked_device_loses_access() {
    let server = TestServer::start().await;
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &b, &export).await;
    let b_id = b.row().device_id;
    let mut e = engine(&a).await.unwrap();
    revoke_device(&mut e, &a.store, &a.vault, b_id, 10)
        .await
        .unwrap();

    assert_eq!(engine(&b).await.err(), Some(SyncError::Revoked));
    let (manifest, _) = a.area();
    assert_eq!(manifest.key_version, 2, "the area key was rotated");
    let on_server = server.store().await;
    assert_eq!(
        on_server
            .envelope_for(AreaId(manifest.area_id.0), b_id)
            .await
            .unwrap(),
        None
    );
    assert!(
        on_server
            .envelope_for(manifest.area_id, a.row().device_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn rotation_keeps_data_readable() {
    let server = TestServer::start().await;
    let (a, b, c) = (Dev::new(), Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &b, &export).await;
    save(&a, &sample_host("one"), 100);
    save(&a, &sample_host("two"), 100);
    sync(&a).await;
    let mut e = engine(&a).await.unwrap();
    revoke_device(&mut e, &a.store, &a.vault, b.row().device_id, 10)
        .await
        .unwrap();

    join(&server, &c, &export).await;
    sync(&c).await;
    let mut names: Vec<String> = c.hosts().into_iter().map(|h| h.name).collect();
    names.sort();
    assert_eq!(names, vec!["one", "two"]);
    // A keeps working with the new key too.
    save(&a, &sample_host("three"), 200);
    sync(&a).await;
    sync(&c).await;
    assert_eq!(c.hosts().len(), 3);
}

#[tokio::test]
async fn cannot_revoke_self_or_recovery() {
    let server = TestServer::start().await;
    let a = Dev::new();
    create(&server, &a, "a@example.org").await;
    let row = a.row();
    let mut e = engine(&a).await.unwrap();
    let own = row.device_id;
    let recovery = recovery_device_id(&AccountPublic(row.account));
    for target in [own, recovery] {
        assert_eq!(
            revoke_device(&mut e, &a.store, &a.vault, target, 10).await,
            Err(SyncError::NotAllowed)
        );
    }
}

#[tokio::test]
async fn device_list_names_and_marks() {
    let server = TestServer::start().await;
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &b, &export).await;
    let mut e = engine(&a).await.unwrap();
    let list = list_devices(&mut e, &a.store).await.unwrap();
    assert_eq!(list.len(), 3);
    let me = list.iter().find(|d| d.this_device).unwrap();
    assert_eq!(
        (me.name.as_str(), me.platform.as_str()),
        ("first", "windows")
    );
    assert!(list.iter().any(|d| d.recovery && !d.this_device));
    let other = list
        .iter()
        .find(|d| d.device_id == b.row().device_id)
        .unwrap();
    assert!(other.active && other.name == "next");

    revoke_device(&mut e, &a.store, &a.vault, b.row().device_id, 10)
        .await
        .unwrap();
    let list = list_devices(&mut e, &a.store).await.unwrap();
    let other = list
        .iter()
        .find(|d| d.device_id == b.row().device_id)
        .unwrap();
    assert!(!other.active);
}

#[tokio::test]
async fn retry_after_a_failed_rotation_still_rotates() {
    let server = TestServer::start().await;
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &b, &export).await;
    // The revocation reached the server, the rotation did not (as after a network error).
    common::revoke_raw(&server, &a, b.row().device_id).await;
    assert_eq!(a.area().0.key_version, 1);
    let mut e = engine(&a).await.unwrap();
    revoke_device(&mut e, &a.store, &a.vault, b.row().device_id, 10)
        .await
        .unwrap();
    assert_eq!(a.area().0.key_version, 2);
    let unknown = uuid::Uuid::new_v4();
    assert_eq!(
        revoke_device(&mut e, &a.store, &a.vault, unknown, 10).await,
        Err(SyncError::NotAllowed)
    );
}

/// A revoke that fails at `op` (after `skip` successful calls) and is finished by the next pull.
async fn interrupted_rotation(op: &str, skip: usize) {
    let server = TestServer::start().await;
    let (a, b, c) = (Dev::new(), Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &b, &export).await;
    for name in ["one", "two", "three"] {
        save(&a, &sample_host(name), 100);
    }
    sync(&a).await;

    sshcraft_sync::client::failpoints::arm(&server.origin, op, skip);
    let mut e = engine(&a).await.unwrap();
    let failed = revoke_device(&mut e, &a.store, &a.vault, b.row().device_id, 10).await;
    assert!(
        matches!(failed, Err(SyncError::Network(_))),
        "{op}: {failed:?}"
    );

    // The next sync on A finishes the rotation.
    sync(&a).await;
    assert_eq!(a.area().0.key_version, 2, "{op}");
    assert_eq!(engine(&b).await.err(), Some(SyncError::Revoked), "{op}");
    join(&server, &c, &export).await;
    sync(&c).await;
    assert_eq!(
        c.hosts().len(),
        3,
        "{op}: every host readable under the new key"
    );
    // A keeps writing under the new key.
    save(&a, &sample_host("four"), 200);
    sync(&a).await;
    sync(&c).await;
    assert_eq!(c.hosts().len(), 4, "{op}");
}

#[tokio::test]
async fn rotation_resumes_after_a_failed_log_append() {
    interrupted_rotation("append_log", 0).await;
}

#[tokio::test]
async fn rotation_resumes_after_a_failed_manifest() {
    interrupted_rotation("put_manifest", 0).await;
}

#[tokio::test]
async fn rotation_resumes_after_failed_envelopes() {
    interrupted_rotation("put_envelopes", 0).await;
}

#[tokio::test]
async fn rotation_resumes_midway_through_resealing() {
    interrupted_rotation("put_record", 1).await;
}

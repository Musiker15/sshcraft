mod common;

use common::{Dev, TestServer, account, create, engine, join, server_sql, stored_area};
use sshcraft_proto::ProtoError;
use sshcraft_proto::area::{AreaId, Member, Role, open_area_key};
use sshcraft_sync::SyncError;
use sshcraft_sync::areas::{create_area, set_members};
use sshcraft_sync::client::failpoints;
use sshcraft_sync::devices::revoke_device;

#[tokio::test]
async fn create_area_seals_to_every_member_device() {
    let server = TestServer::start().await;
    let (a, b, b2) = (Dev::new(), Dev::new(), Dev::new());
    create(&server, &a, "a@example.org").await;
    let export = create(&server, &b, "b@example.org").await;
    join(&server, &b2, &export).await;
    let e = engine(&a).await;
    let members = vec![Member { account: account(&b), role: Role::Read }];
    let (manifest, key) = create_area(&e, &a.store, members).await.unwrap();
    for dev in [&b, &b2] {
        let envelope = dev.client(&server).await.my_envelope(manifest.area_id).await.unwrap().expect("an envelope for every device of the member");
        let opened = open_area_key(&envelope, dev.row().device_id, &dev.keys(), &manifest).unwrap();
        assert!(opened.same_key_as(&key));
    }
    let (stored, _) = stored_area(&a, manifest.area_id.0);
    assert_eq!(stored, manifest);
}

#[tokio::test]
async fn foreign_log_rollback_is_refused() {
    let server = TestServer::start().await;
    let (a, b, b2) = (Dev::new(), Dev::new(), Dev::new());
    create(&server, &a, "a@example.org").await;
    let export = create(&server, &b, "b@example.org").await;
    join(&server, &b2, &export).await;
    let e = engine(&a).await;
    let seen = e.account_devices(&a.store, &account(&b)).await.unwrap();
    assert!(seen.get_active(b2.row().device_id).is_some());
    // The server drops the last entry of B's log (the second device).
    server_sql(
        &server,
        "DELETE FROM log_entries WHERE account = ?1 AND seq = (SELECT MAX(seq) FROM log_entries WHERE account = ?1)",
        [account(&b).0.to_vec()],
    );
    assert_eq!(e.account_devices(&a.store, &account(&b)).await.unwrap_err(), SyncError::Proto(ProtoError::Rollback));
}

#[tokio::test]
async fn revoke_rotates_every_owned_area() {
    let server = TestServer::start().await;
    let (a, a2, b) = (Dev::new(), Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &a2, &export).await;
    create(&server, &b, "b@example.org").await;
    let mut e = engine(&a).await;
    e.pull(&a.store, &a.vault).await.unwrap();
    let members = vec![Member { account: account(&b), role: Role::Write }];
    let (shared, _) = create_area(&e, &a.store, members).await.unwrap();
    revoke_device(&mut e, &a.store, &a.vault, a2.row().device_id, 10).await.unwrap();
    let server_id = a.row().id;
    let areas = a.store.lock().unwrap().areas(server_id).unwrap();
    assert_eq!(areas.len(), 2);
    for area in areas {
        let (manifest, _) = stored_area(&a, area.area_id);
        assert_eq!(manifest.key_version, 2, "area {}", area.area_id);
    }
    // B still reads the share under the new key; the revoked device does not.
    let (manifest, key) = stored_area(&a, shared.area_id.0);
    let envelope = b.client(&server).await.my_envelope(shared.area_id).await.unwrap().unwrap();
    assert!(open_area_key(&envelope, b.row().device_id, &b.keys(), &manifest).unwrap().same_key_as(&key));
}

#[tokio::test]
async fn rotation_resumes_for_a_shared_area() {
    let server = TestServer::start().await;
    let (a, b, c) = (Dev::new(), Dev::new(), Dev::new());
    create(&server, &a, "a@example.org").await;
    create(&server, &b, "b@example.org").await;
    create(&server, &c, "c@example.org").await;
    let mut e = engine(&a).await;
    let both = vec![Member { account: account(&b), role: Role::Read }, Member { account: account(&c), role: Role::Read }];
    let (manifest, _) = create_area(&e, &a.store, both.clone()).await.unwrap();
    failpoints::arm(&server.origin, "put_envelopes", 0);
    let failed = set_members(&e, &a.store, &a.vault, manifest.area_id, vec![both[0].clone()], 5).await;
    assert!(matches!(failed, Err(SyncError::Network(_))), "{failed:?}");
    e.pull(&a.store, &a.vault).await.unwrap();
    let (rotated, key) = stored_area(&a, manifest.area_id.0);
    assert_eq!(rotated.key_version, 2);
    assert_eq!(rotated.members, vec![both[0].clone()]);
    let removed = c.client(&server).await.my_envelope(manifest.area_id).await;
    assert!(matches!(removed, Ok(None) | Err(_)), "{removed:?}");
    let kept = b.client(&server).await.my_envelope(manifest.area_id).await.unwrap().unwrap();
    assert!(open_area_key(&kept, b.row().device_id, &b.keys(), &rotated).unwrap().same_key_as(&key));
    let _: AreaId = manifest.area_id;
}

#[tokio::test]
async fn items_last_written_by_a_revoked_device_survive_the_rotation() {
    let server = TestServer::start().await;
    let (a, a2, a3) = (Dev::new(), Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &a2, &export).await;
    let server_a2 = a2.row().id;
    let mut host = common::sample_host("from-a2");
    host.location = Some(server_a2);
    {
        let store = a2.store.lock().unwrap();
        store.upsert_host(&host).unwrap();
        sshcraft_sync::engine::record_local_change(&store, sshcraft_store::store::ItemKind::Host, &host.id.0.to_string(), 5).unwrap();
    }
    common::sync(&a2).await;
    common::sync(&a).await;
    let mut e = engine(&a).await;
    revoke_device(&mut e, &a.store, &a.vault, a2.row().device_id, 10).await.unwrap();
    join(&server, &a3, &export).await;
    common::sync(&a3).await;
    assert_eq!(a3.hosts().len(), 1, "the host written by the revoked device is still there");
}

#[tokio::test]
async fn one_lock_per_server() {
    use sshcraft_model::model::ServerId;
    use sshcraft_sync::engine::server_lock;
    let (one, other) = (ServerId::new(), ServerId::new());
    assert!(std::sync::Arc::ptr_eq(&server_lock(one), &server_lock(one)));
    assert!(!std::sync::Arc::ptr_eq(&server_lock(one), &server_lock(other)));
    let held = server_lock(one).lock_owned().await;
    assert!(server_lock(one).try_lock().is_err(), "a second user of the same server waits");
    assert!(server_lock(other).try_lock().is_ok());
    drop(held);
}

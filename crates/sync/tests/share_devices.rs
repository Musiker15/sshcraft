mod common;

use common::{Dev, TestServer, create, engine, join, member, stored_area, sync};
use sshcraft_proto::area::{AreaId, Role, open_area_key};
use sshcraft_sync::areas::{create_area, reseal_envelopes};
use sshcraft_sync::devices::revoke_device;

async fn envelope_count(server: &TestServer, area: AreaId, device: uuid::Uuid) -> bool {
    server.store().await.envelope_for(area, device).await.unwrap().is_some()
}

#[tokio::test]
async fn members_new_device_can_read_after_the_owners_next_sync() {
    let server = TestServer::start().await;
    let (a, b, b2) = (Dev::new(), Dev::new(), Dev::new());
    create(&server, &a, "a@example.org").await;
    let export = create(&server, &b, "b@example.org").await;
    let e = engine(&a).await;
    let (manifest, key) = create_area(&e, &a.store, vec![member(&b, Role::Read)]).await.unwrap();
    join(&server, &b2, &export).await;
    assert!(b2.client(&server).await.my_envelope(manifest.area_id).await.unwrap().is_none());
    sync(&a).await;
    let envelope = b2.client(&server).await.my_envelope(manifest.area_id).await.unwrap().expect("sealed for the new device");
    let opened = open_area_key(&envelope, b2.row().device_id, &b2.keys(), &manifest).unwrap();
    assert!(opened.same_key_as(&key));
}

#[tokio::test]
async fn members_revoked_device_cannot_read_new_data() {
    let server = TestServer::start().await;
    let (a, b, b2) = (Dev::new(), Dev::new(), Dev::new());
    create(&server, &a, "a@example.org").await;
    let export = create(&server, &b, "b@example.org").await;
    join(&server, &b2, &export).await;
    let e = engine(&a).await;
    let (manifest, _) = create_area(&e, &a.store, vec![member(&b, Role::Read)]).await.unwrap();
    let gone = b2.row().device_id;
    assert!(envelope_count(&server, manifest.area_id, gone).await);
    let mut eb = engine(&b).await;
    revoke_device(&mut eb, &b.store, &b.vault, gone, 10).await.unwrap();
    sync(&a).await;
    let (rotated, key) = stored_area(&a, manifest.area_id.0);
    assert_eq!(rotated.key_version, 2);
    assert!(!envelope_count(&server, manifest.area_id, gone).await);
    let kept = b.client(&server).await.my_envelope(manifest.area_id).await.unwrap().unwrap();
    assert!(open_area_key(&kept, b.row().device_id, &b.keys(), &rotated).unwrap().same_key_as(&key));
}

#[tokio::test]
async fn nothing_to_do_without_changes() {
    let server = TestServer::start().await;
    let (a, b) = (Dev::new(), Dev::new());
    create(&server, &a, "a@example.org").await;
    create(&server, &b, "b@example.org").await;
    let e = engine(&a).await;
    let (manifest, _) = create_area(&e, &a.store, vec![member(&b, Role::Read)]).await.unwrap();
    sync(&a).await;
    sync(&a).await;
    assert_eq!(stored_area(&a, manifest.area_id.0).0.key_version, 1);
    assert_eq!(reseal_envelopes(&e, &a.store, manifest.area_id).await.unwrap(), 0);
}

#[tokio::test]
async fn a_revocation_is_noticed_by_an_owner_device_that_missed_the_last_rotation() {
    let server = TestServer::start().await;
    let (a, a2, a3, b, b2) = (Dev::new(), Dev::new(), Dev::new(), Dev::new(), Dev::new());
    let export_a = create(&server, &a, "a@example.org").await;
    join(&server, &a2, &export_a).await;
    join(&server, &a3, &export_a).await;
    let export_b = create(&server, &b, "b@example.org").await;
    join(&server, &b2, &export_b).await;
    let mut ea = engine(&a).await;
    let (manifest, _) = create_area(&ea, &a.store, vec![member(&b, Role::Read)]).await.unwrap();
    sync(&a2).await;
    // A rotates without A2 (revoking A3), then B revokes its second device.
    revoke_device(&mut ea, &a.store, &a.vault, a3.row().device_id, 10).await.unwrap();
    let gone = b2.row().device_id;
    assert!(envelope_count(&server, manifest.area_id, gone).await);
    let mut eb = engine(&b).await;
    revoke_device(&mut eb, &b.store, &b.vault, gone, 11).await.unwrap();
    // Only A2 syncs.
    sync(&a2).await;
    assert_eq!(stored_area(&a2, manifest.area_id.0).0.key_version, 3);
    assert!(!envelope_count(&server, manifest.area_id, gone).await);
}

#[tokio::test]
async fn one_members_bad_log_does_not_stop_the_owners_sync() {
    let server = TestServer::start().await;
    let (a, b, c, c2) = (Dev::new(), Dev::new(), Dev::new(), Dev::new());
    create(&server, &a, "a@example.org").await;
    create(&server, &b, "b@example.org").await;
    let export_c = create(&server, &c, "c@example.org").await;
    join(&server, &c2, &export_c).await;
    let e = engine(&a).await;
    create_area(&e, &a.store, vec![member(&b, Role::Read), member(&c, Role::Read)]).await.unwrap();
    sync(&a).await;
    common::server_sql(
        &server,
        "DELETE FROM log_entries WHERE account = ?1 AND seq = (SELECT MAX(seq) FROM log_entries WHERE account = ?1)",
        [common::account(&c).0.to_vec()],
    );
    let mut e = engine(&a).await;
    let report = e.pull(&a.store, &a.vault).await.expect("the pull goes on");
    assert!(!report.rejected.is_empty(), "{report:?}");
}

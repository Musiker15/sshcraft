mod common;

use common::{Dev, TestServer, create, join, revoke_raw, sample_host, write_host};
use sshcraft_sync::SyncError;
use sshcraft_sync::engine::Engine;
use uuid::Uuid;

async fn pull(dev: &Dev) -> Result<sshcraft_sync::engine::PullReport, SyncError> {
    let mut engine = Engine::load(&dev.store, &dev.vault, dev.row().id).await?;
    engine.pull(&dev.store, &dev.vault).await
}

#[tokio::test]
async fn pull_applies_records_from_another_device() {
    let server = TestServer::start().await;
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &b, &export).await;
    let host = sample_host("web");
    write_host(&server, &a, &host, Uuid::new_v4(), 1, 100).await;

    let report = pull(&b).await.unwrap();
    assert_eq!((report.applied, report.rejected.len()), (1, 0));
    let hosts = b.hosts();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "web");
    assert_eq!(hosts[0].location, Some(b.row().id));
}

#[tokio::test]
async fn pull_is_idempotent() {
    let server = TestServer::start().await;
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &b, &export).await;
    write_host(&server, &a, &sample_host("web"), Uuid::new_v4(), 1, 100).await;
    assert_eq!(pull(&b).await.unwrap().applied, 1);
    let again = pull(&b).await.unwrap();
    assert_eq!((again.applied, again.rejected.len()), (0, 0));
    assert_eq!(b.hosts().len(), 1);
}

#[tokio::test]
async fn pull_rejects_a_rollback() {
    let server = TestServer::start().await;
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &b, &export).await;
    let mut host = sample_host("old");
    let id = Uuid::new_v4();
    let first = write_host(&server, &a, &host, id, 1, 100).await;
    host.name = "new".into();
    write_host(&server, &a, &host, id, 2, 200).await;
    pull(&b).await.unwrap();
    assert_eq!(b.hosts()[0].name, "new");

    // The server hands out the old version again, under a later change number.
    let path = server
        .database_url
        .trim_start_matches("sqlite://")
        .to_owned();
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute(
        "UPDATE records SET record = ?1, version = 1, seq = 100 WHERE record_id = ?2",
        rusqlite::params![serde_json::to_string(&first).unwrap(), id.to_string()],
    )
    .unwrap();

    let report = pull(&b).await.unwrap();
    assert_eq!(report.applied, 0);
    assert_eq!(report.rejected.len(), 1, "{:?}", report.rejected);
    assert_eq!(b.hosts()[0].name, "new");
}

#[tokio::test]
async fn pull_rejects_a_record_from_a_revoked_device() {
    let server = TestServer::start().await;
    let (a, c, d) = (Dev::new(), Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &c, &export).await;
    write_host(&server, &c, &sample_host("planted"), Uuid::new_v4(), 1, 100).await;
    revoke_raw(&server, &a, c.row().device_id).await;

    join(&server, &d, &export).await;
    let report = pull(&d).await.unwrap();
    assert_eq!((report.applied, report.rejected.len()), (0, 1));
    assert!(d.hosts().is_empty());
}

#[tokio::test]
async fn revoked_device_stops_and_keeps_its_data() {
    let server = TestServer::start().await;
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(&server, &a, "a@example.org").await;
    join(&server, &b, &export).await;
    write_host(&server, &a, &sample_host("web"), Uuid::new_v4(), 1, 100).await;
    pull(&b).await.unwrap();

    revoke_raw(&server, &a, b.row().device_id).await;
    assert_eq!(pull(&b).await.unwrap_err(), SyncError::Revoked);
    assert!(b.row().revoked);
    assert_eq!(b.hosts().len(), 1, "local data stays");
}

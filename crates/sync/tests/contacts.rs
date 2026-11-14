mod common;

use common::{Dev, TestServer, account, create, engine, server_sql};
use sshcraft_proto::identity::safety_number;
use sshcraft_sync::SyncError;
use sshcraft_sync::contacts::{KnownState, confirm, find, require_confirmed};

#[tokio::test]
async fn new_recipient_shows_the_safety_number() {
    let server = TestServer::start().await;
    let (a, b) = (Dev::new(), Dev::new());
    create(&server, &a, "a@example.org").await;
    create(&server, &b, "b@example.org").await;
    let e = engine(&a).await;
    let found = find(&e, &a.store, "  B@Example.org ").await.unwrap();
    assert_eq!(found.account, account(&b));
    assert_eq!(found.email, "b@example.org");
    assert_eq!(found.safety_number, safety_number(&account(&b)));
    assert_eq!(found.known, KnownState::New);
    let server_id = a.row().id;
    assert_eq!(require_confirmed(&a.store, server_id, &account(&b)), Err(SyncError::ContactUnconfirmed));
    confirm(&a.store, server_id, "b@example.org", &account(&b)).unwrap();
    require_confirmed(&a.store, server_id, &account(&b)).unwrap();
    let again = find(&e, &a.store, "b@example.org").await.unwrap();
    assert_eq!(again.known, KnownState::Confirmed);
}

#[tokio::test]
async fn changed_key_is_reported_and_not_trusted() {
    let server = TestServer::start().await;
    let (a, b, c) = (Dev::new(), Dev::new(), Dev::new());
    create(&server, &a, "a@example.org").await;
    create(&server, &b, "b@example.org").await;
    create(&server, &c, "c@example.org").await;
    let e = engine(&a).await;
    let server_id = a.row().id;
    find(&e, &a.store, "b@example.org").await.unwrap();
    confirm(&a.store, server_id, "b@example.org", &account(&b)).unwrap();
    // A taken-over server answers the lookup for B's e-mail with another account.
    server_sql(&server, "UPDATE accounts SET email_norm = 'moved@example.org' WHERE account = ?1", [account(&b).0.to_vec()]);
    server_sql(&server, "UPDATE accounts SET email_norm = 'b@example.org' WHERE account = ?1", [account(&c).0.to_vec()]);
    let found = find(&e, &a.store, "b@example.org").await.unwrap();
    assert_eq!(found.account, account(&c));
    assert_eq!(found.known, KnownState::Changed { previous_safety_number: safety_number(&account(&b)) });
    assert_eq!(require_confirmed(&a.store, server_id, &account(&c)), Err(SyncError::ContactUnconfirmed));
    // The known key stays what it was until the user accepts the new one.
    require_confirmed(&a.store, server_id, &account(&b)).unwrap();
    let still = find(&e, &a.store, "b@example.org").await.unwrap();
    assert!(matches!(still.known, KnownState::Changed { .. }));
    confirm(&a.store, server_id, "b@example.org", &account(&c)).unwrap();
    require_confirmed(&a.store, server_id, &account(&c)).unwrap();
}

#[tokio::test]
async fn sharing_with_oneself_is_refused() {
    let server = TestServer::start().await;
    let a = Dev::new();
    create(&server, &a, "a@example.org").await;
    let e = engine(&a).await;
    assert_eq!(find(&e, &a.store, "a@example.org").await.unwrap_err(), SyncError::NotAllowed);
}

#[tokio::test]
async fn unknown_email_is_not_found() {
    let server = TestServer::start().await;
    let a = Dev::new();
    create(&server, &a, "a@example.org").await;
    let e = engine(&a).await;
    let err = find(&e, &a.store, "nobody@example.org").await.unwrap_err();
    assert_eq!(err.server_code(), Some("not_found"));
}

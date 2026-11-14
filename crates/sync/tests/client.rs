mod common;

use common::{RawAccount, TestServer};
use sshcraft_sync::SyncError;
use sshcraft_sync::client::ServerClient;

#[tokio::test]
async fn server_info_matches_the_origin() {
    let server = TestServer::start().await;
    let client = ServerClient::new(&server.origin).unwrap();
    let info = client.server_info().await.unwrap();
    assert_eq!(info.api, 1);
    assert_eq!(info.origin, server.origin);
}

#[tokio::test]
async fn register_and_login_round_trip() {
    let server = TestServer::start().await;
    let account = RawAccount::new();
    let client = ServerClient::new(&server.origin)
        .unwrap()
        .with_api_key(server.api_key.clone());
    client
        .register(
            &account.key.public(),
            "Alice",
            "alice@example.org",
            &account.log,
        )
        .await
        .unwrap();
    let mut client = client;
    let session = client
        .login(account.device_id, &account.keys)
        .await
        .unwrap();
    assert!(!session.token.is_empty());
    let me = client.me().await.unwrap();
    assert_eq!(
        (me.name.as_str(), me.account),
        ("Alice", account.key.public())
    );
    let log = client.log(&account.key.public(), 0).await.unwrap();
    assert_eq!(log, account.log.entries);
}

#[tokio::test]
async fn server_errors_keep_their_code() {
    let server = TestServer::start().await;
    let client = ServerClient::new(&server.origin)
        .unwrap()
        .with_api_key(server.api_key.clone());
    let first = RawAccount::new();
    client
        .register(&first.key.public(), "A", "same@example.org", &first.log)
        .await
        .unwrap();
    let second = RawAccount::new();
    let err = client
        .register(&second.key.public(), "B", "same@example.org", &second.log)
        .await
        .unwrap_err();
    match err {
        SyncError::Server { status, code, .. } => {
            assert_eq!((status, code.as_str()), (409, "email_taken"))
        }
        other => panic!("{other:?}"),
    }
    let wrong = ServerClient::new(&server.origin)
        .unwrap()
        .with_api_key("nope".into());
    let third = RawAccount::new();
    let err = wrong
        .register(&third.key.public(), "C", "c@example.org", &third.log)
        .await
        .unwrap_err();
    assert_eq!(err.server_code(), Some("bad_api_key"));
}

#[tokio::test]
async fn unreachable_server_is_a_network_error() {
    let port = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let client = ServerClient::new(&format!("http://127.0.0.1:{port}")).unwrap();
    assert!(matches!(
        client.server_info().await,
        Err(SyncError::Network(_))
    ));
}

#[test]
fn origin_must_be_http() {
    assert!(matches!(
        ServerClient::new("ftp://sync.example.org"),
        Err(SyncError::BadOrigin)
    ));
    assert!(ServerClient::new("sync.example.org").is_err());
    let client = ServerClient::new("HTTPS://Sync.Example.org:443/").unwrap();
    assert_eq!(client.origin(), "https://sync.example.org");
    assert_eq!(client.events_url(), "wss://sync.example.org/v1/events");
}

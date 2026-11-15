//! The wire's limits: line sizes, connections, a silent client, requests side by side.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::json;
use sshcraft_link::auth::{client_proof, nonce};
use sshcraft_link::{Client, LinkError, MAX_CONNECTIONS, Server, ServerOptions, Token};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::{Echo, addr, echo_server};

/// A raw connection signed in with `token`.
async fn signed_in(server: &Server, token: &Token) -> (BufReader<tokio::net::tcp::OwnedReadHalf>, tokio::net::tcp::OwnedWriteHalf) {
    let (read, mut write) = TcpStream::connect(addr(server)).await.unwrap().into_split();
    let mut read = BufReader::new(read);
    let cn = nonce();
    write.write_all(format!("{}\n", json!({"id": 1, "method": "auth.challenge", "params": {"clientNonce": cn}})).as_bytes()).await.unwrap();
    let mut text = String::new();
    read.read_line(&mut text).await.unwrap();
    let sn = serde_json::from_str::<serde_json::Value>(&text).unwrap()["result"]["serverNonce"].as_str().unwrap().to_string();
    write
        .write_all(format!("{}\n", json!({"id": 2, "method": "auth", "params": {"proof": client_proof(token, &cn, &sn)}})).as_bytes())
        .await
        .unwrap();
    text.clear();
    read.read_line(&mut text).await.unwrap();
    (read, write)
}

#[tokio::test]
async fn an_oversized_line_closes_the_connection() {
    let (server, handler, token) = echo_server().await;
    let (mut read, mut write) = signed_in(&server, &token).await;
    let big = vec![b'x'; (1 << 20) + 10];
    let _ = write.write_all(&big).await;
    let _ = write.write_all(b"\n").await;
    let mut rest = Vec::new();
    let read_all = tokio::time::timeout(Duration::from_secs(5), read.read_to_end(&mut rest)).await;
    assert!(read_all.is_ok(), "the server closed the connection");
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_oversized_reply_becomes_an_error() {
    let (server, _handler, token) = echo_server().await;
    let client = Client::connect(addr(&server), &token).await.unwrap();
    match client.call("big", json!({})).await {
        Err(LinkError::Remote(message)) => assert!(message.contains("too large"), "{message}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(client.call("ping", json!({})).await.unwrap(), json!("pong"), "the connection goes on");
}

#[tokio::test]
async fn the_seventeenth_connection_is_refused() {
    let (server, _handler, token) = echo_server().await;
    let mut clients = Vec::new();
    for _ in 0..MAX_CONNECTIONS {
        clients.push(Client::connect(addr(&server), &token).await.unwrap());
    }
    assert!(Client::connect(addr(&server), &token).await.is_err());
    drop(clients.pop());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(Client::connect(addr(&server), &token).await.is_ok(), "a freed place takes the next");
}

#[tokio::test]
async fn a_silent_client_is_dropped_after_the_sign_in_time() {
    let token = Token::generate();
    let options = ServerOptions { auth_timeout: Duration::from_millis(200) };
    let server = Server::bind_with(0, token, Arc::new(Echo::default()), options).await.unwrap();
    let mut stream = TcpStream::connect(addr(&server)).await.unwrap();
    let mut rest = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut rest)).await;
    assert!(read.is_ok(), "closed after the sign-in time");
}

#[tokio::test]
async fn requests_run_side_by_side_and_cancel() {
    let (server, handler, token) = echo_server().await;
    let client = Client::connect(addr(&server), &token).await.unwrap();
    let waiting = tokio::time::timeout(Duration::from_millis(300), client.call("wait", json!({})));
    let (waited, pinged) = tokio::join!(waiting, client.call("ping", json!({})));
    assert!(waited.is_err(), "the waiting request did not answer");
    assert_eq!(pinged.unwrap(), json!("pong"), "it did not hold up the other");
    // The dropped call reaches the server as a cancellation.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while handler.cancelled.load(Ordering::SeqCst) == 0 {
        assert!(std::time::Instant::now() < deadline, "not cancelled");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_closed_connection_is_reported_and_stop_drops_all() {
    let (server, handler, token) = echo_server().await;
    let client = Client::connect(addr(&server), &token).await.unwrap();
    drop(client);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while handler.closed.load(Ordering::SeqCst) == 0 {
        assert!(std::time::Instant::now() < deadline, "not reported");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let client = Client::connect(addr(&server), &token).await.unwrap();
    server.stop();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(client.call("ping", json!({})).await.is_err(), "stopping cut it off");
    assert_eq!(handler.closed.load(Ordering::SeqCst), 2);
}

async fn answer_of(read: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> serde_json::Value {
    let mut text = String::new();
    tokio::time::timeout(Duration::from_secs(5), read.read_line(&mut text)).await.unwrap().unwrap();
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn an_id_still_running_is_refused_and_the_cap_holds() {
    let (server, handler, token) = echo_server().await;
    let (mut read, mut write) = signed_in(&server, &token).await;
    let wait = |id: u64| format!("{}\n", json!({"id": id, "method": "wait"}));
    write.write_all(wait(5).as_bytes()).await.unwrap();
    write.write_all(wait(5).as_bytes()).await.unwrap();
    let answer = answer_of(&mut read).await;
    assert_eq!(answer["id"], 5);
    assert_eq!(answer["error"], "a request with this id is still running");
    for id in 100..(100 + sshcraft_link::MAX_RUNNING as u64 - 1) {
        write.write_all(wait(id).as_bytes()).await.unwrap();
    }
    write.write_all(wait(999).as_bytes()).await.unwrap();
    let answer = answer_of(&mut read).await;
    assert_eq!(answer["id"], 999);
    assert_eq!(answer["error"], "too many requests at once");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(handler.calls.load(Ordering::SeqCst), sshcraft_link::MAX_RUNNING, "only what fits runs");
}

#[tokio::test]
async fn silent_connections_cannot_keep_a_client_out() {
    let (server, _handler, token) = echo_server().await;
    // Something on the computer opens connections and never signs in.
    let mut silent = Vec::new();
    for _ in 0..(MAX_CONNECTIONS * 2) {
        silent.push(TcpStream::connect(addr(&server)).await.unwrap());
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let client = Client::connect(addr(&server), &token).await;
    assert!(client.is_ok(), "a client that signs in gets a place: {client:?}");
}

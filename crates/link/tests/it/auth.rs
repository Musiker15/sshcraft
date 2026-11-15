//! Signing in: the server proves it holds the token before the client shows anything derived
//! from it, and nothing runs before the sign-in.

use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::{Value, json};
use sshcraft_link::auth::{client_proof, nonce};
use sshcraft_link::{Client, LinkError, Token};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::{addr, echo_server};

async fn line(read: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> Option<Value> {
    let mut text = String::new();
    match tokio::time::timeout(Duration::from_secs(5), read.read_line(&mut text)).await {
        Ok(Ok(0)) | Err(_) | Ok(Err(_)) => None,
        Ok(Ok(_)) => serde_json::from_str(&text).ok(),
    }
}

#[tokio::test]
async fn a_client_and_server_agree() {
    let (server, _handler, token) = echo_server().await;
    let client = Client::connect(addr(&server), &token).await.unwrap();
    assert_eq!(client.call("ping", json!({})).await.unwrap(), json!("pong"));
    assert_eq!(client.call("echo", json!({"a": 1})).await.unwrap(), json!({"a": 1}));
}

#[tokio::test]
async fn a_wrong_token_is_refused_both_ways() {
    let (server, handler, token) = echo_server().await;
    // The client checks the server's proof first: with another token it stops there.
    let other = Token::generate();
    assert!(matches!(Client::connect(addr(&server), &other).await, Err(LinkError::Refused(_))));
    // A client that sends a wrong proof is refused and cut off.
    let (read, mut write) = TcpStream::connect(addr(&server)).await.unwrap().into_split();
    let mut read = BufReader::new(read);
    let client_nonce = nonce();
    write.write_all(format!("{}\n", json!({"id": 1, "method": "auth.challenge", "params": {"clientNonce": client_nonce}})).as_bytes()).await.unwrap();
    let challenge = line(&mut read).await.unwrap();
    let server_nonce = challenge["result"]["serverNonce"].as_str().unwrap().to_string();
    let wrong = client_proof(&other, &client_nonce, &server_nonce);
    write.write_all(format!("{}\n", json!({"id": 2, "method": "auth", "params": {"proof": wrong}})).as_bytes()).await.unwrap();
    let answer = line(&mut read).await.unwrap();
    assert_eq!(answer["ok"], false);
    assert_eq!(answer["error"], "authentication required");
    assert!(line(&mut read).await.is_none(), "the connection is closed");
    // The right proof gets in.
    let (read, mut write) = TcpStream::connect(addr(&server)).await.unwrap().into_split();
    let mut read = BufReader::new(read);
    let client_nonce = nonce();
    write.write_all(format!("{}\n", json!({"id": 1, "method": "auth.challenge", "params": {"clientNonce": client_nonce}})).as_bytes()).await.unwrap();
    let challenge = line(&mut read).await.unwrap();
    let server_nonce = challenge["result"]["serverNonce"].as_str().unwrap().to_string();
    let right = client_proof(&token, &client_nonce, &server_nonce);
    write.write_all(format!("{}\n", json!({"id": 2, "method": "auth", "params": {"proof": right}})).as_bytes()).await.unwrap();
    assert_eq!(line(&mut read).await.unwrap()["result"]["authenticated"], true);
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0, "signing in runs nothing");
}

#[tokio::test]
async fn a_listener_without_the_token_learns_nothing() {
    // Something else took the port of a stale connection file.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let recorder = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut read = BufReader::new(read);
        let mut first = String::new();
        read.read_line(&mut first).await.unwrap();
        // It cannot know the token: its proof is made up.
        let fake = json!({"id": 1, "ok": true, "result": {"serverNonce": nonce(), "serverProof": "00".repeat(32)}});
        write.write_all(format!("{fake}\n").as_bytes()).await.unwrap();
        // Line by line: a cancelled read would lose what it read so far.
        let mut seen = first;
        loop {
            let mut next = String::new();
            match tokio::time::timeout(Duration::from_secs(2), read.read_line(&mut next)).await {
                Ok(Ok(n)) if n > 0 => seen.push_str(&next),
                _ => break,
            }
        }
        seen
    });
    let token = Token::generate();
    let result = Client::connect(std::net::SocketAddr::from(([127, 0, 0, 1], port)), &token).await;
    assert!(matches!(result, Err(LinkError::Refused(_))), "{result:?}");
    let seen = recorder.await.unwrap();
    assert!(!seen.contains(token.hex()), "the token never left");
    assert_eq!(seen.lines().count(), 1, "only the challenge was sent: {seen}");
    assert!(!seen.contains("proof"), "nothing derived from the token was sent: {seen}");
}

#[tokio::test]
async fn nothing_runs_before_auth() {
    let (server, handler, _token) = echo_server().await;
    let (read, mut write) = TcpStream::connect(addr(&server)).await.unwrap().into_split();
    let mut read = BufReader::new(read);
    write.write_all(b"{\"id\":1,\"method\":\"ping\",\"params\":{}}\n").await.unwrap();
    let answer = line(&mut read).await.unwrap();
    assert_eq!(answer["error"], "authentication required");
    assert!(line(&mut read).await.is_none(), "the connection is closed");
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
}

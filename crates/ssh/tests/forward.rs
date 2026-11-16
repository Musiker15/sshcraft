use std::sync::Arc;
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey};
use sshcraft_ssh::testing::{TestAuth, TestServer, agent_opener};
use sshcraft_ssh::{Auth, Hop, HostKeyVerifier, NoPrompter, RouteParams, SessionOptions, connect_route};
use zeroize::Zeroizing;

struct Trust;

#[async_trait::async_trait]
impl HostKeyVerifier for Trust {
    async fn verify(&self, _: &str, _: &str, _: u16, _: &str) -> bool {
        true
    }
}

async fn server() -> TestServer {
    TestServer::start_with(TestAuth { password: Some("pw".into()), ..Default::default() }).await
}

fn hop(name: &str, port: u16) -> Hop {
    Hop {
        host_name: name.into(),
        address: "127.0.0.1".into(),
        port,
        username: "tester".into(),
        auth: Auth { key: None, password: Some(Zeroizing::new("pw".into())), certificate: None, totp: None },
    }
}

fn agent_key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

/// Waits until the server tried to use the forwarded agent; `None` if the client refused.
async fn forwarded(server: &TestServer) -> Option<usize> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = server.agent_attempt() {
            return result;
        }
        assert!(tokio::time::Instant::now() < deadline, "the server never tried the agent");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn open(hops: Vec<Hop>, options: SessionOptions) -> sshcraft_ssh::Session {
    connect_route(RouteParams { hops, proxy: None, options, connect_timeout: Duration::from_secs(10) }, Arc::new(Trust), Arc::new(NoPrompter))
        .await
        .unwrap()
}

#[tokio::test]
async fn env_vars_reach_server() {
    let s = server().await;
    let session = open(
        vec![hop("t", s.port)],
        SessionOptions { env: vec![("LANG".into(), "de_DE.UTF-8".into()), ("APP_MODE".into(), "test".into())], ..Default::default() },
    )
    .await;
    let _shell = session.open_shell(80, 24, "xterm-256color").await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while s.received_env().len() < 2 {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(s.received_env(), vec![("LANG".to_owned(), "de_DE.UTF-8".to_owned()), ("APP_MODE".to_owned(), "test".to_owned()),]);
}

#[tokio::test]
async fn forwarded_agent_lists_keys() {
    let s = server().await;
    let session = open(vec![hop("t", s.port)], SessionOptions { agent_forwarding: Some(agent_opener(&[agent_key()])), ..Default::default() }).await;
    let _shell = session.open_shell(80, 24, "xterm-256color").await.unwrap();
    assert_eq!(forwarded(&s).await, Some(1));
}

#[tokio::test]
async fn agent_channel_rejected_without_forwarding() {
    let s = server().await;
    let session = open(vec![hop("t", s.port)], SessionOptions::default()).await;
    let _shell = session.open_shell(80, 24, "xterm-256color").await.unwrap();
    assert_eq!(forwarded(&s).await, None);
}

#[tokio::test]
async fn jump_host_cannot_open_agent_channel() {
    let jump = server().await;
    let target = server().await;
    let session = open(
        vec![hop("jump", jump.port), hop("target", target.port)],
        SessionOptions { agent_forwarding: Some(agent_opener(&[agent_key()])), ..Default::default() },
    )
    .await;
    let _shell = session.open_shell(80, 24, "xterm-256color").await.unwrap();
    assert_eq!(forwarded(&jump).await, None);
    assert_eq!(forwarded(&target).await, Some(1));
}

/// A local "display": the bytes of the first connection, until it closes.
async fn display() -> (sshcraft_ssh::x11::Display, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut got = Vec::new();
        let mut chunk = [0u8; 256];
        // The forward keeps the connection open: read what arrives within a moment.
        while let Ok(Ok(n)) = tokio::time::timeout(Duration::from_millis(1500), tokio::io::AsyncReadExt::read(&mut socket, &mut chunk)).await {
            if n == 0 {
                break;
            }
            got.extend_from_slice(&chunk[..n]);
        }
        got
    });
    let display =
        sshcraft_ssh::x11::Display { name: format!("127.0.0.1:{port}"), target: sshcraft_ssh::x11::Target::Tcp("127.0.0.1".into(), port), screen: 0 };
    (display, task)
}

fn x11(display: sshcraft_ssh::x11::Display, cookie: Option<Vec<u8>>) -> SessionOptions {
    SessionOptions { x11: Some(sshcraft_ssh::x11::X11Options { display, cookie }), ..Default::default() }
}

#[tokio::test]
async fn x11_reaches_the_display_with_the_real_cookie() {
    let s = server().await;
    let (display, got) = display().await;
    let session = open(vec![hop("t", s.port)], x11(display, Some(vec![1, 2, 3, 4]))).await;
    let _shell = session.open_shell(80, 24, "xterm").await.unwrap();
    let mut want = sshcraft_ssh::testing::x11_setup(b"MIT-MAGIC-COOKIE-1", &[1, 2, 3, 4]);
    want.extend_from_slice(b"hello");
    assert_eq!(tokio::time::timeout(Duration::from_secs(10), got).await.unwrap().unwrap(), want);
    // The server never saw the real cookie.
    let (protocol, cookie) = s.x11_request().unwrap();
    assert_eq!(protocol, "MIT-MAGIC-COOKIE-1");
    assert_ne!(cookie, "01020304");
    assert_eq!(cookie.len(), 32);
}

#[tokio::test]
async fn x11_with_a_wrong_cookie_never_reaches_the_display() {
    let s = server().await;
    s.set_x11_cookie(vec![9; 16]);
    let (display, got) = display().await;
    let session = open(vec![hop("t", s.port)], x11(display, Some(vec![1, 2, 3, 4]))).await;
    let _shell = session.open_shell(80, 24, "xterm").await.unwrap();
    assert_eq!(tokio::time::timeout(Duration::from_secs(10), got).await.unwrap().unwrap(), Vec::<u8>::new());
}

#[tokio::test]
async fn an_x11_channel_before_the_request_is_refused() {
    let s = server().await;
    let (display, _got) = display().await;
    let _session = open(vec![hop("t", s.port)], x11(display, None)).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while s.x11_unasked().is_none() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(s.x11_unasked(), Some(false));
}

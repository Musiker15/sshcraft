use std::sync::Arc;
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ssh::{
    Auth, ConnectParams, Credential, HostKeyVerifier, NoPrompter, Session, SshError, connect,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct Trust;

#[async_trait::async_trait]
impl HostKeyVerifier for Trust {
    async fn verify(&self, _: &str, _: &str, _: u16, _: &str) -> bool {
        true
    }
}

async fn session_with(auth: impl FnOnce(TestAuth) -> TestAuth) -> (TestServer, Session) {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server = TestServer::start_with(auth(TestAuth {
        key: Some(key.public_key().clone()),
        ..Default::default()
    }))
    .await;
    let session = connect(
        ConnectParams {
            address: "127.0.0.1".into(),
            port: server.port,
            username: "tester".into(),
            auth: Auth::key(Credential::Key(key)),
        },
        Arc::new(Trust),
        Arc::new(NoPrompter),
    )
    .await
    .unwrap();
    (server, session)
}

async fn session() -> (TestServer, Session) {
    session_with(|a| a).await
}

/// An echo server on a free port; returns the port.
async fn echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut read, mut write) = stream.split();
                let _ = tokio::io::copy(&mut read, &mut write).await;
            });
        }
    });
    port
}

async fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Sends `text` and reads the same number of bytes back.
async fn round_trip(stream: &mut TcpStream, text: &[u8]) -> Vec<u8> {
    stream.write_all(text).await.unwrap();
    let mut back = vec![0u8; text.len()];
    tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut back))
        .await
        .expect("answer in time")
        .unwrap();
    back
}

#[tokio::test]
async fn local_forward_reaches_target() {
    let (_server, session) = session().await;
    let target = echo_server().await;
    let tunnel = session
        .forward_local("127.0.0.1", 0, "127.0.0.1", target)
        .await
        .unwrap();
    let mut client = TcpStream::connect(("127.0.0.1", tunnel.bound_port()))
        .await
        .unwrap();
    assert_eq!(
        round_trip(&mut client, b"hello tunnel").await,
        b"hello tunnel"
    );
    assert_eq!(tunnel.connections(), 1);
}

#[tokio::test]
async fn dynamic_forward_speaks_socks5() {
    let (_server, session) = session().await;
    let target = echo_server().await;
    let tunnel = session.forward_dynamic("127.0.0.1", 0).await.unwrap();
    let mut client = TcpStream::connect(("127.0.0.1", tunnel.bound_port()))
        .await
        .unwrap();
    client.write_all(&[5, 1, 0]).await.unwrap();
    let mut choice = [0u8; 2];
    client.read_exact(&mut choice).await.unwrap();
    assert_eq!(choice, [5, 0]);
    let mut request = vec![5, 1, 0, 3, 9];
    request.extend_from_slice(b"localhost");
    request.extend_from_slice(&target.to_be_bytes());
    client.write_all(&request).await.unwrap();
    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[..2], [5, 0]);
    assert_eq!(
        round_trip(&mut client, b"through socks").await,
        b"through socks"
    );
}

#[tokio::test]
async fn remote_forward_reaches_local_target() {
    let (_server, session) = session().await;
    let target = echo_server().await;
    let wanted = free_port().await;
    let tunnel = session
        .forward_remote("127.0.0.1", wanted, "127.0.0.1", target)
        .await
        .unwrap();
    assert_eq!(tunnel.bound_port(), wanted);
    // The test server listens on the requested port, as sshd would.
    let mut client = TcpStream::connect(("127.0.0.1", wanted)).await.unwrap();
    assert_eq!(
        round_trip(&mut client, b"from the server side").await,
        b"from the server side"
    );
}

#[tokio::test]
async fn bound_port_in_use_is_port_in_use() {
    let (_server, session) = session().await;
    let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = taken.local_addr().unwrap().port();
    let result = session
        .forward_local("127.0.0.1", port, "127.0.0.1", 9)
        .await;
    assert!(
        matches!(result, Err(SshError::PortInUse)),
        "{:?}",
        result.err()
    );
    let result = session.forward_dynamic("127.0.0.1", port).await;
    assert!(matches!(result, Err(SshError::PortInUse)));
}

#[tokio::test]
async fn refused_remote_forward_is_forward_refused() {
    let (_server, session) = session_with(|a| TestAuth {
        refuse_forwarding: true,
        ..a
    })
    .await;
    let result = session
        .forward_remote("127.0.0.1", free_port().await, "127.0.0.1", 9)
        .await;
    assert!(
        matches!(result, Err(SshError::ForwardRefused)),
        "{:?}",
        result.err()
    );
}

#[tokio::test]
async fn closed_resolves_when_server_stops() {
    let (mut server, session) = session().await;
    let closed = session.closed();
    server.stop();
    tokio::time::timeout(Duration::from_secs(10), closed)
        .await
        .expect("closed in time");
}

#[tokio::test]
async fn dropping_tunnel_frees_port_and_ends_connections() {
    let (_server, session) = session().await;
    let target = echo_server().await;
    let tunnel = session
        .forward_local("127.0.0.1", 0, "127.0.0.1", target)
        .await
        .unwrap();
    let port = tunnel.bound_port();
    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    assert_eq!(round_trip(&mut client, b"x").await, b"x");
    drop(tunnel);
    // The open connection ends ...
    let mut rest = Vec::new();
    let ended = tokio::time::timeout(Duration::from_secs(10), client.read_to_end(&mut rest)).await;
    assert!(ended.is_ok(), "the connection stayed open");
    // ... and the port can be bound again.
    let mut rebound = None;
    for _ in 0..50 {
        if let Ok(l) = TcpListener::bind(("127.0.0.1", port)).await {
            rebound = Some(l);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(rebound.is_some(), "port {port} still taken");
}

#[tokio::test]
async fn connections_the_server_refuses_are_counted() {
    let (_server, session) = session_with(|a| TestAuth {
        refuse_forwarding: true,
        ..a
    })
    .await;
    let tunnel = session
        .forward_local("127.0.0.1", 0, "127.0.0.1", 9)
        .await
        .unwrap();
    let mut client = TcpStream::connect(("127.0.0.1", tunnel.bound_port()))
        .await
        .unwrap();
    let mut rest = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(10), client.read_to_end(&mut rest)).await;
    for _ in 0..100 {
        if tunnel.refused() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(tunnel.refused(), 1);
    assert_eq!(tunnel.connections(), 0);
}

#[tokio::test]
async fn localhost_listens_on_ipv4_loopback() {
    let (_server, session) = session().await;
    let target = echo_server().await;
    let tunnel = session
        .forward_local("localhost", 0, "localhost", target)
        .await
        .unwrap();
    let mut client = TcpStream::connect(("127.0.0.1", tunnel.bound_port()))
        .await
        .unwrap();
    assert_eq!(round_trip(&mut client, b"v4").await, b"v4");
    // Where this machine has IPv6 loopback, the same port answers there too.
    if TcpListener::bind("[::1]:0").await.is_ok() {
        let mut client = TcpStream::connect(("::1", tunnel.bound_port()))
            .await
            .unwrap();
        assert_eq!(round_trip(&mut client, b"v6").await, b"v6");
    }
}

#[tokio::test]
async fn bracketed_addresses_are_accepted() {
    let (_server, session) = session().await;
    let target = echo_server().await;
    let tunnel = session
        .forward_local("[127.0.0.1]", 0, "[127.0.0.1]", target)
        .await
        .unwrap();
    let mut client = TcpStream::connect(("127.0.0.1", tunnel.bound_port()))
        .await
        .unwrap();
    assert_eq!(round_trip(&mut client, b"brackets").await, b"brackets");
}

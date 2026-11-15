//! Terrapin (CVE-2023-48795): a man in the middle drops or adds packets during the key
//! exchange, before any of it is protected. OpenSSH's "strict kex" closes that gap: with it, the
//! first packet must be the KEXINIT and sequence numbers start again after NEWKEYS. These tests
//! use a fake server speaking plain packets to check that SSHCraft offers strict kex and
//! refuses a packet slipped in before the server's KEXINIT, and (counter-check) accepts the same
//! packet when strict kex was not agreed.

use std::sync::Arc;
use std::time::Duration;

use sshcraft_ssh::{Auth, Hop, HostKeyVerifier, NoPrompter, RouteParams, SessionOptions, connect_route};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

struct Trusting;

#[async_trait::async_trait]
impl HostKeyVerifier for Trusting {
    async fn verify(&self, _: &str, _: &str, _: u16, _: &str) -> bool {
        true
    }
}

const MSG_IGNORE: u8 = 2;
const MSG_KEXINIT: u8 = 20;
const MSG_KEX_ECDH_INIT: u8 = 30;

/// A packet without encryption: length, padding length, payload, padding to a multiple of 8.
fn packet(payload: &[u8]) -> Vec<u8> {
    let mut padding = 8 - (payload.len() + 5) % 8;
    if padding < 4 {
        padding += 8;
    }
    let length = u32::try_from(payload.len() + padding + 1).unwrap();
    let mut out = length.to_be_bytes().to_vec();
    out.push(u8::try_from(padding).unwrap());
    out.extend_from_slice(payload);
    out.extend(std::iter::repeat_n(0u8, padding));
    out
}

fn name_list(out: &mut Vec<u8>, names: &str) {
    out.extend_from_slice(&u32::try_from(names.len()).unwrap().to_be_bytes());
    out.extend_from_slice(names.as_bytes());
}

fn kexinit(kex: &str) -> Vec<u8> {
    let mut p = vec![MSG_KEXINIT];
    p.extend_from_slice(&[7u8; 16]);
    for names in [kex, "ssh-ed25519", "aes128-ctr", "aes128-ctr", "hmac-sha2-256", "hmac-sha2-256", "none", "none", "", ""] {
        name_list(&mut p, names);
    }
    p.push(0);
    p.extend_from_slice(&0u32.to_be_bytes());
    p
}

/// Reads one plain packet and returns its payload; `None` when the client closed.
async fn read_packet(stream: &mut BufReader<tokio::net::TcpStream>) -> Option<Vec<u8>> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len).await.ok()?;
    let len = usize::try_from(u32::from_be_bytes(len)).ok()?;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await.ok()?;
    let padding = usize::from(*body.first()?);
    body.get(1..len.checked_sub(padding)?).map(<[u8]>::to_vec)
}

/// The kex algorithm names of a KEXINIT payload.
fn kex_names(payload: &[u8]) -> String {
    let len = usize::try_from(u32::from_be_bytes(payload[17..21].try_into().unwrap())).unwrap();
    String::from_utf8(payload[21..21 + len].to_vec()).unwrap()
}

/// What the fake server saw from the client.
struct Seen {
    client_kex: String,
    /// The first message the client sent after its KEXINIT, if any.
    next: Option<u8>,
}

/// Starts a fake server that sends an IGNORE first when `inject`, then its KEXINIT with `kex`.
async fn fake_server(kex: &'static str, inject: bool) -> (u16, tokio::task::JoinHandle<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(tcp);
        stream.get_mut().write_all(b"SSH-2.0-FakeServer_1.0\r\n").await.unwrap();
        let mut banner = String::new();
        stream.read_line(&mut banner).await.unwrap();
        let client_kexinit = read_packet(&mut stream).await.unwrap();
        assert_eq!(client_kexinit[0], MSG_KEXINIT);
        if inject {
            stream.get_mut().write_all(&packet(&[MSG_IGNORE, 0, 0, 0, 1, b'x'])).await.unwrap();
        }
        stream.get_mut().write_all(&packet(&kexinit(kex))).await.unwrap();
        let next = tokio::time::timeout(Duration::from_secs(5), read_packet(&mut stream)).await.ok().flatten().and_then(|p| p.first().copied());
        Seen { client_kex: kex_names(&client_kexinit), next }
    });
    (port, task)
}

async fn connect(port: u16) -> bool {
    let hop = Hop { host_name: "fake".into(), address: "127.0.0.1".into(), port, username: "u".into(), auth: Auth::default() };
    let params = RouteParams { hops: vec![hop], proxy: None, options: SessionOptions::default(), connect_timeout: Duration::from_secs(5) };
    let attempt = connect_route(params, Arc::new(Trusting), Arc::new(NoPrompter));
    // The fake server never finishes the exchange: an error or a hang both mean "not connected".
    matches!(tokio::time::timeout(Duration::from_secs(3), attempt).await, Ok(Ok(_)))
}

#[tokio::test]
async fn the_client_offers_strict_kex() {
    let (port, server) = fake_server("curve25519-sha256,kex-strict-s-v00@openssh.com", false).await;
    assert!(!connect(port).await);
    let seen = server.await.unwrap();
    assert!(seen.client_kex.split(',').any(|n| n == "kex-strict-c-v00@openssh.com"), "{}", seen.client_kex);
}

#[tokio::test]
async fn an_ignore_before_kexinit_is_refused_under_strict_kex() {
    let (port, server) = fake_server("curve25519-sha256,kex-strict-s-v00@openssh.com", true).await;
    assert!(!connect(port).await);
    let seen = server.await.unwrap();
    assert_ne!(seen.next, Some(MSG_KEX_ECDH_INIT), "the exchange must stop at the slipped-in packet");
}

/// Counter-check: without strict kex the same packet is allowed, so the client goes on with the
/// exchange. This shows the test above fails for the reason it names.
#[tokio::test]
async fn the_same_ignore_is_accepted_without_strict_kex() {
    let (port, server) = fake_server("curve25519-sha256", true).await;
    assert!(!connect(port).await);
    let seen = server.await.unwrap();
    assert_eq!(seen.next, Some(MSG_KEX_ECDH_INIT));
}

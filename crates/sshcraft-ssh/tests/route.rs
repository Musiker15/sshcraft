use std::sync::{Arc, Mutex};
use std::time::Duration;

use sshcraft_ssh::testing::{TestAuth, TestServer, spawn_http_proxy, spawn_socks5_proxy};
use sshcraft_ssh::{
    Auth, Hop, HostKeyVerifier, NoPrompter, ProxyConfig, ProxyTarget, RouteParams, SessionOptions,
    ShellEvent, SshError, client_config, connect_route,
};
use zeroize::Zeroizing;

/// Trusts every host key and notes which hop asked.
#[derive(Default)]
struct Noting {
    seen: Mutex<Vec<(String, String, u16)>>,
}

#[async_trait::async_trait]
impl HostKeyVerifier for Noting {
    async fn verify(&self, host_name: &str, address: &str, port: u16, _fp: &str) -> bool {
        self.seen
            .lock()
            .unwrap()
            .push((host_name.to_owned(), address.to_owned(), port));
        true
    }
}

async fn password_server(password: &str) -> TestServer {
    TestServer::start_with(TestAuth {
        password: Some(password.into()),
        ..Default::default()
    })
    .await
}

fn hop(name: &str, address: &str, port: u16, password: &str) -> Hop {
    Hop {
        host_name: name.into(),
        address: address.into(),
        port,
        username: "tester".into(),
        auth: Auth {
            key: None,
            password: Some(Zeroizing::new(password.to_owned())),
            certificate: None,
        },
    }
}

fn route(hops: Vec<Hop>, proxy: Option<ProxyConfig>) -> RouteParams {
    RouteParams {
        hops,
        proxy,
        options: SessionOptions::default(),
        connect_timeout: Duration::from_secs(10),
    }
}

fn target(port: u16, credentials: Option<(&str, &str)>) -> ProxyTarget {
    ProxyTarget {
        address: "127.0.0.1".into(),
        port,
        username: credentials.map(|(u, _)| u.to_owned()),
        password: credentials.map(|(_, p)| Zeroizing::new(p.to_owned())),
    }
}

/// Opens a shell and checks an echo comes back: the session really reaches the target.
async fn echoes(session: &sshcraft_ssh::Session) {
    let mut shell = session.open_shell(80, 24, "xterm-256color").await.unwrap();
    shell.write(b"ping\n").await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut got = Vec::new();
    while !got.windows(4).any(|w| w == b"ping") {
        match tokio::time::timeout_at(deadline, shell.events.recv()).await {
            Ok(Some(ShellEvent::Data(d))) => got.extend_from_slice(&d),
            other => panic!("no echo: {other:?}"),
        }
    }
}

async fn go(params: RouteParams, verifier: Arc<Noting>) -> Result<sshcraft_ssh::Session, SshError> {
    connect_route(params, verifier, Arc::new(NoPrompter)).await
}

#[test]
fn kex_prefers_mlkem() {
    assert_eq!(
        client_config().preferred.kex[0],
        russh::kex::MLKEM768X25519_SHA256
    );
}

#[tokio::test]
async fn through_http_proxy() {
    let server = password_server("pw").await;
    let proxy = spawn_http_proxy(None).await;
    let params = route(
        vec![hop("t", "127.0.0.1", server.port, "pw")],
        Some(ProxyConfig::Http(target(proxy.port, None))),
    );
    let session = go(params, Arc::default()).await.unwrap();
    echoes(&session).await;
    assert_eq!(proxy.connects(), 1);
}

#[tokio::test]
async fn http_proxy_with_password() {
    let server = password_server("pw").await;
    let proxy = spawn_http_proxy(Some(("u".into(), "p".into()))).await;
    let params = route(
        vec![hop("t", "127.0.0.1", server.port, "pw")],
        Some(ProxyConfig::Http(target(proxy.port, Some(("u", "p"))))),
    );
    go(params, Arc::default()).await.unwrap();
}

#[tokio::test]
async fn http_proxy_wrong_password() {
    let server = password_server("pw").await;
    let proxy = spawn_http_proxy(Some(("u".into(), "p".into()))).await;
    let params = route(
        vec![hop("t", "127.0.0.1", server.port, "pw")],
        Some(ProxyConfig::Http(target(proxy.port, Some(("u", "wrong"))))),
    );
    assert_eq!(
        go(params, Arc::default()).await.err(),
        Some(SshError::ProxyAuthFailed)
    );
}

#[tokio::test]
async fn through_socks5_proxy_with_password() {
    let server = password_server("pw").await;
    let proxy = spawn_socks5_proxy(Some(("u".into(), "p".into()))).await;
    let params = route(
        vec![hop("t", "127.0.0.1", server.port, "pw")],
        Some(ProxyConfig::Socks5(target(proxy.port, Some(("u", "p"))))),
    );
    let session = go(params, Arc::default()).await.unwrap();
    echoes(&session).await;
}

#[tokio::test]
async fn socks5_proxy_wrong_password() {
    let server = password_server("pw").await;
    let proxy = spawn_socks5_proxy(Some(("u".into(), "p".into()))).await;
    let params = route(
        vec![hop("t", "127.0.0.1", server.port, "pw")],
        Some(ProxyConfig::Socks5(target(proxy.port, Some(("u", "no"))))),
    );
    assert_eq!(
        go(params, Arc::default()).await.err(),
        Some(SshError::ProxyAuthFailed)
    );
}

#[tokio::test]
async fn socks5_proxy_resolves_name() {
    let server = password_server("pw").await;
    let proxy = spawn_socks5_proxy(None).await;
    let params = route(
        vec![hop("t", "localhost", server.port, "pw")],
        Some(ProxyConfig::Socks5(target(proxy.port, None))),
    );
    go(params, Arc::default()).await.unwrap();
    assert_eq!(proxy.targets(), vec![format!("localhost:{}", server.port)]);
}

#[tokio::test]
async fn through_one_jump_host() {
    let jump = password_server("a").await;
    let inner = password_server("b").await;
    let params = route(
        vec![
            hop("jump", "127.0.0.1", jump.port, "a"),
            hop("inner", "127.0.0.1", inner.port, "b"),
        ],
        None,
    );
    let session = go(params, Arc::default()).await.unwrap();
    echoes(&session).await;
}

#[tokio::test]
async fn through_two_jump_hosts() {
    let one = password_server("1").await;
    let two = password_server("2").await;
    let inner = password_server("3").await;
    let params = route(
        vec![
            hop("one", "127.0.0.1", one.port, "1"),
            hop("two", "127.0.0.1", two.port, "2"),
            hop("inner", "127.0.0.1", inner.port, "3"),
        ],
        None,
    );
    let session = go(params, Arc::default()).await.unwrap();
    echoes(&session).await;
}

#[tokio::test]
async fn jump_host_key_is_verified_with_its_own_name() {
    let jump = password_server("a").await;
    let inner = password_server("b").await;
    let verifier = Arc::new(Noting::default());
    let params = route(
        vec![
            hop("jump", "127.0.0.1", jump.port, "a"),
            hop("inner", "127.0.0.1", inner.port, "b"),
        ],
        None,
    );
    go(params, verifier.clone()).await.unwrap();
    assert_eq!(
        *verifier.seen.lock().unwrap(),
        vec![
            ("jump".to_owned(), "127.0.0.1".to_owned(), jump.port),
            ("inner".to_owned(), "127.0.0.1".to_owned(), inner.port),
        ]
    );
}

#[tokio::test]
async fn each_hop_gets_only_its_own_password() {
    let jump = password_server("A").await;
    let inner = password_server("B").await;
    let params = route(
        vec![
            hop("jump", "127.0.0.1", jump.port, "A"),
            hop("inner", "127.0.0.1", inner.port, "B"),
        ],
        None,
    );
    go(params, Arc::default()).await.unwrap();
    assert_eq!(jump.received_passwords(), vec!["A".to_owned()]);
    assert_eq!(inner.received_passwords(), vec!["B".to_owned()]);
}

#[tokio::test]
async fn jump_auth_failure_names_the_hop() {
    let jump = password_server("A").await;
    let inner = password_server("B").await;
    let params = route(
        vec![
            hop("jump", "127.0.0.1", jump.port, "wrong"),
            hop("inner", "127.0.0.1", inner.port, "B"),
        ],
        None,
    );
    match go(params, Arc::default()).await {
        Err(SshError::Hop {
            index,
            host_name,
            error,
        }) => {
            assert_eq!((index, host_name.as_str()), (0, "jump"));
            assert_eq!(*error, SshError::PasswordRejected);
        }
        other => panic!("{:?}", other.err()),
    }
    assert!(inner.received_passwords().is_empty());
}

#[tokio::test]
async fn proxy_is_used_for_the_first_hop_only() {
    let jump = password_server("a").await;
    let inner = password_server("b").await;
    let proxy = spawn_http_proxy(None).await;
    let params = route(
        vec![
            hop("jump", "127.0.0.1", jump.port, "a"),
            hop("inner", "127.0.0.1", inner.port, "b"),
        ],
        Some(ProxyConfig::Http(target(proxy.port, None))),
    );
    go(params, Arc::default()).await.unwrap();
    assert_eq!(proxy.connects(), 1);
    assert_eq!(proxy.targets(), vec![format!("127.0.0.1:{}", jump.port)]);
}

#[tokio::test]
async fn unreachable_proxy_times_out() {
    // Accepts and never answers, like a proxy that hangs.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = silent.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = silent.accept().await {
            held.push(socket);
        }
    });
    let mut params = route(
        vec![hop("t", "127.0.0.1", 1, "pw")],
        Some(ProxyConfig::Http(target(port, None))),
    );
    params.connect_timeout = Duration::from_millis(300);
    let started = std::time::Instant::now();
    let result = go(params, Arc::default()).await;
    assert!(
        matches!(result, Err(SshError::Proxy(_))),
        "{:?}",
        result.err()
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// Trusts every host key, but each one only just now (a first contact).
struct FirstContact;

#[async_trait::async_trait]
impl HostKeyVerifier for FirstContact {
    async fn verify(&self, _: &str, _: &str, _: u16, _: &str) -> bool {
        true
    }

    fn first_contact(&self, _address: &str, _port: u16) -> bool {
        true
    }
}

#[tokio::test]
async fn stored_password_is_not_sent_on_first_contact() {
    let server = password_server("pw").await;
    let result = connect_route(
        route(vec![hop("t", "127.0.0.1", server.port, "pw")], None),
        Arc::new(FirstContact),
        Arc::new(NoPrompter),
    )
    .await;
    assert_eq!(result.err(), Some(SshError::AuthCancelled));
    assert!(server.received_passwords().is_empty());
}

#[tokio::test]
async fn proxy_user_with_line_break_or_colon_is_refused() {
    let server = password_server("pw").await;
    let proxy = spawn_http_proxy(Some(("u".into(), "p".into()))).await;
    for user in ["u\r\nX-Evil: 1", "a:b"] {
        let mut t = target(proxy.port, Some((user, "p")));
        t.username = Some(user.into());
        let params = route(
            vec![hop("t", "127.0.0.1", server.port, "pw")],
            Some(ProxyConfig::Http(t)),
        );
        assert!(matches!(
            go(params, Arc::default()).await,
            Err(SshError::Proxy(_))
        ));
    }
    assert_eq!(proxy.connects(), 0);
}

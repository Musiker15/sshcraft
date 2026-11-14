use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey};
use sshcraft_model::policy::Disconnect;
use sshcraft_ssh::testing::{TestServer, flood_pattern, spawn_agent, spawn_stalled_agent};
use sshcraft_ssh::{Auth, ConnectParams, Credential, HostKeyVerifier, NoPrompter, Shell, ShellEvent, SshError, connect, list_agent_keys};

struct Verifier {
    answer: bool,
    seen: Mutex<Vec<String>>,
}

impl Verifier {
    fn new(answer: bool) -> Arc<Self> {
        Arc::new(Self { answer, seen: Mutex::new(Vec::new()) })
    }
}

#[async_trait::async_trait]
impl HostKeyVerifier for Verifier {
    async fn verify(&self, _host_name: &str, _address: &str, _port: u16, fingerprint: &str) -> bool {
        self.seen.lock().unwrap().push(fingerprint.to_owned());
        self.answer
    }
}

fn key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

fn params(port: u16, key: PrivateKey) -> ConnectParams {
    ConnectParams { address: "127.0.0.1".into(), port, username: "tester".into(), auth: Auth::key(Credential::Key(key)) }
}

fn fingerprint(key: &PrivateKey) -> String {
    key.fingerprint(russh::keys::HashAlg::Sha256).to_string()
}

async fn agent_params(port: u16, agent_keys: &[PrivateKey], confirm: bool, wanted: &PrivateKey) -> ConnectParams {
    ConnectParams {
        address: "127.0.0.1".into(),
        port,
        username: "tester".into(),
        auth: Auth::key(Credential::Agent { fingerprint: fingerprint(wanted), agent: spawn_agent(agent_keys, confirm).await }),
    }
}

async fn setup() -> (TestServer, PrivateKey) {
    let k = key();
    (TestServer::start(k.public_key().clone()).await, k)
}

/// Collects data until `pred` holds on everything received so far, or an end event arrives.
async fn collect_until(shell: &mut Shell, pred: impl Fn(&[u8]) -> bool) -> (Vec<u8>, Option<Disconnect>) {
    let mut buf = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        match tokio::time::timeout_at(deadline, shell.events.recv()).await {
            Ok(Some(ShellEvent::Data(d))) => {
                buf.extend_from_slice(&d);
                if pred(&buf) {
                    return (buf, None);
                }
            }
            Ok(Some(ShellEvent::Ended(reason))) => return (buf, Some(reason)),
            Ok(None) | Err(_) => return (buf, None),
        }
    }
}

#[tokio::test]
async fn connects_and_echoes() {
    let (server, k) = setup().await;
    let session = connect(params(server.port, k), Verifier::new(true), Arc::new(NoPrompter)).await.unwrap();
    let mut shell = session.open_shell(80, 24, "xterm-256color").await.unwrap();
    shell.write(b"hi\n").await.unwrap();
    let (data, _) = collect_until(&mut shell, |b| b.windows(2).any(|w| w == b"hi")).await;
    assert!(data.windows(2).any(|w| w == b"hi"));
}

#[tokio::test]
async fn shell_requests_the_given_terminal_type() {
    let (server, k) = setup().await;
    let session = connect(params(server.port, k), Verifier::new(true), Arc::new(NoPrompter)).await.unwrap();
    let _shell = session.open_shell(80, 24, "vt100").await.unwrap();
    // The request is sent without waiting for a reply: give the server a moment.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while server.requested_terms().is_empty() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(server.requested_terms(), vec!["vt100".to_owned()]);
}

#[tokio::test]
async fn rejects_wrong_client_key() {
    let (server, _) = setup().await;
    let result = connect(params(server.port, key()), Verifier::new(true), Arc::new(NoPrompter)).await;
    assert_eq!(result.err(), Some(SshError::AuthFailed));
}

#[tokio::test]
async fn verifier_false_aborts() {
    let (server, k) = setup().await;
    let result = connect(params(server.port, k), Verifier::new(false), Arc::new(NoPrompter)).await;
    assert_eq!(result.err(), Some(SshError::HostKeyRejected));
}

#[tokio::test]
async fn verifier_sees_server_fingerprint() {
    let (server, k) = setup().await;
    let verifier = Verifier::new(true);
    connect(params(server.port, k), verifier.clone(), Arc::new(NoPrompter)).await.unwrap();
    assert_eq!(*verifier.seen.lock().unwrap(), vec![server.host_fingerprint.clone()]);
}

#[tokio::test]
async fn exit_yields_exited() {
    let (server, k) = setup().await;
    let session = connect(params(server.port, k), Verifier::new(true), Arc::new(NoPrompter)).await.unwrap();
    let mut shell = session.open_shell(80, 24, "xterm-256color").await.unwrap();
    shell.write(b"exit\n").await.unwrap();
    let (_, end) = collect_until(&mut shell, |_| false).await;
    assert_eq!(end, Some(Disconnect::Exited(Some(0))));
}

#[tokio::test]
async fn server_drop_yields_lost() {
    let (mut server, k) = setup().await;
    let session = connect(params(server.port, k), Verifier::new(true), Arc::new(NoPrompter)).await.unwrap();
    let mut shell = session.open_shell(80, 24, "xterm-256color").await.unwrap();
    server.stop();
    let (_, end) = tokio::time::timeout(Duration::from_secs(5), collect_until(&mut shell, |_| false)).await.expect("end event within 5 s");
    assert!(matches!(end, Some(Disconnect::Lost(_))), "got {end:?}");
}

#[tokio::test]
async fn large_output_arrives_complete_and_in_order() {
    let (server, k) = setup().await;
    let session = connect(params(server.port, k), Verifier::new(true), Arc::new(NoPrompter)).await.unwrap();
    let mut shell = session.open_shell(80, 24, "xterm-256color").await.unwrap();
    let expected = flood_pattern();
    shell.write(b"flood\n").await.unwrap();
    let (data, _) = collect_until(&mut shell, |b| b.len() >= expected.len()).await;
    assert_eq!(data.len(), expected.len());
    assert!(data == expected, "flood output differs from the pattern");
}

#[tokio::test]
async fn connects_with_agent_key() {
    let (server, k) = setup().await;
    let p = agent_params(server.port, std::slice::from_ref(&k), true, &k).await;
    assert!(connect(p, Verifier::new(true), Arc::new(NoPrompter)).await.is_ok());
}

#[tokio::test]
async fn agent_without_that_key_fails() {
    let (server, k) = setup().await;
    let other = key();
    let p = agent_params(server.port, std::slice::from_ref(&other), true, &k).await;
    assert_eq!(connect(p, Verifier::new(true), Arc::new(NoPrompter)).await.err(), Some(SshError::AgentKeyMissing));
}

#[tokio::test]
async fn agent_refusing_to_sign_fails() {
    let (server, k) = setup().await;
    let p = agent_params(server.port, std::slice::from_ref(&k), false, &k).await;
    let result = tokio::time::timeout(Duration::from_secs(5), connect(p, Verifier::new(true), Arc::new(NoPrompter))).await.expect("must not hang");
    assert_eq!(result.err(), Some(SshError::AgentRefused));
}

#[tokio::test]
async fn list_agent_keys_returns_fingerprints() {
    let (a, b) = (key(), key());
    let mut agent = spawn_agent(&[a.clone(), b.clone()], true).await;
    let keys = list_agent_keys(&mut agent).await.unwrap();
    let mut got: Vec<_> = keys.iter().map(|k| k.fingerprint.clone()).collect();
    let mut want = vec![fingerprint(&a), fingerprint(&b)];
    got.sort();
    want.sort();
    assert_eq!(got, want);
    assert!(keys.iter().all(|k| k.algorithm == "ssh-ed25519"));
}

#[tokio::test]
async fn stalled_agent_times_out_on_connect() {
    let (server, k) = setup().await;
    let p = ConnectParams {
        address: "127.0.0.1".into(),
        port: server.port,
        username: "tester".into(),
        auth: Auth::key(Credential::Agent { fingerprint: fingerprint(&k), agent: spawn_stalled_agent(std::slice::from_ref(&k)).await }),
    };
    let result = tokio::time::timeout(Duration::from_secs(8), connect(p, Verifier::new(true), Arc::new(NoPrompter)))
        .await
        .expect("a stalled agent must not hang the connect");
    assert!(matches!(result, Err(SshError::AgentUnavailable(_))), "{:?}", result.err());
}

#[tokio::test]
async fn stalled_agent_times_out_on_list() {
    let mut agent = spawn_stalled_agent(&[key()]).await;
    let result = tokio::time::timeout(Duration::from_secs(8), list_agent_keys(&mut agent)).await.expect("a stalled agent must not hang the key list");
    assert!(matches!(result, Err(SshError::AgentUnavailable(_))));
}

#[tokio::test]
async fn exec_returns_output() {
    let (server, k) = setup().await;
    let session = connect(params(server.port, k), Verifier::new(true), Arc::new(NoPrompter)).await.unwrap();
    let out = session.exec("cat /etc/os-release", 4096).await.unwrap();
    assert_eq!(out, sshcraft_ssh::testing::TEST_EXEC_OUTPUT.as_bytes());
    let short = session.exec("cat /etc/os-release", 4).await.unwrap();
    assert_eq!(short.len(), 4);
    assert_eq!(sshcraft_model::os_detect::from_output(&String::from_utf8_lossy(&out)), Some(sshcraft_model::model::HostOs::Debian));
}

#[tokio::test]
async fn shell_writer_runs_exec_beside_the_shell() {
    let (server, k) = setup().await;
    server.set_exec_output("a/\nb\n");
    let session = connect(params(server.port, k), Verifier::new(true), Arc::new(NoPrompter)).await.unwrap();
    let shell = session.open_shell(80, 24, "xterm-256color").await.unwrap();
    let (writer, _events) = shell.split();
    let out = writer.exec("ls", Some(b"/tmp\n"), 1024).await.unwrap();
    assert_eq!(out, b"a/\nb\n");
    assert_eq!(server.received_commands(), vec!["ls".to_owned()]);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while server.received_exec_input().is_empty() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(server.received_exec_input(), b"/tmp\n");
}

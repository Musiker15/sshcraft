//! The contract of `execute`: unknown commands, the crash guard, the journal, the parameter
//! limits, the data lock, and that no command panics on any input.

use std::sync::Arc;

use serde_json::{Value, json};
use sshcraft_store::store::Store;
use sshcraft_store::vault::{MasterKeyProvider, MemoryMasterKey, Vault, VaultError};
use zeroize::Zeroizing;

use crate::caller::{AgentName, Caller, ConnId, Via};
use crate::cmd::command_specs;
use crate::error::{EngineError, ErrorCode};
use crate::journal::{MAX_BYTES, MAX_ENTRIES};
use crate::{Session, SessionConfig};

fn open(dir: &std::path::Path) -> Session {
    Session::open(SessionConfig::for_tests(dir)).unwrap().0
}

/// A sync server in this process on a free local port, with SQLite in `dir`; its origin and API
/// key. It runs until the returned sender is dropped.
async fn sync_server(dir: &std::path::Path) -> (String, String, tokio::sync::oneshot::Sender<()>) {
    let database_url = format!("sqlite://{}", dir.join("server.db").display());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://127.0.0.1:{}", addr.port());
    let config = sshcraft_server_lib::config::ServeConfig {
        listen: addr,
        database_url: database_url.clone(),
        public_origin: origin.clone(),
        ws_recheck: std::time::Duration::from_millis(200),
    };
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = sshcraft_server_lib::serve_on(listener, config, async {
            let _ = stopped.await;
        })
        .await;
    });
    for _ in 0..100 {
        if sshcraft_server_lib::health(addr).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let api_key = sshcraft_server_lib::store::open(&database_url).await.unwrap().api_key().await.unwrap().0;
    (origin, api_key, stop)
}

#[tokio::test]
async fn starting_the_engine_reports_sync_status() {
    let server_dir = tempfile::tempdir().unwrap();
    let (origin, api_key, _stop) = sync_server(server_dir.path()).await;
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    // The account is set up without starting sync, as it is after a restart.
    let input = sshcraft_sync::account::NewAccount {
        server_name: "home".into(),
        origin,
        api_key,
        name: "Alice".into(),
        email: "alice@example.org".into(),
        device_name: "laptop".into(),
        platform: "windows".into(),
    };
    sshcraft_sync::account::create_account(&session.shared_store(), &session.vault().unwrap(), input, 1).await.unwrap();
    let mut events = session.subscribe();
    session.start();
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(20), events.recv()).await.expect("no sync status after start") {
            Ok(crate::EngineEvent::SyncStatus { .. }) => break,
            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(e) => panic!("{e}"),
        }
    }

    // Without a server nothing connects and nothing is reported.
    let other_dir = tempfile::tempdir().unwrap();
    let other = open(other_dir.path());
    let mut events = other.subscribe();
    other.start();
    let quiet = tokio::time::timeout(std::time::Duration::from_millis(300), events.recv()).await;
    assert!(quiet.is_err(), "{quiet:?}");
}

fn agent() -> Caller {
    Caller::Agent { name: AgentName::new("claude-code").unwrap(), via: Via::Mcp, conn: ConnId(1) }
}

#[tokio::test]
async fn unknown_command_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let err = session.execute(Caller::User, "no.such", json!({})).await.unwrap_err();
    assert_eq!(err, EngineError::UnknownCommand("no.such".into()));
}

#[tokio::test]
async fn a_panicking_command_becomes_internal_and_the_next_call_works() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let err = session.execute(Caller::User, "test.panic", json!({})).await.unwrap_err();
    assert_eq!(err, EngineError::Internal { id: "test.panic".into() });
    assert_eq!(session.execute(Caller::User, "test.echo", json!({"a": 1})).await.unwrap(), json!({"a": 1}));
    drop(session.store());
}

#[tokio::test]
async fn journal_leaves_out_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    // A fixed list, not SECRET_KEYS itself: shortening that list must turn this test red.
    for key in ["password", "passphrase", "secret", "api_key", "account_key", "answers", "text", "data"] {
        let params = json!({ "keep": "visible", key: "S3CRET", "deep": { "deeper": [{ key: "S3CRET" }] } });
        session.execute(Caller::User, "test.echo", params).await.unwrap();
    }
    let text = serde_json::to_string(&session.journal()).unwrap();
    assert!(!text.contains("S3CRET"), "{text}");
    assert!(text.contains("visible"));
}

#[tokio::test]
async fn journal_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    for i in 0..1100 {
        session.execute(Caller::User, "test.echo", json!({ "i": i })).await.unwrap();
    }
    assert_eq!(session.journal().len(), MAX_ENTRIES);
    session.execute(Caller::User, "test.echo", json!({ "big": "x".repeat(2 * 1024 * 1024) })).await.unwrap();
    let size = serde_json::to_vec(&session.journal()).unwrap().len();
    assert!(size < MAX_BYTES + 200_000, "{size}");
    assert!(size < 2 * 1024 * 1024);
}

#[tokio::test]
async fn deep_params_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let mut deep = json!(1);
    for _ in 0..1000 {
        deep = Value::Array(vec![deep]);
    }
    let err = session.execute(Caller::User, "test.echo", deep).await.unwrap_err();
    assert!(matches!(err, EngineError::BadParams { .. }), "{err:?}");
}

#[tokio::test]
async fn a_caller_without_an_open_connection_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    for caller in [agent(), Caller::Control { conn: ConnId(2) }] {
        let err = session.execute(caller, "host.list", json!({})).await.unwrap_err();
        assert_eq!(err, EngineError::Denied("unknown connection".into()));
    }
}

#[test]
fn command_ids_are_unique_and_well_formed() {
    let mut seen = std::collections::HashSet::new();
    for spec in command_specs() {
        assert!(seen.insert(spec.id), "{} twice", spec.id);
        let mut parts = spec.id.split('.');
        let area = parts.next().unwrap_or_default();
        assert!(!area.is_empty() && area.chars().all(|c| c.is_ascii_lowercase()), "{}", spec.id);
        let rest: Vec<&str> = parts.collect();
        assert!(!rest.is_empty(), "{}", spec.id);
        for part in rest {
            assert!(!part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{}", spec.id);
        }
        assert!(!spec.label.is_empty() && !spec.params.is_empty(), "{}", spec.id);
        assert!(!spec.undoable, "{}", spec.id);
    }
}

/// The parameters every command must survive without a panic.
fn adversarial() -> Vec<Value> {
    vec![
        json!({}),
        Value::Null,
        json!([]),
        json!(5),
        json!("x"),
        json!({ "id": "not-a-uuid" }),
        json!({ "id": "00000000-0000-0000-0000-000000000000" }),
        Value::String("x".repeat(1024 * 1024)),
        json!({ "name": "\u{0}\u{1b}[31m\u{202E}evil" }),
        json!({ "path": "../../etc/passwd" }),
        json!(-1),
        json!(u64::MAX),
        json!({ "cols": 4_294_967_295u64, "rows": 0 }),
    ]
}

#[tokio::test]
async fn every_command_survives_empty_params() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let conn = session.agent_connection("claude-code", Via::Mcp).unwrap();
    for spec in command_specs().iter().filter(|s| !["test.panic", "test.agent_panic"].contains(&s.id)) {
        for caller in [Caller::User, conn.caller()] {
            let result = session.execute(caller, spec.id, json!({})).await;
            assert!(!matches!(result, Err(EngineError::Internal { .. })), "{} panicked", spec.id);
        }
    }
}

#[tokio::test]
async fn every_command_survives_adversarial_params() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let conn = session.agent_connection("claude-code", Via::Mcp).unwrap();
    for spec in command_specs().iter().filter(|s| !["test.panic", "test.agent_panic"].contains(&s.id)) {
        for params in adversarial() {
            for caller in [Caller::User, conn.caller()] {
                let result = session.execute(caller, spec.id, params.clone()).await;
                assert!(!matches!(result, Err(EngineError::Internal { .. })), "{} panicked", spec.id);
            }
        }
    }
}

#[tokio::test]
async fn a_second_engine_on_the_same_data_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let _first = open(dir.path());
    let second = Session::open(SessionConfig::for_tests(dir.path()));
    assert!(matches!(second, Err(EngineError::DataInUse)));
}

#[tokio::test]
async fn a_running_tauri_version_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = SessionConfig::for_tests(dir.path());
    config.legacy_running = || true;
    assert!(matches!(Session::open(config), Err(EngineError::LegacyRunning)));
    assert!(!dir.path().join("sshcraft.db").exists());
}

/// A master key shared with the test, to see whether the engine wrote one.
struct Shared(Arc<MemoryMasterKey>);

impl MasterKeyProvider for Shared {
    fn load(&self) -> Result<Option<Zeroizing<[u8; 32]>>, VaultError> {
        self.0.load()
    }
    fn save(&self, key: &[u8; 32]) -> Result<(), VaultError> {
        self.0.save(key)
    }
}

#[tokio::test]
async fn open_without_master_key_creates_none() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = Store::open(&dir.path().join("sshcraft.db")).unwrap();
        let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
        vault.generate_ed25519(&store, "k").unwrap();
    }
    let empty = Arc::new(MemoryMasterKey::default());
    let mut config = SessionConfig::for_tests(dir.path());
    config.master_key = Box::new(Shared(empty.clone()));
    let (session, report) = Session::open(config).unwrap();
    assert_eq!(report.vault, Err(ErrorCode::MasterKeyMissing));
    assert!(empty.load().unwrap().is_none());
    let err = session.execute(Caller::User, "test.vault", json!({})).await.unwrap_err();
    assert!(matches!(err, EngineError::Disabled { .. }), "{err:?}");
}

/// Writing commands tell the interface that data changed; reading commands do not.
#[tokio::test]
async fn data_commands_announce_changes() {
    use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId};
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let mut events = session.subscribe();
    let group =
        Group { location: None, id: GroupId::new(), name: "eu".into(), defaults: ConnectionSettings::default(), parent_id: None, tags: Vec::new() };
    let host = Host {
        forwards: Vec::new(),
        location: None,
        id: HostId::new(),
        name: "web".into(),
        group_id: Some(group.id),
        settings: ConnectionSettings { address: Some("10.0.0.1".into()), ..Default::default() },
        tags: Vec::new(),
        os: None,
    };
    let writes = [
        ("group.save", json!({ "group": group })),
        ("host.save", json!({ "host": host })),
        ("identity.save", json!({ "identity": { "name": "me", "password": { "kind": "keep" } } })),
        ("group.delete", json!({ "id": group.id })),
    ];
    for (id, params) in writes {
        session.execute(Caller::User, id, params).await.unwrap();
        assert!(matches!(events.try_recv(), Ok(crate::EngineEvent::DataChanged { .. })), "{id} sent no DataChanged");
    }
    session.execute(Caller::User, "host.list", json!({})).await.unwrap();
    assert!(events.try_recv().is_err(), "a read sent an event");
}

/// No answer of a key command carries a private key or a passphrase; nor does the journal.
#[tokio::test]
async fn key_commands_never_return_private_material() {
    use russh::keys::ssh_key::LineEnding;
    use russh::keys::{Algorithm, PrivateKey};
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let pass = "PASS-PHRASE-4711";
    let text = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap().to_openssh(LineEnding::LF).unwrap();
    let imported = session.execute(Caller::User, "key.import", json!({ "name": "imported", "text": text.as_str() })).await.unwrap();
    let id = imported["id"].clone();
    let mut answers = vec![imported];
    let spec = json!({ "type": "ed25519" });
    answers.push(session.execute(Caller::User, "key.generate", json!({ "name": "made", "spec": spec, "passphrase": pass })).await.unwrap());
    answers.push(session.execute(Caller::User, "key.protect", json!({ "id": id, "passphrase": pass })).await.unwrap());
    answers.push(session.execute(Caller::User, "key.remember_passphrase", json!({ "id": id, "passphrase": pass })).await.unwrap());
    let list = session.execute(Caller::User, "key.list", json!({})).await.unwrap();
    assert_eq!(list.as_array().map(Vec::len), Some(2), "{list}");
    answers.push(list);
    answers.push(serde_json::to_value(session.journal()).unwrap());
    for answer in answers {
        let shown = answer.to_string();
        for secret in ["PRIVATE KEY", "PuTTY-User-Key-File", pass] {
            assert!(!shown.contains(secret), "{secret} in {shown}");
        }
    }
}

/// Without the vault every key command that needs it is disabled, with a reason; the list of
/// keys still works.
#[tokio::test]
async fn locked_vault_disables_key_commands() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = Store::open(&dir.path().join("sshcraft.db")).unwrap();
        let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
        vault.generate_ed25519(&store, "k").unwrap();
    }
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    // A fixed list: dropping `needs_vault` from one of these must turn this test red.
    let needing = [
        "key.import",
        "key.generate",
        "key.create_security",
        "key.attach_certificate",
        "key.detach_certificate",
        "key.remember_passphrase",
        "key.protect",
        "key.passcli_import",
    ];
    for id in needing {
        match session.execute(Caller::User, id, json!({})).await {
            Err(EngineError::Disabled { reason, .. }) => assert!(reason.contains("vault"), "{id}: {reason}"),
            other => panic!("{id}: {other:?}"),
        }
    }
    let keys = session.execute(Caller::User, "key.list", json!({})).await.unwrap();
    assert_eq!(keys.as_array().map(Vec::len), Some(1));
    let status = session.execute(Caller::User, "vault.status", json!({})).await.unwrap();
    assert_eq!(status["ok"], json!(false));
    assert_eq!(status["code"], json!("master_key_missing"));
}

#[tokio::test]
async fn a_panic_in_the_agent_gate_becomes_internal_and_the_next_call_works() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let conn = session.agent_connection("claude-code", Via::Mcp).unwrap();
    let err = session.execute(conn.caller(), "test.agent_panic", json!({})).await.unwrap_err();
    assert_eq!(err, EngineError::Internal { id: "test.agent_panic".into() });
    session.execute(conn.caller(), "host.list", json!({})).await.unwrap();
}

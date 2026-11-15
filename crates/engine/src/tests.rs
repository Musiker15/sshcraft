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

/// R2 review: `execute` assumed the caller polls it inside the engine's tokio runtime.
#[test]
fn execute_works_without_a_tokio_context() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let session = {
        let _inside = runtime.enter();
        open(dir.path())
    };
    let s = session.clone();
    // A plain thread with an executor that is not tokio: the egui frame loop in R4.
    let result = std::thread::spawn(move || futures::executor::block_on(s.execute(Caller::User, "local.shells", json!({})))).join().unwrap();
    assert!(result.is_ok(), "{result:?}");
}

/// R2 review: snippet content, comments and commands stayed in the in-memory journal.
#[tokio::test]
async fn the_journal_keeps_no_snippet_comment_or_command_text() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    for key in ["content", "command", "text"] {
        let params = json!({ "keep": "visible", "deep": { key: "S3CRET" } });
        session.execute(Caller::User, "test.echo", params).await.unwrap();
    }
    let text = serde_json::to_string(&session.journal()).unwrap();
    assert!(!text.contains("S3CRET"), "{text}");
    assert!(text.contains("visible"));
}

/// R2 review: a session an agent opened while its connection went away stayed open.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_opened_for_a_gone_connection_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let (_server, host) = crate::testing::trusted_test_host(&session).await;
    let opened = session
        .execute(Caller::User, "session.open", json!({ "target": { "kind": "host", "host_id": host }, "cols": 80, "rows": 24 }))
        .await
        .unwrap();
    let id: crate::SessionId = serde_json::from_value(opened["session"].clone()).unwrap();
    let conn = session.control_connection();
    let gone = conn.id();
    drop(conn);
    session.own_agent_session(gone, id, host);
    assert_eq!(session.unacknowledged(id), None, "the session is closed");
}

/// Host keys are listed, removed one by one and written as a `known_hosts` file; none of it is
/// open to agents (a removed key makes the next connection ask again).
#[tokio::test]
async fn host_keys_are_listed_removed_and_exported() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let key = sshcraft_store::store::HostKey {
        address: "a.org".into(),
        port: 2222,
        algorithm: Some("ssh-ed25519".into()),
        public_key: Some("AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl".into()),
        fingerprint: "SHA256:x".into(),
        added_at: 1,
    };
    session.store().add_host_key(&key).unwrap();
    session.store().set_known_host("b.org", 22, "SHA256:y").unwrap();
    let all = session.execute(Caller::User, "hostkey.list", Value::Null).await.unwrap();
    assert_eq!(all.as_array().unwrap().len(), 2);
    let one = session.execute(Caller::User, "hostkey.list", json!({ "address": "a.org" })).await.unwrap();
    assert_eq!(one[0]["fingerprint"], "SHA256:x");
    let path = dir.path().join("known_hosts");
    let out = session.execute(Caller::User, "hostkey.export", json!({ "path": path.to_string_lossy() })).await.unwrap();
    assert_eq!(out["skipped"], 1);
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("[a.org]:2222 ssh-ed25519 AAAA"));
    for id in ["hostkey.list", "hostkey.remove", "hostkey.export"] {
        let r = session.execute(agent(), id, json!({ "address": "a.org", "port": 2222, "fingerprint": "SHA256:x", "path": "x" })).await;
        assert!(r.is_err(), "{id} must not be open to agents");
    }
    session.execute(Caller::User, "hostkey.remove", json!({ "address": "a.org", "port": 2222, "fingerprint": "SHA256:x" })).await.unwrap();
    assert!(session.store().host_keys("a.org", 2222).unwrap().is_empty());
    assert!(session.execute(Caller::User, "hostkey.remove", json!({ "address": "a.org", "port": 2222, "fingerprint": "SHA256:x" })).await.is_err());
}

/// A deleted host waits in the trash; restoring it brings it back and queues it for its sync
/// server again. Agents can do neither.
#[tokio::test]
async fn deleting_a_host_keeps_it_in_the_trash_and_a_restore_syncs_it_again() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let server = sshcraft_model::model::ServerId::new();
    session
        .store()
        .add_server(&sshcraft_store::store::ServerRow {
            id: server,
            name: "home".into(),
            origin: "https://sync.example.org".into(),
            account: [7; 32],
            device_id: uuid::Uuid::new_v4(),
            sealed_device: Vec::new(),
            sync_enabled: true,
            sync_keys: false,
            revoked: false,
            log_head: None,
            tracker: None,
            created_at: 1,
        })
        .unwrap();
    let host = sshcraft_model::model::Host {
        id: sshcraft_model::model::HostId::new(),
        name: "web".into(),
        group_id: None,
        settings: sshcraft_model::model::ConnectionSettings { address: Some("web.org".into()), ..Default::default() },
        location: Some(server),
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    };
    session.store().upsert_host(&host).unwrap();
    session.execute(Caller::User, "host.delete", json!({ "id": host.id })).await.unwrap();
    assert!(session.store().list_hosts().unwrap().is_empty());
    let trash = session.execute(Caller::User, "trash.list", Value::Null).await.unwrap();
    assert_eq!(trash[0]["kind"], "host");
    assert_eq!(trash[0]["name"], "web");
    let queued = session.store().outbox(server).unwrap();
    for (kind, id) in queued {
        session.store().dequeue(server, kind, &id).unwrap();
    }
    for id in ["trash.list", "trash.restore", "trash.purge"] {
        assert!(session.execute(agent(), id, json!({ "kind": "host", "id": host.id.0.to_string() })).await.is_err(), "{id}");
    }
    session.execute(Caller::User, "trash.restore", json!({ "kind": "host", "id": host.id.0.to_string() })).await.unwrap();
    assert_eq!(session.store().list_hosts().unwrap(), vec![host.clone()]);
    assert!(session.store().outbox(server).unwrap().contains(&(sshcraft_store::store::ItemKind::Host, host.id.0.to_string())));
    assert!(session.store().list_trash().unwrap().is_empty());
}

/// Every save keeps the stand before; restoring one saves it again (and so keeps the newer
/// stand as a version too).
#[tokio::test]
async fn saving_keeps_the_previous_version_and_a_version_restores() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let mut host = sshcraft_model::model::Host {
        id: sshcraft_model::model::HostId::new(),
        name: "web".into(),
        group_id: None,
        settings: sshcraft_model::model::ConnectionSettings { address: Some("old.org".into()), ..Default::default() },
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    };
    session.execute(Caller::User, "host.save", json!({ "host": host })).await.unwrap();
    host.settings.address = Some("new.org".into());
    session.execute(Caller::User, "host.save", json!({ "host": host })).await.unwrap();
    // Saving the same again keeps no new version.
    session.execute(Caller::User, "host.save", json!({ "host": host })).await.unwrap();
    let id = host.id.0.to_string();
    let versions = session.execute(Caller::User, "trash.versions", json!({ "kind": "host", "id": id })).await.unwrap();
    assert_eq!(versions.as_array().unwrap().len(), 1);
    let version = versions[0]["version"].clone();
    session.execute(Caller::User, "trash.restore_version", json!({ "kind": "host", "id": id, "version": version })).await.unwrap();
    assert_eq!(session.store().list_hosts().unwrap()[0].settings.address.as_deref(), Some("old.org"));
    let versions = session.execute(Caller::User, "trash.versions", json!({ "kind": "host", "id": id })).await.unwrap();
    assert_eq!(versions.as_array().unwrap().len(), 2, "the newer stand is a version now");
    for cmd in ["trash.versions", "trash.restore_version"] {
        assert!(session.execute(agent(), cmd, json!({ "kind": "host", "id": id, "version": 1 })).await.is_err(), "{cmd}");
    }
}

/// A backup is written and read back by the user; an agent can do neither, and the passphrase
/// is not in the journal.
#[tokio::test]
async fn backups_are_for_the_user_only() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let path = dir.path().join("sshcraft.backup").to_string_lossy().into_owned();
    let params = json!({ "path": path, "passphrase": "a long passphrase" });
    for id in ["backup.create", "backup.restore"] {
        assert!(session.execute(agent(), id, params.clone()).await.is_err(), "{id}");
    }
    assert!(!std::path::Path::new(&path).exists());
    session.execute(Caller::User, "backup.create", params.clone()).await.unwrap();
    session.execute(Caller::User, "backup.restore", params).await.unwrap();
    let journal = serde_json::to_string(&session.journal()).unwrap();
    assert!(journal.contains("backup.create"), "the call is journaled");
    assert!(!journal.contains("a long passphrase"));
}

/// A password reference entered in the form is allowed for the host's address; saved without
/// entering it (a rename) it is not.
#[tokio::test]
async fn an_entered_password_reference_is_allowed_for_its_address() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let reference =
        sshcraft_model::secret_ref::SecretRef { manager: sshcraft_model::secret_ref::Manager::OnePassword, reference: "op://S/web/password".into() };
    let host = sshcraft_model::model::Host {
        id: sshcraft_model::model::HostId::new(),
        name: "web".into(),
        group_id: None,
        settings: sshcraft_model::model::ConnectionSettings {
            address: Some("web.org".into()),
            password_ref: Some(reference.clone()),
            ..Default::default()
        },
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    };
    let digest = crate::secrets::approval_digest(&reference, "web.org", 22);
    session.execute(Caller::User, "host.save", json!({ "host": host })).await.unwrap();
    assert!(!session.store().command_approved(&digest).unwrap());
    session.execute(Caller::User, "host.save", json!({ "host": host, "approve_secret_ref": true })).await.unwrap();
    assert!(session.store().command_approved(&digest).unwrap());
    // Another address is another approval.
    assert!(!session.store().command_approved(&crate::secrets::approval_digest(&reference, "evil.org", 22)).unwrap());
}

#[tokio::test]
async fn a_totp_secret_must_be_base32() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let host = sshcraft_model::model::Host {
        id: sshcraft_model::model::HostId::new(),
        name: "web".into(),
        group_id: None,
        settings: sshcraft_model::model::ConnectionSettings { address: Some("web.org".into()), ..Default::default() },
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    };
    session.execute(Caller::User, "host.save", json!({ "host": host })).await.unwrap();
    let owner = json!({ "kind": "host_totp", "id": host.id });
    assert!(session.execute(Caller::User, "password.set", json!({ "owner": owner, "secret": "not base32!" })).await.is_err());
    session.execute(Caller::User, "password.set", json!({ "owner": owner, "secret": "GEZDGNBVGY3TQOJQ" })).await.unwrap();
    let listed = session.execute(Caller::User, "host.list", Value::Null).await.unwrap();
    assert_eq!(listed[0]["has_totp"], true);
    assert!(listed[0]["settings"].get("totp").is_none(), "the id is not shown");
}

#[tokio::test]
async fn the_connection_list_names_every_sync_server_and_no_update_check() {
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    session
        .store()
        .add_server(&sshcraft_store::store::ServerRow {
            id: sshcraft_model::model::ServerId::new(),
            name: "home".into(),
            origin: "https://sync.example.org".into(),
            account: [7; 32],
            device_id: uuid::Uuid::new_v4(),
            sealed_device: Vec::new(),
            sync_enabled: true,
            sync_keys: false,
            revoked: false,
            log_head: None,
            tracker: None,
            created_at: 1,
        })
        .unwrap();
    let list = session.execute(Caller::User, "app.connections", Value::Null).await.unwrap();
    let list = list.as_array().unwrap();
    assert!(list.iter().any(|c| c["kind"] == "sync" && c["target"] == "https://sync.example.org" && c["active"] == true));
    assert!(list.iter().any(|c| c["kind"] == "update_check" && c["target"] == "none"));
    assert!(list.iter().any(|c| c["kind"] == "agent_port" && c["active"] == false), "off until the user turns it on");
}

/// A host of a share received to read only, deleted by its owner, comes back from the trash on
/// this device only: the share would not take it back.
#[tokio::test]
async fn a_host_of_a_received_share_comes_back_local() {
    use sshcraft_store::store::{ShareKind, ShareRole, ShareRow};
    let dir = tempfile::tempdir().unwrap();
    let session = open(dir.path());
    let server = sshcraft_model::model::ServerId::new();
    let area = uuid::Uuid::new_v4();
    {
        let store = session.store();
        store
            .add_server(&sshcraft_store::store::ServerRow {
                id: server,
                name: "home".into(),
                origin: "https://sync.example.org".into(),
                account: [7; 32],
                device_id: uuid::Uuid::new_v4(),
                sealed_device: Vec::new(),
                sync_enabled: true,
                sync_keys: false,
                revoked: false,
                log_head: None,
                tracker: None,
                created_at: 1,
            })
            .unwrap();
        store
            .put_share(
                server,
                &ShareRow {
                    area_id: area,
                    kind: ShareKind::Selection,
                    group_id: None,
                    with_keys: false,
                    owned: false,
                    role: Some(ShareRole::Read),
                    owner: [9; 32],
                },
            )
            .unwrap();
    }
    let host = sshcraft_model::model::Host {
        id: sshcraft_model::model::HostId::new(),
        name: "theirs".into(),
        group_id: None,
        settings: sshcraft_model::model::ConnectionSettings { address: Some("t.org".into()), ..Default::default() },
        location: Some(server),
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    };
    {
        let store = session.store();
        store.upsert_host(&host).unwrap();
        store.set_item_area(server, sshcraft_store::store::ItemKind::Host, &host.id.0.to_string(), area).unwrap();
        // What the owner's delete does here.
        store.trash_host(host.id).unwrap();
        store.delete_host(host.id).unwrap();
    }
    session.execute(Caller::User, "trash.restore", json!({ "kind": "host", "id": host.id.0.to_string() })).await.unwrap();
    let back = session.store().list_hosts().unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].location, None, "local, not back into the share");
}

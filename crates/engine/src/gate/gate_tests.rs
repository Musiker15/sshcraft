//! The agent gate against a real engine: in-memory vault, a test SSH server for `web`, and a
//! confirmer with a fixed answer (or the engine's own questions through the event bus).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use serde_json::{Value, json};
use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId, IdentityId, KeyRef, PasswordId};
use sshcraft_ssh::testing::{TEST_EXEC_OUTPUT, TestAuth, TestServer};
use sshcraft_store::agent::{AgentCategory, AgentLevel, AgentLogEntry, AgentOutcome, AgentPolicy, log_list};
use sshcraft_store::store::Store;
use uuid::Uuid;

use super::{AgentConnection, AgentPrompt, Confirmer};
use crate::caller::{Caller, Via};
use crate::error::EngineError;
use crate::{Session, SessionConfig};

/// Answers questions with a fixed answer, or never; may run something while asked.
struct Answer {
    answer: Option<bool>,
    asked: Mutex<Vec<AgentPrompt>>,
    count: AtomicUsize,
    meanwhile: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Answer {
    fn new(answer: Option<bool>) -> Arc<Self> {
        Arc::new(Self { answer, asked: Mutex::new(Vec::new()), count: AtomicUsize::new(0), meanwhile: Mutex::new(None) })
    }
}

#[async_trait::async_trait]
impl Confirmer for Answer {
    async fn confirm(&self, prompt: AgentPrompt) -> bool {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.asked.lock().unwrap().push(prompt);
        if let Some(f) = self.meanwhile.lock().unwrap().as_ref() {
            f();
        }
        match self.answer {
            Some(a) => a,
            None => std::future::pending().await,
        }
    }
}

struct World {
    session: Session,
    server: TestServer,
    web: Host,
    db: Host,
    prod: Group,
    answer: Arc<Answer>,
    _dir: tempfile::TempDir,
}

fn host(name: &str, group: Option<GroupId>) -> Host {
    Host {
        id: HostId(Uuid::new_v4()),
        name: name.into(),
        group_id: group,
        settings: ConnectionSettings { address: Some(format!("{name}.example.org")), ..Default::default() },
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    }
}

fn group(name: &str, parent: Option<GroupId>) -> Group {
    Group {
        id: GroupId(Uuid::new_v4()),
        name: name.into(),
        defaults: ConnectionSettings::default(),
        location: None,
        parent_id: parent,
        tags: Vec::new(),
    }
}

/// `web` on a test server (its key stored, its host key trusted), `db` in prod/eu, hidden
/// through prod. `answer`: `Some` answers through a fixed confirmer, `None` uses the engine's
/// own questions.
async fn world_with(answer: Option<Option<bool>>, confirm_timeout: Duration) -> World {
    let dir = tempfile::tempdir().unwrap();
    let fixed = Answer::new(answer.flatten());
    let mut config = SessionConfig::for_tests(dir.path());
    config.confirm_timeout = confirm_timeout;
    if answer.is_some() {
        config.confirmer = Some(fixed.clone());
    }
    let (session, _) = Session::open(config).unwrap();
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server = TestServer::start_with(TestAuth { key: Some(key.public_key().clone()), ..Default::default() }).await;
    let prod = group("prod", None);
    let eu = group("eu", Some(prod.id));
    let mut web = host("web", None);
    let db = host("db", Some(eu.id));
    {
        let store = session.store();
        let info = session.vault().unwrap().import_key(&store, "test", &key.to_openssh(LineEnding::LF).unwrap(), None).unwrap();
        web.settings = ConnectionSettings {
            address: Some("127.0.0.1".into()),
            port: Some(server.port),
            username: Some("tester".into()),
            key: Some(KeyRef::Stored { id: info.id }),
            ..Default::default()
        };
        store.set_known_host("127.0.0.1", server.port, &server.host_fingerprint).unwrap();
        for g in [&prod, &eu] {
            store.upsert_group(g).unwrap();
        }
        for h in [&web, &db] {
            store.upsert_host(h).unwrap();
        }
        AgentPolicy::set_hidden(&store, prod.id.0, true).unwrap();
    }
    World { session, server, web, db, prod, answer: fixed, _dir: dir }
}

async fn world(answer: Option<bool>) -> World {
    world_with(Some(answer), Duration::from_secs(2)).await
}

impl World {
    fn conn(&self) -> AgentConnection {
        self.session.agent_connection("claude-code", Via::Mcp).unwrap()
    }

    fn set(&self, category: AgentCategory, level: AgentLevel) {
        AgentPolicy::set_level(&self.session.store(), category, level).unwrap();
    }

    fn hide(&self, id: Uuid) {
        AgentPolicy::set_hidden(&self.session.store(), id, true).unwrap();
    }

    fn add_host(&self, h: &Host) {
        self.session.store().upsert_host(h).unwrap();
    }

    fn add_group(&self, g: &Group) {
        self.session.store().upsert_group(g).unwrap();
    }

    fn stored(&self, id: HostId) -> Option<Host> {
        self.session.store().list_hosts().unwrap().into_iter().find(|h| h.id == id)
    }

    /// The log, oldest first.
    fn log(&self) -> Vec<AgentLogEntry> {
        let mut entries = log_list(&self.session.vault().unwrap(), &self.session.store()).unwrap();
        entries.reverse();
        entries
    }

    fn outcomes(&self) -> Vec<AgentOutcome> {
        self.log().iter().map(|e| e.outcome).collect()
    }

    /// Commands the user's commands ran for agents (journal entries of agents).
    fn agent_runs(&self) -> usize {
        self.session.journal().iter().filter(|e| e.caller != "user").count()
    }
}

async fn call(w: &World, conn: &AgentConnection, id: &str, params: Value) -> Result<Value, EngineError> {
    w.session.execute(conn.caller(), id, params).await
}

fn exec(host: &str, command: &str) -> Value {
    json!({ "host": host, "command": command })
}

fn is_denied(r: &Result<Value, EngineError>) -> bool {
    matches!(r, Err(EngineError::Denied(_)))
}

fn is_not_found(r: &Result<Value, EngineError>) -> bool {
    matches!(r, Err(EngineError::NotFound(_)))
}

fn is_invalid(r: &Result<Value, EngineError>) -> bool {
    matches!(r, Err(EngineError::BadParams { .. }))
}

// ---- Moved from the Tauri version (agents/gate_tests.rs) ----

#[tokio::test]
async fn ask_runs_only_after_yes() {
    let w = world(Some(true)).await;
    let conn = w.conn();
    let r = call(&w, &conn, "host.exec", exec("web", "uptime")).await.unwrap();
    assert_eq!(r["output"], TEST_EXEC_OUTPUT);
    assert!(w.server.received_commands().contains(&"uptime".to_owned()));
    let asked = w.answer.asked.lock().unwrap()[0].clone();
    assert_eq!(asked.agent, "claude-code");
    assert_eq!(asked.target, "web (127.0.0.1)");
    assert_eq!(asked.detail, "uptime");
    assert_eq!(w.outcomes(), vec![AgentOutcome::AskedAllowed]);

    let w = world(Some(false)).await;
    let conn = w.conn();
    assert!(is_denied(&call(&w, &conn, "host.exec", exec("web", "rm -rf /")).await));
    assert!(w.server.received_commands().is_empty());
    assert_eq!(w.outcomes(), vec![AgentOutcome::AskedDenied]);
}

#[tokio::test]
async fn allow_runs_without_asking_deny_never() {
    let w = world(None).await;
    w.set(AgentCategory::Connect, AgentLevel::Allow);
    let conn = w.conn();
    call(&w, &conn, "session.open", json!({ "host": "web" })).await.unwrap();
    assert!(is_denied(&call(&w, &conn, "host.delete", json!({ "host": "web" })).await));
    assert_eq!(w.answer.count.load(Ordering::SeqCst), 0);
    assert!(w.stored(w.web.id).is_some());
    assert_eq!(w.outcomes(), vec![AgentOutcome::Allowed, AgentOutcome::Denied]);
}

#[tokio::test]
async fn hidden_hosts_are_unknown_everywhere() {
    let w = world(None).await;
    w.set(AgentCategory::Connect, AgentLevel::Allow);
    w.set(AgentCategory::Delete, AgentLevel::Allow);
    let conn = w.conn();
    let listed = call(&w, &conn, "host.list", Value::Null).await.unwrap();
    let names: Vec<&str> = listed.as_array().unwrap().iter().map(|h| h["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["web"]);
    for (id, params) in [
        ("host.info", json!({ "host": "db" })),
        ("host.exec", exec(&w.db.id.0.to_string(), "ls")),
        ("session.open", json!({ "host": "db" })),
        ("host.delete", json!({ "host": "db" })),
    ] {
        let r = call(&w, &conn, id, params).await;
        assert_eq!(r, Err(EngineError::NotFound("host".into())), "{id}");
    }
    assert!(w.stored(w.db.id).is_some());
    assert!(w.server.received_commands().is_empty());
    assert!(call(&w, &conn, "group.list", Value::Null).await.unwrap().as_array().unwrap().is_empty());
    // host.info is a list and not logged; the other three are, as hidden.
    assert_eq!(w.outcomes().iter().filter(|o| **o == AgentOutcome::Hidden).count(), 3);
    hidden_hosts_stay_out_of_forwards_logs_workspaces_and_snippets(&w, &conn).await;
}

/// The same hidden `db` in the categories that came with R2.8, all of them allowed.
async fn hidden_hosts_stay_out_of_forwards_logs_workspaces_and_snippets(w: &World, conn: &AgentConnection) {
    use sshcraft_model::forward::{ForwardId, ForwardKind, ForwardRule};
    use sshcraft_store::session_log::{self, LogKind, LogMeta};
    for category in [AgentCategory::Forward, AgentCategory::Logs, AgentCategory::Workspaces, AgentCategory::Snippets] {
        w.set(category, AgentLevel::Allow);
    }
    let user = |id: &'static str, params: Value| w.session.execute(Caller::User, id, params);
    // A rule on `db` the user started; nothing listens where it points.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut db = w.stored(w.db.id).unwrap();
    db.settings.address = Some("127.0.0.1".into());
    db.settings.port = Some(port);
    let rule = ForwardRule {
        id: ForwardId::new(),
        name: "db".into(),
        kind: ForwardKind::Local,
        bind_address: "127.0.0.1".into(),
        bind_port: std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port(),
        target_host: "127.0.0.1".into(),
        target_port: 5432,
    };
    db.forwards.push(rule.clone());
    w.add_host(&db);
    user("forward.start", json!({ "host_id": db.id, "rule_id": rule.id })).await.unwrap();
    while user("forward.status", Value::Null).await.unwrap().as_array().unwrap().is_empty() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(call(w, conn, "forward.status", Value::Null).await.unwrap(), json!([]));
    assert!(is_not_found(&call(w, conn, "forward.start", json!({ "host_id": db.id, "rule_id": rule.id })).await));
    // A session log recorded on `db`.
    {
        let vault = w.session.vault().unwrap();
        let store = w.session.store();
        let meta = LogMeta {
            id: sshcraft_model::model::LogId(Uuid::new_v4()),
            title: "db".into(),
            kind: LogKind::Ssh,
            host_id: Some(db.id),
            started_at: 1,
            ended_at: None,
            size: 0,
            truncated: false,
            marks: Vec::new(),
        };
        session_log::create(&vault, &store, &meta).unwrap();
        session_log::append_chunk(&vault, &store, meta.id, 0, b"secret-db-output\r\n", 2).unwrap();
        session_log::finish(&vault, &store, meta.id, 3, 18, false).unwrap();
    }
    assert_eq!(user("log.list", Value::Null).await.unwrap().as_array().unwrap().len(), 1);
    assert_eq!(call(w, conn, "log.list", Value::Null).await.unwrap(), json!([]));
    assert_eq!(user("log.search", json!({ "query": "secret-db" })).await.unwrap().as_array().unwrap().len(), 1);
    assert_eq!(call(w, conn, "log.search", json!({ "query": "secret-db" })).await.unwrap(), json!([]));
    // A workspace with a pane on `db`, a snippet that runs on it.
    let template = json!({
        "id": Uuid::new_v4(),
        "name": "db work",
        "layout": "split",
        "col_sizes": [1.0],
        "row_sizes": [1.0],
        "panes": [{ "target": { "kind": "host", "host_id": db.id }, "title": "db" }],
    });
    user("workspace.save", json!({ "template": template })).await.unwrap();
    assert_eq!(call(w, conn, "workspace.list", Value::Null).await.unwrap(), json!([]));
    let snippet = json!({ "id": Uuid::new_v4(), "name": "vacuum", "content": "vacuumdb", "default_targets": [db.id] });
    user("snippet.save", json!({ "snippet": snippet })).await.unwrap();
    assert_eq!(call(w, conn, "snippet.list", Value::Null).await.unwrap(), json!([]));
}

#[tokio::test]
async fn log_export_without_path_asks_only_the_user() {
    let w = world(None).await;
    w.set(AgentCategory::Logs, AgentLevel::Allow);
    let mut events = w.session.subscribe();
    let id = sshcraft_model::model::LogId(Uuid::new_v4());
    {
        use sshcraft_store::session_log::{self, LogKind, LogMeta};
        let vault = w.session.vault().unwrap();
        let store = w.session.store();
        let meta = LogMeta {
            id,
            title: "web".into(),
            kind: LogKind::Ssh,
            host_id: None,
            started_at: 1,
            ended_at: None,
            size: 0,
            truncated: false,
            marks: Vec::new(),
        };
        session_log::create(&vault, &store, &meta).unwrap();
        session_log::append_chunk(&vault, &store, id, 0, b"ok\r\n", 2).unwrap();
        session_log::finish(&vault, &store, id, 3, 4, false).unwrap();
    }
    let params = json!({ "id": id, "format": "text" });
    let r = w.session.execute(Caller::User, "log.export", params.clone()).await.unwrap();
    assert_eq!(r, json!({ "dialog": "saveLog" }));
    let asked = loop {
        if let Ok(crate::EngineEvent::Ui(crate::UiRequest::Dialog { name, params })) = events.recv().await {
            break (name, params);
        }
    };
    assert_eq!(asked.0, "saveLog");
    assert_eq!(asked.1["name"], "web-1970-01-01.txt");
    // Never for a program: log.export is not reachable for agents or the control channel.
    let control = w.session.control_connection();
    for conn in [&w.conn(), &control] {
        assert!(is_denied(&call(&w, conn, "log.export", params.clone()).await));
    }
    // And the helper itself refuses every caller but the user.
    let ctx = crate::Ctx { session: w.session.clone(), caller: control.caller() };
    assert!(matches!(crate::cmd::dialog_or_error(&ctx, "saveLog", &params), Err(EngineError::BadParams { .. })));
}

#[tokio::test]
async fn a_host_behind_a_hidden_jump_host_is_hidden_too() {
    let w = world(None).await;
    w.set(AgentCategory::Connect, AgentLevel::Allow);
    // `gate` jumps through the hidden `db`, `edge` through a group that does.
    let mut through = host("gate", None);
    through.settings.jump_hosts = Some(vec![w.db.id]);
    let mut jumping = group("dmz", None);
    jumping.defaults.jump_hosts = Some(vec![through.id]);
    let edge = host("edge", Some(jumping.id));
    w.add_group(&jumping);
    w.add_host(&through);
    w.add_host(&edge);
    let conn = w.conn();
    let listed = call(&w, &conn, "host.list", Value::Null).await.unwrap();
    let names: Vec<&str> = listed.as_array().unwrap().iter().map(|h| h["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["web"]);
    for name in ["gate", "edge"] {
        assert!(is_not_found(&call(&w, &conn, "host.exec", exec(name, "ls")).await), "{name}");
    }
    assert!(w.server.received_commands().is_empty());
}

#[tokio::test]
async fn a_group_with_something_hidden_below_is_not_deleted() {
    let w = world(None).await;
    let top = group("all", None);
    let inner = group("secret", Some(top.id));
    let free = group("free", None);
    for g in [&top, &inner, &free] {
        w.add_group(g);
    }
    w.add_host(&host("vault", Some(inner.id)));
    w.hide(inner.id.0);
    w.set(AgentCategory::Delete, AgentLevel::Allow);
    let conn = w.conn();
    assert!(is_denied(&call(&w, &conn, "group.delete", json!({ "group": "all" })).await));
    call(&w, &conn, "group.delete", json!({ "group": "free" })).await.unwrap();
    let groups: Vec<String> = w.session.store().list_groups().unwrap().into_iter().map(|g| g.name).collect();
    assert!(groups.contains(&"all".to_owned()) && groups.contains(&"secret".to_owned()));
    assert!(!groups.contains(&"free".to_owned()));
}

#[tokio::test]
async fn nothing_goes_into_a_hidden_group() {
    let w = world(None).await;
    w.set(AgentCategory::Edit, AgentLevel::Allow);
    let conn = w.conn();
    let r = call(&w, &conn, "host.save", json!({ "host": { "name": "new", "group": w.prod.id.0 } })).await;
    assert!(is_not_found(&r), "{r:?}");
    let r = call(&w, &conn, "group.save", json!({ "group": { "name": "sub", "parent": w.prod.id.0 } })).await;
    assert!(is_not_found(&r), "{r:?}");
    assert_eq!(w.agent_runs(), 0);
}

#[tokio::test]
async fn a_change_touches_only_what_it_names_and_shows_it() {
    let w = world(Some(true)).await;
    let mut before = w.stored(w.web.id).unwrap();
    before.settings.port = Some(2222);
    before.settings.key = Some(KeyRef::Agent { fingerprint: "SHA256:proton".into() });
    before.tags = vec!["prod".into()];
    w.add_host(&before);
    let conn = w.conn();
    let r = call(&w, &conn, "host.save", json!({ "host": { "id": w.web.id.0, "name": "web-1", "username": "deploy" } })).await;
    assert_eq!(r.unwrap()["name"], "web-1");
    let saved = w.stored(w.web.id).unwrap();
    assert_eq!(saved.name, "web-1");
    assert_eq!(saved.settings.username.as_deref(), Some("deploy"));
    assert_eq!(saved.settings.address, before.settings.address);
    assert_eq!(saved.settings.port, Some(2222));
    assert!(matches!(saved.settings.key, Some(KeyRef::Agent { .. })));
    assert_eq!(saved.tags, vec!["prod".to_owned()]);
    let detail = w.answer.asked.lock().unwrap()[0].detail.clone();
    assert_eq!(detail, "name: web -> web-1\nuser: tester -> deploy");
}

#[tokio::test]
async fn stored_sign_in_data_is_never_pointed_elsewhere() {
    let w = world(None).await;
    w.set(AgentCategory::Edit, AgentLevel::Allow);
    let mut web = w.stored(w.web.id).unwrap();
    web.settings.password = Some(PasswordId(Uuid::new_v4()));
    w.add_host(&web);
    let conn = w.conn();
    let r = call(&w, &conn, "host.save", json!({ "host": { "id": w.web.id.0, "address": "evil.example" } })).await;
    assert!(is_denied(&r), "{r:?}");
    assert_eq!(w.stored(w.web.id).unwrap().settings.address.as_deref(), Some("127.0.0.1"));
    // Renaming is fine.
    call(&w, &conn, "host.save", json!({ "host": { "id": w.web.id.0, "name": "web-2" } })).await.unwrap();
    // A new host in a group that signs in with an identity is refused too.
    let mut team = group("team", None);
    team.defaults.identity = Some(IdentityId(Uuid::new_v4()));
    w.add_group(&team);
    let r = call(&w, &conn, "host.save", json!({ "host": { "name": "trap", "group": team.id.0, "address": "evil.example" } })).await;
    assert!(is_denied(&r), "{r:?}");
    let names: Vec<String> = w.session.store().list_hosts().unwrap().into_iter().map(|h| h.name).collect();
    assert!(names.contains(&"web-2".to_owned()) && !names.contains(&"trap".to_owned()));
}

#[tokio::test]
async fn the_decision_is_logged_before_running_and_failures_after() {
    let w = world(None).await;
    w.set(AgentCategory::Connect, AgentLevel::Allow);
    // A host whose port nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut down = host("down", None);
    down.settings = ConnectionSettings { address: Some("127.0.0.1".into()), port: Some(port), username: Some("x".into()), ..Default::default() };
    w.add_host(&down);
    let conn = w.conn();
    let r = call(&w, &conn, "host.exec", exec("down", "uptime")).await;
    assert!(matches!(r, Err(EngineError::Failed { .. })), "{r:?}");
    assert_eq!(w.outcomes(), vec![AgentOutcome::Allowed, AgentOutcome::Failed]);
}

#[tokio::test]
async fn nothing_runs_without_a_log_or_a_readable_policy() {
    let w = world(None).await;
    w.set(AgentCategory::Connect, AgentLevel::Allow);
    let conn = w.conn();
    w.session.store().execute_raw("CREATE TRIGGER no_log BEFORE INSERT ON agent_log BEGIN SELECT RAISE(FAIL, 'disk full'); END;").unwrap();
    let r = call(&w, &conn, "host.exec", exec("web", "uptime")).await;
    assert!(matches!(r, Err(EngineError::Failed { .. })), "{r:?}");
    w.session.store().execute_raw("DROP TRIGGER no_log;").unwrap();
    w.session.store().put_setting("agents.policy", r#"{"hidden": 5}"#).unwrap();
    let r = call(&w, &conn, "host.list", Value::Null).await;
    assert!(matches!(r, Err(EngineError::Failed { .. })), "{r:?}");
    assert!(w.server.received_commands().is_empty());
}

#[tokio::test]
async fn hiding_while_asked_stops_the_action() {
    let w = world(Some(true)).await;
    let session = w.session.clone();
    let id = w.web.id.0;
    *w.answer.meanwhile.lock().unwrap() = Some(Box::new(move || {
        AgentPolicy::set_hidden(&session.store(), id, true).unwrap();
    }));
    let conn = w.conn();
    assert!(is_not_found(&call(&w, &conn, "host.exec", exec("web", "uptime")).await));
    assert!(w.server.received_commands().is_empty());
}

#[tokio::test]
async fn a_dropped_question_is_logged_as_cancelled() {
    let w = world(None).await;
    let conn = w.conn();
    let pending = call(&w, &conn, "host.exec", exec("web", "uptime"));
    assert!(tokio::time::timeout(Duration::from_millis(50), pending).await.is_err());
    assert_eq!(w.outcomes(), vec![AgentOutcome::Cancelled]);
}

#[tokio::test]
async fn key_material_never_leaves_but_command_output_may_name_keys() {
    let w = world(None).await;
    w.set(AgentCategory::Keys, AgentLevel::Allow);
    w.set(AgentCategory::Connect, AgentLevel::Allow);
    let conn = w.conn();
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap().to_openssh(LineEnding::LF).unwrap();
    // A key whose name looks like key material: the answer would carry it, so it is not sent.
    let r = call(&w, &conn, "key.import", json!({ "name": "-----BEGIN OPENSSH PRIVATE KEY-----", "text": key.as_str() })).await;
    assert!(matches!(r, Err(EngineError::Failed { .. })), "{r:?}");
    let body = key.lines().nth(1).unwrap().to_owned();
    assert!(w.log().iter().all(|e| !e.detail.contains(&body) && !e.target.contains(&body)));
    // A remote file the agent reads is its business; a host named like a key header lists fine.
    w.server.set_exec_output("-----BEGIN OPENSSH PRIVATE KEY----- id_rsa\n");
    call(&w, &conn, "host.exec", exec("web", "head -1 ~/.ssh/id_rsa")).await.unwrap();
    w.add_host(&host("PRIVATE KEY store", None));
    call(&w, &conn, "host.list", Value::Null).await.unwrap();
}

#[tokio::test]
async fn an_unanswered_question_counts_as_no() {
    let w = world_with(Some(None), Duration::from_millis(200)).await;
    let conn = w.conn();
    assert!(is_denied(&call(&w, &conn, "session.open", json!({ "host": "web" })).await));
    assert!(w.session.sessions().ids().is_empty());
    assert_eq!(w.outcomes(), vec![AgentOutcome::AskedDenied]);
}

#[tokio::test]
async fn sessions_belong_to_their_connection() {
    let w = world(Some(true)).await;
    let mine = w.conn();
    let other = w.conn();
    let opened = call(&w, &mine, "session.open", json!({ "host": "web" })).await.unwrap();
    let session = opened["session"].clone();
    let write = json!({ "id": session, "data": "ls\n" });
    assert!(is_not_found(&call(&w, &other, "session.send", write.clone()).await));
    call(&w, &mine, "session.send", write).await.unwrap();
    call(&w, &mine, "session.read", json!({ "id": session, "wait_ms": 10 })).await.unwrap();
    // Only opening asked; the foreign write is logged as failed, reading is not logged.
    assert_eq!(w.answer.count.load(Ordering::SeqCst), 1);
    assert_eq!(w.outcomes(), vec![AgentOutcome::AskedAllowed, AgentOutcome::Failed, AgentOutcome::Allowed]);
    // Closing works even with connecting blocked.
    w.set(AgentCategory::Connect, AgentLevel::Deny);
    assert!(is_denied(&call(&w, &mine, "session.open", json!({ "host": "web" })).await));
    call(&w, &mine, "session.close", json!({ "id": session })).await.unwrap();
    assert!(w.session.sessions().ids().is_empty());
}

#[tokio::test]
async fn a_name_two_hosts_share_must_be_an_id() {
    let w = world(Some(true)).await;
    w.add_host(&host("web", None));
    let conn = w.conn();
    assert!(is_invalid(&call(&w, &conn, "host.info", json!({ "host": "web" })).await));
}

#[tokio::test]
async fn bad_agent_names_long_commands_and_timeouts_are_refused() {
    let w = world(Some(true)).await;
    for name in ["\n", "claude\u{202E}edoc", ""] {
        assert!(matches!(w.session.agent_connection(name, Via::Mcp), Err(EngineError::BadParams { .. })), "{name:?}");
    }
    let conn = w.conn();
    let r = call(&w, &conn, "host.exec", json!({ "host": "web", "command": "sleep 1", "timeout_secs": 601 })).await;
    assert!(is_invalid(&r), "{r:?}");
    let long = format!("{}rm -rf /", " ".repeat(sshcraft_store::agent::MAX_DETAIL));
    assert!(is_invalid(&call(&w, &conn, "host.exec", exec("web", &long)).await));
    assert!(w.server.received_commands().is_empty());
}

// ---- Moved from the Tauri version (agents/server.rs) ----

#[tokio::test]
async fn a_waiting_question_holds_up_nothing_and_cancel_withdraws_it() {
    let w = world_with(None, Duration::from_secs(60)).await;
    let mut events = w.session.subscribe();
    let conn = w.conn();
    let waiting = {
        let session = w.session.clone();
        let caller = conn.caller();
        tokio::spawn(async move { session.execute(caller, "session.open", json!({ "host": "web" })).await })
    };
    let prompt_id = loop {
        if let Ok(crate::EngineEvent::AgentPrompt(p)) = events.recv().await {
            break p.prompt_id;
        }
    };
    // Another call goes through while the question waits.
    call(&w, &conn, "host.list", Value::Null).await.unwrap();
    waiting.abort();
    let _ = waiting.await;
    let withdrawn = loop {
        if let Ok(crate::EngineEvent::PromptWithdrawn { prompt_id }) = events.recv().await {
            break prompt_id;
        }
    };
    assert_eq!(withdrawn, prompt_id);
    assert_eq!(w.session.execute(Caller::User, "prompt.list", Value::Null).await.unwrap(), json!([]));
    assert_eq!(w.outcomes(), vec![AgentOutcome::Cancelled]);
}

#[tokio::test]
async fn a_gone_client_withdraws_its_question_and_loses_its_sessions() {
    let w = world_with(None, Duration::from_secs(60)).await;
    let _events = w.session.subscribe();
    w.set(AgentCategory::Connect, AgentLevel::Allow);
    let conn = w.conn();
    call(&w, &conn, "session.open", json!({ "host": "web" })).await.unwrap();
    assert_eq!(w.session.sessions().ids().len(), 1);
    // Ask for something that waits for the user, then go away.
    w.set(AgentCategory::Connect, AgentLevel::Ask);
    let waiting = {
        let session = w.session.clone();
        let caller = conn.caller();
        tokio::spawn(async move { session.execute(caller, "session.open", json!({ "host": "web" })).await })
    };
    while w.session.prompts().open_prompts().is_empty() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    waiting.abort();
    let _ = waiting.await;
    drop(conn);
    assert!(w.session.prompts().open_prompts().is_empty());
    assert!(w.session.sessions().ids().is_empty());
}

// ---- New in R2.7 ----

#[tokio::test]
async fn lists_and_reads_leave_no_log_entry() {
    let w = world(None).await;
    w.set(AgentCategory::Connect, AgentLevel::Allow);
    let conn = w.conn();
    let session = call(&w, &conn, "session.open", json!({ "host": "web" })).await.unwrap()["session"].clone();
    let before = w.log().len();
    for _ in 0..1000 {
        call(&w, &conn, "host.list", Value::Null).await.unwrap();
        call(&w, &conn, "session.read", json!({ "id": session })).await.unwrap();
    }
    assert_eq!(w.log().len(), before);
}

#[tokio::test]
async fn commands_without_category_are_never_reachable() {
    let w = world(Some(true)).await;
    for level in [AgentLevel::Allow, AgentLevel::Ask] {
        for category in AgentCategory::ALL {
            w.set(category, level);
        }
    }
    let agent = w.conn();
    let control = w.session.control_connection();
    for spec in crate::cmd::command_specs().iter().filter(|s| s.agent.is_none()) {
        for conn in [&agent, &control] {
            let r = call(&w, conn, spec.id, json!({})).await;
            assert!(matches!(&r, Err(EngineError::Denied(m)) if m == "not allowed for agents"), "{}: {r:?}", spec.id);
        }
    }
    assert_eq!(w.agent_runs(), 0);
    assert_eq!(w.answer.count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn control_counts_as_agent() {
    let w = world(None).await;
    w.set(AgentCategory::Connect, AgentLevel::Deny);
    let control = w.session.control_connection();
    assert!(matches!(control.caller(), Caller::Control { .. }));
    assert!(is_denied(&call(&w, &control, "session.open", json!({ "host": "web" })).await));
    assert!(is_not_found(&call(&w, &control, "host.info", json!({ "host": "db" })).await));
    let log = w.log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].agent, "control");
    assert_eq!(log[0].outcome, AgentOutcome::Denied);
    assert!(w.session.sessions().ids().is_empty());
}

#[tokio::test]
async fn ask_without_a_window_is_no() {
    let w = world_with(None, Duration::from_secs(60)).await;
    let conn = w.conn();
    let started = std::time::Instant::now();
    let r = call(&w, &conn, "session.open", json!({ "host": "web" })).await;
    assert_eq!(r, Err(EngineError::Denied("no SSHCraft window to ask in".into())));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(w.outcomes(), vec![AgentOutcome::AskedDenied]);
}

#[tokio::test]
async fn an_old_prompt_id_does_not_answer_the_next() {
    let w = world_with(None, Duration::from_secs(60)).await;
    let _events = w.session.subscribe();
    // Question 1: a host key the user is asked about, withdrawn when the tab closes.
    let unknown = host("unknown", None);
    let mut unknown = unknown;
    unknown.settings =
        ConnectionSettings { address: Some("127.0.0.1".into()), port: Some(w.server.port), username: Some("tester".into()), ..Default::default() };
    w.add_host(&unknown);
    w.session.store().forget_known_host("127.0.0.1", w.server.port).unwrap();
    let tab = w
        .session
        .execute(Caller::User, "session.open", json!({ "target": { "kind": "host", "host_id": unknown.id }, "cols": 80, "rows": 24 }))
        .await
        .unwrap();
    while w.session.prompts().open_prompts().is_empty() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let first = w.session.prompts().open_prompts()[0].prompt_id();
    w.session.execute(Caller::User, "session.close", json!({ "id": tab["session"] })).await.unwrap();
    while !w.session.prompts().open_prompts().is_empty() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // Question 2: an agent's action.
    let conn = w.conn();
    let waiting = {
        let session = w.session.clone();
        let caller = conn.caller();
        tokio::spawn(async move { session.execute(caller, "host.exec", json!({ "host": "web", "command": "uptime" })).await })
    };
    while w.session.prompts().open_prompts().is_empty() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let second = w.session.prompts().open_prompts()[0].prompt_id();
    assert_ne!(first, second);
    let old = w.session.execute(Caller::User, "prompt.agent", json!({ "prompt_id": first, "allow": true })).await;
    assert!(old.is_err());
    assert_eq!(w.session.prompts().open_prompts().len(), 1);
    w.session.execute(Caller::User, "prompt.agent", json!({ "prompt_id": second, "allow": false })).await.unwrap();
    assert!(is_denied(&waiting.await.unwrap()));
    assert!(w.server.received_commands().is_empty());
}

#[tokio::test]
async fn only_the_user_answers_agent_questions() {
    let w = world(Some(true)).await;
    let agent = w.conn();
    let control = w.session.control_connection();
    for conn in [&agent, &control] {
        for (id, params) in [
            ("prompt.agent", json!({ "prompt_id": 1, "allow": true })),
            ("prompt.host_key", json!({ "prompt_id": 1, "accept": true })),
            ("prompt.auth", json!({ "prompt_id": 1, "answers": ["x"] })),
        ] {
            assert!(is_denied(&call(&w, conn, id, params).await), "{id}");
        }
    }
}

#[tokio::test]
async fn agent_host_fields_outside_the_list_are_refused() {
    let w = world(None).await;
    w.set(AgentCategory::Edit, AgentLevel::Allow);
    let conn = w.conn();
    for field in [
        json!({ "jump_hosts": [w.db.id] }),
        json!({ "identity": Uuid::new_v4() }),
        json!({ "password_id": Uuid::new_v4() }),
        json!({ "agent_forwarding": true }),
        json!({ "proxy": { "kind": "socks5" } }),
    ] {
        let mut host = json!({ "id": w.web.id.0, "name": "renamed" });
        host.as_object_mut().unwrap().extend(field.as_object().unwrap().clone());
        let r = call(&w, &conn, "host.save", json!({ "host": host })).await;
        assert!(matches!(&r, Err(EngineError::BadParams { message, .. }) if message.contains("field not allowed for agents")), "{r:?}");
    }
    assert_eq!(w.stored(w.web.id).unwrap().name, "web");
    assert!(w.outcomes().iter().all(|o| *o == AgentOutcome::Failed));
    assert_eq!(w.outcomes().len(), 5);
}

fn store_before_open(dir: &std::path::Path, policy: &str) {
    let store = Store::open(&dir.join("sshcraft.db")).unwrap();
    store.put_setting("agents.policy", policy).unwrap();
}

#[tokio::test]
async fn new_categories_start_denied_and_old_policies_load() {
    let dir = tempfile::tempdir().unwrap();
    let hidden = Uuid::new_v4();
    store_before_open(dir.path(), &format!(r#"{{"levels":{{"connect":"allow","delete":"ask"}},"hidden":["{hidden}"]}}"#));
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    let policy = AgentPolicy::load_checked(&session.store()).unwrap();
    assert_eq!(policy.level(AgentCategory::Connect), AgentLevel::Allow);
    assert_eq!(policy.level(AgentCategory::Delete), AgentLevel::Ask);
    assert!(policy.hidden.contains(&hidden));
    for category in [AgentCategory::Forward, AgentCategory::Logs, AgentCategory::Workspaces, AgentCategory::Snippets] {
        assert_eq!(policy.level(category), AgentLevel::Deny, "{category:?}");
    }
    assert_eq!(AgentCategory::ALL.len(), 11);
}

#[tokio::test]
async fn share_and_sync_levels_reset_once() {
    let dir = tempfile::tempdir().unwrap();
    store_before_open(dir.path(), r#"{"levels":{"share":"allow","sync":"allow","connect":"allow"},"hidden":[]}"#);
    {
        let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
        let policy = AgentPolicy::load(&session.store());
        assert_eq!(policy.level(AgentCategory::Share), AgentLevel::Deny);
        assert_eq!(policy.level(AgentCategory::Sync), AgentLevel::Deny);
        assert_eq!(policy.level(AgentCategory::Connect), AgentLevel::Allow);
        session.execute(Caller::User, "agent.set_level", json!({ "category": "share", "level": "allow" })).await.unwrap();
    }
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    assert_eq!(AgentPolicy::load(&session.store()).level(AgentCategory::Share), AgentLevel::Allow);
}

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use sshcraft_agent::{ExecResult, GroupInput, HostInput, KeyType, KeyView, SessionOutput};
use sshcraft_core::model::{ConnectionSettings, PasswordId};

use super::*;

#[derive(Default)]
pub(crate) struct Fake {
    policy: Mutex<AgentPolicy>,
    policy_broken: AtomicBool,
    log_broken: AtomicBool,
    hosts: Mutex<Vec<Host>>,
    groups: Mutex<Vec<Group>>,
    keys: Mutex<Vec<KeyView>>,
    log: Mutex<Vec<AgentLogEntry>>,
    calls: Mutex<Vec<String>>,
    closed: Mutex<Vec<String>>,
    exec_output: Mutex<Option<String>>,
}

impl Fake {
    /// One free host, `web`.
    pub(crate) fn with_web() -> Self {
        let fake = Self::default();
        *fake.hosts.lock().unwrap() = vec![host("web", None)];
        fake
    }
    pub(crate) fn allow_connect(&self) {
        self.set(AgentCategory::Connect, AgentLevel::Allow);
    }
    pub(crate) fn deny_connect(&self) {
        self.set(AgentCategory::Connect, AgentLevel::Deny);
    }
    pub(crate) fn ask_connect(&self) {
        self.set(AgentCategory::Connect, AgentLevel::Ask);
    }
    pub(crate) fn closed(&self) -> Vec<String> {
        self.closed.lock().unwrap().clone()
    }
    fn called(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn call(&self, what: impl Into<String>) {
        self.calls.lock().unwrap().push(what.into());
    }
    fn set(&self, category: AgentCategory, level: AgentLevel) {
        self.policy.lock().unwrap().levels.insert(category, level);
    }
    fn hide(&self, id: Uuid) {
        self.policy.lock().unwrap().hidden.insert(id);
    }
    fn outcomes(&self) -> Vec<AgentOutcome> {
        self.log.lock().unwrap().iter().map(|e| e.outcome).collect()
    }
    fn stored(&self, id: HostId) -> Host {
        self.hosts
            .lock()
            .unwrap()
            .iter()
            .find(|h| h.id == id)
            .cloned()
            .unwrap()
    }
}

#[async_trait::async_trait]
impl AgentBackend for Fake {
    fn policy(&self) -> Result<AgentPolicy, AgentError> {
        if self.policy_broken.load(Ordering::SeqCst) {
            return Err(AgentError::new(ErrorKind::Failed, "unreadable"));
        }
        Ok(self.policy.lock().unwrap().clone())
    }
    fn hosts(&self) -> Result<Vec<Host>, AgentError> {
        Ok(self.hosts.lock().unwrap().clone())
    }
    fn groups(&self) -> Result<Vec<Group>, AgentError> {
        Ok(self.groups.lock().unwrap().clone())
    }
    fn keys(&self) -> Result<Vec<KeyView>, AgentError> {
        Ok(self.keys.lock().unwrap().clone())
    }
    fn log(&self, entry: AgentLogEntry) -> Result<(), AgentError> {
        if self.log_broken.load(Ordering::SeqCst) {
            return Err(AgentError::new(ErrorKind::Failed, "disk full"));
        }
        self.log.lock().unwrap().push(entry);
        Ok(())
    }
    async fn exec(
        &self,
        host: HostId,
        command: &str,
        _: Duration,
    ) -> Result<ExecResult, AgentError> {
        self.call(format!("exec {} {command}", host.0));
        if command == "fail" {
            return Err(AgentError::new(ErrorKind::Failed, "connection refused"));
        }
        Ok(ExecResult {
            output: self
                .exec_output
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| "ok\n".into()),
            truncated: false,
            exit_status: Some(0),
        })
    }
    fn session_open(&self, host: HostId) -> Result<String, AgentError> {
        self.call(format!("open {}", host.0));
        Ok(Uuid::new_v4().to_string())
    }
    fn session_write(&self, session: &str, _: &[u8]) -> Result<(), AgentError> {
        self.call(format!("write {session}"));
        Ok(())
    }
    async fn session_read(&self, _: &str, _: Duration) -> Result<SessionOutput, AgentError> {
        Ok(SessionOutput {
            output: "x".into(),
            ended: false,
        })
    }
    fn session_close(&self, session: &str) -> Result<(), AgentError> {
        self.closed.lock().unwrap().push(session.to_owned());
        Ok(())
    }
    fn save_host(&self, host: Host) -> Result<(), AgentError> {
        self.call(format!("save_host {}", host.name));
        let mut hosts = self.hosts.lock().unwrap();
        hosts.retain(|h| h.id != host.id);
        hosts.push(host);
        Ok(())
    }
    fn save_group(&self, group: Group) -> Result<(), AgentError> {
        self.call(format!("save_group {}", group.name));
        Ok(())
    }
    fn delete_host(&self, host: HostId) -> Result<(), AgentError> {
        self.call(format!("delete_host {}", host.0));
        Ok(())
    }
    fn delete_group(&self, group: GroupId) -> Result<(), AgentError> {
        self.call(format!("delete_group {}", group.0));
        Ok(())
    }
    fn delete_key(&self, key: KeyId) -> Result<(), AgentError> {
        self.call(format!("delete_key {}", key.0));
        Ok(())
    }
    async fn generate_key(&self, name: &str, _: KeyType) -> Result<KeyView, AgentError> {
        self.call(format!("generate {name}"));
        Ok(KeyView {
            id: Uuid::new_v4(),
            name: name.into(),
            kind: "ed25519".into(),
            fingerprint: "SHA256:abc".into(),
        })
    }
    async fn import_key(
        &self,
        name: &str,
        _: &str,
        _: Option<&str>,
    ) -> Result<KeyView, AgentError> {
        self.call(format!("import {name}"));
        // A broken backend that echoes key text: the gate must stop it.
        Ok(KeyView {
            id: Uuid::new_v4(),
            name: "-----BEGIN OPENSSH PRIVATE KEY-----".into(),
            kind: "ed25519".into(),
            fingerprint: "SHA256:abc".into(),
        })
    }
}

/// Answers questions with a fixed answer, or never; may run something while asked.
struct Answer {
    answer: Option<bool>,
    asked: Mutex<Vec<AgentPrompt>>,
    count: AtomicUsize,
    meanwhile: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Answer {
    fn new(answer: Option<bool>) -> Arc<Self> {
        Arc::new(Self {
            answer,
            asked: Mutex::new(Vec::new()),
            count: AtomicUsize::new(0),
            meanwhile: Mutex::new(None),
        })
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

fn host(name: &str, group: Option<GroupId>) -> Host {
    Host {
        id: HostId(Uuid::new_v4()),
        name: name.into(),
        group_id: group,
        settings: ConnectionSettings {
            address: Some(format!("{name}.example.org")),
            ..Default::default()
        },
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

/// web (free), db (in prod/eu, hidden through prod).
fn world() -> (Arc<Fake>, Host, Host, Group) {
    let fake = Arc::new(Fake::default());
    let prod = group("prod", None);
    let eu = group("eu", Some(prod.id));
    let web = host("web", None);
    let db = host("db", Some(eu.id));
    *fake.groups.lock().unwrap() = vec![prod.clone(), eu];
    *fake.hosts.lock().unwrap() = vec![web.clone(), db.clone()];
    fake.hide(prod.id.0);
    (fake, web, db, prod)
}

fn gate(fake: &Arc<Fake>, answer: &Arc<Answer>) -> Gate<Fake, Answer> {
    Gate::new(fake.clone(), answer.clone())
}

async fn ask_on(gate: &Gate<Fake, Answer>, conn: &Connection, action: Action) -> Response {
    gate.handle(
        Request {
            id: 1,
            agent: "claude-code".into(),
            action,
        },
        conn,
    )
    .await
}

async fn ask(gate: &Gate<Fake, Answer>, action: Action) -> Response {
    ask_on(gate, &Connection::default(), action).await
}

fn kind(response: &Response) -> Option<ErrorKind> {
    response.error.as_ref().map(|e| e.kind)
}

fn exec(host: &str, command: &str) -> Action {
    Action::Exec {
        host: host.into(),
        command: command.into(),
        timeout_secs: None,
    }
}

#[tokio::test]
async fn ask_runs_only_after_yes() {
    let (fake, web, _, _) = world();
    let yes = Answer::new(Some(true));
    let r = ask(&gate(&fake, &yes), exec("web", "uptime")).await;
    assert!(r.error.is_none(), "{r:?}");
    assert_eq!(fake.called(), vec![format!("exec {} uptime", web.id.0)]);
    let asked = yes.asked.lock().unwrap()[0].clone();
    assert_eq!(asked.agent, "claude-code");
    assert_eq!(asked.target, "web (web.example.org)");
    assert_eq!(asked.detail, "uptime");
    assert_eq!(fake.outcomes(), vec![AgentOutcome::AskedAllowed]);

    let (fake, _, _, _) = world();
    let no = Answer::new(Some(false));
    let r = ask(&gate(&fake, &no), exec("web", "rm -rf /")).await;
    assert_eq!(kind(&r), Some(ErrorKind::Denied));
    assert!(fake.called().is_empty());
    assert_eq!(fake.outcomes(), vec![AgentOutcome::AskedDenied]);
}

#[tokio::test]
async fn allow_runs_without_asking_deny_never() {
    let (fake, _, _, _) = world();
    fake.allow_connect();
    let never = Answer::new(None);
    let g = gate(&fake, &never);
    assert!(
        ask(&g, Action::SessionOpen { host: "web".into() })
            .await
            .error
            .is_none()
    );
    let r = ask(&g, Action::DeleteHost { host: "web".into() }).await;
    assert_eq!(kind(&r), Some(ErrorKind::Denied));
    assert_eq!(never.count.load(Ordering::SeqCst), 0);
    assert!(!fake.called().iter().any(|c| c.starts_with("delete")));
    assert_eq!(
        fake.outcomes(),
        vec![AgentOutcome::Allowed, AgentOutcome::Denied]
    );
}

#[tokio::test]
async fn hidden_hosts_are_unknown_everywhere() {
    let (fake, _, db, _) = world();
    fake.allow_connect();
    fake.set(AgentCategory::Delete, AgentLevel::Allow);
    let g = gate(&fake, &Answer::new(None));
    let listed = ask(&g, Action::ListHosts).await.ok.unwrap();
    let names: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["web"]);
    for action in [
        Action::HostInfo { host: "db".into() },
        exec(&db.id.0.to_string(), "ls"),
        Action::SessionOpen { host: "db".into() },
        Action::DeleteHost { host: "db".into() },
    ] {
        let r = ask(&g, action).await;
        assert_eq!(kind(&r), Some(ErrorKind::NotFound));
        assert_eq!(r.error.unwrap().message, "no such host");
    }
    assert!(fake.called().is_empty());
    assert!(
        ask(&g, Action::ListGroups)
            .await
            .ok
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    // host_info is a list action and not logged; the other three are, as hidden.
    assert_eq!(
        fake.outcomes()
            .iter()
            .filter(|o| **o == AgentOutcome::Hidden)
            .count(),
        3
    );
}

#[tokio::test]
async fn a_host_behind_a_hidden_jump_host_is_hidden_too() {
    let (fake, _, db, prod) = world();
    fake.allow_connect();
    // `gate` jumps through the hidden `db`, `edge` through a group that does.
    let mut through = host("gate", None);
    through.settings.jump_hosts = Some(vec![db.id]);
    let mut jumping = group("dmz", None);
    jumping.defaults.jump_hosts = Some(vec![through.id]);
    let edge = host("edge", Some(jumping.id));
    fake.hosts.lock().unwrap().extend([through, edge]);
    fake.groups.lock().unwrap().push(jumping);
    let g = gate(&fake, &Answer::new(None));
    let listed = ask(&g, Action::ListHosts).await.ok.unwrap().to_string();
    assert!(!listed.contains("gate") && !listed.contains("edge") && !listed.contains("db"));
    for name in ["gate", "edge"] {
        assert_eq!(
            kind(&ask(&g, exec(name, "ls")).await),
            Some(ErrorKind::NotFound)
        );
    }
    assert!(fake.called().is_empty());
    let _ = prod;
}

#[tokio::test]
async fn a_group_with_something_hidden_below_is_not_deleted() {
    let fake = Arc::new(Fake::default());
    let top = group("all", None);
    let inner = group("secret", Some(top.id));
    let free = group("free", None);
    *fake.groups.lock().unwrap() = vec![top.clone(), inner.clone(), free.clone()];
    *fake.hosts.lock().unwrap() = vec![host("vault", Some(inner.id))];
    fake.hide(inner.id.0);
    fake.set(AgentCategory::Delete, AgentLevel::Allow);
    let g = gate(&fake, &Answer::new(None));
    assert_eq!(
        kind(
            &ask(
                &g,
                Action::DeleteGroup {
                    group: "all".into()
                }
            )
            .await
        ),
        Some(ErrorKind::Denied)
    );
    assert!(
        ask(
            &g,
            Action::DeleteGroup {
                group: "free".into()
            }
        )
        .await
        .error
        .is_none()
    );
    assert_eq!(fake.called(), vec![format!("delete_group {}", free.id.0)]);
}

#[tokio::test]
async fn nothing_goes_into_a_hidden_group() {
    let (fake, _, _, prod) = world();
    fake.set(AgentCategory::Edit, AgentLevel::Allow);
    let g = gate(&fake, &Answer::new(None));
    let r = ask(
        &g,
        Action::SaveHost {
            host: HostInput {
                name: Some("new".into()),
                group: Some(Some(prod.id.0)),
                ..HostInput::default()
            },
        },
    )
    .await;
    assert_eq!(kind(&r), Some(ErrorKind::NotFound));
    let r = ask(
        &g,
        Action::SaveGroup {
            group: GroupInput {
                id: None,
                name: "sub".into(),
                parent: Some(prod.id.0),
            },
        },
    )
    .await;
    assert_eq!(kind(&r), Some(ErrorKind::NotFound));
    assert!(fake.called().is_empty());
}

#[tokio::test]
async fn a_change_touches_only_what_it_names_and_shows_it() {
    let (fake, web, _, _) = world();
    {
        let mut hosts = fake.hosts.lock().unwrap();
        let w = hosts.iter_mut().find(|h| h.id == web.id).unwrap();
        w.settings.port = Some(2222);
        w.settings.key = Some(KeyRef::Agent {
            fingerprint: "SHA256:proton".into(),
        });
        w.tags = vec!["prod".into()];
    }
    let yes = Answer::new(Some(true));
    let g = gate(&fake, &yes);
    let r = ask(
        &g,
        Action::SaveHost {
            host: HostInput {
                id: Some(web.id.0),
                name: Some("web-1".into()),
                username: Some(Some("deploy".into())),
                ..HostInput::default()
            },
        },
    )
    .await;
    assert!(r.error.is_none(), "{r:?}");
    let saved = fake.stored(web.id);
    assert_eq!(saved.name, "web-1");
    assert_eq!(saved.settings.username.as_deref(), Some("deploy"));
    assert_eq!(saved.settings.address, web.settings.address);
    assert_eq!(saved.settings.port, Some(2222));
    assert!(matches!(saved.settings.key, Some(KeyRef::Agent { .. })));
    assert_eq!(saved.tags, vec!["prod".to_owned()]);
    let detail = yes.asked.lock().unwrap()[0].detail.clone();
    assert_eq!(detail, "name: web -> web-1\nuser: - -> deploy");
}

#[tokio::test]
async fn stored_sign_in_data_is_never_pointed_elsewhere() {
    let (fake, web, _, _) = world();
    fake.set(AgentCategory::Edit, AgentLevel::Allow);
    {
        let mut hosts = fake.hosts.lock().unwrap();
        let w = hosts.iter_mut().find(|h| h.id == web.id).unwrap();
        w.settings.password = Some(PasswordId(Uuid::new_v4()));
    }
    let g = gate(&fake, &Answer::new(None));
    let r = ask(
        &g,
        Action::SaveHost {
            host: HostInput {
                id: Some(web.id.0),
                address: Some(Some("evil.example".into())),
                ..HostInput::default()
            },
        },
    )
    .await;
    assert_eq!(kind(&r), Some(ErrorKind::Denied));
    // Renaming is fine.
    let r = ask(
        &g,
        Action::SaveHost {
            host: HostInput {
                id: Some(web.id.0),
                name: Some("web-2".into()),
                ..HostInput::default()
            },
        },
    )
    .await;
    assert!(r.error.is_none());
    // A new host in a group that signs in with an identity is refused too.
    let mut team = group("team", None);
    team.defaults.identity = Some(sshcraft_core::model::IdentityId(Uuid::new_v4()));
    fake.groups.lock().unwrap().push(team.clone());
    let r = ask(
        &g,
        Action::SaveHost {
            host: HostInput {
                name: Some("trap".into()),
                group: Some(Some(team.id.0)),
                address: Some(Some("evil.example".into())),
                ..HostInput::default()
            },
        },
    )
    .await;
    assert_eq!(kind(&r), Some(ErrorKind::Denied));
    assert_eq!(fake.called(), vec!["save_host web-2".to_owned()]);
}

#[tokio::test]
async fn the_decision_is_logged_before_running_and_failures_after() {
    let (fake, _, _, _) = world();
    fake.allow_connect();
    let g = gate(&fake, &Answer::new(None));
    let r = ask(&g, exec("web", "fail")).await;
    assert_eq!(kind(&r), Some(ErrorKind::Failed));
    assert_eq!(
        fake.outcomes(),
        vec![AgentOutcome::Allowed, AgentOutcome::Failed]
    );
}

#[tokio::test]
async fn nothing_runs_without_a_log_or_a_readable_policy() {
    let (fake, _, _, _) = world();
    fake.allow_connect();
    let g = gate(&fake, &Answer::new(None));
    fake.log_broken.store(true, Ordering::SeqCst);
    assert_eq!(
        kind(&ask(&g, exec("web", "uptime")).await),
        Some(ErrorKind::Failed)
    );
    fake.log_broken.store(false, Ordering::SeqCst);
    fake.policy_broken.store(true, Ordering::SeqCst);
    assert_eq!(
        kind(&ask(&g, Action::ListHosts).await),
        Some(ErrorKind::Failed)
    );
    assert!(fake.called().is_empty());
}

#[tokio::test]
async fn hiding_while_asked_stops_the_action() {
    let (fake, web, _, _) = world();
    let yes = Answer::new(Some(true));
    let hide = fake.clone();
    *yes.meanwhile.lock().unwrap() = Some(Box::new(move || hide.hide(web.id.0)));
    let r = ask(&gate(&fake, &yes), exec("web", "uptime")).await;
    assert_eq!(kind(&r), Some(ErrorKind::NotFound));
    assert!(fake.called().is_empty());
}

#[tokio::test]
async fn a_dropped_question_is_logged_as_cancelled() {
    let (fake, _, _, _) = world();
    let g = gate(&fake, &Answer::new(None));
    let conn = Connection::default();
    let pending = ask_on(&g, &conn, exec("web", "uptime"));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), pending)
            .await
            .is_err()
    );
    assert_eq!(fake.outcomes(), vec![AgentOutcome::Cancelled]);
}

#[tokio::test]
async fn key_material_never_leaves_but_command_output_may_name_keys() {
    let (fake, _, _, _) = world();
    fake.set(AgentCategory::Keys, AgentLevel::Allow);
    fake.allow_connect();
    let g = gate(&fake, &Answer::new(None));
    let r = ask(
        &g,
        Action::ImportKey {
            name: "k".into(),
            text: "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----"
                .into(),
            passphrase: None,
        },
    )
    .await;
    assert!(r.ok.is_none());
    assert_eq!(kind(&r), Some(ErrorKind::Failed));
    assert!(
        fake.log
            .lock()
            .unwrap()
            .iter()
            .all(|e| !e.detail.contains("AAAA"))
    );
    // A remote file the agent reads is its business; a host named like a key header lists fine.
    *fake.exec_output.lock().unwrap() = Some("id_rsa: OPENSSH PRIVATE KEY\n".into());
    assert!(
        ask(&g, exec("web", "file ~/.ssh/id_rsa"))
            .await
            .error
            .is_none()
    );
    fake.hosts
        .lock()
        .unwrap()
        .push(host("PRIVATE KEY store", None));
    assert!(ask(&g, Action::ListHosts).await.error.is_none());
}

#[tokio::test(start_paused = true)]
async fn an_unanswered_question_counts_as_no() {
    let (fake, _, _, _) = world();
    let r = ask(
        &gate(&fake, &Answer::new(None)),
        Action::SessionOpen { host: "web".into() },
    )
    .await;
    assert_eq!(kind(&r), Some(ErrorKind::Denied));
    assert!(fake.called().is_empty());
    assert_eq!(fake.outcomes(), vec![AgentOutcome::AskedDenied]);
}

#[tokio::test]
async fn sessions_belong_to_their_connection() {
    let (fake, _, _, _) = world();
    let yes = Answer::new(Some(true));
    let g = gate(&fake, &yes);
    let mine = Connection::default();
    let other = Connection::default();
    let opened = ask_on(&g, &mine, Action::SessionOpen { host: "web".into() }).await;
    let session = opened.ok.unwrap()["session"].as_str().unwrap().to_owned();
    let write = Action::SessionWrite {
        session: session.clone(),
        data: "ls\n".into(),
    };
    assert_eq!(
        kind(&ask_on(&g, &other, write.clone()).await),
        Some(ErrorKind::NotFound)
    );
    assert!(ask_on(&g, &mine, write).await.error.is_none());
    let read = Action::SessionRead {
        session: session.clone(),
        wait_ms: Some(10),
    };
    assert!(ask_on(&g, &mine, read).await.error.is_none());
    // Only opening asked; the foreign write is logged as failed, reading is not logged.
    assert_eq!(yes.count.load(Ordering::SeqCst), 1);
    assert_eq!(
        fake.outcomes(),
        vec![
            AgentOutcome::AskedAllowed,
            AgentOutcome::Failed,
            AgentOutcome::Allowed
        ]
    );
    // Closing works even with connecting blocked; a gone connection closes the rest.
    fake.deny_connect();
    let second = ask_on(&g, &mine, Action::SessionOpen { host: "web".into() }).await;
    assert_eq!(kind(&second), Some(ErrorKind::Denied));
    assert!(
        ask_on(
            &g,
            &mine,
            Action::SessionClose {
                session: session.clone()
            }
        )
        .await
        .error
        .is_none()
    );
    assert_eq!(fake.closed(), vec![session]);
}

#[tokio::test]
async fn a_name_two_hosts_share_must_be_an_id() {
    let (fake, _, _, _) = world();
    fake.hosts.lock().unwrap().push(host("web", None));
    let g = gate(&fake, &Answer::new(Some(true)));
    assert_eq!(
        kind(&ask(&g, Action::HostInfo { host: "web".into() }).await),
        Some(ErrorKind::Invalid)
    );
}

#[tokio::test]
async fn bad_agent_names_long_commands_and_timeouts_are_refused() {
    let (fake, _, _, _) = world();
    let g = gate(&fake, &Answer::new(Some(true)));
    for agent in ["\n", "claude\u{202E}edoc"] {
        let r = g
            .handle(
                Request {
                    id: 3,
                    agent: agent.into(),
                    action: Action::ListHosts,
                },
                &Connection::default(),
            )
            .await;
        assert_eq!(kind(&r), Some(ErrorKind::Invalid));
    }
    let r = ask(
        &g,
        Action::Exec {
            host: "web".into(),
            command: "sleep 1".into(),
            timeout_secs: Some(601),
        },
    )
    .await;
    assert_eq!(kind(&r), Some(ErrorKind::Invalid));
    let long = format!("{}rm -rf /", " ".repeat(MAX_COMMAND));
    assert_eq!(
        kind(&ask(&g, exec("web", &long)).await),
        Some(ErrorKind::Invalid)
    );
    assert!(fake.called().is_empty());
}

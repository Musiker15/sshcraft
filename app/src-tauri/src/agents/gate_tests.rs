use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use sshcraft_agent::{ExecResult, GroupInput, HostInput, KeyType, KeyView, SessionOutput};
use sshcraft_core::model::ConnectionSettings;

use super::*;

#[derive(Default)]
pub(crate) struct Fake {
    policy: Mutex<AgentPolicy>,
    hosts: Mutex<Vec<Host>>,
    groups: Mutex<Vec<Group>>,
    keys: Mutex<Vec<KeyView>>,
    log: Mutex<Vec<AgentLogEntry>>,
    calls: Mutex<Vec<String>>,
    sessions: Mutex<Vec<String>>,
}

impl Fake {
    /// One free host, `web`.
    pub(crate) fn with_web() -> Self {
        let fake = Self::default();
        *fake.hosts.lock().unwrap() = vec![host("web", None)];
        fake
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
}

#[async_trait::async_trait]
impl AgentBackend for Fake {
    fn policy(&self) -> AgentPolicy {
        self.policy.lock().unwrap().clone()
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
    fn log(&self, entry: AgentLogEntry) {
        self.log.lock().unwrap().push(entry);
    }
    async fn exec(
        &self,
        host: HostId,
        command: &str,
        _: Duration,
    ) -> Result<ExecResult, AgentError> {
        self.call(format!("exec {} {command}", host.0));
        Ok(ExecResult {
            output: "ok\n".into(),
            truncated: false,
            exit_status: Some(0),
        })
    }
    fn session_open(&self, host: HostId) -> Result<String, AgentError> {
        self.call(format!("open {}", host.0));
        let id = Uuid::new_v4().to_string();
        self.sessions.lock().unwrap().push(id.clone());
        Ok(id)
    }
    fn session_owned(&self, session: &str) -> bool {
        self.sessions.lock().unwrap().iter().any(|s| s == session)
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
        self.call(format!("close {session}"));
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

/// Answers questions with a fixed answer, or never.
struct Answer {
    answer: Option<bool>,
    asked: Mutex<Vec<AgentPrompt>>,
    count: AtomicUsize,
}

impl Answer {
    fn new(answer: Option<bool>) -> Arc<Self> {
        Arc::new(Self {
            answer,
            asked: Mutex::new(Vec::new()),
            count: AtomicUsize::new(0),
        })
    }
}

#[async_trait::async_trait]
impl Confirmer for Answer {
    async fn confirm(&self, prompt: AgentPrompt) -> bool {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.asked.lock().unwrap().push(prompt);
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
            password: Some(sshcraft_core::model::PasswordId(Uuid::new_v4())),
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

async fn ask(gate: &Gate<Fake, Answer>, action: Action) -> Response {
    gate.handle(Request {
        id: 1,
        agent: "claude-code".into(),
        action,
    })
    .await
}

fn kind(response: &Response) -> Option<ErrorKind> {
    response.error.as_ref().map(|e| e.kind)
}

#[tokio::test]
async fn ask_runs_only_after_yes() {
    let (fake, web, _, _) = world();
    let yes = Answer::new(Some(true));
    let r = ask(
        &gate(&fake, &yes),
        Action::Exec {
            host: "web".into(),
            command: "uptime".into(),
            timeout_secs: None,
        },
    )
    .await;
    assert!(r.error.is_none(), "{r:?}");
    assert_eq!(fake.called(), vec![format!("exec {} uptime", web.id.0)]);
    let asked = yes.asked.lock().unwrap()[0].clone();
    assert_eq!(asked.agent, "claude-code");
    assert_eq!(asked.target, "web (web.example.org)");
    assert_eq!(asked.detail, "uptime");
    assert_eq!(fake.outcomes(), vec![AgentOutcome::AskedAllowed]);

    let (fake, _, _, _) = world();
    let no = Answer::new(Some(false));
    let r = ask(
        &gate(&fake, &no),
        Action::Exec {
            host: "web".into(),
            command: "rm -rf /".into(),
            timeout_secs: None,
        },
    )
    .await;
    assert_eq!(kind(&r), Some(ErrorKind::Denied));
    assert!(fake.called().is_empty());
    assert_eq!(fake.outcomes(), vec![AgentOutcome::AskedDenied]);
}

#[tokio::test]
async fn allow_runs_without_asking_deny_never() {
    let (fake, _, _, _) = world();
    fake.set(AgentCategory::Connect, AgentLevel::Allow);
    let never = Answer::new(None);
    let g = gate(&fake, &never);
    let r = ask(&g, Action::SessionOpen { host: "web".into() }).await;
    assert!(r.error.is_none());
    assert_eq!(never.count.load(Ordering::SeqCst), 0);

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
    fake.set(AgentCategory::Connect, AgentLevel::Allow);
    fake.set(AgentCategory::Delete, AgentLevel::Allow);
    let never = Answer::new(None);
    let g = gate(&fake, &never);
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
        Action::Exec {
            host: db.id.0.to_string(),
            command: "ls".into(),
            timeout_secs: None,
        },
        Action::SessionOpen { host: "db".into() },
        Action::DeleteHost { host: "db".into() },
    ] {
        let r = ask(&g, action).await;
        assert_eq!(kind(&r), Some(ErrorKind::NotFound));
        assert_eq!(r.error.unwrap().message, "no such host");
    }
    assert!(fake.called().is_empty());
    let groups = ask(&g, Action::ListGroups).await.ok.unwrap();
    assert!(groups.as_array().unwrap().is_empty());
    assert_eq!(
        fake.outcomes()
            .iter()
            .filter(|o| **o == AgentOutcome::Hidden)
            .count(),
        4
    );
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
                id: None,
                name: "new".into(),
                group: Some(prod.id.0),
                address: None,
                port: None,
                username: None,
                key: None,
                tags: Vec::new(),
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
async fn saving_a_host_keeps_its_password_and_never_shows_it() {
    let (fake, web, _, _) = world();
    fake.set(AgentCategory::Edit, AgentLevel::Allow);
    let g = gate(&fake, &Answer::new(None));
    let r = ask(
        &g,
        Action::SaveHost {
            host: HostInput {
                id: Some(web.id.0),
                name: "web-renamed".into(),
                group: None,
                address: Some("10.1.1.1".into()),
                port: Some(2222),
                username: Some("deploy".into()),
                key: None,
                tags: vec!["x".into()],
            },
        },
    )
    .await;
    assert!(r.error.is_none(), "{r:?}");
    let saved = fake
        .hosts
        .lock()
        .unwrap()
        .iter()
        .find(|h| h.id == web.id)
        .cloned()
        .unwrap();
    assert_eq!(saved.name, "web-renamed");
    assert_eq!(saved.settings.port, Some(2222));
    assert_eq!(saved.settings.password, web.settings.password);
    let password = web.settings.password.unwrap().0.to_string();
    let listed = ask(&g, Action::ListHosts).await.ok.unwrap().to_string();
    assert!(!listed.contains(&password));
    assert!(!r.ok.unwrap().to_string().contains(&password));
}

#[tokio::test]
async fn key_material_never_leaves() {
    let (fake, _, _, _) = world();
    fake.set(AgentCategory::Keys, AgentLevel::Allow);
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
    // Neither the log nor a question saw the key text.
    let log = fake.log.lock().unwrap().clone();
    assert!(log.iter().all(|e| !e.detail.contains("PRIVATE")));
}

#[tokio::test(start_paused = true)]
async fn an_unanswered_question_counts_as_no() {
    let (fake, _, _, _) = world();
    let silent = Answer::new(None);
    let g = gate(&fake, &silent);
    let r = ask(&g, Action::SessionOpen { host: "web".into() }).await;
    assert_eq!(kind(&r), Some(ErrorKind::Denied));
    assert!(fake.called().is_empty());
    assert_eq!(fake.outcomes(), vec![AgentOutcome::AskedDenied]);
}

#[tokio::test]
async fn sessions_follow_their_opening() {
    let (fake, _, _, _) = world();
    let yes = Answer::new(Some(true));
    let g = gate(&fake, &yes);
    let opened = ask(&g, Action::SessionOpen { host: "web".into() }).await;
    let session = opened.ok.unwrap()["session"].as_str().unwrap().to_owned();
    for action in [
        Action::SessionWrite {
            session: session.clone(),
            data: "ls\n".into(),
        },
        Action::SessionRead {
            session: session.clone(),
            wait_ms: Some(10),
        },
        Action::SessionClose {
            session: session.clone(),
        },
    ] {
        assert!(ask(&g, action).await.error.is_none());
    }
    // Only opening asked; reading is not logged.
    assert_eq!(yes.count.load(Ordering::SeqCst), 1);
    assert_eq!(fake.log.lock().unwrap().len(), 3);
    let r = ask(
        &g,
        Action::SessionWrite {
            session: "someone-else".into(),
            data: "x".into(),
        },
    )
    .await;
    assert_eq!(kind(&r), Some(ErrorKind::NotFound));
}

#[tokio::test]
async fn a_name_two_hosts_share_must_be_an_id() {
    let (fake, _, _, _) = world();
    fake.hosts.lock().unwrap().push(host("web", None));
    let g = gate(&fake, &Answer::new(Some(true)));
    let r = ask(&g, Action::HostInfo { host: "web".into() }).await;
    assert_eq!(kind(&r), Some(ErrorKind::Invalid));
}

#[tokio::test]
async fn bad_agent_names_and_timeouts_are_refused() {
    let (fake, _, _, _) = world();
    let g = gate(&fake, &Answer::new(Some(true)));
    let r = g
        .handle(Request {
            id: 3,
            agent: "\n".into(),
            action: Action::ListHosts,
        })
        .await;
    assert_eq!(kind(&r), Some(ErrorKind::Invalid));
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
    assert!(fake.called().is_empty());
}

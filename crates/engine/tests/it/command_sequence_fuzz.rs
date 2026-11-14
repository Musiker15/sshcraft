//! Random sequences of commands from random callers against a real engine (test SSH server, in
//! memory vault), with the invariants checked after every step.
//!
//! `SSHCRAFT_FUZZ_SEEDS` (6) and `SSHCRAFT_FUZZ_STEPS` (40) set the size; `SSHCRAFT_FUZZ_SEED`
//! replays one seed; `SSHCRAFT_FUZZ_KEEP_GOING` reports every failing seed instead of the first.
//!
//! Left out on purpose: `sync.create_account` and `sync.join_account` (they talk to a sync
//! server over the network) and `key.passcli_import` (an outside program; its fake covers it).
//! Generated parameters never name a `path` or `dir`, so nothing is written outside the test.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use serde_json::{Map, Value, json};
use sshcraft_engine::cmd::command_specs;
use sshcraft_engine::journal::{MAX_BYTES, MAX_ENTRIES};
use sshcraft_engine::settings::EngineSettings;
use sshcraft_engine::{AgentConnection, AgentPrompt, Caller, Confirmer, EngineError, EngineEvent, Session, SessionConfig, Via};
use sshcraft_model::forward::{ForwardId, ForwardKind, ForwardRule};
use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId, KeyId, KeyRef, group_chain};
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_store::agent::{AgentCategory, AgentLevel, AgentPolicy, MAX_LOG};
use uuid::Uuid;

const EXCLUDED: &[&str] = &["sync.create_account", "sync.join_account", "key.passcli_import"];

/// A small, fast generator with a fixed sequence per seed.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<T: Clone>(&mut self, items: &[T]) -> Option<T> {
        (!items.is_empty()).then(|| items[self.below(items.len())].clone())
    }
}

/// Answers questions about agents' actions at random.
struct RandomAnswer(Mutex<SplitMix64>);

#[async_trait::async_trait]
impl Confirmer for RandomAnswer {
    async fn confirm(&self, _: AgentPrompt) -> bool {
        self.0.lock().unwrap().chance(50)
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_sequence_fuzz() {
    let seeds: Vec<u64> = match std::env::var("SSHCRAFT_FUZZ_SEED").ok().and_then(|s| s.parse().ok()) {
        Some(seed) => vec![seed],
        None => (0..env_u64("SSHCRAFT_FUZZ_SEEDS", 6)).map(|i| 0x5EED_0000 + i).collect(),
    };
    let steps = env_u64("SSHCRAFT_FUZZ_STEPS", 40);
    let keep_going = std::env::var_os("SSHCRAFT_FUZZ_KEEP_GOING").is_some();
    let mut failures = Vec::new();
    for seed in seeds {
        if let Err(failure) = run_seed(seed, steps).await {
            let report = format!("seed {seed}: {failure} (replay with SSHCRAFT_FUZZ_SEED={seed})");
            assert!(keep_going, "{report}");
            failures.push(report);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

struct World {
    session: Session,
    server: TestServer,
    agent: AgentConnection,
    control: AgentConnection,
    key: KeyId,
    hosts: Vec<HostId>,
    groups: Vec<GroupId>,
    sessions: Vec<Value>,
    rules: Vec<(HostId, ForwardId)>,
    /// Every password set so far: none may ever show in an answer or the journal.
    secrets: Vec<String>,
    rng: SplitMix64,
    names: u64,
}

impl World {
    fn name(&mut self, prefix: &str) -> String {
        self.names += 1;
        format!("{prefix}{}", self.names)
    }

    fn word(&mut self) -> String {
        const WORDS: &[&str] = &["ls", "uptime", "echo hi", "web", "db", "prod", "", "https://example.org", "äöü", "x\u{202E}y"];
        self.rng.pick(WORDS).unwrap_or_default().to_owned()
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    /// A host on the test server, with the stored key and maybe a forwarding rule.
    fn host(&mut self, id: HostId, group: Option<GroupId>) -> Host {
        let mut forwards = Vec::new();
        if self.rng.chance(30) {
            let rule = ForwardRule {
                id: ForwardId::new(),
                name: "fwd".into(),
                kind: ForwardKind::Local,
                bind_address: "127.0.0.1".into(),
                bind_port: Self::free_port(),
                target_host: "127.0.0.1".into(),
                target_port: self.server.port,
            };
            self.rules.push((id, rule.id));
            forwards.push(rule);
        }
        let jumps = if self.rng.chance(15) { self.rng.pick(&self.hosts).map(|h| vec![h]) } else { None };
        Host {
            id,
            name: self.name("h"),
            group_id: group,
            settings: ConnectionSettings {
                address: Some("127.0.0.1".into()),
                port: Some(self.server.port),
                username: Some("tester".into()),
                key: Some(KeyRef::Stored { id: self.key }),
                jump_hosts: jumps,
                ..Default::default()
            },
            location: None,
            tags: Vec::new(),
            os: None,
            forwards,
        }
    }

    fn some_group(&mut self) -> Option<GroupId> {
        if self.rng.chance(40) { None } else { self.rng.pick(&self.groups) }
    }

    fn random_value(&mut self) -> Value {
        match self.rng.below(9) {
            0 => self.rng.pick(&self.hosts).map_or(Value::Null, |h| json!(h.0.to_string())),
            1 => self.rng.pick(&self.groups).map_or(Value::Null, |g| json!(g.0.to_string())),
            2 => json!(Uuid::new_v4()),
            3 => json!(self.rng.below(100)),
            4 => json!(self.rng.chance(50)),
            5 => json!(self.word()),
            6 => Value::Null,
            7 => self.rng.pick(&self.sessions).unwrap_or(Value::Null),
            _ => json!({ "id": Uuid::new_v4(), "kind": "host" }),
        }
    }

    /// Parameters for any command: shapes that fit some command and many that fit none.
    fn random_params(&mut self) -> Value {
        const KEYS: &[&str] = &[
            "id",
            "host",
            "host_id",
            "group",
            "name",
            "command",
            "data",
            "query",
            "scope",
            "server",
            "area",
            "rule_id",
            "prompt_id",
            "accept",
            "allow",
            "category",
            "level",
            "hidden",
            "limit",
            "wait_ms",
            "cols",
            "rows",
            "format",
            "url",
            "ids",
            "key",
            "kind",
            "item_id",
        ];
        match self.rng.below(6) {
            0 => json!({}),
            1 => Value::Null,
            2 => json!([1, 2, 3]),
            _ => {
                let mut map = Map::new();
                for _ in 0..=self.rng.below(4) {
                    let key = self.rng.pick(KEYS).unwrap_or("id");
                    map.insert(key.to_owned(), self.random_value());
                }
                Value::Object(map)
            }
        }
    }

    /// A command from the curated list, with parameters that fit it for `caller`.
    async fn curated(&mut self, user: bool) -> (&'static str, Value) {
        match self.rng.below(18) {
            0 => {
                let existing = if self.rng.chance(40) { self.rng.pick(&self.hosts) } else { None };
                let id = existing.unwrap_or_else(HostId::new);
                if existing.is_none() {
                    self.hosts.push(id);
                }
                let group = self.some_group();
                if user {
                    ("host.save", json!({ "host": self.host(id, group) }))
                } else {
                    let name = self.name("a");
                    (
                        "host.save",
                        json!({ "host": { "id": existing.map(|h| h.0), "name": name, "group": group.map(|g| g.0), "address": "127.0.0.1", "port": self.server.port } }),
                    )
                }
            }
            1 => {
                let existing = if self.rng.chance(40) { self.rng.pick(&self.groups) } else { None };
                let id = existing.unwrap_or_else(|| GroupId(Uuid::new_v4()));
                if existing.is_none() {
                    self.groups.push(id);
                }
                let parent = self.some_group();
                let name = self.name("g");
                if user {
                    let group = Group { id, name, defaults: ConnectionSettings::default(), location: None, parent_id: parent, tags: Vec::new() };
                    ("group.save", json!({ "group": group }))
                } else {
                    ("group.save", json!({ "group": { "id": existing.map(|g| g.0), "name": name, "parent": parent.map(|g| g.0) } }))
                }
            }
            2 => {
                let host = self.rng.pick(&self.hosts).unwrap_or_default();
                if user { ("host.delete", json!({ "id": host })) } else { ("host.delete", json!({ "host": host.0.to_string() })) }
            }
            3 => {
                let group = self.rng.pick(&self.groups).unwrap_or_else(|| GroupId(Uuid::new_v4()));
                if user { ("group.delete", json!({ "id": group })) } else { ("group.delete", json!({ "group": group.0.to_string() })) }
            }
            4 => {
                let secret = format!("pw-{}-identity", self.rng.next());
                self.secrets.push(secret.clone());
                let name = self.name("i");
                ("identity.save", json!({ "identity": { "name": name, "username": "deploy", "password": { "kind": "set", "secret": secret } } }))
            }
            5 => {
                let secret = format!("pw-{}-host", self.rng.next());
                self.secrets.push(secret.clone());
                let host = self.rng.pick(&self.hosts).unwrap_or_default();
                ("password.set", json!({ "owner": { "kind": "host", "id": host }, "secret": secret }))
            }
            6 => {
                let targets: Vec<HostId> = self.rng.pick(&self.hosts).into_iter().collect();
                let name = self.name("s");
                ("snippet.save", json!({ "snippet": { "id": Uuid::new_v4(), "name": name, "content": "uptime", "default_targets": targets } }))
            }
            7 => {
                let pane = match self.rng.pick(&self.hosts) {
                    Some(host) => json!({ "target": { "kind": "host", "host_id": host }, "title": "pane" }),
                    None => json!({ "target": { "kind": "local", "shell": { "program": "cmd.exe", "args": [], "label": "cmd" } }, "title": "pane" }),
                };
                let name = self.name("w");
                (
                    "workspace.save",
                    json!({ "template": { "id": Uuid::new_v4(), "name": name, "layout": "split", "col_sizes": [1.0], "row_sizes": [1.0], "panes": [pane] } }),
                )
            }
            8 => {
                let id = if self.rng.chance(50) { self.rng.pick(&self.hosts).map(|h| h.0) } else { self.rng.pick(&self.groups).map(|g| g.0) };
                ("agent.set_hidden", json!({ "id": id.unwrap_or_else(Uuid::new_v4), "hidden": self.rng.chance(50) }))
            }
            9 => {
                let category = self.rng.pick(&AgentCategory::ALL).unwrap_or(AgentCategory::List);
                let level = self.rng.pick(&[AgentLevel::Allow, AgentLevel::Ask, AgentLevel::Deny]).unwrap_or(AgentLevel::Allow);
                ("agent.set_level", json!({ "category": category, "level": level }))
            }
            10 => {
                let mut settings = self.session.execute(Caller::User, "settings.get", Value::Null).await.unwrap();
                settings["appearance"]["font_size"] = json!(self.rng.below(200));
                settings["terminal"]["terminal_type"] = json!(self.word());
                settings["logs"]["log_retention_days"] = json!(self.rng.below(400));
                ("settings.save", json!({ "settings": settings }))
            }
            11 => {
                let scope = self.rng.pick(&self.hosts).map_or_else(|| "local".to_owned(), |h| format!("host:{}", h.0));
                ("history.record", json!({ "scope": scope, "command": self.word() }))
            }
            12 => {
                let host = self.rng.pick(&self.hosts).unwrap_or_default();
                if user {
                    ("session.open", json!({ "target": { "kind": "host", "host_id": host }, "cols": 80, "rows": 24 }))
                } else {
                    ("session.open", json!({ "host": host.0.to_string() }))
                }
            }
            13 => {
                let id = self.rng.pick(&self.sessions).unwrap_or(Value::Null);
                ("session.send", json!({ "id": id, "data": format!("{}\n", self.word()) }))
            }
            14 => ("session.close", json!({ "id": self.rng.pick(&self.sessions).unwrap_or(Value::Null) })),
            15 => ("session.read", json!({ "id": self.rng.pick(&self.sessions).unwrap_or(Value::Null), "wait_ms": 0 })),
            16 => {
                let (host, rule) = self.rng.pick(&self.rules).unwrap_or((HostId::new(), ForwardId::new()));
                ("forward.start", json!({ "host_id": host, "rule_id": rule }))
            }
            _ => {
                let (host, rule) = self.rng.pick(&self.rules).unwrap_or((HostId::new(), ForwardId::new()));
                ("forward.stop", json!({ "host_id": host, "rule_id": rule }))
            }
        }
    }

    async fn user(&self, id: &str) -> Value {
        self.session.execute(Caller::User, id, Value::Null).await.unwrap()
    }

    /// Ids of the hosts and groups agents must not see right now: hidden themselves, through a
    /// group, or (hosts) through a hidden jump host.
    async fn hidden(&self) -> HashSet<String> {
        let policy: AgentPolicy = serde_json::from_value(self.user("agent.policy").await).unwrap();
        let hosts: Vec<Host> = serde_json::from_value(self.user("host.list").await).unwrap();
        let groups: Vec<Group> = serde_json::from_value(self.user("group.list").await).unwrap();
        let mut hidden_hosts: HashSet<HostId> = hosts.iter().filter(|h| policy.host_hidden(&groups, h)).map(|h| h.id).collect();
        loop {
            let before = hidden_hosts.len();
            for host in &hosts {
                let jumps = host
                    .settings
                    .jump_hosts
                    .clone()
                    .or_else(|| host.group_id.and_then(|g| group_chain(&groups, g).iter().find_map(|g| g.defaults.jump_hosts.clone())));
                if jumps.unwrap_or_default().iter().any(|j| hidden_hosts.contains(j)) {
                    hidden_hosts.insert(host.id);
                }
            }
            if hidden_hosts.len() == before {
                break;
            }
        }
        let mut out: HashSet<String> = hidden_hosts.iter().map(|h| h.0.to_string()).collect();
        out.extend(groups.iter().filter(|g| policy.group_hidden(&groups, g.id)).map(|g| g.id.0.to_string()));
        out
    }

    /// The invariants that hold after every step, whoever called what.
    async fn check(&self) -> Result<(), String> {
        let hosts: Vec<Host> = serde_json::from_value(self.user("host.list").await).map_err(|e| format!("host.list: {e}"))?;
        let groups: Vec<Group> = serde_json::from_value(self.user("group.list").await).map_err(|e| format!("group.list: {e}"))?;
        for group in &groups {
            let mut seen = vec![group.id];
            let mut next = group.parent_id;
            while let Some(parent) = next {
                if seen.contains(&parent) {
                    return Err(format!("group chain of {} loops", group.name));
                }
                seen.push(parent);
                next = groups.iter().find(|g| g.id == parent).and_then(|g| g.parent_id);
            }
        }
        for host in &hosts {
            if let Some(group) = host.group_id
                && !groups.iter().any(|g| g.id == group)
            {
                return Err(format!("host {} points to a group that does not exist", host.name));
            }
        }
        let log = self.user("agent.log").await;
        if log.as_array().map_or(0, Vec::len) > MAX_LOG {
            return Err("the agent log grew past its limit".into());
        }
        let journal = self.session.journal();
        let text = serde_json::to_string(&journal).unwrap_or_default();
        if journal.len() > MAX_ENTRIES || text.len() > MAX_BYTES + 200_000 {
            return Err(format!("the journal grew past its limits: {} entries, {} bytes", journal.len(), text.len()));
        }
        if let Some(secret) = self.secrets.iter().find(|s| text.contains(s.as_str())) {
            return Err(format!("the journal holds a password: {secret}"));
        }
        let settings: EngineSettings = serde_json::from_value(self.user("settings.get").await).map_err(|e| format!("settings.get: {e}"))?;
        if settings.clone().sanitized() != settings {
            return Err(format!("settings outside their ranges: {settings:?}"));
        }
        Ok(())
    }
}

async fn run_seed(seed: u64, steps: u64) -> Result<(), String> {
    let dir = tempfile::tempdir().unwrap();
    let mut rng = SplitMix64(seed);
    let mut config = SessionConfig::for_tests(dir.path());
    config.confirmer = Some(Arc::new(RandomAnswer(Mutex::new(SplitMix64(rng.next())))));
    let (session, _) = Session::open(config).unwrap();
    // Every host key question is answered yes, as a user would for the test server.
    let mut events = session.subscribe();
    let answering = session.clone();
    let answers = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(EngineEvent::HostKeyPrompt(p)) => {
                    let _ = answering.execute(Caller::User, "prompt.host_key", json!({ "prompt_id": p.prompt_id, "accept": true })).await;
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return,
            }
        }
    });

    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server = TestServer::start_with(TestAuth { key: Some(key.public_key().clone()), ..Default::default() }).await;
    let imported = session
        .execute(Caller::User, "key.import", json!({ "name": "fuzz", "text": key.to_openssh(LineEnding::LF).unwrap().as_str() }))
        .await
        .unwrap();
    let key_id: KeyId = serde_json::from_value(imported["id"].clone()).unwrap();
    let agent = session.agent_connection("fuzz-agent", Via::Mcp).unwrap();
    let control = session.control_connection();
    let mut w = World {
        session: session.clone(),
        server,
        agent,
        control,
        key: key_id,
        hosts: Vec::new(),
        groups: Vec::new(),
        sessions: Vec::new(),
        rules: Vec::new(),
        secrets: Vec::new(),
        rng,
        names: 0,
    };

    // Two groups (one hidden), four hosts (one hidden, one in the hidden group).
    for _ in 0..2 {
        let id = GroupId(Uuid::new_v4());
        let group = Group { id, name: w.name("g"), defaults: ConnectionSettings::default(), location: None, parent_id: None, tags: Vec::new() };
        session.execute(Caller::User, "group.save", json!({ "group": group })).await.unwrap();
        w.groups.push(id);
    }
    for i in 0..4 {
        let id = HostId::new();
        let group = (i == 3).then_some(w.groups[1]);
        let host = w.host(id, group);
        session.execute(Caller::User, "host.save", json!({ "host": host })).await.unwrap();
        w.hosts.push(id);
    }
    for hidden in [w.hosts[0].0, w.groups[1].0] {
        session.execute(Caller::User, "agent.set_hidden", json!({ "id": hidden, "hidden": true })).await.unwrap();
    }
    let ask = w.rng.pick(&AgentCategory::ALL).unwrap_or(AgentCategory::Connect);
    for category in AgentCategory::ALL {
        let level = if category == ask { AgentLevel::Ask } else { AgentLevel::Allow };
        session.execute(Caller::User, "agent.set_level", json!({ "category": category, "level": level })).await.unwrap();
    }

    let specs: Vec<&'static str> = command_specs().iter().map(|s| s.id).filter(|id| !EXCLUDED.contains(id)).collect();
    let result = async {
        for step in 0..steps {
            let roll = w.rng.below(10);
            let caller = match roll {
                0..=3 => Caller::User,
                4..=7 => w.agent.caller(),
                _ => w.control.caller(),
            };
            let user = caller.is_user();
            let (id, params) = if w.rng.chance(75) { w.curated(user).await } else { (w.rng.pick(&specs).unwrap_or("host.list"), w.random_params()) };
            let hidden = if user { HashSet::new() } else { w.hidden().await };
            let result = session.execute(caller, id, params.clone()).await;
            let at = format!("step {step}, {id} {params}");
            if let Err(EngineError::Internal { .. }) = result {
                return Err(format!("{at}: the command panicked"));
            }
            if let Ok(value) = &result
                && id == "session.open"
                && let Some(opened) = value.get("session")
            {
                w.sessions.push(opened.clone());
            }
            if !user {
                let answer = match &result {
                    Ok(value) => value.to_string(),
                    Err(e) => e.to_string(),
                };
                let asked = params.to_string();
                if let Some(leak) = hidden.iter().find(|h| answer.contains(h.as_str()) && !asked.contains(h.as_str())) {
                    return Err(format!("{at}: an agent was told about hidden {leak}: {answer}"));
                }
                if answer.contains("PRIVATE KEY") {
                    return Err(format!("{at}: an agent got key material"));
                }
                if let Some(secret) = w.secrets.iter().find(|s| answer.contains(s.as_str())) {
                    return Err(format!("{at}: an agent got the password {secret}"));
                }
            }
            w.check().await.map_err(|e| format!("{at}: {e}"))?;
        }
        Ok(())
    }
    .await;
    answers.abort();
    result
}

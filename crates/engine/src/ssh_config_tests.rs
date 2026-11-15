use super::*;
use crate::SessionConfig;

struct World {
    session: Session,
    home: tempfile::TempDir,
    _data: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let (session, _) = Session::open(SessionConfig::for_tests(data.path())).unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".ssh")).unwrap();
        Self { session, home, _data: data }
    }

    fn config(&self) -> PathBuf {
        self.home.path().join(".ssh").join("config")
    }

    fn write(&self, text: &str) {
        std::fs::write(self.config(), text).unwrap();
    }

    async fn refresh(&self) -> RefreshOutcome {
        refresh_from(&self.session, self.home.path()).await.unwrap().0
    }

    fn host(&self, name: &str) -> Option<Host> {
        self.session.store().list_hosts().unwrap().into_iter().find(|h| h.name == name)
    }
}

#[tokio::test]
async fn live_refresh_adds_updates_and_unlinks() {
    let w = World::new();
    w.write("Host web\n  HostName web.org\n  User deploy\nHost db\n  HostName db.org\n");
    let first = w.refresh().await;
    assert_eq!((first.added, first.updated, first.unlinked), (2, 0, 0));
    let web = w.host("web").unwrap();
    assert_eq!(web.settings.address.as_deref(), Some("web.org"));
    let group = w.session.store().list_groups().unwrap().into_iter().find(|g| Some(g.id) == web.group_id).unwrap();
    assert_eq!(group.name, GROUP_NAME);

    w.write("Host web\n  HostName new.web.org\n  User deploy\n");
    let second = w.refresh().await;
    assert_eq!((second.added, second.updated, second.unlinked), (0, 1, 1));
    assert_eq!(w.host("web").unwrap().settings.address.as_deref(), Some("new.web.org"));
    assert_eq!(w.host("web").unwrap().id, web.id, "the same host, updated");
    // db left the file: it stays in SSHCraft, only its link is gone.
    assert!(w.host("db").is_some());
    assert_eq!(w.session.store().config_links().unwrap().len(), 1);
}

#[tokio::test]
async fn unchanged_file_is_not_applied_twice() {
    let w = World::new();
    w.write("Host web\n  HostName web.org\n");
    w.refresh().await;
    let mut web = w.host("web").unwrap();
    web.settings.username = Some("edited-here".into());
    w.session.store().upsert_host(&web).unwrap();
    let again = w.refresh().await;
    assert_eq!((again.added, again.updated), (0, 0));
    assert_eq!(w.host("web").unwrap().settings.username.as_deref(), Some("edited-here"), "an edit in SSHCraft stays until the file changes");
}

#[tokio::test]
async fn includes_are_followed_and_their_file_remembered() {
    let w = World::new();
    std::fs::write(w.home.path().join(".ssh").join("work.conf"), "Host work\n  HostName work.org\n").unwrap();
    w.write("Include work.conf\n");
    w.refresh().await;
    let work = w.host("work").unwrap();
    let link = w.session.store().config_links().unwrap().into_iter().find(|l| l.host_id == work.id).unwrap();
    assert!(link.file.ends_with("work.conf"), "{}", link.file);
}

#[tokio::test]
async fn live_hosts_approve_their_proxy_command() {
    let w = World::new();
    w.write("Host inner\n  HostName 10.0.0.5\n  ProxyCommand ssh -W %h:%p gw\n");
    w.refresh().await;
    let argv: Vec<String> = ["ssh", "-W", "%h:%p", "gw"].map(String::from).to_vec();
    assert_eq!(w.host("inner").unwrap().settings.proxy, Some(Proxy::Command { argv: argv.clone() }));
    assert!(w.session.store().command_approved(&sshcraft_store::store::command_digest(&argv)).unwrap());
}

#[tokio::test]
async fn write_back_is_atomic_and_keeps_a_backup() {
    let w = World::new();
    let original = "# mine\nHost web\n  HostName web.org\n  IdentityFile ~/.ssh/web\n";
    w.write(original);
    w.refresh().await;
    let mut web = w.host("web").unwrap();
    web.settings.port = Some(2222);
    w.session.store().upsert_host(&web).unwrap();
    let files = write_back(&w.session, &[web.id], w.home.path()).unwrap();
    assert_eq!(files.len(), 1);
    let now = std::fs::read_to_string(w.config()).unwrap();
    assert_eq!(now, "# mine\nHost web\n  HostName web.org\n  Port 2222\n  IdentityFile ~/.ssh/web\n");
    let backups: Vec<_> = std::fs::read_dir(backup_dir(w.home.path())).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(std::fs::read_to_string(&backups[0]).unwrap(), original);
    let left: Vec<_> =
        std::fs::read_dir(w.home.path().join(".ssh")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(!left.iter().any(|n| n.contains("sshcraft-tmp")), "{left:?}");
    // A second write-back keeps the first copy.
    std::thread::sleep(std::time::Duration::from_millis(5));
    write_back(&w.session, &[web.id], w.home.path()).unwrap();
    assert_eq!(std::fs::read_dir(backup_dir(w.home.path())).unwrap().count(), 2);
}

#[tokio::test]
async fn write_back_appends_a_new_host() {
    let w = World::new();
    let host = Host {
        id: HostId::new(),
        name: "fresh".into(),
        group_id: None,
        settings: ConnectionSettings { address: Some("fresh.org".into()), username: Some("me".into()), ..Default::default() },
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    };
    w.session.store().upsert_host(&host).unwrap();
    write_back(&w.session, &[host.id], w.home.path()).unwrap();
    assert_eq!(std::fs::read_to_string(w.config()).unwrap(), "Host fresh\n    HostName fresh.org\n    User me\n");
    assert!(w.session.store().config_links().unwrap().iter().any(|l| l.host_id == host.id && l.alias == "fresh"));
    // A name that cannot be an alias is refused, and nothing is written.
    let mut bad = host.clone();
    bad.id = HostId::new();
    bad.name = "two words".into();
    w.session.store().upsert_host(&bad).unwrap();
    assert!(write_back(&w.session, &[bad.id], w.home.path()).is_err());
    assert_eq!(std::fs::read_to_string(w.config()).unwrap(), "Host fresh\n    HostName fresh.org\n    User me\n");
}

#[tokio::test]
async fn agents_cannot_read_or_write_the_config() {
    let w = World::new();
    let agent = crate::caller::Caller::Agent {
        name: crate::caller::AgentName::new("claude-code").unwrap(),
        via: crate::caller::Via::Mcp,
        conn: crate::caller::ConnId(1),
    };
    for (id, params) in [("sshconfig.refresh", json!({})), ("sshconfig.write", json!({ "host_ids": [] }))] {
        assert!(w.session.execute(agent.clone(), id, params).await.is_err(), "{id}");
    }
}

/// A proxy command not allowed on this device never goes into the file: `ssh` would run it
/// through a shell, and the next refresh would take it as the user's own.
#[tokio::test]
async fn an_unapproved_command_is_not_written() {
    let w = World::new();
    let argv: Vec<String> = ["sh", "-c", "curl evil"].map(String::from).to_vec();
    let host = Host {
        id: HostId::new(),
        name: "synced".into(),
        group_id: None,
        settings: ConnectionSettings { address: Some("s.org".into()), proxy: Some(Proxy::Command { argv: argv.clone() }), ..Default::default() },
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    };
    w.session.store().upsert_host(&host).unwrap();
    assert!(write_back(&w.session, &[host.id], w.home.path()).is_err());
    assert!(!w.config().exists());
    // Counter-check: once allowed, it is written.
    w.session.store().approve_command(&sshcraft_store::store::command_digest(&argv)).unwrap();
    write_back(&w.session, &[host.id], w.home.path()).unwrap();
    assert!(std::fs::read_to_string(w.config()).unwrap().contains("ProxyCommand sh -c 'curl evil'"));
}

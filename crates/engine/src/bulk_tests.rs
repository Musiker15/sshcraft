//! `host.bulk_update`: one change to many hosts, each saved as `host.save` would.

use serde_json::json;
use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId, PasswordId};

use crate::{Caller, Session, SessionConfig};

fn host(name: &str, tags: &[&str]) -> Host {
    Host {
        id: HostId::new(),
        name: name.into(),
        group_id: None,
        settings: ConnectionSettings { address: Some(format!("{name}.example.org")), username: Some("old".into()), ..Default::default() },
        location: None,
        tags: tags.iter().map(|t| (*t).to_string()).collect(),
        os: None,
        forwards: Vec::new(),
    }
}

fn open() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    (dir, session)
}

fn stored(session: &Session, id: HostId) -> Host {
    session.store().list_hosts().unwrap().into_iter().find(|h| h.id == id).unwrap()
}

#[tokio::test]
async fn one_change_reaches_every_chosen_host() {
    let (_dir, session) = open();
    let (a, b, c) = (host("a", &["web"]), host("b", &["web", "old"]), host("c", &[]));
    let group = Group { id: GroupId::new(), name: "prod".into(), parent_id: None, defaults: Default::default(), location: None, tags: Vec::new() };
    session.execute(Caller::User, "group.save", json!({ "group": group })).await.unwrap();
    for h in [&a, &b, &c] {
        session.execute(Caller::User, "host.save", json!({ "host": h })).await.unwrap();
    }
    let patch = json!({ "group_id": group.id, "add_tags": ["db"], "remove_tags": ["old"], "username": "deploy", "port": 2222 });
    let out = session.execute(Caller::User, "host.bulk_update", json!({ "ids": [a.id, b.id, HostId::new()], "patch": patch })).await.unwrap();
    assert_eq!(out["updated"], 2);
    for id in [a.id, b.id] {
        let h = stored(&session, id);
        assert_eq!(h.group_id, Some(group.id));
        assert_eq!(h.settings.username.as_deref(), Some("deploy"));
        assert_eq!(h.settings.port, Some(2222));
        assert!(h.tags.contains(&"db".to_string()) && !h.tags.contains(&"old".to_string()));
    }
    assert_eq!(stored(&session, b.id).tags, vec!["web".to_string(), "db".to_string()]);
    let untouched = stored(&session, c.id);
    assert_eq!(untouched.settings.username.as_deref(), Some("old"), "a host not chosen stays as it was");
    // Back to the top level, user inherited again.
    let patch = json!({ "to_top_level": true, "username": "" });
    session.execute(Caller::User, "host.bulk_update", json!({ "ids": [a.id], "patch": patch })).await.unwrap();
    let h = stored(&session, a.id);
    assert_eq!(h.group_id, None);
    assert_eq!(h.settings.username, None);
}

#[tokio::test]
async fn a_bulk_change_keeps_passwords_and_leaves_one_version() {
    let (_dir, session) = open();
    let mut a = host("a", &[]);
    session.execute(Caller::User, "host.save", json!({ "host": a })).await.unwrap();
    // A password as the vault stores one: the change must not drop or replace it.
    a.settings.password = Some(PasswordId::new());
    session.store().upsert_host(&a).unwrap();
    let id = a.id.0.to_string();
    let versions = |s: &Session| s.store().list_versions(sshcraft_store::trash::TrashKind::Host, &id).unwrap().len();
    let before = versions(&session);
    session.execute(Caller::User, "host.bulk_update", json!({ "ids": [a.id], "patch": { "add_tags": ["t"] } })).await.unwrap();
    assert_eq!(stored(&session, a.id).settings.password, a.settings.password, "never touches secrets");
    assert_eq!(versions(&session), before + 1, "the stand before stays as one version");
}

#[tokio::test]
async fn limits_and_agents_are_refused() {
    let (_dir, session) = open();
    let ids: Vec<HostId> = (0..1001).map(|_| HostId::new()).collect();
    assert!(session.execute(Caller::User, "host.bulk_update", json!({ "ids": ids, "patch": {} })).await.is_err());
    assert!(session.execute(Caller::User, "host.bulk_update", json!({ "ids": [], "patch": { "port": 0 } })).await.is_err());
    use sshcraft_store::agent::{AgentCategory, AgentLevel, AgentPolicy};
    AgentPolicy::set_level(&session.store(), AgentCategory::Edit, AgentLevel::Allow).unwrap();
    let conn = session.agent_connection("claude-code", crate::caller::Via::Mcp).unwrap();
    assert!(session.execute(conn.caller(), "host.bulk_update", json!({ "ids": [], "patch": {} })).await.is_err());
}

use super::*;
use crate::vault::MemoryMasterKey;
use sshcraft_model::model::ConnectionSettings;

fn setup() -> (Store, Vault) {
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    (store, vault)
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

fn host(name: &str, group: Option<GroupId>) -> Host {
    Host {
        id: HostId(Uuid::new_v4()),
        name: name.into(),
        group_id: group,
        settings: ConnectionSettings::default(),
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    }
}

fn entry(at: i64, detail: &str) -> AgentLogEntry {
    AgentLogEntry {
        id: Uuid::new_v4(),
        at,
        agent: "claude-code".into(),
        action: "exec".into(),
        target: "web".into(),
        detail: detail.into(),
        outcome: AgentOutcome::AskedAllowed,
    }
}

#[test]
fn fresh_install_levels_follow_the_design() {
    let policy = AgentPolicy::default();
    assert_eq!(policy.level(AgentCategory::List), AgentLevel::Allow);
    for ask in [
        AgentCategory::Connect,
        AgentCategory::Edit,
        AgentCategory::Keys,
    ] {
        assert_eq!(policy.level(ask), AgentLevel::Ask, "{ask:?}");
    }
    for deny in [
        AgentCategory::Delete,
        AgentCategory::Share,
        AgentCategory::Sync,
    ] {
        assert_eq!(policy.level(deny), AgentLevel::Deny, "{deny:?}");
    }
}

#[test]
fn a_stored_level_wins_and_survives_reloading() {
    let (store, _) = setup();
    let mut policy = AgentPolicy::default();
    policy
        .levels
        .insert(AgentCategory::Connect, AgentLevel::Allow);
    policy.hidden.insert(Uuid::new_v4());
    policy.save(&store).unwrap();
    let loaded = AgentPolicy::load(&store);
    assert_eq!(loaded, policy);
    assert_eq!(loaded.level(AgentCategory::Connect), AgentLevel::Allow);
}

#[test]
fn unknown_or_broken_entries_fall_back_to_defaults() {
    let (store, _) = setup();
    store
        .put_setting(
            POLICY_KEY,
            r#"{"levels": {"teleport": "allow", "delete": "allow", "edit": "maybe"}}"#,
        )
        .unwrap();
    let loaded = AgentPolicy::load(&store);
    assert_eq!(loaded.level(AgentCategory::Delete), AgentLevel::Allow);
    assert_eq!(loaded.level(AgentCategory::Edit), AgentLevel::Ask);
    store.put_setting(POLICY_KEY, "not json").unwrap();
    assert_eq!(AgentPolicy::load(&store), AgentPolicy::default());
}

#[test]
fn hosts_below_a_hidden_group_are_hidden_at_any_depth() {
    let top = group("prod", None);
    let mid = group("eu", Some(top.id));
    let low = group("db", Some(mid.id));
    let other = group("dev", None);
    let deep = host("db-1", Some(low.id));
    let free = host("dev-1", Some(other.id));
    let single = host("router", None);
    let groups = vec![top.clone(), mid, low, other];
    let mut policy = AgentPolicy::default();
    policy.hidden.insert(top.id.0);
    policy.hidden.insert(single.id.0);
    let hosts = vec![deep.clone(), free.clone(), single.clone()];
    let hidden = policy.hidden_hosts(&hosts, &groups);
    assert!(hidden.contains(&deep.id));
    assert!(hidden.contains(&single.id));
    assert!(!hidden.contains(&free.id));
}

#[test]
fn a_group_cycle_does_not_hang() {
    let mut a = group("a", None);
    let b = group("b", Some(a.id));
    a.parent_id = Some(b.id);
    let groups = vec![a.clone(), b];
    let policy = AgentPolicy::default();
    assert!(!policy.group_hidden(&groups, a.id));
}

#[test]
fn the_log_keeps_the_newest_thousand() {
    let (store, vault) = setup();
    for at in 0..(MAX_LOG as i64 + 5) {
        log_append(&vault, &store, &entry(at, "uptime")).unwrap();
    }
    let list = log_list(&vault, &store).unwrap();
    assert_eq!(list.len(), MAX_LOG);
    assert_eq!(list[0].at, MAX_LOG as i64 + 4);
    assert_eq!(list.last().unwrap().at, 5);
    log_clear(&store).unwrap();
    assert!(log_list(&vault, &store).unwrap().is_empty());
}

#[test]
fn log_entries_are_sealed_and_cut() {
    let (store, vault) = setup();
    let long = "x".repeat(MAX_DETAIL + 10);
    log_append(&vault, &store, &entry(1, "mysql -psecret123")).unwrap();
    log_append(&vault, &store, &entry(2, &long)).unwrap();
    let raw: Vec<Vec<u8>> = store
        .conn
        .prepare("SELECT sealed FROM agent_log")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(raw.iter().all(|b| !b.windows(9).any(|w| w == b"secret123")));
    let list = log_list(&vault, &store).unwrap();
    assert_eq!(list[0].detail.chars().count(), MAX_DETAIL);
    assert_eq!(list[1].detail, "mysql -psecret123");
}

#[test]
fn a_damaged_hidden_list_is_an_error_not_an_empty_list() {
    let (store, _) = setup();
    assert!(AgentPolicy::load_checked(&store).is_ok());
    store
        .put_setting(POLICY_KEY, r#"{"hidden": ["not-a-uuid"]}"#)
        .unwrap();
    assert!(AgentPolicy::load_checked(&store).is_err());
    store.put_setting(POLICY_KEY, "{").unwrap();
    assert!(AgentPolicy::load_checked(&store).is_err());
}

#[test]
fn hiding_and_levels_change_one_thing_at_a_time() {
    let (store, _) = setup();
    let id = Uuid::new_v4();
    AgentPolicy::set_level(&store, AgentCategory::Delete, AgentLevel::Ask).unwrap();
    let policy = AgentPolicy::set_hidden(&store, id, true).unwrap();
    assert!(policy.hidden.contains(&id));
    assert_eq!(policy.level(AgentCategory::Delete), AgentLevel::Ask);
    let policy = AgentPolicy::set_hidden(&store, id, false).unwrap();
    assert!(policy.hidden.is_empty());
}

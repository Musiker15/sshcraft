use super::*;
use crate::model::{
    ConnectionSettings, Group, GroupId, Host, HostId, KeyId, KeyRef, ServerId, group_chain, resolve,
};

fn group(name: &str, parent: Option<GroupId>, defaults: ConnectionSettings) -> Group {
    Group {
        id: GroupId::new(),
        name: name.into(),
        defaults,
        location: None,
        parent_id: parent,
        tags: Vec::new(),
    }
}

fn host(name: &str, group_id: Option<GroupId>, settings: ConnectionSettings) -> Host {
    Host {
        forwards: Vec::new(),
        id: HostId::new(),
        name: name.into(),
        group_id,
        settings,
        location: None,
        tags: Vec::new(),
        os: None,
    }
}

/// top > middle > bottom, each saved.
fn three_levels(store: &Store) -> (Group, Group, Group) {
    let top = group(
        "top",
        None,
        ConnectionSettings {
            address: Some("top.example.org".into()),
            username: Some("top-user".into()),
            key: Some(KeyRef::Stored { id: KeyId::new() }),
            ..Default::default()
        },
    );
    let middle = group(
        "middle",
        Some(top.id),
        ConnectionSettings {
            username: Some("middle-user".into()),
            ..Default::default()
        },
    );
    let bottom = group("bottom", Some(middle.id), ConnectionSettings::default());
    for g in [&top, &middle, &bottom] {
        store.upsert_group(g).unwrap();
    }
    (top, middle, bottom)
}

#[test]
fn migration_from_v4_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v4.db");
    let id = GroupId::new();
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(SCHEMA_V2).unwrap();
        conn.execute_batch(SCHEMA_V3).unwrap();
        conn.execute_batch(share_store::SCHEMA_V4).unwrap();
        conn.pragma_update(None, "user_version", 4).unwrap();
        conn.execute(
            "INSERT INTO groups (id, name, defaults) VALUES (?1, 'old', '{}')",
            [id.0.to_string()],
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let groups = store.list_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].parent_id, None);
    assert!(groups[0].tags.is_empty());
}

#[test]
fn three_levels_inherit() {
    let store = Store::open_in_memory().unwrap();
    let (_, _, bottom) = three_levels(&store);
    let h = host(
        "h",
        Some(bottom.id),
        ConnectionSettings {
            port: Some(2200),
            ..Default::default()
        },
    );
    let groups = store.list_groups().unwrap();
    let chain = group_chain(&groups, bottom.id);
    let refs: Vec<&Group> = chain.iter().collect();
    let resolved = resolve(&h, &refs, &[]).unwrap();
    assert_eq!(resolved.address, "top.example.org");
    assert_eq!(resolved.port, 2200);
    assert_eq!(resolved.username, "middle-user", "the nearest group wins");
}

#[test]
fn nearest_group_wins() {
    let store = Store::open_in_memory().unwrap();
    let (top, middle, _) = three_levels(&store);
    let h = host("h", Some(middle.id), ConnectionSettings::default());
    let chain = [&middle, &top];
    assert_eq!(resolve(&h, &chain, &[]).unwrap().username, "middle-user");
    // A chain that is not the host's own is refused.
    let wrong = [&top];
    assert!(resolve(&h, &wrong, &[]).is_err());
}

#[test]
fn group_into_own_subgroup_is_refused() {
    let store = Store::open_in_memory().unwrap();
    let (mut top, _, bottom) = three_levels(&store);
    top.parent_id = Some(bottom.id);
    assert!(matches!(
        store.upsert_group(&top),
        Err(StoreError::GroupCycle)
    ));
    top.parent_id = Some(top.id);
    assert!(matches!(
        store.upsert_group(&top),
        Err(StoreError::GroupCycle)
    ));
    top.parent_id = Some(GroupId::new());
    assert!(
        store.upsert_group(&top).is_err(),
        "a missing parent is refused"
    );
}

#[test]
fn subgroup_follows_parent_location() {
    let store = Store::open_in_memory().unwrap();
    let server = ServerId::new();
    let (top, middle, bottom) = three_levels(&store);
    let h = host("h", Some(bottom.id), ConnectionSettings::default());
    store.upsert_host(&h).unwrap();
    // Can't add a server row here without the sync tables' needs; set_location only stores ids.
    store
        .set_location(
            crate::store::ItemKind::Group,
            &top.id.0.to_string(),
            Some(server),
        )
        .unwrap();
    for g in store.list_groups().unwrap() {
        assert_eq!(g.location, Some(server), "{}", g.name);
    }
    assert_eq!(store.list_hosts().unwrap()[0].location, Some(server));
    assert!(
        store
            .set_location(
                crate::store::ItemKind::Group,
                &middle.id.0.to_string(),
                None
            )
            .is_err(),
        "a subgroup moves only with its parent"
    );
    // A new subgroup of a group at the server is at the server too.
    let late = group("late", Some(bottom.id), ConnectionSettings::default());
    store.upsert_group(&late).unwrap();
    let late = store
        .list_groups()
        .unwrap()
        .into_iter()
        .find(|g| g.id == late.id)
        .unwrap();
    assert_eq!(late.location, Some(server));
    assert_eq!(store.subgroups(top.id).unwrap().len(), 3);
}

#[test]
fn deleting_a_group_deletes_subgroups_and_their_hosts() {
    let store = Store::open_in_memory().unwrap();
    let (top, _, bottom) = three_levels(&store);
    store
        .upsert_host(&host("h", Some(bottom.id), ConnectionSettings::default()))
        .unwrap();
    store.delete_group(top.id).unwrap();
    assert!(store.list_groups().unwrap().is_empty());
    assert!(store.list_hosts().unwrap().is_empty());
}

#[test]
fn group_chain_stops_on_a_cycle() {
    let a = group("a", None, ConnectionSettings::default());
    let mut b = group("b", Some(a.id), ConnectionSettings::default());
    let mut a2 = a.clone();
    a2.parent_id = Some(b.id);
    b.parent_id = Some(a2.id);
    let chain = group_chain(&[a2.clone(), b.clone()], b.id);
    assert_eq!(chain.len(), 2);
    assert_eq!(group_chain(&[a2], GroupId::new()).len(), 0);
}

#[test]
fn tags_are_normalized() {
    let store = Store::open_in_memory().unwrap();
    let mut h = host("h", None, ConnectionSettings::default());
    h.tags = vec![
        " prod ".into(),
        "".into(),
        "Prod".into(),
        "web".into(),
        "x".repeat(100),
    ];
    store.upsert_host(&h).unwrap();
    let saved = store.list_hosts().unwrap().pop().unwrap();
    assert_eq!(
        saved.tags,
        vec!["prod".to_owned(), "web".to_owned(), "x".repeat(64)]
    );
}

#[test]
fn old_host_json_without_tags_reads() {
    let text = r#"{"id":"0b1c5d8a-6b8e-4e5e-9f0a-1a2b3c4d5e6f","name":"old","group_id":null,
        "settings":{"address":null,"port":null,"username":null,"key":null}}"#;
    let h: Host = serde_json::from_str(text).unwrap();
    assert!(h.tags.is_empty());
    assert_eq!(h.os, None);
    let g: Group = serde_json::from_str(
        r#"{"id":"0b1c5d8a-6b8e-4e5e-9f0a-1a2b3c4d5e6f","name":"g","defaults":{"address":null,"port":null,"username":null,"key":null}}"#,
    )
    .unwrap();
    assert_eq!(g.parent_id, None);
}

#[test]
fn too_deep_is_refused() {
    let store = Store::open_in_memory().unwrap();
    let mut parent: Option<GroupId> = None;
    for i in 0..crate::model::MAX_GROUP_DEPTH {
        let g = group(&format!("g{i}"), parent, ConnectionSettings::default());
        store.upsert_group(&g).unwrap();
        parent = Some(g.id);
    }
    let one_more = group("deep", parent, ConnectionSettings::default());
    assert!(matches!(
        store.upsert_group(&one_more),
        Err(StoreError::TooDeep)
    ));
}

#[test]
fn unknown_os_reads_as_other() {
    let text = r#"{"id":"0b1c5d8a-6b8e-4e5e-9f0a-1a2b3c4d5e6f","name":"h","group_id":null,
        "settings":{"address":null,"port":null,"username":null,"key":null},"os":"plan9"}"#;
    let h: Host = serde_json::from_str(text).unwrap();
    assert_eq!(h.os, Some(crate::model::HostOs::Other));
}

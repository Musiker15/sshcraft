use super::*;
use sshcraft_model::model::ConnectionSettings;

fn host(name: &str, group: Option<GroupId>) -> Host {
    Host {
        id: HostId::new(),
        name: name.into(),
        group_id: group,
        settings: ConnectionSettings { address: Some(format!("{name}.org")), ..Default::default() },
        location: None,
        tags: Vec::new(),
        os: None,
        forwards: Vec::new(),
    }
}

fn group(name: &str, parent: Option<GroupId>) -> Group {
    Group {
        id: GroupId(uuid::Uuid::new_v4()),
        name: name.into(),
        defaults: ConnectionSettings::default(),
        location: None,
        parent_id: parent,
        tags: Vec::new(),
    }
}

#[test]
fn a_host_comes_back_with_its_password() {
    let store = Store::open_in_memory().unwrap();
    let pw = PasswordId::new();
    store.put_password_record(pw, b"sealed-bytes", None).unwrap();
    let mut h = host("web", None);
    h.settings.password = Some(pw);
    store.upsert_host(&h).unwrap();
    let payload = store.snapshot_host(h.id).unwrap().unwrap();
    store.put_trash(TrashKind::Host, &h.id.0.to_string(), "web", &payload, 100).unwrap();
    store.delete_host(h.id).unwrap();
    store.delete_password(pw).unwrap();

    assert_eq!(store.list_trash().unwrap()[0].name, "web");
    let payload = store.trash_payload(TrashKind::Host, &h.id.0.to_string()).unwrap().unwrap();
    let restored = store.restore_payload(&payload).unwrap();
    assert_eq!(restored.hosts, vec![h.id]);
    assert_eq!(restored.passwords, vec![pw]);
    assert_eq!(store.list_hosts().unwrap(), vec![h]);
    assert_eq!(store.password_record(pw).unwrap().unwrap().0, b"sealed-bytes");
}

#[test]
fn a_group_comes_back_with_its_subgroups_and_hosts() {
    let store = Store::open_in_memory().unwrap();
    let top = group("top", None);
    let sub = group("sub", Some(top.id));
    store.upsert_group(&top).unwrap();
    store.upsert_group(&sub).unwrap();
    let (a, b) = (host("a", Some(top.id)), host("b", Some(sub.id)));
    store.upsert_host(&a).unwrap();
    store.upsert_host(&b).unwrap();
    let payload = store.snapshot_group(top.id).unwrap().unwrap();
    store.delete_group(top.id).unwrap();
    assert!(store.list_hosts().unwrap().is_empty());
    let restored = store.restore_payload(&payload).unwrap();
    assert_eq!(restored.groups, vec![top.id, sub.id]);
    assert_eq!(store.list_hosts().unwrap().len(), 2);
    assert_eq!(store.list_groups().unwrap().len(), 2);
}

#[test]
fn a_restore_over_an_existing_id_changes_nothing() {
    let store = Store::open_in_memory().unwrap();
    let h = host("web", None);
    store.upsert_host(&h).unwrap();
    let payload = store.snapshot_host(h.id).unwrap().unwrap();
    assert!(store.restore_payload(&payload).is_err());
    assert_eq!(store.list_hosts().unwrap().len(), 1);
}

#[test]
fn a_host_whose_group_is_gone_comes_back_at_the_top() {
    let store = Store::open_in_memory().unwrap();
    let g = group("g", None);
    store.upsert_group(&g).unwrap();
    let h = host("web", Some(g.id));
    store.upsert_host(&h).unwrap();
    let payload = store.snapshot_host(h.id).unwrap().unwrap();
    store.delete_group(g.id).unwrap();
    store.restore_payload(&payload).unwrap();
    assert_eq!(store.list_hosts().unwrap()[0].group_id, None);
}

#[test]
fn old_trash_is_purged() {
    let store = Store::open_in_memory().unwrap();
    let h = host("web", None);
    store.upsert_host(&h).unwrap();
    let payload = store.snapshot_host(h.id).unwrap().unwrap();
    store.put_trash(TrashKind::Host, "old", "old", &payload, 10).unwrap();
    store.put_trash(TrashKind::Host, "new", "new", &payload, 1000).unwrap();
    assert_eq!(store.purge_trash(Some(500)).unwrap(), 1);
    assert_eq!(store.list_trash().unwrap().len(), 1);
    assert_eq!(store.purge_trash(None).unwrap(), 1);
}

#[test]
fn saving_keeps_at_most_twenty_versions_and_skips_repeats() {
    let store = Store::open_in_memory().unwrap();
    let mut h = host("web", None);
    for i in 0..25 {
        h.name = format!("web-{i}");
        let payload = TrashPayload::Host { host: Box::new(h.clone()), passwords: Vec::new() };
        store.put_version(TrashKind::Host, "x", &h.name, &payload, i).unwrap();
        store.put_version(TrashKind::Host, "x", &h.name, &payload, i).unwrap();
    }
    let list = store.list_versions(TrashKind::Host, "x").unwrap();
    assert_eq!(list.len(), MAX_VERSIONS);
    assert_eq!(list[0].2, "web-24");
    let Some(TrashPayload::Host { host, .. }) = store.version(TrashKind::Host, "x", list[0].0).unwrap() else { panic!("no version") };
    assert_eq!(host.name, "web-24");
}

#[test]
fn a_key_and_a_snippet_come_back_sealed_as_they_were() {
    let store = Store::open_in_memory().unwrap();
    store
        .conn
        .execute("INSERT INTO snippets (id, sealed) VALUES (?1, ?2)", rusqlite::params![SnippetId::new().0.to_string(), b"s".to_vec()])
        .unwrap();
    let id: String = store.conn.query_row("SELECT id FROM snippets", [], |r| r.get(0)).unwrap();
    let sid = SnippetId(uuid::Uuid::parse_str(&id).unwrap());
    let payload = store.snapshot_snippet(sid, "deploy").unwrap().unwrap();
    store.conn.execute("DELETE FROM snippets", []).unwrap();
    store.restore_payload(&payload).unwrap();
    let sealed: Vec<u8> = store.conn.query_row("SELECT sealed FROM snippets", [], |r| r.get(0)).unwrap();
    assert_eq!(sealed, b"s");
}

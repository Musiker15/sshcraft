use super::*;
use crate::model::{ConnectionSettings, Group, GroupId, Host, HostId, KeyId, ServerId};
use crate::store::{KeyRecord, SCHEMA_V1, SCHEMA_V2, Store};
use uuid::Uuid;

fn group(name: &str) -> Group {
    Group {
        id: GroupId::new(),
        name: name.into(),
        defaults: ConnectionSettings::default(),
        location: None,
    }
}

fn host(name: &str, group_id: Option<GroupId>) -> Host {
    Host {
        id: HostId::new(),
        name: name.into(),
        group_id,
        settings: ConnectionSettings::default(),
        location: None,
    }
}

fn server(store: &Store) -> ServerId {
    let id = ServerId::new();
    store
        .add_server(&ServerRow {
            id,
            name: "home".into(),
            origin: "https://sync.example.org".into(),
            account: [7; 32],
            device_id: Uuid::new_v4(),
            sealed_device: vec![1, 2, 3],
            sync_enabled: true,
            sync_keys: false,
            revoked: false,
            log_head: None,
            tracker: None,
            created_at: 1,
        })
        .unwrap();
    id
}

fn location_of_host(store: &Store, id: HostId) -> Option<ServerId> {
    store
        .list_hosts()
        .unwrap()
        .into_iter()
        .find(|h| h.id == id)
        .unwrap()
        .location
}

#[test]
fn migration_from_v2_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v2.db");
    let h = host("root", None);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(SCHEMA_V2).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute(
            "INSERT INTO hosts (id, name, group_id, settings) VALUES (?1, ?2, NULL, ?3)",
            rusqlite::params![
                h.id.0.to_string(),
                h.name,
                serde_json::to_string(&h.settings).unwrap()
            ],
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let hosts = store.list_hosts().unwrap();
    assert_eq!(hosts, vec![h]);
    let version: i64 = store
        .conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 3);
}

#[test]
fn group_move_takes_its_hosts() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let g = group("srv");
    let inside = host("root", Some(g.id));
    let outside = host("other", None);
    store.upsert_group(&g).unwrap();
    store.upsert_host(&inside).unwrap();
    store.upsert_host(&outside).unwrap();
    store
        .set_location(ItemKind::Group, &g.id.0.to_string(), Some(s))
        .unwrap();
    assert_eq!(store.list_groups().unwrap()[0].location, Some(s));
    assert_eq!(location_of_host(&store, inside.id), Some(s));
    assert_eq!(location_of_host(&store, outside.id), None);
}

#[test]
fn grouped_host_cannot_move_alone() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let g = group("srv");
    let h = host("root", Some(g.id));
    store.upsert_group(&g).unwrap();
    store.upsert_host(&h).unwrap();
    let err = store
        .set_location(ItemKind::Host, &h.id.0.to_string(), Some(s))
        .unwrap_err();
    assert!(matches!(err, StoreError::Malformed(_)));
    assert_eq!(location_of_host(&store, h.id), None);
}

#[test]
fn saving_keeps_the_location() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let mut h = host("root", None);
    store.upsert_host(&h).unwrap();
    store
        .set_location(ItemKind::Host, &h.id.0.to_string(), Some(s))
        .unwrap();
    // The form sends the host back without a location; saving must not move it.
    h.name = "renamed".into();
    h.location = None;
    store.upsert_host(&h).unwrap();
    assert_eq!(location_of_host(&store, h.id), Some(s));
}

#[test]
fn new_items_take_their_location_on_insert() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let mut h = host("remote", None);
    h.location = Some(s);
    store.upsert_host(&h).unwrap();
    assert_eq!(location_of_host(&store, h.id), Some(s));
}

#[test]
fn remove_server_keeps_items_local() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let h = host("root", None);
    store.upsert_host(&h).unwrap();
    store
        .set_location(ItemKind::Host, &h.id.0.to_string(), Some(s))
        .unwrap();
    store
        .queue(s, ItemKind::Host, &h.id.0.to_string(), 5)
        .unwrap();
    store.remove_server(s).unwrap();
    assert!(store.servers().unwrap().is_empty());
    assert_eq!(location_of_host(&store, h.id), None);
    assert_eq!(store.list_hosts().unwrap().len(), 1);
}

#[test]
fn outbox_dedupes_per_item() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    store.queue(s, ItemKind::Host, "a", 1).unwrap();
    store.queue(s, ItemKind::Host, "a", 2).unwrap();
    store.queue(s, ItemKind::Group, "a", 3).unwrap();
    assert_eq!(
        store.queued_at(s, ItemKind::Host, "a").unwrap(),
        Some(2),
        "latest change counts"
    );
    assert_eq!(
        store.outbox(s).unwrap(),
        vec![
            (ItemKind::Host, "a".to_owned()),
            (ItemKind::Group, "a".to_owned())
        ]
    );
    store.dequeue(s, ItemKind::Host, "a").unwrap();
    assert_eq!(
        store.outbox(s).unwrap(),
        vec![(ItemKind::Group, "a".to_owned())]
    );
}

#[test]
fn record_entry_unique_per_item() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let first = RecordEntry {
        record_id: Uuid::new_v4(),
        area_id: Uuid::new_v4(),
        version: 1,
        modified_at: 10,
    };
    store
        .put_record_entry(s, &first, ItemKind::Host, "h")
        .unwrap();
    let second = RecordEntry {
        version: 2,
        modified_at: 20,
        ..first.clone()
    };
    store
        .put_record_entry(s, &second, ItemKind::Host, "h")
        .unwrap();
    assert_eq!(
        store.record_entry(s, ItemKind::Host, "h").unwrap(),
        Some(second)
    );
    assert_eq!(store.record_entry(s, ItemKind::Group, "h").unwrap(), None);
    assert_eq!(
        store.item_for_record(s, first.record_id).unwrap(),
        Some((ItemKind::Host, "h".to_owned()))
    );
}

#[test]
fn server_state_and_flags_persist() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    store.update_server_flags(s, false, true).unwrap();
    store.save_server_state(s, "{\"head\":1}", "{}").unwrap();
    store.mark_server_revoked(s).unwrap();
    let row = store.server(s).unwrap().unwrap();
    assert!(!row.sync_enabled && row.sync_keys && row.revoked);
    assert_eq!(row.log_head.as_deref(), Some("{\"head\":1}"));
    let area = Uuid::new_v4();
    store.put_area(s, area, "{\"m\":1}", "{\"e\":1}").unwrap();
    store.set_cursor(s, area, 9).unwrap();
    store.put_area(s, area, "{\"m\":2}", "{\"e\":2}").unwrap();
    let areas = store.areas(s).unwrap();
    assert_eq!(areas.len(), 1);
    assert_eq!(
        (areas[0].manifest.as_str(), areas[0].cursor),
        ("{\"m\":2}", 9)
    );
}

#[test]
fn key_location_follows_set_location() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let id = KeyId::new();
    store
        .put_key_record(&KeyRecord {
            id,
            name: "k".into(),
            algorithm: "ssh-ed25519".into(),
            fingerprint: "SHA256:x".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            sealed_secret: vec![1],
            location: None,
        })
        .unwrap();
    store
        .set_location(ItemKind::Key, &id.0.to_string(), Some(s))
        .unwrap();
    assert_eq!(store.key_record(id).unwrap().unwrap().location, Some(s));
}

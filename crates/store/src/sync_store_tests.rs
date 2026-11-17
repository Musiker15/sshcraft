use super::*;
use crate::store::{KeyRecord, SCHEMA_V1, SCHEMA_V2, Store};
use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId, KeyId, ServerId};
use uuid::Uuid;

fn group(name: &str) -> Group {
    Group { id: GroupId::new(), name: name.into(), defaults: ConnectionSettings::default(), location: None, parent_id: None, tags: Vec::new() }
}

fn host(name: &str, group_id: Option<GroupId>) -> Host {
    Host {
        forwards: Vec::new(),
        id: HostId::new(),
        name: name.into(),
        group_id,
        settings: ConnectionSettings::default(),
        location: None,
        tags: Vec::new(),
        os: None,
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
    store.list_hosts().unwrap().into_iter().find(|h| h.id == id).unwrap().location
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
            rusqlite::params![h.id.0.to_string(), h.name, serde_json::to_string(&h.settings).unwrap()],
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let hosts = store.list_hosts().unwrap();
    assert_eq!(hosts, vec![h]);
    let version: i64 = store.conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, crate::store::SCHEMA_VERSION);
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
    store.set_location(ItemKind::Group, &g.id.0.to_string(), Some(s)).unwrap();
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
    let err = store.set_location(ItemKind::Host, &h.id.0.to_string(), Some(s)).unwrap_err();
    assert!(matches!(err, StoreError::Malformed(_)));
    assert_eq!(location_of_host(&store, h.id), None);
}

#[test]
fn saving_keeps_the_location() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let mut h = host("root", None);
    store.upsert_host(&h).unwrap();
    store.set_location(ItemKind::Host, &h.id.0.to_string(), Some(s)).unwrap();
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
    store.set_location(ItemKind::Host, &h.id.0.to_string(), Some(s)).unwrap();
    store.queue(s, ItemKind::Host, &h.id.0.to_string(), 5).unwrap();
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
    assert_eq!(store.queued_at(s, ItemKind::Host, "a").unwrap(), Some(2), "latest change counts");
    assert_eq!(store.outbox(s).unwrap(), vec![(ItemKind::Host, "a".to_owned()), (ItemKind::Group, "a".to_owned())]);
    store.dequeue(s, ItemKind::Host, "a").unwrap();
    assert_eq!(store.outbox(s).unwrap(), vec![(ItemKind::Group, "a".to_owned())]);
}

#[test]
fn record_entry_unique_per_item() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let first = RecordEntry { record_id: Uuid::new_v4(), area_id: Uuid::new_v4(), version: 1, modified_at: 10 };
    store.put_record_entry(s, &first, ItemKind::Host, "h").unwrap();
    let second = RecordEntry { version: 2, modified_at: 20, ..first.clone() };
    store.put_record_entry(s, &second, ItemKind::Host, "h").unwrap();
    assert_eq!(store.record_entry(s, ItemKind::Host, "h").unwrap(), Some(second));
    assert_eq!(store.record_entry(s, ItemKind::Group, "h").unwrap(), None);
    assert_eq!(store.item_for_record(s, first.record_id).unwrap(), Some((ItemKind::Host, "h".to_owned())));
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
    assert_eq!((areas[0].manifest.as_str(), areas[0].cursor), ("{\"m\":2}", 9));
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
            kind: Default::default(),
            protected: false,
            certificate: None,
        })
        .unwrap();
    store.set_location(ItemKind::Key, &id.0.to_string(), Some(s)).unwrap();
    assert_eq!(store.key_record(id).unwrap().unwrap().location, Some(s));
}

#[test]
fn host_takes_its_groups_location() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let g = group("srv");
    store.upsert_group(&g).unwrap();
    store.set_location(ItemKind::Group, &g.id.0.to_string(), Some(s)).unwrap();
    // New host in a synced group, sent without a location by the form.
    let h = host("new", Some(g.id));
    store.upsert_host(&h).unwrap();
    assert_eq!(location_of_host(&store, h.id), Some(s));
    // A local host moved into the group follows it too.
    let mut loose = host("loose", None);
    store.upsert_host(&loose).unwrap();
    loose.group_id = Some(g.id);
    store.upsert_host(&loose).unwrap();
    assert_eq!(location_of_host(&store, loose.id), Some(s));
}

#[test]
fn dequeue_keeps_a_change_made_while_sending() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    store.queue(s, ItemKind::Host, "a", 1).unwrap();
    // The record for the change at 1 is on its way; the user changes the host again.
    store.queue(s, ItemKind::Host, "a", 2).unwrap();
    store.dequeue_if(s, ItemKind::Host, "a", 1).unwrap();
    assert_eq!(store.queued_at(s, ItemKind::Host, "a").unwrap(), Some(2));
    store.dequeue_if(s, ItemKind::Host, "a", 2).unwrap();
    assert_eq!(store.queued_at(s, ItemKind::Host, "a").unwrap(), None);
}

#[test]
fn turning_key_sync_on_refetches_from_the_start() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let area = Uuid::new_v4();
    store.put_area(s, area, "{}", "{}").unwrap();
    store.set_cursor(s, area, 40).unwrap();
    store.update_server_flags(s, true, false).unwrap();
    assert_eq!(store.areas(s).unwrap()[0].cursor, 40);
    store.update_server_flags(s, true, true).unwrap();
    assert_eq!(store.areas(s).unwrap()[0].cursor, 0);
}

#[test]
fn device_key_cannot_move_to_server() {
    let store = Store::open_in_memory().unwrap();
    let server = server(&store);
    let vault = crate::vault::Vault::unlock(&store, Box::new(crate::vault::MemoryMasterKey::default())).unwrap();
    let key = russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
    let info = vault.store_device_key(&store, "hello", &key).unwrap();
    let id = info.id.0.to_string();
    assert!(matches!(store.set_location(ItemKind::Key, &id, Some(server)), Err(StoreError::DeviceBound)));
    assert_eq!(store.location_of(ItemKind::Key, &id).unwrap(), None);
    // Back to "only on this device" is fine.
    store.set_location(ItemKind::Key, &id, None).unwrap();
}

fn key_record(kind: sshcraft_model::keytypes::KeyKind) -> KeyRecord {
    KeyRecord {
        id: KeyId::new(),
        name: "k".into(),
        algorithm: "ssh-ed25519".into(),
        fingerprint: "SHA256:x".into(),
        public_key: "ssh-ed25519 AAAA".into(),
        sealed_secret: vec![1],
        location: None,
        kind,
        protected: false,
        certificate: None,
    }
}

/// With "sync everything" on for a server, what is made here goes there: top-level groups and
/// hosts, identities, and keys when the server syncs keys. A host in a group still follows its
/// group, and a key bound to this device stays here.
#[test]
fn new_items_go_to_the_server_that_syncs_everything() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    store.set_sync_all(s, true).unwrap();
    let g = group("prod");
    store.upsert_group(&g).unwrap();
    let top = host("web", None);
    store.upsert_host(&top).unwrap();
    assert_eq!(store.location_of(ItemKind::Group, &g.id.0.to_string()).unwrap(), Some(s));
    assert_eq!(location_of_host(&store, top.id), Some(s));
    let key = key_record(sshcraft_model::keytypes::KeyKind::Vault);
    store.put_key_record(&key).unwrap();
    assert_eq!(store.location_of(ItemKind::Key, &key.id.0.to_string()).unwrap(), Some(s), "set_sync_all turns key sync on");
    let device = key_record(sshcraft_model::keytypes::KeyKind::Device);
    store.put_key_record(&device).unwrap();
    assert_eq!(store.location_of(ItemKind::Key, &device.id.0.to_string()).unwrap(), None, "a device key never leaves");
}

/// The counter-check: without "sync everything" new items stay on this device, as before.
#[test]
fn without_sync_everything_new_items_stay_here() {
    let store = Store::open_in_memory().unwrap();
    let _s = server(&store);
    let top = host("web", None);
    store.upsert_host(&top).unwrap();
    assert_eq!(location_of_host(&store, top.id), None);
}

/// One server at a time syncs everything; switching it off keeps items where they are, and a
/// server with sync switched off takes nothing.
#[test]
fn one_server_syncs_everything_at_a_time() {
    let store = Store::open_in_memory().unwrap();
    let a = server(&store);
    let b = server(&store);
    store.set_sync_all(a, true).unwrap();
    store.set_sync_all(b, true).unwrap();
    assert_eq!(store.everything_server().unwrap(), Some(b));
    assert!(!store.sync_all(a).unwrap());
    store.update_server_flags(b, false, true).unwrap();
    assert_eq!(store.everything_server().unwrap(), None, "a server with sync off takes nothing");
    let h = host("db", None);
    store.upsert_host(&h).unwrap();
    assert_eq!(location_of_host(&store, h.id), None);
}

/// What still lives only here when "sync everything" goes on: the top of each tree.
#[test]
fn local_roots_are_the_tops_of_what_is_here() {
    let store = Store::open_in_memory().unwrap();
    let g = group("prod");
    store.upsert_group(&g).unwrap();
    let inside = host("web", Some(g.id));
    store.upsert_host(&inside).unwrap();
    let top = host("db", None);
    store.upsert_host(&top).unwrap();
    store.put_key_record(&key_record(sshcraft_model::keytypes::KeyKind::Vault)).unwrap();
    store.put_key_record(&key_record(sshcraft_model::keytypes::KeyKind::Device)).unwrap();
    let roots = store.local_roots().unwrap();
    let kinds: Vec<ItemKind> = roots.iter().map(|(k, _)| *k).collect();
    assert_eq!(kinds.iter().filter(|k| **k == ItemKind::Group).count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == ItemKind::Host).count(), 1, "the grouped host goes with its group");
    assert_eq!(kinds.iter().filter(|k| **k == ItemKind::Key).count(), 1, "the device key stays");
    assert!(roots.contains(&(ItemKind::Host, top.id.0.to_string())));
}
use super::*;
use crate::model::{ConnectionSettings, Group, GroupId, Host, HostId, ServerId};
use crate::store::{ItemKind, RecordEntry, SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, ServerRow, Store};
use uuid::Uuid;

fn group() -> Group {
    Group {
        id: GroupId::new(),
        name: "g".into(),
        defaults: ConnectionSettings::default(),
        location: None,
        parent_id: None,
        tags: Vec::new(),
    }
}

fn host(group_id: Option<GroupId>) -> Host {
    Host {
        forwards: Vec::new(),
        id: HostId::new(),
        name: "h".into(),
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
            sealed_device: vec![1],
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

fn share(area: Uuid, owned: bool, role: Option<ShareRole>, kind: ShareKind) -> ShareRow {
    ShareRow {
        area_id: area,
        kind,
        group_id: None,
        with_keys: false,
        owned,
        role,
        owner: [9; 32],
    }
}

#[test]
fn migration_from_v3_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v3.db");
    let h = host(None);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(SCHEMA_V2).unwrap();
        conn.execute_batch(SCHEMA_V3).unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();
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
    assert_eq!(store.list_hosts().unwrap(), vec![h]);
    let s = server(&store);
    assert!(store.shares(s).unwrap().is_empty());
}

#[test]
fn host_follows_its_groups_area() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let g = group();
    store.upsert_group(&g).unwrap();
    let h = host(Some(g.id));
    store.upsert_host(&h).unwrap();
    let personal = Uuid::new_v4();
    let shared = Uuid::new_v4();
    let hid = h.id.0.to_string();
    assert_eq!(
        store.area_for(s, ItemKind::Host, &hid, personal).unwrap(),
        personal
    );
    store
        .set_item_area(s, ItemKind::Group, &g.id.0.to_string(), shared)
        .unwrap();
    assert_eq!(
        store.area_for(s, ItemKind::Host, &hid, personal).unwrap(),
        shared
    );
}

#[test]
fn explicit_area_wins_over_the_group() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let g = group();
    store.upsert_group(&g).unwrap();
    let h = host(Some(g.id));
    store.upsert_host(&h).unwrap();
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let hid = h.id.0.to_string();
    store
        .set_item_area(s, ItemKind::Group, &g.id.0.to_string(), a)
        .unwrap();
    store.set_item_area(s, ItemKind::Host, &hid, b).unwrap();
    assert_eq!(
        store
            .area_for(s, ItemKind::Host, &hid, Uuid::nil())
            .unwrap(),
        b
    );
    store.clear_item_area(s, ItemKind::Host, &hid).unwrap();
    assert_eq!(
        store
            .area_for(s, ItemKind::Host, &hid, Uuid::nil())
            .unwrap(),
        a
    );
}

#[test]
fn contact_change_is_detected() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let first = Contact {
        email: "bob@example.org".into(),
        account: [1; 32],
        name: "Bob".into(),
        first_seen: 5,
        confirmed: false,
    };
    store.put_contact(s, &first).unwrap();
    store
        .confirm_contact(s, "bob@example.org", &[1; 32])
        .unwrap();
    assert!(
        store
            .contact(s, "bob@example.org")
            .unwrap()
            .unwrap()
            .confirmed
    );
    assert_eq!(
        store
            .contact_by_account(s, &[1; 32])
            .unwrap()
            .unwrap()
            .email,
        "bob@example.org"
    );
    let changed = Contact {
        account: [2; 32],
        first_seen: 9,
        ..first.clone()
    };
    store.put_contact(s, &changed).unwrap();
    let now = store.contact(s, "bob@example.org").unwrap().unwrap();
    assert_eq!(now.account, [2; 32]);
    assert!(!now.confirmed);
    assert_eq!(now.first_seen, 5, "first contact stays");
}

#[test]
fn copies_map_both_ways() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let area = Uuid::new_v4();
    store
        .put_share(s, &share(area, true, None, ShareKind::Selection))
        .unwrap();
    store
        .put_copy(s, area, ItemKind::Host, "orig", "copy")
        .unwrap();
    assert_eq!(
        store.copies_of(s, ItemKind::Host, "orig").unwrap(),
        vec![(area, "copy".to_owned())]
    );
    assert_eq!(
        store.original_of(s, ItemKind::Host, "copy").unwrap(),
        Some("orig".to_owned())
    );
    assert_eq!(store.copies_in(s, area).unwrap().len(), 1);
    store.remove_share(s, area).unwrap();
    assert!(
        store
            .copies_of(s, ItemKind::Host, "orig")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn received_read_share_is_read_only() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let g = group();
    store.upsert_group(&g).unwrap();
    let h = host(Some(g.id));
    store.upsert_host(&h).unwrap();
    let area = Uuid::new_v4();
    store
        .put_share(
            s,
            &share(area, false, Some(ShareRole::Read), ShareKind::GroupAll),
        )
        .unwrap();
    store
        .set_item_area(s, ItemKind::Group, &g.id.0.to_string(), area)
        .unwrap();
    let hid = h.id.0.to_string();
    assert!(store.read_only(s, ItemKind::Host, &hid).unwrap());
    assert_eq!(
        store
            .received_share_of(s, ItemKind::Host, &hid)
            .unwrap()
            .unwrap()
            .area_id,
        area
    );
    store
        .put_share(
            s,
            &share(area, false, Some(ShareRole::Write), ShareKind::GroupAll),
        )
        .unwrap();
    assert!(!store.read_only(s, ItemKind::Host, &hid).unwrap());
    let own = Uuid::new_v4();
    store
        .put_share(s, &share(own, true, None, ShareKind::Host))
        .unwrap();
    store.set_item_area(s, ItemKind::Host, &hid, own).unwrap();
    assert!(
        store
            .received_share_of(s, ItemKind::Host, &hid)
            .unwrap()
            .is_none()
    );
}

#[test]
fn removing_a_server_removes_its_shares_and_contacts() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let area = Uuid::new_v4();
    store
        .put_share(s, &share(area, true, None, ShareKind::Host))
        .unwrap();
    store.put_copy(s, area, ItemKind::Host, "o", "c").unwrap();
    store.set_item_area(s, ItemKind::Host, "o", area).unwrap();
    store
        .put_contact(
            s,
            &Contact {
                email: "x@example.org".into(),
                account: [3; 32],
                name: "X".into(),
                first_seen: 1,
                confirmed: true,
            },
        )
        .unwrap();
    store.remove_server(s).unwrap();
    let s2 = server(&store);
    let _ = s2;
    assert!(store.shares(s).unwrap().is_empty());
    assert!(store.contact(s, "x@example.org").unwrap().is_none());
    assert!(store.item_area(s, ItemKind::Host, "o").unwrap().is_none());
    assert!(store.original_of(s, ItemKind::Host, "c").unwrap().is_none());
}

#[test]
fn record_entry_moves_with_the_item() {
    let store = Store::open_in_memory().unwrap();
    let s = server(&store);
    let entry = |area| RecordEntry {
        record_id: Uuid::new_v4(),
        area_id: area,
        version: 1,
        modified_at: 1,
    };
    let old = entry(Uuid::new_v4());
    store
        .put_record_entry(s, &old, ItemKind::Host, "h")
        .unwrap();
    let new = entry(Uuid::new_v4());
    store
        .put_record_entry(s, &new, ItemKind::Host, "h")
        .unwrap();
    assert_eq!(
        store.record_entry(s, ItemKind::Host, "h").unwrap(),
        Some(new)
    );
    assert!(store.item_for_record(s, old.record_id).unwrap().is_none());
}

#[test]
fn copy_ids_are_stable_and_distinct() {
    let area = Uuid::new_v4();
    let original = Uuid::new_v4();
    assert_eq!(copy_id(area, original), copy_id(area, original));
    assert_ne!(copy_id(area, original), copy_id(Uuid::new_v4(), original));
    assert_ne!(copy_id(area, original), original);
}

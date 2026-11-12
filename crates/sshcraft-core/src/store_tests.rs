use super::*;
use crate::model::{ConnectionSettings, Group, GroupId, Host, HostId};

fn group(name: &str) -> Group {
    Group {
        id: GroupId::new(),
        name: name.into(),
        defaults: ConnectionSettings {
            address: Some("example.org".into()),
            ..Default::default()
        },
    }
}

fn host(name: &str, group_id: Option<GroupId>) -> Host {
    Host {
        id: HostId::new(),
        name: name.into(),
        group_id,
        settings: ConnectionSettings::default(),
    }
}

#[test]
fn group_and_host_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    let g = group("srv");
    let h = host("root", Some(g.id));
    store.upsert_group(&g).unwrap();
    store.upsert_host(&h).unwrap();
    assert_eq!(store.list_groups().unwrap(), vec![g]);
    assert_eq!(store.list_hosts().unwrap(), vec![h]);
}

#[test]
fn upsert_updates_existing() {
    let store = Store::open_in_memory().unwrap();
    let mut g = group("old");
    store.upsert_group(&g).unwrap();
    g.name = "new".into();
    store.upsert_group(&g).unwrap();
    let groups = store.list_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, "new");
}

#[test]
fn delete_group_removes_its_hosts() {
    let store = Store::open_in_memory().unwrap();
    let g = group("srv");
    store.upsert_group(&g).unwrap();
    store.upsert_host(&host("in", Some(g.id))).unwrap();
    let loose = host("loose", None);
    store.upsert_host(&loose).unwrap();
    store.delete_group(g.id).unwrap();
    assert!(store.list_groups().unwrap().is_empty());
    assert_eq!(store.list_hosts().unwrap(), vec![loose]);
}

#[test]
fn known_host_set_get_forget() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.known_host("a", 22).unwrap(), None);
    store.set_known_host("a", 22, "SHA256:abc").unwrap();
    assert_eq!(
        store.known_host("a", 22).unwrap().as_deref(),
        Some("SHA256:abc")
    );
    assert_eq!(store.known_host("a", 2222).unwrap(), None);
    store.forget_known_host("a", 22).unwrap();
    assert_eq!(store.known_host("a", 22).unwrap(), None);
}

#[test]
fn key_records_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    assert!(!store.has_key_records().unwrap());
    let rec = KeyRecord {
        id: KeyId::new(),
        name: "k".into(),
        algorithm: "ssh-ed25519".into(),
        fingerprint: "SHA256:x".into(),
        public_key: "ssh-ed25519 AAAA".into(),
        sealed_secret: vec![1, 2, 3],
    };
    store.put_key_record(&rec).unwrap();
    assert!(store.has_key_records().unwrap());
    assert_eq!(store.key_record(rec.id).unwrap(), Some(rec.clone()));
    assert_eq!(store.list_key_records().unwrap(), vec![rec.clone()]);
    store.delete_key_record(rec.id).unwrap();
    assert_eq!(store.key_record(rec.id).unwrap(), None);
}

#[test]
fn data_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sshcraft.db");
    let g = group("srv");
    {
        let store = Store::open(&path).unwrap();
        store.upsert_group(&g).unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.list_groups().unwrap(), vec![g]);
}

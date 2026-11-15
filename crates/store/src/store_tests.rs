use super::*;
use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId};

fn group(name: &str) -> Group {
    Group {
        location: None,
        id: GroupId::new(),
        name: name.into(),
        defaults: ConnectionSettings { address: Some("example.org".into()), ..Default::default() },
        parent_id: None,
        tags: Vec::new(),
    }
}

fn host(name: &str, group_id: Option<GroupId>) -> Host {
    Host {
        forwards: Vec::new(),
        location: None,
        id: HostId::new(),
        name: name.into(),
        group_id,
        settings: ConnectionSettings::default(),
        tags: Vec::new(),
        os: None,
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
    assert_eq!(store.known_host("a", 22).unwrap().as_deref(), Some("SHA256:abc"));
    assert_eq!(store.known_host("a", 2222).unwrap(), None);
    store.forget_known_host("a", 22).unwrap();
    assert_eq!(store.known_host("a", 22).unwrap(), None);
}

#[test]
fn key_records_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    assert!(!store.has_key_records().unwrap());
    let rec = KeyRecord {
        location: None,
        id: KeyId::new(),
        name: "k".into(),
        algorithm: "ssh-ed25519".into(),
        fingerprint: "SHA256:x".into(),
        public_key: "ssh-ed25519 AAAA".into(),
        sealed_secret: vec![1, 2, 3],
        kind: Default::default(),
        protected: false,
        certificate: None,
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

#[test]
fn upgrade_from_v1_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    let g = group("srv");
    let h = host("root", Some(g.id));
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO groups (id, name, defaults) VALUES (?1, ?2, ?3)",
            rusqlite::params![g.id.0.to_string(), g.name, serde_json::to_string(&g.defaults).unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO hosts (id, name, group_id, settings) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![h.id.0.to_string(), h.name, g.id.0.to_string(), serde_json::to_string(&h.settings).unwrap()],
        )
        .unwrap();
        conn.execute("INSERT INTO known_hosts (address, port, fingerprint) VALUES ('example.org', 22, 'SHA256:x')", []).unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.list_groups().unwrap(), vec![g]);
    assert_eq!(store.list_hosts().unwrap(), vec![h.clone()]);
    assert_eq!(store.known_host("example.org", 22).unwrap().as_deref(), Some("SHA256:x"));
    let version: i64 = store.conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    store.put_setting("app", "{}").unwrap();
    store.touch_recent(h.id, 1).unwrap();
    drop(store);
    // Opening again must not run any migration a second time.
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get_setting("app").unwrap().as_deref(), Some("{}"));
    assert_eq!(store.recent_hosts(10).unwrap(), vec![h.id]);
}

#[test]
fn settings_roundtrip_and_overwrite() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.get_setting("app").unwrap(), None);
    store.put_setting("app", "a").unwrap();
    store.put_setting("app", "b").unwrap();
    assert_eq!(store.get_setting("app").unwrap().as_deref(), Some("b"));
}

#[test]
fn recent_hosts_newest_first_and_limited() {
    let store = Store::open_in_memory().unwrap();
    let hosts: Vec<Host> = (0..4).map(|i| host(&format!("h{i}"), None)).collect();
    for (i, h) in hosts.iter().enumerate() {
        store.upsert_host(h).unwrap();
        store.touch_recent(h.id, i as i64 * 10).unwrap();
    }
    assert_eq!(store.recent_hosts(3).unwrap(), vec![hosts[3].id, hosts[2].id, hosts[1].id]);
}

#[test]
fn touch_recent_twice_keeps_one_row() {
    let store = Store::open_in_memory().unwrap();
    let (a, b) = (host("a", None), host("b", None));
    store.upsert_host(&a).unwrap();
    store.upsert_host(&b).unwrap();
    store.touch_recent(a.id, 1).unwrap();
    store.touch_recent(b.id, 2).unwrap();
    store.touch_recent(a.id, 3).unwrap();
    assert_eq!(store.recent_hosts(10).unwrap(), vec![a.id, b.id]);
}

#[test]
fn deleting_host_removes_recent() {
    let store = Store::open_in_memory().unwrap();
    let h = host("a", None);
    store.upsert_host(&h).unwrap();
    store.touch_recent(h.id, 1).unwrap();
    store.delete_host(h.id).unwrap();
    assert!(store.recent_hosts(10).unwrap().is_empty());
}

#[test]
fn half_done_upgrade_to_v2_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("half.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        // The process died after the first table of version 2 was created.
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);").unwrap();
    }
    let store = Store::open(&path).unwrap();
    store.put_setting("app", "{}").unwrap();
    assert!(store.recent_hosts(1).unwrap().is_empty());
}

#[test]
fn schema_7_adds_key_columns_with_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v6.db");
    let id = KeyId::new();
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for batch in [SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, super::share_store::SCHEMA_V4, SCHEMA_V5, super::credential_store::SCHEMA_V6] {
            conn.execute_batch(batch).unwrap();
        }
        conn.pragma_update(None, "user_version", 6).unwrap();
        conn.execute(
            "INSERT INTO keys (id, name, algorithm, fingerprint, public_key, sealed_secret)
             VALUES (?1, 'k', 'ssh-ed25519', 'SHA256:x', 'ssh-ed25519 AAAA', x'01')",
            [id.0.to_string()],
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let record = store.key_record(id).unwrap().unwrap();
    assert_eq!(record.kind, sshcraft_model::keytypes::KeyKind::Vault);
    assert!(!record.protected);
    assert_eq!(record.certificate, None);
}

fn local_rule(port: u16) -> sshcraft_model::forward::ForwardRule {
    sshcraft_model::forward::ForwardRule {
        id: sshcraft_model::forward::ForwardId::new(),
        name: String::new(),
        kind: sshcraft_model::forward::ForwardKind::Local,
        bind_address: "127.0.0.1".into(),
        bind_port: port,
        target_host: "db".into(),
        target_port: 5432,
    }
}

#[test]
fn host_forwards_round_trip() {
    let store = Store::open_in_memory().unwrap();
    let mut h = host("web", None);
    h.forwards = vec![local_rule(15432), local_rule(15433)];
    store.upsert_host(&h).unwrap();
    assert_eq!(store.list_hosts().unwrap()[0].forwards, h.forwards);
}

#[test]
fn unreadable_forwards_column_gives_none() {
    let store = Store::open_in_memory().unwrap();
    let h = host("web", None);
    store.upsert_host(&h).unwrap();
    store.conn.execute("UPDATE hosts SET forwards = 'garbage'", []).unwrap();
    assert!(store.list_hosts().unwrap()[0].forwards.is_empty());
}

#[test]
fn invalid_forward_from_elsewhere_is_dropped_on_load() {
    let store = Store::open_in_memory().unwrap();
    let h = host("web", None);
    store.upsert_host(&h).unwrap();
    let good = local_rule(15432);
    let mut bad = serde_json::to_value(local_rule(1)).unwrap();
    bad["bind_port"] = serde_json::json!(70000);
    let json = serde_json::to_string(&vec![serde_json::to_value(&good).unwrap(), bad]).unwrap();
    store.conn.execute("UPDATE hosts SET forwards = ?1", [json]).unwrap();
    assert_eq!(store.list_hosts().unwrap()[0].forwards, vec![good]);
}

#[test]
fn schema_8_adds_forwards_to_existing_hosts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v7.db");
    let id = HostId::new();
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for batch in [SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, super::share_store::SCHEMA_V4, SCHEMA_V5, super::credential_store::SCHEMA_V6, SCHEMA_V7] {
            conn.execute_batch(batch).unwrap();
        }
        conn.pragma_update(None, "user_version", 7).unwrap();
        conn.execute("INSERT INTO hosts (id, name, settings) VALUES (?1, 'web', '{}')", [id.0.to_string()]).unwrap();
    }
    let store = Store::open(&path).unwrap();
    let hosts = store.list_hosts().unwrap();
    assert_eq!(hosts[0].id, id);
    assert!(hosts[0].forwards.is_empty());
}

#[test]
fn saving_an_invalid_rule_is_refused() {
    let store = Store::open_in_memory().unwrap();
    let mut h = host("web", None);
    h.forwards = vec![local_rule(0)];
    assert!(matches!(store.upsert_host(&h), Err(StoreError::InvalidForward(_))));
    assert!(store.list_hosts().unwrap().is_empty());
}

/// A database the Tauri version left at schema 11 opens with one migration to 12: its known host
/// fingerprints move to `host_keys`, without the key itself, and nothing else changes.
#[test]
fn schema_11_migrates_known_hosts_to_host_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sshcraft.db");
    let store = Store::open(&path).unwrap();
    store.put_setting("app", "{}").unwrap();
    // Back to schema 11 as the Tauri version and R1 to R7 left it.
    store
        .conn
        .execute_batch(
            "DROP TABLE trash; DROP TABLE versions;
             DROP TABLE approved_commands; DROP TABLE ssh_config_links; DROP TABLE host_keys;
             CREATE TABLE known_hosts (address TEXT NOT NULL, port INTEGER NOT NULL,
                 fingerprint TEXT NOT NULL, PRIMARY KEY (address, port));
             INSERT INTO known_hosts VALUES ('a.org', 22, 'SHA256:a'), ('b.org', 2222, 'SHA256:b');
             PRAGMA user_version = 11;",
        )
        .unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    let version: i64 = store.conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(version, 13);
    let keys = store.all_host_keys().unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!((keys[0].address.as_str(), keys[0].port, keys[0].fingerprint.as_str()), ("a.org", 22, "SHA256:a"));
    assert_eq!((keys[1].address.as_str(), keys[1].port, keys[1].fingerprint.as_str()), ("b.org", 2222, "SHA256:b"));
    assert!(keys.iter().all(|k| k.public_key.is_none() && k.algorithm.is_none()));
    assert_eq!(store.get_setting("app").unwrap().as_deref(), Some("{}"));
    let tables: i64 =
        store.conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'known_hosts'", [], |r| r.get(0)).unwrap();
    assert_eq!(tables, 0);
}

fn key(address: &str, port: u16, fingerprint: &str) -> HostKey {
    HostKey {
        address: address.into(),
        port,
        algorithm: Some("ssh-ed25519".into()),
        public_key: Some(format!("AAAA{fingerprint}")),
        fingerprint: fingerprint.into(),
        added_at: 5,
    }
}

#[test]
fn several_keys_per_address() {
    let store = Store::open_in_memory().unwrap();
    store.add_host_key(&key("a", 22, "SHA256:1")).unwrap();
    store.add_host_key(&key("a", 22, "SHA256:2")).unwrap();
    store.add_host_key(&key("a", 2222, "SHA256:3")).unwrap();
    let fps: Vec<String> = store.host_keys("a", 22).unwrap().into_iter().map(|k| k.fingerprint).collect();
    assert_eq!(fps, vec!["SHA256:1", "SHA256:2"]);
    assert!(store.remove_host_key("a", 22, "SHA256:1").unwrap());
    assert!(!store.remove_host_key("a", 22, "SHA256:1").unwrap());
    assert_eq!(store.host_keys("a", 22).unwrap().len(), 1);
    store.forget_host_keys("a", 22).unwrap();
    assert!(store.host_keys("a", 22).unwrap().is_empty());
    assert_eq!(store.host_keys("a", 2222).unwrap().len(), 1);
}

/// A fingerprint known without its key (from schema 11 or a fingerprint-only import) gets the
/// key once it is seen; a key already stored is not replaced by a fingerprint-only write.
#[test]
fn a_known_fingerprint_gains_its_key() {
    let store = Store::open_in_memory().unwrap();
    store.set_known_host("a", 22, "SHA256:1").unwrap();
    store.add_host_key(&key("a", 22, "SHA256:1")).unwrap();
    let k = store.host_keys("a", 22).unwrap();
    assert_eq!(k.len(), 1);
    assert_eq!(k[0].public_key.as_deref(), Some("AAAASHA256:1"));
    store.set_known_host("a", 22, "SHA256:1").unwrap();
    assert_eq!(store.host_keys("a", 22).unwrap()[0].public_key.as_deref(), Some("AAAASHA256:1"));
}

#[test]
fn deleting_a_host_drops_its_config_link() {
    let store = Store::open_in_memory().unwrap();
    let h = host("web", None);
    store.upsert_host(&h).unwrap();
    store.set_config_link(&ConfigLink { host_id: h.id, alias: "web".into(), file: "/x/config".into(), digest: "c1".into() }).unwrap();
    assert_eq!(store.config_links().unwrap().len(), 1);
    store.delete_host(h.id).unwrap();
    assert!(store.config_links().unwrap().is_empty());
}

#[test]
fn approved_commands_are_kept_by_digest() {
    let store = Store::open_in_memory().unwrap();
    let a = command_digest(&["ssh".into(), "-W".into(), "%h:%p".into()]);
    let b = command_digest(&["ssh -W".into(), "%h:%p".into()]);
    assert_ne!(a, b, "the same text split differently is another command");
    store.approve_command(&a).unwrap();
    store.approve_command(&a).unwrap();
    assert!(store.command_approved(&a).unwrap());
    assert!(!store.command_approved(&b).unwrap());
}

use super::*;
use crate::store::ItemKind;
use crate::vault::{MemoryMasterKey, Vault, VaultError};
use sshcraft_model::model::{
    ConnectionSettings, EnvVar, Group, GroupId, Host, HostId, Identity, IdentityId, PasswordId, Proxy, ProxyEndpoint, ServerId,
};

const SECRET: &str = "hunter2-sshcraft";

fn vault(store: &Store) -> Vault {
    Vault::unlock(store, Box::new(MemoryMasterKey::default())).unwrap()
}

fn server(store: &Store) -> ServerId {
    let id = ServerId::new();
    store
        .conn
        .execute(
            "INSERT INTO sync_servers (id, name, origin, account, device_id, sealed_device,
                 sync_enabled, sync_keys, created_at)
             VALUES (?1, 's', 'https://s', x'00', 'd', x'00', 1, 1, 0)",
            [id.0.to_string()],
        )
        .unwrap();
    id
}

fn host(settings: ConnectionSettings, group: Option<GroupId>) -> Host {
    Host { forwards: Vec::new(), id: HostId::new(), name: "h".into(), group_id: group, settings, location: None, tags: Vec::new(), os: None }
}

fn group(parent: Option<GroupId>) -> Group {
    Group { id: GroupId::new(), name: "g".into(), defaults: ConnectionSettings::default(), location: None, parent_id: parent, tags: Vec::new() }
}

fn identity(password: Option<PasswordId>) -> Identity {
    Identity { id: IdentityId::new(), name: "ops".into(), username: Some("deploy".into()), key: None, password, location: None }
}

fn password_location(store: &Store, id: PasswordId) -> Option<ServerId> {
    store.password_record(id).unwrap().unwrap().1
}

#[test]
fn migration_from_v5_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v5.db");
    let id = GroupId::new();
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(SCHEMA_V2).unwrap();
        conn.execute_batch(SCHEMA_V3).unwrap();
        conn.execute_batch(super::share_store::SCHEMA_V4).unwrap();
        conn.execute_batch(SCHEMA_V5).unwrap();
        conn.pragma_update(None, "user_version", 5).unwrap();
        conn.execute("INSERT INTO groups (id, name, defaults) VALUES (?1, 'old', '{}')", [id.0.to_string()]).unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.list_groups().unwrap().len(), 1);
    assert!(store.list_identities().unwrap().is_empty());
    assert!(store.list_password_ids().unwrap().is_empty());
}

#[test]
fn identity_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    let mut i = identity(Some(PasswordId::new()));
    store.upsert_identity(&i).unwrap();
    assert_eq!(store.list_identities().unwrap(), vec![i.clone()]);
    i.username = Some("other".into());
    store.upsert_identity(&i).unwrap();
    assert_eq!(store.list_identities().unwrap(), vec![i.clone()]);
    store.delete_identity(i.id).unwrap();
    assert!(store.list_identities().unwrap().is_empty());
}

#[test]
fn password_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let id = PasswordId::new();
    v.store_password(&store, id, SECRET, None).unwrap();
    assert_eq!(v.password(&store, id).unwrap().as_str(), SECRET);
    assert_eq!(store.list_password_ids().unwrap(), vec![id]);
    v.store_password(&store, id, "changed", None).unwrap();
    assert_eq!(v.password(&store, id).unwrap().as_str(), "changed");
    store.delete_password(id).unwrap();
    assert!(matches!(v.password(&store, id), Err(VaultError::NotFound)));
}

#[test]
fn password_is_sealed_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.db");
    {
        let store = Store::open(&path).unwrap();
        let v = vault(&store);
        v.store_password(&store, PasswordId::new(), SECRET, None).unwrap();
    }
    let bytes = std::fs::read(&path).unwrap();
    assert!(!bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()));
}

#[test]
fn password_aad_binds_id() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let a = PasswordId::new();
    v.store_password(&store, a, SECRET, None).unwrap();
    let (sealed, _) = store.password_record(a).unwrap().unwrap();
    let b = PasswordId::new();
    store.put_password_record(b, &sealed, None).unwrap();
    assert!(matches!(v.password(&store, b), Err(VaultError::Corrupt)));
}

#[test]
fn password_cannot_be_opened_as_key() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let id = PasswordId::new();
    v.store_password(&store, id, SECRET, None).unwrap();
    let (sealed, _) = store.password_record(id).unwrap().unwrap();
    store
        .put_key_record(&KeyRecord {
            id: sshcraft_model::model::KeyId(id.0),
            name: "k".into(),
            algorithm: "ssh-ed25519".into(),
            fingerprint: "SHA256:x".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            sealed_secret: sealed,
            location: None,
            kind: Default::default(),
            protected: false,
            certificate: None,
        })
        .unwrap();
    assert!(v.private_key(&store, sshcraft_model::model::KeyId(id.0)).is_err());
    assert!(v.open_secret("password", &store.password_record(id).unwrap().unwrap().0).is_err());
}

#[test]
fn moving_a_host_moves_its_password() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let s = server(&store);
    let p = PasswordId::new();
    v.store_password(&store, p, SECRET, None).unwrap();
    let h = host(ConnectionSettings { password: Some(p), ..Default::default() }, None);
    store.upsert_host(&h).unwrap();
    store.set_location(ItemKind::Host, &h.id.0.to_string(), Some(s)).unwrap();
    assert_eq!(password_location(&store, p), Some(s));
    store.set_location(ItemKind::Host, &h.id.0.to_string(), None).unwrap();
    assert_eq!(password_location(&store, p), None);
}

#[test]
fn saving_a_host_at_a_server_moves_its_password_there() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let s = server(&store);
    let p = PasswordId::new();
    v.store_password(&store, p, SECRET, None).unwrap();
    let mut h = host(ConnectionSettings { password: Some(p), ..Default::default() }, None);
    h.location = Some(s);
    store.upsert_host(&h).unwrap();
    assert_eq!(password_location(&store, p), Some(s));
}

#[test]
fn moving_a_group_moves_passwords_below_it() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let s = server(&store);
    let (own, proxy) = (PasswordId::new(), PasswordId::new());
    for p in [own, proxy] {
        v.store_password(&store, p, SECRET, None).unwrap();
    }
    let top = group(None);
    let mut sub = group(Some(top.id));
    sub.defaults.password = Some(own);
    store.upsert_group(&top).unwrap();
    store.upsert_group(&sub).unwrap();
    let h = host(
        ConnectionSettings {
            proxy: Some(Proxy::Socks5(ProxyEndpoint { address: "p".into(), port: 1080, username: Some("u".into()), password: Some(proxy) })),
            ..Default::default()
        },
        Some(sub.id),
    );
    store.upsert_host(&h).unwrap();
    store.set_location(ItemKind::Group, &top.id.0.to_string(), Some(s)).unwrap();
    assert_eq!(password_location(&store, own), Some(s));
    assert_eq!(password_location(&store, proxy), Some(s));
}

#[test]
fn moving_an_identity_moves_its_password() {
    let store = Store::open_in_memory().unwrap();
    let v = vault(&store);
    let s = server(&store);
    let p = PasswordId::new();
    v.store_password(&store, p, SECRET, None).unwrap();
    let i = identity(Some(p));
    store.upsert_identity(&i).unwrap();
    store.set_location(ItemKind::Identity, &i.id.0.to_string(), Some(s)).unwrap();
    assert_eq!(store.location_of(ItemKind::Identity, &i.id.0.to_string()).unwrap(), Some(s));
    assert_eq!(password_location(&store, p), Some(s));
}

#[test]
fn env_is_normalized_on_save() {
    let store = Store::open_in_memory().unwrap();
    let h = host(
        ConnectionSettings {
            env: vec![EnvVar { name: "bad-name".into(), value: "1".into() }, EnvVar { name: "LANG".into(), value: "C".into() }],
            ..Default::default()
        },
        None,
    );
    store.upsert_host(&h).unwrap();
    let mut g = group(None);
    g.defaults.env = h.settings.env.clone();
    store.upsert_group(&g).unwrap();
    let names = |env: &[EnvVar]| env.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&store.list_hosts().unwrap()[0].settings.env), vec!["LANG"]);
    assert_eq!(names(&store.list_groups().unwrap()[0].defaults.env), vec!["LANG"]);
}

#[test]
fn item_kinds_parse() {
    for kind in [ItemKind::Identity, ItemKind::Password] {
        assert_eq!(ItemKind::parse(kind.as_str()).unwrap(), kind);
    }
}

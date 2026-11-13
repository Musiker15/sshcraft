use sshcraft_core::model::{ConnectionSettings, Host, HostId, KeyId, KeyRef, ServerId};
use sshcraft_core::store::{ItemKind, Store};
use sshcraft_core::vault::{MemoryMasterKey, Vault};
use sshcraft_sync::SyncError;
use sshcraft_sync::items::{ItemBody, SyncItem, apply, decode, encode, local_item, newer};
use zeroize::Zeroizing;

fn setup() -> (Store, Vault) {
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    (store, vault)
}

fn host(key: Option<KeyRef>) -> Host {
    Host {
        id: HostId::new(),
        name: "web".into(),
        group_id: None,
        settings: ConnectionSettings {
            address: Some("10.0.0.1".into()),
            port: None,
            username: Some("root".into()),
            key,
            ..Default::default()
        },
        location: None,
        tags: Vec::new(),
        os: None,
    }
}

#[test]
fn host_round_trips_without_location() {
    let (store, vault) = setup();
    let mut h = host(None);
    h.location = Some(ServerId::new());
    store.upsert_host(&h).unwrap();
    let item = local_item(&store, &vault, ItemKind::Host, &h.id.0.to_string(), 42).unwrap();
    let bytes = encode(&item).unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!text.contains("location"), "{text}");
    let back = decode(&bytes).unwrap();
    assert_eq!(back.modified_at, 42);
    match back.body {
        ItemBody::Host { host } => assert_eq!((host.name.as_str(), host.location), ("web", None)),
        _ => panic!("expected a host"),
    }
}

#[test]
fn key_body_carries_the_private_key_and_is_zeroized() {
    let (store, vault) = setup();
    let info = vault.generate_ed25519(&store, "deploy").unwrap();
    let item = local_item(&store, &vault, ItemKind::Key, &info.id.0.to_string(), 1).unwrap();
    let ItemBody::Key {
        id, name, openssh, ..
    } = &item.body
    else {
        panic!("expected a key");
    };
    let _: &Zeroizing<String> = openssh;
    assert_eq!((*id, name.as_str()), (info.id, "deploy"));
    assert!(openssh.contains("OPENSSH PRIVATE KEY"));

    // Applying it on another device stores the same key under the same id there.
    let (other, other_vault) = setup();
    let server = ServerId::new();
    apply(&other, &other_vault, server, &item).unwrap();
    let record = other.key_record(info.id).unwrap().unwrap();
    assert_eq!(
        (record.location, record.fingerprint.clone()),
        (Some(server), info.fingerprint)
    );
}

#[test]
fn apply_deleted_removes_the_item() {
    let (store, vault) = setup();
    let h = host(None);
    store.upsert_host(&h).unwrap();
    let item = SyncItem {
        modified_at: 5,
        body: ItemBody::Deleted {
            item_kind: ItemKind::Host,
            item_id: h.id.0.to_string(),
        },
    };
    apply(&store, &vault, ServerId::new(), &item).unwrap();
    assert!(store.list_hosts().unwrap().is_empty());
}

#[test]
fn missing_item_becomes_deleted() {
    let (store, vault) = setup();
    let id = HostId::new().0.to_string();
    let item = local_item(&store, &vault, ItemKind::Host, &id, 9).unwrap();
    assert!(matches!(
        item.body,
        ItemBody::Deleted { item_kind: ItemKind::Host, ref item_id } if *item_id == id
    ));
}

#[test]
fn apply_host_with_unknown_key_keeps_the_reference() {
    let (store, vault) = setup();
    let unknown = KeyId::new();
    let h = host(Some(KeyRef::Stored { id: unknown }));
    let server = ServerId::new();
    apply(
        &store,
        &vault,
        server,
        &SyncItem {
            modified_at: 1,
            body: ItemBody::Host { host: h.clone() },
        },
    )
    .unwrap();
    let stored = store.list_hosts().unwrap().pop().unwrap();
    assert_eq!(stored.settings.key, Some(KeyRef::Stored { id: unknown }));
    assert_eq!(stored.location, Some(server));
}

#[test]
fn newer_breaks_ties_for_the_server() {
    assert!(newer(2000, 1000));
    assert!(!newer(1000, 2000));
    assert!(!newer(1000, 1000));
}

#[test]
fn decode_rejects_garbage() {
    assert!(matches!(decode(b"not json"), Err(SyncError::Proto(_))));
    assert!(matches!(
        decode(br#"{"modified_at":1,"body":{"kind":"spaceship"}}"#),
        Err(SyncError::Proto(_))
    ));
}

#[test]
fn remote_deletion_never_removes_a_device_key() {
    let store = sshcraft_core::store::Store::open_in_memory().unwrap();
    let vault = sshcraft_core::vault::Vault::unlock(
        &store,
        Box::new(sshcraft_core::vault::MemoryMasterKey::default()),
    )
    .unwrap();
    let key =
        russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
    let info = vault.store_device_key(&store, "hello", &key).unwrap();
    let item = sshcraft_sync::items::SyncItem {
        body: sshcraft_sync::items::ItemBody::Deleted {
            item_kind: sshcraft_core::store::ItemKind::Key,
            item_id: info.id.0.to_string(),
        },
        modified_at: 1,
    };
    sshcraft_sync::items::apply(&store, &vault, sshcraft_core::model::ServerId::new(), &item)
        .unwrap();
    assert!(store.key_record(info.id).unwrap().is_some());
}

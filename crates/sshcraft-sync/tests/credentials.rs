mod common;

use common::{Dev, TestServer, create, join, sample_host, sync};
use sshcraft_core::model::{
    ConnectionSettings, Directory, Host, Identity, IdentityId, PasswordId, ServerId, resolve_route,
};
use sshcraft_core::store::ItemKind;
use sshcraft_sync::engine::record_local_change;

const SECRET: &str = "hunter2-sshcraft";

fn server_id(dev: &Dev) -> ServerId {
    dev.row().id
}

fn save_password(dev: &Dev, id: PasswordId, location: Option<ServerId>) {
    let store = dev.store.lock().unwrap();
    dev.vault
        .store_password(&store, id, SECRET, location)
        .unwrap();
    record_local_change(&store, ItemKind::Password, &id.0.to_string(), 100).unwrap();
}

fn save_identity(dev: &Dev, identity: &Identity) {
    let server = server_id(dev);
    let store = dev.store.lock().unwrap();
    let mut identity = identity.clone();
    identity.location = Some(server);
    store.upsert_identity(&identity).unwrap();
    record_local_change(&store, ItemKind::Identity, &identity.id.0.to_string(), 100).unwrap();
}

fn save_host(dev: &Dev, host: &Host) {
    let server = server_id(dev);
    let store = dev.store.lock().unwrap();
    let mut host = host.clone();
    host.location = Some(server);
    store.upsert_host(&host).unwrap();
    record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 100).unwrap();
}

async fn pair(server: &TestServer) -> (Dev, Dev) {
    let (a, b) = (Dev::new(), Dev::new());
    let export = create(server, &a, "a@example.org").await;
    join(server, &b, &export).await;
    for dev in [&a, &b] {
        let id = server_id(dev);
        dev.store
            .lock()
            .unwrap()
            .update_server_flags(id, true, true)
            .unwrap();
    }
    (a, b)
}

fn identity(password: PasswordId) -> Identity {
    Identity {
        id: IdentityId::new(),
        name: "ops".into(),
        username: Some("deploy".into()),
        key: None,
        password: Some(password),
        location: None,
    }
}

fn host_with(settings: ConnectionSettings) -> Host {
    let mut h = sample_host("web");
    h.settings = ConnectionSettings {
        address: Some("web.example.org".into()),
        ..settings
    };
    h
}

/// The username and password `dev` would connect `host` with.
fn credentials_on(dev: &Dev, host: &Host) -> (String, Option<String>) {
    let store = dev.store.lock().unwrap();
    let (groups, hosts, identities) = (
        store.list_groups().unwrap(),
        store.list_hosts().unwrap(),
        store.list_identities().unwrap(),
    );
    let route = resolve_route(
        &Directory {
            groups: &groups,
            hosts: &hosts,
            identities: &identities,
        },
        host.id,
    )
    .unwrap();
    let password = route
        .target
        .password
        .and_then(|p| dev.vault.password(&store, p).ok())
        .map(|p| p.to_string());
    (route.target.username, password)
}

fn server_bytes(server: &TestServer) -> Vec<u8> {
    let path = server.database_url.trim_start_matches("sqlite://");
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn.prepare("SELECT record FROM records").unwrap();
    let rows: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut bytes = rows.join("\n").into_bytes();
    bytes.extend(std::fs::read(path).unwrap());
    bytes
}

#[tokio::test]
async fn identity_and_password_reach_second_device() {
    let server = TestServer::start().await;
    let (a, b) = pair(&server).await;
    let p = PasswordId::new();
    let i = identity(p);
    save_password(&a, p, Some(server_id(&a)));
    save_identity(&a, &i);
    let host = host_with(ConnectionSettings {
        identity: Some(i.id),
        ..Default::default()
    });
    save_host(&a, &host);
    sync(&a).await;
    sync(&b).await;
    assert_eq!(
        credentials_on(&b, &host),
        ("deploy".to_owned(), Some(SECRET.to_owned()))
    );
}

#[tokio::test]
async fn password_not_sent_without_sync_keys() {
    let server = TestServer::start().await;
    let (a, b) = pair(&server).await;
    let id = server_id(&a);
    a.store
        .lock()
        .unwrap()
        .update_server_flags(id, true, false)
        .unwrap();
    let p = PasswordId::new();
    save_password(&a, p, Some(server_id(&a)));
    let host = host_with(ConnectionSettings {
        username: Some("root".into()),
        password: Some(p),
        ..Default::default()
    });
    save_host(&a, &host);
    sync(&a).await;
    sync(&b).await;
    assert_eq!(b.hosts().len(), 1);
    assert!(
        b.store
            .lock()
            .unwrap()
            .list_password_ids()
            .unwrap()
            .is_empty()
    );
    assert_eq!(credentials_on(&b, &host).1, None);
}

#[tokio::test]
async fn server_never_sees_password_plaintext() {
    let server = TestServer::start().await;
    let (a, b) = pair(&server).await;
    let p = PasswordId::new();
    save_password(&a, p, Some(server_id(&a)));
    save_host(
        &a,
        &host_with(ConnectionSettings {
            username: Some("root".into()),
            password: Some(p),
            ..Default::default()
        }),
    );
    sync(&a).await;
    sync(&b).await;
    assert_eq!(
        b.store.lock().unwrap().list_password_ids().unwrap(),
        vec![p]
    );
    let bytes = server_bytes(&server);
    assert!(!bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()));
}

#[tokio::test]
async fn password_at_other_location_is_not_referenced() {
    let server = TestServer::start().await;
    let (a, b) = pair(&server).await;
    let p = PasswordId::new();
    save_password(&a, p, None);
    let host = host_with(ConnectionSettings {
        username: Some("root".into()),
        password: Some(p),
        ..Default::default()
    });
    // Saving moves the password to the host's place; put it back to this device only.
    save_host(&a, &host);
    {
        let store = a.store.lock().unwrap();
        let sealed = store.password_record(p).unwrap().unwrap().0;
        store.put_password_record(p, &sealed, None).unwrap();
    }
    sync(&a).await;
    sync(&b).await;
    let on_b = b.hosts().pop().unwrap();
    assert_eq!(on_b.settings.password, None);
    sync(&a).await;
    let on_a = a.hosts().pop().unwrap();
    assert_eq!(on_a.settings.password, Some(p));
}

#[tokio::test]
async fn deleting_identity_reaches_second_device() {
    let server = TestServer::start().await;
    let (a, b) = pair(&server).await;
    let p = PasswordId::new();
    let i = identity(p);
    save_password(&a, p, Some(server_id(&a)));
    save_identity(&a, &i);
    sync(&a).await;
    sync(&b).await;
    assert_eq!(b.store.lock().unwrap().list_identities().unwrap().len(), 1);
    {
        let store = a.store.lock().unwrap();
        record_local_change(&store, ItemKind::Identity, &i.id.0.to_string(), 200).unwrap();
        store.delete_identity(i.id).unwrap();
    }
    sync(&a).await;
    sync(&b).await;
    assert!(
        b.store
            .lock()
            .unwrap()
            .list_identities()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn protected_key_syncs_encrypted_and_stays_protected() {
    let server = TestServer::start().await;
    let (a, b) = pair(&server).await;
    let server_a = server_id(&a);
    let id = {
        let store = a.store.lock().unwrap();
        let info = a
            .vault
            .generate(
                &store,
                "deploy",
                &sshcraft_core::keytypes::KeySpec::Ed25519,
                Some("pw-sshcraft"),
            )
            .unwrap();
        store
            .set_location(ItemKind::Key, &info.id.0.to_string(), Some(server_a))
            .unwrap();
        record_local_change(&store, ItemKind::Key, &info.id.0.to_string(), 100).unwrap();
        info.id
    };
    sync(&a).await;
    sync(&b).await;
    let store = b.store.lock().unwrap();
    let record = store.key_record(id).unwrap().expect("key reached b");
    assert!(record.protected);
    assert!(matches!(
        b.vault.private_key(&store, id),
        Err(sshcraft_core::vault::VaultError::PassphraseRequired)
    ));
    let a_key = {
        let a_store = a.store.lock().unwrap();
        a.vault.unlock_key(&a_store, id, "pw-sshcraft").unwrap()
    };
    let b_key = b.vault.unlock_key(&store, id, "pw-sshcraft").unwrap();
    assert_eq!(
        b_key.fingerprint(russh::keys::HashAlg::Sha256),
        a_key.fingerprint(russh::keys::HashAlg::Sha256)
    );
}

#[tokio::test]
async fn device_key_never_syncs() {
    let server = TestServer::start().await;
    let (a, b) = pair(&server).await;
    let key = {
        let store = a.store.lock().unwrap();
        let info = a.vault.generate_ed25519(&store, "plain").unwrap();
        let key = a.vault.private_key(&store, info.id).unwrap();
        store.delete_key_record(info.id).unwrap();
        a.vault.store_device_key(&store, "hello", &key).unwrap()
    };
    let host = host_with(ConnectionSettings {
        key: Some(sshcraft_core::model::KeyRef::Stored { id: key.id }),
        ..Default::default()
    });
    save_host(&a, &host);
    // Even a stray outbox entry for the key must not send it.
    record_local_change(
        &a.store.lock().unwrap(),
        ItemKind::Key,
        &key.id.0.to_string(),
        100,
    )
    .unwrap();
    sync(&a).await;
    sync(&b).await;
    let store = b.store.lock().unwrap();
    let hosts = store.list_hosts().unwrap();
    let synced = hosts
        .iter()
        .find(|h| h.id == host.id)
        .expect("host reached b");
    assert_eq!(synced.settings.key, None);
    assert!(store.key_record(key.id).unwrap().is_none());
    assert!(store.list_key_records().unwrap().is_empty());
}

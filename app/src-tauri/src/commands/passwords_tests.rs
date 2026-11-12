use super::*;
use crate::commands::identities::{IdentityInput, IdentityView, save_identity};
use sshcraft_core::model::{ProxyEndpoint, ServerId};
use sshcraft_core::store::{ShareKind, ShareRole, ShareRow};
use sshcraft_core::vault::MemoryMasterKey;
use uuid::Uuid;

const SECRET: &str = "hunter2-sshcraft";

fn setup() -> (Store, Vault) {
    let store = Store::open_in_memory().unwrap();
    let vault = Vault::unlock(&store, Box::new(MemoryMasterKey::default())).unwrap();
    (store, vault)
}

fn new_host(store: &Store, settings: ConnectionSettings) -> Host {
    let host = Host {
        id: HostId::new(),
        name: "web".into(),
        group_id: None,
        settings,
        location: None,
        tags: Vec::new(),
        os: None,
    };
    store.upsert_host(&host).unwrap();
    host
}

fn stored(store: &Store, id: HostId) -> Host {
    store
        .list_hosts()
        .unwrap()
        .into_iter()
        .find(|h| h.id == id)
        .unwrap()
}

fn socks() -> Proxy {
    Proxy::Socks5(ProxyEndpoint {
        address: "proxy".into(),
        port: 1080,
        username: Some("u".into()),
        password: None,
    })
}

#[test]
fn password_set_and_clear_roundtrip() {
    let (store, vault) = setup();
    let host = new_host(&store, ConnectionSettings::default());
    let owner = PasswordOwner::Host { id: host.id };
    set_password(&store, &vault, owner, SECRET).unwrap();
    let id = stored(&store, host.id).settings.password.unwrap();
    assert_eq!(vault.password(&store, id).unwrap().as_str(), SECRET);
    // Changing keeps the id.
    set_password(&store, &vault, owner, "other").unwrap();
    assert_eq!(stored(&store, host.id).settings.password, Some(id));
    assert_eq!(vault.password(&store, id).unwrap().as_str(), "other");
    clear_password(&store, owner).unwrap();
    assert_eq!(stored(&store, host.id).settings.password, None);
    assert!(store.list_password_ids().unwrap().is_empty());
}

#[test]
fn proxy_password_needs_a_proxy() {
    let (store, vault) = setup();
    let host = new_host(&store, ConnectionSettings::default());
    let err = set_password(
        &store,
        &vault,
        PasswordOwner::HostProxy { id: host.id },
        SECRET,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    let host = new_host(
        &store,
        ConnectionSettings {
            proxy: Some(socks()),
            ..Default::default()
        },
    );
    set_password(
        &store,
        &vault,
        PasswordOwner::HostProxy { id: host.id },
        SECRET,
    )
    .unwrap();
    assert!(
        proxy_password(&stored(&store, host.id).settings)
            .unwrap()
            .is_some()
    );
}

#[test]
fn password_never_returned() {
    let (store, vault) = setup();
    let host = new_host(
        &store,
        ConnectionSettings {
            proxy: Some(socks()),
            ..Default::default()
        },
    );
    set_password(&store, &vault, PasswordOwner::Host { id: host.id }, SECRET).unwrap();
    set_password(
        &store,
        &vault,
        PasswordOwner::HostProxy { id: host.id },
        SECRET,
    )
    .unwrap();
    let (identity, _) = save_identity(
        &store,
        Some(&vault),
        IdentityInput {
            id: None,
            name: "ops".into(),
            username: Some("deploy".into()),
            key: None,
            location: None,
            password: PasswordChange::Set {
                secret: SECRET.into(),
            },
        },
    )
    .unwrap();
    let view = host_view(stored(&store, host.id));
    assert!(view.has_password && view.proxy_has_password);
    let identities: Vec<IdentityView> = store
        .list_identities()
        .unwrap()
        .into_iter()
        .map(IdentityView::from)
        .collect();
    assert!(identities[0].has_password);
    let json = format!(
        "{}{}{}",
        serde_json::to_string(&view).unwrap(),
        serde_json::to_string(&identities).unwrap(),
        serde_json::to_string(&identity).unwrap()
    );
    assert!(!json.contains(SECRET));
    for id in store.list_password_ids().unwrap() {
        assert!(!json.contains(&id.0.to_string()), "{json}");
    }
}

#[test]
fn form_without_password_ids_keeps_them() {
    let (store, vault) = setup();
    let host = new_host(
        &store,
        ConnectionSettings {
            proxy: Some(socks()),
            ..Default::default()
        },
    );
    set_password(&store, &vault, PasswordOwner::Host { id: host.id }, SECRET).unwrap();
    set_password(
        &store,
        &vault,
        PasswordOwner::HostProxy { id: host.id },
        SECRET,
    )
    .unwrap();
    let before = stored(&store, host.id);
    let mut from_form = host_view(before.clone()).host;
    from_form.name = "renamed".into();
    let orphans = keep_passwords(Some(&before.settings), &mut from_form.settings);
    assert!(orphans.is_empty());
    assert_eq!(from_form.settings, before.settings);
    // The proxy switched off: its password is left over.
    from_form.settings.proxy = Some(Proxy::Direct);
    let orphans = keep_passwords(Some(&before.settings), &mut from_form.settings);
    assert_eq!(
        orphans,
        vec![proxy_password(&before.settings).unwrap().unwrap()]
    );
    assert_eq!(from_form.settings.password, before.settings.password);
}

#[test]
fn read_only_share_refuses_password_set() {
    let (store, vault) = setup();
    let server = ServerId::new();
    store
        .add_server(&sshcraft_core::store::ServerRow {
            id: server,
            name: "home".into(),
            origin: "https://sync.example.org".into(),
            account: [1; 32],
            device_id: Uuid::new_v4(),
            sealed_device: vec![0],
            sync_enabled: true,
            sync_keys: true,
            revoked: false,
            log_head: None,
            tracker: None,
            created_at: 1,
        })
        .unwrap();
    let host = new_host(&store, ConnectionSettings::default());
    store
        .set_location(ItemKind::Host, &host.id.0.to_string(), Some(server))
        .unwrap();
    let area = Uuid::new_v4();
    store
        .put_share(
            server,
            &ShareRow {
                area_id: area,
                kind: ShareKind::Host,
                group_id: None,
                with_keys: true,
                owned: false,
                role: Some(ShareRole::Read),
                owner: [9; 32],
            },
        )
        .unwrap();
    store
        .set_item_area(server, ItemKind::Host, &host.id.0.to_string(), area)
        .unwrap();
    let owner = PasswordOwner::Host { id: host.id };
    assert_eq!(
        set_password(&store, &vault, owner, SECRET)
            .unwrap_err()
            .code,
        ErrorCode::ShareReadOnly
    );
    assert_eq!(
        clear_password(&store, owner).unwrap_err().code,
        ErrorCode::ShareReadOnly
    );
    assert!(store.list_password_ids().unwrap().is_empty());
}

/// A host in a share this account received with `role`.
fn received_host(role: ShareRole) -> (Store, Vault, Host, ServerId, Uuid) {
    let (store, vault) = setup();
    let server = ServerId::new();
    store
        .add_server(&sshcraft_core::store::ServerRow {
            id: server,
            name: "home".into(),
            origin: "https://sync.example.org".into(),
            account: [1; 32],
            device_id: Uuid::new_v4(),
            sealed_device: vec![0],
            sync_enabled: true,
            sync_keys: true,
            revoked: false,
            log_head: None,
            tracker: None,
            created_at: 1,
        })
        .unwrap();
    let host = new_host(&store, ConnectionSettings::default());
    store
        .set_location(ItemKind::Host, &host.id.0.to_string(), Some(server))
        .unwrap();
    let area = Uuid::new_v4();
    store
        .put_share(
            server,
            &ShareRow {
                area_id: area,
                kind: ShareKind::Host,
                group_id: None,
                with_keys: false,
                owned: false,
                role: Some(role),
                owner: [9; 32],
            },
        )
        .unwrap();
    store
        .set_item_area(server, ItemKind::Host, &host.id.0.to_string(), area)
        .unwrap();
    (store, vault, host, server, area)
}

#[test]
fn write_share_without_keys_allows_own_password() {
    let (store, vault, host, _, _) = received_host(ShareRole::Write);
    set_password(&store, &vault, PasswordOwner::Host { id: host.id }, SECRET).unwrap();
    let id = stored(&store, host.id).settings.password.unwrap();
    assert_eq!(vault.password(&store, id).unwrap().as_str(), SECRET);
}

#[test]
fn shared_password_cannot_be_changed() {
    let (store, vault, host, server, area) = received_host(ShareRole::Write);
    let owner = PasswordOwner::Host { id: host.id };
    set_password(&store, &vault, owner, SECRET).unwrap();
    let id = stored(&store, host.id).settings.password.unwrap();
    // As if the password had come with the share.
    store
        .set_item_area(server, ItemKind::Password, &id.0.to_string(), area)
        .unwrap();
    assert_eq!(
        set_password(&store, &vault, owner, "other")
            .unwrap_err()
            .code,
        ErrorCode::ShareReadOnly
    );
}

#[test]
fn form_changing_proxy_address_drops_proxy_password() {
    let (store, vault) = setup();
    let host = new_host(
        &store,
        ConnectionSettings {
            proxy: Some(socks()),
            ..Default::default()
        },
    );
    set_password(
        &store,
        &vault,
        PasswordOwner::HostProxy { id: host.id },
        SECRET,
    )
    .unwrap();
    let before = stored(&store, host.id);
    let mut from_form = host_view(before.clone()).host;
    from_form.settings.proxy = Some(Proxy::Socks5(ProxyEndpoint {
        address: "elsewhere.example.org".into(),
        port: 1080,
        username: Some("u".into()),
        password: None,
    }));
    let orphans = keep_passwords(Some(&before.settings), &mut from_form.settings);
    assert_eq!(
        orphans,
        vec![proxy_password(&before.settings).unwrap().unwrap()]
    );
    assert_eq!(proxy_password(&from_form.settings).unwrap(), None);
}

#[test]
fn locked_vault_saves_no_identity() {
    let (store, _vault) = setup();
    let err = save_identity(
        &store,
        None,
        IdentityInput {
            id: None,
            name: "ops".into(),
            username: Some("deploy".into()),
            key: None,
            location: None,
            password: PasswordChange::Set {
                secret: SECRET.into(),
            },
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::VaultLocked);
    assert!(store.list_identities().unwrap().is_empty());
}

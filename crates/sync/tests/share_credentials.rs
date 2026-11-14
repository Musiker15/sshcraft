mod common;

use common::{Dev, TestServer, account, create, engine, seal_in, sync};
use sshcraft_core::model::{
    ConnectionSettings, Directory, Group, GroupId, Host, Identity, IdentityId, PasswordId, Proxy,
    ProxyEndpoint, ResolvedHost, resolve_route,
};
use sshcraft_core::store::ItemKind;
use sshcraft_proto::area::Role;
use sshcraft_sync::contacts::{confirm, find};
use sshcraft_sync::engine::record_local_change;
use sshcraft_sync::items::ItemBody;
use sshcraft_sync::shares::{Grant, ShareWhat, share};
use uuid::Uuid;
use zeroize::Zeroizing;

const SECRET: &str = "hunter2-sshcraft";
const PROXY_SECRET: &str = "proxy-pass-sshcraft";

async fn people(server: &TestServer) -> (Dev, Dev) {
    let (a, b) = (Dev::new(), Dev::new());
    create(server, &a, "a@example.org").await;
    create(server, &b, "b@example.org").await;
    for dev in [&a, &b] {
        let id = dev.row().id;
        dev.store
            .lock()
            .unwrap()
            .update_server_flags(id, true, true)
            .unwrap();
    }
    let e = engine(&a).await;
    let found = find(&e, &a.store, "b@example.org").await.unwrap();
    confirm(&a.store, a.row().id, "b@example.org", &found.account).unwrap();
    (a, b)
}

struct Setup {
    group: Group,
    host: Host,
    identity_password: PasswordId,
    proxy_password: PasswordId,
}

/// Group "team" with an identity (user `deploy`, a password) and host "one" in it that goes
/// through a SOCKS5 proxy with a password; all at the server, synced.
async fn team(a: &Dev) -> Setup {
    let server = a.row().id;
    let (identity_password, proxy_password) = (PasswordId::new(), PasswordId::new());
    let identity = Identity {
        id: IdentityId::new(),
        name: "ops".into(),
        username: Some("deploy".into()),
        key: None,
        password: Some(identity_password),
        location: Some(server),
    };
    let group = Group {
        id: GroupId::new(),
        name: "team".into(),
        defaults: ConnectionSettings {
            identity: Some(identity.id),
            ..Default::default()
        },
        location: Some(server),
        parent_id: None,
        tags: Vec::new(),
    };
    let mut host = common::sample_host("one");
    host.group_id = Some(group.id);
    host.location = Some(server);
    host.settings = ConnectionSettings {
        address: Some("one.example.org".into()),
        proxy: Some(Proxy::Socks5(ProxyEndpoint {
            address: "proxy.example.org".into(),
            port: 1080,
            username: Some("p".into()),
            password: Some(proxy_password),
        })),
        ..Default::default()
    };
    {
        let store = a.store.lock().unwrap();
        a.vault
            .store_password(&store, identity_password, SECRET, Some(server))
            .unwrap();
        a.vault
            .store_password(&store, proxy_password, PROXY_SECRET, Some(server))
            .unwrap();
        store.upsert_identity(&identity).unwrap();
        store.upsert_group(&group).unwrap();
        store.upsert_host(&host).unwrap();
        for (kind, id) in [
            (ItemKind::Password, identity_password.0),
            (ItemKind::Password, proxy_password.0),
            (ItemKind::Identity, identity.id.0),
            (ItemKind::Group, group.id.0),
            (ItemKind::Host, host.id.0),
        ] {
            record_local_change(&store, kind, &id.to_string(), 1).unwrap();
        }
    }
    sync(a).await;
    Setup {
        group,
        host,
        identity_password,
        proxy_password,
    }
}

async fn share_it(a: &Dev, b: &Dev, what: ShareWhat, role: Role, with_keys: bool) -> Uuid {
    let mut e = engine(a).await;
    let outcome = share(
        &mut e,
        &a.store,
        &a.vault,
        what,
        vec![Grant {
            account: account(b),
            role,
        }],
        with_keys,
        5,
    )
    .await
    .unwrap();
    sync(b).await;
    outcome.area
}

/// How `dev` would connect to the only host it has with name `name`.
fn route_on(dev: &Dev, name: &str) -> ResolvedHost {
    let store = dev.store.lock().unwrap();
    let (groups, hosts, identities) = (
        store.list_groups().unwrap(),
        store.list_hosts().unwrap(),
        store.list_identities().unwrap(),
    );
    let host = hosts.iter().find(|h| h.name == name).expect("host there");
    resolve_route(
        &Directory {
            groups: &groups,
            hosts: &hosts,
            identities: &identities,
        },
        host.id,
    )
    .unwrap()
    .target
}

fn secret(dev: &Dev, id: Option<PasswordId>) -> Option<String> {
    let store = dev.store.lock().unwrap();
    id.and_then(|p| dev.vault.password(&store, p).ok())
        .map(|p| p.to_string())
}

fn proxy_password(route: &ResolvedHost) -> Option<PasswordId> {
    match &route.proxy {
        Some(Proxy::Socks5(e) | Proxy::Http(e)) => e.password,
        _ => None,
    }
}

#[tokio::test]
async fn group_share_with_keys_carries_password_and_identity() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    share_it(&a, &b, ShareWhat::GroupAll(s.group.id), Role::Read, true).await;
    let route = route_on(&b, "one");
    assert_eq!(route.username, "deploy");
    assert_eq!(secret(&b, route.password).as_deref(), Some(SECRET));
}

#[tokio::test]
async fn group_share_without_keys_carries_identity_user_only() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    share_it(&a, &b, ShareWhat::GroupAll(s.group.id), Role::Read, false).await;
    let route = route_on(&b, "one");
    assert_eq!(route.username, "deploy");
    assert_eq!(secret(&b, route.password), None);
    assert_eq!(secret(&b, proxy_password(&route)), None);
    assert!(
        b.store
            .lock()
            .unwrap()
            .list_password_ids()
            .unwrap()
            .is_empty()
    );
    // The owner keeps everything.
    let mine = route_on(&a, "one");
    assert_eq!(secret(&a, mine.password).as_deref(), Some(SECRET));
}

#[tokio::test]
async fn selection_share_copies_identity_and_password() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    share_it(
        &a,
        &b,
        ShareWhat::Selection {
            group: s.group.id,
            hosts: vec![s.host.id],
        },
        Role::Read,
        true,
    )
    .await;
    let route = route_on(&b, "one");
    assert_eq!(route.username, "deploy");
    assert_eq!(secret(&b, route.password).as_deref(), Some(SECRET));
}

#[tokio::test]
async fn single_host_share_without_keys_has_user_but_no_password() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    share_it(&a, &b, ShareWhat::Host(s.host.id), Role::Read, false).await;
    let route = route_on(&b, "one");
    assert_eq!(route.username, "deploy");
    assert_eq!(secret(&b, route.password), None);
    assert!(
        b.store
            .lock()
            .unwrap()
            .list_password_ids()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn proxy_password_follows_with_keys() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    share_it(&a, &b, ShareWhat::Host(s.host.id), Role::Read, true).await;
    let route = route_on(&b, "one");
    assert_eq!(
        secret(&b, proxy_password(&route)).as_deref(),
        Some(PROXY_SECRET)
    );
    // The member's copy, not the owner's original.
    assert_ne!(proxy_password(&route), Some(s.proxy_password));
}

#[tokio::test]
async fn member_cannot_change_shared_password() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    share_it(&a, &b, ShareWhat::GroupAll(s.group.id), Role::Write, true).await;
    let mine = route_on(&b, "one").password.unwrap();
    {
        let store = b.store.lock().unwrap();
        let at = store
            .location_of(ItemKind::Password, &mine.0.to_string())
            .unwrap();
        b.vault
            .store_password(&store, mine, "changed-by-member", at)
            .unwrap();
        record_local_change(&store, ItemKind::Password, &mine.0.to_string(), 50).unwrap();
    }
    let mut e = engine(&b).await;
    e.pull(&b.store, &b.vault).await.unwrap();
    e.push(&b.store, &b.vault, 50).await.unwrap();
    assert!(
        e.warnings().iter().any(|w| w == "share_read_only"),
        "{:?}",
        e.warnings()
    );
    sync(&a).await;
    assert_eq!(
        secret(&a, Some(s.identity_password)).as_deref(),
        Some(SECRET)
    );
    assert_eq!(secret(&b, Some(mine)).as_deref(), Some(SECRET));
}

#[tokio::test]
async fn member_written_password_is_refused_by_owner() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    let area = share_it(&a, &b, ShareWhat::GroupAll(s.group.id), Role::Write, true).await;
    // B's client would refuse to send it; a modified client writes it anyway.
    let server_b = b.row().id;
    let entry = {
        let store = b.store.lock().unwrap();
        store
            .record_entry(
                server_b,
                ItemKind::Password,
                &s.identity_password.0.to_string(),
            )
            .unwrap()
            .unwrap()
    };
    let forged = seal_in(
        &b,
        area,
        ItemBody::Password {
            id: s.identity_password,
            secret: Zeroizing::new("forged".into()),
        },
        entry.record_id,
        entry.version + 1,
        999,
    );
    b.client(&server).await.put_record(&forged).await.unwrap();
    sync(&a).await;
    assert_eq!(
        secret(&a, Some(s.identity_password)).as_deref(),
        Some(SECRET)
    );
}

#[tokio::test]
async fn owner_password_change_reaches_member() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    share_it(
        &a,
        &b,
        ShareWhat::Selection {
            group: s.group.id,
            hosts: vec![s.host.id],
        },
        Role::Read,
        true,
    )
    .await;
    {
        let store = a.store.lock().unwrap();
        let at = store
            .location_of(ItemKind::Password, &s.identity_password.0.to_string())
            .unwrap();
        a.vault
            .store_password(&store, s.identity_password, "rotated", at)
            .unwrap();
        record_local_change(
            &store,
            ItemKind::Password,
            &s.identity_password.0.to_string(),
            60,
        )
        .unwrap();
    }
    sync(&a).await;
    sync(&b).await;
    let route = route_on(&b, "one");
    assert_eq!(secret(&b, route.password).as_deref(), Some("rotated"));
}

/// B changes the proxy of the shared host "one" (as the normal form would) and sends it.
async fn member_moves_proxy(b: &Dev, port: u16) {
    let server_b = b.row().id;
    {
        let store = b.store.lock().unwrap();
        let mut host = store
            .list_hosts()
            .unwrap()
            .into_iter()
            .find(|h| h.name == "one")
            .unwrap();
        host.settings.proxy = Some(Proxy::Socks5(ProxyEndpoint {
            address: "attacker.example.org".into(),
            port,
            username: Some("p".into()),
            password: None,
        }));
        store.upsert_host(&host).unwrap();
        record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 70).unwrap();
    }
    let _ = server_b;
    sync(b).await;
}

#[tokio::test]
async fn member_changing_the_proxy_does_not_get_the_owners_proxy_password() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    share_it(&a, &b, ShareWhat::GroupAll(s.group.id), Role::Write, false).await;
    member_moves_proxy(&b, 1080).await;
    sync(&a).await;
    let route = route_on(&a, "one");
    match &route.proxy {
        Some(Proxy::Socks5(e)) => assert_eq!(e.address, "attacker.example.org"),
        other => panic!("{other:?}"),
    }
    assert_eq!(proxy_password(&route), None);
}

#[tokio::test]
async fn member_changing_a_copy_proxy_does_not_get_the_owners_proxy_password() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    share_it(&a, &b, ShareWhat::Host(s.host.id), Role::Write, false).await;
    member_moves_proxy(&b, 1081).await;
    sync(&a).await;
    let route = route_on(&a, "one");
    match &route.proxy {
        Some(Proxy::Socks5(e)) => {
            assert_eq!((e.address.as_str(), e.port), ("attacker.example.org", 1081))
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(proxy_password(&route), None);
}

#[tokio::test]
async fn member_edit_elsewhere_keeps_the_owners_proxy_password() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    share_it(&a, &b, ShareWhat::Host(s.host.id), Role::Write, false).await;
    {
        let store = b.store.lock().unwrap();
        let mut host = store
            .list_hosts()
            .unwrap()
            .into_iter()
            .find(|h| h.name == "one")
            .unwrap();
        host.settings.port = Some(2222);
        store.upsert_host(&host).unwrap();
        record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 70).unwrap();
    }
    sync(&b).await;
    sync(&a).await;
    let route = route_on(&a, "one");
    assert_eq!(route.port, 2222);
    assert_eq!(
        secret(&a, proxy_password(&route)).as_deref(),
        Some(PROXY_SECRET)
    );
}

#[tokio::test]
async fn device_key_never_shared_even_with_keys() {
    let server = TestServer::start().await;
    let (a, b) = people(&server).await;
    let s = team(&a).await;
    {
        let store = a.store.lock().unwrap();
        let info = a.vault.generate_ed25519(&store, "plain").unwrap();
        let key = a.vault.private_key(&store, info.id).unwrap();
        store.delete_key_record(info.id).unwrap();
        let device = a.vault.store_device_key(&store, "hello", &key).unwrap();
        let mut host = s.host.clone();
        host.settings.key = Some(sshcraft_core::model::KeyRef::Stored { id: device.id });
        store.upsert_host(&host).unwrap();
        record_local_change(&store, ItemKind::Host, &host.id.0.to_string(), 2).unwrap();
        record_local_change(&store, ItemKind::Key, &device.id.0.to_string(), 2).unwrap();
    }
    sync(&a).await;
    share_it(&a, &b, ShareWhat::GroupAll(s.group.id), Role::Read, true).await;
    let route = route_on(&b, "one");
    assert_eq!(route.key, None);
    assert!(
        b.store
            .lock()
            .unwrap()
            .list_key_records()
            .unwrap()
            .is_empty()
    );
    // The password still went with the keys: the share itself worked.
    assert_eq!(secret(&b, route.password).as_deref(), Some(SECRET));
}

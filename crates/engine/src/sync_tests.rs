use super::*;
use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId};
use sshcraft_store::store::ServerRow;
use uuid::Uuid;

fn store_with_server() -> (Store, ServerId) {
    let store = Store::open_in_memory().unwrap();
    let id = ServerId::new();
    store
        .add_server(&ServerRow {
            id,
            name: "home".into(),
            origin: "https://sync.example.org".into(),
            account: [1; 32],
            device_id: Uuid::new_v4(),
            sealed_device: vec![0],
            sync_enabled: true,
            sync_keys: false,
            revoked: false,
            log_head: None,
            tracker: None,
            created_at: 1,
        })
        .unwrap();
    (store, id)
}

fn host(group: Option<GroupId>) -> Host {
    Host {
        forwards: Vec::new(),
        id: HostId::new(),
        name: "web".into(),
        group_id: group,
        settings: ConnectionSettings::default(),
        location: None,
        tags: Vec::new(),
        os: None,
    }
}

#[test]
fn saving_a_synced_host_queues_it() {
    let (store, server) = store_with_server();
    let mut h = host(None);
    h.location = Some(server);
    store.upsert_host(&h).unwrap();
    let id = h.id.0.to_string();
    let nudge = note_change(&store, ItemKind::Host, &id, Some(server), 5).unwrap();
    assert_eq!(nudge, vec![server]);
    assert_eq!(store.outbox(server).unwrap(), vec![(ItemKind::Host, id)]);
}

#[test]
fn saving_a_local_host_queues_nothing() {
    let (store, server) = store_with_server();
    let h = host(None);
    store.upsert_host(&h).unwrap();
    let nudge = note_change(&store, ItemKind::Host, &h.id.0.to_string(), None, 5).unwrap();
    assert!(nudge.is_empty());
    assert!(store.outbox(server).unwrap().is_empty());
}

#[test]
fn a_host_moved_into_a_synced_group_queues_for_its_server() {
    let (store, server) = store_with_server();
    let g = Group {
        id: GroupId::new(),
        name: "srv".into(),
        defaults: ConnectionSettings::default(),
        location: Some(server),
        parent_id: None,
        tags: Vec::new(),
    };
    store.upsert_group(&g).unwrap();
    let mut h = host(None);
    store.upsert_host(&h).unwrap();
    h.group_id = Some(g.id);
    store.upsert_host(&h).unwrap();
    let nudge = note_change(&store, ItemKind::Host, &h.id.0.to_string(), None, 5).unwrap();
    assert_eq!(nudge, vec![server]);
}

#[test]
fn leaving_a_server_queues_the_old_server_too() {
    let (store, server) = store_with_server();
    let mut h = host(None);
    h.location = Some(server);
    store.upsert_host(&h).unwrap();
    let id = h.id.0.to_string();
    store.set_location(ItemKind::Host, &id, None).unwrap();
    let nudge = note_change(&store, ItemKind::Host, &id, Some(server), 5).unwrap();
    assert_eq!(nudge, vec![server]);
    assert_eq!(store.outbox(server).unwrap(), vec![(ItemKind::Host, id)]);
}

#[test]
fn every_sync_error_maps_to_a_code() {
    let server = |code: &str| SyncError::Server { status: 409, code: code.into(), current: None };
    let cases = [
        (SyncError::BadOrigin, ErrorCode::SyncBadOrigin),
        (SyncError::OriginMismatch { server: "x".into() }, ErrorCode::SyncOriginMismatch),
        (SyncError::Network("x".into()), ErrorCode::SyncNetwork),
        (server("bad_api_key"), ErrorCode::SyncBadApiKey),
        (server("email_taken"), ErrorCode::SyncEmailTaken),
        (server("device_inactive"), ErrorCode::SyncRevoked),
        (server("busy"), ErrorCode::SyncServer),
        (SyncError::Proto(ProtoError::BadChecksum), ErrorCode::SyncChecksum),
        (SyncError::Proto(ProtoError::BadAccountKeyFormat), ErrorCode::SyncBadAccountKey),
        (SyncError::Proto(ProtoError::Rollback), ErrorCode::SyncProtocol),
        (SyncError::Revoked, ErrorCode::SyncRevoked),
        (SyncError::Blocked, ErrorCode::SyncBlocked),
        (server("device_blocked"), ErrorCode::SyncBlocked),
        (SyncError::NotAllowed, ErrorCode::SyncNotAllowed),
        (SyncError::AccountNotFound, ErrorCode::SyncAccountNotFound),
    ];
    for (err, code) in cases {
        assert_eq!(sync_error(err).failure(), code);
    }
    assert_eq!(sync_error(server("busy")).detail_text().as_deref(), Some("busy"));
}

#[test]
fn a_revoked_server_shows_revoked_without_a_runtime() {
    assert_eq!(server_status(true, StatusView::Off), StatusView::Revoked);
    assert_eq!(server_status(false, StatusView::Off), StatusView::Off);
    assert_eq!(server_status(false, StatusView::Idle), StatusView::Idle);
}

#[test]
fn only_known_server_codes_reach_the_window() {
    let err = sync_error(SyncError::Server { status: 400, code: "<b>call me</b>".into(), current: None });
    assert_eq!(err.detail_text().as_deref(), Some("unknown"));
    let err = sync_error(SyncError::Server { status: 503, code: "busy".into(), current: None });
    assert_eq!(err.detail_text().as_deref(), Some("busy"));
}

#[test]
fn editing_a_read_only_item_is_refused() {
    use sshcraft_model::model::{KeyId, KeyRef};
    use sshcraft_store::store::{ShareKind, ShareRole, ShareRow};
    let (store, server) = store_with_server();
    let mut h = host(None);
    h.location = Some(server);
    store.upsert_host(&h).unwrap();
    let id = h.id.0.to_string();
    // Not shared: anything goes and is queued.
    assert!(share_rights(&store, Change::SaveHost(&h)).unwrap());
    let area = Uuid::new_v4();
    let received = |role, kind| ShareRow { area_id: area, kind, group_id: None, with_keys: false, owned: false, role: Some(role), owner: [9; 32] };
    store.put_share(server, &received(ShareRole::Read, ShareKind::Host)).unwrap();
    store.set_item_area(server, ItemKind::Host, &id, area).unwrap();
    let mut renamed = h.clone();
    renamed.name = "mine".into();
    let refused = share_rights(&store, Change::SaveHost(&renamed)).unwrap_err();
    assert_eq!(refused.failure(), ErrorCode::ShareReadOnly);
    assert_eq!(share_rights(&store, Change::DeleteHost(h.id)).unwrap_err().failure(), ErrorCode::ShareReadOnly);
    assert_eq!(share_rights(&store, Change::Move(ItemKind::Host, &id)).unwrap_err().failure(), ErrorCode::ShareNotOwner);
    // Assigning an own key stays on this device.
    let mut keyed = h.clone();
    keyed.settings.key = Some(KeyRef::Stored { id: KeyId::new() });
    assert!(!share_rights(&store, Change::SaveHost(&keyed)).unwrap());
    // With write in a single-host share: changing yes, deleting no.
    store.put_share(server, &received(ShareRole::Write, ShareKind::Host)).unwrap();
    assert!(share_rights(&store, Change::SaveHost(&renamed)).unwrap());
    assert_eq!(share_rights(&store, Change::DeleteHost(h.id)).unwrap_err().failure(), ErrorCode::ShareReadOnly);
}

fn share_row(area: Uuid, owned: bool, role: Option<sshcraft_store::store::ShareRole>) -> sshcraft_store::store::ShareRow {
    sshcraft_store::store::ShareRow {
        area_id: area,
        kind: sshcraft_store::store::ShareKind::Selection,
        group_id: None,
        with_keys: false,
        owned,
        role,
        owner: [9; 32],
    }
}

#[test]
fn moving_an_own_host_into_a_received_group_is_refused() {
    use sshcraft_store::store::ShareRole;
    let (store, server) = store_with_server();
    let group = Group {
        id: GroupId::new(),
        name: "theirs".into(),
        defaults: ConnectionSettings::default(),
        location: Some(server),
        parent_id: None,
        tags: Vec::new(),
    };
    store.upsert_group(&group).unwrap();
    let area = Uuid::new_v4();
    store.put_share(server, &share_row(area, false, Some(ShareRole::Write))).unwrap();
    store.set_item_area(server, ItemKind::Group, &group.id.0.to_string(), area).unwrap();
    let mut mine = host(None);
    mine.location = Some(server);
    store.upsert_host(&mine).unwrap();
    let mut moved = mine.clone();
    moved.group_id = Some(group.id);
    assert_eq!(share_rights(&store, Change::SaveHost(&moved)).unwrap_err().failure(), ErrorCode::ShareReadOnly);
}

#[test]
fn received_keys_cannot_be_deleted() {
    use sshcraft_model::model::KeyId;
    use sshcraft_store::store::ShareRole;
    let (store, server) = store_with_server();
    let key = KeyId::new();
    let area = Uuid::new_v4();
    store.put_share(server, &share_row(area, false, Some(ShareRole::Write))).unwrap();
    store.set_item_area(server, ItemKind::Key, &key.0.to_string(), area).unwrap();
    // The key record itself is not needed for the check: its area decides.
    let refused = share_rights_for_key(&store, server, key).unwrap_err();
    assert_eq!(refused.failure(), ErrorCode::ShareNotOwner);
}

#[test]
fn a_shared_group_is_ended_before_it_is_deleted() {
    let (store, server) = store_with_server();
    let group = Group {
        id: GroupId::new(),
        name: "mine".into(),
        defaults: ConnectionSettings::default(),
        location: Some(server),
        parent_id: None,
        tags: Vec::new(),
    };
    store.upsert_group(&group).unwrap();
    let area = Uuid::new_v4();
    store.put_share(server, &share_row(area, true, None)).unwrap();
    store.set_item_area(server, ItemKind::Group, &group.id.0.to_string(), area).unwrap();
    assert_eq!(share_rights(&store, Change::DeleteGroup(group.id)).unwrap_err().failure(), ErrorCode::ShareEndFirst);
}

#[test]
fn moving_a_group_into_a_received_group_is_refused() {
    use sshcraft_store::store::ShareRole;
    let (store, server) = store_with_server();
    let theirs = Group {
        id: GroupId::new(),
        name: "theirs".into(),
        defaults: ConnectionSettings::default(),
        location: Some(server),
        parent_id: None,
        tags: Vec::new(),
    };
    store.upsert_group(&theirs).unwrap();
    let area = Uuid::new_v4();
    store.put_share(server, &share_row(area, false, Some(ShareRole::Write))).unwrap();
    store.set_item_area(server, ItemKind::Group, &theirs.id.0.to_string(), area).unwrap();
    let mine = Group {
        id: GroupId::new(),
        name: "mine".into(),
        defaults: ConnectionSettings::default(),
        location: Some(server),
        parent_id: None,
        tags: Vec::new(),
    };
    store.upsert_group(&mine).unwrap();
    let moved = Group { parent_id: Some(theirs.id), ..mine.clone() };
    assert_eq!(share_rights(&store, Change::SaveGroup(&moved)).unwrap_err().failure(), ErrorCode::ShareReadOnly);
}

#[test]
fn writer_may_delete_a_subgroup_in_a_group_share() {
    use sshcraft_store::store::{ShareKind, ShareRole, ShareRow};
    let (store, server) = store_with_server();
    let group = |name: &str, parent: Option<GroupId>| Group {
        id: GroupId::new(),
        name: name.into(),
        defaults: ConnectionSettings::default(),
        location: Some(server),
        parent_id: parent,
        tags: Vec::new(),
    };
    let root = group("root", None);
    let sub = group("sub", Some(root.id));
    store.upsert_group(&root).unwrap();
    store.upsert_group(&sub).unwrap();
    let area = Uuid::new_v4();
    store
        .put_share(
            server,
            &ShareRow {
                area_id: area,
                kind: ShareKind::GroupAll,
                group_id: Some(root.id.0.to_string()),
                with_keys: false,
                owned: false,
                role: Some(ShareRole::Write),
                owner: [9; 32],
            },
        )
        .unwrap();
    store.set_item_area(server, ItemKind::Group, &root.id.0.to_string(), area).unwrap();
    assert!(share_rights(&store, Change::DeleteGroup(sub.id)).unwrap());
    assert_eq!(share_rights(&store, Change::DeleteGroup(root.id)).unwrap_err().failure(), ErrorCode::ShareNotOwner);
}

#[tokio::test]
async fn a_poisoned_store_lock_does_not_stop_sync() {
    let (store, server) = store_with_server();
    let vault = sshcraft_store::vault::Vault::unlock(&store, Box::new(sshcraft_store::vault::MemoryMasterKey::default())).unwrap();
    let store = Arc::new(Mutex::new(store));
    // A thread that died holding the store, as a command that panicked would leave it.
    let holder = store.clone();
    let _ = std::thread::spawn(move || {
        let _held = holder.lock().unwrap();
        panic!("dies holding the store");
    })
    .join();
    assert!(store.is_poisoned());
    let manager =
        SyncManager::new(store, Some(Arc::new(vault)), Arc::new(StatusRecorder { inner: Arc::new(NoObserver), statuses: Mutex::default() }));
    manager.start_all();
    assert_ne!(manager.status(server), StatusView::Off, "sync did not start after the store lock was poisoned");
    manager.stop(server);
}

struct NoObserver;

impl SyncObserver for NoObserver {
    fn status(&self, _: ServerId, _: SyncStatus) {}
    fn data_changed(&self, _: ServerId) {}
    fn warning(&self, _: ServerId, _: String) {}
}

/// "Sync everything" moves what lives only here to the server, groups with all they hold, and
/// what is made afterwards goes there too. Switched off, items stay where they are.
#[test]
fn syncing_everything_moves_what_is_here() {
    let (store, server) = store_with_server();
    let top =
        Group { id: GroupId::new(), name: "srv".into(), defaults: ConnectionSettings::default(), location: None, parent_id: None, tags: Vec::new() };
    let sub = Group { id: GroupId::new(), name: "web".into(), parent_id: Some(top.id), ..top.clone() };
    store.upsert_group(&top).unwrap();
    store.upsert_group(&sub).unwrap();
    let inner = host(Some(sub.id));
    let loose = host(None);
    store.upsert_host(&inner).unwrap();
    store.upsert_host(&loose).unwrap();

    let nudge = sync_everything(&store, server, true, 5).unwrap();
    assert_eq!(nudge, vec![server]);
    assert!(store.sync_all(server).unwrap());
    assert!(store.list_hosts().unwrap().iter().all(|h| h.location == Some(server)), "every host moved");
    assert!(store.list_groups().unwrap().iter().all(|g| g.location == Some(server)), "every group moved");
    let queued = store.outbox(server).unwrap();
    for id in [top.id.0, sub.id.0, inner.id.0, loose.id.0] {
        assert!(queued.iter().any(|(_, q)| q == &id.to_string()), "{id} is queued");
    }

    let later = host(None);
    store.upsert_host(&later).unwrap();
    let stored = store.list_hosts().unwrap().into_iter().find(|h| h.id == later.id).unwrap();
    assert_eq!(stored.location, Some(server), "a new host goes to the server too");

    sync_everything(&store, server, false, 6).unwrap();
    assert!(!store.sync_all(server).unwrap());
    assert!(store.list_hosts().unwrap().iter().all(|h| h.location == Some(server)), "switching off moves nothing back");
    let after = host(None);
    store.upsert_host(&after).unwrap();
    let stored = store.list_hosts().unwrap().into_iter().find(|h| h.id == after.id).unwrap();
    assert_eq!(stored.location, None, "afterwards new hosts stay here again");
}

/// With "Sync everything" on, a key made or imported afterwards goes to the server and is queued
/// for it, like a new host; before, it got the server as its place but was never sent until it
/// was moved by hand.
#[tokio::test]
async fn new_keys_are_queued_for_the_server_that_syncs_everything() {
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = crate::Session::open(crate::SessionConfig::for_tests(dir.path())).unwrap();
    let server = ServerId::new();
    {
        let store = session.store();
        store
            .add_server(&ServerRow {
                id: server,
                name: "home".into(),
                origin: "https://sync.example.org".into(),
                account: [1; 32],
                device_id: Uuid::new_v4(),
                sealed_device: vec![0],
                sync_enabled: false,
                sync_keys: false,
                revoked: false,
                log_head: None,
                tracker: None,
                created_at: 1,
            })
            .unwrap();
        sync_everything(&store, server, true, 5).unwrap();
    }
    let generated = session
        .execute(crate::Caller::User, "key.generate", serde_json::json!({ "name": "made", "spec": { "type": "ed25519" }, "passphrase": null }))
        .await
        .unwrap();
    let key = russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
    let text = key.to_openssh(russh::keys::ssh_key::LineEnding::LF).unwrap().to_string();
    let imported = session.execute(crate::Caller::User, "key.import", serde_json::json!({ "name": "brought", "text": text })).await.unwrap();
    let queued = session.store().outbox(server).unwrap();
    for info in [generated, imported] {
        let id = info["id"].as_str().unwrap().to_string();
        assert!(queued.iter().any(|(_, q)| q == &id), "the new key {id} is queued for the server");
    }
}

use super::*;
use crate::commands::sync::account_key_qr_svg_checked;
use sshcraft_core::model::{ConnectionSettings, Group, GroupId, Host, HostId};
use sshcraft_core::store::ServerRow;
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
fn qr_for_a_bad_account_key_is_refused() {
    assert_eq!(
        account_key_qr_svg_checked("hello").unwrap_err().code,
        ErrorCode::SyncBadAccountKey
    );
    let key = sshcraft_proto::identity::AccountKey::generate().export();
    let mut typo = key.to_string();
    // A character in the middle: the last one partly holds Base32 padding bits, so a
    // change there can be a format error instead of a checksum error.
    let at = typo.len() / 2 + 10;
    let swapped = if typo.as_bytes()[at] == b'A' {
        "B"
    } else {
        "A"
    };
    typo.replace_range(at..=at, swapped);
    assert_eq!(
        account_key_qr_svg_checked(&typo).unwrap_err().code,
        ErrorCode::SyncChecksum
    );
    assert!(account_key_qr_svg_checked(&key).unwrap().contains("<svg"));
}

#[test]
fn every_sync_error_maps_to_a_code() {
    let server = |code: &str| SyncError::Server {
        status: 409,
        code: code.into(),
        current: None,
    };
    let cases = [
        (SyncError::BadOrigin, ErrorCode::SyncBadOrigin),
        (
            SyncError::OriginMismatch { server: "x".into() },
            ErrorCode::SyncOriginMismatch,
        ),
        (SyncError::Network("x".into()), ErrorCode::SyncNetwork),
        (server("bad_api_key"), ErrorCode::SyncBadApiKey),
        (server("email_taken"), ErrorCode::SyncEmailTaken),
        (server("device_inactive"), ErrorCode::SyncRevoked),
        (server("busy"), ErrorCode::SyncServer),
        (
            SyncError::Proto(ProtoError::BadChecksum),
            ErrorCode::SyncChecksum,
        ),
        (
            SyncError::Proto(ProtoError::BadAccountKeyFormat),
            ErrorCode::SyncBadAccountKey,
        ),
        (
            SyncError::Proto(ProtoError::Rollback),
            ErrorCode::SyncProtocol,
        ),
        (SyncError::Revoked, ErrorCode::SyncRevoked),
        (SyncError::Blocked, ErrorCode::SyncBlocked),
        (server("device_blocked"), ErrorCode::SyncBlocked),
        (SyncError::NotAllowed, ErrorCode::SyncNotAllowed),
        (SyncError::AccountNotFound, ErrorCode::SyncAccountNotFound),
    ];
    for (err, code) in cases {
        assert_eq!(sync_error(err).code, code);
    }
    assert_eq!(sync_error(server("busy")).detail.as_deref(), Some("busy"));
}

#[test]
fn a_revoked_server_shows_revoked_without_a_runtime() {
    assert_eq!(server_status(true, StatusView::Off), StatusView::Revoked);
    assert_eq!(server_status(false, StatusView::Off), StatusView::Off);
    assert_eq!(server_status(false, StatusView::Idle), StatusView::Idle);
}

#[test]
fn only_known_server_codes_reach_the_window() {
    let err = sync_error(SyncError::Server {
        status: 400,
        code: "<b>call me</b>".into(),
        current: None,
    });
    assert_eq!(err.detail.as_deref(), Some("unknown"));
    let err = sync_error(SyncError::Server {
        status: 503,
        code: "busy".into(),
        current: None,
    });
    assert_eq!(err.detail.as_deref(), Some("busy"));
}

#[test]
fn inputs_never_show_their_secrets() {
    let input: crate::commands::sync::JoinInput = serde_json::from_value(serde_json::json!({
        "server_name": "home",
        "origin": "https://sync.example.org",
        "api_key": "api-secret",
        "account_key": "sshcraft-account-1:SECRET",
        "device_name": "laptop",
    }))
    .unwrap();
    let shown = format!("{input:?}");
    assert!(
        !shown.contains("api-secret") && !shown.contains("SECRET"),
        "{shown}"
    );
}

#[test]
fn editing_a_read_only_item_is_refused() {
    use sshcraft_core::model::{KeyId, KeyRef};
    use sshcraft_core::store::{ShareKind, ShareRole, ShareRow};
    let (store, server) = store_with_server();
    let mut h = host(None);
    h.location = Some(server);
    store.upsert_host(&h).unwrap();
    let id = h.id.0.to_string();
    // Not shared: anything goes and is queued.
    assert!(share_rights(&store, Change::SaveHost(&h)).unwrap());
    let area = Uuid::new_v4();
    let received = |role, kind| ShareRow {
        area_id: area,
        kind,
        group_id: None,
        with_keys: false,
        owned: false,
        role: Some(role),
        owner: [9; 32],
    };
    store
        .put_share(server, &received(ShareRole::Read, ShareKind::Host))
        .unwrap();
    store
        .set_item_area(server, ItemKind::Host, &id, area)
        .unwrap();
    let mut renamed = h.clone();
    renamed.name = "mine".into();
    let refused = share_rights(&store, Change::SaveHost(&renamed)).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ShareReadOnly);
    assert_eq!(
        share_rights(&store, Change::DeleteHost(h.id))
            .unwrap_err()
            .code,
        ErrorCode::ShareReadOnly
    );
    assert_eq!(
        share_rights(&store, Change::Move(ItemKind::Host, &id))
            .unwrap_err()
            .code,
        ErrorCode::ShareNotOwner
    );
    // Assigning an own key stays on this device.
    let mut keyed = h.clone();
    keyed.settings.key = Some(KeyRef::Stored { id: KeyId::new() });
    assert!(!share_rights(&store, Change::SaveHost(&keyed)).unwrap());
    // With write in a single-host share: changing yes, deleting no.
    store
        .put_share(server, &received(ShareRole::Write, ShareKind::Host))
        .unwrap();
    assert!(share_rights(&store, Change::SaveHost(&renamed)).unwrap());
    assert_eq!(
        share_rights(&store, Change::DeleteHost(h.id))
            .unwrap_err()
            .code,
        ErrorCode::ShareReadOnly
    );
}

fn share_row(
    area: Uuid,
    owned: bool,
    role: Option<sshcraft_core::store::ShareRole>,
) -> sshcraft_core::store::ShareRow {
    sshcraft_core::store::ShareRow {
        area_id: area,
        kind: sshcraft_core::store::ShareKind::Selection,
        group_id: None,
        with_keys: false,
        owned,
        role,
        owner: [9; 32],
    }
}

#[test]
fn moving_an_own_host_into_a_received_group_is_refused() {
    use sshcraft_core::store::ShareRole;
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
    store
        .put_share(server, &share_row(area, false, Some(ShareRole::Write)))
        .unwrap();
    store
        .set_item_area(server, ItemKind::Group, &group.id.0.to_string(), area)
        .unwrap();
    let mut mine = host(None);
    mine.location = Some(server);
    store.upsert_host(&mine).unwrap();
    let mut moved = mine.clone();
    moved.group_id = Some(group.id);
    assert_eq!(
        share_rights(&store, Change::SaveHost(&moved))
            .unwrap_err()
            .code,
        ErrorCode::ShareReadOnly
    );
}

#[test]
fn received_keys_cannot_be_deleted() {
    use sshcraft_core::model::KeyId;
    use sshcraft_core::store::ShareRole;
    let (store, server) = store_with_server();
    let key = KeyId::new();
    let area = Uuid::new_v4();
    store
        .put_share(server, &share_row(area, false, Some(ShareRole::Write)))
        .unwrap();
    store
        .set_item_area(server, ItemKind::Key, &key.0.to_string(), area)
        .unwrap();
    // The key record itself is not needed for the check: its area decides.
    let refused = share_rights_for_key(&store, server, key).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ShareNotOwner);
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
    store
        .put_share(server, &share_row(area, true, None))
        .unwrap();
    store
        .set_item_area(server, ItemKind::Group, &group.id.0.to_string(), area)
        .unwrap();
    assert_eq!(
        share_rights(&store, Change::DeleteGroup(group.id))
            .unwrap_err()
            .code,
        ErrorCode::ShareEndFirst
    );
}

#[test]
fn moving_a_group_into_a_received_group_is_refused() {
    use sshcraft_core::store::ShareRole;
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
    store
        .put_share(server, &share_row(area, false, Some(ShareRole::Write)))
        .unwrap();
    store
        .set_item_area(server, ItemKind::Group, &theirs.id.0.to_string(), area)
        .unwrap();
    let mine = Group {
        id: GroupId::new(),
        name: "mine".into(),
        defaults: ConnectionSettings::default(),
        location: Some(server),
        parent_id: None,
        tags: Vec::new(),
    };
    store.upsert_group(&mine).unwrap();
    let moved = Group {
        parent_id: Some(theirs.id),
        ..mine.clone()
    };
    assert_eq!(
        share_rights(&store, Change::SaveGroup(&moved))
            .unwrap_err()
            .code,
        ErrorCode::ShareReadOnly
    );
}

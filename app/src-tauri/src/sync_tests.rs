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
    let last = typo.pop().unwrap();
    typo.push(if last == 'A' { 'B' } else { 'A' });
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
        (SyncError::NotAllowed, ErrorCode::SyncNotAllowed),
        (SyncError::AccountNotFound, ErrorCode::SyncAccountNotFound),
    ];
    for (err, code) in cases {
        assert_eq!(sync_error(err).code, code);
    }
    assert_eq!(sync_error(server("busy")).detail.as_deref(), Some("busy"));
}

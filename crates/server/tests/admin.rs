mod common;

use common::{Person, fresh_sqlite};
use sshcraft_proto::area::{AreaId, AreaKey, AreaManifest, Member, Role};
use sshcraft_server_lib::admin::{self, AdminError};
use sshcraft_server_lib::store::Store;
use uuid::Uuid;

async fn area(store: &dyn Store, owner: &Person, members: Vec<Member>) -> AreaId {
    let id = AreaId(Uuid::new_v4());
    let m = AreaManifest::sign(id, 1, 1, &AreaKey::generate(), owner.public(), members, owner.devices[0].id, &owner.devices[0].keys);
    store.put_manifest(&owner.public(), &m, None).await.unwrap();
    id
}

fn text(out: Vec<u8>) -> String {
    String::from_utf8(out).unwrap()
}

/// Alice owns one area shared with a writer and one shared only with a reader.
async fn world(store: &dyn Store) -> (Person, Person, Person, AreaId, AreaId) {
    let (a, w, r) = (Person::new("alice"), Person::new("wendy"), Person::new("rita"));
    for p in [&a, &w, &r] {
        p.create_in(store).await;
    }
    let shared = area(store, &a, vec![Member { account: w.public(), role: Role::Write }]).await;
    let read_only = area(store, &a, vec![Member { account: r.public(), role: Role::Read }]).await;
    (a, w, r, shared, read_only)
}

#[tokio::test]
async fn delete_without_yes_changes_nothing() {
    let (store, _dir) = fresh_sqlite().await;
    let (a, _, _, shared, read_only) = world(store.as_ref()).await;
    let mut out = Vec::new();
    admin::users_delete(store.as_ref(), " Alice@Example.org ", false, &mut out).await.unwrap();
    let out = text(out);
    assert!(out.contains(&shared.0.to_string()) && out.contains("transfer to wendy@example.org"), "{out}");
    assert!(out.contains(&read_only.0.to_string()) && out.contains("delete"), "{out}");
    assert!(out.contains("--yes"), "{out}");
    assert!(store.account(&a.public()).await.unwrap().is_some());
}

#[tokio::test]
async fn delete_with_yes_transfers_and_deletes() {
    let (store, _dir) = fresh_sqlite().await;
    let (a, w, _, shared, read_only) = world(store.as_ref()).await;
    let mut out = Vec::new();
    admin::users_delete(store.as_ref(), "alice@example.org", true, &mut out).await.unwrap();
    let out = text(out);
    assert!(out.contains("transfer to wendy@example.org"), "{out}");
    assert!(store.account(&a.public()).await.unwrap().is_none());
    assert_eq!(store.area(shared).await.unwrap().unwrap().owner, w.public());
    assert!(store.area(read_only).await.unwrap().is_none());
}

#[tokio::test]
async fn lock_unlock_and_block_take_effect() {
    let (store, _dir) = fresh_sqlite().await;
    let a = Person::new("alice");
    a.create_in(store.as_ref()).await;
    let device = a.devices[0].id;
    store.create_session([1; 32], device, &a.public(), i64::MAX).await.unwrap();
    let mut out = Vec::new();
    admin::users_lock(store.as_ref(), "alice@example.org", true, &mut out).await.unwrap();
    assert!(store.account(&a.public()).await.unwrap().unwrap().locked);
    assert!(store.session([1; 32]).await.unwrap().is_none());
    admin::users_lock(store.as_ref(), "alice@example.org", false, &mut out).await.unwrap();
    assert!(!store.account(&a.public()).await.unwrap().unwrap().locked);

    store.create_session([2; 32], device, &a.public(), i64::MAX).await.unwrap();
    admin::devices_block(store.as_ref(), "alice@example.org", device, true, &mut out).await.unwrap();
    assert!(store.device(device).await.unwrap().unwrap().admin_blocked);
    assert!(store.session([2; 32]).await.unwrap().is_none());

    let mut listing = Vec::new();
    admin::devices_list(store.as_ref(), "alice@example.org", &mut listing).await.unwrap();
    let listing = text(listing);
    assert!(listing.contains(&device.to_string()) && listing.contains("alice-0") && listing.contains("blocked"), "{listing}");
    let mut users = Vec::new();
    admin::users_list(store.as_ref(), &mut users).await.unwrap();
    assert!(text(users).contains("alice@example.org"));

    let other = Person::new("bob");
    other.create_in(store.as_ref()).await;
    let err = admin::devices_block(store.as_ref(), "alice@example.org", other.devices[0].id, true, &mut out).await.unwrap_err();
    assert!(matches!(err, AdminError::UnknownDevice(_)));
}

#[tokio::test]
async fn api_key_rotate_changes_the_key() {
    let (store, _dir) = fresh_sqlite().await;
    let mut shown = Vec::new();
    admin::api_key_show(store.as_ref(), &mut shown).await.unwrap();
    let before = text(shown).trim().to_owned();
    let mut rotated = Vec::new();
    admin::api_key_rotate(store.as_ref(), &mut rotated).await.unwrap();
    let after = store.api_key().await.unwrap().0;
    assert_ne!(before, after);
    assert!(text(rotated).contains(&after));
}

#[tokio::test]
async fn unknown_user_is_an_error() {
    let (store, _dir) = fresh_sqlite().await;
    let mut out = Vec::new();
    let err = admin::users_lock(store.as_ref(), "nobody@example.org", true, &mut out).await.unwrap_err();
    assert!(matches!(err, AdminError::UnknownUser(_)));
    let err = admin::users_delete(store.as_ref(), "nobody@example.org", true, &mut out).await.unwrap_err();
    assert!(matches!(err, AdminError::UnknownUser(_)));
}

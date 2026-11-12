//! The storage contract. Every check runs against SQLite; with `SSHCRAFT_TEST_POSTGRES_URL`
//! set, the same checks run against PostgreSQL.

mod common;

use common::Person;
use sshcraft_proto::area::{AreaId, AreaKey, AreaManifest, Member, Role, seal_area_key};
use sshcraft_proto::record::{RecordHeader, SealedRecord};
use sshcraft_server::store::{AreaOutcome, DeviceChanges, Store, StoreError};
use std::sync::Arc;
use uuid::Uuid;

/// A store plus whatever has to live as long as it (a temp dir, a test database).
pub struct Fresh {
    pub store: Arc<dyn Store>,
    _guard: Box<dyn std::any::Any + Send>,
}

async fn fresh_sqlite() -> Option<Fresh> {
    let (store, dir) = common::fresh_sqlite().await;
    Some(Fresh {
        store,
        _guard: Box::new(dir),
    })
}

macro_rules! contract_tests {
    ($backend:ident, $fresh:path, [$($name:ident),* $(,)?]) => {
        mod $backend {
            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn $name() {
                    let Some(fresh) = $fresh().await else { return };
                    super::$name(fresh.store.clone()).await;
                }
            )*
        }
    };
}

macro_rules! all_contract_tests {
    ($backend:ident, $fresh:path) => {
        contract_tests!(
            $backend,
            $fresh,
            [
                api_key_is_created_once_and_rotates,
                email_is_unique_after_normalization,
                append_log_is_compare_and_swap,
                revoking_in_the_log_ends_sessions,
                locking_and_blocking_end_sessions,
                manifest_versions,
                members_index_follows_the_manifest,
                higher_key_version_drops_old_envelopes,
                records_are_compare_and_swap_and_numbered,
                account_deletion_follows_the_plan,
                sessions_expire,
            ]
        );
    };
}

all_contract_tests!(sqlite, super::fresh_sqlite);

fn manifest(
    owner: &Person,
    area: AreaId,
    version: u64,
    key_version: u32,
    members: Vec<Member>,
) -> (AreaManifest, AreaKey) {
    let key = AreaKey::generate();
    let m = AreaManifest::sign(
        area,
        version,
        key_version,
        &key,
        owner.public(),
        members,
        owner.devices[0].id,
        &owner.devices[0].keys,
    );
    (m, key)
}

fn record(by: &Person, area: AreaId, record_id: Uuid, version: u64, key: &AreaKey) -> SealedRecord {
    SealedRecord::seal(
        RecordHeader {
            record_id,
            area_id: area,
            version,
            key_version: 1,
            author_account: by.public(),
            author_device: by.devices[0].id,
        },
        b"body",
        key,
        &by.devices[0].keys,
    )
}

async fn api_key_is_created_once_and_rotates(store: Arc<dyn Store>) {
    let (first, created) = store.api_key().await.unwrap();
    assert!(created);
    assert!(first.len() >= 40);
    let (again, created) = store.api_key().await.unwrap();
    assert_eq!(again, first);
    assert!(!created);
    let rotated = store.rotate_api_key().await.unwrap();
    assert_ne!(rotated, first);
    assert_eq!(store.api_key().await.unwrap(), (rotated, false));
}

async fn email_is_unique_after_normalization(store: Arc<dyn Store>) {
    let a = Person::new("alice");
    a.create_in(store.as_ref()).await;
    let mut b = Person::new("bob");
    b.email = "ALICE@example.org".into();
    let err = store
        .create_account(
            &b.row(),
            &b.email_norm(),
            &b.log.entries[0],
            &b.devices[0].cert,
        )
        .await
        .unwrap_err();
    assert_eq!(err, StoreError::EmailTaken);
    let err = store
        .create_account(
            &a.row(),
            "other@example.org",
            &a.log.entries[0],
            &a.devices[0].cert,
        )
        .await
        .unwrap_err();
    assert_eq!(err, StoreError::AccountExists);
    let c = Person::new("carol");
    let err = store
        .create_account(
            &c.row(),
            &c.email_norm(),
            &c.log.entries[0],
            &a.devices[0].cert,
        )
        .await
        .unwrap_err();
    assert_eq!(err, StoreError::DeviceIdTaken);
    let found = store
        .account_by_email("alice@example.org")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.account, a.public());
    assert_eq!(store.list_accounts().await.unwrap().len(), 1);
}

async fn append_log_is_compare_and_swap(store: Arc<dyn Store>) {
    let mut a = Person::new("alice");
    a.create_in(store.as_ref()).await;
    let (i, entry) = a.add_device();
    let changes = DeviceChanges {
        added: vec![a.devices[i].cert.clone()],
        revoked: vec![],
    };
    let err = store
        .append_log(&a.public(), 5, std::slice::from_ref(&entry), &changes)
        .await
        .unwrap_err();
    assert_eq!(err, StoreError::Conflict { current: 1 });
    let account = a.public();
    let (r1, r2) = tokio::join!(
        store.append_log(&account, 1, std::slice::from_ref(&entry), &changes),
        store.append_log(&account, 1, std::slice::from_ref(&entry), &changes),
    );
    assert_eq!([r1.is_ok(), r2.is_ok()].iter().filter(|ok| **ok).count(), 1);
    assert!(matches!(
        r1.err().or(r2.err()),
        Some(StoreError::Conflict { current: 2 })
    ));
    assert_eq!(store.device_log(&account).await.unwrap(), a.log);
    let devices = store.devices_of(&account).await.unwrap();
    assert_eq!(devices.len(), 2);
    assert!(devices.iter().all(|d| d.active && !d.has_logged_in));
}

async fn revoking_in_the_log_ends_sessions(store: Arc<dyn Store>) {
    let mut a = Person::new("alice");
    a.create_in(store.as_ref()).await;
    let (i, entry) = a.add_device();
    let add = DeviceChanges {
        added: vec![a.devices[i].cert.clone()],
        revoked: vec![],
    };
    store
        .append_log(&a.public(), 1, &[entry], &add)
        .await
        .unwrap();
    let target = a.devices[1].id;
    store
        .create_session([7; 32], target, &a.public(), 100)
        .await
        .unwrap();
    assert!(store.session([7; 32]).await.unwrap().is_some());
    let revoke = a.revoke_by_device(0, 1);
    let changes = DeviceChanges {
        added: vec![],
        revoked: vec![target],
    };
    store
        .append_log(&a.public(), 2, &[revoke], &changes)
        .await
        .unwrap();
    assert_eq!(store.session([7; 32]).await.unwrap(), None);
    let row = store.device(target).await.unwrap().unwrap();
    assert!(!row.active);
    assert_eq!(row.cert, a.devices[1].cert);
}

async fn locking_and_blocking_end_sessions(store: Arc<dyn Store>) {
    let a = Person::new("alice");
    a.create_in(store.as_ref()).await;
    let id = a.devices[0].id;
    store
        .create_session([1; 32], id, &a.public(), 100)
        .await
        .unwrap();
    store.set_locked(&a.public(), true).await.unwrap();
    assert!(store.account(&a.public()).await.unwrap().unwrap().locked);
    assert_eq!(store.session([1; 32]).await.unwrap(), None);
    store.set_locked(&a.public(), false).await.unwrap();
    store
        .create_session([2; 32], id, &a.public(), 100)
        .await
        .unwrap();
    store.set_admin_blocked(id, true).await.unwrap();
    assert!(store.device(id).await.unwrap().unwrap().admin_blocked);
    assert_eq!(store.session([2; 32]).await.unwrap(), None);
    store.mark_logged_in(id, 42).await.unwrap();
    let row = store.device(id).await.unwrap().unwrap();
    assert!(row.has_logged_in);
    assert_eq!(row.last_seen, Some(42));
    store.touch_device(id, 50).await.unwrap();
    assert_eq!(store.device(id).await.unwrap().unwrap().last_seen, Some(50));
    let stranger = Person::new("nobody");
    assert_eq!(
        store.set_locked(&stranger.public(), true).await,
        Err(StoreError::NotFound)
    );
    assert_eq!(
        store.set_admin_blocked(Uuid::new_v4(), true).await,
        Err(StoreError::NotFound)
    );
}

async fn manifest_versions(store: Arc<dyn Store>) {
    let a = Person::new("alice");
    a.create_in(store.as_ref()).await;
    let area = AreaId(Uuid::new_v4());
    let (v2, _) = manifest(&a, area, 2, 1, vec![]);
    assert_eq!(
        store.put_manifest(&a.public(), &v2, None).await,
        Err(StoreError::Conflict { current: 0 })
    );
    let (v1, _) = manifest(&a, area, 1, 1, vec![]);
    store.put_manifest(&a.public(), &v1, None).await.unwrap();
    assert_eq!(store.area(area).await.unwrap().unwrap().manifest, v1);
    assert_eq!(
        store.put_manifest(&a.public(), &v1, None).await,
        Err(StoreError::Conflict { current: 1 })
    );
    let (v3, _) = manifest(&a, area, 3, 1, vec![]);
    assert_eq!(
        store.put_manifest(&a.public(), &v3, Some(1)).await,
        Err(StoreError::Conflict { current: 1 })
    );
    let (v2a, _) = manifest(&a, area, 2, 1, vec![]);
    let (v2b, _) = manifest(&a, area, 2, 1, vec![]);
    let owner = a.public();
    let (r1, r2) = tokio::join!(
        store.put_manifest(&owner, &v2a, Some(1)),
        store.put_manifest(&owner, &v2b, Some(1)),
    );
    assert_eq!([r1.is_ok(), r2.is_ok()].iter().filter(|ok| **ok).count(), 1);
    assert!(matches!(
        r1.err().or(r2.err()),
        Some(StoreError::Conflict { current: 2 })
    ));
    store.delete_area(area).await.unwrap();
    assert_eq!(store.area(area).await.unwrap(), None);
    assert_eq!(store.delete_area(area).await, Err(StoreError::NotFound));
}

async fn members_index_follows_the_manifest(store: Arc<dyn Store>) {
    let a = Person::new("alice");
    let b = Person::new("bob");
    a.create_in(store.as_ref()).await;
    b.create_in(store.as_ref()).await;
    let area = AreaId(Uuid::new_v4());
    let member = Member {
        account: b.public(),
        role: Role::Read,
    };
    let (v1, _) = manifest(&a, area, 1, 1, vec![member]);
    store.put_manifest(&a.public(), &v1, None).await.unwrap();
    assert_eq!(store.areas_of(&b.public()).await.unwrap().len(), 1);
    assert_eq!(store.areas_of(&a.public()).await.unwrap().len(), 1);
    assert_eq!(
        store.area_peers(&a.public()).await.unwrap(),
        vec![b.public()]
    );
    assert_eq!(
        store.area_peers(&b.public()).await.unwrap(),
        vec![a.public()]
    );
    let (v2, _) = manifest(&a, area, 2, 1, vec![]);
    store.put_manifest(&a.public(), &v2, Some(1)).await.unwrap();
    assert!(store.areas_of(&b.public()).await.unwrap().is_empty());
    assert!(store.area_peers(&a.public()).await.unwrap().is_empty());
}

async fn higher_key_version_drops_old_envelopes(store: Arc<dyn Store>) {
    let a = Person::new("alice");
    a.create_in(store.as_ref()).await;
    let area = AreaId(Uuid::new_v4());
    let (v1, key) = manifest(&a, area, 1, 1, vec![]);
    store.put_manifest(&a.public(), &v1, None).await.unwrap();
    let envelope = seal_area_key(&key, area, 1, &a.devices[0].cert).unwrap();
    store
        .put_envelopes(area, std::slice::from_ref(&envelope))
        .await
        .unwrap();
    let device = a.devices[0].id;
    assert_eq!(
        store.envelope_for(area, device).await.unwrap(),
        Some(envelope)
    );
    let (v2, _) = manifest(&a, area, 2, 2, vec![]);
    store.put_manifest(&a.public(), &v2, Some(1)).await.unwrap();
    assert_eq!(store.envelope_for(area, device).await.unwrap(), None);
}

async fn records_are_compare_and_swap_and_numbered(store: Arc<dyn Store>) {
    let a = Person::new("alice");
    a.create_in(store.as_ref()).await;
    let area = AreaId(Uuid::new_v4());
    let (v1, key) = manifest(&a, area, 1, 1, vec![]);
    let missing = record(&a, area, Uuid::new_v4(), 1, &key);
    assert_eq!(store.put_record(&missing).await, Err(StoreError::NotFound));
    store.put_manifest(&a.public(), &v1, None).await.unwrap();
    let id = Uuid::new_v4();
    assert_eq!(
        store.put_record(&record(&a, area, id, 2, &key)).await,
        Err(StoreError::Conflict { current: 0 })
    );
    assert_eq!(
        store.put_record(&record(&a, area, id, 1, &key)).await,
        Ok(1)
    );
    assert_eq!(
        store.put_record(&record(&a, area, id, 2, &key)).await,
        Ok(2)
    );
    assert_eq!(
        store.put_record(&record(&a, area, id, 4, &key)).await,
        Err(StoreError::Conflict { current: 2 })
    );
    let (third_a, third_b) = (record(&a, area, id, 3, &key), record(&a, area, id, 3, &key));
    let (r1, r2) = tokio::join!(store.put_record(&third_a), store.put_record(&third_b));
    assert_eq!([r1.is_ok(), r2.is_ok()].iter().filter(|ok| **ok).count(), 1);
    assert_eq!(r1.clone().ok().or(r2.clone().ok()), Some(3));
    assert!(matches!(
        r1.err().or(r2.err()),
        Some(StoreError::Conflict { current: 3 })
    ));
    let other = Uuid::new_v4();
    assert_eq!(
        store.put_record(&record(&a, area, other, 1, &key)).await,
        Ok(4)
    );
    // A record keeps only its latest write, so `id` now sits at seq 3.
    let since = store.records_since(area, 1, 10).await.unwrap();
    assert_eq!(since.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![3, 4]);
    let later = store.records_since(area, 3, 10).await.unwrap();
    assert_eq!(later.len(), 1);
    assert_eq!(later[0].record.header.record_id, other);
    let all = store.records_since(area, 0, 1).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].record.header.version, 3);
}

async fn account_deletion_follows_the_plan(store: Arc<dyn Store>) {
    let a = Person::new("alice");
    let w = Person::new("writer");
    let r = Person::new("reader");
    for p in [&a, &w, &r] {
        p.create_in(store.as_ref()).await;
    }
    let shared = AreaId(Uuid::new_v4());
    let read_only = AreaId(Uuid::new_v4());
    let personal = AreaId(Uuid::new_v4());
    let both = vec![
        Member {
            account: r.public(),
            role: Role::Read,
        },
        Member {
            account: w.public(),
            role: Role::Write,
        },
    ];
    let reader_only = vec![Member {
        account: r.public(),
        role: Role::Read,
    }];
    for (area, members) in [(shared, both), (read_only, reader_only), (personal, vec![])] {
        let (m, _) = manifest(&a, area, 1, 1, members);
        store.put_manifest(&a.public(), &m, None).await.unwrap();
    }
    let plan = store.plan_account_deletion(&a.public()).await.unwrap();
    assert_eq!(plan.len(), 3);
    let fate = |id: AreaId| plan.iter().find(|f| f.area_id == id).unwrap().clone();
    assert_eq!(fate(shared).outcome, AreaOutcome::TransferTo(w.public()));
    assert_eq!(fate(shared).members, 2);
    assert_eq!(fate(read_only).outcome, AreaOutcome::Delete);
    assert_eq!(fate(personal).outcome, AreaOutcome::Delete);
    assert!(store.account(&a.public()).await.unwrap().is_some());

    let done = store.delete_account(&a.public()).await.unwrap();
    assert_eq!(done, plan);
    assert_eq!(store.account(&a.public()).await.unwrap(), None);
    assert_eq!(store.device(a.devices[0].id).await.unwrap(), None);
    assert_eq!(store.area(shared).await.unwrap().unwrap().owner, w.public());
    assert_eq!(store.area(read_only).await.unwrap(), None);
    assert_eq!(store.area(personal).await.unwrap(), None);
    assert_eq!(store.areas_of(&r.public()).await.unwrap().len(), 1);
    assert_eq!(
        store.delete_account(&a.public()).await,
        Err(StoreError::NotFound)
    );
}

async fn sessions_expire(store: Arc<dyn Store>) {
    let a = Person::new("alice");
    a.create_in(store.as_ref()).await;
    let id = a.devices[0].id;
    store
        .create_session([1; 32], id, &a.public(), 100)
        .await
        .unwrap();
    store
        .create_session([2; 32], id, &a.public(), 200)
        .await
        .unwrap();
    assert_eq!(store.delete_expired_sessions(150).await.unwrap(), 1);
    assert_eq!(store.session([1; 32]).await.unwrap(), None);
    let left = store.session([2; 32]).await.unwrap().unwrap();
    assert_eq!((left.device_id, left.expires_at), (id, 200));
}

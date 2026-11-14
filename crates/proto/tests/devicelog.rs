use sshcraft_proto::ProtoError;
use sshcraft_proto::devicelog::{DeviceLog, LogHead};
use sshcraft_proto::identity::{AccountKey, DeviceCertificate, DeviceKeys};
use uuid::Uuid;

struct Device {
    id: Uuid,
    keys: DeviceKeys,
}

fn device(account: &AccountKey, log: &mut DeviceLog, name: &str) -> Device {
    let keys = DeviceKeys::generate();
    let id = Uuid::new_v4();
    let cert = DeviceCertificate::issue(account, id, keys.public(), name, "windows", 100);
    log.append_add(account, cert, 100);
    Device { id, keys }
}

fn active_ids(log: &DeviceLog, account: &AccountKey) -> Vec<Uuid> {
    let mut ids: Vec<_> = log.verify(&account.public()).unwrap().active().map(|c| c.device_id).collect();
    ids.sort();
    ids
}

fn broken(result: Result<impl Sized, ProtoError>) -> bool {
    matches!(result, Err(ProtoError::LogBroken(_)))
}

#[test]
fn two_devices_added_by_account_are_active() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    let a = device(&account, &mut log, "a");
    let b = device(&account, &mut log, "b");
    let mut want = vec![a.id, b.id];
    want.sort();
    assert_eq!(active_ids(&log, &account), want);
}

#[test]
fn device_can_revoke_another_device() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    let a = device(&account, &mut log, "a");
    let b = device(&account, &mut log, "b");
    log.append_revoke_by_device(a.id, &a.keys, b.id, 200);
    assert_eq!(active_ids(&log, &account), vec![a.id]);
}

#[test]
fn account_can_revoke_when_no_device_is_left() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    let a = device(&account, &mut log, "a");
    log.append_revoke_by_account(&account, a.id, 200);
    assert!(active_ids(&log, &account).is_empty());
    let c = device(&account, &mut log, "new");
    assert_eq!(active_ids(&log, &account), vec![c.id]);
}

#[test]
fn add_signed_by_a_device_is_rejected() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    let a = device(&account, &mut log, "a");
    let keys = DeviceKeys::generate();
    let cert = DeviceCertificate::issue(&account, Uuid::new_v4(), keys.public(), "x", "x", 1);
    log.append_add_signed_by_device_for_test(a.id, &a.keys, cert, 2);
    assert!(broken(log.verify(&account.public())));
}

#[test]
fn add_of_a_certificate_for_another_account_is_rejected() {
    let account = AccountKey::generate();
    let other = AccountKey::generate();
    let mut log = DeviceLog::default();
    let keys = DeviceKeys::generate();
    let foreign = DeviceCertificate::issue(&other, Uuid::new_v4(), keys.public(), "x", "x", 1);
    log.append_add(&account, foreign, 1);
    assert!(broken(log.verify(&account.public())));
}

#[test]
fn revoked_device_cannot_sign_afterwards() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    let a = device(&account, &mut log, "a");
    let b = device(&account, &mut log, "b");
    log.append_revoke_by_device(a.id, &a.keys, b.id, 200);
    log.append_revoke_by_device(b.id, &b.keys, a.id, 201);
    assert!(broken(log.verify(&account.public())));
}

#[test]
fn forged_entry_signature_is_rejected() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    device(&account, &mut log, "a");
    log.entries[0].signature[0] ^= 1;
    assert!(log.verify(&account.public()).is_err());
}

#[test]
fn log_of_another_account_is_rejected() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    device(&account, &mut log, "a");
    assert!(log.verify(&AccountKey::generate().public()).is_err());
}

#[test]
fn reordered_or_gapped_entries_are_rejected() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    device(&account, &mut log, "a");
    device(&account, &mut log, "b");
    device(&account, &mut log, "c");
    let mut swapped = log.clone();
    swapped.entries.swap(0, 1);
    assert!(broken(swapped.verify(&account.public())));
    let mut gapped = log.clone();
    gapped.entries.remove(1);
    assert!(broken(gapped.verify(&account.public())));
}

#[test]
fn changed_prev_hash_is_rejected() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    device(&account, &mut log, "a");
    device(&account, &mut log, "b");
    log.entries[1].prev_hash[0] ^= 1;
    assert!(log.verify(&account.public()).is_err());
}

#[test]
fn shorter_log_than_known_is_a_rollback() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    for name in ["a", "b", "c", "d"] {
        device(&account, &mut log, name);
    }
    let known = log.verify(&account.public()).unwrap().head().unwrap();
    let mut shorter = log.clone();
    shorter.entries.truncate(2);
    assert_eq!(shorter.verify_extends(&account.public(), Some(&known)).err(), Some(ProtoError::Rollback));
}

#[test]
fn different_entry_at_known_seq_is_a_fork() {
    let account = AccountKey::generate();
    let mut base = DeviceLog::default();
    device(&account, &mut base, "a");
    let mut ours = base.clone();
    device(&account, &mut ours, "b");
    let known: LogHead = ours.verify(&account.public()).unwrap().head().unwrap();
    let mut theirs = base;
    device(&account, &mut theirs, "c");
    assert_eq!(theirs.verify_extends(&account.public(), Some(&known)).err(), Some(ProtoError::Fork));
}

#[test]
fn longer_log_extending_known_head_is_accepted() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    device(&account, &mut log, "a");
    let known = log.verify(&account.public()).unwrap().head().unwrap();
    device(&account, &mut log, "b");
    let set = log.verify_extends(&account.public(), Some(&known)).unwrap();
    assert_eq!(set.active().count(), 2);
}

#[test]
fn log_survives_json() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    let a = device(&account, &mut log, "a");
    let b = device(&account, &mut log, "b");
    log.append_revoke_by_device(a.id, &a.keys, b.id, 5);
    let json = serde_json::to_string(&log).unwrap();
    let back: DeviceLog = serde_json::from_str(&json).unwrap();
    assert_eq!(active_ids(&back, &account), vec![a.id]);
}

#[test]
fn validly_signed_entry_from_another_chain_is_rejected() {
    let account = AccountKey::generate();
    let mut ours = DeviceLog::default();
    device(&account, &mut ours, "a");
    device(&account, &mut ours, "b");
    let mut theirs = DeviceLog::default();
    device(&account, &mut theirs, "c");
    // Entry 1 of `ours` is correctly signed, but points to `ours[0]`, not to `theirs[0]`.
    theirs.entries.push(ours.entries[1].clone());
    assert!(broken(theirs.verify(&account.public())));
}

#[test]
fn adding_same_device_twice_is_rejected() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    let keys = DeviceKeys::generate();
    let id = Uuid::new_v4();
    let cert = DeviceCertificate::issue(&account, id, keys.public(), "a", "windows", 100);
    log.append_add(&account, cert.clone(), 100);
    log.append_add(&account, cert, 101);
    assert!(log.verify(&account.public()).is_err());
}

#[test]
fn revoking_unknown_or_revoked_device_is_rejected() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    device(&account, &mut log, "a");
    let mut unknown = log.clone();
    unknown.append_revoke_by_account(&account, Uuid::new_v4(), 200);
    assert!(unknown.verify(&account.public()).is_err());

    let b = device(&account, &mut log, "b");
    log.append_revoke_by_account(&account, b.id, 200);
    assert!(log.verify(&account.public()).is_ok());
    log.append_revoke_by_account(&account, b.id, 201);
    assert!(log.verify(&account.public()).is_err());
}

#[test]
fn verify_extends_with_empty_log_and_known_head_is_rejected() {
    let account = AccountKey::generate();
    let mut seen = DeviceLog::default();
    device(&account, &mut seen, "a");
    let known = seen.verify(&account.public()).unwrap().head().unwrap();
    let empty = DeviceLog::default();
    assert_eq!(empty.verify_extends(&account.public(), Some(&known)).err(), Some(ProtoError::Rollback));
}

#[test]
fn mutual_revocation_keeps_first_in_chain() {
    let account = AccountKey::generate();
    let mut log = DeviceLog::default();
    let a = device(&account, &mut log, "a");
    let b = device(&account, &mut log, "b");
    log.append_revoke_by_device(a.id, &a.keys, b.id, 200);
    let settled = log.clone();
    // B answers by revoking A, but B is already revoked when its entry is chained.
    log.append_revoke_by_device(b.id, &b.keys, a.id, 201);
    assert!(log.verify(&account.public()).is_err());
    assert_eq!(active_ids(&settled, &account), vec![a.id]);
}

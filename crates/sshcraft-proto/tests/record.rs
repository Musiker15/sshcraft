mod common;

use common::World;
use sshcraft_proto::ProtoError;
use sshcraft_proto::area::AreaId;
use sshcraft_proto::record::{Freshness, SealedRecord, VersionTracker};
use uuid::Uuid;

#[test]
fn owner_record_roundtrips() {
    let w = World::new();
    let r = w.seal(&w.owner, 1, b"host data");
    let body = r.open(&w.key, &w.manifest, &w.owner.set()).unwrap();
    assert_eq!(&body[..], b"host data");
}

#[test]
fn writer_member_record_is_accepted() {
    let w = World::new();
    let r = w.seal(&w.writer, 0, b"x");
    assert!(r.open(&w.key, &w.manifest, &w.writer.set()).is_ok());
}

#[test]
fn reader_member_record_is_rejected() {
    let w = World::new();
    let r = w.seal(&w.reader, 0, b"x");
    assert_eq!(
        r.open(&w.key, &w.manifest, &w.reader.set()).err(),
        Some(ProtoError::NotAllowed)
    );
}

#[test]
fn stranger_record_is_rejected() {
    let w = World::new();
    let r = w.seal(&w.stranger, 0, b"x");
    assert_eq!(
        r.open(&w.key, &w.manifest, &w.stranger.set()).err(),
        Some(ProtoError::NotAllowed)
    );
}

#[test]
fn record_from_revoked_device_is_rejected() {
    let mut w = World::new();
    let r = w.seal(&w.owner, 1, b"x");
    w.owner.revoke(1);
    assert_eq!(
        r.open(&w.key, &w.manifest, &w.owner.set()).err(),
        Some(ProtoError::DeviceRevoked)
    );
}

#[test]
fn record_claiming_another_account_is_rejected() {
    let w = World::new();
    // The stranger signs with his own device but claims to be the owner.
    let mut header = w.header(&w.stranger, 0, 1);
    header.author_account = w.owner.public();
    let r = SealedRecord::seal(header, b"x", &w.key, &w.stranger.devices[0].keys);
    // Checked against the stranger's devices: the device does not belong to the claimed account.
    assert!(r.open(&w.key, &w.manifest, &w.stranger.set()).is_err());
    // Checked against the owner's devices: the device is unknown there.
    assert_eq!(
        r.open(&w.key, &w.manifest, &w.owner.set()).err(),
        Some(ProtoError::UnknownDevice)
    );
}

#[test]
fn header_tampering_is_rejected() {
    let w = World::new();
    let r = w.seal(&w.owner, 0, b"x");
    let devices = w.owner.set();

    let mut t = r.clone();
    t.header.record_id = Uuid::new_v4();
    assert_eq!(
        t.open(&w.key, &w.manifest, &devices).err(),
        Some(ProtoError::BadSignature)
    );
    let mut t = r.clone();
    t.header.version += 1;
    assert_eq!(
        t.open(&w.key, &w.manifest, &devices).err(),
        Some(ProtoError::BadSignature)
    );
    let mut t = r.clone();
    t.header.area_id = AreaId(Uuid::new_v4());
    assert_eq!(
        t.open(&w.key, &w.manifest, &devices).err(),
        Some(ProtoError::NotAllowed)
    );
    let mut t = r;
    t.header.key_version += 1;
    assert_eq!(
        t.open(&w.key, &w.manifest, &devices).err(),
        Some(ProtoError::NotAllowed)
    );
}

#[test]
fn ciphertext_tampering_is_rejected() {
    let w = World::new();
    let mut r = w.seal(&w.owner, 0, b"x");
    r.ciphertext[0] ^= 1;
    assert_eq!(
        r.open(&w.key, &w.manifest, &w.owner.set()).err(),
        Some(ProtoError::BadSignature)
    );
}

#[test]
fn signature_of_one_record_does_not_fit_another() {
    let w = World::new();
    let a = w.seal(&w.owner, 0, b"a");
    let mut b = w.seal(&w.owner, 0, b"b");
    b.signature = a.signature;
    assert_eq!(
        b.open(&w.key, &w.manifest, &w.owner.set()).err(),
        Some(ProtoError::BadSignature)
    );
}

#[test]
fn wrong_area_key_fails_to_decrypt() {
    let w = World::new();
    let r = w.seal(&w.owner, 0, b"x");
    let other = sshcraft_proto::area::AreaKey::generate();
    assert_eq!(
        r.open(&other, &w.manifest, &w.owner.set()).err(),
        Some(ProtoError::Decrypt)
    );
}

#[test]
fn tracker_accepts_newer_and_unchanged() {
    let mut t = VersionTracker::default();
    let id = Uuid::new_v4();
    assert_eq!(t.check(id, 1, [1; 32]), Ok(Freshness::New));
    assert_eq!(t.check(id, 1, [1; 32]), Ok(Freshness::Unchanged));
    assert_eq!(t.check(id, 2, [2; 32]), Ok(Freshness::New));
}

#[test]
fn tracker_rejects_rollback() {
    let mut t = VersionTracker::default();
    let id = Uuid::new_v4();
    t.check(id, 5, [5; 32]).unwrap();
    assert_eq!(t.check(id, 4, [4; 32]), Err(ProtoError::Rollback));
}

#[test]
fn tracker_rejects_fork() {
    let mut t = VersionTracker::default();
    let id = Uuid::new_v4();
    t.check(id, 3, [3; 32]).unwrap();
    assert_eq!(t.check(id, 3, [9; 32]), Err(ProtoError::Fork));
}

#[test]
fn tracker_survives_serialization() {
    let mut t = VersionTracker::default();
    let id = Uuid::new_v4();
    t.check(id, 7, [7; 32]).unwrap();
    let mut back: VersionTracker =
        serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
    assert_eq!(back.check(id, 6, [6; 32]), Err(ProtoError::Rollback));
}

#[test]
fn record_digest_tracks_versions() {
    let w = World::new();
    let r = w.seal(&w.owner, 0, b"x");
    let mut t = VersionTracker::default();
    assert_eq!(
        t.check(r.header.record_id, r.header.version, r.digest()),
        Ok(Freshness::New)
    );
    let other = SealedRecord::seal(r.header.clone(), b"y", &w.key, &w.owner.devices[0].keys);
    assert_eq!(
        t.check(other.header.record_id, other.header.version, other.digest()),
        Err(ProtoError::Fork)
    );
}

#[test]
fn sealed_record_survives_json() {
    let w = World::new();
    let r = w.seal(&w.owner, 0, b"json");
    let back: SealedRecord = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(
        &back.open(&w.key, &w.manifest, &w.owner.set()).unwrap()[..],
        b"json"
    );
}

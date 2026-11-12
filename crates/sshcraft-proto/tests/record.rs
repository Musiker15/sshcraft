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

fn sealed(w: &World, record_id: Uuid, version: u64, body: &[u8]) -> SealedRecord {
    let mut header = w.header(&w.owner, 0, version);
    header.record_id = record_id;
    SealedRecord::seal(header, body, &w.key, &w.owner.devices[0].keys)
}

fn accept(t: &mut VersionTracker, w: &World, r: &SealedRecord) -> Result<Freshness, ProtoError> {
    t.accept_record(r, &w.key, &w.manifest, &w.owner.set())
        .map(|(f, _)| f)
}

#[test]
fn tracker_accepts_newer_and_unchanged() {
    let w = World::new();
    let mut t = VersionTracker::default();
    let id = Uuid::new_v4();
    let v1 = sealed(&w, id, 1, b"a");
    assert_eq!(accept(&mut t, &w, &v1), Ok(Freshness::New));
    assert_eq!(accept(&mut t, &w, &v1), Ok(Freshness::Unchanged));
    assert_eq!(
        accept(&mut t, &w, &sealed(&w, id, 2, b"b")),
        Ok(Freshness::New)
    );
}

#[test]
fn accepted_record_returns_its_body() {
    let w = World::new();
    let mut t = VersionTracker::default();
    let r = sealed(&w, Uuid::new_v4(), 1, b"body");
    let (_, body) = t
        .accept_record(&r, &w.key, &w.manifest, &w.owner.set())
        .unwrap();
    assert_eq!(&body[..], b"body");
}

#[test]
fn tracker_rejects_rollback() {
    let w = World::new();
    let mut t = VersionTracker::default();
    let id = Uuid::new_v4();
    accept(&mut t, &w, &sealed(&w, id, 5, b"a")).unwrap();
    assert_eq!(
        accept(&mut t, &w, &sealed(&w, id, 4, b"b")),
        Err(ProtoError::Rollback)
    );
}

#[test]
fn tracker_rejects_fork() {
    let w = World::new();
    let mut t = VersionTracker::default();
    let id = Uuid::new_v4();
    accept(&mut t, &w, &sealed(&w, id, 3, b"a")).unwrap();
    assert_eq!(
        accept(&mut t, &w, &sealed(&w, id, 3, b"other")),
        Err(ProtoError::Fork)
    );
}

#[test]
fn unverified_record_does_not_move_the_tracker() {
    // A forged record at the highest possible version must not block later legitimate updates.
    let w = World::new();
    let mut t = VersionTracker::default();
    let id = Uuid::new_v4();
    let mut forged = sealed(&w, id, u64::MAX, b"x");
    forged.signature[0] ^= 1;
    assert_eq!(accept(&mut t, &w, &forged), Err(ProtoError::BadSignature));
    assert_eq!(
        accept(&mut t, &w, &sealed(&w, id, 2, b"ok")),
        Ok(Freshness::New)
    );
}

#[test]
fn record_ids_and_area_ids_are_tracked_separately() {
    // A writer could pick record_id == area_id; that must not touch the manifest's version.
    let w = World::new();
    let mut t = VersionTracker::default();
    accept(&mut t, &w, &sealed(&w, w.area.0, 99, b"x")).unwrap();
    assert_eq!(
        t.accept_manifest(&w.manifest, &w.owner.set()),
        Ok(Freshness::New)
    );
}

#[test]
fn manifest_tracking_rejects_rollback_and_fork() {
    let w = World::new();
    let mut t = VersionTracker::default();
    let owner = &w.owner;
    let resign = |version: u64, members| {
        sshcraft_proto::area::AreaManifest::sign(
            w.area,
            version,
            w.manifest.key_version,
            &w.key,
            owner.public(),
            members,
            owner.devices[0].id,
            &owner.devices[0].keys,
        )
    };
    assert_eq!(
        t.accept_manifest(&resign(2, vec![]), &owner.set()),
        Ok(Freshness::New)
    );
    assert_eq!(
        t.accept_manifest(&w.manifest, &owner.set()),
        Err(ProtoError::Rollback)
    );
    let other = resign(2, w.manifest.members.clone());
    assert_eq!(
        t.accept_manifest(&other, &owner.set()),
        Err(ProtoError::Fork)
    );
}

#[test]
fn unverified_manifest_does_not_move_the_tracker() {
    let w = World::new();
    let mut t = VersionTracker::default();
    let mut forged = w.manifest.clone();
    forged.version = u64::MAX;
    assert!(t.accept_manifest(&forged, &w.owner.set()).is_err());
    assert_eq!(
        t.accept_manifest(&w.manifest, &w.owner.set()),
        Ok(Freshness::New)
    );
}

#[test]
fn tracker_survives_serialization() {
    let w = World::new();
    let mut t = VersionTracker::default();
    let id = Uuid::new_v4();
    accept(&mut t, &w, &sealed(&w, id, 7, b"a")).unwrap();
    let mut back: VersionTracker =
        serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
    assert_eq!(
        accept(&mut back, &w, &sealed(&w, id, 6, b"b")),
        Err(ProtoError::Rollback)
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

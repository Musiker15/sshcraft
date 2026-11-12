use sshcraft_proto::ProtoError;
use sshcraft_proto::area::{
    AreaId, AreaKey, AreaManifest, KeyEnvelope, open_area_key, seal_area_key,
};
use sshcraft_proto::identity::{AccountKey, DeviceCertificate, DeviceKeys};
use uuid::Uuid;

struct Device {
    keys: DeviceKeys,
    cert: DeviceCertificate,
}

fn device(account: &AccountKey) -> Device {
    let keys = DeviceKeys::generate();
    let cert = DeviceCertificate::issue(account, Uuid::new_v4(), keys.public(), "d", "windows", 1);
    Device { keys, cert }
}

/// An account with two devices and a personal area whose manifest commits to `key`.
struct Setup {
    a: Device,
    b: Device,
    key: AreaKey,
    area: AreaId,
    manifest: AreaManifest,
}

fn setup() -> Setup {
    let account = AccountKey::generate();
    let (a, b) = (device(&account), device(&account));
    let key = AreaKey::generate();
    let area = AreaId(Uuid::new_v4());
    let manifest = AreaManifest::sign(
        area,
        1,
        1,
        &key,
        account.public(),
        vec![],
        a.cert.device_id,
        &a.keys,
    );
    Setup {
        a,
        b,
        key,
        area,
        manifest,
    }
}

fn open(s: &Setup, envelope: &KeyEnvelope, who: &Device) -> Result<AreaKey, ProtoError> {
    open_area_key(envelope, who.cert.device_id, &who.keys, &s.manifest)
}

#[test]
fn sealed_area_key_opens_for_its_device() {
    let s = setup();
    let envelope = seal_area_key(&s.key, s.area, 1, &s.b.cert).unwrap();
    assert!(open(&s, &envelope, &s.b).unwrap().same_key_as(&s.key));
}

#[test]
fn key_planted_by_the_server_is_rejected() {
    // Anyone who knows B's public HPKE key (the server does) can seal an envelope to B.
    let s = setup();
    let planted = AreaKey::generate();
    let envelope = seal_area_key(&planted, s.area, 1, &s.b.cert).unwrap();
    assert_eq!(
        open(&s, &envelope, &s.b).err(),
        Some(ProtoError::WrongAreaKey)
    );
}

#[test]
fn envelope_for_another_area_or_key_version_than_the_manifest_is_not_allowed() {
    let s = setup();
    let other_area = seal_area_key(&s.key, AreaId(Uuid::new_v4()), 1, &s.b.cert).unwrap();
    assert_eq!(
        open(&s, &other_area, &s.b).err(),
        Some(ProtoError::NotAllowed)
    );
    let other_version = seal_area_key(&s.key, s.area, 2, &s.b.cert).unwrap();
    assert_eq!(
        open(&s, &other_version, &s.b).err(),
        Some(ProtoError::NotAllowed)
    );
}

#[test]
fn other_device_cannot_open() {
    let s = setup();
    let envelope = seal_area_key(&s.key, s.area, 1, &s.a.cert).unwrap();
    assert!(open(&s, &envelope, &s.b).is_err());
    assert!(open_area_key(&envelope, s.a.cert.device_id, &s.b.keys, &s.manifest).is_err());
}

#[test]
fn envelope_moved_to_other_area_or_version_fails() {
    let s = setup();
    let envelope = seal_area_key(&s.key, AreaId(Uuid::new_v4()), 7, &s.b.cert).unwrap();
    // Relabel it to match the manifest: the AAD still says the original area and version.
    let mut moved = envelope;
    moved.area_id = s.area;
    moved.key_version = 1;
    assert_eq!(open(&s, &moved, &s.b).err(), Some(ProtoError::Decrypt));
}

#[test]
fn envelope_relabelled_for_other_device_fails() {
    let s = setup();
    let mut envelope = seal_area_key(&s.key, s.area, 1, &s.a.cert).unwrap();
    envelope.device_id = s.b.cert.device_id;
    assert!(open(&s, &envelope, &s.b).is_err());
}

#[test]
fn envelope_for_another_device_id_is_not_allowed() {
    let s = setup();
    let envelope = seal_area_key(&s.key, s.area, 1, &s.a.cert).unwrap();
    assert_eq!(
        open_area_key(&envelope, s.b.cert.device_id, &s.a.keys, &s.manifest).err(),
        Some(ProtoError::NotAllowed)
    );
}

#[test]
fn two_seals_of_the_same_key_differ() {
    let s = setup();
    let one = seal_area_key(&s.key, s.area, 1, &s.b.cert).unwrap();
    let two = seal_area_key(&s.key, s.area, 1, &s.b.cert).unwrap();
    assert_ne!(one.encapped, two.encapped);
    assert_ne!(one.ciphertext, two.ciphertext);
}

#[test]
fn envelope_survives_json() {
    let s = setup();
    let envelope = seal_area_key(&s.key, s.area, 1, &s.b.cert).unwrap();
    let back: KeyEnvelope =
        serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
    assert!(open(&s, &back, &s.b).unwrap().same_key_as(&s.key));
}

#[test]
fn area_key_round_trips_through_bytes() {
    let key = sshcraft_proto::area::AreaKey::generate();
    let again = sshcraft_proto::area::AreaKey::from_bytes(&key.to_bytes());
    assert!(again.same_key_as(&key));
}

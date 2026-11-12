use sshcraft_proto::ProtoError;
use sshcraft_proto::area::{AreaId, AreaKey, open_area_key, seal_area_key};
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

fn area() -> AreaId {
    AreaId(Uuid::new_v4())
}

#[test]
fn sealed_area_key_opens_for_its_device() {
    let account = AccountKey::generate();
    let d = device(&account);
    let key = AreaKey::generate();
    let area = area();
    let envelope = seal_area_key(&key, area, 1, &d.cert).unwrap();
    let opened = open_area_key(&envelope, d.cert.device_id, &d.keys).unwrap();
    assert!(opened.same_key_as(&key));
}

#[test]
fn other_device_cannot_open() {
    let account = AccountKey::generate();
    let (a, b) = (device(&account), device(&account));
    let envelope = seal_area_key(&AreaKey::generate(), area(), 1, &a.cert).unwrap();
    assert!(open_area_key(&envelope, b.cert.device_id, &b.keys).is_err());
    assert!(open_area_key(&envelope, a.cert.device_id, &b.keys).is_err());
}

#[test]
fn envelope_moved_to_other_area_or_version_fails() {
    let account = AccountKey::generate();
    let d = device(&account);
    let envelope = seal_area_key(&AreaKey::generate(), area(), 1, &d.cert).unwrap();
    let mut moved = envelope.clone();
    moved.area_id = area();
    assert_eq!(
        open_area_key(&moved, d.cert.device_id, &d.keys).err(),
        Some(ProtoError::Decrypt)
    );
    let mut bumped = envelope;
    bumped.key_version = 2;
    assert_eq!(
        open_area_key(&bumped, d.cert.device_id, &d.keys).err(),
        Some(ProtoError::Decrypt)
    );
}

#[test]
fn envelope_relabelled_for_other_device_fails() {
    let account = AccountKey::generate();
    let (a, b) = (device(&account), device(&account));
    // Sealed for A, then relabelled as B's: B's id passes the label check, the AAD does not.
    let mut envelope = seal_area_key(&AreaKey::generate(), area(), 1, &a.cert).unwrap();
    envelope.device_id = b.cert.device_id;
    assert!(open_area_key(&envelope, b.cert.device_id, &b.keys).is_err());
    assert!(open_area_key(&envelope, b.cert.device_id, &a.keys).is_err());
}

#[test]
fn envelope_for_another_device_id_is_not_allowed() {
    let account = AccountKey::generate();
    let (a, b) = (device(&account), device(&account));
    let envelope = seal_area_key(&AreaKey::generate(), area(), 1, &a.cert).unwrap();
    assert_eq!(
        open_area_key(&envelope, b.cert.device_id, &a.keys).err(),
        Some(ProtoError::NotAllowed)
    );
}

#[test]
fn two_seals_of_the_same_key_differ() {
    let account = AccountKey::generate();
    let d = device(&account);
    let key = AreaKey::generate();
    let area = area();
    let one = seal_area_key(&key, area, 1, &d.cert).unwrap();
    let two = seal_area_key(&key, area, 1, &d.cert).unwrap();
    assert_ne!(one.encapped, two.encapped);
    assert_ne!(one.ciphertext, two.ciphertext);
}

#[test]
fn envelope_survives_json() {
    let account = AccountKey::generate();
    let d = device(&account);
    let key = AreaKey::generate();
    let envelope = seal_area_key(&key, area(), 3, &d.cert).unwrap();
    let back: sshcraft_proto::area::KeyEnvelope =
        serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
    assert!(
        open_area_key(&back, d.cert.device_id, &d.keys)
            .unwrap()
            .same_key_as(&key)
    );
}

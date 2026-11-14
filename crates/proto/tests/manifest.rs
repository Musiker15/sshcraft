mod common;

use common::World;
use sshcraft_proto::ProtoError;
use sshcraft_proto::area::Role;

#[test]
fn manifest_signed_by_owner_device_verifies() {
    let w = World::new();
    assert!(w.manifest.verify(&w.owner.set()).is_ok());
}

#[test]
fn manifest_signed_by_revoked_device_is_rejected() {
    let mut w = World::new();
    w.owner.revoke(0);
    assert_eq!(w.manifest.verify(&w.owner.set()).err(), Some(ProtoError::DeviceRevoked));
}

#[test]
fn manifest_checked_against_another_accounts_devices_is_rejected() {
    let w = World::new();
    assert!(w.manifest.verify(&w.stranger.set()).is_err());
}

#[test]
fn manifest_with_changed_member_role_is_rejected() {
    let w = World::new();
    let mut m = w.manifest.clone();
    m.members[1].role = Role::Write;
    assert_eq!(m.verify(&w.owner.set()).err(), Some(ProtoError::BadSignature));
}

#[test]
fn manifest_with_changed_version_or_key_version_is_rejected() {
    let w = World::new();
    let mut m = w.manifest.clone();
    m.version += 1;
    assert_eq!(m.verify(&w.owner.set()).err(), Some(ProtoError::BadSignature));
    let mut m = w.manifest.clone();
    m.key_version += 1;
    assert_eq!(m.verify(&w.owner.set()).err(), Some(ProtoError::BadSignature));
}

#[test]
fn write_permission_follows_roles() {
    let w = World::new();
    assert!(w.manifest.can_write(&w.owner.public()));
    assert!(w.manifest.can_write(&w.writer.public()));
    assert!(!w.manifest.can_write(&w.reader.public()));
    assert!(!w.manifest.can_write(&w.stranger.public()));
}

#[test]
fn manifest_with_changed_key_commitment_is_rejected() {
    let w = World::new();
    let mut m = w.manifest.clone();
    m.key_commitment[0] ^= 1;
    assert_eq!(m.verify(&w.owner.set()).err(), Some(ProtoError::BadSignature));
}

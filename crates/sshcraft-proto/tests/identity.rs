use sshcraft_proto::ProtoError;
use sshcraft_proto::identity::{AccountKey, DeviceCertificate, DeviceKeys, safety_number};
use uuid::Uuid;

fn cert(account: &AccountKey, keys: &DeviceKeys) -> DeviceCertificate {
    DeviceCertificate::issue(
        account,
        Uuid::new_v4(),
        keys.public(),
        "Laptop",
        "windows",
        1_700_000_000,
    )
}

#[test]
fn account_key_export_import_roundtrip() {
    let key = AccountKey::generate();
    let text = key.export();
    assert!(text.starts_with("sshcraft-account-1:"));
    assert_eq!(AccountKey::import(&text).unwrap().public(), key.public());
}

#[test]
fn import_tolerates_whitespace_newlines_and_case() {
    let key = AccountKey::generate();
    let text = key.export().to_string();
    let (prefix, body) = text.split_once(':').unwrap();
    let mangled = format!(
        "  {prefix}:\n{}\r\n {}  ",
        body[..10].to_lowercase(),
        &body[10..]
    );
    assert_eq!(AccountKey::import(&mangled).unwrap().public(), key.public());
}

#[test]
fn one_changed_character_fails_the_checksum() {
    let text = AccountKey::generate().export().to_string();
    let start = text.find(':').unwrap() + 1;
    for i in start..text.len() {
        let mut chars: Vec<char> = text.chars().collect();
        chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
        let changed: String = chars.into_iter().collect();
        let result = AccountKey::import(&changed);
        assert!(
            matches!(
                result,
                Err(ProtoError::BadChecksum) | Err(ProtoError::BadAccountKeyFormat)
            ),
            "position {i} was accepted"
        );
    }
}

#[test]
fn wrong_prefix_or_length_is_a_format_error() {
    let text = AccountKey::generate().export().to_string();
    let body = text.split_once(':').unwrap().1;
    assert_eq!(
        AccountKey::import(&format!("sshcraft-account-2:{body}")).err(),
        Some(ProtoError::BadAccountKeyFormat)
    );
    assert_eq!(
        AccountKey::import(&text[..text.len() - 8]).err(),
        Some(ProtoError::BadAccountKeyFormat)
    );
    assert_eq!(
        AccountKey::import("hello").err(),
        Some(ProtoError::BadAccountKeyFormat)
    );
}

#[test]
fn safety_number_is_stable_and_formatted() {
    let key = AccountKey::generate().public();
    let number = safety_number(&key);
    assert_eq!(number, safety_number(&key));
    let groups: Vec<_> = number.split(' ').collect();
    assert_eq!(groups.len(), 6);
    assert!(
        groups
            .iter()
            .all(|g| g.len() == 5 && g.chars().all(|c| c.is_ascii_digit()))
    );
}

#[test]
fn different_keys_have_different_safety_numbers() {
    assert_ne!(
        safety_number(&AccountKey::generate().public()),
        safety_number(&AccountKey::generate().public())
    );
}

#[test]
fn device_keys_roundtrip_bytes() {
    let keys = DeviceKeys::generate();
    let again = DeviceKeys::from_bytes(&keys.to_bytes()).unwrap();
    assert_eq!(again.public(), keys.public());
}

#[test]
fn issued_certificate_verifies() {
    let account = AccountKey::generate();
    assert!(cert(&account, &DeviceKeys::generate()).verify().is_ok());
}

#[test]
fn certificate_with_changed_field_fails() {
    let account = AccountKey::generate();
    let good = cert(&account, &DeviceKeys::generate());
    let other = DeviceKeys::generate().public();

    let mut c = good.clone();
    c.name = "Phone".into();
    assert_eq!(c.verify().err(), Some(ProtoError::BadSignature));
    let mut c = good.clone();
    c.platform = "android".into();
    assert_eq!(c.verify().err(), Some(ProtoError::BadSignature));
    let mut c = good.clone();
    c.created_at += 1;
    assert_eq!(c.verify().err(), Some(ProtoError::BadSignature));
    let mut c = good.clone();
    c.keys.hpke = other.hpke;
    assert_eq!(c.verify().err(), Some(ProtoError::BadSignature));
    let mut c = good;
    c.device_id = Uuid::new_v4();
    assert_eq!(c.verify().err(), Some(ProtoError::BadSignature));
}

#[test]
fn certificate_signed_by_other_account_fails() {
    let mut c = cert(&AccountKey::generate(), &DeviceKeys::generate());
    c.account = AccountKey::generate().public();
    assert_eq!(c.verify().err(), Some(ProtoError::BadSignature));
}

#[test]
fn auth_signature_verifies_only_for_its_challenge_and_device() {
    let account = AccountKey::generate();
    let keys = DeviceKeys::generate();
    let c = cert(&account, &keys);
    let sig = keys.sign_auth(b"challenge-1", c.device_id);
    assert!(c.verify_auth(b"challenge-1", &sig).is_ok());
    assert_eq!(
        c.verify_auth(b"challenge-2", &sig).err(),
        Some(ProtoError::BadSignature)
    );
    let other = cert(&account, &DeviceKeys::generate());
    assert_eq!(
        other.verify_auth(b"challenge-1", &sig).err(),
        Some(ProtoError::BadSignature)
    );
    let mut relabelled = c.clone();
    relabelled.device_id = Uuid::new_v4();
    assert_eq!(
        relabelled.verify_auth(b"challenge-1", &sig).err(),
        Some(ProtoError::BadSignature)
    );
}

#[test]
fn certificate_json_roundtrip() {
    let c = cert(&AccountKey::generate(), &DeviceKeys::generate());
    let json = serde_json::to_string(&c).unwrap();
    let back: DeviceCertificate = serde_json::from_str(&json).unwrap();
    assert!(back.verify().is_ok());
    assert_eq!(back, c);
}

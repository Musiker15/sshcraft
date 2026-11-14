use super::*;
use sha2::{Digest, Sha256};

fn cbor(value: &ciborium::Value) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out).unwrap();
    out
}

fn int(i: i64) -> ciborium::Value {
    ciborium::Value::Integer(i.into())
}

fn es256(x: &[u8], y: &[u8]) -> Vec<u8> {
    use ciborium::Value;
    cbor(&Value::Map(vec![
        (int(1), int(2)),
        (int(3), int(-7)),
        (int(-1), int(1)),
        (int(-2), Value::Bytes(x.to_vec())),
        (int(-3), Value::Bytes(y.to_vec())),
    ]))
}

fn eddsa(x: &[u8]) -> Vec<u8> {
    use ciborium::Value;
    cbor(&Value::Map(vec![
        (int(1), int(1)),
        (int(3), int(-8)),
        (int(-1), int(6)),
        (int(-2), Value::Bytes(x.to_vec())),
    ]))
}

fn p256_point() -> (Vec<u8>, Vec<u8>) {
    use p256::elliptic_curve::Generate;
    let secret = p256::ecdsa::SigningKey::generate_from_rng(&mut rand::rng());
    let point = secret.verifying_key().to_sec1_point(false);
    (point.x().unwrap().to_vec(), point.y().unwrap().to_vec())
}

fn auth_data(application: &str, flags: u8, counter: u32) -> Vec<u8> {
    let mut data = Sha256::digest(application.as_bytes()).to_vec();
    data.push(flags);
    data.extend_from_slice(&counter.to_be_bytes());
    data
}

#[test]
fn cose_parses_es256_and_eddsa() {
    let (x, y) = p256_point();
    let mut sec1 = vec![4];
    sec1.extend_from_slice(&x);
    sec1.extend_from_slice(&y);
    assert_eq!(
        cose::public_key(&es256(&x, &y), SkAlgorithm::EcdsaP256).unwrap(),
        cose::CosePublic::EcdsaP256 { sec1 }
    );
    let ed = [7u8; 32];
    assert_eq!(
        cose::public_key(&eddsa(&ed), SkAlgorithm::Ed25519).unwrap(),
        cose::CosePublic::Ed25519 { public: ed }
    );
}

#[test]
fn cose_rejects_wrong_algorithm_and_short_coordinates() {
    let (x, y) = p256_point();
    // An EdDSA key where ES256 was asked for.
    assert!(cose::public_key(&eddsa(&[7u8; 32]), SkAlgorithm::EcdsaP256).is_err());
    // A coordinate one byte short.
    assert!(cose::public_key(&es256(&x[1..], &y), SkAlgorithm::EcdsaP256).is_err());
    assert!(cose::public_key(&eddsa(&[7u8; 31]), SkAlgorithm::Ed25519).is_err());
    assert!(cose::public_key(b"\xff\x00", SkAlgorithm::Ed25519).is_err());
}

#[test]
fn assertion_with_extensions_is_rejected() {
    let assertion = Assertion {
        authenticator_data: auth_data(SSH_APPLICATION, 0x01 | 0x80, 1),
        signature: vec![0u8; 64],
    };
    assert!(matches!(
        signature::ssh_signature(SkAlgorithm::Ed25519, SSH_APPLICATION, &assertion),
        Err(FidoError::BadResponse(_))
    ));
}

#[test]
fn assertion_with_wrong_rp_hash_is_rejected() {
    let assertion = Assertion {
        authenticator_data: auth_data("https://evil.example", 0x01, 1),
        signature: vec![0u8; 64],
    };
    assert!(matches!(
        signature::ssh_signature(SkAlgorithm::Ed25519, SSH_APPLICATION, &assertion),
        Err(FidoError::BadResponse(_))
    ));
}

#[test]
fn ed25519_blob_has_flags_and_counter() {
    let assertion = Assertion {
        authenticator_data: auth_data(SSH_APPLICATION, 0x05, 0x0102_0304),
        signature: vec![9u8; 64],
    };
    let blob = signature::ssh_signature(SkAlgorithm::Ed25519, SSH_APPLICATION, &assertion).unwrap();
    let alg = b"sk-ssh-ed25519@openssh.com";
    let mut expected = (alg.len() as u32).to_be_bytes().to_vec();
    expected.extend_from_slice(alg);
    expected.extend_from_slice(&64u32.to_be_bytes());
    expected.extend_from_slice(&[9u8; 64]);
    expected.push(0x05);
    expected.extend_from_slice(&[1, 2, 3, 4]);
    assert_eq!(blob, expected);
}

#[tokio::test]
async fn created_key_carries_application_flags_and_handle() {
    let token = soft::SoftAuthenticator::new(Attachment::CrossPlatform);
    for (verify_user, flags) in [(true, 0x05u8), (false, 0x01u8)] {
        let options = CreateOptions {
            algorithm: SkAlgorithm::EcdsaP256,
            attachment: Attachment::CrossPlatform,
            verify_user,
            user_name: "tester".into(),
        };
        let key = create_key(&token, &options, "yubikey").await.unwrap();
        assert!(is_security_key(&key));
        let russh::keys::ssh_key::private::KeypairData::SkEcdsaSha2NistP256(sk) = key.key_data()
        else {
            panic!("expected an sk-ecdsa key");
        };
        assert_eq!(sk.public().application(), SSH_APPLICATION);
        assert_eq!(sk.flags(), flags);
        assert!(!sk.key_handle().is_empty());
        assert_eq!(key.comment().as_str_lossy(), "yubikey");
    }
}

#[test]
fn ordinary_key_is_no_security_key() {
    let key =
        russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
    assert!(!is_security_key(&key));
    assert!(
        SkSigner::new(
            std::sync::Arc::new(soft::SoftAuthenticator::new(Attachment::CrossPlatform)),
            &key,
            Attachment::CrossPlatform
        )
        .is_err()
    );
}

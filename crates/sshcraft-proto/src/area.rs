//! Areas (a person's own data, or a share) and their symmetric keys.
//!
//! Every area has a random key. It is sent to each device that may read the area inside a
//! [`KeyEnvelope`]: HPKE (RFC 9180, base mode, DHKEM(X25519, HKDF-SHA256), HKDF-SHA256,
//! ChaCha20Poly1305) to the device's HPKE key, bound to area, key version and device.

use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::ProtoError;
use crate::encoding::{DOMAIN_ENVELOPE_INFO, b64, signing_bytes};
use crate::identity::{DeviceCertificate, DeviceKeys};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AreaId(pub Uuid);

/// The symmetric key of an area (XChaCha20-Poly1305).
pub struct AreaKey(Zeroizing<[u8; 32]>);

impl std::fmt::Debug for AreaKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AreaKey(..)")
    }
}

impl AreaKey {
    pub fn generate() -> Self {
        let mut bytes = Zeroizing::new([0u8; 32]);
        rand::rng().fill(&mut bytes[..]);
        Self(bytes)
    }

    /// Whether two handles hold the same key (for tests and key rotation checks).
    pub fn same_key_as(&self, other: &AreaKey) -> bool {
        self.0[..] == other.0[..]
    }

    pub(crate) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// An area key sealed to one device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEnvelope {
    pub area_id: AreaId,
    pub key_version: u32,
    pub device_id: Uuid,
    #[serde(with = "b64")]
    pub encapped: Vec<u8>,
    #[serde(with = "b64")]
    pub ciphertext: Vec<u8>,
}

fn envelope_aad(area_id: AreaId, key_version: u32, device_id: Uuid) -> Vec<u8> {
    signing_bytes(
        DOMAIN_ENVELOPE_INFO,
        &[
            area_id.0.as_bytes(),
            &key_version.to_be_bytes(),
            device_id.as_bytes(),
        ],
    )
}

pub fn seal_area_key(
    key: &AreaKey,
    area_id: AreaId,
    key_version: u32,
    recipient: &DeviceCertificate,
) -> Result<KeyEnvelope, ProtoError> {
    let public = <X25519HkdfSha256 as Kem>::PublicKey::from_bytes(&recipient.keys.hpke)
        .map_err(|_| ProtoError::Malformed("device hpke key".into()))?;
    let aad = envelope_aad(area_id, key_version, recipient.device_id);
    let (encapped, ciphertext) =
        hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
            &OpModeS::Base,
            &public,
            DOMAIN_ENVELOPE_INFO.as_bytes(),
            key.bytes(),
            &aad,
        )
        .map_err(|_| ProtoError::Malformed("hpke seal failed".into()))?;
    Ok(KeyEnvelope {
        area_id,
        key_version,
        device_id: recipient.device_id,
        encapped: encapped.to_bytes().to_vec(),
        ciphertext,
    })
}

pub fn open_area_key(
    envelope: &KeyEnvelope,
    device_id: Uuid,
    keys: &DeviceKeys,
) -> Result<AreaKey, ProtoError> {
    if envelope.device_id != device_id {
        return Err(ProtoError::NotAllowed);
    }
    let encapped = <X25519HkdfSha256 as Kem>::EncappedKey::from_bytes(&envelope.encapped)
        .map_err(|_| ProtoError::Decrypt)?;
    let aad = envelope_aad(envelope.area_id, envelope.key_version, envelope.device_id);
    let plain = Zeroizing::new(
        hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
            &OpModeR::Base,
            &keys.hpke_private(),
            &encapped,
            DOMAIN_ENVELOPE_INFO.as_bytes(),
            &envelope.ciphertext,
            &aad,
        )
        .map_err(|_| ProtoError::Decrypt)?,
    );
    let bytes: [u8; 32] = plain[..].try_into().map_err(|_| ProtoError::Decrypt)?;
    Ok(AreaKey(Zeroizing::new(bytes)))
}

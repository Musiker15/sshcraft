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
use crate::devicelog::DeviceSet;
use crate::encoding::{
    DOMAIN_ENVELOPE_INFO, DOMAIN_KEY_CHECK, DOMAIN_MANIFEST, b64, signing_bytes,
};
use crate::identity::{AccountPublic, DeviceCertificate, DeviceKeys};

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

    /// The key's bytes, for storing it sealed on the device (an unfinished key rotation).
    pub fn to_bytes(&self) -> Zeroizing<[u8; 32]> {
        self.0.clone()
    }

    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(Zeroizing::new(*bytes))
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

/// Commitment to an area key, kept in the signed manifest. Envelopes are not authenticated
/// (anyone with a device's public key can seal one), so this is what ties a key to the owner.
pub fn key_commitment(area_id: AreaId, key_version: u32, key: &AreaKey) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(signing_bytes(
        DOMAIN_KEY_CHECK,
        &[
            area_id.0.as_bytes(),
            &key_version.to_be_bytes(),
            key.bytes(),
        ],
    ))
    .into()
}

/// Opens an envelope for this device and checks the key against the owner's commitment in
/// `manifest`, which the caller must already have verified with [`AreaManifest::verify`].
pub fn open_area_key(
    envelope: &KeyEnvelope,
    device_id: Uuid,
    keys: &DeviceKeys,
    manifest: &AreaManifest,
) -> Result<AreaKey, ProtoError> {
    if envelope.device_id != device_id
        || envelope.area_id != manifest.area_id
        || envelope.key_version != manifest.key_version
    {
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
    let key = AreaKey(Zeroizing::new(bytes));
    if key_commitment(manifest.area_id, manifest.key_version, &key) != manifest.key_commitment {
        return Err(ProtoError::WrongAreaKey);
    }
    Ok(key)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub account: AccountPublic,
    pub role: Role,
}

/// Who owns an area and who may read or write it, signed by one of the owner's devices.
/// A personal area has no members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaManifest {
    pub area_id: AreaId,
    pub version: u64,
    pub key_version: u32,
    /// [`key_commitment`] of the area key for `key_version`.
    #[serde(with = "b64")]
    pub key_commitment: [u8; 32],
    pub owner: AccountPublic,
    pub members: Vec<Member>,
    pub author_device: Uuid,
    #[serde(with = "b64")]
    pub signature: [u8; 64],
}

impl AreaManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn sign(
        area_id: AreaId,
        version: u64,
        key_version: u32,
        key: &AreaKey,
        owner: AccountPublic,
        members: Vec<Member>,
        author_device: Uuid,
        keys: &DeviceKeys,
    ) -> Self {
        let mut manifest = Self {
            area_id,
            version,
            key_version,
            key_commitment: key_commitment(area_id, key_version, key),
            owner,
            members,
            author_device,
            signature: [0; 64],
        };
        manifest.signature = keys.sign(&manifest.signed_bytes());
        manifest
    }

    fn signed_bytes(&self) -> Vec<u8> {
        let members: Vec<u8> = self
            .members
            .iter()
            .flat_map(|m| {
                let role: &[u8] = match m.role {
                    Role::Read => b"read",
                    Role::Write => b"write",
                };
                signing_bytes("member", &[&m.account.0, role])
            })
            .collect();
        signing_bytes(
            DOMAIN_MANIFEST,
            &[
                self.area_id.0.as_bytes(),
                &self.version.to_be_bytes(),
                &self.key_version.to_be_bytes(),
                &self.key_commitment,
                &self.owner.0,
                &members,
                self.author_device.as_bytes(),
            ],
        )
    }

    /// `owner_devices` must be the verified device set of `owner`.
    pub fn verify(&self, owner_devices: &DeviceSet) -> Result<(), ProtoError> {
        let device = owner_devices.require_active(self.author_device)?;
        if device.account != self.owner {
            return Err(ProtoError::NotAllowed);
        }
        device.verify_signed(&self.signed_bytes(), &self.signature)
    }

    pub fn can_write(&self, account: &AccountPublic) -> bool {
        *account == self.owner
            || self
                .members
                .iter()
                .any(|m| m.account == *account && m.role == Role::Write)
    }

    /// For [`VersionTracker`](crate::record::VersionTracker).
    pub fn digest(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        Sha256::digest(self.signature).into()
    }
}

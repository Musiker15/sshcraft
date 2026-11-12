//! Account and device identities.
//!
//! The account key is the root of an account. It lives with the user (Proton Pass) and is only
//! on a device while that device is being set up. It signs device certificates. Each device has
//! its own signing key (records, log entries, server login) and its own HPKE key (receiving
//! area keys).

use data_encoding::BASE32_NOPAD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, Serializable};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::ProtoError;
use crate::encoding::{DOMAIN_AUTH, DOMAIN_DEVICE_CERT, DOMAIN_SAFETY, b64, signing_bytes};

const EXPORT_PREFIX: &str = "stollen-account-1:";
const CHECKSUM_LEN: usize = 4;

fn random_32() -> Zeroizing<[u8; 32]> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    rand::rng().fill(&mut bytes[..]);
    bytes
}

/// Verifies an Ed25519 signature made with the public key `public`.
pub(crate) fn verify_ed25519(
    public: &[u8; 32],
    msg: &[u8],
    sig: &[u8; 64],
) -> Result<(), ProtoError> {
    let key = VerifyingKey::from_bytes(public).map_err(|_| ProtoError::BadSignature)?;
    key.verify_strict(msg, &Signature::from_bytes(sig))
        .map_err(|_| ProtoError::BadSignature)
}

/// The secret root key of an account.
pub struct AccountKey {
    signing: SigningKey,
}

impl std::fmt::Debug for AccountKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AccountKey({})", safety_number(&self.public()))
    }
}

impl AccountKey {
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::from_bytes(&random_32()),
        }
    }

    pub fn public(&self) -> AccountPublic {
        AccountPublic(self.signing.verifying_key().to_bytes())
    }

    /// `stollen-account-1:` + Base32 of `seed || SHA-256(seed)[0..4]`, for Proton Pass or a QR code.
    pub fn export(&self) -> Zeroizing<String> {
        let seed = Zeroizing::new(self.signing.to_bytes());
        let mut body = Zeroizing::new(Vec::with_capacity(32 + CHECKSUM_LEN));
        body.extend_from_slice(&seed[..]);
        body.extend_from_slice(&Sha256::digest(&seed[..])[..CHECKSUM_LEN]);
        Zeroizing::new(format!("{EXPORT_PREFIX}{}", BASE32_NOPAD.encode(&body)))
    }

    /// Accepts the exported text with any whitespace and in any letter case.
    pub fn import(text: &str) -> Result<Self, ProtoError> {
        let compact: Zeroizing<String> = Zeroizing::new(
            text.chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>(),
        );
        let lower = Zeroizing::new(compact.to_lowercase());
        let body = lower
            .strip_prefix(EXPORT_PREFIX)
            .ok_or(ProtoError::BadAccountKeyFormat)?;
        let upper = Zeroizing::new(body.to_uppercase());
        let bytes = Zeroizing::new(
            BASE32_NOPAD
                .decode(upper.as_bytes())
                .map_err(|_| ProtoError::BadAccountKeyFormat)?,
        );
        if bytes.len() != 32 + CHECKSUM_LEN {
            return Err(ProtoError::BadAccountKeyFormat);
        }
        let (seed, checksum) = bytes.split_at(32);
        if Sha256::digest(seed)[..CHECKSUM_LEN] != *checksum {
            return Err(ProtoError::BadChecksum);
        }
        let seed: Zeroizing<[u8; 32]> = Zeroizing::new(
            seed.try_into()
                .map_err(|_| ProtoError::BadAccountKeyFormat)?,
        );
        Ok(Self {
            signing: SigningKey::from_bytes(&seed),
        })
    }

    pub(crate) fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.signing.sign(msg).to_bytes()
    }
}

/// The public half of an account key; identifies an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountPublic(#[serde(with = "b64")] pub [u8; 32]);

impl AccountPublic {
    pub fn verify(&self, msg: &[u8], sig: &[u8; 64]) -> Result<(), ProtoError> {
        verify_ed25519(&self.0, msg, sig)
    }
}

/// Six groups of five digits that two people compare to make sure they see the same account.
pub fn safety_number(account: &AccountPublic) -> String {
    let digest = Sha256::digest(signing_bytes(DOMAIN_SAFETY, &[&account.0]));
    digest[..30]
        .chunks(5)
        .map(|chunk| {
            let value = chunk.iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b));
            format!("{:05}", value % 100_000)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The secret keys of one device.
pub struct DeviceKeys {
    signing: SigningKey,
    hpke: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for DeviceKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceKeys(..)")
    }
}

impl DeviceKeys {
    pub fn generate() -> Self {
        let (private, _) = X25519HkdfSha256::gen_keypair();
        let mut hpke = Zeroizing::new([0u8; 32]);
        hpke.copy_from_slice(&private.to_bytes());
        Self {
            signing: SigningKey::from_bytes(&random_32()),
            hpke,
        }
    }

    pub fn public(&self) -> DevicePublic {
        let mut hpke = [0u8; 32];
        hpke.copy_from_slice(&X25519HkdfSha256::sk_to_pk(&self.hpke_private()).to_bytes());
        DevicePublic {
            signing: self.signing.verifying_key().to_bytes(),
            hpke,
        }
    }

    /// Signing seed followed by the HPKE private key.
    pub fn to_bytes(&self) -> Zeroizing<[u8; 64]> {
        let mut out = Zeroizing::new([0u8; 64]);
        out[..32].copy_from_slice(&self.signing.to_bytes());
        out[32..].copy_from_slice(&self.hpke[..]);
        out
    }

    pub fn from_bytes(bytes: &[u8; 64]) -> Result<Self, ProtoError> {
        let mut seed = Zeroizing::new([0u8; 32]);
        seed.copy_from_slice(&bytes[..32]);
        let mut hpke = Zeroizing::new([0u8; 32]);
        hpke.copy_from_slice(&bytes[32..]);
        <X25519HkdfSha256 as Kem>::PrivateKey::from_bytes(&hpke[..])
            .map_err(|_| ProtoError::Malformed("device hpke key".into()))?;
        Ok(Self {
            signing: SigningKey::from_bytes(&seed),
            hpke,
        })
    }

    pub(crate) fn hpke_private(&self) -> <X25519HkdfSha256 as Kem>::PrivateKey {
        <X25519HkdfSha256 as Kem>::PrivateKey::from_bytes(&self.hpke[..])
            .expect("validated when the keys were created")
    }

    pub(crate) fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.signing.sign(msg).to_bytes()
    }

    /// Signature for logging in to a sync server with the challenge it sent.
    pub fn sign_auth(&self, server_challenge: &[u8], device_id: Uuid) -> [u8; 64] {
        self.sign(&signing_bytes(
            DOMAIN_AUTH,
            &[server_challenge, device_id.as_bytes()],
        ))
    }
}

/// The public keys of one device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicePublic {
    #[serde(with = "b64")]
    pub signing: [u8; 32],
    #[serde(with = "b64")]
    pub hpke: [u8; 32],
}

/// A device's public keys, signed by its account key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCertificate {
    pub device_id: Uuid,
    pub account: AccountPublic,
    pub keys: DevicePublic,
    pub name: String,
    pub platform: String,
    pub created_at: i64,
    #[serde(with = "b64")]
    pub signature: [u8; 64],
}

impl DeviceCertificate {
    pub fn issue(
        account: &AccountKey,
        device_id: Uuid,
        keys: DevicePublic,
        name: &str,
        platform: &str,
        created_at: i64,
    ) -> Self {
        let mut cert = Self {
            device_id,
            account: account.public(),
            keys,
            name: name.to_owned(),
            platform: platform.to_owned(),
            created_at,
            signature: [0; 64],
        };
        cert.signature = account.sign(&cert.signed_bytes());
        cert
    }

    pub(crate) fn signed_bytes(&self) -> Vec<u8> {
        signing_bytes(
            DOMAIN_DEVICE_CERT,
            &[
                self.device_id.as_bytes(),
                &self.account.0,
                &self.keys.signing,
                &self.keys.hpke,
                self.name.as_bytes(),
                self.platform.as_bytes(),
                &self.created_at.to_be_bytes(),
            ],
        )
    }

    /// Checks the account's signature over every field.
    pub fn verify(&self) -> Result<(), ProtoError> {
        self.account.verify(&self.signed_bytes(), &self.signature)
    }

    /// Checks a server login signature made by this device.
    pub fn verify_auth(
        &self,
        server_challenge: &[u8],
        signature: &[u8; 64],
    ) -> Result<(), ProtoError> {
        verify_ed25519(
            &self.keys.signing,
            &signing_bytes(DOMAIN_AUTH, &[server_challenge, self.device_id.as_bytes()]),
            signature,
        )
    }

    /// Checks a signature made by this device's signing key.
    pub(crate) fn verify_signed(&self, msg: &[u8], signature: &[u8; 64]) -> Result<(), ProtoError> {
        verify_ed25519(&self.keys.signing, msg, signature)
    }
}

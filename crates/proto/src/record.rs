//! Records: one host, group, key or other item, encrypted with the area key and signed by the
//! writing device. The header stays readable for the server; the body is opaque bytes.

use std::collections::HashMap;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::ProtoError;
use crate::area::{AreaId, AreaKey, AreaManifest};
use crate::devicelog::DeviceSet;
use crate::encoding::{DOMAIN_RECORD, DOMAIN_RECORD_AAD, b64, signing_bytes};
use crate::identity::{AccountPublic, DeviceKeys};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordHeader {
    pub record_id: Uuid,
    pub area_id: AreaId,
    pub version: u64,
    pub key_version: u32,
    pub author_account: AccountPublic,
    pub author_device: Uuid,
}

impl RecordHeader {
    fn aad(&self) -> Vec<u8> {
        signing_bytes(
            DOMAIN_RECORD_AAD,
            &[
                self.record_id.as_bytes(),
                self.area_id.0.as_bytes(),
                &self.version.to_be_bytes(),
                &self.key_version.to_be_bytes(),
                &self.author_account.0,
                self.author_device.as_bytes(),
            ],
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedRecord {
    pub header: RecordHeader,
    #[serde(with = "b64")]
    pub nonce: [u8; 24],
    #[serde(with = "b64")]
    pub ciphertext: Vec<u8>,
    #[serde(with = "b64")]
    pub signature: [u8; 64],
}

fn cipher(key: &AreaKey) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key.bytes().into())
}

impl SealedRecord {
    fn signed_bytes(header: &RecordHeader, nonce: &[u8; 24], ciphertext: &[u8]) -> Vec<u8> {
        signing_bytes(DOMAIN_RECORD, &[&header.aad(), nonce, ciphertext])
    }

    // Cannot fail: XChaCha20-Poly1305 refuses only plaintexts beyond about 256 GiB, which no
    // record in memory reaches.
    #[allow(clippy::expect_used)]
    pub fn seal(header: RecordHeader, body: &[u8], key: &AreaKey, keys: &DeviceKeys) -> Self {
        let mut nonce = [0u8; 24];
        rand::rng().fill(&mut nonce[..]);
        let ciphertext = cipher(key)
            .encrypt(&XNonce::from(nonce), Payload { msg: body, aad: &header.aad() })
            .expect("XChaCha20-Poly1305 encryption does not fail for in-memory buffers");
        let signature = keys.sign(&Self::signed_bytes(&header, &nonce, &ciphertext));
        Self { header, nonce, ciphertext, signature }
    }

    /// Checks area, write permission, author device and signature, then decrypts.
    ///
    /// Preconditions: `manifest` must already have passed [`AreaManifest::verify`], and
    /// `author_devices` must be the verified device set of `header.author_account`. This
    /// function trusts both and does not check them again.
    pub fn open(&self, key: &AreaKey, manifest: &AreaManifest, author_devices: &DeviceSet) -> Result<Zeroizing<Vec<u8>>, ProtoError> {
        let header = &self.header;
        if header.area_id != manifest.area_id || header.key_version != manifest.key_version {
            return Err(ProtoError::NotAllowed);
        }
        if !manifest.can_write(&header.author_account) {
            return Err(ProtoError::NotAllowed);
        }
        let device = author_devices.require_active(header.author_device)?;
        if device.account != header.author_account {
            return Err(ProtoError::NotAllowed);
        }
        device.verify_signed(&Self::signed_bytes(header, &self.nonce, &self.ciphertext), &self.signature)?;
        cipher(key)
            .decrypt(&XNonce::from(self.nonce), Payload { msg: &self.ciphertext, aad: &header.aad() })
            .map(Zeroizing::new)
            .map_err(|_| ProtoError::Decrypt)
    }

    /// For [`VersionTracker`]: two records with the same id and version must have the same digest.
    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.signature).into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    New,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Seen {
    version: u64,
    #[serde(with = "b64")]
    digest: [u8; 32],
}

/// Remembers the highest accepted version per record and per area manifest, so a server cannot
/// serve older or forked states. Records and manifests are only counted after they verified,
/// so nothing unverified can move a counter; the two kinds are tracked separately.
///
/// Rules for clients and servers (see docs/protocol.md): the server only accepts version
/// `previous + 1` for an id (compare and swap), and a client counts its own writes only after
/// the server acknowledged them, so a lost race is a conflict to resolve, not a fork.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionTracker {
    records: HashMap<Uuid, Seen>,
    manifests: HashMap<Uuid, Seen>,
}

fn check(map: &mut HashMap<Uuid, Seen>, id: Uuid, version: u64, digest: [u8; 32]) -> Result<Freshness, ProtoError> {
    match map.get(&id) {
        Some(seen) if version < seen.version => Err(ProtoError::Rollback),
        Some(seen) if version == seen.version && digest != seen.digest => Err(ProtoError::Fork),
        Some(seen) if version == seen.version => Ok(Freshness::Unchanged),
        _ => {
            map.insert(id, Seen { version, digest });
            Ok(Freshness::New)
        }
    }
}

impl VersionTracker {
    /// Opens the record (all checks of [`SealedRecord::open`]) and only then counts it.
    pub fn accept_record(
        &mut self,
        record: &SealedRecord,
        key: &AreaKey,
        manifest: &AreaManifest,
        author_devices: &DeviceSet,
    ) -> Result<(Freshness, Zeroizing<Vec<u8>>), ProtoError> {
        let body = record.open(key, manifest, author_devices)?;
        let freshness = check(&mut self.records, record.header.record_id, record.header.version, record.digest())?;
        Ok((freshness, body))
    }

    /// Verifies the manifest against its owner's devices and only then counts it.
    pub fn accept_manifest(&mut self, manifest: &AreaManifest, owner_devices: &DeviceSet) -> Result<Freshness, ProtoError> {
        manifest.verify(owner_devices)?;
        check(&mut self.manifests, manifest.area_id.0, manifest.version, manifest.digest())
    }
}

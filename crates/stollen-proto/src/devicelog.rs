//! The device log of an account: an append-only, hash-chained list of signed entries that adds
//! and revokes devices. Every client replays it to learn which devices are active, so a server
//! cannot add a device, hide a revocation or reorder entries without being noticed.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::ProtoError;
use crate::encoding::{DOMAIN_LOG_ENTRY, DOMAIN_LOG_HASH, b64, signing_bytes};
use crate::identity::{AccountKey, AccountPublic, DeviceCertificate, DeviceKeys};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogAction {
    Add { certificate: Box<DeviceCertificate> },
    Revoke { device_id: Uuid },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogSigner {
    Account,
    Device { device_id: Uuid },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub seq: u64,
    #[serde(with = "b64")]
    pub prev_hash: [u8; 32],
    pub action: LogAction,
    pub signer: LogSigner,
    pub created_at: i64,
    #[serde(with = "b64")]
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogHead {
    pub seq: u64,
    #[serde(with = "b64")]
    pub hash: [u8; 32],
}

impl LogAction {
    fn canonical(&self) -> Vec<u8> {
        match self {
            LogAction::Add { certificate } => signing_bytes(
                "add",
                &[&certificate.signed_bytes(), &certificate.signature],
            ),
            LogAction::Revoke { device_id } => signing_bytes("revoke", &[device_id.as_bytes()]),
        }
    }
}

impl LogSigner {
    fn canonical(&self) -> Vec<u8> {
        match self {
            LogSigner::Account => signing_bytes("account", &[]),
            LogSigner::Device { device_id } => signing_bytes("device", &[device_id.as_bytes()]),
        }
    }
}

impl LogEntry {
    fn signed_bytes(&self) -> Vec<u8> {
        signing_bytes(
            DOMAIN_LOG_ENTRY,
            &[
                &self.seq.to_be_bytes(),
                &self.prev_hash,
                &self.action.canonical(),
                &self.signer.canonical(),
                &self.created_at.to_be_bytes(),
            ],
        )
    }

    /// Hash over the signed content and the signature; the next entry points to it.
    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(signing_bytes(
            DOMAIN_LOG_HASH,
            &[&self.signed_bytes(), &self.signature],
        ))
        .into()
    }
}

/// The devices that are active after replaying a valid log.
#[derive(Debug, Clone, Default)]
pub struct DeviceSet {
    order: Vec<Uuid>,
    active: HashMap<Uuid, DeviceCertificate>,
    head: Option<LogHead>,
}

impl DeviceSet {
    pub fn active(&self) -> impl Iterator<Item = &DeviceCertificate> {
        self.order.iter().filter_map(|id| self.active.get(id))
    }

    pub fn get_active(&self, id: Uuid) -> Option<&DeviceCertificate> {
        self.active.get(&id)
    }

    /// The last entry; `None` for an empty log.
    pub fn head(&self) -> Option<LogHead> {
        self.head
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceLog {
    pub entries: Vec<LogEntry>,
}

impl DeviceLog {
    fn next(&self) -> (u64, [u8; 32]) {
        match self.entries.last() {
            Some(last) => (last.seq + 1, last.hash()),
            None => (0, [0; 32]),
        }
    }

    fn push(
        &mut self,
        action: LogAction,
        signer: LogSigner,
        created_at: i64,
        sign: impl FnOnce(&[u8]) -> [u8; 64],
    ) {
        let (seq, prev_hash) = self.next();
        let mut entry = LogEntry {
            seq,
            prev_hash,
            action,
            signer,
            created_at,
            signature: [0; 64],
        };
        entry.signature = sign(&entry.signed_bytes());
        self.entries.push(entry);
    }

    /// Adds a device; only the account key may do this (when a device is set up).
    pub fn append_add(&mut self, account: &AccountKey, cert: DeviceCertificate, created_at: i64) {
        self.push(
            LogAction::Add {
                certificate: Box::new(cert),
            },
            LogSigner::Account,
            created_at,
            |m| account.sign(m),
        );
    }

    pub fn append_revoke_by_device(
        &mut self,
        device_id: Uuid,
        keys: &DeviceKeys,
        target: Uuid,
        created_at: i64,
    ) {
        self.push(
            LogAction::Revoke { device_id: target },
            LogSigner::Device { device_id },
            created_at,
            |m| keys.sign(m),
        );
    }

    pub fn append_revoke_by_account(
        &mut self,
        account: &AccountKey,
        target: Uuid,
        created_at: i64,
    ) {
        self.push(
            LogAction::Revoke { device_id: target },
            LogSigner::Account,
            created_at,
            |m| account.sign(m),
        );
    }

    /// Builds an invalid entry (an `Add` signed by a device) so tests can check it is refused.
    #[doc(hidden)]
    pub fn append_add_signed_by_device_for_test(
        &mut self,
        device_id: Uuid,
        keys: &DeviceKeys,
        cert: DeviceCertificate,
        created_at: i64,
    ) {
        self.push(
            LogAction::Add {
                certificate: Box::new(cert),
            },
            LogSigner::Device { device_id },
            created_at,
            |m| keys.sign(m),
        );
    }

    /// Replays the log and returns the active devices, or why the log is not acceptable.
    pub fn verify(&self, account: &AccountPublic) -> Result<DeviceSet, ProtoError> {
        let broken = |why: &str| ProtoError::LogBroken(why.to_owned());
        let mut set = DeviceSet::default();
        let mut seen: HashSet<Uuid> = HashSet::new();
        let mut prev = [0u8; 32];

        for (index, entry) in self.entries.iter().enumerate() {
            if entry.seq != index as u64 {
                return Err(broken("sequence numbers are not contiguous"));
            }
            if entry.prev_hash != prev {
                return Err(broken("entry does not point to its predecessor"));
            }
            let msg = entry.signed_bytes();
            match entry.signer {
                LogSigner::Account => account.verify(&msg, &entry.signature)?,
                LogSigner::Device { device_id } => set
                    .active
                    .get(&device_id)
                    .ok_or_else(|| broken("signer is not an active device"))?
                    .verify_signed(&msg, &entry.signature)?,
            }
            match &entry.action {
                LogAction::Add { certificate } => {
                    if entry.signer != LogSigner::Account {
                        return Err(broken("only the account key may add a device"));
                    }
                    certificate.verify()?;
                    if certificate.account != *account {
                        return Err(broken("certificate belongs to another account"));
                    }
                    if !seen.insert(certificate.device_id) {
                        return Err(broken("device added twice"));
                    }
                    set.order.push(certificate.device_id);
                    set.active
                        .insert(certificate.device_id, (**certificate).clone());
                }
                LogAction::Revoke { device_id } => {
                    if set.active.remove(device_id).is_none() {
                        return Err(broken("revoked device is not active"));
                    }
                }
            }
            prev = entry.hash();
            set.head = Some(LogHead {
                seq: entry.seq,
                hash: prev,
            });
        }
        Ok(set)
    }

    /// Like [`verify`](Self::verify), and the log must contain the head this client saw before:
    /// a shorter log is a rollback, a different entry at that position a fork.
    pub fn verify_extends(
        &self,
        account: &AccountPublic,
        known: Option<&LogHead>,
    ) -> Result<DeviceSet, ProtoError> {
        let set = self.verify(account)?;
        if let Some(known) = known {
            let entry = self
                .entries
                .get(known.seq as usize)
                .ok_or(ProtoError::Rollback)?;
            if entry.hash() != known.hash {
                return Err(ProtoError::Fork);
            }
        }
        Ok(set)
    }
}

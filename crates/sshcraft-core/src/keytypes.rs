//! What kind of key the vault holds and how a new one is made.

use serde::{Deserialize, Serialize};

/// RSA sizes SSHCraft creates. Smaller keys are too weak, larger ones only slow.
pub const RSA_BITS: [u32; 2] = [3072, 4096];

/// The curves SSHCraft creates ECDSA keys on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcdsaCurveName {
    Nistp256,
    Nistp384,
    Nistp521,
}

/// A key to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KeySpec {
    Ed25519,
    Ecdsa { curve: EcdsaCurveName },
    Rsa { bits: u32 },
}

/// Where a key may go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyKind {
    /// Held in the vault; may be synced and shared.
    #[default]
    Vault,
    /// Bound to this device (Windows Hello): never synced, never shared, never exported.
    Device,
}

impl KeyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            KeyKind::Vault => "vault",
            KeyKind::Device => "device",
        }
    }

    pub fn parse(s: &str) -> Option<KeyKind> {
        match s {
            "vault" => Some(KeyKind::Vault),
            "device" => Some(KeyKind::Device),
            _ => None,
        }
    }
}

/// Public facts about the certificate attached to a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CertificateInfo {
    pub key_id: String,
    pub principals: Vec<String>,
    /// Unix seconds.
    pub valid_after: u64,
    /// Unix seconds; `u64::MAX` means "no end".
    pub valid_before: u64,
    pub ca_fingerprint: String,
}

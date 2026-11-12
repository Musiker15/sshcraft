//! The public key in a new credential: authenticator data with attested credential data, its
//! public key as a COSE_Key in CBOR (RFC 9052, RFC 9053).

use ciborium::Value;
use sha2::{Digest, Sha256};

use crate::{FLAG_ATTESTED, FidoError, SkAlgorithm};

const ES256: i64 = -7;
const EDDSA: i64 = -8;
const KTY_OKP: i64 = 1;
const KTY_EC2: i64 = 2;
const CRV_P256: i64 = 1;
const CRV_ED25519: i64 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CosePublic {
    /// Uncompressed SEC1 point: `04 || x || y`.
    EcdsaP256 {
        sec1: Vec<u8>,
    },
    Ed25519 {
        public: [u8; 32],
    },
}

fn bad(what: &str) -> FidoError {
    FidoError::BadResponse(what.to_owned())
}

/// The public key from the authenticator data of a new credential, after checking that it was
/// made for `application`.
pub fn attested_public_key(
    auth_data: &[u8],
    application: &str,
    algorithm: SkAlgorithm,
) -> Result<CosePublic, FidoError> {
    if auth_data.len() < 37 + 16 + 2 {
        return Err(bad("authenticator data too short"));
    }
    if auth_data[..32] != Sha256::digest(application.as_bytes())[..] {
        return Err(bad("credential for another application"));
    }
    if auth_data[32] & FLAG_ATTESTED == 0 {
        return Err(bad("no attested credential data"));
    }
    let id_len = u16::from_be_bytes([auth_data[53], auth_data[54]]) as usize;
    let key = auth_data
        .get(55 + id_len..)
        .ok_or_else(|| bad("credential id too long"))?;
    public_key(key, algorithm)
}

/// Reads a COSE_Key and checks it is the kind asked for.
pub fn public_key(cbor: &[u8], algorithm: SkAlgorithm) -> Result<CosePublic, FidoError> {
    let value: Value = ciborium::from_reader(cbor).map_err(|_| bad("COSE key is no CBOR"))?;
    let Value::Map(entries) = value else {
        return Err(bad("COSE key is no map"));
    };
    let get = |label: i64| {
        entries.iter().find_map(|(k, v)| match k {
            Value::Integer(i) if i128::from(*i) == i128::from(label) => Some(v),
            _ => None,
        })
    };
    let int = |label: i64| match get(label) {
        Some(Value::Integer(i)) => i64::try_from(i128::from(*i)).ok(),
        _ => None,
    };
    let bytes = |label: i64, len: usize| match get(label) {
        Some(Value::Bytes(b)) if b.len() == len => Some(b.clone()),
        _ => None,
    };
    match algorithm {
        SkAlgorithm::EcdsaP256 => {
            if int(1) != Some(KTY_EC2) || int(3) != Some(ES256) || int(-1) != Some(CRV_P256) {
                return Err(bad("not an ES256 P-256 key"));
            }
            let x = bytes(-2, 32).ok_or_else(|| bad("bad x coordinate"))?;
            let y = bytes(-3, 32).ok_or_else(|| bad("bad y coordinate"))?;
            let mut sec1 = Vec::with_capacity(65);
            sec1.push(4);
            sec1.extend_from_slice(&x);
            sec1.extend_from_slice(&y);
            Ok(CosePublic::EcdsaP256 { sec1 })
        }
        SkAlgorithm::Ed25519 => {
            if int(1) != Some(KTY_OKP) || int(3) != Some(EDDSA) || int(-1) != Some(CRV_ED25519) {
                return Err(bad("not an EdDSA Ed25519 key"));
            }
            let x = bytes(-2, 32).ok_or_else(|| bad("bad Ed25519 key"))?;
            let mut public = [0u8; 32];
            public.copy_from_slice(&x);
            Ok(CosePublic::Ed25519 { public })
        }
    }
}

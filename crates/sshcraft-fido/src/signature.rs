//! An authenticator's assertion as an SSH `sk-` signature (OpenSSH `PROTOCOL.u2f`).

use sha2::{Digest, Sha256};

use crate::{Assertion, FLAG_EXTENSIONS, FidoError, SkAlgorithm};

fn bad(what: &str) -> FidoError {
    FidoError::BadResponse(what.to_owned())
}

fn put_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// An SSH `mpint` body: big-endian, no leading zeros, one zero byte if the top bit is set.
fn mpint(bytes: &[u8]) -> Vec<u8> {
    let start = bytes.iter().position(|b| *b != 0).unwrap_or(bytes.len());
    let trimmed = &bytes[start..];
    let mut out = Vec::with_capacity(trimmed.len() + 1);
    if trimmed.first().is_some_and(|b| b & 0x80 != 0) {
        out.push(0);
    }
    out.extend_from_slice(trimmed);
    out
}

/// The SSH signature blob: `string algorithm, string signature, byte flags, uint32 counter`.
/// The authenticator data must be for `application` and carry no extensions: an `sk-`
/// signature has no room for them.
pub fn ssh_signature(
    algorithm: SkAlgorithm,
    application: &str,
    assertion: &Assertion,
) -> Result<Vec<u8>, FidoError> {
    let data = &assertion.authenticator_data;
    if data.len() < 37 {
        return Err(bad("authenticator data too short"));
    }
    if data[..32] != Sha256::digest(application.as_bytes())[..] {
        return Err(bad("signature for another application"));
    }
    let flags = data[32];
    if flags & FLAG_EXTENSIONS != 0 {
        return Err(bad("extensions in the authenticator data"));
    }
    let counter = &data[33..37];
    let mut out = Vec::new();
    match algorithm {
        SkAlgorithm::EcdsaP256 => {
            let signature = p256::ecdsa::Signature::from_der(&assertion.signature)
                .map_err(|_| bad("ECDSA signature is no DER"))?;
            let (r, s) = signature.split_bytes();
            let mut inner = Vec::new();
            put_string(&mut inner, &mpint(&r));
            put_string(&mut inner, &mpint(&s));
            put_string(&mut out, b"sk-ecdsa-sha2-nistp256@openssh.com");
            put_string(&mut out, &inner);
        }
        SkAlgorithm::Ed25519 => {
            if assertion.signature.len() != 64 {
                return Err(bad("Ed25519 signature is not 64 bytes"));
            }
            put_string(&mut out, b"sk-ssh-ed25519@openssh.com");
            put_string(&mut out, &assertion.signature);
        }
    }
    out.push(flags);
    out.extend_from_slice(counter);
    Ok(out)
}

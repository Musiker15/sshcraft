//! Canonical bytes for signatures, domain constants and Base64 helpers for serde.

pub const DOMAIN_DEVICE_CERT: &str = "sshcraft/device-cert/v1";
pub const DOMAIN_LOG_ENTRY: &str = "sshcraft/device-log/v1";
pub const DOMAIN_MANIFEST: &str = "sshcraft/area-manifest/v1";
pub const DOMAIN_RECORD: &str = "sshcraft/record/v1";
pub const DOMAIN_RECORD_AAD: &str = "sshcraft/record-aad/v1";
pub const DOMAIN_ENVELOPE_INFO: &str = "sshcraft/area-key/v1";
pub const DOMAIN_SAFETY: &str = "sshcraft/safety-number/v1";
pub const DOMAIN_LOG_HASH: &str = "sshcraft/device-log-hash/v1";
pub const DOMAIN_AUTH: &str = "sshcraft/server-auth/v1";
pub const DOMAIN_KEY_CHECK: &str = "sshcraft/area-key-check/v1";
pub const DOMAIN_RECOVERY_DEVICE: &str = "sshcraft/recovery-device/v1";
pub const DOMAIN_RECOVERY_ID: &str = "sshcraft/recovery-device-id/v1";

/// The domain, then every field as a big-endian `u32` length followed by its bytes.
/// Field boundaries stay unambiguous, also for empty fields.
pub(crate) fn signing_bytes(domain: &str, fields: &[&[u8]]) -> Vec<u8> {
    let total = domain.len() + 4 + fields.iter().map(|f| 4 + f.len()).sum::<usize>();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&(domain.len() as u32).to_be_bytes());
    out.extend_from_slice(domain.as_bytes());
    for field in fields {
        out.extend_from_slice(&(field.len() as u32).to_be_bytes());
        out.extend_from_slice(field);
    }
    out
}

/// Base64 (URL alphabet, no padding) for byte fields in JSON.
pub mod b64 {
    use data_encoding::BASE64URL_NOPAD;
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer, T: AsRef<[u8]>>(bytes: &T, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&BASE64URL_NOPAD.encode(bytes.as_ref()))
    }

    pub fn deserialize<'de, D, T>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: TryFrom<Vec<u8>>,
    {
        let text = String::deserialize(d)?;
        let bytes = BASE64URL_NOPAD
            .decode(text.as_bytes())
            .map_err(D::Error::custom)?;
        T::try_from(bytes).map_err(|_| D::Error::custom("wrong length"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_boundaries_are_unambiguous() {
        assert_ne!(
            signing_bytes("d", &[b"ab", b"c"]),
            signing_bytes("d", &[b"a", b"bc"])
        );
    }

    #[test]
    fn empty_fields_are_distinct() {
        let a = signing_bytes("d", &[b"", b"x"]);
        let b = signing_bytes("d", &[b"x", b""]);
        let c = signing_bytes("d", &[b"x"]);
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn domain_separates() {
        assert_ne!(signing_bytes("a", &[b"x"]), signing_bytes("b", &[b"x"]));
    }
}

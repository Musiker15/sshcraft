//! Time-based one-time passwords (RFC 6238): HMAC-SHA-1, 30 seconds, 6 digits, the setting
//! every authenticator app and `google-authenticator` on servers use. The secret comes as Base32,
//! alone or in an `otpauth://` link.

use hmac::{Hmac, KeyInit, Mac};

/// The longest secret taken (Base32 characters).
pub const MAX_SECRET: usize = 256;

/// The code for `secret` at `unix` seconds.
pub fn code(secret: &[u8], unix: u64) -> String {
    let counter = (unix / 30).to_be_bytes();
    let Ok(mut mac) = <Hmac<sha1::Sha1> as KeyInit>::new_from_slice(secret) else {
        return String::new();
    };
    mac.update(&counter);
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest.get(19).copied().unwrap_or(0) & 0x0f);
    let word = digest.get(offset..offset + 4).map_or(0, |b| u32::from_be_bytes([b[0] & 0x7f, b[1], b[2], b[3]]));
    format!("{:06}", word % 1_000_000)
}

/// The secret bytes from Base32 (spaces, `-` and padding allowed, any case) or from an
/// `otpauth://totp/…?secret=…` link; `None` when it is not one.
pub fn decode_secret(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    let base32 = match text.strip_prefix("otpauth://") {
        Some(rest) => {
            let query = rest.split_once('?')?.1;
            // Only the setting the codes are made with: SHA-1, 6 digits, 30 seconds. A link that
            // asks for another would give wrong codes, so it is refused.
            for (name, value) in query.split('&').filter_map(|p| p.split_once('=')) {
                let fine = match name.to_ascii_lowercase().as_str() {
                    "algorithm" => value.eq_ignore_ascii_case("SHA1"),
                    "digits" => value == "6",
                    "period" => value == "30",
                    _ => true,
                };
                if !fine {
                    return None;
                }
            }
            query.split('&').find_map(|p| p.strip_prefix("secret="))?
        }
        None => text,
    };
    let clean: String = base32.chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '=').map(|c| c.to_ascii_uppercase()).collect();
    if clean.is_empty() || clean.len() > MAX_SECRET {
        return None;
    }
    let mut out = Vec::with_capacity(clean.len() * 5 / 8);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for c in clean.chars() {
        let value = match c {
            'A'..='Z' => u32::from(c) - u32::from('A'),
            '2'..='7' => u32::from(c) - u32::from('2') + 26,
            _ => return None,
        };
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buffer >> bits) & 0xff).ok()?);
        }
        buffer &= (1 << bits) - 1;
    }
    (!out.is_empty()).then_some(out)
}

/// Whether a keyboard-interactive question asks for a one-time code (and not for a password).
pub fn is_code_prompt(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    if p.contains("password") && !p.contains("one-time password") {
        return false;
    }
    ["verification code", "code", "token", "otp", "one-time", "authenticator", "2fa", "two-factor"].iter().any(|w| p.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SHA-1 test vectors of RFC 6238, appendix B (8 digits there; the last 6 here).
    #[test]
    fn rfc_6238_vectors() {
        // "12345678901234567890", the secret of the RFC, made rather than written out.
        let secret: Vec<u8> = (1..=20u8).map(|i| b'0' + i % 10).collect();
        let secret = secret.as_slice();
        for (time, eight) in
            [(59u64, "94287082"), (1_111_111_109, "07081804"), (1_111_111_111, "14050471"), (1_234_567_890, "89005924"), (2_000_000_000, "69279037")]
        {
            assert_eq!(code(secret, time), eight[2..], "t = {time}");
        }
    }

    #[test]
    fn base32_and_links_decode() {
        // "12345678901234567890" in Base32.
        let b32 = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        assert_eq!(decode_secret(b32).unwrap(), b"12345678901234567890");
        assert_eq!(decode_secret("gezd gnbv gy3t qojq gezd gnbv gy3t qojq").unwrap(), b"12345678901234567890");
        assert_eq!(decode_secret(&format!("otpauth://totp/me@web?secret={b32}&issuer=web")).unwrap(), b"12345678901234567890");
        assert!(decode_secret("not base32 1890!").is_none());
        assert!(decode_secret(&format!("otpauth://totp/x?secret={b32}&algorithm=SHA1&digits=6&period=30")).is_some());
        for other in ["algorithm=SHA256", "digits=8", "period=60"] {
            assert!(decode_secret(&format!("otpauth://totp/x?secret={b32}&{other}")).is_none(), "{other}");
        }
        assert!(decode_secret("").is_none());
        assert!(decode_secret(&"A".repeat(MAX_SECRET + 1)).is_none());
    }

    #[test]
    fn code_prompts_are_told_from_password_prompts() {
        for p in ["Verification code: ", "One-time password (OATH) for `me':", "Token:", "2FA code", "Authenticator code:"] {
            assert!(is_code_prompt(p), "{p}");
        }
        for p in ["Password: ", "me@web's password:", "Passphrase for key:"] {
            assert!(!is_code_prompt(p), "{p}");
        }
    }
}

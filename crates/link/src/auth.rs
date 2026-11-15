//! Signing in by challenge. The client sends a fresh nonce; the server answers with its own
//! nonce and a proof; the client checks that proof before it sends its own. Both proofs are
//! lowercase hex HMAC-SHA256, keyed by the token's 64 hex characters as ASCII, over
//! `<label>\n<clientNonce>\n<serverNonce>`. A listener that does not hold the token learns
//! nothing derived from it, and the token never crosses the wire.

use std::io::Write;
use std::path::Path;

use data_encoding::HEXLOWER;
use hmac::{Hmac, KeyInit, Mac};
use rand::RngExt;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::LinkError;

pub const SERVER_LABEL: &str = "sshcraft-link-v1 server";
pub const CLIENT_LABEL: &str = "sshcraft-link-v1 client";

/// 256 random bits as 64 lowercase hex characters.
#[derive(Clone)]
pub struct Token(Zeroizing<String>);

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(..)")
    }
}

fn is_hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Token {
    pub fn generate() -> Self {
        let mut bytes = Zeroizing::new([0u8; 32]);
        rand::rng().fill(&mut bytes[..]);
        Self(Zeroizing::new(HEXLOWER.encode(&bytes[..])))
    }

    /// A token from its text; `None` unless it is 64 lowercase hex characters.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        is_hex64(text).then(|| Self(Zeroizing::new(text.to_string())))
    }

    /// The token's text, for its file and for tests; never for a log or an answer.
    pub fn hex(&self) -> &str {
        &self.0
    }

    /// The token in `path`; `None` when the file is missing or holds no token. Clients read it
    /// this way: only the app makes the file.
    pub fn load(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        // A regular file: a pipe or device in its place would block the reader.
        if !meta.is_file() || meta.len() > 1024 {
            return None;
        }
        Self::parse(&std::fs::read_to_string(path).ok()?)
    }

    /// A fresh token in `path`, replacing whatever was there: a token for this start only.
    pub fn create_fresh(path: &Path) -> Result<Self, LinkError> {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(LinkError::Io(format!("{}: {e}", path.display()))),
        }
        // Made new, never a file that appeared in between: that would be someone else's token.
        Self::create(path)
    }

    /// A fresh token in a new file at `path`; an error when a file is there.
    fn create(path: &Path) -> Result<Self, LinkError> {
        let token = Self::generate();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| LinkError::Io(format!("{}: {e}", dir.display())))?;
        }
        let mut file = private_file(path).map_err(|e| LinkError::Io(format!("{}: {e}", path.display())))?;
        file.write_all(token.hex().as_bytes()).map_err(|e| LinkError::Io(format!("{}: {e}", path.display())))?;
        Ok(token)
    }

    /// The token in `path`, made (with a fresh token, only the owner may read it) when the file
    /// is missing. A file that holds no token is an error: deleting it gives a new one.
    pub fn load_or_create(path: &Path) -> Result<Self, LinkError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).ok_or_else(|| LinkError::Io(format!("{} holds no token; delete it to get a new one", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::create(path),
            Err(e) => Err(LinkError::Io(format!("{}: {e}", path.display()))),
        }
    }
}

/// A new file only its owner may read: mode 0600 on Unix. On Windows the folder it lives in
/// (the user's profile) admits only the user, SYSTEM and administrators.
fn private_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// A fresh nonce: 32 random bytes as 64 lowercase hex characters.
pub fn nonce() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes[..]);
    HEXLOWER.encode(&bytes)
}

/// Whether `text` is a nonce as [`nonce`] makes them.
pub fn valid_nonce(text: &str) -> bool {
    is_hex64(text)
}

// Cannot fail: HMAC takes a key of any length.
#[allow(clippy::expect_used)]
fn proof(token: &Token, label: &str, client_nonce: &str, server_nonce: &str) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(token.hex().as_bytes()).expect("any key length");
    mac.update(format!("{label}\n{client_nonce}\n{server_nonce}").as_bytes());
    HEXLOWER.encode(&mac.finalize().into_bytes())
}

/// The server's proof that it holds `token`.
pub fn server_proof(token: &Token, client_nonce: &str, server_nonce: &str) -> String {
    proof(token, SERVER_LABEL, client_nonce, server_nonce)
}

/// The client's proof that it holds `token`.
pub fn client_proof(token: &Token, client_nonce: &str, server_nonce: &str) -> String {
    proof(token, CLIENT_LABEL, client_nonce, server_nonce)
}

/// Compares two proofs in constant time.
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

//! Puts a public key into `~/.ssh/authorized_keys` on a server, with one `sh -c` command.
//!
//! The line is built from the key itself, never from text the user typed: the key type and its
//! Base64 data, a comment cut down to harmless characters, and `verify-required` in front when
//! the key asks for a PIN. Inside the script these values only ever stand in double quotes, and
//! none of their characters means anything there.

use russh::keys::PublicKey;
use serde::Serialize;

const ADDED: &str = "SSHCRAFT_ADDED";
const PRESENT: &str = "SSHCRAFT_PRESENT";
const MAX_COMMENT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallOutcome {
    Added,
    AlreadyPresent,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthorizedKeysError {
    /// The server did not run the script (no `sh`, a Windows server, a forced command).
    #[error("the server did not run the install script")]
    Unsupported,
}

/// The command that adds `public_key` to `authorized_keys`, unless it is there already (same type
/// and data, whatever options or comment the line has).
pub fn install_command(public_key: &PublicKey, verify_required: bool) -> String {
    let mut bare = public_key.clone();
    bare.set_comment("");
    let body = bare.to_openssh().unwrap_or_default().trim().to_owned();
    let comment: String = public_key
        .comment()
        .as_str_lossy()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '-'))
        .take(MAX_COMMENT)
        .collect();
    let mut line = String::new();
    if verify_required {
        line.push_str("verify-required ");
    }
    line.push_str(&body);
    if !comment.is_empty() {
        line.push(' ');
        line.push_str(&comment);
    }
    format!(
        "sh -c 'umask 077; d=\"$HOME/.ssh\"; f=\"$d/authorized_keys\"; \
         mkdir -p \"$d\" && touch \"$f\" || exit 1; \
         if grep -qF \"{body}\" \"$f\"; then echo {PRESENT}; exit 0; fi; \
         if [ -s \"$f\" ] && [ -n \"$(tail -c 1 \"$f\")\" ]; then echo >> \"$f\"; fi; \
         printf \"%s\\n\" \"{line}\" >> \"$f\" && echo {ADDED}'"
    )
}

/// Reads what the script printed. Anything without one of its marks means it did not run.
pub fn parse_outcome(output: &[u8]) -> Result<InstallOutcome, AuthorizedKeysError> {
    let text = String::from_utf8_lossy(output);
    text.lines()
        .rev()
        .map(str::trim)
        .find_map(|line| match line {
            ADDED => Some(InstallOutcome::Added),
            PRESENT => Some(InstallOutcome::AlreadyPresent),
            _ => None,
        })
        .ok_or(AuthorizedKeysError::Unsupported)
}

#[cfg(test)]
#[path = "authorized_keys_tests.rs"]
mod tests;

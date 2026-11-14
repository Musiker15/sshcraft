//! Pure decisions about host keys and reconnecting, kept free of I/O so they are easy to test.

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    /// The presented key matches the stored one.
    Trusted,
    /// Nothing stored yet: the user has to decide.
    Unknown,
    /// A different key is stored. Never connect.
    Mismatch { expected: String },
}

pub fn check_host_key(known: Option<&str>, presented: &str) -> HostKeyStatus {
    match known {
        None => HostKeyStatus::Unknown,
        Some(k) if k == presented => HostKeyStatus::Trusted,
        Some(k) => HostKeyStatus::Mismatch { expected: k.to_owned() },
    }
}

/// Why a session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disconnect {
    /// The remote shell exited on its own (`exit`, logout).
    Exited(Option<u32>),
    /// The connection dropped (network change, sleep, server gone).
    Lost(String),
    HostKeyRejected,
    AuthFailed,
}

/// What a terminal session (SSH, local or serial) reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    /// Raw bytes from the terminal, not decoded.
    Data(Vec<u8>),
    Ended(Disconnect),
}

/// Delay before reconnect attempt `attempt` (1-based), or `None` to give up.
/// Only a lost connection is retried.
pub fn next_reconnect_delay(reason: &Disconnect, attempt: u32) -> Option<Duration> {
    if !matches!(reason, Disconnect::Lost(_)) {
        return None;
    }
    let secs = match attempt {
        1 => 1,
        2 => 2,
        3 => 5,
        4 | 5 => 10,
        _ => return None,
    };
    Some(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_when_nothing_stored() {
        assert_eq!(check_host_key(None, "SHA256:a"), HostKeyStatus::Unknown);
    }

    #[test]
    fn trusted_when_equal() {
        assert_eq!(check_host_key(Some("SHA256:a"), "SHA256:a"), HostKeyStatus::Trusted);
    }

    #[test]
    fn mismatch_when_different() {
        assert_eq!(check_host_key(Some("SHA256:a"), "SHA256:b"), HostKeyStatus::Mismatch { expected: "SHA256:a".into() });
    }

    #[test]
    fn reconnect_delays_for_lost_connection() {
        let lost = Disconnect::Lost("reset".into());
        let delays: Vec<_> = (1..=6).map(|a| next_reconnect_delay(&lost, a)).collect();
        let secs = |s| Some(Duration::from_secs(s));
        assert_eq!(delays, vec![secs(1), secs(2), secs(5), secs(10), secs(10), None]);
    }

    #[test]
    fn no_reconnect_after_exit() {
        assert_eq!(next_reconnect_delay(&Disconnect::Exited(Some(0)), 1), None);
    }

    #[test]
    fn no_reconnect_after_host_key_rejected() {
        assert_eq!(next_reconnect_delay(&Disconnect::HostKeyRejected, 1), None);
    }

    #[test]
    fn no_reconnect_after_auth_failed() {
        assert_eq!(next_reconnect_delay(&Disconnect::AuthFailed, 1), None);
    }
}

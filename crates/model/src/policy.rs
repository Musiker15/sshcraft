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

/// Checks a presented fingerprint against the ones known for the address. A server may have
/// several keys (one per algorithm); any of them is trusted. A key beside known ones is a
/// mismatch, not a new key: someone else may be answering.
pub fn check_host_key(known: &[String], presented: &str) -> HostKeyStatus {
    if known.is_empty() {
        HostKeyStatus::Unknown
    } else if known.iter().any(|k| k == presented) {
        HostKeyStatus::Trusted
    } else {
        HostKeyStatus::Mismatch { expected: known.join(", ") }
    }
}

/// The most host keys taken from one announcement (`UpdateHostKeys`).
pub const MAX_ANNOUNCED_KEYS: usize = 16;

/// What an `UpdateHostKeys` announcement changes: fingerprints to add (after the server proved
/// it holds them) and fingerprints to drop.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyUpdate {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// Plans an `UpdateHostKeys` announcement the way OpenSSH does: only when the key this
/// connection was verified with is among the announced ones (else the announcement says
/// nothing about this server), new keys are added and keys no longer announced are dropped.
/// `None` when nothing changes.
pub fn plan_key_update(known: &[String], announced: &[String], used: &str) -> Option<KeyUpdate> {
    if announced.len() > MAX_ANNOUNCED_KEYS || !announced.iter().any(|a| a == used) || !known.iter().any(|k| k == used) {
        return None;
    }
    let mut add: Vec<String> = announced.iter().filter(|a| !known.contains(a)).cloned().collect();
    add.dedup();
    let remove: Vec<String> = known.iter().filter(|k| !announced.contains(k)).cloned().collect();
    (!add.is_empty() || !remove.is_empty()).then_some(KeyUpdate { add, remove })
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

/// The longest wait between two reconnect attempts.
pub const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

/// Delay before reconnect attempt `attempt` (1-based), or `None` to give up: 1, 2, 4, 8, 16,
/// 32 seconds, then every minute, for at most `limit` attempts (`None`: without end, for port
/// forwarding). Only a lost connection is retried.
pub fn next_reconnect_delay(reason: &Disconnect, attempt: u32, limit: Option<u32>) -> Option<Duration> {
    if !matches!(reason, Disconnect::Lost(_)) || attempt == 0 || limit.is_some_and(|limit| attempt > limit) {
        return None;
    }
    let secs = 1u64.checked_shl(attempt - 1).unwrap_or(u64::MAX);
    Some(Duration::from_secs(secs).min(MAX_RECONNECT_DELAY))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fps(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn unknown_when_nothing_stored() {
        assert_eq!(check_host_key(&[], "SHA256:a"), HostKeyStatus::Unknown);
    }

    #[test]
    fn trusted_when_equal() {
        assert_eq!(check_host_key(&fps(&["SHA256:a"]), "SHA256:a"), HostKeyStatus::Trusted);
    }

    #[test]
    fn mismatch_when_different() {
        assert_eq!(check_host_key(&fps(&["SHA256:a"]), "SHA256:b"), HostKeyStatus::Mismatch { expected: "SHA256:a".into() });
    }

    #[test]
    fn a_matching_key_among_several_is_trusted() {
        assert_eq!(check_host_key(&fps(&["SHA256:rsa", "SHA256:ed"]), "SHA256:ed"), HostKeyStatus::Trusted);
    }

    #[test]
    fn an_unknown_key_beside_known_ones_is_a_mismatch() {
        assert!(matches!(check_host_key(&fps(&["SHA256:rsa", "SHA256:ed"]), "SHA256:new"), HostKeyStatus::Mismatch { .. }));
    }

    #[test]
    fn an_announcement_adds_new_keys_and_drops_old_ones() {
        let plan = plan_key_update(&fps(&["SHA256:ed", "SHA256:old"]), &fps(&["SHA256:ed", "SHA256:new"]), "SHA256:ed");
        assert_eq!(plan, Some(KeyUpdate { add: fps(&["SHA256:new"]), remove: fps(&["SHA256:old"]) }));
    }

    #[test]
    fn an_announcement_without_the_used_key_changes_nothing() {
        assert_eq!(plan_key_update(&fps(&["SHA256:ed"]), &fps(&["SHA256:new"]), "SHA256:ed"), None);
        assert_eq!(plan_key_update(&fps(&["SHA256:x"]), &fps(&["SHA256:ed"]), "SHA256:ed"), None, "the used key must be a known one");
    }

    #[test]
    fn the_same_keys_change_nothing() {
        assert_eq!(plan_key_update(&fps(&["SHA256:ed"]), &fps(&["SHA256:ed"]), "SHA256:ed"), None);
    }

    #[test]
    fn too_many_announced_keys_change_nothing() {
        let many: Vec<String> = (0..=MAX_ANNOUNCED_KEYS).map(|i| format!("SHA256:{i}")).collect();
        assert_eq!(plan_key_update(&fps(&["SHA256:0"]), &many, "SHA256:0"), None);
    }

    #[test]
    fn backoff_doubles_up_to_a_minute() {
        let lost = Disconnect::Lost("reset".into());
        let delays: Vec<_> = (1..=9).map(|a| next_reconnect_delay(&lost, a, None)).collect();
        let secs = |s| Some(Duration::from_secs(s));
        assert_eq!(delays, vec![secs(1), secs(2), secs(4), secs(8), secs(16), secs(32), secs(60), secs(60), secs(60)]);
        assert_eq!(next_reconnect_delay(&lost, u32::MAX, None), secs(60), "no overflow");
    }

    #[test]
    fn terminals_stop_at_the_limit() {
        let lost = Disconnect::Lost("reset".into());
        assert_eq!(next_reconnect_delay(&lost, 10, Some(10)), Some(Duration::from_secs(60)));
        assert_eq!(next_reconnect_delay(&lost, 11, Some(10)), None);
    }

    #[test]
    fn zero_attempts_means_no_reconnect() {
        assert_eq!(next_reconnect_delay(&Disconnect::Lost("reset".into()), 1, Some(0)), None);
    }

    #[test]
    fn forwards_retry_without_limit() {
        assert!(next_reconnect_delay(&Disconnect::Lost("reset".into()), 10_000, None).is_some());
    }

    #[test]
    fn no_reconnect_after_exit() {
        assert_eq!(next_reconnect_delay(&Disconnect::Exited(Some(0)), 1, None), None);
    }

    #[test]
    fn no_reconnect_after_host_key_rejected() {
        assert_eq!(next_reconnect_delay(&Disconnect::HostKeyRejected, 1, None), None);
    }

    #[test]
    fn no_reconnect_after_auth_failed() {
        assert_eq!(next_reconnect_delay(&Disconnect::AuthFailed, 1, None), None);
    }
}

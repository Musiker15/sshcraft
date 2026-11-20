//! The user's own sign-in through polkit on Linux and FreeBSD, where Windows has Windows Hello:
//! SSHCraft's action (`packaging/linux/de.musiker15.sshcraft.policy`, `auth_self`) has the
//! desktop's authentication agent ask for the user's password or fingerprint. `pkcheck` runs with
//! an argument list, never through a shell.

// Windows and macOS use only the mapping, in its test.
#![cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]

use sshcraft_fido::presence::Presence;

/// SSHCraft's polkit action, as the packages install it.
pub const ACTION: &str = "de.musiker15.sshcraft.unlock";

/// What `pkcheck`'s exit status means for SSHCraft. Only a sign-in that was refused or called off
/// is "declined"; everything that says polkit cannot ask here (no authentication agent, the
/// action not installed, no polkit at all) is "unavailable", like a PC without Windows Hello, so
/// nobody is locked out of their own sessions.
pub fn presence_for(status: Option<i32>) -> Presence {
    match status {
        // Authorized: the password or fingerprint was right.
        Some(0) => Presence::Verified,
        // 1: not authorized (a wrong password three times, or a rule says no); 3: dismissed.
        Some(1 | 3) => Presence::Declined,
        // 2: authentication needed but no agent runs; 127 and others: polkit failed, for example
        // because the action is not installed; `None`: pkcheck is missing or was killed.
        _ => Presence::Unavailable,
    }
}

/// Whether polkit knows SSHCraft's action here, so the app lock can be offered.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn installed() -> bool {
    std::process::Command::new("pkaction")
        .args(["--action-id", ACTION])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Asks polkit for the user's sign-in on behalf of this process.
#[cfg(all(unix, not(target_os = "macos")))]
pub struct Polkit;

#[cfg(all(unix, not(target_os = "macos")))]
#[async_trait::async_trait]
impl sshcraft_fido::presence::UserPresence for Polkit {
    async fn verify(&self, _reason: &str) -> Presence {
        // The text in the dialog is the action's message from the policy file.
        let pid = std::process::id().to_string();
        let status = tokio::process::Command::new("pkcheck")
            .args(["--action-id", ACTION, "--process", &pid, "--allow-user-interaction"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // A dropped request (the tab was closed) ends pkcheck, which closes the dialog.
            .kill_on_drop(true)
            .status()
            .await;
        presence_for(status.ok().and_then(|s| s.code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a right sign-in unlocks, a wrong or dismissed one stays locked, and a system where
    /// polkit cannot ask counts as having no sign-in at all (pkcheck's statuses on Debian 13:
    /// 0 with the right password, 1 after a wrong one, 2 without an agent, 127 without the
    /// action).
    #[test]
    fn pkcheck_statuses_mean_what_polkit_says() {
        assert_eq!(presence_for(Some(0)), Presence::Verified);
        assert_eq!(presence_for(Some(1)), Presence::Declined);
        assert_eq!(presence_for(Some(3)), Presence::Declined);
        for status in [Some(2), Some(4), Some(127), Some(-1), None] {
            assert_eq!(presence_for(status), Presence::Unavailable, "{status:?}");
        }
    }

    /// Asks the real pkcheck for this process and compares with what the system is set up for.
    /// Run by hand where polkit runs, as a user who is not root:
    /// `SSHCRAFT_POLKIT_EXPECT=verified|declined|unavailable cargo test -p sshcraft -- --ignored`.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[tokio::test]
    #[ignore = "needs a running polkit set up for the expected answer"]
    async fn pkcheck_answers_for_this_process() {
        use sshcraft_fido::presence::UserPresence;
        let expected = match std::env::var("SSHCRAFT_POLKIT_EXPECT").as_deref() {
            Ok("verified") => Presence::Verified,
            Ok("declined") => Presence::Declined,
            _ => Presence::Unavailable,
        };
        assert_eq!(Polkit.verify("test").await, expected);
        assert_eq!(installed(), expected != Presence::Unavailable || std::env::var("SSHCRAFT_POLKIT_INSTALLED").is_ok());
    }
}

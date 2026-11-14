//! Finds a running Tauri version of SSHCraft (up to the tag `tauri-final`). It must not run next
//! to the rebuilt app: two sync engines with the same device key would undo each other's work.
//!
//! The Tauri version listened on the named pipe `\\.\pipe\sshcraft-agent-<SID>` (Windows) or the
//! socket `sshcraft/agent.sock` in the runtime folder (Unix). No new service may use the prefix
//! `sshcraft-agent-`.

/// Whether a pipe name looks like the Tauri version's agent pipe.
const PIPE_PREFIX: &str = "sshcraft-agent-";

pub fn is_legacy_pipe_name(name: &str) -> bool {
    name.strip_prefix(PIPE_PREFIX).and_then(|sid| sid.strip_prefix("S-")).is_some_and(|rest| !rest.is_empty())
}

/// Whether a pipe counts as the Tauri version of this user. Without a known SID every pipe of the
/// Tauri version counts: better not to start than to run two sync engines.
pub fn is_own_legacy_pipe(name: &str, own_sid: Option<&str>) -> bool {
    match own_sid {
        Some(sid) => name.strip_prefix(PIPE_PREFIX) == Some(sid),
        None => is_legacy_pipe_name(name),
    }
}

/// The SID from the output of `whoami /user /fo csv /nh`: `"domain\user","S-1-5-…"`.
pub fn parse_whoami_sid(output: &str) -> Option<String> {
    let line = output.lines().next()?.trim();
    let mut fields = line.split("\",\"");
    let _user = fields.next()?;
    let sid = fields.next()?.strip_suffix('"')?;
    if fields.next().is_some() || !sid.starts_with("S-1-") || !sid.chars().all(|c| c.is_ascii_digit() || c == '-' || c == 'S') {
        return None;
    }
    Some(sid.to_string())
}

/// Whether one of the given pipe names belongs to a running Tauri version of this user.
pub fn legacy_in_pipes<I: IntoIterator<Item = String>>(names: I, own_sid: Option<&str>) -> bool {
    names.into_iter().any(|name| is_own_legacy_pipe(&name, own_sid))
}

/// The SID of the user this process runs as, from `whoami` (Windows only). It runs from the
/// system folder by absolute path with an argument list, never through a shell.
#[cfg(windows)]
pub fn current_user_sid() -> Option<String> {
    let root = std::env::var_os("SystemRoot")?;
    let whoami = std::path::Path::new(&root).join("System32").join("whoami.exe");
    let out = std::process::Command::new(whoami).args(["/user", "/fo", "csv", "/nh"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_whoami_sid(&String::from_utf8_lossy(&out.stdout))
}

/// The SID of the user this process runs as; there is none outside Windows.
#[cfg(not(windows))]
pub fn current_user_sid() -> Option<String> {
    None
}

/// Whether a Tauri version of SSHCraft runs for this user on this machine. A folder or socket
/// that cannot be read counts as "not running".
#[cfg(windows)]
pub fn legacy_instance_running() -> bool {
    let Ok(pipes) = std::fs::read_dir(r"\\.\pipe\") else {
        return false;
    };
    let names = pipes.filter_map(Result::ok).map(|entry| entry.file_name().to_string_lossy().into_owned());
    legacy_in_pipes(names, current_user_sid().as_deref())
}

/// Whether a Tauri version of SSHCraft runs for this user on this machine: its agent socket in
/// the runtime folder (or `~/.sshcraft`) accepts a connection.
#[cfg(unix)]
pub fn legacy_instance_running() -> bool {
    let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime) if !runtime.is_empty() => std::path::PathBuf::from(runtime).join("sshcraft"),
        _ => match crate::home_dir() {
            Some(home) => home.join(".sshcraft"),
            None => return false,
        },
    };
    std::os::unix::net::UnixStream::connect(dir.join("agent.sock")).is_ok()
}

#[cfg(test)]
mod tests {
    use super::{is_legacy_pipe_name, is_own_legacy_pipe, legacy_in_pipes, parse_whoami_sid};

    #[test]
    fn legacy_pipe_names_are_recognized() {
        assert!(is_legacy_pipe_name("sshcraft-agent-S-1-5-21-1-2-3-1001"));
        for other in ["sshcraft-agent", "sshcraft-agentS-1", "openssh-ssh-agent", "sshcraft-control-S-1", "sshcraft-agent-S-"] {
            assert!(!is_legacy_pipe_name(other), "{other}");
        }
    }

    #[test]
    fn legacy_check_without_pipes_is_false() {
        assert!(!legacy_in_pipes(Vec::new(), Some("S-1-5-21-1")));
        assert!(!legacy_in_pipes(vec!["openssh-ssh-agent".to_string()], None));
    }

    #[test]
    fn sid_is_parsed_from_whoami_csv() {
        assert_eq!(parse_whoami_sid("\"desktop\\moritz\",\"S-1-5-21-1-2-3-1001\"\r\n").as_deref(), Some("S-1-5-21-1-2-3-1001"));
        for bad in ["", "\"desktop\\moritz\"", "\"a\",\"b\"", "garbage", "\"a\",\"S-1-5\" extra,\"S\""] {
            assert_eq!(parse_whoami_sid(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_the_own_users_pipe_counts() {
        assert!(!is_own_legacy_pipe("sshcraft-agent-S-1-5-21-9", Some("S-1-5-21-1")));
        assert!(is_own_legacy_pipe("sshcraft-agent-S-1-5-21-1", Some("S-1-5-21-1")));
        assert!(is_own_legacy_pipe("sshcraft-agent-S-1-5-21-9", None));
        assert!(legacy_in_pipes(vec!["x".to_string(), "sshcraft-agent-S-1-5-21-1".to_string()], Some("S-1-5-21-1")));
        assert!(!legacy_in_pipes(vec!["sshcraft-agent-S-1-5-21-9".to_string()], Some("S-1-5-21-1")));
    }

    /// whoami answers on every Windows machine; without the SID the check would block on the
    /// Tauri version of any user.
    #[cfg(windows)]
    #[test]
    fn own_sid_is_found_on_windows() {
        assert!(super::current_user_sid().is_some_and(|sid| sid.starts_with("S-1-")));
    }

    /// Start the installed Tauri version first, then run with `--ignored`.
    #[test]
    #[ignore = "needs a running Tauri version of SSHCraft"]
    fn legacy_check_sees_a_running_tauri_version() {
        assert!(super::legacy_instance_running());
    }
}

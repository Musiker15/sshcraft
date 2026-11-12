//! Telling a host's operating system from `/etc/os-release` or `uname -s`, for its symbol.

use crate::model::HostOs;

/// What the app runs once after the first connection to a host without a symbol.
pub const OS_PROBE: &str = "cat /etc/os-release 2>/dev/null; uname -s 2>/dev/null";

fn from_id(id: &str) -> Option<HostOs> {
    Some(match id {
        "debian" => HostOs::Debian,
        "ubuntu" => HostOs::Ubuntu,
        "fedora" => HostOs::Fedora,
        "rhel" => HostOs::RedHat,
        "centos" => HostOs::CentOs,
        "rocky" => HostOs::Rocky,
        "almalinux" => HostOs::Alma,
        "arch" | "archarm" => HostOs::Arch,
        "alpine" => HostOs::Alpine,
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "suse" | "sles" => HostOs::OpenSuse,
        "raspbian" => HostOs::Raspbian,
        _ => return None,
    })
}

fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let value = line.trim().strip_prefix(key)?.strip_prefix('=')?;
        Some(value.trim().trim_matches(['"', '\'']))
    })
}

/// The operating system named in the output of [`OS_PROBE`], if it names a known one.
pub fn from_output(text: &str) -> Option<HostOs> {
    if let Some(os) = field(text, "ID").and_then(|id| from_id(&id.to_lowercase())) {
        return Some(os);
    }
    if let Some(like) = field(text, "ID_LIKE")
        && let Some(os) = like
            .split_whitespace()
            .find_map(|id| from_id(&id.to_lowercase()))
    {
        return Some(os);
    }
    text.lines().map(str::trim).find_map(|line| match line {
        "Darwin" => Some(HostOs::MacOs),
        "FreeBSD" => Some(HostOs::FreeBsd),
        "OpenBSD" => Some(HostOs::OpenBsd),
        "Linux" => Some(HostOs::Linux),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debian_ubuntu_alpine_rocky_from_os_release() {
        let release = |id: &str| format!("NAME=\"X\"\nID={id}\nVERSION_ID=\"1\"\nLinux\n");
        assert_eq!(from_output(&release("debian")), Some(HostOs::Debian));
        assert_eq!(from_output(&release("ubuntu")), Some(HostOs::Ubuntu));
        assert_eq!(from_output(&release("alpine")), Some(HostOs::Alpine));
        assert_eq!(from_output(&release("\"rocky\"")), Some(HostOs::Rocky));
    }

    #[test]
    fn id_like_falls_back() {
        let text = "ID=pop\nID_LIKE=\"ubuntu debian\"\nLinux\n";
        assert_eq!(from_output(text), Some(HostOs::Ubuntu));
        assert_eq!(from_output("ID=somethingnew\nLinux\n"), Some(HostOs::Linux));
    }

    #[test]
    fn darwin_and_freebsd_from_uname() {
        assert_eq!(from_output("Darwin\n"), Some(HostOs::MacOs));
        assert_eq!(from_output("FreeBSD\n"), Some(HostOs::FreeBsd));
    }

    #[test]
    fn unknown_is_none() {
        assert_eq!(from_output(""), None);
        assert_eq!(from_output("command not found\n"), None);
    }
}

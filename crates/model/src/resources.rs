//! The resource bar under a terminal: load, memory and disk of the server, read by one fixed
//! command over the tab's connection. The output is the server's and so hostile: it is read
//! line by line within limits, and whatever does not fit is left out.

use serde::{Deserialize, Serialize};

/// What runs on the server: fixed text, nothing from the host or the user goes into it.
pub const PROBE: &str = "cat /proc/loadavg 2>/dev/null; echo @@SSHCRAFT@@; cat /proc/meminfo 2>/dev/null; echo @@SSHCRAFT@@; df -Pk / 2>/dev/null";
/// Separates the three parts of the output.
const MARK: &str = "@@SSHCRAFT@@";
/// Lines read per part.
const MAX_LINES: usize = 200;

/// What the bar shows; each part is missing where the server did not answer it (not Linux, no
/// `/proc`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Resources {
    /// The load average over one minute.
    pub load: Option<f32>,
    /// Memory in use, in percent of all.
    pub memory_percent: Option<u8>,
    pub memory_total_kib: Option<u64>,
    /// The root file system in use, in percent.
    pub disk_percent: Option<u8>,
}

impl Resources {
    pub fn is_empty(&self) -> bool {
        self.load.is_none() && self.memory_percent.is_none() && self.disk_percent.is_none()
    }
}

fn percent(used: u64, total: u64) -> Option<u8> {
    if total == 0 {
        return None;
    }
    // In 128 bits: a hostile total near `u64::MAX` must not overflow the product.
    u8::try_from(u128::from(used.min(total)) * 100 / u128::from(total)).ok()
}

/// Reads the probe's output; `None` when nothing in it could be read.
pub fn parse(output: &str) -> Option<Resources> {
    let mut parts = output.split(MARK);
    let (loadavg, meminfo, df) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
    let load = loadavg.split_whitespace().next().and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite() && *v >= 0.0 && *v < 1_000_000.0);
    let field = |name: &str| -> Option<u64> {
        meminfo.lines().take(MAX_LINES).find_map(|line| {
            let rest = line.strip_prefix(name)?.strip_prefix(':')?;
            rest.split_whitespace().next()?.parse().ok()
        })
    };
    let total = field("MemTotal");
    let available = field("MemAvailable");
    let memory_percent = match (total, available) {
        (Some(t), Some(a)) => percent(t.saturating_sub(a), t),
        _ => None,
    };
    let disk_percent = df.lines().take(MAX_LINES).skip_while(|l| !l.trim_start().starts_with("Filesystem")).nth(1).and_then(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let size = fields.get(1)?.parse::<u64>().ok()?;
        let used = fields.get(2)?.parse::<u64>().ok()?;
        percent(used, size)
    });
    let out = Resources { load, memory_percent, memory_total_kib: total, disk_percent };
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINUX: &str = "0.42 0.30 0.25 1/123 4567\n@@SSHCRAFT@@\nMemTotal:       16000000 kB\nMemFree:  1000000 kB\nMemAvailable:    4000000 kB\n@@SSHCRAFT@@\nFilesystem     1024-blocks      Used Available Capacity Mounted on\n/dev/sda1        100000000  42000000  58000000      42% /\n";

    #[test]
    fn a_linux_server_is_read() {
        let r = parse(LINUX).unwrap();
        assert_eq!(r.load, Some(0.42));
        assert_eq!(r.memory_percent, Some(75));
        assert_eq!(r.memory_total_kib, Some(16_000_000));
        assert_eq!(r.disk_percent, Some(42));
    }

    #[test]
    fn missing_parts_stay_missing() {
        let r = parse("@@SSHCRAFT@@\n@@SSHCRAFT@@\nFilesystem 512-blocks Used Available Capacity Mounted on\n/dev/disk1 200 50 150 25% /\n").unwrap();
        assert_eq!((r.load, r.memory_percent, r.disk_percent), (None, None, Some(25)));
        assert_eq!(parse("command not found"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn hostile_numbers_do_not_crash() {
        let odd =
            "NaN\n@@SSHCRAFT@@\nMemTotal: 0 kB\nMemAvailable: 99999999999999999999 kB\n@@SSHCRAFT@@\nFilesystem x\n/ 0 18446744073709551615 0 0% /\n";
        assert_eq!(parse(odd), None);
        let big = format!("1e30\n@@SSHCRAFT@@\nMemTotal: 18446744073709551615 kB\nMemAvailable: 0 kB\n@@SSHCRAFT@@\n{}", "x\n".repeat(10_000));
        let r = parse(&big).unwrap();
        assert_eq!(r.load, None);
        assert_eq!(r.memory_percent, Some(100));
    }
}

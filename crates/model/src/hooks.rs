//! Steps before connecting and after the session ends: wake a computer (Wake-on-LAN), knock on
//! ports, run a local program (a VPN up or down). A program starts from its argument list, never
//! through a shell, and only once it was allowed on this device like a proxy command; the digest
//! of [`approval_argv`] is what gets allowed.

use serde::{Deserialize, Serialize};

/// Steps one list may have.
pub const MAX_STEPS: usize = 16;
/// Ports one knock may have.
pub const MAX_PORTS: usize = 64;
/// Arguments one program may have.
pub const MAX_ARGS: usize = 32;
/// The longest argument, in characters.
pub const MAX_ARG_CHARS: usize = 1024;
/// The longest wait after a step, in seconds.
pub const MAX_WAIT_SECS: u32 = 300;
/// The longest pause between two knocks, in milliseconds.
pub const MAX_DELAY_MS: u32 = 5000;

/// One step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectStep {
    /// Sends a magic packet for `mac` to `broadcast` (default 255.255.255.255) on `port`
    /// (default 9), then waits `wait_secs` for the computer to start.
    WakeOnLan {
        mac: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        broadcast: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
        #[serde(default)]
        wait_secs: u32,
    },
    /// Opens a TCP connection to each port of the host's address in turn, `delay_ms` apart.
    Knock {
        ports: Vec<u16>,
        #[serde(default)]
        delay_ms: u32,
    },
    /// Runs a local program and waits for it (at most a minute), then `wait_secs` more.
    Command {
        argv: Vec<String>,
        #[serde(default)]
        wait_secs: u32,
    },
}

/// Why a list of steps cannot be saved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StepError {
    #[error("at most 16 steps")]
    TooManySteps,
    #[error("step {0}: not a MAC address like aa:bb:cc:dd:ee:ff")]
    Mac(usize),
    #[error("step {0}: not an IPv4 broadcast address")]
    Broadcast(usize),
    #[error("step {0}: 1 to 64 ports from 1 to 65535")]
    Ports(usize),
    #[error("step {0}: 1 to 32 arguments of at most 1024 characters, without control characters")]
    Argv(usize),
    #[error("step {0}: wait at most 300 seconds, pause at most 5000 milliseconds")]
    Wait(usize),
}

/// The six bytes of `mac` (`aa:bb:cc:dd:ee:ff`, `aa-bb-…` or `aabbccddeeff`).
pub fn parse_mac(mac: &str) -> Option<[u8; 6]> {
    let hex: String = mac.chars().filter(|c| !matches!(c, ':' | '-')).collect();
    if hex.len() != 12 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 6];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// The Wake-on-LAN packet for `mac`: six bytes 0xFF, then the address sixteen times.
pub fn magic_packet(mac: [u8; 6]) -> [u8; 102] {
    let mut packet = [0xFF; 102];
    for (i, byte) in packet.iter_mut().enumerate().skip(6) {
        *byte = mac[(i - 6) % 6];
    }
    packet
}

/// Whether `steps` keep the limits.
pub fn check(steps: &[ConnectStep]) -> Result<(), StepError> {
    if steps.len() > MAX_STEPS {
        return Err(StepError::TooManySteps);
    }
    for (i, step) in steps.iter().enumerate() {
        let n = i + 1;
        match step {
            ConnectStep::WakeOnLan { mac, broadcast, wait_secs, .. } => {
                parse_mac(mac).ok_or(StepError::Mac(n))?;
                if broadcast.as_deref().is_some_and(|b| b.parse::<std::net::Ipv4Addr>().is_err()) {
                    return Err(StepError::Broadcast(n));
                }
                if *wait_secs > MAX_WAIT_SECS {
                    return Err(StepError::Wait(n));
                }
            }
            ConnectStep::Knock { ports, delay_ms } => {
                if ports.is_empty() || ports.len() > MAX_PORTS || ports.contains(&0) {
                    return Err(StepError::Ports(n));
                }
                if *delay_ms > MAX_DELAY_MS {
                    return Err(StepError::Wait(n));
                }
            }
            ConnectStep::Command { argv, wait_secs } => {
                let bad = argv.is_empty()
                    || argv.len() > MAX_ARGS
                    || argv.iter().any(|a| a.chars().count() > MAX_ARG_CHARS || a.chars().any(char::is_control))
                    || argv.first().is_some_and(|p| p.trim().is_empty());
                if bad {
                    return Err(StepError::Argv(n));
                }
                if *wait_secs > MAX_WAIT_SECS {
                    return Err(StepError::Wait(n));
                }
            }
        }
    }
    Ok(())
}

/// What is allowed for one program of a step: its exact arguments.
pub fn approval_argv(argv: &[String]) -> Vec<String> {
    std::iter::once("connect-step".to_string()).chain(argv.iter().cloned()).collect()
}

/// The programs `steps` run.
pub fn commands(steps: &[ConnectStep]) -> impl Iterator<Item = &Vec<String>> {
    steps.iter().filter_map(|s| match s {
        ConnectStep::Command { argv, .. } => Some(argv),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mac_is_read_in_its_usual_forms() {
        let want = Some([0xaa, 0xbb, 0xcc, 0x01, 0x02, 0x03]);
        assert_eq!(parse_mac("aa:bb:cc:01:02:03"), want);
        assert_eq!(parse_mac("AA-BB-CC-01-02-03"), want);
        assert_eq!(parse_mac("aabbcc010203"), want);
        assert_eq!(parse_mac("aa:bb:cc:01:02"), None);
        assert_eq!(parse_mac("zz:bb:cc:01:02:03"), None);
    }

    #[test]
    fn the_magic_packet_is_six_ff_then_the_mac_sixteen_times() {
        let mac = [1, 2, 3, 4, 5, 6];
        let packet = magic_packet(mac);
        assert!(packet[..6].iter().all(|b| *b == 0xFF));
        for k in 0..16 {
            assert_eq!(&packet[6 + k * 6..12 + k * 6], &mac);
        }
    }

    #[test]
    fn limits_are_checked() {
        let wol = |mac: &str| ConnectStep::WakeOnLan { mac: mac.into(), broadcast: None, port: None, wait_secs: 0 };
        assert_eq!(check(&[wol("aa:bb:cc:dd:ee:ff")]), Ok(()));
        assert_eq!(check(&[wol("nope")]), Err(StepError::Mac(1)));
        assert_eq!(check(&[ConnectStep::Knock { ports: vec![], delay_ms: 0 }]), Err(StepError::Ports(1)));
        assert_eq!(check(&[ConnectStep::Knock { ports: vec![0], delay_ms: 0 }]), Err(StepError::Ports(1)));
        assert_eq!(check(&[ConnectStep::Command { argv: vec!["vpn\nup".into()], wait_secs: 0 }]), Err(StepError::Argv(1)));
        assert_eq!(check(&vec![wol("aa:bb:cc:dd:ee:ff"); 17]), Err(StepError::TooManySteps));
    }

    #[test]
    fn a_changed_program_needs_a_new_approval() {
        assert_ne!(approval_argv(&["wg-quick".into(), "up".into()]), approval_argv(&["wg-quick".into(), "down".into()]));
    }
}

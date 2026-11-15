//! Notices that the computer slept or that its network changed, so connections are checked at
//! once instead of after the keep-alive gives up (up to 45 seconds), and a reconnect that waits
//! tries again now. Nothing here talks to the network; it reads the clocks and the addresses of
//! the network interfaces every few seconds.

use std::net::IpAddr;
use std::time::{Duration, Instant};

/// How often the clocks and the addresses are read.
pub(crate) const TICK: Duration = Duration::from_secs(5);

/// A tick this much later than planned means the computer slept (or the process was stopped).
const SLEPT: Duration = Duration::from_secs(20);

/// How long a connection may take to answer a ping after a wake event.
pub(crate) const PING_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeEvent {
    /// The computer woke from sleep.
    Resumed,
    /// The addresses of the network interfaces changed (another network, a VPN, a cable).
    NetworkChanged,
}

/// One reading of the clocks and the addresses.
#[derive(Debug, Clone)]
pub(crate) struct Probe {
    /// Wall-clock milliseconds; it moves on while the computer sleeps.
    pub wall_ms: i64,
    /// The monotonic clock; on Linux and macOS it stands still while the computer sleeps.
    pub mono: Instant,
    /// Addresses other than loopback, sorted.
    pub addresses: Vec<IpAddr>,
}

impl Probe {
    pub(crate) fn now() -> Self {
        // Loopback and IPv6 link-local addresses come and go without the network changing.
        let link_local = |a: &IpAddr| matches!(a, IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80);
        let mut addresses: Vec<IpAddr> =
            sshcraft_platform::interface_addresses().into_iter().filter(|a| !a.is_loopback() && !link_local(a)).collect();
        addresses.sort_unstable();
        addresses.dedup();
        Self { wall_ms: crate::now_ms(), mono: Instant::now(), addresses }
    }
}

/// What happened between two readings `TICK` apart.
pub(crate) fn detect(prev: &Probe, now: &Probe) -> Option<WakeEvent> {
    let mono = now.mono.saturating_duration_since(prev.mono);
    let wall = Duration::from_millis(u64::try_from(now.wall_ms.saturating_sub(prev.wall_ms)).unwrap_or(0));
    // Either clock running far ahead of the tick: the monotonic one where it counts sleep
    // (Windows), the wall clock where the monotonic one stood still (Linux, macOS).
    if mono.max(wall) > TICK + SLEPT {
        return Some(WakeEvent::Resumed);
    }
    // No addresses at all is no network (or a failed read): nothing to reconnect to yet.
    (!now.addresses.is_empty() && now.addresses != prev.addresses).then_some(WakeEvent::NetworkChanged)
}

/// At most one "network changed" per this time: interfaces that flap (VPNs, containers,
/// temporary IPv6 addresses) must not use up the reconnect attempts.
const NETWORK_QUIET: Duration = Duration::from_secs(60);

/// Reads the clocks and addresses every [`TICK`] and sends what it notices, until the engine is
/// gone.
pub(crate) fn spawn(runtime: &tokio::runtime::Handle, context: std::sync::Weak<crate::sessions::Context>) {
    runtime.spawn(async move {
        let mut prev = tokio::task::spawn_blocking(Probe::now).await.ok();
        let mut last_network: Option<Instant> = None;
        loop {
            tokio::time::sleep(TICK).await;
            let Ok(now) = tokio::task::spawn_blocking(Probe::now).await else {
                continue;
            };
            let event = prev.as_ref().and_then(|p| detect(p, &now));
            let quiet = last_network.is_some_and(|t| t.elapsed() < NETWORK_QUIET);
            let event = match event {
                Some(WakeEvent::NetworkChanged) if quiet => None,
                Some(WakeEvent::NetworkChanged) => {
                    last_network = Some(Instant::now());
                    Some(WakeEvent::NetworkChanged)
                }
                other => other,
            };
            {
                let Some(context) = context.upgrade() else {
                    return;
                };
                if let Some(event) = event {
                    // Without a session or forward listening, nobody needs to know.
                    let _ = context.wake.send(event);
                }
            }
            prev = Some(now);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(wall_ms: i64, mono: Instant, addresses: &[&str]) -> Probe {
        Probe { wall_ms, mono, addresses: addresses.iter().map(|a| a.parse().unwrap()).collect() }
    }

    #[test]
    fn a_quiet_tick_is_nothing() {
        let t = Instant::now();
        assert_eq!(detect(&probe(0, t, &["10.0.0.2"]), &probe(5_000, t + TICK, &["10.0.0.2"])), None);
    }

    #[test]
    fn a_clock_jump_means_resumed() {
        let t = Instant::now();
        // Linux and macOS: the monotonic clock stood still, the wall clock moved an hour.
        assert_eq!(detect(&probe(0, t, &[]), &probe(3_600_000, t + TICK, &[])), Some(WakeEvent::Resumed));
        // Windows: the monotonic clock counts the sleep too.
        assert_eq!(detect(&probe(0, t, &[]), &probe(3_600_000, t + Duration::from_secs(3600), &[])), Some(WakeEvent::Resumed));
    }

    #[test]
    fn new_addresses_mean_network_changed() {
        let t = Instant::now();
        assert_eq!(detect(&probe(0, t, &["10.0.0.2"]), &probe(5_000, t + TICK, &["192.168.1.7"])), Some(WakeEvent::NetworkChanged));
        assert_eq!(detect(&probe(0, t, &["10.0.0.2"]), &probe(5_000, t + TICK, &[])), None, "no addresses is no network to move to");
    }
}

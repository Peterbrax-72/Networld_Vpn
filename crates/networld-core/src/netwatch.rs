//! Watches the device's internet connectivity (Wi-Fi / mobile data / Ethernet)
//! and writes "network down" / "network back" lines to the log, so a failed
//! connection can be told apart from "the phone simply has no signal".
//!
//! A probe is a plain TCP connect to a few well-known public endpoints. The
//! check runs outside the tunnel on Android (the app is excluded from the VPN)
//! and in proxy mode on Windows, so it reflects the real uplink.

use crate::logging::LogBus;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;

const PROBES: [&str; 3] = ["1.1.1.1:443", "8.8.8.8:53", "9.9.9.9:443"];
const INTERVAL: Duration = Duration::from_secs(8);
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
/// Consecutive failed probes before the network is declared down.
const FAILS_TO_DOWN: u32 = 2;

#[derive(Debug, PartialEq, Eq)]
pub enum NetEvent {
    Up { was_down_for: Option<Duration> },
    Down,
}

/// Debounced up/down state machine (pure, so it can be unit-tested).
#[derive(Default)]
pub struct Tracker {
    up: Option<bool>,
    fails: u32,
    down_since: Option<Instant>,
}

impl Tracker {
    pub fn observe(&mut self, ok: bool, now: Instant) -> Option<NetEvent> {
        if ok {
            self.fails = 0;
            if self.up != Some(true) {
                let was_down_for = self.down_since.take().map(|t| now.duration_since(t));
                self.up = Some(true);
                return Some(NetEvent::Up { was_down_for });
            }
            None
        } else {
            self.fails += 1;
            if self.fails >= FAILS_TO_DOWN && self.up != Some(false) {
                self.up = Some(false);
                self.down_since = Some(now);
                return Some(NetEvent::Down);
            }
            None
        }
    }
}

async fn probe() -> bool {
    for target in PROBES {
        if let Ok(addr) = target.parse::<SocketAddr>() {
            if let Ok(Ok(_)) = tokio::time::timeout(PROBE_TIMEOUT, TcpStream::connect(addr)).await {
                return true;
            }
        }
    }
    false
}

pub fn spawn(logs: Arc<LogBus>) {
    tokio::spawn(async move {
        let mut tracker = Tracker::default();
        loop {
            let ok = probe().await;
            match tracker.observe(ok, Instant::now()) {
                Some(NetEvent::Up { was_down_for: None }) => {
                    logs.info("network", "Internet is reachable (Wi-Fi / mobile data up)")
                }
                Some(NetEvent::Up {
                    was_down_for: Some(d),
                }) => logs.info(
                    "network",
                    format!(
                        "Internet is back (Wi-Fi / mobile data up) after {}s offline",
                        d.as_secs()
                    ),
                ),
                Some(NetEvent::Down) => logs.warn(
                    "network",
                    "Internet is down - no Wi-Fi / mobile data connectivity",
                ),
                None => {}
            }
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounces_down_and_reports_recovery() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        assert_eq!(
            t.observe(true, t0),
            Some(NetEvent::Up { was_down_for: None })
        );
        assert_eq!(t.observe(true, t0), None);
        assert_eq!(t.observe(false, t0), None); // one miss is not enough
        assert_eq!(t.observe(false, t0), Some(NetEvent::Down));
        assert_eq!(t.observe(false, t0), None); // already down
        let later = t0 + Duration::from_secs(9);
        assert_eq!(
            t.observe(true, later),
            Some(NetEvent::Up {
                was_down_for: Some(Duration::from_secs(9))
            })
        );
    }

    #[test]
    fn a_single_miss_between_successes_is_ignored() {
        let mut t = Tracker::default();
        let n = Instant::now();
        t.observe(true, n);
        assert_eq!(t.observe(false, n), None);
        assert_eq!(t.observe(true, n), None);
    }
}

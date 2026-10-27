//! Traffic counters, live speed sampling and a persisted daily data-usage ledger.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
struct Sample {
    at: Option<Instant>,
    up: u64,
    down: u64,
}

/// Lock-free byte counters shared by every proxy connection.
#[derive(Debug)]
pub struct TrafficCounters {
    up: AtomicU64,
    down: AtomicU64,
    active_conns: AtomicU64,
    total_conns: AtomicU64,
    started: Mutex<Option<Instant>>,
    last: Mutex<Sample>,
    speed: Mutex<(u64, u64)>,
}

impl Default for TrafficCounters {
    fn default() -> Self {
        Self::new()
    }
}

impl TrafficCounters {
    pub fn new() -> Self {
        Self {
            up: AtomicU64::new(0),
            down: AtomicU64::new(0),
            active_conns: AtomicU64::new(0),
            total_conns: AtomicU64::new(0),
            started: Mutex::new(None),
            last: Mutex::new(Sample::default()),
            speed: Mutex::new((0, 0)),
        }
    }

    pub fn start_session(&self) {
        self.up.store(0, Ordering::Relaxed);
        self.down.store(0, Ordering::Relaxed);
        self.active_conns.store(0, Ordering::Relaxed);
        self.total_conns.store(0, Ordering::Relaxed);
        if let Ok(mut s) = self.started.lock() {
            *s = Some(Instant::now());
        }
        if let Ok(mut l) = self.last.lock() {
            *l = Sample {
                at: Some(Instant::now()),
                up: 0,
                down: 0,
            };
        }
    }

    pub fn end_session(&self) {
        if let Ok(mut s) = self.started.lock() {
            *s = None;
        }
        if let Ok(mut sp) = self.speed.lock() {
            *sp = (0, 0);
        }
    }

    pub fn add_up(&self, n: u64) {
        self.up.fetch_add(n, Ordering::Relaxed);
    }
    pub fn add_down(&self, n: u64) {
        self.down.fetch_add(n, Ordering::Relaxed);
    }
    pub fn conn_opened(&self) {
        self.active_conns.fetch_add(1, Ordering::Relaxed);
        self.total_conns.fetch_add(1, Ordering::Relaxed);
    }
    pub fn conn_closed(&self) {
        let mut cur = self.active_conns.load(Ordering::Relaxed);
        while cur > 0 {
            match self.active_conns.compare_exchange_weak(
                cur,
                cur - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => cur = observed,
            }
        }
    }

    pub fn bytes_up(&self) -> u64 {
        self.up.load(Ordering::Relaxed)
    }
    pub fn bytes_down(&self) -> u64 {
        self.down.load(Ordering::Relaxed)
    }

    pub fn uptime(&self) -> Duration {
        self.started
            .lock()
            .ok()
            .and_then(|g| g.map(|t| t.elapsed()))
            .unwrap_or_default()
    }

    /// Call once per second from the status ticker to refresh the speed gauges.
    pub fn tick(&self) -> StatsSnapshot {
        let up = self.bytes_up();
        let down = self.bytes_down();
        let now = Instant::now();
        let (mut up_bps, mut down_bps) = (0u64, 0u64);
        if let Ok(mut last) = self.last.lock() {
            if let Some(at) = last.at {
                let dt = now.duration_since(at).as_secs_f64();
                if dt >= 0.2 {
                    up_bps = (((up.saturating_sub(last.up)) as f64) / dt) as u64;
                    down_bps = (((down.saturating_sub(last.down)) as f64) / dt) as u64;
                    *last = Sample {
                        at: Some(now),
                        up,
                        down,
                    };
                }
            } else {
                *last = Sample {
                    at: Some(now),
                    up,
                    down,
                };
            }
        }
        if let Ok(mut sp) = self.speed.lock() {
            if up_bps != 0 || down_bps != 0 {
                *sp = (up_bps, down_bps);
            } else {
                up_bps = sp.0;
                down_bps = sp.1;
            }
        }
        StatsSnapshot {
            uptime_secs: self.uptime().as_secs(),
            bytes_up: up,
            bytes_down: down,
            speed_up_bps: up_bps,
            speed_down_bps: down_bps,
            active_connections: self.active_conns.load(Ordering::Relaxed),
            total_connections: self.total_conns.load(Ordering::Relaxed),
        }
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        let sp = self.speed.lock().map(|g| *g).unwrap_or((0, 0));
        StatsSnapshot {
            uptime_secs: self.uptime().as_secs(),
            bytes_up: self.bytes_up(),
            bytes_down: self.bytes_down(),
            speed_up_bps: sp.0,
            speed_down_bps: sp.1,
            active_connections: self.active_conns.load(Ordering::Relaxed),
            total_connections: self.total_conns.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsSnapshot {
    pub uptime_secs: u64,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub speed_up_bps: u64,
    pub speed_down_bps: u64,
    pub active_connections: u64,
    pub total_connections: u64,
}

/// Persisted per-day usage, keyed by `YYYY-MM-DD` (UTC).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataUsageLedger {
    #[serde(default)]
    pub days: BTreeMap<String, DayUsage>,
    #[serde(default)]
    pub lifetime_up: u64,
    #[serde(default)]
    pub lifetime_down: u64,
    #[serde(default)]
    pub sessions: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DayUsage {
    pub up: u64,
    pub down: u64,
    pub seconds: u64,
}

impl DataUsageLedger {
    pub fn record(&mut self, up: u64, down: u64, seconds: u64) {
        let key = utc_day_key(crate::logging::now_ms());
        let day = self.days.entry(key).or_default();
        day.up += up;
        day.down += down;
        day.seconds += seconds;
        self.lifetime_up += up;
        self.lifetime_down += down;
        self.sessions += 1;
        // Keep ~1 year of history.
        while self.days.len() > 370 {
            if let Some(first) = self.days.keys().next().cloned() {
                self.days.remove(&first);
            } else {
                break;
            }
        }
    }

    pub fn last_days(&self, n: usize) -> Vec<(String, DayUsage)> {
        self.days
            .iter()
            .rev()
            .take(n)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

/// Civil-date conversion (Howard Hinnant's algorithm) so we avoid a chrono dependency.
pub fn utc_day_key(ts_ms: u64) -> String {
    let days = (ts_ms / 86_400_000) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", n, UNITS[0])
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_accumulate() {
        let c = TrafficCounters::new();
        c.start_session();
        c.add_up(1000);
        c.add_down(2500);
        let s = c.snapshot();
        assert_eq!(s.bytes_up, 1000);
        assert_eq!(s.bytes_down, 2500);
    }

    #[test]
    fn day_key_is_iso() {
        // 2021-01-01T00:00:00Z
        assert_eq!(utc_day_key(1_609_459_200_000), "2021-01-01");
    }

    #[test]
    fn humanise() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.50 KB");
    }
}

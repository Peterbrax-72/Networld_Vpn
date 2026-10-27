//! In-process log bus: ring buffer + broadcast channel.
//!
//! The UI subscribes through the Tauri layer and receives every line live,
//! while the ring buffer lets a freshly opened Logs tab backfill history.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn label(&self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub seq: u64,
    pub ts_ms: u64,
    pub level: LogLevel,
    /// Subsystem tag, e.g. `ssh`, `socks`, `payload`, `xray`, `killswitch`.
    pub tag: String,
    pub message: String,
}

impl LogEntry {
    pub fn to_line(&self) -> String {
        format!(
            "[{}] {:<5} {:<10} {}",
            fmt_clock(self.ts_ms),
            self.level.label(),
            self.tag,
            self.message
        )
    }
}

/// `HH:MM:SS.mmm` without pulling in a date-time crate.
pub fn fmt_clock(ts_ms: u64) -> String {
    let secs = ts_ms / 1000;
    let ms = ts_ms % 1000;
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct LogBus {
    tx: broadcast::Sender<LogEntry>,
    buffer: Mutex<VecDeque<LogEntry>>,
    seq: AtomicU64,
    capacity: usize,
    min_level: Mutex<LogLevel>,
}

impl std::fmt::Debug for LogBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogBus")
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl LogBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(512);
        Self {
            tx,
            buffer: Mutex::new(VecDeque::with_capacity(capacity.min(4096))),
            seq: AtomicU64::new(0),
            capacity,
            min_level: Mutex::new(LogLevel::Debug),
        }
    }

    pub fn set_min_level(&self, level: LogLevel) {
        if let Ok(mut g) = self.min_level.lock() {
            *g = level;
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.tx.subscribe()
    }

    pub fn push(&self, level: LogLevel, tag: &str, message: impl Into<String>) {
        let min = self.min_level.lock().map(|g| *g).unwrap_or(LogLevel::Trace);
        if level < min {
            return;
        }
        let entry = LogEntry {
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            ts_ms: now_ms(),
            level,
            tag: tag.to_string(),
            message: message.into(),
        };
        if let Ok(mut buf) = self.buffer.lock() {
            if buf.len() >= self.capacity {
                buf.pop_front();
            }
            buf.push_back(entry.clone());
        }
        // Ignore send errors: nobody subscribed yet is a normal state.
        let _ = self.tx.send(entry);
    }

    pub fn trace(&self, tag: &str, m: impl Into<String>) {
        self.push(LogLevel::Trace, tag, m)
    }
    pub fn debug(&self, tag: &str, m: impl Into<String>) {
        self.push(LogLevel::Debug, tag, m)
    }
    pub fn info(&self, tag: &str, m: impl Into<String>) {
        self.push(LogLevel::Info, tag, m)
    }
    pub fn warn(&self, tag: &str, m: impl Into<String>) {
        self.push(LogLevel::Warn, tag, m)
    }
    pub fn error(&self, tag: &str, m: impl Into<String>) {
        self.push(LogLevel::Error, tag, m)
    }

    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.buffer
            .lock()
            .map(|b| b.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut b) = self.buffer.lock() {
            b.clear();
        }
    }

    /// Plain-text dump used by "Export logs" in the UI.
    pub fn export_text(&self) -> String {
        let mut out = String::from("# Networld VPN session log\n");
        for e in self.snapshot() {
            out.push_str(&e.to_line());
            out.push('\n');
        }
        out
    }
}

impl Default for LogBus {
    fn default() -> Self {
        Self::new(5000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_evicts_oldest() {
        let bus = LogBus::new(3);
        for i in 0..5 {
            bus.info("test", format!("line {i}"));
        }
        let snap = bus.snapshot();
        assert_eq!(snap.len(), 3);
        assert!(snap[0].message.contains("line 2"));
    }

    #[test]
    fn min_level_filters() {
        let bus = LogBus::new(10);
        bus.set_min_level(LogLevel::Warn);
        bus.debug("t", "nope");
        bus.error("t", "yes");
        assert_eq!(bus.snapshot().len(), 1);
    }
}

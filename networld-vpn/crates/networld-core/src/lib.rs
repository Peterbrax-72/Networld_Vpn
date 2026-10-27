//! # networld-core
//!
//! The tunnelling core of **Networld VPN** — deliberately free of any UI or
//! Tauri dependency so it can be unit-tested, reused by a CLI, or embedded in
//! a Windows service later.
//!
//! ```text
//!            ┌──────────── VpnManager ────────────┐
//!  UI ──IPC──▶  config · status · auto-reconnect  │
//!            └───┬───────────────┬────────────────┘
//!                │               │
//!        ┌───────▼──────┐  ┌─────▼────────────────────────┐
//!        │ local proxies│  │ engines (TunnelEngine trait) │
//!        │ SOCKS5 / HTTP│  │  ssh · xray · wireguard      │
//!        └───────┬──────┘  └─────┬────────────────────────┘
//!                │ Dialer        │ transport
//!                └───────────────▼──────────────────────────┐
//!                  TCP → payload injector → TLS/SNI → stream │
//!                                                            ┘
//! ```

pub mod config;
pub mod dialer;
pub mod engine;
pub mod engines;
pub mod error;
pub mod http_proxy;
pub mod killswitch;
pub mod links;
pub mod logging;
pub mod manager;
pub mod payload;
pub mod relay;
pub mod sidecar;
pub mod socks;
pub mod speedtest;
pub mod splittunnel;
pub mod ssh;
pub mod stats;
pub mod sysproxy;
pub mod tls;
pub mod transport;
pub mod tun;
pub mod uri;

pub use config::{AppConfig, Paths, ProfileKind, ServerProfile, Settings};
pub use engine::{ConnectionState, EngineKind, TunnelEngine};
pub use error::{CoreError, Result};
pub use logging::{LogBus, LogEntry, LogLevel};
pub use manager::{ManagerEvent, StatusSnapshot, VpnManager};
pub use payload::{PayloadPreset, PayloadTemplate};
pub use stats::{human_bytes, StatsSnapshot};
pub use transport::ResponsePosition;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Install process-wide defaults (crypto provider). Call once at startup.
pub fn init() {
    tls::init();
}

//! The engine contract. Every tunnelling backend implements `TunnelEngine`
//! and hands back an `EngineHandle` that exposes a [`Dialer`].
//!
//! Adding a protocol later = one new file + one match arm in `engines::build`.

use crate::config::{Paths, ServerProfile, Settings};
use crate::dialer::Dialer;
use crate::error::Result;
use crate::logging::LogBus;
use crate::stats::TrafficCounters;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineKind {
    Ssh,
    Xray,
    Wireguard,
}

impl EngineKind {
    pub fn label(&self) -> &'static str {
        match self {
            EngineKind::Ssh => "SSH",
            EngineKind::Xray => "V2Ray / Xray",
            EngineKind::Wireguard => "WireGuard",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineCapabilities {
    pub udp: bool,
    pub tun_mode: bool,
    pub requires_elevation: bool,
    /// Name of the sidecar binary this engine needs, if any.
    pub sidecar: Option<&'static str>,
    /// Whether the engine is usable inside an MSIX/Store sandbox.
    pub store_safe: bool,
}

impl Default for EngineCapabilities {
    fn default() -> Self {
        Self {
            udp: false,
            tun_mode: false,
            requires_elevation: false,
            sidecar: None,
            store_safe: true,
        }
    }
}

/// Everything an engine needs to start.
pub struct EngineContext {
    pub profile: ServerProfile,
    pub settings: Settings,
    pub logs: Arc<LogBus>,
    pub stats: Arc<TrafficCounters>,
    pub paths: Arc<Paths>,
}

#[async_trait]
pub trait EngineHandle: Send + Sync {
    /// The dialer the local proxies will use.
    fn dialer(&self) -> Arc<dyn Dialer>;
    /// One-line description shown on the Home screen ("SSH-SSL · bug.host").
    fn info(&self) -> String;
    fn is_alive(&self) -> bool;
    async fn shutdown(&self);
}

#[async_trait]
pub trait TunnelEngine: Send + Sync {
    fn kind(&self) -> EngineKind;
    fn capabilities(&self) -> EngineCapabilities;
    /// Validate the profile before any socket is opened; returns user-facing errors.
    fn validate(&self, profile: &ServerProfile) -> Result<()>;
    async fn start(&self, ctx: EngineContext) -> Result<Arc<dyn EngineHandle>>;
}

/// UI-facing connection state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionState {
    Disconnected,
    Resolving,
    Connecting,
    Authenticating,
    StartingProxies,
    Connected,
    Reconnecting,
    Disconnecting,
    Error,
}

impl ConnectionState {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            ConnectionState::Resolving
                | ConnectionState::Connecting
                | ConnectionState::Authenticating
                | ConnectionState::StartingProxies
                | ConnectionState::Reconnecting
                | ConnectionState::Disconnecting
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            ConnectionState::Disconnected => "Disconnected",
            ConnectionState::Resolving => "Resolving…",
            ConnectionState::Connecting => "Connecting…",
            ConnectionState::Authenticating => "Authenticating…",
            ConnectionState::StartingProxies => "Starting proxies…",
            ConnectionState::Connected => "Connected",
            ConnectionState::Reconnecting => "Reconnecting…",
            ConnectionState::Disconnecting => "Disconnecting…",
            ConnectionState::Error => "Error",
        }
    }
}

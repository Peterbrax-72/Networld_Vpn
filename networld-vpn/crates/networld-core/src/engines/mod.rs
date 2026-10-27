//! Engine registry. One match arm per protocol — this is the extension point
//! for new backends (OpenVPN, Hysteria, TUIC…) added later.

pub mod slowdns;
pub mod ssh_engine;
pub mod wireguard;
pub mod xray;

use crate::config::{ProfileKind, ServerProfile};
use crate::engine::{EngineCapabilities, EngineKind, TunnelEngine};

/// Pick the engine implementation for a profile.
pub fn build(profile: &ServerProfile) -> Box<dyn TunnelEngine> {
    match profile.kind {
        ProfileKind::Ssh(_) => Box::new(ssh_engine::SshEngine),
        ProfileKind::Xray(_) => Box::new(xray::XrayEngine),
        ProfileKind::Wireguard(_) => Box::new(wireguard::WireguardEngine),
    }
}

/// Static capability table — the Settings UI uses it to grey out options that
/// the current build cannot honour (e.g. elevation-only engines in the Store
/// package).
pub fn capability_matrix() -> Vec<(EngineKind, EngineCapabilities)> {
    vec![
        (EngineKind::Ssh, ssh_engine::SshEngine.caps()),
        (EngineKind::Xray, xray::XrayEngine.caps()),
        (EngineKind::Wireguard, wireguard::WireguardEngine.caps()),
    ]
}

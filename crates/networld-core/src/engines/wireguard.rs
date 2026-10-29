//! WireGuard engine slot.
//!
//! Windows WireGuard runs as a system tunnel service, so this engine installs
//! a tunnel from a generated `.conf` and then lets ordinary (direct) dialing
//! ride the OS interface. It needs elevation, which an MSIX Store package does
//! not get — the Store build therefore reports it as unavailable and the UI
//! greys it out, while the sideload build enables it.

use crate::config::{ProfileKind, ServerProfile, WireguardProfile};
use crate::dialer::{Dialer, DirectDialer};
use crate::engine::{EngineCapabilities, EngineContext, EngineHandle, EngineKind, TunnelEngine};
use crate::error::{CoreError, Result};
use async_trait::async_trait;
use std::sync::Arc;

pub const BINARY: &str = "wireguard";

#[derive(Default)]
pub struct WireguardEngine;

impl WireguardEngine {
    pub fn caps(&self) -> EngineCapabilities {
        EngineCapabilities {
            udp: true,
            tun_mode: true,
            requires_elevation: true,
            sidecar: Some(BINARY),
            store_safe: false,
        }
    }
}

pub struct WireguardHandle {
    tunnel_name: String,
    dialer: Arc<dyn Dialer>,
    detail: String,
}

#[async_trait]
impl EngineHandle for WireguardHandle {
    fn dialer(&self) -> Arc<dyn Dialer> {
        self.dialer.clone()
    }
    fn info(&self) -> String {
        self.detail.clone()
    }
    fn is_alive(&self) -> bool {
        true
    }
    async fn shutdown(&self) {
        #[cfg(windows)]
        {
            let _ = tokio::process::Command::new("wireguard.exe")
                .args(["/uninstalltunnelservice", &self.tunnel_name])
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: never flash a console
                .output()
                .await;
        }
        #[cfg(not(windows))]
        let _ = &self.tunnel_name;
    }
}

#[async_trait]
impl TunnelEngine for WireguardEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Wireguard
    }

    fn capabilities(&self) -> EngineCapabilities {
        self.caps()
    }

    fn validate(&self, profile: &ServerProfile) -> Result<()> {
        let ProfileKind::Wireguard(w) = &profile.kind else {
            return Err(CoreError::config("not a WireGuard profile"));
        };
        if w.private_key.trim().is_empty() || w.public_key.trim().is_empty() {
            return Err(CoreError::config("Both key pair fields are required"));
        }
        if w.endpoint.trim().is_empty() {
            return Err(CoreError::config("Endpoint is required"));
        }
        if w.address.is_empty() {
            return Err(CoreError::config(
                "At least one interface address is required",
            ));
        }
        Ok(())
    }

    async fn start(&self, ctx: EngineContext) -> Result<Arc<dyn EngineHandle>> {
        let ProfileKind::Wireguard(profile) = ctx.profile.kind.clone() else {
            return Err(CoreError::config("not a WireGuard profile"));
        };
        let logs = ctx.logs.clone();

        if !cfg!(windows) {
            return Err(CoreError::Unsupported(
                "WireGuard tunnels are Windows-only in this build".into(),
            ));
        }

        ctx.paths.ensure()?;
        let name = format!(
            "Networld-{}",
            ctx.profile
                .name
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .take(16)
                .collect::<String>()
        );
        let conf_path = ctx.paths.runtime_dir.join(format!("{name}.conf"));
        std::fs::write(&conf_path, render_conf(&profile))?;
        logs.info(
            "wireguard",
            format!("Installing tunnel service from {}", conf_path.display()),
        );

        #[cfg(windows)]
        {
            let out = tokio::process::Command::new("wireguard.exe")
                .args(["/installtunnelservice", &conf_path.display().to_string()])
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: never flash a console
                .output()
                .await
                .map_err(|e| CoreError::ElevationRequired(format!("wireguard.exe: {e}")))?;
            if !out.status.success() {
                return Err(CoreError::ElevationRequired(format!(
                    "wireguard.exe refused to install the tunnel: {}",
                    String::from_utf8_lossy(&out.stderr)
                )));
            }
        }

        Ok(Arc::new(WireguardHandle {
            tunnel_name: name,
            dialer: Arc::new(DirectDialer::default()),
            detail: format!("WireGuard · {}", profile.endpoint),
        }))
    }
}

/// Render a standard `wg-quick` style configuration file.
pub fn render_conf(p: &WireguardProfile) -> String {
    let mut s = String::from("[Interface]\n");
    s.push_str(&format!("PrivateKey = {}\n", p.private_key));
    if !p.address.is_empty() {
        s.push_str(&format!("Address = {}\n", p.address.join(", ")));
    }
    if !p.dns.is_empty() {
        s.push_str(&format!("DNS = {}\n", p.dns.join(", ")));
    }
    if p.mtu > 0 {
        s.push_str(&format!("MTU = {}\n", p.mtu));
    }
    s.push_str("\n[Peer]\n");
    s.push_str(&format!("PublicKey = {}\n", p.public_key));
    if let Some(psk) = p.preshared_key.as_ref().filter(|k| !k.is_empty()) {
        s.push_str(&format!("PresharedKey = {psk}\n"));
    }
    s.push_str(&format!("Endpoint = {}\n", p.endpoint));
    let allowed = if p.allowed_ips.is_empty() {
        vec!["0.0.0.0/0".to_string(), "::/0".to_string()]
    } else {
        p.allowed_ips.clone()
    };
    s.push_str(&format!("AllowedIPs = {}\n", allowed.join(", ")));
    if p.persistent_keepalive > 0 {
        s.push_str(&format!(
            "PersistentKeepalive = {}\n",
            p.persistent_keepalive
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conf_contains_required_sections() {
        let p = WireguardProfile {
            private_key: "PRIV".into(),
            public_key: "PUB".into(),
            endpoint: "vpn.example.com:51820".into(),
            address: vec!["10.7.0.2/32".into()],
            dns: vec!["1.1.1.1".into()],
            mtu: 1420,
            ..Default::default()
        };
        let c = render_conf(&p);
        assert!(c.contains("[Interface]"));
        assert!(c.contains("PrivateKey = PRIV"));
        assert!(c.contains("AllowedIPs = 0.0.0.0/0, ::/0"));
    }
}

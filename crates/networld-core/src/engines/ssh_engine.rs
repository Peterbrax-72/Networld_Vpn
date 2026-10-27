//! SSH engine: Direct / SSL-TLS / Proxy+Payload / SlowDNS, all sharing one
//! transport pipeline and one russh session.

use crate::config::{ProfileKind, ServerProfile, SshMode};
use crate::dialer::Dialer;
use crate::engine::{EngineCapabilities, EngineContext, EngineHandle, EngineKind, TunnelEngine};
use crate::engines::slowdns;
use crate::error::{CoreError, Result};
use crate::sidecar::Sidecar;
use crate::ssh::{SshDialer, SshTunnel};
use crate::transport;
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Default)]
pub struct SshEngine;

impl SshEngine {
    pub fn caps(&self) -> EngineCapabilities {
        EngineCapabilities {
            udp: true, // through badvpn-udpgw in TUN mode
            tun_mode: true,
            requires_elevation: false,
            sidecar: None,
            store_safe: true,
        }
    }
}

pub struct SshEngineHandle {
    tunnel: Arc<SshTunnel>,
    dialer: Arc<dyn Dialer>,
    relay: Option<Arc<Sidecar>>,
    detail: String,
}

#[async_trait]
impl EngineHandle for SshEngineHandle {
    fn dialer(&self) -> Arc<dyn Dialer> {
        self.dialer.clone()
    }

    fn info(&self) -> String {
        self.detail.clone()
    }

    fn is_alive(&self) -> bool {
        self.tunnel.is_alive()
    }

    async fn shutdown(&self) {
        self.tunnel.disconnect().await;
        if let Some(s) = &self.relay {
            s.kill().await;
        }
    }
}

#[async_trait]
impl TunnelEngine for SshEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Ssh
    }

    fn capabilities(&self) -> EngineCapabilities {
        self.caps()
    }

    fn validate(&self, profile: &ServerProfile) -> Result<()> {
        let ProfileKind::Ssh(s) = &profile.kind else {
            return Err(CoreError::config("not an SSH profile"));
        };
        if s.host.trim().is_empty() {
            return Err(CoreError::config("Server host is required"));
        }
        if s.port == 0 {
            return Err(CoreError::config("Server port is required"));
        }
        if s.username.trim().is_empty() {
            return Err(CoreError::config("Username is required"));
        }
        if matches!(s.mode, SshMode::Proxy | SshMode::ProxySsl)
            && s.proxy_host
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            return Err(CoreError::config(
                "Proxy host is required for proxy/payload modes",
            ));
        }
        if matches!(s.mode, SshMode::SlowDns) {
            let d = s
                .slowdns
                .as_ref()
                .ok_or_else(|| CoreError::config("SlowDNS settings are missing"))?;
            if d.domain.trim().is_empty() || d.public_key.trim().is_empty() {
                return Err(CoreError::config(
                    "SlowDNS needs both an NS domain and a public key",
                ));
            }
        }
        Ok(())
    }

    async fn start(&self, ctx: EngineContext) -> Result<Arc<dyn EngineHandle>> {
        let ProfileKind::Ssh(profile) = ctx.profile.kind.clone() else {
            return Err(CoreError::config("not an SSH profile"));
        };
        let logs = ctx.logs.clone();

        // Optional SlowDNS relay in front of the transport.
        let relay = if matches!(profile.mode, SshMode::SlowDns) {
            let dns = profile
                .slowdns
                .clone()
                .ok_or_else(|| CoreError::config("SlowDNS settings are missing"))?;
            Some(slowdns::start(&dns, &ctx.paths, logs.clone()).await?)
        } else {
            None
        };

        let tcfg = profile.to_transport();
        logs.info(
            "engine",
            format!("SSH engine starting — {}", tcfg.describe()),
        );
        let stream = transport::connect(&tcfg, &logs).await?;

        let ssh_cfg = profile.to_ssh_config();
        let tunnel = SshTunnel::connect(stream, &ssh_cfg, logs.clone()).await?;

        let detail = format!(
            "{} · {}:{}{}",
            match profile.mode {
                SshMode::Direct => "SSH Direct",
                SshMode::Ssl => "SSH over TLS",
                SshMode::Proxy => "SSH over Proxy",
                SshMode::ProxySsl => "SSH over Proxy+TLS",
                SshMode::SlowDns => "SSH over SlowDNS",
            },
            profile.host,
            profile.port,
            profile
                .sni
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| format!(" · {s}"))
                .unwrap_or_default()
        );

        let dialer: Arc<dyn Dialer> = Arc::new(SshDialer {
            tunnel: tunnel.clone(),
        });

        Ok(Arc::new(SshEngineHandle {
            tunnel,
            dialer,
            relay,
            detail,
        }))
    }
}

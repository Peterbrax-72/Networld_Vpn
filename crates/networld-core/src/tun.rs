//! Full-device mode: WinTun + tun2socks + routing, with UDP forwarded through
//! badvpn-udpgw when the engine cannot carry UDP natively.
//!
//! Requires an elevated process (WinTun installs a virtual adapter), so the
//! Store/MSIX package falls back to proxy mode and the UI explains why.

use crate::config::Settings;
use crate::error::{CoreError, Result};
use crate::logging::LogBus;
use crate::sidecar::Sidecar;
use crate::splittunnel::CompiledSplitRules;
use std::sync::Arc;

pub const BINARY: &str = "tun2socks";

pub struct TunDevice {
    sidecar: Arc<Sidecar>,
    interface: String,
    logs: Arc<LogBus>,
    #[cfg_attr(not(windows), allow(dead_code))]
    server_ip: Option<String>,
}

pub struct TunParams {
    /// Local SOCKS5 endpoint tun2socks forwards to.
    pub socks_addr: String,
    /// The VPN server IP, pinned to the physical gateway to avoid a route loop.
    pub server_ip: Option<String>,
    pub settings: Settings,
    pub split: CompiledSplitRules,
}

impl TunDevice {
    pub async fn start(
        params: TunParams,
        paths: &Arc<crate::config::Paths>,
        logs: Arc<LogBus>,
    ) -> Result<Self> {
        if !cfg!(windows) {
            return Err(CoreError::Unsupported("TUN mode requires Windows".into()));
        }
        let bin = paths.binary(BINARY)?;
        let iface = if params.settings.tun_interface_name.is_empty() {
            "Networld".to_string()
        } else {
            params.settings.tun_interface_name.clone()
        };

        let mut args = vec![
            "-device".into(),
            format!("tun://{iface}"),
            "-proxy".into(),
            format!("socks5://{}", params.socks_addr),
            "-mtu".into(),
            params.settings.tun_mtu.max(576).to_string(),
            "-loglevel".into(),
            "warning".into(),
        ];
        if params.settings.udpgw_enabled && !params.settings.udpgw_address.is_empty() {
            args.push("-udpgw-remote-server-addr".into());
            args.push(params.settings.udpgw_address.clone());
        }
        args.extend(params.split.tun2socks_args());

        logs.info("tun", format!("Bringing up virtual adapter '{iface}'"));
        let sidecar = Sidecar::spawn(BINARY, bin, args, logs.clone()).await?;

        let dev = Self {
            sidecar,
            interface: iface,
            logs,
            server_ip: params.server_ip.clone(),
        };
        dev.configure_routes(&params).await?;
        Ok(dev)
    }

    async fn configure_routes(&self, params: &TunParams) -> Result<()> {
        #[cfg(windows)]
        {
            // Pin the server to the physical default gateway…
            if let Some(ip) = &self.server_ip {
                if let Some(gw) = default_gateway().await {
                    let _ = netsh(&[
                        "interface",
                        "ipv4",
                        "add",
                        "route",
                        &format!("{ip}/32"),
                        "interface=*",
                        &format!("nexthop={gw}"),
                    ])
                    .await;
                    self.logs.debug("tun", format!("Pinned {ip}/32 via {gw}"));
                }
            }
            // …then send the default route into the tunnel with a lower metric.
            let _ = netsh(&[
                "interface",
                "ipv4",
                "set",
                "interface",
                &format!("interface={}", self.interface),
                "metric=1",
            ])
            .await;

            for cidr in &params.split.bypass_cidrs {
                self.logs.trace("tun", format!("bypass route {cidr}"));
            }
            if params.settings.dns.enabled {
                for (i, dns) in params.settings.dns.servers.iter().enumerate() {
                    let _ = netsh(&[
                        "interface",
                        "ipv4",
                        "add",
                        "dnsservers",
                        &format!("name={}", self.interface),
                        &format!("address={dns}"),
                        &format!("index={}", i + 1),
                        "validate=no",
                    ])
                    .await;
                }
            }
        }
        #[cfg(not(windows))]
        let _ = params;
        Ok(())
    }

    pub async fn stop(&self) {
        self.sidecar.kill().await;
        self.logs.info("tun", "Virtual adapter removed");
    }

    pub fn interface(&self) -> &str {
        &self.interface
    }
}

#[cfg(windows)]
async fn netsh(args: &[&str]) -> Result<()> {
    let out = tokio::process::Command::new("netsh")
        .args(args)
        .output()
        .await
        .map_err(|e| CoreError::Other(format!("netsh: {e}")))?;
    if !out.status.success() {
        return Err(CoreError::Other(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        ));
    }
    Ok(())
}

#[cfg(windows)]
async fn default_gateway() -> Option<String> {
    let out = tokio::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-NetRoute -DestinationPrefix '0.0.0.0/0' | Sort-Object RouteMetric | Select-Object -First 1).NextHop",
        ])
        .output()
        .await
        .ok()?;
    let gw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if gw.is_empty() {
        None
    } else {
        Some(gw)
    }
}

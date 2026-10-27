//! Per-app split tunnelling.
//!
//! Enforcement differs per tunnel mode:
//!
//! * **Proxy mode** — apps are steered by the system-proxy bypass list plus the
//!   per-app rules compiled here; anything excluded simply never points at the
//!   local proxy.
//! * **TUN mode** — the rules are handed to tun2socks as include/exclude app
//!   lists, backed by WFP callouts in the sideload build (needs the helper
//!   service; an MSIX Store package cannot install one).
//!
//! The data model and the compiled rule set are platform independent so the UI
//! and tests work everywhere.

use crate::config::{SplitTunnelMode, SplitTunnelSettings};
use crate::error::Result;
use crate::logging::LogBus;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledSplitRules {
    pub mode: SplitTunnelMode,
    /// Lower-cased executable names (`chrome.exe`).
    pub app_names: Vec<String>,
    /// Full paths as entered by the user.
    pub app_paths: Vec<String>,
    pub bypass_cidrs: Vec<String>,
    pub bypass_domains: Vec<String>,
}

impl CompiledSplitRules {
    pub fn is_active(&self) -> bool {
        !matches!(self.mode, SplitTunnelMode::Off)
    }

    /// Should traffic from `exe` go through the tunnel?
    pub fn tunnel_app(&self, exe: &str) -> bool {
        let name = exe
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(exe)
            .to_ascii_lowercase();
        match self.mode {
            SplitTunnelMode::Off => true,
            SplitTunnelMode::Include => self.app_names.contains(&name),
            SplitTunnelMode::Exclude => !self.app_names.contains(&name),
        }
    }

    /// Extra entries for the WinINet proxy bypass list.
    pub fn proxy_bypass_entries(&self) -> Vec<String> {
        let mut v = self.bypass_domains.clone();
        v.extend(self.bypass_cidrs.iter().cloned());
        v
    }

    /// Arguments appended to the tun2socks command line.
    pub fn tun2socks_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        match self.mode {
            SplitTunnelMode::Include if !self.app_paths.is_empty() => {
                args.push("-include-apps".into());
                args.push(self.app_paths.join(","));
            }
            SplitTunnelMode::Exclude if !self.app_paths.is_empty() => {
                args.push("-exclude-apps".into());
                args.push(self.app_paths.join(","));
            }
            _ => {}
        }
        args
    }
}

pub fn compile(settings: &SplitTunnelSettings) -> CompiledSplitRules {
    let app_names = settings
        .apps
        .iter()
        .map(|p| {
            p.rsplit(['\\', '/'])
                .next()
                .unwrap_or(p)
                .to_ascii_lowercase()
        })
        .collect();
    CompiledSplitRules {
        mode: settings.mode,
        app_names,
        app_paths: settings.apps.clone(),
        bypass_cidrs: if settings.bypass_cidrs.is_empty() {
            default_bypass_cidrs()
        } else {
            settings.bypass_cidrs.clone()
        },
        bypass_domains: settings.bypass_domains.clone(),
    }
}

pub fn default_bypass_cidrs() -> Vec<String> {
    vec![
        "10.0.0.0/8".into(),
        "172.16.0.0/12".into(),
        "192.168.0.0/16".into(),
        "127.0.0.0/8".into(),
        "169.254.0.0/16".into(),
        "224.0.0.0/4".into(),
    ]
}

/// Report what the current build can actually enforce.
pub fn apply(rules: &CompiledSplitRules, logs: &Arc<LogBus>) -> Result<()> {
    if !rules.is_active() {
        return Ok(());
    }
    logs.info(
        "splittunnel",
        format!(
            "{:?} mode with {} app rule(s), {} bypass route(s)",
            rules.mode,
            rules.app_names.len(),
            rules.bypass_cidrs.len()
        ),
    );
    if cfg!(not(windows)) {
        logs.warn(
            "splittunnel",
            "Per-app enforcement is Windows-only; rules recorded but not applied",
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(mode: SplitTunnelMode) -> SplitTunnelSettings {
        SplitTunnelSettings {
            mode,
            apps: vec![r"C:\Program Files\Google\Chrome\Application\chrome.exe".into()],
            ..Default::default()
        }
    }

    #[test]
    fn include_mode_only_tunnels_listed_apps() {
        let r = compile(&settings(SplitTunnelMode::Include));
        assert!(r.tunnel_app(r"C:\x\chrome.exe"));
        assert!(!r.tunnel_app(r"C:\x\firefox.exe"));
    }

    #[test]
    fn exclude_mode_inverts() {
        let r = compile(&settings(SplitTunnelMode::Exclude));
        assert!(!r.tunnel_app("chrome.exe"));
        assert!(r.tunnel_app("firefox.exe"));
    }

    #[test]
    fn off_mode_tunnels_everything() {
        let r = compile(&settings(SplitTunnelMode::Off));
        assert!(r.tunnel_app("anything.exe"));
        assert!(!r.is_active());
    }
}

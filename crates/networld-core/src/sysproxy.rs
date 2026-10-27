//! Windows system proxy (WinINet) integration.
//!
//! Flipping `ProxyEnable`/`ProxyServer` in the registry is only half the job —
//! running processes must be told to re-read the settings, which is what the
//! `InternetSetOptionW` calls below do.

use crate::error::Result;
use crate::logging::LogBus;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct PreviousProxy {
    pub enabled: u32,
    pub server: String,
    pub overrides: String,
}

pub const DEFAULT_BYPASS: &str =
    "localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*;<local>";

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    const KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    fn key(write: bool) -> std::io::Result<RegKey> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.open_subkey_with_flags(
            KEY_PATH,
            if write {
                KEY_READ | KEY_WRITE
            } else {
                KEY_READ
            },
        )
    }

    pub fn snapshot() -> Result<PreviousProxy> {
        let k = key(false)?;
        Ok(PreviousProxy {
            enabled: k.get_value::<u32, _>("ProxyEnable").unwrap_or(0),
            server: k.get_value::<String, _>("ProxyServer").unwrap_or_default(),
            overrides: k
                .get_value::<String, _>("ProxyOverride")
                .unwrap_or_default(),
        })
    }

    pub fn set(server: &str, bypass: &str, logs: &Arc<LogBus>) -> Result<()> {
        let k = key(true)?;
        k.set_value("ProxyEnable", &1u32)?;
        k.set_value("ProxyServer", &server.to_string())?;
        k.set_value("ProxyOverride", &bypass.to_string())?;
        refresh();
        logs.info("sysproxy", format!("System proxy set to {server}"));
        Ok(())
    }

    pub fn restore(prev: &PreviousProxy, logs: &Arc<LogBus>) -> Result<()> {
        let k = key(true)?;
        k.set_value("ProxyEnable", &prev.enabled)?;
        k.set_value("ProxyServer", &prev.server)?;
        k.set_value("ProxyOverride", &prev.overrides)?;
        refresh();
        logs.info("sysproxy", "System proxy restored");
        Ok(())
    }

    fn refresh() {
        use windows_sys::Win32::Networking::WinInet::{
            InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
        };
        unsafe {
            InternetSetOptionW(
                std::ptr::null_mut(),
                INTERNET_OPTION_SETTINGS_CHANGED,
                std::ptr::null_mut(),
                0,
            );
            InternetSetOptionW(
                std::ptr::null_mut(),
                INTERNET_OPTION_REFRESH,
                std::ptr::null_mut(),
                0,
            );
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn snapshot() -> Result<PreviousProxy> {
        Ok(PreviousProxy::default())
    }
    pub fn set(server: &str, _bypass: &str, logs: &Arc<LogBus>) -> Result<()> {
        logs.warn(
            "sysproxy",
            format!("System proxy is Windows-only; would have set {server}"),
        );
        Ok(())
    }
    pub fn restore(_prev: &PreviousProxy, _logs: &Arc<LogBus>) -> Result<()> {
        Ok(())
    }
}

pub use imp::{restore, set, snapshot};

/// Convenience wrapper that remembers the previous configuration.
pub struct SystemProxyGuard {
    previous: PreviousProxy,
    logs: Arc<LogBus>,
    active: bool,
    lock: std::path::PathBuf,
}

const LOCK_NAME: &str = "sysproxy.lock";

/// If the app was killed (crash, power loss, task manager) while it owned the
/// system proxy, the machine would be left pointing at a dead local port and
/// the internet would look "broken". Call this once at startup: when the
/// lock file written by `apply` is still there and the registry still holds
/// our value, put the previous settings back.
pub fn recover_stale(dir: &std::path::Path, logs: &Arc<LogBus>) {
    let path = dir.join(LOCK_NAME);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        let ours = v["server"].as_str().unwrap_or_default();
        if let Ok(cur) = snapshot() {
            if cur.enabled == 1 && !ours.is_empty() && cur.server == ours {
                let prev = PreviousProxy {
                    enabled: v["prevEnabled"].as_u64().unwrap_or(0) as u32,
                    server: v["prevServer"].as_str().unwrap_or_default().to_string(),
                    overrides: v["prevOverrides"].as_str().unwrap_or_default().to_string(),
                };
                let _ = restore(&prev, logs);
                logs.info(
                    "sysproxy",
                    "Cleared a proxy setting left by a previous session",
                );
            }
        }
    }
    let _ = std::fs::remove_file(path);
}

impl SystemProxyGuard {
    pub fn apply(
        http_port: u16,
        extra_bypass: &[String],
        logs: Arc<LogBus>,
        lock_dir: &std::path::Path,
    ) -> Result<Self> {
        let previous = snapshot()?;
        let mut bypass = DEFAULT_BYPASS.to_string();
        for b in extra_bypass {
            if !b.trim().is_empty() {
                bypass.push(';');
                bypass.push_str(b.trim());
            }
        }
        let server = format!("127.0.0.1:{http_port}");
        set(&server, &bypass, &logs)?;
        let lock = lock_dir.join(LOCK_NAME);
        let _ = std::fs::create_dir_all(lock_dir);
        let _ = std::fs::write(
            &lock,
            serde_json::json!({
                "server": server,
                "prevEnabled": previous.enabled,
                "prevServer": previous.server,
                "prevOverrides": previous.overrides,
            })
            .to_string(),
        );
        Ok(Self {
            previous,
            logs,
            active: true,
            lock,
        })
    }

    pub fn release(&mut self) {
        if self.active {
            let _ = restore(&self.previous, &self.logs);
            let _ = std::fs::remove_file(&self.lock);
            self.active = false;
        }
    }
}

impl Drop for SystemProxyGuard {
    fn drop(&mut self) {
        self.release();
    }
}

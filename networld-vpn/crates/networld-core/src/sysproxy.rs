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
}

impl SystemProxyGuard {
    pub fn apply(http_port: u16, extra_bypass: &[String], logs: Arc<LogBus>) -> Result<Self> {
        let previous = snapshot()?;
        let mut bypass = DEFAULT_BYPASS.to_string();
        for b in extra_bypass {
            if !b.trim().is_empty() {
                bypass.push(';');
                bypass.push_str(b.trim());
            }
        }
        set(&format!("127.0.0.1:{http_port}"), &bypass, &logs)?;
        Ok(Self {
            previous,
            logs,
            active: true,
        })
    }

    pub fn release(&mut self) {
        if self.active {
            let _ = restore(&self.previous, &self.logs);
            self.active = false;
        }
    }
}

impl Drop for SystemProxyGuard {
    fn drop(&mut self) {
        self.release();
    }
}

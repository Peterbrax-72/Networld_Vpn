//! Profile + settings model and on-disk persistence.
//!
//! Everything the UI edits lives here. The file is written atomically to
//! `%LOCALAPPDATA%\Networld VPN\config.json` (MSIX redirects this per-package).

use crate::error::{CoreError, Result};
use crate::logging::LogLevel;
use crate::payload::PayloadTemplate;
use crate::ssh::{SshAuth, SshConfig};
use crate::stats::DataUsageLedger;
use crate::transport::{
    ProxyOptions, ResponsePosition, TlsOptions, TransportConfig, TransportMode,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

pub const CONFIG_VERSION: u32 = 1;
pub const APP_DIR_NAME: &str = "Networld VPN";

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SshMode {
    /// Plain SSH on the wire.
    #[default]
    Direct,
    /// SSH wrapped in TLS (a.k.a. SSH-SSL / stunnel style), SNI spoofable.
    Ssl,
    /// SSH through an HTTP proxy with an injected payload.
    Proxy,
    /// Proxy + payload, then TLS to the server.
    ProxySsl,
    /// SSH through a SlowDNS/dnstt local relay.
    SlowDns,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SshProfile {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub private_key_path: Option<String>,
    pub key_password: Option<String>,
    pub mode: SshMode,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
    pub proxy_user: Option<String>,
    pub proxy_pass: Option<String>,
    /// Payload template text (see [`crate::payload`]).
    pub payload: String,
    /// SNI / bug host.
    pub sni: Option<String>,
    pub insecure_tls: bool,
    /// `host:port` of the server-side badvpn-udpgw, enables UDP over SSH.
    pub udpgw: Option<String>,
    pub compression: bool,
    pub keepalive_secs: u64,
    /// How many HTTP responses to read after the payload (double CONNECT = 2).
    pub expect_responses: usize,
    pub accept_any_status: bool,
    /// Exact status code the gateway must answer with (NetMod "Response Status").
    /// `None` falls back to the generic 2xx/101 check.
    #[serde(default)]
    pub expected_status: Option<u16>,
    /// Whether the HTTP response arrives before or after the payload is written
    /// (NetMod "Response Position").
    #[serde(default)]
    pub response_position: ResponsePosition,
    pub slowdns: Option<SlowDnsProfile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SlowDnsProfile {
    /// Upstream resolver, e.g. `8.8.8.8:53` or a DoH URL.
    pub resolver: String,
    /// dnstt server public key (hex).
    pub public_key: String,
    /// NS domain delegated to the dnstt server.
    pub domain: String,
    /// `udp` | `doh` | `dot`
    pub transport: String,
    /// Local port the relay listens on; SSH then targets 127.0.0.1:this.
    pub listen_port: u16,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct XrayProfile {
    /// `vmess` | `vless` | `trojan` | `shadowsocks`
    pub protocol: String,
    pub address: String,
    pub port: u16,
    /// UUID for vmess/vless, password for trojan/ss.
    /// Named `uuid` so it cannot collide with `ServerProfile::id` when the
    /// profile kind is flattened into the parent document.
    pub uuid: String,
    /// vmess alterId (legacy), ss method, etc.
    pub security: String,
    pub encryption: String,
    /// `tcp` | `ws` | `grpc` | `http` | `quic` | `httpupgrade`
    pub network: String,
    pub path: String,
    pub host: String,
    pub service_name: String,
    pub sni: String,
    pub flow: String,
    #[serde(default)]
    pub alpn: Vec<String>,
    /// `none` | `tls` | `reality`
    pub tls: String,
    pub fingerprint: String,
    pub public_key: String,
    pub short_id: String,
    pub spider_x: String,
    pub allow_insecure: bool,
    pub mux_enabled: bool,
    pub mux_concurrency: i32,
    /// Hand-written Xray JSON ("JSON Config" in the protocol menu). When set it
    /// replaces the generated document; the local SOCKS inbound is re-injected
    /// so the rest of the app keeps working.
    #[serde(default)]
    pub raw_json: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WireguardProfile {
    pub private_key: String,
    pub public_key: String,
    pub preshared_key: Option<String>,
    pub endpoint: String,
    /// Local interface addresses, e.g. `10.0.0.2/32`.
    pub address: Vec<String>,
    pub dns: Vec<String>,
    pub mtu: u16,
    pub allowed_ips: Vec<String>,
    pub persistent_keepalive: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProfileKind {
    Ssh(SshProfile),
    Xray(XrayProfile),
    Wireguard(WireguardProfile),
}

impl ProfileKind {
    pub fn engine_label(&self) -> &'static str {
        match self {
            ProfileKind::Ssh(_) => "SSH",
            ProfileKind::Xray(_) => "V2Ray / Xray",
            ProfileKind::Wireguard(_) => "WireGuard",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub favorite: bool,
    /// Locked configs hide their credentials in the UI (NetMod-style sharing).
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(flatten)]
    pub kind: ProfileKind,
}

/// A short, collision-resistant profile id (time prefix + random tail).
/// Replaces `uuid::Uuid::new_v4()`; one less dependency in the binary.
pub fn new_id() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    format!("{ms:x}{}", crate::payload::random_alnum(10))
}

/// What a locked config shows in place of every credential, proxy detail and port.
pub const MASK: &str = "••••••••";

fn mask(s: &mut String) {
    if !s.is_empty() {
        *s = MASK.to_string();
    }
}

fn mask_opt(s: &mut Option<String>) {
    if s.as_deref().is_some_and(|v| !v.is_empty()) {
        *s = Some(MASK.to_string());
    }
}

impl ServerProfile {
    /// The copy that is allowed to reach the UI. A locked profile has every
    /// credential (SSH login and key, proxy host/port and login, server port,
    /// UUIDs, keys, raw Xray JSON) hidden; the real values never leave the core.
    pub fn redacted(&self) -> ServerProfile {
        let mut p = self.clone();
        if !p.locked {
            return p;
        }
        match &mut p.kind {
            ProfileKind::Ssh(s) => {
                mask(&mut s.username);
                mask(&mut s.password);
                mask_opt(&mut s.private_key_path);
                mask_opt(&mut s.key_password);
                mask_opt(&mut s.proxy_host);
                mask_opt(&mut s.proxy_user);
                mask_opt(&mut s.proxy_pass);
                // Numbers cannot hold dots: 0 is the "hidden" value and the
                // UI renders it as dots for a locked config.
                s.port = 0;
                if s.proxy_port.is_some() {
                    s.proxy_port = Some(0);
                }
            }
            ProfileKind::Xray(x) => {
                mask(&mut x.uuid);
                mask(&mut x.public_key);
                mask(&mut x.short_id);
                mask_opt(&mut x.raw_json);
                x.port = 0;
            }
            ProfileKind::Wireguard(w) => {
                mask(&mut w.private_key);
                mask(&mut w.public_key);
                mask_opt(&mut w.preshared_key);
                if let Some((host, _)) = w.endpoint.rsplit_once(':') {
                    w.endpoint = format!("{host}:{MASK}");
                }
            }
        }
        p
    }

    pub fn new(name: impl Into<String>, kind: ProfileKind) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            group: String::new(),
            notes: String::new(),
            tags: Vec::new(),
            favorite: false,
            locked: false,
            created_at_ms: crate::logging::now_ms(),
            kind,
        }
    }

    /// `host:port` of whatever endpoint this profile ultimately reaches.
    pub fn endpoint(&self) -> String {
        match &self.kind {
            ProfileKind::Ssh(s) => format!("{}:{}", s.host, s.port),
            ProfileKind::Xray(x) => format!("{}:{}", x.address, x.port),
            ProfileKind::Wireguard(w) => w.endpoint.clone(),
        }
    }

    /// The IP/host the kill switch must keep reachable.
    pub fn server_host(&self) -> String {
        match &self.kind {
            ProfileKind::Ssh(s) => s
                .proxy_host
                .clone()
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| s.host.clone()),
            ProfileKind::Xray(x) => x.address.clone(),
            ProfileKind::Wireguard(w) => w
                .endpoint
                .rsplit_once(':')
                .map(|(h, _)| h.to_string())
                .unwrap_or_default(),
        }
    }
}

impl SshProfile {
    pub fn transport_mode(&self) -> TransportMode {
        match self.mode {
            SshMode::Direct | SshMode::SlowDns => TransportMode::Direct,
            SshMode::Ssl => TransportMode::Tls,
            SshMode::Proxy => TransportMode::Proxy,
            SshMode::ProxySsl => TransportMode::ProxyTls,
        }
    }

    /// Translate the profile into a concrete transport description.
    pub fn to_transport(&self) -> TransportConfig {
        let (host, port) = match self.mode {
            SshMode::SlowDns => (
                "127.0.0.1".to_string(),
                self.slowdns.as_ref().map(|s| s.listen_port).unwrap_or(2222),
            ),
            _ => (self.host.clone(), self.port),
        };
        let proxy = match (self.proxy_host.clone(), self.proxy_port) {
            (Some(h), Some(p)) if !h.is_empty() => Some(ProxyOptions {
                host: h,
                port: p,
                username: self.proxy_user.clone(),
                password: self.proxy_pass.clone(),
            }),
            _ => None,
        };
        TransportConfig {
            mode: self.transport_mode(),
            host,
            port,
            proxy,
            payload: if self.payload.trim().is_empty() {
                None
            } else {
                Some(PayloadTemplate::new(self.payload.clone()))
            },
            bug_host: self.sni.clone().filter(|s| !s.is_empty()),
            tls: TlsOptions {
                sni: self.sni.clone().filter(|s| !s.is_empty()),
                insecure: self.insecure_tls,
                alpn: vec![],
            },
            connect_timeout: Duration::from_secs(20),
            response_timeout: Duration::from_secs(20),
            expect_responses: self.expect_responses.max(1),
            accept_any_status: self.accept_any_status,
            expected_status: self.expected_status,
            response_position: self.response_position,
        }
    }

    pub fn to_ssh_config(&self) -> SshConfig {
        let auth = match (&self.private_key_path, self.password.is_empty()) {
            (Some(p), _) if !p.is_empty() => SshAuth::PrivateKey {
                path: p.clone(),
                key_password: self.key_password.clone(),
            },
            (_, false) => SshAuth::Password {
                password: self.password.clone(),
            },
            _ => SshAuth::None,
        };
        SshConfig {
            username: self.username.clone(),
            auth,
            keepalive_secs: if self.keepalive_secs == 0 {
                30
            } else {
                self.keepalive_secs
            },
            inactivity_timeout_secs: 0,
            compression: self.compression,
            pin_fingerprint: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TunnelMode {
    /// Local SOCKS5 + HTTP proxy (optionally set as the Windows system proxy).
    #[default]
    Proxy,
    /// Full-device capture through WinTun + tun2socks.
    Tun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SplitTunnelMode {
    #[default]
    Off,
    /// Only the listed apps go through the tunnel.
    Include,
    /// Everything except the listed apps goes through the tunnel.
    Exclude,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SplitTunnelSettings {
    pub mode: SplitTunnelMode,
    /// Absolute executable paths.
    pub apps: Vec<String>,
    /// CIDR routes that always bypass the tunnel (LAN by default).
    pub bypass_cidrs: Vec<String>,
    /// Domains that always bypass the tunnel.
    pub bypass_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DnsSettings {
    pub enabled: bool,
    pub servers: Vec<String>,
    /// Resolve names on the server side instead of locally (recommended).
    pub remote_resolve: bool,
    pub doh_url: Option<String>,
}

impl Default for DnsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            servers: vec!["1.1.1.1".into(), "8.8.8.8".into()],
            remote_resolve: true,
            doh_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub theme: String,
    pub accent: String,
    pub language: String,
    pub autostart: bool,
    pub start_minimized: bool,
    pub minimize_to_tray: bool,
    pub connect_on_launch: bool,
    pub auto_reconnect: bool,
    pub max_reconnect_attempts: u32,
    pub reconnect_delay_secs: u64,
    pub tunnel_mode: TunnelMode,
    pub socks_port: u16,
    pub http_port: u16,
    pub allow_lan: bool,
    pub set_system_proxy: bool,
    pub kill_switch: bool,
    pub block_ipv6: bool,
    pub udpgw_enabled: bool,
    pub udpgw_address: String,
    pub dns: DnsSettings,
    pub split_tunnel: SplitTunnelSettings,
    pub notifications: bool,
    pub log_level: LogLevel,
    pub keep_logs_lines: usize,
    pub update_channel: String,
    pub check_updates: bool,
    pub tun_mtu: u16,
    pub tun_interface_name: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "midnight".into(),
            accent: "#3b82f6".into(),
            language: "en".into(),
            autostart: false,
            start_minimized: false,
            minimize_to_tray: true,
            connect_on_launch: false,
            auto_reconnect: true,
            max_reconnect_attempts: 10,
            reconnect_delay_secs: 5,
            tunnel_mode: TunnelMode::Proxy,
            socks_port: 1080,
            http_port: 8086,
            allow_lan: false,
            set_system_proxy: true,
            kill_switch: false,
            block_ipv6: true,
            udpgw_enabled: false,
            udpgw_address: "127.0.0.1:7300".into(),
            dns: DnsSettings::default(),
            split_tunnel: SplitTunnelSettings::default(),
            notifications: true,
            log_level: LogLevel::Info,
            keep_logs_lines: 1500,
            update_channel: "stable".into(),
            check_updates: true,
            tun_mtu: 1420,
            tun_interface_name: "Networld".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Root document
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppConfig {
    pub version: u32,
    pub settings: Settings,
    pub profiles: Vec<ServerProfile>,
    pub active_profile_id: Option<String>,
    pub data_usage: DataUsageLedger,
    /// Saved payload templates from the generator.
    pub payload_library: Vec<SavedPayload>,
    /// Subscription URLs for bulk config import.
    pub subscriptions: Vec<Subscription>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedPayload {
    pub id: String,
    pub name: String,
    pub template: String,
    pub notes: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub url: String,
    pub last_updated_ms: u64,
    pub auto_update: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            settings: Settings::default(),
            profiles: Vec::new(),
            active_profile_id: None,
            data_usage: DataUsageLedger::default(),
            payload_library: Vec::new(),
            subscriptions: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn get(&self, id: &str) -> Option<&ServerProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ServerProfile> {
        self.profiles.iter_mut().find(|p| p.id == id)
    }

    pub fn active(&self) -> Option<&ServerProfile> {
        self.active_profile_id
            .as_deref()
            .and_then(|id| self.get(id))
    }

    pub fn upsert(&mut self, profile: ServerProfile) {
        match self.profiles.iter_mut().find(|p| p.id == profile.id) {
            Some(slot) => *slot = profile,
            None => self.profiles.push(profile),
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.profiles.len();
        self.profiles.retain(|p| p.id != id);
        if self.active_profile_id.as_deref() == Some(id) {
            self.active_profile_id = None;
        }
        before != self.profiles.len()
    }

    /// Forward-compatible migration hook.
    pub fn migrate(&mut self) {
        if self.version < CONFIG_VERSION {
            self.version = CONFIG_VERSION;
        }
    }
}

// ---------------------------------------------------------------------------
// Paths + persistence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub logs_dir: PathBuf,
    /// Folder that ships the sidecar engine binaries (xray, tun2socks…).
    pub bin_dir: PathBuf,
    pub runtime_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Self {
        let base = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_DIR_NAME);
        let bin_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("resources").join("bin")))
            .unwrap_or_else(|| PathBuf::from("resources/bin"));
        Self {
            config_file: base.join("config.json"),
            logs_dir: base.join("logs"),
            runtime_dir: base.join("runtime"),
            config_dir: base,
            bin_dir,
        }
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.logs_dir)?;
        std::fs::create_dir_all(&self.runtime_dir)?;
        Ok(())
    }

    /// Resolve a bundled binary, trying `.exe` on Windows.
    pub fn binary(&self, name: &str) -> Result<PathBuf> {
        let candidates = [
            self.bin_dir.join(name),
            self.bin_dir.join(format!("{name}.exe")),
        ];
        candidates
            .iter()
            .find(|p| p.exists())
            .cloned()
            .ok_or_else(|| CoreError::MissingBinary(name.to_string()))
    }
}

pub fn load(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let text = std::fs::read_to_string(path)?;
    let mut cfg: AppConfig = serde_json::from_str(&text).map_err(|e| {
        CoreError::config(format!("config.json is not valid ({e}); fix or delete it"))
    })?;
    cfg.migrate();
    Ok(cfg)
}

/// Atomic save: write to `*.tmp`, then rename over the real file.
pub fn save(path: &Path, cfg: &AppConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&tmp, json.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_profile_maps_to_transport() {
        let mut p = SshProfile {
            host: "ssh.example.com".into(),
            port: 443,
            mode: SshMode::ProxySsl,
            proxy_host: Some("10.0.0.1".into()),
            proxy_port: Some(8080),
            sni: Some("bug.carrier.net".into()),
            payload: "[raw][crlf][crlf]".into(),
            ..Default::default()
        };
        p.expect_responses = 0;
        let t = p.to_transport();
        assert_eq!(t.mode, TransportMode::ProxyTls);
        assert_eq!(t.expect_responses, 1);
        assert_eq!(t.bug_host.as_deref(), Some("bug.carrier.net"));
        assert!(t.payload.is_some());
    }

    #[test]
    fn config_roundtrip() {
        let mut cfg = AppConfig::default();
        cfg.upsert(ServerProfile::new(
            "Test",
            ProfileKind::Ssh(SshProfile {
                host: "a.b".into(),
                port: 22,
                ..Default::default()
            }),
        ));
        let json = serde_json::to_string(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.profiles.len(), 1);
        assert_eq!(back.profiles[0].name, "Test");
    }

    /// Regression: `ServerProfile::id` and the flattened payload must never
    /// emit the same JSON key (an `XrayProfile` credential field once did).
    #[test]
    fn flattened_kinds_do_not_collide_with_profile_id() {
        for kind in [
            ProfileKind::Ssh(SshProfile::default()),
            ProfileKind::Xray(XrayProfile {
                uuid: "cred".into(),
                ..Default::default()
            }),
            ProfileKind::Wireguard(WireguardProfile::default()),
        ] {
            let p = ServerProfile::new("Round trip", kind);
            let json = serde_json::to_string(&p).unwrap();
            let back: ServerProfile = serde_json::from_str(&json).unwrap();
            assert_eq!(back.id, p.id);
            if let ProfileKind::Xray(x) = &back.kind {
                assert_eq!(x.uuid, "cred");
            }
        }
    }

    #[test]
    fn slowdns_targets_local_relay() {
        let p = SshProfile {
            host: "ssh.example.com".into(),
            port: 22,
            mode: SshMode::SlowDns,
            slowdns: Some(SlowDnsProfile {
                listen_port: 5300,
                ..Default::default()
            }),
            ..Default::default()
        };
        let t = p.to_transport();
        assert_eq!(t.host, "127.0.0.1");
        assert_eq!(t.port, 5300);
    }
}

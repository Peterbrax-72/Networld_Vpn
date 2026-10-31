//! `VpnManager` — the single object the UI layer talks to.
//!
//! Owns the config, the active engine, the local proxies, the kill switch,
//! the system-proxy guard, TUN mode and the auto-reconnect supervisor.

use crate::config::{self, AppConfig, Paths, ServerProfile, Settings, TunnelMode};
use crate::dialer::Dialer;
use crate::engine::{ConnectionState, EngineContext, EngineHandle, EngineKind};
use crate::engines;
use crate::error::{CoreError, Result};
use crate::http_proxy::{HttpProxyOptions, HttpProxyServer};
use crate::killswitch::{KillSwitch, KillSwitchPlan};
use crate::logging::LogBus;
use crate::socks::{DialerSlot, SocksOptions, SocksServer};
use crate::speedtest::{SpeedTestOptions, SpeedTestResult};
use crate::splittunnel;
use crate::stats::{StatsSnapshot, TrafficCounters};
use crate::sysproxy::SystemProxyGuard;
use crate::tun::{TunDevice, TunParams};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSnapshot {
    pub state: ConnectionState,
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub engine: Option<EngineKind>,
    pub detail: String,
    pub message: String,
    pub hint: String,
    pub socks_port: Option<u16>,
    pub http_port: Option<u16>,
    pub tunnel_mode: TunnelMode,
    pub kill_switch: bool,
    pub system_proxy: bool,
    pub connected_since_ms: Option<u64>,
    pub reconnect_attempt: u32,
    pub stats: StatsSnapshot,
}

impl Default for StatusSnapshot {
    fn default() -> Self {
        Self {
            state: ConnectionState::Disconnected,
            profile_id: None,
            profile_name: None,
            engine: None,
            detail: String::new(),
            message: String::new(),
            hint: String::new(),
            socks_port: None,
            http_port: None,
            tunnel_mode: TunnelMode::Proxy,
            kill_switch: false,
            system_proxy: false,
            connected_since_ms: None,
            reconnect_attempt: 0,
            stats: StatsSnapshot::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ManagerEvent {
    Status(StatusSnapshot),
    Stats(StatsSnapshot),
    ConfigChanged,
}

pub struct VpnManager {
    pub logs: Arc<LogBus>,
    pub stats: Arc<TrafficCounters>,
    pub paths: Arc<Paths>,
    config: RwLock<AppConfig>,
    status: RwLock<StatusSnapshot>,
    dialer_slot: DialerSlot,
    engine: RwLock<Option<Arc<dyn EngineHandle>>>,
    socks: Mutex<Option<SocksServer>>,
    http: Mutex<Option<HttpProxyServer>>,
    tun: Mutex<Option<TunDevice>>,
    sysproxy: Mutex<Option<SystemProxyGuard>>,
    killswitch: Mutex<KillSwitch>,
    events: broadcast::Sender<ManagerEvent>,
    connect_lock: Mutex<()>,
}

impl VpnManager {
    pub fn new(paths: Paths) -> Arc<Self> {
        let logs = Arc::new(LogBus::new(1500));
        let paths = Arc::new(paths);
        let _ = paths.ensure();
        let cfg = config::load(&paths.config_file).unwrap_or_else(|e| {
            logs.error("config", format!("{e} — starting from defaults"));
            AppConfig::default()
        });
        logs.set_min_level(cfg.settings.log_level);
        logs.info(
            "app",
            format!(
                "Networld VPN core ready — {} profile(s) loaded",
                cfg.profiles.len()
            ),
        );
        let (events, _) = broadcast::channel(256);

        let mgr = Arc::new(Self {
            killswitch: Mutex::new(KillSwitch::new(logs.clone())),
            logs,
            stats: Arc::new(TrafficCounters::new()),
            paths,
            config: RwLock::new(cfg),
            status: RwLock::new(StatusSnapshot::default()),
            dialer_slot: Arc::new(RwLock::new(None)),
            engine: RwLock::new(None),
            socks: Mutex::new(None),
            http: Mutex::new(None),
            tun: Mutex::new(None),
            sysproxy: Mutex::new(None),
            events,
            connect_lock: Mutex::new(()),
        });
        crate::sysproxy::recover_stale(&mgr.paths.runtime_dir, &mgr.logs);
        mgr.clone().spawn_supervisor();
        crate::netwatch::spawn(mgr.logs.clone());
        mgr
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ManagerEvent> {
        self.events.subscribe()
    }

    // -- config -----------------------------------------------------------

    pub async fn config(&self) -> AppConfig {
        self.config.read().await.clone()
    }

    pub async fn settings(&self) -> Settings {
        self.config.read().await.settings.clone()
    }

    pub async fn save_config(&self) -> Result<()> {
        let cfg = self.config.read().await;
        config::save(&self.paths.config_file, &cfg)?;
        let _ = self.events.send(ManagerEvent::ConfigChanged);
        Ok(())
    }

    pub async fn update_settings(&self, settings: Settings) -> Result<()> {
        {
            let mut cfg = self.config.write().await;
            self.logs.set_min_level(settings.log_level);
            cfg.settings = settings;
        }
        self.save_config().await
    }

    pub async fn upsert_profile(&self, profile: ServerProfile) -> Result<()> {
        {
            let mut cfg = self.config.write().await;
            let id = profile.id.clone();
            cfg.upsert(profile);
            // The app ships with no servers, so the first config to arrive —
            // typed in or imported from a link / file / QR — becomes active.
            if cfg.active_profile_id.is_none() {
                cfg.active_profile_id = Some(id);
            }
        }
        self.save_config().await
    }

    pub async fn delete_profile(&self, id: &str) -> Result<bool> {
        let removed = {
            let mut cfg = self.config.write().await;
            let removed = cfg.remove(id);
            if cfg.active_profile_id.as_deref() == Some(id) {
                cfg.active_profile_id = cfg.profiles.first().map(|p| p.id.clone());
            }
            removed
        };
        self.save_config().await?;
        Ok(removed)
    }

    pub async fn replace_payload_library(
        &self,
        library: Vec<crate::config::SavedPayload>,
    ) -> Result<()> {
        {
            let mut cfg = self.config.write().await;
            cfg.payload_library = library;
        }
        self.save_config().await
    }

    pub async fn replace_subscriptions(
        &self,
        subs: Vec<crate::config::Subscription>,
    ) -> Result<()> {
        {
            let mut cfg = self.config.write().await;
            cfg.subscriptions = subs;
        }
        self.save_config().await
    }

    pub async fn set_active_profile(&self, id: Option<String>) -> Result<()> {
        {
            let mut cfg = self.config.write().await;
            cfg.active_profile_id = id;
        }
        self.save_config().await
    }

    // -- status -----------------------------------------------------------

    pub async fn status(&self) -> StatusSnapshot {
        let mut s = self.status.read().await.clone();
        s.stats = self.stats.snapshot();
        s
    }

    async fn set_state(&self, state: ConnectionState, message: impl Into<String>) {
        let snapshot = {
            let mut st = self.status.write().await;
            let was_connected = st.state == ConnectionState::Connected;
            st.state = state;
            st.message = message.into();
            if state == ConnectionState::Connected && !was_connected {
                st.connected_since_ms = Some(crate::logging::now_ms());
            }
            if state != ConnectionState::Connected {
                st.connected_since_ms = None;
            }
            if state == ConnectionState::Disconnected {
                st.detail.clear();
                st.socks_port = None;
                st.http_port = None;
            }
            st.clone()
        };
        let _ = self.events.send(ManagerEvent::Status(snapshot));
    }

    // -- connect / disconnect ---------------------------------------------

    pub async fn connect(self: &Arc<Self>, profile_id: Option<String>) -> Result<()> {
        let _guard = self.connect_lock.lock().await;

        let (profile, settings) = {
            let cfg = self.config.read().await;
            let id = profile_id
                .or_else(|| cfg.active_profile_id.clone())
                .ok_or_else(|| CoreError::config("No server selected"))?;
            let p = cfg
                .get(&id)
                .cloned()
                .ok_or_else(|| CoreError::config("The selected server no longer exists"))?;
            (p, cfg.settings.clone())
        };

        self.disconnect_inner(false).await;
        self.stats.start_session();

        {
            let mut st = self.status.write().await;
            st.profile_id = Some(profile.id.clone());
            st.profile_name = Some(profile.name.clone());
            st.tunnel_mode = settings.tunnel_mode;
            st.reconnect_attempt = 0;
        }

        let engine = engines::build(&profile);
        engine.validate(&profile)?;
        {
            let mut st = self.status.write().await;
            st.engine = Some(engine.kind());
        }

        self.set_state(
            ConnectionState::Connecting,
            format!("Connecting to {}", profile.name),
        )
        .await;
        self.logs.info(
            "manager",
            format!(
                "Connecting '{}' ({} · {})",
                profile.name,
                engine.kind().label(),
                profile.endpoint()
            ),
        );

        let caps = engine.capabilities();
        if caps.requires_elevation && !is_elevated() {
            self.set_state(
                ConnectionState::Error,
                "This engine needs administrator rights",
            )
            .await;
            return Err(CoreError::ElevationRequired(engine.kind().label().into()));
        }

        self.set_state(ConnectionState::Authenticating, "Authenticating")
            .await;
        let ctx = EngineContext {
            profile: profile.clone(),
            settings: settings.clone(),
            logs: self.logs.clone(),
            stats: self.stats.clone(),
            paths: self.paths.clone(),
        };
        let handle = match engine.start(ctx).await {
            Ok(h) => h,
            Err(e) => {
                self.logs.error("manager", format!("Connect failed: {e}"));
                {
                    let mut st = self.status.write().await;
                    st.hint = e.user_hint().to_string();
                }
                self.set_state(ConnectionState::Error, e.to_string()).await;
                self.stats.end_session();
                return Err(e);
            }
        };

        let dialer: Arc<dyn Dialer> = handle.dialer();
        *self.dialer_slot.write().await = Some(dialer.clone());
        *self.engine.write().await = Some(handle.clone());

        self.set_state(ConnectionState::StartingProxies, "Starting local proxies")
            .await;
        let socks_addr: SocketAddr = format!("127.0.0.1:{}", settings.socks_port)
            .parse()
            .map_err(|_| CoreError::config("Invalid SOCKS port"))?;
        let http_addr: SocketAddr = format!("127.0.0.1:{}", settings.http_port)
            .parse()
            .map_err(|_| CoreError::config("Invalid HTTP proxy port"))?;

        // A busy port must never stop the user from connecting: fall back to a
        // free one (the real port is reported in the status and used for the
        // system proxy).
        let socks_opts = |bind: SocketAddr| SocksOptions {
            bind,
            auth: None,
            allow_lan: settings.allow_lan,
            udp_enabled: settings.udpgw_enabled,
        };
        let socks = match SocksServer::start(
            socks_opts(socks_addr),
            self.dialer_slot.clone(),
            self.stats.clone(),
            self.logs.clone(),
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                self.logs.warn(
                    "manager",
                    format!(
                        "SOCKS port {} unavailable ({e}); using a free port",
                        socks_addr.port()
                    ),
                );
                let free: SocketAddr = ([127, 0, 0, 1], 0).into();
                SocksServer::start(
                    socks_opts(free),
                    self.dialer_slot.clone(),
                    self.stats.clone(),
                    self.logs.clone(),
                )
                .await?
            }
        };
        let socks_port = socks.local_addr.port();
        *self.socks.lock().await = Some(socks);

        let http_opts = |bind: SocketAddr| HttpProxyOptions {
            bind,
            allow_lan: settings.allow_lan,
            auth: None,
        };
        let http = match HttpProxyServer::start(
            http_opts(http_addr),
            self.dialer_slot.clone(),
            self.stats.clone(),
            self.logs.clone(),
        )
        .await
        {
            Ok(h) => h,
            Err(e) => {
                self.logs.warn(
                    "manager",
                    format!(
                        "HTTP port {} unavailable ({e}); using a free port",
                        http_addr.port()
                    ),
                );
                let free: SocketAddr = ([127, 0, 0, 1], 0).into();
                HttpProxyServer::start(
                    http_opts(free),
                    self.dialer_slot.clone(),
                    self.stats.clone(),
                    self.logs.clone(),
                )
                .await?
            }
        };
        let http_port = http.local_addr.port();
        *self.http.lock().await = Some(http);

        // Split tunnel rules
        let split = splittunnel::compile(&settings.split_tunnel);
        splittunnel::apply(&split, &self.logs)?;

        // TUN mode (full device capture)
        if matches!(settings.tunnel_mode, TunnelMode::Tun) {
            let server_ip = crate::transport::resolve_one(&profile.server_host(), 80)
                .await
                .ok()
                .map(|a| a.ip().to_string());
            match TunDevice::start(
                TunParams {
                    socks_addr: format!("127.0.0.1:{socks_port}"),
                    server_ip,
                    settings: settings.clone(),
                    split: split.clone(),
                },
                &self.paths,
                self.logs.clone(),
            )
            .await
            {
                Ok(dev) => *self.tun.lock().await = Some(dev),
                Err(e) => {
                    self.logs.warn(
                        "manager",
                        format!("TUN mode unavailable ({e}); continuing in proxy mode"),
                    );
                }
            }
        } else if settings.set_system_proxy {
            match SystemProxyGuard::apply(
                http_port,
                &split.proxy_bypass_entries(),
                self.logs.clone(),
                &self.paths.runtime_dir,
            ) {
                Ok(g) => *self.sysproxy.lock().await = Some(g),
                Err(e) => self
                    .logs
                    .warn("manager", format!("Could not set the system proxy: {e}")),
            }
        }

        // Kill switch
        if settings.kill_switch {
            let plan = KillSwitchPlan {
                app_path: std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                server_host: profile.server_host(),
                allow_lan: settings.allow_lan,
                block_ipv6: settings.block_ipv6,
            };
            if let Err(e) = self.killswitch.lock().await.engage(&plan).await {
                self.logs
                    .warn("manager", format!("Kill switch could not be engaged: {e}"));
            }
        }
        // Report what is really in force, not what was requested.
        let kill_switch_engaged = self.killswitch.lock().await.is_engaged();

        {
            let mut st = self.status.write().await;
            st.socks_port = Some(socks_port);
            st.http_port = Some(http_port);
            st.detail = handle.info();
            st.kill_switch = kill_switch_engaged;
            st.system_proxy =
                settings.set_system_proxy && matches!(settings.tunnel_mode, TunnelMode::Proxy);
            st.hint.clear();
        }
        self.set_state(ConnectionState::Connected, "Connected")
            .await;
        self.logs.info(
            "manager",
            format!("Connected — SOCKS5 127.0.0.1:{socks_port} · HTTP 127.0.0.1:{http_port}"),
        );
        Ok(())
    }

    pub async fn disconnect(self: &Arc<Self>) -> Result<()> {
        let _guard = self.connect_lock.lock().await;
        self.set_state(ConnectionState::Disconnecting, "Disconnecting")
            .await;
        self.disconnect_inner(true).await;
        self.set_state(ConnectionState::Disconnected, "Disconnected")
            .await;
        Ok(())
    }

    async fn disconnect_inner(&self, record_usage: bool) {
        if let Some(mut g) = self.sysproxy.lock().await.take() {
            g.release();
        }
        self.killswitch.lock().await.release().await;
        if let Some(t) = self.tun.lock().await.take() {
            t.stop().await;
        }
        if let Some(s) = self.socks.lock().await.take() {
            s.stop();
        }
        if let Some(h) = self.http.lock().await.take() {
            h.stop();
        }
        *self.dialer_slot.write().await = None;
        if let Some(e) = self.engine.write().await.take() {
            e.shutdown().await;
        }

        if record_usage {
            let snap = self.stats.snapshot();
            if snap.bytes_up > 0 || snap.bytes_down > 0 {
                let mut cfg = self.config.write().await;
                cfg.data_usage
                    .record(snap.bytes_up, snap.bytes_down, snap.uptime_secs);
                let _ = config::save(&self.paths.config_file, &cfg);
            }
        }
        self.stats.end_session();
    }

    // -- supervisor --------------------------------------------------------

    fn spawn_supervisor(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                ticker.tick().await;
                let snapshot = self.stats.tick();
                let state = self.status.read().await.state;
                if state == ConnectionState::Connected {
                    let _ = self.events.send(ManagerEvent::Stats(snapshot));

                    let alive = match self.engine.read().await.as_ref() {
                        Some(e) => e.is_alive(),
                        None => false,
                    };
                    if !alive {
                        self.logs
                            .warn("manager", "Tunnel went down — starting auto-reconnect");
                        let settings = self.settings().await;
                        if settings.auto_reconnect {
                            self.clone().reconnect_loop(settings).await;
                        } else {
                            self.disconnect_inner(true).await;
                            self.set_state(ConnectionState::Disconnected, "Tunnel closed")
                                .await;
                        }
                    }
                }
            }
        });
    }

    async fn reconnect_loop(self: Arc<Self>, settings: Settings) {
        let mut profile_id = self.status.read().await.profile_id.clone();
        let switching = settings.auto_switch_config;
        let per_config = if switching {
            SWITCH_AFTER_ATTEMPTS
        } else {
            settings.max_reconnect_attempts.max(1)
        };
        let mut tried: Vec<String> = profile_id.iter().cloned().collect();
        loop {
            for attempt in 1..=per_config {
                {
                    let mut st = self.status.write().await;
                    st.reconnect_attempt = attempt;
                }
                self.set_state(
                    ConnectionState::Reconnecting,
                    format!("Reconnecting (attempt {attempt})"),
                )
                .await;
                tokio::time::sleep(std::time::Duration::from_secs(
                    settings.reconnect_delay_secs.max(1),
                ))
                .await;
                match self.connect(profile_id.clone()).await {
                    Ok(_) => {
                        self.logs.info("manager", "Reconnected successfully");
                        return;
                    }
                    Err(e) => self.logs.warn(
                        "manager",
                        format!("Reconnect attempt {attempt} failed: {e}"),
                    ),
                }
            }
            if !switching {
                break;
            }
            let ids: Vec<String> = self
                .config
                .read()
                .await
                .profiles
                .iter()
                .map(|p| p.id.clone())
                .collect();
            let current = profile_id.clone().unwrap_or_default();
            match next_profile_id(&ids, &current, &tried) {
                Some(next) => {
                    self.logs
                        .info("manager", "Config keeps failing, switching to the next one");
                    tried.push(next.clone());
                    profile_id = Some(next);
                }
                None => {
                    self.logs.warn("manager", "Tried every saved config");
                    break;
                }
            }
        }
        self.disconnect_inner(true).await;
        self.set_state(ConnectionState::Error, "Auto-reconnect gave up")
            .await;
    }

    // -- tools -------------------------------------------------------------

    pub async fn speed_test(&self, opts: SpeedTestOptions) -> Result<SpeedTestResult> {
        let dialer = self
            .dialer_slot
            .read()
            .await
            .clone()
            .ok_or_else(|| CoreError::config("Connect first, then run the speed test"))?;
        crate::speedtest::run(dialer, &opts, self.logs.clone()).await
    }

    /// Dry-run a profile: build the transport, authenticate, then tear down.
    pub async fn test_profile(&self, profile: &ServerProfile) -> Result<String> {
        let engine = engines::build(profile);
        engine.validate(profile)?;
        let started = std::time::Instant::now();
        let ctx = EngineContext {
            profile: profile.clone(),
            settings: self.settings().await,
            logs: self.logs.clone(),
            stats: self.stats.clone(),
            paths: self.paths.clone(),
        };
        let handle = engine.start(ctx).await?;
        let info = handle.info();
        handle.shutdown().await;
        Ok(format!(
            "{info} — handshake in {} ms",
            started.elapsed().as_millis()
        ))
    }
}

/// Failed attempts on one config before moving on (when auto-switch is on).
const SWITCH_AFTER_ATTEMPTS: u32 = 3;

/// Next config after `current` (cyclic) that has not been tried yet.
fn next_profile_id(ids: &[String], current: &str, tried: &[String]) -> Option<String> {
    let start = ids.iter().position(|i| i == current).map_or(0, |p| p + 1);
    (0..ids.len())
        .map(|k| &ids[(start + k) % ids.len()])
        .find(|id| !tried.contains(id))
        .cloned()
}

#[cfg(windows)]
fn is_elevated() -> bool {
    // Best-effort: writing to a protected path is the cheapest reliable probe.
    std::fs::metadata(r"C:\Windows\System32\config\system").is_ok()
}

#[cfg(not(windows))]
fn is_elevated() -> bool {
    false
}

#[cfg(test)]
mod switch_tests {
    use super::next_profile_id;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn walks_cyclically_and_stops_when_exhausted() {
        let ids = v(&["a", "b", "c"]);
        assert_eq!(next_profile_id(&ids, "b", &v(&["b"])), Some("c".into()));
        assert_eq!(
            next_profile_id(&ids, "c", &v(&["b", "c"])),
            Some("a".into())
        );
        assert_eq!(next_profile_id(&ids, "a", &v(&["a", "b", "c"])), None);
        assert_eq!(next_profile_id(&v(&["a"]), "a", &v(&["a"])), None);
    }
}

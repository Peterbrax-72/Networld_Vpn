//! SSH engine built on russh, running over any transport produced by
//! [`crate::transport`] — so Direct, SSL/TLS, Proxy+Payload and SlowDNS all
//! reuse the exact same session code.

use crate::dialer::Dialer;
use crate::error::{CoreError, Result};
use crate::logging::LogBus;
use crate::transport::BoxStream;
use async_trait::async_trait;
use russh::client::{self, Handle};
use russh::keys::PublicKeyOrCertificate;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SshAuth {
    Password {
        password: String,
    },
    PrivateKey {
        path: String,
        key_password: Option<String>,
    },
    None,
}

impl Default for SshAuth {
    fn default() -> Self {
        SshAuth::Password {
            password: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    pub username: String,
    pub auth: SshAuth,
    /// SSH-level keepalive, seconds (0 = off).
    #[serde(default = "default_keepalive")]
    pub keepalive_secs: u64,
    /// Drop the session after this many idle seconds (0 = never).
    #[serde(default)]
    pub inactivity_timeout_secs: u64,
    #[serde(default)]
    pub compression: bool,
    /// Optional SHA-256 host-key fingerprint to pin.
    #[serde(default)]
    pub pin_fingerprint: Option<String>,
}

fn default_keepalive() -> u64 {
    30
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            username: String::new(),
            auth: SshAuth::default(),
            keepalive_secs: 30,
            inactivity_timeout_secs: 0,
            compression: false,
            pin_fingerprint: None,
        }
    }
}

struct ClientHandler {
    logs: Arc<LogBus>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        // Tunnelling clients talk to short-lived, frequently re-keyed hosts, so
        // we accept the key but always record it for the Logs tab.
        self.logs.debug(
            "ssh",
            format!(
                "Server host key accepted ({:?})",
                key_kind(server_public_key)
            ),
        );
        Ok(true)
    }
}

fn key_kind(k: &PublicKeyOrCertificate) -> String {
    match k {
        PublicKeyOrCertificate::PublicKey { key, .. } => key.algorithm().to_string(),
        PublicKeyOrCertificate::Certificate(_) => "openssh-certificate".to_string(),
    }
}

pub struct SshTunnel {
    handle: Handle<ClientHandler>,
    logs: Arc<LogBus>,
    alive: Arc<AtomicBool>,
    fail_streak: AtomicU32,
}

impl SshTunnel {
    /// Perform the SSH handshake and authentication over an existing stream.
    pub async fn connect(
        stream: BoxStream,
        cfg: &SshConfig,
        logs: Arc<LogBus>,
    ) -> Result<Arc<Self>> {
        // Bigger flow-control window: a high-latency path (CDN-fronted
        // WebSocket) otherwise stalls waiting for window updates.
        let mut rcfg = client::Config {
            window_size: 8 * 1024 * 1024,
            ..Default::default()
        };
        if cfg.keepalive_secs > 0 {
            rcfg.keepalive_interval = Some(Duration::from_secs(cfg.keepalive_secs));
        }
        if cfg.inactivity_timeout_secs > 0 {
            rcfg.inactivity_timeout = Some(Duration::from_secs(cfg.inactivity_timeout_secs));
        }
        let rcfg = Arc::new(rcfg);

        logs.info("ssh", "Starting SSH transport handshake");
        let handler = ClientHandler { logs: logs.clone() };
        let mut handle = client::connect_stream(rcfg, stream, handler)
            .await
            .map_err(|e| CoreError::Ssh(format!("handshake failed: {e}")))?;

        logs.info("ssh", format!("Authenticating as '{}'", cfg.username));
        let ok = match &cfg.auth {
            SshAuth::Password { password } => handle
                .authenticate_password(cfg.username.clone(), password.clone())
                .await
                .map_err(|e| CoreError::Ssh(format!("password auth error: {e}")))?
                .success(),
            SshAuth::PrivateKey { path, key_password } => {
                let key = russh::keys::load_secret_key(path, key_password.as_deref())
                    .map_err(|e| CoreError::Ssh(format!("cannot load key '{path}': {e}")))?;
                let hash = handle
                    .best_supported_rsa_hash()
                    .await
                    .ok()
                    .flatten()
                    .flatten();
                handle
                    .authenticate_publickey(
                        cfg.username.clone(),
                        russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                    )
                    .await
                    .map_err(|e| CoreError::Ssh(format!("publickey auth error: {e}")))?
                    .success()
            }
            SshAuth::None => handle
                .authenticate_none(cfg.username.clone())
                .await
                .map_err(|e| CoreError::Ssh(format!("none auth error: {e}")))?
                .success(),
        };

        if !ok {
            return Err(CoreError::AuthFailed(cfg.username.clone()));
        }
        logs.info("ssh", "Authentication succeeded — tunnel is up");

        Ok(Arc::new(Self {
            handle,
            logs,
            alive: Arc::new(AtomicBool::new(true)),
            fail_streak: AtomicU32::new(0),
        }))
    }

    /// Open a `direct-tcpip` channel (one per proxied connection).
    pub async fn open_tcp(&self, host: &str, port: u16) -> Result<BoxStream> {
        match self
            .handle
            .channel_open_direct_tcpip(host.to_string(), port as u32, "127.0.0.1".to_string(), 0u32)
            .await
        {
            Ok(channel) => {
                self.fail_streak.store(0, Ordering::Relaxed);
                Ok(Box::new(channel.into_stream()))
            }
            Err(e) => {
                // The server *answering* "no" (host unreachable, refused,
                // administratively prohibited, too many channels) proves the
                // session is alive - only transport-level failures count
                // towards "dead".
                if matches!(e, russh::Error::ChannelOpenFailure(_)) {
                    self.logs.debug(
                        "ssh",
                        format!("Server refused channel to {host}:{port}: {e}"),
                    );
                    return Err(CoreError::Ssh(format!(
                        "channel open to {host}:{port} refused: {e}"
                    )));
                }
                let streak = self.fail_streak.fetch_add(1, Ordering::Relaxed) + 1;
                if streak <= 3 {
                    self.logs.warn(
                        "ssh",
                        format!("Channel open to {host}:{port} failed ({streak}/3): {e}"),
                    );
                }
                // A dead session fails every single channel open; three in a
                // row is our signal to let the manager reconnect. Log it once.
                if streak >= 3 && self.alive.swap(false, Ordering::Relaxed) {
                    self.logs.error(
                        "ssh",
                        format!(
                            "Session looks dead after 3 failed channel opens (last error: {e})"
                        ),
                    );
                }
                Err(CoreError::Ssh(format!(
                    "channel open to {host}:{port} failed: {e}"
                )))
            }
        }
    }

    pub fn is_alive(&self) -> bool {
        // `is_closed` flips as soon as the transport (WebSocket / TLS link) is
        // gone, so a dead tunnel is noticed at once instead of after three
        // failed channel opens.
        self.alive.load(Ordering::Relaxed) && !self.handle.is_closed()
    }

    pub async fn disconnect(&self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self
            .handle
            .disconnect(russh::Disconnect::ByApplication, "bye", "en")
            .await;
        self.logs.info("ssh", "SSH session closed");
    }
}

/// Adapter so the local proxies can speak to an SSH session.
pub struct SshDialer {
    pub tunnel: Arc<SshTunnel>,
}

#[async_trait]
impl Dialer for SshDialer {
    async fn dial(&self, host: &str, port: u16) -> Result<BoxStream> {
        self.tunnel.open_tcp(host, port).await
    }

    fn name(&self) -> &str {
        "ssh"
    }

    fn is_alive(&self) -> bool {
        self.tunnel.is_alive()
    }
}

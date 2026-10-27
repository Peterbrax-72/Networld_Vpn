//! The `Dialer` abstraction — the single seam between "how the tunnel works"
//! and "what the local proxies do with it".
//!
//! Every engine (SSH, Xray/V2Ray, WireGuard, SlowDNS…) ultimately hands the
//! manager a `Dialer`, and the SOCKS5 / HTTP front-ends only ever see this.

use crate::error::{CoreError, Result};
use crate::transport::{resolve_one, BoxStream};
use async_trait::async_trait;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[async_trait]
pub trait Dialer: Send + Sync {
    /// Open a TCP stream to `host:port` through the tunnel.
    async fn dial(&self, host: &str, port: u16) -> Result<BoxStream>;

    fn name(&self) -> &str {
        "dialer"
    }

    /// False once the underlying tunnel is known to be dead, so the manager
    /// can trigger auto-reconnect.
    fn is_alive(&self) -> bool {
        true
    }
}

/// Bypasses the tunnel — used for split-tunnel "exclude" rules and self-tests.
pub struct DirectDialer {
    pub timeout: Duration,
}

impl Default for DirectDialer {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(15),
        }
    }
}

#[async_trait]
impl Dialer for DirectDialer {
    async fn dial(&self, host: &str, port: u16) -> Result<BoxStream> {
        let addr = resolve_one(host, port).await?;
        let s = tokio::time::timeout(self.timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| CoreError::Timeout(self.timeout))??;
        s.set_nodelay(true).ok();
        Ok(Box::new(s))
    }

    fn name(&self) -> &str {
        "direct"
    }
}

/// Chains into an upstream SOCKS5 listener — this is how the sidecar engines
/// (Xray/V2Ray, Trojan, Shadowsocks, tun2socks) plug into the same pipeline.
pub struct Socks5Dialer {
    pub addr: SocketAddr,
    pub auth: Option<(String, String)>,
    pub timeout: Duration,
    pub alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Socks5Dialer {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            auth: None,
            timeout: Duration::from_secs(15),
            alive: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }
}

#[async_trait]
impl Dialer for Socks5Dialer {
    async fn dial(&self, host: &str, port: u16) -> Result<BoxStream> {
        let mut s = tokio::time::timeout(self.timeout, TcpStream::connect(self.addr))
            .await
            .map_err(|_| CoreError::Timeout(self.timeout))??;
        s.set_nodelay(true).ok();

        // Greeting
        let methods: &[u8] = if self.auth.is_some() {
            &[0x00, 0x02]
        } else {
            &[0x00]
        };
        let mut greet = vec![0x05u8, methods.len() as u8];
        greet.extend_from_slice(methods);
        s.write_all(&greet).await?;
        let mut resp = [0u8; 2];
        s.read_exact(&mut resp).await?;
        if resp[0] != 0x05 {
            return Err(CoreError::Proxy("upstream is not SOCKS5".into()));
        }
        match resp[1] {
            0x00 => {}
            0x02 => {
                let (u, p) = self
                    .auth
                    .clone()
                    .ok_or_else(|| CoreError::Proxy("upstream demands authentication".into()))?;
                let mut req = vec![0x01u8, u.len() as u8];
                req.extend_from_slice(u.as_bytes());
                req.push(p.len() as u8);
                req.extend_from_slice(p.as_bytes());
                s.write_all(&req).await?;
                let mut ar = [0u8; 2];
                s.read_exact(&mut ar).await?;
                if ar[1] != 0x00 {
                    return Err(CoreError::Proxy("upstream rejected credentials".into()));
                }
            }
            m => {
                return Err(CoreError::Proxy(format!(
                    "unsupported SOCKS5 method 0x{m:02x}"
                )))
            }
        }

        // CONNECT request (always as a domain name — let the far end resolve).
        let host_bytes = host.as_bytes();
        if host_bytes.len() > 255 {
            return Err(CoreError::Proxy("host name too long for SOCKS5".into()));
        }
        let mut req = vec![0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8];
        req.extend_from_slice(host_bytes);
        req.extend_from_slice(&port.to_be_bytes());
        s.write_all(&req).await?;

        let mut head = [0u8; 4];
        s.read_exact(&mut head).await?;
        if head[1] != 0x00 {
            self.alive
                .store(false, std::sync::atomic::Ordering::Relaxed);
            return Err(CoreError::Proxy(format!(
                "upstream SOCKS5 refused: {}",
                socks_reply_text(head[1])
            )));
        }
        match head[3] {
            0x01 => {
                let mut skip = [0u8; 4 + 2];
                s.read_exact(&mut skip).await?;
            }
            0x03 => {
                let mut len = [0u8; 1];
                s.read_exact(&mut len).await?;
                let mut skip = vec![0u8; len[0] as usize + 2];
                s.read_exact(&mut skip).await?;
            }
            0x04 => {
                let mut skip = [0u8; 16 + 2];
                s.read_exact(&mut skip).await?;
            }
            t => return Err(CoreError::Proxy(format!("bad SOCKS5 address type {t}"))),
        }
        Ok(Box::new(s))
    }

    fn name(&self) -> &str {
        "socks5-upstream"
    }

    fn is_alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::Relaxed)
    }
}

pub fn socks_reply_text(code: u8) -> &'static str {
    match code {
        0x00 => "succeeded",
        0x01 => "general failure",
        0x02 => "connection not allowed",
        0x03 => "network unreachable",
        0x04 => "host unreachable",
        0x05 => "connection refused",
        0x06 => "TTL expired",
        0x07 => "command not supported",
        0x08 => "address type not supported",
        _ => "unknown error",
    }
}

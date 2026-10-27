//! Local SOCKS5 front-end (RFC 1928 + RFC 1929 user/password auth).
//!
//! Applications — or tun2socks in full-device mode — point at this listener and
//! every accepted connection is forwarded through the active [`Dialer`].

use crate::dialer::Dialer;
use crate::error::{CoreError, Result};
use crate::logging::LogBus;
use crate::relay;
use crate::stats::TrafficCounters;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, RwLock};

/// Hot-swappable dialer slot: reconnects replace the dialer without ever
/// dropping the listener, so apps keep their proxy settings.
pub type DialerSlot = Arc<RwLock<Option<Arc<dyn Dialer>>>>;

#[derive(Debug, Clone)]
pub struct SocksOptions {
    pub bind: SocketAddr,
    pub auth: Option<(String, String)>,
    pub allow_lan: bool,
    pub udp_enabled: bool,
}

impl Default for SocksOptions {
    fn default() -> Self {
        Self {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1080),
            auth: None,
            allow_lan: false,
            udp_enabled: false,
        }
    }
}

pub struct SocksServer {
    pub local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
}

impl SocksServer {
    pub async fn start(
        opts: SocksOptions,
        dialer: DialerSlot,
        stats: Arc<TrafficCounters>,
        logs: Arc<LogBus>,
    ) -> Result<Self> {
        let mut bind = opts.bind;
        if opts.allow_lan {
            bind.set_ip(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        }
        let listener = TcpListener::bind(bind)
            .await
            .map_err(|e| CoreError::Proxy(format!("cannot bind SOCKS5 on {bind}: {e}")))?;
        let local_addr = listener.local_addr()?;
        logs.info("socks", format!("SOCKS5 listening on {local_addr}"));

        let (tx, mut rx) = watch::channel(false);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = rx.changed() => {
                        if *rx.borrow() { break; }
                    }
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((sock, peer)) => {
                                let dialer = dialer.clone();
                                let stats = stats.clone();
                                let logs = logs.clone();
                                let opts = opts.clone();
                                tokio::spawn(async move {
                                    stats.conn_opened();
                                    if let Err(e) = handle_conn(sock, peer, opts, dialer, stats.clone(), logs.clone()).await {
                                        logs.debug("socks", format!("{peer} → {e}"));
                                    }
                                    stats.conn_closed();
                                });
                            }
                            Err(e) => {
                                logs.warn("socks", format!("accept failed: {e}"));
                                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            }
                        }
                    }
                }
            }
            logs.info("socks", "SOCKS5 listener stopped");
        });

        Ok(Self {
            local_addr,
            shutdown: tx,
        })
    }

    pub fn stop(&self) {
        let _ = self.shutdown.send(true);
    }
}

impl Drop for SocksServer {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn handle_conn(
    mut sock: TcpStream,
    peer: SocketAddr,
    opts: SocksOptions,
    dialer: DialerSlot,
    stats: Arc<TrafficCounters>,
    logs: Arc<LogBus>,
) -> Result<()> {
    sock.set_nodelay(true).ok();

    // ---- greeting ---------------------------------------------------------
    let mut head = [0u8; 2];
    sock.read_exact(&mut head).await?;
    if head[0] != 0x05 {
        return Err(CoreError::Proxy("not a SOCKS5 client".into()));
    }
    let n = head[1] as usize;
    let mut methods = vec![0u8; n];
    sock.read_exact(&mut methods).await?;

    let want_auth = opts.auth.is_some();
    let chosen = if want_auth && methods.contains(&0x02) {
        0x02
    } else if !want_auth && methods.contains(&0x00) {
        0x00
    } else {
        sock.write_all(&[0x05, 0xFF]).await?;
        return Err(CoreError::Proxy("no acceptable auth method".into()));
    };
    sock.write_all(&[0x05, chosen]).await?;

    if chosen == 0x02 {
        let mut vh = [0u8; 2];
        sock.read_exact(&mut vh).await?;
        let mut user = vec![0u8; vh[1] as usize];
        sock.read_exact(&mut user).await?;
        let mut pl = [0u8; 1];
        sock.read_exact(&mut pl).await?;
        let mut pass = vec![0u8; pl[0] as usize];
        sock.read_exact(&mut pass).await?;
        let ok = opts
            .auth
            .as_ref()
            .map(|(u, p)| u.as_bytes() == user.as_slice() && p.as_bytes() == pass.as_slice())
            .unwrap_or(false);
        sock.write_all(&[0x01, if ok { 0x00 } else { 0x01 }])
            .await?;
        if !ok {
            return Err(CoreError::Proxy(format!("{peer} failed SOCKS5 auth")));
        }
    }

    // ---- request ----------------------------------------------------------
    let mut req = [0u8; 4];
    sock.read_exact(&mut req).await?;
    let cmd = req[1];
    let (host, port) = read_target(&mut sock, req[3]).await?;

    match cmd {
        0x01 => {} // CONNECT
        0x03 => {
            // UDP ASSOCIATE — served by the UDPGW relay in TUN mode only.
            let msg = if opts.udp_enabled {
                "UDP ASSOCIATE needs the UDPGW relay; enable it in Settings → Tunnel"
            } else {
                "UDP ASSOCIATE is disabled"
            };
            logs.debug("socks", msg);
            reply(&mut sock, 0x07).await?;
            return Ok(());
        }
        _ => {
            reply(&mut sock, 0x07).await?;
            return Err(CoreError::Proxy(format!(
                "unsupported SOCKS5 command {cmd}"
            )));
        }
    }

    let active = { dialer.read().await.clone() };
    let Some(active) = active else {
        reply(&mut sock, 0x03).await?;
        return Err(CoreError::Proxy("tunnel is not connected".into()));
    };

    logs.trace("socks", format!("{peer} ⇒ {host}:{port}"));
    match active.dial(&host, port).await {
        Ok(remote) => {
            reply(&mut sock, 0x00).await?;
            let (up, down) = relay::pipe(Box::new(sock), remote, stats).await;
            logs.trace("socks", format!("{host}:{port} closed (↑{up}B ↓{down}B)"));
            Ok(())
        }
        Err(e) => {
            reply(&mut sock, 0x05).await?;
            Err(CoreError::Proxy(format!("dial {host}:{port} failed: {e}")))
        }
    }
}

async fn read_target(sock: &mut TcpStream, atyp: u8) -> Result<(String, u16)> {
    let host = match atyp {
        0x01 => {
            let mut b = [0u8; 4];
            sock.read_exact(&mut b).await?;
            Ipv4Addr::from(b).to_string()
        }
        0x03 => {
            let mut l = [0u8; 1];
            sock.read_exact(&mut l).await?;
            let mut d = vec![0u8; l[0] as usize];
            sock.read_exact(&mut d).await?;
            String::from_utf8_lossy(&d).to_string()
        }
        0x04 => {
            let mut b = [0u8; 16];
            sock.read_exact(&mut b).await?;
            std::net::Ipv6Addr::from(b).to_string()
        }
        other => return Err(CoreError::Proxy(format!("bad address type {other}"))),
    };
    let mut p = [0u8; 2];
    sock.read_exact(&mut p).await?;
    Ok((host, u16::from_be_bytes(p)))
}

async fn reply(sock: &mut TcpStream, code: u8) -> Result<()> {
    sock.write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    sock.flush().await?;
    Ok(())
}

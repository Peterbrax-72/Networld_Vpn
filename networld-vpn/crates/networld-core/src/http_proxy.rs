//! Local HTTP/HTTPS proxy front-end.
//!
//! Windows applications and the WinINet system-proxy setting speak HTTP proxy,
//! so this is what "Set as system proxy" points at. Handles both `CONNECT`
//! tunnels and absolute-URI plain HTTP requests.

use crate::error::{CoreError, Result};
use crate::logging::LogBus;
use crate::relay;
use crate::socks::DialerSlot;
use crate::stats::TrafficCounters;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct HttpProxyOptions {
    pub bind: SocketAddr,
    pub allow_lan: bool,
    pub auth: Option<(String, String)>,
}

impl Default for HttpProxyOptions {
    fn default() -> Self {
        Self {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8086),
            allow_lan: false,
            auth: None,
        }
    }
}

pub struct HttpProxyServer {
    pub local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
}

impl HttpProxyServer {
    pub async fn start(
        opts: HttpProxyOptions,
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
            .map_err(|e| CoreError::Proxy(format!("cannot bind HTTP proxy on {bind}: {e}")))?;
        let local_addr = listener.local_addr()?;
        logs.info("http", format!("HTTP proxy listening on {local_addr}"));

        let (tx, mut rx) = watch::channel(false);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = rx.changed() => { if *rx.borrow() { break; } }
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((sock, peer)) => {
                                let dialer = dialer.clone();
                                let stats = stats.clone();
                                let logs = logs.clone();
                                let opts = opts.clone();
                                tokio::spawn(async move {
                                    stats.conn_opened();
                                    if let Err(e) = handle(sock, peer, opts, dialer, stats.clone(), logs.clone()).await {
                                        logs.debug("http", format!("{peer} → {e}"));
                                    }
                                    stats.conn_closed();
                                });
                            }
                            Err(e) => {
                                logs.warn("http", format!("accept failed: {e}"));
                                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            }
                        }
                    }
                }
            }
            logs.info("http", "HTTP proxy stopped");
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

impl Drop for HttpProxyServer {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn handle(
    mut sock: TcpStream,
    peer: SocketAddr,
    opts: HttpProxyOptions,
    dialer: DialerSlot,
    stats: Arc<TrafficCounters>,
    logs: Arc<LogBus>,
) -> Result<()> {
    sock.set_nodelay(true).ok();
    let head = read_head(&mut sock).await?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default().to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();

    if let Some((user, pass)) = &opts.auth {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let expect = STANDARD.encode(format!("{user}:{pass}"));
        let ok = head.to_ascii_lowercase().lines().any(|l| {
            l.starts_with("proxy-authorization:") && l.contains(&expect.to_ascii_lowercase())
        });
        if !ok {
            sock.write_all(
                b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"Networld VPN\"\r\nContent-Length: 0\r\n\r\n",
            )
            .await?;
            return Err(CoreError::Proxy(format!("{peer} failed proxy auth")));
        }
    }

    let active = { dialer.read().await.clone() };
    let Some(active) = active else {
        sock.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
            .await?;
        return Err(CoreError::Proxy("tunnel is not connected".into()));
    };

    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = split_host_port(&target, 443);
        logs.trace("http", format!("{peer} CONNECT {host}:{port}"));
        match active.dial(&host, port).await {
            Ok(remote) => {
                sock.write_all(
                    b"HTTP/1.1 200 Connection established\r\nProxy-Agent: Networld-VPN\r\n\r\n",
                )
                .await?;
                relay::pipe(Box::new(sock), remote, stats).await;
                Ok(())
            }
            Err(e) => {
                sock.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                    .await?;
                Err(CoreError::Proxy(format!("CONNECT {host}:{port}: {e}")))
            }
        }
    } else {
        // Absolute-URI form: GET http://host/path HTTP/1.1
        let url = crate::uri::parse(&target)
            .ok_or_else(|| CoreError::Proxy(format!("bad absolute URI '{target}'")))?;
        let host = url.host.clone();
        let port = if url.port == 0 { 80 } else { url.port };
        let path = url.request_target();
        logs.trace("http", format!("{peer} {method} {host}:{port}{path}"));

        let mut remote = active.dial(&host, port).await?;
        let mut forwarded = format!("{method} {path} {version}\r\n");
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("proxy-connection:") || lower.starts_with("proxy-authorization:") {
                continue;
            }
            forwarded.push_str(line);
            forwarded.push_str("\r\n");
        }
        forwarded.push_str("\r\n");
        remote.write_all(forwarded.as_bytes()).await?;
        remote.flush().await?;
        relay::pipe(Box::new(sock), remote, stats).await;
        Ok(())
    }
}

async fn read_head(sock: &mut TcpStream) -> Result<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut b = [0u8; 1];
    loop {
        let n = sock.read(&mut b).await?;
        if n == 0 {
            break;
        }
        buf.push(b[0]);
        if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
            break;
        }
        if buf.len() > 64 * 1024 {
            return Err(CoreError::Proxy("request head too large".into()));
        }
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

pub fn split_host_port(target: &str, default_port: u16) -> (String, u16) {
    if let Some(stripped) = target.strip_prefix('[') {
        // [::1]:443
        if let Some(end) = stripped.find(']') {
            let host = stripped[..end].to_string();
            let port = stripped[end + 1..]
                .trim_start_matches(':')
                .parse()
                .unwrap_or(default_port);
            return (host, port);
        }
    }
    match target.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(default_port)),
        None => (target.to_string(), default_port),
    }
}

#[cfg(test)]
mod tests {
    use super::split_host_port;

    #[test]
    fn parses_host_port() {
        assert_eq!(
            split_host_port("example.com:8443", 443),
            ("example.com".into(), 8443)
        );
        assert_eq!(
            split_host_port("example.com", 443),
            ("example.com".into(), 443)
        );
        assert_eq!(split_host_port("[::1]:9000", 443), ("::1".into(), 9000));
    }
}

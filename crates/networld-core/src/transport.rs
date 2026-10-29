//! Outbound transport construction: TCP → (payload injection) → (TLS/SNI) → stream.
//!
//! Every engine that needs a raw byte pipe to the server goes through here, so
//! SSH-Direct, SSH-over-TLS, SSH-over-Proxy+Payload and SSH-over-SlowDNS all
//! share one battle-tested connect path.

use crate::error::{CoreError, Result};
use crate::logging::LogBus;
use crate::payload::{PayloadContext, PayloadTemplate};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

/// Anything we can run a protocol over.
pub trait AsyncStream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> AsyncStream for T {}
pub type BoxStream = Box<dyn AsyncStream>;

/// Where the gateway's HTTP response sits relative to the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsePosition {
    /// Write the payload, then read the response(s). The usual case.
    #[default]
    PostRequest,
    /// The gateway greets first: read one response, then write the payload.
    PreRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransportMode {
    /// Straight TCP to the server.
    #[default]
    Direct,
    /// TCP + TLS (SNI / bug host spoofing available).
    Tls,
    /// TCP to an HTTP proxy, with an optional injected payload.
    Proxy,
    /// TCP to an HTTP proxy, payload, then TLS to the server.
    ProxyTls,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TlsOptions {
    /// Server Name Indication override — the "SNI" / "bug host" field in the UI.
    pub sni: Option<String>,
    /// Skip certificate validation (common with SNI spoofing).
    pub insecure: bool,
    /// ALPN protocols, e.g. `["http/1.1"]` or `["h2"]`.
    #[serde(default)]
    pub alpn: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyOptions {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TransportConfig {
    pub mode: TransportMode,
    pub host: String,
    pub port: u16,
    pub proxy: Option<ProxyOptions>,
    /// Payload template. Injected for `Proxy`/`ProxyTls`, and also usable on
    /// `Direct`/`Tls` for servers that front SSH behind an HTTP endpoint.
    pub payload: Option<PayloadTemplate>,
    /// Bug / front host used by `[host]` and by SNI when `tls.sni` is unset.
    pub bug_host: Option<String>,
    pub tls: TlsOptions,
    pub connect_timeout: Duration,
    pub response_timeout: Duration,
    /// How many HTTP responses to consume after the payload (double CONNECT → 2).
    pub expect_responses: usize,
    /// Accept any HTTP status instead of only 2xx/101.
    pub accept_any_status: bool,
    /// Require this exact status code (NetMod "Response Status").
    pub expected_status: Option<u16>,
    /// Read the response before or after writing the payload.
    pub response_position: ResponsePosition,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            mode: TransportMode::Direct,
            host: String::new(),
            port: 22,
            proxy: None,
            payload: None,
            bug_host: None,
            tls: TlsOptions::default(),
            connect_timeout: Duration::from_secs(15),
            response_timeout: Duration::from_secs(20),
            expect_responses: 1,
            accept_any_status: false,
            expected_status: None,
            response_position: ResponsePosition::PostRequest,
        }
    }
}

impl TransportConfig {
    /// Is this response head acceptable?
    ///
    /// * `accept_any_status` wins outright,
    /// * then an explicit `expected_status` must match exactly,
    /// * otherwise fall back to 2xx/101.
    pub fn status_accepted(&self, status: &HttpStatus) -> bool {
        if self.accept_any_status {
            return true;
        }
        match self.expected_status {
            Some(code) => status.code == code,
            None => status.is_success(),
        }
    }

    pub fn describe(&self) -> String {
        let via = match &self.proxy {
            Some(p) => format!(" via proxy {}:{}", p.host, p.port),
            None => String::new(),
        };
        let sni = self
            .tls
            .sni
            .clone()
            .or_else(|| self.bug_host.clone())
            .map(|s| format!(" sni={s}"))
            .unwrap_or_default();
        format!("{:?} {}:{}{}{}", self.mode, self.host, self.port, via, sni)
    }
}

/// Proxy ports that normally speak plain HTTP. For these the payload / CONNECT
/// goes first and TLS to the server follows; any other proxy port (443, 8443,
/// CDN edges...) is assumed to be a TLS endpoint that must be wrapped first.
fn proxy_is_plain_http(port: u16) -> bool {
    matches!(
        port,
        80 | 81 | 3128 | 3129 | 8000 | 8008 | 8080 | 8081 | 8888
    )
}

/// Build the transport described by `cfg`.
///
/// `ProxyTls` has two real-world shapes and configs do not say which one they
/// are, so the likelier one (chosen from the proxy port) is tried first and the
/// other one is the automatic fallback.
pub async fn connect(cfg: &TransportConfig, logs: &Arc<LogBus>) -> Result<BoxStream> {
    let proxy_port = match (&cfg.mode, &cfg.proxy) {
        (TransportMode::ProxyTls, Some(p)) => p.port,
        _ => return connect_with(cfg, logs, false).await,
    };
    let first = !proxy_is_plain_http(proxy_port);
    match connect_with(cfg, logs, first).await {
        Ok(s) => Ok(s),
        Err(e) => {
            logs.warn(
                "transport",
                format!(
                    "{} failed ({e}); retrying with {}",
                    if first {
                        "TLS-then-payload"
                    } else {
                        "payload-then-TLS"
                    },
                    if first {
                        "payload-then-TLS"
                    } else {
                        "TLS-then-payload"
                    },
                ),
            );
            connect_with(cfg, logs, !first).await
        }
    }
}

/// `tls_first` (ProxyTls only): wrap the link to the proxy / CDN edge in TLS
/// (SNI = bug host) and inject the payload inside it. Otherwise: payload in
/// clear text to the proxy, then TLS to the server through it.
async fn connect_with(
    cfg: &TransportConfig,
    logs: &Arc<LogBus>,
    tls_first: bool,
) -> Result<BoxStream> {
    let (dial_host, dial_port) = match &cfg.proxy {
        Some(p) if matches!(cfg.mode, TransportMode::Proxy | TransportMode::ProxyTls) => {
            (p.host.clone(), p.port)
        }
        _ => (cfg.host.clone(), cfg.port),
    };

    logs.info(
        "transport",
        format!("Dialing {dial_host}:{dial_port} — {}", cfg.describe()),
    );

    let addr = resolve_one(&dial_host, dial_port).await?;
    let tcp = tokio::time::timeout(cfg.connect_timeout, TcpStream::connect(addr))
        .await
        .map_err(|_| CoreError::Timeout(cfg.connect_timeout))??;
    tcp.set_nodelay(true).ok();
    logs.debug("transport", format!("TCP established → {addr}"));

    let mut stream: BoxStream = Box::new(tcp);

    if tls_first {
        let sni = cfg
            .tls
            .sni
            .clone()
            .or_else(|| cfg.bug_host.clone())
            .unwrap_or_else(|| dial_host.clone());
        logs.info(
            "tls",
            format!(
                "TLS to the proxy first (sni={sni}, verify={})",
                !cfg.tls.insecure
            ),
        );
        stream = crate::tls::wrap(stream, &sni, &cfg.tls, cfg.connect_timeout).await?;
        logs.info("tls", "Handshake complete");
    }

    // ---- payload / CONNECT stage -----------------------------------------
    let needs_connect = matches!(cfg.mode, TransportMode::Proxy | TransportMode::ProxyTls);
    if let Some(tpl) = cfg.payload.as_ref().filter(|t| !t.is_empty()) {
        let mut ctx =
            PayloadContext::new(cfg.host.clone(), cfg.port).with_bug_host(cfg.bug_host.as_deref());
        ctx.resolved_ip = Some(addr.ip().to_string());
        if let Some(p) = &cfg.proxy {
            ctx = ctx.with_auth(p.username.as_deref(), p.password.as_deref());
        }
        let mut reads = cfg.expect_responses.max(1);

        // Some gateways answer before they will look at anything we send.
        if cfg.response_position == ResponsePosition::PreRequest {
            let status = read_http_response(&mut stream, cfg.response_timeout).await?;
            logs.info("payload", format!("Greeting → {}", status.line));
            if !cfg.status_accepted(&status) {
                return Err(CoreError::PayloadRejected(status.line));
            }
            reads = reads.saturating_sub(1);
        }

        write_payload(&mut stream, tpl, &ctx, logs).await?;

        for n in 0..reads {
            let status = read_http_response(&mut stream, cfg.response_timeout).await?;
            logs.info("payload", format!("Response {} → {}", n + 1, status.line));
            if !cfg.status_accepted(&status) {
                return Err(CoreError::PayloadRejected(status.line));
            }
        }
    } else if needs_connect {
        let proxy = cfg.proxy.clone().unwrap_or_default();
        let target_host = cfg.bug_host.clone().unwrap_or_else(|| cfg.host.clone());
        let mut req = format!(
            "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\nProxy-Connection: Keep-Alive\r\n",
            host = target_host,
            port = cfg.port
        );
        if let Some(u) = proxy.username.as_deref().filter(|u| !u.is_empty()) {
            use base64::{engine::general_purpose::STANDARD, Engine};
            let token = STANDARD.encode(format!("{u}:{}", proxy.password.unwrap_or_default()));
            req.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
        }
        req.push_str("\r\n");
        stream.write_all(req.as_bytes()).await?;
        stream.flush().await?;
        let status = read_http_response(&mut stream, cfg.response_timeout).await?;
        logs.info("proxy", format!("CONNECT → {}", status.line));
        if !cfg.accept_any_status && !status.is_success() {
            return Err(CoreError::PayloadRejected(status.line));
        }
    }

    // ---- TLS stage --------------------------------------------------------
    if !tls_first && matches!(cfg.mode, TransportMode::Tls | TransportMode::ProxyTls) {
        let sni = cfg
            .tls
            .sni
            .clone()
            .or_else(|| cfg.bug_host.clone())
            .unwrap_or_else(|| cfg.host.clone());
        logs.info(
            "tls",
            format!(
                "Starting TLS handshake (sni={sni}, verify={})",
                !cfg.tls.insecure
            ),
        );
        stream = crate::tls::wrap(stream, &sni, &cfg.tls, cfg.connect_timeout).await?;
        logs.info("tls", "Handshake complete");
    }

    Ok(stream)
}

async fn write_payload(
    stream: &mut BoxStream,
    tpl: &PayloadTemplate,
    ctx: &PayloadContext,
    logs: &Arc<LogBus>,
) -> Result<()> {
    let chunks = tpl.render(ctx);
    logs.debug(
        "payload",
        format!("Injecting payload in {} segment(s)", chunks.len()),
    );
    for (i, chunk) in chunks.iter().enumerate() {
        if chunk.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(chunk.delay_ms as u64)).await;
        }
        if chunk.data.is_empty() {
            continue;
        }
        stream.write_all(&chunk.data).await?;
        stream.flush().await?;
        logs.trace(
            "payload",
            format!(
                "segment {}/{} ({} bytes): {}",
                i + 1,
                chunks.len(),
                chunk.data.len(),
                String::from_utf8_lossy(&chunk.data)
                    .replace('\r', "\\r")
                    .replace('\n', "\\n")
            ),
        );
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct HttpStatus {
    pub line: String,
    pub code: u16,
    pub headers: String,
}

impl HttpStatus {
    pub fn is_success(&self) -> bool {
        self.code == 101 || (200..300).contains(&self.code)
    }
}

/// Read an HTTP response head (up to the blank line) from the stream.
pub async fn read_http_response(stream: &mut BoxStream, timeout: Duration) -> Result<HttpStatus> {
    let fut = async {
        let mut buf = Vec::with_capacity(512);
        let mut byte = [0u8; 1];
        loop {
            let n = stream.read(&mut byte).await?;
            if n == 0 {
                return Err(CoreError::PayloadRejected(
                    "connection closed before an HTTP response arrived".into(),
                ));
            }
            buf.push(byte[0]);
            if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
                break;
            }
            if buf.len() >= 2 && &buf[buf.len() - 2..] == b"\n\n" {
                break;
            }
            if buf.len() > 32 * 1024 {
                return Err(CoreError::PayloadRejected(
                    "HTTP response head too large".into(),
                ));
            }
        }
        let text = String::from_utf8_lossy(&buf).to_string();
        let line = text.lines().next().unwrap_or_default().trim().to_string();
        let code = line
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse::<u16>().ok())
            .unwrap_or(0);
        Ok(HttpStatus {
            line,
            code,
            headers: text,
        })
    };

    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| CoreError::Timeout(timeout))?
}

pub async fn resolve_one(host: &str, port: u16) -> Result<std::net::SocketAddr> {
    // Literal IPs skip DNS entirely.
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Ok(std::net::SocketAddr::new(ip, port));
    }
    let mut it = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| CoreError::Other(format!("DNS lookup for '{host}' failed: {e}")))?;
    it.next()
        .ok_or_else(|| CoreError::Other(format!("DNS lookup for '{host}' returned no records")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(code: u16) -> HttpStatus {
        HttpStatus {
            line: format!("HTTP/1.1 {code} Test"),
            code,
            headers: String::new(),
        }
    }

    #[test]
    fn status_matcher_precedence() {
        let mut cfg = TransportConfig::default();

        // default: 2xx/101 only
        assert!(cfg.status_accepted(&status(200)));
        assert!(cfg.status_accepted(&status(101)));
        assert!(!cfg.status_accepted(&status(403)));

        // an explicit expected status is exact
        cfg.expected_status = Some(200);
        assert!(cfg.status_accepted(&status(200)));
        assert!(!cfg.status_accepted(&status(201)));
        cfg.expected_status = Some(101);
        assert!(!cfg.status_accepted(&status(200)));
        assert!(cfg.status_accepted(&status(101)));

        // accept-any overrides everything
        cfg.accept_any_status = true;
        assert!(cfg.status_accepted(&status(502)));
    }

    #[test]
    fn response_position_defaults_to_post_request() {
        assert_eq!(
            TransportConfig::default().response_position,
            ResponsePosition::PostRequest
        );
    }

    #[tokio::test]
    async fn resolves_literal_ip_without_dns() {
        let a = resolve_one("127.0.0.1", 8080).await.unwrap();
        assert_eq!(a.to_string(), "127.0.0.1:8080");
    }

    #[test]
    fn status_success_rules() {
        let ok = HttpStatus {
            line: "HTTP/1.1 200 Connection established".into(),
            code: 200,
            headers: String::new(),
        };
        assert!(ok.is_success());
        let bad = HttpStatus {
            line: "HTTP/1.1 403 Forbidden".into(),
            code: 403,
            headers: String::new(),
        };
        assert!(!bad.is_success());
    }
}

#[cfg(test)]
mod order_tests {
    use super::proxy_is_plain_http;

    #[test]
    fn tls_endpoints_are_wrapped_first() {
        for p in [80, 8080, 3128] {
            assert!(proxy_is_plain_http(p));
        }
        for p in [443, 8443, 2053] {
            assert!(!proxy_is_plain_http(p));
        }
    }
}

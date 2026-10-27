//! NetMod / HTTP-Injector style **payload engine**.
//!
//! A payload is a text template that is written to the TCP socket *before* the
//! real protocol (SSH, TLS…) starts. Templates use square-bracket tokens that
//! expand at connect time, and may split the write into several TCP segments
//! with optional delays — the classic way to shape a request so that a captive
//! portal / DPI box lets it through.
//!
//! Supported tokens
//! | token | expands to |
//! |---|---|
//! | `[crlf]` `[cr]` `[lf]` `[lfcr]` | line terminators |
//! | `[host]` | bug host (or remote host when no bug host is set) |
//! | `[host_port]` | `host:port` |
//! | `[ip]` / `[port]` | resolved remote address / port |
//! | `[real_host]` / `[real_port]` | the true SSH endpoint, ignoring the bug host |
//! | `[method]` | request method (default `CONNECT`) |
//! | `[protocol]` | `HTTP/1.1` |
//! | `[ua]` | configured user agent |
//! | `[auth]` | `Proxy-Authorization: Basic …` when proxy credentials exist |
//! | `[raw]` | `METHOD host:port HTTP/1.1` |
//! | `[real_raw]` | `METHOD real_host:real_port HTTP/1.1` |
//! | `[netData]` | the original CONNECT request (request line + CRLF CRLF) |
//! | `[rand:N]` | N random alphanumeric characters |
//! | `[split]` | start a new TCP segment |
//! | `[delay_split]` | new TCP segment after the default delay |
//! | `[delay_N]` | new TCP segment after N milliseconds |

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";
pub const DEFAULT_SPLIT_DELAY_MS: u32 = 150;

/// Everything the template engine needs in order to expand tokens.
#[derive(Debug, Clone)]
pub struct PayloadContext {
    /// Bug / front host used by `[host]`.
    pub host: String,
    pub port: u16,
    /// Real tunnel endpoint, used by `[real_host]` / `[real_raw]`.
    pub real_host: String,
    pub real_port: u16,
    pub resolved_ip: Option<String>,
    pub method: String,
    pub protocol: String,
    pub user_agent: String,
    pub proxy_auth: Option<(String, String)>,
    pub default_delay_ms: u32,
}

impl PayloadContext {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        let host = host.into();
        Self {
            real_host: host.clone(),
            real_port: port,
            host,
            port,
            resolved_ip: None,
            method: "CONNECT".into(),
            protocol: "HTTP/1.1".into(),
            user_agent: DEFAULT_USER_AGENT.into(),
            proxy_auth: None,
            default_delay_ms: DEFAULT_SPLIT_DELAY_MS,
        }
    }

    pub fn with_bug_host(mut self, bug: Option<&str>) -> Self {
        if let Some(b) = bug {
            if !b.trim().is_empty() {
                self.host = b.trim().to_string();
            }
        }
        self
    }

    pub fn with_auth(mut self, user: Option<&str>, pass: Option<&str>) -> Self {
        if let Some(u) = user {
            if !u.is_empty() {
                self.proxy_auth = Some((u.to_string(), pass.unwrap_or_default().to_string()));
            }
        }
        self
    }
}

/// One TCP write, optionally preceded by a delay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadChunk {
    pub data: Vec<u8>,
    pub delay_ms: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PayloadTemplate(pub String);

impl PayloadTemplate {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Expand the template into the exact byte segments to put on the wire.
    pub fn render(&self, ctx: &PayloadContext) -> Vec<PayloadChunk> {
        let mut chunks: Vec<PayloadChunk> = Vec::new();
        let mut current = Vec::<u8>::new();
        let mut pending_delay = 0u32;
        let src: Vec<char> = self.0.chars().collect();
        let mut i = 0usize;

        macro_rules! flush {
            ($delay:expr) => {{
                chunks.push(PayloadChunk {
                    data: std::mem::take(&mut current),
                    delay_ms: pending_delay,
                });
                pending_delay = $delay;
            }};
        }

        while i < src.len() {
            if src[i] == '[' {
                if let Some(close) = find_close(&src, i) {
                    let token: String = src[i + 1..close].iter().collect();
                    let lower = token.to_ascii_lowercase();
                    i = close + 1;
                    match lower.as_str() {
                        "crlf" => current.extend_from_slice(b"\r\n"),
                        "cr" => current.push(b'\r'),
                        "lf" => current.push(b'\n'),
                        "lfcr" => current.extend_from_slice(b"\n\r"),
                        "host" => current.extend_from_slice(ctx.host.as_bytes()),
                        "host_port" => current
                            .extend_from_slice(format!("{}:{}", ctx.host, ctx.port).as_bytes()),
                        "real_host" => current.extend_from_slice(ctx.real_host.as_bytes()),
                        "real_port" => {
                            current.extend_from_slice(ctx.real_port.to_string().as_bytes())
                        }
                        "ip" => current.extend_from_slice(
                            ctx.resolved_ip
                                .clone()
                                .unwrap_or_else(|| ctx.host.clone())
                                .as_bytes(),
                        ),
                        "port" => current.extend_from_slice(ctx.port.to_string().as_bytes()),
                        "method" => current.extend_from_slice(ctx.method.as_bytes()),
                        "protocol" => current.extend_from_slice(ctx.protocol.as_bytes()),
                        "ua" | "user-agent" => current.extend_from_slice(ctx.user_agent.as_bytes()),
                        "auth" => {
                            if let Some(h) = basic_auth_header(ctx) {
                                current.extend_from_slice(h.as_bytes());
                            }
                        }
                        "raw" => current.extend_from_slice(
                            format!("{} {}:{} {}", ctx.method, ctx.host, ctx.port, ctx.protocol)
                                .as_bytes(),
                        ),
                        "real_raw" => current.extend_from_slice(
                            format!(
                                "{} {}:{} {}",
                                ctx.method, ctx.real_host, ctx.real_port, ctx.protocol
                            )
                            .as_bytes(),
                        ),
                        "netdata" => current.extend_from_slice(
                            format!(
                                "{} {}:{} {}\r\n\r\n",
                                ctx.method, ctx.real_host, ctx.real_port, ctx.protocol
                            )
                            .as_bytes(),
                        ),
                        "split" => flush!(0),
                        "delay_split" => flush!(ctx.default_delay_ms),
                        other => {
                            if let Some(rest) = other.strip_prefix("delay_") {
                                let ms = rest.parse::<u32>().unwrap_or(ctx.default_delay_ms);
                                flush!(ms);
                            } else if let Some(rest) = other.strip_prefix("rand:") {
                                let n = rest.parse::<usize>().unwrap_or(8).min(512);
                                current.extend_from_slice(random_alnum(n).as_bytes());
                            } else {
                                // Unknown token: emit verbatim so nothing is silently dropped.
                                current.push(b'[');
                                current.extend_from_slice(token.as_bytes());
                                current.push(b']');
                            }
                        }
                    }
                    continue;
                }
            }
            let mut buf = [0u8; 4];
            current.extend_from_slice(src[i].encode_utf8(&mut buf).as_bytes());
            i += 1;
        }

        chunks.push(PayloadChunk {
            data: current,
            delay_ms: pending_delay,
        });
        chunks.retain(|c| !c.data.is_empty() || c.delay_ms > 0);
        chunks
    }

    /// Flat preview (`\r` → `\\r`) for the payload generator UI.
    pub fn preview(&self, ctx: &PayloadContext) -> String {
        self.render(ctx)
            .iter()
            .map(|c| {
                let body = String::from_utf8_lossy(&c.data)
                    .replace('\r', "\\r")
                    .replace('\n', "\\n");
                if c.delay_ms > 0 {
                    format!("⟪wait {}ms⟫{}", c.delay_ms, body)
                } else {
                    body
                }
            })
            .collect::<Vec<_>>()
            .join("⟦split⟧")
    }
}

fn find_close(src: &[char], start: usize) -> Option<usize> {
    src.iter()
        .skip(start + 1)
        .position(|c| *c == ']')
        .map(|p| p + start + 1)
}

fn basic_auth_header(ctx: &PayloadContext) -> Option<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    ctx.proxy_auth.as_ref().map(|(u, p)| {
        format!(
            "Proxy-Authorization: Basic {}",
            STANDARD.encode(format!("{u}:{p}"))
        )
    })
}

/// Tiny xorshift PRNG — avoids pulling the `rand` crate into the core.
pub(crate) fn random_alnum(n: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut state = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        | 1;
    (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ALPHABET[(state % ALPHABET.len() as u64) as usize] as char
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Payload generator presets (used by the Payload Generator page)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PayloadPreset {
    /// Plain CONNECT, no injection.
    Direct,
    /// Bug host placed in front of the real request.
    FrontQuery,
    /// Bug host appended after the real request.
    BackQuery,
    /// Request split across two TCP segments.
    SplitQuery,
    /// Two CONNECT lines back to back.
    DoubleConnect,
    /// GET + Upgrade: websocket (good against WS-aware middleboxes).
    WebsocketUpgrade,
    /// GET request with the bug host in the Host header only.
    GetHostHeader,
    /// Reverse proxy style with X-Forwarded / X-Online-Host headers.
    ForwardedHost,
}

impl PayloadPreset {
    pub fn all() -> &'static [PayloadPreset] {
        use PayloadPreset::*;
        &[
            Direct,
            FrontQuery,
            BackQuery,
            SplitQuery,
            DoubleConnect,
            WebsocketUpgrade,
            GetHostHeader,
            ForwardedHost,
        ]
    }

    pub fn label(&self) -> &'static str {
        use PayloadPreset::*;
        match self {
            Direct => "Direct (no injection)",
            FrontQuery => "Front query",
            BackQuery => "Back query",
            SplitQuery => "Split query",
            DoubleConnect => "Double CONNECT",
            WebsocketUpgrade => "WebSocket upgrade",
            GetHostHeader => "GET + Host header",
            ForwardedHost => "X-Online-Host / forwarded",
        }
    }

    /// Build the template text for this preset.
    pub fn template(&self) -> PayloadTemplate {
        use PayloadPreset::*;
        let t = match self {
            Direct => "[raw][crlf][crlf]",
            FrontQuery => {
                "[method] [host_port] [protocol][crlf]Host: [host][crlf]X-Online-Host: [host][crlf]Connection: Keep-Alive[crlf][crlf][netData]"
            }
            BackQuery => {
                "[netData][method] [host_port] [protocol][crlf]Host: [host][crlf]Connection: Keep-Alive[crlf][crlf]"
            }
            SplitQuery => {
                "[method] [host_port] [protocol][crlf]Host: [host][delay_split][crlf]User-Agent: [ua][crlf]Connection: Keep-Alive[crlf][crlf]"
            }
            DoubleConnect => {
                "[raw][crlf][crlf][split][real_raw][crlf]Host: [host][crlf][crlf]"
            }
            WebsocketUpgrade => {
                "GET / [protocol][crlf]Host: [host][crlf]Upgrade: websocket[crlf]Connection: Upgrade[crlf]Sec-WebSocket-Key: [rand:16][crlf]Sec-WebSocket-Version: 13[crlf][crlf]"
            }
            GetHostHeader => {
                "GET http://[host]/ [protocol][crlf]Host: [host][crlf]User-Agent: [ua][crlf]Connection: Keep-Alive[crlf][crlf]"
            }
            ForwardedHost => {
                "[method] [host_port] [protocol][crlf]Host: [host][crlf]X-Online-Host: [host][crlf]X-Forward-Host: [host][crlf]X-Forwarded-For: [host][crlf]Connection: Keep-Alive[crlf][crlf]"
            }
        };
        PayloadTemplate::new(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> PayloadContext {
        let mut c = PayloadContext::new("ssh.example.com", 443);
        c.host = "bug.carrier.net".into();
        c
    }

    #[test]
    fn expands_basic_tokens() {
        let t = PayloadTemplate::new("[method] [host_port] [protocol][crlf][crlf]");
        let chunks = t.render(&ctx());
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            String::from_utf8_lossy(&chunks[0].data),
            "CONNECT bug.carrier.net:443 HTTP/1.1\r\n\r\n"
        );
    }

    #[test]
    fn real_host_ignores_bug_host() {
        let t = PayloadTemplate::new("[real_raw]");
        let chunks = t.render(&ctx());
        assert_eq!(
            String::from_utf8_lossy(&chunks[0].data),
            "CONNECT ssh.example.com:443 HTTP/1.1"
        );
    }

    #[test]
    fn split_creates_segments() {
        let t = PayloadTemplate::new("AAA[split]BBB[delay_250]CCC");
        let chunks = t.render(&ctx());
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].delay_ms, 0);
        assert_eq!(chunks[2].delay_ms, 250);
        assert_eq!(String::from_utf8_lossy(&chunks[2].data), "CCC");
    }

    #[test]
    fn unknown_tokens_survive() {
        let t = PayloadTemplate::new("x[bogus]y");
        let chunks = t.render(&ctx());
        assert_eq!(String::from_utf8_lossy(&chunks[0].data), "x[bogus]y");
    }

    #[test]
    fn presets_render() {
        for p in PayloadPreset::all() {
            let chunks = p.template().render(&ctx());
            assert!(!chunks.is_empty(), "{:?} produced nothing", p);
        }
    }

    #[test]
    fn rand_token_length() {
        let t = PayloadTemplate::new("[rand:16]");
        let chunks = t.render(&ctx());
        assert_eq!(chunks[0].data.len(), 16);
    }
}

//! Config import / export.
//!
//! Supported inbound formats:
//! * `vmess://`, `vless://`, `trojan://`, `ss://` (SIP002 + legacy)
//! * `ssh://` — Networld's SSH/payload link
//! * `networld://` — full profile, base64 JSON (round-trips everything)
//! * raw or base64 subscription blobs containing any of the above, one per line
//! * `.nw` files — JSON, optionally AES-256-GCM encrypted ("locked config")

use crate::config::{ProfileKind, ServerProfile, SshMode, SshProfile, XrayProfile};
use crate::error::{CoreError, Result};
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use base64::Engine;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use serde_json::Value;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

pub fn parse_link(raw: &str) -> Result<ServerProfile> {
    let link = raw.trim();
    let (scheme, body) = link
        .split_once("://")
        .ok_or_else(|| CoreError::parse("link", "missing scheme"))?;
    match scheme.to_ascii_lowercase().as_str() {
        "vmess" => parse_vmess(body),
        "vless" => parse_vless_like(body, "vless"),
        "trojan" => parse_vless_like(body, "trojan"),
        "ss" => parse_ss(body),
        "ssh" | "networld-ssh" => parse_ssh(body),
        "networld" => parse_networld(body),
        other => Err(CoreError::parse(
            "link",
            format!("unsupported scheme '{other}'"),
        )),
    }
}

/// Import many links at once (subscription body, clipboard dump, file).
pub fn parse_bulk(input: &str) -> (Vec<ServerProfile>, Vec<String>) {
    let text = match decode_b64_any(input.trim()) {
        Some(decoded) if decoded.contains("://") => decoded,
        _ => input.to_string(),
    };
    let mut ok = Vec::new();
    let mut errors = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.contains("://") {
            continue;
        }
        match parse_link(line) {
            Ok(p) => ok.push(p),
            Err(e) => errors.push(format!("{line}: {e}")),
        }
    }
    (ok, errors)
}

fn parse_vmess(body: &str) -> Result<ServerProfile> {
    let json = decode_b64_any(body)
        .ok_or_else(|| CoreError::parse("vmess link", "body is not valid base64"))?;
    let v: Value =
        serde_json::from_str(&json).map_err(|e| CoreError::parse("vmess link", e.to_string()))?;

    let s = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let port = v
        .get("port")
        .map(|p| {
            p.as_u64()
                .unwrap_or_else(|| p.as_str().and_then(|s| s.parse().ok()).unwrap_or(443))
        })
        .unwrap_or(443) as u16;

    let profile = XrayProfile {
        protocol: "vmess".into(),
        address: s("add"),
        port,
        uuid: s("id"),
        security: {
            let scy = s("scy");
            if scy.is_empty() {
                "auto".into()
            } else {
                scy
            }
        },
        network: {
            let n = s("net");
            if n.is_empty() {
                "tcp".into()
            } else {
                n
            }
        },
        path: s("path"),
        host: s("host"),
        sni: s("sni"),
        tls: {
            let t = s("tls");
            if t.is_empty() {
                "none".into()
            } else {
                t
            }
        },
        fingerprint: s("fp"),
        alpn: s("alpn")
            .split(',')
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect(),
        service_name: s("path"),
        ..Default::default()
    };
    let name = if s("ps").is_empty() {
        format!("{}:{}", profile.address, profile.port)
    } else {
        s("ps")
    };
    Ok(ServerProfile::new(name, ProfileKind::Xray(profile)))
}

fn parse_vless_like(body: &str, protocol: &str) -> Result<ServerProfile> {
    let (main, fragment) = split_fragment(body);
    let (userinfo, hostpart) = main
        .split_once('@')
        .ok_or_else(|| CoreError::parse(format!("{protocol} link"), "missing '@'"))?;
    let (hostport, query) = match hostpart.split_once('?') {
        Some((h, q)) => (h, q),
        None => (hostpart, ""),
    };
    let (host, port) = crate::http_proxy::split_host_port(hostport, 443);
    let q = parse_query(query);

    let profile = XrayProfile {
        protocol: protocol.to_string(),
        address: host.clone(),
        port,
        uuid: percent_decode(userinfo),
        encryption: q
            .get("encryption")
            .cloned()
            .unwrap_or_else(|| "none".into()),
        network: q.get("type").cloned().unwrap_or_else(|| "tcp".into()),
        path: q.get("path").cloned().unwrap_or_default(),
        host: q.get("host").cloned().unwrap_or_default(),
        service_name: q.get("serviceName").cloned().unwrap_or_default(),
        sni: q.get("sni").cloned().unwrap_or_default(),
        flow: q.get("flow").cloned().unwrap_or_default(),
        alpn: q
            .get("alpn")
            .map(|a| a.split(',').map(String::from).collect())
            .unwrap_or_default(),
        tls: q.get("security").cloned().unwrap_or_else(|| "none".into()),
        fingerprint: q.get("fp").cloned().unwrap_or_default(),
        public_key: q.get("pbk").cloned().unwrap_or_default(),
        short_id: q.get("sid").cloned().unwrap_or_default(),
        spider_x: q.get("spx").cloned().unwrap_or_default(),
        allow_insecure: q
            .get("allowInsecure")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false),
        ..Default::default()
    };
    let name = if fragment.is_empty() {
        format!("{host}:{port}")
    } else {
        fragment
    };
    Ok(ServerProfile::new(name, ProfileKind::Xray(profile)))
}

fn parse_ss(body: &str) -> Result<ServerProfile> {
    let (main, fragment) = split_fragment(body);
    let (userinfo, hostport) = match main.split_once('@') {
        // SIP002: ss://base64(method:password)@host:port
        Some((u, h)) => (
            decode_b64_any(u).unwrap_or_else(|| percent_decode(u)),
            h.split('?').next().unwrap_or(h).to_string(),
        ),
        // Legacy: ss://base64(method:password@host:port)
        None => {
            let decoded = decode_b64_any(main)
                .ok_or_else(|| CoreError::parse("ss link", "body is not valid base64"))?;
            let (u, h) = decoded
                .split_once('@')
                .ok_or_else(|| CoreError::parse("ss link", "missing '@'"))?;
            (u.to_string(), h.to_string())
        }
    };
    let (method, password) = userinfo
        .split_once(':')
        .ok_or_else(|| CoreError::parse("ss link", "missing method:password"))?;
    let (host, port) = crate::http_proxy::split_host_port(&hostport, 8388);

    let profile = XrayProfile {
        protocol: "shadowsocks".into(),
        address: host.clone(),
        port,
        uuid: password.to_string(),
        security: method.to_string(),
        network: "tcp".into(),
        tls: "none".into(),
        ..Default::default()
    };
    let name = if fragment.is_empty() {
        format!("{host}:{port}")
    } else {
        fragment
    };
    Ok(ServerProfile::new(name, ProfileKind::Xray(profile)))
}

/// `ssh://user:pass@host:port?mode=ssl&sni=bug.host&payload=…&proxy=1.2.3.4:8080#Name`
fn parse_ssh(body: &str) -> Result<ServerProfile> {
    let (main, fragment) = split_fragment(body);
    let (userinfo, hostpart) = main
        .split_once('@')
        .ok_or_else(|| CoreError::parse("ssh link", "missing '@'"))?;
    let (hostport, query) = match hostpart.split_once('?') {
        Some((h, q)) => (h, q),
        None => (hostpart, ""),
    };
    let (host, port) = crate::http_proxy::split_host_port(hostport, 22);
    let (user, pass) = match userinfo.split_once(':') {
        Some((u, p)) => (percent_decode(u), percent_decode(p)),
        None => (percent_decode(userinfo), String::new()),
    };
    let q = parse_query(query);
    let mode = match q.get("mode").map(|s| s.as_str()).unwrap_or("direct") {
        "ssl" | "tls" => SshMode::Ssl,
        "proxy" => SshMode::Proxy,
        "proxy-ssl" | "proxyssl" => SshMode::ProxySsl,
        "slowdns" | "dns" => SshMode::SlowDns,
        _ => SshMode::Direct,
    };
    let (proxy_host, proxy_port) = match q.get("proxy") {
        Some(p) => {
            let (h, pt) = crate::http_proxy::split_host_port(p, 8080);
            (Some(h), Some(pt))
        }
        None => (None, None),
    };

    let profile = SshProfile {
        host: host.clone(),
        port,
        username: user,
        password: pass,
        mode,
        proxy_host,
        proxy_port,
        payload: q.get("payload").cloned().unwrap_or_default(),
        sni: q.get("sni").cloned(),
        insecure_tls: q
            .get("insecure")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(true),
        keepalive_secs: 30,
        expect_responses: q.get("responses").and_then(|v| v.parse().ok()).unwrap_or(1),
        ..Default::default()
    };
    let name = if fragment.is_empty() {
        format!("{host}:{port}")
    } else {
        fragment
    };
    Ok(ServerProfile::new(name, ProfileKind::Ssh(profile)))
}

fn parse_networld(body: &str) -> Result<ServerProfile> {
    let json = decode_b64_any(body)
        .ok_or_else(|| CoreError::parse("networld link", "body is not valid base64"))?;
    let mut p: ServerProfile = serde_json::from_str(&json)?;
    if p.id.is_empty() {
        p.id = crate::config::new_id();
    }
    Ok(p)
}

// ---------------------------------------------------------------------------
// Serialising
// ---------------------------------------------------------------------------

/// Render a profile back to a share link.
pub fn to_link(p: &ServerProfile) -> String {
    match &p.kind {
        ProfileKind::Xray(x) if x.protocol == "vmess" => {
            let v = serde_json::json!({
                "v": "2",
                "ps": p.name,
                "add": x.address,
                "port": x.port.to_string(),
                "id": x.uuid,
                "aid": "0",
                "scy": x.security,
                "net": x.network,
                "type": "none",
                "host": x.host,
                "path": x.path,
                "tls": x.tls,
                "sni": x.sni,
                "fp": x.fingerprint,
            });
            format!("vmess://{}", STANDARD.encode(v.to_string()))
        }
        ProfileKind::Xray(x) if x.protocol == "shadowsocks" => {
            let userinfo = STANDARD_NO_PAD.encode(format!("{}:{}", x.security, x.uuid));
            format!(
                "ss://{}@{}:{}#{}",
                userinfo,
                x.address,
                x.port,
                enc(&p.name)
            )
        }
        ProfileKind::Xray(x) => {
            let mut q = vec![
                format!("security={}", enc(&x.tls)),
                format!("type={}", enc(&x.network)),
            ];
            if !x.sni.is_empty() {
                q.push(format!("sni={}", enc(&x.sni)));
            }
            if !x.path.is_empty() {
                q.push(format!("path={}", enc(&x.path)));
            }
            if !x.host.is_empty() {
                q.push(format!("host={}", enc(&x.host)));
            }
            if !x.flow.is_empty() {
                q.push(format!("flow={}", enc(&x.flow)));
            }
            if !x.public_key.is_empty() {
                q.push(format!("pbk={}", enc(&x.public_key)));
            }
            if !x.short_id.is_empty() {
                q.push(format!("sid={}", enc(&x.short_id)));
            }
            format!(
                "{}://{}@{}:{}?{}#{}",
                x.protocol,
                x.uuid,
                x.address,
                x.port,
                q.join("&"),
                enc(&p.name)
            )
        }
        ProfileKind::Ssh(s) => {
            let mode = match s.mode {
                SshMode::Direct => "direct",
                SshMode::Ssl => "ssl",
                SshMode::Proxy => "proxy",
                SshMode::ProxySsl => "proxy-ssl",
                SshMode::SlowDns => "slowdns",
            };
            let mut q = vec![format!("mode={mode}")];
            if let Some(sni) = s.sni.as_ref().filter(|v| !v.is_empty()) {
                q.push(format!("sni={}", enc(sni)));
            }
            if !s.payload.is_empty() {
                q.push(format!("payload={}", enc(&s.payload)));
            }
            if let (Some(h), Some(pt)) = (&s.proxy_host, s.proxy_port) {
                q.push(format!("proxy={}", enc(&format!("{h}:{pt}"))));
            }
            format!(
                "ssh://{}:{}@{}:{}?{}#{}",
                enc(&s.username),
                enc(&s.password),
                s.host,
                s.port,
                q.join("&"),
                enc(&p.name)
            )
        }
        ProfileKind::Wireguard(_) => to_networld_link(p),
    }
}

/// Lossless link that round-trips every field (including WireGuard).
pub fn to_networld_link(p: &ServerProfile) -> String {
    let json = serde_json::to_string(p).unwrap_or_default();
    format!("networld://{}", STANDARD.encode(json))
}

// ---------------------------------------------------------------------------
// .nw files — plain or AES-256-GCM "locked" configs
// ---------------------------------------------------------------------------

const NW_MAGIC: &str = "NW1";
/// Blobs written before the format was renamed to `.nw`.
const NW_MAGIC_LEGACY: &str = "NWC1";

pub fn export_nw(profiles: &[ServerProfile], password: Option<&str>) -> Result<String> {
    let json = serde_json::to_string(profiles)?;
    match password.filter(|p| !p.is_empty()) {
        None => Ok(format!("{NW_MAGIC}:plain:{}", STANDARD.encode(json))),
        Some(pass) => {
            use aes_gcm::aead::{Aead, KeyInit};
            use aes_gcm::Aes256Gcm;
            use sha2::{Digest, Sha256};

            let key = Sha256::digest(pass.as_bytes());
            let cipher = Aes256Gcm::new_from_slice(&key)
                .map_err(|e| CoreError::other(format!("cipher init failed: {e}")))?;
            let nonce_bytes = nonce_from_time();
            let ct = cipher
                .encrypt((&nonce_bytes).into(), json.as_bytes())
                .map_err(|e| CoreError::other(format!("encryption failed: {e}")))?;
            Ok(format!(
                "{NW_MAGIC}:aes:{}:{}",
                STANDARD.encode(nonce_bytes),
                STANDARD.encode(ct)
            ))
        }
    }
}

pub fn import_nw(blob: &str, password: Option<&str>) -> Result<Vec<ServerProfile>> {
    let blob = blob.trim();
    let mut parts = blob.splitn(4, ':');
    let magic = parts.next().unwrap_or_default();
    if magic != NW_MAGIC && magic != NW_MAGIC_LEGACY {
        // Fall back to a raw JSON array for hand-written files.
        return serde_json::from_str(blob).map_err(Into::into);
    }
    match parts.next().unwrap_or_default() {
        "plain" => {
            let b64 = parts.next().unwrap_or_default();
            let json = String::from_utf8(
                STANDARD
                    .decode(b64)
                    .map_err(|e| CoreError::parse("nw", e.to_string()))?,
            )
            .map_err(|e| CoreError::parse("nw", e.to_string()))?;
            Ok(serde_json::from_str(&json)?)
        }
        "aes" => {
            use aes_gcm::aead::{Aead, KeyInit};
            use aes_gcm::Aes256Gcm;
            use sha2::{Digest, Sha256};

            let pass = password.filter(|p| !p.is_empty()).ok_or_else(|| {
                CoreError::config("this config is locked — a password is required")
            })?;
            let nonce_b64 = parts.next().unwrap_or_default();
            let ct_b64 = parts.next().unwrap_or_default();
            let nonce_bytes = STANDARD
                .decode(nonce_b64)
                .map_err(|e| CoreError::parse("nw nonce", e.to_string()))?;
            let ct = STANDARD
                .decode(ct_b64)
                .map_err(|e| CoreError::parse("nw body", e.to_string()))?;
            let key = Sha256::digest(pass.as_bytes());
            let cipher = Aes256Gcm::new_from_slice(&key)
                .map_err(|e| CoreError::other(format!("cipher init failed: {e}")))?;
            let nonce: [u8; 12] = nonce_bytes
                .as_slice()
                .try_into()
                .map_err(|_| CoreError::parse("nw nonce", "expected 12 bytes"))?;
            let plain = cipher
                .decrypt((&nonce).into(), ct.as_ref())
                .map_err(|_| CoreError::config("wrong password, or the file is corrupt"))?;
            Ok(serde_json::from_slice(&plain)?)
        }
        other => Err(CoreError::parse("nw", format!("unknown mode '{other}'"))),
    }
}

fn nonce_from_time() -> [u8; 12] {
    let mut out = [0u8; 12];
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut state = now | 1;
    for b in out.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *b = (state & 0xFF) as u8;
    }
    out
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn split_fragment(s: &str) -> (&str, String) {
    match s.split_once('#') {
        Some((main, frag)) => (main, percent_decode(frag)),
        None => (s, String::new()),
    }
}

fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((k.to_string(), percent_decode(v)))
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().to_string()
}

fn enc(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

/// Try padded, unpadded and URL-safe base64 before giving up.
pub fn decode_b64_any(s: &str) -> Option<String> {
    let clean: String = s.trim().chars().filter(|c| !c.is_whitespace()).collect();
    for engine in [
        &STANDARD as &dyn EngineRef,
        &STANDARD_NO_PAD,
        &URL_SAFE_NO_PAD,
    ] {
        if let Some(text) = engine.try_decode(&clean) {
            return Some(text);
        }
    }
    None
}

trait EngineRef {
    fn try_decode(&self, s: &str) -> Option<String>;
}

impl<T: Engine> EngineRef for T {
    fn try_decode(&self, s: &str) -> Option<String> {
        self.decode(s).ok().and_then(|b| String::from_utf8(b).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vmess_roundtrip() {
        let raw = serde_json::json!({
            "v":"2","ps":"Tokyo Edge","add":"edge.example.com","port":"443",
            "id":"11111111-2222-3333-4444-555555555555","aid":"0","scy":"auto",
            "net":"ws","type":"none","host":"cdn.example.com","path":"/ray","tls":"tls","sni":"cdn.example.com"
        })
        .to_string();
        let link = format!("vmess://{}", STANDARD.encode(raw));
        let p = parse_link(&link).unwrap();
        assert_eq!(p.name, "Tokyo Edge");
        let ProfileKind::Xray(x) = &p.kind else {
            panic!()
        };
        assert_eq!(x.network, "ws");
        assert_eq!(x.port, 443);
        assert!(to_link(&p).starts_with("vmess://"));
    }

    #[test]
    fn vless_reality_parses() {
        let link = "vless://uuid-here@1.2.3.4:443?security=reality&type=tcp&sni=www.apple.com&pbk=PUBKEY&sid=ab12&flow=xtls-rprx-vision#Reality%20Node";
        let p = parse_link(link).unwrap();
        assert_eq!(p.name, "Reality Node");
        let ProfileKind::Xray(x) = &p.kind else {
            panic!()
        };
        assert_eq!(x.tls, "reality");
        assert_eq!(x.public_key, "PUBKEY");
        assert_eq!(x.flow, "xtls-rprx-vision");
    }

    #[test]
    fn trojan_parses() {
        let p = parse_link("trojan://pass123@t.example.com:443?security=tls&sni=t.example.com#T1")
            .unwrap();
        let ProfileKind::Xray(x) = &p.kind else {
            panic!()
        };
        assert_eq!(x.protocol, "trojan");
        assert_eq!(x.uuid, "pass123");
    }

    #[test]
    fn ss_sip002_parses() {
        let userinfo = STANDARD_NO_PAD.encode("aes-256-gcm:hunter2");
        let link = format!("ss://{userinfo}@ss.example.com:8388#SS%20Node");
        let p = parse_link(&link).unwrap();
        let ProfileKind::Xray(x) = &p.kind else {
            panic!()
        };
        assert_eq!(x.security, "aes-256-gcm");
        assert_eq!(x.uuid, "hunter2");
        assert_eq!(x.port, 8388);
    }

    #[test]
    fn ssh_link_with_payload() {
        let link = "ssh://user:pass@ssh.example.com:443?mode=proxy-ssl&sni=bug.carrier.net&payload=%5Braw%5D%5Bcrlf%5D%5Bcrlf%5D&proxy=10.0.0.1%3A8080#Carrier";
        let p = parse_link(link).unwrap();
        let ProfileKind::Ssh(s) = &p.kind else {
            panic!()
        };
        assert_eq!(s.mode, SshMode::ProxySsl);
        assert_eq!(s.payload, "[raw][crlf][crlf]");
        assert_eq!(s.proxy_host.as_deref(), Some("10.0.0.1"));
        assert_eq!(s.proxy_port, Some(8080));
        assert_eq!(p.name, "Carrier");
    }

    #[test]
    fn networld_link_roundtrip() {
        let p = ServerProfile::new(
            "Full",
            ProfileKind::Ssh(SshProfile {
                host: "a.example".into(),
                port: 22,
                username: "u".into(),
                ..Default::default()
            }),
        );
        let link = to_networld_link(&p);
        let back = parse_link(&link).unwrap();
        assert_eq!(back.name, "Full");
    }

    #[test]
    fn bulk_import_from_base64_subscription() {
        let body = "trojan://a@h1:443#One\nvmess://eyJ2IjoiMiIsInBzIjoiVHdvIiwiYWRkIjoiaDIiLCJwb3J0IjoiNDQzIiwiaWQiOiJ4IiwibmV0IjoidGNwIn0=\n";
        let encoded = STANDARD.encode(body);
        let (ok, errors) = parse_bulk(&encoded);
        assert_eq!(ok.len(), 2, "errors: {errors:?}");
    }

    #[test]
    fn locked_nw_requires_password() {
        let profiles = vec![ServerProfile::new(
            "Locked",
            ProfileKind::Ssh(SshProfile::default()),
        )];
        let blob = export_nw(&profiles, Some("s3cret")).unwrap();
        assert!(import_nw(&blob, None).is_err());
        let back = import_nw(&blob, Some("s3cret")).unwrap();
        assert_eq!(back[0].name, "Locked");
    }

    #[test]
    fn legacy_nwc_blob_still_imports() {
        let profiles = vec![ServerProfile::new(
            "legacy",
            ProfileKind::Ssh(SshProfile::default()),
        )];
        let blob = export_nw(&profiles, None).unwrap();
        assert!(blob.starts_with("NW1:"));
        let legacy = blob.replacen("NW1:", "NWC1:", 1);
        let back = import_nw(&legacy, None).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "legacy");
    }

    #[test]
    fn plain_nw_roundtrip() {
        let profiles = vec![ServerProfile::new(
            "Plain",
            ProfileKind::Xray(XrayProfile::default()),
        )];
        let blob = export_nw(&profiles, None).unwrap();
        let back = import_nw(&blob, None).unwrap();
        assert_eq!(back.len(), 1);
    }
}

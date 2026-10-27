//! V2Ray / Xray engine slot — VMess, VLESS (+ XTLS/REALITY), Trojan and
//! Shadowsocks, driven by the bundled `xray` sidecar.
//!
//! The sidecar exposes a local SOCKS5 inbound; we chain into it with
//! [`Socks5Dialer`], so the rest of the app (counters, split tunnel, TUN mode)
//! behaves exactly as it does for SSH.

use crate::config::{ProfileKind, ServerProfile, XrayProfile};
use crate::dialer::{Dialer, Socks5Dialer};
use crate::engine::{EngineCapabilities, EngineContext, EngineHandle, EngineKind, TunnelEngine};
use crate::error::{CoreError, Result};
use crate::sidecar::{free_port, wait_for_port, Sidecar};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

pub const BINARY: &str = "xray";

#[derive(Default)]
pub struct XrayEngine;

impl XrayEngine {
    pub fn caps(&self) -> EngineCapabilities {
        EngineCapabilities {
            udp: true,
            tun_mode: true,
            requires_elevation: false,
            sidecar: Some(BINARY),
            store_safe: true,
        }
    }
}

pub struct XrayHandle {
    sidecar: Arc<Sidecar>,
    dialer: Arc<dyn Dialer>,
    detail: String,
}

#[async_trait]
impl EngineHandle for XrayHandle {
    fn dialer(&self) -> Arc<dyn Dialer> {
        self.dialer.clone()
    }
    fn info(&self) -> String {
        self.detail.clone()
    }
    fn is_alive(&self) -> bool {
        self.dialer.is_alive()
    }
    async fn shutdown(&self) {
        self.sidecar.kill().await;
    }
}

#[async_trait]
impl TunnelEngine for XrayEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Xray
    }

    fn capabilities(&self) -> EngineCapabilities {
        self.caps()
    }

    fn validate(&self, profile: &ServerProfile) -> Result<()> {
        let ProfileKind::Xray(x) = &profile.kind else {
            return Err(CoreError::config("not a V2Ray/Xray profile"));
        };
        if x.address.trim().is_empty() || x.port == 0 {
            return Err(CoreError::config("Address and port are required"));
        }
        if x.uuid.trim().is_empty() {
            return Err(CoreError::config("UUID / password is required"));
        }
        match x.protocol.as_str() {
            "vmess" | "vless" | "trojan" | "shadowsocks" => Ok(()),
            other => Err(CoreError::config(format!("unsupported protocol '{other}'"))),
        }
    }

    async fn start(&self, ctx: EngineContext) -> Result<Arc<dyn EngineHandle>> {
        let ProfileKind::Xray(profile) = ctx.profile.kind.clone() else {
            return Err(CoreError::config("not a V2Ray/Xray profile"));
        };
        let logs = ctx.logs.clone();
        let bin = ctx.paths.binary(BINARY)?;

        let port = free_port().await?;
        let config = build_config(&profile, port);
        ctx.paths.ensure()?;
        let cfg_path = ctx.paths.runtime_dir.join("xray-active.json");
        std::fs::write(&cfg_path, serde_json::to_vec_pretty(&config)?)?;
        logs.debug("xray", format!("Config written to {}", cfg_path.display()));

        let sidecar = Sidecar::spawn(
            BINARY,
            bin,
            vec!["run".into(), "-c".into(), cfg_path.display().to_string()],
            logs.clone(),
        )
        .await?;

        let addr: std::net::SocketAddr = format!("127.0.0.1:{port}")
            .parse()
            .map_err(|_| CoreError::other("bad local address"))?;
        wait_for_port(addr, Duration::from_secs(15))
            .await
            .map_err(|_| CoreError::EngineUnavailable {
                engine: "xray".into(),
                reason: "sidecar did not open its SOCKS inbound — check the Logs tab".into(),
            })?;
        logs.info("xray", format!("Sidecar ready on 127.0.0.1:{port}"));

        let detail = format!(
            "{} · {}:{} · {}{}",
            profile.protocol.to_uppercase(),
            profile.address,
            profile.port,
            profile.network,
            if profile.tls == "none" {
                String::new()
            } else {
                format!("+{}", profile.tls)
            }
        );

        Ok(Arc::new(XrayHandle {
            sidecar,
            dialer: Arc::new(Socks5Dialer::new(addr)),
            detail,
        }))
    }
}

/// The local SOCKS inbound every engine exposes to the rest of the app.
fn socks_inbound(socks_port: u16) -> Value {
    json!({
        "tag": "socks-in",
        "listen": "127.0.0.1",
        "port": socks_port,
        "protocol": "socks",
        "settings": { "auth": "noauth", "udp": true, "ip": "127.0.0.1" },
        "sniffing": { "enabled": true, "destOverride": ["http", "tls", "quic"] }
    })
}

/// Translate a [`XrayProfile`] into an Xray-core JSON configuration.
///
/// When the profile carries hand-written JSON ("JSON Config") that document is
/// used verbatim, except that our `socks-in` inbound is injected (replacing any
/// inbound that already claims the tag) so the proxy port stays predictable.
pub fn build_config(p: &XrayProfile, socks_port: u16) -> Value {
    if let Some(raw) = p
        .raw_json
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if let Ok(mut doc) = serde_json::from_str::<Value>(raw) {
            if doc.is_object() {
                let mut kept: Vec<Value> = doc
                    .get("inbounds")
                    .and_then(Value::as_array)
                    .map(|v| {
                        v.iter()
                            .filter(|i| i.get("tag").and_then(Value::as_str) != Some("socks-in"))
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                kept.insert(0, socks_inbound(socks_port));
                doc["inbounds"] = Value::Array(kept);
                return doc;
            }
        }
    }
    json!({
        "log": { "loglevel": "warning" },
        "inbounds": [ socks_inbound(socks_port) ],
        "outbounds": [ outbound(p), { "protocol": "freedom", "tag": "direct" } ],
        "routing": {
            "domainStrategy": "AsIs",
            "rules": [{ "type": "field", "inboundTag": ["socks-in"], "outboundTag": "proxy" }]
        }
    })
}

fn outbound(p: &XrayProfile) -> Value {
    let settings = match p.protocol.as_str() {
        "vmess" => json!({
            "vnext": [{
                "address": p.address,
                "port": p.port,
                "users": [{
                    "id": p.uuid,
                    "alterId": 0,
                    "security": if p.security.is_empty() { "auto" } else { &p.security }
                }]
            }]
        }),
        "vless" => json!({
            "vnext": [{
                "address": p.address,
                "port": p.port,
                "users": [{
                    "id": p.uuid,
                    "encryption": if p.encryption.is_empty() { "none" } else { &p.encryption },
                    "flow": p.flow
                }]
            }]
        }),
        "trojan" => json!({
            "servers": [{ "address": p.address, "port": p.port, "password": p.uuid }]
        }),
        "socks" | "http" => {
            let mut server = json!({ "address": p.address, "port": p.port });
            // username lives in `encryption`, password in `uuid` for these kinds
            if !p.encryption.is_empty() {
                server["users"] = json!([{ "user": p.encryption, "pass": p.uuid }]);
            }
            json!({ "servers": [server] })
        }
        _ => json!({
            "servers": [{
                "address": p.address,
                "port": p.port,
                "method": if p.security.is_empty() { "aes-256-gcm" } else { &p.security },
                "password": p.uuid,
                "uot": true
            }]
        }),
    };

    let mut out = json!({
        "tag": "proxy",
        // shadowsocks-2022 is plain `shadowsocks` to Xray; the 2022-blake3-*
        // cipher is selected through `security` (the method field).
        "protocol": match p.protocol.as_str() {
            "shadowsocks2022" | "ss2022" | "ss" => "shadowsocks",
            other => other,
        },
        "settings": settings,
        "streamSettings": stream_settings(p)
    });

    if p.mux_enabled {
        out["mux"] = json!({
            "enabled": true,
            "concurrency": if p.mux_concurrency > 0 { p.mux_concurrency } else { 8 }
        });
    }
    out
}

fn stream_settings(p: &XrayProfile) -> Value {
    let net = if p.network.is_empty() {
        "tcp"
    } else {
        p.network.as_str()
    };
    let mut ss = json!({ "network": net, "security": if p.tls.is_empty() { "none" } else { p.tls.as_str() } });

    match p.tls.as_str() {
        "tls" => {
            ss["tlsSettings"] = json!({
                "serverName": if p.sni.is_empty() { &p.address } else { &p.sni },
                "allowInsecure": p.allow_insecure,
                "alpn": if p.alpn.is_empty() { vec!["h2".to_string(), "http/1.1".to_string()] } else { p.alpn.clone() },
                "fingerprint": if p.fingerprint.is_empty() { "chrome" } else { &p.fingerprint }
            });
        }
        "reality" => {
            ss["realitySettings"] = json!({
                "serverName": p.sni,
                "fingerprint": if p.fingerprint.is_empty() { "chrome" } else { &p.fingerprint },
                "publicKey": p.public_key,
                "shortId": p.short_id,
                "spiderX": p.spider_x
            });
        }
        _ => {}
    }

    match net {
        "ws" => {
            ss["wsSettings"] = json!({
                "path": if p.path.is_empty() { "/" } else { &p.path },
                "headers": { "Host": if p.host.is_empty() { &p.address } else { &p.host } }
            });
        }
        "httpupgrade" => {
            ss["httpupgradeSettings"] = json!({
                "path": if p.path.is_empty() { "/" } else { &p.path },
                "host": if p.host.is_empty() { &p.address } else { &p.host }
            });
        }
        "grpc" => {
            ss["grpcSettings"] = json!({
                "serviceName": p.service_name,
                "multiMode": false
            });
        }
        "http" | "h2" => {
            ss["httpSettings"] = json!({
                "path": if p.path.is_empty() { "/" } else { &p.path },
                "host": [ if p.host.is_empty() { &p.address } else { &p.host } ]
            });
        }
        "tcp" if !p.host.is_empty() || !p.path.is_empty() => {
            ss["tcpSettings"] = json!({
                "header": {
                    "type": "http",
                    "request": {
                        "path": [ if p.path.is_empty() { "/" } else { &p.path } ],
                        "headers": { "Host": [ if p.host.is_empty() { &p.address } else { &p.host } ] }
                    }
                }
            });
        }
        _ => {}
    }
    ss
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_json_is_used_verbatim_with_our_inbound() {
        let p = XrayProfile {
            protocol: "vmess".into(),
            raw_json: Some(
                r#"{
                    "log": { "loglevel": "debug" },
                    "inbounds": [
                        { "tag": "socks-in", "port": 1, "protocol": "socks" },
                        { "tag": "api", "port": 9000, "protocol": "dokodemo-door" }
                    ],
                    "outbounds": [{ "protocol": "freedom", "tag": "mine" }]
                }"#
                .into(),
            ),
            ..Default::default()
        };
        let cfg = build_config(&p, 10810);
        // our inbound replaces the hand-written socks-in and comes first
        assert_eq!(cfg["inbounds"][0]["tag"], "socks-in");
        assert_eq!(cfg["inbounds"][0]["port"], 10810);
        assert_eq!(cfg["inbounds"][1]["tag"], "api");
        assert_eq!(cfg["inbounds"].as_array().unwrap().len(), 2);
        // everything else is the user's document
        assert_eq!(cfg["log"]["loglevel"], "debug");
        assert_eq!(cfg["outbounds"][0]["tag"], "mine");
    }

    #[test]
    fn invalid_raw_json_falls_back_to_generated_config() {
        let p = XrayProfile {
            protocol: "trojan".into(),
            address: "t.example.com".into(),
            port: 443,
            uuid: "pw".into(),
            raw_json: Some("{ not json".into()),
            ..Default::default()
        };
        let cfg = build_config(&p, 1080);
        assert_eq!(cfg["outbounds"][0]["protocol"], "trojan");
        assert_eq!(cfg["inbounds"][0]["port"], 1080);
    }

    #[test]
    fn socks_outbound_carries_credentials() {
        let p = XrayProfile {
            protocol: "socks".into(),
            address: "127.0.0.1".into(),
            port: 1080,
            encryption: "alice".into(),
            uuid: "hunter2".into(),
            ..Default::default()
        };
        let cfg = build_config(&p, 10808);
        assert_eq!(cfg["outbounds"][0]["protocol"], "socks");
        let srv = &cfg["outbounds"][0]["settings"]["servers"][0];
        assert_eq!(srv["address"], "127.0.0.1");
        assert_eq!(srv["users"][0]["user"], "alice");
        assert_eq!(srv["users"][0]["pass"], "hunter2");
    }

    #[test]
    fn shadowsocks_2022_maps_to_shadowsocks_with_blake3_method() {
        let p = XrayProfile {
            protocol: "shadowsocks2022".into(),
            address: "ss.example.com".into(),
            port: 8388,
            security: "2022-blake3-aes-256-gcm".into(),
            uuid: "base64key".into(),
            ..Default::default()
        };
        let cfg = build_config(&p, 10808);
        assert_eq!(cfg["outbounds"][0]["protocol"], "shadowsocks");
        assert_eq!(
            cfg["outbounds"][0]["settings"]["servers"][0]["method"],
            "2022-blake3-aes-256-gcm"
        );
    }

    #[test]
    fn vmess_ws_tls_config_shape() {
        let p = XrayProfile {
            protocol: "vmess".into(),
            address: "edge.example.com".into(),
            port: 443,
            uuid: "11111111-2222-3333-4444-555555555555".into(),
            network: "ws".into(),
            path: "/ray".into(),
            host: "cdn.example.com".into(),
            tls: "tls".into(),
            sni: "cdn.example.com".into(),
            ..Default::default()
        };
        let cfg = build_config(&p, 10808);
        assert_eq!(cfg["inbounds"][0]["port"], 10808);
        assert_eq!(cfg["outbounds"][0]["protocol"], "vmess");
        assert_eq!(cfg["outbounds"][0]["streamSettings"]["network"], "ws");
        assert_eq!(
            cfg["outbounds"][0]["streamSettings"]["wsSettings"]["headers"]["Host"],
            "cdn.example.com"
        );
    }

    #[test]
    fn trojan_uses_password_servers() {
        let p = XrayProfile {
            protocol: "trojan".into(),
            address: "t.example.com".into(),
            port: 443,
            uuid: "s3cret".into(),
            tls: "tls".into(),
            ..Default::default()
        };
        let cfg = build_config(&p, 1081);
        assert_eq!(
            cfg["outbounds"][0]["settings"]["servers"][0]["password"],
            "s3cret"
        );
    }
}

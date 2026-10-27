/**
 * Everything the UI needs to turn a protocol choice into a profile, and a
 * profile back into the short labels the NetMod-style list shows.
 */
import type { ServerProfile, SshMode } from "../types";

export const uid = () => Math.random().toString(36).slice(2, 10) + Date.now().toString(36);

/** The entries of the "+" protocol menu, in NetMod order. */
export type ProtoId =
  | "vmess"
  | "vless"
  | "socks"
  | "trojan"
  | "shadowsocks"
  | "shadowsocks2022"
  | "wireguard"
  | "json"
  | "ssh"
  | "dnstt";

export interface ProtoEntry {
  id: ProtoId;
  label: string;
  engine: "XRAY" | "NATIVE";
  hint: string;
}

export const PROTOCOLS: ProtoEntry[] = [
  { id: "vmess", label: "VMess", engine: "XRAY", hint: "V2Ray / Xray UUID based" },
  { id: "vless", label: "VLess", engine: "XRAY", hint: "XTLS · Reality capable" },
  { id: "socks", label: "Socks", engine: "XRAY", hint: "Plain SOCKS5 upstream" },
  { id: "trojan", label: "Trojan", engine: "XRAY", hint: "TLS password tunnel" },
  { id: "shadowsocks", label: "Shadowsocks", engine: "XRAY", hint: "AEAD ciphers" },
  { id: "shadowsocks2022", label: "Shadowsocks 2022", engine: "XRAY", hint: "2022-blake3 ciphers" },
  { id: "wireguard", label: "WireGuard", engine: "NATIVE", hint: "Kernel-speed UDP tunnel" },
  { id: "json", label: "JSON Config", engine: "XRAY", hint: "Paste a full Xray config" },
  { id: "ssh", label: "SSH", engine: "NATIVE", hint: "Direct · TLS · proxy + payload" },
  { id: "dnstt", label: "DNSTT + SSH", engine: "NATIVE", hint: "SlowDNS over SSH" },
];

export type ProxyType = "none" | "http" | "socks5";
export type TlsType = "none" | "tls";

/** NetMod models transport as Proxy Type × TLS Type; the core calls it `mode`. */
export function deriveMode(proxy: ProxyType, tls: TlsType, slowdns = false): SshMode {
  if (slowdns) return "slow-dns";
  if (proxy === "none") return tls === "tls" ? "ssl" : "direct";
  return tls === "tls" ? "proxy-ssl" : "proxy";
}

export function modeParts(mode: SshMode): { proxy: ProxyType; tls: TlsType } {
  switch (mode) {
    case "ssl":
      return { proxy: "none", tls: "tls" };
    case "proxy":
      return { proxy: "http", tls: "none" };
    case "proxy-ssl":
      return { proxy: "http", tls: "tls" };
    default:
      return { proxy: "none", tls: "none" };
  }
}

/** `ee-1.stunnel.cc:80` — the endpoint shown in bold on a card. */
export function endpointOf(p: ServerProfile): string {
  // A locked config reports port 0: show dots instead.
  const port = (n: number) => (p.locked && n === 0 ? "••••" : String(n));
  if (p.kind === "ssh") return `${p.host || "—"}:${port(p.port)}`;
  if (p.kind === "xray") return `${p.address || "—"}:${port(p.port)}`;
  return p.endpoint || "—";
}

/** `(SSH + HTTP + TLS)` — the derived transport summary under the endpoint. */
export function transportLabel(p: ServerProfile): string {
  const parts: string[] = [];
  if (p.kind === "ssh") {
    parts.push("SSH");
    if (p.mode === "slow-dns") parts.push("DNSTT");
    else {
      const { proxy, tls } = modeParts(p.mode);
      if (proxy === "http") parts.push("HTTP");
      if (proxy === "socks5") parts.push("SOCKS");
      if (tls === "tls") parts.push("TLS");
      if (proxy === "none" && tls === "none") parts.push("DIRECT");
    }
  } else if (p.kind === "xray") {
    parts.push(p.rawJson?.trim() ? "JSON" : p.protocol.toUpperCase());
    if (p.network) parts.push(p.network.toUpperCase());
    if (p.tls && p.tls !== "none") parts.push(p.tls.toUpperCase());
  } else {
    parts.push("WIREGUARD", "UDP");
  }
  return `(${parts.join(" + ")})`;
}

export function engineBadge(p: ServerProfile): "XRAY" | "NATIVE" {
  return p.kind === "xray" ? "XRAY" : "NATIVE";
}

/** The letter in the round badge on each card. */
export function initialOf(p: ServerProfile): string {
  const src = (p.name || endpointOf(p)).trim();
  return (src[0] || "?").toUpperCase();
}

function base() {
  return {
    id: "",
    name: "",
    group: "",
    notes: "",
    tags: [] as string[],
    favorite: false,
    locked: false,
    createdAtMs: Date.now(),
  };
}

const SAMPLE_JSON = `{
  "outbounds": [
    {
      "tag": "proxy",
      "protocol": "vmess",
      "settings": {
        "vnext": [
          { "address": "example.com", "port": 443, "users": [{ "id": "uuid-here" }] }
        ]
      }
    }
  ]
}`;

/** Build an empty profile for a menu entry. */
export function blankFor(proto: ProtoId): ServerProfile {
  const b = base();

  if (proto === "ssh" || proto === "dnstt") {
    return {
      ...b,
      kind: "ssh",
      host: "",
      port: proto === "dnstt" ? 22 : 443,
      username: "",
      password: "",
      privateKeyPath: null,
      keyPassword: null,
      mode: proto === "dnstt" ? "slow-dns" : "ssl",
      proxyHost: null,
      proxyPort: null,
      proxyUser: null,
      proxyPass: null,
      payload: "",
      sni: "",
      insecureTls: true,
      udpgw: null,
      compression: false,
      keepaliveSecs: 30,
      expectResponses: 1,
      acceptAnyStatus: false,
      expectedStatus: 200,
      responsePosition: "post_request",
      slowdns:
        proto === "dnstt"
          ? { resolver: "8.8.8.8:53", publicKey: "", domain: "", transport: "udp", listenPort: 5300 }
          : null,
    } as ServerProfile;
  }

  if (proto === "wireguard") {
    return {
      ...b,
      kind: "wireguard",
      privateKey: "",
      publicKey: "",
      presharedKey: null,
      endpoint: "",
      address: ["10.0.0.2/32"],
      dns: ["1.1.1.1"],
      mtu: 1420,
      allowedIps: ["0.0.0.0/0", "::/0"],
      persistentKeepalive: 25,
    } as ServerProfile;
  }

  const protocol =
    proto === "json" ? "vmess" : (proto as "vmess" | "vless" | "trojan" | "shadowsocks" | "socks");

  return {
    ...b,
    kind: "xray",
    protocol: proto === "shadowsocks2022" ? "shadowsocks2022" : protocol,
    address: "",
    port: 443,
    uuid: "",
    security:
      proto === "shadowsocks"
        ? "aes-256-gcm"
        : proto === "shadowsocks2022"
          ? "2022-blake3-aes-256-gcm"
          : "auto",
    encryption: "none",
    network: proto === "socks" ? "tcp" : "ws",
    path: "/",
    host: "",
    serviceName: "",
    sni: "",
    flow: "",
    alpn: [],
    tls: proto === "socks" ? "none" : "tls",
    fingerprint: "chrome",
    publicKey: "",
    shortId: "",
    spiderX: "",
    allowInsecure: false,
    muxEnabled: false,
    muxConcurrency: 8,
    rawJson: proto === "json" ? SAMPLE_JSON : null,
  } as ServerProfile;
}

/** Which menu entry produced this profile — used to pick the editor's fields. */
export function protoOf(p: ServerProfile): ProtoId {
  if (p.kind === "wireguard") return "wireguard";
  if (p.kind === "ssh") return p.mode === "slow-dns" ? "dnstt" : "ssh";
  if (p.rawJson && p.rawJson.trim()) return "json";
  const proto = p.protocol as ProtoId;
  return PROTOCOLS.some((x) => x.id === proto) ? proto : "vmess";
}

export function titleOf(proto: ProtoId): string {
  return PROTOCOLS.find((p) => p.id === proto)?.label ?? "Config";
}

/** Response-status choices, NetMod style. `any` accepts whatever arrives. */
export const RESPONSE_STATUSES: { id: string; value: number | null; label: string }[] = [
  { id: "200", value: 200, label: "200 Connection established" },
  { id: "101", value: 101, label: "101 Switching Protocols" },
  { id: "201", value: 201, label: "201 Created" },
  { id: "204", value: 204, label: "204 No Content" },
  { id: "301", value: 301, label: "301 Moved Permanently" },
  { id: "302", value: 302, label: "302 Found" },
  { id: "any", value: null, label: "Any status (accept all)" },
];

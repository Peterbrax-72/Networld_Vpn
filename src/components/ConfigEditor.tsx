/**
 * The NetMod-style "Add Config" sheet: a title bar with help + save, then a
 * single column of plain fields. Only what the chosen protocol needs is shown;
 * everything else hides behind "Advanced".
 */
import { useMemo, useState } from "react";
import { Icon } from "./ui";
import { useStore } from "../state/store";
import {
  deriveMode,
  modeParts,
  protoOf,
  RESPONSE_STATUSES,
  titleOf,
  uid,
  type ProxyType,
  type TlsType,
} from "../lib/profiles";
import type { ServerProfile } from "../types";

// ---------------------------------------------------------------------------
// small field primitives — filled inputs with a floating caption
// ---------------------------------------------------------------------------

function Txt({
  value,
  onChange,
  placeholder,
  type = "text",
  mono,
  flex,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
  type?: string;
  mono?: boolean;
  flex?: number;
}) {
  return (
    <label className="nm-field" style={flex ? { flex } : undefined}>
      <span className={`nm-cap ${value ? "on" : ""}`}>{placeholder}</span>
      <input
        className={mono ? "mono" : ""}
        type={type}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.currentTarget.value)}
      />
    </label>
  );
}

function Num({
  value,
  onChange,
  placeholder,
  width,
  masked,
}: {
  value: number;
  onChange: (v: number) => void;
  placeholder: string;
  width?: number;
  /** Locked config: show dots instead of the number. */
  masked?: boolean;
}) {
  if (masked) {
    return (
      <label className="nm-field" style={width ? { width, flex: "none" } : undefined}>
        <span className="nm-cap on">{placeholder}</span>
        <input type="text" value="••••" readOnly />
      </label>
    );
  }
  return (
    <label className="nm-field" style={width ? { width, flex: "none" } : undefined}>
      <span className={`nm-cap ${value ? "on" : ""}`}>{placeholder}</span>
      <input
        type="number"
        value={Number.isFinite(value) ? value : 0}
        placeholder={placeholder}
        onChange={(e) => onChange(Number(e.currentTarget.value) || 0)}
      />
    </label>
  );
}

function Sel({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
}) {
  return (
    <label className="nm-select">
      <span className="nm-legend">{label}</span>
      <select value={value} onChange={(e) => onChange(e.currentTarget.value)}>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <Icon name="chev" size={16} />
    </label>
  );
}

function Area({
  value,
  onChange,
  placeholder,
  rows = 4,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
  rows?: number;
}) {
  return (
    <label className="nm-field area">
      <span className={`nm-cap ${value ? "on" : ""}`}>{placeholder}</span>
      <textarea
        className="mono"
        rows={rows}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.currentTarget.value)}
      />
    </label>
  );
}

function Check({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="nm-check">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.currentTarget.checked)} />
      <span>{label}</span>
    </label>
  );
}

// ---------------------------------------------------------------------------

const HELP: Record<string, string> = {
  ssh: "Payload is the HTTP request sent before the SSH handshake. Tokens like [host], [crlf] and [split] are expanded; Response Status is the reply the gateway must answer with.",
  dnstt: "DNSTT tunnels SSH inside DNS queries. Point the resolver at any open DNS server, then give the nameserver domain and the server's public key.",
  wireguard: "Paste the peer values from your .conf. WireGuard needs the tunnel service, so it is unavailable in Microsoft Store builds.",
  json: "Paste a complete Xray config. Networld replaces the inbounds with its own SOCKS listener and keeps everything else exactly as written.",
  xray: "Address, port and the UUID or password identify the server; Network and TLS must match what the server expects.",
};

export default function ConfigEditor({
  profile,
  onClose,
}: {
  profile: ServerProfile;
  onClose: () => void;
}) {
  const { saveProfile, toast } = useStore();
  const [p, setP] = useState<ServerProfile>(profile);
  const [showHelp, setShowHelp] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [saving, setSaving] = useState(false);

  const proto = useMemo(() => protoOf(profile), [profile]);
  const isNew = !profile.id;
  const set = (patch: Partial<ServerProfile>) => setP((cur) => ({ ...cur, ...patch }) as ServerProfile);

  const readOnly = profile.locked;

  const save = async () => {
    if (readOnly) return;
    const endpoint =
      p.kind === "ssh" ? p.host : p.kind === "xray" ? p.address : p.endpoint.split(":")[0];
    if (!endpoint.trim() && !(p.kind === "xray" && p.rawJson?.trim())) {
      toast("warn", "Address is required");
      return;
    }
    setSaving(true);
    try {
      await saveProfile({
        ...p,
        id: p.id || uid(),
        name: p.name.trim() || endpoint.trim() || titleOf(proto),
      });
      onClose();
    } catch (e) {
      toast("err", String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="sheet-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <section className="sheet" role="dialog" aria-label={`${isNew ? "Add" : "Edit"} config`}>
        <header className="sheet-bar">
          <span className="sheet-kind">{titleOf(proto)} settings</span>
          <button className="icon-btn" onClick={onClose} title="Close">
            <Icon name="x" size={16} />
          </button>
        </header>

        <div className="sheet-head">
          <h2>{readOnly ? "Locked Config" : isNew ? "Add Config" : "Edit Config"}</h2>
          <div className="row gap">
            <button
              className={`icon-btn square ${showHelp ? "on" : ""}`}
              title="What are these fields?"
              onClick={() => setShowHelp((v) => !v)}
            >
              ?
            </button>
            {!readOnly && (
              <button className="icon-btn save" title="Save" disabled={saving} onClick={save}>
                <Icon name="check" size={20} />
              </button>
            )}
          </div>
        </div>

        {showHelp && <p className="sheet-help">{HELP[proto] ?? HELP.xray}</p>}

        {readOnly && (
          <p className="sheet-help lock-banner">
            <Icon name="lock" size={14} /> This config is locked. Its credentials stay hidden and it can't be edited,
            but it connects normally. Delete it to remove it.
          </p>
        )}

        <fieldset className="sheet-body lock-fieldset" disabled={readOnly}>
          <Txt value={p.name} onChange={(v) => set({ name: v })} placeholder="Remarks" />

          {p.kind === "ssh" && <SshFields p={p} set={set} proto={proto} advanced={advanced} />}
          {p.kind === "xray" && <XrayFields p={p} set={set} proto={proto} advanced={advanced} />}
          {p.kind === "wireguard" && <WgFields p={p} set={set} advanced={advanced} />}

          <button className="sheet-more" onClick={() => setAdvanced((v) => !v)}>
            <Icon name={advanced ? "chev-up" : "chev"} size={14} />
            {advanced ? "Hide advanced" : "Advanced options"}
          </button>
        </fieldset>
      </section>
    </div>
  );
}

// ---------------------------------------------------------------------------
// SSH (and DNSTT + SSH) — the NetMod field order
// ---------------------------------------------------------------------------

function SshFields({
  p,
  set,
  proto,
  advanced,
}: {
  p: Extract<ServerProfile, { kind: "ssh" }>;
  set: (patch: Partial<ServerProfile>) => void;
  proto: string;
  advanced: boolean;
}) {
  const { proxy, tls } = modeParts(p.mode);
  const slowdns = proto === "dnstt";
  const statusId = p.acceptAnyStatus
    ? "any"
    : (RESPONSE_STATUSES.find((s) => s.value === (p.expectedStatus ?? 200))?.id ?? "200");

  const setTransport = (next: { proxy?: ProxyType; tls?: TlsType }) => {
    const pr = next.proxy ?? proxy;
    const tl = next.tls ?? tls;
    set({ mode: deriveMode(pr, tl, slowdns), ...(pr === "none" ? { proxyHost: null, proxyPort: null } : {}) });
  };

  return (
    <>
      <div className="nm-row">
        <Txt value={p.host} onChange={(v) => set({ host: v })} placeholder="Address" flex={2} />
        <Num value={p.port} onChange={(v) => set({ port: v })} placeholder="Port" width={110} masked={p.locked} />
      </div>
      <Txt value={p.username} onChange={(v) => set({ username: v })} placeholder="Username" />
      <Txt
        value={p.password}
        onChange={(v) => set({ password: v })}
        placeholder="Password"
        type="password"
      />
      <Area
        value={p.payload}
        onChange={(v) => set({ payload: v })}
        placeholder="Payload (HTTP Request Header)"
        rows={3}
      />

      <Sel
        label="Response Status"
        value={statusId}
        onChange={(id) => {
          const hit = RESPONSE_STATUSES.find((s) => s.id === id);
          set({ acceptAnyStatus: id === "any", expectedStatus: hit?.value ?? null });
        }}
        options={RESPONSE_STATUSES.map((s) => ({ value: s.id, label: s.label }))}
      />
      <Sel
        label="Response Position"
        value={p.responsePosition ?? "post_request"}
        onChange={(v) => set({ responsePosition: v as "pre_request" | "post_request" })}
        options={[
          { value: "pre_request", label: "pre_request" },
          { value: "post_request", label: "post_request" },
        ]}
      />
      <Sel
        label="Proxy Type"
        value={proxy}
        onChange={(v) => setTransport({ proxy: v as ProxyType })}
        options={[
          { value: "none", label: "none" },
          { value: "http", label: "http" },
          { value: "socks5", label: "socks5" },
        ]}
      />

      {proxy !== "none" && (
        <>
          <div className="nm-row">
            <Txt
              value={p.proxyHost ?? ""}
              onChange={(v) => set({ proxyHost: v })}
              placeholder="Proxy host"
              flex={2}
            />
            <Num
              value={p.proxyPort ?? 8080}
              onChange={(v) => set({ proxyPort: v })}
              placeholder="Proxy port"
              width={110}
              masked={p.locked}
            />
          </div>
          {advanced && (
            <div className="nm-row">
              <Txt
                value={p.proxyUser ?? ""}
                onChange={(v) => set({ proxyUser: v })}
                placeholder="Proxy user"
              />
              <Txt
                value={p.proxyPass ?? ""}
                onChange={(v) => set({ proxyPass: v })}
                placeholder="Proxy password"
                type="password"
              />
            </div>
          )}
        </>
      )}

      <Sel
        label="TLS Type"
        value={tls}
        onChange={(v) => setTransport({ tls: v as TlsType })}
        options={[
          { value: "none", label: "none" },
          { value: "tls", label: "tls" },
        ]}
      />

      {tls === "tls" && (
        <Txt value={p.sni ?? ""} onChange={(v) => set({ sni: v })} placeholder="SNI / Bug host" />
      )}

      {slowdns && (
        <>
          <div className="nm-sep">DNSTT</div>
          <Txt
            value={p.slowdns?.resolver ?? ""}
            onChange={(v) => set({ slowdns: { ...dns(p), resolver: v } })}
            placeholder="DNS resolver (8.8.8.8:53)"
          />
          <Txt
            value={p.slowdns?.domain ?? ""}
            onChange={(v) => set({ slowdns: { ...dns(p), domain: v } })}
            placeholder="Nameserver domain"
          />
          <Txt
            value={p.slowdns?.publicKey ?? ""}
            onChange={(v) => set({ slowdns: { ...dns(p), publicKey: v } })}
            placeholder="Server public key"
            mono
          />
        </>
      )}

      {advanced && (
        <>
          <div className="nm-sep">Advanced</div>
          <div className="nm-row">
            <Num
              value={p.expectResponses}
              onChange={(v) => set({ expectResponses: Math.max(1, v) })}
              placeholder="Expected responses"
            />
            <Num
              value={p.keepaliveSecs}
              onChange={(v) => set({ keepaliveSecs: v })}
              placeholder="Keepalive (s)"
            />
          </div>
          <Txt
            value={p.udpgw ?? ""}
            onChange={(v) => set({ udpgw: v })}
            placeholder="UDPGW (127.0.0.1:7300)"
          />
          <Txt
            value={p.privateKeyPath ?? ""}
            onChange={(v) => set({ privateKeyPath: v })}
            placeholder="Private key file"
          />
          <Check
            label="Ignore certificate errors"
            checked={p.insecureTls}
            onChange={(v) => set({ insecureTls: v })}
          />
          <Check
            label="Compression"
            checked={p.compression}
            onChange={(v) => set({ compression: v })}
          />
          <Check
            label="Lock config (hide credentials and make read-only — can't be undone)"
            checked={p.locked}
            onChange={(v) => set({ locked: v })}
          />
        </>
      )}
    </>
  );
}

function dns(p: Extract<ServerProfile, { kind: "ssh" }>) {
  return (
    p.slowdns ?? { resolver: "8.8.8.8:53", publicKey: "", domain: "", transport: "udp", listenPort: 5300 }
  );
}

// ---------------------------------------------------------------------------
// Xray family
// ---------------------------------------------------------------------------

function XrayFields({
  p,
  set,
  proto,
  advanced,
}: {
  p: Extract<ServerProfile, { kind: "xray" }>;
  set: (patch: Partial<ServerProfile>) => void;
  proto: string;
  advanced: boolean;
}) {
  if (proto === "json") {
    return (
      <>
        <Area
          value={p.rawJson ?? ""}
          onChange={(v) => set({ rawJson: v })}
          placeholder="Xray JSON config"
          rows={14}
        />
        <p className="muted tiny">
          Inbounds are replaced with Networld's local SOCKS listener; outbounds, routing and DNS are
          used exactly as written.
        </p>
      </>
    );
  }

  const isSs = proto === "shadowsocks" || proto === "shadowsocks2022";
  const isSocks = proto === "socks";
  const secret = isSocks ? "Password" : isSs || proto === "trojan" ? "Password" : "UUID";

  return (
    <>
      <div className="nm-row">
        <Txt value={p.address} onChange={(v) => set({ address: v })} placeholder="Address" flex={2} />
        <Num value={p.port} onChange={(v) => set({ port: v })} placeholder="Port" width={110} masked={p.locked} />
      </div>

      {isSocks && (
        <Txt
          value={p.encryption}
          onChange={(v) => set({ encryption: v })}
          placeholder="Username (optional)"
        />
      )}
      <Txt
        value={p.uuid}
        onChange={(v) => set({ uuid: v })}
        placeholder={secret}
        mono={!isSocks}
        type={isSocks ? "password" : "text"}
      />

      {isSs && (
        <Sel
          label="Method"
          value={p.security}
          onChange={(v) => set({ security: v })}
          options={(proto === "shadowsocks2022"
            ? ["2022-blake3-aes-128-gcm", "2022-blake3-aes-256-gcm", "2022-blake3-chacha20-poly1305"]
            : ["aes-256-gcm", "aes-128-gcm", "chacha20-ietf-poly1305", "none"]
          ).map((v) => ({ value: v, label: v }))}
        />
      )}

      {!isSocks && !isSs && (
        <Sel
          label="Network"
          value={p.network}
          onChange={(v) => set({ network: v })}
          options={["tcp", "ws", "grpc", "http", "httpupgrade", "quic"].map((v) => ({
            value: v,
            label: v,
          }))}
        />
      )}

      {(p.network === "ws" || p.network === "http" || p.network === "httpupgrade") && !isSocks && (
        <div className="nm-row">
          <Txt value={p.path} onChange={(v) => set({ path: v })} placeholder="Path" flex={2} />
          <Txt value={p.host} onChange={(v) => set({ host: v })} placeholder="Host header" flex={2} />
        </div>
      )}
      {p.network === "grpc" && !isSocks && (
        <Txt
          value={p.serviceName}
          onChange={(v) => set({ serviceName: v })}
          placeholder="gRPC service name"
        />
      )}

      <Sel
        label="TLS Type"
        value={p.tls || "none"}
        onChange={(v) => set({ tls: v })}
        options={[
          { value: "none", label: "none" },
          { value: "tls", label: "tls" },
          { value: "reality", label: "reality" },
        ]}
      />

      {p.tls !== "none" && (
        <Txt value={p.sni} onChange={(v) => set({ sni: v })} placeholder="SNI / Bug host" />
      )}

      {p.tls === "reality" && (
        <>
          <Txt
            value={p.publicKey}
            onChange={(v) => set({ publicKey: v })}
            placeholder="Reality public key"
            mono
          />
          <div className="nm-row">
            <Txt value={p.shortId} onChange={(v) => set({ shortId: v })} placeholder="Short ID" />
            <Txt value={p.spiderX} onChange={(v) => set({ spiderX: v })} placeholder="SpiderX" />
          </div>
        </>
      )}

      {advanced && (
        <>
          <div className="nm-sep">Advanced</div>
          {proto === "vmess" && (
            <Sel
              label="Security"
              value={p.security}
              onChange={(v) => set({ security: v })}
              options={["auto", "aes-128-gcm", "chacha20-poly1305", "none", "zero"].map((v) => ({
                value: v,
                label: v,
              }))}
            />
          )}
          {proto === "vless" && (
            <Sel
              label="Flow"
              value={p.flow || ""}
              onChange={(v) => set({ flow: v })}
              options={[
                { value: "", label: "none" },
                { value: "xtls-rprx-vision", label: "xtls-rprx-vision" },
              ]}
            />
          )}
          <Sel
            label="Fingerprint"
            value={p.fingerprint}
            onChange={(v) => set({ fingerprint: v })}
            options={["chrome", "firefox", "safari", "edge", "ios", "random"].map((v) => ({
              value: v,
              label: v,
            }))}
          />
          <Check
            label="Allow insecure TLS"
            checked={p.allowInsecure}
            onChange={(v) => set({ allowInsecure: v })}
          />
          <Check label="Mux" checked={p.muxEnabled} onChange={(v) => set({ muxEnabled: v })} />
          {p.muxEnabled && (
            <Num
              value={p.muxConcurrency}
              onChange={(v) => set({ muxConcurrency: v })}
              placeholder="Mux concurrency"
            />
          )}
        </>
      )}
    </>
  );
}

// ---------------------------------------------------------------------------
// WireGuard
// ---------------------------------------------------------------------------

function WgFields({
  p,
  set,
  advanced,
}: {
  p: Extract<ServerProfile, { kind: "wireguard" }>;
  set: (patch: Partial<ServerProfile>) => void;
  advanced: boolean;
}) {
  const list = (v: string) =>
    v
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);

  return (
    <>
      <Txt
        value={p.endpoint}
        onChange={(v) => set({ endpoint: v })}
        placeholder="Endpoint (host:port)"
      />
      <Txt
        value={p.privateKey}
        onChange={(v) => set({ privateKey: v })}
        placeholder="Private key"
        mono
      />
      <Txt
        value={p.publicKey}
        onChange={(v) => set({ publicKey: v })}
        placeholder="Peer public key"
        mono
      />
      <Txt
        value={p.address.join(", ")}
        onChange={(v) => set({ address: list(v) })}
        placeholder="Interface address"
      />
      <Txt value={p.dns.join(", ")} onChange={(v) => set({ dns: list(v) })} placeholder="DNS" />

      {advanced && (
        <>
          <div className="nm-sep">Advanced</div>
          <Txt
            value={p.presharedKey ?? ""}
            onChange={(v) => set({ presharedKey: v })}
            placeholder="Preshared key"
            mono
          />
          <Txt
            value={p.allowedIps.join(", ")}
            onChange={(v) => set({ allowedIps: list(v) })}
            placeholder="Allowed IPs"
          />
          <div className="nm-row">
            <Num value={p.mtu} onChange={(v) => set({ mtu: v })} placeholder="MTU" />
            <Num
              value={p.persistentKeepalive}
              onChange={(v) => set({ persistentKeepalive: v })}
              placeholder="Keepalive (s)"
            />
          </div>
        </>
      )}
    </>
  );
}

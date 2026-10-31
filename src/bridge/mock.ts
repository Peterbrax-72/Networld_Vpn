/**
 * Browser-side simulator.
 *
 * Runs the exact same IPC contract as the Rust backend so the interface can be
 * demoed, designed and tested without building the Windows app. Nothing in
 * here ships behaviour the real app does not have — it is a stand-in only.
 */
import type {
  AppConfig,
  AppInfo,
  ConnectionState,
  DataUsageLedger,
  LogEntry,
  PresetInfo,
  PreviewResponse,
  SavedPayload,
  ServerProfile,
  Settings,
  SpeedTestResult,
  StatusSnapshot,
} from "../types";

type Handler = (payload: any) => void;
const listeners = new Map<string, Set<Handler>>();
let seq = 0;

function emit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((h) => h(payload));
}

export function listen<T>(event: string, handler: (p: T) => void): () => void {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(handler as Handler);
  return () => listeners.get(event)?.delete(handler as Handler);
}

function log(level: LogEntry["level"], tag: string, message: string) {
  const entry: LogEntry = { seq: seq++, tsMs: Date.now(), level, tag, message };
  logs.push(entry);
  if (logs.length > 2000) logs.shift();
  emit("networld://log", entry);
}

const activate = (id: string) => {
  if (!config.activeProfileId) config.activeProfileId = id;
};
const uid = () => Math.random().toString(36).slice(2, 10) + Date.now().toString(36);
/** The real IPC boundary serialises everything; mirror that so React sees new objects. */
const clone = <X,>(value: X): X => JSON.parse(JSON.stringify(value)) as X;

/** Mirrors ServerProfile::redacted() in the Rust core: locked configs never show credentials. */
const MASK = "••••••••";
function redact(p: ServerProfile): ServerProfile {
  if (!p.locked) return p;
  const r = clone(p) as any;
  const mask = (k: string) => {
    if (typeof r[k] === "string" && r[k]) r[k] = MASK;
  };
  if (r.kind === "ssh" || r.kind === "xray") r.port = 0;
  if (r.kind === "ssh" && r.proxyPort != null) r.proxyPort = 0;
  if (r.kind === "wireguard" && typeof r.endpoint === "string" && r.endpoint.includes(":"))
    r.endpoint = `${r.endpoint.slice(0, r.endpoint.lastIndexOf(":"))}:${MASK}`;
  if (r.kind === "ssh") {
    ["username", "password", "privateKeyPath", "keyPassword", "proxyHost", "proxyUser", "proxyPass"].forEach(mask);
  } else if (r.kind === "xray") {
    ["uuid", "publicKey", "shortId", "rawJson"].forEach(mask);
  } else if (r.kind === "wireguard") {
    ["privateKey", "publicKey", "presharedKey"].forEach(mask);
  }
  return r as ServerProfile;
}

// ---------------------------------------------------------------------------
// seed data
// ---------------------------------------------------------------------------

const defaultSettings: Settings = {
  theme: "midnight",
  accent: "#3b82f6",
  language: "en",
  autostart: false,
  startMinimized: false,
  minimizeToTray: true,
  connectOnLaunch: false,
  autoReconnect: true,
  maxReconnectAttempts: 10,
  reconnectDelaySecs: 5,
  autoSwitchConfig: false,
  tunnelMode: "proxy",
  socksPort: 0,
  httpPort: 0,
  allowLan: false,
  setSystemProxy: true,
  killSwitch: false,
  blockIpv6: true,
  udpgwEnabled: false,
  udpgwAddress: "127.0.0.1:7300",
  dns: { enabled: true, servers: ["1.1.1.1", "8.8.8.8"], remoteResolve: true, dohUrl: null },
  splitTunnel: { mode: "off", apps: [], bypassCidrs: [], bypassDomains: [] },
  notifications: true,
  logLevel: "info",
  keepLogsLines: 5000,
  updateChannel: "stable",
  checkUpdates: true,
  tunMtu: 1420,
  tunInterfaceName: "Networld",
};

// No bundled servers. The app ships empty — every config is created by the
// user in the editor, or imported from a link, a .nw file or a QR code.
// These factories only exist so the import simulator can materialise a
// profile from a pasted link, exactly like the Rust `links::parse_link` does.

function blankSsh(name: string, host = "", port = 443): ServerProfile {
  return {
    id: uid(),
    name,
    group: "",
    notes: "",
    tags: [],
    favorite: false,
    locked: false,
    createdAtMs: Date.now(),
    kind: "ssh",
    host,
    port,
    username: "",
    password: "",
    privateKeyPath: null,
    keyPassword: null,
    mode: "direct",
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
    slowdns: null,
  } as ServerProfile;
}

function blankXray(name: string, protocol: string, address = "", port = 443): ServerProfile {
  return {
    id: uid(),
    name,
    group: "",
    notes: "",
    tags: [],
    favorite: false,
    locked: false,
    createdAtMs: Date.now(),
    kind: "xray",
    protocol,
    address,
    port,
    uuid: "",
    security: "auto",
    encryption: "none",
    network: "tcp",
    path: "",
    host: "",
    serviceName: "",
    sni: "",
    flow: "",
    alpn: [],
    tls: "none",
    fingerprint: "chrome",
    publicKey: "",
    shortId: "",
    spiderX: "",
    allowInsecure: false,
    muxEnabled: false,
    muxConcurrency: 8,
  } as unknown as ServerProfile;
}

// UI-Preview.html is built with VITE_DEMO=1 so the showcase is not an empty
// screen. The desktop app never reaches this file, and ordinary dev/CI builds
// stay empty too.
const demoProfiles: ServerProfile[] =
  import.meta.env.VITE_DEMO === "1"
    ? [
        Object.assign(blankSsh("Demo SSH", "ssh.example.com", 443), {
          username: "demo",
          password: "demo-password",
          mode: "ssl",
          sni: "cdn.example.com",
        }),
        blankXray("Demo VLESS", "vless", "vless.example.com", 443),
        blankSsh("Demo SSH · port 80", "edge.example.net", 80),
      ]
    : [];

const config: AppConfig = {
  version: 1,
  settings: { ...defaultSettings },
  profiles: demoProfiles,
  activeProfileId: demoProfiles[0]?.id ?? null,
  dataUsage: { days: {}, lifetimeUp: 0, lifetimeDown: 0, sessions: 0 },
  payloadLibrary: [],
  subscriptions: [],
};

const logs: LogEntry[] = [];
let status: StatusSnapshot = {
  state: "disconnected",
  profileId: null,
  profileName: null,
  engine: "ssh",
  detail: "",
  message: "",
  hint: "",
  socksPort: null,
  httpPort: null,
  tunnelMode: "proxy",
  killSwitch: false,
  systemProxy: false,
  connectedSinceMs: null,
  reconnectAttempt: 0,
  stats: {
    uptimeSecs: 0,
    bytesUp: 0,
    bytesDown: 0,
    speedUpBps: 0,
    speedDownBps: 0,
    activeConnections: 0,
    totalConnections: 0,
  },
};

let ticker: number | undefined;

function setState(state: ConnectionState, message: string) {
  status = { ...status, state, message };
  emit("networld://status", status);
}

function startTicker() {
  stopTicker();
  const started = Date.now();
  ticker = window.setInterval(() => {
    const down = 1.6e6 + Math.random() * 5.2e6;
    const up = 2.1e5 + Math.random() * 9e5;
    const s = status.stats;
    status = {
      ...status,
      stats: {
        uptimeSecs: Math.floor((Date.now() - started) / 1000),
        bytesUp: s.bytesUp + up,
        bytesDown: s.bytesDown + down,
        speedUpBps: up,
        speedDownBps: down,
        activeConnections: 3 + Math.floor(Math.random() * 9),
        totalConnections: s.totalConnections + Math.floor(Math.random() * 4),
      },
    };
    emit("networld://stats", status.stats);
  }, 1000);
}

function stopTicker() {
  if (ticker) window.clearInterval(ticker);
  ticker = undefined;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function simulateConnect(id?: string | null): Promise<StatusSnapshot> {
  const profile =
    config.profiles.find((p) => p.id === (id ?? config.activeProfileId)) ?? config.profiles[0];
  status = {
    ...status,
    profileId: profile.id,
    profileName: profile.name,
    engine: profile.kind === "ssh" ? "ssh" : profile.kind === "xray" ? "xray" : "wireguard",
    tunnelMode: config.settings.tunnelMode,
    reconnectAttempt: 0,
    stats: { ...status.stats, bytesUp: 0, bytesDown: 0, uptimeSecs: 0 },
  };

  setState("resolving", "Resolving host");
  log("info", "manager", `Connecting '${profile.name}'`);
  await sleep(380);

  if (profile.kind === "ssh") {
    log("info", "transport", `Dialing ${profile.host}:${profile.port}`);
    setState("connecting", "Opening transport");
    await sleep(520);
    if (profile.payload) {
      log("debug", "payload", "Injecting payload in 1 segment(s)");
      log("info", "payload", "Response 1 → HTTP/1.1 200 Connection established");
    }
    if (profile.mode === "ssl" || profile.mode === "proxy-ssl") {
      log("info", "tls", `Starting TLS handshake (sni=${profile.sni ?? profile.host})`);
      await sleep(360);
      log("info", "tls", "Handshake complete");
    }
    setState("authenticating", "Authenticating");
    log("info", "ssh", `Authenticating as '${profile.username}'`);
    await sleep(540);
    log("info", "ssh", "Authentication succeeded — tunnel is up");
  } else if (profile.kind === "xray") {
    setState("connecting", "Starting Xray sidecar");
    log("info", "xray", "Sidecar ready on 127.0.0.1:10809");
    await sleep(700);
  } else {
    setState("connecting", "Installing WireGuard tunnel service");
    await sleep(700);
  }

  setState("starting-proxies", "Starting local proxies");
  log("info", "socks", `SOCKS5 listening on 127.0.0.1:${config.settings.socksPort}`);
  log("info", "http", `HTTP proxy listening on 127.0.0.1:${config.settings.httpPort}`);
  await sleep(300);

  if (config.settings.killSwitch) log("info", "killswitch", "Kill switch engaged");
  if (config.settings.setSystemProxy)
    log("info", "sysproxy", `System proxy set to 127.0.0.1:${config.settings.httpPort}`);

  status = {
    ...status,
    socksPort: config.settings.socksPort,
    httpPort: config.settings.httpPort,
    killSwitch: config.settings.killSwitch,
    systemProxy: config.settings.setSystemProxy && config.settings.tunnelMode === "proxy",
    connectedSinceMs: Date.now(),
    detail:
      profile.kind === "ssh"
        ? `${
            { direct: "SSH Direct", ssl: "SSH over TLS", proxy: "SSH over Proxy", "proxy-ssl": "SSH over Proxy+TLS", "slow-dns": "SSH over SlowDNS" }[
              profile.mode
            ]
          } · ${profile.host}:${profile.port}${profile.sni ? " · " + profile.sni : ""}`
        : profile.kind === "xray"
          ? `${profile.protocol.toUpperCase()} · ${profile.address}:${profile.port} · ${profile.network}+${profile.tls}`
          : `WireGuard · ${profile.endpoint}`,
  };
  setState("connected", "Connected");
  startTicker();
  return status;
}

async function simulateDisconnect(): Promise<StatusSnapshot> {
  setState("disconnecting", "Disconnecting");
  stopTicker();
  await sleep(300);
  log("info", "manager", "Disconnected");
  status = {
    ...status,
    socksPort: null,
    httpPort: null,
    detail: "",
    connectedSinceMs: null,
    killSwitch: false,
    systemProxy: false,
    stats: { ...status.stats, speedUpBps: 0, speedDownBps: 0 },
  };
  setState("disconnected", "Disconnected");
  return status;
}

// ---------------------------------------------------------------------------
// payload renderer (mirrors crates/networld-core/src/payload.rs)
// ---------------------------------------------------------------------------

function renderPayload(
  template: string,
  host: string,
  port: number,
  bugHost?: string | null,
  method = "CONNECT",
): PreviewResponse {
  const h = bugHost && bugHost.trim() ? bugHost.trim() : host;
  const ua =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";
  const segments: { text: string; bytes: number; delayMs: number }[] = [];
  let cur = "";
  let pending = 0;
  const flush = (delay: number) => {
    segments.push({ text: cur, bytes: new TextEncoder().encode(cur).length, delayMs: pending });
    cur = "";
    pending = delay;
  };
  const rand = (n: number) =>
    Array.from({ length: n }, () => "abcdefghijklmnopqrstuvwxyz0123456789"[Math.floor(Math.random() * 36)]).join("");

  const re = /\[([^\]]+)\]/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(template))) {
    cur += template.slice(last, m.index);
    last = m.index + m[0].length;
    const t = m[1].toLowerCase();
    if (t === "crlf") cur += "\r\n";
    else if (t === "cr") cur += "\r";
    else if (t === "lf") cur += "\n";
    else if (t === "lfcr") cur += "\n\r";
    else if (t === "host") cur += h;
    else if (t === "host_port") cur += `${h}:${port}`;
    else if (t === "real_host") cur += host;
    else if (t === "real_port") cur += String(port);
    else if (t === "ip") cur += host;
    else if (t === "port") cur += String(port);
    else if (t === "method") cur += method;
    else if (t === "protocol") cur += "HTTP/1.1";
    else if (t === "ua" || t === "user-agent") cur += ua;
    else if (t === "raw") cur += `${method} ${h}:${port} HTTP/1.1`;
    else if (t === "real_raw") cur += `${method} ${host}:${port} HTTP/1.1`;
    else if (t === "netdata") cur += `${method} ${host}:${port} HTTP/1.1\r\n\r\n`;
    else if (t === "split") flush(0);
    else if (t === "delay_split") flush(150);
    else if (t.startsWith("delay_")) flush(parseInt(t.slice(6), 10) || 150);
    else if (t.startsWith("rand:")) cur += rand(parseInt(t.slice(5), 10) || 8);
    else cur += m[0];
  }
  cur += template.slice(last);
  flush(0);
  const kept = segments.filter((s) => s.text.length > 0 || s.delayMs > 0);
  const esc = (s: string) => s.replace(/\r/g, "\\r").replace(/\n/g, "\\n");
  return {
    preview: kept.map((s) => (s.delayMs ? `⟪wait ${s.delayMs}ms⟫` : "") + esc(s.text)).join("⟦split⟧"),
    segments: kept.map((s) => ({ ...s, text: esc(s.text) })),
    totalBytes: kept.reduce((a, s) => a + s.bytes, 0),
  };
}

const presets: PresetInfo[] = [
  { id: "direct", label: "Direct (no injection)", template: "[raw][crlf][crlf]" },
  {
    id: "front-query",
    label: "Front query",
    template:
      "[method] [host_port] [protocol][crlf]Host: [host][crlf]X-Online-Host: [host][crlf]Connection: Keep-Alive[crlf][crlf][netData]",
  },
  {
    id: "back-query",
    label: "Back query",
    template:
      "[netData][method] [host_port] [protocol][crlf]Host: [host][crlf]Connection: Keep-Alive[crlf][crlf]",
  },
  {
    id: "split-query",
    label: "Split query",
    template:
      "[method] [host_port] [protocol][crlf]Host: [host][delay_split][crlf]User-Agent: [ua][crlf]Connection: Keep-Alive[crlf][crlf]",
  },
  {
    id: "double-connect",
    label: "Double CONNECT",
    template: "[raw][crlf][crlf][split][real_raw][crlf]Host: [host][crlf][crlf]",
  },
  {
    id: "websocket-upgrade",
    label: "WebSocket upgrade",
    template:
      "GET / [protocol][crlf]Host: [host][crlf]Upgrade: websocket[crlf]Connection: Upgrade[crlf]Sec-WebSocket-Key: [rand:16][crlf]Sec-WebSocket-Version: 13[crlf][crlf]",
  },
  {
    id: "get-host-header",
    label: "GET + Host header",
    template:
      "GET http://[host]/ [protocol][crlf]Host: [host][crlf]User-Agent: [ua][crlf]Connection: Keep-Alive[crlf][crlf]",
  },
  {
    id: "forwarded-host",
    label: "X-Online-Host / forwarded",
    template:
      "[method] [host_port] [protocol][crlf]Host: [host][crlf]X-Online-Host: [host][crlf]X-Forward-Host: [host][crlf]X-Forwarded-For: [host][crlf]Connection: Keep-Alive[crlf][crlf]",
  },
];

const appInfo: AppInfo = {
  name: "Networld VPN",
  version: "1.0.0",
  coreVersion: "1.0.0",
  platform: "preview",
  storeBuild: true,
  mobileBuild: typeof location !== "undefined" && /[?&]mobile\b/.test(location.search),
  configPath: "%LOCALAPPDATA%\\Networld VPN\\config.json",
  binPath: "<install>\\resources\\bin",
  engines: [
    { kind: "ssh", label: "SSH", udp: true, tunMode: true, requiresElevation: false, sidecar: null, available: true, reason: "" },
    { kind: "xray", label: "V2Ray / Xray", udp: true, tunMode: true, requiresElevation: false, sidecar: "xray", available: true, reason: "" },
    {
      kind: "wireguard",
      label: "WireGuard",
      udp: true,
      tunMode: true,
      requiresElevation: true,
      sidecar: "wireguard",
      available: false,
      reason: "Not available in the Microsoft Store build (needs administrator rights)",
    },
  ],
};

log("info", "app", "Networld VPN core ready — 4 profile(s) loaded");
log("info", "app", "Preview mode: the Rust core is simulated in the browser");

// ---------------------------------------------------------------------------
// command router
// ---------------------------------------------------------------------------

export async function invoke<T>(cmd: string, args: any = {}): Promise<T> {
  switch (cmd) {
    case "app_info":
      return appInfo as T;
    case "get_status":
      return clone(status) as T;
    case "get_config":
      return { ...clone(config), profiles: config.profiles.map(redact) } as T;
    case "update_settings":
      config.settings = args.settings;
      emit("networld://config-changed", null);
      return undefined as T;
    case "list_profiles":
      return config.profiles.map(redact) as T;
    case "lock_profile": {
      const p = config.profiles.find((x) => x.id === args.id);
      if (!p) throw new Error("Profile not found");
      p.locked = true;
      emit("networld://config-changed", null);
      return redact(p) as T;
    }
    case "save_profile": {
      const p: ServerProfile = { ...args.profile };
      if (!p.id) p.id = uid();
      if (config.profiles.some((x) => x.id === p.id && x.locked))
        throw new Error("This config is locked and cannot be edited");
      const i = config.profiles.findIndex((x) => x.id === p.id);
      if (i >= 0) config.profiles[i] = p;
      else config.profiles.push(p);
      activate(p.id);
      emit("networld://config-changed", null);
      return redact(p) as T;
    }
    case "delete_profile":
      config.profiles = config.profiles.filter((p) => p.id !== args.id);
      if (config.activeProfileId === args.id) config.activeProfileId = config.profiles[0]?.id ?? null;
      emit("networld://config-changed", null);
      return true as T;
    case "duplicate_profile": {
      const src = config.profiles.find((p) => p.id === args.id)!;
      const copy = { ...src, id: uid(), name: `${src.name} (copy)` };
      config.profiles.push(copy);
      activate(copy.id);
      emit("networld://config-changed", null);
      return redact(copy) as T;
    }
    case "set_active_profile":
      config.activeProfileId = args.id;
      emit("networld://config-changed", null);
      return undefined as T;
    case "test_profile": {
      await sleep(900);
      const p = config.profiles.find((x) => x.id === args.id);
      log("info", "manager", `Test of '${p?.name}' succeeded`);
      return `${p?.name} — handshake in ${320 + Math.floor(Math.random() * 400)} ms` as T;
    }
    case "connect":
      return (await simulateConnect(args.id)) as T;
    case "disconnect":
      return (await simulateDisconnect()) as T;
    case "import_links": {
      const lines: string[] = String(args.text || "")
        .split(/\r?\n/)
        .map((l) => l.trim())
        .filter((l) => l && l.includes("://"));
      const imported: ServerProfile[] = [];
      const errors: string[] = [];
      for (const line of lines) {
        const scheme = line.split("://")[0].toLowerCase();
        if (["vmess", "vless", "trojan", "ss"].includes(scheme)) {
          const name = decodeURIComponent(line.split("#")[1] ?? `${scheme} node`);
          const host = (line.split("://")[1] ?? "").split("@").pop()?.split(/[?#/]/)[0] ?? "";
          imported.push(
            blankXray(name, scheme === "ss" ? "shadowsocks" : scheme, host.split(":")[0], Number(host.split(":")[1]) || 443),
          );
        } else if (scheme === "ssh" || scheme === "networld") {
          const rest = (line.split("://")[1] ?? "").split("#")[0];
          const [authority, rawQuery = ""] = rest.split("?");
          const creds = authority.includes("@") ? authority.split("@")[0] : "";
          const hostPart = authority.includes("@") ? authority.split("@")[1] : authority;
          const q = new URLSearchParams(rawQuery);
          const prof = blankSsh(
            decodeURIComponent(line.split("#")[1] ?? "SSH node"),
            hostPart.split(":")[0] ?? "",
            Number(hostPart.split(":")[1]) || 443,
          ) as any;
          prof.username = decodeURIComponent(creds.split(":")[0] ?? "");
          prof.password = decodeURIComponent(creds.split(":")[1] ?? "");
          prof.mode = q.get("mode") ?? "direct";
          prof.sni = q.get("sni") ?? "";
          prof.payload = q.get("payload") ? decodeURIComponent(q.get("payload")!) : "";
          imported.push(prof as ServerProfile);
        } else {
          errors.push(`${line}: unsupported scheme '${scheme}'`);
        }
      }
      config.profiles.push(...imported);
      if (imported.length) activate(imported[0].id);
      emit("networld://config-changed", null);
      log("info", "import", `Imported ${imported.length} config(s)`);
      return { imported: imported.length, profiles: imported, errors } as T;
    }
    case "import_nw": {
      if (!/^(NW1|NWC1):/.test(String(args.blob))) throw new Error("Not a Networld config file");
      if (String(args.blob).includes(":aes:") && !args.password)
        throw new Error("this config is locked — a password is required");
      const restored = blankSsh("Imported config");
      if (String(args.blob).includes(":lock:")) restored.locked = true;
      config.profiles.push(restored);
      activate(restored.id);
      emit("networld://config-changed", null);
      return { imported: 1, profiles: [restored], errors: [] } as T;
    }
    case "export_nw": {
      const anyLocked =
        args.lock || config.profiles.some((x) => (!args.ids.length || args.ids.includes(x.id)) && x.locked);
      const mode = args.password ? "aes" : anyLocked ? "lock" : "plain";
      return `NW1:${mode}:${btoa(JSON.stringify(args.ids)).slice(0, 48)}…` as T;
    }
    case "profile_to_link": {
      const p = config.profiles.find((x) => x.id === args.id)!;
      if (args.lock || p.locked)
        return `networld://L1.${btoa(JSON.stringify({ name: p.name, sealed: true })).slice(0, 56)}…` as T;
      if (args.lossless) return `networld://${btoa(JSON.stringify({ name: p.name })).slice(0, 60)}…` as T;
      if (p.kind === "ssh")
        return `ssh://${p.username}:***@${p.host}:${p.port}?mode=${p.mode}&sni=${p.sni ?? ""}#${encodeURIComponent(p.name)}` as T;
      if (p.kind === "xray")
        return `${p.protocol}://${p.uuid}@${p.address}:${p.port}?security=${p.tls}&type=${p.network}#${encodeURIComponent(p.name)}` as T;
      return `networld://${btoa(p.name)}` as T;
    }
    case "payload_presets":
      return presets as T;
    case "preview_payload":
      return renderPayload(
        args.req.template,
        args.req.host || "ssh.example.com",
        args.req.port || 443,
        args.req.bugHost,
        args.req.method || "CONNECT",
      ) as T;
    case "save_payload": {
      const p: SavedPayload = { ...args.payload, id: args.payload.id || uid() };
      const i = config.payloadLibrary.findIndex((x) => x.id === p.id);
      if (i >= 0) config.payloadLibrary[i] = p;
      else config.payloadLibrary.push(p);
      return config.payloadLibrary as T;
    }
    case "delete_payload":
      config.payloadLibrary = config.payloadLibrary.filter((p) => p.id !== args.id);
      return config.payloadLibrary as T;
    case "get_logs":
      return logs as T;
    case "clear_logs":
      logs.length = 0;
      return undefined as T;
    case "export_logs":
      return logs.map((l) => `[${new Date(l.tsMs).toISOString()}] ${l.level.toUpperCase()} ${l.tag} ${l.message}`).join("\n") as T;
    case "speed_test": {
      log("info", "speedtest", "Measuring latency");
      await sleep(1800);
      const r: SpeedTestResult = {
        latencyMs: 38 + Math.floor(Math.random() * 60),
        jitterMs: 2 + Math.floor(Math.random() * 12),
        downloadBps: 4.2e6 + Math.random() * 9e6,
        uploadBps: 1.1e6 + Math.random() * 3e6,
        bytesDown: 25e6,
        bytesUp: 8e6,
        durationMs: 11800,
        endpoint: "speed.cloudflare.com:443",
      };
      log("info", "speedtest", `Done — ${r.latencyMs} ms`);
      return r as T;
    }
    case "get_data_usage":
      return config.dataUsage as DataUsageLedger as T;
    case "open_config_dir":
      log("info", "app", "Opening config folder (no-op in preview)");
      return undefined as T;
    default:
      throw new Error(`Unknown command '${cmd}'`);
  }
}

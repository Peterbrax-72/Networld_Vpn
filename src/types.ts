// Mirrors the serde models in crates/networld-core.

export type ConnectionState =
  | "disconnected"
  | "resolving"
  | "connecting"
  | "authenticating"
  | "starting-proxies"
  | "connected"
  | "reconnecting"
  | "disconnecting"
  | "error";

export type SshMode = "direct" | "ssl" | "proxy" | "proxy-ssl" | "slow-dns";
/** Does the gateway answer before or after the payload is written? */
export type ResponsePosition = "pre_request" | "post_request";
export type TunnelMode = "proxy" | "tun";
export type SplitTunnelMode = "off" | "include" | "exclude";
export type LogLevel = "trace" | "debug" | "info" | "warn" | "error";

export interface SlowDnsProfile {
  resolver: string;
  publicKey: string;
  domain: string;
  transport: string;
  listenPort: number;
}

export interface SshProfile {
  host: string;
  port: number;
  username: string;
  password: string;
  privateKeyPath?: string | null;
  keyPassword?: string | null;
  mode: SshMode;
  proxyHost?: string | null;
  proxyPort?: number | null;
  proxyUser?: string | null;
  proxyPass?: string | null;
  payload: string;
  sni?: string | null;
  insecureTls: boolean;
  udpgw?: string | null;
  compression: boolean;
  keepaliveSecs: number;
  expectResponses: number;
  acceptAnyStatus: boolean;
  /** Exact status the gateway must answer with; null falls back to 2xx/101. */
  expectedStatus?: number | null;
  responsePosition: ResponsePosition;
  slowdns?: SlowDnsProfile | null;
}

export interface XrayProfile {
  protocol: "vmess" | "vless" | "trojan" | "shadowsocks" | "shadowsocks2022" | "socks";
  address: string;
  port: number;
  /** UUID (vmess/vless) or password (trojan/ss). Named `uuid` so it never
   *  collides with the profile's own `id` when the kind is flattened. */
  uuid: string;
  security: string;
  encryption: string;
  network: string;
  path: string;
  host: string;
  serviceName: string;
  sni: string;
  flow: string;
  alpn: string[];
  tls: string;
  fingerprint: string;
  publicKey: string;
  shortId: string;
  spiderX: string;
  allowInsecure: boolean;
  muxEnabled: boolean;
  muxConcurrency: number;
  /** Hand-written Xray JSON; when set it replaces the generated document. */
  rawJson?: string | null;
}

export interface WireguardProfile {
  privateKey: string;
  publicKey: string;
  presharedKey?: string | null;
  endpoint: string;
  address: string[];
  dns: string[];
  mtu: number;
  allowedIps: string[];
  persistentKeepalive: number;
}

export type ProfileKind =
  | ({ kind: "ssh" } & SshProfile)
  | ({ kind: "xray" } & XrayProfile)
  | ({ kind: "wireguard" } & WireguardProfile);

export type ServerProfile = {
  id: string;
  name: string;
  group: string;
  notes: string;
  tags: string[];
  favorite: boolean;
  locked: boolean;
  createdAtMs: number;
} & ProfileKind;

export interface DnsSettings {
  enabled: boolean;
  servers: string[];
  remoteResolve: boolean;
  dohUrl?: string | null;
}

export interface SplitTunnelSettings {
  mode: SplitTunnelMode;
  apps: string[];
  bypassCidrs: string[];
  bypassDomains: string[];
}

export interface Settings {
  theme: string;
  accent: string;
  language: string;
  autostart: boolean;
  startMinimized: boolean;
  minimizeToTray: boolean;
  connectOnLaunch: boolean;
  autoReconnect: boolean;
  maxReconnectAttempts: number;
  reconnectDelaySecs: number;
  autoSwitchConfig: boolean;
  tunnelMode: TunnelMode;
  socksPort: number;
  httpPort: number;
  allowLan: boolean;
  setSystemProxy: boolean;
  killSwitch: boolean;
  blockIpv6: boolean;
  udpgwEnabled: boolean;
  udpgwAddress: string;
  dns: DnsSettings;
  splitTunnel: SplitTunnelSettings;
  notifications: boolean;
  logLevel: LogLevel;
  keepLogsLines: number;
  updateChannel: string;
  checkUpdates: boolean;
  tunMtu: number;
  tunInterfaceName: string;
}

export interface SavedPayload {
  id: string;
  name: string;
  template: string;
  notes: string;
}

export interface DayUsage {
  up: number;
  down: number;
  seconds: number;
}

export interface DataUsageLedger {
  days: Record<string, DayUsage>;
  lifetimeUp: number;
  lifetimeDown: number;
  sessions: number;
}

export interface AppConfig {
  version: number;
  settings: Settings;
  profiles: ServerProfile[];
  activeProfileId?: string | null;
  dataUsage: DataUsageLedger;
  payloadLibrary: SavedPayload[];
  subscriptions: { id: string; name: string; url: string; lastUpdatedMs: number; autoUpdate: boolean }[];
}

export interface StatsSnapshot {
  uptimeSecs: number;
  bytesUp: number;
  bytesDown: number;
  speedUpBps: number;
  speedDownBps: number;
  activeConnections: number;
  totalConnections: number;
}

export interface StatusSnapshot {
  state: ConnectionState;
  profileId?: string | null;
  profileName?: string | null;
  engine?: "ssh" | "xray" | "wireguard" | null;
  detail: string;
  message: string;
  hint: string;
  socksPort?: number | null;
  httpPort?: number | null;
  tunnelMode: TunnelMode;
  killSwitch: boolean;
  systemProxy: boolean;
  connectedSinceMs?: number | null;
  reconnectAttempt: number;
  stats: StatsSnapshot;
}

export interface LogEntry {
  seq: number;
  tsMs: number;
  level: LogLevel;
  tag: string;
  message: string;
}

export interface EngineInfo {
  kind: string;
  label: string;
  udp: boolean;
  tunMode: boolean;
  requiresElevation: boolean;
  sidecar?: string | null;
  available: boolean;
  reason: string;
}

export interface AppInfo {
  name: string;
  version: string;
  coreVersion: string;
  platform: string;
  storeBuild: boolean;
  configPath: string;
  binPath: string;
  engines: EngineInfo[];
}

export interface PresetInfo {
  id: string;
  label: string;
  template: string;
}

export interface PreviewSegment {
  text: string;
  bytes: number;
  delayMs: number;
}

export interface PreviewResponse {
  preview: string;
  segments: PreviewSegment[];
  totalBytes: number;
}

export interface ImportReport {
  imported: number;
  profiles: ServerProfile[];
  errors: string[];
}

export interface SpeedTestResult {
  latencyMs: number;
  jitterMs: number;
  downloadBps: number;
  uploadBps: number;
  bytesDown: number;
  bytesUp: number;
  durationMs: number;
  endpoint: string;
}

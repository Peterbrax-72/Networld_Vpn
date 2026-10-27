/**
 * The only place that talks to Rust.
 *
 * Inside the Tauri shell every call goes over IPC. In a plain browser (the
 * design preview, or `npm run dev` without Tauri) it transparently falls back
 * to the simulator in `mock.ts`, so the whole UI stays clickable.
 */
import type {
  AppConfig,
  AppInfo,
  DataUsageLedger,
  ImportReport,
  LogEntry,
  PresetInfo,
  PreviewResponse,
  SavedPayload,
  ServerProfile,
  Settings,
  SpeedTestResult,
  StatusSnapshot,
} from "../types";
import * as mock from "./mock";

export const IS_TAURI =
  typeof window !== "undefined" &&
  (("__TAURI_INTERNALS__" in window) || ("__TAURI__" in window));

export const EVT = {
  status: "networld://status",
  stats: "networld://stats",
  log: "networld://log",
  config: "networld://config-changed",
} as const;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!IS_TAURI) return mock.invoke<T>(cmd, args);
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export async function listen<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (!IS_TAURI) return mock.listen<T>(event, handler);
  const { listen: tauriListen } = await import("@tauri-apps/api/event");
  const un = await tauriListen<T>(event, (e) => handler(e.payload));
  return un;
}

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  getStatus: () => call<StatusSnapshot>("get_status"),
  getConfig: () => call<AppConfig>("get_config"),
  updateSettings: (settings: Settings) => call<void>("update_settings", { settings }),

  listProfiles: () => call<ServerProfile[]>("list_profiles"),
  saveProfile: (profile: ServerProfile) => call<ServerProfile>("save_profile", { profile }),
  deleteProfile: (id: string) => call<boolean>("delete_profile", { id }),
  duplicateProfile: (id: string) => call<ServerProfile>("duplicate_profile", { id }),
  setActiveProfile: (id: string | null) => call<void>("set_active_profile", { id }),
  testProfile: (id: string) => call<string>("test_profile", { id }),

  connect: (id?: string | null) => call<StatusSnapshot>("connect", { id: id ?? null }),
  disconnect: () => call<StatusSnapshot>("disconnect"),

  importLinks: (text: string) => call<ImportReport>("import_links", { text }),
  importNw: (blob: string, password?: string | null) =>
    call<ImportReport>("import_nw", { blob, password: password ?? null }),
  exportNw: (ids: string[], password?: string | null) =>
    call<string>("export_nw", { ids, password: password ?? null }),
  profileToLink: (id: string, lossless = false) =>
    call<string>("profile_to_link", { id, lossless }),

  payloadPresets: () => call<PresetInfo[]>("payload_presets"),
  previewPayload: (req: {
    template: string;
    host: string;
    port: number;
    bugHost?: string | null;
    method?: string | null;
  }) => call<PreviewResponse>("preview_payload", { req }),
  savePayload: (payload: SavedPayload) => call<SavedPayload[]>("save_payload", { payload }),
  deletePayload: (id: string) => call<SavedPayload[]>("delete_payload", { id }),

  getLogs: () => call<LogEntry[]>("get_logs"),
  clearLogs: () => call<void>("clear_logs"),
  exportLogs: () => call<string>("export_logs"),

  speedTest: () => call<SpeedTestResult>("speed_test", { options: null }),
  dataUsage: () => call<DataUsageLedger>("get_data_usage"),
  openConfigDir: () => call<void>("open_config_dir"),
};

// ---------------------------------------------------------------------------
// formatting helpers used all over the UI
// ---------------------------------------------------------------------------

export function humanBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${i === 0 ? v.toFixed(0) : v.toFixed(2)} ${units[i]}`;
}

export function humanSpeed(bps: number): string {
  if (!Number.isFinite(bps) || bps <= 0) return "0 KB/s";
  return `${humanBytes(bps)}/s`;
}

export function formatDuration(totalSecs: number): string {
  const s = Math.max(0, Math.floor(totalSecs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  return [h, m, sec].map((v) => String(v).padStart(2, "0")).join(":");
}

export function clockTime(tsMs: number): string {
  const d = new Date(tsMs);
  return d.toLocaleTimeString(undefined, { hour12: false }) + "." + String(d.getMilliseconds()).padStart(3, "0");
}

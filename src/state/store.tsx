import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { api, EVT, listen } from "../bridge/api";
import type {
  AppConfig,
  AppInfo,
  LogEntry,
  ServerProfile,
  Settings,
  StatsSnapshot,
  StatusSnapshot,
} from "../types";

export interface Toast {
  id: number;
  kind: "ok" | "warn" | "err";
  text: string;
}

interface Store {
  ready: boolean;
  info: AppInfo | null;
  config: AppConfig | null;
  status: StatusSnapshot | null;
  logs: LogEntry[];
  toasts: Toast[];
  profiles: ServerProfile[];
  settings: Settings | null;
  busy: boolean;
  reload: () => Promise<void>;
  connect: (id?: string | null) => Promise<void>;
  disconnect: () => Promise<void>;
  saveProfile: (p: ServerProfile) => Promise<void>;
  deleteProfile: (id: string) => Promise<void>;
  duplicateProfile: (id: string) => Promise<void>;
  selectProfile: (id: string) => Promise<void>;
  patchSettings: (patch: Partial<Settings>) => Promise<void>;
  clearLogs: () => Promise<void>;
  toast: (kind: Toast["kind"], text: string) => void;
}

const Ctx = createContext<Store | null>(null);

export function StoreProvider({ children }: { children: React.ReactNode }) {
  const [ready, setReady] = useState(false);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [status, setStatus] = useState<StatusSnapshot | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [busy, setBusy] = useState(false);
  const toastSeq = useRef(0);

  const toast = useCallback((kind: Toast["kind"], text: string) => {
    const id = ++toastSeq.current;
    setToasts((t) => [...t, { id, kind, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4200);
  }, []);

  const reload = useCallback(async () => {
    const [cfg, st] = await Promise.all([api.getConfig(), api.getStatus()]);
    setConfig(cfg);
    setStatus(st);

  }, []);

  useEffect(() => {
    const disposers: Array<() => void> = [];
    let cancelled = false;
    // StrictMode mounts effects twice in dev; a listener that resolves after
    // cleanup must dispose itself instead of double-reporting every event.
    const track = async (pending: Promise<() => void>) => {
      const dispose = await pending;
      if (cancelled) dispose();
      else disposers.push(dispose);
    };
    (async () => {
      try {
        const [i, cfg, st, lg] = await Promise.all([
          api.appInfo(),
          api.getConfig(),
          api.getStatus(),
          api.getLogs(),
        ]);
        setInfo(i);
        setConfig(cfg);
        setStatus(st);
        setLogs(lg);
        setReady(true);

        await track(
          listen<StatusSnapshot>(EVT.status, (s) => setStatus((prev) => ({ ...(prev ?? s), ...s }))),
        );
        await track(
          listen<StatsSnapshot>(EVT.stats, (s) =>
            setStatus((prev) => (prev ? { ...prev, stats: s } : prev)),
          ),
        );
        await track(
          listen<LogEntry>(EVT.log, (l) =>
            setLogs((prev) => {
              if (prev.slice(-64).some((x) => x.seq === l.seq)) return prev;
              const next = [...prev, l];
              return next.length > 1200 ? next.slice(next.length - 1200) : next;
            }),
          ),
        );
        await track(listen<null>(EVT.config, () => void reload()));
      } catch (e) {
        console.error(e);
        toast("err", String(e));
        setReady(true);
      }
    })();
    return () => {
      cancelled = true;
      disposers.forEach((d) => d());
    };
  }, [reload, toast]);

  // Apply the accent colour chosen in Settings.
  useEffect(() => {
    if (config?.settings) {
      document.documentElement.style.setProperty("--accent", config.settings.accent);
      document.documentElement.dataset.theme = config.settings.theme;
    }
  }, [config?.settings?.accent, config?.settings?.theme]);

  const connect = useCallback(
    async (id?: string | null) => {
      setBusy(true);
      try {
        await api.connect(id ?? config?.activeProfileId ?? null);
      } catch (e) {
        toast("err", String(e));
      } finally {
        setBusy(false);
        void reload();
      }
    },
    [config?.activeProfileId, reload, toast],
  );

  const disconnect = useCallback(async () => {
    setBusy(true);
    try {
      await api.disconnect();
    } catch (e) {
      toast("err", String(e));
    } finally {
      setBusy(false);
      void reload();
    }
  }, [reload, toast]);

  const saveProfile = useCallback(
    async (p: ServerProfile) => {
      await api.saveProfile(p);
      await reload();
      toast("ok", `Saved “${p.name}”`);
    },
    [reload, toast],
  );

  const deleteProfile = useCallback(
    async (id: string) => {
      await api.deleteProfile(id);
      await reload();
      toast("ok", "Server removed");
    },
    [reload, toast],
  );

  const duplicateProfile = useCallback(
    async (id: string) => {
      await api.duplicateProfile(id);
      await reload();
      toast("ok", "Server duplicated");
    },
    [reload, toast],
  );

  const selectProfile = useCallback(
    async (id: string) => {
      await api.setActiveProfile(id);
      await reload();
    },
    [reload],
  );

  const patchSettings = useCallback(
    async (patch: Partial<Settings>) => {
      if (!config) return;
      const next = { ...config.settings, ...patch };
      setConfig({ ...config, settings: next });
      try {
        await api.updateSettings(next);
      } catch (e) {
        toast("err", String(e));
        await reload();
      }
    },
    [config, reload, toast],
  );

  const clearLogs = useCallback(async () => {
    await api.clearLogs();
    setLogs([]);
  }, []);

  const value = useMemo<Store>(
    () => ({
      ready,
      info,
      config,
      status,
      logs,
      toasts,
      busy,
      profiles: config?.profiles ?? [],
      settings: config?.settings ?? null,
      reload,
      connect,
      disconnect,
      saveProfile,
      deleteProfile,
      duplicateProfile,
      selectProfile,
      patchSettings,
      clearLogs,
      toast,
    }),
    [
      ready, info, config, status, logs, toasts, busy, reload, connect, disconnect,
      saveProfile, deleteProfile, duplicateProfile, selectProfile, patchSettings, clearLogs, toast,
    ],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useStore(): Store {
  const v = useContext(Ctx);
  if (!v) throw new Error("useStore must be used inside <StoreProvider>");
  return v;
}

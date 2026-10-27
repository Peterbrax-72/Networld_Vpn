import { useEffect, useState } from "react";
import { Icon } from "./components/ui";
import markUrl from "./assets/mark.png";
import { IS_TAURI } from "./bridge/api";
import { useStore } from "./state/store";
import Home from "./pages/Home";
import Tools from "./pages/Tools";
import Logs from "./pages/Logs";
import SettingsPage from "./pages/Settings";

const NAV = [
  { id: "home", label: "Home", icon: "home" },
  { id: "logs", label: "Logs", icon: "list" },
  { id: "tools", label: "Tools & speed test", icon: "gauge" },
  { id: "settings", label: "Settings", icon: "cog" },
] as const;

type PageId = (typeof NAV)[number]["id"];

export default function App() {
  const [page, setPage] = useState<PageId>("home");
  const { ready, status, toasts, info } = useStore();

  // Pages ask for navigation with window.dispatchEvent(new CustomEvent("networld:navigate", …))
  useEffect(() => {
    const onNav = (e: Event) => {
      const target = (e as CustomEvent<string>).detail as PageId;
      if (NAV.some((n) => n.id === target)) setPage(target);
    };
    window.addEventListener("networld:navigate", onNav);
    return () => window.removeEventListener("networld:navigate", onNav);
  }, []);

  if (!ready) {
    return (
      <div className="boot">
        <div className="boot-logo">
          <img src={markUrl} alt="" width={58} height={58} />
        </div>
        <p>Starting Networld VPN…</p>
      </div>
    );
  }

  const state = status?.state ?? "disconnected";

  return (
    <div className="app">
      <aside className="rail">
        <div className="rail-brand" title="Networld VPN">
          <img src={markUrl} alt="Networld VPN" width={32} height={32} />
        </div>

        <nav>
          {NAV.map((n) => (
            <button
              key={n.id}
              className={`rail-item ${page === n.id ? "active" : ""}`}
              title={n.label}
              aria-label={n.label}
              onClick={() => setPage(n.id)}
            >
              <Icon name={n.icon} size={20} />
              <span className="rail-label">{n.label.split(" ")[0]}</span>
            </button>
          ))}
        </nav>

        <div className="rail-foot" title={`${state} · v${info?.version ?? "1.0.0"}`}>
          <span className={`dot ${state}`} />
          <span className="rail-ver">v{info?.version ?? "1.0.0"}</span>
          <span className="rail-tip">
            {state === "connected" ? "Tunnel up" : state.replace("-", " ")} ·{" "}
            {IS_TAURI ? "desktop" : "preview"}
          </span>
        </div>
      </aside>

      <main className="content">
        {page === "home" && <Home />}
        {page === "logs" && <Logs />}
        {page === "tools" && <Tools />}
        {page === "settings" && <SettingsPage />}
      </main>

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`}>
            <Icon name={t.kind === "ok" ? "check" : t.kind === "warn" ? "bolt" : "x"} size={16} />
            <span>{t.text}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

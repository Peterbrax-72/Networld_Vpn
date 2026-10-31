import { useEffect, useRef, useState } from "react";
import { Icon } from "./components/ui";
import markUrl from "./assets/mark.png";
import { IS_TAURI } from "./bridge/api";
import { useStore } from "./state/store";
import Home from "./pages/Home";
import Tools from "./pages/Tools";
import Logs from "./pages/Logs";

const NAV = [
  { id: "home", label: "Home", icon: "home" },
  { id: "logs", label: "Logs", icon: "list" },
  { id: "tools", label: "Tools & speed test", icon: "gauge" },
] as const;

type PageId = (typeof NAV)[number]["id"];

export default function App() {
  const [page, setPage] = useState<PageId>("home");
  const [dir, setDir] = useState<"next" | "prev" | "">("");
  const pageRef = useRef<PageId>("home");
  pageRef.current = page;

  const go = (target: PageId, d: "next" | "prev" | "" = "") => {
    setDir(d);
    setPage(target);
  };

  // Mobile: swipe left/right on the content to move between panels.
  const touch = useRef<{ x: number; y: number; ok: boolean } | null>(null);
  const onTouchStart = (e: TouchEvent) => {
    const t = e.touches[0];
    const el = e.target as HTMLElement | null;
    // Don't hijack gestures meant for inputs, scrollable charts, popups…
    const blocked = !!el?.closest("input, textarea, select, .usage-chart, .modal, .sheet, .menu, .confirm");
    touch.current = { x: t.clientX, y: t.clientY, ok: !blocked && e.touches.length === 1 };
  };
  const onTouchEnd = (e: TouchEvent) => {
    const st = touch.current;
    touch.current = null;
    if (!st?.ok) return;
    const t = e.changedTouches[0];
    const dx = t.clientX - st.x;
    const dy = t.clientY - st.y;
    if (Math.abs(dx) < 60 || Math.abs(dx) < Math.abs(dy) * 1.5) return;
    const i = NAV.findIndex((n) => n.id === pageRef.current);
    const j = dx < 0 ? i + 1 : i - 1;
    if (j >= 0 && j < NAV.length) go(NAV[j].id, dx < 0 ? "next" : "prev");
  };
  const { ready, status, toasts, info } = useStore();

  // Pages ask for navigation with window.dispatchEvent(new CustomEvent("networld:navigate", …))
  useEffect(() => {
    const onNav = (e: Event) => {
      const target = (e as CustomEvent<string>).detail as PageId;
      if (NAV.some((n) => n.id === target)) go(target);
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
              onClick={() => {
                const from = NAV.findIndex((x) => x.id === page);
                const to = NAV.findIndex((x) => x.id === n.id);
                go(n.id, to > from ? "next" : to < from ? "prev" : "");
              }}
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

      <main
        className="content"
        onTouchStart={onTouchStart as any}
        onTouchEnd={onTouchEnd as any}
      >
        <div key={page} className={`page-swap ${dir}`}>
          {page === "home" && <Home />}
          {page === "logs" && <Logs />}
          {page === "tools" && <Tools />}
        </div>
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

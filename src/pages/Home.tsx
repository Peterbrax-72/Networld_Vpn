/**
 * Home — the whole app in one screen, NetMod style.
 *
 * A title bar with three actions, the list of saved configs, and a round
 * connect button in the corner. Everything else is one tap away.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { api, formatDuration, humanSpeed } from "../bridge/api";
import { Icon } from "../components/ui";
import ConfigEditor from "../components/ConfigEditor";
import { ImportModal, ShareModal } from "../components/share";
import logoUrl from "../assets/logo.png";
import { useStore } from "../state/store";
import {
  blankFor,
  endpointOf,
  engineBadge,
  initialOf,
  PROTOCOLS,
  transportLabel,
  type ProtoId,
} from "../lib/profiles";
import type { ServerProfile } from "../types";

const BUSY = new Set(["resolving", "connecting", "authenticating", "starting-proxies", "reconnecting"]);

export default function Home() {
  const { profiles, config, status, connect, disconnect, deleteProfile, selectProfile, toast, busy, info } =
    useStore();

  const [editing, setEditing] = useState<ServerProfile | null>(null);
  const [sharing, setSharing] = useState<ServerProfile | null>(null);
  const [importTab, setImportTab] = useState<"paste" | "qr" | "export" | null>(null);
  const [menu, setMenu] = useState<"proto" | "more" | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const head = useRef<HTMLDivElement>(null);

  const activeId = config?.activeProfileId ?? null;
  const state = status?.state ?? "disconnected";
  const connected = state === "connected";
  const working = BUSY.has(state) || busy;

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => {
      if (!head.current?.contains(e.target as Node)) setMenu(null);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setMenu(null);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [menu]);

  const add = (proto: ProtoId) => {
    setMenu(null);
    setEditing(blankFor(proto));
  };

  const go = (page: string) =>
    window.dispatchEvent(new CustomEvent("networld:navigate", { detail: page }));

  // Uptime starts counting the moment the state becomes Connected and ticks every second.
  const [now, setNow] = useState(Date.now());
  const since = connected ? status?.connectedSinceMs ?? null : null;
  useEffect(() => {
    if (!connected) return;
    setNow(Date.now());
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [connected, since]);

  const subtitle = useMemo(() => {
    if (connected) {
      const up = since ? Math.max(0, Math.floor((now - since) / 1000)) : 0;
      return `Connected · ${formatDuration(up)}`;
    }
    if (working) return status?.message || "Working…";
    return `${profiles.length} config${profiles.length === 1 ? "" : "s"}`;
  }, [connected, working, status, since, now, profiles.length]);

  return (
    <div className="home">
      <div className="home-head" ref={head}>
        <div className="home-title">
          <h1>Home</h1>
          <span className={`home-sub ${state}`}>{subtitle}</span>
        </div>

        <div className="home-acts">
          <button
            className={`icon-btn ${menu === "proto" ? "on" : ""}`}
            title="Add config"
            onClick={() => setMenu(menu === "proto" ? null : "proto")}
          >
            <Icon name="doc-add" size={21} />
          </button>
          <button className="icon-btn" title="Scan QR code" onClick={() => setImportTab("qr")}>
            <Icon name="qr" size={20} />
          </button>
          <button
            className={`icon-btn ${menu === "more" ? "on" : ""}`}
            title="More"
            onClick={() => setMenu(menu === "more" ? null : "more")}
          >
            <Icon name="dots" size={20} />
          </button>
        </div>

        {menu === "proto" && (
          <div className="menu proto-menu">
            {PROTOCOLS.filter((x) => !info?.mobileBuild || x.id === "ssh").map((x) => (
              <button key={x.id} className="menu-item" onClick={() => add(x.id)}>
                <span className="menu-label">
                  {x.label}
                  <em>{x.hint}</em>
                </span>
                <span className={`eng ${x.engine.toLowerCase()}`}>{x.engine}</span>
              </button>
            ))}
          </div>
        )}

        {menu === "more" && (
          <div className="menu more-menu">
            <button
              className="menu-item plain"
              onClick={() => {
                setMenu(null);
                setImportTab("paste");
              }}
            >
              <Icon name="download" size={15} /> Import from link or file
            </button>
            <button
              className="menu-item plain"
              onClick={() => {
                setMenu(null);
                setImportTab("export");
              }}
            >
              <Icon name="upload" size={15} /> Export all configs
            </button>
            <button
              className="menu-item plain"
              onClick={() => {
                setMenu(null);
                go("logs");
              }}
            >
              <Icon name="list" size={15} /> Logs
            </button>
            {!info?.mobileBuild && (
              <button
              className="menu-item plain"
              onClick={() => {
                setMenu(null);
                api.openConfigDir().catch((e) => toast("err", String(e)));
              }}
            >
              <Icon name="folder" size={15} /> Open config folder
              </button>
            )}
            <button
              className="menu-item plain"
              onClick={() => {
                setMenu(null);
                go("settings");
              }}
            >
              <Icon name="cog" size={15} /> Settings
            </button>
          </div>
        )}
      </div>

      {(() => {
        const act = profiles.find((x) => x.id === activeId);
        const title = connected
          ? "Connected"
          : working
            ? "Connecting…"
            : state === "error"
              ? "Connection failed"
              : "Not connected";
        const sub = connected || working
          ? status?.detail || status?.message || status?.profileName || ""
          : act
            ? `Ready · ${act.name || "Config"} — ${endpointOf(act)}`
            : "Add a config to get started";
        return (
          <section className={`hero ${state}`} aria-live="polite">
            <span className="hero-orb" aria-hidden="true">
              <Icon name={connected ? "check" : working ? "bolt" : "play"} size={18} />
            </span>
            <div className="hero-text">
              <strong className="hero-title">{title}</strong>
              <span className="hero-sub">{sub}</span>
            </div>
            <div className="hero-speed">
              <span className="hero-stat down">
                <Icon name="download" size={14} />
                <b>{humanSpeed(connected ? (status?.stats.speedDownBps ?? 0) : 0)}</b>
              </span>
              <span className="hero-stat up">
                <Icon name="upload" size={14} />
                <b>{humanSpeed(connected ? (status?.stats.speedUpBps ?? 0) : 0)}</b>
              </span>
            </div>
          </section>
        );
      })()}

      <div className="cfg-list">
        {profiles.length === 0 && (
          <div className="first-run">
            <div className="fr-mark">
              <img src={logoUrl} alt="Networld VPN" width={112} height={112} />
            </div>
            <h3>No configs yet</h3>
            <p>
              Tap the <Icon name="doc-add" size={14} /> icon to add an SSH, V2Ray or WireGuard config
              — or scan a QR code from another device.
            </p>
            <div className="row gap center">
              <button className="btn btn-primary" onClick={() => setMenu("proto")}>
                <Icon name="plus" size={15} /> Add config
              </button>
              <button className="btn" onClick={() => setImportTab("paste")}>
                <Icon name="download" size={15} /> Import
              </button>
            </div>
          </div>
        )}

        {profiles.map((p) => {
          const active = p.id === activeId;
          return (
            <article
              key={p.id}
              className={`cfg-card ${active ? "active" : ""}`}
              onClick={() => void selectProfile(p.id)}
            >
              <span className={`cfg-chip eng-${engineBadge(p).toLowerCase()}`} aria-hidden="true">
                {initialOf(p)}
              </span>
              <div className="cfg-main">
                <span className="cfg-name">
                  {p.locked && <Icon name="lock" size={12} />} {p.name || "Config"}
                </span>
                <strong className="cfg-endpoint">{endpointOf(p)}</strong>
              </div>

              <div className="cfg-side" onClick={(e) => e.stopPropagation()}>
                <div className="cfg-acts">
                  <button className="icon-btn sm" title="Share" onClick={() => setSharing(p)}>
                    <Icon name="share" size={17} />
                  </button>
                  <button className="icon-btn sm" title="Edit" onClick={() => setEditing(p)}>
                    <Icon name="edit" size={17} />
                  </button>
                  <button
                    className="icon-btn sm danger"
                    title="Delete"
                    onClick={() => setConfirm(p.id)}
                  >
                    <Icon name="trash" size={17} />
                  </button>
                </div>
                <span className="cfg-mode" title={`${engineBadge(p)} engine`}>
                  {transportLabel(p)}
                </span>
              </div>
            </article>
          );
        })}
      </div>

      <div
        className={`fab-wrap ${connected ? "on" : ""} ${working ? "working" : ""} ${profiles.length ? "" : "off"}`}
      >
        <button
          className={`fab ${connected ? "on" : ""} ${working ? "working" : ""}`}
          title={connected || working ? "Disconnect" : "Connect"}
          aria-label={connected || working ? "Disconnect" : "Connect"}
          disabled={!profiles.length}
          onClick={() => (connected || working ? void disconnect() : void connect(activeId))}
        >
          {/* the ring is drawn around the button once the tunnel is up */}
          <svg className="fab-ring" viewBox="0 0 80 80" aria-hidden="true">
            <circle className="fab-ring-track" cx="40" cy="40" r="37" />
            <circle className="fab-ring-draw" cx="40" cy="40" r="37" pathLength="100" />
          </svg>
          <span className="fab-bolt">
            <Icon name="bolt" size={connected ? 28 : 26} />
          </span>
        </button>
        <span className="fab-label">
          {working ? "CONNECTING" : connected ? "CONNECTED" : "START"}
        </span>
      </div>

      {editing && <ConfigEditor profile={editing} onClose={() => setEditing(null)} />}
      {sharing && <ShareModal profile={sharing} onClose={() => setSharing(null)} />}
      <ImportModal
        open={importTab !== null}
        initialTab={importTab ?? "paste"}
        onClose={() => setImportTab(null)}
      />

      {confirm && (
        <div className="sheet-backdrop" onMouseDown={() => setConfirm(null)}>
          <div className="confirm" onMouseDown={(e) => e.stopPropagation()}>
            <h3>Delete this config?</h3>
            <p className="muted small">
              {profiles.find((x) => x.id === confirm)?.name || "Config"} will be removed from this
              device.
            </p>
            <div className="row gap end">
              <button className="btn" onClick={() => setConfirm(null)}>
                Cancel
              </button>
              <button
                className="btn btn-danger"
                onClick={async () => {
                  const id = confirm;
                  setConfirm(null);
                  await deleteProfile(id);
                }}
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

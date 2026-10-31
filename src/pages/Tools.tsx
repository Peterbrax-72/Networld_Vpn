import { useEffect, useState } from "react";
import { api, humanBytes, humanSpeed } from "../bridge/api";
import { Badge, Button, Card, CopyButton, Stat } from "../components/ui";
import SecurityCard from "../components/SecurityCard";
import { useStore } from "../state/store";
import type { DataUsageLedger, SpeedTestResult } from "../types";

export default function Tools() {
  const { status, toast, info } = useStore();
  const [result, setResult] = useState<SpeedTestResult | null>(null);
  const [running, setRunning] = useState(false);
  const [usage, setUsage] = useState<DataUsageLedger | null>(null);

  useEffect(() => {
    api
      .dataUsage()
      .then(setUsage)
      .catch(() => void 0);
  }, [status?.state]);

  const connected = status?.state === "connected";
  const days = usage ? Object.entries(usage.days).slice(-14) : [];
  const peak = Math.max(1, ...days.map(([, d]) => d.up + d.down));

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Tools</h1>
          <p className="muted">
            Measure the tunnel and review how much data it moved
          </p>
        </div>
      </header>

      <div className="grid-2">
        <Card
          title="Speed test"
          subtitle="Runs through the active tunnel, not your physical link"
          right={
            connected ? (
              <Badge tone="ok">tunnel up</Badge>
            ) : (
              <Badge tone="warn">offline</Badge>
            )
          }
        >
          <div className="row gap">
            <Button
              variant="primary"
              icon="gauge"
              loading={running}
              disabled={!connected}
              onClick={async () => {
                setRunning(true);
                try {
                  setResult(await api.speedTest());
                } catch (e) {
                  toast("err", String(e));
                } finally {
                  setRunning(false);
                }
              }}
            >
              {running ? "Testing…" : "Run speed test"}
            </Button>
            {!connected && (
              <span className="muted small">
                Connect first to enable the test.
              </span>
            )}
          </div>

          {result && (
            <>
              <div className="stat-row mt">
                <Stat
                  label="Latency"
                  value={`${result.latencyMs} ms`}
                  sub={`jitter ${result.jitterMs} ms`}
                  icon="bolt"
                />
                <Stat
                  label="Download"
                  value={humanSpeed(result.downloadBps)}
                  sub={humanBytes(result.bytesDown)}
                  icon="download"
                  tone="down"
                />
                <Stat
                  label="Upload"
                  value={humanSpeed(result.uploadBps)}
                  sub={humanBytes(result.bytesUp)}
                  icon="upload"
                  tone="up"
                />
              </div>
              <p className="muted tiny mt">
                Endpoint {result.endpoint} ·{" "}
                {(result.durationMs / 1000).toFixed(1)} s
              </p>
            </>
          )}
        </Card>

        <SecurityCard />

        <Card title="Connection details">
          <div className="kv">
            <div>
              <span>State</span>
              <strong>{status?.state ?? "—"}</strong>
            </div>
            <div>
              <span>Engine</span>
              <strong>{status?.detail || "—"}</strong>
            </div>
            <div>
              <span>SOCKS5</span>
              <strong>
                {status?.socksPort ? `127.0.0.1:${status.socksPort}` : "—"}
              </strong>
            </div>
            <div>
              <span>HTTP proxy</span>
              <strong>
                {status?.httpPort ? `127.0.0.1:${status.httpPort}` : "—"}
              </strong>
            </div>
            {!info?.mobileBuild && (
              <>
                <div>
                  <span>Kill switch</span>
                  <strong>{status?.killSwitch ? "engaged" : "off"}</strong>
                </div>
                <div>
                  <span>System proxy</span>
                  <strong>{status?.systemProxy ? "applied" : "off"}</strong>
                </div>
              </>
            )}
          </div>
          {status?.httpPort && (
            <div className="row gap mt">
              <CopyButton
                value={`127.0.0.1:${status.httpPort}`}
                label="Copy HTTP proxy"
              />
              <CopyButton
                value={`socks5://127.0.0.1:${status.socksPort}`}
                label="Copy SOCKS URL"
              />
            </div>
          )}
        </Card>
      </div>

      <Card
        title="Data usage"
        subtitle="Recorded locally when a session ends — nothing leaves the machine"
        right={
          usage ? (
            <Badge tone="accent">
              {usage.sessions} sessions ·{" "}
              {humanBytes(usage.lifetimeDown + usage.lifetimeUp)} lifetime
            </Badge>
          ) : null
        }
      >
        {days.length === 0 ? (
          <p className="muted small">No sessions recorded yet.</p>
        ) : (
          <div className="usage-chart">
            {days.map(([day, d]) => {
              const total = d.up + d.down;
              return (
                <div
                  key={day}
                  className="usage-col"
                  title={`${day} · ↓${humanBytes(d.down)} ↑${humanBytes(d.up)}`}
                >
                  <div className="usage-bar">
                    <div
                      className="usage-down"
                      style={{ height: `${(d.down / peak) * 100}%` }}
                    />
                    <div
                      className="usage-up"
                      style={{ height: `${(d.up / peak) * 100}%` }}
                    />
                  </div>
                  <span className="tiny muted">{day.slice(5)}</span>
                  <span className="tiny">{humanBytes(total)}</span>
                </div>
              );
            })}
          </div>
        )}
      </Card>

      {info && !info.mobileBuild && (
        <Card title="Engine availability">
          <ul className="list">
            {info.engines.map((e) => (
              <li key={e.kind}>
                <div className="grow">
                  <strong>{e.label}</strong>
                  <p className="muted tiny">
                    {e.udp ? "UDP · " : ""}
                    {e.tunMode ? "TUN capable · " : ""}
                    {e.sidecar ? `sidecar: ${e.sidecar}` : "built in"}
                    {e.reason ? ` — ${e.reason}` : ""}
                  </p>
                </div>
                <Badge tone={e.available ? "ok" : "warn"}>
                  {e.available ? "ready" : "unavailable"}
                </Badge>
              </li>
            ))}
          </ul>
        </Card>
      )}
    </div>
  );
}

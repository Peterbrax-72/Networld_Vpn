import { useEffect, useRef } from "react";
import { clockTime } from "../bridge/api";
import { Button, CopyButton } from "../components/ui";
import { useStore } from "../state/store";

/** Just the log — no filters, no search, no tags. One Clear button. */
export default function Logs() {
  const { logs, clearLogs } = useStore();
  const boxRef = useRef<HTMLDivElement>(null);

  // always stick to the newest line
  useEffect(() => {
    if (boxRef.current) boxRef.current.scrollTop = boxRef.current.scrollHeight;
  }, [logs.length]);

  return (
    <div className="page logs-page">
      <header className="page-head">
        <h1>Logs</h1>
        <CopyButton
          label="Copy all"
          value={logs
            .map((l) => `${clockTime(l.tsMs)} ${l.level.toUpperCase()} [${l.tag}] ${l.message}`)
            .join("\n")}
        />
        <Button icon="trash" onClick={() => clearLogs()}>
          Clear
        </Button>
      </header>

      <div className="log-box" ref={boxRef}>
        {logs.length === 0 ? (
          <p className="muted small pad">Nothing logged yet.</p>
        ) : (
          logs.map((l) => (
            <div key={l.seq} className={`log-line ${l.level}`}>
              <span className="ts">{clockTime(l.tsMs)}</span>
              <span className={`lvl ${l.level}`}>{l.level.toUpperCase()}</span>
              <span className="tag-cell">{l.tag}</span>
              <span className="msg">{l.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

import { useEffect, useState } from "react";
import { api } from "../bridge/api";
import { Button, CopyButton, Field, Modal, Tabs, Toggle } from "./ui";
import { QrDropZone, QrPanel } from "./qr";
import { NW_FILTER, openTextFile, saveTextFile, slug } from "../bridge/files";
import { useStore } from "../state/store";
import type { ServerProfile } from "../types";

// ---------------------------------------------------------------------------
// Share — link · QR · file
// ---------------------------------------------------------------------------

export function ShareModal({ profile, onClose }: { profile: ServerProfile; onClose: () => void }) {
  const { toast } = useStore();
  const [tab, setTab] = useState("link");
  const [link, setLink] = useState("");
  const [lossless, setLossless] = useState(false);
  const [file, setFile] = useState("");
  const [pass, setPass] = useState("");

  useEffect(() => {
    let alive = true;
    api
      .profileToLink(profile.id, lossless)
      .then((l) => alive && setLink(l))
      .catch((e) => alive && toast("err", String(e)));
    return () => {
      alive = false;
    };
  }, [profile.id, lossless]);

  return (
    <Modal
      open
      title={`Share “${profile.name || "config"}”`}
      onClose={onClose}
      footer={<Button onClick={onClose}>Close</Button>}
    >
      <Tabs
        tabs={[
          { id: "link", label: "Link" },
          { id: "qr", label: "QR code" },
          { id: "file", label: "File" },
        ]}
        active={tab}
        onChange={setTab}
      />

      <Toggle
        label="Lossless networld:// link"
        hint="Keeps payload, SNI and every advanced field. Standard links only carry what their scheme supports."
        checked={lossless}
        onChange={setLossless}
      />

      {tab === "link" && (
        <div className="stack">
          <pre className="codeblock">{link || "…"}</pre>
          <div className="row gap">
            <CopyButton value={link} label="Copy link" />
            <Button
              icon="file"
              onClick={async () => {
                const path = await saveTextFile(`${slug(profile.name)}.txt`, link);
                if (path) toast("ok", `Saved ${path}`);
              }}
            >
              Save as .txt
            </Button>
          </div>
          {profile.locked && <p className="muted tiny">This config is locked — credentials are redacted in links.</p>}
        </div>
      )}

      {tab === "qr" && (
        <div className="stack center-col">
          <QrPanel value={link} name={profile.name || "config"} />
          <p className="muted tiny">Scan it with another Networld install, or any client that reads the scheme.</p>
        </div>
      )}

      {tab === "file" && (
        <div className="stack">
          <p className="muted small">
            A <code>.nw</code> bundle carries the whole config. Add a password to encrypt it with AES-256-GCM.
          </p>
          <Field label="Password (optional)">
            <input type="password" value={pass} onChange={(e) => setPass(e.currentTarget.value)} />
          </Field>
          <div className="row gap">
            <Button
              variant="primary"
              icon="file"
              onClick={async () => {
                try {
                  const blob = await api.exportNw([profile.id], pass || null);
                  setFile(blob);
                  const path = await saveTextFile(`${slug(profile.name)}.nw`, blob, [NW_FILTER]);
                  if (path) toast("ok", `Saved ${path}`);
                } catch (e) {
                  toast("err", String(e));
                }
              }}
            >
              Export .nw
            </Button>
            {file && <CopyButton value={file} label="Copy file text" />}
          </div>
          {file && <pre className="codeblock">{file}</pre>}
        </div>
      )}
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Import / export — paste · file · QR
// ---------------------------------------------------------------------------

export function ImportModal({
  open,
  onClose,
  initialTab = "paste",
}: {
  open: boolean;
  onClose: () => void;
  initialTab?: "paste" | "file" | "qr" | "export";
}) {
  const { reload, toast, profiles } = useStore();
  const [text, setText] = useState("");
  const [pass, setPass] = useState("");
  const [exported, setExported] = useState("");
  const [tab, setTab] = useState<string>(initialTab);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (open) setTab(initialTab);
  }, [open, initialTab]);

  const runImport = async (payload: string) => {
    const body = payload.trim();
    if (!body) {
      toast("warn", "Nothing to import");
      return;
    }
    setBusy(true);
    try {
      const r = /^(NW1|NWC1):/.test(body) ? await api.importNw(body, pass || null) : await api.importLinks(body);
      await reload();
      toast(r.errors.length ? "warn" : "ok", `Imported ${r.imported} config(s)`);
      if (r.errors.length) console.warn(r.errors);
      setText("");
      if (!r.errors.length) onClose();
    } catch (e) {
      toast("err", String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      wide
      title="Import & export configs"
      onClose={onClose}
      footer={<Button onClick={onClose}>Close</Button>}
    >
      <Tabs
        tabs={[
          { id: "paste", label: "Paste link" },
          { id: "file", label: "From file" },
          { id: "qr", label: "Scan QR" },
          { id: "export", label: "Export all" },
        ]}
        active={tab}
        onChange={setTab}
      />

      {tab === "paste" && (
        <>
          <Field
            label="Links or a subscription body"
            hint="vmess:// vless:// trojan:// ss:// ssh:// networld:// — one per line, base64 blobs accepted"
          >
            <textarea
              rows={8}
              className="mono"
              value={text}
              onChange={(e) => setText(e.currentTarget.value)}
              placeholder={"vmess://eyJ2IjoiMiIs…\nssh://user:pass@host:443?mode=ssl&sni=bug.host#My%20config"}
            />
          </Field>
          <Field label="Password" hint="Only needed for locked .nw files">
            <input type="password" value={pass} onChange={(e) => setPass(e.currentTarget.value)} />
          </Field>
          <div className="row gap end">
            <Button variant="primary" icon="download" loading={busy} onClick={() => runImport(text)}>
              Import
            </Button>
          </div>
        </>
      )}

      {tab === "file" && (
        <div className="stack">
          <p className="muted small">
            Pick a <code>.nw</code> bundle or any text file holding share links — one per line.
          </p>
          <Field label="Password" hint="Only needed for locked .nw files">
            <input type="password" value={pass} onChange={(e) => setPass(e.currentTarget.value)} />
          </Field>
          <div className="row gap">
            <Button
              variant="primary"
              icon="file"
              loading={busy}
              onClick={async () => {
                const picked = await openTextFile();
                if (!picked) return;
                toast("ok", `Reading ${picked.name}`);
                await runImport(picked.text);
              }}
            >
              Choose file…
            </Button>
          </div>
          {text && <pre className="codeblock">{text.slice(0, 400)}</pre>}
        </div>
      )}

      {tab === "qr" && (
        <div className="stack">
          <QrDropZone
            onText={(decoded) => {
              setText(decoded);
              void runImport(decoded);
            }}
          />
          <p className="muted tiny">
            Works with a saved QR image, a screenshot pasted with Ctrl+V, or a photo of another device's screen.
          </p>
        </div>
      )}

      {tab === "export" && (
        <div className="stack">
          <p className="muted small">
            Bundles all {profiles.length} stored config(s) into one <code>.nw</code>. Add a password to encrypt it
            with AES-256-GCM.
          </p>
          <Field label="Password (optional)">
            <input type="password" value={pass} onChange={(e) => setPass(e.currentTarget.value)} />
          </Field>
          <div className="row gap wrap">
            <Button
              variant="primary"
              icon="file"
              onClick={async () => {
                try {
                  const blob = await api.exportNw([], pass || null);
                  setExported(blob);
                  const path = await saveTextFile("networld-configs.nw", blob, [NW_FILTER]);
                  if (path) toast("ok", `Saved ${path}`);
                } catch (e) {
                  toast("err", String(e));
                }
              }}
            >
              Export .nw file
            </Button>
            {exported && <CopyButton value={exported} label="Copy file text" />}
          </div>
          {exported && <pre className="codeblock">{exported}</pre>}
        </div>
      )}
    </Modal>
  );
}

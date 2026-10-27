/**
 * QR encode (share) and decode (import).
 *
 * Both libraries are bundled, so this works offline and inside the packaged
 * app — nothing is fetched from a CDN and no camera permission is required.
 */
import React, { useEffect, useRef, useState } from "react";
import { Button, Icon } from "./ui";
import { saveDataUrl, slug } from "../bridge/files";

const QR_LIMIT = 2500; // byte-mode capacity at the error-correction level we use

/** Render `value` as a QR code; returns a data: URL. */
export function useQr(value: string, size = 240): { dataUrl: string; error: string } {
  const [dataUrl, setDataUrl] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    let alive = true;
    if (!value) {
      setDataUrl("");
      setError("");
      return;
    }
    if (value.length > QR_LIMIT) {
      setDataUrl("");
      setError(`Too long for a QR code (${value.length} chars) — share it as a file instead.`);
      return;
    }
    // Loaded on demand so the encoder stays out of the startup bundle.
    import("qrcode")
      .then(({ default: QRCode }) =>
        QRCode.toDataURL(value, {
          width: size,
          margin: 2,
          errorCorrectionLevel: "M",
          color: { dark: "#0b1220ff", light: "#ffffffff" },
        }),
      )
      .then((url) => {
        if (!alive) return;
        setDataUrl(url);
        setError("");
      })
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [value, size]);

  return { dataUrl, error };
}

/** QR panel with a download button — used by the share sheet. */
export function QrPanel({ value, name, size = 240 }: { value: string; name: string; size?: number }) {
  const { dataUrl, error } = useQr(value, size);

  if (error) return <p className="muted small">{error}</p>;
  if (!dataUrl) return <div className="qr-box skeleton" style={{ width: size, height: size }} />;

  return (
    <div className="qr-wrap">
      <div className="qr-box">
        <img src={dataUrl} width={size} height={size} alt="Configuration QR code" />
      </div>
      <Button icon="download" onClick={() => saveDataUrl(`${slug(name)}-qr.png`, dataUrl)}>
        Save PNG
      </Button>
    </div>
  );
}

/** Decode the first QR code found in an image blob. */
export async function decodeQrBlob(blob: Blob): Promise<string> {
  const { default: jsQR } = await import("jsqr");
  const bitmap = await createImageBitmap(blob);
  // Downscale very large photos — jsQR is happier under ~1600 px.
  const scale = Math.min(1, 1600 / Math.max(bitmap.width, bitmap.height));
  const w = Math.max(1, Math.round(bitmap.width * scale));
  const h = Math.max(1, Math.round(bitmap.height * scale));

  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) throw new Error("Canvas is unavailable in this webview");
  ctx.drawImage(bitmap, 0, 0, w, h);

  const image = ctx.getImageData(0, 0, w, h);
  const hit =
    jsQR(image.data, w, h, { inversionAttempts: "attemptBoth" }) ??
    // second pass with a hard threshold helps with screen photos
    jsQR(threshold(image).data, w, h, { inversionAttempts: "attemptBoth" });

  if (!hit?.data) throw new Error("No QR code found in that image");
  return hit.data;
}

function threshold(image: ImageData): ImageData {
  const out = new ImageData(new Uint8ClampedArray(image.data), image.width, image.height);
  const d = out.data;
  for (let i = 0; i < d.length; i += 4) {
    const v = (d[i] * 0.299 + d[i + 1] * 0.587 + d[i + 2] * 0.114) > 128 ? 255 : 0;
    d[i] = d[i + 1] = d[i + 2] = v;
  }
  return out;
}

/** Drop-zone + picker that yields the decoded QR payload. */
export function QrDropZone({ onText }: { onText: (text: string) => void }) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [hover, setHover] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const handle = async (blob: Blob | null | undefined) => {
    if (!blob) return;
    setBusy(true);
    setErr("");
    try {
      onText(await decodeQrBlob(blob));
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  // Ctrl+V of a screenshot straight into the modal.
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const item = Array.from(e.clipboardData?.items ?? []).find((i) => i.type.startsWith("image/"));
      if (item) void handle(item.getAsFile());
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, []);

  return (
    <div className="stack">
      <div
        className={`dropzone ${hover ? "hover" : ""} ${busy ? "busy" : ""}`}
        onDragOver={(e) => {
          e.preventDefault();
          setHover(true);
        }}
        onDragLeave={() => setHover(false)}
        onDrop={(e) => {
          e.preventDefault();
          setHover(false);
          void handle(e.dataTransfer?.files?.[0]);
        }}
        onClick={() => inputRef.current?.click()}
        role="button"
        tabIndex={0}
      >
        <Icon name="qr" size={30} />
        <strong>{busy ? "Decoding…" : "Drop a QR image here"}</strong>
        <span className="muted tiny">click to browse · or press Ctrl+V to paste a screenshot</span>
        <input
          ref={inputRef}
          type="file"
          accept="image/*"
          hidden
          onChange={(e) => void handle(e.currentTarget.files?.[0])}
        />
      </div>
      {err && <p className="err-line">{err}</p>}
    </div>
  );
}

/**
 * File in/out that works in both worlds:
 *   • inside Tauri  → native save/open dialogs via the dialog + fs plugins
 *   • in the browser → <a download> / <input type=file>
 *
 * Every helper degrades gracefully, so the Vite preview behaves like the
 * packaged app minus the native chrome.
 */
import { IS_TAURI } from "./api";

export type FileFilter = { name: string; extensions: string[] };

export const NW_FILTER: FileFilter = { name: "Networld config", extensions: ["nw"] };
const TXT_FILTER: FileFilter = { name: "Text / links", extensions: ["txt", "nw", "json", "conf"] };

function browserDownload(name: string, blob: Blob) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}

function pickBrowserFile(accept: string, as: "text"): Promise<{ name: string; text: string } | null>;
function pickBrowserFile(accept: string, as: "file"): Promise<File | null>;
function pickBrowserFile(accept: string, as: "text" | "file"): Promise<any> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return resolve(null);
      resolve(as === "file" ? file : { name: file.name, text: await file.text() });
    };
    // Safari/WebView2 need the element in the DOM for the picker to open.
    input.style.display = "none";
    document.body.appendChild(input);
    input.click();
    setTimeout(() => input.remove(), 60_000);
  });
}

/** Save UTF-8 text. Returns the chosen path (Tauri) or the file name (browser). */
export async function saveTextFile(
  defaultName: string,
  contents: string,
  filters: FileFilter[] = [TXT_FILTER],
): Promise<string | null> {
  if (IS_TAURI) {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const { writeTextFile } = await import("@tauri-apps/plugin-fs");
      const path = await save({ defaultPath: defaultName, filters });
      if (!path) return null;
      await writeTextFile(path, contents);
      return path;
    } catch {
      /* fall through to the browser path */
    }
  }
  browserDownload(defaultName, new Blob([contents], { type: "text/plain;charset=utf-8" }));
  return defaultName;
}

/** Save a PNG given as a data: URL (used for QR codes). */
export async function saveDataUrl(defaultName: string, dataUrl: string): Promise<string | null> {
  const bytes = dataUrlToBytes(dataUrl);
  if (IS_TAURI) {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const { writeFile } = await import("@tauri-apps/plugin-fs");
      const path = await save({ defaultPath: defaultName, filters: [{ name: "PNG image", extensions: ["png"] }] });
      if (!path) return null;
      await writeFile(path, bytes);
      return path;
    } catch {
      /* fall through */
    }
  }
  browserDownload(defaultName, new Blob([bytes], { type: "image/png" }));
  return defaultName;
}

/** Open a text file (links, .nw bundle). */
export async function openTextFile(filters: FileFilter[] = [TXT_FILTER]): Promise<{ name: string; text: string } | null> {
  // Android's picker greys out file types it does not know (".nw"), and the
  // native dialog hands back content:// URIs. The WebView's own file chooser
  // shows every file and gives us the contents directly.
  if (/Android/i.test(navigator.userAgent)) {
    return pickBrowserFile("*/*", "text");
  }
  if (IS_TAURI) {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const { readTextFile } = await import("@tauri-apps/plugin-fs");
      const path = await open({ multiple: false, filters });
      if (!path || typeof path !== "string") return null;
      return { name: path.split(/[\\/]/).pop() ?? path, text: await readTextFile(path) };
    } catch {
      /* fall through */
    }
  }
  return pickBrowserFile(".nw,.txt,.json,.conf,text/plain", "text");
}

function dataUrlToBytes(dataUrl: string): Uint8Array<ArrayBuffer> {
  const base64 = dataUrl.slice(dataUrl.indexOf(",") + 1);
  const bin = atob(base64);
  const out = new Uint8Array(new ArrayBuffer(bin.length));
  for (let i = 0; i < bin.length; i += 1) out[i] = bin.charCodeAt(i);
  return out;
}

/** Filename-safe slug for export names. */
export function slug(input: string, fallback = "networld"): string {
  const s = input
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48);
  return s || fallback;
}

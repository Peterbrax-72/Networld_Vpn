// Builds ONE self-contained HTML file of the whole UI running on the mock
// bridge (no Tauri, no install): double-click it to see the real look and the
// clickable behaviour of the app.   npm run preview:html  ->  UI-Preview.html (repo root)
import { build } from "vite";
import { readFileSync, writeFileSync, rmSync, readdirSync } from "node:fs";
import { join } from "node:path";

process.env.VITE_DEMO = "1"; // seed three demo configs (see src/bridge/mock.ts)
const out = ".preview-tmp";
await build({
  logLevel: "warn",
  build: {
    outDir: out,
    emptyOutDir: true,
    assetsInlineLimit: 100_000_000,
    cssCodeSplit: false,
    rollupOptions: { output: { inlineDynamicImports: true } },
  },
});

let html = readFileSync(join(out, "index.html"), "utf8");
const assets = join(out, "assets");
for (const f of readdirSync(assets)) {
  const body = readFileSync(join(assets, f), "utf8");
  if (f.endsWith(".css")) {
    html = html.replace(new RegExp(`<link[^>]*${f}[^>]*>`), () => `<style>${body}</style>`);
  } else if (f.endsWith(".js")) {
    html = html.replace(new RegExp(`<script[^>]*${f}[^>]*></script>`), () => "");
    html = html.replace("</body>", () => `<script type="module">${body.replace(/<\/script/gi, "<\\/script")}</script></body>`);
  }
}
const file = "UI-Preview.html";
writeFileSync(file, html);
rmSync(out, { recursive: true, force: true });
console.log(`wrote ${file} (${(html.length / 1024).toFixed(0)} kB)`);

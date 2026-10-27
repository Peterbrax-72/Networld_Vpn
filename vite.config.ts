import { defineConfig } from "vite";

// Networld VPN — Vite config tuned for Tauri v2.
//
// The UI is written against the React API but runs on Preact (≈3 kB core):
// esbuild compiles JSX straight to `preact/jsx-runtime`, and the aliases below
// point every `react` import at `preact/compat`. No React, no plugin runtime.
export default defineConfig({
  clearScreen: false,
  esbuild: { jsx: "automatic", jsxImportSource: "preact" },
  resolve: {
    alias: {
      react: "preact/compat",
      "react-dom/client": "preact/compat/client",
      "react-dom": "preact/compat",
      "react/jsx-runtime": "preact/jsx-runtime",
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    // Tauri v2 on Windows targets Edge WebView2 (Chromium).
    target: "chrome105",
    minify: "esbuild",
    cssMinify: "esbuild",
    sourcemap: false,
    reportCompressedSize: false,
    outDir: "dist",
  },
});

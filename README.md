# Networld VPN

A NetMod-style tunnelling client for Windows: **you build the payload, you own
the configs.** Nothing is bundled — no server list, no presets you did not
make. Configs come from your own hands, a share link, a `.nw` file or a **QR
code**, and the app ships through the **Microsoft Store as an MSIX** (plus a
portable exe and an NSIS installer on every push).

```
Preact + TypeScript + Vite  ──IPC──▶   Tauri v2 shell   ──▶   networld-core (Rust)
        src/                            src-tauri/              crates/networld-core/
```

---

## What it does

| Area | Details |
| --- | --- |
| **SSH engine** (native Rust) | Direct, **SSH over SSL/TLS**, SSH over HTTP/SOCKS proxy — russh + rustls, no sidecar needed |
| **Payload injector** | Full NetMod-style template language, 21 tokens, written straight into the Add Config sheet |
| **SNI / bug host** | Separate SNI, host-header and real-host fields for TLS and HTTP fronting |
| **V2Ray / Xray** | VMess, VLESS, Socks, Trojan, Shadowsocks, Shadowsocks-2022 and raw **JSON Config** via the `xray` sidecar |
| **WireGuard** | Native WireGuard tunnels (desktop/NSIS build — needs administrator rights) |
| **SlowDNS / DNSTT** | DNS tunnelling, chained in front of the SSH engine |
| **UDPGW** | UDP forwarding over the SSH tunnel for games and VoIP |
| **Local proxies** | SOCKS5 + HTTP CONNECT listeners, optional LAN sharing, optional system-proxy takeover |
| **TUN mode** | System-wide capture via `tun2socks` (desktop build) |
| **Split tunnelling** | Per-app include/exclude lists, bypass CIDRs, bypass domains (core + settings file; no screen in the UI) |
| **Kill switch** | Firewall-level block-all with targeted allow rules; restores cleanly on release |
| **Config portability** | Share and import by **link**, **file** (`.nw`, plain text) or **QR code** — drop an image, paste a screenshot with Ctrl+V, or export a PNG |
| **Link schemes** | `vmess://`, `vless://`, `trojan://`, `ss://`, `ssh://` and lossless `networld://` (keeps payload + SNI + every advanced field) |
| **Telemetry (local only)** | Live up/down speed, session totals, 14-day data-usage ledger, speed test |
| **Quality of life** | One-screen config list, filtered log viewer, 3 themes × 6 accents, 6 languages, autostart, tray |

Scope notes:

* **No bundled servers.** The app starts empty; the first config you add or
  import becomes the active one automatically.
* No donation features, no host checker, no location checker.
* The UI follows **NetMod**: an icon rail, one list of configs, and a round
  connect button in the corner. Everything advanced hides behind "Advanced".

---

## The screen

```
┌──┬──────────────────────────────────────────────────────┐
│ ▣│  Home                           [+config] [QR]  [⋮]  │
│ ⌘│ ─────────────────────────────────────────────────────│
│ ⇄│   Demo                                 ⇄   ✎   🗑    │
│ ◴│   ee-1.stunnel.cc:80                 (SSH + HTTP + TLS)
│ ☰│                                                      │
│ ⚙│                                              ( ▶ )   │
└──┴──────────────────────────────────────────────────────┘
```

* **add-config icon** → protocol menu: VMess · VLess · Socks · Trojan ·
  Shadowsocks · Shadowsocks 2022 · WireGuard · JSON Config (`XRAY`), SSH ·
  DNSTT + SSH (`NATIVE`).
* **Add Config sheet** — Remarks, Address + Port, Username, Password, Payload,
  **Response Status**, **Response Position**, **Proxy Type**, **TLS Type**.
  Proxy Type × TLS Type is what the core stores as the transport mode, and it
  is what the card prints as `(SSH + HTTP + TLS)`.
* **QR icon** scans a code; **⋮** holds import/export, logs and the config folder.
* The rail is three icons — **home · logs · tools** — and is icon-only; hover
  for a label. Reconnect / auto-switch / kill-switch options live on **Tools**
  (Security card). There is no Settings page: ports are chosen automatically.
* **Logs** is deliberately bare: the live stream plus one *Clear* button.

---

## Branding

`branding/networld-logo-source.png` is the artwork; `python3 scripts/make-icons.py`
derives everything else from two masters:

* **`branding/networld-icon.png`** — the full lock-up (globe, shield, NETWORLD VPN
  wordmark). Used for every icon 64 px and up: Store tiles, splash, installer.
* **`src/assets/mark.png`** — the symbol-only round badge. Used below 64 px
  (taskbar, title bar, `.ico` 16–48 frames, `LockScreenLogo`) where the wordmark
  would be unreadable, and for the rail brand + boot screen inside the app.

Re-run the script after changing either file; it rewrites 15 Tauri icons,
39 MSIX assets and the two Store listing images.

---

## Footprint

Deliberately small. The app is a Rust core plus a Preact UI — no Electron, no
React runtime, no bundled browser.

| Thing | Weight |
| --- | --- |
| UI bundle (`dist/`) | **300 KB** total; 89.6 KB main chunk, QR decoder (130 KB) lazy-loaded only when you scan |
| Node packages to build | **52** (the React toolchain is gone; esbuild compiles JSX straight to `preact/jsx-runtime`) |
| Rust dependency graph | **164 crates** (was 208 — dropped `url`/`idna`'s Unicode tables, `uuid`, a second CA root store and a duplicate AES/SHA stack) |
| Release profile | `opt-level = "s"`, fat LTO, 1 codegen unit, `panic = "abort"`, symbols stripped |
| Threads | 2 Tokio workers regardless of core count (`TOKIO_WORKER_THREADS`) |
| Per-connection memory | 16 KiB × 2 relay buffers |
| Log history | 1 500 lines in the core ring, 1 200 in the UI |
| Runtime | Edge WebView2 (already part of Windows) — the app ships no browser engine |

Tauri plugins are limited to what the app actually calls (`dialog`, `fs`,
`single-instance`, `autostart`); `shell`, `opener` and `notification` were
removed along with their capability grants.

---

## Quick start

### 1. UI development (any OS, no Rust needed)

```bash
npm install
npm run dev          # http://localhost:1420
```

Outside Tauri the UI runs against `src/bridge/mock.ts` — a full in-browser
simulator with seed servers, a connect state machine, streaming logs, a payload
renderer and fake traffic counters. Every screen is explorable without a
backend.

### 2. Full Windows app

Requirements: Windows 10 1809+, Node 20+, Rust stable (MSVC), the
[WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/),
and **NASM** (`winget install nasm`) — `ring`, used by rustls and russh, needs
it to assemble its x86-64 primitives.

```powershell
npm install
pwsh .\scripts\fetch-sidecars.ps1       # optional engine binaries
npm run tauri dev                        # or: npm run tauri build
```

### 3. Packages

Push to GitHub and the **Windows build** workflow produces three artifacts on
every run:

| Artifact | What it is |
| --- | --- |
| `NetworldVPN-<ver>-portable-exe` | a single `NetworldVPN.exe`, no installation |
| `NetworldVPN-<ver>-installer` | NSIS setup with shortcuts and an uninstaller |
| `NetworldVPN-<ver>-msix` | the Store / sideload MSIX package |

Tagging `v*` additionally drafts a GitHub release with all three attached.

### 4. Continuous integration

| Workflow | Trigger | What it guards |
| --- | --- | --- |
| **CI** (`ci.yml`) | push · PR | `cargo fmt`/`clippy -D warnings`/tests on `networld-core`, front-end typecheck + build with a bundle-size table, MSIX asset ↔ manifest cross-check, PowerShell parse check |
| **Windows build** (`windows-build.yml`) | push · PR · tag · manual | portable exe, NSIS installer and MSIX; signs a sideload MSIX and drafts a release on `v*` tags |
| **Security audit** (`security.yml`) | weekly · lockfile changes · manual | `cargo audit` (RustSec), `npm audit` on runtime deps, and a licence sweep that fails if a GPL crate enters the tree |
| **Store submission** (`store-submit.yml`) | manual | validates a built MSIX (identity, publisher, revision 0) then uploads it to Partner Center — skips cleanly when the Azure AD secrets are absent |

Dependabot (`.github/dependabot.yml`) keeps npm, Cargo and the actions
themselves current, grouped so Tauri and the crypto stack land as single PRs.
Accepted RustSec advisories live in `.cargo/audit.toml`, each with a reason and
a revisit trigger.
Locally:

```powershell
.\packaging\msix\Build-Msix.ps1 `
    -Version 1.0.0.0 `
    -IdentityName "12345Networld.NetworldVPN" `
    -Publisher "CN=A1B2C3D4-5E6F-7890-ABCD-EF1234567890" `
    -PublisherDisplayName "Networld"
```

Full walkthrough: **[docs/MSIX-PUBLISHING.md](docs/MSIX-PUBLISHING.md)**.
Sharing formats: **[docs/SHARING.md](docs/SHARING.md)**.

---

## Layout

```
crates/networld-core/     Tauri-free Rust engine library (33 unit tests)
  src/engines/            ssh_engine · slowdns · xray · wireguard  (pluggable)
  src/{ssh,tls,dialer}.rs transport stack
  src/payload.rs          template renderer
  src/{socks,http_proxy}.rs  local listeners sharing a hot-swappable DialerSlot
  src/{killswitch,splittunnel,sysproxy,tun}.rs  Windows integrations
src-tauri/                Tauri v2 shell — 26 IPC commands, tray, autostart
src/                      React UI (7 pages, no external CSS/font/JS assets)
  components/qr.tsx       QR encode (share) + decode (import, drop/paste)
  bridge/files.ts         native save/open dialogs, browser download fallback
packaging/msix/           AppxManifest · priconfig · Build-Msix.ps1 · Assets
scripts/                  make-icons.py · fetch-sidecars.ps1
branding/                 logo masters, Store listing art, screenshots
.github/workflows/        ci · windows-build · security · store-submit
docs/                     publishing, architecture, engines, payload reference
```

---

## Architecture in one paragraph

`networld-core` owns everything that touches the network and never links
Tauri, so it compiles and tests on Linux/macOS CI. A `VpnManager` holds the
active profile, a `LogBus`, traffic counters and a 1 s supervisor tick,
and emits `ManagerEvent::{Status,Stats,ConfigChanged}`. Each protocol is a
`TunnelEngine` that returns an `EngineHandle` plus a `Dialer`; the SOCKS and
HTTP listeners start **once** and read the current dialer from a shared
`DialerSlot`, so reconnecting never drops a local port or the system-proxy
setting. The Tauri layer is a thin bridge: 26 commands plus four
`networld://…` events forwarded to the UI. See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

Adding a protocol is a three-step contract — drop a file in
`crates/networld-core/src/engines/`, add one arm to `engines::build()`, add one
row to `capability_matrix()`. See [docs/ENGINES.md](docs/ENGINES.md).

---

## Status

- ✅ Rust core: `cargo test -p networld-core` → **33 passed**,
  `cargo clippy -- -D warnings` clean, `cargo fmt --check` clean
- ✅ Front-end: `tsc --noEmit` clean, Vite build clean, all 7 pages implemented,
  zero console errors, no horizontal overflow at 1280×820
- ✅ Sharing: link ⇄ QR ⇄ file round-trip verified in-browser (export a QR,
  delete the config, re-import the PNG, config returns intact)
- ✅ MSIX pipeline: manifest, assets, PRI config, pack/sign script, CI workflows
  producing exe + installer + MSIX
- ⏳ Needs a Windows machine (or the CI job) for the first end-to-end
  `cargo build` of the Tauri shell — this sandbox has no WebView2/webkit
- ⏳ Partner Center identity values are placeholders until you create the app
  reservation

## Licence

MIT — see [LICENSE](LICENSE). Bundled sidecars keep their own licences; see
[src-tauri/resources/bin/README.md](src-tauri/resources/bin/README.md).

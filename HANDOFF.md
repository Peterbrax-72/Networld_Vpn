# Networld VPN — handoff notes

A Windows VPN/tunnelling client with NetMod-style capabilities, built as a
Tauri v2 app: a Rust core (`crates/networld-core`), a thin Tauri shell
(`src-tauri`), and a Preact + TypeScript UI (`src`). Packaged for the
Microsoft Store via MSIX.

This file is a briefing for whoever picks the project up next — what is real,
what is verified, and where the sharp edges are.

---

## Product rules (fixed)

* Target platform is **Windows PC**. MSIX publishing to the Microsoft Store is
  a first-class requirement.
* **Excluded on purpose, do not add back:** donation features, host checker,
  location checker.
* Feature set: SSH (Direct / SSL-TLS / Proxy) with payload injector and
  SNI/bug host, V2Ray-Xray (VMess, VLESS, Trojan, Shadowsocks, SS2022, raw
  JSON), WireGuard, SlowDNS/DNSTT, UDPGW, split tunnel, kill switch, config
  import/export and share links, logs, speed/data counters, themes, autostart,
  multi-language.

---

## Layout

```
crates/networld-core/   24 modules, Tauri-free, 46 unit tests
  config.rs             profiles + settings + atomic config.json persistence
  engine.rs engines/    pluggable TunnelEngine: ssh_engine, xray, wireguard, slowdns
  dialer.rs transport.rs ssh.rs tls.rs payload.rs   SSH-over-TLS/proxy + payload injection
  socks.rs http_proxy.rs relay.rs                   local SOCKS5 + HTTP proxy
  killswitch.rs sysproxy.rs splittunnel.rs tun.rs   Windows integration
  links.rs              share links + .nw import/export (AES-256-GCM)
  logging.rs stats.rs speedtest.rs sidecar.rs manager.rs uri.rs
src-tauri/              26 IPC commands, tray, single instance, capabilities
src/                    Preact UI: App, pages/{Home,Logs,Tools,Settings},
                        components/{ConfigEditor,share,qr,ui}, state/store.tsx,
                        bridge/{api,mock,files}.ts, lib/profiles.ts
packaging/msix/         AppxManifest.xml, Assets (39), Build-Msix.ps1, priconfig
scripts/                make-icons.py, fetch-sidecars.ps1
branding/               logo masters + Store listing art + 9 screenshots
.github/                ci, windows-build, security, store-submit, dependabot
docs/                   MSIX-PUBLISHING, ARCHITECTURE, PAYLOADS, SHARING
```

---

## Commands

```bash
npm ci && npm run dev          # UI on :1420 against src/bridge/mock.ts
npm run build                  # tsc --noEmit && vite build
npm run preview:html           # regenerates UI-Preview.html (open it, no install)
cargo test -p networld-core    # 46 tests
cargo clippy -p networld-core --all-targets -- -D warnings
cargo fmt --all -- --check
python3 scripts/make-icons.py  # regenerate 15 Tauri icons + 39 MSIX assets
npm run tauri build            # Windows only
pwsh packaging/msix/Build-Msix.ps1 -Target Store   # Windows only
```

`src/bridge/api.ts` detects Tauri and falls back to `mock.ts` in a plain
browser, so the whole UI is clickable on Linux/macOS without compiling Rust.

---

## State of play

**Verified here:** `cargo fmt`, `cargo clippy -D warnings`, 46/46 core tests,
`tsc --noEmit`, `npm run build`, and Playwright passes over the real UI
(config create → connect → share/export `.nw` → import NW1 + legacy NWC1 →
logs). All GitHub workflow YAML parses; the Linux-runnable CI steps were
extracted and executed successfully.

**Never compiled or executed anywhere:** `src-tauri` (needs Windows +
WebView2), `Build-Msix.ps1`, `fetch-sidecars.ps1`. They are reviewed but
unproven — the first Windows CI run is the real test.

**Known open items**

1. `src/pages/Logs.tsx` renders list children keyed by `log.seq`, which can
   repeat → duplicate-key warning.
2. `tauri-plugin-autostart` is initialised but nothing calls enable/disable;
   the Settings toggle for it was removed, so autostart currently relies on the
   MSIX `StartupTask` extension only.
3. Settings no longer exposes Theme / accent / Language pickers (removed by
   request). The values still live in `config.json` and still work — only the
   UI is gone. Defaults must be changed in **two** places if touched:
   `src/bridge/mock.ts` and `crates/networld-core/src/config.rs`.
4. Xray cannot do ShadowsocksR; the menu ships Shadowsocks + SS2022 instead.

---

## Conventions and traps

* **`#[serde(flatten)]` collisions.** Flattened kind structs must not define
  `id, name, group, notes, tags, favorite, locked, createdAtMs`. `XrayProfile`
  uses `uuid` for exactly this reason.
* **Adding a required field to `SshProfile`/`XrayProfile` in `src/types.ts`
  breaks `src/bridge/mock.ts`** (TS2352) — update the mock seeds in the same
  change.
* **Preact via `preact/compat` aliases, no Vite plugin** (esbuild
  `jsx: automatic`, `jsxImportSource: preact`). Consequences: use
  `e.currentTarget` not `e.target`, `e.dataTransfer` is nullable, `spellcheck`
  is lowercase, and **`className` is not mapped on SVG elements — SVG must use
  `class=`**.
* **`cargo clippy --fix` fails in this workspace** (exit 101). Fix lints by hand.
* `stats.rs` must not use `AtomicU64::fetch_update` (deprecated) — it uses
  `compare_exchange_weak`.
* Config file format is **`.nw`**: `NW1:plain:<b64>` or `NW1:aes:<salt>:<b64>`;
  the importer also accepts legacy `NWC1:` and bare JSON arrays.
* The word for the `.nw` encryption secret is **password** everywhere (UI,
  IPC args, core API, errors) — not "passphrase".
* Release profile: `opt-level = "s"`, fat LTO, 1 codegen unit, `panic = abort`,
  stripped. Tokio is capped at 2 worker threads via `TOKIO_WORKER_THREADS` in
  `src-tauri/src/main.rs`.
* Icons come from two masters: `branding/networld-icon.png` (full lock-up, used
  ≥64 px) and `branding/networld-symbol.png` (round badge, used <64 px).
  Re-run `scripts/make-icons.py` after changing either.

---

## IPC contract

27 commands: `app_info, get_status, get_config, update_settings, list_profiles,
save_profile, lock_profile, delete_profile, duplicate_profile, set_active_profile,
test_profile, connect, disconnect, import_links, import_nw, export_nw,
profile_to_link, payload_presets, preview_payload, save_payload, delete_payload,
get_logs, clear_logs, export_logs, speed_test, get_data_usage, open_config_dir`.

Events: `networld://status`, `networld://stats`, `networld://log`,
`networld://config-changed`. Arguments are camelCase.

---

## Locked configs

Export panels (Share → Link/QR/File, and Import & export → Export all) have a
**Lock credentials** switch. A locked profile has `locked: true`; the core's
`ServerProfile::redacted()` replaces SSH login/password/key, proxy host and
login, Xray uuid/keys/raw JSON and WireGuard keys with `••••••••`, and the server/proxy ports (reported as 0, drawn as dots by the UI) before
anything reaches the UI (`get_config`, `list_profiles`, `save_profile`,
duplicate, import reports). `save_profile` refuses to touch a locked profile
and there is no unlock. Locked configs travel as `networld://L1.…` links or
`NW1:lock:…` files, sealed with a key embedded in the app (a deterrent, not
strong secrecy; a `.nw` password is the real protection). Standard links are
never produced for a locked config. `lock_profile` locks a config on this
device irreversibly. Core tests cover redaction and both sealed formats.

## Before publishing

`packaging/msix/AppxManifest.xml` carries three `REPLACE-ME` placeholders
(Identity Name, Publisher, Publisher Display Name) that come from Partner
Center. Set them as repo variables `MSIX_IDENTITY_NAME`, `MSIX_PUBLISHER`,
`MSIX_PUBLISHER_DISPLAY_NAME`; `store-submit.yml` refuses to submit while a
placeholder survives. Store packages must end in revision `0`.

Sandboxed MSIX cannot install drivers or services, so the Store build defaults
to proxy-only: `cargo build --features store` makes `app_info` report
WireGuard/TUN as unavailable, and `fetch-sidecars.ps1 -StoreSafe` skips the
GPL-3.0 `tun2socks` binary.

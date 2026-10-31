# Architecture

## Three layers

```
┌─────────────────────────────────────────────────────────────┐
│  src/            Preact (React API) + TypeScript + Vite     │
│  pages/          Home (configs + connect) · Logs ·          │
│                  Tools (speed test + Security card)         │
│  components/     ConfigEditor (add/edit sheet) · share · ui │
│  lib/profiles.ts protocol menu, blank configs, card labels  │
│  state/store.tsx single context store, event subscriptions  │
│  bridge/api.ts   26 typed command wrappers + 4 event feeds  │
│  bridge/files.ts native save/open dialogs ↔ browser download│
│  components/qr   QR encode (share) + decode (drop/paste)    │
│  bridge/mock.ts  browser simulator (used when not in Tauri) │
│  uri.rs          pocket URI parser — keeps `url`/`idna` out  │
└───────────────────────────┬─────────────────────────────────┘
                            │ Tauri IPC (camelCase args)
┌───────────────────────────┴─────────────────────────────────┐
│  src-tauri/      Tauri v2 shell — commands, tray, autostart │
│                  single instance, close-to-tray, event pump │
└───────────────────────────┬─────────────────────────────────┘
                            │ plain Rust calls (no Tauri types)
┌───────────────────────────┴─────────────────────────────────┐
│  crates/networld-core/   VpnManager · engines · transports  │
│                          listeners · Windows integrations   │
└─────────────────────────────────────────────────────────────┘
```

`networld-core` never links Tauri. That is what lets `cargo test -p
networld-core` run on Linux CI while the GUI only builds on Windows.

---

## Connection lifecycle

```
UI connect(id)
   └─▶ VpnManager::connect
         ├─ load ServerProfile            (config.rs)
         ├─ engines::build(kind)          (engines/mod.rs)
         ├─ engine.start(profile, ctx)    → EngineHandle + Arc<dyn Dialer>
         │     SSH:  TcpStream ─▶ [TLS]/[proxy CONNECT]/[payload] ─▶ russh
         │     Xray: spawn sidecar, wait for its local SOCKS port
         │     SlowDNS: spawn dnstt-client, then chain the SSH engine onto it
         │     WireGuard: bring the OS tunnel up → DirectDialer
         ├─ DialerSlot::set(dialer)       ← SOCKS/HTTP listeners pick it up live
         ├─ sysproxy::apply()             (optional)
         ├─ killswitch::engage()          (optional)
         └─ supervisor tick 1 s: stats → ManagerEvent::Stats, health → Status
```

**Hot-swap:** `SocksServer` and `HttpProxyServer` bind once at startup and read
the active dialer out of a shared `DialerSlot` for every accepted connection.
Reconnecting or switching servers therefore never closes a listening port,
never drops the system-proxy setting, and never breaks an app that already
pointed at `127.0.0.1:1080`.

---

## The transport stack (SSH family)

```
DirectDialer ─┐
Socks5Dialer ─┼─▶ transport.rs  ─▶  tls.rs (rustls, custom SNI)  ─┐
HTTP CONNECT ─┘                                                   ├─▶ payload.rs
                                                                  │   (raw bytes
                                                                  │    injected
                                                                  ▼    first)
                                                            russh client
                                                        channel_open_direct_tcpip
                                                                  │
                                                     SocksServer / HttpProxyServer
```

* `connect_stream` from russh 0.64 accepts any `AsyncRead + AsyncWrite` stream,
  which is the hook that makes "SSH over TLS over a proxy with a payload"
  a composition of three small pieces rather than a special case.
* Payload injection runs for **any** SSH mode when the template is non-empty;
  otherwise Proxy modes fall back to a standard `CONNECT host:port` with
  optional Basic auth.
* `expect_responses` consumes N HTTP response heads before handing the socket
  over; `accept_any_status` skips the 2xx/101 check for servers that answer
  `HTTP/1.1 400` but still upgrade.

---

## IPC contract

26 commands, registered in `src-tauri/src/lib.rs`, mirrored one-for-one in
`src/bridge/api.ts` and `src/bridge/mock.ts`.

| Group | Commands |
| --- | --- |
| App | `app_info`, `get_status`, `get_config`, `update_settings`, `open_config_dir` |
| Profiles | `list_profiles`, `save_profile`, `delete_profile`, `duplicate_profile`, `set_active_profile`, `test_profile` |
| Tunnel | `connect`, `disconnect` |
| Share | `import_links`, `import_nw`, `export_nw`, `profile_to_link` |
| Payload | `payload_presets`, `preview_payload`, `save_payload`, `delete_payload` |
| Logs | `get_logs`, `clear_logs`, `export_logs` |
| Metrics | `speed_test`, `get_data_usage` |

Events pushed from Rust to the UI:

| Event | Payload | Cadence |
| --- | --- | --- |
| `networld://status` | `StatusSnapshot` | on change |
| `networld://stats` | `StatsSnapshot` | 1 s while connected |
| `networld://log` | `LogEntry` | as emitted (UI caps at 2500) |
| `networld://config-changed` | `AppConfig` | after any mutation |

Argument names are **camelCase** on the wire. `preview_payload` takes a single
`req` object; `connect` takes `id`.

---

## Configs are user-owned

Nothing ships preloaded: `AppConfig::default()` has an empty `profiles` vector
and `active_profile_id: None`. `VpnManager::upsert_profile` promotes the first
config that arrives — typed in, imported from a link, a `.nw` file or a QR
code — to active, and `delete_profile` hands the pointer to the next one left.
The UI mirrors that in `src/bridge/mock.ts` so the browser preview behaves the
same. Sharing formats live in [SHARING.md](SHARING.md).

## State & storage

* Config lives at `%LOCALAPPDATA%\Networld VPN\config.json`, written atomically
  (temp file + rename) so a crash mid-save cannot corrupt it.
* `AppConfig` = `{ profiles, active_profile_id, settings, data_usage,
  payload_library, subscriptions }`.
* `ServerProfile` carries shared fields plus a `#[serde(flatten)]`
  `ProfileKind` tagged on `kind`.
  **Flattened kind structs must never reuse the field names** `id`, `name`,
  `group`, `notes`, `tags`, `favorite`, `locked`, `createdAtMs` — a collision
  produces a `duplicate field` error at *deserialise* time only. `XrayProfile`
  stores its credential in `uuid` for exactly this reason, and
  `flattened_kinds_do_not_collide_with_profile_id` guards it.

---

## Windows integrations

| Module | Mechanism |
| --- | --- |
| `sysproxy.rs` | per-user `HKCU\…\Internet Settings` + `InternetSetOption` refresh |
| `killswitch.rs` | `netsh advfirewall`: targeted allow rules (app path, server IP, loopback, DHCP 67/68, optional LocalSubnet) then `firewallpolicy blockinbound,blockoutbound`; release restores `blockinbound,allowoutbound` and deletes all `NetworldVPN-*` rules |
| `splittunnel.rs` | include/exclude app lists enforced at the proxy layer; bypass CIDRs and domains short-circuit to `DirectDialer` |
| `tun.rs` | spawns `tun2socks` against the local SOCKS port (desktop build only) |

---

## Front-end conventions

* No external CSS, fonts or scripts — one `src/styles.css`, inline SVG icons,
  and one bundled asset (`src/assets/logo.png`, the app mark). The app renders
  identically offline and inside a sandboxed preview.
* Theme = `html[data-theme]` (`midnight` | `carbon` | `aurora`) plus an
  `--accent` custom property; both are applied by the store on settings change.
  The Settings page no longer exposes pickers for them — they stay in
  `config.json` for power users and future presets.
* Everything that can fail shows a toast (4.2 s) and a log line; nothing fails
  silently.

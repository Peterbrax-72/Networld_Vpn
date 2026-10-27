# Publishing Networld VPN to the Microsoft Store (MSIX)

Everything here assumes Windows 10 1809+ with the **Windows 10/11 SDK**
installed (`winget install Microsoft.WindowsSDK.10.0.22621`), which supplies
`makeappx.exe`, `makepri.exe` and `signtool.exe`.

---

## 0. Why a custom MSIX step?

Tauri's bundler emits **NSIS** and **MSI** only — it has no MSIX target. So the
pipeline builds the app *unpackaged* (plain `cargo build --release`) and then
packages the resulting executable with the SDK tools:

```
dist/ + networld-vpn.exe + Assets/ + resources/bin/
          │
          ├── AppxManifest.xml   (identity, capabilities, tiles, startup task)
          ├── makepri new        → resources.pri
          └── makeappx pack      → NetworldVPN_1.0.0.0_x64.msix
```

`packaging/msix/Build-Msix.ps1` does all of it.

---

## 1. Reserve the name in Partner Center

1. <https://partner.microsoft.com/dashboard> → **Apps and games** → **New product** → **MSIX or PWA app**.
2. Reserve **Networld VPN** (take the alternative names you want too).
3. Open **Product management → Product identity** and copy three values:

| Partner Center field | Goes into |
| --- | --- |
| **Package/Identity/Name** (e.g. `12345Networld.NetworldVPN`) | `-IdentityName` |
| **Package/Identity/Publisher** (e.g. `CN=A1B2C3D4-…`) | `-Publisher` |
| **Package/Properties/PublisherDisplayName** | `-PublisherDisplayName` |

The manifest in the repo ships with `REPLACE-ME` placeholders on purpose — a
mismatched identity is the single most common submission rejection.

Store them as repository **variables** (not secrets — they are public):

```
MSIX_IDENTITY_NAME          12345Networld.NetworldVPN
MSIX_PUBLISHER              CN=A1B2C3D4-5E6F-7890-ABCD-EF1234567890
MSIX_PUBLISHER_DISPLAY_NAME Networld
```

---

## 2. Build the package

### Locally

```powershell
.\packaging\msix\Build-Msix.ps1 `
    -Version 1.0.0.0 `
    -IdentityName "12345Networld.NetworldVPN" `
    -Publisher "CN=A1B2C3D4-5E6F-7890-ABCD-EF1234567890" `
    -PublisherDisplayName "Networld"
# → packaging\msix\out\NetworldVPN_1.0.0.0_x64.msix
```

### In CI

Every push runs **Windows build (exe · MSIX · installer)** on `windows-latest`
and uploads all three artifacts. Push a tag to also draft a release:

```bash
git tag v1.0.0 && git push origin v1.0.0
```

Three repo settings drive the packaging workflows:

| Setting | Kind | Used by |
| --- | --- | --- |
| `MSIX_IDENTITY_NAME`, `MSIX_PUBLISHER`, `MSIX_PUBLISHER_DISPLAY_NAME` | variables | `windows-build.yml` (manifest patching), `store-submit.yml` (validation) |
| `SIDELOAD_PFX_BASE64`, `SIDELOAD_PFX_PASSWORD` | secrets | signing the sideload MSIX on `v*` tags |
| `AZURE_AD_TENANT_ID`, `AZURE_AD_CLIENT_ID`, `AZURE_AD_CLIENT_SECRET` + `STORE_PRODUCT_ID` | secrets + variable | `store-submit.yml` (Partner Center upload) |

Run `store-submit.yml` once with **publish = false**: it downloads the MSIX from
the latest successful Windows build, checks the Identity/Publisher against your
repo variables and that the version ends in revision `0`, then leaves a draft in
Partner Center for you to inspect.

The workflow installs NASM before building (`ring`, used by rustls and russh,
assembles with NASM on `windows-msvc` and fails in `build.rs` without it), then
builds twice: once with the full feature set for the portable exe and the NSIS
installer, once with `--features store` for the MSIX. Untagged builds get the
run number as the revision (`1.0.0.<run>`); tagged builds use the tag.

**Versioning rule:** MSIX versions are `Major.Minor.Build.Revision` and the
Store requires **Revision = 0**. `v1.2.3` becomes `1.2.3.0` automatically.
Every submission must have a strictly higher version than the last.

---

## 3. Store-safe behaviour (already handled in code)

The Store MSIX runs in a sandbox that **cannot install drivers or elevated
services**. Building with `--features store` makes `app_info` report those
engines as unavailable, and the UI greys them out with a reason instead of
failing at connect time:

| Capability | Store (MSIX) | Sideload / NSIS |
| --- | --- | --- |
| SSH Direct / SSL-TLS / Proxy + payload | ✅ | ✅ |
| Local SOCKS5 + HTTP proxy | ✅ | ✅ |
| Xray (VMess/VLESS/Trojan/SS) | ✅ | ✅ |
| SlowDNS / DNSTT, UDPGW | ✅ | ✅ |
| System-proxy takeover | ✅ (per-user registry) | ✅ |
| Split tunnelling (proxy-level) | ✅ | ✅ |
| Kill switch (firewall rules) | ⚠️ needs elevation at runtime | ✅ |
| **TUN mode** (`tun2socks` + WinTun driver) | ❌ hidden | ✅ |
| **WireGuard** (tunnel service) | ❌ hidden | ✅ |

Ship the two "❌" features through the NSIS installer channel; keep the Store
build proxy-only.

### Declared capabilities

| Capability | Why |
| --- | --- |
| `internetClient` | outbound tunnel connections |
| `internetClientServer` | local SOCKS/HTTP listeners, optional LAN sharing |
| `privateNetworkClientServer` | LAN sharing, local-subnet bypass |
| `runFullTrust` (restricted) | mandatory for any Win32/Desktop-Bridge payload |

`runFullTrust` is auto-approved for Desktop Bridge apps; no extra request is
needed. Do **not** add `networkingVpnProvider` unless you rewrite the data path
as a UWP VPN plug-in — the Store will ask for justification you cannot give.

---

## 4. Submission checklist

- [ ] Identity matches Partner Center exactly (`Build-Msix.ps1` warns otherwise).
- [ ] Version is `X.Y.Z.0`, higher than the previous submission.
- [ ] GPL-3.0 sidecars (`tun2socks`) are **not** in the package — use `-StoreSafe`.
- [ ] Age rating questionnaire completed; **"this app accesses the internet"** = yes.
- [ ] Privacy policy URL is live (the app stores everything locally — say so).
- [ ] Store listing screenshots: 1366×768 or larger; `branding/store-hero-2400x1200.png` works for the hero image.
- [ ] Submission notes tell the reviewer how to test:

  > Networld VPN is a tunnelling client; it needs a server the reviewer owns.
  > Test with: Home → **add-config icon** → SSH → address/port/username/password,
  > or paste a `vmess://` link via Home → **⋮ → Import**. Without a server, the app opens,
  > the UI is fully navigable, and connect fails with a clear error — that is
  > expected behaviour, not a crash.

- [ ] Note that the app bundles `xray.exe` as a plain file (no service, no driver).

---

## 5. Sideload testing before submission

```powershell
.\packaging\msix\New-SelfSignedCert.ps1       # subject must equal the manifest Publisher
# elevated:
Import-Certificate -FilePath .\packaging\msix\NetworldTest.cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople
.\packaging\msix\Build-Msix.ps1 -Target Sideload -CertPath .\packaging\msix\NetworldTest.pfx -CertPassword (Read-Host -AsSecureString)
Add-AppxPackage .\packaging\msix\out\NetworldVPN_1.0.0.0_x64.msix
```

To sign in CI instead, set the secrets `SIDELOAD_PFX_BASE64`
(`[Convert]::ToBase64String([IO.File]::ReadAllBytes("sign.pfx"))`) and
`SIDELOAD_PFX_PASSWORD`, then run the release workflow with
**sideload = true**.

---

## 6. Troubleshooting

| Symptom | Cause / fix |
| --- | --- |
| `makeappx: error 0x80080204` | Manifest/asset mismatch — an image named in `AppxManifest.xml` is missing from `Assets\`. Re-run `python3 scripts/make-icons.py`. |
| Partner Center: *"package identity does not match"* | `-IdentityName`/`-Publisher` differ from Product identity. |
| Partner Center: *"invalid package version"* | Revision must be `0` and the version must increase. |
| Install fails with `0x800B0109` | Sideload cert not in **Trusted People** on that machine. |
| App starts then exits immediately | WebView2 runtime missing on the test machine. |
| `ring` build error `nasm not found` | Install NASM (`winget install nasm`) or keep the `ilammy/setup-nasm` CI step. |
| Tray icon duplicated | Define the tray **either** in `tauri.conf.json` **or** in `src-tauri/src/tray.rs` — this repo uses the Rust side only. |
| Kill switch silently does nothing | Firewall edits need elevation; the Store build cannot elevate itself. Surface the error from `killswitch::engage()` in the UI. |

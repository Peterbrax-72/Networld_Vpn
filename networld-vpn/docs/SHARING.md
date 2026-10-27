# Sharing configs — link · file · QR

Networld ships with **no servers**. Everything on the Home list is something
you typed in or brought in, and it leaves the same three ways.

```
          ┌──────────── link ────────────┐
config ──▶├──────────── .nw file ───────┤──▶ another machine
          └──────────── QR code ─────────┘
```

Open them from **Home → ⇄ (share)** on a card, or **Home → ⋮ → Import /
export** for the whole library.

---

## 1. Links

| Scheme | Carries |
| --- | --- |
| `ssh://user:pass@host:port?mode=ssl&sni=…&payload=…#Name` | the SSH family |
| `vmess://`, `vless://`, `trojan://`, `ss://` | the usual V2Ray/Xray encodings |
| `networld://<base64>` | **lossless** — payload, SNI, expect-responses, UDPGW, split-tunnel hints, everything |

Standard schemes only carry what the scheme itself defines, so a payload or a
bug host can be lost on the way out. Toggle **Lossless networld:// link** in
the share sheet when the other end is also Networld.

Import accepts a whole block at once: paste any number of lines, or a base64
subscription body, and each line is parsed independently — bad lines are
reported, good ones are kept.

A `locked` config redacts its credentials when turned into a link.

## 2. Files (`.nw`)

A `.nw` bundle is a text blob beginning with `NW1:`:

```
NW1:plain:<base64 json>      # portable, readable
NW1:aes:<base64 payload>     # AES-256-GCM, key derived from your password
```

* **Share sheet → File** exports the single config you are looking at.
* **Import / export → Export all** bundles the whole library.
* Import detects `.nw` automatically — paste it, or pick the file.

Inside the packaged app the save/open dialogs are native (Tauri `dialog` + `fs`
plugins); in a browser the same buttons fall back to a normal download and file
picker, so the preview behaves identically.

## 3. QR codes

* **Export** — the share sheet renders the link as a QR and **Save PNG** writes
  it anywhere you like.
* **Import** — *Import / export → Scan QR* takes:
  * a dropped image file,
  * a click-to-browse pick,
  * or **Ctrl+V** of a screenshot straight from the clipboard.

Decoding runs locally (`jsqr`), with a second thresholded pass so photographs
of another screen still scan. Nothing is uploaded and no camera permission is
requested.

QR capacity is finite: links longer than ~2500 characters (big lossless
payloads) refuse to encode and the sheet tells you to share a file instead.

---

## Round trip, verified

The preview build is tested end to end: export a config as a QR, delete the
config, re-import the PNG — the config comes back with its name, mode, host,
SNI and payload intact.

## Where configs live

`%LOCALAPPDATA%\Networld VPN\config.json`, written atomically. Nothing is
synced anywhere; the only way a config leaves the machine is one of the three
above.

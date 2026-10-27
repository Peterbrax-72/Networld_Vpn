#!/usr/bin/env python3
"""Generate every Windows/MSIX icon asset from branding/networld-icon.png.

    python3 scripts/make-icons.py

Produces:
  src-tauri/icons/*            — icons used by the Tauri bundler
  packaging/msix/Assets/*      — the full MSIX tile/logo set (scale-100 + scale-200)
"""
from __future__ import annotations

import pathlib
from PIL import Image, ImageDraw

ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC = ROOT / "branding" / "networld-icon.png"
# Symbol-only variant (globe + shield, no wordmark). Used for every icon
# rendered below 64 px, where "NETWORLD VPN" would just be grey mush.
SYMBOL = ROOT / "branding" / "networld-symbol.png"
SMALL_CUTOFF = 64
TAURI = ROOT / "src-tauri" / "icons"
MSIX = ROOT / "packaging" / "msix" / "Assets"
BG = (11, 18, 32, 255)  # #0b1220 — matches the manifest BackgroundColor


def load_master() -> Image.Image:
    """Crop the artwork to the rounded square, knocking out a white margin if present."""
    img = Image.open(SRC).convert("RGBA")
    w, h = img.size
    px = img.load()

    corner = px[1, 1]
    on_white = corner[0] > 225 and corner[1] > 225 and corner[2] > 225
    if not on_white:
        # Artwork already bleeds to the edge — use it as-is.
        return img

    # 1. flood the outer white margin to transparent
    for seed in ((1, 1), (w - 2, 1), (1, h - 2), (w - 2, h - 2)):
        ImageDraw.floodfill(img, seed, (0, 0, 0, 0), thresh=40)

    # 2. drop any remaining near-white pixels (soft drop shadow)
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a and r > 232 and g > 232 and b > 232:
                px[x, y] = (r, g, b, 0)

    # 3. crop to what is left
    bbox = img.getbbox()
    return img.crop(bbox) if bbox else img


def pick(master: Image.Image, symbol: Image.Image | None, size: int) -> Image.Image:
    """Full lock-up when there is room for the wordmark, symbol when there is not."""
    return symbol if (symbol is not None and size < SMALL_CUTOFF) else master


def square(master: Image.Image, size: int, pad_ratio: float = 0.0, bg=None) -> Image.Image:
    canvas = Image.new("RGBA", (size, size), bg or (0, 0, 0, 0))
    inner = max(1, int(size * (1 - pad_ratio * 2)))
    art = master.resize((inner, inner), Image.LANCZOS)
    canvas.paste(art, ((size - inner) // 2, (size - inner) // 2), art)
    return canvas


def wide(master: Image.Image, w: int, h: int, bg=BG) -> Image.Image:
    canvas = Image.new("RGBA", (w, h), bg)
    inner = int(h * 0.74)
    art = master.resize((inner, inner), Image.LANCZOS)
    canvas.paste(art, ((w - inner) // 2, (h - inner) // 2), art)
    return canvas


def main() -> None:
    master = load_master()
    symbol = Image.open(SYMBOL).convert("RGBA") if SYMBOL.exists() else None
    TAURI.mkdir(parents=True, exist_ok=True)
    MSIX.mkdir(parents=True, exist_ok=True)

    # ---- Tauri bundler icons -------------------------------------------
    for name, size in {
        "32x32.png": 32,
        "128x128.png": 128,
        "128x128@2x.png": 256,
        "icon.png": 512,
        "Square30x30Logo.png": 30,
        "Square44x44Logo.png": 44,
        "Square71x71Logo.png": 71,
        "Square89x89Logo.png": 89,
        "Square107x107Logo.png": 107,
        "Square142x142Logo.png": 142,
        "Square150x150Logo.png": 150,
        "Square284x284Logo.png": 284,
        "Square310x310Logo.png": 310,
        "StoreLogo.png": 50,
    }.items():
        square(pick(master, symbol, size), size).save(TAURI / name)

    # .ico: small frames carry the symbol, large frames the full lock-up
    frames = [square(pick(master, symbol, s), s) for s in (16, 24, 32, 48)]
    big = square(master, 256)
    big.save(TAURI / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
             append_images=frames)

    # ---- MSIX asset set (scale-100 and scale-200) ----------------------
    tiles = {
        "Square44x44Logo": 44,
        "Square71x71Logo": 71,
        "Square150x150Logo": 150,
        "Square310x310Logo": 310,
        "StoreLogo": 50,
        "LockScreenLogo": 24,
    }
    for name, size in tiles.items():
        square(pick(master, symbol, size), size).save(MSIX / f"{name}.png")
        square(pick(master, symbol, size), size).save(MSIX / f"{name}.scale-100.png")
        square(pick(master, symbol, size * 2), size * 2).save(MSIX / f"{name}.scale-200.png")

    # Taskbar / start list target sizes, plated and unplated
    for ts in (16, 24, 32, 48, 256):
        art = square(pick(master, symbol, ts), ts)
        art.save(MSIX / f"Square44x44Logo.targetsize-{ts}.png")
        art.save(MSIX / f"Square44x44Logo.targetsize-{ts}_altform-unplated.png")
        art.save(MSIX / f"Square44x44Logo.altform-lightunplated_targetsize-{ts}.png")

    wide(master, 310, 150).save(MSIX / "Wide310x150Logo.png")
    wide(master, 310, 150).save(MSIX / "Wide310x150Logo.scale-100.png")
    wide(master, 620, 300).save(MSIX / "Wide310x150Logo.scale-200.png")
    wide(master, 620, 300).save(MSIX / "SplashScreen.png")
    wide(master, 620, 300).save(MSIX / "SplashScreen.scale-100.png")
    wide(master, 1240, 600).save(MSIX / "SplashScreen.scale-200.png")

    # Store listing art (1:1 and 16:9) — handy for Partner Center uploads
    square(master, 300, bg=BG).save(ROOT / "branding" / "store-icon-300.png")
    wide(master, 2400, 1200).save(ROOT / "branding" / "store-hero-2400x1200.png")

    print(f"tauri icons  → {TAURI}")
    print(f"msix assets  → {MSIX}")


if __name__ == "__main__":
    main()

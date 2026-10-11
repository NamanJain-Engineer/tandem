"""Draws Vibezeo Remote's icons (the desktop's two-ring mark) into
apps/mobile/assets: the 1024 px app icon (opaque, for iOS), the Android
adaptive icon's foreground (transparent, inside the 66% safe zone) and
monochrome layer, the notification icon (white on transparent) and the
splash mark. Run: python3 scripts/mobile/gen-icons.py"""
from pathlib import Path
from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parents[2] / "apps" / "mobile" / "assets"
BG = (11, 18, 32, 255)       # the desktop icon's background
TEAL = (45, 212, 191, 255)   # accent #2DD4BF (DECISIONS, dark theme)
WHITE = (255, 255, 255, 255)
SS = 4                       # supersampling for smooth edges


def rings(size, scale, color, bg=None):
    big = size * SS
    img = Image.new("RGBA", (big, big), bg or (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # The mark is 28 units wide: circles at x=10.5 and 17.5, r=7, stroke 2.2.
    unit = big * scale / 28.0
    cx, cy = big / 2, big / 2
    r, w = 7 * unit, 2.2 * unit
    for x in (10.5, 17.5):
        px = cx + (x - 14) * unit
        d.ellipse([px - r - w / 2, cy - r - w / 2, px + r + w / 2, cy + r + w / 2], outline=color, width=round(w))
    return img.resize((size, size), Image.LANCZOS)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    rings(1024, 0.7, TEAL, BG).save(OUT / "icon.png")
    rings(1024, 0.56, TEAL).save(OUT / "adaptive-icon.png")
    rings(1024, 0.56, WHITE).save(OUT / "adaptive-monochrome.png")
    rings(96, 0.9, WHITE).save(OUT / "notification-icon.png")
    rings(512, 0.9, TEAL).save(OUT / "splash-icon.png")


if __name__ == "__main__":
    main()

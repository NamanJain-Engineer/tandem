#!/usr/bin/env python3
"""Writes AVIF and WebP beside the site's real captures (D-1807).

The site serves each capture as <picture>: AVIF, then WebP, then the original
PNG or JPEG. Run after re-capturing (OPERATING §6):

    python3 apps/web/scripts/optimize-screens.py

Needs Pillow with AVIF and WebP support (Pillow 11.3 or later). Writes only
files that are missing or older than their source.
"""
import pathlib
import sys

from PIL import Image, features

ROOT = pathlib.Path(__file__).resolve().parent.parent / "public"
# The demo still, every PNG capture in public/screens, and the blog's images
# (public/blog/<slug>/*.png or *.jpg, F-WB-06).
SOURCES = [
    ROOT / "demo" / "still.jpg",
    *sorted((ROOT / "screens").glob("*.png")),
    *sorted(p for p in (ROOT / "blog").glob("*/*") if p.suffix in (".png", ".jpg", ".jpeg")),
]
QUALITY = {"avif": 60, "webp": 80}


def main() -> int:
    for fmt in QUALITY:
        if not features.check(fmt):
            print(f"Pillow here can't write {fmt}", file=sys.stderr)
            return 1
    for src in SOURCES:
        image = Image.open(src).convert("RGB")
        for fmt, quality in QUALITY.items():
            out = src.with_suffix(f".{fmt}")
            if out.exists() and out.stat().st_mtime >= src.stat().st_mtime:
                continue
            image.save(out, fmt.upper(), quality=quality, **({"method": 6} if fmt == "webp" else {"speed": 4}))
            print(f"{out.relative_to(ROOT)}: {out.stat().st_size // 1000} KB (from {src.stat().st_size // 1000} KB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

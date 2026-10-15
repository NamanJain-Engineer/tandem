#!/usr/bin/env python3
"""Writes AVIF and WebP beside the site's real captures (D-1807, D-1838).

The site serves each capture as <picture>: AVIF, then WebP, then the original
PNG or JPEG. Captures are taken at 2x and kept in apps/web/media as
`<name>@2x.png` (not served): from each, this writes the 1x original into
public/ and AVIF and WebP at 1x and 2x beside it. Run after re-capturing
(OPERATING §6):

    python3 apps/web/scripts/optimize-screens.py

Needs Pillow with AVIF and WebP support (Pillow 11.3 or later). Writes only
files that are missing or older than their source.
"""
import pathlib
import sys

from PIL import Image, features

ROOT = pathlib.Path(__file__).resolve().parent.parent / "public"
MEDIA = ROOT.parent / "media"
# 2x masters and where their 1x original goes. The demo still is a JPEG.
MASTERS = {"still": ROOT / "demo" / "still.jpg"}
# Smaller widths a phone picks instead (D-1842): the home page's still sits
# just under the fold, so its bytes count toward the largest paint.
NARROW = {"still": 800}
# The demo still, every PNG capture in public/screens, and the blog's images
# (public/blog/<slug>/*.png or *.jpg, F-WB-06).
SOURCES = [
    ROOT / "demo" / "still.jpg",
    *sorted((ROOT / "screens").glob("*.png")),
    *sorted(p for p in (ROOT / "blog").glob("*/*") if p.suffix in (".png", ".jpg", ".jpeg")),
]
QUALITY = {"avif": 60, "webp": 80}
BLOG_NARROW = 800


def fresh(out: pathlib.Path, src: pathlib.Path) -> bool:
    return out.exists() and out.stat().st_mtime >= src.stat().st_mtime


def save(image: Image.Image, out: pathlib.Path, fmt: str) -> None:
    quality = QUALITY[fmt]
    image.save(out, fmt.upper(), quality=quality, **({"method": 6} if fmt == "webp" else {"speed": 4}))
    print(f"{out.relative_to(ROOT)}: {out.stat().st_size // 1000} KB")


def masters() -> None:
    """From media/<name>@2x.png: the 1x original, then AVIF and WebP at 2x."""
    for master in sorted(MEDIA.glob("*@2x.png")):
        name = master.name.removesuffix("@2x.png")
        one = MASTERS.get(name, ROOT / "screens" / f"{name}.png")
        image = Image.open(master).convert("RGB")
        if not fresh(one, master):
            small = image.resize((image.width // 2, image.height // 2), Image.LANCZOS)
            if one.suffix == ".jpg":
                small.save(one, "JPEG", quality=88, optimize=True, progressive=True)
            else:
                small.save(one, "PNG", optimize=True)
            print(f"{one.relative_to(ROOT)}: {small.width}x{small.height}")
        for fmt in QUALITY:
            out = one.with_name(f"{one.stem}@2x.{fmt}")
            if not fresh(out, master):
                save(image, out, fmt)
            if name in NARROW:
                w = NARROW[name]
                out = one.with_name(f"{one.stem}-{w}.{fmt}")
                if not fresh(out, master):
                    save(image.resize((w, image.height * w // image.width), Image.LANCZOS), out, fmt)


def main() -> int:
    for fmt in QUALITY:
        if not features.check(fmt):
            print(f"Pillow here can't write {fmt}", file=sys.stderr)
            return 1
    masters()
    for src in SOURCES:
        image = Image.open(src).convert("RGB")
        for fmt in QUALITY:
            out = src.with_suffix(f".{fmt}")
            if not fresh(out, src):
                save(image, out, fmt)
            # A blog image wider than a phone also gets an 800 px copy for
            # phones (F-WB-06, D-1864), as the demo still does (D-1842).
            if "blog" in src.relative_to(ROOT).parts and image.width > BLOG_NARROW:
                out = src.with_name(f"{src.stem}-{BLOG_NARROW}.{fmt}")
                if not fresh(out, src):
                    save(image.resize((BLOG_NARROW, image.height * BLOG_NARROW // image.width), Image.LANCZOS), out, fmt)
    return 0


if __name__ == "__main__":
    sys.exit(main())

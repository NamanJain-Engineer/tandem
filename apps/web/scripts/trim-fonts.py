#!/usr/bin/env python3
"""Writes the site's fonts, trimmed to the weights it draws (D-1844).

The fontsource files carry every weight (Inter 100-900 with its optical-size
axis, 73 KB; JetBrains Mono 100-800, 40 KB). Both load before the home
page's largest paint, so their bytes count toward it. This keeps Inter's
optical sizes and the weights the site uses, and writes
app/fonts/inter-site.woff2 (about 52 KB) and app/fonts/mono-site.woff2
(about 29 KB). Run after updating @fontsource-variable/*:

    pip install fonttools brotli
    python3 apps/web/scripts/trim-fonts.py

app/frame.test.tsx fails if the site asks for a weight outside these ranges.
"""
import pathlib

from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

WEB = pathlib.Path(__file__).resolve().parent.parent
FILES = WEB / "node_modules" / "@fontsource-variable"
OUT = WEB / "app" / "fonts"
FONTS = [
    (FILES / "inter" / "files" / "inter-latin-opsz-normal.woff2", OUT / "inter-site.woff2", {"wght": (400, 650)}),
    (FILES / "jetbrains-mono" / "files" / "jetbrains-mono-latin-wght-normal.woff2", OUT / "mono-site.woff2", {"wght": (400, 500)}),
]

for src, out, limits in FONTS:
    font = instancer.instantiateVariableFont(TTFont(src), limits)
    font.flavor = "woff2"
    font.save(out)
    print(f"{out.relative_to(WEB)}: {out.stat().st_size // 1000} KB (from {src.stat().st_size // 1000} KB)")

#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["fonttools"]
# ///
"""Makes the terminal app's xiangqi pieces: the desktop app's (gui/public/vendor/game-pieces),
with the character as a glyph outline instead of text, as the terminal app draws SVGs without
fonts (resvg without text).

    uv run tools/xiangqi-outlines.py [path/to/NotoSerifCJK-Bold.ttc]

The outlines are Noto Serif CJK TC Bold's (SIL Open Font License 1.1, as the shogi tiles' Noto
Sans CJK outlines), the desktop app's first font for them. Each file keeps the source's
FNV-1a hash (without CRs) in its first comment, so a test can tell that it is current.
"""
import re, sys
from pathlib import Path
from fontTools.ttLib import TTCollection
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.boundsPen import BoundsPen

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "gui/public/vendor/game-pieces"
OUT = ROOT / "crates/coxswain/pieces"
FONT = sys.argv[1] if len(sys.argv) > 1 else "/usr/share/fonts/noto-cjk/NotoSerifCJK-Bold.ttc"


def fnv(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def font():
    for f in TTCollection(FONT).fonts:
        if "TC" in f["name"].getDebugName(1):
            return f
    sys.exit(f"{FONT}: no Noto Serif CJK TC in it")


def main():
    f = font()
    cmap, glyphs = f.getBestCmap(), f.getGlyphSet()
    upm = f["head"].unitsPerEm
    for svg in sorted(SRC.glob("xiangqi-*.svg")):
        raw = svg.read_bytes()
        text = raw.decode()
        m = re.search(r'<text [^>]*font-size="([\d.]+)"[^>]*fill="(#[0-9a-fA-F]+)"[^>]*>(.)</text>', text)
        size, fill, ch = float(m.group(1)), m.group(2), m.group(3)
        name = cmap[ord(ch)]
        pen, box = SVGPathPen(glyphs), BoundsPen(glyphs)
        glyphs[name].draw(pen)
        glyphs[name].draw(box)
        x0, y0, x1, y1 = box.bounds
        s = size / upm
        # The glyph's box centred on the disc, y up in the font.
        tx, ty = 22.5 - (x0 + x1) / 2 * s, 22.5 + (y0 + y1) / 2 * s
        path = f'<path transform="translate({tx:.3f} {ty:.3f}) scale({s:.5f} {-s:.5f})" fill="{fill}" d="{pen.getCommands()}"/>'
        out = text.replace(m.group(0), path)
        out = out.replace("<!--", f"<!-- Made by tools/xiangqi-outlines.py from {svg.name} ({fnv(raw.replace(b'\r', b'')):016x}): the character as Noto Serif CJK TC Bold's outline (SIL Open Font License 1.1).\n", 1)
        (OUT / svg.name).write_text(out)
        print(svg.name, ch)


main()

#!/usr/bin/env python3
"""Makes the terminal app's piece masks from the desktop app's SVG pieces.

    python3 tools/mosaic.py

Needs `resvg` (or `rsvg-convert`) and Pillow. Each set is one file under
crates/coxswain-core/src/mosaic/: an 8-byte FNV-1a hash of its SVG files, without CRs (so a test can tell
that the masks are current), then each piece as SIZE x SIZE bytes, row by row: the high four
bits how much of the dot the piece covers, the low four how much of it is dark.
A new set is one line in SETS.
"""
import shutil, subprocess, sys, tempfile
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SIZE = 64
SETS = {
    # name: (folder of SVG files, piece files in the order the code knows them)
    "chess": ("gui/public/vendor/chess-pieces", [f"{c}{p}.svg" for c in "wb" for p in "KQRBNP"]),
}


def fnv(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def render(svg: Path, png: Path):
    if shutil.which("resvg"):
        subprocess.run(["resvg", "-w", str(SIZE), "-h", str(SIZE), str(svg), str(png)], check=True)
    else:
        subprocess.run(["rsvg-convert", "-w", str(SIZE), "-h", str(SIZE), "-o", str(png), str(svg)], check=True)


def main():
    for name, (folder, files) in SETS.items():
        paths = [ROOT / folder / f for f in files]
        out = bytearray()
        sources = b"".join(p.read_bytes() for p in paths).replace(b"\r", b"")
        out += fnv(sources).to_bytes(8, "little")
        with tempfile.TemporaryDirectory() as tmp:
            for p in paths:
                png = Path(tmp) / (p.stem + ".png")
                render(p, png)
                img = Image.open(png).convert("RGBA")
                for y in range(SIZE):
                    for x in range(SIZE):
                        r, g, b, a = img.getpixel((x, y))
                        luma = (r * 299 + g * 587 + b * 114) / 255000
                        cover = round(a / 255 * 15)
                        dark = round(a / 255 * (1 - luma) * 15)
                        out.append(cover << 4 | dark)
        (ROOT / "crates/coxswain-core/src/mosaic" / f"{name}.bin").write_bytes(out)
        print(f"{name}: {len(paths)} pieces")


if __name__ == "__main__":
    sys.exit(main())

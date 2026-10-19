"""Old files for the demo home it runs in (Python's own library only): an Amiga IFF picture of
a checked ball, a comic made of the demo's pictures, a dBase table, a tracker module and a
mailbox. demo-home.sh runs it after the pictures are drawn."""
import math, struct, zipfile
from pathlib import Path


def chunk(cid, data):
    return cid + struct.pack(">I", len(data)) + data + (b"\0" if len(data) % 2 else b"")


def byterun1(row):
    """ByteRun1 (PackBits): runs of equal bytes, literals between them."""
    out, i = bytearray(), 0
    while i < len(row):
        j = i
        while j < len(row) and j - i < 128 and row[j] == row[i]:
            j += 1
        if j - i > 2:
            out += bytes([257 - (j - i), row[i]])
            i = j
            continue
        j = i
        while j < len(row) and j - i < 128 and not (j + 2 < len(row) and row[j] == row[j + 1] == row[j + 2]):
            j += 1
        out += bytes([j - i - 1]) + row[i:j]
        i = j
    return bytes(out)


def boing(path):
    w, h, planes = 320, 200, 5
    pal = [(170, 170, 170), (160, 0, 170), (230, 30, 30), (255, 255, 255), (102, 102, 102)] + [(0, 0, 0)] * 27
    px = [[0] * w for _ in range(h)]
    for y in range(h):
        for x in range(w):
            if x % 20 == 0 or (y % 20 == 0 and y < 180):
                px[y][x] = 1
    cx, cy, r = 150, 92, 70
    for y in range(h):
        for x in range(w):
            sx, sy = x - (cx + 18), y - (cy + 6)
            if sx * sx + sy * sy < r * r and px[y][x] != 3:
                px[y][x] = 4
    for y in range(h):
        for x in range(w):
            dx, dy = (x - cx) / r, (y - cy) / r
            d = dx * dx + dy * dy
            if d < 1:
                z = math.sqrt(1 - d)
                lon = math.atan2(dx, z) + 0.35
                lat = math.asin(max(-1, min(1, dy)))
                px[y][x] = 2 if (int((lon + 4) / (math.pi / 8)) + int((lat + 4) / (math.pi / 8))) % 2 else 3
    row_bytes = (w + 15) // 16 * 2
    body = bytearray()
    for y in range(h):
        row = bytearray()
        for p in range(planes):
            bits = bytearray(row_bytes)
            for x in range(w):
                if px[y][x] >> p & 1:
                    bits[x >> 3] |= 0x80 >> (x & 7)
            row += bits
        body += byterun1(bytes(row))
    bmhd = struct.pack(">HHhhBBBBHBBhh", w, h, 0, 0, planes, 0, 1, 0, 0, 10, 11, w, h)
    form = b"ILBM" + chunk(b"BMHD", bmhd) + chunk(b"CMAP", bytes(c for rgb in pal for c in rgb)) + chunk(b"CAMG", struct.pack(">I", 0)) + chunk(b"BODY", bytes(body))
    path.write_bytes(b"FORM" + struct.pack(">I", len(form)) + form)


def comic(path, pages):
    with zipfile.ZipFile(path, "w") as z:
        for i, p in enumerate(pages, 1):
            z.write(p, f"{i:02d}{p.suffix}")


def dbf(path):
    fields = [("NAME", "C", 12, 0), ("MASS", "N", 8, 2), ("LAUNCHED", "D", 8, 0), ("FLOWN", "L", 1, 0)]
    rows = [("Vostok 1", "4725.00", "19610412", "T"), ("Mercury", "1355.00", "19610505", "T"), ("Gemini", "3851.00", "19650323", "T"), ("Apollo 11", "45702.00", "19690716", "T")]
    rec = 1 + sum(f[2] for f in fields)
    start = 32 + 32 * len(fields) + 1
    out = bytearray(struct.pack("<B3BIHH", 0x03, 126, 10, 19, len(rows), start, rec) + bytes(17) + b"\x02" + bytes(2))
    for n, k, l, dec in fields:
        out += n.encode().ljust(11, b"\0") + k.encode() + bytes(4) + bytes([l, dec]) + bytes(14)
    out += b"\r"
    for r in rows:
        out += b" " + b"".join(v.encode().ljust(f[2]) if f[1] != "N" else v.encode().rjust(f[2]) for v, f in zip(r, fields))
    path.write_bytes(bytes(out) + b"\x1a")


def module(path):
    b = bytearray(1084 + 1024)
    b[:5] = b"Orbit"
    for i, n in enumerate(["kick", "snare", "bass", "pad", "greetings to the crew"]):
        o = 20 + i * 30
        b[o:o + len(n)] = n.encode()
        b[o + 22:o + 24] = struct.pack(">H", 64)
        b[o + 25] = 64
    b[950] = 1
    b[1080:1084] = b"M.K."
    path.write_bytes(bytes(b))


def mailbox(path):
    mails = [
        ("ada@example.com", "Ada Lovelace", "Mon, 28 Sep 2026 10:00:00 +0000", "Mon Sep 28 10:00:00 2026", "Go for launch", "Weather is fine; fuelling starts at 06:00."),
        ("bob@example.com", "Bob Shepard", "Tue, 29 Sep 2026 11:30:00 +0000", "Tue Sep 29 11:30:00 2026", "Flight 7 report", "Ascent nominal until T+92 s. The full report is in Documents."),
        ("ada@example.com", "Ada Lovelace", "Wed, 30 Sep 2026 09:15:00 +0000", "Wed Sep 30 09:15:00 2026", "Valve test", "The bench test is booked for Friday."),
    ]
    text = "".join(f"From {a} {d2}\nFrom: {n} <{a}>\nTo: Mission Control <control@example.com>\nDate: {d}\nSubject: {s}\n\n{body}\n\n" for a, n, d, d2, s, body in mails)
    path.write_text(text)


if __name__ == "__main__":
    home = Path(".")
    boing(home / "Pictures/boing.iff")
    pics = [home / "Pictures" / n for n in ("sunset.jpg", "lagoon.jpg", "glow.jpg")]
    if all(p.exists() for p in pics):
        comic(home / "Documents/rocket-comic.cbz", pics)
    dbf(home / "Documents/fleet.dbf")
    module(home / "Music/orbit.mod")
    mailbox(home / "Documents/launch.mbox")

"""The spike's test data from lichess's January 2013 games (CC0): full.pgn (121,332 games),
pgn-20k.pgn (its first 20,000), and scid-big.* (the SCID test base's 30 games, 4,000 times)."""
import os, shutil, sys
import zstandard

src, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
with open(src, "rb") as f, open(f"{out}/full.pgn", "wb") as o:
    zstandard.ZstdDecompressor().copy_stream(f, o)
b = open(f"{out}/full.pgn", "rb").read()
i = -1
for _ in range(20001):
    i = b.find(b"\n[Event ", i + 1)
open(f"{out}/pgn-20k.pgn", "wb").write(b[: i + 1])
scid = os.path.join(os.path.dirname(__file__), "../../crates/coxswain-core/src/chess/scid-test")
si = open(f"{scid}/test.si4", "rb").read()
head, ents = bytearray(si[:182]), si[182:]
n = len(ents) // 47
reps = 120000 // n
head[14:17] = (n * reps).to_bytes(3, "big")
open(f"{out}/scid-big.si4", "wb").write(bytes(head) + ents[: n * 47] * reps)
shutil.copy(f"{scid}/test.sn4", f"{out}/scid-big.sn4")
shutil.copy(f"{scid}/test.sg4", f"{out}/scid-big.sg4")

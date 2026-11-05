# /// script
# requires-python = ">=3.10"
# dependencies = ["websocket-client", "playwright"]
# ///
"""The terminal app's pictures as named scenes: a start (two folders), the keys sent, the picture.
After any change to the terminal screen, retake them all (a release build first, `nice cargo build
--release -p coxswain`; the sandbox runs target/release):

    uv run docs/screenshots/scenes.py                  # every scene
    uv run docs/screenshots/scenes.py copy.gif tui-search.png   # some of them
    uv run docs/screenshots/scenes.py --list

Pictures are made in a folder of their own (OUT, default <tmp>/scenes-out) so they can be looked
at against the old ones first; `--write` puts them into docs/screenshots/. Every scene runs
against the demo home (demo-home.sh) with its config.toml and state as the demo has them; they
are put back after each scene, and what a scene adds to the demo home is removed again.
Keys are tmux's names ("F5", "IC" = Insert, "M-b" = Alt+B, "C-F2", "S-F10", "Escape"); "type:abc"
types text, "wait:2" waits. Needs what cap.py needs.
"""

import os, shutil, sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cap
from cap import Terminal, gif

RIGHT = ["/home/demo/projects/rocket", "/home/demo"]
SCENES = {}


def scene(name, args=RIGHT, keys=(), size=(140, 36), config="", setup=None, record=None, guide=True, pre=()):
    """record=seconds makes a GIF (name ends .gif) while `keys` are sent."""
    SCENES[name] = dict(args=args, keys=keys, size=size, config=config, setup=setup, record=record, guide=guide, pre=pre)


def demo(*parts):
    return Path(os.environ.get("DEMO_HOME", cap.RUNTIME / "demo-home"), *parts)


def mk(*files, size=0):
    """Make files in the demo home; the scene's end removes them."""
    def run():
        for f in files:
            p = demo(f)
            p.parent.mkdir(parents=True, exist_ok=True)
            with p.open("wb") as o:
                o.truncate(size)
        return lambda: [demo(f).unlink(missing_ok=True) for f in files]
    return run


def mk_compare():
    """A backup of rocket that differs: src changed, notes.txt only here, the rest left out."""
    def run():
        d = demo("Backups", "rocket")
        try:
            (d / "src").mkdir(parents=True)
            (d / "Cargo.toml").write_text(demo("projects", "rocket", "Cargo.toml").read_text())
            (d / "notes.txt").write_text("remember the fuel\n")
            (d / "src" / "main.rs").write_text("fn main() {}\n")
        except BaseException:
            shutil.rmtree(d, ignore_errors=True)       # a half-made setup is undone here
            raise
        return lambda: shutil.rmtree(d, ignore_errors=True)
    return run


def mk_diverged():
    """origin/master has a commit rocket's master lacks, and master has one origin lacks."""
    import subprocess
    def run():
        repo = str(demo("projects", "rocket"))
        env = dict(os.environ, GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1", GIT_AUTHOR_NAME="Ada", GIT_AUTHOR_EMAIL="ada@example.com",
                   GIT_COMMITTER_NAME="Ada", GIT_COMMITTER_EMAIL="ada@example.com")
        git = lambda *a: subprocess.run(["git", "-C", repo, *a], env=env, capture_output=True, text=True, check=True).stdout.strip()
        bare = str(demo(".remotes", "rocket.git"))
        old = git("rev-parse", "origin/master")
        url = git("remote", "get-url", "origin")
        new = git("commit-tree", "origin/master^{tree}", "-p", old, "-m", "Tune the valve")
        git("push", bare, f"{new}:refs/heads/master")          # the remote moved on; this clone has not looked yet
        git("remote", "set-url", "origin", "/home/demo/.remotes/rocket.git")   # a path, so no network
        def undo():
            git("remote", "set-url", "origin", url)
            git("--git-dir", bare, "update-ref", "refs/heads/master", old)
            git("update-ref", "refs/remotes/origin/master", old)
        return undo
    return run


def keep(*folders):
    """Whatever a scene adds to these demo folders (a copy) is removed again at its end."""
    def run():
        before = {f: set(demo(f).iterdir()) for f in folders}
        def undo():
            for f in folders:
                for p in set(demo(f).iterdir()) - before[f]:
                    shutil.rmtree(p) if p.is_dir() else p.unlink()
        return undo
    return run


def mk_logs():
    def run():
        d = demo("logs")
        try:
            (d / "old").mkdir(parents=True)
            line = "2026-10-22T22:49:01Z info engine ignition sequence nominal thrust 98.7\n"
            for f in ("engine.log", "old/engine-1.log"):
                (d / f).write_text(line * 40000)
        except BaseException:
            shutil.rmtree(d, ignore_errors=True)
            raise
        return lambda: shutil.rmtree(d, ignore_errors=True)
    return run


def mk_big():
    """Big: a folder of small files and a 16 GB disk image (sparse: it takes no room, and a copy
    of it skips its zeros), so a copy of both runs long enough to be seen. What the copy puts into
    Backups goes again."""
    def run():
        d = demo("Big")
        shutil.rmtree(d, ignore_errors=True)          # a run stopped before its undo
        backups = keep("Backups")()
        try:
            (d / "tree").mkdir(parents=True)
            for i in range(40):
                (d / "tree" / f"leaf-{i:02}.txt").write_bytes(b"leaf\n" * 2800)
            with (d / "disk.img").open("wb") as f:
                f.truncate(16 << 30)
        except BaseException:
            shutil.rmtree(d, ignore_errors=True)
            raise
        return lambda: (shutil.rmtree(d, ignore_errors=True), backups())
    return run


# The EICAR test file: no virus, but every scanner reports it as one (in two parts, so this
# file is not taken for it).
EICAR = "X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR" + "-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*"
# Containers never run in the sandbox: for the virus scan a stand-in podman, bound over the
# real one for that scene only, runs the clamscan installed here instead, on a database of
# EICAR alone that it makes itself, and answers the questions the app asks podman first.
PODMAN = r'''#!/usr/bin/python3
import hashlib, os, subprocess, sys, tempfile
a = sys.argv[1:]
if a[:2] == ["image", "inspect"]:
    sys.exit(print(312000000))
if a[:1] == ["info"]:
    sys.exit(print("true"))
if a[:1] != ["run"] or "clamscan" not in a:
    sys.exit(0)
at = {}
for i, x in enumerate(a[:-1]):
    if x == "--mount":
        m = dict(kv.split("=", 1) for kv in a[i + 1].replace('"', "").split(",") if "=" in kv)
        at[m["target"]] = m["source"]
eicar = "EICAR_TEXT".encode()
tmp = tempfile.mkdtemp()
open(f"{tmp}/eicar.hdb", "w").write(f"{hashlib.md5(eicar).hexdigest()}:{len(eicar)}:Eicar-Test-Signature\n")
src = at["/src"].rstrip("/") + "/"
names = open(at["/list"] + "/files").read().splitlines()
open(f"{tmp}/files", "w").write("".join(src + n[5:] + "\n" for n in names if n.startswith("/src/")))
r = subprocess.run(["clamscan", f"--database={tmp}/eicar.hdb", f"--file-list={tmp}/files", "--no-summary", "--stdout"], capture_output=True, text=True)
sys.stderr.write(r.stderr)
for line in r.stdout.splitlines():
    print("/src/" + line[len(src):].replace(".UNOFFICIAL", "") if line.startswith(src) else line, flush=True)
sys.exit(r.returncode)
'''.replace("EICAR_TEXT", EICAR.replace("\\", "\\\\"))


def mk_viruses():
    """The EICAR file and a zip holding it in Downloads, a virus database said to be fresh, and
    the stand-in podman."""
    import time, zipfile
    def run():
        dl, db = demo("Downloads"), demo(".cache", "coxswain", "clamav")
        shim = cap.RUNTIME / "podman-stand-in"
        old_args = os.environ.get("SANDBOX_ARGS")
        def undo():
            for f in ("eicar.com", "test-files.zip"):
                (dl / f).unlink(missing_ok=True)
            shutil.rmtree(db, ignore_errors=True)
            shim.unlink(missing_ok=True)
            if old_args is None:
                os.environ.pop("SANDBOX_ARGS", None)
            else:
                os.environ["SANDBOX_ARGS"] = old_args
        try:
            assert shutil.which("clamscan") and shutil.which("podman"), "needs clamscan and podman installed here"
            (dl / "eicar.com").write_text(EICAR)
            with zipfile.ZipFile(dl / "test-files.zip", "w") as z:
                z.writestr("test-files/eicar.com", EICAR)
                z.writestr("test-files/readme.txt", "Test files for the virus scanner.\n")
            db.mkdir(parents=True, exist_ok=True)
            (db / "main.cvd").write_bytes(b"")
            (db / "updated").write_bytes(b"")
            os.utime(db / "updated", (time.time() - 3 * 3600,) * 2)
            shim.write_text(PODMAN)
            shim.chmod(0o755)
            real = os.path.realpath(shutil.which("podman") or "/usr/bin/podman")
            os.environ["SANDBOX_ARGS"] = f"{old_args or ''} --ro-bind {shim} {real}"
        except BaseException:
            undo()
            raise
        return undo
    return run



# A games database for the opening tree (docs/previews/chess-positions.md): openings played on to
# move 7 to 14, by made-up players, made the same each time.
LINES = [
    (9, "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be6"),
    (6, "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 g6 Be3 Bg7 f3 O-O"),
    (5, "e4 c5 Nf3 e6 d4 cxd4 Nxd4 Nf6 Nc3 d6 Be2 Be7 O-O O-O"),
    (4, "e4 c5 Nf3 e6 d4 cxd4 Nxd4 Nc6 Nc3 Qc7 Be2 a6"),
    (4, "e4 c5 Nf3 Nc6 d4 cxd4 Nxd4 Nf6 Nc3 e5 Ndb5 d6 Bg5 a6"),
    (3, "e4 c5 c3 Nf6 e5 Nd5 d4 cxd4 Nf3 Nc6"),
    (2, "e4 c5 Nc3 Nc6 g3 g6 Bg2 Bg7 d3 d6"),
    (3, "e4 c5 Nf3 Nc6 Bb5 g6 O-O Bg7 Re1 Nf6"),
    (2, "e4 c5 Nf3 d6 Bb5+ Bd7 Bxd7+ Qxd7 c4 Nc6"),
    (2, "e4 c5 Nf3 e6 d4 cxd4 Nxd4 a6 Bd3 Nf6 O-O d6"),
    (1, "e4 c5 d4 cxd4 c3 dxc3 Nxc3 Nc6 Nf3 d6"),
    (6, "e4 e5 Nf3 Nc6 Bb5 a6 Ba4 Nf6 O-O Be7 Re1 b5 Bb3 d6"),
    (4, "e4 e5 Nf3 Nc6 Bc4 Bc5 c3 Nf6 d3 d6 O-O O-O"),
    (4, "e4 e6 d4 d5 Nc3 Bb4 e5 c5 a3 Bxc3+ bxc3 Ne7"),
    (3, "e4 c6 d4 d5 Nc3 dxe4 Nxe4 Bf5 Ng3 Bg6 h4 h6"),
    (5, "d4 d5 c4 e6 Nc3 Nf6 Bg5 Be7 e3 O-O Nf3 h6"),
    (4, "d4 d5 c4 c6 Nf3 Nf6 Nc3 dxc4 a4 Bf5"),
    (4, "d4 Nf6 c4 g6 Nc3 Bg7 e4 d6 Nf3 O-O Be2 e5"),
    (4, "d4 Nf6 c4 e6 Nc3 Bb4 Qc2 O-O a3 Bxc3+ Qxc3 b6"),
    (3, "c4 e5 Nc3 Nf6 Nf3 Nc6 g3 Bb4 Bg2 O-O"),
    (2, "Nf3 d5 g3 Nf6 Bg2 c6 O-O Bg4 d3 Nbd7"),
]
PLAYERS = ["Aaltonen, Mikko", "Berglund, Ida", "Castell, Marc", "Dvořák, Jan", "Eriksen, Sofie", "Fontaine, Luc",
           "Galdós, Ana", "Halvorsen, Erik", "Ilić, Petar", "Jansen, Bram", "Kowalczyk, Ewa", "Lindqvist, Nils",
           "Moreau, Claire", "Novak, Tomas", "Okafor, Daniel", "Petrova, Maria", "Quint, Oskar", "Rossi, Giulia",
           "Santos, Rui", "Takács, Bence", "Ueda, Ren", "Varga, Lili", "Weiss, Jonas", "Yilmaz, Deniz"]


def club_pgn(n=4000):
    """The games' text: chosen by a fixed sequence, so every run makes the same file."""
    seed = 7
    def rnd(k):
        nonlocal seed
        seed = (seed * 6364136223846793005 + 1442695040888963407) % (1 << 64)
        return (seed >> 33) % k
    pool = [line for w, line in LINES for _ in range(w)]
    out = []
    for i in range(n):
        moves = pool[rnd(len(pool))].split()
        moves = moves[: max(8, len(moves) - rnd(4))]
        w, b = rnd(len(PLAYERS)), rnd(len(PLAYERS))
        if w == b:
            b = (b + 1) % len(PLAYERS)
        welo, belo = 1700 + rnd(30) * 25 + w * 5, 1700 + rnd(30) * 25 + b * 5
        result = ["1-0", "1/2-1/2", "0-1", "1-0", "1/2-1/2"][rnd(5)]
        text = " ".join((f"{k // 2 + 1}. {m}" if k % 2 == 0 else m) for k, m in enumerate(moves))
        out.append(f'[Event "Club championship"]\n[Site "Riverside"]\n[Date "{1996 + rnd(30)}.{1 + rnd(12):02}.{1 + rnd(28):02}"]\n'
                   f'[Round "{1 + rnd(9)}"]\n[White "{PLAYERS[w]}"]\n[Black "{PLAYERS[b]}"]\n[Result "{result}"]\n'
                   f'[WhiteElo "{welo}"]\n[BlackElo "{belo}"]\n\n{text} {result}\n')
    return "\n".join(out)


def mk_club():
    """Documents/Chess/tournaments.pgn, and what its index and table leave in the cache, gone again."""
    def run():
        p = demo("Documents", "Chess", "tournaments.pgn")
        p.write_text(club_pgn())
        cache = demo(".cache", "coxswain")
        before = {q for d in ("positions", "chess/tables") for q in (cache / d).glob("*")}
        def undo():
            p.unlink(missing_ok=True)
            for d in ("positions", "chess/tables"):
                for q in (cache / d).glob("*"):
                    if q not in before:
                        shutil.rmtree(q, ignore_errors=True)
        return undo
    return run

START = []                         # the first-run guide is closed before the keys (guide=False: not)
ROCKET_README = START + ["Down"] * 8

# ---- the panels and what opens over them
scene("action-menu-hint-tui.png", keys=ROCKET_README + ["wait:4"])
scene("action-menu-tui.png", keys=ROCKET_README + ["S-F10", "Down*60"])
scene("checksums-make-tui.png", args=["/home/demo/Downloads/rocket-1.4.0", "/home/demo/Downloads"],
      keys=START + ["Down", "IC", "IC", "IC", "M-h"])
scene("commands-scripts-tui.png", args=["/home/demo/Pictures", "/home/demo/Backups"],
      keys=START + ["Down"] * 4 + ["IC", "IC", "IC", "F2"])
scene("commands-user-menu-tui.png", args=RIGHT, keys=START + ["Down"] * 6 + ["F2"])
scene("compare-folders-tui.png", args=["/home/demo/Backups/rocket", "/home/demo/projects/rocket"],
      keys=START + ["C-F2", "wait:3"], setup=mk_compare())
scene("customise-glyphs.png", args=["/home/demo/projects/rocket", "/home/demo/projects/website"],
      keys=START, config='glyphs = "ascii"\n')
scene("join-tui.png", args=["/home/demo/Downloads", "/home/demo/Downloads"],
      keys=START + ["Down", "Down", "Enter"], setup=mk("Downloads/big.bin.001", "Downloads/big.bin.002", "Downloads/big.bin.003", "Downloads/big.bin.sha256", size=1000))
scene("opening-enter-tui.png", args=["/home/demo/Backups/old-laptop-2019", "/home/demo"],
      keys=["wait:3", "Down", "Down", "Enter", "wait:3", "Down", "Enter", "wait:3", "Down*5", "wait:2"])
scene("panels-features-tui.png", keys=START + ["F1", "Tab", "type:archive"])
scene("panels-help.png", keys=START + ["F1"] + ["PageDown"] * 7)
scene("panels-quick-search.png", args=["/home/demo/projects/rocket", "/home/demo/Documents"],
      keys=START + ["Tab", "M-e", "l", "e"])
scene("tui-clouds.png", args=["/home/demo", "/home/demo/projects"], keys=START + ["M-F1"])
scene("tui-config-mistakes.png", args=["/home/demo", "/home/demo"], keys=["wait:1"], guide=False,
      config='# Coxswain\ntheme = 3\n# 7 lines\ncheck_updates = false\n\n[search]\nmax_results = "many"\n')
scene("tui-search.png", keys=START + ["M-F7", "type:*.rs|*.toml src/", "wait:3"])
scene("tui-text-search.png", keys=START + ["M-F7", "type:engine", "Tab", "wait:3"])

scene("tui-git-facts.png", args=["/home/demo/projects", "/home/demo/projects/rocket"], size=(200, 18),
      keys=["Down*5"])

scene("tui-fetch-pull.png", args=["/home/demo/projects/rocket", "/home/demo/projects/website"], keys=["M-d", "wait:2"], setup=mk_diverged())
scene("tui-long-files-menu.png", args=["/home/demo", "/home/demo/logs"], setup=mk_logs(),
      keys=["Down*13", "S-F10", "Down*60", "Up*2", "Enter"])

# ---- other version controls (demo-home.sh makes kite, glider and lander when cvs, hg and jj are installed)
scene("cvs-line.png", args=["/home/demo/projects/kite", "/home/demo/projects/rocket"], keys=START + ["Down*2"])
scene("tui-cvs-ask.png", args=["/home/demo/projects/kite", "/home/demo/projects/rocket"], keys=START + ["C-g"])
scene("tui-cvs-history.png", args=["/home/demo/projects/kite", "/home/demo/projects/rocket"], keys=START + ["C-g", "Enter", "wait:3"])
scene("tui-cvs-commit.png", args=["/home/demo/projects/kite", "/home/demo"], keys=START + ["C-g", "Enter", "wait:3", "Down*2", "Enter", "wait:2"])
scene("tui-hg-line.png", args=["/home/demo/projects/glider", "/home/demo/projects/rocket"], keys=START + ["Down*3", "wait:2"])
scene("tui-jj-line.png", args=["/home/demo/projects/lander", "/home/demo/projects/rocket"], keys=START + ["Down*3", "wait:2"])

# ---- files
scene("tui-virus-scan.png", args=["/home/demo/Downloads", "/home/demo/Documents"], setup=mk_viruses(),
      config="preview.virus_scan = true\npreview.container = \"podman\"\n", keys=START + ["Home", "M-i", "wait:2", "Enter", "wait:4", "q"])
scene("tui-copy-stop.png", args=["/home/demo/Big", "/home/demo/Backups"], setup=mk_big(),
      keys=START + ["Down", "IC", "IC", "F5", "Enter", "wait:2"])

# ---- pick mode (coxswain --pick): the key line shows the picker's keys, other files are dimmed
scene("pick-tui.png", args=["--pick", "--multiple", "--filter", "rs,toml", "--start", "/home/demo/projects/rocket"], guide=False, keys=["Down*6", "IC"])

# ---- chess: the opening tree of a database with its position index (i builds it, t shows the tree)
scene("tui-chess-tree.png", args=["/home/demo/Documents/Chess", "/home/demo"], setup=mk_club(), size=(140, 36),
      keys=["type:tournaments", "Escape", "F3", "wait:3", "i", "wait:5", "Enter", "wait:2", "Right", "Right", "t", "Down", "wait:2"])

# ---- GIFs: the first frame is saved beside as <name>.png
scene("copy.gif", args=["/home/demo/Documents", "/home/demo/Backups"], record=5.5, setup=keep("Backups"),
      pre=["Down*12"], keys=["wait:0.5", "IC", "IC", "IC", "wait:0.5", "F5", "wait:1", "Enter", "wait:1.5"], size=(100, 22))
scene("branches.gif", args=["/home/demo/projects/rocket", "/home/demo/projects/website"], record=6.5,
      keys=["wait:0.5", "M-b", "Down", "wait:0.5", "Enter", "wait:0.5", "Down", "Enter", "wait:1"], size=(100, 19))


def play(t, keys, pause=1.0):
    for k in keys:
        if k.startswith("wait:"):
            t.wait(float(k[5:]))
        elif k.startswith("type:"):
            t.type(k[5:])
        elif "*" in k:                       # "Down*40": a key many times
            key, n = k.split("*")
            t.keys(*[key] * int(n), pause=0.4)    # one by one: the app drops a burst
            t.wait(pause)
        else:
            t.keys(k, pause=pause)


def run(name, out):
    s = SCENES[name]
    cfg = demo(".config", "coxswain", "config.toml")
    saved = {p: (p.read_bytes() if p.exists() else None) for p in (cfg, *demo(".local").rglob("*.json"))}
    undo = None
    try:
        undo = s["setup"]() if s["setup"] else None
        if s["config"]:
            # Top-level keys go first: the demo's config ends in a [search] table. A text with its
            # own [search] table replaces the demo's.
            old = cfg.read_text() if cfg.exists() else ""
            cfg.write_text(s["config"] + ("" if "[search]" in s["config"] else "\n" + old))
        with Terminal(args=s["args"], cols=s["size"][0], rows=s["size"][1]) as t:
            t.wait(4)
            if s["guide"]:
                t.keys("Escape", pause=2)
            if s["record"]:
                play(t, s["pre"])          # to the start of the GIF, not recorded
                frames = t.record(s["record"], during=lambda: play(t, s["keys"], 0.5))
                gif(frames, out / name)
            else:
                play(t, s["keys"])
                t.wait(1.5)
                t.shot(out / name)
    finally:
        for p, b in saved.items():
            p.write_bytes(b) if b is not None else p.unlink(missing_ok=True)
        for p in demo(".local").rglob("*.json"):
            if p not in saved:
                p.unlink()
        if undo:
            undo()


if __name__ == "__main__":
    args = sys.argv[1:]
    if "--list" in args:
        print("\n".join(SCENES))
        sys.exit()
    write = "--write" in args
    names = [a for a in args if not a.startswith("--")] or list(SCENES)
    out = Path(os.environ.get("OUT", cap.RUNTIME / "scenes-out"))
    out.mkdir(parents=True, exist_ok=True)
    unknown = [n for n in names if n not in SCENES]
    if unknown:
        sys.exit(f"no such scene: {', '.join(unknown)} (see --list)")
    failed = []
    for n in names:
        print(n, flush=True)
        try:
            run(n, out)
            if write:
                for f in (n, n.replace(".gif", ".png")):
                    shutil.copy(out / f, Path(__file__).parent / f)
        except Exception as e:                  # the rest still run
            failed.append(n)
            print(f"FAILED {n}: {e!r}", file=sys.stderr, flush=True)
    if failed:
        sys.exit(f"failed: {', '.join(failed)}")

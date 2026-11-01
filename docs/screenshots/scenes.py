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
        (d / "src").mkdir(parents=True)
        (d / "Cargo.toml").write_text(demo("projects", "rocket", "Cargo.toml").read_text())
        (d / "notes.txt").write_text("remember the fuel\n")
        (d / "src" / "main.rs").write_text("fn main() {}\n")
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
        (d / "old").mkdir(parents=True)
        line = "2026-10-22T22:49:01Z info engine ignition sequence nominal thrust 98.7\n"
        for f in ("engine.log", "old/engine-1.log"):
            (d / f).write_text(line * 40000)
        return lambda: shutil.rmtree(d, ignore_errors=True)
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
    undo = s["setup"]() if s["setup"] else None
    if s["config"]:
        # Top-level keys go first: the demo's config ends in a [search] table. A text with its own
        # [search] table replaces the demo's.
        old = cfg.read_text() if cfg.exists() else ""
        cfg.write_text(s["config"] + ("" if "[search]" in s["config"] else "\n" + old))
    try:
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
    for n in names:
        print(n, flush=True)
        run(n, out)
        if write:
            for f in (n, n.replace(".gif", ".png")):
                shutil.copy(out / f, Path(__file__).parent / f)

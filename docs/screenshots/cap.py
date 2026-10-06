# /// script
# requires-python = ">=3.10"
# dependencies = ["websocket-client", "playwright"]
# ///
"""Pictures and GIFs of both apps in the demo sandbox (sandbox.sh), on displays of their own:
nothing appears on the screen, and no key reaches another window.

- The desktop app runs on GTK's Broadway display, shown by a headless Chromium. Keys go to that
  page through the DevTools protocol (Input.dispatchKeyEvent), pictures come from it
  (Page.captureScreenshot), clipped to the window.
- The terminal app runs in a tmux of its own (its own socket), 140×36; keys by `send-keys`,
  pictures from `capture-pane`, drawn in MesloLGM Nerd Font Mono by Chromium.

A GIF is frames taken ~12 times a second, then ffmpeg with palettegen/paletteuse.

    from cap import Desktop, Terminal, gif
    with Desktop(args=["/home/demo/Downloads"]) as d:   # or args=["--settings=search"]
        d.keys("End", "Enter"); d.shot("inside.png")      # d.app(js) for what has no key
        frames = d.record(6, during=lambda: (d.keys("Ctrl+f"), d.type("engine", 0.15)))
    gif(frames, "find.gif")                              # and find.png, its first frame
    with Terminal(args=["/home/demo/Documents", "/home/demo/Backups"]) as t:
        t.keys("IC", "F5"); t.shot("copy.png")           # tmux key names

The demo home is $DEMO_HOME (default <tmp>/coxswain-cap-<uid>/demo-home, made with
demo-home.sh). `SANDBOX_ARGS` (extra bwrap arguments, e.g. `--ro-bind hosts /etc/hosts`) is
passed to sandbox.sh; keep the network, the page is reached over 127.0.0.1. Run with
`uv run --with websocket-client --with playwright python my-script.py` from this folder.
Linux only; needs bwrap, broadwayd, chromium, tmux and ffmpeg.
"""

import base64, json, os, shutil, signal, subprocess, tempfile, time, urllib.request
from pathlib import Path

import websocket

HERE = Path(__file__).resolve().parent
SANDBOX = HERE / "sandbox.sh"
RUNTIME = Path(tempfile.gettempdir()) / f"coxswain-cap-{os.getuid()}"


def _env(**extra):
    RUNTIME.mkdir(mode=0o700, exist_ok=True)
    env = {k: v for k, v in os.environ.items() if k in ("PATH", "LANG", "DEMO_HOME", "SANDBOX_ARGS")}
    # A runtime folder of its own: no Wayland, no session bus, no systemd of the user's reachable.
    env.update(XDG_RUNTIME_DIR=str(RUNTIME), WAYLAND_DISPLAY="", DISPLAY="", **extra)
    env.setdefault("DEMO_HOME", str(RUNTIME / "demo-home"))
    # Its own process namespace: when the sandbox stops, so does all it started (the search helper too).
    env["SANDBOX_ARGS"] = "--unshare-pid --die-with-parent " + env.get("SANDBOX_ARGS", "")
    return env


def _stop(p):
    """Stop a process started here, and everything it started (its own process group)."""
    if p and p.poll() is None:
        try:
            os.killpg(p.pid, signal.SIGTERM)
            p.wait(5)
        except (ProcessLookupError, subprocess.TimeoutExpired):
            os.killpg(p.pid, signal.SIGKILL)


# Keys by name, as the DevTools protocol wants them: (key, code, windowsVirtualKeyCode).
NAMED = {"Enter": ("Enter", "Enter", 13), "Esc": ("Escape", "Escape", 27), "Tab": ("Tab", "Tab", 9),
         "Backspace": ("Backspace", "Backspace", 8), "Up": ("ArrowUp", "ArrowUp", 38), "Down": ("ArrowDown", "ArrowDown", 40),
         "Left": ("ArrowLeft", "ArrowLeft", 37), "Right": ("ArrowRight", "ArrowRight", 39), "Home": ("Home", "Home", 36),
         "End": ("End", "End", 35), "PageDown": ("PageDown", "PageDown", 34), "PageUp": ("PageUp", "PageUp", 33),
         "Insert": ("Insert", "Insert", 45), "Delete": ("Delete", "Delete", 46), "Space": (" ", "Space", 32)}
NAMED.update({f"F{i}": (f"F{i}", f"F{i}", 111 + i) for i in range(1, 13)})
MODS = {"Alt": 1, "Ctrl": 2, "Meta": 4, "Shift": 8}


# Broadway has no screen resolution (-1), which WebKitGTK turns into a page zoom of -1/96: the
# page lays out at a negative size. The app is started with this (and only the app: WebKit's own
# processes start without it).
DPI = r"""
#include <stdlib.h>
double gdk_screen_get_resolution(void *screen) { (void)screen; return 96.0; }
__attribute__((constructor)) static void only_here(void) { unsetenv("LD_PRELOAD"); }
"""


def _dpi():
    so = RUNTIME / "dpi96.so"
    if not so.exists():
        subprocess.run(["cc", "-shared", "-fPIC", "-O2", "-x", "c", "-", "-o", str(so)], input=DPI, text=True, check=True)
    return so


class Desktop:
    def __init__(self, args=(), lang="en_GB.UTF-8", size=(1440, 900), port=8123, display=":23", debug=9233, inspector=9312):
        self.args, self.lang, self.size, self.port, self.display, self.debug, self.inspector = list(args), lang, size, port, display, debug, inspector
        self.procs, self.n, self.ws, self.wk, self.target = [], 0, None, None, None

    def __enter__(self):
        # Now and then the first window never reaches the display: start again.
        for attempt in range(3):
            self._start()
            try:
                return self.ready()
            except TimeoutError:
                if attempt == 2:
                    raise
                self.__exit__()
                self.procs, self.wk, self.target = [], None, None

    def _start(self):
        env = _env()
        _dpi()
        # broadwayd in the sandbox too: GTK hands it the pictures through /dev/shm, which is the
        # sandbox's own. The app waits for the page, so the display has its size first.
        go = Path(env["DEMO_HOME"]) / ".cap-go"
        go.unlink(missing_ok=True)
        run = (f"broadwayd --address 127.0.0.1 --port {self.port} {self.display} >/dev/null 2>&1 & until [ -e /home/demo/.cap-go ]; do sleep 0.2; done; rm /home/demo/.cap-go; "
               f'exec env LD_PRELOAD={RUNTIME}/dpi96.so GDK_BACKEND=broadway BROADWAY_DISPLAY={self.display} WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:{self.inspector} LANG={self.lang} coxswain-gui "$@"')
        self.procs.append(subprocess.Popen([str(SANDBOX), "sh", "-c", run, "sh", *self.args], env=env, stdout=open(RUNTIME / "gui.log", "w"), stderr=subprocess.STDOUT, start_new_session=True))
        for _ in range(100):
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{self.port}/", timeout=1)
                break
            except OSError:
                time.sleep(0.2)
        # The page first, so the display has its size before the window opens.
        w, h = self.size
        self.profile = tempfile.mkdtemp(prefix="cap-chromium-")
        self.procs.append(subprocess.Popen(["chromium", "--headless=new", "--no-sandbox", "--hide-scrollbars", f"--user-data-dir={self.profile}", f"--window-size={w + 400},{h + 450}",
                                            f"--remote-debugging-port={self.debug}", f"http://127.0.0.1:{self.port}/"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True))
        for _ in range(50):
            try:
                tabs = json.load(urllib.request.urlopen(f"http://127.0.0.1:{self.debug}/json"))
                self.ws = websocket.create_connection([t for t in tabs if t["type"] == "page"][0]["webSocketDebuggerUrl"], suppress_origin=True)
                break
            except (OSError, IndexError):
                time.sleep(0.2)
        time.sleep(1.5)
        go.touch()

    def __exit__(self, *_):
        if self.wk:
            self.wk.close()
        for p in reversed(self.procs):
            _stop(p)
        shutil.rmtree(self.profile, ignore_errors=True)

    def call(self, method, **params):
        self.n += 1
        self.ws.send(json.dumps({"id": self.n, "method": method, "params": params}))
        while True:
            r = json.loads(self.ws.recv())
            if r.get("id") == self.n:
                return r.get("result", r)

    def wait(self, secs):
        time.sleep(secs)

    def ready(self, timeout=60):
        """Wait until the window is drawn: a canvas of the window's size with something on it."""
        probe = "(()=>{const c=[...document.querySelectorAll('canvas')].find(c=>c.width>600);if(!c)return 0;const d=c.getContext('2d').getImageData(0,c.height/2,c.width,1).data;let n=0;for(let i=0;i<d.length;i+=4)if(d[i+3])n++;return n})()"
        end = time.time() + timeout
        while time.time() < end:
            if self.call("Runtime.evaluate", expression=probe, returnByValue=True)["result"].get("value", 0) > 600:
                time.sleep(2)
                # Focus: a click on the title bar, so keys reach the window.
                x, y, w, _ = self.frame()
                self.click(x + w * 0.3, y + 18)
                time.sleep(0.5)
                return self
            # A window can wait for an event before its first picture: a click on its title bar.
            if time.time() > end - timeout + 8:
                x, y, w, _ = self.frame()
                self.click(x + w * 0.3, y + 18)
            time.sleep(1)
        Path(RUNTIME / "not-drawn.png").write_bytes(self.png([0, 0, *self.size]))
        raise TimeoutError(f"the window was not drawn: see {RUNTIME}/not-drawn.png and gui.log")

    def app(self, expr, timeout=30):
        """Evaluate in the app's own page, through WebKit's inspector: for scrolling and clicking
        what has no key. A promise is waited for."""
        if not self.wk:
            self.wk = websocket.create_connection(f"ws://127.0.0.1:{self.inspector}/socket/1/1/WebPage", suppress_origin=True, timeout=5)
            try:
                while not self.target:
                    m = json.loads(self.wk.recv())
                    if m.get("method") == "Target.targetCreated" and m["params"]["targetInfo"].get("type") == "page":
                        self.target = m["params"]["targetInfo"]["targetId"]
            except websocket.WebSocketTimeoutException:
                pass
        self.n += 1
        n = self.n
        inner = {"id": n, "method": "Runtime.evaluate", "params": {"expression": expr, "returnByValue": True, "awaitPromise": True}}
        self.wk.send(json.dumps({"id": n, "method": "Target.sendMessageToTarget", "params": {"targetId": self.target, "message": json.dumps(inner)}}))
        end = time.time() + timeout
        while time.time() < end:
            try:
                m = json.loads(self.wk.recv())
            except websocket.WebSocketTimeoutException:
                continue
            if m.get("method") == "Target.dispatchMessageFromTarget":
                r = json.loads(m["params"]["message"])
                if r.get("id") == n:
                    res = r.get("result", {})
                    if res.get("wasThrown"):
                        raise RuntimeError(res["result"].get("description"))
                    return res.get("result", {}).get("value")
        raise TimeoutError(expr)

    def js(self, expr):
        """Evaluate in the Broadway page (not the app's)."""
        return self.call("Runtime.evaluate", expression=expr, returnByValue=True)["result"].get("value")

    def keys(self, *combos, pause=0.4):
        """Press keys: "Ctrl+f", "F5", "Enter", "Alt+Down"."""
        for combo in combos:
            *mods, k = combo.split("+") if combo != "+" else ["+"]
            m = sum(MODS[x] for x in mods)
            key, code, vk = NAMED.get(k) or (k, f"Key{k.upper()}" if k.isalpha() else "", ord(k.upper()))
            text = {"Enter": "\r"}.get(key, key if len(key) == 1 and not (m & 3) else "")
            for x in mods:
                self.call("Input.dispatchKeyEvent", type="rawKeyDown", key=x if x != "Ctrl" else "Control", modifiers=m)
            self.call("Input.dispatchKeyEvent", type="keyDown" if text else "rawKeyDown", key=key, code=code, windowsVirtualKeyCode=vk, modifiers=m, text=text)
            self.call("Input.dispatchKeyEvent", type="keyUp", key=key, code=code, windowsVirtualKeyCode=vk, modifiers=m)
            for x in reversed(mods):
                self.call("Input.dispatchKeyEvent", type="keyUp", key=x if x != "Ctrl" else "Control", modifiers=0)
            time.sleep(pause)

    def type(self, text, gap=0.08):
        for ch in text:
            self.keys("Space" if ch == " " else ch, pause=gap)

    def click(self, x, y):
        for t in ("mousePressed", "mouseReleased"):
            self.call("Input.dispatchMouseEvent", type=t, x=x, y=y, button="left", clickCount=1)

    def frame(self):
        """The window's rectangle on the page: Broadway draws it with its shadow at a place of its own."""
        r = self.call("Runtime.evaluate", expression="JSON.stringify([...document.querySelectorAll('canvas,div,img')].map(e => e.getBoundingClientRect()).filter(r => r.width > 600 && r.height > 400).map(r => [r.x, r.y, r.width, r.height])[0] || null)", returnByValue=True)
        v = json.loads(r["result"]["value"])
        return v or [100, 100, *self.size]

    def png(self, clip=None):
        x, y, w, h = clip or self.frame()
        r = self.call("Page.captureScreenshot", format="png", clip={"x": x, "y": y, "width": w, "height": h, "scale": 1})
        return base64.b64decode(r["data"])

    def shot(self, out, clip=None):
        Path(out).write_bytes(self.png(clip))

    def record(self, secs, during=None, fps=12, width=800):
        """Frames for `secs` seconds while `during` runs (in a thread), `width` wide; [(time, png)]."""
        import threading
        clip, frames, t0 = self.frame(), [], time.time()
        th = threading.Thread(target=during) if during else None
        # Its own connection: keys go on the main one meanwhile.
        tabs = json.load(urllib.request.urlopen(f"http://127.0.0.1:{self.debug}/json"))
        ws = websocket.create_connection([t for t in tabs if t["type"] == "page"][0]["webSocketDebuggerUrl"], suppress_origin=True)
        if th:
            # The main connection is the thread's; frames come over this one.
            th.start()
        i = 10_000
        while time.time() - t0 < secs:
            i += 1
            ws.send(json.dumps({"id": i, "method": "Page.captureScreenshot", "params": {"format": "png", "clip": {"x": clip[0], "y": clip[1], "width": clip[2], "height": clip[3], "scale": width / clip[2]}}}))
            while True:
                r = json.loads(ws.recv())
                if r.get("id") == i:
                    frames.append((time.time() - t0, base64.b64decode(r["result"]["data"])))
                    break
            time.sleep(max(0, t0 + len(frames) / fps - time.time()))
        if th:
            th.join()
        ws.close()
        return frames


class Terminal:
    """The terminal app in tmux (a server of its own), `cols`×`rows`."""

    def __init__(self, args=(), lang="en_GB.UTF-8", cols=140, rows=36):
        self.args, self.lang, self.cols, self.rows = list(args), lang, cols, rows
        self.sock = str(RUNTIME / "tmux.sock")

    def tmux(self, *a):
        return subprocess.run(["tmux", "-f", "/dev/null", "-S", self.sock, *a], capture_output=True, text=True, env=_env()).stdout

    def __enter__(self):
        cmd = " ".join([str(SANDBOX), "env", f"LANG={self.lang}", "COLORTERM=truecolor", "coxswain", *self.args])
        self.tmux("new-session", "-d", "-s", "t", "-x", str(self.cols), "-y", str(self.rows), cmd)
        return self

    def __exit__(self, *_):
        self.tmux("kill-server")

    def wait(self, secs):
        time.sleep(secs)

    def keys(self, *keys, pause=0.5):
        """tmux key names: "F5", "IC" (Insert), "M-b" (Alt+B), "Enter", "Escape"."""
        for k in keys:
            self.tmux("send-keys", "-t", "t", k)
            time.sleep(pause)

    def type(self, text, gap=0.08):
        for ch in text:
            self.tmux("send-keys", "-t", "t", "-l", ch)
            time.sleep(gap)

    def ansi(self):
        return self.tmux("capture-pane", "-p", "-e", "-N", "-t", "t")

    def shot(self, out):
        render([self.ansi()], [out], self.cols, self.rows)

    def record(self, secs, during=None, fps=12):
        import threading
        th = threading.Thread(target=during) if during else None
        frames, t0 = [], time.time()
        if th:
            th.start()
        while time.time() - t0 < secs:
            frames.append((time.time() - t0, self.ansi()))
            time.sleep(1 / fps)
        if th:
            th.join()
        tmp = Path(tempfile.mkdtemp(prefix="cap-frames-"))
        outs = [tmp / f"{i:04}.png" for i in range(len(frames))]
        render([a for _, a in frames], outs, self.cols, self.rows)
        return [(t, o.read_bytes()) for (t, _), o in zip(frames, outs)]

# ---------------------------------------------------------------- tmux's text as a page

PAL = ["#1d1f21", "#cc6666", "#b5bd68", "#f0c674", "#81a2be", "#b294bb", "#8abeb7", "#c5c8c6",
       "#666666", "#d54e53", "#b9ca4a", "#e7c547", "#7aa6da", "#c397d8", "#70c0b1", "#eaeaea"]
FG, BG = "#d8d8d8", "#181818"


def _c256(n):
    if n < 16:
        return PAL[n]
    if n < 232:
        n -= 16
        v = [0, 95, 135, 175, 215, 255]
        return "#%02x%02x%02x" % (v[n // 36], v[n // 6 % 6], v[n % 6])
    g = 8 + (n - 232) * 10
    return "#%02x%02x%02x" % (g, g, g)


def page(text, cols, rows):
    """`capture-pane -p -e -N` output as an HTML page with the terminal in `#t`."""
    import html, re
    st = {"fg": None, "bg": None, "b": False, "rev": False, "dim": False, "it": False, "ul": False}
    lines = text.split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    out = []
    for line in lines:
        pos = 0
        # tmux carries attributes from one line to the next.
        for m in re.finditer(r"\x1b\[([0-9;:]*)m", line + "\x1b[999m"):
            t = line[pos:m.start()]
            if t:
                fg, bg = st["fg"] or FG, st["bg"] or BG
                if st["rev"]:
                    fg, bg = bg, fg
                css = f"color:{fg};background:{bg}" + (";font-weight:bold" if st["b"] else "") + (";opacity:.7" if st["dim"] else "") + (";font-style:italic" if st["it"] else "") + (";text-decoration:underline" if st["ul"] else "")
                out.append(f'<span style="{css}">{html.escape(t)}</span>')
            pos = m.end()
            ps = [int(x) if x else 0 for x in re.split("[;:]", m.group(1))] if m.group(1) else [0]
            i = 0
            while i < len(ps):
                p = ps[i]
                if p == 0:
                    st.update(fg=None, bg=None, b=False, rev=False, dim=False, it=False, ul=False)
                elif p in (1, 2, 3, 4, 7):
                    st[{1: "b", 2: "dim", 3: "it", 4: "ul", 7: "rev"}[p]] = True
                elif p == 22:
                    st["b"] = st["dim"] = False
                elif p in (23, 24, 27):
                    st[{23: "it", 24: "ul", 27: "rev"}[p]] = False
                elif 30 <= p <= 37 or 90 <= p <= 97:
                    st["fg"] = PAL[p - 30 if p < 90 else p - 82]
                elif 40 <= p <= 47 or 100 <= p <= 107:
                    st["bg"] = PAL[p - 40 if p < 100 else p - 92]
                elif p in (39, 49):
                    st["fg" if p == 39 else "bg"] = None
                elif p in (38, 48) and i + 1 < len(ps):
                    key = "fg" if p == 38 else "bg"
                    if ps[i + 1] == 5:
                        st[key] = _c256(ps[i + 2]); i += 2
                    elif ps[i + 1] == 2:
                        st[key] = "#%02x%02x%02x" % tuple(ps[i + 2:i + 5]); i += 4
                i += 1
        out.append("\n")
    return f"""<!doctype html><meta charset="utf-8"><style>
body {{ margin: 0; background: {BG}; }}
pre {{ margin: 0; width: {cols}ch; height: calc({rows} * 1.17em); padding: 2px 6px; font: 20px/1.17 'MesloLGM Nerd Font Mono'; color: {FG}; background: {BG}; display: inline-block; font-variant-ligatures: none; }}
</style><pre id="t">{"".join(out)}</pre>"""


def render(screens, outs, cols, rows):
    """tmux `capture-pane -p -e -N` text as pictures of the terminal."""
    from playwright.sync_api import sync_playwright
    with sync_playwright() as p:
        b = p.chromium.launch(executable_path=shutil.which("chromium"), args=["--headless=new"])
        pg = b.new_page(viewport={"width": 2400, "height": 1400})
        for s, o in zip(screens, outs):
            pg.set_content(page(s, cols, rows))
            pg.locator("#t").screenshot(path=str(o))
        b.close()


def gif(frames, out, width=800, fps=12, hold=0.6):
    """[(time, png)] as a looping GIF, `width` wide, `fps` a second; the first frame is held
    `hold` seconds at both ends (a calm start and end) and saved beside it as <name>.png."""
    out = Path(out)
    tmp = Path(tempfile.mkdtemp(prefix="cap-gif-"))
    first = frames[0][1]
    subprocess.run(["magick", "png:-", "-resize", f"{width}x", str(out.with_suffix(".png"))], input=first, check=True)
    # Even steps: at each tick the latest frame taken by then.
    end = frames[-1][0]
    ticks, j = [], 0
    for k in range(int(end * fps) + 1):
        while j + 1 < len(frames) and frames[j + 1][0] <= k / fps:
            j += 1
        ticks.append(frames[j][1])
    seq = [first] * int(hold * fps) + ticks + [first] * int(hold * fps)
    for i, f in enumerate(seq):
        (tmp / f"{i:04}.png").write_bytes(f)
    vf = f"scale={width}:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle"
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-framerate", str(fps), "-i", str(tmp / "%04d.png"), "-vf", vf, "-loop", "0", str(out)], check=True)
    shutil.rmtree(tmp, ignore_errors=True)
    return out.stat().st_size

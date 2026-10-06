# /// script
# requires-python = ">=3.10"
# dependencies = ["websockets", "pyarrow", "openpyxl"]
# ///
"""Opens the desktop app's preview of one file of each kind and says how long each took, or
that it showed nothing. It runs the real app (a release build) on a display of its own (GTK's
Broadway, nothing appears on screen), in a home folder of its own, and reads the page through
WebKit's inspector. Linux only.

    cd gui && npm run build && cd .. && cargo build --release -p coxswain-gui
    uv run tools/preview-check.py            # every kind
    uv run tools/preview-check.py tex md     # some

A kind that shows nothing within its time fails the run (exit code 1).
"""

import asyncio, json, os, shutil, sqlite3, subprocess, sys, tempfile, time, urllib.request, zipfile, zlib, struct
from pathlib import Path

import websockets

REPO = Path(__file__).resolve().parent.parent
APP = REPO / "target/release/coxswain-gui"
PORT, DISPLAY, INSPECTOR = 8095, ":15", 9395


def png(path):
    raw = b"".join(b"\0" + b"\xff\x00\x00" * 8 for _ in range(8))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 8, 8, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def pdf(path):
    objs = ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>", "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"]
    text = b"BT /F1 18 Tf 20 50 Td (Rocket) Tj ET"
    objs.append(f"<< /Length {len(text)} >>\nstream\n{text.decode()}\nendstream")
    objs.append("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    out, offsets = b"%PDF-1.4\n", []
    for i, o in enumerate(objs, 1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n{o}\nendobj\n".encode()
    xref = len(out)
    out += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode() + b"".join(f"{o:010} 00000 n \n".encode() for o in offsets)
    out += f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    path.write_bytes(out)


def docx(path):
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("[Content_Types].xml", '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>')
        z.writestr("_rels/.rels", '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
        z.writestr("word/document.xml", '<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Rocket report</w:t></w:r></w:p></w:body></w:document>')


def files(d):
    """One file of each kind, and what in the preview shows it was drawn."""
    import pyarrow as pa, pyarrow.parquet as pq, openpyxl
    (d / "a.md").write_text("# Rocket\n\nFuel and *orbits*.\n")
    pq.write_table(pa.table({"id": list(range(300_000)), "city": ["Aarhus", "Odense", "Aalborg"] * 100_000}), d / "b.parquet")
    db = sqlite3.connect(d / "c.db")
    db.executescript("CREATE TABLE launch(id INTEGER, name TEXT); INSERT INTO launch VALUES (1, 'Tern'), (2, 'Kite');")
    db.commit()
    db.close()
    (d / "d.tex").write_text("\\documentclass{article}\n\\begin{document}\nHello, rockets.\n\\end{document}\n")
    pdf(d / "e.pdf")
    docx(d / "f.docx")
    wb = openpyxl.Workbook()
    wb.active.append(["id", "name"])
    wb.active.append([1, "Tern"])
    wb.save(d / "g.xlsx")
    png(d / "h.png")
    (d / "i.mmd").write_text("flowchart LR\n  A[Fuel] --> B[Lift-off]\n")
    (d / "j.html").write_text('<!doctype html><title>Rocket</title><h1>Rocket page</h1><img src="h.png">')
    # Provenance whose first output is a file here ("foo" has the digest it names).
    (d / "dist").mkdir()
    (d / "dist/rocket-1.4.0.tar.gz").write_text("foo")
    shutil.copy(REPO / "crates/coxswain-core/src/provenance/testdata/made/statement-v1.json", d / "k.provenance.json")
    return {
        "md": ("a.md", "article h1", "Rocket", 10),
        "parquet": ("b.parquet", "table td", "Aalborg", 10),
        "db": ("c.db", "table.db", "launch", 10),
        # A first build may fetch packages (tectonic); a second look is from the cache.
        "tex": ("d.tex", "iframe", "", 300),
        "pdf": ("e.pdf", "iframe", "", 10),
        "docx": ("f.docx", "article p", "Rocket report", 10),
        "xlsx": ("g.xlsx", "table td", "Tern", 10),
        "png": ("h.png", "img", "", 10),
        "mmd": ("i.mmd", "svg", "Lift-off", 20),
        "html": ("j.html", "iframe.page", "", 10),
        "provenance": ("k.provenance.json", ".prov .flow", "matches the file here", 10),
    }


MEASURE = """(async () => {
  const [name, ready, text, limit] = ARGS;
  const key = (k) => { document.activeElement?.blur?.(); window.dispatchEvent(new KeyboardEvent("keydown", { key: k, code: k, bubbles: true })); };
  const title = () => document.querySelector(".preview header b")?.textContent;
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  // The preview pane: F3 shows it.
  if (!document.querySelector(".preview")) { key("F3"); await sleep(500); }
  for (let i = 0; i < 30 && title() !== name; i++) { key("ArrowUp"); await sleep(5); }
  for (let i = 0; i < 30 && title() !== name; i++) { key("ArrowDown"); await sleep(5); }
  if (title() !== name) return "not reached: " + (title() ?? "no preview pane") + " " + document.body.innerText.slice(0, 200);
  key("ArrowUp"); await sleep(300);
  const t0 = performance.now(); key("ArrowDown");
  while (performance.now() - t0 < limit * 1000) {
    if (title() === name && [...document.querySelectorAll(".preview .body " + ready)].some((e) => e.textContent.includes(text))) return Math.round(performance.now() - t0);
    await sleep(20);
  }
  return "nothing shown: " + (document.querySelector(".preview .body")?.innerText ?? "").slice(0, 200);
})()"""


async def evaluate(expr, timeout):
    async with websockets.connect(f"ws://127.0.0.1:{INSPECTOR}/socket/1/1/WebPage", max_size=None) as ws:
        target, n, errors = None, 0, []

        async def until(pred, t):
            nonlocal target
            while True:
                m = json.loads(await asyncio.wait_for(ws.recv(), t))
                if m.get("method") == "Target.targetCreated":
                    info = m["params"]["targetInfo"]
                    # The page, not a frame in it (the PDF viewer has its own).
                    if info.get("type") == "page":
                        target = target or info["targetId"]
                if m.get("method") == "Target.dispatchMessageFromTarget":
                    inner = json.loads(m["params"]["message"])
                    if inner.get("method") == "Console.messageAdded" and inner["params"]["message"].get("level") == "error":
                        errors.append(inner["params"]["message"].get("text"))
                    if pred(inner):
                        return inner

        async def send(method, params):
            nonlocal n
            n += 1
            await ws.send(json.dumps({"id": n, "method": "Target.sendMessageToTarget", "params": {"targetId": target, "message": json.dumps({"id": n, "method": method, "params": params})}}))
            return await until(lambda i, m=n: i.get("id") == m, 120)

        try:
            await until(lambda i: False, 1.5)
        except asyncio.TimeoutError:
            pass
        await send("Console.enable", {})
        await send("Runtime.evaluate", {"expression": f"window.__check = undefined; Promise.resolve({expr}).then(v => window.__check = v, e => window.__check = 'error: ' + e)"})
        end = time.time() + timeout
        while time.time() < end:
            v = (await send("Runtime.evaluate", {"expression": "window.__check", "returnByValue": True}))["result"]["result"]
            if v.get("type") != "undefined":
                return v.get("value"), errors
            await asyncio.sleep(0.25)
        return "no answer", errors


def main():
    if not APP.exists():
        sys.exit(f"{APP} is missing: build the release first (see the top of this file)")
    home = Path(tempfile.mkdtemp(prefix="coxswain-preview-check-"))
    d = home / "files"
    d.mkdir()
    kinds = files(d)
    wanted = sys.argv[1:] or list(kinds)
    env = {"HOME": str(home), "USER": "demo", "PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "XDG_RUNTIME_DIR": os.environ.get("XDG_RUNTIME_DIR", str(home)),
           "XDG_CONFIG_HOME": str(home / ".config"), "XDG_CACHE_HOME": str(home / ".cache"), "XDG_DATA_HOME": str(home / ".local/share"),
           "GDK_BACKEND": "broadway", "BROADWAY_DISPLAY": DISPLAY, "WEBKIT_INSPECTOR_HTTP_SERVER": f"127.0.0.1:{INSPECTOR}", "WEBKIT_DISABLE_DMABUF_RENDERER": "1"}
    (home / ".config/coxswain").mkdir(parents=True)
    # Tectonic's packages, as this user has them: a first build need not download them again.
    tectonic = Path.home() / ".cache/tectonic"
    if tectonic.is_dir():
        (home / ".cache").mkdir()
        (home / ".cache/tectonic").symlink_to(tectonic)
    (home / ".config/coxswain/config.toml").write_text(f'[search]\nname_roots = ["{home}"]\ntext = false\n')
    # Not a first start: the first-run guide would cover the preview.
    (home / ".local/share/coxswain").mkdir(parents=True)
    (home / ".local/share/coxswain/state.json").write_text('{"guide_seen": true}')
    broadway = subprocess.Popen(["broadwayd", "--address", "127.0.0.1", "--port", str(PORT), DISPLAY], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    # Its own D-Bus too: nothing reaches the session of whoever runs this.
    app = subprocess.Popen(["dbus-run-session", "--", str(APP), str(d / kinds[wanted[0]][0])], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    failed = False
    try:
        for _ in range(60):
            try:
                if b"tauri://" in urllib.request.urlopen(f"http://127.0.0.1:{INSPECTOR}/", timeout=2).read():
                    break
            except OSError:
                pass
            time.sleep(1)
        time.sleep(3)
        for kind in wanted:
            name, ready, text, limit = kinds[kind]
            for look in ("first", "again") if kind == "tex" else ("first",):
                try:
                    ms, errors = asyncio.run(evaluate(MEASURE.replace("ARGS", json.dumps([name, ready, text, limit])), limit + 30))
                except (OSError, asyncio.TimeoutError, websockets.WebSocketException) as e:
                    ms, errors = f"the inspector did not answer ({type(e).__name__})", []
                ok = isinstance(ms, (int, float))
                failed |= not ok
                print(f"{kind:8} {look:6} {f'{ms} ms' if ok else 'FAILED: ' + str(ms)}" + (f"  console: {errors[-3:]}" if errors and not ok else ""))
    finally:
        os.killpg(app.pid, 15)
        broadway.terminate()
        # The search helper leaves the app's session to outlive it: find it by its home folder.
        for p in Path("/proc").glob("[0-9]*"):
            try:
                if f"\0HOME={home}\0".encode() in b"\0" + (p / "environ").read_bytes():
                    os.kill(int(p.name), 15)
            except OSError:
                pass
        shutil.rmtree(home, ignore_errors=True)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()

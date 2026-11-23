// The plugin's own page, in a sandboxed frame (opaque origin). Its one channel: postMessage.
const PORT = __PORT__, HOST = "__HOST__", FILE = "__FILE__";
const GLYPH = { K: "♔", Q: "♕", R: "♖", B: "♗", N: "♘", P: "♙", k: "♚", q: "♛", r: "♜", b: "♝", n: "♞", p: "♟" };
const board = document.getElementById("b");
const cells = [];
for (let i = 0; i < 64; i++) {
  const d = document.createElement("div");
  d.className = ((i >> 3) + (i & 7)) % 2 ? "d" : "l";
  board.appendChild(d);
  cells.push(d);
}
function draw(fen) {
  let i = 0;
  for (const c of fen.split(" ")[0]) {
    if (c === "/") continue;
    if (c >= "1" && c <= "8") for (let k = 0; k < +c; k++) cells[i++].textContent = "";
    else cells[i++].textContent = GLYPH[c] || "";
  }
}
const tbody = document.getElementById("t");
function render(rows) {
  const f = document.createDocumentFragment();
  for (const r of rows) {
    const tr = document.createElement("tr");
    for (const v of [r.white, r.black, r.result, r.date, r.eco, r.plies]) {
      const td = document.createElement("td");
      td.textContent = v;
      tr.appendChild(td);
    }
    f.appendChild(tr);
  }
  tbody.replaceChildren(f);
}

let next = 0;
const waiting = new Map();
function call(fn, args) {
  return new Promise((ok) => {
    const id = ++next;
    waiting.set(id, ok);
    parent.postMessage({ type: "call", id, fn, args }, "*");
  });
}

async function probes() {
  const r = {};
  const tryit = (k, f) => { try { r[k] = "REACHED " + String(f()).slice(0, 80); } catch (e) { r[k] = "blocked: " + e.name; } };
  tryit("parent_document", () => parent.document.title);
  tryit("top_location_read", () => top.location.href);
  tryit("parent_tauri_internals", () => typeof parent.__TAURI_INTERNALS__);
  r.globals = { __TAURI__: typeof window.__TAURI__, __TAURI_INTERNALS__: typeof window.__TAURI_INTERNALS__, ipc: typeof window.ipc,
    webkit_handlers: typeof (window.webkit && window.webkit.messageHandlers), chrome_webview: typeof (window.chrome && window.chrome.webview) };
  // Tauri's IPC by every door it has: the Rust side counts any call that arrives.
  const ipcMsg = JSON.stringify({ cmd: "breach", callback: 1, error: 2, payload: {}, options: {} });
  tryit("ipc_internals_invoke", () => window.__TAURI_INTERNALS__.invoke("breach"));
  tryit("ipc_window_ipc", () => window.ipc.postMessage(ipcMsg));
  tryit("ipc_webkit_handler", () => window.webkit.messageHandlers.ipc.postMessage(ipcMsg));
  tryit("ipc_webview2", () => window.chrome.webview.postMessage(ipcMsg));
  const hdr = { "Content-Type": "application/json", "Tauri-Callback": "1", "Tauri-Error": "2" };
  const tries = {
    ipc_fetch_scheme: () => fetch("ipc://localhost/breach", { method: "POST", body: "{}", headers: hdr }),
    ipc_fetch_http: () => fetch("http://ipc.localhost/breach", { method: "POST", body: "{}", headers: hdr }),
    net_fetch_local: () => fetch(`http://127.0.0.1:${PORT}/fetch`),
    net_fetch_nocors: () => fetch(`http://127.0.0.1:${PORT}/fetch-no-cors`, { mode: "no-cors" }),
    net_fetch_internet: () => fetch("https://example.com/", { mode: "no-cors" }),
    net_xhr: () => new Promise((ok, no) => { const x = new XMLHttpRequest(); x.open("GET", `http://127.0.0.1:${PORT}/xhr`); x.onload = () => ok(x.status); x.onerror = () => no(new Error("xhr error")); x.send(); }),
    net_img: () => new Promise((ok, no) => { const i = new Image(); i.onload = () => ok("loaded"); i.onerror = () => no(new Error("img error")); i.src = `http://127.0.0.1:${PORT}/img`; }),
    net_websocket: () => new Promise((ok, no) => { try { const w = new WebSocket(`ws://127.0.0.1:${PORT}/ws`); w.onopen = () => ok("open"); w.onerror = () => no(new Error("ws error")); } catch (e) { no(e); } }),
    net_beacon: () => Promise.resolve(navigator.sendBeacon(`http://127.0.0.1:${PORT}/beacon`, "x")),
    net_css_url: () => { document.body.style.backgroundImage = `url(http://127.0.0.1:${PORT}/css)`; return Promise.resolve("set"); },
    file_fetch: () => fetch(FILE).then((x) => x.text()),
    app_origin_fetch: () => fetch(HOST + "/index.html").then((x) => x.text()),
    asset_fetch: () => fetch("asset://localhost/" + encodeURIComponent(FILE.replace("file://", ""))).then((x) => x.text()),
    asset_fetch_http: () => fetch("http://asset.localhost/" + encodeURIComponent(FILE.replace("file://", ""))).then((x) => x.text()),
    plugin_scheme_fetch: () => fetch(location.href).then((x) => x.text()),
  };
  await Promise.all(Object.entries(tries).map(async ([k, f]) => {
    try {
      const v = await Promise.race([f(), new Promise((_, no) => setTimeout(() => no(new Error("timeout")), 2000))]);
      r[k] = "resolved " + String(v && v.status !== undefined ? "status " + v.status + " type " + v.type : v).slice(0, 60);
    } catch (e) { r[k] = "blocked: " + (e.name || "") + " " + (e.message || "").slice(0, 60); }
  }));
  tryit("window_open", () => { const w = window.open(HOST + "/index.html?breach=popup"); return w ? "window" : "null"; });
  r.window_open = r.window_open === "REACHED null" ? "blocked: window.open returned null" : r.window_open;
  r.theme = { "--bg": getComputedStyle(document.documentElement).getPropertyValue("--bg").trim(), body_background: getComputedStyle(document.body).backgroundColor };
  r.origin = String(self.origin);
  return r;
}

window.addEventListener("message", async (e) => {
  if (e.source !== parent) return;
  const m = e.data;
  if (m.type === "reply") { const ok = waiting.get(m.id); waiting.delete(m.id); if (ok) ok(m.value); return; }
  if (m.type === "probe") parent.postMessage({ type: "probes", value: await probes() }, "*");
  if (m.type === "leaked") {
    // The real IPC message, key and all, by every door left in this frame.
    const msg = JSON.stringify({ cmd: "breach", callback: 1, error: 2, payload: {}, options: {}, __TAURI_INVOKE_KEY__: m.key });
    const r = {};
    try { window.webkit.messageHandlers.ipc.postMessage(msg); r.webkit_handler = "sent"; } catch (err) { r.webkit_handler = "blocked: " + err.name; }
    try { window.chrome.webview.postMessage(msg); r.webview2 = "sent"; } catch (err) { r.webview2 = "blocked: " + err.name; }
    try { window.ipc.postMessage(msg); r.window_ipc = "sent"; } catch (err) { r.window_ipc = "blocked: " + err.name; }
    for (const u of ["ipc://localhost/breach", "http://ipc.localhost/breach"]) {
      try { await fetch(u, { method: "POST", body: "{}", headers: { "Content-Type": "application/json", "Tauri-Callback": "1", "Tauri-Error": "2", "Tauri-Invoke-Key": m.key } }); r[u] = "resolved"; }
      catch (err) { r[u] = "blocked: " + err.name; }
    }
    parent.postMessage({ type: "leaked", value: r }, "*");
  }
  if (m.type === "topnav") {
    try { top.location.href = HOST + "/index.html?breach=topnav"; parent.postMessage({ type: "topnav", value: "no exception" }, "*"); }
    catch (err) { parent.postMessage({ type: "topnav", value: "blocked: " + err.name }, "*"); }
  }
  if (m.type === "bench-step") {
    // (b) every position in one round trip, then stepped here; (c) a round trip a step.
    let t = performance.now();
    const fens = await call("positions", {});
    const one = performance.now() - t;
    t = performance.now();
    for (const f of fens) draw(f);
    const local = (performance.now() - t) / fens.length;
    t = performance.now();
    for (let i = 0; i < 200; i++) { await call("page", { start: i, count: 1 }); draw(fens[i % fens.length]); }
    const per = (performance.now() - t) / 200;
    parent.postMessage({ type: "step", value: { positions: fens.length, all_positions_roundtrip_ms: one, local_step_ms: local, roundtrip_per_step_ms: per } }, "*");
  }
  if (m.type === "bench-table") {
    const out = {};
    for (const size of m.sizes) {
      const t = performance.now();
      let pages = 0;
      for (let s = 0; s < m.total; s += size) { render(await call("page", { start: s, count: Math.min(size, m.total - s) })); pages++; }
      const ms = performance.now() - t;
      out[size] = { total_ms: Math.round(ms), per_page_ms: +(ms / pages).toFixed(3) };
    }
    parent.postMessage({ type: "table", value: out }, "*");
  }
});

draw("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
// Painted, then ready.
requestAnimationFrame(() => setTimeout(() => parent.postMessage({ type: "ready" }, "*"), 0));

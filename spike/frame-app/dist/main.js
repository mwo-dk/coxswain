// The host page: today's way (the board drawn here from one invoke) and the plugin frame.
const invoke = (c, a) => window.__TAURI_INTERNALS__.invoke(c, a);
const PLUGIN = navigator.userAgent.includes("Windows") ? "http://plugin.localhost/frame.html" : "plugin://localhost/frame.html";
const GLYPH = { K: "♔", Q: "♕", R: "♖", B: "♗", N: "♘", P: "♙", k: "♚", q: "♛", r: "♜", b: "♝", n: "♞", p: "♟" };
const board = document.getElementById("b");
const cells = [];
for (let i = 0; i < 64; i++) { const d = document.createElement("div"); d.className = ((i >> 3) + (i & 7)) % 2 ? "d" : "l"; board.appendChild(d); cells.push(d); }
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
    for (const v of [r.white, r.black, r.result, r.date, r.eco, r.plies]) { const td = document.createElement("td"); td.textContent = v; tr.appendChild(td); }
    f.appendChild(tr);
  }
  tbody.replaceChildren(f);
}
const log = (s) => (document.getElementById("log").textContent += s + "\n");

let frame = null;
const handlers = {};
const once = (type) => new Promise((ok) => (handlers[type] = ok));
// The frame's one channel: a message from that frame only, to a named plugin call only.
window.addEventListener("message", async (e) => {
  if (!frame || e.source !== frame.contentWindow) return;
  const m = e.data;
  if (m.type === "call") {
    const fn = { positions: "plugin_positions", page: "plugin_page" }[m.fn];
    if (!fn) return;
    const value = await invoke(fn, m.args);
    frame.contentWindow.postMessage({ type: "reply", id: m.id, value }, "*");
  } else if (handlers[m.type]) { const h = handlers[m.type]; delete handlers[m.type]; h(m.value); }
});
function open() {
  const t = performance.now();
  const ready = once("ready");
  if (frame) frame.remove();
  frame = document.createElement("iframe");
  frame.setAttribute("sandbox", "allow-scripts");
  frame.src = PLUGIN;
  document.getElementById("slot").appendChild(frame);
  return ready.then(() => performance.now() - t);
}
const send = (m) => frame.contentWindow.postMessage(m, "*");

async function selftest() {
  const rep = { user_agent: navigator.userAgent };
  const params = new URLSearchParams(location.search);
  if (params.get("breach")) {
    // A frame moved this page or opened it: the seal failed there.
    rep.reached = params.get("breach");
    return invoke("report", { json: JSON.stringify({ ...JSON.parse((await invoke("stash", {})) || "{}"), ["breach_" + params.get("breach")]: "REACHED" }) });
  }
  // Control: the frame's raw IPC attempts, made from this page, then one proper invoke. Shows
  // whether a raw message without Tauri's own wrapper reaches Rust at all, and that the counter counts.
  const raw = JSON.stringify({ cmd: "breach", callback: 1, error: 2, payload: {}, options: {} });
  try { window.webkit.messageHandlers.ipc.postMessage(raw); } catch (e) {}
  try { window.ipc.postMessage(raw); } catch (e) {}
  try { window.chrome.webview.postMessage(raw); } catch (e) {}
  for (const u of ["ipc://localhost/breach", "http://ipc.localhost/breach"]) {
    try { await fetch(u, { method: "POST", body: "{}", headers: { "Content-Type": "application/json", "Tauri-Callback": "1", "Tauri-Error": "2" } }); } catch (e) {}
  }
  await new Promise((ok) => setTimeout(ok, 500));
  rep.control_raw_from_host = await invoke("counts");
  await invoke("breach");
  rep.control_proper_invoke = await invoke("counts");
  rep.open_ms = { cold: await open(), second: await open(), third: await open() };
  send({ type: "probe" });
  rep.probes = await once("probes");
  await new Promise((ok) => setTimeout(ok, 1500));
  rep.rust_side = await invoke("counts");
  // (a) today's way: one invoke for every position, stepped here.
  let t = performance.now();
  const fens = await invoke("native_positions");
  const one = performance.now() - t;
  t = performance.now();
  for (const f of fens) draw(f);
  rep.step_today = { positions: fens.length, all_positions_invoke_ms: one, local_step_ms: (performance.now() - t) / fens.length };
  t = performance.now();
  for (let i = 0; i < 200; i++) { await invoke("native_page", { start: i, count: 1 }); draw(fens[i % fens.length]); }
  rep.step_today.invoke_per_step_ms = (performance.now() - t) / 200;
  send({ type: "bench-step" });
  rep.step_frame = await once("step");
  const total = 100000, sizes = [50, 100, 500, 1000];
  send({ type: "bench-table", sizes, total });
  rep.table_frame = await once("table");
  for (const [name, cmd] of [["table_today_native", "native_page"], ["table_host_plugin_direct", "plugin_page"]]) {
    rep[name] = {};
    for (const size of sizes) {
      const t = performance.now();
      let pages = 0;
      for (let s = 0; s < total; s += size) { render(await invoke(cmd, { start: s, count: Math.min(size, total - s) })); pages++; }
      const ms = performance.now() - t;
      rep[name][size] = { total_ms: Math.round(ms), per_page_ms: +(ms / pages).toFixed(3) };
    }
  }
  // Last: top navigation and pop-ups. If one gets through, this page reloads and reports it.
  // What a frame could do with a leaked invoke key (it never gets one in the design).
  const before = (await invoke("counts")).breach_calls;
  send({ type: "leaked", key: await invoke("leak_key") });
  rep.leaked_key_probes = await once("leaked");
  await new Promise((ok) => setTimeout(ok, 1000));
  rep.leaked_key_breach_calls_added = (await invoke("counts")).breach_calls - before;
  rep.rust_side_end = await invoke("counts");
  await invoke("stash", { json: JSON.stringify(rep) });
  send({ type: "topnav" });
  rep.top_navigation = await Promise.race([once("topnav"), new Promise((ok) => setTimeout(() => ok("no answer"), 2000))]);
  await new Promise((ok) => setTimeout(ok, 1500));
  rep.top_navigation_page_still_here = true;
  invoke("report", { json: JSON.stringify(rep) });
}
invoke("selftest").then((on) => (on ? selftest() : open()));

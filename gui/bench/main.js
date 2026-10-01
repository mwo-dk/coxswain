// Mounts the real DetailsView (or GridView with ?view=grid) with a synthetic 100k listing
// and measures, synchronously.
import { mount, flushSync, unmount } from "svelte";
import { ui, newTab } from "../src/app.svelte.js";
import DetailsView from "../src/DetailsView.svelte";
import GridView from "../src/GridView.svelte";

const q = new URLSearchParams(location.search);
const n = Number(q.get("n") ?? 100000);
const View = q.get("view") === "grid" ? GridView : DetailsView;
ui.cfg = { glyphs: {}, gui: { font_size: 13, line_height: 1.9 }, looks: {}, strings: {}, settings: {} };
document.documentElement.style.setProperty("--row", "25px");
ui.panes = [{ active: 0, tabs: [newTab("/bench")] }];
const t = ui.panes[0].tabs[0];
const items = [];
for (let i = 0; i < n; i++) items.push({ name: `file_${i}.txt`, path: `/bench/file_${i}.txt`, is_dir: i % 50 === 0, is_symlink: false, is_exec: false, hidden: false, size: i * 7, modified: 1700000000 + i, created: 0, icon: { glyph: "", color: "" }, tag: null });
const res = { n, view: q.get("view") ?? "details" };
const time = (k, f) => { const t0 = performance.now(); f(); flushSync(); document.getElementById("app").offsetHeight; res[k] = +(performance.now() - t0).toFixed(1); document.getElementById("out").textContent = JSON.stringify(res); };
window.onerror = (e) => (document.getElementById("out").textContent += " ERR " + e);
const app = mount(View, { target: document.getElementById("app"), props: { t, active: true, onfocus() {} } });
time("mount_empty_ms", () => {});
time("set_items_ms", () => (t.items = items));
res.rows_in_dom = document.querySelectorAll('[role="option"]').length;
time("cursor_down_ms", () => (t.cursor = 1));
time("cursor_end_ms", () => (t.cursor = n - 1));
res.cursor_end_in_dom = !!document.querySelector('[aria-selected="true"]');
time("cursor_home_ms", () => (t.cursor = 0));
time("mark_one_ms", () => t.marked.add(items[0].path));
time("relist_same_ms", () => (t.items = items.map((e) => ({ ...e }))));
time("relist_reversed_ms", () => (t.items = [...items].reverse()));
// Scrolled far down, then a short list with the same cursor: its rows show (they did not).
const box = document.querySelector('[role="listbox"]');
box.scrollTop = box.scrollHeight;
box.dispatchEvent(new Event("scroll"));
flushSync();
// (the cursor stays where it was, at the top)
time("shrink_ms", () => (t.items = items.slice(0, 30)));
await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
flushSync();
res.shrunk_rows_in_dom = document.querySelectorAll('[role="option"]').length;
time("clear_ms", () => (t.items = []));
res.heap_mb = performance.memory ? +(performance.memory.usedJSHeapSize / 1e6).toFixed(0) : null;
unmount(app);
document.getElementById("out").textContent = JSON.stringify(res);

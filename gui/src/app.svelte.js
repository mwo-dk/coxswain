// Shared app state and navigation. Components read `ui` and call these functions.

import { SvelteSet } from "svelte/reactivity";
import { invoke, basename, parent, applyTheme } from "./lib.js";

export const ui = $state({
  cfg: null,
  panes: [],
  activePane: 0,
  dual: true,
  showHidden: true,
  showSidebar: true,
  showPreview: false,
  sidebarW: 230,
  previewW: 400,
  split: 50,
  theme: "dark",
  status: "",
  quick: null,
  cmd: "",
  lastOutput: "",
  /** One modal at a time: { kind, ... } */
  modal: null,
  favorites: [],
  recent: [],
  places: [],
  disks: [],
});

let nextId = 1;

export function newTab(dir, view = "details") {
  return {
    id: nextId++,
    dir,
    items: [],
    cursor: 0,
    marked: new SvelteSet(),
    sort: "name",
    reverse: false,
    view,
    git: null,
    error: null,
    sizes: {},
    hasNotes: false,
    back: [],
    fwd: [],
  };
}

export const pane = (i = ui.activePane) => ui.panes[i];
export const tab = (i = ui.activePane) => ui.panes[i]?.tabs[ui.panes[i].active];
export const otherTab = () => tab(ui.dual ? ui.activePane ^ 1 : ui.activePane);
export const item = (t = tab()) => t?.items[t.cursor];

export function setTheme(name) {
  const t = ui.cfg.themes[name] ?? ui.cfg.themes.dark;
  ui.theme = ui.cfg.themes[name] ? name : "dark";
  applyTheme(t, ui.cfg.gui);
}

/** Re-read a tab, keeping the cursor on `focus` (default: the current name). */
export async function load(t, dir = t.dir, focus) {
  const keep = focus ?? item(t)?.name;
  try {
    const r = await invoke("list_dir", { dir, showHidden: ui.showHidden, sort: t.sort, reverse: t.reverse });
    if (r.dir !== t.dir) {
      t.marked.clear();
      t.git = null;
      t.sizes = {};
    }
    const alive = new Set(r.items.map((e) => e.path));
    for (const m of [...t.marked]) if (!alive.has(m)) t.marked.delete(m);
    t.dir = r.dir;
    t.items = r.items;
    t.hasNotes = r.has_notes;
    t.error = null;
    const at = keep ? r.items.findIndex((e) => e.name === keep) : -1;
    t.cursor = at >= 0 ? at : Math.max(0, Math.min(t.cursor, r.items.length - 1));
  } catch (e) {
    t.error = String(e);
    return false;
  }
  invoke("git_status", { dir: t.dir }).then((g) => {
    if (t.dir === dir) t.git = g;
    if (g && !ui.recent.includes(g.root)) ui.recent = [g.root, ...ui.recent].slice(0, 12);
  });
  return true;
}

export async function cd(t, dir, history = true) {
  if (!dir || dir === t.dir) return;
  const from = t.dir;
  const prevCursor = t.cursor;
  t.cursor = 0;
  const ok = await load(t, dir, parent(from) === dir ? basename(from) : undefined);
  if (!ok) {
    t.cursor = prevCursor;
    ui.status = t.error;
    t.error = null;
    return;
  }
  if (history) {
    t.back.push(from);
    t.fwd = [];
  }
}

export function goBack(t = tab()) {
  const d = t.back.pop();
  if (d) {
    t.fwd.push(t.dir);
    cd(t, d, false);
  }
}

export function goForward(t = tab()) {
  const d = t.fwd.pop();
  if (d) {
    t.back.push(t.dir);
    cd(t, d, false);
  }
}

export const reloadAll = () => Promise.all(visibleTabs().map((t) => load(t)));

export function visibleTabs() {
  return (ui.dual ? ui.panes : [pane()]).map((p) => p.tabs[p.active]);
}

export function targets(t = tab()) {
  if (t.marked.size) return t.items.filter((e) => t.marked.has(e.path)).map((e) => e.path);
  const e = item(t);
  return e && e.name !== ".." ? [e.path] : [];
}

// ------------------------------------------------------------ session

export function snapshot() {
  return {
    panes: ui.panes.map((p) => ({
      active: p.active,
      tabs: p.tabs.map((t) => ({ dir: t.dir, view: t.view, sort: t.sort, reverse: t.reverse })),
    })),
    activePane: ui.activePane,
    dual: ui.dual,
    showHidden: ui.showHidden,
    showSidebar: ui.showSidebar,
    showPreview: ui.showPreview,
    sidebarW: ui.sidebarW,
    previewW: ui.previewW,
    split: ui.split,
    theme: ui.theme,
  };
}

export async function init() {
  ui.cfg = await invoke("get_config");
  const st = await invoke("get_state");
  const s = st.session ?? {};
  ui.favorites = st.favorites;
  ui.recent = st.recent_repos;
  for (const k of ["dual", "showHidden", "showSidebar", "showPreview", "sidebarW", "previewW", "split"]) if (k in s) ui[k] = s[k];
  if (!("showHidden" in s)) ui.showHidden = ui.cfg.show_hidden;
  setTheme(s.theme ?? ui.cfg.gui.theme);
  // Directories given on the command line win over the saved session.
  const fromArgs = ui.cfg.start;
  ui.panes = (s.panes?.length === 2 ? s.panes : [{ tabs: [{ dir: fromArgs[0] }] }, { tabs: [{ dir: fromArgs[1] }] }]).map((p, i) => ({
    active: Math.min(p.active ?? 0, p.tabs.length - 1),
    tabs: p.tabs.map((x) => Object.assign(newTab(x.dir, x.view), { sort: x.sort ?? "name", reverse: !!x.reverse })),
  }));
  ui.activePane = s.activePane ?? 0;
  const t0 = tab(0);
  if (s.panes && fromArgs[0] !== t0.dir && !ui.panes[0].tabs.some((t) => t.dir === fromArgs[0])) {
    ui.panes[0].tabs.push(newTab(fromArgs[0]));
    ui.panes[0].active = ui.panes[0].tabs.length - 1;
  }
  // A saved directory may be gone; fall back to home.
  await Promise.all(
    ui.panes.flatMap((p) => p.tabs).map(async (t) => {
      if (!(await load(t))) await load(t, ui.cfg.home);
    }),
  );
  invoke("places").then((p) => (ui.places = p));
  invoke("disks").then((d) => (ui.disks = d));
}

/** Enter / double-click: folders open in place, files in their default application. */
export function openItem(t, i = t.cursor) {
  const e = t.items[i];
  if (!e) return;
  if (e.is_dir) return cd(t, e.path);
  invoke("open_path", { path: e.path }).then(
    () => (ui.status = `Opened ${e.name}`),
    (err) => (ui.status = String(err)),
  );
}

export function toggleMark(t, i) {
  const e = t.items[i];
  if (!e || e.name === "..") return;
  t.marked.has(e.path) ? t.marked.delete(e.path) : t.marked.add(e.path);
}

/** Make pane `p` (and optionally its tab `ti`) the active one. */
export function focusPane(p, ti) {
  ui.activePane = p;
  if (ti !== undefined) ui.panes[p].active = ti;
}

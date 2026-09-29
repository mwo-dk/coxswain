// Shared app state and navigation. Components read `ui` and call these functions.

import { SvelteSet } from "svelte/reactivity";
import { invoke, basename, parent, applyTheme } from "./lib.js";
import { setLanguage, t } from "./i18n.svelte.js";

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
  theme: "cyber",
  status: "",
  quick: null,
  cmd: "",
  lastOutput: "",
  /** Pane under a file being dragged in, for the highlight. */
  dropPane: null,
  /** Optional columns in the details view. */
  columns: { type: true, size: true, files: false, modified: true, created: false },
  /** Measure every folder's size as a folder opens. */
  // Folder sizes measured in the background. `sizes` in the session: the older `autoSizes`
  // was off unless switched on, and is not read any more.
  autoSizes: true,
  /** Preview: source instead of rendered/tree, and diff instead of file. Kept across files. */
  previewSource: false,
  previewDiff: false,
  /** Tool -> the engine id picked in the preview (local program or container). */
  previewEngine: {},
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
    /** Folder path -> number of files inside it, all levels (measured with the size). */
    counts: {},
    hasNotes: false,
    back: [],
    fwd: [],
  };
}

export const pane = (i = ui.activePane) => ui.panes[i];
export const tab = (i = ui.activePane) => ui.panes[i]?.tabs[ui.panes[i].active];
export const otherTab = () => tab(ui.dual ? ui.activePane ^ 1 : ui.activePane);
export const item = (t = tab()) => t?.items[t.cursor];

/** Show a theme; with `save`, also store it in config.toml, as Settings does. */
export async function setTheme(name, save = false) {
  if (!ui.cfg.themes[name]) name = "cyber";
  ui.theme = name;
  applyTheme(ui.cfg.themes[name], ui.cfg.gui, ui.cfg.looks[name]);
  if (!save) return;
  try {
    ui.cfg = await invoke("save_settings", { changes: { theme: name } });
  } catch (e) {
    ui.status = String(e);
  }
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
      t.counts = {};
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
  if (ui.autoSizes) measureFolders(t);
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

/** Size and file count of the given folders (default: all in the tab), one at a time so the
 *  first results show quickly. Stops as soon as the tab shows another folder. */
const measuring = new WeakMap();
const tabIds = new WeakMap();
let tabCount = 0;
const tabId = (t) => tabIds.get(t) ?? (tabIds.set(t, String(++tabCount)), tabIds.get(t));
export async function measureFolders(t, paths) {
  const dir = t.dir;
  // A reload while the automatic run is going (the watcher does that) must not start a second one.
  if (!paths && measuring.get(t) === dir) return;
  if (!paths) measuring.set(t, dir);
  try {
    const todo = paths ?? t.items.filter((e) => e.is_dir && e.name !== ".." && t.sizes[e.path] === undefined).map((e) => e.path);
    for (const p of todo) {
      if (t.dir !== dir) return;
      // The automatic run is this tab's own: a new one stops it, and recent sizes are reused.
      const r = (await invoke("dir_sizes", { paths: [p], tab: paths ? null : tabId(t) }))[p];
      if (t.dir !== dir) return;
      if (!r) continue;
      t.sizes[p] = r[0];
      t.counts[p] = r[1];
    }
  } finally {
    if (!paths && measuring.get(t) === dir) measuring.delete(t);
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
    columns: ui.columns,
    sizes: ui.autoSizes,
    previewSource: ui.previewSource,
    previewDiff: ui.previewDiff,
    previewEngine: ui.previewEngine,
    sidebarW: ui.sidebarW,
    previewW: ui.previewW,
    split: ui.split,
  };
}

export async function init() {
  ui.cfg = await invoke("get_config");
  setLanguage(ui.cfg);
  const st = await invoke("get_state");
  const s = st.session ?? {};
  ui.favorites = st.favorites;
  ui.recent = st.recent_repos;
  for (const k of ["dual", "showHidden", "showSidebar", "showPreview", "sidebarW", "previewW", "split", "previewSource", "previewDiff", "previewEngine"]) if (k in s) ui[k] = s[k];
  if (s.columns) ui.columns = { ...ui.columns, ...s.columns };
  if (!("showHidden" in s)) ui.showHidden = ui.cfg.show_hidden;
  ui.autoSizes = s.sizes ?? ui.cfg.folder_sizes;
  setTheme(ui.cfg.gui.theme);
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
  // A file (`bosum-gui ~/Pictures/cat.jpg`) opens its folder with the cursor on it. A saved
  // directory may be gone; fall back to home.
  await Promise.all(
    ui.panes.flatMap((p) => p.tabs).map(async (t) => {
      if (await load(t)) return;
      const up = parent(t.dir);
      if (!(up && (await load(t, up, basename(t.dir))))) await load(t, ui.cfg.home);
    }),
  );
  invoke("places").then((p) => (ui.places = p));
  // `bosum-gui --duplicates <folders>` starts straight in a scan of those folders.
  if (ui.cfg.duplicates) ui.modal = { kind: "dupes", roots: Object.fromEntries(ui.cfg.duplicates.map((p) => [p, true])), autostart: true };
  if (ui.cfg.open_settings) ui.modal = { kind: "settings" };
  invoke("disks").then((d) => (ui.disks = d));
}

/** Enter / double-click: folders open in place, files in their default application. */
export function openItem(tb, i = tb.cursor) {
  const e = tb.items[i];
  if (!e) return;
  if (e.is_dir) return cd(tb, e.path);
  invoke("open_path", { path: e.path }).then(
    () => (ui.status = t("status.opened", { name: e.name })),
    (err) => (ui.status = String(err)),
  );
}

export function toggleMark(t, i) {
  const e = t.items[i];
  if (!e || e.name === "..") return;
  t.marked.has(e.path) ? t.marked.delete(e.path) : t.marked.add(e.path);
}

/** Native drag of the marked files (or the one at `i`), so other apps can take them. */
export function dragOut(t, i) {
  const e = t.items[i];
  if (!e || e.name === "..") return;
  t.cursor = i;
  const paths = t.marked.has(e.path) ? targets(t) : [e.path];
  invoke("start_drag", { paths }).catch((err) => (ui.status = String(err)));
}

/** The details view's column choices and automatic folder sizes, as a menu. */
export function columnMenu() {
  const names = { type: t("app.col.type"), size: t("app.col.size"), files: t("app.col.files"), modified: t("app.col.modified"), created: t("app.col.created") };
  const box = (on) => (on ? "\u{f0132}" : "\u{f0131}");
  ui.modal = {
    kind: "menu",
    title: t("action.columns"),
    direct: false,
    filter: "",
    cursor: 0,
    items: [
      ...Object.entries(names).map(([id, label]) => ({ label, icon: box(ui.columns[id]), run: () => ((ui.columns[id] = !ui.columns[id]), columnMenu()) })),
      {
        label: t("app.auto_sizes"),
        icon: box(ui.autoSizes),
        run: () => {
          ui.autoSizes = !ui.autoSizes;
          if (ui.autoSizes) for (const x of visibleTabs()) measureFolders(x);
          columnMenu();
        },
      },
    ],
  };
}

const VIEWS = ["details", "columns", "grid"];
export const nextView = (v) => VIEWS[(VIEWS.indexOf(v) + 1) % VIEWS.length];

/** Make pane `p` (and optionally its tab `ti`) the active one. */
export function focusPane(p, ti) {
  ui.activePane = p;
  if (ti !== undefined) ui.panes[p].active = ti;
}

// Shared app state and navigation. Components read `ui` and call these functions.

import { SvelteSet } from "svelte/reactivity";
import { invoke, listDir, basename, parent, applyTheme, isArchive, HISTORY, LOCKED } from "./lib.js";
import { setLanguage, t } from "./i18n.svelte.js";
/** The texts, for functions whose tab is called `t`. */
const tr = t;

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
  columns: { type: true, size: true, files: false, modified: true, commit: true, created: false },
  /** Measure every folder's size as a folder opens. */
  // Folder sizes measured in the background. `sizes` in the session: the older `autoSizes`
  // was off unless switched on, and is not read any more.
  autoSizes: true,
  /** Preview: source (true) instead of rendered/tree, and diff instead of file. Kept across files.
   *  A BOM also has "sunburst", which other kinds show as rendered. */
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
    /** How far the list is scrolled, kept per tab. */
    top: 0,
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

/** Every theme's id: the built-in ones in their order, then the user's `[themes.<name>]`. */
export const themeIds = () => [...ui.cfg.builtin_themes, ...Object.keys(ui.cfg.themes).filter((id) => !ui.cfg.builtin_themes.includes(id))];
/** A theme's name as shown: translated when built in, else as the user named it. */
export const themeName = (id) => (ui.cfg.builtin_themes.includes(id) ? t(`theme.${id}`) : id);

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
  // A slow disk: after a moment the status line says what is being read.
  const slow = setTimeout(() => (ui.status = tr("status.busy", { what: basename(dir) || dir })), 150);
  try {
    const r = await listDir({ dir, showHidden: ui.showHidden, sort: t.sort, reverse: t.reverse }).finally(() => {
      clearTimeout(slow);
      if (ui.status === tr("status.busy", { what: basename(dir) || dir })) ui.status = "";
    });
    if (r.dir !== t.dir) {
      t.marked.clear();
      // In the same repository the last-commit column stays (empty) until git answers, so
      // the columns do not jump at every step.
      const same = t.git && r.dir.startsWith(t.git.root);
      t.git = null;
      t.last = same && t.last ? {} : null;
      t.lastHead = null;
      t.sizes = {};
      t.counts = {};
    }
    const alive = new Set(r.items.map((e) => e.path));
    for (const m of [...t.marked]) if (!alive.has(m)) t.marked.delete(m);
    t.dir = r.dir;
    t.items = r.items;
    t.archive = r.archive;
    t.locked = r.locked;
    t.history = r.history;
    t.hasNotes = r.has_notes;
    t.error = null;
    // Inside an archive a folder's size comes with the listing; there is nothing to measure.
    if (r.archive) for (const e of r.items) if (e.is_dir && e.name !== "..") t.sizes[e.path] = e.size;
    const at = keep ? r.items.findIndex((e) => e.name === keep || e.path === keep) : -1;
    t.cursor = at >= 0 ? at : Math.max(0, Math.min(t.cursor, r.items.length - 1));
  } catch (e) {
    // A locked archive, looked into: its password, kept by the app for this run, then again.
    const locked = String(e).includes(LOCKED);
    if (locked) {
      ui.modal = {
        kind: "input",
        secret: true,
        title: tr("archive.locked_title"),
        label: tr("archive.locked_label"),
        value: "",
        run: (password) => invoke("archive_password", { path: dir, password }).then(() => load(t, dir, focus)),
      };
    }
    t.error = locked ? tr("archive.locked_title") : String(e);
    return false;
  }
  if (ui.autoSizes) measureFolders(t);
  invoke("git_status", { dir: t.dir }).then((g) => {
    if (t.dir === dir) t.git = g;
    if (g && !ui.recent.includes(g.root)) ui.recent = [g.root, ...ui.recent].slice(0, 12);
  }, () => {});
  // The last commit of each entry: one git walk for the folder, cached until HEAD moves, and
  // not sent again while HEAD stays. A listing sorted by commit before the walk was done
  // (it came sorted by name) is sorted now.
  if (ui.cfg.settings.git_last_commit)
    invoke("git_last", { dir: t.dir, have: t.lastHead }).then((r) => {
      if (t.dir !== dir) return;
      if (!r) return void (t.last = null);
      t.lastHead = r.head;
      if (!r.lasts) return;
      t.last = r.lasts;
      if (t.sort === "commit") {
        const at = t.items[t.cursor]?.path;
        t.items = byLast(t.items, r.lasts, t.reverse);
        t.cursor = Math.max(0, t.items.findIndex((e) => e.path === at));
      }
    }, () => {});
  return true;
}

/** `items` by their last commit, newest first (`..` first, folders before files), as the
 * backend sorts them; entries without one last. */
export function byLast(items, last, reverse) {
  const group = (e) => (e.name === ".." ? 0 : e.is_dir ? 1 : 2);
  const when = (e) => last[e.name]?.time ?? 0;
  const name = (a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0);
  return [...items].sort((a, b) => group(a) - group(b) || (reverse ? -1 : 1) * (when(b) - when(a) || name(a, b)));
}

/** Where `..` leads: the listing says (out of a history, back to the folder on disk). */
export const up = (t) => (t.items[0]?.name === ".." ? t.items[0].path : parent(t.dir));

/** Into the git history of the entry under the cursor (of the folder itself on `..`). */
export function openHistory(tb = tab()) {
  const e = item(tb);
  if (tb.archive || tb.history) return void (ui.status = tr("history.not_here"));
  if (!tb.git) return void (ui.status = tr("history.no_repo"));
  const target = e && e.name !== ".." ? e.path : tb.dir;
  return cd(tb, `${target.replace(/[\\/]$/, "")}${ui.cfg.sep}${HISTORY}`);
}

export async function cd(t, dir, history = true) {
  if (!dir || dir === t.dir) return;
  const from = t.dir;
  const prevCursor = t.cursor;
  t.cursor = 0;
  // Coming up: the cursor on the folder (or the file whose history it was) it came from.
  const pre = dir.endsWith(ui.cfg.sep) ? dir : dir + ui.cfg.sep;
  const came = from.startsWith(pre) ? pre + from.slice(pre.length).split(/[\\/]/)[0] : undefined;
  const ok = await load(t, dir, came);
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
  // A file (`coxswain-gui ~/Pictures/cat.jpg`) opens its folder with the cursor on it. A saved
  // directory may be gone; fall back to home.
  await Promise.all(
    ui.panes.flatMap((p) => p.tabs).map(async (t) => {
      if (await load(t)) return;
      const up = parent(t.dir);
      if (!(up && (await load(t, up, basename(t.dir))))) await load(t, ui.cfg.home);
    }),
  );
  invoke("places").then((p) => (ui.places = p));
  // `coxswain-gui --duplicates <folders>` starts straight in a scan of those folders; with
  // `--settings` too, the scan (which has work to do) wins, since there is one window at a time.
  if (ui.cfg.duplicates) ui.modal = { kind: "dupes", roots: Object.fromEntries(ui.cfg.duplicates.map((p) => [p, true])), autostart: true };
  else if (ui.cfg.open_settings != null) ui.modal = { kind: "settings", section: ui.cfg.open_settings };
  refreshDisks();
}

let disksBusy = false;
/** Read the drives and their free space again. One read at a time, so a slow network mount
 * cannot pile up calls; a failed read keeps the list shown, and an unchanged one is left alone. */
export function refreshDisks() {
  if (disksBusy) return;
  disksBusy = true;
  invoke("disks")
    .then((d) => {
      if (JSON.stringify(d) !== JSON.stringify($state.snapshot(ui.disks))) ui.disks = d;
    }, () => {})
    .finally(() => (disksBusy = false));
}

/** Enter / double-click: folders open in place, files in their default application. */
export function openItem(tb, i = tb.cursor) {
  const e = tb.items[i];
  if (!e) return;
  // An archive opens like a folder; its files are copied out with F5. An archive inside one is
  // a file like the others.
  if (e.is_dir || (isArchive(e.name) && !tb.archive)) return cd(tb, e.path);
  if (tb.archive) return void (ui.status = t("archive.copy_out_hint", { archive: basename(tb.archive) }));
  if (tb.history) return void (ui.status = t("history.file_hint"));
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
  const names = { type: t("app.col.type"), size: t("app.col.size"), files: t("app.col.files"), modified: t("app.col.modified"), ...(ui.cfg.settings.git_last_commit ? { commit: t("app.col.commit") } : {}), created: t("app.col.created") };
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

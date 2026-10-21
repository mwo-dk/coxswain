// Shared app state and navigation. Components read `ui` and call these functions.

import { SvelteSet } from "svelte/reactivity";
import { invoke, listDir, ranks, basename, parent, applyTheme, isArchive, chessFormat, setArchiveEndings, setDiskEndings, setPictureEndings, setMediaEndings, isMedia, isEditable, setGeoEndings, isGeo, isScad, diskName, diskSure, diskSeen, sortItems, HISTORY, ALL, BRANCHES, WORKTREES, SNAPSHOTS, PACKAGE, LOCKED } from "./lib.js";
import { setLanguage, t } from "./i18n.svelte.js";
import { failure } from "./errors.js";
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
  /** A *Make PDF searchable* run under way: which PDF of how many, and what it said last. */
  searchable: null,
  /** A *Convert video or audio* or *Convert or edit pictures* run under way, as `searchable`,
   *  with the action that stops it. */
  media: null,
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
  /** A game to show on the board when its PGN file is previewed (a Find hit): { path, index }. */
  chessAt: null,
  /** A Find hit's game in an SGF file: { path, index }, for GoView. */
  goAt: null,
  /** The last .bin file the preview found to be a Polyglot book, for the action menu. */
  book: "",
  /** Settings → What's new: the notices not dismissed, and the versions not read. */
  news: { notices: [], unread: [] },
  favorites: [],
  recent: [],
  /** The ending Pack suggests: the last one packed. */
  packEnding: ".zip",
  places: [],
  /** FreeBSD's boot environments and jails, for the sidebar. */
  bsdPlaces: [],
  clouds: [],
  disks: [],
  /** What Ctrl+Z would undo ("" for nothing), and how many operations the history has kept. */
  undo: "",
  undoSeq: 0,
  /** A repository's folder -> its git facts, one a line (null when it is none): `gitFacts`. */
  facts: {},
});

const factsAt = new Map();
/** Read the git facts of `dir`, a repository's own folder, for a tooltip: when hovered, and
 *  again only after ten seconds. Nothing is read per listing. */
export function gitFacts(dir) {
  if (Date.now() - (factsAt.get(dir) ?? 0) < 10000) return;
  // Kept for the folders hovered lately only: past 200, all are forgotten and read again.
  if (factsAt.size >= 200) {
    factsAt.clear();
    ui.facts = {};
  }
  factsAt.set(dir, Date.now());
  invoke("git_facts", { dir }).then((f) => (ui.facts[dir] = f), () => {});
}

/** Read what Ctrl+Z would undo. After an operation that went well, `ok` is said on the status
 *  line, with how to undo it when it can be. */
export async function refreshUndo(ok) {
  const [seq, label] = await invoke("undo_next").catch(() => [ui.undoSeq, ui.undo]);
  const fresh = seq !== ui.undoSeq;
  ui.undoSeq = seq;
  ui.undo = label ?? "";
  if (ok !== undefined) ui.status = fresh ? `${ok} · ${t("undo.hint", { key: ui.cfg.actions.undo?.[1] ?? "" })}` : ok;
}

/** An action's name in F1 and F9: Undo says what it would undo. */
export const actionLabel = (name) => (name === "undo" && ui.undo ? t("undo.menu", { what: ui.undo }) : (ui.cfg.actions[name]?.[0] ?? name));

/** The keys of an action, its first default key first, a letter key written as on the
 *  keyboard: Ctrl+G, not the keymap's Ctrl+g. */
export function keysOf(name) {
  const first = ui.cfg.actions[name]?.[1];
  return Object.entries(ui.cfg.keymap)
    .filter(([, a]) => a === name)
    .map(([k]) => k)
    .sort((a, b) => (b === first) - (a === first))
    .map((k) => k.replace(/\+([a-z])$/, (_, c) => "+" + c.toUpperCase()));
}

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

/** A new tab in pane `p` at its tab's folder and view, made the active one. */
export function addTab(p) {
  const t = p.tabs[p.active];
  p.tabs.push(newTab(t.dir, t.view));
  p.active = p.tabs.length - 1;
  load(p.tabs[p.active]);
}

/** Close tab `i` of pane `p`, unless it is the last; the active tab stays the one it was. */
export function closeTab(p, i = p.active) {
  if (p.tabs.length === 1) return;
  p.tabs.splice(i, 1);
  if (i < p.active) p.active--;
  else p.active = Math.min(p.active, p.tabs.length - 1);
}

/** Find, at a kind: `all` (Ctrl+F), `in_files` (Shift+F7) or `ask` (Ctrl+F7). */
export function openFind(chip) {
  ui.modal = { kind: "search", query: "", chip, here: false, show: chip === "ask" ? "answer" : "list", out: null, cursor: 0, scopeHint: false };
  // The first few times: how to limit Find to the pane's folder.
  const m = ui.modal;
  invoke("hint_once", { id: "find_scope" }).then((b) => (m.scopeHint = b), () => {});
}

export const pane = (i = ui.activePane) => ui.panes[i];
export const tab = (i = ui.activePane) => ui.panes[i]?.tabs[ui.panes[i].active];
export const otherTab = () => tab(ui.dual ? ui.activePane ^ 1 : ui.activePane);
export const item = (t = tab()) => t?.items[t.cursor];

/** The built-in themes' ids, in their order, made once per `ui.cfg.builtin_themes`. */
let builtin = { of: null, ids: [], set: new Set() };
const builtinIds = () => {
  if (builtin.of !== ui.cfg.builtin_themes) {
    const ids = ui.cfg.builtin_themes.map(([id]) => id);
    builtin = { of: ui.cfg.builtin_themes, ids, set: new Set(ids) };
  }
  return builtin;
};
/** Every theme's id: the built-in ones in their order, then the user's `[themes.<name>]`. */
export const themeIds = () => {
  const { ids, set } = builtinIds();
  return [...ids, ...Object.keys(ui.cfg.themes).filter((id) => !set.has(id))];
};
/** A theme's name as shown: translated when built in, else as the user named it. */
export const themeName = (id) => (builtinIds().set.has(id) ? t(`theme.${id}`) : id);

/** Show a theme; with `save`, also store it in config.toml, as Settings does. */
export async function setTheme(name, save = false) {
  if (!ui.cfg.themes[name]) name = "cyber";
  ui.theme = name;
  applyTheme(ui.cfg.themes[name], ui.cfg.gui, ui.cfg.looks[name], ui.cfg.crt?.[name]);
  if (!save) return;
  try {
    ui.cfg = await invoke("save_settings", { changes: { theme: name } });
  } catch (e) {
    ui.status = String(e);
  }
}

/** Each tab's latest listing asked for: an answer to an older one is dropped. */
const listing = new WeakMap();

/** Re-read a tab, keeping the cursor on `focus` (default: the current name). True when listed,
 *  false when it failed, undefined when a later load of the tab overtook it. */
export async function load(t, dir = t.dir, focus) {
  const keep = focus ?? item(t)?.name;
  const seq = (listing.get(t) ?? 0) + 1;
  listing.set(t, seq);
  const stale = () => listing.get(t) !== seq;
  // A slow disk: after a moment the status line says what is being read.
  const slow = setTimeout(() => (ui.status = tr("status.busy", { what: basename(dir) || dir })), 150);
  try {
    const r = await listDir({ dir, showHidden: ui.showHidden, sort: t.sort, reverse: t.reverse }).finally(() => {
      clearTimeout(slow);
      if (ui.status === tr("status.busy", { what: basename(dir) || dir })) ui.status = "";
    });
    // A slow folder answering after a later cd would show the wrong folder.
    if (stale()) return undefined;
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
    t.diskImage = r.disk_image;
    t.image = r.image;
    t.history = r.history;
    t.snapshot = r.snapshot;
    t.package = r.package;
    t.packages = r.packages;
    // In a model store's folder: the model each entry is, shown beside its name.
    t.models = r.models ?? null;
    t.hasNotes = r.has_notes;
    t.error = null;
    // Inside an archive a folder's size comes with the listing; there is nothing to measure. A
    // history's folders have none (git keeps no size for a tree): left empty, not "0 B".
    if (r.archive && !r.history) for (const e of r.items) if (e.is_dir && e.name !== "..") t.sizes[e.path] = e.size;
    // A list of snapshots: each one's own space.
    if (r.snapshot && !r.snapshot.snapshot) for (const e of r.items) if (e.name !== "..") t.sizes[e.path] = e.size;
    const at = keep ? r.items.findIndex((e) => e.name === keep || e.path === keep) : -1;
    t.cursor = at >= 0 ? at : Math.max(0, Math.min(t.cursor, r.items.length - 1));
  } catch (e) {
    if (stale()) return undefined;
    // A locked archive, looked into: its password, kept by the app for this run, then again.
    const locked = String(e).includes(LOCKED);
    if (locked) {
      ui.modal = {
        kind: "input",
        feature: "archive_passwords",
        secret: true,
        title: tr("archive.locked_title"),
        label: tr("archive.locked_label"),
        value: "",
        ok: tr("verb.unlock"),
        run: (password) => invoke("archive_password", { path: dir, password }).then(() => (dir === t.dir ? load(t, dir, focus) : cd(t, dir))),
      };
    }
    t.error = locked ? tr("archive.locked_title") : String(e);
    return false;
  }
  resortSoon(t);
  if (ui.autoSizes) measureFolders(t);
  // On ZFS: the dataset, its compression and space, for the footer.
  invoke("zfs_facts", { dir: t.dir }).then((z) => t.dir === dir && (t.zfs = z), () => {});
  invoke("git_status", { dir: t.dir }).then((g) => {
    if (t.dir === dir) t.git = g;
    if (g) ui.recent = g.recent;
  }, () => {});
  // The last commit of each entry: one git walk for the folder, cached until HEAD moves, and
  // not sent again while HEAD stays. A listing sorted by commit before the walk was done
  // (it came sorted by name) is sorted now.
  if (ui.cfg.settings.git_last_commit)
    invoke("git_last", { dir: t.dir, have: t.lastHead }).then(async (r) => {
      if (t.dir !== dir) return;
      if (!r) return void (t.last = null);
      t.lastHead = r.head;
      if (!r.lasts) return;
      t.last = r.lasts;
      if (t.sort === "commit") {
        const items = t.items;
        const rank = await ranks(items.map((e) => e.name));
        if (t.dir !== dir || t.items !== items || t.sort !== "commit") return;
        const at = t.items[t.cursor]?.path;
        t.items = byLast(t.items, r.lasts, t.reverse, rank);
        t.cursor = Math.max(0, t.items.findIndex((e) => e.path === at));
      }
    }, () => {});
  return true;
}

/** `items` by their last commit, newest first (`..` first, folders before files), as the
 * backend sorts them; entries without one last, and alike by name (`rank`, from `ranks`). */
export function byLast(items, last, reverse, rank) {
  const group = (e) => (e.name === ".." ? 0 : e.is_dir ? 1 : 2);
  const when = (e) => last[e.name]?.time ?? 0;
  const name = (a, b) => rank(a.name) - rank(b.name);
  return [...items].sort((a, b) => group(a) - group(b) || (reverse ? -1 : 1) * (when(b) - when(a) || name(a, b)));
}

/** Where `..` leads: the listing says (out of a history, back to the folder on disk). */
export const up = (t) => (t.items[0]?.name === ".." ? t.items[0].path : parent(t.dir));

/** Into the git history of the entry under the cursor (of the folder itself on `..`), with
 *  `all` every branch's commits. In a history, `all` goes over to the other branches' list. */
export function openHistory(tb = tab(), all = false) {
  const e = item(tb);
  const s = ui.cfg.sep;
  if (all && tb.history) {
    const h = tb.history;
    if (h.view !== "history") return void (ui.status = tr("history.all_not_here"));
    return cd(tb, `${h.target.replace(/[\\/]$/, "")}${s}${HISTORY}${h.scope === "all" ? "" : s + ALL}`);
  }
  if (tb.archive || tb.history) return void (ui.status = tr("history.not_here"));
  if (!tb.git) return void (ui.status = tr("history.no_repo"));
  const target = e && e.name !== ".." ? e.path : tb.dir;
  return cd(tb, `${target.replace(/[\\/]$/, "")}${s}${HISTORY}${all ? s + ALL : ""}`);
}

/** To the ZFS snapshots of the folder under the cursor (of the tab's folder on `..` or a file).
 *  Inside a snapshot: back to the list. */
export function openSnapshots(tb = tab()) {
  const e = item(tb);
  if (tb.snapshot?.snapshot) return cd(tb, `${tb.snapshot.live.replace(/[\\/]$/, "")}${ui.cfg.sep}${SNAPSHOTS}`);
  if (tb.archive || tb.history || tb.snapshot || tb.package) return void (ui.status = tr("zfs.not_here"));
  if (!tb.zfs) return void (ui.status = tr("zfs.not_zfs", { dir: tb.dir }));
  const target = e && e.name !== ".." && e.is_dir ? e.path : tb.dir;
  return cd(tb, `${target.replace(/[\\/]$/, "")}${ui.cfg.sep}${SNAPSHOTS}`);
}

/** To the files of the FreeBSD package the file under the cursor belongs to. */
export function openPackage(tb = tab(), path = item(tb)?.is_dir ? null : item(tb)?.path) {
  if (!path || tb.archive || tb.history || tb.snapshot || tb.package) return void (ui.status = tr("pkg.not_here"));
  return cd(tb, `${path}${ui.cfg.sep}${PACKAGE}`);
}

/** To the branches (or the worktrees) of the repository the tab's folder is in. */
export function openGitView(worktrees = false, tb = tab()) {
  // From the branches to the worktrees (and back), and inside a branch.
  const listed = tb.history && tb.history.view !== "history" ? tb.history.base : null;
  if (!listed && (tb.archive || tb.history)) return void (ui.status = tr("branches.not_here"));
  if (!listed && !tb.git) return void (ui.status = tr("branches.no_repo"));
  return cd(tb, `${(listed ?? tb.git.root).replace(/[\\/]$/, "")}${ui.cfg.sep}${worktrees ? WORKTREES : BRANCHES}`);
}

/** In the list of branches: the tab's folder is it. */
const inBranches = (tb) => tb.history?.view === "branches" && !tb.history.commit;
/** The branch an entry of the list is called (`branches::name_of`). */
const branchOf = (e) => invoke("branch_name", { entry: e.name });

/** What git said: on the status line, or, when it refused, in a message under `title` (what
 *  could not be done), the cause first and git's words under Details. */
async function gitSaid(title, call) {
  try {
    ui.status = (await call).split("\n")[0];
  } catch (err) {
    ui.modal = failure(title, err);
  }
  reloadAll();
}

/** Switch to the branch under the cursor in the list of branches, once confirmed. */
export async function switchBranch(tb = tab()) {
  const e = item(tb);
  const key = ui.cfg.actions.branches?.[1] ?? "";
  if (!inBranches(tb) || !e || e.name === "..") return void (ui.status = tr("branches.switch_where", { key }));
  const title = tr("action.switch_branch");
  const dir = tb.dir;
  const branch = await branchOf(e);
  ui.modal = {
    kind: "confirm",
    feature: "git_branches",
    title,
    text: tr("branches.switch_text", { branch, repo: basename(tb.history.base) }),
    ok: tr("verb.switch"),
    run: () => gitSaid(tr("error.switch", { branch }), invoke("git_switch", { dir, entry: e.name })),
  };
}

/** A new branch, named in a prompt: from the branch under the cursor in the list of branches,
 * or from the current commit. */
export async function newBranch(tb = tab()) {
  const e = item(tb);
  const from = inBranches(tb) && e && e.name !== ".." ? e.name : null;
  if (!from && (!tb.git || tb.history)) return void (ui.status = tr("branches.no_repo"));
  const title = tr("action.new_branch");
  const dir = tb.dir;
  const branch = from && (await branchOf(e));
  ui.modal = {
    kind: "input",
    feature: "git_branches",
    title,
    label: from ? tr("branches.new_from", { branch }) : tr("branches.new_here"),
    value: "",
    ok: tr("verb.create"),
    run: (name) => name.trim() && gitSaid(tr("error.new_branch", { name }), invoke("git_new_branch", { dir, name, from })),
  };
}

/** Commit, push, pull and the rest: the repository the tab's folder is in, in the user's own
 *  git tool (`[git] tool`, else the first one installed, else a terminal there). */
export async function gitTool(tb = tab()) {
  try {
    ui.status = await invoke("git_tool", { dir: tb.dir });
  } catch (err) {
    ui.status = String(err);
  }
  reloadAll();
}

/** The git config as git uses it in the tab's folder (outside a repository: system and global),
 *  read now, in a dialog: each value with its scope and file, secrets hidden until Show. */
export async function openGitConfig(tb = tab()) {
  try {
    const [repo, entries, hint] = await invoke("git_config", { dir: tb.dir });
    ui.modal = { kind: "gitconfig", feature: "git_config", repo, entries, hint, shown: {}, cursor: 0 };
  } catch (err) {
    ui.status = String(err);
  }
}

/** *Make PDF searchable* (`coxswain_core::searchable`): the PDFs among `paths`, asked first
 *  (what it writes, which program runs, the first pull and its size), then run, the status line
 *  saying how far it is with a Stop button (or the key again). One run at a time. */
export async function makeSearchable(paths = targets()) {
  // Its key again while it runs stops it, as the status line says.
  if (ui.searchable) return stopSearchable();
  let plan;
  try {
    plan = await invoke("searchable_plan", { paths });
  } catch (err) {
    return void (ui.status = String(err));
  }
  if (!plan.pdfs.length) return void (ui.status = t("searchable.none"));
  const title = t("action.searchable_pdf");
  // Nothing to run it with: what to install or turn on, and Settings at the place for it.
  if (plan.missing) return void (ui.modal = { kind: "message", title, text: plan.missing, settings: ui.cfg.settings.preview_container === "off" ? "preview_container" : "previews", feature: "searchable_pdf" });
  const what = plan.pdfs.length === 1 ? t("searchable.confirm_one", { name: basename(plan.pdfs[0]), target: plan.targets[0] }) : t("searchable.confirm_many", { n: plan.pdfs.length });
  ui.modal = { kind: "confirm", title, text: `${what} ${plan.about}`, ok: t("searchable.go"), safe: true, run: () => runSearchable(plan.pdfs), feature: "searchable_pdf" };
}

async function runSearchable(pdfs) {
  ui.searchable = { at: 0, all: pdfs.length, name: basename(pdfs[0]), line: "", started: Date.now(), now: Date.now(), stopping: false };
  const poll = setInterval(async () => {
    const p = await invoke("searchable_progress").catch(() => null);
    if (p && ui.searchable) Object.assign(ui.searchable, p, { now: Date.now() });
  }, 500);
  try {
    const made = await invoke("searchable_run", { paths: pdfs });
    ui.status = made.length === 1 ? t("searchable.done_one", { name: basename(made[0]) }) : t("searchable.done_many", { n: made.length });
  } catch (err) {
    if (ui.searchable?.stopping) ui.status = t("searchable.stopped");
    else ui.modal = failure(t("searchable.failed"), err);
  } finally {
    clearInterval(poll);
    ui.searchable = null;
    await reloadAll();
  }
}

/** Stop the *Make PDF searchable* run under way. */
export function stopSearchable() {
  if (!ui.searchable) return;
  ui.searchable.stopping = true;
  invoke("searchable_stop");
}

/** *Convert video or audio* (`program` 0, ffmpeg), *Convert or edit pictures* (1, ImageMagick),
 *  *Convert geodata* (2, GDAL) or *Export 3D model* (3, OpenSCAD) on the files among `paths` it takes (`coxswain_core::media`): what to make of them from a
 *  menu, a clip's times or a size asked next, then the question (what it writes, which program
 *  runs, the first pull and its size), then the run, the status line saying how far it is with
 *  a Stop button. The action again while it runs stops it. */
export function convertMedia(program, paths = targets()) {
  const action = ["convert_media", "edit_pictures", "convert_geodata", "export_model"][program];
  const none = () => (ui.status = t(["media.none_media", "media.none_pictures", "geo.none", "scad.none"][program]));
  if (ui.media) return stopMedia();
  const files = paths.filter((p) => [isMedia, isEditable, isGeo, isScad][program](basename(p)));
  if (!files.length) return void none();
  const label = (id) => {
    const [base, value] = id.split(":");
    return base === "geo" ? t(`geo.to.${value}`) : base === "stl" || base === "3mf" ? t(`scad.op.${base}`) : t(`media.op.${base}`, { degrees: value });
  };
  const choose = (id) => {
    const [base] = id.split(":");
    if (base === "clip") return void (ui.modal = { kind: "input", feature: action, title: label(id), label: t("media.clip_label"), value: "0:00 0:30", ok: t("common.apply"), run: (v) => clip(v) });
    if (base === "resize") return void (ui.modal = { kind: "input", feature: action, title: label(id), label: t("media.resize_label"), value: id.split(":")[1], ok: t("common.apply"), run: (v) => (/^\s*\d+\s*$/.test(v) ? ask(`resize:${v.trim()}`, label(id)) : (ui.status = t("media.resize_bad"))) });
    ask(id, label(id));
  };
  // `1:30 0:45`: the start and the length, as seconds, m:ss or h:mm:ss (`media::clip`).
  const clip = (v) => {
    const secs = (s) => (/^\d+(:\d+){0,2}$/.test(s) ? s.split(":").reduce((a, p) => a * 60 + Number(p), 0) : NaN);
    const [a, b, more] = v.trim().split(/\s+/);
    const [start, length] = [secs(a ?? ""), secs(b ?? "")];
    if (more !== undefined || !(start >= 0) || !(length > 0)) return void (ui.status = t("media.clip_bad"));
    ask(`clip:${start}:${length}`, label("clip"));
  };
  const ask = async (op, title) => {
    let plan;
    try {
      plan = await invoke("media_plan", { paths: files, op });
    } catch (err) {
      return void (ui.status = String(err));
    }
    if (!plan.files.length) return void none();
    if (plan.missing) return void (ui.modal = { kind: "message", title, text: plan.missing, settings: ui.cfg.settings.preview_container === "off" ? "preview_container" : "previews", feature: action });
    ui.modal = { kind: "confirm", title, text: plan.about, ok: t("media.go"), safe: true, run: () => runMedia(plan.files, op, action), feature: action };
  };
  ui.modal = {
    kind: "menu",
    feature: action,
    title: actionLabel(action),
    direct: true,
    filter: "",
    cursor: 0,
    items: ui.cfg.media_ops[program].map((id, i) => ({ key: String(i + 1), label: label(id), run: () => choose(id) })),
  };
}

async function runMedia(files, op, action) {
  ui.media = { at: 0, all: files.length, name: basename(files[0]), line: "", started: Date.now(), now: Date.now(), stopping: false, action };
  const poll = setInterval(async () => {
    const p = await invoke("media_progress").catch(() => null);
    if (p && ui.media) Object.assign(ui.media, p, { now: Date.now() });
  }, 500);
  try {
    const made = await invoke("media_run", { paths: files, op });
    ui.status = made.length === 1 ? t("media.done_one", { name: basename(made[0]) }) : t("media.done_many", { n: made.length });
  } catch (err) {
    if (ui.media?.stopping) ui.status = t("media.stopped");
    else ui.modal = failure(t("media.failed"), err);
  } finally {
    clearInterval(poll);
    ui.media = null;
    await reloadAll();
  }
}

/** Stop the conversion under way. */
export function stopMedia() {
  if (!ui.media) return;
  ui.media.stopping = true;
  invoke("media_stop");
}

/** Space on `dir` (default: the folder under the cursor, else the tab's): where the space goes,
 *  full window. Only folders on a disk. */
export function openSpace(tb = tab(), dir) {
  if (tb.archive || tb.history || tb.snapshot || tb.package) return void (ui.status = t("space.not_here"));
  const e = item(tb);
  const folder = dir ?? (e?.is_dir && e.name !== ".." && !tb.marked.size ? e.path : tb.dir);
  ui.modal = { kind: "space", feature: "space", root: folder };
}

/** The git line's menu (a right-click on it): the repository's branches and worktrees, and
 *  config, and the handoff to the user's git tool, each with its key. */
export function gitMenu(tb = tab()) {
  const entry = (n, run) => ({ key: ui.cfg.actions[n]?.[1] ?? "", label: actionLabel(n), run });
  ui.modal = {
    kind: "menu",
    feature: "git",
    title: tb.git?.prompt ?? t("preview.git"),
    direct: false,
    filter: "",
    cursor: 0,
    items: [entry("branches", () => openGitView(false, tb)), entry("worktrees", () => openGitView(true, tb)), entry("git_config", () => openGitConfig(tb)), entry("git_tool", () => gitTool(tb))],
  };
}

/** Tab `tb` shows `path`: a folder opened, a file in its folder with the cursor on it. */
export async function showInTab(tb, path, isDir = false) {
  await cd(tb, isDir ? path : parent(path));
  if (isDir) return;
  const i = tb.items.findIndex((e) => e.path === path);
  if (i >= 0) tb.cursor = i;
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
  if (ok === undefined) return;
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
  // In an archive the listing has the sizes; in a history there are none to measure. A list of
  // snapshots has theirs, and a package's files are files.
  if (t.archive || t.history || (t.snapshot && !t.snapshot.snapshot) || t.package) return;
  // A reload while the automatic run is going (the watcher does that) must not start a second one.
  if (!paths && measuring.get(t) === dir) return;
  if (!paths) measuring.set(t, dir);
  try {
    // The folder above only when its size is already known (measured lately, or in the store):
    // never walked for this.
    const up = !paths && t.items.find((e) => e.name === "..");
    if (up && t.sizes[up.path] === undefined) {
      const r = await invoke("dir_size_known", { path: up.path });
      if (t.dir !== dir) return;
      if (r) [t.sizes[up.path], t.counts[up.path]] = r;
    }
    const todo = paths ?? t.items.filter((e) => e.is_dir && e.name !== ".." && t.sizes[e.path] === undefined).map((e) => e.path);
    for (const p of todo) {
      if (t.dir !== dir) return;
      // The automatic run is this tab's own: a new one stops it, and recent sizes are reused.
      const r = (await invoke("dir_sizes", { paths: [p], tab: paths ? null : tabId(t) }))[p];
      if (t.dir !== dir) return;
      if (!r) continue;
      t.sizes[p] = r[0];
      t.counts[p] = r[1];
      resortSoon(t);
    }
  } finally {
    if (!paths && measuring.get(t) === dir) measuring.delete(t);
  }
}

/** `ranks` of a tab's names, asked once while its listing keeps the same names. */
const ranked = new WeakMap();
async function tabRanks(t) {
  const had = ranked.get(t);
  if (had && t.items.every((e) => had.has(e.name))) return had;
  const rank = await ranks(t.items.map((e) => e.name));
  ranked.set(t, rank);
  return rank;
}

/** Sorted by size: the folders measured since the listing (which sorted them by name) go to
 *  their place, the cursor on the same entry. A quarter second after the first arrival, so a
 *  folder of thousands is sorted four times a second at most. */
const resorting = new WeakSet();
export function resortSoon(t) {
  if (t.sort !== "size" || resorting.has(t)) return;
  resorting.add(t);
  setTimeout(async () => {
    resorting.delete(t);
    // A history, a list of snapshots, an archive or a package keeps the backend's order.
    if (t.sort !== "size" || t.archive || t.history || (t.snapshot && !t.snapshot.snapshot) || t.package) return;
    const items = t.items;
    const rank = await tabRanks(t);
    if (t.items !== items || t.sort !== "size") return;
    const at = t.items[t.cursor]?.path;
    t.items = sortItems(t.items, "size", t.reverse, t.sizes, rank);
    t.cursor = Math.max(0, t.items.findIndex((e) => e.path === at));
  }, 250);
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
  setArchiveEndings(ui.cfg.archive_endings);
  setDiskEndings(ui.cfg.disk_endings);
  setPictureEndings(ui.cfg.picture_endings);
  setMediaEndings(ui.cfg.media_endings);
  setGeoEndings(ui.cfg.geo_endings);
  const st = await invoke("get_state");
  const s = st.session ?? {};
  ui.favorites = st.favorites;
  ui.recent = st.recent_repos;
  if (ui.cfg.pack_formats.some((f) => f.endings[0] === st.pack_ending)) ui.packEnding = st.pack_ending;
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
      const ok = await load(t);
      if (ok !== false) return;
      const up = parent(t.dir);
      const above = up ? await load(t, up, basename(t.dir)) : false;
      if (above === false) await load(t, ui.cfg.home);
    }),
  );
  invoke("places").then((p) => (ui.places = p));
  invoke("bsd_places").then((p) => (ui.bsdPlaces = p), () => {});
  // `coxswain-gui --duplicates <folders>` starts straight in a scan of those folders; with
  // `--settings` too, the scan (which has work to do) wins, since there is one window at a time.
  if (ui.cfg.duplicates) ui.modal = { kind: "dupes", roots: Object.fromEntries(ui.cfg.duplicates.map((p) => [p, true])), autostart: true };
  else if (ui.cfg.open_settings != null) ui.modal = { kind: "settings", section: ui.cfg.open_settings };
  // The very first start: the first-run guide.
  else if (ui.cfg.guide) ui.modal = { kind: "guide", step: 0 };
  refreshDisks();
}

let disksBusy = false;
/** Read the drives and their free space again, and the clouds. One read at a time, so a slow
 * network mount cannot pile up calls; a failed read keeps the list shown, and an unchanged one is
 * left alone. */
export function refreshDisks() {
  if (disksBusy) return;
  disksBusy = true;
  invoke("disks")
    .then((d) => {
      if (JSON.stringify(d) !== JSON.stringify($state.snapshot(ui.disks))) ui.disks = d;
    }, () => {})
    .then(() => invoke("clouds"))
    .then((c) => {
      if (c && JSON.stringify(c) !== JSON.stringify($state.snapshot(ui.clouds))) ui.clouds = c;
    }, () => {})
    .finally(() => (disksBusy = false));
}

/** An OCI image layout's index.json in `tb`: it opens as the image (`container_image::layout_index`). */
export const ociIndex = (tb, e) => !!e && !e.is_dir && e.name === "index.json" && !tb.archive && tb.items.some((x) => x.name === "oci-layout" && !x.is_dir);

/** Enter / double-click: folders open in place, files in their default application. */
export function openItem(tb, i = tb.cursor) {
  const e = tb.items[i];
  if (!e) return;
  // An archive opens like a folder; its files are copied out with F5. An archive inside one is
  // a file like the others.
  if (e.is_dir || (isArchive(e.name) && !tb.archive) || ociIndex(tb, e)) return cd(tb, e.path);
  if (tb.archive) return void (ui.status = t(tb.diskImage ? "disk_image.copy_out_hint" : "archive.copy_out_hint", { archive: basename(tb.archive) }));
  // A disk image opens like a folder while that option is on (and a .vhd only when it is one).
  if (ui.cfg.settings.disk_images && diskName(e.name)) {
    return invoke("opens_as_folder", { path: e.path }).then((yes) => (yes ? cd(tb, e.path) : openFile(tb, e)), (err) => (ui.status = String(err)));
  }
  openFile(tb, e);
}

/** A file opened as it is: in its default application, or its place said. */
function openFile(tb, e) {
  if (tb.history) return void (ui.status = t("history.file_hint"));
  // A file of a package: to its folder, the cursor on it.
  if (tb.package) return cd(tb, parent(e.path)).then(() => (tb.cursor = Math.max(0, tb.items.findIndex((x) => x.path === e.path || x.name === basename(e.path)))));
  if (tb.snapshot) return void (ui.status = t("zfs.file_hint"));
  invoke("open_path", { path: e.path }).then(
    () => (ui.status = t("status.opened", { name: e.name })),
    (err) => (ui.status = String(err)),
  );
}

/** What the action menu and the hints are about: the marked entries (or the one under the
 *  cursor, none on `..`), and where they are; `menu::Subject` in the core. */
export function subject(tb = tab()) {
  const e = item(tb);
  const cur = e && e.name !== ".." ? e : null;
  const list = tb.marked.size ? tb.items.filter((x) => tb.marked.has(x.path)) : cur ? [cur] : [];
  const h = tb.history;
  const place = tb.archive ? (tb.diskImage ? "disk_image" : "archive") : h ? (h.view === "branches" && !h.commit ? "branches" : "history") : tb.snapshot ? "snapshot" : tb.package ? "package" : "disk";
  const files = list.filter((x) => !x.is_dir);
  return {
    files: files.length,
    folders: list.length - files.length,
    archives: files.filter((x) => isArchive(x.name) || ociIndex(tb, x) || (ui.cfg.settings.disk_images && (diskSure(x.name) || diskSeen.has(x.path)))).length,
    disk_images: ui.cfg.settings.disk_images ? 0 : files.filter((x) => diskSure(x.name) || diskSeen.has(x.path)).length,
    pdfs: files.filter((x) => /\.pdf$/i.test(x.name)).length,
    chess: files.filter((x) => chessFormat(x.name) || x.path === ui.book).length,
    media: files.filter((x) => isMedia(x.name)).length,
    pictures: files.filter((x) => isEditable(x.name)).length,
    // By name: a TIFF, geodata only when it is a GeoTIFF, offers it in its preview instead.
    geo: files.filter((x) => isGeo(x.name) && !/\.tiff?$/i.test(x.name)).length,
    models: files.filter((x) => isScad(x.name)).length,
    images: files.filter((x) => ociIndex(tb, x)).length,
    // `local_models::is_model`: a model file by its ending, or a blob of a model store.
    local_models: files.filter((x) => /\.(gguf|safetensors|pt|pth|ckpt)$|\.safetensors\.index\.json$|^pytorch_model.*\.bin$/i.test(x.name) || (tb.models?.[x.name] && /^sha256-/.test(x.name))).length,
    one: list.length === 1 && list[0] === cur,
    marked: tb.marked.size,
    place,
    git: !!tb.git,
    zfs: !!tb.zfs,
    packages: !!tb.packages,
    // A drive's top, or a folder measured at 1 GB or more: where Space is worth a hint.
    big: place === "disk" && (cur?.is_dir && list.length === 1 ? (tb.sizes[cur.path] ?? 0) >= 2 ** 30 || ui.disks.some((d) => d.mount === cur.path) : ui.disks.some((d) => d.mount === tb.dir)),
  };
}

/** Set by App.svelte, which runs the actions: opens the action menu. */
export const hooks = { actionMenu: () => {} };

/** A right-click on row `i`: marks it, as in Norton Commander, or, with Settings → Behaviour →
 *  Right-click on the menu, puts the cursor on it and opens the action menu. Ctrl+right-click
 *  does the other one. */
export function rowContext(ev, t, i, onfocus) {
  ev.preventDefault();
  onfocus();
  t.cursor = i;
  if ((ui.cfg.settings.right_click === "menu") !== (ev.ctrlKey || ev.metaKey)) hooks.actionMenu();
  else toggleMark(t, i);
}

/** The ⋯ button on a row: the action menu on that row. */
export function rowMenu(ev, t, i, onfocus) {
  ev.stopPropagation();
  onfocus();
  t.cursor = i;
  hooks.actionMenu();
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

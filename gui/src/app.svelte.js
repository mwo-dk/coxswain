// Shared app state and navigation. Components read `ui` and call these functions.

import { Paged, SPAN } from "./paged.svelte.js";
import { invoke, size, basename, parent, applyTheme, isArchive, chessFormat, isDrawio, setArchiveEndings, setDiskEndings, setPictureEndings, setMediaEndings, isMedia, isEditable, setGeoEndings, isGeo, isScad, CONVERTER, previewKind, isKicad, isSums, isFirstPart, diskSure, diskSeen, HISTORY, ALL, UPSTREAM, BRANCHES, WORKTREES, SNAPSHOTS, PACKAGE, LOCKED } from "./lib.js";
import { setLanguage, t, tn } from "./i18n.svelte.js";
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
  /** A log to follow once the preview pane shows it (the action Follow log). */
  followLog: "",
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
  /** A *Convert e-book* run under way: which book of how many, and what calibre said last. */
  ebook: null,
  /** Checksums being made or checked (`checksum`): the action, the status line, Stop asked. */
  work: null,
  /** The file operation under way (copy, move, delete, extract, pack), for its progress bar:
   *  the item it is on of how many, its name, how many wait behind it, and whether Stop was
   *  pressed. */
  job: null,
  /** A *Build PDF of AsciiDoc* or *Export draw.io diagram* run under way, as `searchable`,
   *  with the action that stops it. */
  exporting: null,
  /** A *Scan for viruses* run under way: what it said last, since when. */
  viruses: null,
  quick: null,
  cmd: "",
  lastOutput: "",
  /** Pane under a file being dragged in, for the highlight, and the folder row or sidebar
   *  place under it when the drop would go into that. */
  dropPane: null,
  dropDir: null,
  /** The last drag out of a pane: the tab, its paths there, and the paths dragged (copies out
   *  of an archive or a history), so a drop back on a pane copies from where they really are. */
  drag: null,
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
  /** The preview shows who changed each line of the file (`blame`). */
  previewBlame: false,
  /** Tool -> the engine id picked in the preview (local program or container). */
  previewEngine: {},
  /** Counts each change of a preview tool's choice (`[preview.tools]`): previews made by a tool
   *  are looked up again. */
  toolsChanged: 0,
  /** One modal at a time: { kind, ... } */
  modal: null,
  /** A game to show on the board when its PGN file is previewed (a Find hit): { path, index }. */
  chessAt: null,
  /** A Find hit's game in an SGF file: { path, index }, for GoView. */
  goAt: null,
  /** The last file the preview found to be games by its bytes (a Polyglot book, a DhtmlXQ
   *  xiangqi game), for the action menu. */
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

let jobs = 0;
let jobPoll = null;

/** A file operation starts: its progress bar shows while it (or another) runs, in place of
 *  `busy` on the status line. Call what it gives back when it has ended. */
export function watchJob(busy) {
  jobs += 1;
  jobPoll ??= setInterval(async () => {
    const p = await invoke("job_progress").catch(() => null);
    if (p && !ui.job && ui.status === busy) ui.status = "";
    ui.job = p;
  }, 250);
  return () => {
    jobs -= 1;
    if (jobs) return;
    clearInterval(jobPoll);
    jobPoll = null;
    ui.job = null;
  };
}

/** Stop the file operation under way: the file it is on is removed (a move keeps its
 *  original), what is done stays and can be undone, and those waiting behind it do not start. */
export function stopJob() {
  if (ui.job) ui.job.stopping = true;
  invoke("job_stop").catch(() => {});
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
/** A new id for a listing in the backend (`tabs.rs`): a tab's, or a Miller column's. */
export const newId = () => nextId++;

/** No marks: how many, their files' bytes, the folders among them, what they are (`tabs::Marks`). */
const NO_MARKS = { count: 0, bytes: 0, dirs: [], dir_bytes: 0, subject: null };

export function newTab(dir, view = "details") {
  const t = {
    id: newId(),
    dir,
    cursor: 0,
    /** How far the list is scrolled, kept per tab. */
    top: 0,
    marks: NO_MARKS,
    /** Where `..` leads, when the listing has it. */
    up: null,
    /** Not read yet, or changed while in the background: read when shown (`readWhenShown`). */
    unread: false,
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
  // The listing: rows asked of the backend (tabs.rs) as they come into view. Inside an archive
  // a folder's size comes with the listing; there is nothing to measure. A history's folders
  // have none (git keeps no size for a tree): left empty, not "0 B". A list of snapshots: each
  // one's own space.
  t.items = new Paged(t.id, (rows) => {
    const tb = tabOf(t.id);
    if (!tb) return;
    if (tb.archive && !tb.history) for (const e of rows) if (e.is_dir && e.name !== "..") tb.sizes[e.path] = e.size;
    if (tb.snapshot && !tb.snapshot.snapshot) for (const e of rows) if (e.name !== "..") tb.sizes[e.path] = e.size;
  });
  return t;
}

/** The tab with this id, as the panes hold it (reactive). */
const tabOf = (id) => ui.panes.flatMap((p) => p.tabs).find((t) => t.id === id);

/** Change the marks of tab `t` (`tabs::MarkOp`: `{ toggle: i }`, `{ range: [a, b] }`, "all",
 *  "none", "invert", `{ glob: { patterns, on } }`); its rows show them. */
export async function mark(t, op) {
  try {
    t.marks = await invoke("tab_mark", { tab: t.id, generation: t.items.shown, op });
  } catch (e) {
    // The listing changed meanwhile: rows by number would be other entries now.
    return void (String(e) !== STALE && (ui.status = String(e)));
  }
  t.items.refresh();
}

/** Entries of the tab's listing (`tabs::tab_pick`): "marked", "dirs" or "all", never `..`. */
export const pick = (t, which) => invoke("tab_pick", { tab: t.id, which });

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
  invoke("tab_drop", { tab: p.tabs[i].id }).catch(() => {});
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
export const item = (t = tab()) => t?.items.at(t.cursor);

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

/** Re-read a tab, keeping the cursor on `focus` (default: the current name). True when listed,
 *  false when it failed, undefined when a later load of the tab overtook it. */
export async function load(t, dir = t.dir, focus) {
  const keep = focus ?? item(t)?.name;
  const generation = ++t.items.generation;
  t.items.loading = generation;
  const stale = () => t.items.generation !== generation;
  // A slow disk: after a moment the status line says what is being read.
  const slow = setTimeout(() => (ui.status = tr("status.busy", { what: basename(dir) || dir })), 150);
  try {
    const r = await invoke("tab_list", { tab: t.id, generation, dir, showHidden: ui.showHidden, sort: t.sort, reverse: t.reverse, focus: keep ?? null, cursor: t.cursor, span: SPAN }).finally(() => {
      if (t.items.loading === generation) t.items.loading = 0;
      clearTimeout(slow);
      if (ui.status === tr("status.busy", { what: basename(dir) || dir })) ui.status = "";
    });
    // A slow folder answering after a later cd would show the wrong folder.
    if (stale()) return undefined;
    if (r.dir !== t.dir) {
      // In the same repository the last-commit column stays (empty) until git answers, so
      // the columns do not jump at every step.
      const same = t.git && r.dir.startsWith(t.git.root);
      t.git = null;
      t.last = same && t.last ? {} : null;
      t.lastHead = null;
      t.sizes = {};
      t.counts = {};
    }
    t.dir = r.dir;
    t.archive = r.archive;
    t.locked = r.locked;
    t.diskImage = r.disk_image;
    t.nested = r.nested;
    t.image = r.image;
    t.oci = r.oci;
    t.history = r.history;
    t.snapshot = r.snapshot;
    t.package = r.package;
    t.packages = r.packages;
    // In a model store's folder: the model each entry is, shown beside its name.
    t.models = r.models ?? null;
    t.hasNotes = r.has_notes;
    t.up = r.up;
    t.marks = r.marks;
    t.error = null;
    t.items.reset(r, generation);
    t.cursor = r.cursor;
    if (ui.folders) cmpStates(t);
  } catch (e) {
    if (stale() || String(e) === STALE) return undefined;
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
      if (t.sort === "commit") sortTab(t);
    }, () => {});
  return true;
}

/** What the backend says to a listing a later one overtook. */
const STALE = "stale listing";

/** Sort the tab's listing again by `t.sort` and `t.reverse`, the cursor kept on its entry. A
 *  history, a list of snapshots, an archive or a package has an order of its own, which a
 *  fresh read keeps. */
export async function sortTab(t) {
  // A folder being read: read with the new order instead.
  if (t.items.loading) return load(t);
  if (t.history || t.snapshot || t.archive || t.package || [HISTORY, SNAPSHOTS].includes(basename(t.dir))) return load(t);
  const generation = ++t.items.generation;
  try {
    // By size, folders go by their measured sizes.
    const measured = t.sort === "size" ? $state.snapshot(t.sizes) : {};
    const r = await invoke("tab_sort", { tab: t.id, generation, sort: t.sort, reverse: t.reverse, measured, cursor: t.cursor, span: SPAN });
    if (t.items.generation !== generation) return;
    t.items.reset(r, generation);
    t.cursor = r.cursor;
  } catch (e) {
    // A later listing of the tab came first: it is the one shown.
    if (String(e) !== STALE) ui.status = String(e);
  }
}

/** Where `..` leads: the listing says (out of a history, back to the folder on disk). */
export const up = (t) => t.up ?? parent(t.dir);

/** Into the git history of the entry under the cursor (of the folder itself on `..`): this
 *  branch's commits, every branch's ("all") or those against the upstream ("upstream"). In a
 *  history, "all" and "upstream" switch to that list, and back. */
export function openHistory(tb = tab(), scope = "branch") {
  const e = item(tb);
  const s = ui.cfg.sep;
  const seg = (sc) => ({ all: s + ALL, upstream: s + UPSTREAM })[sc] ?? "";
  if (scope !== "branch" && tb.history) {
    const h = tb.history;
    if (h.view !== "history") return void (ui.status = tr("history.all_not_here"));
    return cd(tb, `${h.target.replace(/[\\/]$/, "")}${s}${HISTORY}${seg(h.scope === scope ? "branch" : scope)}`);
  }
  if (tb.archive || tb.history) return void (ui.status = tr("history.not_here"));
  if (!tb.git) return void (ui.status = tr("history.no_repo"));
  const target = e && e.name !== ".." ? e.path : tb.dir;
  const go = () => cd(tb, `${target.replace(/[\\/]$/, "")}${s}${HISTORY}${seg(scope)}`);
  // A Subversion or CVS history is on its server: asked once per server, before anything is sent.
  return invoke("history_server", { path: target }).then((ask) => {
    if (!ask) return go();
    ui.modal = {
      kind: "confirm",
      feature: ask.program === "svn" ? "subversion" : "cvs",
      title: tr("history.ask_server_title"),
      text: tr("history.ask_server_text", { system: ask.system, server: ask.server, program: ask.program }),
      ok: tr("history.ask_server_yes"),
      safe: true,
      run: () => invoke("history_allow", { server: ask.server }).then(go, (err) => (ui.status = String(err))),
    };
  }, go);
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

/** The key of action `name`, as menus show it ("" when it has none). */
export const keyOf = (name) => ui.cfg.actions[name]?.[1] ?? "";

/** The commits that changed the function at `line` (from 0) of `path`, or lines `line..=to`,
 *  in a window over what is open (the editor comes back as it was). `text` is the editor's. */
export async function lineHistory(path, text, line, to) {
  ui.status = t("blame.tracing");
  try {
    const h = await invoke("git_line_history", { path, text, line, to });
    if (!h.changes.length) return void (ui.status = t("blame.no_changes"));
    ui.status = "";
    ui.modal = { kind: "linehistory", feature: "git_line_history", path, h, back: ui.modal };
  } catch (err) {
    ui.status = String(err);
  }
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
export async function makeSearchable(paths) {
  // Its key again while it runs stops it, as the status line says.
  if (ui.searchable) return stopSearchable();
  paths ??= await targets();
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

/** Whether a recording's speech can be transcribed, by its name (`transcribe::is_speech`). */
export function isSpeech(name) {
  const dot = name.lastIndexOf(".");
  return dot > 0 && (ui.cfg.speech_endings ?? []).includes(name.slice(dot + 1).toLowerCase());
}

/** *Transcribe speech* (`coxswain_core::transcribe`) on `paths`: with transcribing off, how to
 *  turn it on; else how much sound and how long it takes here, asked first, then handed to the
 *  search helper, which transcribes in the background. */
export async function transcribe(paths) {
  const title = t("action.transcribe");
  let plan;
  try {
    plan = await invoke("transcribe_plan", { paths });
  } catch (e) {
    return void (ui.modal = failure(title, e));
  }
  if (plan.off) return void (ui.modal = { kind: "message", title, text: plan.text, settings: "search_whisper", feature: "transcripts" });
  if (!plan.files.length) return void (ui.status = plan.text);
  ui.modal = {
    kind: "confirm", title, text: plan.text, ok: t("whisper.go"), safe: true, feature: "transcripts",
    run: async () => {
      try {
        const n = await invoke("transcribe", { files: plan.files });
        ui.status = t("whisper.taken", { n, key: ui.cfg.actions.settings?.[1] ?? "" });
      } catch (e) {
        ui.modal = failure(title, e);
      }
    },
  };
}

/** Stop the *Make PDF searchable* run under way. */
export function stopSearchable() {
  if (!ui.searchable) return;
  ui.searchable.stopping = true;
  invoke("searchable_stop");
}

/** Choose how the kind `r` (a `preview_tools::Row`) is made ("" follows How previews are made):
 *  written into config.toml, a pick in the preview for it forgotten, the preview made again. */
export async function setPreviewTool(r, choice) {
  ui.cfg = await invoke("set_preview_tool", { kind: r.kind, choice });
  ui.previewEngine = Object.fromEntries(Object.entries(ui.previewEngine).filter(([tool]) => !r.tools.includes(tool)));
  ui.toolsChanged++;
}

/** *Preview with…* (`preview_tools`) on the file under the cursor: the choices for its kind,
 *  each with what it would use, the line that installs its program, and Settings at Preview
 *  tools. */
export async function previewWith(e = item(tab())) {
  const spec = e && !e.is_dir ? CONVERTER[previewKind(e)] : null;
  if (!spec) return void (ui.status = t("ptools.none_here"));
  let r;
  try {
    r = await invoke("preview_with", { tool: spec.tool });
  } catch (err) {
    return void (ui.status = String(err));
  }
  const installed = r.programs.filter((p) => p.path).map((p) => (p.version ? `${p.name} ${p.version}` : p.name));
  const im = r.images.find((i) => i.image);
  const container = im ? `${t("ptools.choice.container")}: ${im.image.split("@")[0].split("/").pop()} (${im.pulled != null ? t("settings.image_pulled", { size: size(im.pulled) }) : im.download ? `${t("settings.image_not_pulled")} (${im.download})` : t("settings.image_not_pulled")})` : null;
  const prefer = t({ auto: "settings.prefer_auto", local: "settings.prefer_local", container: "settings.prefer_container" }[ui.cfg.settings.preview_prefer] ?? "settings.prefer_auto");
  const choices = [
    ["", t("ptools.choice.default", { value: prefer })],
    ["local", `${t("ptools.choice.local")}: ${installed.length ? installed.join(", ") : t("convert.not_installed", { program: r.programs[0]?.name ?? "" })}`],
    ["container", container],
    ["off", t("ptools.choice.off")],
  ].filter(([, label]) => label != null); // No image: Container cannot run it.
  const set = (c) => setPreviewTool(r, c).then(() => (ui.status = t("ptools.chosen", { kind: r.title })), (err) => (ui.status = String(err)));
  const items = choices.map(([c, label], i) => ({ key: String(i + 1), label: r.choice === c ? `${label} · ${t("ptools.current")}` : label, run: () => set(c) }));
  if (!installed.length && r.install) items.push({ key: "c", label: t("ptools.menu_copy", { line: r.install }), run: () => invoke("copy_text", { text: r.install }).then(() => (ui.status = t("guide.copied", { command: r.install })), (err) => (ui.status = String(err))) });
  items.push({ key: "s", label: t("ptools.menu_settings"), run: () => (ui.modal = { kind: "settings", section: "preview_tools" }) });
  ui.modal = { kind: "menu", feature: "preview_tools", title: `${actionLabel("preview_with")} ${r.title}`, direct: true, filter: "", cursor: 0, items };
}

/** *Long files: sample or everything…* (`coxswain_core::long_files`) on the folder under the
 *  cursor: Everything here, Sample here or like the folders above, each with what it would
 *  cost (the long files, the room on the disk, the time); the one taken is saved. */
export async function longFiles(e = item(tab())) {
  if (!e || !e.is_dir || e.name === "..") return void (ui.status = t("long.on_folder"));
  let choices;
  try {
    choices = await invoke("long_files_choices", { dir: e.path });
  } catch (err) {
    return void (ui.status = String(err));
  }
  const save = (c) => invoke("save_settings", { changes: c.changes }).then((cfg) => ((ui.cfg = cfg), (ui.status = t("long.saved", { name: e.name }))), (err) => (ui.status = String(err)));
  const items = choices.map((c, i) => ({ key: String(i + 1), label: c.label, sub: c.cost, run: () => (c.current ? null : save(c)) }));
  ui.modal = { kind: "menu", feature: "long_files", title: `${actionLabel("long_files")} ${e.name}`, direct: true, filter: "", cursor: 0, items };
}

/** *Convert video or audio* (`program` 0, ffmpeg), *Convert or edit pictures* (1, ImageMagick),
 *  *Convert geodata* (2, GDAL), *Export 3D model* (3, OpenSCAD) or *Export from KiCad* (4) on the files among `paths` it takes (`coxswain_core::media`): what to make of them from a
 *  menu, a clip's times or a size asked next, then the question (what it writes, which program
 *  runs, the first pull and its size), then the run, the status line saying how far it is with
 *  a Stop button. The action again while it runs stops it. */
export async function convertMedia(program, paths) {
  const action = ["convert_media", "edit_pictures", "convert_geodata", "export_model", "export_kicad"][program];
  const none = () => (ui.status = t(["media.none_media", "media.none_pictures", "geo.none", "scad.none", "kicad.none"][program]));
  if (ui.media) return stopMedia();
  paths ??= await targets();
  const files = paths.filter((p) => [isMedia, isEditable, isGeo, isScad, isKicad][program](basename(p)));
  if (!files.length) return void none();
  const label = (id) => {
    const [base, value] = id.split(":");
    return base === "geo" ? t(`geo.to.${value}`) : base === "kicad" ? t(`kicad.to.${value}`) : base === "stl" || base === "3mf" ? t(`scad.op.${base}`) : t(`media.op.${base}`, { degrees: value });
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

/** *Convert e-book* (`coxswain_core::ebook`): the books among `paths` made into EPUB, AZW3 or
 *  PDF by calibre, next to each. The format from a menu, then the question (what it writes,
 *  which calibre runs, the first pull and its size), then the run, the status line saying how
 *  far it is with a Stop button. One run at a time; the action again stops it. */
export async function convertEbook(paths) {
  if (ui.ebook) return stopEbook();
  paths ??= await targets();
  const items = ["epub", "azw3", "pdf"].map((to, i) => ({ key: String(i + 1), label: t(`ebook.to_${to}`), run: () => planEbook(paths, to) }));
  ui.modal = { kind: "menu", feature: "ebooks", title: t("action.convert_ebook"), direct: true, filter: "", cursor: 0, items };
}

async function planEbook(paths, to) {
  let plan;
  try {
    plan = await invoke("ebook_plan", { paths, to });
  } catch (err) {
    return void (ui.status = String(err));
  }
  const title = t("action.convert_ebook");
  if (plan.missing) return void (ui.modal = { kind: "message", title, text: plan.missing, settings: ui.cfg.settings.ebooks ? (ui.cfg.settings.preview_container === "off" ? "preview_container" : "previews") : "ebooks", feature: "ebooks" });
  if (!plan.books.length) return void (ui.status = t("ebook.none"));
  const what = plan.books.length === 1 ? t("ebook.confirm_one", { name: basename(plan.books[0]), target: plan.targets[0] }) : t("ebook.confirm_many", { n: plan.books.length, format: to.toUpperCase() });
  ui.modal = { kind: "confirm", title, text: `${what} ${t("ebook.no_drm")} ${plan.about}`, ok: t("ebook.go"), safe: true, run: () => runEbook(plan.books, to), feature: "ebooks" };
}

async function runEbook(books, to) {
  ui.ebook = { at: 0, all: books.length, name: basename(books[0]), line: "", started: Date.now(), now: Date.now(), stopping: false };
  const poll = setInterval(async () => {
    const p = await invoke("ebook_progress").catch(() => null);
    if (p && ui.ebook) Object.assign(ui.ebook, p, { now: Date.now() });
  }, 500);
  try {
    const made = await invoke("ebook_run", { paths: books, to });
    ui.status = made.length === 1 ? t("ebook.done_one", { name: basename(made[0]) }) : t("ebook.done_many", { n: made.length });
  } catch (err) {
    if (ui.ebook?.stopping) ui.status = t("ebook.stopped");
    else ui.modal = failure(t("ebook.failed"), err);
  } finally {
    clearInterval(poll);
    ui.ebook = null;
    await reloadAll();
  }
}

/** Stop the conversion under way. */
export function stopMedia() {
  if (!ui.media) return;
  ui.media.stopping = true;
  invoke("media_stop");
}

/** Stop the *Convert e-book* run under way. */
export function stopEbook() {
  if (!ui.ebook) return;
  ui.ebook.stopping = true;
  invoke("ebook_stop");
}

/** *Make checksums* (`coxswain_core::checksum`): the algorithm from a menu, then a SHA256SUMS,
 *  SHA512SUMS or B3SUMS of `paths` in the tab's folder, the status line saying how far it is with
 *  a Stop button. Its key again while checksums run stops them. */
export async function makeChecksums(paths) {
  if (ui.work) return stopWork();
  paths ??= await targets();
  const tb = tab();
  if (tb.archive || tb.history || tb.snapshot || tb.package) return void (ui.status = t("sums.not_here"));
  if (!paths.length) return void (ui.status = t("sums.nothing"));
  const algos = await invoke("sums_algos", { dir: tb.dir });
  const make = (algo) =>
    runWork("make_checksums", () => invoke("sums_make", { dir: tb.dir, paths, algo }), (m) => (ui.status = tn("sums.made", m.files, { name: basename(m.path) })), "sums.make_failed");
  ui.modal = { kind: "menu", feature: "checksums", title: actionLabel("make_checksums"), direct: true, filter: "", cursor: 0, items: algos.map(([id, algo, file], i) => ({ key: String(i + 1), label: t("sums.algo", { algo, file }), run: () => make(id) })) };
}

/** *Verify checksums*: every file `list` names checked against its sum, the result in a dialog,
 *  and the entries of the list's folder that hold a changed or unreadable file marked. */
export function verifyChecksums(list = item()?.path, tb = tab()) {
  if (ui.work) return stopWork();
  if (!list || !isSums(basename(list))) return void (ui.status = t("sums.no_list"));
  return runWork("verify_checksums", () => invoke("sums_verify", { list }), (v) => {
    const dir = parent(list);
    const top = (p) => (p.startsWith(dir) ? dir + p.slice(dir.length).replace(/^([\\/][^\\/]+).*$/, "$1") : null);
    const bad = new Set(v.checked.filter((c) => c.state === "changed" || c.state === "unreadable").map((c) => top(c.path)).filter(Boolean));
    let text = v.summary;
    if (tb.dir === dir) {
      mark(tb, "none").then(() => bad.size && mark(tb, { paths: [...bad] }));
      if (bad.size) text += ` ${t("sums.marked")}`;
    }
    const details = v.checked.map((c, i) => [c, v.words[i]]).filter(([c]) => c.state !== "ok").map(([c, w]) => `${c.name}: ${w}`).join("\n");
    ui.modal = { kind: "message", feature: "checksums", title: t("sums.verify_title", { name: basename(list) }), text, details: details || undefined, open: true };
  }, "sums.verify_failed");
}

/** *Split file* (`coxswain_core::split`): the size of the parts from a menu, or typed, then the
 *  file under the cursor cut into `name.001`, `name.002` … with `name.sha256` beside them. */
export async function splitFile(tb = tab()) {
  if (ui.work) return stopWork();
  const e = item(tb);
  if (!e || e.is_dir || e.name === "..") return void (ui.status = t("split.no_file"));
  if (tb.archive || tb.history || tb.snapshot || tb.package) return void (ui.status = t("sums.not_here"));
  const run = (size) => runWork("split_file", () => invoke("split_file", { path: e.path, size }), (s) => (ui.status = tn("split.done", s.parts.length, { sums: basename(s.sums) })), "split.split_failed");
  const typed = async (v) => {
    const size = await invoke("split_size", { text: v });
    if (size) run(size);
    else ui.status = t("split.size_bad");
  };
  const sizes = await invoke("split_sizes");
  const items = sizes.map(([size, label], i) => ({ key: String(i + 1), label, run: () => run(size) }));
  items.push({ key: String(items.length + 1), label: t("split.size.other"), run: () => (ui.modal = { kind: "input", feature: "split_and_join", title: actionLabel("split_file"), label: t("split.size_label"), value: "250M", ok: t("common.apply"), run: typed }) });
  ui.modal = { kind: "menu", feature: "split_and_join", title: `${actionLabel("split_file")} ${e.name}`, direct: true, filter: "", cursor: 0, items };
}

/** *Join parts*: the parts from `first` (`name.001`) on joined into `name`, asked first (and
 *  again before a file of that name is replaced), then checked against `name.sha256`. */
export async function joinParts(first = item()?.path) {
  if (ui.work) return stopWork();
  const plan = first && (await invoke("join_plan", { first }));
  if (!plan) return void (ui.status = t("split.not_first"));
  const [name, n, there] = plan;
  const run = () =>
    runWork("join_parts", () => invoke("join_parts", { first, replace: there }), (j) => {
      if (j.matches === false) ui.modal = { kind: "message", feature: "split_and_join", title: actionLabel("join_parts"), text: t("split.mismatch", { name }) };
      else ui.status = t(j.matches ? "split.joined_ok" : "split.joined", { name });
    }, "split.join_failed");
  ui.modal = { kind: "menu", feature: "split_and_join", title: actionLabel("join_parts"), direct: true, filter: "", cursor: 0, items: [{ key: "1", label: there ? t("split.replace", { name }) : tn("split.join_into", n, { name }), run }] };
}

async function runWork(action, start, done, failed) {
  ui.work = { action, line: "", stopping: false };
  const what = actionLabel(action);
  const poll = setInterval(async () => {
    const line = await invoke("work_progress", { what }).catch(() => null);
    if (line && ui.work) ui.work.line = line;
  }, 300);
  try {
    done(await start());
  } catch (err) {
    if (ui.work?.stopping) ui.status = t(action === "split_file" || action === "join_parts" ? "split.stopped" : "sums.stopped");
    else ui.modal = failure(t(failed), err);
  } finally {
    clearInterval(poll);
    ui.work = null;
    await reloadAll();
  }
}

/** Stop the checksums under way. */
export function stopWork() {
  if (!ui.work) return;
  ui.work.stopping = true;
  invoke("work_stop");
}

/** *Build PDF of AsciiDoc* (`op` "adoc_pdf") or *Export draw.io diagram* ("drawio_png",
 *  "drawio_svg", "drawio_pdf"; none: picked from a menu) on the files among `paths` it takes
 *  (`coxswain_core::export`): the question (what it writes, which program runs, the first pull
 *  and its size), then the run, the status line saying how far it is with a Stop button. The
 *  action again while it runs stops it. */
export function makeExport(adoc, op = adoc ? "adoc_pdf" : "", paths = targets()) {
  const action = adoc ? "adoc_pdf" : "drawio_export";
  if (ui.exporting) return stopExport();
  const files = paths.filter((p) => (adoc ? /\.(adoc|asciidoc)$/i.test(p) : isDrawio(basename(p))));
  if (!files.length) return void (ui.status = t(adoc ? "export.none_adoc" : "export.none_drawio"));
  const ask = async (op) => {
    let plan;
    try {
      plan = await invoke("export_plan", { paths: files, op });
    } catch (err) {
      return void (ui.status = String(err));
    }
    const title = actionLabel(action);
    if (!plan.files.length) return void (ui.status = t(adoc ? "export.none_adoc" : "export.none_drawio"));
    // draw.io off: Settings opens at its switch; else what to install, or the container runtime.
    if (plan.missing) return void (ui.modal = { kind: "message", title, text: plan.missing, settings: plan.off ? "drawio" : ui.cfg.settings.preview_container === "off" ? "preview_container" : "previews", feature: action });
    ui.modal = { kind: "confirm", title, text: plan.about, ok: t(adoc ? "export.go_adoc" : "export.go_drawio"), safe: true, run: () => runExport(plan.files, op, action), feature: action };
  };
  if (op) return ask(op);
  ui.modal = {
    kind: "menu",
    feature: action,
    title: actionLabel(action),
    direct: true,
    filter: "",
    cursor: 0,
    items: ["drawio_png", "drawio_svg", "drawio_pdf"].map((id, i) => ({ key: String(i + 1), label: t(`export.op.${id}`), run: () => ask(id) })),
  };
}

async function runExport(files, op, action) {
  ui.exporting = { at: 0, all: files.length, name: basename(files[0]), line: "", started: Date.now(), now: Date.now(), stopping: false, action };
  const poll = setInterval(async () => {
    const p = await invoke("export_progress").catch(() => null);
    if (p && ui.exporting) Object.assign(ui.exporting, p, { now: Date.now() });
  }, 500);
  try {
    const made = await invoke("export_run", { paths: files, op });
    ui.status = made.length === 1 ? t("export.done_one", { name: basename(made[0]) }) : t("export.done_many", { n: made.length });
  } catch (err) {
    if (ui.exporting?.stopping) ui.status = t("export.stopped");
    else ui.modal = failure(t("export.failed"), err);
  } finally {
    clearInterval(poll);
    ui.exporting = null;
    await reloadAll();
  }
}

/** Stop the export under way. */
export function stopExport() {
  if (!ui.exporting) return;
  ui.exporting.stopping = true;
  invoke("export_stop");
}

/** *Scan for viruses* (`coxswain_core::viruses`): the marked entries, the one under the
 *  cursor, or on `..` the tab's folder; asked first (how many files, the database and its
 *  update, the first pull), then run, the status line saying how far it is with a Stop button
 *  (or the key again). What it finds is marked in the tab and listed, each file with a choice. */
export async function scanViruses(tb = tab()) {
  if (ui.viruses) return stopViruses();
  if (subject(tb).place !== "disk") return void (ui.status = t("viruses.none"));
  const paths = targets(tb).length ? targets(tb) : [tb.dir];
  let plan;
  try {
    plan = await invoke("viruses_plan", { paths });
  } catch (err) {
    return void (ui.status = String(err));
  }
  const title = t("action.scan_viruses");
  // Off, or nothing to run it with: what to turn on or install, and Settings at the place for it.
  if (plan.missing) return void (ui.modal = { kind: "message", title, text: plan.missing, settings: plan.off ? "virus_scan" : ui.cfg.settings.preview_container === "off" ? "preview_container" : "previews", feature: "virus_scan" });
  ui.modal = { kind: "confirm", title, text: plan.text, ok: t("viruses.go"), safe: true, run: () => runViruses(paths), feature: "virus_scan" };
}

async function runViruses(paths) {
  ui.viruses = { line: t("viruses.busy_start"), started: Date.now(), now: Date.now(), stopping: false };
  const poll = setInterval(async () => {
    const line = await invoke("viruses_progress").catch(() => null);
    if (ui.viruses) Object.assign(ui.viruses, line ? { line } : {}, { now: Date.now() });
  }, 500);
  try {
    const report = await invoke("viruses_run", { paths });
    const [summary, notes, quarantine] = await invoke("viruses_facts", { report });
    if (!report.found.length) {
      ui.status = summary;
      if (notes.length) ui.modal = { kind: "message", title: t("action.scan_viruses"), text: `${summary}.`, notes, feature: "virus_scan" };
      return;
    }
    // Marked in the panes: each file found there, and each folder with one in it.
    const holds = (e) => e.name !== ".." && report.found.some((f) => f.path === e.path || f.path.startsWith(e.path + "/") || f.path.startsWith(e.path + "\\"));
    for (const v of visibleTabs()) {
      const hit = v.items.filter(holds);
      if (!hit.length) continue;
      v.marked.clear();
      for (const e of hit) v.marked.add(e.path);
    }
    ui.modal = { kind: "viruses", feature: "virus_scan", report, notes, quarantine, choices: report.found.map(() => "nothing"), cursor: 0 };
  } catch (err) {
    if (ui.viruses?.stopping) ui.status = t("viruses.stopped");
    else ui.modal = failure(t("viruses.failed"), err);
  } finally {
    clearInterval(poll);
    ui.viruses = null;
  }
}

/** Do what was chosen for each file found. */
export async function applyViruses(m) {
  ui.modal = null;
  const done = await invoke("viruses_act", { choices: m.choices });
  ui.status = t("viruses.done", { bin: done.bin, quarantine: done.quarantine, left: done.left });
  if (done.failed.length) ui.modal = failure(t("viruses.failed_act"), done.failed.join("\n"));
  await reloadAll();
}

/** Stop the *Scan for viruses* run under way. */
export function stopViruses() {
  if (!ui.viruses) return;
  ui.viruses.stopping = true;
  invoke("viruses_stop");
}

/** Find and replace across files (`coxswain_core::replace`): in `roots` (Find's results), else
 *  the marked entries, the one under the cursor or, on `..`, the tab's folder; paths shown from
 *  `base`, `find` typed in. */
export async function openReplace(roots, base, find = "") {
  const tb = tab();
  if (!roots) {
    if (tb.archive || tb.history || tb.snapshot || tb.package || tb.diskImage) return void (ui.status = t("replace.not_here"));
    roots = await targets(tb);
    if (!roots.length) roots = [tb.dir];
    base = tb.dir;
  }
  if (!roots.length) return void (ui.status = t("replace.no_files"));
  const what = roots.length === 1 ? basename(roots[0]) || roots[0] : tn("items", roots.length);
  ui.modal = { kind: "replace", title: t("replace.title", { what }), roots, base, query: { find, with: "", regex: false, case: false, word: false }, plan: null, ticks: [], busy: null, error: "" };
}

/** Permissions and dates in bulk (`coxswain_core::attrs`) of the marked entries, or the one
 *  under the cursor. */
export async function openAttrs() {
  const tb = tab();
  if (tb.archive || tb.history || tb.snapshot || tb.package || tb.diskImage) return void (ui.status = t("attrs.not_here"));
  const picked = tb.marks.count ? await pick(tb, "marked") : [item(tb)].filter((e) => e && e.name !== "..");
  const roots = picked.map((e) => e.path);
  if (!roots.length) return void (ui.status = t("attrs.nothing_marked"));
  const what = roots.length === 1 ? basename(roots[0]) || roots[0] : tn("items", roots.length);
  // Into folders by default when only folders are marked.
  const dirs = !/Windows/.test(navigator.userAgent) && picked.every((e) => e.is_dir);
  ui.modal = { kind: "attrs", title: t("attrs.title", { what }), roots, base: tb.dir, spec: { mode: "", date: "", accessed: false, recursive: dirs }, plan: null, rows: [], busy: null, error: "" };
}

/** Space on `dir` (default: the folder under the cursor, else the tab's): where the space goes,
 *  full window. Only folders on a disk. */
export function openSpace(tb = tab(), dir) {
  if (tb.archive || tb.history || tb.snapshot || tb.package) return void (ui.status = t("space.not_here"));
  const e = item(tb);
  const folder = dir ?? (e?.is_dir && e.name !== ".." && !tb.marks.count ? e.path : tb.dir);
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
  const i = await invoke("tab_find", { tab: tb.id, name: path }).catch(() => null);
  if (i != null) tb.cursor = i;
}

/** Tab `t` to `dir`, the cursor on `focus` (a name) when given. */
export async function cd(t, dir, history = true, focus) {
  if (!dir || dir === t.dir) return;
  const from = t.dir;
  const prevCursor = t.cursor;
  t.cursor = 0;
  // Coming up: the cursor on the folder (or the file whose history it was) it came from.
  const pre = dir.endsWith(ui.cfg.sep) ? dir : dir + ui.cfg.sep;
  const came = from.startsWith(pre) ? pre + from.slice(pre.length).split(/[\\/]/)[0] : undefined;
  const ok = await load(t, dir, focus ?? came);
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
    const up = !paths && t.up;
    if (up && t.sizes[up] === undefined) {
      const r = await invoke("dir_size_known", { path: up });
      if (t.dir !== dir) return;
      if (r) [t.sizes[up], t.counts[up]] = r;
    }
    const todo = paths ?? (await pick(t, "dirs")).map((e) => e.path).filter((p) => t.sizes[p] === undefined);
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

/** Sorted by size: the folders measured since the listing (which sorted them by name) go to
 *  their place, the cursor on the same entry. A quarter second after the first arrival, so a
 *  folder of thousands is sorted four times a second at most. */
const resorting = new WeakSet();
export function resortSoon(t) {
  if (t.sort !== "size" || resorting.has(t)) return;
  resorting.add(t);
  setTimeout(async () => {
    resorting.delete(t);
    // A history, a list of snapshots, an archive or a package keeps the backend's order; with
    // no folder measured there is nothing to move.
    if (t.sort !== "size" || t.archive || t.history || (t.snapshot && !t.snapshot.snapshot) || t.package || !Object.keys(t.sizes).length) return;
    sortTab(t);
  }, 250);
}

/** A tab not read yet (restored with the session, or changed while in the background) is read
 *  as it is shown. A file (`coxswain-gui ~/Pictures/cat.jpg`) opens its folder with the cursor
 *  on it. A saved directory may be gone: the folder above it, else home. */
export async function readWhenShown(t) {
  if (!t.unread) return;
  t.unread = false;
  const ok = await load(t);
  if (ok !== false) return;
  const up = parent(t.dir);
  const above = up ? await load(t, up, basename(t.dir)) : false;
  if (above === false) await load(t, ui.cfg.home);
}

export const reloadAll = () => Promise.all(visibleTabs().map((t) => load(t)));

export function visibleTabs() {
  return (ui.dual ? ui.panes : [pane()]).map((p) => p.tabs[p.active]);
}

/** The marked entries' paths, in the listing's order, or the one under the cursor (not `..`). */
export async function targets(t = tab()) {
  if (t.marks.count) return (await pick(t, "marked")).map((e) => e.path);
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
  // A page loaded again starts its tabs anew: the listings the backend kept are dropped.
  await invoke("tab_reset").catch(() => {});
  setLanguage(ui.cfg);
  setArchiveEndings(ui.cfg.archive_endings);
  setDiskEndings(ui.cfg.disk_endings);
  setPictureEndings(ui.cfg.picture_endings);
  setMediaEndings(ui.cfg.media_endings);
  setGeoEndings(ui.cfg.geo_endings);
  const st = await invoke("get_state");
  const s = st.session ?? {};
  ui.favorites = st.favorites;
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
  // The tabs in view are read now; the others when they are shown (`readWhenShown`).
  for (const t of ui.panes.flatMap((p) => p.tabs)) t.unread = true;
  await Promise.all(visibleTabs().map(readWhenShown));
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
export const ociIndex = (tb, e) => !!e && !e.is_dir && e.name === "index.json" && !tb.archive && tb.oci;

/** Enter / double-click (`enter::enter`): into anything that holds things (a folder, an archive,
 *  a disk or container image, a mailbox); games, positions and opening books in the games window,
 *  a file of one game on its board; any other file in its default application. */
export async function openItem(tb, i = tb.cursor) {
  const e = tb.items.at(i);
  if (!e) return;
  if (e.is_dir) return cd(tb, e.path);
  const inside = !!tb.archive;
  let on;
  try {
    on = await invoke("enter_on", { path: e.path, isDir: false, inside });
  } catch (err) {
    return void (ui.status = String(err));
  }
  if (["archive", "disk_image", "image", "mailbox"].includes(on.enter)) return cd(tb, e.path);
  if (!(tb.history || tb.package) && (await gamesOf(e, on, inside))) return;
  if (tb.archive) return void (ui.status = t(tb.diskImage ? "disk_image.copy_out_hint" : "archive.copy_out_hint", { archive: basename(tb.archive) }));
  openFile(tb, e);
}

/** Games, positions or an opening book (`on`: `enter_on`'s answer, asked when not given) open in
 *  the games window: Enter and Open games (Alt+K). Inside an archive a .bin is told by a copy's
 *  bytes. Whether they opened. */
export async function gamesOf(e, on, inside = !!tab().archive) {
  on ??= await invoke("enter_on", { path: e.path, isDir: false, inside }).catch(() => null);
  if (!on) return false;
  if (["games", "positions", "book"].includes(on.enter)) return (await openGames(e.path, inside), true);
  if (!on.maybe_games) return false;
  const copy = await invoke("archive_peek", { path: e.path }).catch(() => null);
  if (!copy || !(await invoke("chess_sniffed", { path: copy }).catch(() => false))) return false;
  await openGames(copy, false);
  return true;
}

/** The games window on `path` (inside an archive: on a copy of it), as Open games does. */
export async function openGames(path, inside) {
  let file = path;
  if (inside) {
    try {
      file = await invoke("archive_peek", { path });
    } catch (err) {
      return void (ui.status = String(err));
    }
  }
  const f = chessFormat(basename(file));
  ui.modal = { kind: f === "sgf" ? "go" : "chess", path: file, feature: { pdn: "draughts", shogi: "shogi", pbn: "bridge" }[f] };
}

/** A file opened as it is: in its default application, or its place said. */
function openFile(tb, e) {
  if (tb.history) return void (ui.status = t("history.file_hint"));
  // A file of a package: to its folder, the cursor on it.
  if (tb.package) return showInTab(tb, e.path);
  // A list of checksums is checked; the first part of a split file offers Join.
  if (isSums(e.name)) return verifyChecksums(e.path, tb);
  if (isFirstPart(e.name) && !tb.snapshot) return joinParts(e.path);
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
  // Marked: what they are, as the backend counted them (`Subject::of`).
  const list = tb.marks.count ? [] : cur ? [cur] : [];
  const h = tb.history;
  const place = tb.archive ? (tb.diskImage || tb.nested ? "disk_image" : "archive") : h ? (h.view === "branches" && !h.commit ? "branches" : "history") : tb.snapshot ? "snapshot" : tb.package ? "package" : "disk";
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
    sums: files.filter((x) => isSums(x.name)).length,
    parts: files.filter((x) => isFirstPart(x.name)).length,
    images: files.filter((x) => ociIndex(tb, x)).length,
    // `local_models::is_model`: a model file by its ending, or a blob of a model store.
    local_models: files.filter((x) => /\.(gguf|safetensors|pt|pth|ckpt)$|\.safetensors\.index\.json$|^pytorch_model.*\.bin$/i.test(x.name) || (tb.models?.[x.name] && /^sha256-/.test(x.name))).length,
    // A .pdb is an e-book only by its header (a debugging database otherwise): F9 offers it.
    ebooks: files.filter((x) => /\.(mobi|azw3?|fb2|lit)$/i.test(x.name)).length,
    kicad: files.filter((x) => isKicad(x.name)).length,
    speech: files.filter((x) => isSpeech(x.name)).length,
    adocs: files.filter((x) => /\.(adoc|asciidoc)$/i.test(x.name)).length,
    drawios: files.filter((x) => isDrawio(x.name)).length,
    previewed: files.filter((x) => CONVERTER[previewKind(x)]).length,
    ...(tb.marks.count ? tb.marks.subject : {}),
    // The one marked entry is the one under the cursor (`m`: marked).
    one: tb.marks.count ? tb.marks.count === 1 && !!cur?.m : !!cur,
    marked: tb.marks.count,
    place,
    git: !!tb.git,
    zfs: !!tb.zfs,
    packages: !!tb.packages,
    // A drive's top, or a folder measured at 1 GB or more: where Space is worth a hint.
    big: place === "disk" && (cur?.is_dir && list.length === 1 ? (tb.sizes[cur.path] ?? 0) >= 2 ** 30 || ui.disks.some((d) => d.mount === cur.path) : ui.disks.some((d) => d.mount === tb.dir)),
    whisper: !!ui.cfg.settings.search_whisper,
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
  const e = t.items.at(i);
  if (!e || e.name === "..") return;
  return mark(t, { toggle: i });
}

/** Native drag of the marked files (or the one at `i`), so other apps can take them; what is
 *  only inside an archive or a history is copied out first (`drag::out`). */
export async function dragOut(t, i) {
  const e = t.items.at(i);
  if (!e || e.name === "..") return;
  t.cursor = i;
  const paths = e.m ? await targets(t) : [e.path];
  const scale = Math.max(1, Math.round(devicePixelRatio));
  try {
    const real = await invoke("start_drag", { paths, image: [...(await dragImage(paths, e, scale))], scale });
    ui.drag = { tab: t, paths, real };
  } catch (err) {
    ui.status = String(err);
  }
}

/** The picture under the pointer while dragging: the entry's icon and name, or the number of
 *  entries, in the theme's colours; a PNG at `scale` (device pixels per pixel). */
async function dragImage(paths, e, scale) {
  const css = getComputedStyle(document.documentElement);
  const v = (name, or) => css.getPropertyValue(name).trim() || or;
  const font = `${ui.cfg.gui.font_size}px ${v("--font", "monospace")}`;
  const text = paths.length === 1 ? e.name : tn("items", paths.length);
  const c = document.createElement("canvas");
  const g = c.getContext("2d");
  g.font = font;
  const pad = 8;
  const h = Math.round(ui.cfg.gui.font_size * 2);
  const w = Math.min(320, Math.ceil(g.measureText(text).width) + h + pad * 2);
  // A small gap from the pointer, which sits at the top left corner.
  const at = 12;
  c.width = (w + at) * scale;
  c.height = (h + at) * scale;
  g.scale(scale, scale);
  g.translate(at, at);
  g.fillStyle = v("--cursor-bg", "#333");
  g.strokeStyle = v("--accent-bg", "#888");
  g.beginPath();
  g.roundRect(0.5, 0.5, w - 1, h - 1, 4);
  g.fill();
  g.stroke();
  g.fillStyle = v("--cursor-fg", "#fff");
  g.textBaseline = "middle";
  g.font = `${ui.cfg.gui.font_size}px ${v("--icon-font", "monospace")}`;
  g.fillText(paths.length === 1 ? e.icon.glyph : "\u{f0c5}", pad, h / 2);
  g.font = font;
  g.fillText(text, pad + h, h / 2, w - h - pad * 2);
  const blob = await new Promise((ok) => c.toBlob(ok, "image/png"));
  return new Uint8Array(await blob.arrayBuffer());
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

// ---------------------------------------------------------------- compare folders

/** The side of the panes `t` is in: 0 left, 1 right. */
const sideOf = (t) => Math.max(0, ui.panes.findIndex((p) => p.tabs.includes(t)));
const FLIP = { only_left: "only_right", only_right: "only_left", newer_left: "newer_right", newer_right: "newer_left" };
/** How a state reads on side `side`: as the left sees it. */
const asLeft = (state, side) => (side === 0 ? state : (FLIP[state] ?? state));
const GLYPH = { only_left: "+", newer_left: ">", newer_right: "<", different: "≠", same: "=", only_right: "-" };
/** The mark beside a name in a pane while folders are compared (`State::glyph`). */
export const cmpGlyph = (t, state) => GLYPH[asLeft(state, sideOf(t))];
export const cmpClass = (t, state) => asLeft(state, sideOf(t));
/** The words for a state, for a tooltip. */
export const cmpLabel = (state) => t(`compare.${state === "same" ? "same_state" : state}`);
/** How the entries of `t` stand against the other pane's, while folders are compared. With
 *  Differences only, the listing itself (`tab_list`) leaves out what is the same. */
export async function cmpStates(t) {
  if (!ui.folders) return void (t.cmp = null);
  t.cmp = await invoke("folders_states", { dir: t.dir, side: sideOf(t) }).catch(() => null);
}

let folderTimer;
async function pollFolders() {
  clearTimeout(folderTimer);
  if (!ui.folders) return;
  const st = await invoke("folders_state").catch(() => null);
  if (!st || !ui.folders) return;
  Object.assign(ui.folders, st);
  // After a run that made one side like the other, what it did stays in front of the summary.
  ui.status = ui.folders.lead ? `${ui.folders.lead} · ${st.summary}` : st.summary;
  for (const t of visibleTabs()) await cmpStates(t);
  if (!st.done) folderTimer = setTimeout(pollFolders, 300);
  else if (ui.folders.diffOnly) await reloadAll();
}

/** Compare folders (Ctrl+F2): the left pane's folder against the right one's; while it is on,
 *  its menu. */
export function compareFolders() {
  if (!ui.folders) return startFolders("quick");
  const f = ui.folders;
  const entry = (key, label, run) => ({ key, label: t(label), run });
  ui.modal = {
    kind: "menu",
    feature: "compare_folders",
    title: actionLabel("compare_folders"),
    direct: true,
    filter: "",
    cursor: 0,
    items: [
      entry("1", "compare.mark_differences", markDifferences),
      entry("2", f.diffOnly ? "compare.show_all" : "compare.differences_only", () => diffOnly(!f.diffOnly)),
      entry("3", f.mode === "quick" ? "compare.by_content" : "compare.by_size_date", () => startFolders(f.mode === "quick" ? "content" : "quick")),
      entry("4", "compare.stop_comparing", stopFolders),
      entry("5", "compare.make_right", () => openSync(1)),
      entry("6", "compare.make_left", () => openSync(0)),
    ],
  };
}

/** Make side `to` (0 left, 1 right) like the other (`coxswain_core::compare::sync`): the plan
 *  is made at once; the dialog shows it, then runs it. */
export async function openSync(to) {
  try {
    await invoke("sync_check", { to });
  } catch (e) {
    return void (ui.status = String(e));
  }
  const st = await invoke("folders_state").catch(() => null);
  if (!st) return;
  const name = (p) => basename(p) || p;
  const [from, dest] = [st.roots[1 - to], st.roots[to]];
  ui.modal = { kind: "sync", feature: "compare_folders", to, title: t("sync.title", { to: name(dest), from: name(from) }), dir: name(dest), delete: false, plan: null, summary: "", rows: [], busy: null, error: "" };
  syncRun(ui.modal, false);
}

/** The plan (`writing` false), or the steps it shows; how far it is, polled. */
export async function syncRun(m, writing) {
  m.error = "";
  m.busy = { writing, files: 0, stopping: false };
  const poll = setInterval(async () => {
    const n = await invoke("sync_progress").catch(() => null);
    if (n != null && m.busy) m.busy.files = n;
  }, 200);
  try {
    if (!writing) {
      m.plan = null;
      const r = await invoke("sync_plan", { to: m.to, delete: m.delete });
      Object.assign(m, { plan: r.plan, rows: r.rows, summary: r.summary });
      if (!r.plan.total) m.error = t("sync.none");
      return;
    }
    const r = await invoke("sync_apply", { token: m.plan.token });
    if (ui.modal === m) ui.modal = null;
    await refreshUndo(r.summary);
    // Compared again, so the marks show what is left; the run's result stays on the status line.
    if (ui.folders) await startFolders(ui.folders.mode, ui.status);
    await reloadAll();
    if (r.failed.length) ui.modal = failure(t("sync.failed"), r.failed.join("\n"));
  } catch (e) {
    m.error = String(e);
  } finally {
    clearInterval(poll);
    m.busy = null;
  }
}

export async function startFolders(mode, lead = "") {
  const [l, r] = [tab(0), tab(1)];
  const keep = ui.folders?.diffOnly;
  if (!ui.dual || !l || !r) return void (ui.status = t("compare.two_panes"));
  try {
    await invoke("folders_open", { left: l.dir, right: r.dir, mode });
  } catch (e) {
    return void (ui.status = String(e));
  }
  ui.folders = { mode, diffOnly: false, done: false, summary: "", lead };
  if (keep) await diffOnly(true);
  pollFolders();
}

/** Differences only, on or off: the listings leave out what is the same, or show it again. */
async function diffOnly(on) {
  if (!ui.folders) return;
  ui.folders.diffOnly = on;
  await invoke("folders_diff_only", { on }).catch(() => {});
  await reloadAll();
  // The summary stays on the status line: the key that chose this took it away.
  if (ui.folders?.done && ui.folders.summary) ui.status = ui.folders.summary;
}

function stopFolders() {
  invoke("folders_close");
  const all = ui.folders?.diffOnly;
  ui.folders = null;
  for (const t of visibleTabs()) t.cmp = null;
  if (all) reloadAll();
  ui.status = t("compare.stopped_comparing");
}

/** Marks, in both panes, what is only there, newer there or different: ready for F5. */
async function markDifferences() {
  let n = 0;
  for (const tb of visibleTabs()) {
    const paths = await invoke("folders_ahead", { dir: tb.dir, side: sideOf(tb) }).catch(() => []);
    if (!paths.length) continue;
    const before = tb.marks?.count ?? 0;
    await mark(tb, { paths });
    n += (tb.marks?.count ?? 0) - before;
  }
  ui.status = tn("compare.marked", n);
}

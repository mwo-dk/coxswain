<script>
  // Every modal: prompts, confirmations, find file, menus, batch rename, tags, help.
  // App forwards keys through `handleKey`; returning true means "handled".
  import { tick, untrack } from "svelte";
  import { ui, tab, cd, showInTab, load, openPackage, refreshUndo, actionLabel, keysOf, gitTool, applyViruses, openReplace, reloadAll, syncRun } from "./app.svelte.js";
  import { failure } from "./errors.js";
  import { Channel } from "@tauri-apps/api/core";
  import { invoke, takesPassword, withEnding, packFormat, basename, parent, size, date, TAGS, TAG_COLORS, tagName, isHistory, historyOf, chessFormat } from "./lib.js";
  import { t, tn, num } from "./i18n.svelte.js";
  import { hljs } from "./hl.js";
  import { KINDS, prefix, nextKind, step } from "./find.js";
  import DiskNext from "./DiskNext.svelte";

  let input = $state();
  let listEl = $state();

  // Focus the first field whenever a modal opens.
  $effect(() => {
    if (ui.modal) tick().then(() => {
      input?.focus();
      if (["input", "pack"].includes(ui.modal?.kind)) input?.select?.();
    });
  });

  // Keep the highlighted row in view.
  $effect(() => {
    if (ui.modal && "cursor" in ui.modal) {
      ui.modal.cursor;
      tick().then(() => listEl?.querySelector(".cursor")?.scrollIntoView({ block: "nearest" }));
    }
  });

  // F1 → Features from a dialog goes back to it on Esc.
  const close = () => (ui.modal = ui.modal?.back ?? null);

  // ------------------------------------------------------------ features (F1)

  /** The feature (`coxswain_core::features`) each kind of window is about, for F1 in it; a
   *  modal's own `feature` wins. */
  const FEATURE_OF = { pack: "pack_and_extract", props: "properties", rename: "batch_rename", attrs: "permissions_and_dates", sync: "compare_folders", replace: "find_replace", tag: "tags", settings: "settings_window", dupes: "duplicates", bom: "cbom", chess: "chess", editor: "editor", compare: "compare_files", go: "go", provenance: "provenance", guide: "first_run", setup: "search_setup", search: "find_file" };

  /** F1 → Features, filtered to the feature of the window `from` (none: all of them). */
  export function features(from) {
    const id = from && (from.feature ?? FEATURE_OF[from.kind]);
    const filter = id ? t(`feature.${id}.title`) : "";
    ui.modal = { kind: "help", tab: "features", filter, back: from ?? null };
  }

  /** The other tab of Help: the keys, or every feature with its questions. */
  function helpTab(m, tab) {
    m.tab = tab;
    tick().then(() => input?.focus());
  }

  // The list for what is typed, from the core, in the current language.
  $effect(() => {
    const m = ui.modal;
    if (m?.kind !== "help" || m.tab !== "features") return;
    const query = m.filter ?? "";
    invoke("features", { query }).then((list) => ui.modal === m && (m.filter ?? "") === query && (m.list = list), () => {});
  });

  /** "Show me": the feature's page on GitHub, in the browser. */
  const showMe = (f) => invoke("open_docs", { id: f.id }).then((url) => (ui.status = t("features.opening", { url })), (e) => (ui.status = String(e)));

  /** Close `m` and run its action, once, even if Enter and a button click both fire. */
  function confirm(m, ...args) {
    if (ui.modal !== m) return;
    close();
    m.run(...args);
  }

  // ------------------------------------------------------------ find file

  // One search at a time: keys typed while one runs collapse into a single follow-up, so
  // fast typing never queues a scan per keystroke.
  let busy = false;
  let again = false;
  /** A snippet with the words it found marked; everything else is text, never markup. */
  const marked = (s) =>
    s
      .replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c])
      .replaceAll("\u0001", "<mark>")
      .replaceAll("\u0002", "</mark>");

  /** The kind Find shows: a prefix typed first wins over the chip. */
  const kindOf = (m) => prefix(m.query).kind ?? m.chip;
  /** Whether the answer shows in place of the list. */
  const answering = (m) => m.show === "answer" || kindOf(m) === "ask";
  /** The folder Find is limited to, if it is. */
  const scopeOf = (m) => (m.here ? tab().dir : null);

  export async function runSearch() {
    const m = ui.modal;
    if (m?.kind !== "search" || answering(m)) return;
    if (busy) return void (again = true);
    busy = true;
    try {
      const out = await invoke("find", { query: m.query, scope: scopeOf(m), chip: m.chip, askProblem });
      if (ui.modal === m) {
        m.out = out;
        m.cursor = out.start;
      }
    } finally {
      busy = false;
    }
    if (again) {
      again = false;
      runSearch();
    }
  }
  /** A change of kind, scope or view: the list again from the top. */
  function refind(m, change) {
    Object.assign(m, change, { cursor: 0 });
    runSearch();
    input?.focus();
  }

  // Ask again while the names are still being counted.
  $effect(() => {
    if (ui.modal?.kind !== "search") return;
    // The footer (what can be searched) from the start.
    untrack(runSearch);
    const timer = setInterval(() => ui.modal?.out?.building && runSearch(), 700);
    return () => clearInterval(timer);
  });

  // Ask: a question answered by the user's chat model from the passages closest to it, with
  // the sources numbered. Follow-ups carry the turns before; closing Find file forgets them.
  const askReady = () => ui.cfg.settings.search_meaning && !!ui.cfg.settings.ask_model;
  // Find open: the chat model is asked whether it can answer (one that only makes vectors
  // cannot), once per model while Find file is open.
  let askProblem = $state(null);
  let askChecked = null;
  $effect(() => {
    const model = ui.modal?.kind === "search" && askReady() ? ui.cfg.settings.ask_model : "";
    if (ui.modal?.kind !== "search") askChecked = null;
    if (!model || model === askChecked) return;
    askChecked = model;
    askProblem = null;
    invoke("ask_check", { tryIt: false }).then((p) => {
      askProblem = p;
      runSearch();
    }, () => {});
  });
  /** Why Ask cannot be asked now, as Find's rows say it; null when it can. */
  const askOff = (m) => m.out?.ask ?? (askReady() ? (askProblem ? { off: "ask_model", detail: askProblem } : null) : { off: "ask_needs_meaning" });
  async function askNow(m) {
    const question = prefix(m.query).rest.trim();
    if (!question || m.asking || askOff(m)) return;
    const earlier = (m.chat ?? []).filter((c) => c.a && !c.error).map((c) => [c.q, c.a]);
    m.chat = [...(m.chat ?? []), { q: question, a: "", sources: [], error: "" }];
    const turn = m.chat[m.chat.length - 1];
    Object.assign(m, { query: "", asking: true, cursor: 0, show: "answer" });
    const events = new Channel();
    events.onmessage = (e) => {
      if (ui.modal !== m) return void invoke("ask_stop");
      if (e.k === "sources") Object.assign(turn, { sources: e.paths, similar: e.similar ?? [], read: e.read });
      else turn.a += e.text;
      // Follows the answer down, unless you scrolled up to read something.
      const end = !listEl || listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 40;
      if (end) tick().then(() => listEl?.lastElementChild?.scrollIntoView({ block: "end" }));
    };
    try {
      await invoke("ask", { question, earlier, scope: scopeOf(m), here: tab().dir, onEvent: events });
    } catch (e) {
      turn.error = String(e);
    } finally {
      m.asking = false;
    }
  }
  /** An answer in pieces: text, and the [n] that point at its sources. */
  const cited = (text) => text.split(/(\[\d+\])/).map((part) => ({ part, n: /^\[\d+\]$/.test(part) ? +part.slice(1, -1) : 0 }));
  const lastSources = (m) => m.chat?.at(-1)?.sources ?? [];

  /** The step of a row that says what is missing. */
  async function findStep(m, off) {
    if (off.off === "no_helper") {
      await invoke("index_action", { what: "restart" }).catch(() => {});
      return runSearch();
    }
    if (off.off === "not_read") {
      ui.cfg = await invoke("find_read_too", { dir: off.detail }).catch((e) => (ui.status = String(e), ui.cfg));
      return runSearch();
    }
    ui.modal = { kind: "setup" };
  }
  /** Ask's chat model is slow here: the better one is taken (and Find asks again), or Settings
   *  opens at Ask, whose line says what to do. */
  async function takeBetter(poor) {
    if (!poor.button) return void (ui.modal = { kind: "settings", section: "ask_model" });
    ui.cfg = await invoke("save_settings", { changes: poor.changes }).catch((e) => ((ui.status = String(e)), ui.cfg));
    askChecked = null;
    runSearch();
  }
  /** Sends a tip away for good. */
  function dismissRow(m, r) {
    invoke("dismiss_notice", { id: r.dismiss }).then(runSearch, () => {});
  }
  /** Enter on a row: go to a hit, ask, show a group alone, or take a row's step. */
  function enterRow(m, r) {
    if (!r) return;
    if (r.row === "hit") goToHit(r.hit);
    else if (r.row === "ask" && !r.off) askNow(m);
    else if (r.row === "slow") takeBetter(r.poor);
    else if (r.row === "more") refind(m, { chip: { names: "names", in_files: "in_files", history: "in_files", about: "about" }[r.group] });
    else if (r.off) findStep(m, r.off);
  }

  async function goToHit(h) {
    close();
    const tb = tab();
    // A commit: its folder as it was then.
    if (isHistory(h.path)) return cd(tb, h.path);
    await showInTab(tb, h.path);
    // A game in a PGN file: that game, on the board in the preview.
    if (h.snippet && ["pgn", "pdn", "shogi", "pbn"].includes(chessFormat(basename(h.path)))) {
      const index = await invoke("chess_hit", { path: h.path, snippet: h.snippet });
      if (index != null) {
        ui.chessAt = { path: h.path, index };
        ui.showPreview = true;
      }
    }
    // A game in an SGF file: the same.
    if (h.snippet && chessFormat(basename(h.path)) === "sgf") {
      const index = await invoke("go_hit", { path: h.path, snippet: h.snippet });
      if (index != null) {
        ui.goAt = { path: h.path, index };
        ui.showPreview = true;
      }
    }
  }

  // ------------------------------------------------------------ batch rename

  let planSeq = 0;
  export async function replan() {
    const m = ui.modal;
    if (m?.kind !== "rename") return;
    const n = ++planSeq;
    try {
      const plan = await invoke("rename_plan", {
        dir: m.dir,
        selected: m.names,
        pattern: m.pattern || "^$",
        replacement: m.replacement,
        flags: { case_insensitive: m.ci, global: m.global, whole_name: m.whole },
      });
      if (n === planSeq) Object.assign(m, { plan, error: "" });
    } catch (e) {
      if (n === planSeq) Object.assign(m, { plan: [], error: String(e) });
    }
  }

  async function applyRename() {
    const m = ui.modal;
    const changed = m.plan.filter((p) => p.from !== p.to);
    if (!changed.length || m.plan.some((p) => p.conflict)) return;
    try {
      await invoke("rename_apply", { dir: m.dir, plan: $state.snapshot(m.plan) });
      await refreshUndo(tn("dialogs.renamed", changed.length));
      close();
      load(tab());
    } catch (e) {
      m.error = String(e);
    }
  }

  // ------------------------------------------------------------ permissions and dates

  const WINDOWS = typeof navigator !== "undefined" && /Windows/.test(navigator.userAgent);
  const attrsRel = (m, p) => (m.base && p.startsWith(m.base) ? p.slice(m.base.length).replace(/^[\\/]+/, "") : "") || p;

  /** The plan (`writing` false), or the changes it shows; how far it is, polled. */
  async function attrsRun(m, writing) {
    m.error = "";
    m.busy = { writing, files: 0, stopping: false };
    const poll = setInterval(async () => {
      const n = await invoke("attrs_progress").catch(() => null);
      if (n != null && m.busy) m.busy.files = n;
    }, 200);
    const args = { roots: m.roots, spec: $state.snapshot(m.spec), base: m.base };
    try {
      if (!writing) {
        m.plan = null;
        const r = await invoke("attrs_plan", args);
        Object.assign(m, { plan: r.plan, rows: r.rows });
        if (!r.plan.total) m.error = t("attrs.none");
        return;
      }
      const r = await invoke("attrs_apply", { token: m.plan.token });
      close();
      await refreshUndo(r.summary);
      reloadAll();
      if (r.failed.length) ui.modal = failure(t("attrs.failed"), r.failed.join("\n"));
    } catch (e) {
      m.error = String(e);
    } finally {
      clearInterval(poll);
      m.busy = null;
    }
  }

  function attrsKey(m, k) {
    if (k === "Esc") {
      if (!m.busy) close();
      else if (!m.busy.stopping) (m.busy.stopping = true), invoke("attrs_stop");
    } else if (k === "Enter" && !m.busy) attrsRun(m, m.plan?.total > 0);
    else return false;
    return true;
  }

  function attrsState(m) {
    const b = m.busy;
    if (b) return `${t(b.writing ? "attrs.writing" : "attrs.looking", { files: tn("preview.files", b.files) })} · ${b.stopping ? t("attrs.stopping") : t("attrs.stop_key", { key: "Esc" })}`;
    if (!m.plan) return t(WINDOWS ? "attrs.about_windows" : "attrs.about");
    let s = tn("attrs.planned", m.plan.total);
    if (m.plan.refused.length) s += ` · ${t("attrs.left_out", { n: m.plan.refused.length })}`;
    if (m.plan.stopped) s += ` · ${t("attrs.stopped_early")}`;
    return s;
  }

  // ------------------------------------------------------------ make one side like the other

  function syncKey(m, k) {
    if (k === "Esc") {
      if (!m.busy) close();
      else if (!m.busy.stopping) (m.busy.stopping = true), invoke("sync_stop");
    } else if (k === "Enter" && !m.busy) syncRun(m, m.plan?.total > 0);
    else if (k === "Space" && !m.busy) (m.delete = !m.delete), syncRun(m, false);
    else return false;
    return true;
  }

  function syncState(m) {
    const b = m.busy;
    if (b) return `${t(b.writing ? "attrs.writing" : "attrs.looking", { files: tn("preview.files", b.files) })} · ${b.stopping ? t("attrs.stopping") : t("attrs.stop_key", { key: "Esc" })}`;
    return m.plan ? m.summary : "";
  }

  // ------------------------------------------------------------ find and replace

  /** A path as the list shows it: from the folder the files are in. */
  const replaceRel = (m, p) => (m.base && p.startsWith(m.base) ? p.slice(m.base.length).replace(/^[\\/]+/, "") : "") || p;
  function replaceTicked(m) {
    let changes = 0, files = 0;
    for (const tk of m.ticks) {
      const n = tk.filter(Boolean).length;
      changes += n;
      if (n) files++;
    }
    return { changes, files };
  }

  /** The plan (`writing` false), or the writing of what is ticked; how far it is, polled. */
  async function replaceRun(m, writing) {
    m.error = "";
    m.busy = { writing, files: 0, changes: 0, all: 0, stopping: false };
    const poll = setInterval(async () => {
      const p = await invoke("replace_progress").catch(() => null);
      if (p && m.busy) [m.busy.files, m.busy.changes] = p;
    }, 200);
    try {
      if (!writing) {
        m.plan = null;
        const plan = await invoke("replace_plan", { roots: m.roots, query: $state.snapshot(m.query) });
        m.ticks = plan.files.map((f) => f.changes.map(() => true));
        m.plan = plan;
        if (!plan.files.length) m.error = t("replace.none", { looked: tn("preview.files", plan.looked) });
        return;
      }
      const picks = m.plan.files.flatMap((f, i) => (m.ticks[i].some(Boolean) ? [{ file: i, skip: m.ticks[i].flatMap((on, j) => (on ? [] : [j])) }] : []));
      m.busy.all = picks.length;
      const r = await invoke("replace_apply", { picks, token: m.plan.token });
      close();
      await refreshUndo(r.summary);
      reloadAll();
      if (r.failed.length) ui.modal = failure(t("replace.failed"), r.failed.join("\n"));
    } catch (e) {
      m.error = String(e);
    } finally {
      clearInterval(poll);
      m.busy = null;
    }
  }

  function replaceKey(m, k) {
    if (k === "Esc") {
      if (!m.busy) close();
      else if (!m.busy.stopping) (m.busy.stopping = true), invoke("replace_stop");
    } else if (k === "Enter" && !m.busy) {
      const write = !!m.plan?.files.length && replaceTicked(m).changes > 0;
      replaceRun(m, write);
    }
    else return false;
    return true;
  }

  /** What the line under the options says: how far it is, or what Replace would do. */
  function replaceState(m) {
    const b = m.busy;
    if (b) {
      const line = b.writing ? t("replace.writing", { n: Math.min(b.files + 1, b.all), all: b.all }) : t("replace.looking", { files: tn("preview.files", b.files), changes: tn("replace.changes", b.changes) });
      return `${line} · ${b.stopping ? t("replace.stopping") : t("replace.stop_key", { key: "Esc" })}`;
    }
    if (!m.plan) return t("replace.about");
    const { changes, files } = replaceTicked(m);
    let s = t("replace.ticked", { changes: tn("replace.changes", changes), files: tn("preview.files", files) });
    const out = m.plan.skipped.length + m.plan.more + m.plan.binary;
    if (out) s += ` · ${t("replace.left_out", { n: out })}`;
    if (m.plan.full) s += ` · ${t("replace.full", { n: num(20000) })}`;
    else if (m.plan.stopped) s += ` · ${t("replace.stopped_early")}`;
    return s;
  }

  /** Find's files, for Find and replace: those it lists, what was searched in them typed in. */
  function replaceFound(m) {
    const files = [...new Set((m.out?.rows ?? []).filter((r) => r.row === "hit" && !r.hit.is_dir && !isHistory(r.hit.path)).map((r) => r.hit.path))];
    const text = m.chip === "in_files" || prefix(m.query).kind === "in_files";
    const dirs = files.map(parent);
    let base = dirs[0] ?? "";
    const under = (d) => d === base || d.startsWith(/[\\/]$/.test(base) ? base : base + (base.includes("\\") ? "\\" : "/"));
    while (base && !dirs.every(under)) {
      const up = parent(base);
      base = up === base ? "" : up;
    }
    openReplace(files, base, text ? prefix(m.query).rest.trim() : "");
  }

  // ------------------------------------------------------------ tags

  async function setTag(color) {
    const m = ui.modal;
    close();
    await invoke("set_tags", { paths: m.paths, color }).catch((e) => (ui.status = String(e)));
    load(tab());
  }

  // ------------------------------------------------------------ properties

  async function savePerms() {
    const m = ui.modal;
    const mode = m.props.mode === null ? null : parseInt(m.mode, 8);
    if (mode !== null && !validMode(m.mode)) return void (m.error = t("dialogs.perm_invalid"));
    try {
      // The user flags, when they changed (FreeBSD, macOS).
      const was = (m.props.flags?.user ?? []).filter(([, on]) => on).map(([n]) => n).join(" ");
      const on = Object.keys(m.flags).filter((n) => m.flags[n]);
      if (m.props.flags?.can_set && on.join(" ") !== was) await invoke("set_flags", { path: m.props.path, on });
      await invoke("set_permissions", { path: m.props.path, mode, readonly: m.readonly });
      close();
      load(tab());
    } catch (e) {
      m.error = String(e);
    }
  }

  const validMode = (s) => /^[0-7]{3,4}$/.test(s);

  /** rwxr-xr-x for 0o755. */
  const rwx = (mode) => [6, 3, 0].map((s) => ["r", "w", "x"].map((c, i) => (mode >> (s + 2 - i)) & 1 ? c : "-").join("")).join("");

  // Splits a text at its {placeholders}: odd entries are the placeholder names, so the
  // markup around them (code, bold) stays out of the translated text.
  const parts = (s) => s.split(/\{(\w+)\}/);

  // Screen-reader name for modals that have no title of their own.
  const KIND_LABEL = { linehistory: "action.line_history", help: "help.title", search: "search.title", rename: "action.batch_rename", props: "action.properties", tag: "action.tag" };

  // ------------------------------------------------------------ pack

  /** Pack, with the password if the target takes one and both fields say the same. */
  function packNow(m) {
    const locks = takesPassword(m.value);
    if (locks && m.password !== m.again) return;
    confirm(m, m.value, locks && m.password ? m.password : null, /\.7z$/i.test(m.value) && m.hide);
  }

  // ------------------------------------------------------------ menus

  const menuItems = (m) => m.items.filter((it) => it.label.toLowerCase().includes(m.filter.toLowerCase()));

  function run(it) {
    close();
    it.run();
  }

  /** Keys in Find: they go by action, so a user's own keys for Find, text and Ask work too. */
  function findKey(m, e, k, act) {
    const rows = m.out?.rows ?? [];
    const src = lastSources(m);
    const answer = answering(m);
    const question = prefix(m.query).rest.trim();
    if (k === "Esc") {
      if (m.show === "list") close();
      else refind(m, { show: "list", chip: m.chip === "ask" ? "all" : m.chip });
    } else if (k === "F1" || act === "help") {
      // F1 shows the syntax; again, the questions about Find.
      if (m.show === "syntax") features(m);
      else m.show = "syntax";
    }
    else if (act === "search") refind(m, { here: !m.here });
    else if (act === "replace" && !answer) replaceFound(m);
    else if (act === "search_text") refind(m, { chip: m.chip === "in_files" ? "all" : "in_files", show: "list" });
    else if (act === "ask" || k === "Ctrl+Enter") {
      // Ask not set up: the answer's place says what it needs.
      if (question && !askOff(m)) askNow(m);
      else Object.assign(m, { chip: question ? m.chip : "ask", show: "answer", cursor: 0 });
    } else if (k === "Tab" || k === "Shift+Tab") {
      const chip = nextKind(m.chip, k === "Shift+Tab");
      refind(m, { chip, query: prefix(m.query).rest, show: chip === "ask" ? "answer" : "list" });
    } else if (k === "Enter" && answer) {
      if (question) askNow(m);
      else if (src[m.cursor]) goToHit({ path: src[m.cursor] });
    } else if (k === "Enter") enterRow(m, rows[m.cursor]);
    else if (answer && k === "Up") m.cursor = Math.max(0, m.cursor - 1);
    else if (answer && k === "Down") m.cursor = Math.min(src.length - 1, m.cursor + 1);
    else if (k === "Up" || k === "Down" || k === "PageUp" || k === "PageDown") m.cursor = step(rows, m.cursor, { Up: -1, Down: 1, PageUp: -15, PageDown: 15 }[k]);
    else if (act === "edit" || act === "edit_here") {
      const p = answer ? src[m.cursor] : rows[m.cursor]?.row === "hit" && !rows[m.cursor].hit.is_dir ? rows[m.cursor].hit.path : null;
      if (!p || isHistory(p)) return true;
      // The built-in editor over the search, which comes back as it was when it closes.
      if (act === "edit_here") ui.modal = { kind: "editor", path: p, back: m };
      else invoke("edit_path", { path: p }).catch((err) => (ui.status = String(err)));
    } else if (k === "Delete" && rows[m.cursor]?.dismiss && e.target?.selectionStart === m.query.length) dismissRow(m, rows[m.cursor]);
    else return false;
    return true;
  }

  // ------------------------------------------------------------ git config

  /** An entry's value as it shows: its secret only when shown; a bare key is true. */
  const gcValue = (m, i) => (m.shown[i] ? m.entries[i].value : (m.entries[i].hidden ?? m.entries[i].value)) ?? "true";
  /** Where it comes from: its scope (included, set earlier) and its file. */
  const gcScope = (e) => [t(`gitconfig.scope_${e.scope}`), e.included && t("gitconfig.included"), e.earlier && t("gitconfig.earlier")].filter(Boolean).join(" · ");
  function gcShow(m, i) {
    if (m.entries[i]?.hidden != null) m.shown[i] = !m.shown[i];
  }
  /** The line to copy: hidden, unless its secret is shown (`Entry::copy` in core). */
  function gcCopy(m, i) {
    const e = m.entries[i];
    if (!e) return;
    const text = m.shown[i] ? (e.value == null ? e.key : `${e.key}=${e.value}`) : e.line;
    invoke("copy_text", { text }).then(() => (ui.status = t("gitconfig.copied", { line: e.line })), (err) => (ui.status = String(err)));
  }
  /** Its file in the editor; a value from the command line or a blob has none. */
  function gcEdit(m, i) {
    const e = m.entries[i];
    if (!e) return;
    if (e.file) invoke("edit_path", { path: e.file }).catch((err) => (ui.status = String(err)));
    else ui.status = t("gitconfig.no_file", { origin: e.origin });
  }

  /** Keys while a modal is open. */
  export function handleKey(e, k) {
    const m = ui.modal;
    const act = ui.cfg.keymap[k];
    if (m.kind === "search") return findKey(m, e, k, act);
    // F1 in a dialog or window: Features, at what it is about.
    if (act === "help" && m.kind !== "help") return features(m), true;
    if (m.kind === "guide") return m.key?.(k) ?? false;
    if (m.kind === "space") return m.key?.(k, act) ?? false;
    if (m.kind === "editor") return m.key?.(k, act, e) ?? false;
    if (m.kind === "compare") return m.key?.(k, act, e) ?? false;
    if (m.kind === "replace") return replaceKey(m, k);
    if (m.kind === "attrs") return attrsKey(m, k);
    if (m.kind === "sync") return syncKey(m, k);
    if (k === "Esc") return close(), true;
    switch (m.kind) {
      case "input":
        if (k !== "Enter") return false;
        confirm(m, m.value);
        return true;
      case "pack":
        if (k !== "Enter") return false;
        packNow(m);
        return true;
      case "confirm":
        if (k === "Enter" || k === "y" || k === "Shift+Y") confirm(m);
        else if (k === "n" || k === "Shift+N") close();
        return true;
      case "menu": {
        const items = menuItems(m);
        // An entry's key is one character, upper case for Shift (`Shift+A` is `A`).
        const ch = k.replace(/^Shift\+/, "");
        if (k === "Enter" && items[m.cursor]) run(items[m.cursor]);
        else if (k === "Up") m.cursor = Math.max(0, m.cursor - 1);
        else if (k === "Down") m.cursor = Math.min(items.length - 1, m.cursor + 1);
        else if (m.direct && [...ch].length === 1 && m.items.some((it) => it.key === ch)) run(m.items.find((it) => it.key === ch));
        else return false;
        return true;
      }
      case "gitconfig":
        if (k === "Up") m.cursor = Math.max(0, m.cursor - 1);
        else if (k === "Down") m.cursor = Math.min(m.entries.length - 1, m.cursor + 1);
        else if (k === "PageUp") m.cursor = Math.max(0, m.cursor - 15);
        else if (k === "PageDown") m.cursor = Math.min(m.entries.length - 1, m.cursor + 15);
        else if (k === "Home") m.cursor = 0;
        else if (k === "End") m.cursor = Math.max(0, m.entries.length - 1);
        else if (k === "Enter") gcShow(m, m.cursor);
        else if (act === "edit") gcEdit(m, m.cursor);
        else if (act === "clip_copy") gcCopy(m, m.cursor);
        else if (act === "git_tool") (close(), gitTool());
        else return false;
        return true;
      case "viruses": {
        // A choice per file: N leave it, B bin, Q quarantine; Enter does them all.
        const n = m.report.found.length;
        const c = { n: "nothing", b: "bin", q: "quarantine" }[k.replace(/^Shift\+/, "").toLowerCase()];
        if (k === "Up") m.cursor = Math.max(0, m.cursor - 1);
        else if (k === "Down") m.cursor = Math.min(n - 1, m.cursor + 1);
        else if (k === "Home") m.cursor = 0;
        else if (k === "End") m.cursor = n - 1;
        else if (c) m.choices[m.cursor] = c;
        else if (k === "Enter") applyViruses(m);
        else return false;
        return true;
      }
      case "props":
        if (k === "Enter") savePerms();
        else return false;
        return true;
      case "rename":
        if (k === "Enter") applyRename();
        else return false;
        return true;
      case "tag": {
        const n = Number(k);
        if (k === "0") setTag("");
        else if (n >= 1 && n <= TAGS.length) setTag(TAGS[n - 1]);
        return true;
      }
      case "bom":
      case "chess":
      case "go":
      case "settings":
      case "dupes":
      case "setup":
      case "provenance":
        // windows with fields and buttons of their own: Esc (above) closes them, Enter is theirs
        return false;
      case "linehistory": {
        // The list scrolls; every other key stays here.
        const by = { Up: -40, Down: 40, PageUp: -400, PageDown: 400, Home: -1e9, End: 1e9 }[k];
        if (by) listEl?.scrollBy({ top: by });
        else if (k === "Enter") close();
        return true;
      }
      case "help":
        if (!(act === "help" || (k === "Enter" && m.tab !== "features"))) return false;
        close();
        return true;
      default:
        if (k === "Enter" || act === "help") close();
        return m.kind === "message";
    }
  }
</script>

<!-- The keys of a dialog, in one line the terminal app shares: "Enter Copy · Esc Cancel". -->
{#snippet keys(verb)}<p class="keys">{t("dialog.keys", { verb, cancel: t("common.cancel") })}</p>{/snippet}

{#if ui.modal && !["dupes", "settings", "bom", "chess", "go", "space", "editor", "compare"].includes(ui.modal.kind)}
  {@const m = ui.modal}
  <div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
    <div class="dialog {m.kind}" role="dialog" aria-modal="true" aria-label={m.title ?? (KIND_LABEL[m.kind] ? t(KIND_LABEL[m.kind]) : m.kind)}>
      {#if m.kind === "input"}
        <h2>{m.title}</h2>
        <label>{m.label}{#if m.secret}<input bind:this={input} bind:value={m.value} type="password" autocomplete="off" />{:else}<input bind:this={input} bind:value={m.value} spellcheck="false" />{/if}</label>
        <!-- Into an archive: that the disk may be short of room for it written anew. -->
        {#if m.room}<p class="err">{m.room}</p>{/if}
        <div class="buttons">
          <button class="primary" onclick={() => confirm(m, m.value)}>{m.ok}</button>
          <button onclick={close}>{t("common.cancel")}</button>
        </div>
        {@render keys(m.ok)}
      {:else if m.kind === "pack"}
        {@const locks = takesPassword(m.value)}
        <h2>{m.title}</h2>
        {@const fmt = packFormat(ui.cfg.pack_formats, m.value)}
        <div class="packname">
          <label>{m.label}<input bind:this={input} bind:value={m.value} spellcheck="false" /></label>
          <label>{t("archive.format")}<select value={fmt?.id ?? ""} onchange={(e) => (m.value = withEnding(m.value, ui.cfg.pack_formats.find((f) => f.id === e.currentTarget.value).endings[0]))}>
            {#if !fmt}<option value="" disabled>—</option>{/if}
            {#each ui.cfg.pack_formats as f (f.id)}<option value={f.id}>{f.label}</option>{/each}
          </select></label>
        </div>
        {#if fmt}<p class="meta">{t(`archive.format_${fmt.id}`)}</p>{/if}
        <!-- That the disk may be short of room: the archive is written beside the old one first. -->
        {#if m.room}<p class="err">{m.room}</p>{/if}
        {#if locks}
          <div class="grid2">
            <label>{t("archive.pack_password")}<input bind:value={m.password} type="password" autocomplete="new-password" /></label>
            <label>{t("archive.pack_confirm")}<input bind:value={m.again} type="password" autocomplete="new-password" /></label>
          </div>
          {#if /\.7z$/i.test(m.value)}<label class="check"><input type="checkbox" bind:checked={m.hide} disabled={!m.password} /> {t("archive.pack_hide_names")}</label>{/if}
          {#if m.password !== m.again && m.again}<p class="err">{t("archive.pack_mismatch")}</p>{/if}
        {:else}
          <p class="meta">{t("archive.pack_no_password")}</p>
        {/if}
        <div class="buttons">
          <button class="primary" disabled={locks && m.password !== m.again} onclick={() => packNow(m)}>{t("verb.pack")}</button>
          <button onclick={close}>{t("common.cancel")}</button>
        </div>
        {@render keys(t("verb.pack"))}
      {:else if m.kind === "confirm"}
        <h2>{m.title}</h2>
        <p>{m.text}</p>
        <div class="buttons">
          <button class="primary" class:danger={!m.safe} bind:this={input} onclick={() => confirm(m)}>{m.ok}</button>
          <button onclick={close}>{t("common.cancel")}</button>
        </div>
        {@render keys(m.ok)}
      {:else if m.kind === "linehistory"}
        <!-- The commits that changed a function (or some lines), newest first, each change under it. -->
        <h2>{m.h.name ? t("blame.title_function", { name: basename(m.path), function: m.h.name }) : t("blame.title_lines", { name: basename(m.path), from: m.h.from, to: m.h.to })}</h2>
        <p class="meta">{t("blame.commits", { n: m.h.changes.length })}{#if m.h.more} · {t("blame.more_changes", { n: m.h.changes.length })}{/if}</p>
        <div class="lhlist" bind:this={listEl}>
          {#each m.h.changes as c, i (c.commit.hash)}
            <details open={i === 0}>
              <summary><span class="mono">{c.commit.hash.slice(0, 7)}</span> · {c.commit.author} · {date(c.commit.time)} · <b>{c.commit.subject}</b></summary>
              <pre class="mono code"><code class="hljs">{@html hljs.highlight(c.diff, { language: "diff" }).value}</code></pre>
            </details>
          {/each}
        </div>
        <div class="buttons"><button class="primary" onclick={close}>{t("common.close")}</button></div>
        <p class="keys">{t("dialog.keys_one", { verb: t("common.close") })}</p>
      {:else if m.kind === "message"}
        <h2>{m.title}</h2>
        <p>{m.text}</p>
        {#each m.notes ?? [] as note, i (i)}<p class="meta">{note}</p>{/each}
        {#if m.details}<details open={!!m.open}><summary>{t("dialog.details")}</summary><pre>{m.details}</pre></details>{/if}
        <div class="buttons">
          <!-- What is missing or off: Settings at the place to change it. -->
          {#if m.settings}<button onclick={() => (ui.modal = { kind: "settings", section: m.settings })}>{t("preview.settings")}</button>{/if}
          <button class="primary" bind:this={input} onclick={close}>{t("common.close")}</button>
        </div>
        <p class="keys">{t("dialog.keys_one", { verb: t("common.close") })}</p>
      {:else if m.kind === "viruses"}
        <!-- What a virus scan found: a choice per file, nothing done until Enter. -->
        <h2>{t("viruses.title")}</h2>
        <p>{t("viruses.found", { n: num(m.report.scanned) })} {t("viruses.quarantine_at", { path: m.quarantine })}</p>
        <ul class="gclist vlist" bind:this={listEl}>
          {#each m.report.found as f, i (f.path)}
            <li class="row" class:cursor={i === m.cursor} onmouseenter={() => (m.cursor = i)}>
              <select bind:value={m.choices[i]} aria-label={f.path}>
                {#each ["nothing", "bin", "quarantine"] as c (c)}<option value={c}>{t(`viruses.choice.${c}`)}</option>{/each}
              </select>
              <span class="value">{f.name}</span>
              <span class="file" title={f.path}><bdi>{f.path}</bdi></span>
            </li>
          {/each}
        </ul>
        {#each m.notes as note, i (i)}<p class="meta">{note}</p>{/each}
        <div class="buttons">
          <button class="primary" onclick={() => applyViruses(m)}>{t("viruses.apply")}</button>
          <button onclick={close}>{t("common.close")}</button>
        </div>
        <p class="keys">{t("viruses.keys", { verb: t("viruses.apply"), cancel: t("common.close") })}</p>
      {:else if m.kind === "help"}
        <h2>{t("dialogs.help_title", { version: ui.cfg.version })}</h2>
        <div class="tabs" role="tablist">
          <button role="tab" class:on={m.tab !== "features"} aria-selected={m.tab !== "features"} onclick={() => helpTab(m, "keys")}>{t("features.tab_keys")}</button>
          <button role="tab" class:on={m.tab === "features"} aria-selected={m.tab === "features"} onclick={() => helpTab(m, "features")}>{t("features.title")}</button>
          <span class="grow"></span>
          <button onclick={() => (ui.modal = { kind: "guide", step: 0 })}>{t("guide.show_again")}</button>
        </div>
        {#if m.tab === "features"}
          <input bind:this={input} bind:value={m.filter} placeholder={t("features.filter")} spellcheck="false" />
          <div class="help features">
            {#each m.list ?? [] as f, i (f.id)}
              {#if !m.filter && f.area !== m.list[i - 1]?.area}<h3>{f.area}</h3>{/if}
              <section>
                <div class="head">
                  <b>{f.title}</b>
                  {#each f.keys as [label, keys] (label)}<span class="key">{label} {#each keys.split(" / ") as k (k)}<kbd>{k}</kbd>{/each}</span>{/each}
                  <span class="grow"></span>
                  <button title={t("features.show_me_tip")} onclick={() => showMe(f)}>{t("features.show_me")}</button>
                </div>
                <p>{f.what}</p>
                <dl>
                  {#each f.qa as [q, a] (q)}<dt>{q}</dt><dd>{a}</dd>{/each}
                </dl>
              </section>
            {:else}
              {#if m.list}<p class="none">{t("features.none")}</p>{/if}
            {/each}
          </div>
        {:else}
        <div class="help" bind:this={input} tabindex="-1">
          <table>
            <tbody>
              <!-- Under group headings, the most used first; the first configured key first, and a
                   letter key written as on the keyboard: Ctrl+G, not Ctrl+g. -->
              {#each ui.cfg.groups as [group, names] (group)}
                <tr class="head"><th colspan="2">{group}</th></tr>
                {#each names as name (name)}
                  {@const label = actionLabel(name)}
                  {@const keys = keysOf(name)}
                  <tr><td>{label}</td><td>{#each keys as k, i (i)}<kbd>{k}</kbd>{/each}</td></tr>
                {/each}
              {/each}
            </tbody>
          </table>
          <p>{t("dialogs.help_mouse", { where: `${t("settings.title")} → ${t("settings.area.behaviour")} → ${t("setting.right_click")}` })}</p>
          <p>{t("dialogs.help_quick", { key: ui.cfg.actions.command_line?.[1] ?? "" })}</p>
          <p>{t("dialogs.help_views")}</p>
          <p>{#each parts(t("dialogs.help_syntax")) as s, i (i)}{#if i % 2}<b>{t("search.title")}</b>{:else}{s}{/if}{/each} <code>foo bar</code> · <code>foo|bar</code> · <code>!foo</code> · <code>*.rs</code> · <code>ext:rs;toml</code> · <code>file:</code> <code>folder:</code> · <code>src/ foo</code> · <code>case:</code></p>
          <p>
            {#each parts(t("dialogs.help_config")) as s, i (i)}{#if i % 2}<code>{s === "path" ? ui.cfg.config_path : "coxswain --dump-config"}</code>{:else}{s}{/if}{/each}
          </p>
        </div>
        {/if}
      {:else if m.kind === "menu"}
        <h2>{m.title}</h2>
        {#if m.note}<p class="meta">{m.note}</p>{/if}
        {#if !m.direct}
          <input bind:this={input} bind:value={m.filter} oninput={() => (m.cursor = 0)} placeholder={t("dialogs.filter")} spellcheck="false" />
        {/if}
        <ul class="list" bind:this={listEl}>
          {#each menuItems(m) as it, i (it.label + i)}
            <!-- Unfiltered, the list is m.items: a heading where the group changes. -->
            {#if !m.filter && it.group && it.group !== m.items[i - 1]?.group}<li class="head">{it.group}</li>{/if}
            <li>
              <button class:cursor={i === m.cursor} onclick={() => run(it)} onmouseenter={() => (m.cursor = i)}>
                {#if it.icon}<span class="glyph">{it.icon}</span>{/if}
                <!-- A second line that wraps: what a choice would cost (Long files). -->
                <span class="grow">{it.label}{#if it.sub}<span class="sub">{it.sub}</span>{/if}</span>
                {#if it.key}<kbd>{it.key}</kbd>{/if}
              </button>
            </li>
          {:else}
            <li class="none">{t("dialogs.no_matches")}</li>
          {/each}
        </ul>
      {:else if m.kind === "gitconfig"}
        <!-- The git config as git uses it: read-only; changed in the user's own git tool. -->
        <h2>{m.repo ? t("gitconfig.title", { repo: m.repo }) : t("gitconfig.title_outside")}</h2>
        <p class="meta">{t("gitconfig.about")}{#if !m.repo} {t("gitconfig.outside")}{/if}</p>
        {#if m.hint}<p class="meta">{t("hint.git_config_show", { key: "Enter" })}</p>{/if}
        <ul class="gclist" bind:this={listEl} style:--nw={`${Math.min(24, Math.max(4, ...m.entries.map((e) => e.name.length)))}ch`}>
          {#each m.entries as e, i (i)}
            {#if e.section !== m.entries[i - 1]?.section}<li class="head">[{e.section}]</li>{/if}
            <li class="row" class:cursor={i === m.cursor} class:earlier={e.earlier} onmouseenter={() => (m.cursor = i)}>
              <span class="name">{e.name}</span>
              <span class="value" title={e.hidden == null || m.shown[i] ? e.value : ""}>{gcValue(m, i)}</span>
              <span class="from" title={e.file ?? e.origin}>{gcScope(e)}</span>
              <!-- The file's end is what tells it apart: cut at its start. -->
              <span class="file" title={e.file ?? e.origin}><bdi>{e.file ?? e.origin}</bdi></span>
              <span class="acts">
                {#if e.hidden != null}<button class="link" onclick={() => gcShow(m, i)}>{m.shown[i] ? t("gitconfig.hide") : t("gitconfig.show")}</button>{/if}
                <button class="link" onclick={() => gcCopy(m, i)}>{t("common.copy")}</button>
                <button class="link" disabled={!e.file} title={e.file ? "" : t("gitconfig.no_file", { origin: e.origin })} onclick={() => gcEdit(m, i)}>{t("gitconfig.edit")}</button>
              </span>
            </li>
          {:else}
            <li class="none">{t("gitconfig.empty")}</li>
          {/each}
        </ul>
        <p class="meta">{t("gitconfig.change", { key: ui.cfg.actions.git_tool?.[1] || "F9" })} <button class="link" onclick={() => (close(), gitTool())}>{t("action.git_tool")}</button></p>
        <p class="keys">{t("gitconfig.keys", { edit: ui.cfg.actions.edit?.[1] ?? "", copy: ui.cfg.actions.clip_copy?.[1] ?? "", tool: ui.cfg.actions.git_tool?.[1] ?? "" })}</p>
      {:else if m.kind === "search"}
        {@const answer = answering(m)}
        {@const question = prefix(m.query).rest.trim()}
        {@const off = askOff(m)}
        {@const scopeKey = ui.cfg.find_key}
        {@const folder = tab().bin ? t("sidebar.bin") : basename(tab().dir)}
        <div class="search-bar">
          <span class="glyph">{"\u{f002}"}</span>
          <input bind:this={input} bind:value={m.query} oninput={runSearch} placeholder={answer ? t("dialogs.ask_placeholder") : t(m.here ? "find.placeholder_here" : "find.placeholder_everywhere", { folder, key: scopeKey })} spellcheck="false" />
          <!-- The scope: everywhere, or the active pane's folder; Ctrl+F (the search key) flips it. -->
          <div class="scopes" role="radiogroup" aria-label={scopeKey}>
            <button class="scope" class:on={!m.here} role="radio" aria-checked={!m.here} title={scopeKey} onclick={() => refind(m, { here: false })}>{t("dialogs.scope_everywhere")}</button>
            <button class="scope" class:on={m.here} role="radio" aria-checked={m.here} title={`${tab().dir} · ${scopeKey}`} onclick={() => refind(m, { here: true })}>{t("dialogs.scope_in", { folder })}</button>
          </div>
        </div>
        <!-- The kinds: Tab and Shift+Tab, or a click; a prefix typed first turns its kind on. -->
        <div class="scopes kinds" role="radiogroup" aria-label={t("search.title")}>
          {#each KINDS as kind (kind)}
            {@const on = (answer ? "ask" : kindOf(m)) === kind}
            <button class="scope" class:on role="radio" aria-checked={on} title="Tab / Shift+Tab" onclick={() => refind(m, { chip: kind, query: prefix(m.query).rest, show: kind === "ask" ? "answer" : "list" })}>{t(`find.kind_${kind}`)}</button>
          {/each}
        </div>
        {#if m.show === "syntax"}
          <div class="list syntax">
            <p>{#each parts(t("dialogs.help_syntax")) as s, i (i)}{#if i % 2}<b>{t("search.title")}</b>{:else}{s}{/if}{/each} <code>foo bar</code> · <code>foo|bar</code> · <code>!foo</code> · <code>*.rs</code> · <code>ext:rs;toml</code> · <code>file:</code> <code>folder:</code> · <code>src/ foo</code> · <code>case:</code></p>
            <p>{t("find.help_prefixes")}</p>
          </div>
        {:else if answer}
          {#if off}
            <p class="meta tip" class:err={off.off === "ask_model"}>
              {off.off === "ask_model" ? off.detail : t(off.off === "ask_no_model" ? "find.off_ask_model" : "find.off_ask_meaning")}
              <button class="link" onclick={() => (ui.modal = { kind: "setup" })}>{t("find.step_set_up")}</button>
            </p>
          {:else if !m.chat?.length}
            <p class="meta">{t("dialogs.ask_hint", { model: ui.cfg.ask_name })}</p>
          {/if}
          <div class="list chat" bind:this={listEl}>
            {#each m.chat ?? [] as c, ci (ci)}
              <p class="question" dir="auto">{c.q}</p>
              <p class="answer" dir="auto">
                {#each cited(c.a) as { part, n }, pi (pi)}{#if n && c.sources[n - 1]}<button class="cite" title={c.sources[n - 1]} onclick={() => goToHit({ path: c.sources[n - 1] })}>{part}</button>{:else}{part}{/if}{/each}{#if m.asking && ci === m.chat.length - 1}<span class="typing">▍</span>{/if}
              </p>
              {#if m.asking && ci === m.chat.length - 1 && !c.a && c.sources.length}<p class="meta">{t("dialogs.ask_waiting", { model: ui.cfg.ask_name })}</p>{/if}
              {#if c.error}<p class="err">{c.error}</p>{/if}
              {#if c.sources.length}
                {#if c.read}<p class="meta">{c.read}</p>{/if}
                <ol class="sources">
                  {#each c.sources as src, i (i)}
                    <li><button class:cursor={ci === m.chat.length - 1 && i === m.cursor} onclick={() => goToHit({ path: src })}><b>{basename(src)}</b> <span class="where"><bdi>{parent(src)}</bdi></span>{#if c.similar?.[i]} <span class="where">{tn("ask.similar_files", c.similar[i])}</span>{/if}</button></li>
                  {/each}
                </ol>
              {/if}
            {/each}
          </div>
        {:else if !question}
          <p class="meta empty">{t("find.empty")}{#if m.scopeHint && !m.here}<br />{t("hint.find_scope", { key: scopeKey, folder: basename(tab().dir) })}{/if}</p>
          <div class="list hits"></div>
        {:else if m.out && !m.out.rows.length}
          <p class="meta empty">
            {m.here ? t("find.nothing_in", { query: question, folder: basename(tab().dir) }) : t("find.nothing", { query: question })}
            {#if m.here}<br />{t("find.try_everywhere", { key: scopeKey })}{/if}
            {#if !off && question.split(/\s+/).length > 1}<br />{t("find.try_ask", { key: "Ctrl+Enter" })}{/if}
          </p>
          <div class="list hits"></div>
        {:else}
          <ul class="list hits" bind:this={listEl}>
            {#each m.out?.rows ?? [] as r, i (i)}
              <li class={r.row}>
                {#if r.row === "head"}
                  <span class="group">{t(`find.group_${r.group}`)}</span>{#if r.total}<span class="count">{t("find.of", { shown: num(r.shown), total: num(r.total) })}</span>{/if}
                {:else if r.row === "ask"}
                  <button class:cursor={i === m.cursor} class:off={r.off} onclick={() => (m.cursor = i)} ondblclick={() => enterRow(m, r)}>
                    <span class="glyph">?</span>
                    {#if r.off}<span>{r.text}</span>{:else}<b>{t("find.ask_row", { query: question })}</b><kbd>Ctrl+Enter</kbd>{/if}
                  </button>
                  {#if r.off}<button class="link step" onclick={() => findStep(m, r.off)}>{r.step}</button>{/if}
                  {#if r.dismiss}<button class="x" title={t("find.dismiss")} aria-label={t("find.dismiss")} onclick={() => dismissRow(m, r)}>×</button>{/if}
                {:else if r.row === "slow"}
                  <span class="offtext" class:cursor={i === m.cursor}>{r.poor.short}</span>
                  <button class="link step" onclick={() => takeBetter(r.poor)}>{r.poor.button ?? t("find.step_settings")}</button>
                {:else if r.row === "more"}
                  <button class="more" class:cursor={i === m.cursor} onclick={() => enterRow(m, r)}>{t("find.more", { n: num(r.n) })}</button>
                {:else if r.row === "off"}
                  <span class="offtext" class:cursor={i === m.cursor}>{r.text}</span>
                  <button class="link step" onclick={() => findStep(m, r.off)}>{r.step}</button>
                  {#if r.dismiss}<button class="x" title={t("find.dismiss")} aria-label={t("find.dismiss")} onclick={() => dismissRow(m, r)}>×</button>{/if}
                {:else}
                  {@const h = r.hit}
                  <button class:cursor={i === m.cursor} onclick={() => (m.cursor = i)} ondblclick={() => goToHit(h)}>
                    {#if isHistory(h.path)}
                      <!-- A commit: the repository it is in; the snippet says which commit. -->
                      <span class="glyph">{"\u{f417}"}</span>
                      <b>{basename(historyOf(h.path))}</b>
                      <span class="where"><bdi>{t("history.commit_in", { repo: historyOf(h.path) })}</bdi></span>
                    {:else}
                      <span class="glyph" class:dir={h.is_dir}>{h.is_dir ? "\u{f07b}" : "\u{f15b}"}</span>
                      <b>{basename(h.path)}</b>
                      <span class="where"><bdi>{parent(h.path)}</bdi></span>
                    {/if}
                    {#if h.similar != null}<span class="snippet about" dir="auto">{h.snippet}</span>
                    {:else if h.snippet}<span class="snippet" dir="auto">{@html marked(h.snippet)}</span>{/if}
                  </button>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
        <p class="meta">{m.out?.footer ?? ""}</p>
        <p class="meta">{answer ? t("find.keys_answer_gui") : t("find.keys_gui", { scope: scopeKey })}{#if !answer && m.out?.rows?.some((r) => r.row === "hit" && !r.hit.is_dir)} · <button class="link" onclick={() => replaceFound(m)}>{t("replace.in_find_button")}</button> <kbd>{keysOf("replace")[0] ?? ""}</kbd>{/if}</p>
      {:else if m.kind === "attrs"}
        <h2>{m.title}</h2>
        <div class="grid2">
          <label>{t("attrs.mode")}<input bind:this={input} bind:value={m.spec.mode} oninput={() => (m.plan = null)} disabled={!!m.busy} spellcheck="false" placeholder={WINDOWS ? "-w · +h" : "644 · u+x,go-w · a+X"} /></label>
          <label>{t("attrs.date")}<input bind:value={m.spec.date} oninput={() => (m.plan = null)} disabled={!!m.busy} spellcheck="false" placeholder={`${t("attrs.now")} · 2026-10-21 14:30`} /></label>
        </div>
        <div class="flags">
          {#if !WINDOWS}<label><input type="checkbox" bind:checked={m.spec.accessed} onchange={() => (m.plan = null)} disabled={!!m.busy} /> {t("attrs.accessed")}</label>{/if}
          <label><input type="checkbox" bind:checked={m.spec.recursive} onchange={() => (m.plan = null)} disabled={!!m.busy} /> {t("attrs.recursive")}</label>
        </div>
        <p class="meta">{attrsState(m)}</p>
        {#if m.error}<p class="err">{m.error}</p>{/if}
        {#if m.plan?.total}
          <ul class="list attrs-plan">
            {#each m.rows as [path, mode, date] (path)}
              <li><span class="name">{attrsRel(m, path)}</span><span>{mode}</span><span>{date}</span></li>
            {/each}
            {#if m.plan.total > m.rows.length}<li class="more">{tn("attrs.more", m.plan.total - m.rows.length)}</li>{/if}
          </ul>
        {/if}
        {#if m.plan?.refused.length}
          <details><summary>{t("attrs.left_out", { n: m.plan.refused.length })}</summary>
            <ul class="list skipped">{#each m.plan.refused as r, i (i)}<li class="why">{r}</li>{/each}</ul>
          </details>
        {/if}
        <div class="buttons">
          {#if m.busy}
            <button disabled={m.busy.stopping} onclick={() => attrsKey(m, "Esc")}>{t("searchable.stop")}</button>
          {:else if m.plan?.total}
            <button class="primary" onclick={() => attrsRun(m, true)}>{tn("attrs.go", m.plan.total)}</button>
            <button onclick={() => attrsRun(m, false)}>{t("attrs.plan")}</button>
          {:else}
            <button class="primary" disabled={!m.spec.mode.trim() && !m.spec.date.trim()} onclick={() => attrsRun(m, false)}>{t("attrs.plan")}</button>
          {/if}
          <button onclick={close} disabled={!!m.busy}>{t("common.cancel")}</button>
        </div>
        {@render keys(m.plan?.total ? tn("attrs.go", m.plan.total) : t("attrs.plan"))}
      {:else if m.kind === "sync"}
        <h2>{m.title}</h2>
        <p class="meta">{t("sync.about", { undo: keysOf("undo")[0] ?? "" })}</p>
        <div class="flags">
          <label><input type="checkbox" checked={m.delete} onchange={() => syncKey(m, "Space")} disabled={!!m.busy} /> {t("sync.delete", { dir: m.dir })} <kbd>Space</kbd></label>
        </div>
        <p class="meta">{syncState(m)}</p>
        {#if m.error}<p class="err">{m.error}</p>{/if}
        {#if m.plan?.total}
          <ul class="list sync-plan">
            {#each m.rows as [act, path, older, gone], i (i)}
              <li class:warn={older || gone}><span>{act}</span><span class="name">{path}</span><span>{older ? t("sync.older") : ""}</span></li>
            {/each}
            {#if m.plan.total > m.rows.length}<li class="more">{tn("attrs.more", m.plan.total - m.rows.length)}</li>{/if}
          </ul>
        {/if}
        {#if m.plan?.refused.length}
          <details><summary>{t("attrs.left_out", { n: m.plan.refused.length })}</summary>
            <ul class="list skipped">{#each m.plan.refused as r, i (i)}<li class="why">{r}</li>{/each}</ul>
          </details>
        {/if}
        <div class="buttons">
          {#if m.busy}
            <button disabled={m.busy.stopping} onclick={() => syncKey(m, "Esc")}>{t("searchable.stop")}</button>
          {:else if m.plan?.total}
            <button class="primary" onclick={() => syncRun(m, true)}>{t("sync.go", { n: m.plan.total })}</button>
            <button onclick={() => syncRun(m, false)}>{t("attrs.plan")}</button>
          {:else}
            <button class="primary" onclick={() => syncRun(m, false)}>{t("attrs.plan")}</button>
          {/if}
          <button onclick={close} disabled={!!m.busy}>{t("common.cancel")}</button>
        </div>
        {@render keys(m.plan?.total ? t("sync.go", { n: m.plan.total }) : t("attrs.plan"))}
      {:else if m.kind === "replace"}
        {@const ticked = replaceTicked(m)}
        <h2>{m.title}</h2>
        <div class="grid2">
          <label>{t("replace.find")}<input bind:this={input} bind:value={m.query.find} oninput={() => (m.plan = null)} disabled={!!m.busy} spellcheck="false" /></label>
          <label>{t("replace.with")}<input bind:value={m.query.with} oninput={() => (m.plan = null)} disabled={!!m.busy} spellcheck="false" placeholder={m.query.regex ? "$1 · ${name}" : ""} /></label>
        </div>
        <div class="flags">
          <label><input type="checkbox" bind:checked={m.query.regex} onchange={() => (m.plan = null)} disabled={!!m.busy} /> {t("replace.regex")}</label>
          <label><input type="checkbox" bind:checked={m.query.case} onchange={() => (m.plan = null)} disabled={!!m.busy} /> {t("replace.case")}</label>
          <label><input type="checkbox" bind:checked={m.query.word} onchange={() => (m.plan = null)} disabled={!!m.busy} /> {t("replace.word")}</label>
        </div>
        <p class="meta">{replaceState(m)}</p>
        {#if m.error}<p class="err">{m.error}</p>{/if}
        {#if m.plan?.files.length}
          <ul class="list changes">
            {#each m.plan.files as f, i (f.path)}
              {@const all = m.ticks[i].every(Boolean)}
              <li class="file"><label><input type="checkbox" checked={all} indeterminate={!all && m.ticks[i].some(Boolean)} onchange={() => (m.ticks[i] = m.ticks[i].map(() => !all))} /> <b>{replaceRel(m, f.path)}</b> <span class="count">({f.changes.length})</span></label></li>
              {#each f.changes as c, j (j)}
                <li class="change" class:off={!m.ticks[i][j]}>
                  <label><input type="checkbox" bind:checked={m.ticks[i][j]} /><span class="line">{c.line}</span><span class="before">− {c.before}</span><span class="after">+ {c.after}</span></label>
                </li>
              {/each}
            {/each}
          </ul>
        {/if}
        {#if m.plan?.skipped.length}
          <details><summary>{t("replace.left_out_head")} ({m.plan.skipped.length})</summary>
            <ul class="list skipped">{#each m.plan.skipped as sk (sk.path)}<li>{replaceRel(m, sk.path)} <span class="why">{sk.why}</span></li>{/each}</ul>
          </details>
        {/if}
        <div class="buttons">
          {#if m.busy}
            <button disabled={m.busy.stopping} onclick={() => replaceKey(m, "Esc")}>{t("searchable.stop")}</button>
          {:else if m.plan?.files.length}
            <button class="primary" disabled={!ticked.changes} onclick={() => replaceRun(m, true)}>{tn("replace.go", ticked.changes, { files: tn("preview.files", ticked.files) })}</button>
            <button onclick={() => replaceRun(m, false)}>{t("replace.preview_again")}</button>
          {:else}
            <button class="primary" disabled={!m.query.find} onclick={() => replaceRun(m, false)}>{t("replace.preview")}</button>
          {/if}
          <button onclick={close} disabled={!!m.busy}>{t("common.cancel")}</button>
        </div>
        {@render keys(m.plan?.files.length ? t("replace.replace") : t("replace.preview"))}
      {:else if m.kind === "rename"}
        <h2>{tn("dialogs.rename_title", m.names.length)}</h2>
        <div class="grid2">
          <label>{t("dialogs.rename_find")}<input bind:this={input} bind:value={m.pattern} oninput={replan} spellcheck="false" placeholder={t("dialogs.rename_find_hint")} /></label>
          <label>{t("dialogs.rename_replace")}<input bind:value={m.replacement} oninput={replan} spellcheck="false" placeholder={"$1 · {n} · {n:3}"} /></label>
        </div>
        <div class="flags">
          <label><input type="checkbox" bind:checked={m.ci} onchange={replan} /> {t("dialogs.rename_ignore_case")}</label>
          <label><input type="checkbox" bind:checked={m.global} onchange={replan} /> {t("dialogs.rename_all")}</label>
          <label><input type="checkbox" bind:checked={m.whole} onchange={replan} /> {t("dialogs.rename_extension")}</label>
        </div>
        {#if m.error}<p class="err">{m.error}</p>{/if}
        <ul class="list plan">
          {#each m.plan as p (p.from)}
            <li class:changed={p.from !== p.to} class:bad={p.conflict}>
              <span class="from">{p.from_shown}</span><span class="arrow">→</span><span class="to">{p.to_shown}</span>
              {#if p.conflict}<span class="why">{p.conflict}</span>{/if}
            </li>
          {/each}
        </ul>
        <div class="buttons">
          <button class="primary" disabled={!m.plan.some((p) => p.from !== p.to) || m.plan.some((p) => p.conflict)} onclick={applyRename}>{t("common.rename")}</button>
          <button onclick={close}>{t("common.cancel")}</button>
        </div>
        {@render keys(t("common.rename"))}
      {:else if m.kind === "props"}
        {@const p = m.props}
        <h2>{basename(p.path)}</h2>
        <dl class="props">
          <dt>{t("dialogs.location")}</dt><dd>{parent(p.path)}</dd>
          <dt>{t("dialogs.type")}</dt><dd>{t(`dialogs.kind.${p.kind}`)}{#if p.link_target} → {p.link_target}{/if}</dd>
          <dt>{t("dialogs.size")}</dt><dd>{#if p.unreadable}{t("dialogs.unreadable", { why: p.unreadable })}{:else}{size(p.size)}{#if p.size >= 10_240} {tn("dialogs.bytes", p.size)}{/if}{#if p.kind === "folder"} {tn("dialogs.in_files", p.files)}{/if}{/if}</dd>
          {#if p.created}<dt>{t("dialogs.created")}</dt><dd>{date(p.created)}</dd>{/if}
          {#if p.modified}<dt>{t("dialogs.modified")}</dt><dd>{date(p.modified)}</dd>{/if}
          {#if p.accessed}<dt>{t("dialogs.accessed")}</dt><dd>{date(p.accessed)}</dd>{/if}
          {#if p.mode !== null}
            <dt>{t("dialogs.owner")}</dt><dd>{#if p.owner}{p.owner} · {/if}{t("dialogs.owner_ids", { uid: p.uid, gid: p.gid })}</dd>
            <dt>{t("dialogs.permissions")}</dt>
            <dd class="perm"><input bind:this={input} bind:value={m.mode} size="5" spellcheck="false" /> <code>{validMode(m.mode) ? rwx(parseInt(m.mode, 8)) : ""}</code></dd>
          {:else}
            <dt>{t("dialogs.attributes")}</dt><dd><label class="check"><input type="checkbox" bind:checked={m.readonly} /> {t("dialogs.readonly")}</label></dd>
          {/if}
          {#if p.flags}
            <!-- chflags: the user flags to tick when you own it; the system ones only shown. -->
            <dt>{t("props.flags")}</dt>
            <dd>
              {p.flags.set.length ? p.flags.set.join(", ") : t("props.flags_none")}
              {#if p.flags.can_set && p.flags.user.length}
                <span class="flags">{#each p.flags.user as [n] (n)}<label class="check"><input type="checkbox" bind:checked={m.flags[n]} /> <code>{n}</code></label>{/each}</span>
              {/if}
              {#if p.flags.system}<small class="hint">{t("props.flags_system")}</small>{/if}
            </dd>
          {/if}
          {#if p.package}
            <dt>{t("props.package")}</dt>
            <dd>{p.package} <button class="link" onclick={() => { const path = p.path; close(); openPackage(tab(), path); }}>{t("action.package")}</button></dd>
          {/if}
          {#each [...(p.disk_image ?? []), ...(p.facts ?? [])] as [k, v], i (i)}<dt>{k}</dt><dd>{v}</dd>{/each}
          {#if p.zfs}
            {@const z = p.zfs}
            <dt>{t("props.dataset")}</dt><dd>{z.dataset}</dd>
            <dt>{t("props.mountpoint")}</dt><dd>{z.mountpoint}</dd>
            <dt>{t("props.compression")}</dt><dd>{t("props.compression_value", { compression: z.compression, ratio: z.compressratio })}</dd>
            <dt>{t("props.space")}</dt><dd>{t("props.space_value", { used: size(z.used), free: size(z.available), referenced: size(z.referenced) })}</dd>
            {#if z.quota || z.refquota}
              <dt>{t("props.quota")}</dt><dd>{t("props.quota_value", { quota: z.quota ? size(z.quota) : t("props.none"), refquota: z.refquota ? size(z.refquota) : t("props.none") })}</dd>
            {/if}
            {#if p.snapshots !== null}
              <dt>{t("props.snapshots")}</dt><dd>{t("props.snapshots_value", { n: p.snapshots, key: ui.cfg.actions.snapshots?.[1] ?? "" })}</dd>
            {/if}
          {/if}
        </dl>
        {#if p.disk_next}<DiskNext next={p.disk_next} onsettings={close} />{/if}
        {#if m.error}<p class="err">{m.error}</p>{/if}
        <div class="buttons">
          <button class="primary" onclick={savePerms}>{t("common.apply")}</button>
          <button onclick={close}>{t("common.close")}</button>
        </div>
        <p class="keys">{t("dialog.keys", { verb: t("common.apply"), cancel: t("common.close") })}</p>
      {:else if m.kind === "tag"}
        <h2>{t("action.tag")}</h2>
        <div class="tags">
          {#each TAGS as c, i (c)}
            <button onclick={() => setTag(c)} title={tagName(c)}><span class="dot" style:background={TAG_COLORS[c]}></span>{i + 1} {tagName(c)}</button>
          {/each}
          <button onclick={() => setTag("")}><span class="dot none"></span>0 {t("dialogs.tag_none")}</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .keys {
    margin: 0;
    text-align: end;
    font-size: 0.85em;
    opacity: 0.7;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: start center;
    padding-top: 12vh;
    background: rgb(0 0 0 / 0.45);
    z-index: 10;
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: min(460px, 92vw);
    max-width: 92vw;
    max-height: 76vh;
    padding: 16px 18px;
    background: var(--dialog-bg);
    color: var(--dialog-fg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
    box-sizing: border-box;
  }
  .dialog.search,
  .dialog.rename,
  .dialog.replace,
  .dialog.attrs,
  .dialog.sync,
  .dialog.help {
    width: min(900px, 92vw);
  }
  /* Help's two tabs: the keys, and every feature with its questions. */
  .tabs {
    display: flex;
    gap: 6px;
    align-items: center;
    margin-bottom: 8px;
  }
  .tabs [role="tab"].on {
    border-color: var(--accent-bg);
    font-weight: 600;
  }
  .features {
    margin-top: 8px;
  }
  .features h3 {
    margin: 14px 0 4px;
    font-size: 1em;
    color: var(--directory-fg);
  }
  .features section {
    padding: 6px 0 8px;
    border-bottom: 1px solid color-mix(in srgb, var(--border-fg) 50%, transparent);
  }
  .features .head {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    align-items: center;
  }
  .features .key {
    opacity: 0.85;
  }
  .features p,
  .features dl {
    margin: 4px 0 0;
  }
  .features dt {
    font-weight: 600;
    margin-top: 4px;
  }
  .features dd {
    margin: 0 0 0 1em;
  }
  .dialog.search {
    height: 70vh;
    padding: 10px 12px;
  }
  .dialog.menu {
    width: min(520px, 92vw);
  }
  .dialog.gitconfig,
  .dialog.viruses {
    width: min(1000px, 92vw);
  }
  .gclist.vlist .row {
    grid-template-columns: max-content max-content minmax(0, 1fr);
  }
  .gclist.vlist select {
    width: auto;
  }
  .dialog.linehistory {
    width: min(1100px, 94vw);
    max-height: 90vh;
    display: flex;
    flex-direction: column;
  }
  .lhlist {
    overflow: auto;
    min-height: 0;
  }
  .lhlist summary {
    cursor: pointer;
    padding: 4px 0;
  }
  .lhlist pre {
    margin: 4px 0 10px;
    font-size: 0.9em;
    white-space: pre;
    overflow-x: auto;
  }
  .lhlist :global(.hljs-addition) { color: var(--git-added-fg); }
  .lhlist :global(.hljs-deletion) { color: var(--git-deleted-fg); }
  .lhlist :global(.hljs-meta) { color: var(--git-renamed-fg); }
  .gclist {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: auto;
    min-height: 0;
  }
  .gclist li.head {
    padding: 10px 10px 2px;
    font-weight: 600;
    color: var(--directory-fg);
  }
  .gclist .row {
    display: grid;
    grid-template-columns: var(--nw) minmax(0, 1fr) max-content minmax(0, 18em) max-content;
    align-items: baseline;
    gap: 12px;
    padding: 3px 10px;
    border-radius: var(--r);
    white-space: nowrap;
  }
  .gclist .row.cursor {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .gclist .row.earlier {
    opacity: 0.55;
  }
  .gclist .name,
  .gclist .value {
    font-family: var(--mono-font), var(--scripts);
  }
  .gclist .name,
  .gclist .value,
  .gclist .file {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .gclist .from,
  .gclist .file {
    font-size: 0.85em;
  }
  .gclist .file {
    direction: rtl;
    text-align: left;
  }
  .gclist .acts {
    display: flex;
    gap: 8px;
    justify-content: end;
    font-size: 0.85em;
  }
  .gclist li.none {
    padding: 10px;
  }
  h2 {
    margin: 0;
    font-size: 1.05em;
    font-weight: 600;
  }
  p {
    margin: 0;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--hidden-fg);
    font-size: 0.9em;
  }
  input:not([type="checkbox"]) {
    font: inherit;
    font-size: 1rem;
    color: var(--dialog-input-fg);
    background: var(--dialog-input-bg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 7px 10px;
    outline: none;
  }
  input:focus {
    border-color: var(--accent-bg);
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    cursor: pointer;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .buttons button {
    padding: 6px 16px;
    border-radius: var(--r);
    border: 1px solid var(--border-fg);
  }
  .buttons .primary {
    background: var(--accent-bg);
    color: var(--accent-fg);
    border-color: transparent;
  }
  .buttons .danger {
    background: var(--git-deleted-fg);
  }
  .buttons button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  pre {
    margin: 0;
    white-space: pre-wrap;
    font-family: var(--mono-font), var(--scripts);
    overflow: auto;
  }
  .glyph {
    font-family: var(--icon-font);
    color: var(--hidden-fg);
  }
  .glyph.dir {
    color: var(--directory-fg);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: auto;
    min-height: 0;
  }
  .list li button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 6px 10px;
    border-radius: var(--r);
    text-align: start;
    white-space: nowrap;
  }
  .list button.cursor {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .grow {
    flex: 1;
  }
  .grow:has(.sub) {
    min-width: 0;
  }
  .grow .sub {
    display: block;
    white-space: normal;
    font-size: 0.9em;
    opacity: 0.8;
  }
  kbd {
    font-family: var(--mono-font), var(--scripts);
    font-size: 0.8em;
    padding: 1px 6px;
    margin-inline-start: 4px;
    border-radius: var(--r-sm);
    border: 1px solid var(--border-fg);
    color: var(--hidden-fg);
  }
  .none {
    padding: 8px 10px;
    color: var(--hidden-fg);
  }
  .search-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 6px;
  }
  .search-bar input {
    flex: 1;
    font-size: 1.1rem;
    border: 0;
    background: transparent;
    padding: 6px 0;
  }
  .scopes {
    display: flex;
    gap: 4px;
  }
  .search-bar .scope {
    max-width: 16em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope.on {
    color: var(--accent-fg);
    background: var(--accent-bg);
    border-color: transparent;
  }
  .tip .link {
    background: none;
    border: 0;
    padding: 0;
    color: var(--accent-bg);
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
  }
  .scope {
    padding: 4px 10px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r-pill);
    font-size: 0.85em;
    color: var(--hidden-fg);
  }
  .meta {
    font-size: 0.85em;
    color: var(--hidden-fg);
    padding: 0 8px;
    /* A note may list files, one a line. */
    white-space: pre-line;
  }
  .chat {
    overflow-y: auto;
    padding: 0 0.25em;
  }
  .chat .question {
    font-weight: bold;
    margin: 0.8em 0 0.3em;
  }
  .chat .answer {
    white-space: pre-wrap;
    margin: 0 0 0.4em;
    line-height: 1.45;
  }
  .chat .cite {
    all: unset;
    cursor: pointer;
    color: var(--accent, currentColor);
    text-decoration: underline;
  }
  .chat .sources {
    margin: 0 0 0.6em;
    padding-inline-start: 2em;
    font-size: 0.9em;
  }
  .chat .sources button {
    all: unset;
    cursor: pointer;
    display: block;
    width: 100%;
  }
  .chat .sources button.cursor {
    outline: 1px solid currentColor;
  }
  .typing {
    animation: blink 1s steps(2) infinite;
  }
  @keyframes blink {
    to {
      opacity: 0;
    }
  }
  .kinds {
    padding: 0 6px 6px;
  }
  .hits li.head {
    display: flex;
    justify-content: space-between;
    padding: 8px 8px 2px;
    font-size: 0.8em;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--hidden-fg);
  }
  .hits li.ask,
  .hits li.slow,
  .hits li.off {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .hits li.ask > button:first-child {
    flex: 1;
  }
  .hits li.slow .offtext,
  .hits li.off .offtext {
    padding: 4px 8px 4px 1.9em;
    color: var(--hidden-fg);
  }
  .hits li.slow .offtext.cursor,
  .hits li.off .offtext.cursor {
    outline: 1px solid currentColor;
  }
  .hits .link,
  .hits .x {
    background: none;
    border: 0;
    padding: 0 4px;
    cursor: pointer;
    font: inherit;
  }
  .hits .link {
    color: var(--accent-bg);
    text-decoration: underline;
  }
  .hits .x {
    color: var(--hidden-fg);
  }
  .hits .more {
    color: var(--hidden-fg);
    padding-inline-start: 1.9em;
    font-size: 0.9em;
  }
  .hits kbd {
    margin-inline-start: auto;
    font-size: 0.8em;
    color: var(--hidden-fg);
  }
  .snippet.about {
    font-style: italic;
  }
  .empty {
    padding: 12px 8px;
  }
  .syntax {
    flex: 1;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .hits {
    flex: 1;
    border-top: 1px solid var(--border-fg);
    padding-top: 6px;
  }
  .hits b {
    font-weight: 600;
    color: var(--search-hit-fg);
  }
  .list button.cursor b {
    color: inherit;
  }
  .snippet {
    flex-basis: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding-inline-start: 1.9em;
    color: var(--hidden-fg);
    font-size: 0.9em;
    text-align: start;
  }
  .snippet :global(mark) {
    color: var(--search-hit-fg);
    background: none;
    font-weight: 600;
  }
  .list.hits button:has(.snippet) {
    flex-wrap: wrap;
    row-gap: 0;
  }
  .where {
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
    color: var(--hidden-fg);
    font-size: 0.9em;
  }
  .packname {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: end;
    gap: 10px;
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .flags {
    display: flex;
    gap: 18px;
  }
  .flags label {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    color: var(--dialog-fg);
  }
  /* One grid for all rows, so a conflict's reason does not push its row out of line. */
  .plan {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr auto 1fr auto;
    align-content: start;
    column-gap: 10px;
    font-family: var(--mono-font), var(--scripts);
    font-size: 0.9em;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 6px 10px;
  }
  .plan li {
    display: grid;
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    padding: 2px 0;
    color: var(--hidden-fg);
    white-space: nowrap;
  }
  .plan span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .plan .changed .to {
    color: var(--git-added-fg);
  }
  .plan .bad .to,
  .why,
  .err {
    color: var(--git-deleted-fg);
  }
  .why {
    font-family: var(--font), var(--scripts);
    font-size: 0.85em;
  }
  /* Find and replace: a file's line, then each change with its line number, before and after. */
  .changes {
    flex: 1;
    overflow: auto;
    max-height: 55vh;
    font-family: var(--mono-font), var(--scripts);
    font-size: 0.9em;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 6px 10px;
  }
  .changes .file label {
    display: flex;
    flex-direction: row;
    justify-content: flex-start;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
  }
  .changes .change label {
    display: grid;
    grid-template-columns: auto 4em 1fr;
    column-gap: 8px;
    padding-left: 1.5em;
  }
  .changes .change span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: pre;
  }
  .changes .line {
    color: var(--hidden-fg);
    text-align: right;
    grid-row: span 2;
  }
  .changes .before {
    color: var(--git-deleted-fg);
  }
  .changes .after {
    grid-column: 3;
    color: var(--git-added-fg);
  }
  .changes .off .after {
    color: var(--hidden-fg);
    text-decoration: line-through;
  }
  .skipped {
    font-size: 0.9em;
  }
  .tags {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 6px;
  }
  .tags button {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--r);
    border: 1px solid var(--border-fg);
  }
  .tags button:hover {
    background: var(--cursor-bg);
  }
  .dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
  }
  .dot.none {
    border: 1px dashed var(--hidden-fg);
  }
  .props {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 16px;
    margin: 0;
  }
  .flags {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 12px;
    margin-top: 2px;
  }
  .hint {
    display: block;
    opacity: 0.7;
  }
  .link {
    font: inherit;
    padding: 0 6px;
    margin-inline-start: 6px;
    text-decoration: underline;
    cursor: pointer;
  }
  .props dt {
    color: var(--hidden-fg);
  }
  .props dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .perm {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .perm input {
    width: 5em;
    font-family: var(--mono-font), var(--scripts);
    padding: 3px 8px;
  }
  .check {
    flex-direction: row;
    align-items: center;
    gap: 6px;
    color: var(--dialog-fg);
  }
  .help {
    overflow: auto;
    outline: none;
  }
  .help table {
    width: 100%;
    border-collapse: collapse;
  }
  .help td {
    padding: 3px 6px;
    border-bottom: 1px solid color-mix(in srgb, var(--border-fg) 50%, transparent);
  }
  .help th,
  .list li.head {
    padding: 12px 6px 4px;
    text-align: start;
    font-weight: 600;
    color: var(--directory-fg);
  }
  .list li.head {
    padding-inline: 10px;
  }
  .help p {
    margin-top: 10px;
    color: var(--hidden-fg);
  }
  code {
    font-family: var(--mono-font), var(--scripts);
  }
  /* Permissions and dates: each file, then its bits and its date, before → after. */
  .attrs-plan {
    overflow: auto;
    max-height: 55vh;
    display: grid;
    grid-template-columns: 1fr auto auto;
    column-gap: 16px;
    font-family: var(--mono-font), var(--scripts);
    font-size: 0.9em;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 6px 10px;
  }
  .attrs-plan li {
    display: grid;
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    white-space: nowrap;
  }
  .attrs-plan .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .attrs-plan .more {
    color: var(--hidden-fg);
  }
  /* Make one side like the other: each step's word, its path, and why it stands out. */
  .sync-plan {
    overflow: auto;
    max-height: 50vh;
    display: grid;
    grid-template-columns: auto 1fr auto;
    column-gap: 16px;
    font-family: var(--mono-font), var(--scripts);
    font-size: 0.9em;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 6px 10px;
  }
  .sync-plan li {
    display: grid;
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    white-space: nowrap;
  }
  .sync-plan .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sync-plan .warn {
    color: var(--git-conflict-fg);
  }
  .sync-plan .more {
    color: var(--hidden-fg);
  }
</style>

<script module>
  /** Undo and redo of the editor open, kept while F1 → Features is shown over it. */
  const kept = { modal: null, undo: [], redo: [] };
</script>

<script>
  // The built-in editor (docs/commands/editor.md), Shift+F4: a text file in a window over the
  // panels, highlighted as the preview highlights code. Opening and saving are the core's
  // (edit.rs): the file keeps its encoding, line endings and permissions, and one that changed
  // on the disk since it was opened is not saved over without asking. The text is kept on the
  // modal, so F1 → Features and back keeps it; undo is this window's own.
  import { tick } from "svelte";
  import { ui, reloadAll, keyOf, lineHistory } from "./app.svelte.js";
  import { invoke, basename, parent } from "./lib.js";
  import { t } from "./i18n.svelte.js";
  import { hljs } from "./hl.js";

  const m = ui.modal;
  /** Text longer than this is not highlighted: it would slow typing. */
  const HIGHLIGHT_MOST = 60_000;
  /** What the title calls each encoding (`edit::Encoding`). */
  const ENCODINGS = { utf8: "UTF-8", utf8_bom: "UTF-8 BOM", utf16_le: "UTF-16 LE", utf16_be: "UTF-16 BE", windows1252: "Windows-1252" };
  /** Undo keeps at most this many characters of earlier texts. */
  const UNDO_MOST = 64 << 20;

  let area = $state();
  let pre = $state();
  let gutter = $state();
  let field = $state();
  let error = $state("");
  // "close": changes not saved; "changed": changed on the disk since.
  let asking = $state(null);
  // "find", "replace" or "line": the bar under the text.
  let bar = $state(null);
  let said = $state("");
  let caret = $state({ line: 1, col: 1 });
  let top = $state(0);
  let left = $state(0);

  const name = basename(m.path);
  const dirty = $derived(m.doc && m.text !== m.saved);
  const lines = $derived.by(() => {
    let n = 1;
    for (let i = m.text?.indexOf("\n") ?? -1; i >= 0; i = m.text.indexOf("\n", i + 1)) n++;
    return n;
  });
  /** Who changed each line beside its number (`blame`), until the text is changed. */
  const numbers = $derived(Array.from({ length: lines }, (_, i) => (m.blame ? `${(m.blame.commits[m.blame.lines[i]]?.margin ?? "").padEnd(31)}  ${String(i + 1).padStart(String(lines).length)}` : i + 1)).join("\n"));
  /** In a repository: blame and a function's history can be asked. */
  let git = $state(false);
  let gitBusy = $state(false);
  invoke("git_lines_here", { dir: parent(m.path) }).then((v) => (git = v), () => {});
  const html = $derived(
    m.text != null && m.text.length <= HIGHLIGHT_MOST && m.lang && hljs.getLanguage(m.lang) ? hljs.highlight(m.text + "\n", { language: m.lang }).value : "",
  );
  // What one Tab puts in: a tab when the file indents with tabs, else four spaces.
  const indent = $derived.by(() => {
    const s = m.text ?? "";
    return (s.match(/^\t/gm)?.length ?? 0) > (s.match(/^ {2}/gm)?.length ?? 0) ? "\t" : "    ";
  });

  if (kept.modal !== m) Object.assign(kept, { modal: m, undo: [], redo: [] });

  $effect(() => {
    if (m.doc) return void tick().then(ready);
    invoke("edit_open", { path: m.path })
      .then(async (doc) => {
        m.lang = await invoke("code_language", { path: m.path, head: doc.text.slice(0, 4096) }).catch(() => null);
        Object.assign(m, { doc, text: doc.text, saved: doc.text });
        await tick();
        ready();
      })
      .catch((e) => (error = String(e)));
  });

  function ready() {
    if (!area) return;
    area.value = m.text;
    area.focus();
    area.setSelectionRange(m.at ?? 0, m.at ?? 0);
    where();
  }

  // ------------------------------------------------------------ changes and undo

  let before = null;
  let lastAt = 0;

  /** The text before a change typed into the field, for undo. */
  function beforeinput(e) {
    if (e.inputType === "historyUndo" || e.inputType === "historyRedo") {
      e.preventDefault();
      return void (e.inputType === "historyUndo" ? undo() : redo());
    }
    before = { text: area.value, start: area.selectionStart, end: area.selectionEnd, kind: e.inputType };
  }

  function oninput() {
    // Letters typed one after another are undone together; a pause or another kind of change
    // starts the next step.
    const now = Date.now();
    const last = kept.undo.at(-1);
    const merge = before?.kind === "insertText" && last?.kind === "insertText" && now - lastAt < 1000 && !/\s/.test(area.value[area.selectionStart - 1] ?? "");
    if (before && !merge) push(before);
    lastAt = now;
    before = null;
    kept.redo = [];
    m.text = area.value;
    unblame();
    where();
  }

  /** The lines moved: who changed them is no longer known. */
  function unblame() {
    if (!m.blame) return;
    m.blame = null;
    said = t("blame.edited");
  }

  async function toggleBlame() {
    if (!git) return void (said = t("blame.not_here"));
    if (m.blame) return void ((m.blame = null), (said = t("blame.off")));
    if (dirty) return void (said = t("blame.save_first"));
    said = t("blame.reading");
    gitBusy = true;
    try {
      const b = await invoke("git_blame", { path: m.path });
      if (m.text === m.saved) m.blame = b;
      said = b.cut ? t("blame.cut", { n: b.lines.length }) : t("blame.shown");
    } catch (e) {
      said = String(e);
    }
    gitBusy = false;
  }

  /** The function the caret is in, or the lines selected. */
  async function trace() {
    if (!git) return void (said = t("blame.not_here"));
    const v = area.value;
    const lineAt = (at) => v.slice(0, at).split("\n").length - 1;
    const a = lineAt(area.selectionStart);
    let b = lineAt(area.selectionEnd);
    if (b > a && v[area.selectionEnd - 1] === "\n") b--;
    said = t("blame.tracing");
    gitBusy = true;
    m.at = area.selectionStart;
    await lineHistory(m.path, v, a, b);
    gitBusy = false;
    said = ui.status;
  }

  function push(state) {
    kept.undo.push(state);
    let total = kept.undo.reduce((n, s) => n + s.text.length, 0);
    while (total > UNDO_MOST && kept.undo.length > 1) total -= kept.undo.shift().text.length;
  }

  /** `s` in place of the selection, as one undo step. */
  function insert(s) {
    push({ text: area.value, start: area.selectionStart, end: area.selectionEnd, kind: "insert" });
    kept.redo = [];
    area.setRangeText(s, area.selectionStart, area.selectionEnd, "end");
    m.text = area.value;
    unblame();
    where();
  }

  function restore(from, to) {
    const s = from.pop();
    if (!s) return void (said = t(from === kept.undo ? "editor.nothing_to_undo" : "editor.nothing_to_redo"));
    to.push({ text: area.value, start: area.selectionStart, end: area.selectionEnd, kind: "restore" });
    area.value = m.text = s.text;
    unblame();
    area.setSelectionRange(s.start, s.end);
    show(s.start);
  }
  const undo = () => restore(kept.undo, kept.redo);
  const redo = () => restore(kept.redo, kept.undo);

  /** A new line, indented as the line it was started from. */
  function newline() {
    const v = area.value;
    const start = v.lastIndexOf("\n", area.selectionStart - 1) + 1;
    const lead = v.slice(start, area.selectionStart).match(/^[ \t]*/)[0];
    insert("\n" + lead);
  }

  // ------------------------------------------------------------ where the cursor is

  function where() {
    if (!area) return;
    const at = area.selectionStart;
    const v = area.value;
    let line = 1;
    for (let i = v.indexOf("\n"); i >= 0 && i < at; i = v.indexOf("\n", i + 1)) line++;
    caret = { line, col: at - (v.lastIndexOf("\n", at - 1) + 1) + 1 };
    m.at = at;
  }

  function scrolled() {
    top = area.scrollTop;
    left = area.scrollLeft;
  }

  /** The text scrolled so that offset `at` shows, a few lines from the top. */
  function show(at) {
    const line = area.value.slice(0, at).split("\n").length - 1;
    const h = parseFloat(getComputedStyle(area).lineHeight) || 18;
    if (line * h < area.scrollTop || (line + 1) * h > area.scrollTop + area.clientHeight) area.scrollTop = Math.max(0, (line - 3) * h);
    area.focus();
    where();
  }

  // ------------------------------------------------------------ find, replace, go to line

  async function open(kind) {
    bar = kind;
    if (kind !== "line" && area.selectionEnd > area.selectionStart && !area.value.slice(area.selectionStart, area.selectionEnd).includes("\n"))
      m.query = area.value.slice(area.selectionStart, area.selectionEnd);
    await tick();
    field?.focus();
    field?.select();
  }

  /** The next hit of the search (case aside) after the selection, or the one before it. */
  function find(back = false) {
    const q = (m.query ?? "").toLowerCase();
    if (!q) return void open("find");
    const v = area.value.toLowerCase();
    let i = back ? v.lastIndexOf(q, area.selectionStart - 1) : v.indexOf(q, area.selectionEnd);
    if (i < 0) i = back ? v.lastIndexOf(q) : v.indexOf(q);
    if (i < 0) return void (said = t("viewer.not_found", { text: m.query }));
    said = "";
    area.setSelectionRange(i, i + q.length);
    show(i);
  }

  function replaceAll() {
    const q = m.query ?? "";
    if (!q) return;
    const v = area.value;
    const low = v.toLowerCase();
    const ql = q.toLowerCase();
    let out = "";
    let n = 0;
    let from = 0;
    // Lowercasing changed the length (İ): only exact matches are found then.
    const hay = low.length === v.length ? low : v;
    const needle = low.length === v.length ? ql : q;
    for (let i = hay.indexOf(needle); i >= 0; i = hay.indexOf(needle, from)) {
      out += v.slice(from, i) + (m.with ?? "");
      from = i + q.length;
      n++;
    }
    if (!n) return void (said = t("viewer.not_found", { text: q }));
    area.setSelectionRange(0, v.length);
    insert(out + v.slice(from));
    area.setSelectionRange(0, 0);
    show(0);
    said = t("editor.replaced", { n });
    bar = null;
  }

  function goLine() {
    const n = Math.max(1, parseInt(m.line ?? "", 10) || 1);
    const v = area.value;
    let at = 0;
    for (let k = 1; k < n; k++) {
      const i = v.indexOf("\n", at);
      if (i < 0) break;
      at = i + 1;
    }
    bar = null;
    area.setSelectionRange(at, at);
    show(at);
  }

  function barEnter() {
    if (bar === "find") find();
    else if (bar === "replace") replaceAll();
    else if (bar === "line") goLine();
  }

  // ------------------------------------------------------------ saving and closing

  async function save(force = false) {
    try {
      const text = area.value;
      m.doc.stamp = await invoke("edit_save", { path: m.path, text, encoding: m.doc.encoding, crlf: m.doc.crlf, stamp: force ? null : m.doc.stamp });
      m.saved = text;
      m.wrote = true;
      asking = null;
      said = t("editor.saved", { name });
      return true;
    } catch (e) {
      if (String(e) === "changed") asking = "changed";
      else said = t("editor.not_saved", { error: String(e) });
      return false;
    }
  }

  async function reload() {
    try {
      const doc = await invoke("edit_open", { path: m.path });
      Object.assign(m, { doc, text: doc.text, saved: doc.text });
      Object.assign(kept, { undo: [], redo: [] });
      area.value = doc.text;
      asking = null;
      said = t("editor.reloaded");
    } catch (e) {
      said = String(e);
    }
  }

  function close() {
    ui.modal = m.back ?? null;
    kept.modal = null;
    if (m.wrote) reloadAll();
  }

  function tryClose() {
    if (dirty) asking = "close";
    else close();
  }

  async function saveAndClose() {
    if (await save()) close();
  }

  /** Keys while the editor is open (Dialogs.svelte passes them on): true when taken. */
  m.key = (k, act, e) => {
    const inField = e?.target === area;
    const inBar = e?.target?.closest?.(".bar");
    if (k === "Esc") {
      if (gitBusy) invoke("git_lines_cancel"), (gitBusy = false), (said = t("blame.stopped"));
      else if (asking) asking = null;
      else if (bar) (bar = null), area?.focus();
      else tryClose();
      return true;
    }
    if (!m.doc) return false;
    if (k === "Ctrl+s" || k === "F2") return save(), true;
    if (act === "quit") return tryClose(), true;
    if (act === "blame") return toggleBlame(), true;
    if (act === "line_history") return trace(), true;
    if (k === "Ctrl+f" || k === "F7") return open("find"), true;
    if (k === "Shift+F7") return find(), true;
    if (k === "Ctrl+r") return open("replace"), true;
    if (k === "Ctrl+g") return open("line"), true;
    if (k === "Ctrl+z") return undo(), true;
    if (k === "Ctrl+Shift+Z" || k === "Ctrl+y") return redo(), true;
    if (inBar && k === "Enter") return barEnter(), true;
    if (inField && k === "Enter") return newline(), true;
    if (inField && k === "Tab") return insert(indent), true;
    return false;
  };
</script>

<div class="editor" role="dialog" aria-modal="true" aria-label={t("action.edit_here")}>
  <header>
    <h2><bdi>{name}</bdi>{#if dirty}<span class="dot" title={t("editor.modified")}> ●</span>{/if}</h2>
    {#if m.doc}
      <span class="meta">{m.lang ?? t("viewer.plain")} · {ENCODINGS[m.doc.encoding]} · {m.doc.crlf ? "CRLF" : "LF"}</span>
    {/if}
    <span class="acts">
      <button class="primary" disabled={!m.doc} onclick={() => save()}>{t("editor.save")} <kbd>Ctrl+S</kbd></button>
      <button disabled={!m.doc} onclick={() => open("find")}>{t("viewer.search")} <kbd>Ctrl+F</kbd></button>
      <button disabled={!m.doc} onclick={() => open("replace")}>{t("editor.replace")} <kbd>Ctrl+R</kbd></button>
      <button disabled={!m.doc} onclick={() => open("line")}>{t("viewer.go_to_line")} <kbd>Ctrl+G</kbd></button>
      {#if git}
        <button class:on={!!m.blame} disabled={!m.doc} title={t("blame.tip", { key: keyOf("blame") })} onclick={toggleBlame}>{t("blame.button")} <kbd>{keyOf("blame")}</kbd></button>
        <button disabled={!m.doc} title={t("blame.history_tip")} onclick={trace}>{t("action.line_history")} <kbd>{keyOf("line_history")}</kbd></button>
      {/if}
      <button onclick={tryClose}>{t("common.close")} <kbd>Esc</kbd></button>
    </span>
  </header>

  {#if error}
    <p class="error">{error}</p>
  {:else}
    <div class="body">
      <pre class="gutter mono" bind:this={gutter} aria-hidden="true" style:transform={`translateY(${-top}px)`}>{numbers}</pre>
      <div class="text">
        {#if html}<pre class="mono code" bind:this={pre} aria-hidden="true" style:transform={`translate(${-left}px, ${-top}px)`}><code class="hljs">{@html html}</code></pre>{/if}
        <textarea
          class="mono"
          class:painted={!!html}
          bind:this={area}
          wrap="off"
          spellcheck="false"
          autocomplete="off"
          aria-label={name}
          disabled={!m.doc}
          onbeforeinput={beforeinput}
          {oninput}
          onscroll={scrolled}
          onkeyup={where}
          onmouseup={where}></textarea>
      </div>
    </div>
  {/if}

  {#if asking === "close"}
    <div class="ask">
      <span>{t("editor.ask_close_gui", { name })}</span>
      <button class="primary" onclick={saveAndClose}>{t("editor.save")}</button>
      <button class="danger" onclick={close}>{t("editor.discard")}</button>
      <button onclick={() => (asking = null)}>{t("common.cancel")}</button>
    </div>
  {:else if asking === "changed"}
    <div class="ask">
      <span>{t("editor.ask_changed_gui", { name })}</span>
      <button class="danger" onclick={() => save(true)}>{t("editor.save_over")}</button>
      <button onclick={reload}>{t("editor.read_again")}</button>
      <button onclick={() => (asking = null)}>{t("common.cancel")}</button>
    </div>
  {:else if bar}
    <div class="bar">
      {#if bar === "line"}
        <label>{t("viewer.go_to_line")} <input bind:this={field} bind:value={m.line} inputmode="numeric" spellcheck="false" /></label>
        <button class="primary" onclick={goLine}>{t("editor.go")} <kbd>Enter</kbd></button>
      {:else}
        <label>{t("viewer.search")} <input bind:this={field} bind:value={m.query} spellcheck="false" /></label>
        {#if bar === "replace"}
          <label>{t("editor.with")} <input bind:value={m.with} spellcheck="false" /></label>
          <button class="primary" onclick={replaceAll}>{t("editor.replace_all")} <kbd>Enter</kbd></button>
        {:else}
          <button class="primary" onclick={() => find()}>{t("editor.next")} <kbd>Enter</kbd></button>
          <button onclick={() => find(true)}>{t("editor.previous")}</button>
        {/if}
      {/if}
      <button onclick={() => ((bar = null), area?.focus())}>{t("common.close")} <kbd>Esc</kbd></button>
    </div>
  {/if}

  <footer>
    <span class="said">{said || t("editor.at", { line: caret.line, col: caret.col })}</span>
    <span class="keys">{t("editor.keys_gui")}</span>
  </footer>
</div>

<style>
  .editor {
    position: fixed;
    inset: 3vh 3vw;
    z-index: 11;
    display: flex;
    flex-direction: column;
    background: var(--panel-bg, var(--dialog-bg));
    color: var(--panel-fg, var(--dialog-fg));
    border: 1px solid var(--border-fg);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  header,
  footer,
  .bar,
  .ask {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 14px;
    background: var(--dialog-bg);
    color: var(--dialog-fg);
  }
  header {
    border-bottom: 1px solid var(--border-fg);
  }
  footer,
  .bar,
  .ask {
    border-top: 1px solid var(--border-fg);
  }
  h2 {
    margin: 0;
    font-size: 1.05em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dot {
    color: var(--git-modified-fg);
  }
  .meta,
  .keys,
  .said {
    color: var(--hidden-fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .acts {
    margin-left: auto;
    display: flex;
    gap: 6px;
  }
  .keys {
    margin-left: auto;
    font-size: 0.9em;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    overflow: hidden;
  }
  .mono {
    font-family: var(--mono-font), var(--scripts);
    font-size: 0.95em;
    line-height: 1.45;
    tab-size: 4;
  }
  pre {
    margin: 0;
  }
  .gutter {
    padding: 8px 8px 8px 12px;
    text-align: right;
    color: var(--hidden-fg);
    user-select: none;
    min-width: 3ch;
    border-right: 1px solid var(--border-fg);
    align-self: flex-start;
  }
  .text {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }
  .text pre,
  textarea {
    position: absolute;
    inset: 0;
    padding: 8px 12px;
    white-space: pre;
    border: 0;
  }
  .text pre {
    bottom: auto;
    right: auto;
    pointer-events: none;
  }
  textarea {
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    resize: none;
    outline: none;
    background: transparent;
    color: inherit;
    caret-color: var(--panel-fg, currentColor);
    overflow: auto;
  }
  /* The highlighted copy shows through; the field keeps the caret and the selection. */
  textarea.painted {
    color: transparent;
  }
  textarea.painted::selection {
    color: transparent;
    background: color-mix(in srgb, var(--accent-bg) 45%, transparent);
  }
  .bar label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .bar input {
    font: inherit;
    background: var(--dialog-input-bg);
    color: var(--dialog-input-fg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 3px 6px;
  }
  .error {
    margin: auto;
    color: var(--git-deleted-fg);
  }
  .code :global(.hljs-keyword),
  .code :global(.hljs-selector-tag),
  .code :global(.hljs-built_in) { color: var(--symlink-fg); }
  .code :global(.hljs-string),
  .code :global(.hljs-attr) { color: var(--git-added-fg); }
  .code :global(.hljs-number),
  .code :global(.hljs-literal) { color: var(--search-hit-fg); }
  .code :global(.hljs-comment) { color: var(--hidden-fg); font-style: italic; }
  .code :global(.hljs-title),
  .code :global(.hljs-function) { color: var(--directory-fg); }
  .code :global(.hljs-type),
  .code :global(.hljs-class) { color: var(--git-modified-fg); }
  .code :global(.hljs-meta) { color: var(--git-renamed-fg); }
  .code :global(.hljs-addition) { color: var(--git-added-fg); }
  .code :global(.hljs-deletion) { color: var(--git-deleted-fg); }
</style>

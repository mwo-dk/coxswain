<script>
  import { ui, tab, item } from "./app.svelte.js";
  import { renderMarkdown, renderMermaid, highlight, renderDocx, readSheet, renderNotebook, loadFont } from "./renderers.js";
  import { invoke, convertFileSrc, size, date, age, ageColor, previewKind } from "./lib.js";

  /** Set by App when the command output should show here instead of the file. */
  let { output = null, onclearoutput, notesFocus = 0 } = $props();

  const t = $derived(tab());
  const e = $derived(item(t));
  const kind = $derived(output ? "output" : previewKind(e));
  let text = $state("");
  let html = $state("");
  let truncated = $state(false);
  let binary = $state(false);
  let note = $state("");
  let noteDir = "";
  let noteArea = $state();
  let summary = $state(null);

  const LIMIT = 512 * 1024;
  /** Markdown and Mermaid: show the rendered result, or the source. Kept across files. */
  let source = $state(false);

  // Load text-like previews; debounced so holding an arrow key stays smooth.
  $effect(() => {
    const cur = e;
    const k = kind;
    text = "";
    html = "";
    const src = source;
    if (!cur || !["text", "markdown", "mermaid"].includes(k)) return;
    const timer = setTimeout(async () => {
      try {
        const [s, trunc, bin] = await invoke("read_text", { path: cur.path, max: LIMIT });
        if (item(tab())?.path !== cur.path) return;
        truncated = trunc;
        binary = bin;
        // Rendered markdown and diagrams come back sanitized from renderers.js.
        const rendered = bin || src ? "" : k === "markdown" ? await renderMarkdown(s) : k === "mermaid" ? await renderMermaid(s) : "";
        if (item(tab())?.path !== cur.path) return;
        if (rendered) {
          html = rendered;
        } else if (!bin && s.length < 200_000) {
          const ext = cur.name.split(".").pop().toLowerCase();
          html = highlight(s, k === "text" ? ext : "markdown");
        } else {
          text = s;
        }
      } catch (err) {
        text = String(err);
      }
    }, 80);
    return () => clearTimeout(timer);
  });

  // Documents, notebooks, spreadsheets and fonts, each rendered by a library loaded on first use.
  let rich = $state(null);
  let sheetName = $state("");
  $effect(() => {
    const cur = e;
    const k = kind;
    rich = null;
    if (!["docx", "notebook", "sheet", "font"].includes(k)) return;
    const timer = setTimeout(async () => {
      let r;
      try {
        if (k === "docx") r = { html: await renderDocx(cur.path) };
        else if (k === "notebook") r = { html: await renderNotebook(cur.path) };
        else if (k === "font") r = { family: await loadFont(cur.path) };
        else r = { sheet: await readSheet(cur.path) };
      } catch (err) {
        r = { error: String(err?.message ?? err) };
      }
      if (item(tab())?.path !== cur.path) return;
      if (r.sheet) sheetName = r.sheet.names[0] ?? "";
      rich = r;
    }, 80);
    return () => clearTimeout(timer);
  });
  const rows = $derived(rich?.sheet && sheetName ? rich.sheet.rows(sheetName) : []);

  // Archive: what is inside.
  let archive = $state(null);
  $effect(() => {
    const cur = e;
    archive = null;
    if (kind !== "archive") return;
    const timer = setTimeout(async () => {
      const r = await invoke("archive_list", { path: cur.path }).catch((err) => ({ error: String(err) }));
      if (item(tab())?.path === cur.path) archive = r;
    }, 80);
    return () => clearTimeout(timer);
  });

  // Folder: summary and notes.
  $effect(() => {
    const cur = e;
    summary = null;
    if (kind !== "folder" || !cur || cur.name === "..") return;
    const timer = setTimeout(async () => {
      try {
        const r = await invoke("list_dir", { dir: cur.path, showHidden: ui.showHidden, sort: "name", reverse: false });
        const items = r.items.filter((x) => x.name !== "..");
        summary = {
          folders: items.filter((x) => x.is_dir).length,
          files: items.filter((x) => !x.is_dir).length,
          bytes: items.reduce((a, x) => a + (x.is_dir ? 0 : x.size), 0),
          newest: items.reduce((a, x) => Math.max(a, x.modified), 0),
        };
      } catch {
        summary = null;
      }
    }, 80);
    return () => clearTimeout(timer);
  });

  // Notes belong to the folder under the cursor, or to the current folder for files.
  const notesDir = $derived(e?.is_dir && e.name !== ".." ? e.path : t?.dir);
  $effect(() => {
    const dir = notesDir;
    if (!dir) return;
    invoke("get_note", { dir }).then((n) => {
      note = n;
      noteDir = dir;
    });
  });
  $effect(() => {
    if (notesFocus) noteArea?.focus();
  });

  async function saveNote() {
    if (!noteDir) return;
    await invoke("set_note", { dir: noteDir, text: note });
    const tb = tab();
    if (tb.dir === noteDir) tb.hasNotes = !!note.trim();
  }

  async function calcSize() {
    const [bytes] = (await invoke("dir_sizes", { paths: [e.path] }))[e.path];
    t.sizes[e.path] = bytes;
  }
</script>

<aside class="preview" aria-label="Preview">
  {#if kind === "output"}
    <header>
      <span class="icon">{"\u{f120}"}</span>
      <div class="title"><b>{output.title}</b><small>Command output</small></div>
      <button class="close" title="Back to preview" onclick={onclearoutput}>×</button>
    </header>
    <pre class="body mono">{output.text || "(no output)"}</pre>
  {:else if !e}
    <p class="empty">Nothing selected</p>
  {:else}
    <header>
      <span class="icon big" style:color={e.icon.color || "var(--directory-fg)"}>{e.icon.glyph}</span>
      <div class="title">
        <b title={e.name}>{e.name}</b>
        <small>
          {#if e.is_dir}{t.sizes[e.path] !== undefined ? size(t.sizes[e.path]) : "Folder"}{:else}{size(e.size)}{/if}
          · <span class="age" style:background={ageColor(e.modified)}>{age(e.modified)}</span>
          {date(e.modified)}
        </small>
      </div>
      {#if kind === "markdown" || kind === "mermaid"}
        <div class="modes" role="group" aria-label="Show">
          <button class:on={!source} onclick={() => (source = false)}>Rendered</button>
          <button class:on={source} onclick={() => (source = true)}>Source</button>
        </div>
      {/if}
    </header>

    <div class="body">
      {#if kind === "image"}
        <div class="media checker"><img src={convertFileSrc(e.path)} alt={e.name} /></div>
      {:else if kind === "video"}
        <div class="media">
          <!-- svelte-ignore a11y_media_has_caption -->
          <video src={convertFileSrc(e.path)} controls preload="metadata"></video>
        </div>
      {:else if kind === "audio"}
        <div class="media"><audio src={convertFileSrc(e.path)} controls preload="metadata"></audio></div>
      {:else if kind === "pdf"}
        <iframe class="pdf" src={convertFileSrc(e.path)} title={e.name}></iframe>
      {:else if kind === "archive"}
        {#if archive?.error}
          <p class="more">{archive.error}</p>
        {:else if archive}
          <p class="more">{archive.entries.length}{archive.more ? "+" : ""} entries · Ctrl+E extracts to the other pane</p>
          <ul class="archive mono">
            {#each archive.entries as a (a.name)}
              <li class:dir={a.is_dir}><span>{a.name}</span>{#if !a.is_dir}<span class="sz">{size(a.size)}</span>{/if}</li>
            {/each}
          </ul>
        {/if}
      {:else if (kind === "markdown" || kind === "mermaid") && html && !source}
        <article class="markdown">{@html html}</article>
      {:else if (kind === "markdown" || kind === "mermaid") && html}
        <pre class="mono code"><code class="hljs">{@html html}</code></pre>
      {:else if rich?.error}
        <p class="more">{rich.error}</p>
      {:else if rich?.html !== undefined}
        <article class="markdown {kind}">{@html rich.html}</article>
      {:else if rich?.family}
        <div class="font" style:font-family="'{rich.family}'">
          <p style:font-size="2.6em">The quick brown fox jumps over the lazy dog</p>
          <p style:font-size="1.6em">Sphinx of black quartz, judge my vow</p>
          <p style:font-size="1.1em">ABCDEFGHIJKLMNOPQRSTUVWXYZ<br />abcdefghijklmnopqrstuvwxyz<br />0123456789 &amp;@#$%*(){"{}"}[]&lt;&gt;?!</p>
          <p style:font-size="0.85em">Æble, øl og å — Grüße — ﬁ ﬂ — “quotes” — €£¥</p>
        </div>
      {:else if rich?.sheet}
        {#if rich.sheet.names.length > 1}
          <div class="modes sheets" role="group" aria-label="Sheet">
            {#each rich.sheet.names as n (n)}<button class:on={n === sheetName} onclick={() => (sheetName = n)}>{n}</button>{/each}
          </div>
        {/if}
        <div class="table-wrap">
          <table class="sheet">
            <tbody>
              {#each rows.slice(0, 200) as row, i (i)}
                <tr>{#each row as c, j (j)}{#if i === 0}<th>{c}</th>{:else}<td>{c}</td>{/if}{/each}</tr>
              {/each}
            </tbody>
          </table>
        </div>
        {#if rows.length > 200}<p class="more">Showing the first 200 rows</p>{/if}
      {:else if kind === "text"}
        {#if html}
          <pre class="mono code"><code class="hljs">{@html html}</code></pre>
        {:else}
          <pre class="mono" class:hex={binary}>{text}</pre>
        {/if}
        {#if truncated}<p class="more">Showing the first {size(LIMIT)}</p>{/if}
      {:else if kind === "folder"}
        <dl class="facts">
          {#if summary}
            <dt>Contents</dt><dd>{summary.folders} folders, {summary.files} files</dd>
            <dt>Files here</dt><dd>{size(summary.bytes)}</dd>
            {#if summary.newest}<dt>Newest</dt><dd>{date(summary.newest)} ({age(summary.newest)})</dd>{/if}
          {/if}
          <dt>Total size</dt>
          <dd>
            {#if t.sizes[e.path] !== undefined}{size(t.sizes[e.path])}{:else}<button class="link" onclick={calcSize}>Calculate (Ctrl+Space)</button>{/if}
          </dd>
          {#if t.git}<dt>Git</dt><dd class="git">{t.git.prompt}</dd>{/if}
        </dl>
      {/if}
    </div>

    <section class="notes">
      <label for="notes">{"\u{f249}"} Notes for {notesDir === t.dir ? "this folder" : e.name}</label>
      <textarea id="notes" bind:this={noteArea} bind:value={note} onblur={saveNote} placeholder="To-dos, reminders… saved when you leave the field (Esc)"></textarea>
    </section>
  {/if}
</aside>

<style>
  .preview {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--preview-bg);
    color: var(--preview-fg);
    border-radius: 8px;
    border: 1px solid var(--border-fg);
    overflow: hidden;
    box-sizing: border-box;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border-fg);
  }
  .icon {
    font-family: var(--icon-font);
    font-size: 1.4em;
  }
  .icon.big {
    font-size: 2.2em;
  }
  .title {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }
  .title b {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  small {
    color: var(--hidden-fg);
  }
  .age {
    border-radius: 4px;
    padding: 0 5px;
    color: #fff;
    font-size: 0.9em;
  }
  .close {
    font: inherit;
    font-size: 1.3em;
    color: inherit;
    background: none;
    border: 0;
    cursor: pointer;
  }
  .body {
    flex: 1;
    overflow: auto;
    min-height: 0;
    padding: 10px 14px;
  }
  .media {
    display: grid;
    place-items: center;
    min-height: 100%;
  }
  .media img,
  .media video {
    max-width: 100%;
    max-height: 60vh;
    border-radius: 6px;
  }
  .pdf {
    width: 100%;
    height: 100%;
    min-height: 60vh;
    border: 0;
    border-radius: 6px;
    background: #fff;
  }
  .archive {
    list-style: none;
    padding: 0;
    margin: 6px 0 0;
  }
  .archive li {
    display: flex;
    gap: 12px;
    white-space: nowrap;
  }
  .archive li span:first-child {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .archive .dir {
    color: var(--directory-fg);
  }
  .sz {
    color: var(--hidden-fg);
  }
  .modes {
    display: flex;
    flex: none;
    border: 1px solid var(--border-fg);
    border-radius: 7px;
    overflow: hidden;
  }
  .modes button {
    font: inherit;
    font-size: 0.85em;
    color: var(--hidden-fg);
    background: none;
    border: 0;
    padding: 3px 9px;
    cursor: pointer;
  }
  .modes button.on {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .modes.sheets {
    flex-wrap: wrap;
    margin-bottom: 8px;
    width: fit-content;
  }
  .markdown :global(.diagram) {
    display: grid;
    place-items: center;
    margin: 10px 0;
    padding: 10px;
    background: color-mix(in srgb, var(--dialog-input-bg) 60%, transparent);
    border-radius: 6px;
  }
  .markdown :global(.diagram svg) {
    max-width: 100%;
    max-height: 360px;
    height: auto;
  }
  .markdown :global(.diagram-error),
  .notebook :global(.err) {
    color: var(--git-deleted-fg);
    white-space: pre-wrap;
  }
  .notebook :global(.cell) {
    margin: 8px 0;
  }
  .notebook :global(.prompt) {
    font-family: var(--mono-font);
    font-size: 0.8em;
    color: var(--hidden-fg);
  }
  .notebook :global(pre) {
    white-space: pre-wrap;
  }
  .notebook :global(.out) {
    margin: 4px 0 10px;
    max-width: 100%;
    font-family: var(--mono-font);
    font-size: 0.9em;
  }
  .docx :global(img) {
    max-width: 100%;
  }
  .font p {
    margin: 0 0 14px;
    line-height: 1.25;
    overflow-wrap: anywhere;
  }
  .table-wrap {
    overflow: auto;
  }
  .sheet {
    border-collapse: collapse;
    font-size: 0.9em;
    font-variant-numeric: tabular-nums;
  }
  .sheet th,
  .sheet td {
    border: 1px solid var(--border-fg);
    padding: 2px 8px;
    white-space: nowrap;
    max-width: 22em;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
  }
  .sheet th {
    background: var(--dialog-input-bg);
    font-weight: 600;
  }
  .checker {
    background: repeating-conic-gradient(color-mix(in srgb, var(--border-fg) 50%, transparent) 0 25%, transparent 0 50%) 0 0 / 16px 16px;
    border-radius: 6px;
  }
  .mono {
    font-family: var(--mono-font);
    font-size: 0.9em;
    margin: 0;
    white-space: pre;
    tab-size: 4;
  }
  .hex {
    font-size: 0.8em;
  }
  .more {
    color: var(--hidden-fg);
    font-size: 0.85em;
  }
  .markdown {
    line-height: 1.55;
  }
  .markdown :global(pre),
  .markdown :global(code) {
    font-family: var(--mono-font);
    background: var(--dialog-input-bg);
    border-radius: 4px;
  }
  .markdown :global(pre) {
    padding: 8px;
    overflow: auto;
  }
  .markdown :global(a) {
    color: var(--accent-bg);
  }
  .markdown :global(img) {
    max-width: 100%;
  }
  .markdown :global(table) {
    border-collapse: collapse;
  }
  .markdown :global(td),
  .markdown :global(th) {
    border: 1px solid var(--border-fg);
    padding: 2px 6px;
  }
  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 14px;
    margin: 0;
  }
  dt {
    color: var(--hidden-fg);
  }
  dd {
    margin: 0;
  }
  .git {
    font-family: var(--icon-font), var(--font);
    color: var(--git-branch-fg);
  }
  .link {
    font: inherit;
    color: var(--accent-bg);
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
    text-decoration: underline;
  }
  .notes {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 14px 12px;
    border-top: 1px solid var(--border-fg);
  }
  .notes label {
    font-size: 0.85em;
    color: var(--hidden-fg);
    font-family: var(--icon-font), var(--font);
  }
  textarea {
    font: inherit;
    min-height: 5.5em;
    resize: vertical;
    background: var(--dialog-input-bg);
    color: var(--dialog-input-fg);
    border: 1px solid var(--border-fg);
    border-radius: 6px;
    padding: 6px 8px;
    outline: none;
  }
  textarea:focus {
    border-color: var(--accent-bg);
  }
  .empty {
    margin: auto;
    color: var(--hidden-fg);
  }
  /* highlight.js tokens, tied to the theme's git/status colors so every theme works */
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
</style>

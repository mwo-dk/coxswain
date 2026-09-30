<script>
  import { ui, tab, item } from "./app.svelte.js";
  import { renderPptx, renderDrawio, renderMarkdown, renderMermaid, highlight, renderDocx, readSheet, renderNotebook, loadFont, clean, parseData, jsonLines, calendar, contacts, logLines, renderGraphviz, renderAsciidoc, readParquet } from "./renderers.js";
  import { invoke, convertFileSrc, size, date, age, ageColor, previewKind, looksLikeBom, CONVERTER } from "./lib.js";
  import BomView from "./BomView.svelte";
  import { t, tn, num } from "./i18n.svelte.js";

  /** Set by App when the command output should show here instead of the file. */
  let { output = null, onclearoutput, notesFocus = 0 } = $props();

  const pane = $derived(tab());
  const e = $derived(item(pane));
  /** A JSON or XML file whose first bytes turned out to be a CycloneDX BOM's. */
  let sniffedBom = $state("");
  const kind = $derived(output ? "output" : e && sniffedBom === e.path ? "bom" : previewKind(e));
  let text = $state("");
  let html = $state("");
  let truncated = $state(false);
  let binary = $state(false);
  let note = $state("");
  let noteDir = "";
  let noteArea = $state();
  let summary = $state(null);

  const LIMIT = 512 * 1024;

  /** Draws a draw.io diagram into the element, again when it changes. */
  function drawioView(el, xml) {
    const draw = (x) => renderDrawio(el, x).catch((err) => (el.textContent = String(err?.message ?? err)));
    draw(xml);
    return { update: draw };
  }
  /** Markdown, Mermaid and data files: show the rendered result or tree, or the source. Kept across files. */
  const source = $derived(ui.previewSource);
  /** For a file git has changes for: show the file, or its diff. Kept across files. */
  const showDiff = $derived(ui.previewDiff);
  const TEXTUAL = ["text", "markdown", "mermaid", "data", "jsonl", "calendar", "contacts", "log", "graphviz", "asciidoc", "bom"];
  const ext = (f) => f.name.split(".").pop().toLowerCase();
  const gitKind = $derived(e && !e.is_dir ? pane.git?.files[e.name]?.kind : undefined);
  const hasDiff = $derived(gitKind && !["untracked", "ignored"].includes(gitKind));
  const diffing = $derived(hasDiff && showDiff);
  /** Parsed forms of textual files: a data tree, a table, calendar events or contact cards. */
  let tree = $state(undefined);
  let table = $state(null);
  let cards = $state(null);

  // Load text-like previews; debounced so holding an arrow key stays smooth.
  $effect(() => {
    const cur = e;
    const k = kind;
    text = "";
    html = "";
    tree = undefined;
    table = null;
    cards = null;
    const src = source;
    const diff = diffing;
    if (!cur || cur.is_dir || !(TEXTUAL.includes(k) || diff)) return;
    // The BOM view reads the file itself; only its source is read here.
    if (k === "bom" && !src && !diff) return;
    const timer = setTimeout(async () => {
      try {
        if (diff) {
          const d = await invoke("git_diff", { path: cur.path });
          if (item(tab())?.path === cur.path) html = d ? highlight(d, "diff") : highlight(t("preview.no_changes"), "plaintext");
          return;
        }
        const [s, trunc, bin] = await invoke("read_text", { path: cur.path, max: LIMIT });
        if (item(tab())?.path !== cur.path) return;
        truncated = trunc;
        binary = bin;
        if (!bin && (k === "data" || k === "text") && /\.(json|xml)$/i.test(cur.name) && looksLikeBom(s)) {
          sniffedBom = cur.path;
          if (!src) return;
        }
        if (!bin && !src && k === "data") {
          try {
            tree = await parseData(s, ext(cur));
            return;
          } catch {
            // Not parseable (or cut off at the size limit): show the source instead.
          }
        }
        if (!bin && k === "jsonl" && !src) return void (table = jsonLines(s));
        if (!bin && k === "calendar" && !src) return void (cards = { events: calendar(s) });
        if (!bin && k === "contacts" && !src) return void (cards = { people: contacts(s) });
        if (!bin && k === "log") return void (html = logLines(s));
        // Rendered markdown and diagrams come back sanitized from renderers.js.
        const render = { markdown: renderMarkdown, mermaid: renderMermaid, graphviz: renderGraphviz, asciidoc: renderAsciidoc }[k];
        const rendered = bin || src || !render ? "" : await render(s);
        if (item(tab())?.path !== cur.path) return;
        if (rendered) {
          html = rendered;
        } else if (!bin && s.length < 200_000) {
          const lang = { markdown: "markdown", mermaid: "plaintext", jsonl: "json", calendar: "plaintext", contacts: "plaintext", graphviz: "plaintext", asciidoc: "asciidoc" }[k];
          html = highlight(s, lang ?? ext(cur));
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
    const deck = k === "office" && ["pptx", "pptm", "ppsx", "potx"].includes(ext(cur));
    if (!["docx", "notebook", "sheet", "font", "parquet", "drawio"].includes(k) && !deck) return;
    const timer = setTimeout(async () => {
      let r;
      try {
        if (k === "docx") r = { html: await renderDocx(cur.path) };
        else if (k === "notebook") r = { html: await renderNotebook(cur.path) };
        else if (k === "font") r = { family: await loadFont(cur.path) };
        else if (k === "parquet") r = { parquet: await readParquet(cur.path) };
        else if (deck) r = { slides: await renderPptx(cur.path) };
        else if (k === "drawio") r = { drawio: (await invoke("read_text", { path: cur.path, max: 32 * 1024 * 1024 }))[0] };
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

  // Formats read by the Rust side: SQLite, EPUB, certificates, e-mail, property lists.
  let backend = $state(null);
  $effect(() => {
    const cur = e;
    const k = kind;
    backend = null;
    if (!["sqlite", "epub", "cert", "mail", "plist"].includes(k) || diffing) return;
    const cmd = { sqlite: "sqlite_info", epub: "epub_preview", cert: "cert_info", mail: "mail_preview", plist: "plist_xml" }[k];
    const timer = setTimeout(async () => {
      const r = await invoke(cmd, { path: cur.path }).then(
        (v) => ({ [k]: v }),
        (err) => ({ error: String(err) }),
      );
      if (item(tab())?.path === cur.path) backend = r;
    }, 80);
    return () => clearTimeout(timer);
  });

  // Facts under any file: photo EXIF, audio tags, what an executable is built for.
  let facts = $state([]);
  $effect(() => {
    const cur = e;
    facts = [];
    if (!cur || cur.is_dir) return;
    const timer = setTimeout(async () => {
      const f = await invoke("file_facts", { path: cur.path }).catch(() => []);
      if (item(tab())?.path === cur.path) facts = f;
    }, 120);
    return () => clearTimeout(timer);
  });

  const days = (secs) => Math.round(secs / 86400);

  // Previews made by an external tool (convert.rs): pick an engine, render, show the cached result.
  let conv = $state(null);
  const engineOf = (c) => c.engines.find((x) => x.id === ui.previewEngine[c.tool] && x.available) ?? c.engines.find((x) => x.available);
  $effect(() => {
    const cur = e;
    const spec = CONVERTER[kind];
    conv = null;
    if (!spec || !cur || diffing) return;
    const timer = setTimeout(async () => {
      const engines = await invoke("preview_engines", { tool: spec.tool }).catch(() => []);
      if (item(tab())?.path !== cur.path) return;
      conv = { ...spec, path: cur.path, engines, status: "idle", result: null, error: "" };
      const eng = engineOf(conv);
      if (!eng) return;
      const cached = await invoke("convert", { path: cur.path, tool: spec.tool, engine: eng.id, cachedOnly: true }).catch(() => null);
      if (conv?.path !== cur.path) return;
      if (cached) Object.assign(conv, { status: "done", result: cached });
      // Quick tools run by themselves, unless a container image still has to be pulled.
      else if ((typeof spec.auto === "string" ? ui.cfg.settings[spec.auto] : spec.auto) && !eng.needs_pull) renderConv();
    }, spec.wait ?? 150);
    return () => clearTimeout(timer);
  });

  // While a container run pulls its image, show the runtime's progress line.
  let pulling = $state("");
  $effect(() => {
    if (conv?.status !== "running") return void (pulling = "");
    const id = setInterval(async () => {
      const p = await invoke("pull_progress").catch(() => ({}));
      const [image, line] = Object.entries(p)[0] ?? [];
      pulling = image ? `${t("convert.pulling", { image })} ${line ?? ""}` : "";
    }, 500);
    return () => clearInterval(id);
  });

  async function renderConv(engineId) {
    const c = conv;
    if (!c) return;
    if (engineId) ui.previewEngine = { ...ui.previewEngine, [c.tool]: engineId };
    const eng = engineOf(c);
    if (!eng) return;
    Object.assign(c, { status: "running", error: "", result: null });
    try {
      const r = await invoke("convert", { path: c.path, tool: c.tool, engine: eng.id, cachedOnly: false });
      if (conv === c) Object.assign(c, { status: "done", result: r });
    } catch (err) {
      if (conv === c) Object.assign(c, { status: "error", error: String(err) });
    }
  }

  /** DuckDB's JSON rows as a table. */
  const jsonTable = (text) => {
    const rows = JSON.parse(text || "[]");
    const cols = rows.length ? Object.keys(rows[0]) : [];
    return [cols, ...rows.map((r) => cols.map((c) => String(r[c] ?? "")))];
  };

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
  const notesDir = $derived(e?.is_dir && e.name !== ".." ? e.path : pane?.dir);
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
    const r = (await invoke("dir_sizes", { paths: [e.path] }))[e.path];
    if (r) pane.sizes[e.path] = r[0];
  }
</script>

{#snippet node(key, v, depth)}
  {#if v !== null && typeof v === "object" && typeof v.toISOString !== "function"}
    {@const entries = Object.entries(v)}
    <details open={depth < 2}>
      <summary>{#if key !== null}<span class="k">{key}</span>{/if} <span class="meta">{Array.isArray(v) ? `[${v.length}]` : `{${entries.length}}`}</span></summary>
      <div class="kids">
        {#each entries.slice(0, 500) as [k, c] (k)}{@render node(k, c, depth + 1)}{/each}
        {#if entries.length > 500}<div class="meta">{tn("preview.more", entries.length - 500)}</div>{/if}
      </div>
    </details>
  {:else}
    <div class="leaf">{#if key !== null}<span class="k">{key}</span>: {/if}<span class="v {v === null ? 'null' : typeof v}">{typeof v === "string" ? JSON.stringify(v) : String(v)}</span></div>
  {/if}
{/snippet}

{#snippet grid(rows)}
  <div class="table-wrap">
    <table class="sheet">
      <tbody>
        {#each rows.slice(0, 201) as row, i (i)}
          <tr>{#each row as c, j (j)}{#if i === 0}<th>{c}</th>{:else}<td>{c}</td>{/if}{/each}</tr>
        {/each}
      </tbody>
    </table>
  </div>
{/snippet}

<aside class="preview" aria-label={t("action.toggle_preview")}>
  {#if kind === "output"}
    <header>
      <span class="icon">{"\u{f120}"}</span>
      <div class="title"><b>{output.title}</b><small>{t("preview.command_output")}</small></div>
      <button class="close" title={t("preview.back")} onclick={onclearoutput}>×</button>
    </header>
    <pre class="body mono">{output.text || t("preview.no_output")}</pre>
  {:else if !e}
    <p class="empty">{t("preview.nothing_selected")}</p>
  {:else}
    <header>
      <span class="icon big" style:color={e.icon.color || "var(--directory-fg)"}>{e.icon.glyph}</span>
      <div class="title">
        <b title={e.name}>{e.name}</b>
        <small>
          {#if e.is_dir}{pane.sizes[e.path] !== undefined ? size(pane.sizes[e.path]) : t("preview.folder")}{:else}{size(e.size)}{/if}
          · <span class="age" style:background={ageColor(e.modified, ui.cfg.looks[ui.theme])}>{age(e.modified)}</span>
          {date(e.modified)}
        </small>
      </div>
      {#if hasDiff}
        <div class="modes" role="group" aria-label={t("preview.git")}>
          <button class:on={!showDiff} onclick={() => (ui.previewDiff = false)}>{t("preview.file")}</button>
          <button class:on={showDiff} onclick={() => (ui.previewDiff = true)} title={t("preview.changes_against_head")}>{t("preview.diff")}</button>
        </div>
      {/if}
      {#if !diffing && ["markdown", "mermaid", "data", "jsonl", "calendar", "contacts", "graphviz", "asciidoc", "bom"].includes(kind)}
        <div class="modes" role="group" aria-label={t("preview.show")}>
          <button class:on={!source} onclick={() => (ui.previewSource = false)}>{kind === "data" || kind === "bom" ? t("preview.tree") : kind === "jsonl" ? t("preview.table") : t("preview.rendered")}</button>
          <button class:on={source} onclick={() => (ui.previewSource = true)}>{t("preview.source")}</button>
        </div>
      {/if}
    </header>

    <div class="body" class:flush={kind === "bom" && !source && !diffing}>
      {#if diffing}
        {#if html}<pre class="mono code"><code class="hljs">{@html html}</code></pre>{/if}
      {:else if kind === "bom" && !source}
        <BomView path={e.path} />
      {:else if kind === "bom"}
        {#if html}<pre class="mono code"><code class="hljs">{@html html}</code></pre>{:else}<pre class="mono">{text}</pre>{/if}
        {#if truncated}<p class="more">{t("preview.showing_first", { size: size(LIMIT) })}</p>{/if}
      {:else if tree !== undefined}
        <div class="tree">{@render node(null, tree, 0)}</div>
      {:else if table}
        {@render grid(table)}
      {:else if cards?.events}
        <ul class="cards">
          {#each cards.events as ev, i (i)}
            <li><b>{ev.title}</b><small>{ev.start}{ev.end ? ` – ${ev.end}` : ""}{ev.where ? ` · ${ev.where}` : ""}</small>{#if ev.note}<p>{ev.note}</p>{/if}</li>
          {:else}<li>{t("preview.no_events")}</li>{/each}
        </ul>
      {:else if cards?.people}
        <ul class="cards">
          {#each cards.people as c, i (i)}
            <li><b>{c.name ?? t("preview.no_name")}</b>{#if c.org || c.title}<small>{[c.title, c.org].filter(Boolean).join(", ")}</small>{/if}
              {#each c.email as m (m)}<p>{m}</p>{/each}{#each c.tel as n (n)}<p>{n}</p>{/each}</li>
          {:else}<li>{t("preview.no_contacts")}</li>{/each}
        </ul>
      {:else if kind === "log" && html}
        <pre class="mono log">{@html html}</pre>
        {#if truncated}<p class="more">{t("preview.showing_first", { size: size(LIMIT) })}</p>{/if}
      {:else if backend?.error}
        <p class="more">{backend.error}</p>
      {:else if backend?.sqlite}
        <table class="sheet db">
          <tbody>
            <tr><th>{t("preview.db_table")}</th><th>{t("preview.db_rows")}</th></tr>
            {#each backend.sqlite as tb (tb.name)}
              <tr>
                <td><details><summary>{tb.kind === "view" ? t("preview.view", { name: tb.name }) : tb.name}</summary><pre class="mono">{tb.sql}</pre></details></td>
                <td class="num">{tb.rows != null ? num(tb.rows) : "–"}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else if backend?.epub}
        {#if backend.epub.title}<h2 class="book">{backend.epub.title}</h2>{/if}
        <article class="markdown">{@html clean(backend.epub.html)}</article>
      {:else if backend?.cert}
        {#each backend.cert as c, i (i)}
          <dl class="facts cert">
            <dt>{t("preview.subject")}</dt><dd>{c.subject}</dd>
            <dt>{t("preview.issuer")}</dt><dd>{c.is_ca ? t("preview.ca", { name: c.issuer }) : c.issuer}</dd>
            <dt>{t("preview.valid")}</dt><dd>{c.not_before} – {c.not_after}</dd>
            <dt>{t("preview.expires")}</dt>
            <dd class:bad={c.expires_in < 0} class:warn={c.expires_in >= 0 && c.expires_in < 30 * 86400}>
              {c.expires_in < 0 ? tn("preview.expired_ago", -days(c.expires_in)) : tn("preview.expires_in", days(c.expires_in))}
            </dd>
            {#if c.names.length}<dt>{t("preview.names")}</dt><dd>{c.names.join(", ")}</dd>{/if}
            <dt>{t("preview.serial")}</dt><dd class="mono">{c.serial}</dd>
          </dl>
        {/each}
      {:else if backend?.mail}
        <dl class="facts">
          <dt>{t("preview.subject")}</dt><dd><b>{backend.mail.subject}</b></dd>
          <dt>{t("preview.from")}</dt><dd>{backend.mail.from}</dd>
          <dt>{t("preview.to")}</dt><dd>{backend.mail.to}</dd>
          <dt>{t("preview.date")}</dt><dd>{backend.mail.date}</dd>
          {#if backend.mail.attachments.length}
            <dt>{t("preview.attachments")}</dt><dd>{#each backend.mail.attachments as [n, sz], i (i)}<div>{n} ({size(sz)})</div>{/each}</dd>
          {/if}
        </dl>
        <pre class="mail">{backend.mail.text}</pre>
      {:else if backend?.plist}
        <pre class="mono code"><code class="hljs">{@html highlight(backend.plist, "xml")}</code></pre>
      {:else if conv}
        <div class="engines" role="group" aria-label={t("preview.render_with")}>
          {#each conv.engines as en (en.id)}
            <button class:on={engineOf(conv)?.id === en.id} disabled={!en.available || conv.status === "running"} title={en.note} onclick={() => renderConv(en.id)}>{en.label}</button>
          {/each}
        </div>
        {#if rich?.slides && conv.status !== "done"}
          <!-- The deck at once; LibreOffice's exact rendering takes its place when it is ready. -->
          {#if conv.status === "running"}<p class="more">{t("preview.exact_coming", { engine: engineOf(conv)?.label })}</p>{/if}
          {#if conv.status === "error"}<p class="more">{conv.error.split("\n")[0]}</p>{/if}
          <div class="slides">
            {#each rich.slides as s, i (i)}<div class="slide">{@html s}</div>{/each}
          </div>
        {:else if !conv.engines.some((x) => x.available)}
          <p class="more">{t("preview.no_engine", { notes: conv.engines.map((x) => x.note).filter(Boolean).join(". ") })}</p>
        {:else if conv.status === "idle"}
          <button class="render" onclick={() => renderConv()}>{t(conv.verb)}</button>
          <p class="more">{engineOf(conv)?.note}</p>
        {:else if conv.status === "running"}
          <p class="more">{t("preview.rendering_with", { engine: engineOf(conv)?.label })}</p>
          {#if pulling}<p class="more mono">{pulling}</p>{/if}
        {:else if conv.status === "error"}
          <pre class="diagram-error mono">{conv.error}</pre>
          <button class="render" onclick={() => renderConv()}>{t("common.try_again")}</button>
        {:else if conv.result?.kind === "pdf"}
          {#if conv.result.instead}<p class="more">{conv.result.instead}</p>{/if}
          {#if conv.result.note}<details class="note"><summary>{t("preview.built_with_errors")}</summary><pre class="mono">{conv.result.note}</pre></details>{/if}
          <iframe class="pdf" src={convertFileSrc(conv.result.file) + "#zoom=page-width"} title={e.name}></iframe>
        {:else if conv.result?.kind === "svg"}
          <div class="media svg"><img src={convertFileSrc(conv.result.file)} alt={e.name} /></div>
        {:else if conv.result?.kind === "html"}
          <article class="markdown">{@html clean(conv.result.text)}</article>
        {:else if conv.result?.kind === "json"}
          {@render grid(jsonTable(conv.result.text))}
        {/if}
      {:else if rich?.parquet}
        <p class="more">{t("preview.parquet_summary", { rows: tn("preview.rows", rich.parquet.rows), columns: tn("preview.columns", rich.parquet.schema.length) })}</p>
        {@render grid(rich.parquet.table)}
        <details class="schema">
          <summary>{t("preview.schema")}</summary>
          {@render grid([[t("preview.column"), t("preview.type"), t("preview.repetition")], ...rich.parquet.schema])}
        </details>
      {:else if rich?.slides}
        <div class="slides">
          {#each rich.slides as s, i (i)}<div class="slide">{@html s}</div>{/each}
        </div>
      {:else if rich?.drawio !== undefined}
        <div class="drawio" use:drawioView={rich.drawio}></div>
      {:else if kind === "image"}
        <div class="media checker"><img src={convertFileSrc(e.path)} alt={e.name} /></div>
      {:else if kind === "video"}
        <div class="media">
          <!-- svelte-ignore a11y_media_has_caption -->
          <video src={convertFileSrc(e.path)} controls preload="metadata"></video>
        </div>
      {:else if kind === "audio"}
        <div class="media"><audio src={convertFileSrc(e.path)} controls preload="metadata"></audio></div>
      {:else if kind === "pdf"}
        <iframe class="pdf" src={convertFileSrc(e.path) + "#zoom=page-width"} title={e.name}></iframe>
      {:else if kind === "archive"}
        {#if archive?.error}
          <p class="more">{archive.error}</p>
        {:else if archive}
          <p class="more">{tn(archive.more ? "preview.archive_more" : "preview.archive", archive.entries.length)}</p>
          <ul class="archive mono">
            {#each archive.entries as a (a.name)}
              <li class:dir={a.is_dir}><span>{a.name}</span>{#if !a.is_dir}<span class="sz">{size(a.size)}</span>{/if}</li>
            {/each}
          </ul>
        {/if}
      {:else if ["markdown", "mermaid", "graphviz", "asciidoc"].includes(kind) && html && !source}
        <article class="markdown">{@html html}</article>
      {:else if ["markdown", "mermaid", "graphviz", "asciidoc"].includes(kind) && html}
        <pre class="mono code"><code class="hljs">{@html html}</code></pre>
      {:else if rich?.error}
        <p class="more">{rich.error}</p>
      {:else if rich?.html !== undefined}
        <article class="markdown {kind}">{@html rich.html}</article>
      {:else if rich?.family}
        <div class="font" style:font-family="'{rich.family}'">
          <p style:font-size="2.6em">{t("preview.font_sample1")}</p>
          <p style:font-size="1.6em">{t("preview.font_sample2")}</p>
          <p style:font-size="1.1em">ABCDEFGHIJKLMNOPQRSTUVWXYZ<br />abcdefghijklmnopqrstuvwxyz<br />0123456789 &amp;@#$%*(){"{}"}[]&lt;&gt;?!</p>
          <p style:font-size="0.85em">{t("preview.font_sample3")}</p>
        </div>
      {:else if rich?.sheet}
        {#if rich.sheet.names.length > 1}
          <div class="modes sheets" role="group" aria-label={t("preview.sheet")}>
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
        {#if rows.length > 200}<p class="more">{tn("preview.first_rows", 200)}</p>{/if}
      {:else if kind === "text"}
        {#if html}
          <pre class="mono code"><code class="hljs">{@html html}</code></pre>
        {:else}
          <pre class="mono" class:hex={binary}>{text}</pre>
        {/if}
        {#if truncated}<p class="more">{t("preview.showing_first", { size: size(LIMIT) })}</p>{/if}
      {:else if kind === "folder"}
        <dl class="facts">
          {#if summary}
            <dt>{t("preview.contents")}</dt><dd>{t("preview.contents_value", { folders: tn("preview.folders", summary.folders), files: tn("preview.files", summary.files) })}</dd>
            <dt>{t("preview.files_here")}</dt><dd>{size(summary.bytes)}</dd>
            {#if summary.newest}<dt>{t("preview.newest")}</dt><dd>{date(summary.newest)} ({age(summary.newest)})</dd>{/if}
          {/if}
          <dt>{t("preview.total_size")}</dt>
          <dd>
            {#if pane.sizes[e.path] !== undefined}{size(pane.sizes[e.path])}{:else}<button class="link" onclick={calcSize}>{t("preview.calculate")}</button>{/if}
          </dd>
          {#if pane.git}<dt>{t("preview.git")}</dt><dd class="git">{pane.git.prompt}</dd>{/if}
        </dl>
      {/if}
      {#if facts.length}
        <dl class="facts extra">
          {#each facts as [k, v] (k)}<dt>{k}</dt><dd>{v}</dd>{/each}
        </dl>
      {/if}
    </div>

    <section class="notes">
      <label for="notes">{"\u{f249}"} {notesDir === pane.dir ? t("preview.notes_here") : t("preview.notes_for", { name: e.name })}</label>
      <textarea id="notes" dir="auto" bind:this={noteArea} bind:value={note} onblur={saveNote} placeholder={t("preview.notes_placeholder")}></textarea>
    </section>
  {/if}
</aside>

<style>
  .slides {
    display: grid;
    gap: 10px;
    overflow: auto;
  }
  .slide {
    background: #fff;
    color: #000;
    box-shadow: 0 0 0 1px var(--border-fg);
    overflow: hidden;
  }
  .note {
    margin: 0 0 6px;
    color: var(--git-modified-fg);
  }
  .note pre {
    white-space: pre-wrap;
    font-size: 0.85em;
    margin: 4px 0 0;
  }
  /* Diagrams are drawn for a white page, whatever the theme. */
  .drawio {
    flex: 1;
    min-height: 240px;
    overflow: auto;
    background: #fff;
    color: #000;
    border-radius: var(--r);
  }
  .preview {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    background: var(--preview-bg);
    color: var(--preview-fg);
    border-radius: var(--r);
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
    border-radius: var(--r-sm);
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
  .body.flush {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    padding: 0;
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
    border-radius: var(--r);
  }
  .pdf {
    width: 100%;
    height: 100%;
    min-height: 60vh;
    border: 0;
    border-radius: var(--r);
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
    border-radius: var(--r);
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
    border-radius: var(--r);
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
    text-align: start;
  }
  .sheet th {
    background: var(--dialog-input-bg);
    font-weight: 600;
  }
  .tree {
    font-family: var(--mono-font);
    font-size: 0.9em;
    line-height: 1.5;
  }
  .tree summary {
    cursor: pointer;
  }
  .tree .kids {
    padding-inline-start: 1.2em;
    border-inline-start: 1px solid color-mix(in srgb, var(--border-fg) 60%, transparent);
    margin-inline-start: 0.3em;
  }
  .tree .k {
    color: var(--directory-fg);
  }
  .tree .meta {
    color: var(--hidden-fg);
  }
  .tree .string {
    color: var(--git-added-fg);
  }
  .tree .number,
  .tree .boolean {
    color: var(--search-hit-fg);
  }
  .tree .null {
    color: var(--hidden-fg);
  }
  .log :global(.lv-error) {
    color: var(--git-deleted-fg);
  }
  .log :global(.lv-warn) {
    color: var(--git-modified-fg);
  }
  .log :global(.lv-info) {
    color: var(--git-added-fg);
  }
  .log :global(.lv-debug) {
    color: var(--hidden-fg);
  }
  .cards {
    list-style: none;
    padding: 0;
    margin: 0;
  }
  .cards li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 10px;
    margin-bottom: 6px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
  }
  .cards p {
    margin: 0;
    white-space: pre-wrap;
  }
  .db td {
    vertical-align: top;
  }
  .db summary {
    cursor: pointer;
  }
  .db pre {
    white-space: pre-wrap;
    font-size: 0.85em;
    color: var(--hidden-fg);
  }
  .num {
    text-align: end !important;
  }
  .book {
    margin: 0 0 8px;
    font-size: 1.1em;
  }
  .cert {
    margin-bottom: 14px;
  }
  .cert dd,
  .facts dd {
    overflow-wrap: anywhere;
  }
  .bad {
    color: var(--git-deleted-fg);
    font-weight: 600;
  }
  .warn {
    color: var(--git-modified-fg);
    font-weight: 600;
  }
  .mail {
    white-space: pre-wrap;
    font-family: var(--font);
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--border-fg);
  }
  .facts.extra {
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--border-fg);
  }
  .engines {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 10px;
  }
  .engines button,
  .render {
    font: inherit;
    font-size: 0.85em;
    color: var(--panel-fg);
    background: none;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 3px 10px;
    cursor: pointer;
  }
  .engines button.on {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
    border-color: transparent;
  }
  .engines button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .render {
    background: var(--accent-bg);
    color: var(--accent-fg);
    border-color: transparent;
    padding: 5px 14px;
  }
  .media.svg img {
    background: #fff;
    border-radius: var(--r);
    padding: 8px;
  }
  .schema {
    margin-top: 10px;
  }
  .checker {
    background: repeating-conic-gradient(color-mix(in srgb, var(--border-fg) 50%, transparent) 0 25%, transparent 0 50%) 0 0 / 16px 16px;
    border-radius: var(--r);
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
    border-radius: var(--r-sm);
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
    border-radius: var(--r);
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
  .code :global(.hljs-addition) { color: var(--git-added-fg); background: color-mix(in srgb, var(--git-added-fg) 12%, transparent); }
  .code :global(.hljs-deletion) { color: var(--git-deleted-fg); background: color-mix(in srgb, var(--git-deleted-fg) 12%, transparent); }
</style>

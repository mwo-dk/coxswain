<script>
  import { ui, load, openItem, toggleMark, dragOut, columnMenu } from "./app.svelte.js";
  import { size, date, age, ageColor, TAG_COLORS, tagName } from "./lib.js";
  import { t as tr, num } from "./i18n.svelte.js"; // `t` is the tab here
  import Rows from "./Rows.svelte";

  /** @type {{ t: any, active: boolean, onfocus: Function }} */
  let { t, active, onfocus } = $props();
  let root = $state();
  let width = $state(1000);

  $effect(() => {
    if (!root) return;
    const ro = new ResizeObserver(() => (width = root.clientWidth));
    ro.observe(root);
    return () => ro.disconnect();
  });

  // Chosen columns, minus the ones a narrow pane has no room for. Modified shrinks to its age
  // chip before anything else goes.
  const COLS = [
    // rem, not em: the header's smaller font must not make its columns narrower than the rows'.
    { id: "type", w: "6rem", min: 620 },
    { id: "size", w: "6.5rem", min: 340 },
    { id: "files", w: "5rem", min: 620 },
    { id: "modified", w: "13rem", narrow: "3.2rem" },
    // The last commit: in a repository (or a history), when switched on in Settings.
    { id: "commit", w: "12rem", min: 620 },
    { id: "created", w: "6.5rem", min: 620 },
  ];
  const has = (id) => id !== "commit" || (ui.cfg.settings.git_last_commit && !!t.last);
  const shown = $derived(new Set(COLS.filter((c) => ui.columns[c.id] && width >= (c.min ?? 0) && has(c.id)).map((c) => c.id)));
  const template = $derived(
    ["minmax(0, 1fr)", ...COLS.filter((c) => shown.has(c.id)).map((c) => (c.narrow && width < 620 ? c.narrow : c.w))].join(" "),
  );

  /** Every row is this high (`--row`, set by applyTheme from the same two settings). */
  const rowH = $derived(Math.round(ui.cfg.gui.font_size * ui.cfg.gui.line_height));

  const g = $derived(ui.cfg.glyphs);
  const gitGlyph = (kind) =>
    ({ modified: g.modified, added: g.staged, untracked: g.untracked, deleted: g.deleted, renamed: g.renamed, conflict: g.conflict, ignored: g.ignored })[kind] ?? "";

  /** The git status for a tooltip: "modified", or "modified (staged)". */
  const gitTip = (st) => {
    const k = tr(`details.git.${st.kind}`);
    return st.staged ? tr("details.git_staged", { kind: k }) : k;
  };

  function kind(e) {
    if (e.is_dir) return "directory";
    if (e.is_symlink) return "symlink";
    if (e.is_exec) return "executable";
    return "";
  }

  const ext = (e) => (!e.is_dir && e.name.lastIndexOf(".") > 0 ? e.name.slice(e.name.lastIndexOf(".") + 1) : "");

  function sortBy(k) {
    onfocus();
    t.reverse = t.sort === k && !t.reverse;
    t.sort = k;
    load(t);
  }
  const arrow = (k) => (t.sort === k ? (t.reverse ? "\u{f0de}" : "\u{f0dd}") : "");

  function click(ev, i) {
    onfocus();
    if (ev.shiftKey) {
      const [a, b] = [Math.min(t.cursor, i), Math.max(t.cursor, i)];
      for (let r = a; r <= b; r++) if (t.items[r].name !== "..") t.marked.add(t.items[r].path);
    }
    t.cursor = i;
    if (ev.ctrlKey || ev.metaKey) toggleMark(t, i);
  }
</script>

<div class="details" bind:this={root} style:--cols={template} class:narrow={width < 620}>
  <div
    class="cols head"
    role="row"
    tabindex="-1"
    title={tr("details.head_tip")}
    oncontextmenu={(ev) => {
      ev.preventDefault();
      columnMenu();
    }}
  >
    <button onclick={() => sortBy("name")}>{tr("details.col.name")} <i>{arrow("name")}</i></button>
    {#if shown.has("type")}<button onclick={() => sortBy("ext")}>{tr("details.col.type")} <i>{arrow("ext")}</i></button>{/if}
    {#if shown.has("size")}<button class="r" onclick={() => sortBy("size")}>{tr("details.col.size")} <i>{arrow("size")}</i></button>{/if}
    {#if shown.has("files")}<span class="r" title={tr("details.files_tip")}>{tr("details.col.files")}</span>{/if}
    {#if shown.has("modified")}<button onclick={() => sortBy("time")}>{tr("details.col.modified")} <i>{arrow("time")}</i></button>{/if}
    {#if shown.has("commit")}<button onclick={() => sortBy("commit")} title={tr("details.commit_tip")}>{tr("details.col.commit")} <i>{arrow("commit")}</i></button>{/if}
    {#if shown.has("created")}<span>{tr("details.col.created")}</span>{/if}
  </div>
  <Rows class="rows" items={t.items} {rowH} cursor={t.cursor} bind:top={t.top} role="listbox" tabindex="-1" aria-label={t.dir}>
    {#snippet row(e, i)}
      {@const st = e.name === ".." ? undefined : (t.git?.files[e.name] ?? t.git?.all)}
      {@const marked = t.marked.has(e.path)}
      <!-- Keyboard handling is global (App.svelte); rows are pointer targets. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        role="option"
        tabindex="-1"
        aria-selected={i === t.cursor}
        class="cols row {kind(e)}"
        class:hidden={e.hidden}
        class:marked
        class:cursor={i === t.cursor}
        class:focused={active}
        draggable={e.name !== ".."}
        ondragstart={(ev) => {
          ev.preventDefault();
          dragOut(t, i);
        }}
        onclick={(ev) => click(ev, i)}
        ondblclick={() => openItem(t, i)}
        oncontextmenu={(ev) => {
          ev.preventDefault();
          onfocus();
          t.cursor = i;
          toggleMark(t, i);
        }}
      >
        <span class="name">
          <span class="icon" style:color={e.icon.color || null}>{e.name === ".." ? "\u{f062}" : e.icon.glyph}</span>
          <span class="label">{e.name}</span>
          {#if e.tag}<span class="tag" style:background={TAG_COLORS[e.tag]} title={tagName(e.tag)}></span>{/if}
          {#if st}<span class="git git-{st.kind}" title={gitTip(st)}>{gitGlyph(st.kind)}</span>{/if}
        </span>
        {#if shown.has("type")}<span class="ext">{e.is_dir ? (e.name === ".." ? "" : tr("details.folder")) : ext(e)}</span>{/if}
        {#if shown.has("size")}
          <span class="size">
            {#if e.is_dir}{t.sizes[e.path] !== undefined ? size(t.sizes[e.path]) : ""}{:else}{size(e.size)}{/if}
          </span>
        {/if}
        {#if shown.has("files")}<span class="size">{e.is_dir && t.counts[e.path] !== undefined ? num(t.counts[e.path]) : ""}</span>{/if}
        {#if shown.has("modified")}
          <span class="time">
            {#if e.name !== ".."}
              <span class="age" style:background={ageColor(e.modified, ui.cfg.looks[ui.theme])} title={date(e.modified)}>{age(e.modified)}</span><span class="d">{date(e.modified)}</span>
            {/if}
          </span>
        {/if}
        {#if shown.has("commit")}
          {@const l = t.last?.[e.name]}
          <span class="time commit" title={l ? `${l.hash} · ${l.subject}\n${l.author} · ${date(l.time)}` : ""}>
            {#if l}<span>{date(l.time).replace(/ \d\d:\d\d/, "")}</span><span class="who">{l.author}</span>{:else if l === null}{tr("history.older")}{/if}
          </span>
        {/if}
        {#if shown.has("created")}<span class="time">{date(e.created).replace(/ \d\d:\d\d/, "")}</span>{/if}
      </div>
    {/snippet}
  </Rows>
</div>

<style>
  .details {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    container-type: inline-size;
  }
  .cols {
    display: grid;
    grid-template-columns: var(--cols);
    align-items: center;
    height: var(--row);
    white-space: nowrap;
    padding: 0 8px;
    column-gap: 10px;
  }
  .cols > * {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .head {
    color: var(--header-fg);
    font-size: 0.85em;
    border-bottom: 1px solid var(--border-fg);
    outline: none;
  }
  .head button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    text-align: start;
    cursor: pointer;
  }
  .head .r {
    width: 100%;
    text-align: end;
  }
  .head button:hover {
    color: var(--panel-fg);
  }
  .head i {
    font-style: normal;
    font-family: var(--icon-font);
  }
  /* A narrow pane keeps the age chip and drops the date. */
  .narrow .d {
    display: none;
  }
  .r,
  .size {
    text-align: end;
  }
  /* The list itself is Rows.svelte's element, outside this style's scope. */
  .details :global(.rows) {
    flex: 1;
    overflow-y: auto;
    outline: none;
    padding: 0 4px 8px;
  }
  .row {
    cursor: default;
    user-select: none;
    border-radius: var(--r-sm);
    padding: 0 4px;
  }
  .row:hover {
    background: color-mix(in srgb, var(--cursor-bg) 40%, transparent);
  }
  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .icon {
    font-family: var(--icon-font);
    width: 1.25em;
    flex: none;
    text-align: center;
    font-size: 1.05em;
  }
  .directory .icon {
    color: var(--directory-fg);
  }
  .directory .label {
    color: var(--directory-fg);
    font-weight: var(--directory-weight);
  }
  .executable .label {
    color: var(--executable-fg);
  }
  .symlink .label {
    color: var(--symlink-fg);
    font-style: italic;
  }
  .hidden {
    opacity: 0.6;
  }
  .ext,
  .time {
    color: var(--hidden-fg);
  }
  .size {
    font-variant-numeric: tabular-nums;
  }
  .time {
    display: flex;
    align-items: center;
    gap: 8px;
    font-variant-numeric: tabular-nums;
  }
  .who {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .age {
    display: inline-block;
    min-width: 2.6em;
    text-align: center;
    border-radius: var(--r-sm);
    font-size: 0.8em;
    color: #fff;
    text-shadow: 0 0 2px rgb(0 0 0 / 0.6);
    line-height: 1.5;
  }
  .tag {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .git {
    font-family: var(--icon-font);
    flex: none;
  }
  .git-modified { color: var(--git-modified-fg); }
  .git-added { color: var(--git-added-fg); }
  .git-untracked { color: var(--git-untracked-fg); }
  .git-deleted { color: var(--git-deleted-fg); }
  .git-renamed { color: var(--git-renamed-fg); }
  .git-conflict { color: var(--git-conflict-fg); font-weight: bold; }
  .git-ignored { color: var(--git-ignored-fg); }
  .marked {
    background: color-mix(in srgb, var(--marked-fg) 16%, transparent);
  }
  .marked .label {
    color: var(--marked-fg);
    font-weight: var(--marked-weight);
  }
  .cursor {
    outline: 1px solid var(--cursor-bg);
    outline-offset: -1px;
  }
  .cursor.focused {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
    outline: none;
  }
  .cursor.focused.marked {
    background: var(--marked-cursor-bg);
  }
</style>

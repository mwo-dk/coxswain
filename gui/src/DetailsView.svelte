<script>
  import { ui, load, openItem, toggleMark } from "./app.svelte.js";
  import { size, date, age, ageColor, TAG_COLORS } from "./lib.js";

  /** @type {{ t: any, active: boolean, onfocus: Function }} */
  let { t, active, onfocus } = $props();
  let list = $state();

  $effect(() => {
    list?.children[t.cursor]?.scrollIntoView({ block: "nearest" });
  });

  const g = $derived(ui.cfg.glyphs);
  const gitGlyph = (kind) =>
    ({ modified: g.modified, added: g.staged, untracked: g.untracked, deleted: g.deleted, renamed: g.renamed, conflict: g.conflict, ignored: g.ignored })[kind] ?? "";

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

<div class="details">
  <div class="cols head">
    <button onclick={() => sortBy("name")}>Name <i>{arrow("name")}</i></button>
    <button onclick={() => sortBy("ext")}>Type <i>{arrow("ext")}</i></button>
    <button class="r" onclick={() => sortBy("size")}>Size <i>{arrow("size")}</i></button>
    <button onclick={() => sortBy("time")}>Modified <i>{arrow("time")}</i></button>
  </div>
  <div class="rows" bind:this={list} role="listbox" tabindex="-1" aria-label={t.dir}>
    {#each t.items as e, i (e.path)}
      {@const st = t.git?.files[e.name]}
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
          t.cursor = i;
          ev.dataTransfer.setData("text/plain", e.path);
          ev.dataTransfer.effectAllowed = "copyMove";
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
          {#if e.tag}<span class="tag" style:background={TAG_COLORS[e.tag]} title={e.tag}></span>{/if}
          {#if st}<span class="git git-{st.kind}" title="{st.kind}{st.staged ? ' (staged)' : ''}">{gitGlyph(st.kind)}</span>{/if}
        </span>
        <span class="ext">{e.is_dir ? (e.name === ".." ? "" : "Folder") : ext(e)}</span>
        <span class="size">
          {#if e.is_dir}{t.sizes[e.path] !== undefined ? size(t.sizes[e.path]) : ""}{:else}{size(e.size)}{/if}
        </span>
        <span class="time">
          {#if e.name !== ".."}
            <span class="age" style:background={ageColor(e.modified)} title={date(e.modified)}>{age(e.modified)}</span><span class="d">{date(e.modified)}</span>
          {/if}
        </span>
      </div>
    {/each}
  </div>
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
    grid-template-columns: minmax(0, 1fr) 6em 6.5em 11.5em;
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
  }
  .head button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    text-align: left;
    cursor: pointer;
  }
  .head button:hover {
    color: var(--panel-fg);
  }
  .head i {
    font-style: normal;
    font-family: var(--icon-font);
  }
  /* Narrow panes drop Type, then the date (the age chip stays), then Size. */
  @container (max-width: 620px) {
    .cols {
      grid-template-columns: minmax(0, 1fr) 6.5em 3.2em;
    }
    .cols > :nth-child(2) {
      display: none;
    }
    .d {
      display: none;
    }
  }
  @container (max-width: 340px) {
    .cols {
      grid-template-columns: minmax(0, 1fr) 3.2em;
    }
    .cols > :nth-child(3) {
      display: none;
    }
  }
  .r,
  .size {
    text-align: right;
  }
  .rows {
    flex: 1;
    overflow-y: auto;
    outline: none;
    padding: 2px 4px 8px;
  }
  .row {
    content-visibility: auto;
    contain-intrinsic-size: auto var(--row);
    cursor: default;
    user-select: none;
    border-radius: 5px;
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
  .age {
    display: inline-block;
    min-width: 2.6em;
    text-align: center;
    border-radius: 4px;
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

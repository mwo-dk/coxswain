<script>
  import { size, date } from "./lib.js";

  /** @type {{ panel: any, active: boolean, glyphs: any, onpick: Function, onopen: Function,
   *  onmark: Function, onsort: Function, ongoto: Function, ondropfiles: Function }} */
  let { panel, active, glyphs, onpick, onopen, onmark, onsort, ongoto, ondropfiles } = $props();

  let list = $state();
  let dragOver = $state(false);

  // Keep the cursor row in view.
  $effect(() => {
    const row = list?.children[panel.cursor];
    row?.scrollIntoView({ block: "nearest" });
  });

  const gitGlyph = (kind) =>
    ({
      modified: glyphs.modified,
      added: glyphs.staged,
      untracked: glyphs.untracked,
      deleted: glyphs.deleted,
      renamed: glyphs.renamed,
      conflict: glyphs.conflict,
      ignored: glyphs.ignored,
    })[kind] ?? "";

  function kind(e) {
    if (panel.marked.has(e.path)) return "marked";
    if (e.is_dir) return "directory";
    if (e.is_symlink) return "symlink";
    if (e.is_exec) return "executable";
    if (e.hidden) return "hidden";
    return "";
  }

  let info = $derived.by(() => {
    if (panel.error) return { text: panel.error, cls: "error" };
    if (panel.marked.size) {
      const bytes = panel.entries.filter((e) => panel.marked.has(e.path)).reduce((a, e) => a + e.size, 0);
      return { text: `${size(bytes)} bytes in ${panel.marked.size} selected`, cls: "marked" };
    }
    const e = panel.entries[panel.cursor];
    return { text: e ? `${e.name}${e.is_dir ? "" : "  " + size(e.size)}` : "", cls: "" };
  });

  const sortLabel = { name: "Name", ext: "Ext", time: "Modified", size: "Size" };
  const arrow = (k) => (panel.sort === k ? (panel.reverse ? " ▲" : " ▼") : "");

  function click(ev, i) {
    if (ev.shiftKey) onmark(i, true); // range from the old cursor
    onpick(i);
    if (ev.ctrlKey || ev.metaKey) onmark(i);
  }
</script>

<section
  class="panel"
  class:active
  class:drag-over={dragOver}
  aria-label={panel.dir}
  ondragover={(e) => {
    e.preventDefault();
    dragOver = true;
  }}
  ondragleave={() => (dragOver = false)}
  ondrop={(e) => {
    e.preventDefault();
    dragOver = false;
    ondropfiles(e.shiftKey || e.dataTransfer.dropEffect === "move");
  }}
>
  <button class="path" title="Go to directory (Alt+F1 / Alt+F2)" onclick={ongoto}><bdi>{panel.dir}</bdi></button>
  <div class="cols head">
    <span></span>
    {#each ["name", "size", "time"] as k}
      <button class="col-{k}" onclick={() => onsort(k)}>{sortLabel[k]}{arrow(k)}</button>
    {/each}
  </div>
  <div class="rows" bind:this={list} role="listbox" aria-activedescendant="row-{panel.cursor}" tabindex="-1">
    {#each panel.entries as e, i (e.path)}
      {@const st = panel.git?.files[e.name]}
      <!-- Keyboard handling is global (App.svelte); rows are pointer targets only. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        tabindex="-1"
        id="row-{i}"
        role="option"
        aria-selected={i === panel.cursor}
        class="cols row {kind(e)}"
        class:cursor={i === panel.cursor && active}
        class:cursor-inactive={i === panel.cursor && !active}
        draggable={e.name !== ".."}
        ondragstart={(ev) => {
          onpick(i);
          ev.dataTransfer.setData("text/plain", e.path);
          ev.dataTransfer.effectAllowed = "copyMove";
        }}
        onclick={(ev) => click(ev, i)}
        ondblclick={() => onopen(i)}
        oncontextmenu={(ev) => {
          ev.preventDefault();
          onpick(i);
          onmark(i);
        }}
      >
        <span class="git git-{st?.kind}" title={st ? `${st.kind}${st.staged ? " (staged)" : ""}` : ""}>
          {st ? gitGlyph(st.kind) : ""}
        </span>
        <span class="name">{e.name}</span>
        <span class="size">{e.name === ".." ? "UP--DIR" : e.is_dir ? "SUB-DIR" : size(e.size)}</span>
        <span class="time">{e.name === ".." ? "" : date(e.modified)}</span>
      </div>
    {/each}
  </div>
  <div class="info {info.cls}">{info.text}</div>
  <div class="foot">
    <span class="git-branch">{panel.git?.prompt ?? ""}</span>
    <span>{panel.entries.length - 1} items</span>
  </div>
</section>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--panel-bg);
    color: var(--panel-fg);
    border: 3px double var(--border-fg);
    outline: 2px solid transparent;
  }
  .panel.drag-over {
    outline-color: var(--marked-fg);
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
  }
  .path {
    align-self: center;
    max-width: 100%;
    padding: 0 0.6em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
    color: var(--border-fg);
  }
  .active .path {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .cols {
    display: grid;
    grid-template-columns: 2ch minmax(0, 1fr) 10ch 17ch;
    align-items: center;
    height: var(--row);
    white-space: nowrap;
  }
  .cols > * {
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 0 0.3ch;
  }
  .cols > :nth-child(n + 3) {
    border-left: 1px solid var(--border-fg);
  }
  .head {
    color: var(--header-fg);
    font-weight: var(--header-weight);
    text-align: center;
  }
  .rows {
    flex: 1;
    overflow-y: auto;
    outline: none;
    scrollbar-color: var(--border-fg) transparent;
  }
  .row {
    content-visibility: auto;
    contain-intrinsic-size: auto var(--row);
    cursor: default;
    user-select: none;
  }
  .row .size {
    text-align: right;
  }
  .directory {
    color: var(--directory-fg);
    font-weight: var(--directory-weight);
  }
  .executable {
    color: var(--executable-fg);
  }
  .symlink {
    color: var(--symlink-fg);
  }
  .hidden {
    color: var(--hidden-fg);
  }
  .marked {
    color: var(--marked-fg);
    font-weight: var(--marked-weight);
  }
  .cursor {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .cursor.marked {
    background: var(--marked-cursor-bg);
    color: var(--marked-cursor-fg);
  }
  .cursor-inactive {
    outline: 1px dotted var(--border-fg);
    outline-offset: -1px;
  }
  .git-modified { color: var(--git-modified-fg); }
  .git-added { color: var(--git-added-fg); }
  .git-untracked { color: var(--git-untracked-fg); }
  .git-deleted { color: var(--git-deleted-fg); }
  .git-renamed { color: var(--git-renamed-fg); }
  .git-conflict { color: var(--git-conflict-fg); font-weight: bold; }
  .git-ignored { color: var(--git-ignored-fg); }
  .info {
    border-top: 1px solid var(--border-fg);
    padding: 0 0.5ch;
    height: var(--row);
    line-height: var(--row);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .info.marked {
    text-align: center;
  }
  .info.error {
    color: var(--git-conflict-fg);
  }
  .foot {
    display: flex;
    justify-content: space-between;
    gap: 1ch;
    padding: 0 0.5ch;
    color: var(--border-fg);
    white-space: nowrap;
  }
  .git-branch {
    color: var(--git-branch-fg);
    font-weight: var(--git-branch-weight);
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>

<script>
  import { tick } from "svelte";
  import { ui, newTab, load, cd, goBack, goForward, focusPane } from "./app.svelte.js";
  import { invoke, basename, parent, crumbs, size } from "./lib.js";
  import DetailsView from "./DetailsView.svelte";
  import ColumnsView from "./ColumnsView.svelte";

  /** @type {{ index: number, ondrop: Function }} */
  let { index, ondrop } = $props();

  const p = $derived(ui.panes[index]);
  const t = $derived(p.tabs[p.active]);
  const active = $derived(ui.activePane === index && !ui.modal);
  let editing = $state(false);
  let pathInput = $state();
  let pathValue = $state("");
  let dragOver = $state(false);

  export async function editPath() {
    pathValue = t.dir;
    editing = true;
    await tick();
    pathInput?.select();
  }

  async function submitPath() {
    editing = false;
    const dir = await invoke("resolve_path", { base: t.dir, input: pathValue });
    await cd(t, dir);
  }

  function addTab() {
    p.tabs.push(newTab(t.dir, t.view));
    p.active = p.tabs.length - 1;
    load(p.tabs[p.active]);
  }

  function closeTab(i) {
    if (p.tabs.length === 1) return;
    p.tabs.splice(i, 1);
    p.active = Math.min(p.active, p.tabs.length - 1);
  }

  const selBytes = $derived(t.items.filter((e) => t.marked.has(e.path)).reduce((a, e) => a + (e.is_dir ? t.sizes[e.path] ?? 0 : e.size), 0));
  const count = $derived(t.items.length - (t.items[0]?.name === ".." ? 1 : 0));
</script>

<section
  class="pane"
  class:active
  class:drag-over={dragOver}
  aria-label="Pane {index + 1}"
  onpointerdown={() => focusPane(index)}
  ondragover={(e) => {
    e.preventDefault();
    dragOver = true;
  }}
  ondragleave={() => (dragOver = false)}
  ondrop={(e) => {
    e.preventDefault();
    dragOver = false;
    ondrop(index, e.shiftKey);
  }}
>
  <div class="tabs" role="tablist">
    {#each p.tabs as tb, i (tb.id)}
      <div class="tab" class:current={i === p.active} role="tab" tabindex="-1" aria-selected={i === p.active}
        onclick={() => focusPane(index, i)} onauxclick={(e) => e.button === 1 && closeTab(i)} onkeydown={() => {}} title={tb.dir}>
        <span class="ticon">{tb.git ? "\u{e702}" : "\u{f07b}"}</span>
        <span class="tname">{basename(tb.dir) || tb.dir}</span>
        {#if p.tabs.length > 1}
          <button class="x" title="Close tab (Ctrl+W)" onclick={(e) => { e.stopPropagation(); closeTab(i); }}>×</button>
        {/if}
      </div>
    {/each}
    <button class="newtab" title="New tab (Ctrl+T)" onclick={addTab}>+</button>
  </div>

  <div class="bar">
    <button class="nav" title="Back (Alt+Left)" disabled={!t.back.length} onclick={() => goBack(t)}>{"\u{f060}"}</button>
    <button class="nav" title="Forward (Alt+Right)" disabled={!t.fwd.length} onclick={() => goForward(t)}>{"\u{f061}"}</button>
    <button class="nav" title="Up (Backspace)" disabled={!parent(t.dir)} onclick={() => cd(t, parent(t.dir))}>{"\u{f062}"}</button>
    {#if editing}
      <input
        class="path-edit"
        bind:this={pathInput}
        bind:value={pathValue}
        spellcheck="false"
        onkeydown={(e) => {
          if (e.key === "Enter") submitPath();
          if (e.key === "Escape") editing = false;
          e.stopPropagation();
        }}
        onblur={() => (editing = false)}
      />
    {:else}
      <!-- Ctrl+L is the keyboard way in; a click on empty space is a shortcut. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <div class="crumbs" role="navigation" onclick={(e) => e.target === e.currentTarget && editPath()} title="Click empty space or press Ctrl+L to type a path">
        {#each crumbs(t.dir, ui.cfg.home) as c, i (c.path)}
          {#if i > 0}<span class="sep">{"\u{f054}"}</span>{/if}
          <button class="crumb" onclick={() => cd(t, c.path)}>{c.name}</button>
        {/each}
      </div>
    {/if}
    <button class="nav" title="Details / columns (Alt+V)" onclick={() => (t.view = t.view === "details" ? "columns" : "details")}>
      {t.view === "details" ? "\u{f0db}" : "\u{f03a}"}
    </button>
  </div>

  {#if t.error}<div class="error">{t.error}</div>{/if}

  {#if t.view === "columns"}
    <ColumnsView {t} {active} onfocus={() => focusPane(index)} />
  {:else}
    <DetailsView {t} {active} onfocus={() => focusPane(index)} />
  {/if}

  <footer class="foot">
    <span>{count} items{#if t.marked.size}&nbsp;· <b>{t.marked.size} selected</b> ({size(selBytes)}){/if}</span>
    {#if t.hasNotes}<span class="note" title="This folder has notes (Alt+N)">{"\u{f249}"}</span>{/if}
    <span class="git-prompt" title={t.git?.root ?? ""}>{t.git?.prompt ?? ""}</span>
  </footer>
</section>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    min-width: 180px;
    min-height: 0;
    flex: 1;
    background: var(--panel-bg);
    color: var(--panel-fg);
    border-radius: 8px;
    border: 1px solid var(--border-fg);
    overflow: hidden;
  }
  .pane.active {
    border-color: color-mix(in srgb, var(--accent-bg) 70%, var(--border-fg));
  }
  .pane.drag-over {
    outline: 2px dashed var(--accent-bg);
    outline-offset: -3px;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    cursor: pointer;
    padding: 0;
  }
  .tabs {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    padding: 4px 6px 0;
    background: var(--tab-bg);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 200px;
    padding: 5px 10px;
    border-radius: 7px 7px 0 0;
    color: var(--tab-fg);
    cursor: default;
    white-space: nowrap;
  }
  .tab:hover {
    background: color-mix(in srgb, var(--tab-active-bg) 50%, transparent);
  }
  .tab.current {
    background: var(--tab-active-bg);
    color: var(--tab-active-fg);
  }
  .ticon {
    font-family: var(--icon-font);
    color: var(--directory-fg);
  }
  .tname {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .x {
    opacity: 0;
    width: 1.3em;
    border-radius: 4px;
    line-height: 1.2;
  }
  .tab:hover .x,
  .tab.current .x {
    opacity: 0.7;
  }
  .x:hover {
    opacity: 1;
    background: var(--cursor-bg);
  }
  .newtab {
    padding: 4px 10px;
    color: var(--tab-fg);
    border-radius: 6px;
    margin-bottom: 2px;
  }
  .newtab:hover {
    background: var(--tab-active-bg);
  }
  .bar {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 2px;
    padding: 6px 6px;
    border-bottom: 1px solid var(--border-fg);
  }
  .nav {
    font-family: var(--icon-font);
    width: 28px;
    height: 26px;
    border-radius: 6px;
    color: var(--hidden-fg);
  }
  .nav:hover:not(:disabled) {
    background: var(--cursor-bg);
    color: var(--panel-fg);
  }
  .nav:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .crumbs,
  .path-edit {
    flex: 1;
    min-width: 0;
    height: 26px;
    margin: 0 4px;
    border-radius: 6px;
    background: var(--dialog-input-bg);
    border: 1px solid var(--border-fg);
    box-sizing: border-box;
  }
  .crumbs {
    display: flex;
    align-items: center;
    overflow: hidden;
    padding: 0 4px;
    cursor: text;
    white-space: nowrap;
  }
  .crumb {
    padding: 1px 6px;
    border-radius: 4px;
    cursor: pointer;
    flex: none;
  }
  .crumb:last-child {
    font-weight: 600;
  }
  .crumb:hover {
    background: var(--cursor-bg);
  }
  .sep {
    font-family: var(--icon-font);
    font-size: 0.65em;
    color: var(--hidden-fg);
    flex: none;
  }
  .path-edit {
    font: inherit;
    color: var(--dialog-input-fg);
    padding: 0 8px;
    outline: 1px solid var(--accent-bg);
  }
  .error {
    padding: 6px 12px;
    color: var(--git-deleted-fg);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 12px;
    font-size: 0.85em;
    color: var(--hidden-fg);
    border-top: 1px solid var(--border-fg);
    white-space: nowrap;
  }
  .foot b {
    color: var(--marked-fg);
    font-weight: 600;
  }
  .note {
    font-family: var(--icon-font);
    color: var(--marked-fg);
  }
  .git-prompt {
    margin-left: auto;
    font-family: var(--icon-font), var(--font);
    color: var(--git-branch-fg);
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>

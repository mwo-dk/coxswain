<script>
  // Miller columns: two ancestor levels, the current folder, and a peek into the folder
  // under the cursor. Left/Right walk the hierarchy (handled in App).
  import { ui, cd, load, openItem, toggleMark } from "./app.svelte.js";
  import { invoke, parent, TAG_COLORS } from "./lib.js";
  import { t as tr, i18n } from "./i18n.svelte.js"; // `t` is the tab here
  import Rows from "./Rows.svelte";

  /** @type {{ t: any, active: boolean, onfocus: Function }} */
  let { t, active, onfocus } = $props();
  let ancestors = $state([]);
  let peek = $state(null);
  let strip = $state();
  const rowH = $derived(Math.round(ui.cfg.gui.font_size * ui.cfg.gui.line_height));
  /** The current folder without `..`, which a column does not show; `skip` is its index shift. */
  const skip = $derived(t.items[0]?.name === ".." ? 1 : 0);
  const shown = $derived(skip ? t.items.slice(1) : t.items);

  async function list(dir) {
    try {
      return (await invoke("list_dir", { dir, showHidden: ui.showHidden, sort: t.sort, reverse: t.reverse })).items.filter((e) => e.name !== "..");
    } catch {
      return [];
    }
  }

  // Ancestor columns follow the directory.
  $effect(() => {
    const dir = t.dir;
    const chain = [];
    for (let d = parent(dir), n = 0; d && n < 2; d = parent(d), n++) chain.unshift(d);
    Promise.all(chain.map(async (d, i) => ({ dir: d, items: await list(d), open: chain[i + 1] ?? dir }))).then((cols) => {
      if (t.dir === dir) ancestors = cols;
    });
  });

  // The peek column follows the cursor, a moment later so fast scrolling stays smooth.
  $effect(() => {
    const e = t.items[t.cursor];
    if (!e?.is_dir || e.name === "..") return void (peek = null);
    const timer = setTimeout(async () => {
      const items = await list(e.path);
      if (t.items[t.cursor]?.path === e.path) peek = { dir: e.path, items };
    }, 90);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    ancestors;
    peek;
    // Right to left (Hebrew), the newest column is at the left end, and scrollLeft counts down.
    strip?.scrollTo({ left: i18n.rtl ? -strip.scrollWidth : strip.scrollWidth, behavior: "smooth" });
  });

  async function jump(dir, name) {
    onfocus();
    await cd(t, dir);
    const i = t.items.findIndex((x) => x.name === name);
    if (i >= 0) t.cursor = i;
  }
</script>

{#snippet entry(e, cls, onclick, ondblclick)}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="row {cls}" class:dir={e.is_dir} class:hidden={e.hidden} role="option" tabindex="-1" aria-selected={cls.includes("cursor")} {onclick} {ondblclick}>
    <span class="icon" style:color={e.icon.color || null}>{e.icon.glyph}</span>
    <span class="label">{e.name}</span>
    {#if e.tag}<span class="tag" style:background={TAG_COLORS[e.tag]}></span>{/if}
    {#if e.is_dir}<span class="chev flip">{"\u{f054}"}</span>{/if}
  </div>
{/snippet}

<div class="strip" bind:this={strip}>
  {#each ancestors as col (col.dir)}
    <Rows class="col" items={col.items} {rowH} cursor={col.items.findIndex((e) => e.path === col.open)} role="listbox" aria-label={col.dir}>
      {#snippet row(e)}
        {@render entry(e, e.path === col.open ? "open" : "", () => jump(col.dir, e.name), () => openItem(t))}
      {/snippet}
    </Rows>
  {/each}
  <Rows class="col current {active ? 'active' : ''}" items={shown} {rowH} cursor={t.cursor - skip} role="listbox" aria-label={t.dir}>
    {#snippet row(e, j)}
      {@const i = j + skip}
      {@render entry(
        e,
        `${i === t.cursor ? "cursor" : ""} ${t.marked.has(e.path) ? "marked" : ""}`,
        (ev) => {
          onfocus();
          t.cursor = i;
          if (ev.ctrlKey || ev.metaKey) toggleMark(t, i);
        },
        () => openItem(t, i),
      )}
    {/snippet}
  </Rows>
  {#if peek}
    {#if peek.items.length}
      <Rows class="col peek" items={peek.items} {rowH} role="listbox" aria-label={peek.dir}>
        {#snippet row(e)}
          {@render entry(e, "", () => jump(peek.dir, e.name), () => jump(peek.dir, e.name))}
        {/snippet}
      </Rows>
    {:else}
      <div class="col peek" role="listbox" aria-label={peek.dir}><p class="empty">{tr("columns.empty")}</p></div>
    {/if}
  {/if}
</div>

<style>
  :global([dir="rtl"]) .flip {
    transform: scaleX(-1);
  }
  .strip {
    flex: 1;
    display: flex;
    overflow-x: auto;
    overflow-y: hidden;
    min-height: 0;
    scrollbar-width: thin;
  }
  /* The columns are Rows.svelte's elements, outside this style's scope. */
  .strip :global(.col) {
    flex: 0 0 clamp(180px, 22%, 280px);
    overflow-y: auto;
    border-inline-end: 1px solid var(--border-fg);
    padding: 0 4px 4px;
    box-sizing: border-box;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    height: var(--row);
    padding: 0 8px;
    border-radius: var(--r-sm);
    white-space: nowrap;
    cursor: default;
    user-select: none;
  }
  .row:hover {
    background: color-mix(in srgb, var(--cursor-bg) 40%, transparent);
  }
  .icon,
  .chev {
    font-family: var(--icon-font);
    flex: none;
  }
  .dir .icon {
    color: var(--directory-fg);
  }
  .chev {
    font-size: 0.7em;
    color: var(--hidden-fg);
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hidden {
    opacity: 0.6;
  }
  .tag {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }
  .open {
    background: color-mix(in srgb, var(--cursor-bg) 60%, transparent);
  }
  .marked .label {
    color: var(--marked-fg);
    font-weight: var(--marked-weight);
  }
  .cursor {
    outline: 1px solid var(--cursor-bg);
    outline-offset: -1px;
  }
  :global(.active) .cursor {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
    outline: none;
  }
  .empty {
    color: var(--hidden-fg);
    text-align: center;
    margin-top: 2em;
  }
</style>

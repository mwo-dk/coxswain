<script>
  // Miller columns: two ancestor levels, the current folder, and a peek into the folder
  // under the cursor. Left/Right walk the hierarchy (handled in App).
  import { ui, cd, load, openItem, toggleMark } from "./app.svelte.js";
  import { invoke, parent, TAG_COLORS } from "./lib.js";

  /** @type {{ t: any, active: boolean, onfocus: Function }} */
  let { t, active, onfocus } = $props();
  let ancestors = $state([]);
  let peek = $state(null);
  let strip = $state();
  let current = $state();

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
    strip?.scrollTo({ left: strip.scrollWidth, behavior: "smooth" });
  });

  $effect(() => {
    current?.children[t.cursor]?.scrollIntoView({ block: "nearest" });
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
    {#if e.is_dir}<span class="chev">{"\u{f054}"}</span>{/if}
  </div>
{/snippet}

<div class="strip" bind:this={strip}>
  {#each ancestors as col (col.dir)}
    <div class="col" role="listbox" aria-label={col.dir}>
      {#each col.items as e (e.path)}
        {@render entry(e, e.path === col.open ? "open" : "", () => jump(col.dir, e.name), () => openItem(t))}
      {/each}
    </div>
  {/each}
  <div class="col current" class:active bind:this={current} role="listbox" aria-label={t.dir}>
    {#each t.items as e, i (e.path)}
      {#if e.name !== ".."}
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
      {:else}
        <div hidden></div>
      {/if}
    {/each}
  </div>
  {#if peek}
    <div class="col peek" role="listbox" aria-label={peek.dir}>
      {#each peek.items as e (e.path)}
        {@render entry(e, "", () => jump(peek.dir, e.name), () => jump(peek.dir, e.name))}
      {:else}
        <p class="empty">Empty folder</p>
      {/each}
    </div>
  {/if}
</div>

<style>
  .strip {
    flex: 1;
    display: flex;
    overflow-x: auto;
    overflow-y: hidden;
    min-height: 0;
    scrollbar-width: thin;
  }
  .col {
    flex: 0 0 clamp(180px, 22%, 280px);
    overflow-y: auto;
    border-right: 1px solid var(--border-fg);
    padding: 4px;
    box-sizing: border-box;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    height: var(--row);
    padding: 0 8px;
    border-radius: 5px;
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
  .active .cursor {
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

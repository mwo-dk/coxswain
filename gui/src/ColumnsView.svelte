<script>
  import { untrack } from "svelte";
  // Miller columns: two ancestor levels, the current folder, and a peek into the folder
  // under the cursor. Left/Right walk the hierarchy (handled in App).
  import { ui, cd, openItem, toggleMark, gitFacts, dragOut, newId } from "./app.svelte.js";
  import { invoke, parent, TAG_COLORS } from "./lib.js";
  import { Paged, SPAN } from "./paged.svelte.js";
  import { t as tr, i18n } from "./i18n.svelte.js"; // `t` is the tab here
  import Rows from "./Rows.svelte";

  /** @type {{ t: any, active: boolean, onfocus: Function }} */
  let { t, active, onfocus } = $props();
  let ancestors = $state([]);
  let peek = $state(null);
  let strip = $state();
  const rowH = $derived(Math.round(ui.cfg.gui.font_size * ui.cfg.gui.line_height));
  /** The current folder without `..`, which a column does not show; `skip` is its index shift. */
  const skip = $derived(t.up ? 1 : 0);
  const shown = $derived(t.items.skip(skip));

  /** A column's folder, paged as a tab's is (its own listing in the backend, dropped when the
   *  column goes), without `..`; `cursor`: the row of `focus` in it. */
  async function list(dir, focus = null) {
    const p = new Paged(newId());
    p.generation = 1;
    try {
      const r = await invoke("tab_list", { tab: p.id, generation: 1, dir, showHidden: ui.showHidden, sort: t.sort, reverse: t.reverse, focus, cursor: 0, span: SPAN });
      p.reset(r, 1);
      const n = r.up ? 1 : 0;
      return { items: p.skip(n), cursor: focus ? r.cursor - n : -1, id: p.id };
    } catch {
      invoke("tab_drop", { tab: p.id }).catch(() => {});
      return { items: [], cursor: -1, id: null };
    }
  }
  const drop = (cols) => cols.forEach((c) => c?.id != null && invoke("tab_drop", { tab: c.id }).catch(() => {}));
  // Gone (another view): the columns' listings go, and so do those still being read.
  let gone = false;
  $effect(() => () => {
    gone = true;
    drop([...ancestors, peek]);
  });

  // Ancestor columns follow the directory.
  $effect(() => {
    const dir = t.dir;
    const chain = [];
    for (let d = parent(dir), n = 0; d && n < 2; d = parent(d), n++) chain.unshift(d);
    Promise.all(chain.map(async (d, i) => ({ dir: d, open: chain[i + 1] ?? dir, ...(await list(d, chain[i + 1] ?? dir)) }))).then((cols) => {
      if (gone || t.dir !== dir) return drop(cols);
      drop(untrack(() => ancestors));
      ancestors = cols;
    });
  });

  // The peek column follows the cursor, a moment later so fast scrolling stays smooth.
  // The folder under the cursor, as a path: rows that come again do not make it new.
  const under = $derived.by(() => {
    const e = t.items.at(t.cursor);
    return e?.is_dir && e.name !== ".." ? e.path : null;
  });
  $effect(() => {
    const dir = under;
    // Read again when the folder is, or sorted or shown otherwise.
    t.items.listed, t.sort, t.reverse, ui.showHidden;
    if (!dir) {
      untrack(() => drop([peek]));
      return void (peek = null);
    }
    const timer = setTimeout(async () => {
      const col = { dir, ...(await list(dir)) };
      if (gone || under !== dir) return drop([col]);
      drop([untrack(() => peek)]);
      peek = col;
    }, 90);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    ancestors;
    peek;
    // Right to left (Hebrew), the newest column is at the left end, and scrollLeft counts down.
    strip?.scrollTo({ left: i18n.rtl ? -strip.scrollWidth : strip.scrollWidth, behavior: "smooth" });
  });

  /** To `dir`, the cursor on `name`: its rows come around it. */
  async function jump(dir, name) {
    onfocus();
    if (dir !== t.dir) return cd(t, dir, true, name);
    const i = await invoke("tab_find", { tab: t.id, name }).catch(() => null);
    if (i != null) t.cursor = i;
  }
</script>

{#snippet entry(e, cls, onclick, ondblclick, drag)}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="row {cls}" class:dir={e.is_dir} class:hidden={e.hidden} class:drop={e.is_dir && ui.dropDir === e.path} data-drop={e.is_dir ? e.path : undefined} draggable={!!drag} ondragstart={drag && ((ev) => (ev.preventDefault(), drag()))} role="option" tabindex="-1" aria-selected={cls.includes("cursor")} {onclick} {ondblclick} title={ui.facts[e.path]?.join("\n")} onmouseenter={e.is_dir ? () => gitFacts(e.path) : undefined}>
    <span class="icon" style:color={e.icon.color || null}>{e.icon.glyph}</span>
    <span class="label" dir="auto" title={e.origin ? tr("bin.from", { folder: e.origin }) : null}>{t.models?.[e.name] ?? e.name}</span>
    {#if e.tag}<span class="tag" style:background={TAG_COLORS[e.tag]}></span>{/if}
    {#if e.online}<span class="cloud" title={tr("details.online")}>{ui.cfg.glyphs.cloud}</span>{/if}
    {#if e.is_dir}<span class="chev flip">{"\u{f054}"}</span>{/if}
  </div>
{/snippet}

<div class="strip" bind:this={strip}>
  {#each ancestors as col (col.dir)}
    <Rows class="col" items={col.items} {rowH} cursor={col.cursor} role="listbox" aria-label={col.dir}>
      {#snippet row(e)}
        {@render entry(e, e.path === col.open ? "open" : "", () => jump(col.dir, e.name), () => jump(col.dir, e.name).then(() => openItem(t)))}
      {/snippet}
    </Rows>
  {/each}
  <Rows class="col current {active ? 'active' : ''}" items={shown} {rowH} cursor={t.cursor - skip} role="listbox" aria-label={t.dir}>
    {#snippet row(e, j)}
      {@const i = j + skip}
      {@render entry(
        e,
        `${i === t.cursor ? "cursor" : ""} ${e.m ? "marked" : ""}`,
        (ev) => {
          onfocus();
          t.cursor = i;
          if (ev.ctrlKey || ev.metaKey) toggleMark(t, i);
        },
        () => openItem(t, i),
        () => dragOut(t, i),
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
  .cloud,
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
  /* A name goes the way its first letter does (dir="auto"), aligned as the column is. */
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: match-parent;
    text-align: match-parent;
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

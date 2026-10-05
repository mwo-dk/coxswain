<script>
  // Thumbnails: images show themselves, everything else its icon. Arrow keys move in 2D
  // (handled in App, which reads the column count from `data-cols`).
  import { untrack } from "svelte";
  import { openItem, toggleMark, dragOut } from "./app.svelte.js";
  import { convertFileSrc, previewKind, TAG_COLORS } from "./lib.js";
  import { ui } from "./app.svelte.js";
  import { t as tr } from "./i18n.svelte.js";

  /** @type {{ t: any, active: boolean, onfocus: Function }} */
  let { t, active, onfocus } = $props();
  let grid = $state();
  let tiles = $state();
  let cols = $state(1);
  // Only the rows of tiles in view are in the DOM (a folder of 10,000 photos costs what a
  // screenful does); the rest is padding of the right height, on an inner box (padding on
  // the scrolling one would make it that tall). Tiles are all one height, measured from the
  // first one, since it follows the pane's width.
  let top = $state(0);
  let height = $state(0);
  let tileH = $state(120);
  /** The tiles' gap and padding, as in the style below; rows rendered beyond the edges. */
  const GAP = 6, PAD = 8, OVER = 2;
  const step = $derived(tileH + GAP);
  const rows = $derived(Math.ceil(t.items.length / cols));
  const first = $derived(Math.max(0, Math.min(rows, Math.floor((top - PAD) / step)) - OVER));
  const last = $derived(Math.min(rows, Math.ceil((top + height) / step) + OVER));

  function measure() {
    cols = getComputedStyle(tiles).gridTemplateColumns.split(" ").length;
    height = grid.clientHeight;
    tileH = tiles.firstElementChild?.offsetHeight || untrack(() => tileH);
  }

  // ponytail: the webview decodes full-size images; a thumbnail cache if big photo folders get slow.
  $effect(() => {
    if (!grid || !tiles) return;
    untrack(measure);
    const ro = new ResizeObserver(measure);
    ro.observe(grid);
    return () => ro.disconnect();
  });
  // The first tiles of a folder are the ones to measure (an empty folder had none).
  $effect(() => {
    if (tiles && t.items.length) untrack(measure);
  });

  // The cursor kept in view, as scrollIntoView would.
  $effect(() => {
    if (!grid || !height || t.cursor < 0) return;
    t.items; // a new list brings its cursor into view too
    const y = PAD + Math.floor(t.cursor / cols) * step;
    if (y < grid.scrollTop) grid.scrollTop = y - PAD;
    else if (y + tileH > grid.scrollTop + height) grid.scrollTop = y + tileH + PAD - height;
    top = grid.scrollTop;
  });

  function click(ev, i) {
    onfocus();
    t.cursor = i;
    if (ev.ctrlKey || ev.metaKey) toggleMark(t, i);
  }
</script>

<div
  class="grid"
  bind:this={grid}
  data-cols={cols}
  role="listbox"
  tabindex="-1"
  aria-label={t.dir}
  onscroll={(ev) => (top = ev.currentTarget.scrollTop)}
>
  <div class="tiles" bind:this={tiles} style:padding-top="{PAD + first * step}px" style:padding-bottom="{PAD + (rows - last) * step}px">
    {#each t.items.slice(first * cols, last * cols) as e, j (e.name === ".." ? "\0.." : e.path)}
      {@const i = first * cols + j}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        role="option"
        tabindex="-1"
        aria-selected={i === t.cursor}
        class="tile"
        class:hidden={e.hidden}
        class:marked={t.marked.has(e.path)}
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
        title={e.name}
      >
        <div class="thumb">
          <!-- A picture only in the cloud is not drawn: that would download it. -->
          {#if previewKind(e) === "image" && !e.online}
            <img src={convertFileSrc(e.path)} alt="" loading="lazy" decoding="async" draggable="false" />
          {:else}
            <span class="icon" class:dir={e.is_dir} style:color={e.icon.color || null}>{e.name === ".." ? "\u{f062}" : e.icon.glyph}</span>
          {/if}
        </div>
        <span class="label">
          {#if e.tag}<span class="tag" style:background={TAG_COLORS[e.tag]}></span>{/if}{#if e.online}<span class="cloud" title={tr("details.online")}>{ui.cfg.glyphs.cloud}</span> {/if}{e.name}
        </span>
      </div>
    {/each}
  </div>
</div>

<style>
  .grid {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    outline: none;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(112px, 1fr));
    grid-auto-rows: max-content;
    gap: 6px;
    padding: 8px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 6px;
    border-radius: var(--r);
    cursor: default;
    user-select: none;
    min-width: 0;
  }
  .tile:hover {
    background: color-mix(in srgb, var(--cursor-bg) 40%, transparent);
  }
  .thumb {
    display: grid;
    place-items: center;
    width: 100%;
    aspect-ratio: 4 / 3;
    overflow: hidden;
    border-radius: var(--r);
  }
  .thumb img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }
  .icon {
    font-family: var(--icon-font);
    font-size: 3em;
  }
  .cloud {
    font-family: var(--icon-font);
  }
  .icon.dir {
    color: var(--directory-fg);
  }
  .label {
    width: 100%;
    text-align: center;
    unicode-bidi: plaintext;
    font-size: 0.9em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin-inline-end: 4px;
  }
  .hidden {
    opacity: 0.6;
  }
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
</style>

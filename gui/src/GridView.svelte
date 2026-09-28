<script>
  // Thumbnails: images show themselves, everything else its icon. Arrow keys move in 2D
  // (handled in App, which reads the column count from `data-cols`).
  import { openItem, toggleMark, dragOut } from "./app.svelte.js";
  import { convertFileSrc, previewKind, TAG_COLORS } from "./lib.js";

  /** @type {{ t: any, active: boolean, onfocus: Function }} */
  let { t, active, onfocus } = $props();
  let grid = $state();
  let cols = $state(1);

  $effect(() => {
    grid?.children[t.cursor]?.scrollIntoView({ block: "nearest" });
  });

  // ponytail: the webview decodes full-size images; a thumbnail cache if big photo folders get slow.
  $effect(() => {
    if (!grid) return;
    const ro = new ResizeObserver(() => (cols = getComputedStyle(grid).gridTemplateColumns.split(" ").length));
    ro.observe(grid);
    return () => ro.disconnect();
  });

  function click(ev, i) {
    onfocus();
    t.cursor = i;
    if (ev.ctrlKey || ev.metaKey) toggleMark(t, i);
  }
</script>

<div class="grid" bind:this={grid} data-cols={cols} role="listbox" tabindex="-1" aria-label={t.dir}>
  {#each t.items as e, i (e.path)}
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
        {#if previewKind(e) === "image"}
          <img src={convertFileSrc(e.path)} alt="" loading="lazy" decoding="async" draggable="false" />
        {:else}
          <span class="icon" class:dir={e.is_dir} style:color={e.icon.color || null}>{e.name === ".." ? "\u{f062}" : e.icon.glyph}</span>
        {/if}
      </div>
      <span class="label">
        {#if e.tag}<span class="tag" style:background={TAG_COLORS[e.tag]}></span>{/if}{e.name}
      </span>
    </div>
  {/each}
</div>

<style>
  .grid {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(112px, 1fr));
    grid-auto-rows: max-content;
    gap: 6px;
    padding: 8px;
    outline: none;
  }
  .tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 6px;
    border-radius: 8px;
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
    border-radius: 6px;
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
  .icon.dir {
    color: var(--directory-fg);
  }
  .label {
    width: 100%;
    text-align: center;
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

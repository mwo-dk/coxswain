<script>
  import { ui, tab, cd } from "./app.svelte.js";
  import { invoke, basename, size } from "./lib.js";

  let collapsed = $state({});
  const here = $derived(tab()?.dir);

  const go = (path) => {
    const t = tab();
    if (t) cd(t, path);
  };

  const saveFavorites = () => invoke("save_favorites", { favorites: $state.snapshot(ui.favorites) });

  function addHere(g) {
    if (here && !g.paths.includes(here)) {
      g.paths.push(here);
      saveFavorites();
    }
  }

  function remove(g, p) {
    g.paths = g.paths.filter((x) => x !== p);
    saveFavorites();
  }

  function addGroup() {
    ui.modal = {
      kind: "input",
      title: "New favorites group",
      label: "Name:",
      value: "",
      run: (name) => {
        if (!name.trim()) return;
        ui.favorites.push({ name: name.trim(), paths: [] });
        saveFavorites();
      },
    };
  }

  function groupMenu(e, g, i) {
    e.preventDefault();
    ui.modal = {
      kind: "menu",
      title: g.name,
      direct: false,
      filter: "",
      cursor: 0,
      items: [
        { key: "", label: "Add current folder", run: () => addHere(g) },
        {
          key: "",
          label: "Rename group",
          run: () =>
            (ui.modal = {
              kind: "input",
              title: "Rename group",
              label: "Name:",
              value: g.name,
              run: (n) => {
                if (n.trim()) g.name = n.trim();
                saveFavorites();
              },
            }),
        },
        {
          key: "",
          label: "Delete group",
          run: () => {
            ui.favorites.splice(i, 1);
            saveFavorites();
          },
        },
      ],
    };
  }
</script>

{#snippet section(id, title, extra)}
  <button class="head" onclick={() => (collapsed[id] = !collapsed[id])} aria-expanded={!collapsed[id]}>
    <span class="chev" class:closed={collapsed[id]}>{"\u{f078}"}</span>
    <span class="title">{title}</span>
    {@render extra?.()}
  </button>
{/snippet}

{#snippet row(path, label, icon, color, trailing)}
  <li>
    <button class="row" class:here={path === here} title={path} onclick={() => go(path)}>
      <span class="icon" style:color>{icon}</span>
      <span class="label">{label}</span>
      {@render trailing?.()}
    </button>
  </li>
{/snippet}

<nav class="sidebar" aria-label="Places">
  {@render section("places", "Places")}
  {#if !collapsed.places}
    <ul>
      {#each ui.places as p (p.path)}
        {@render row(p.path, p.name, p.icon, "var(--directory-fg)")}
      {/each}
    </ul>
  {/if}

  {@render section("drives", "Drives")}
  {#if !collapsed.drives}
    <ul>
      {#each ui.disks as d (d.mount)}
        {@const used = 1 - d.free / d.total}
        <li>
          <button class="row drive" class:here={d.mount === here} title="{d.device} · {d.mount}" onclick={() => go(d.mount)}>
            <span class="icon">{d.removable ? "\u{f0287}" : "\u{f02ca}"}</span>
            <span class="label">
              <span class="drive-name"><span>{d.label}</span><small>{size(d.free)} free</small></span>
              <span class="bar"><span style:width="{used * 100}%" class:full={used > 0.9}></span></span>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  {#each ui.favorites as g, gi (gi)}
    {#snippet add()}
      <span class="add" role="button" tabindex="-1" title="Add current folder" onclick={(e) => { e.stopPropagation(); addHere(g); }} onkeydown={() => {}}>+</span>
    {/snippet}
    <div oncontextmenu={(e) => groupMenu(e, g, gi)} role="group">
      {@render section(`fav${gi}`, g.name, add)}
    </div>
    {#if !collapsed[`fav${gi}`]}
      <ul>
        {#each g.paths as p (p)}
          {#snippet x()}
            <span class="remove" role="button" tabindex="-1" title="Remove" onclick={(e) => { e.stopPropagation(); remove(g, p); }} onkeydown={() => {}}>×</span>
          {/snippet}
          {@render row(p, basename(p) || p, "\u{f005}", "var(--marked-fg)", x)}
        {:else}
          <li class="empty">Right-click the group or press + to add the current folder</li>
        {/each}
      </ul>
    {/if}
  {/each}
  <button class="new-group" onclick={addGroup}>+ New group</button>

  {#if ui.recent.length}
    {@render section("repos", "Git repositories")}
    {#if !collapsed.repos}
      <ul>
        {#each ui.recent as r (r)}
          {@render row(r, basename(r), "\u{e702}", "var(--git-branch-fg)")}
        {/each}
      </ul>
    {/if}
  {/if}
</nav>

<style>
  .sidebar {
    height: 100%;
    overflow-y: auto;
    background: var(--sidebar-bg);
    color: var(--sidebar-fg);
    padding: 6px 6px 12px;
    box-sizing: border-box;
    scrollbar-width: thin;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    cursor: pointer;
    text-align: left;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 10px 6px 4px;
    font-size: 0.78em;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--hidden-fg);
  }
  .head .title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chev {
    font-family: var(--icon-font);
    font-size: 0.8em;
    transition: transform 0.15s;
  }
  .chev.closed {
    transform: rotate(-90deg);
  }
  .add,
  .remove {
    visibility: hidden;
    padding: 0 6px;
    border-radius: 4px;
    font-size: 1.2em;
    line-height: 1;
  }
  .head:hover .add,
  .row:hover .remove {
    visibility: visible;
  }
  .add:hover,
  .remove:hover {
    background: var(--cursor-bg);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: var(--row);
    padding: 2px 8px;
    border-radius: 6px;
  }
  .row:hover {
    background: color-mix(in srgb, var(--cursor-bg) 55%, transparent);
  }
  .row.here {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .icon {
    font-family: var(--icon-font);
    width: 1.2em;
    text-align: center;
    flex: none;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .drive .label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 3px 0;
  }
  .drive-name {
    display: flex;
    justify-content: space-between;
    gap: 6px;
  }
  .drive-name small {
    color: var(--hidden-fg);
  }
  .bar {
    height: 3px;
    border-radius: 2px;
    background: var(--border-fg);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent-bg);
  }
  .bar span.full {
    background: var(--git-deleted-fg);
  }
  .empty {
    padding: 2px 10px;
    font-size: 0.85em;
    color: var(--hidden-fg);
  }
  .new-group {
    margin: 8px 6px 0;
    font-size: 0.85em;
    color: var(--hidden-fg);
  }
  .new-group:hover {
    color: var(--sidebar-fg);
  }
</style>

<script>
  // Find duplicate files and folders (the engine is coxswain-core's dupes.rs): pick where to look,
  // scan, review the groups, mark the copies to remove, move them to the trash.
  import { ui, tab, otherTab, cd } from "./app.svelte.js";
  import { invoke, convertFileSrc, size, date, basename, parent, previewKind } from "./lib.js";

  const d = ui.modal;

  // ------------------------------------------------------------ where and how

  const home = ui.cfg.home;
  /** Candidate roots: both panes, favorites, then the drives. Checked ones get scanned. */
  const candidates = [...new Set([tab().dir, otherTab().dir, ...ui.favorites.flatMap((g) => g.paths), ...ui.disks.map((x) => x.mount)])];
  d.roots ??= Object.fromEntries(candidates.map((p) => [p, p === tab().dir]));
  d.minSize ??= 1024;
  d.hidden ??= false;
  d.folders ??= true;
  let extra = $state("");

  const SIZES = [
    [1, "any size"],
    [1024, "1 KB"],
    [100 * 1024, "100 KB"],
    [1024 * 1024, "1 MB"],
    [10 * 1024 * 1024, "10 MB"],
  ];

  async function addRoot() {
    const p = await invoke("resolve_path", { base: tab().dir, input: extra });
    if (p) d.roots[p] = true;
    extra = "";
  }

  // ------------------------------------------------------------ scanning

  const PHASES = ["Looking at files", "Comparing the first 16 KB", "Comparing whole files", "Done"];

  async function scan() {
    const roots = Object.keys(d.roots).filter((p) => d.roots[p]);
    if (!roots.length) return;
    Object.assign(d, { scanning: true, report: null, error: "", progress: null, marked: {} });
    const started = Date.now();
    const timer = setInterval(async () => (d.progress = await invoke("dupes_progress")), 200);
    try {
      d.report = await invoke("dupes_scan", { options: { roots, min_size: d.minSize, hidden: d.hidden, folders: d.folders } });
      d.seconds = (Date.now() - started) / 1000;
    } catch (e) {
      d.error = String(e);
    } finally {
      clearInterval(timer);
      d.scanning = false;
    }
  }

  if (d.autostart) {
    d.autostart = false;
    scan();
  }

  // ------------------------------------------------------------ marking

  /** Every group as { key, size, items: [{ path, modified?, folder? }] }; folders first. */
  const groups = $derived(
    d.report
      ? [
          ...d.report.folders.map((g, i) => ({ key: `d${i}`, size: g.size, files: g.files, wasted: g.wasted, folder: true, items: g.paths.map((path) => ({ path })) })),
          ...d.report.groups.map((g, i) => ({ key: `f${i}`, size: g.size, wasted: g.wasted, items: g.files })),
        ]
      : [],
  );
  const marked = $derived(Object.keys(d.marked ?? {}).filter((p) => d.marked[p]));
  const freed = $derived(groups.reduce((a, g) => a + g.items.filter((it) => d.marked?.[it.path]).length * g.size, 0));

  /** Mark or unmark one copy; the last unmarked copy of a group cannot be marked. */
  function toggle(g, path) {
    if (!d.marked[path] && g.items.filter((it) => !d.marked[it.path]).length <= 1) {
      d.error = "Every group keeps at least one copy.";
      return;
    }
    d.error = "";
    d.marked[path] = !d.marked[path];
  }

  /** Mark all copies but the one `keep` picks, in every group. */
  function rule(keep) {
    const m = {};
    for (const g of groups) {
      const k = keep(g);
      if (!k) continue;
      for (const it of g.items) if (it !== k) m[it.path] = true;
    }
    d.marked = m;
  }
  const newest = (g) => (g.folder ? null : g.items.reduce((a, b) => (b.modified > a.modified ? b : a)));
  const oldest = (g) => (g.folder ? null : g.items.reduce((a, b) => (b.modified < a.modified ? b : a)));
  let keepUnder = $state("");
  const under = (g) => g.items.find((it) => keepUnder && it.path.startsWith(keepUnder));

  async function trash() {
    const paths = marked;
    ui.modal = {
      kind: "confirm",
      title: "Move duplicates to the trash",
      text: `Move ${paths.length} ${paths.length === 1 ? "copy" : "copies"} (${size(freed)}) to the trash? At least one copy of each stays.`,
      ok: "Move to trash",
      run: async () => {
        try {
          await invoke("delete", { paths, forever: false });
          const gone = new Set(paths);
          d.report.folders = d.report.folders.map((g) => ({ ...g, paths: g.paths.filter((p) => !gone.has(p)) })).filter((g) => g.paths.length > 1);
          d.report.groups = d.report.groups.map((g) => ({ ...g, files: g.files.filter((f) => !gone.has(f.path)) })).filter((g) => g.files.length > 1);
          d.marked = {};
          ui.status = `Moved ${paths.length} duplicate${paths.length === 1 ? "" : "s"} to the trash`;
        } catch (e) {
          d.error = String(e);
        }
        ui.modal = d; // back to the results
      },
    };
  }

  function show(path) {
    ui.modal = null;
    const t = tab();
    cd(t, parent(path)).then(() => {
      const i = t.items.findIndex((e) => e.path === path);
      if (i >= 0) t.cursor = i;
    });
  }

  const close = () => {
    if (d.scanning) invoke("dupes_cancel");
    ui.modal = null;
  };
  const isImage = (p) => previewKind({ name: basename(p) }) === "image";
</script>

<div class="dupes" role="dialog" aria-modal="true" aria-label="Duplicates">
  <header>
    <h2>{"\u{f0c5}"} Duplicates</h2>
    <button class="x" title="Close (Esc)" onclick={close}>×</button>
  </header>

  <section class="setup">
    <div class="roots">
      <span class="label">Look in</span>
      {#each Object.keys(d.roots) as p (p)}
        <label class="chip" class:on={d.roots[p]} title={p}>
          <input type="checkbox" bind:checked={d.roots[p]} disabled={d.scanning} />
          {p === home ? "~" : p.startsWith(home + "/") ? "~/" + p.slice(home.length + 1) : p}
        </label>
      {/each}
      <input class="add" bind:value={extra} placeholder="Add a folder…" spellcheck="false" onkeydown={(e) => e.key === "Enter" && (e.stopPropagation(), addRoot())} />
    </div>
    <div class="opts">
      <label>Ignore files under
        <select bind:value={d.minSize} disabled={d.scanning}>{#each SIZES as [v, l] (v)}<option value={v}>{l}</option>{/each}</select>
      </label>
      <label><input type="checkbox" bind:checked={d.hidden} disabled={d.scanning} /> Include hidden files</label>
      <label><input type="checkbox" bind:checked={d.folders} disabled={d.scanning} /> Find duplicate folders</label>
      {#if d.scanning}
        <button class="primary" onclick={() => invoke("dupes_cancel")}>Cancel</button>
      {:else}
        <button class="primary" disabled={!Object.values(d.roots).some(Boolean)} onclick={scan}>{d.report ? "Scan again" : "Scan"}</button>
      {/if}
    </div>
  </section>

  {#if d.scanning}
    {@const p = d.progress}
    <section class="progress">
      <p>{PHASES[p?.phase ?? 0]}{p?.phase ? ` · ${p.done.toLocaleString()} of ${p.total.toLocaleString()}` : ""} · {(p?.files ?? 0).toLocaleString()} files found · {size(p?.bytes ?? 0)} read</p>
      <div class="bar"><div style:width="{p?.phase && p.total ? (100 * p.done) / p.total : 0}%"></div></div>
    </section>
  {:else if d.report}
    <section class="summary">
      <p>
        <b>{groups.length.toLocaleString()} groups</b> of duplicates · <b>{size(d.report.wasted)}</b> could be freed ·
        {d.report.scanned_files.toLocaleString()} files ({size(d.report.scanned_bytes)}) scanned in {d.seconds?.toFixed(1)} s, {size(d.report.hashed_bytes)} read
      </p>
      <div class="rules">
        <span class="label">Mark all but</span>
        <button onclick={() => rule(newest)}>the newest</button>
        <button onclick={() => rule(oldest)}>the oldest</button>
        <button disabled={!keepUnder} onclick={() => rule(under)}>the one under</button>
        <select bind:value={keepUnder}>
          <option value="">choose a folder…</option>
          {#each Object.keys(d.roots).filter((p) => d.roots[p]) as p (p)}<option value={p}>{p}</option>{/each}
        </select>
        <button onclick={() => (d.marked = {})}>Clear marks</button>
      </div>
    </section>
    <ul class="groups">
      {#each groups.slice(0, 500) as g (g.key)}
        <li class="group">
          <div class="ghead">
            <span class="glyph">{g.folder ? "\u{f07b}" : "\u{f0c5}"}</span>
            <b>{g.folder ? `${g.items.length} identical folders` : `${g.items.length} copies`}</b>
            <span>{size(g.size)} each{g.folder ? `, ${g.files} files` : ""}</span>
            <span class="waste">{size(g.wasted)} extra</span>
          </div>
          {#each g.items as it (it.path)}
            <div class="copy" class:marked={d.marked[it.path]}>
              <input type="checkbox" checked={!!d.marked[it.path]} onchange={() => toggle(g, it.path)} title="Mark to move to the trash" />
              {#if !g.folder && isImage(it.path)}<img src={convertFileSrc(it.path)} alt="" loading="lazy" />{:else}<span class="glyph">{g.folder ? "\u{f07b}" : "\u{f15b}"}</span>{/if}
              <span class="path" title={it.path}><bdi><span class="dir">{parent(it.path)}/</span>{basename(it.path)}</bdi></span>
              {#if it.modified}<span class="when">{date(it.modified)}</span>{/if}
              <button class="show" title="Show in the pane" onclick={() => show(it.path)}>Show</button>
            </div>
          {/each}
        </li>
      {:else}
        <li class="none">No duplicates found.</li>
      {/each}
      {#if groups.length > 500}<li class="none">Showing the 500 groups that waste the most space.</li>{/if}
    </ul>
  {/if}

  <footer>
    {#if d.error}<span class="err">{d.error}</span>{/if}
    <span class="grow">{marked.length ? `${marked.length} marked · ${size(freed)} to free` : ""}</span>
    <button class="primary danger" disabled={!marked.length} onclick={trash}>Move marked to trash</button>
  </footer>
</div>

<style>
  .dupes {
    position: fixed;
    inset: 4vh 4vw;
    z-index: 11;
    display: flex;
    flex-direction: column;
    background: var(--dialog-bg);
    color: var(--dialog-fg);
    border: 1px solid var(--border-fg);
    border-radius: 12px;
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.45);
    overflow: hidden;
  }
  header,
  footer {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
  }
  header {
    border-bottom: 1px solid var(--border-fg);
  }
  footer {
    border-top: 1px solid var(--border-fg);
  }
  h2 {
    margin: 0;
    flex: 1;
    font-size: 1.05em;
    font-family: var(--icon-font), var(--font);
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 1px solid var(--border-fg);
    border-radius: 7px;
    padding: 4px 12px;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .x {
    border: 0;
    font-size: 1.3em;
    padding: 0 6px;
  }
  .primary {
    background: var(--accent-bg);
    color: var(--accent-fg);
    border-color: transparent;
  }
  .danger {
    background: var(--git-deleted-fg);
  }
  .setup,
  .summary,
  .progress {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-fg);
  }
  .roots,
  .opts,
  .rules {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .label {
    color: var(--hidden-fg);
    font-size: 0.9em;
  }
  .chip {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 10px 2px 6px;
    border: 1px solid var(--border-fg);
    border-radius: 999px;
    font-size: 0.9em;
    max-width: 22em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip.on {
    border-color: var(--accent-bg);
  }
  .add,
  select {
    font: inherit;
    font-size: 0.9em;
    color: var(--dialog-input-fg);
    background: var(--dialog-input-bg);
    border: 1px solid var(--border-fg);
    border-radius: 7px;
    padding: 3px 8px;
  }
  .opts label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.9em;
  }
  .opts .primary {
    margin-left: auto;
  }
  p {
    margin: 0;
  }
  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--dialog-input-bg);
    overflow: hidden;
  }
  .bar div {
    height: 100%;
    background: var(--accent-bg);
    transition: width 0.2s;
  }
  .groups {
    flex: 1;
    overflow: auto;
    list-style: none;
    margin: 0;
    padding: 8px 16px;
  }
  .group {
    margin-bottom: 10px;
    border: 1px solid var(--border-fg);
    border-radius: 8px;
    overflow: hidden;
  }
  .ghead {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px;
    background: color-mix(in srgb, var(--dialog-input-bg) 70%, transparent);
    font-size: 0.9em;
  }
  .waste {
    margin-left: auto;
    color: var(--git-modified-fg);
  }
  .copy {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 10px;
    border-top: 1px solid color-mix(in srgb, var(--border-fg) 50%, transparent);
  }
  .copy.marked .path {
    text-decoration: line-through;
    color: var(--git-deleted-fg);
  }
  .copy img {
    width: 40px;
    height: 30px;
    object-fit: cover;
    border-radius: 4px;
    flex: none;
  }
  .glyph {
    font-family: var(--icon-font);
    color: var(--directory-fg);
    width: 40px;
    text-align: center;
    flex: none;
  }
  .ghead .glyph {
    width: auto;
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  .dir {
    color: var(--hidden-fg);
  }
  .when {
    color: var(--hidden-fg);
    font-size: 0.85em;
    font-variant-numeric: tabular-nums;
  }
  .show {
    padding: 1px 8px;
    font-size: 0.85em;
  }
  .none {
    color: var(--hidden-fg);
    padding: 10px 0;
  }
  .grow {
    flex: 1;
    color: var(--hidden-fg);
  }
  .err {
    color: var(--git-deleted-fg);
  }
</style>

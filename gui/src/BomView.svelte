<script>
  // A CycloneDX BOM as a rated tree, with filters and a details box (docs/design/bom-viewer.md).
  // In the preview pane, and full-window (`full`) from its ⤢ button. The Rust side (bom.rs)
  // parses and rates; bom.js filters. Every string from the file is shown as text, never HTML.
  import { ui, tab, otherTab, item, cd, focusPane } from "./app.svelte.js";
  import { invoke, basename, parent, previewKind, composing } from "./lib.js";
  import { t, tn, num } from "./i18n.svelte.js";
  import { STATUSES, KINDS, FAMILIES, COLOR, isCrypto, childrenOf, filtering, mask, visibleRows, PAGE, leafCounts, sunburstArcs, arcPath, pathTo } from "./bom.js";

  const MARK = { added: "＋", worsened: "▲", improved: "▼" };

  let { path, full = false } = $props();

  let view = $state(null);
  let error = $state("");
  let mode = $state(null);
  let selected = $state(0);
  let details = $state(null);
  let open = $state(new Set());
  let shown = $state({});
  const filters = $state({ status: new Set(), kind: new Set(), family: new Set(), change: new Set(), query: "", hide: false });
  /** A compare with an older version: { old, mode, diff } or { old, error }. */
  let compare = $state(null);
  let showRemoved = $state(false);
  let treeEl = $state();
  let searchEl = $state();
  /** Tree or sunburst; sticks, as the preview's other switches do (ui.previewSource). */
  const shape = $derived(ui.previewSource === "sunburst" ? "sunburst" : "tree");
  /** The row in the middle of the sunburst. */
  let zoom = $state(0);

  // Read (or take from the cache) when the file or the grouping changes.
  $effect(() => {
    const p = path;
    const m = mode;
    error = "";
    const timer = setTimeout(async () => {
      try {
        const v = await invoke("bom_info", { path: p, mode: m });
        if (p !== path) return;
        // A compare belongs to the BOM it was made for.
        if (compare && compare.path !== p) stopCompare();
        view = v;
        // The first two levels open, unless that makes a long list.
        const kids = childrenOf(v.rows, v.order);
        const second = kids[0].reduce((n, c) => n + Math.min(kids[c].length, PAGE), kids[0].length);
        open = new Set(second <= 1000 ? [0, ...kids[0]] : [0]);
        shown = {};
        zoom = 0;
        if (selected >= v.rows.length) selected = 0;
      } catch (err) {
        if (p === path) error = String(err);
      }
    }, 60);
    return () => clearTimeout(timer);
  });

  const rows = $derived(view?.rows ?? []);
  const kids = $derived(view ? childrenOf(view.rows, view.order) : []);

  /** A row's name: a key named by a UUID by what it is, a group by kind in the user's language. */
  function label(i) {
    const r = rows[i];
    if (!r) return "";
    if (r.key) {
      const key = `bom.material.${r.key[0]}`;
      const type = t(key) === key ? t("bom.material.key") : t(key);
      return t("bom.key_of", { type, algorithm: r.key[1] });
    }
    if (r.gk) return t(`bom.kind.${r.gk}`);
    return r.l;
  }

  const active = $derived(filtering(filters));
  const changes = $derived(compare?.diff?.change ?? null);
  const masked = $derived(view ? mask(rows, view.order, filters, label, changes) : { match: [], keep: [] });
  const hiding = $derived(active && filters.hide);
  const isOpen = (i) => open.has(i) || (hiding && i !== 0 && masked.keep[i] && kids[i].some((c) => masked.keep[c]));
  const visible = $derived(view ? visibleRows(rows, kids, isOpen, shown, masked.keep, hiding) : []);
  const cursorAt = $derived(Math.max(0, visible.findIndex((v) => v.i === selected)));

  // The details of the selected row.
  $effect(() => {
    const p = path;
    const i = selected;
    const m = view?.mode;
    if (!view) return;
    const timer = setTimeout(async () => {
      const d = await invoke("bom_node", { path: p, mode: m, index: i }).catch((err) => ({ error: String(err) }));
      if (p === path && i === selected) details = d;
    }, 60);
    return () => clearTimeout(timer);
  });

  // Keep the selected row in sight.
  $effect(() => {
    const i = selected;
    treeEl?.querySelector(`[data-i="${i}"]`)?.scrollIntoView({ block: "nearest" });
  });

  function toggle(set, v) {
    set.has(v) ? set.delete(v) : set.add(v);
    filters.status = new Set(filters.status);
    filters.kind = new Set(filters.kind);
    filters.family = new Set(filters.family);
    filters.change = new Set(filters.change);
  }

  function clearFilters() {
    Object.assign(filters, { status: new Set(), kind: new Set(), family: new Set(), change: new Set(), query: "" });
  }

  // ------------------------------------------------------------ compare

  /** The file under the cursor in the other pane, when it could be an older version of this BOM. */
  const other = $derived.by(() => {
    const e = ui.dual ? item(otherTab()) : null;
    if (!e || e.is_dir || e.path === path) return null;
    return previewKind(e) === "bom" || /\.(json|xml)$/i.test(e.name) ? e : null;
  });

  async function runCompare(old = other?.path) {
    if (!old || !view) return;
    const mode = view.mode;
    try {
      const diff = await invoke("bom_diff", { old, path, mode });
      if (view?.mode === mode) compare = { old, path, mode, diff };
    } catch (err) {
      compare = { old, path, mode, error: String(err) };
    }
  }

  function stopCompare() {
    compare = null;
    showRemoved = false;
    filters.change = new Set();
  }

  // Another grouping gives other rows: compare again in it.
  $effect(() => {
    if (compare && view && compare.mode !== view.mode && compare.path === path) runCompare(compare.old);
  });

  /** Selects a row and opens what it sits in, so the tree shows it too. */
  function select(i) {
    selected = i;
    const up = pathTo(rows, i).slice(0, -1).filter((p) => !open.has(p));
    if (up.length) open = new Set([...open, ...up]);
  }

  function setOpen(i, on) {
    const next = new Set(open);
    on ? next.add(i) : next.delete(i);
    open = next;
  }

  /** Shows a file in a pane: the other pane from the preview (so the BOM stays), else this one. */
  async function reveal(file) {
    if (!file) return;
    let tb = tab();
    if (full) ui.modal = null;
    else if (ui.dual) {
      tb = otherTab();
      focusPane(ui.activePane ^ 1);
    }
    await cd(tb, parent(file));
    const i = tb.items.findIndex((e) => e.path === file);
    if (i >= 0) tb.cursor = i;
    ui.status = t("status.showing", { name: basename(file) });
  }

  const firstFile = (d) => d?.found_in?.find((f) => f.path)?.path ?? d?.group_path;

  function onkeydown(e) {
    if (!visible.length) return;
    const at = cursorAt;
    const go = (k) => {
      const v = visible[Math.max(0, Math.min(visible.length - 1, k))];
      if (v.i !== undefined) selected = v.i;
    };
    const r = rows[selected];
    switch (e.key) {
      case "ArrowDown":
        go(at + 1);
        break;
      case "ArrowUp":
        go(at - 1);
        break;
      case "PageDown":
        go(at + 15);
        break;
      case "PageUp":
        go(at - 15);
        break;
      case "Home":
        go(0);
        break;
      case "End":
        go(visible.length - 1);
        break;
      case "ArrowRight":
        if (kids[selected]?.length && !isOpen(selected)) setOpen(selected, true);
        else if (isOpen(selected)) go(at + 1);
        break;
      case "ArrowLeft":
        if (isOpen(selected) && selected !== 0) setOpen(selected, false);
        else if (r?.p >= 0) selected = r.p;
        break;
      case "Enter": {
        const v = visible[at];
        if (v?.more !== undefined) showMore(v.more);
        else reveal(firstFile(details));
        break;
      }
      case "/":
        searchEl?.focus();
        break;
      case "Escape":
        if (!full) treeEl.blur();
        return;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  }

  function showMore(i) {
    shown = { ...shown, [i]: (shown[i] ?? PAGE) + PAGE };
  }

  const place = (f) => (f.line ? `${f.location}:${f.line}` : f.location);

  /** One reason, in words. */
  function why(w) {
    const status = w.status ? t(`bom.status.${w.status}`) : "";
    switch (w.kind) {
      case "rule": {
        if (w.missing.length) return t("bom.why.missing", { algorithm: w.algorithm, what: w.missing.map((m) => t(`bom.missing.${m}`)).join(", ") });
        const params = [
          w.key_bits && t("bom.why.key_bits", { bits: w.key_bits }),
          w.security_bits && t("bom.why.security_bits", { bits: w.security_bits }),
          w.param_set && t("bom.why.param_set", { set: w.param_set }),
        ].filter(Boolean);
        const algorithm = params.length ? `${w.algorithm} (${params.join(", ")})` : w.algorithm;
        return t("bom.why.rule", { algorithm, status });
      }
      case "not-rated":
        return w.primitive ? t("bom.why.not_rated_primitive", { primitive: w.primitive }) : t("bom.why.not_rated");
      case "unresolved":
        return t(`bom.why.${w.unresolved}`);
      case "inherited":
        return t("bom.why.inherited", { name: w.node_label, status });
      case "reference":
        return t(["signedWith", "hasKey"].includes(w.edge) ? `bom.why.${w.edge}` : "bom.why.uses", { name: w.node_label, status });
      case "lifecycle":
        return w.days_left < 0 ? tn("bom.why.expired", -w.days_left) : tn("bom.why.expires", w.days_left);
      case "rollup":
        return t("bom.why.rollup", { name: w.node_label, status });
    }
    return "";
  }

  const counted = (map, keys) => keys.filter((k) => map?.[k]).map((k) => [k, map[k]]);

  // ------------------------------------------------------------ sunburst

  const SIZE = 400;
  const C = SIZE / 2;
  const R = C - 4;
  const R0 = R * 0.2;
  const leaves = $derived(view && shape === "sunburst" ? leafCounts(rows, view.order, kids, hiding ? masked.keep : null) : null);
  const arcs = $derived(leaves ? sunburstArcs(rows, kids, leaves, zoom < rows.length ? zoom : 0) : []);
  const ringWidth = $derived((R - R0) / Math.max(1, ...arcs.map((a) => a.depth)));
  const ring = (a) => [R0 + (a.depth - 1) * ringWidth, R0 + a.depth * ringWidth - 0.6];

  function zoomOut() {
    if (zoom > 0) zoom = rows[zoom].p;
  }

  function pickArc(a) {
    const i = a.more ?? a.i;
    select(i);
    if (a.more !== undefined || kids[i].length) zoom = i;
  }

  function onSunKey(e) {
    if (e.key === "Backspace") zoomOut();
    else if (e.key === "Enter" && kids[selected]?.length) zoom = selected;
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

{#snippet dot(status)}
  <span class="dot {COLOR[status]}" class:safe={status === "safe"} title={t(`bom.status.${status}`)}></span>
{/snippet}

<div class="bom" class:full role={full ? "dialog" : undefined} aria-modal={full ? "true" : undefined} aria-label={full ? basename(path) : undefined}>
  {#if full}
    <header class="top">
      <h2>{"\u{f0c9}"} {basename(path)}</h2>
      <div class="modes" role="group">
        <button class:on={shape === "tree"} onclick={() => (ui.previewSource = false)}>{t("preview.tree")}</button>
        <button class:on={shape === "sunburst"} onclick={() => (ui.previewSource = "sunburst")}>{t("bom.sunburst")}</button>
      </div>
      <button class="x" title={t("bom.close")} onclick={() => (ui.modal = null)}>×</button>
    </header>
  {/if}

  {#if error}
    <p class="note">{error}</p>
  {:else if !view}
    <p class="note">{t("bom.reading")}</p>
  {:else}
    <div class="bar">
      <div class="chips">
        {#each counted(view.by_status, STATUSES) as [s, n] (s)}
          <button class="chip" class:on={filters.status.has(s)} onclick={() => toggle(filters.status, s)}>{@render dot(s)}{t(`bom.status.${s}`)} <small>{num(n)}</small></button>
        {/each}
      </div>
      <div class="chips">
        {#each counted(view.by_kind, KINDS) as [k, n] (k)}
          <button class="chip" class:on={filters.kind.has(k)} onclick={() => toggle(filters.kind, k)}>{t(`bom.kind.${k}`)} <small>{num(n)}</small></button>
        {/each}
        {#each counted(view.by_family, FAMILIES) as [f, n] (f)}
          <button class="chip fam" class:on={filters.family.has(f)} onclick={() => toggle(filters.family, f)}>{t(`bom.family.${f}`)} <small>{num(n)}</small></button>
        {/each}
      </div>
      <div class="row">
        <input class="search" bind:this={searchEl} bind:value={filters.query} placeholder={t("bom.search")} spellcheck="false"
          onkeydown={(e) => (e.key === "Enter" || e.key === "ArrowDown") && !composing(e) && (e.preventDefault(), treeEl?.focus())} />
        <div class="modes" role="group">
          <button class:on={!filters.hide} title={t("bom.dim_title")} onclick={() => (filters.hide = false)}>{t("bom.dim")}</button>
          <button class:on={filters.hide} title={t("bom.hide_title")} onclick={() => (filters.hide = true)}>{t("bom.hide")}</button>
        </div>
        {#if view.modes.length > 1}
          <select class="mode" title={t("bom.mode")} value={view.mode} onchange={(e) => (mode = e.currentTarget.value)}>
            {#each view.modes as m (m)}<option value={m}>{t(`bom.mode.${m}`)}</option>{/each}
          </select>
        {/if}
        {#if active}<button class="link" onclick={clearFilters}>{t("bom.clear")}</button>{/if}
        {#if !compare}
          <button class="plain" disabled={!other} title={other ? t("bom.compare_title", { name: other.name }) : t("bom.compare_none")} onclick={() => runCompare()}>⇄ {t("bom.compare")}</button>
        {/if}
        {#if !full}<button class="icon-btn" title={t("bom.window")} onclick={() => (ui.modal = { kind: "bom", path })}>{"\u{f065}"}</button>{/if}
      </div>
    </div>

    {#if compare}
      <div class="chips compare">
        <span class="label">{t("bom.compared", { name: basename(compare.old) === basename(path) ? `${basename(parent(compare.old))}/${basename(compare.old)}` : basename(compare.old) })}</span>
        {#if compare.error}
          <span class="note">{compare.error}</span>
        {:else}
          {@const c = compare.diff.counts}
          <button class="chip risk" class:on={filters.change.has("risk")} disabled={!c.newRisks} onclick={() => toggle(filters.change, "risk")}>{tn("bom.change.risk", c.newRisks)}</button>
          <span class="chip static">{tn("bom.change.fixed", c.fixed)}</span>
          {#each [["added", c.added], ["worsened", c.worsened], ["improved", c.improved]] as [k, n] (k)}
            <button class="chip" class:on={filters.change.has(k)} disabled={!n} onclick={() => toggle(filters.change, k)}><span class="mark {k}">{MARK[k]}</span>{tn(`bom.change.${k}`, n)}</button>
          {/each}
          <button class="chip" class:on={showRemoved} disabled={!c.removed} onclick={() => (showRemoved = !showRemoved)}>− {tn("bom.change.removed", c.removed)}</button>
        {/if}
        <button class="x small" title={t("bom.stop_compare")} onclick={stopCompare}>×</button>
      </div>
      {#if showRemoved && compare.diff?.removed.length}
        <ul class="removed">
          {#each compare.diff.removed.slice(0, 500) as r, k (k)}
            <li>{@render dot(r.status)}<span>{r.label}</span> <small class="word {COLOR[r.status]}">{t(`bom.status.${r.status}`)}</small> <small class="where">{r.where}</small></li>
          {/each}
        </ul>
      {/if}
    {/if}

    <div class="main">
      {#if shape === "sunburst"}
        <div class="sun">
          <nav class="crumbs" aria-label={t("bom.sunburst")}>
            {#each pathTo(rows, zoom) as c, k (c)}{#if k}<span class="sep">›</span>{/if}<button class="link" onclick={() => (zoom = c)}>{label(c)}</button>{/each}
          </nav>
          <!-- Keys: Backspace zooms out, Enter into the selected row. -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
          <svg class="bom-keys" viewBox="0 0 {SIZE} {SIZE}" tabindex="0" role="img" aria-label={label(zoom)} onkeydown={onSunKey}>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <circle class="hub {COLOR[rows[zoom]?.s]}" cx={C} cy={C} r={R0 - 1} onclick={zoomOut}><title>{label(zoom)} · {t(`bom.status.${rows[zoom]?.s}`)}</title></circle>
            {#each arcs as a (a.i ?? `more-${a.more}`)}
              {@const [r0, r1] = ring(a)}
              {@const s = a.more !== undefined ? a.s : rows[a.i].s}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <path d={arcPath(C, C, r0, r1, a.a0, a.a1)} class="arc {COLOR[s] ?? 'muted'}" class:more={a.more !== undefined}
                class:sel={a.i === selected} class:dim={active && !filters.hide && a.i !== undefined && !masked.keep[a.i]} onclick={() => pickArc(a)}>
                <title>{a.more !== undefined ? tn("bom.more", a.n) : `${label(a.i)} · ${t(`bom.status.${s}`)} · ${tn("bom.assets", leaves[a.i])}`}</title>
              </path>
            {/each}
            <text x={C} y={C} class="hub-label">{label(zoom).length > 16 ? label(zoom).slice(0, 15) + "…" : label(zoom)}</text>
          </svg>
          {#if active && !masked.keep[0]}<p class="note">{t("bom.no_match")}</p>{/if}
        </div>
      {:else}
      <div class="tree bom-keys mono" bind:this={treeEl} tabindex="0" role="tree" aria-label={basename(path)} {onkeydown}>
        {#each visible as v (v.i ?? `more-${v.more}`)}
          {#if v.more !== undefined}
            <div class="line more-row" style:padding-inline-start="{v.depth * 1.1 + 1.4}em" role="none">
              <button class="link" onclick={() => showMore(v.more)}>{tn("bom.more", v.n)}</button>
            </div>
          {:else}
            {@const r = rows[v.i]}
            {@const has = kids[v.i].length > 0}
            <div class="line" class:sel={v.i === selected} class:dim={active && !filters.hide && !masked.match[v.i]} data-i={v.i}
              style:padding-inline-start="{v.depth * 1.1}em" role="treeitem" tabindex="-1" aria-selected={v.i === selected} aria-expanded={has ? isOpen(v.i) : undefined}
              onclick={() => ((selected = v.i), treeEl.focus())} ondblclick={() => (has ? setOpen(v.i, !isOpen(v.i)) : reveal(firstFile(details)))} onkeydown={onkeydown}>
              <button class="twist" tabindex="-1" aria-hidden="true" onclick={(e) => (e.stopPropagation(), has && setOpen(v.i, !isOpen(v.i)))}>{has ? (isOpen(v.i) ? "▾" : "▸") : ""}</button>
              {@render dot(r.s)}
              {#if changes && MARK[changes[v.i]]}<span class="mark {changes[v.i]}" title={t(`bom.mark.${changes[v.i]}`)}>{MARK[changes[v.i]]}</span>{/if}
              <span class="name" class:group={r.k === "group" || r.k === "application" || r.k === "component"}>{label(v.i)}</span>
              {#if isCrypto(r)}<span class="word {COLOR[r.s]}">{t(`bom.status.${r.s}`)}</span>{/if}
              {#if r.o}<span class="where">{r.o}</span>{/if}
            </div>
          {/if}
        {/each}
        {#if active && !masked.keep[0]}<p class="note">{t("bom.no_match")}</p>{/if}
      </div>
      {/if}

      <section class="details">
        {#if details?.error}
          <p class="note">{details.error}</p>
        {:else if details}
          <h3>{@render dot(details.status)} <span>{label(selected)}</span> <small>· {t(`bom.status.${details.status}`)}</small></h3>
          {#if details.why.length}
            <div class="label">{t("bom.why")}</div>
            <ul class="why">
              {#each details.why as w, k (k)}
                <li>
                  {#if w.node !== null && w.node !== undefined}<button class="link" onclick={() => select(w.node)}>{why(w)}</button>{:else}{why(w)}{/if}
                  {#if w.source}<small class="src">{t("bom.why.source", { source: w.source })}</small>{/if}
                  {#if w.note}<small class="src">{w.note}</small>{/if}
                </li>
              {/each}
            </ul>
          {/if}
          {#if details.advice.length}
            <div class="label">{t("bom.advice")}</div>
            <ul class="why">{#each details.advice as a, k (k)}<li>{a}</li>{/each}</ul>
          {/if}
          {#if details.found_in.length}
            <div class="label">{t("bom.found_in")}</div>
            <ul class="found">
              {#each details.found_in as f, k (k)}
                <li>
                  {#if f.path}<button class="link mono" title={t("bom.show_file")} onclick={() => reveal(f.path)}>{place(f)}</button>
                  {:else}<span class="mono" title={t("bom.not_next_to_bom")}>{place(f)}</span>{/if}
                  {#if f.context}<small class="mono ctx">{f.context}</small>{/if}
                </li>
              {/each}
            </ul>
          {:else if details.group_path}
            <button class="link mono" onclick={() => reveal(details.group_path)}>{t("bom.show_file")}</button>
          {/if}
          {#if details.raw}
            <details class="raw"><summary>{t("bom.as_in_file")}</summary><pre class="mono">{details.raw}</pre></details>
          {/if}
        {:else}
          <p class="note">{t("bom.select_hint")}</p>
        {/if}
      </section>
    </div>

    <footer>
      {#if !Object.keys(view.by_status).length}<p class="note">{t("bom.no_crypto")}</p>{/if}
      <p class="note">{t(view.reviewed ? "bom.ratings_note_reviewed" : "bom.ratings_note", { profile: view.profile, year: view.year })}</p>
      {#if view.issues.length}
        <details class="issues">
          <summary>{tn("bom.issues", view.issues.length)}</summary>
          <ul>{#each view.issues.slice(0, 200) as is, k (k)}<li class={is.severity}>{is.message}</li>{/each}</ul>
        </details>
      {/if}
    </footer>
  {/if}
</div>

<style>
  /* Ratings keep their meaning in every theme: fixed traffic-light colours that read on dark
     and light backgrounds alike, never the theme's own (a green theme would make unknown green). */
  .bom {
    --st-green: #43a047;
    --st-yellow: #f9a825;
    --st-red: #e53935;
    --st-grey: #9e9e9e;
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    gap: 6px;
    padding: 8px 12px;
  }
  .bom.full {
    position: fixed;
    inset: 4vh 4vw;
    z-index: 11;
    padding: 0 0 8px;
    background: var(--dialog-bg);
    color: var(--dialog-fg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
  }
  .full > :not(.top) {
    margin-inline: 16px;
  }
  .top {
    display: flex;
    align-items: center;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-fg);
  }
  h2 {
    margin: 0;
    flex: 1;
    font-size: 1.05em;
    font-family: var(--icon-font), var(--font), var(--cjk);
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    cursor: pointer;
  }
  .x {
    font-size: 1.3em;
    padding: 0 6px;
  }
  .bar {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .chips,
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    align-items: center;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 0.82em;
    padding: 1px 8px;
    border: 1px solid var(--border-fg);
    border-radius: 999px;
    color: var(--panel-fg);
  }
  .chip small {
    color: var(--hidden-fg);
  }
  .chip.fam {
    border-style: dashed;
  }
  .chip.on {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
    border-color: transparent;
  }
  .chip.on small {
    color: inherit;
  }
  .search {
    flex: 1;
    min-width: 8em;
    font: inherit;
    font-size: 0.9em;
    background: var(--dialog-input-bg);
    color: var(--dialog-input-fg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 3px 8px;
    outline: none;
  }
  .search:focus {
    border-color: var(--accent-bg);
  }
  .modes {
    display: flex;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    overflow: hidden;
  }
  .modes button {
    font-size: 0.82em;
    color: var(--hidden-fg);
    padding: 2px 8px;
  }
  .modes button.on {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .mode {
    font: inherit;
    font-size: 0.82em;
    background: var(--dialog-input-bg);
    color: var(--dialog-input-fg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 2px 4px;
  }
  .icon-btn {
    font-family: var(--icon-font);
    padding: 2px 6px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
  }
  .link {
    color: var(--accent-bg);
    padding: 0;
    text-align: start;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--accent-bg) 40%, transparent);
  }
  .main {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .full .main {
    flex-direction: row;
  }
  .tree {
    flex: 2;
    min-height: 6em;
    overflow: auto;
    outline: none;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 3px 0;
  }
  .tree:focus-visible {
    border-color: var(--accent-bg);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 5px;
    white-space: nowrap;
    padding-inline-end: 8px;
    cursor: default;
    line-height: 1.55;
  }
  .line.sel {
    background: color-mix(in srgb, var(--cursor-bg) 45%, transparent);
  }
  .tree:focus-within .line.sel {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .line.dim {
    opacity: 0.35;
  }
  .twist {
    width: 1.1em;
    flex: none;
    color: var(--hidden-fg);
    padding: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .name.group {
    color: var(--directory-fg);
  }
  .sel .name.group {
    color: inherit;
  }
  .word {
    font-size: 0.85em;
  }
  .where {
    margin-inline-start: auto;
    padding-inline-start: 12px;
    color: var(--hidden-fg);
    font-size: 0.85em;
  }
  .sel .where,
  .tree:focus-within .sel .word {
    color: inherit;
  }
  .dot {
    display: inline-block;
    flex: none;
    width: 0.62em;
    height: 0.62em;
    border-radius: 50%;
    background: var(--st-grey);
  }
  .dot.green {
    background: var(--st-green);
  }
  .dot.yellow {
    background: var(--st-yellow);
  }
  .dot.red {
    background: var(--st-red);
  }
  .dot.muted {
    background: none;
    box-shadow: inset 0 0 0 1.5px var(--st-grey);
  }
  /* quantum-safe: a ring around the green */
  .dot.safe {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--st-green) 45%, transparent);
  }
  .word.green {
    color: var(--st-green);
  }
  .word.yellow {
    color: var(--st-yellow);
  }
  .word.red {
    color: var(--st-red);
  }
  .word.grey {
    color: var(--st-grey);
  }
  .details {
    flex: 1;
    min-height: 0;
    overflow: auto;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 6px 10px;
    font-size: 0.92em;
  }
  .full .details {
    flex: 1;
    max-width: 42%;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 4px;
    font-size: 1em;
    overflow-wrap: anywhere;
  }
  h3 small {
    color: var(--hidden-fg);
    font-weight: normal;
  }
  .label {
    margin-top: 6px;
    color: var(--hidden-fg);
    font-size: 0.85em;
  }
  ul {
    margin: 2px 0;
    padding-inline-start: 1.1em;
  }
  li {
    overflow-wrap: anywhere;
  }
  .src,
  .ctx {
    display: block;
    color: var(--hidden-fg);
  }
  .found {
    list-style: none;
    padding: 0;
  }
  .raw pre {
    white-space: pre-wrap;
    font-size: 0.85em;
    max-height: 18em;
    overflow: auto;
  }
  .raw summary,
  .issues summary {
    cursor: pointer;
    color: var(--hidden-fg);
    font-size: 0.85em;
    margin-top: 6px;
  }
  footer .note {
    margin: 0;
  }
  .issues ul {
    font-size: 0.85em;
    max-height: 10em;
    overflow: auto;
  }
  .issues .warning {
    color: var(--st-yellow);
  }
  .note {
    color: var(--hidden-fg);
    font-size: 0.85em;
  }
  .mono {
    font-family: var(--mono-font), var(--cjk);
    font-size: 0.9em;
  }
  .more-row {
    font-size: 0.85em;
  }
  .plain {
    font-size: 0.82em;
    font-family: var(--icon-font), var(--font), var(--cjk);
    padding: 2px 8px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
  }
  .plain:disabled,
  .chip:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .compare {
    padding: 4px 6px;
    border: 1px dashed var(--accent-bg);
    border-radius: var(--r);
  }
  .compare .label {
    font-size: 0.85em;
    margin-inline-end: 4px;
  }
  .chip.static {
    cursor: default;
  }
  .chip.risk:not(:disabled) {
    border-color: var(--st-red);
  }
  .x.small {
    margin-inline-start: auto;
    font-size: 1.1em;
  }
  .mark {
    flex: none;
    font-size: 0.8em;
    font-weight: 700;
  }
  .mark.added {
    color: var(--accent-bg);
  }
  .mark.worsened {
    color: var(--st-red);
  }
  .mark.improved {
    color: var(--st-green);
  }
  .removed {
    list-style: none;
    margin: 0;
    padding: 4px 8px;
    max-height: 8em;
    overflow: auto;
    font-size: 0.88em;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
  }
  .removed li {
    display: flex;
    align-items: center;
    gap: 5px;
    white-space: nowrap;
  }
  .sun {
    flex: 2;
    min-height: 10em;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .crumbs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    font-size: 0.85em;
  }
  .sep {
    color: var(--hidden-fg);
  }
  svg {
    flex: 1;
    min-height: 0;
    width: 100%;
    outline: none;
  }
  .arc,
  .hub {
    stroke: var(--preview-bg, var(--dialog-bg));
    stroke-width: 0.8;
    cursor: pointer;
    fill: var(--st-grey);
  }
  .arc.green,
  .hub.green {
    fill: var(--st-green);
  }
  .arc.yellow,
  .hub.yellow {
    fill: var(--st-yellow);
  }
  .arc.red,
  .hub.red {
    fill: var(--st-red);
  }
  .arc.muted,
  .hub.muted {
    fill: color-mix(in srgb, var(--st-grey) 35%, transparent);
  }
  .arc.more {
    fill-opacity: 0.55;
  }
  .arc:hover {
    fill-opacity: 0.8;
  }
  .arc.dim {
    opacity: 0.2;
  }
  .arc.sel {
    stroke: var(--accent-bg);
    stroke-width: 2.5;
  }
  .hub-label {
    text-anchor: middle;
    dominant-baseline: middle;
    font-size: 11px;
    fill: var(--dialog-fg, var(--preview-fg));
    pointer-events: none;
    paint-order: stroke;
    stroke: var(--preview-bg, var(--dialog-bg));
    stroke-width: 3px;
  }
</style>

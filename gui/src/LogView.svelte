<script>
  // The preview pane's log view: a log, JSON Lines, or any text too big to read whole, through
  // coxswain-core's `logs::View` (gui/src-tauri/src/logs.rs). Only the rows in view are on the
  // page; a log is coloured by level, its fields can be columns, and it can be filtered by level,
  // by a text or a field's value, its stack traces folded, and followed as it is written. The
  // keys F, e, &, c, z and E work while the view has the focus, as in the terminal app's viewer.
  import { untrack } from "svelte";
  import { invoke } from "./lib.js";
  import { ui } from "./app.svelte.js";
  import { t, tn, num } from "./i18n.svelte.js";

  let { path } = $props();

  /** What `log_open` said: { id, log, format, columns, widths, fold }, or null when it is no text. */
  let info = $state(null);
  let error = $state("");
  /** `log_state`: rows, done, lines, lines_done, capped, filtered, era. */
  let st = $state({ rows: 0, done: false, lines: 0, lines_done: false, capped: false, filtered: false, era: 0 });
  /** The rows on the page: from `from`, as `log_rows` gave them. */
  let rows = $state.raw([]);
  let from = $state(0);
  let era = -1;
  let asked = "";

  // What the controls set.
  let level = $state("");
  let filter = $state("");
  /** The filter as applied (`log_shape` gives it back as it reads it). */
  let applied = $state("");
  let fold = $state(false);
  let columns = $state(true);
  let follow = $state(false);
  let pause = $state(false);
  let said = $state("");
  /** The lines the pause on errors has looked at. */
  let followedTo = 0;
  /** The first time a log is shown here, what the controls do is said. */
  let hint = $state(false);

  let scroller = $state();
  let left = $state(0);
  let filterEl = $state();
  let viewH = $state(400);
  /** A row's height in pixels, measured once the first row is drawn. */
  let rowH = $state(18);
  let top = $state(0);
  /** The scroll position's largest height: past it rows are mapped onto it, as a page cannot be
   *  millions of rows tall. */
  const MAX_H = 4_000_000;

  const total = $derived(st.rows);
  const inView = $derived(Math.ceil(viewH / rowH) + 1);
  const fullH = $derived(Math.min(total * rowH, MAX_H));
  /** The first row in view, from the scroll position. */
  const first = $derived.by(() => {
    if (total * rowH <= MAX_H) return Math.floor(top / rowH);
    const room = Math.max(1, fullH - viewH);
    return Math.round((Math.min(top, room) / room) * Math.max(0, total - inView + 1));
  });
  const hasColumns = $derived(!!info?.columns?.length && columns);
  /** Where the rows in view are drawn: their own place, or (mapped) the scroll position. */
  const offset = $derived(total * rowH <= MAX_H ? first * rowH : top);
  /** The line numbers' width: the lines' count's digits. */
  const noW = $derived(`${Math.max(3, String(st.lines).length)}ch`);

  $effect(() => {
    const p = path;
    info = null;
    error = "";
    rows = [];
    from = 0;
    era = -1;
    asked = "";
    said = "";
    let id = null;
    let gone = false;
    invoke("log_open", { path: p }).then(
      (o) => {
        if (gone) return o && invoke("log_close", { id: o.id });
        if (!o) return void (error = t("logs.no_text"));
        id = o.id;
        info = o;
        fold = o.fold;
        level = "";
        filter = applied = "";
        columns = true;
        follow = untrack(() => ui.followLog === p);
        if (follow) ui.followLog = "";
        if (o.log) invoke("hint_once", { id: "log_view" }).then((b) => (hint = b), () => {});
        refresh();
      },
      (err) => (error = String(err)),
    );
    return () => {
      gone = true;
      if (id) invoke("log_close", { id });
    };
  });

  // The state while rows are being found, or the log is followed.
  $effect(() => {
    if (!info) return;
    const busy = !st.done || follow;
    if (!busy) return;
    const timer = setInterval(refresh, follow ? 300 : 150);
    return () => clearInterval(timer);
  });

  async function refresh() {
    if (!info) return;
    const id = info.id;
    if (follow) {
      const f = await invoke("log_follow", { id, pause, from: followedTo }).catch(() => null);
      if (f?.grew === "anew") followedTo = 0;
      if (f?.error != null) {
        follow = false;
        const [r] = await invoke("log_rows", { id, from: f.error, n: 1, columns: false });
        said = t("viewer.stopped_at_error", { line: num((r?.line ?? f.error) + 1) });
        scrollToRow(Math.max(0, f.error - Math.floor(inView / 3)));
      }
    }
    const s = await invoke("log_state", { id }).catch(() => null);
    if (!s || info?.id !== id) return;
    st = s;
    if (follow) {
      followedTo = s.lines;
      scrollToRow(Math.max(0, s.rows - inView + 2));
    }
  }

  // The rows in view, asked for when the view moves or the rows change.
  $effect(() => {
    if (!info) return;
    const a = Math.max(0, first - 50);
    const n = inView + 100;
    const key = `${st.era}:${a}:${n}:${hasColumns}:${st.rows}`;
    if (key === asked) return;
    // Rows here still cover the view and nothing changed: no need to ask.
    if (st.era === era && from <= first && first + inView <= from + rows.length && asked.endsWith(`${hasColumns}:${st.rows}`)) return;
    asked = key;
    const want = st.era;
    invoke("log_rows", { id: info.id, from: a, n, columns: hasColumns }).then((r) => {
      if (asked !== key) return;
      rows = r;
      from = a;
      era = want;
    }, () => {});
  });

  function scrollToRow(r) {
    if (!scroller) return;
    const room = Math.max(1, fullH - viewH);
    scroller.scrollTop = total * rowH <= MAX_H ? r * rowH : (r / Math.max(1, total - inView + 1)) * room;
  }

  async function reshape(keep = true) {
    if (!info) return;
    // The line at the top stays at the top once the new rows reach it.
    const line = keep ? rows[first - from]?.line : undefined;
    applied = await invoke("log_shape", { id: info.id, level: level || null, filter, fold });
    await refresh();
    if (line == null) return scrollToRow(0);
    for (let i = 0; i < 40; i++) {
      const r = await invoke("log_row_of", { id: info.id, line }).catch(() => 0);
      if (r != null) return scrollToRow(r);
      await new Promise((ok) => setTimeout(ok, 50));
    }
  }

  function setLevel(l) {
    level = l;
    said = t(`viewer.level_${l || "all"}`);
    reshape();
  }
  function nextLevel() {
    setLevel({ "": "warn", warn: "error", error: "" }[level]);
  }
  function setFold(on) {
    fold = on;
    said = t(on ? "viewer.fold_on" : "viewer.fold_off");
    reshape();
  }
  function setFollow(on) {
    follow = on;
    said = t(on ? "viewer.following" : "viewer.not_following");
    if (on) {
      followedTo = st.lines;
      refresh();
    }
  }
  function setPause(on) {
    pause = on;
    said = t(on ? "viewer.pause_on" : "viewer.pause_off");
  }
  function applyFilter() {
    said = filter.trim() ? t("viewer.filtered", { filter: filter.trim() }) : t("viewer.filter_off");
    reshape();
  }
  /** A cell clicked: only the lines where its field has that value. */
  function byField(name, value) {
    filter = `${name}=${value}`;
    applyFilter();
  }

  function onkey(e) {
    if (e.target.closest?.("input, select")) return;
    const k = e.key;
    const act = {
      F: () => setFollow(!follow),
      E: () => setPause(!pause),
      e: () => info?.log && nextLevel(),
      z: () => info?.log && setFold(!fold),
      c: () => info?.columns?.length && (columns = !columns),
      "&": () => filterEl?.focus(),
    }[k];
    if (act && !e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      e.stopPropagation();
      act();
      return;
    }
    // Moving up stops following, as in the terminal app.
    if (follow && ["ArrowUp", "PageUp", "Home"].includes(k)) setFollow(false);
  }

  function onwheel(e) {
    if (follow && e.deltaY < 0) setFollow(false);
  }

  const counted = $derived.by(() => {
    const lines = st.capped ? tn("viewer.first_lines", st.lines) : st.lines_done ? tn("viewer.lines", st.lines) : tn("viewer.lines_so_far", st.lines);
    if (!st.filtered || (st.done && st.rows === st.lines)) return lines;
    return t(st.done ? "viewer.rows_of" : "viewer.rows_so_far", { n: num(st.rows), lines });
  });

  /** The class a row's level colours it with. */
  const lv = (r) => (r.level ? `lv-${r.level}` : "");
  const gridCols = $derived(hasColumns ? `${noW} ${info.widths.map((w) => `${w}ch`).join(" ")} auto` : "");
</script>

{#if error}
  <p class="more">{error}</p>
{:else if info}
  <div class="logview log-keys" style:--no-w={noW}>
    <div class="bar" role="toolbar" aria-label={t("logs.controls")}>
      {#if info.log}
        <select value={level} onchange={(e) => setLevel(e.currentTarget.value)} title={t("logs.level_tip")} aria-label={t("logs.level")}>
          <option value="">{t("viewer.level_all")}</option>
          <option value="warn">{t("viewer.level_warn")}</option>
          <option value="error">{t("viewer.level_error")}</option>
        </select>
      {/if}
      <span class="filter">
        <input bind:this={filterEl} bind:value={filter} placeholder={info.columns.length ? t("logs.filter_fields_placeholder") : t("logs.filter_placeholder")} title={t("logs.filter_tip")} aria-label={t("viewer.filter")}
          onkeydown={(e) => {
            if (e.key === "Enter") (e.preventDefault(), applyFilter(), scroller?.focus());
            else if (e.key === "Escape" && filter) (e.preventDefault(), e.stopPropagation(), (filter = ""), applyFilter());
          }} />
        {#if applied}<button class="clear" title={t("logs.filter_clear")} aria-label={t("logs.filter_clear")} onclick={() => ((filter = ""), applyFilter())}>×</button>{/if}
      </span>
      {#if info.columns.length}
        <button class:on={columns} onclick={() => (columns = !columns)} title={t("logs.columns_tip")}>{t("logs.columns")}</button>
      {/if}
      {#if info.log}
        <button class:on={fold} onclick={() => setFold(!fold)} title={t("logs.fold_tip")}>{t("logs.fold")}</button>
      {/if}
      <button class:on={follow} onclick={() => setFollow(!follow)} title={t("logs.follow_tip")}>{follow ? "● " : ""}{t("logs.follow")}</button>
      <label class="pause" title={t("logs.pause_tip")}><input type="checkbox" checked={pause} onchange={(e) => setPause(e.currentTarget.checked)} />{t("logs.pause")}</label>
    </div>
    <p class="status">
      <span>{info.log ? t(`logs.format.${info.format}`) : t("viewer.plain")} · {counted}</span>
      {#if said}<span class="said">{said}</span>{:else if hint}<span class="said">{t("logs.hint")}</span>{/if}
    </p>
    {#if hasColumns}
      <div class="headwrap mono">
        <div class="head" style:grid-template-columns={gridCols} style:transform={`translateX(${-left}px)`}>
          <span></span>{#each info.columns as c (c)}<span>{c}</span>{/each}<span></span>
        </div>
      </div>
    {/if}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div class="scroller mono" tabindex="0" role="log" aria-label={t("logs.lines")} bind:this={scroller} bind:clientHeight={viewH} onscroll={(e) => ((top = e.currentTarget.scrollTop), (left = e.currentTarget.scrollLeft))} onkeydown={onkey} {onwheel}>
      <div class="row probe" aria-hidden="true" bind:clientHeight={rowH}>0</div>
      <div class="spacer" style:height={`${fullH}px`}>
        <div class="rows" style:transform={`translateY(${offset}px)`}>
          {#each rows.slice(Math.max(0, first - from), Math.max(0, first - from) + inView) as r (r.line)}
            <div class="row {lv(r)}" class:grid={hasColumns && !!r.cells} style:grid-template-columns={hasColumns && r.cells ? gridCols : null}>
              {@render line(r)}
            </div>
          {/each}
        </div>
      </div>
    </div>
  </div>
{:else}
  <p class="more">{t("logs.opening")}</p>
{/if}

{#snippet line(r)}
  <span class="no">{r.line + 1}</span>
  {#if hasColumns && r.cells}
    {#each r.cells as v, i (i)}
      <button class="cell" title={t("logs.cell_tip", { name: info.columns[i], value: v })} onclick={() => byField(info.columns[i], v)}>{v}</button>
    {/each}
    <span class="rest">{#each r.rest as [k, v] (k)}<button class="cell" title={t("logs.cell_tip", { name: k, value: v })} onclick={() => byField(k, v)}>{k}={v}</button>{" "}{/each}{@render folded(r)}</span>
  {:else}
    <span class="text">{r.text}{@render folded(r)}</span>
  {/if}
{/snippet}

{#snippet folded(r)}
  {#if r.folded}<button class="folded" title={t("logs.fold_tip")} onclick={() => setFold(false)}>  ▸ +{tn("viewer.lines", r.folded)}</button>{/if}
{/snippet}

<style>
  .logview {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 12em;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    padding: 4px 0 6px;
  }
  .bar button,
  .bar select,
  .bar input:not([type="checkbox"]) {
    font: inherit;
    font-size: 0.85em;
    color: var(--dialog-input-fg, inherit);
    background: var(--dialog-input-bg, none);
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 3px 8px;
  }
  .bar button {
    color: var(--hidden-fg);
    background: none;
    cursor: pointer;
  }
  .bar button.on {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .pause {
    font-size: 0.85em;
  }
  .filter {
    position: relative;
    flex: 1 1 10em;
    display: flex;
  }
  .filter input {
    flex: 1;
    min-width: 6em;
  }
  .filter .clear {
    position: absolute;
    right: 2px;
    top: 50%;
    transform: translateY(-50%);
    border: 0;
    background: none;
    padding: 0 4px;
  }
  .pause {
    display: flex;
    gap: 4px;
    align-items: center;
    white-space: nowrap;
  }
  .status {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    margin: 0 0 4px;
    opacity: 0.8;
    font-size: 0.9em;
  }
  .status .said {
    color: var(--search-hit-fg);
  }
  .headwrap {
    overflow: hidden;
  }
  .head {
    display: grid;
    gap: 0 1.2ch;
    font-weight: bold;
    opacity: 0.75;
    white-space: pre;
    width: max-content;
  }
  .head span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .scroller {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: auto;
    outline: none;
  }
  .probe {
    position: absolute;
    visibility: hidden;
  }
  .scroller:focus-visible {
    box-shadow: inset 0 0 0 1px var(--cursor-bg);
  }
  .spacer {
    position: relative;
    min-width: max-content;
  }
  .rows {
    position: absolute;
    top: 0;
    left: 0;
    min-width: 100%;
  }
  .row {
    display: flex;
    gap: 1ch;
    white-space: pre;
    line-height: 1.45;
  }
  .row.grid {
    display: grid;
    gap: 0 1.2ch;
  }
  .row .no {
    min-width: var(--no-w, 6ch);
    text-align: right;
    opacity: 0.45;
    user-select: none;
  }
  .cell {
    all: unset;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cell:hover {
    text-decoration: underline;
  }
  .rest {
    opacity: 0.75;
  }
  .folded {
    all: unset;
    cursor: pointer;
    color: var(--hidden-fg);
    font-style: italic;
  }
  .lv-error {
    color: var(--git-deleted-fg);
  }
  .lv-warn {
    color: var(--git-modified-fg);
  }
  .lv-info {
    color: var(--git-added-fg);
  }
  .lv-debug {
    color: var(--hidden-fg);
  }
</style>

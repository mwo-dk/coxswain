<script>
  // Compare files (docs/files/compare-files.md), Shift+F2: two files in a window over the panels.
  // What is compared and how is the core's (compare/): this window asks for the rows in sight
  // and draws them, text side by side or one under the other with its changed words marked,
  // other files as hex rows, pictures side by side, over each other or as their difference.
  import { onDestroy, tick } from "svelte";
  import { ui, reloadAll } from "./app.svelte.js";
  import { invoke, convertFileSrc, basename, isOldPicture } from "./lib.js";
  import { t, tn } from "./i18n.svelte.js";
  import { hljs } from "./hl.js";

  const m = ui.modal;
  const ROW = 20;
  const names = [basename(m.left), basename(m.right)];

  let kind = $state(null);
  let st = $state({ percent: 0, ready: false });
  let options = $state({ whitespace: false, case: false });
  let view = $state("split");
  let rows = $state([]);
  let first = $state(0);
  let cur = $state(0);
  let said = $state("");
  let asking = $state(null);
  let body = $state();
  let height = $state(400);
  let lang = [null, null];
  // Pictures: how they are shown, where the overlay's line is, and what each one is drawn from.
  let pic = $state("side");
  let split = $state(50);
  let srcs = $state([null, null]);
  let wrote = false;
  let timer;

  const dirty = $derived(!!st.summary?.dirty?.some((d) => d));
  const total = $derived(st.total ?? 0);
  const shown = $derived(Math.ceil(height / ROW) + 2);

  async function start(place = true) {
    clearTimeout(timer);
    rows = [];
    first = cur = 0;
    st = { percent: 0, ready: false };
    try {
      kind = await invoke("compare_open", { left: m.left, right: m.right, options });
    } catch (e) {
      st = { ready: true, error: String(e) };
      return;
    }
    if (kind === "picture") srcs = await Promise.all([m.left, m.right].map(async (p) => convertFileSrc(isOldPicture(basename(p)) ? ((await invoke("old_picture", { path: p, page: 0, side: 4096 }).catch(() => null))?.file ?? p) : p)));
    if (kind === "text") lang = await Promise.all([m.left, m.right].map((path) => invoke("code_language", { path, head: "" }).catch(() => null)));
    await poll(place);
  }

  async function poll(place = false) {
    st = await invoke("compare_state", { view }).catch((e) => ({ ready: true, error: String(e) }));
    if (!st.ready || (st.count && !st.count.finished)) timer = setTimeout(() => poll(place && !st.ready), 150);
    if (st.ready && place && st.summary) {
      // The cursor on the first change.
      const r = await invoke("compare_next", { view, row: 0, back: false }).catch(() => null);
      const at0 = (await invoke("compare_rows", { view, from: 0, n: 1 }))[0];
      if (r != null && !(at0 && at0.tag !== "same")) go(r);
    }
    await fetchRows();
  }

  async function fetchRows() {
    if (!st.ready || st.error || kind === "picture") return;
    const from = first;
    const got = await invoke(kind === "binary" ? "compare_hex" : "compare_rows", kind === "binary" ? { from, n: shown } : { view, from, n: shown }).catch(() => []);
    if (from === first) rows = got;
  }

  function scrolled() {
    const f = Math.floor(body.scrollTop / ROW);
    if (f !== first) {
      first = f;
      fetchRows();
    }
  }

  async function go(r) {
    cur = Math.max(0, Math.min(r, total - 1));
    await tick();
    if (!body) return;
    if (cur * ROW < body.scrollTop || (cur + 1) * ROW > body.scrollTop + body.clientHeight) body.scrollTop = Math.max(0, (cur - Math.floor(shown / 3)) * ROW);
    scrolled();
  }

  async function next(back) {
    const r = await invoke("compare_next", { view, row: cur, back }).catch(() => null);
    if (r == null) said = t(back ? "compare.no_earlier" : "compare.no_later");
    else (said = ""), go(r);
  }

  async function take(from) {
    said = await invoke("compare_take", { view, row: cur, from }).catch((e) => String(e));
    await poll();
  }

  async function undo() {
    const r = await invoke("compare_undo", { view });
    if (r == null) said = t("editor.nothing_to_undo");
    await poll();
    if (r != null) go(r);
  }

  async function save(force = false) {
    if (!dirty) return void (said = t("compare.nothing_to_save"));
    try {
      await invoke("compare_save", { force });
      wrote = true;
      asking = null;
      // Read again: where the lines of a side written start is known no more.
      const [c, f] = [cur, first];
      await start(false);
      [cur, first] = [c, f];
      if (body) body.scrollTop = f * ROW;
      said = t("compare.saved");
      return true;
    } catch (e) {
      if (String(e) === "changed") asking = "changed";
      else said = t("editor.not_saved", { error: String(e) });
      return false;
    }
  }

  function setView(v) {
    view = v;
    first = 0;
    if (body) body.scrollTop = 0;
    poll(true);
  }

  function setOption(name) {
    if (dirty) return void (said = t("compare.save_first"));
    options[name] = !options[name];
    start();
  }

  function close() {
    clearTimeout(timer);
    invoke("compare_close");
    ui.modal = null;
    if (wrote) reloadAll();
  }

  const tryClose = () => (dirty ? (asking = "close") : close());

  async function saveAndClose() {
    if (await save()) close();
  }

  m.key = (k, act) => {
    if (k === "Esc") return asking ? (asking = null) : tryClose(), true;
    if (act === "quit") return tryClose(), true;
    const keys = {
      Up: () => go(cur - 1),
      Down: () => go(cur + 1),
      PageUp: () => go(cur - shown + 2),
      PageDown: () => go(cur + shown - 2),
      Home: () => go(0),
      End: () => go(total - 1),
      n: () => next(false),
      p: () => next(true),
      "Shift+N": () => next(true),
      v: () => kind !== "binary" && setView(view === "split" ? "inline" : "split"),
      "Alt+Right": () => take(0),
      "Alt+Left": () => take(1),
      "Ctrl+z": undo,
      "Ctrl+s": () => save(),
      F2: () => save(),
    };
    if (!keys[k]) return false;
    keys[k]();
    return true;
  };

  /** A line's text in pieces, the changed ones marked, each coloured as code. */
  function pieces(cell, side) {
    const text = cell.text;
    const out = [];
    let at = 0;
    for (const [a, b] of [...cell.marks, [text.length, text.length]]) {
      if (a > at) out.push({ s: text.slice(at, a), mark: false });
      if (b > a) out.push({ s: text.slice(a, b), mark: true });
      at = Math.max(at, b);
    }
    const l = lang[side];
    return out.map((p) => ({ ...p, html: l && hljs.getLanguage(l) && p.s.length < 2000 ? hljs.highlight(p.s, { language: l }).value : escape(p.s) }));
  }
  const escape = (s) => s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
  const hex = (b) => (b == null ? "  " : b.toString(16).padStart(2, "0"));
  const SIGN = { same: " ", changed: "~", removed: "−", added: "+", taken: "=" };

  start();
  onDestroy(() => clearTimeout(timer));
</script>

{#snippet cell(r, c, side)}
  <span class="no">{c ? c.no : ""}{c ? SIGN[r.tag] : ""}</span>
  <span class="line hljs">{#if c}{#each pieces(c, side) as p}{#if p.mark}<mark>{@html p.html}</mark>{:else}{@html p.html}{/if}{/each}{/if}</span>
{/snippet}

<div class="compare" role="dialog" aria-modal="true" aria-label={t("action.compare_files")}>
  <header>
    <h2><bdi>{names[0]}</bdi> ↔ <bdi>{names[1]}</bdi>{#if dirty}<span class="dot" title={t("editor.modified")}> ●</span>{/if}</h2>
    <span class="acts">
      {#if kind === "text" || kind === "document"}
        <button onclick={() => next(true)}>{t("compare.previous")} <kbd>P</kbd></button>
        <button onclick={() => next(false)}>{t("compare.next")} <kbd>N</kbd></button>
        <button onclick={() => setView(view === "split" ? "inline" : "split")}>{t(view === "split" ? "compare.inline" : "compare.side_by_side")} <kbd>V</kbd></button>
        <label><input type="checkbox" checked={options.whitespace} onchange={() => setOption("whitespace")} />{t("compare.spaces")}</label>
        <label><input type="checkbox" checked={options.case} onchange={() => setOption("case")} />{t("compare.case")}</label>
        {#if kind === "text"}
          <button disabled={!st.summary?.editable?.[1]} onclick={() => take(0)}>{t("compare.to_right")} <kbd>Alt+→</kbd></button>
          <button disabled={!st.summary?.editable?.[0]} onclick={() => take(1)}>{t("compare.to_left")} <kbd>Alt+←</kbd></button>
          <button onclick={undo}>{t("compare.undo")} <kbd>Ctrl+Z</kbd></button>
          <button class="primary" disabled={!dirty} onclick={() => save()}>{t("compare.save")} <kbd>F2</kbd></button>
        {/if}
      {:else if kind === "binary"}
        <button onclick={() => next(true)}>{t("compare.previous")} <kbd>P</kbd></button>
        <button onclick={() => next(false)}>{t("compare.next")} <kbd>N</kbd></button>
      {:else if kind === "picture"}
        {#each [["side", "compare.side_by_side"], ["overlay", "compare.overlay"], ["difference", "compare.difference"]] as [id, label]}
          <button class:primary={pic === id} onclick={() => (pic = id)}>{t(label)}</button>
        {/each}
      {/if}
      <button onclick={tryClose}>{t("common.close")} <kbd>Esc</kbd></button>
    </span>
  </header>

  <p class="summary">
    {#if !st.ready}
      {t("compare.reading", { percent: st.percent })}
    {:else if st.error}
      <span class="error">{st.error}</span>
    {:else if st.summary}
      {@const s = st.summary}
      {s.changes ? `${tn("compare.changes", s.changes)} · ${tn("compare.removed", s.removed)} · ${tn("compare.added", s.added)}` : t("compare.same_text")}
      {#if s.kind === "document"} · {t("compare.by_text")}{/if}
      {#if s.encoding[0] !== s.encoding[1] || s.crlf[0] !== s.crlf[1]} · {s.encoding[0]} {s.crlf[0] ? "CRLF" : "LF"} ↔ {s.encoding[1]} {s.crlf[1] ? "CRLF" : "LF"}{/if}
      {#if s.cut} · {t("compare.cut")}{/if}
    {:else if st.count}
      {@const c = st.count}
      {c.finished && !c.differ ? t("compare.same") : tn("compare.bytes_differ", c.differ)}
      {#if c.finished && c.differ} · {t("compare.first_at", { offset: "0x" + c.first.toString(16) })}{/if}
      {#if !c.finished} · {t("compare.counting", { percent: Math.floor((100 * c.done) / Math.max(1, ...st.len)) })}{/if}
    {:else if st.pictures}
      {@const p = st.pictures}
      {p.differ == null ? t("compare.pictures_sizes") : p.differ ? tn("compare.pictures_differ", p.differ) : t("compare.pictures_same")}
      · {p.left.width} × {p.left.height} ↔ {p.right.width} × {p.right.height}
    {/if}
  </p>

  {#if kind === "picture" && st.ready && !st.error}
    <div class="pictures {pic}">
      {#if pic === "side"}
        {#each srcs as src, i}<figure><img {src} alt={names[i]} /><figcaption><bdi>{names[i]}</bdi></figcaption></figure>{/each}
      {:else}
        <div class="stack" style:--split="{split}%">
          <img src={srcs[0]} alt={names[0]} />
          <img class="top" src={srcs[1]} alt={names[1]} />
        </div>
        {#if pic === "overlay"}<input type="range" min="0" max="100" bind:value={split} aria-label={t("compare.overlay")} />{/if}
      {/if}
    </div>
  {:else}
    <div class="body mono" bind:this={body} bind:clientHeight={height} onscroll={scrolled}>
      <div style:height="{total * ROW}px" class="space">
        <div class="rows" style:transform="translateY({first * ROW}px)">
          {#each rows as r, i (first + i)}
            {@const at = first + i}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="row {r.tag ?? ''}" class:cur={at === cur} class:inline={view === "inline"} onclick={() => (cur = at)}>
              {#if kind === "binary"}
                <span class="no">{r.offset.toString(16).padStart(8, "0")}</span>
                {#each [[r.left, r.right], [r.right, r.left]] as [a, b]}
                  <span class="hex">{#each Array.from({ length: 16 }, (_, j) => j) as j}<span class:diff={a[j] != null && a[j] !== b[j]}>{hex(a[j])}</span> {/each}</span>
                {/each}
              {:else if view === "split"}
                <div class="half">{@render cell(r, r.left, 0)}</div>
                <div class="half">{@render cell(r, r.right, 1)}</div>
              {:else}
                <div class="half">{@render cell(r, r.tag === "added" ? r.right : r.left, r.tag === "added" ? 1 : 0)}</div>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}

  {#if asking === "close"}
    <div class="ask">
      <span>{t("compare.ask_close_gui")}</span>
      <button class="primary" onclick={saveAndClose}>{t("compare.save")}</button>
      <button class="danger" onclick={close}>{t("editor.discard")}</button>
      <button onclick={() => (asking = null)}>{t("common.cancel")}</button>
    </div>
  {:else if asking === "changed"}
    <div class="ask">
      <span>{t("compare.ask_changed_gui")}</span>
      <button class="danger" onclick={() => save(true)}>{t("editor.save_over")}</button>
      <button onclick={() => (asking = null)}>{t("common.cancel")}</button>
    </div>
  {/if}
  <footer>
    <span class="said">{said}</span>
    {#if kind === "text" || kind === "document"}<span class="keys">{t("compare.keys_gui")}</span>{/if}
  </footer>
</div>

<style>
  .compare {
    position: fixed;
    inset: 3vh 3vw;
    z-index: 11;
    display: flex;
    flex-direction: column;
    background: var(--panel-bg, var(--dialog-bg));
    color: var(--panel-fg, var(--dialog-fg));
    border: 1px solid var(--border-fg);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  header,
  footer,
  .ask {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 14px;
    background: var(--dialog-bg);
    color: var(--dialog-fg);
  }
  header {
    border-bottom: 1px solid var(--border-fg);
    flex-wrap: wrap;
  }
  footer,
  .ask {
    border-top: 1px solid var(--border-fg);
  }
  h2 {
    margin: 0;
    font-size: 1.05em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dot {
    color: var(--git-modified-fg);
  }
  .acts {
    margin-left: auto;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .acts label {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .summary {
    margin: 0;
    padding: 4px 14px;
    color: var(--header-fg);
    border-bottom: 1px solid var(--border-fg);
  }
  .said,
  .keys {
    color: var(--hidden-fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .keys {
    margin-left: auto;
    font-size: 0.9em;
  }
  .error {
    color: var(--git-deleted-fg);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    position: relative;
  }
  .mono {
    font-family: var(--mono-font), var(--scripts);
    font-size: 0.95em;
  }
  .space {
    position: relative;
    min-width: 100%;
  }
  .rows {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
  }
  .row {
    display: flex;
    height: 20px;
    line-height: 20px;
    white-space: pre;
    cursor: default;
  }
  .half {
    flex: 1 1 50%;
    min-width: 0;
    display: flex;
    overflow: hidden;
    border-right: 1px solid var(--border-fg);
  }
  .row.inline .half {
    flex-basis: 100%;
    border: 0;
  }
  .no {
    flex: none;
    min-width: 6ch;
    padding: 0 6px;
    text-align: right;
    color: var(--hidden-fg);
    user-select: none;
  }
  .line {
    overflow: hidden;
    text-overflow: ellipsis;
    tab-size: 4;
    background: transparent;
    padding: 0;
  }
  .row.changed {
    background: color-mix(in srgb, var(--git-modified-fg) 12%, transparent);
  }
  .row.removed {
    background: color-mix(in srgb, var(--git-deleted-fg) 14%, transparent);
  }
  .row.added {
    background: color-mix(in srgb, var(--git-added-fg) 14%, transparent);
  }
  .row.taken {
    background: color-mix(in srgb, var(--git-renamed-fg) 14%, transparent);
  }
  .row.cur {
    outline: 1px solid var(--accent-bg);
    outline-offset: -1px;
  }
  mark,
  .hex .diff {
    background: color-mix(in srgb, var(--git-modified-fg) 45%, transparent);
    color: inherit;
    border-radius: 2px;
  }
  .row.removed mark {
    background: color-mix(in srgb, var(--git-deleted-fg) 45%, transparent);
  }
  .row.added mark {
    background: color-mix(in srgb, var(--git-added-fg) 45%, transparent);
  }
  .hex span {
    margin-right: 0.6ch;
  }
  .hex {
    padding: 0 12px;
    border-right: 1px solid var(--border-fg);
  }
  .pictures {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 12px;
    overflow: auto;
  }
  .pictures.side {
    flex-direction: row;
  }
  figure {
    margin: 0;
    flex: 1;
    min-width: 0;
    text-align: center;
  }
  figure img,
  .stack img {
    max-width: 100%;
    max-height: 70vh;
    object-fit: contain;
  }
  .stack {
    position: relative;
    display: inline-block;
  }
  .stack .top {
    position: absolute;
    left: 0;
    top: 0;
  }
  /* The right picture over the left, cut at the slider. */
  .overlay .stack .top {
    clip-path: inset(0 0 0 var(--split));
  }
  /* What is the same goes black; what changed lights up. */
  .difference .stack {
    isolation: isolate;
    filter: brightness(3);
    background: #000;
  }
  .difference .stack .top {
    mix-blend-mode: difference;
  }
  .difference .stack img {
    max-height: 70vh;
  }
  .stack .top,
  .stack img:first-child {
    width: 100%;
  }
  input[type="range"] {
    width: min(600px, 80%);
  }
  .line :global(.hljs-keyword),
  .line :global(.hljs-selector-tag),
  .line :global(.hljs-built_in) { color: var(--symlink-fg); }
  .line :global(.hljs-string),
  .line :global(.hljs-attr) { color: var(--git-added-fg); }
  .line :global(.hljs-number),
  .line :global(.hljs-literal) { color: var(--search-hit-fg); }
  .line :global(.hljs-comment) { color: var(--hidden-fg); font-style: italic; }
  .line :global(.hljs-title),
  .line :global(.hljs-function) { color: var(--directory-fg); }
  .line :global(.hljs-type),
  .line :global(.hljs-class) { color: var(--git-modified-fg); }
  .line :global(.hljs-meta) { color: var(--git-renamed-fg); }
</style>

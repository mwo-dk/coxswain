<script module>
  // What a Space window had, kept while Delete's confirmation or F1 stands in for it, so it
  // comes back as it was: the modal it belongs to, the walk's result and what was shown.
  let kept = null;
</script>

<script>
  // Space (coxswain-core's space.rs): where the bytes under a folder go. The folder is walked
  // once, then a treemap shows every file as a rectangle by its bytes inside its folder's,
  // coloured by its kind; beside it the bytes by type and the largest files. Clean-up goes
  // through the app's own actions: Delete to the bin, Edit, and Enter to the file in its panel.
  import { onDestroy, untrack } from "svelte";
  import { ui, tab, showInTab, refreshUndo, reloadAll } from "./app.svelte.js";
  import { invoke, size, basename, parent } from "./lib.js";
  import { t, tn, num } from "./i18n.svelte.js";
  import { at, step } from "./space.js";

  const d = ui.modal;
  const back = kept?.modal === d ? kept : null;

  let summary = $state(back?.summary ?? null);
  let progress = $state(null);
  let measuring = $state(false);
  let failed = $state("");
  let zoom = $state(back?.zoom ?? 0);
  let view = $state.raw(back?.view ?? null);
  /** The rectangle under the cursor (an index of view.tiles), its path, and the pointer. */
  let cursor = $state(-1);
  let info = $state.raw(null);
  let pointer = $state(null);
  /** The type whose rectangles stand out (an index of view.types), and the list shown. */
  let hl = $state(back?.hl ?? -1);
  let side = $state(back?.side ?? "types");
  let row = $state(0);

  let box = $state();
  let base = $state();
  let top = $state();
  let w = $state(0);
  let h = $state(0);

  const key = (name) => ui.cfg.actions[name]?.[1] ?? "";

  async function measure() {
    measuring = true;
    failed = "";
    view = null;
    const timer = setInterval(async () => (progress = await invoke("space_progress")), 200);
    try {
      const [bytes, files, online, seconds] = await invoke("space_scan", { dir: d.root });
      summary = { bytes, files, online, seconds };
      zoom = 0;
      hl = -1;
    } catch (e) {
      failed = String(e);
    } finally {
      clearInterval(timer);
      measuring = false;
      progress = null;
    }
  }
  // Back from F1 or a confirmation while it was measuring: that walk was stopped, so again.
  if (!back?.summary) measure();

  // Laid out again when the size or the zoom changes, a moment after a resize stops.
  let laying = 0;
  $effect(() => {
    if (!summary || measuring || !w || !h) return;
    d.gen;
    const [z, ww, hh] = [zoom, w, h];
    const n = ++laying;
    const timer = setTimeout(async () => {
      try {
        const v = await invoke("space_view", { zoom: z, w: ww, h: hh });
        if (n !== laying) return;
        view = v;
        cursor = v.tiles.findIndex((x) => x.kind !== "folder");
        row = Math.min(row, Math.max(0, (side === "types" ? v.types : v.largest).length - 1));
        if (hl >= v.types.length) hl = -1;
      } catch (e) {
        failed = String(e);
      }
    }, untrack(() => view) ? 80 : 0);
    return () => clearTimeout(timer);
  });

  // A delete from Space (its run outlives this window when a confirmation stood in for it):
  // the totals that are left.
  $effect(() => {
    if (!d.gen) return;
    const [bytes, files] = d.totals ?? [];
    untrack(() => summary && bytes != null && (summary = { ...summary, bytes, files }));
  });

  onDestroy(() => {
    kept = { modal: d, summary, zoom, view, hl, side };
    // Parked while F1 or a confirmation stands in for it (they come back to `d`): the tree
    // stays. Closed: it is let go.
    if (ui.modal?.back === d) {
      if (measuring) invoke("space_cancel");
    } else invoke("space_close");
  });

  // ------------------------------------------------------------ drawing

  const css = (name) => getComputedStyle(box).getPropertyValue(name).trim();
  const luma = (hex) => {
    const n = parseInt(hex.slice(1), 16);
    return 0.299 * (n >> 16) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255);
  };

  /** The light from the top left, once: drawn over every file, it makes each a cushion. */
  let cushion;
  function shine() {
    if (cushion) return cushion;
    cushion = document.createElement("canvas");
    cushion.width = cushion.height = 64;
    const c = cushion.getContext("2d");
    const g = c.createRadialGradient(20, 16, 2, 28, 26, 58);
    g.addColorStop(0, "rgba(255,255,255,0.38)");
    g.addColorStop(0.45, "rgba(255,255,255,0.04)");
    g.addColorStop(1, "rgba(0,0,0,0.42)");
    c.fillStyle = g;
    c.fillRect(0, 0, 64, 64);
    return cushion;
  }

  /** `s` cut with … to `max` pixels. */
  function fit(c, s, max) {
    if (c.measureText(s).width <= max) return s;
    let lo = 0;
    let hi = s.length;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (c.measureText(s.slice(0, mid) + "…").width <= max) lo = mid;
      else hi = mid - 1;
    }
    return lo ? s.slice(0, lo) + "…" : "";
  }

  function draw() {
    if (!base || !view) return;
    const dpr = devicePixelRatio || 1;
    for (const cv of [base, top]) {
      cv.width = Math.round(w * dpr);
      cv.height = Math.round(h * dpr);
    }
    const c = base.getContext("2d");
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    const [bg, fg, border] = [css("--dialog-bg"), css("--dialog-fg"), css("--border-fg")];
    c.fillStyle = bg;
    c.fillRect(0, 0, w, h);
    const font = css("--font") || "sans-serif";
    const light = shine();
    // Stripes for the blocks of files too small to show one by one.
    const p = document.createElement("canvas");
    p.width = p.height = 6;
    const pc = p.getContext("2d");
    pc.fillStyle = "#4a525c";
    pc.fillRect(0, 0, 6, 6);
    pc.strokeStyle = "#6b7480";
    pc.beginPath();
    pc.moveTo(0, 6);
    pc.lineTo(6, 0);
    pc.stroke();
    const stripes = c.createPattern(p, "repeat");
    for (const r of view.tiles) {
      const off = hl >= 0 && r.ty !== hl;
      c.globalAlpha = off ? 0.22 : 1;
      if (r.kind === "folder") {
        c.fillStyle = `color-mix(in srgb, ${fg} ${8 + Math.min(r.depth, 4) * 3}%, ${bg})`;
        c.fillRect(r.x, r.y, r.w, r.h);
        c.strokeStyle = border;
        c.lineWidth = 1;
        c.strokeRect(r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1);
        if (r.name) {
          c.globalAlpha = 1;
          c.fillStyle = fg;
          c.font = `600 11px ${font}`;
          c.textBaseline = "middle";
          c.fillText(fit(c, `${r.name}  ${size(r.size)}`, r.w - 8), r.x + 5, r.y + 9);
        }
        continue;
      }
      if (r.kind === "small") {
        c.fillStyle = stripes;
        c.fillRect(r.x, r.y, r.w, r.h);
        continue;
      }
      if (r.kind === "mount") {
        // Another disk: not walked, not to scale; a click opens Space on it.
        c.fillStyle = "#3f5277";
        c.fillRect(r.x, r.y, r.w, r.h);
        c.strokeStyle = "#8aa2d8";
        c.setLineDash([4, 3]);
        c.strokeRect(r.x + 1.5, r.y + 1.5, r.w - 3, r.h - 3);
        c.setLineDash([]);
        if (r.name) {
          c.fillStyle = "#e8eefc";
          c.font = `600 12px ${font}`;
          c.textBaseline = "top";
          c.fillText(fit(c, t("space.other_disk", { name: r.name }), r.w - 10), r.x + 5, r.y + 5);
        }
        continue;
      }
      const colour = view.colours[r.family];
      c.fillStyle = colour;
      c.fillRect(r.x, r.y, r.w, r.h);
      if (r.w > 3 && r.h > 3) {
        c.drawImage(light, r.x, r.y, r.w, r.h);
        c.strokeStyle = "rgba(0,0,0,0.45)";
        c.lineWidth = 1;
        c.strokeRect(r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1);
      }
      if (r.name && !off) {
        c.fillStyle = luma(colour) > 150 ? "#101418" : "#ffffff";
        c.font = `600 12px ${font}`;
        c.textBaseline = "top";
        c.fillText(fit(c, r.name, r.w - 8), r.x + 4, r.y + 3);
        if (r.h >= 34) {
          c.globalAlpha = 0.75;
          c.font = `12px ${font}`;
          c.fillText(fit(c, size(r.size), r.w - 8), r.x + 4, r.y + 17);
        }
      }
    }
    c.globalAlpha = 1;
    drawCursor();
  }

  /** The rectangle under the cursor, lit, and the frame of its folder. */
  function drawCursor() {
    if (!top || !view) return;
    const dpr = devicePixelRatio || 1;
    const c = top.getContext("2d");
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    c.clearRect(0, 0, w, h);
    const r = view.tiles[cursor];
    if (!r) return;
    c.fillStyle = "rgba(255,255,255,0.22)";
    c.fillRect(r.x, r.y, r.w, r.h);
    c.lineWidth = 2;
    c.strokeStyle = "rgba(0,0,0,0.8)";
    c.strokeRect(r.x + 1, r.y + 1, r.w - 2, r.h - 2);
    c.strokeStyle = "#ffffff";
    c.strokeRect(r.x + 2, r.y + 2, Math.max(0, r.w - 4), Math.max(0, r.h - 4));
  }

  $effect(() => {
    view;
    hl;
    w;
    h;
    untrack(draw);
  });
  $effect(() => {
    cursor;
    untrack(drawCursor);
    const r = view?.tiles[cursor];
    if (!r) return void (info = null);
    invoke("space_info", { id: r.id }).then((i) => view?.tiles[cursor] === r && (info = i ? { path: i[0], dir: i[1], tile: r } : null));
  });

  // ------------------------------------------------------------ doing

  const typeOf = (r) => view.types[r.ty];
  const kindOf = (r) => {
    const ty = typeOf(r);
    const name = t(`space.family_${r.family}`);
    return ty?.ext ? `${name} (.${ty.ext})` : name;
  };

  /** The file the keys are about: the cursor's on the map, the row's in Largest. */
  const current = () => (side === "largest" && d.list ? view?.largest[row]?.[2] : info?.tile.kind === "file" ? info.path : null);

  function zoomTo(id) {
    if (id == null || id === zoom) return;
    zoom = id;
  }
  async function zoomIn(r = view?.tiles[cursor]) {
    if (!r) return;
    zoomTo(await invoke("space_toward", { zoom, id: r.id }));
  }
  function zoomOut() {
    if (view?.crumbs.length > 1) zoomTo(view.crumbs.at(-2)[0]);
  }

  /** Space on another disk (a mount block): walked now, in this window. */
  function openDisk(path) {
    if (!path) return;
    d.root = path;
    measure();
  }

  async function go(path) {
    if (!path) return;
    ui.modal = null;
    await showInTab(tab(), path);
  }

  async function remove(path) {
    if (!path) return;
    let plan;
    try {
      plan = await invoke("delete_plan", { paths: [path], forever: false, inside: false });
    } catch (e) {
      return void (ui.status = String(e));
    }
    const what = `"${basename(path)}"`;
    const gone = plan.forever;
    const run = async () => {
      ui.status = t("status.busy", { what });
      try {
        await invoke("delete", { paths: [path], forever: gone });
        await refreshUndo(gone ? t("status.deleted", { what }) : t("status.trashed", { what }));
        d.totals = await invoke("space_gone", { paths: [path] });
        if (kept?.modal === d) kept.view = null;
        d.gen = (d.gen ?? 0) + 1;
      } catch (e) {
        ui.status = String(e);
      }
      ui.modal = d;
      reloadAll();
    };
    if (ui.cfg.confirm_delete || plan.must_confirm) ui.modal = { kind: "confirm", title: t("dialog.delete"), text: t(plan.question, { what, archive: "" }), ok: t(gone ? "common.delete" : "app.move_to_trash"), run, feature: "delete", back: d };
    else run();
  }

  /** Keys while Space is open (Dialogs.svelte passes them on). */
  d.key = (k, act) => {
    if (measuring) {
      if (k === "Esc") (invoke("space_cancel"), (ui.modal = null));
      return true;
    }
    const tiles = view?.tiles ?? [];
    const list = side === "types" ? (view?.types ?? []) : (view?.largest ?? []);
    if (k === "Esc") {
      if (hl >= 0) hl = -1;
      else ui.modal = null;
    } else if (k === "Tab" || k === "Shift+Tab") {
      // The map, then the list beside it, then its other list.
      if (!d.list) d.list = true;
      else if (side === "types") side = "largest";
      else (side = "types"), (d.list = false);
      row = 0;
    } else if (["Up", "Down", "Left", "Right"].includes(k) && !d.list) {
      cursor = step(tiles, cursor, { Left: -1, Right: 1 }[k] ?? 0, { Up: -1, Down: 1 }[k] ?? 0);
    } else if (["Up", "Down", "PageUp", "PageDown", "Home", "End"].includes(k)) {
      const by = { Up: -1, Down: 1, PageUp: -15, PageDown: 15, Home: -1e9, End: 1e9 }[k];
      row = Math.max(0, Math.min(list.length - 1, row + by));
      if (side === "largest") cursor = tiles.findIndex((x) => x.id === view.largest[row]?.[0]);
    } else if (k === "Enter") {
      if (d.list && side === "types") hl = hl === row ? -1 : row;
      else if (!d.list && tiles[cursor]?.kind === "mount") openDisk(info?.path);
      else if (!d.list && tiles[cursor]?.kind !== "file") zoomIn();
      else go(current());
    } else if (k === "+" || k === "=") zoomIn();
    else if (k === "Backspace" || k === "-") zoomOut();
    else if (act === "refresh") measure();
    else if (act === "view") {
      const p = current();
      if (p) go(p).then(() => (ui.showPreview = true));
    } else if (act === "edit") {
      const p = current();
      if (p) invoke("edit_path", { path: p }).catch((e) => (ui.status = String(e)));
    } else if (act === "delete") remove(current());
    else return false;
    return true;
  };

  // ------------------------------------------------------------ the pointer

  function move(e) {
    const rect = top.getBoundingClientRect();
    const [x, y] = [e.clientX - rect.left, e.clientY - rect.top];
    const i = at(view?.tiles ?? [], x, y);
    if (i >= 0) cursor = i;
    d.list = false;
    pointer = { x, y };
  }
  function click(e) {
    move(e);
    const r = view?.tiles[cursor];
    if (!r) return;
    if (r.kind === "file") go(info?.tile === r ? info.path : null);
    else if (r.kind === "mount") openDisk(info?.tile === r ? info.path : null);
    else zoomTo(r.id);
  }
  function context(e) {
    e.preventDefault();
    move(e);
    zoomIn();
  }

  const home = ui.cfg.home;
  const shown = (p) => (p === home ? "~" : p?.startsWith(home + ui.cfg.sep) ? "~" + p.slice(home.length) : p);
  const relative = (p) => (view && p.startsWith(view.path) ? p.slice(view.path.length).replace(/^[\\/]/, "") : p);
</script>

<div class="space" role="dialog" aria-modal="true" aria-label={t("space.window")} bind:this={box}>
  <header>
    <h2>{"\u{f200}"} {t("space.window")}: <bdi>{shown(d.root)}</bdi></h2>
    {#if summary && !measuring}
      <span class="sum">{t("space.summary", { size: size(summary.bytes), files: tn("space.files", summary.files, { n: num(summary.files) }), s: num(summary.seconds, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })}</span>
    {/if}
    <button title={`${t("space.measure_again")} (${key("refresh")})`} disabled={measuring} onclick={measure}>{"\u{f021}"} {t("space.measure_again")}</button>
    <button class="x" title={`${t("space.close")} (Esc)`} onclick={() => (ui.modal = null)}>×</button>
  </header>

  <nav class="crumbs" class:hidden={!view}>
    {#each view?.crumbs ?? [] as [id, name], i (id)}
      {#if i}<span class="sep">›</span>{/if}
      <button class="crumb" class:here={id === zoom} onclick={() => zoomTo(id)}>{i ? name : shown(name)}</button>
    {/each}
    {#if view}<span class="crumb-size">{size(view.size)} · {tn("space.files", view.files, { n: num(view.files) })}</span>{/if}
    <button disabled={(view?.crumbs.length ?? 0) < 2} title={`${t("space.zoom_out")} (Backspace)`} onclick={zoomOut}>{"\u{f010}"} {t("space.zoom_out")}</button>
  </nav>

  <div class="body">
    <div class="map" bind:clientWidth={w} bind:clientHeight={h} role="presentation" onmouseleave={() => (pointer = null)}>
      {#if measuring || failed}
        <section class="wait">
          {#if measuring}
            <p>{t("space.measuring", { files: num(progress?.[0] ?? 0), size: size(progress?.[1] ?? 0) })}</p>
            <button class="primary" onclick={() => (invoke("space_cancel"), (ui.modal = null))}>{t("space.stop")} <kbd>Esc</kbd></button>
          {:else}
            <p>{failed}</p>
          {/if}
        </section>
      {/if}
      <canvas bind:this={base} style:width="{w}px" style:height="{h}px"></canvas>
      <canvas bind:this={top} class="top" style:width="{w}px" style:height="{h}px" onmousemove={move} onclick={click} oncontextmenu={context}></canvas>
      {#if pointer && info}
        <div class="tip" style:left="{Math.min(pointer.x + 14, w - 260)}px" style:top="{Math.min(pointer.y + 16, h - 60)}px">
          {#if info.tile.kind === "small"}
            <b>{t("space.small_title")}</b><span>{t("space.small_tip")}</span>
          {:else if info.tile.kind === "mount"}
            <b>{t("space.other_disk", { name: shown(info.path) })}</b><span>{t("space.other_disk_tip")}</span>
          {:else}
            <b>{basename(info.path)}</b>
            <span>{size(info.tile.size)}{info.tile.kind === "file" ? ` · ${kindOf(info.tile)}` : ""}</span>
            {#if info.tile.kind === "folder"}<span>{t("space.folder_tip")}</span>{/if}
          {/if}
        </div>
      {/if}
    </div>

    <aside>
      <div class="tabs" role="tablist">
        <button role="tab" aria-selected={side === "types"} class:on={side === "types"} onclick={() => ((side = "types"), (row = 0))}>{t("space.by_type")}</button>
        <button role="tab" aria-selected={side === "largest"} class:on={side === "largest"} onclick={() => ((side = "largest"), (row = 0))}>{t("space.largest")}</button>
      </div>
      <ul class="list" class:focus={d.list}>
        {#if side === "types"}
          {#each view?.types ?? [] as r, i (r.ext)}
            <li>
              <button class="type" class:cur={d.list && i === row} class:hl={hl === i} title={t("space.type_tip")} onclick={() => ((row = i), (hl = hl === i ? -1 : i))}>
                <span class="swatch" style:background={view.colours[r.family]}></span>
                <span class="ext">{r.ext ? `.${r.ext}` : t("space.no_ext")}</span>
                <span class="fam">{t(`space.family_${r.family}`)}</span>
                <span class="num">{size(r.bytes)}</span>
                <span class="share"><span style:width="{r.share * 100}%" style:background={view.colours[r.family]}></span></span>
                <span class="num pc">{num(r.share * 100, { maximumFractionDigits: r.share < 0.1 ? 1 : 0 })} %</span>
              </button>
            </li>
          {/each}
        {:else}
          {#each view?.largest ?? [] as [id, bytes, path], i (id)}
            <li>
              <button class="big" class:cur={d.list && i === row} title={`${path}\n${t("space.largest_tip")}`} onmouseenter={() => (cursor = view.tiles.findIndex((x) => x.id === id))} onclick={() => go(path)}>
                <span class="num">{size(bytes)}</span>
                <span class="path"><bdi>{relative(path)}</bdi></span>
              </button>
            </li>
          {/each}
        {/if}
      </ul>
    </aside>
  </div>

  <footer>
    <span class="info">
      {#if info?.tile.kind === "small"}
        {t("space.small", { folder: shown(info.path) })}
      {:else if info?.tile.kind === "mount"}
        {t("space.other_disk", { name: shown(info.path) })} · {t("space.other_disk_tip")}
      {:else if info}
        <bdi>{shown(info.path)}</bdi> · {size(info.tile.size)}{info.tile.kind === "file" ? ` · ${kindOf(info.tile)}` : ""}
      {/if}
      {#if summary?.online}<span class="online"> · {t("space.online", { count: num(summary.online) })}</span>{/if}
    </span>
    <span class="acts">
      <button disabled={!current()} title={t("space.show_tip")} onclick={() => go(current())}>{t("common.show")} <kbd>Enter</kbd></button>
      <button disabled={!current()} onclick={() => d.key("", "edit")}>{ui.cfg.actions.edit?.[0]} <kbd>{key("edit")}</kbd></button>
      <button class="danger" disabled={!current()} onclick={() => remove(current())}>{ui.cfg.actions.delete?.[0]} <kbd>{key("delete")}</kbd></button>
    </span>
  </footer>
  <p class="keys">{t("space.keys_gui")}</p>
</div>

<style>
  .space {
    position: fixed;
    inset: 3vh 3vw;
    z-index: 11;
    display: flex;
    flex-direction: column;
    background: var(--dialog-bg);
    color: var(--dialog-fg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  header,
  footer,
  .crumbs {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 14px;
  }
  header {
    border-bottom: 1px solid var(--border-fg);
  }
  footer {
    border-top: 1px solid var(--border-fg);
  }
  h2 {
    margin: 0;
    font-size: 1.05em;
    font-family: var(--icon-font), var(--font), var(--scripts);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sum {
    flex: 1;
    color: var(--hidden-fg);
    font-size: 0.9em;
    white-space: nowrap;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 3px 10px;
    cursor: pointer;
    font-family: var(--icon-font), var(--font), var(--scripts);
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
  .danger:not(:disabled) {
    border-color: var(--git-deleted-fg);
    color: var(--git-deleted-fg);
  }
  kbd {
    font-size: 0.8em;
    opacity: 0.7;
  }
  .wait {
    position: absolute;
    inset: 0;
    z-index: 1;
    background: var(--dialog-bg);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
  }
  .wait p {
    margin: 0;
    font-size: 1.1em;
    font-variant-numeric: tabular-nums;
  }
  .hidden {
    display: none !important;
  }
  .crumbs {
    padding-block: 6px;
    font-size: 0.9em;
    flex-wrap: wrap;
  }
  .crumb {
    border: 0;
    padding: 1px 4px;
  }
  .crumb.here {
    font-weight: 600;
    color: var(--directory-fg);
  }
  .sep {
    color: var(--hidden-fg);
  }
  .crumb-size {
    flex: 1;
    color: var(--hidden-fg);
  }
  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .map {
    flex: 1;
    position: relative;
    min-width: 0;
    margin: 0 0 8px 12px;
    overflow: hidden;
  }
  canvas {
    position: absolute;
    inset: 0;
  }
  .top {
    cursor: pointer;
  }
  .tip {
    position: absolute;
    pointer-events: none;
    display: flex;
    flex-direction: column;
    max-width: 250px;
    padding: 6px 9px;
    background: var(--dialog-bg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    box-shadow: var(--shadow);
    font-size: 0.85em;
    overflow-wrap: anywhere;
  }
  .tip span {
    color: var(--hidden-fg);
  }
  aside {
    width: min(360px, 34%);
    display: flex;
    flex-direction: column;
    padding: 0 12px 8px;
    min-height: 0;
  }
  .tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 6px;
  }
  .tabs button {
    flex: 1;
    border-radius: var(--r-pill);
  }
  .tabs .on {
    background: var(--accent-bg);
    color: var(--accent-fg);
    border-color: transparent;
  }
  .list {
    flex: 1;
    overflow: auto;
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .list button {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    border: 0;
    border-radius: var(--r-sm);
    padding: 3px 6px;
    text-align: start;
    font-family: var(--font), var(--scripts);
    font-size: 0.9em;
  }
  .list button:hover,
  .list.focus .cur {
    background: var(--dialog-input-bg);
  }
  .list .hl {
    outline: 1px solid var(--accent-bg);
  }
  .swatch {
    width: 11px;
    height: 11px;
    border-radius: 3px;
    flex: none;
  }
  .ext {
    width: 5.5em;
    white-space: nowrap;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fam {
    flex: 1;
    color: var(--hidden-fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .num {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    text-align: end;
    min-width: 5em;
  }
  .pc {
    min-width: 3.6em;
    color: var(--hidden-fg);
  }
  .share {
    width: 46px;
    height: 5px;
    border-radius: 3px;
    background: var(--dialog-input-bg);
    overflow: hidden;
    flex: none;
  }
  .share span {
    display: block;
    height: 100%;
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
  .info {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.9em;
  }
  .online {
    color: var(--hidden-fg);
  }
  .acts {
    display: flex;
    gap: 6px;
  }
  .keys {
    margin: 0;
    padding: 0 14px 8px;
    color: var(--hidden-fg);
    font-size: 0.8em;
  }
</style>

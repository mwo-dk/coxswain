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
  import { at, step, chain, fit, palette, mix, luma } from "./space.js";

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
        const was = untrack(() => view);
        view = v;
        if (was && was.crumbs.at(-1)[0] !== v.crumbs.at(-1)[0]) fly(was, v);
        cursor = v.tiles.findIndex((x) => x.kind !== "folder");
        row = Math.min(row, Math.max(0, (side === "types" ? v.types : v.largest).length - 1));
        if (hl >= v.types.length) hl = -1;
      } catch (e) {
        failed = String(e);
      }
    }, untrack(() => view) ? 80 : 0);
    return () => clearTimeout(timer);
  });

  /** A zoom, shown: the folder grows from its rectangle to the whole map (in), or the map
   *  shrinks into the folder's rectangle in its parent (out). Not with reduced motion. */
  function fly(from, to) {
    if (!base || !top || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const [was, now] = [from.crumbs.at(-1)[0], to.crumbs.at(-1)[0]];
    const into = from.tiles.find((x) => x.kind === "folder" && x.id === now);
    const r = into ?? to.tiles.find((x) => x.kind === "folder" && x.id === was);
    if (!r?.w || !r?.h || !w || !h) return;
    const start = into ? `translate(${r.x}px, ${r.y}px) scale(${r.w / w}, ${r.h / h})` : `scale(${w / r.w}, ${h / r.h}) translate(${-r.x}px, ${-r.y}px)`;
    for (const cv of [base, top]) cv.animate([{ transform: start, opacity: 0.4 }, { transform: "none", opacity: 1 }], { duration: 240, easing: "cubic-bezier(0.2, 0.7, 0.2, 1)" });
  }

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
  /** A theme colour as `#rrggbb` (themes may name them: `blue`, `lightcyan`); null when unset. */
  let probe;
  const hexOf = (v) => {
    if (!v) return null;
    probe ??= document.createElement("canvas").getContext("2d");
    probe.fillStyle = "#000000";
    probe.fillStyle = v;
    return probe.fillStyle.startsWith("#") ? probe.fillStyle : null;
  };
  /** The theme's colours the map is drawn in. */
  function themed() {
    const slots = { red: "--git-deleted-fg", orange: "--search-hit-fg", yellow: "--git-modified-fg", green: "--git-added-fg", blue: "--git-renamed-fg", magenta: "--symlink-fg", dir: "--directory-fg", dim: "--hidden-fg", bg: "--dialog-bg", fg: "--dialog-fg", accent: "--accent-bg" };
    const out = {};
    for (const [k, v] of Object.entries(slots)) {
      const c = hexOf(css(v));
      if (c) out[k] = c;
    }
    out.bg ??= "#000000";
    out.fg ??= luma(out.bg) < 128 ? "#ffffff" : "#000000";
    out.accent ??= out.fg;
    return out;
  }
  let theme = $state.raw({});
  /** Each kind's colour, from the theme (space.js `palette`): the map and By type's swatches. */
  let colours = $derived(view ? palette(theme, view.colours) : {});
  $effect(() => {
    ui.theme;
    if (box) theme = themed();
  });

  /** A rounded rectangle's path. */
  function round(c, x, y, w, h, r) {
    c.beginPath();
    if (c.roundRect) c.roundRect(x, y, w, h, Math.max(0, Math.min(r, w / 2, h / 2)));
    else c.rect(x, y, w, h);
  }

  /** A label's text that fits `max` pixels in the current font, or "" (space.js `fit`). */
  const fits = (c, s, max) => (max > 0 ? fit(s, max, (x) => c.measureText(x).width) : "");

  /** Two lines in a rectangle: the name bold, the size smaller; centred in a big one, at the
   *  top left in the rest. Only what fits is drawn. */
  function label(c, r, name, sub, ink, font) {
    const big = r.w >= 150 && r.h >= 64;
    const pad = big ? 12 : 6;
    c.fillStyle = ink;
    c.textBaseline = "top";
    c.textAlign = big ? "center" : "start";
    c.font = `600 ${big ? 14 : 12}px ${font}`;
    const head = fits(c, name, r.w - 2 * pad);
    if (!head || r.h < 16) return;
    const two = sub && r.h >= (big ? 52 : 32);
    const x = big ? r.x + r.w / 2 : r.x + pad;
    const y = big ? r.y + r.h / 2 - (two ? 17 : 8) : r.y + 4;
    c.fillText(head, x, y);
    if (!two) return;
    c.font = `${big ? 12 : 11}px ${font}`;
    const line = fits(c, sub, r.w - 2 * pad);
    c.globalAlpha *= 0.78;
    if (line) c.fillText(line, x, y + (big ? 19 : 15));
    c.globalAlpha /= 0.78;
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
    const { bg, fg } = theme;
    if (!bg) return;
    const dark = luma(bg) < 128;
    c.fillStyle = bg;
    c.fillRect(0, 0, w, h);
    const font = css("--font") || "sans-serif";
    const dim = theme.dim ?? mix(bg, fg, 0.5);
    for (const r of view.tiles) {
      const off = hl >= 0 && r.ty !== hl;
      c.globalAlpha = off ? 0.22 : 1;
      if (r.kind === "folder") {
        // Deeper folders darker, their frames darker still, so the nesting reads.
        const fill = mix(mix(bg, fg, dark ? 0.08 : 0.05), "#000000", Math.min(r.depth - 1, 5) * (dark ? 0.11 : 0.045));
        round(c, r.x, r.y, r.w, r.h, 5);
        c.fillStyle = fill;
        c.fill();
        c.strokeStyle = mix(fill, "#000000", dark ? 0.5 : 0.22);
        c.lineWidth = 1;
        round(c, r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1, 5);
        c.stroke();
        if (r.name) {
          c.globalAlpha = 1;
          c.textBaseline = "middle";
          c.textAlign = "start";
          c.font = `600 11px ${font}`;
          const name = fits(c, r.name, r.w - 12);
          if (name) {
            c.fillStyle = fg;
            c.fillText(name, r.x + 6, r.y + 10);
            const left = c.measureText(name).width + 14;
            c.font = `11px ${font}`;
            const s = size(r.size);
            if (name === r.name && c.measureText(s).width <= r.w - left - 6) {
              c.fillStyle = dim;
              c.fillText(s, r.x + left, r.y + 10);
            }
          }
        }
        continue;
      }
      // Files and blocks: a gap of two pixels between them, slightly rounded.
      const gap = r.w > 5 && r.h > 5 ? 1 : 0;
      const inner = { x: r.x + gap, y: r.y + gap, w: r.w - 2 * gap, h: r.h - 2 * gap };
      const radius = Math.min(4, inner.w / 4, inner.h / 4);
      if (r.kind === "small") {
        // Many files too small to show one by one: one block, how many it holds.
        round(c, inner.x, inner.y, inner.w, inner.h, radius);
        c.fillStyle = mix(bg, dim, 0.32);
        c.fill();
        if (r.w >= 70 && r.h >= 34) label(c, inner, t("space.small_title"), tn("space.files", r.files, { n: num(r.files) }), mix(dim, fg, 0.4), font);
        continue;
      }
      if (r.kind === "mount") {
        // Another disk: not walked, not to scale; a click opens Space on it.
        const blue = theme.blue ?? "#8aa2d8";
        round(c, inner.x, inner.y, inner.w, inner.h, radius);
        c.fillStyle = mix(bg, blue, 0.3);
        c.fill();
        c.strokeStyle = blue;
        c.setLineDash([4, 3]);
        round(c, inner.x + 1.5, inner.y + 1.5, inner.w - 3, inner.h - 3, radius);
        c.stroke();
        c.setLineDash([]);
        if (r.name) label(c, inner, t("space.other_disk", { name: r.name }), "", mix(blue, fg, 0.6), font);
        continue;
      }
      // A soft light from above: the kind's colour, a little lighter at the top.
      const colour = colours[r.family] ?? dim;
      const [upper, lower] = dark ? [mix(bg, colour, 0.8), mix(bg, colour, 0.56)] : [mix(bg, colour, 0.5), mix(bg, colour, 0.74)];
      const g = c.createLinearGradient(0, inner.y, 0, inner.y + inner.h);
      g.addColorStop(0, upper);
      g.addColorStop(1, lower);
      round(c, inner.x, inner.y, inner.w, inner.h, radius);
      c.fillStyle = g;
      c.fill();
      if (r.name && !off) {
        const middle = mix(upper, lower, 0.5);
        const ink = luma(middle) > 140 ? mix(colour, "#000000", 0.82) : mix(colour, "#ffffff", 0.9);
        label(c, inner, r.name, size(r.size), ink, font);
      }
    }
    c.globalAlpha = 1;
    c.textAlign = "start";
    drawCursor();
  }

  /** The rectangle under the cursor framed in the accent colour, and the folders it is in lit. */
  function drawCursor() {
    if (!top || !view) return;
    const dpr = devicePixelRatio || 1;
    const c = top.getContext("2d");
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    c.clearRect(0, 0, w, h);
    const r = view.tiles[cursor];
    if (!r) return;
    const accent = theme.accent ?? "#ffffff";
    for (const j of chain(view.tiles, cursor)) {
      const f = view.tiles[j];
      c.globalAlpha = 0.14;
      c.fillStyle = accent;
      round(c, f.x, f.y, f.w, Math.min(f.h, 20), 5);
      c.fill();
      c.globalAlpha = 0.6;
      c.strokeStyle = accent;
      c.lineWidth = 1;
      round(c, f.x + 0.5, f.y + 0.5, f.w - 1, f.h - 1, 5);
      c.stroke();
    }
    c.globalAlpha = 1;
    c.fillStyle = "rgba(255,255,255,0.14)";
    round(c, r.x + 1, r.y + 1, r.w - 2, r.h - 2, 4);
    c.fill();
    c.lineWidth = 2;
    c.strokeStyle = accent;
    round(c, r.x + 1, r.y + 1, Math.max(0, r.w - 2), Math.max(0, r.h - 2), 4);
    c.stroke();
  }

  $effect(() => {
    view;
    hl;
    w;
    h;
    colours;
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
        // Stopped (Stop on a file operation before it, while this waited its turn): nothing went.
        const stopped = await invoke("delete", { paths: [path], forever: gone });
        if (stopped) return void (ui.status = stopped);
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

  /** A share in per cent: one decimal under 10 %, and "<0.1 %" rather than a 0 for a sliver. */
  const share = (s) => (s > 0 && s < 0.001 ? `<${num(0.1, { minimumFractionDigits: 1 })} %` : `${num(s * 100, { maximumFractionDigits: s < 0.1 ? 1 : 0 })} %`);

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
            <b>{info.tile.name && info.tile.name !== basename(info.path) ? info.tile.name : basename(info.path)}</b>
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
                <span class="swatch" style:background={colours[r.family]}></span>
                <span class="ext">{r.ext ? `.${r.ext}` : t("space.no_ext")}</span>
                <span class="fam">{t(`space.family_${r.family}`)}</span>
                <span class="num">{size(r.bytes)}</span>
                <span class="share"><span style:width={r.share > 0 ? `max(2px, ${r.share * 100}%)` : "0"} style:background={colours[r.family]}></span></span>
                <span class="num pc">{share(r.share)}</span>
              </button>
            </li>
          {/each}
        {:else}
          {#each view?.largest ?? [] as [id, bytes, path, model], i (id)}
            <li>
              <button class="big" class:cur={d.list && i === row} title={`${path}\n${t("space.largest_tip")}`} onmouseenter={() => (cursor = view.tiles.findIndex((x) => x.id === id))} onclick={() => go(path)}>
                <span class="num">{size(bytes)}</span>
                <span class="path"><bdi>{model ?? relative(path)}</bdi></span>
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
        <!-- A model store's blob: the model it is first (space::Tree::label). -->
        {#if info.tile.name && info.tile.name !== basename(info.path)}{info.tile.name} · {/if}<bdi>{shown(info.path)}</bdi> · {size(info.tile.size)}{info.tile.kind === "file" ? ` · ${kindOf(info.tile)}` : ""}
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
  .crumb:not(.here):hover {
    color: var(--accent-bg);
    text-decoration: underline;
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
    transform-origin: 0 0;
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

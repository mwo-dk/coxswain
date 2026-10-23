<script module>
  /** Views open per file: the last one to close lets the file go (`go_close`). */
  const open = new Map();
</script>

<script>
  // Go (docs/previews/go.md): an SGF file's games as a list, a game as a board of stones with
  // the move, captures, variations and comments beside it. In the preview pane, and
  // full-window (`full`) from its ⤢ button and Alt+K. A file of one game opens on its board.
  // Reading and playing out are the Rust side's (go.rs); every string from the file is shown as
  // text, never HTML.
  import { untrack } from "svelte";
  import { ui } from "./app.svelte.js";
  import { invoke, basename } from "./lib.js";
  import { t } from "./i18n.svelte.js";

  let { path, full = false } = $props();

  let looks = $state(null);
  let error = $state("");
  let games = $state([]);
  let capped = $state(false);
  let loaded = $state(false);
  let cursor = $state(0);
  let listEl = $state();
  // The game on the board: its index, facts and tree; the node shown and its position.
  let game = $state(null);
  let at = $state(0);
  let pos = $state(null);
  let boardEl = $state();

  $effect(() => {
    const theme = ui.theme;
    void ui.cfg;
    invoke("chess_looks", { theme }).then((l) => (looks = l));
  });

  $effect(() => {
    const p = path;
    open.set(p, (open.get(p) ?? 0) + 1);
    return () => {
      const n = (open.get(p) ?? 1) - 1;
      if (n > 0) open.set(p, n);
      else (open.delete(p), invoke("go_close", { path: p }).catch(() => {}));
    };
  });

  // A new file: its games, and a game of its own at once when it has only one.
  $effect(() => {
    const p = path;
    untrack(() => {
      game = null;
      pos = null;
      error = "";
      cursor = 0;
      loaded = false;
      invoke("go_games", { path: p }).then(
        (r) => {
          if (p !== path) return;
          games = r.games;
          capped = r.capped;
          loaded = true;
          if (r.games.length === 1 && !ui.goAt) openGame(0);
        },
        (e) => (error = String(e)),
      );
    });
  });

  $effect(() => {
    if (full && listEl) listEl.focus();
  });

  // A Find hit: that game, at once.
  $effect(() => {
    const want = ui.goAt;
    if (want && want.path === path) {
      ui.goAt = null;
      untrack(() => openGame(want.index));
    }
  });

  async function openGame(index) {
    try {
      const v = await invoke("go_game", { path, index });
      game = { ...v, index };
      cursor = index;
      error = v.note ?? "";
      await show(0);
      setTimeout(() => boardEl?.focus(), 0);
    } catch (e) {
      error = String(e);
    }
  }

  let seq = 0;
  async function show(node) {
    if (!game) return;
    const s = ++seq;
    const p = await invoke("go_position", { path, index: game.index, node });
    if (s !== seq) return;
    at = node;
    pos = p;
  }

  function back() {
    game = null;
    pos = null;
    setTimeout(() => listEl?.focus(), 0);
  }

  const nodes = $derived(game?.nodes ?? []);
  const node = $derived(nodes[at]);
  const siblings = $derived(node && node.parent != null ? nodes[node.parent].children : [at]);

  function step(to) {
    if (!game || !node) return;
    let n = at;
    if (to === "next") n = node.children[0] ?? at;
    else if (to === "prev") n = node.parent ?? at;
    else if (to === "start") n = 0;
    else if (to === "end") while (nodes[n].children.length) n = nodes[n].children[0];
    else if (to === "down" || to === "up") {
      const i = siblings.indexOf(at);
      n = siblings[Math.max(0, Math.min(siblings.length - 1, i + (to === "down" ? 1 : -1)))];
    }
    show(n);
  }

  function nextGame(by) {
    const i = game.index + by;
    if (i >= 0 && i < games.length) openGame(i);
  }

  function onBoardKey(e) {
    const map = { ArrowRight: "next", ArrowLeft: "prev", Home: "start", End: "end", ArrowDown: "down", ArrowUp: "up" };
    if (map[e.key]) step(map[e.key]);
    else if (e.key === "PageDown") nextGame(1);
    else if (e.key === "PageUp") nextGame(-1);
    else if (e.key === "Escape" && games.length > 1) back();
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function onListKey(e) {
    const n = games.length;
    const go = (i) => {
      cursor = Math.max(0, Math.min(n - 1, i));
      listEl.querySelector(`tr[data-i="${cursor}"]`)?.scrollIntoView({ block: "nearest" });
    };
    if (e.key === "ArrowDown") go(cursor + 1);
    else if (e.key === "ArrowUp") go(cursor - 1);
    else if (e.key === "PageDown") go(cursor + 20);
    else if (e.key === "PageUp") go(cursor - 20);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(n - 1);
    else if (e.key === "Enter" && n) openGame(cursor);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  const info = $derived(game?.info);
  const players = (i) => [i.black + (i.black_rank ? ` ${i.black_rank}` : ""), i.white + (i.white_rank ? ` ${i.white_rank}` : "")].join(" – ");
  const letter = (k) => String.fromCharCode(65 + (k % 26));
  // Wood from the theme's light square, the lines from its dark one.
  const wood = $derived(looks ? looks.colours[0] : "#dcb35c");
  const ink = $derived(looks ? looks.colours[1] : "#5a4020");
</script>

<div class="go" class:full role={full ? "dialog" : undefined} aria-modal={full ? "true" : undefined} aria-label={full ? basename(path) : undefined}>
  {#if full}
    <header class="top">
      <h2>● {basename(path)}</h2>
      <button class="x" title={t("chess.close")} onclick={() => (ui.modal = null)}>×</button>
    </header>
  {/if}

  {#if error}<p class="note">{error}</p>{/if}

  {#if game && pos}
    <div class="game">
      <div class="side">
        <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
        <svg class="board chess-keys" viewBox="-1.5 -1.5 {pos.width + 2} {pos.height + 2}" tabindex="0" role="img" aria-label={t("go.board")} bind:this={boardEl} onkeydown={onBoardKey}>
          <rect x="-1.5" y="-1.5" width={pos.width + 2} height={pos.height + 2} fill={wood} />
          {#each Array(pos.width) as _, x (x)}
            <line x1={x} y1="0" x2={x} y2={pos.height - 1} stroke={ink} stroke-width={x === 0 || x === pos.width - 1 ? 0.07 : 0.045} />
            <text x={x} y="-0.75" class="coord" fill={ink} text-anchor="middle">{pos.columns[x]}</text>
            <text x={x} y={pos.height - 1 + 0.95} class="coord" fill={ink} text-anchor="middle">{pos.columns[x]}</text>
          {/each}
          {#each Array(pos.height) as _, y (y)}
            <line x1="0" y1={y} x2={pos.width - 1} y2={y} stroke={ink} stroke-width={y === 0 || y === pos.height - 1 ? 0.07 : 0.045} />
            <text x="-0.9" y={y + 0.16} class="coord" fill={ink} text-anchor="middle">{pos.height - y}</text>
            <text x={pos.width - 1 + 0.9} y={y + 0.16} class="coord" fill={ink} text-anchor="middle">{pos.height - y}</text>
          {/each}
          {#each pos.stars as [x, y] (`${x},${y}`)}<circle cx={x} cy={y} r="0.1" fill={ink} />{/each}
          {#each pos.stones as s, i (i)}
            {#if s}
              {@const x = i % pos.width}
              {@const y = Math.floor(i / pos.width)}
              <image href={`vendor/game-pieces/go-${s === 1 ? "b" : "w"}.svg`} x={x - 0.5} y={y - 0.5} width="1" height="1" />
              {#if pos.last && pos.last[0] === x && pos.last[1] === y}<circle cx={x} cy={y} r="0.2" fill="none" stroke={s === 1 ? "#f4f4f0" : "#141414"} stroke-width="0.06" />{/if}
            {/if}
          {/each}
          {#each pos.branches as [c], k (c)}
            {@const p = nodes[c]?.mv?.at}
            {#if p}
              <circle cx={p[0]} cy={p[1]} r="0.38" fill={wood} />
              <text x={p[0]} y={p[1] + 0.17} class="mark" text-anchor="middle" fill="#b00000">{letter(k)}</text>
            {/if}
          {/each}
        </svg>
        <div class="bar">
          {#if games.length > 1}<button title={t("chess.back_tip")} onclick={back}>{t("chess.back")}</button>{/if}
          <button class="icon" title={t("chess.start_tip")} aria-label={t("chess.start_tip")} onclick={() => step("start")}>|◀</button>
          <button class="icon" title={t("chess.prev_tip")} aria-label={t("chess.prev_tip")} onclick={() => step("prev")}>◀</button>
          <button class="icon" title={t("chess.next_tip")} aria-label={t("chess.next_tip")} onclick={() => step("next")}>▶</button>
          <button class="icon" title={t("chess.end_tip")} aria-label={t("chess.end_tip")} onclick={() => step("end")}>▶|</button>
          {#if !full}<button class="icon-btn" title={t("chess.window")} onclick={() => (ui.modal = { kind: "go", path })}>{"\u{f065}"}</button>{/if}
        </div>
      </div>
      <div class="moves">
        {#if info.black || info.white || info.result}<p class="players"><b>{info.black || info.white ? players(info) : ""}</b> {info.result}</p>{/if}
        <p class="meta">{[info.name, info.event, info.round, info.place, info.date].filter(Boolean).join(" · ")}</p>
        <p class="meta">{[`${t("go.col_size")} ${pos.width === pos.height ? pos.width : `${pos.width}×${pos.height}`}`, info.komi && `${t("go.col_komi")} ${info.komi}`, info.handicap && info.handicap !== "0" && `${t("go.col_handicap")} ${info.handicap}`, info.rules].filter(Boolean).join(" · ")}</p>
        <p class="meta">{t("chess.game_of", { n: game.index + 1, total: games.length })}</p>
        <p class="now">{pos.text ? `${pos.text}  (${t("go.move_of", { n: pos.number, total: pos.total })})` : t("go.start")}</p>
        <p class="meta">{pos.black_next ? t("chess.black_to_move") : t("chess.white_to_move")}</p>
        <p>{t("go.captures", { black: pos.captures[0], white: pos.captures[1] })}</p>
        {#if siblings.length > 1}<p>{t("go.variation_of", { n: siblings.indexOf(at) + 1, total: siblings.length })}</p>{/if}
        {#if pos.branches.length}
          <p>
            {t("go.branches", { list: "" })}
            {#each pos.branches as [c, text], k (c)}<button class="mv" onclick={() => show(c)}>{letter(k)} {text}</button>{" "}{/each}
          </p>
        {/if}
        {#if node?.name}<p><b>{node.name}</b></p>{/if}
        {#if node?.comment}<p class="comment">{node.comment}</p>{/if}
        <p class="meta keys">{t("go.keys_game_gui")}</p>
      </div>
    </div>
  {:else}
    <div class="bar">
      {#if !full}<button class="icon-btn" title={t("chess.window")} onclick={() => (ui.modal = { kind: "go", path })}>{"\u{f065}"}</button>{/if}
    </div>
    <p class="meta">{t("chess.games", { shown: games.length, total: games.length })}{#if capped} · {t("chess.capped", { n: games.length })}{/if}</p>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="list chess-keys" tabindex="0" role="grid" aria-label={basename(path)} bind:this={listEl} onkeydown={onListKey}>
      <table>
        <thead>
          <tr>
            <th>#</th><th>{t("chess.col_black")}</th><th>{t("go.col_rank")}</th><th>{t("chess.col_white")}</th><th>{t("go.col_rank")}</th><th>{t("chess.col_result")}</th><th>{t("chess.col_event")}</th><th>{t("chess.col_date")}</th><th>{t("go.col_size")}</th><th>{t("go.col_komi")}</th><th>{t("go.col_handicap")}</th><th>{t("chess.col_moves")}</th>
          </tr>
        </thead>
        <tbody>
          {#each games as g, i (i)}
            <tr data-i={i} class:cursor={i === cursor} onclick={() => (cursor = i)} ondblclick={() => openGame(i)}>
              <td class="n">{g.n}</td>
              <td>{g.black}</td>
              <td>{g.black_rank}</td>
              <td>{g.white}</td>
              <td>{g.white_rank}</td>
              <td>{g.result}</td>
              <td>{[g.event, g.name].filter(Boolean).join(" · ")}</td>
              <td>{g.date}</td>
              <td>{g.width === g.height ? g.width : `${g.width}×${g.height}`}</td>
              <td>{g.komi}</td>
              <td>{g.handicap}</td>
              <td class="n">{g.moves}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if loaded && !games.length}<p class="meta">{t("go.no_games")}</p>{/if}
    </div>
    <p class="meta keys">{t("go.keys_list_gui")}</p>
  {/if}
</div>

<style>
  .go {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    gap: 6px;
    padding: 8px 12px;
  }
  .go.full {
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
  }
  .x {
    font-size: 1.3em;
    padding: 0 6px;
  }
  .note {
    margin: 0;
    color: var(--git-conflict-fg, #e53935);
  }
  p {
    margin: 0 0 4px;
  }
  .meta {
    font-size: 0.85em;
    opacity: 0.75;
  }
  .list {
    flex: 1;
    min-height: 120px;
    overflow: auto;
    outline: none;
  }
  .list:focus-visible {
    box-shadow: inset 0 0 0 1px var(--accent-bg);
  }
  table {
    border-collapse: collapse;
    font-size: 0.88em;
    white-space: nowrap;
  }
  thead th {
    position: sticky;
    top: 0;
    background: var(--dialog-bg, var(--panel-bg));
    text-align: start;
    padding: 0 6px;
  }
  tbody tr {
    height: 24px;
  }
  td {
    padding: 0 6px;
    max-width: 16em;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  td.n {
    text-align: end;
    font-variant-numeric: tabular-nums;
  }
  tr.cursor {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .game {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    min-height: 0;
    flex: 1;
    overflow: auto;
  }
  .side {
    flex: 0 1 min(100%, 440px);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .full .side {
    flex-basis: min(100%, 64vh);
  }
  .board {
    width: 100%;
    aspect-ratio: 1;
    outline: none;
    border-radius: var(--r);
  }
  .board:focus-visible {
    box-shadow: 0 0 0 2px var(--accent-bg);
  }
  .coord {
    font-size: 0.48px;
    font-family: var(--font);
  }
  .mark {
    font-size: 0.5px;
    font-family: var(--font);
    font-weight: bold;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .bar button {
    padding: 2px 8px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
  }
  .icon-btn {
    font-family: var(--icon-font);
  }
  .moves {
    flex: 1 1 260px;
    min-width: 0;
    overflow: auto;
  }
  .now {
    font-weight: bold;
  }
  .mv {
    font: inherit;
    padding: 0 4px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r-sm);
    background: none;
    box-shadow: none;
    color: inherit;
    cursor: pointer;
  }
  .mv:hover {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .comment {
    font-style: italic;
    white-space: pre-wrap;
    opacity: 0.85;
  }
  .keys {
    margin-top: 6px;
  }
</style>

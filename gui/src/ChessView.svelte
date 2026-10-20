<script module>
  /** Views open per file: the last one to close lets the file go (`chess_close`). */
  const open = new Map();
</script>

<script>
  // Chess (docs/previews/chess.md): a PGN file's games as a table with a filter, filling while
  // the file is read; a game as a board with its moves, variations and comments; an EPD or FEN
  // file's positions. In the preview pane, and full-window (`full`) from its ⤢ button and
  // Alt+K. Reading and playing out are the Rust side's (chess.rs); every string from the file
  // is shown as text, never HTML.
  import { untrack } from "svelte";
  import { ui } from "./app.svelte.js";
  import { invoke, basename, chessFormat } from "./lib.js";
  import { t } from "./i18n.svelte.js";

  let { path, full = false } = $props();

  // A .bin file comes here only when it is a Polyglot opening book (Preview, chess_is_book).
  const format = $derived(/\.bin$/i.test(path) ? "book" : chessFormat(basename(path)));
  const START = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
  const ROW = 24;
  const COLUMNS = [
    ["number", "#"],
    ["white", "chess.col_white"],
    ["white_elo", "chess.col_elo"],
    ["black", "chess.col_black"],
    ["black_elo", "chess.col_elo"],
    ["result", "chess.col_result"],
    ["date", "chess.col_date"],
    ["event", "chess.col_event"],
    ["eco", "ECO"],
    ["opening", "chess.col_opening"],
    ["moves", "chess.col_moves"],
  ];

  let looks = $state(null);
  let letters = $state(false);
  let flip = $state(false);
  let error = $state("");
  // The table.
  let query = $state({ player: "", opening: "", result: "", from: null, to: null, elo: null, sort: "number", desc: false });
  let total = $state(0);
  let progress = $state(null);
  let rows = $state([]);
  let first = $state(0);
  let cursor = $state(0);
  let listEl = $state();
  let height = $state(400);
  // The game or position on the board.
  let game = $state(null);
  let at = $state(null);
  let positions = $state([]);
  let posAt = $state(0);
  let boardEl = $state();
  let movesEl = $state();
  // The opening book's moves for the position shown (chess_book), and in a book of its own
  // the positions before the one shown.
  let book = $state(null);
  let bookLine = $state([]);

  $effect(() => {
    const theme = ui.theme;
    // Settings saved: the board and notation as they say now.
    void ui.cfg;
    invoke("chess_looks", { theme }).then((l) => {
      looks = l;
      letters = l.notation === "letters";
    });
  });

  // A view of the file: counted, and the last one to go lets it go.
  $effect(() => {
    const p = path;
    open.set(p, (open.get(p) ?? 0) + 1);
    return () => {
      const n = (open.get(p) ?? 1) - 1;
      if (n > 0) open.set(p, n);
      else (open.delete(p), invoke("chess_close", { path: p }).catch(() => {}));
    };
  });

  // A new file: back to its table (or its first position).
  $effect(() => {
    const p = path;
    untrack(() => {
      game = null;
      at = null;
      error = "";
      cursor = 0;
      first = 0;
      if (listEl) listEl.scrollTop = 0;
      bookLine = [];
      if (/\.bin$/i.test(p)) {
        game = { game: { start: START, nodes: [], roots: [], tags: [], result: "" }, layout: [], index: 0 };
      } else if (chessFormat(basename(p)) === "epd") {
        invoke("chess_positions", { path: p }).then(
          (v) => p === path && ((positions = v), showPosition(0)),
          (e) => (error = String(e)),
        );
      }
    });
  });

  // The window's table takes the keys when it opens (Alt+K), as the other list dialogs do; in
  // the preview pane the panels keep them until it is clicked.
  $effect(() => {
    if (full && listEl) listEl.focus();
  });

  /** Back from a game to the table, which takes the keys. */
  function back() {
    game = null;
    setTimeout(() => listEl?.focus(), 0);
  }

  // A Find hit: that game, at once.
  $effect(() => {
    const want = ui.chessAt;
    if (want && want.path === path) {
      ui.chessAt = null;
      untrack(() => openGame(want.index));
    }
  });

  let rowsSeq = 0;
  async function fetchRows() {
    if (format !== "pgn") return;
    const p = path;
    const seq = ++rowsSeq;
    const n = Math.ceil(height / ROW) + 20;
    const from = Math.max(0, Math.floor((listEl?.scrollTop ?? 0) / ROW) - 10);
    try {
      const r = await invoke("chess_rows", { path: p, query: $state.snapshot(query), offset: from, limit: n });
      // An answer to an older scroll or filter that came in late: a newer one is on its way.
      if (p !== path || seq !== rowsSeq) return;
      progress = r.progress;
      total = r.total;
      rows = r.rows;
      first = from;
      error = r.progress.error ?? "";
    } catch (e) {
      if (seq === rowsSeq) error = String(e);
    }
  }

  // The table, again when the filter changes, and twice a second while the file is read.
  $effect(() => {
    void path;
    void JSON.stringify(query);
    untrack(fetchRows);
    const timer = setInterval(() => {
      if (!progress?.done) fetchRows();
    }, 500);
    return () => clearInterval(timer);
  });

  const visible = $derived(rows);

  // One request a frame, however many scroll events it brings.
  let scrollFrame = 0;
  function onscroll() {
    if (!scrollFrame) scrollFrame = requestAnimationFrame(() => ((scrollFrame = 0), fetchRows()));
  }

  function sortBy(col) {
    if (query.sort === col) query.desc = !query.desc;
    else (query.sort = col), (query.desc = false);
  }

  async function openGame(index) {
    try {
      const v = await invoke("chess_game", { path, index });
      game = { ...v, index };
      at = null;
      error = v.error ?? "";
      setTimeout(() => boardEl?.focus(), 0);
    } catch (e) {
      error = String(e);
    }
  }

  function showPosition(i) {
    const p = positions[i];
    if (!p) return;
    posAt = i;
    game = { game: { start: p.fen ?? "", nodes: [], roots: [], tags: [], result: "" }, layout: p.about ? [{ t: "comment", text: p.about }] : [], index: i };
    at = null;
    error = p.fen ? "" : t("chess.no_position", { line: p.line });
  }

  const nodes = $derived(game?.game.nodes ?? []);
  const fen = $derived(at == null ? game?.game.start : nodes[at]?.fen);
  const last = $derived(at == null ? null : nodes[at]);
  const pieces = $derived(looks ? (letters ? looks.letters : looks.figurines) : ["♔", "♕", "♖", "♗", "♘"]);

  /** The pieces on the board: 64 squares, a1 first. */
  function squares(f) {
    const out = Array(64).fill(null);
    const rows = (f ?? "").split(" ")[0].split("/");
    rows.slice(0, 8).forEach((row, r) => {
      let file = 0;
      for (const c of row) {
        if (/\d/.test(c)) file += Number(c);
        else if (file < 8) out[(7 - r) * 8 + file++] = c;
      }
    });
    return out;
  }
  const board = $derived(squares(fen));

  // The book's moves, again for each position.
  let bookSeq = 0;
  $effect(() => {
    const f = fen;
    const p = path;
    void ui.cfg;
    const s = ++bookSeq;
    if (!f) return void (book = null);
    invoke("chess_book", { path: p, fen: f }).then(
      (b) => s === bookSeq && (book = b),
      () => {},
    );
  });

  /** A book's move played: its position on the board, the one before kept for ←. */
  function playBook(m) {
    bookLine.push(game.game.start);
    game.game.start = m.fen;
  }

  /** A move as shown, in pieces: [text, figurine] (`chess::show_san`). */
  function show(san) {
    const out = [];
    [...san].forEach((c, i) => {
      const k = "KQRBN".indexOf(c);
      if (k >= 0) out.push([pieces[k], !letters]);
      else if (!(c === "P" && i === 0)) out.push([c, false]);
    });
    return out;
  }
  const NAG = { 1: "!", 2: "?", 3: "!!", 4: "??", 5: "!?", 6: "?!", 7: "□", 10: "=", 11: "=", 12: "=", 13: "∞", 14: "⩲", 15: "⩱", 16: "±", 17: "∓", 18: "+−", 19: "−+", 20: "+−", 21: "−+", 22: "⨀", 23: "⨀", 32: "⟳", 33: "⟳", 36: "↑", 37: "↑", 40: "→", 41: "→", 44: "=∞", 45: "=∞", 132: "⇆", 133: "⇆", 138: "⊕", 139: "⊕", 140: "∆", 142: "⌓", 145: "RR", 146: "N" };
  const glued = (nags) => nags.filter((n) => n <= 6).map((n) => NAG[n]).join("");
  const apart = (nags) => nags.filter((n) => n > 6).map((n) => NAG[n] ?? `$${n}`).join(" ");

  function step(to) {
    if (!game) return;
    const g = game.game;
    const sibs = (n) => (nodes[n].parent == null ? g.roots : nodes[nodes[n].parent].children);
    if (format === "book") {
      if (to === "prev" && bookLine.length) game.game.start = bookLine.pop();
      else if (to === "start" && bookLine.length) (game.game.start = bookLine[0]), (bookLine = []);
      else if (to === "next" && book?.moves.length) playBook(book.moves[0]);
      return;
    }
    if (format === "epd") {
      if (to === "next") showPosition(Math.min(positions.length - 1, posAt + 1));
      else if (to === "prev") showPosition(Math.max(0, posAt - 1));
      else if (to === "start") showPosition(0);
      else if (to === "end") showPosition(positions.length - 1);
      return;
    }
    if (to === "next") at = at == null ? (g.roots[0] ?? null) : (nodes[at].children[0] ?? at);
    else if (to === "prev") at = at == null ? null : nodes[at].parent;
    else if (to === "start") at = null;
    else if (to === "end") {
      let n = at ?? g.roots[0];
      while (n != null && nodes[n].children.length) n = nodes[n].children[0];
      at = n ?? null;
    } else if ((to === "down" || to === "up") && at != null) {
      const s = sibs(at);
      const i = s.indexOf(at);
      at = s[Math.max(0, Math.min(s.length - 1, i + (to === "down" ? 1 : -1)))];
    }
    setTimeout(() => movesEl?.querySelector(".here")?.scrollIntoView({ block: "nearest" }), 0);
  }

  function nextGame(by) {
    const i = rows.findIndex((r) => r[0] === game.index);
    const r = rows[i + by];
    if (r) openGame(r[0]);
  }

  function onBoardKey(e) {
    const map = { ArrowRight: "next", ArrowLeft: "prev", Home: "start", End: "end", ArrowDown: "down", ArrowUp: "up" };
    if (map[e.key]) step(map[e.key]);
    else if (e.key === "f") flip = !flip;
    else if (e.key === "n") letters = !letters;
    else if (e.key === "PageDown" && format === "pgn") nextGame(1);
    else if (e.key === "PageUp" && format === "pgn") nextGame(-1);
    else if (e.key === "Escape" && format === "pgn" && game) back();
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function onListKey(e) {
    const n = total;
    const go = (i) => {
      cursor = Math.max(0, Math.min(n - 1, i));
      const top = cursor * ROW;
      if (top < listEl.scrollTop) listEl.scrollTop = top;
      else if (top + ROW > listEl.scrollTop + listEl.clientHeight - ROW) listEl.scrollTop = top + 2 * ROW - listEl.clientHeight;
    };
    const page = Math.floor(height / ROW) - 2;
    if (e.key === "ArrowDown") go(cursor + 1);
    else if (e.key === "ArrowUp") go(cursor - 1);
    else if (e.key === "PageDown") go(cursor + page);
    else if (e.key === "PageUp") go(cursor - page);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(n - 1);
    else if (e.key === "Enter") {
      const r = rows[cursor - first];
      if (r) openGame(r[0]);
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }

  const tag = (k) => game?.game.tags.find(([n]) => n.toLowerCase() === k.toLowerCase())?.[1] ?? "";
  const known = (v) => (v && v !== "?" && !v.startsWith("????") ? v : "");
  const PIECE = { k: "K", q: "Q", r: "R", b: "B", n: "N", p: "P" };
  const num = (v) => (v === "" || v == null ? null : Number(v) || null);
  const pct = $derived(progress && progress.size ? Math.floor((progress.read * 100) / progress.size) : 0);
</script>

<!-- One square: its colour, the last move's tint, a piece, and the coordinates on the edge. -->
{#snippet square(i)}
  {@const file = flip ? 7 - (i % 8) : i % 8}
  {@const rank = flip ? Math.floor(i / 8) : 7 - Math.floor(i / 8)}
  {@const sq = rank * 8 + file}
  {@const p = board[sq]}
  {@const lit = last && (last.from === sq || last.to === sq)}
  {@const x = i % 8}
  {@const y = Math.floor(i / 8)}
  <rect {x} {y} width="1" height="1" fill={looks ? looks.colours[(rank + file) % 2 === 0 ? 1 : 0] : "#888"} />
  {#if lit}<rect {x} {y} width="1" height="1" fill="rgb(246 224 90 / 0.45)" />{/if}
  {#if p}<image href={`vendor/chess-pieces/${p === p.toUpperCase() ? "w" : "b"}${PIECE[p.toLowerCase()]}.svg`} x={x + 0.04} y={y + 0.04} width="0.92" height="0.92" />{/if}
  {#if y === 7}<text x={x + 0.94} y={y + 0.95} class="coord" text-anchor="end" fill={looks ? looks.colours[(rank + file) % 2 === 0 ? 0 : 1] : "#000"}>{"abcdefgh"[file]}</text>{/if}
  {#if x === 0}<text x={x + 0.05} y={y + 0.22} class="coord" fill={looks ? looks.colours[(rank + file) % 2 === 0 ? 0 : 1] : "#000"}>{rank + 1}</text>{/if}
{/snippet}

<div class="chess" class:full role={full ? "dialog" : undefined} aria-modal={full ? "true" : undefined} aria-label={full ? basename(path) : undefined}>
  {#if full}
    <header class="top">
      <h2>♞ {basename(path)}</h2>
      <button class="x" title={t("chess.close")} onclick={() => (ui.modal = null)}>×</button>
    </header>
  {/if}

  {#if error}<p class="note">{error}</p>{/if}

  {#if game}
    <div class="game">
      <div class="side">
        <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
        <svg class="board chess-keys" viewBox="0 0 8 8" tabindex="0" role="img" aria-label={t("chess.board")} bind:this={boardEl} onkeydown={onBoardKey}>
          {#each Array(64) as _, i (i)}{@render square(i)}{/each}
        </svg>
        <div class="bar">
          {#if format === "pgn"}<button title={t("chess.back_tip")} onclick={back}>{t("chess.back")}</button>{/if}
          <button class="icon" title={t("chess.start_tip")} aria-label={t("chess.start_tip")} onclick={() => step("start")}>|◀</button>
          <button class="icon" title={t("chess.prev_tip")} aria-label={t("chess.prev_tip")} onclick={() => step("prev")}>◀</button>
          <button class="icon" title={t("chess.next_tip")} aria-label={t("chess.next_tip")} onclick={() => step("next")}>▶</button>
          <button class="icon" title={t("chess.end_tip")} aria-label={t("chess.end_tip")} onclick={() => step("end")}>▶|</button>
          <button title={t("chess.flip_tip")} class:on={flip} onclick={() => (flip = !flip)}>{t("chess.flip")}</button>
          <button title={t("chess.notation_tip")} onclick={() => (letters = !letters)}>{#if letters}{pieces.join("")}{:else}<span class="fig">♔♘</span>{/if}</button>
          {#if !full}<button class="icon-btn" title={t("chess.window")} onclick={() => (ui.modal = { kind: "chess", path })}>{"\u{f065}"}</button>{/if}
        </div>
        {#if fen}<p class="meta">{fen.split(" ")[1] === "b" ? t("chess.black_to_move") : t("chess.white_to_move")}</p>{/if}
      </div>
      <div class="moves" bind:this={movesEl}>
        {#if format === "pgn"}
          <p class="players"><b>{known(tag("White"))}{known(tag("WhiteElo")) ? ` (${tag("WhiteElo")})` : ""} – {known(tag("Black"))}{known(tag("BlackElo")) ? ` (${tag("BlackElo")})` : ""}</b> {game.game.result}</p>
          <p class="meta">{[known(tag("Event")), known(tag("Site")), known(tag("Date")), [tag("ECO"), tag("Opening")].join(" ").trim()].filter(Boolean).join(" · ")}</p>
          <p class="meta">{t("chess.game_of", { n: game.index + 1, total: progress?.games ?? "" })}</p>
        {:else if format === "book"}
          <p class="players"><b>{t("chess.book_entries", { n: book?.entries ?? "" })}</b></p>
        {:else}
          <p class="players"><b>{t("chess.position_of", { n: posAt + 1, total: positions.length })}</b></p>
        {/if}
        {#if book && format === "book"}
          {#if !book.moves.length}<p class="meta">{t("chess.book_none")}</p>{/if}
          <table class="book">
            <tbody>
              {#each book.moves as m (m.uci)}
                <tr>
                  <td><button class="mv" title={t("chess.book_play_tip")} onclick={() => playBook(m)}>{#each show(m.san) as [text, fig], j (j)}{#if fig}<span class="fig">{text}</span>{:else}{text}{/if}{/each}</button></td>
                  <td class="n">{m.share.toFixed(1)} %</td>
                  <td class="n">{m.weight}</td>
                  <td class="meta">{m.learn ? t("chess.book_learn", { n: m.learn }) : ""}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {:else if book}
          <p class="meta book-line">{#if book.moves.length}{t("chess.book", { name: book.name, moves: "" })}{#each book.moves.slice(0, 8) as m, k (m.uci)}{k ? " · " : ""}{#each show(m.san) as [text, fig], j (j)}{#if fig}<span class="fig">{text}</span>{:else}{text}{/if}{/each} {m.share.toFixed(0)} %{/each}{:else}{t("chess.book_out", { name: book.name })}{/if}</p>
        {/if}
        <p class="text">
          {#each game.layout as tok, i (i)}
            {#if tok.t === "move"}
              {@const n = nodes[tok.node]}
              <button class="mv" class:here={at === tok.node} class:var={n.depth > 0} onclick={() => (at = tok.node)}>{tok.number ?? ""}{#each show(n.san) as [text, fig], j (j)}{#if fig}<span class="fig">{text}</span>{:else}{text}{/if}{/each}{glued(n.nags)}</button>{#if apart(n.nags)}<span class="nag">{apart(n.nags)}</span>{/if}{" "}
            {:else if tok.t === "comment"}
              <span class="comment">{tok.text}</span>{" "}
            {:else if tok.t === "open"}
              <span class="paren">(</span>
            {:else if tok.t === "close"}
              <span class="paren">)</span>{" "}
            {:else if tok.t === "result"}
              <b>{tok.text}</b>
            {/if}
          {/each}
        </p>
        <p class="meta keys">{format === "pgn" ? t("chess.keys_game_gui") : format === "book" ? t("chess.keys_book_gui") : t("chess.keys_position_gui")}</p>
      </div>
    </div>
  {:else if format === "pgn"}
    <div class="filters">
      <input type="search" placeholder={t("chess.filter_player")} aria-label={t("chess.filter_player")} bind:value={query.player} />
      <input type="search" placeholder={t("chess.filter_opening")} aria-label={t("chess.filter_opening")} bind:value={query.opening} />
      <select aria-label={t("chess.col_result")} bind:value={query.result}>
        <option value="">{t("chess.result_any")}</option>
        <option value="1-0">1-0</option>
        <option value="0-1">0-1</option>
        <option value="1/2-1/2">½-½</option>
      </select>
      <input class="num" type="number" placeholder={t("chess.year_from")} aria-label={t("chess.year_from")} value={query.from ?? ""} onchange={(e) => (query.from = num(e.currentTarget.value))} />
      <input class="num" type="number" placeholder={t("chess.year_to")} aria-label={t("chess.year_to")} value={query.to ?? ""} onchange={(e) => (query.to = num(e.currentTarget.value))} />
      <input class="num" type="number" placeholder={t("chess.filter_elo")} aria-label={t("chess.filter_elo")} value={query.elo ?? ""} onchange={(e) => (query.elo = num(e.currentTarget.value))} />
      {#if !full}<button class="icon-btn" title={t("chess.window")} onclick={() => (ui.modal = { kind: "chess", path })}>{"\u{f065}"}</button>{/if}
    </div>
    <p class="meta">
      {t("chess.games", { shown: total, total: progress?.games ?? 0 })}{#if progress && !progress.done} · {t("chess.reading", { percent: pct })}{/if}{#if progress?.capped} · {t("chess.capped", { n: progress.games })}{/if}
    </p>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="list chess-keys" tabindex="0" role="grid" aria-label={basename(path)} bind:this={listEl} bind:clientHeight={height} {onscroll} onkeydown={onListKey}>
      <table>
        <thead>
          <tr>
            {#each COLUMNS as [col, label] (col)}
              <th class={col}><button onclick={() => sortBy(col)}>{label.includes(".") ? t(label) : label}{query.sort === col ? (query.desc ? " ↓" : " ↑") : ""}</button></th>
            {/each}
          </tr>
        </thead>
        <tbody>
          <tr style:height="{first * ROW}px"></tr>
          {#each visible as [index, r], k (index)}
            <tr class:cursor={first + k === cursor} onclick={() => (cursor = first + k)} ondblclick={() => openGame(index)}>
              <td class="number">{r.n}</td>
              <td>{r.white}</td>
              <td class="n">{r.white_elo ?? ""}</td>
              <td>{r.black}</td>
              <td class="n">{r.black_elo ?? ""}</td>
              <td>{r.result}</td>
              <td>{r.date}</td>
              <td>{r.event}</td>
              <td>{r.eco}</td>
              <td>{r.opening}</td>
              <td class="n">{r.moves}</td>
            </tr>
          {/each}
          <tr style:height="{Math.max(0, total - first - visible.length) * ROW}px"></tr>
        </tbody>
      </table>
      {#if progress?.done && !total}<p class="meta">{t("chess.no_games")}</p>{/if}
    </div>
    <p class="meta keys">{t("chess.keys_list_gui")}</p>
  {/if}
</div>

<style>
  .chess {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    gap: 6px;
    padding: 8px 12px;
    container-type: inline-size;
  }
  .chess.full {
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
  .meta {
    margin: 0;
    font-size: 0.85em;
    opacity: 0.75;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .filters input,
  .filters select {
    font: inherit;
    font-size: 0.9em;
    min-width: 0;
    flex: 1 1 8em;
    background: var(--dialog-input-bg);
    color: var(--dialog-input-fg);
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    padding: 2px 6px;
  }
  .filters .num {
    flex: 0 1 6em;
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
  }
  th button {
    font: inherit;
    font-weight: bold;
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
  td.n,
  td.number {
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
    flex: 0 1 min(100%, 420px);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .full .side {
    flex-basis: min(100%, 60vh);
  }
  .board {
    width: 100%;
    aspect-ratio: 1;
    border: 1px solid var(--border-fg);
    outline: none;
  }
  .board:focus-visible {
    box-shadow: 0 0 0 2px var(--accent-bg);
  }
  .coord {
    font-size: 0.17px;
    font-family: var(--font);
    font-weight: bold;
  }
  /* The theme's colours, not the system's grey buttons (WebKitGTK draws them in GTK's). */
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    cursor: pointer;
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
  .bar button.on {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
  }
  .icon-btn {
    font-family: var(--icon-font);
    padding: 2px 6px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
  }
  .moves {
    flex: 1 1 260px;
    min-width: 0;
    overflow: auto;
  }
  .players {
    margin: 0 0 2px;
  }
  .text {
    line-height: 1.7;
  }
  /* Figurines from a font that draws them at a readable size: a theme's monospaced font often
     has them tiny or not at all. */
  .fig {
    font-family: "DejaVu Sans", "Noto Sans Symbols 2", "Segoe UI Symbol", "Apple Symbols", sans-serif;
    font-weight: normal;
    font-size: 1.15em;
    line-height: 1;
  }
  .mv {
    font: inherit;
    font-weight: bold;
    padding: 0 2px;
    border: none;
    border-radius: var(--r-sm);
    background: none;
    box-shadow: none;
    color: inherit;
    cursor: pointer;
  }
  .mv:hover {
    text-decoration: underline;
  }
  .mv.var {
    font-weight: normal;
    opacity: 0.8;
  }
  .mv.here {
    background: var(--cursor-bg);
    color: var(--cursor-fg);
    opacity: 1;
  }
  .comment {
    font-style: italic;
    opacity: 0.75;
  }
  .paren,
  .nag {
    opacity: 0.75;
  }
  .nag {
    margin-inline-start: 0.3em;
  }
  .keys {
    margin-top: 6px;
  }
  .book {
    border-collapse: collapse;
    margin-block: 4px;
  }
  .book td {
    padding: 0 8px 0 0;
  }
</style>

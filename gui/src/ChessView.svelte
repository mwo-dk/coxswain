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
  import { invoke, basename, keysToBoard, browseKey, chessFormat, viewEnd } from "./lib.js";
  import { t, tn } from "./i18n.svelte.js";

  let { path, full = false } = $props();

  // A .bin file comes here only when it is a Polyglot opening book (Preview, chess_is_book).
  // A games file known by its bytes, not its name (a DhtmlXQ xiangqi game): read as PGN.
  const format = $derived(/\.bin$/i.test(path) ? "book" : chessFormat(basename(path)) || "pgn");
  // A PDN file: draughts games in the same table, on a board of their own, without openings,
  // figurines or a book.
  const draughts = $derived(format === "pdn");
  // KIF and CSA: shogi, on its board with the pieces in hand.
  const shogi = $derived(format === "shogi");
  // PBN: bridge deals, at a table of four hands; the contract stands where the opening does.
  const bridge = $derived(format === "pbn");
  const table = $derived(format === "pgn" || draughts || shogi || bridge);
  // A PGN file of xiangqi, as its tags say (chess_rows): its own board too.
  let kind = $state("");
  const xiangqi = $derived(kind === "xiangqi");
  const other = $derived(draughts || xiangqi || shogi || bridge);
  // F1 in the window: the feature of the file's games.
  $effect(() => {
    if (full && xiangqi && ui.modal?.kind === "chess" && ui.modal.path === path) ui.modal.feature = "xiangqi";
  });
  const columns = $derived(
    bridge ? COLUMNS.filter(([c]) => !["eco", "white_elo", "black_elo"].includes(c)) : other ? COLUMNS.filter(([c]) => c !== "eco" && c !== "opening") : COLUMNS,
  );
  const BRIDGE_COL = { white: "bridge.col_ns", black: "bridge.col_ew", opening: "bridge.col_contract", moves: "bridge.col_tricks" };
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
  // Playing on (chess_moves): the line played from the board, kept here until saved as a new
  // PGN file or cleared ({ base: the move it is played from, moves, at: how many are shown });
  // the legal moves of the position shown, the square clicked, and a promotion to choose.
  let played = $state(null);
  let legal = $state([]);
  let picked = $state(null);
  let promo = $state(null);
  let said = $state("");
  // The engine (chess_analyse): asked about each position while on.
  let analysis = $state(false);
  let engine = $state(null);
  // One of the engine's lines stepped through without playing it (1–5, a click on its move):
  // { k: its place, steps: its moves with their positions, at: how many are shown }. The board
  // shows it; the engine stays on the game's position, and a change of it ends the stepping.
  let browse = $state(null);
  // The whole game analysed (chess_review): how far it is, asked again while it runs.
  let review = $state(null);
  // Find this position (chess_position_games): the games found, null before asking.
  let posGames = $state(null);

  $effect(() => {
    const theme = ui.theme;
    // Settings saved: the board and notation as they say now.
    void ui.cfg;
    invoke("chess_looks", { theme }).then((l) => {
      looks = l;
      letters = l.notation === "letters";
      analysis = l.analysis;
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
      played = null;
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
    played = null;
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
    if (!table) return;
    const p = path;
    const seq = ++rowsSeq;
    const n = Math.ceil(height / ROW) + 20;
    const from = Math.max(0, Math.floor((listEl?.scrollTop ?? 0) / ROW) - 10);
    try {
      const r = await invoke("chess_rows", { path: p, query: $state.snapshot(query), offset: from, limit: n });
      // An answer to an older scroll or filter that came in late: a newer one is on its way.
      if (p !== path || seq !== rowsSeq) return;
      progress = r.progress;
      kind = r.format;
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
      played = null;
      review = null;
      said = "";
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
    played = null;
    game = { game: { start: p.fen ?? "", nodes: [], roots: [], tags: [], result: "" }, layout: p.about ? [{ t: "comment", text: p.about }] : [], index: i };
    at = null;
    error = p.fen ? "" : t("chess.no_position", { line: p.line });
  }

  const nodes = $derived(game?.game.nodes ?? []);
  const inLine = $derived(played != null && played.at > 0);
  const gameFen = $derived(inLine ? played.moves[played.at - 1].fen : at == null ? game?.game.start : nodes[at]?.fen);
  const fen = $derived(browse ? browse.steps[browse.at - 1].fen : gameFen);
  const last = $derived(browse ? browse.steps[browse.at - 1] : inLine ? played.moves[played.at - 1] : at == null ? null : nodes[at]);
  $effect(() => {
    void gameFen;
    browse = null;
  });
  // The squares and arrows the game's comments draw on this position ([%csl], [%cal]).
  const drawn = $derived(inLine || browse ? [] : at == null ? (game?.game.marks ?? []) : (nodes[at]?.marks ?? []));
  const MARK_COLOUR = { G: "rgb(21 120 27 / 0.8)", R: "rgb(200 30 30 / 0.8)", Y: "rgb(230 160 0 / 0.85)", B: "rgb(0 48 136 / 0.8)" };
  /** A square's centre on the board as drawn (flipped or not). */
  const centre = (sq) => [(flip ? 7 - (sq % 8) : sq % 8) + 0.5, (flip ? Math.floor(sq / 8) : 7 - Math.floor(sq / 8)) + 0.5];
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

  const SEAT = ["bridge.north", "bridge.east", "bridge.south", "bridge.west"];
  const SEAT_TAG = ["North", "East", "South", "West"];
  const SUIT = ["♠", "♥", "♦", "♣"];
  /** A bridge table from its state (`bridge::State::text`): each seat's cards by suit as text
   *  ("A K 10"), the trick's card per seat ("♠A"), the tricks won. */
  function bridgeState(f) {
    const [hands = "", trick = "", , ns = "0", ew = "0"] = (f ?? "").split(";");
    const out = { hands: [[], [], [], []], trick: [null, null, null, null], won: [ns, ew] };
    const shown = (r) => (r === "T" ? "10" : r);
    for (const h of hands.split(" ")) {
      const s = "NESW".indexOf(h[0]);
      if (s < 0) continue;
      out.hands[s] = h.slice(2).split(".").map((suit) => [...suit].map(shown).join(" "));
    }
    for (const c of trick.split(" ")) {
      const s = "NESW".indexOf(c[0]);
      if (s >= 0 && c.length >= 4) out.trick[s] = SUIT["SHDC".indexOf(c[2])] + shown(c.slice(3));
    }
    return out;
  }
  /** The auction's calls in rows West, North, East, South, from the seat that called first. */
  function auctionRows(calls) {
    const first = "NESW".indexOf(calls[0]);
    const cells = [...Array([3, 0, 1, 2].indexOf(first < 0 ? 0 : first)).fill(""), ...calls.slice(1)];
    const rows = [];
    for (let i = 0; i < cells.length; i += 4) rows.push(cells.slice(i, i + 4));
    return rows;
  }

  /** Shogi kanji by SFEN letter, plain and promoted (`shogi::glyph`). */
  const SG = { P: "歩", L: "香", N: "桂", S: "銀", G: "金", B: "角", R: "飛", K: "玉" };
  const SGP = { P: "と", L: "杏", N: "圭", S: "全", B: "馬", R: "龍" };
  /** A shogi position from its SFEN: 81 cells (flipped when `flip`), each { k, promoted, up }
   *  (up: drawn upright, the side at the bottom), and each side's hand as text ("歩2 角"). */
  function sgView(f) {
    const [placement = "", , handText = "-"] = (f ?? "").split(" ");
    const cells = [];
    for (const row of placement.split("/").slice(0, 9)) {
      const r = [];
      let plus = false;
      for (const ch of row) {
        if (ch === "+") plus = true;
        else if (/\d/.test(ch)) r.push(...Array(Number(ch)).fill(null)), (plus = false);
        else {
          const up = ch.toUpperCase();
          const sente = ch === up;
          r.push(SG[up] ? { k: plus ? (SGP[up] ?? SG[up]) : up === "K" && !sente ? "王" : SG[up], file: (plus && up !== "G" && up !== "K" ? "p" : "") + up, promoted: plus, sente } : null);
          plus = false;
        }
      }
      cells.push(...[...r, ...Array(9).fill(null)].slice(0, 9));
    }
    while (cells.length < 81) cells.push(null);
    const view = (flip ? cells.reverse() : cells).map((p) => p && { ...p, up: p.sente !== flip });
    const hands = ["", ""];
    let n = "";
    for (const ch of handText === "-" ? "" : handText) {
      if (/\d/.test(ch)) n += ch;
      else if (SG[ch.toUpperCase()]) {
        const side = ch === ch.toUpperCase() ? 0 : 1;
        hands[side] += `${hands[side] ? " " : ""}${SG[ch.toUpperCase()]}${n && n !== "1" ? n : ""}`;
        n = "";
      }
    }
    return { cells: view, hands };
  }

  /** Xiangqi pieces by their FEN letter (`xiangqi::glyph`). */
  const XQ = { K: "帥", A: "仕", B: "相", N: "傌", R: "俥", C: "炮", P: "兵", k: "將", a: "士", b: "象", n: "馬", r: "車", c: "砲", p: "卒" };
  /** A xiangqi position's 90 points from its FEN, rows from Black's side; flipped when `flip`. */
  function xqCells(f) {
    const out = [];
    for (const row of (f ?? "").split(" ")[0].split("/").slice(0, 10)) {
      const r = [];
      for (const ch of row) {
        if (/\d/.test(ch)) r.push(...Array(Number(ch)).fill(null));
        else r.push(XQ[ch] ? ch : null);
      }
      out.push(...[...r, ...Array(9).fill(null)].slice(0, 9));
    }
    while (out.length < 90) out.push(null);
    return flip ? out.reverse() : out;
  }

  /** A draughts position (`draughts::cells`): rows from the top, a run of empty cells as its
   *  count, M K White's man and king, m k Black's. */
  function cells(f) {
    const rows = (f ?? "").split(" ")[0].split("/").slice(0, 12);
    const out = [];
    let w = 0;
    for (const row of rows) {
      const r = [];
      for (const m of row.matchAll(/(\d+)|([^\d])/g)) {
        if (m[1]) r.push(...Array(Math.min(12, Number(m[1]))).fill(null));
        else r.push(m[2]);
      }
      w = Math.max(w, Math.min(12, r.length));
      out.push(r);
    }
    w = w || 1;
    const h = out.length || 1;
    return { w, h, cells: out.flatMap((r) => [...r, ...Array(w).fill(null)].slice(0, w)) };
  }

  // The book's moves, again for each position.
  let bookSeq = 0;
  $effect(() => {
    const f = fen;
    const p = path;
    void ui.cfg;
    const s = ++bookSeq;
    if (!f || other) return void (book = null);
    invoke("chess_book", { path: p, fen: f }).then(
      (b) => s === bookSeq && (book = b),
      () => {},
    );
  });

  // The legal moves, again for each position.
  let movesSeq = 0;
  $effect(() => {
    const f = fen;
    const s = ++movesSeq;
    picked = null;
    promo = null;
    legal = [];
    if (f) invoke("chess_moves", { fen: f }).then((m) => s === movesSeq && (legal = m), () => {});
  });
  const targets = $derived(picked == null ? [] : legal.filter((m) => m.from === picked).map((m) => m.to));

  // The engine, again for each of the game's positions while it is on (not an engine line's).
  let engineSeq = 0;
  $effect(() => {
    const f = gameFen;
    const s = ++engineSeq;
    if (!analysis || !f || other) return void (engine = null);
    engine = { fen: f, thinking: true };
    invoke("chess_analyse", { fen: f }).then(
      (a) => {
        // Options set for the engine that it did not take: said once a session.
        if (a.note) said = a.note;
        if (s === engineSeq) engine = { fen: f, a };
      },
      (e) => s === engineSeq && (engine = { fen: f, error: String(e) }),
    );
  });

  /** A click on a square: a piece of the side to move is picked up, a square it can reach plays
   * the move (a promotion asks for its piece). */
  function clickSquare(sq) {
    if (picked != null) {
      const ms = legal.filter((m) => m.from === picked && m.to === sq);
      if (ms.length > 1) return void (promo = ms);
      if (ms.length) return playMove(ms[0]);
    }
    picked = picked !== sq && legal.some((m) => m.from === sq) ? sq : null;
    boardEl?.focus();
  }

  /** A move played from the position shown: the game's own next move (or a variation's) is
   * stepped to; any other starts the played line, or goes on with it after the move shown. */
  function playMove(m) {
    // From an engine line stepped through: the line up to there first.
    if (browse) playBrowsed();
    picked = null;
    promo = null;
    if (format === "book" && !inLine) {
      bookLine.push(game.game.start);
      game.game.start = m.fen;
      return;
    }
    if (!inLine) {
      const next = at == null ? game.game.roots : nodes[at].children;
      const n = next.find((n) => nodes[n].fen === m.fen);
      if (n != null) return void (at = n);
      played = { base: at, moves: [m], at: 1 };
      return;
    }
    played.moves = [...played.moves.slice(0, played.at), m];
    played.at = played.moves.length;
  }

  /** The engine's line `k` (from 0) of the game's position, stepped into at move `at`. */
  function browseLine(k, at = 1) {
    const l = engine?.fen === gameFen ? engine.a?.lines?.[k] : null;
    const n = engine?.fen === gameFen ? (engine.a?.lines?.length ?? 0) : 0;
    // The engine answered with fewer lines than the one asked for.
    if (!l?.steps?.length) return void (said = k >= n && n > 0 ? tn("chess.engine_only_lines", n) : t("chess.engine_no_best"));
    browse = { k, steps: l.steps, at };
    boardEl?.focus();
  }

  /** The engine line stepped through, played up to the move shown, as moves played on the
   * board are: into the game where it goes on with the game, else into your line. */
  function playBrowsed() {
    const b = browse;
    browse = null;
    for (const m of b?.steps.slice(0, b.at) ?? []) playMove(m);
  }

  /** The played line as a new PGN file next to this one: the game's moves to its base, then it. */
  async function saveLine() {
    if (!played) return void (said = t("chess.line_none"));
    const sans = [];
    for (let n = played.base; n != null; n = nodes[n].parent) sans.unshift(nodes[n].san);
    try {
      said = await invoke("chess_save_line", { path, tags: game.game.tags, start: game.game.start, sans: [...sans, ...played.moves.map((m) => m.san)] });
    } catch (e) {
      said = String(e);
    }
  }

  /** The whole game analysed (g): started, or stopped while it runs; its state asked for. */
  async function reviewAct(act) {
    if (format !== "pgn" || !game) return;
    const index = game.index;
    try {
      const r = await invoke("chess_review", { path, index, act });
      if (game?.index === index) review = r;
    } catch (e) {
      said = String(e);
    }
  }
  $effect(() => {
    if (!review || review.done) return;
    const timer = setInterval(() => reviewAct("state"), 700);
    return () => clearInterval(timer);
  });
  async function saveReview() {
    try {
      said = await invoke("chess_review_save", { path, index: game.index });
    } catch (e) {
      said = String(e);
    }
  }
  const CAP = 1000;
  const share = (v) => 1 / (1 + Math.exp(-Math.max(-CAP, Math.min(CAP, v)) / 250));
  /** An evaluation as shown (`review::shown`): +0.35, −1.20, #3, #−2. */
  function shownEval(v) {
    if (Math.abs(v) > CAP) return `#${v < 0 ? "−" : ""}${CAP + 100 - Math.abs(v)}`;
    return `${v < 0 ? "−" : "+"}${(Math.abs(v) / 100).toFixed(2)}`;
  }
  // The position's evaluation from White's side: the game's analysis, else the engine's.
  const evalNow = $derived.by(() => {
    const cp = (l) => (l.mate != null ? (l.mate > 0 ? CAP + 100 - Math.min(l.mate, 99) : -(CAP + 100 + Math.max(l.mate, -99))) : (l.cp ?? null));
    // An engine line stepped through: its own evaluation.
    if (browse) {
      const l = engine?.a?.lines?.[browse.k];
      return l ? cp(l) : null;
    }
    if (!inLine && review) {
      const v = at == null ? review.start : review.plies.find((p) => p.node === at)?.cp;
      if (v != null) return v;
    }
    const l = engine?.fen === fen ? engine.a?.lines?.[0] : null;
    return l ? cp(l) : null;
  });
  const marks = $derived(new Map((review?.plies ?? []).filter((p) => p.mark).map((p) => [p.node, p.mark])));
  const MARK = { 2: "?", 4: "??", 5: "!?", 6: "?!" };
  // The graph: the start and each move analysed, White's share from 0 to 100.
  const points = $derived(review?.start == null ? [] : [{ node: null, v: review.start }, ...review.plies.map((p) => ({ node: p.node, v: p.cp, mark: p.mark }))]);
  function graphClick(e) {
    const r = e.currentTarget.getBoundingClientRect();
    const i = Math.round(((e.clientX - r.left) / r.width) * (points.length - 1));
    if (points[i]) goto(points[i].node);
    boardEl?.focus();
  }

  /** Find this position (p): the games in the search store that reach the board shown. */
  async function findPosition() {
    if (!fen || other) return;
    said = t("chess.position_searching");
    try {
      const r = await invoke("chess_position_games", { path, fen });
      if (r == null) (said = t("chess.position_off")), (posGames = null);
      else if (!r.length) (said = t("chess.position_none")), (posGames = null);
      else (said = ""), (posGames = r);
    } catch (e) {
      said = String(e);
    }
  }
  /** A game found: this file's on the board, another file's in its own window. */
  function openFound(g) {
    posGames = null;
    if (g.path === path) return void openGame(g.index);
    ui.modal = { kind: "chess", path: g.path };
    ui.chessAt = { path: g.path, index: g.index };
  }

  /** A move of the game shown: out of the played line, which stays. */
  function goto(n) {
    if (played) played.at = 0;
    at = n;
  }

  /** A move's number as written: "12." for White's, "12…" for Black's when it starts a line. */
  const numbered = (m, first) => (m.white ? `${m.number}.` : first ? `${m.number}…` : "");
  const baseName = $derived(played?.base == null ? t("chess.line_start") : `${nodes[played.base].number}${nodes[played.base].white ? "." : "…"}${show(nodes[played.base].san).map(([x]) => x).join("")}`);

  /** A book's move played: its position on the board, the one before kept for ←. */
  function playBook(m) {
    bookLine.push(game.game.start);
    game.game.start = m.fen;
  }

  /** A move as shown, in pieces: [text, figurine] (`chess::show_san`). */
  function show(san) {
    if (other) return [[san, false]];
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
    // In the played line, → ← step it; → on the move it starts from enters it again. In a
    // position, → ← step it whenever there is one.
    if (played) {
      const baseHere = played.at === 0 && (format !== "pgn" || at === played.base);
      if (played.at > 0 || baseHere) {
        if (to === "next") return void (played.at = Math.min(played.at + 1, played.moves.length));
        if (to === "prev" && played.at > 0) return void played.at--;
        if (to === "prev" && format !== "pgn") return;
        played.at = 0;
      }
    }
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
    setTimeout(keepInView, 0);
  }

  /** The move shown in view in the moves, and after it, while there is room, its variations
   *  and the other moves from its position (`viewEnd`): before, it was scrolled to the
   *  bottom edge and they stayed out of sight in a short pane. Only the moves scroll. */
  function keepInView() {
    const here = movesEl?.querySelector(".here");
    if (!here) return;
    const i = game.layout.findIndex((t) => t.t === "move" && t.node === at);
    const end = (i < 0 ? null : movesEl.querySelector(`[data-tok="${viewEnd(game.layout, i)}"]`)) ?? here;
    const box = movesEl.getBoundingClientRect();
    const top = here.getBoundingClientRect().top - box.top + movesEl.scrollTop;
    const bottom = end.getBoundingClientRect().bottom - box.top + movesEl.scrollTop;
    const view = movesEl.clientHeight;
    // The end at the bottom when it is below; the move shown never above the top.
    let y = movesEl.scrollTop;
    if (bottom > y + view) y = bottom - view;
    if (top < y) y = top;
    movesEl.scrollTop = y;
  }

  function nextGame(by) {
    const i = rows.findIndex((r) => r[0] === game.index);
    const r = rows[i + by];
    if (r) openGame(r[0]);
  }

  function onBoardKey(e) {
    // An engine line stepped through (`browseKey`).
    if (browse) {
      const r = browseKey(browse, e.key);
      if (r.used === "play") playBrowsed();
      else browse = r.browse;
      if (r.used) {
        e.preventDefault();
        e.stopPropagation();
        return;
      }
    }
    const map = { ArrowRight: "next", ArrowLeft: "prev", Home: "start", End: "end", ArrowDown: "down", ArrowUp: "up" };
    if (map[e.key]) step(map[e.key]);
    else if (e.key === "f") flip = !flip;
    else if (e.key === "n" && !other) letters = !letters;
    else if (e.key === "a" && !other) analysis = !analysis;
    else if (/^[1-5]$/.test(e.key) && !e.ctrlKey && !e.altKey && !e.metaKey && !other) browseLine(+e.key - 1);
    else if (e.key === "s" && !other) saveLine();
    else if (e.key === "b" && !other && engine?.a?.lines[0]) playMove(engine.a.lines[0].best);
    else if (e.key === "g" && format === "pgn" && !other) reviewAct(review && !review.done ? "stop" : "start");
    else if (e.key === "w" && format === "pgn" && !other) saveReview();
    else if (e.key === "p" && !other) findPosition();
    else if (e.key === "c" && played) played = null;
    else if (e.key === "Escape" && (picked != null || promo)) (picked = null), (promo = null);
    else if (e.key === "PageDown" && table) nextGame(1);
    else if (e.key === "PageUp" && table) nextGame(-1);
    else if (e.key === "Escape" && table && game) back();
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
  /** A number field as wide as its label, which is all it shows until something is typed. */
  /** A width that holds `label` whole (wide characters count twice), with the field's padding. */
  const fits = (label) => `calc(${[...label].reduce((n, c) => n + (c.codePointAt(0) >= 0x2e80 ? 2 : 1), 0)}ch + 2.5em)`;
  /** A text filter as wide as its placeholder, never cut, but no wider than the pane. */
  const whole = (label) => `flex-basis: ${fits(label)}; min-width: min(100%, ${fits(label)})`;
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
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <rect {x} {y} width="1" height="1" fill={looks ? looks.colours[(rank + file) % 2 === 0 ? 1 : 0] : "#888"} onclick={() => clickSquare(sq)} />
  {#if picked === sq}<rect {x} {y} width="1" height="1" class="mark" fill="rgb(90 160 246 / 0.5)" />{/if}
  {#if lit}<rect class="mark" {x} {y} width="1" height="1" fill="rgb(246 224 90 / 0.45)" />{/if}
  {#each drawn.filter((m) => m.from === sq && m.to === sq) as m, k (k)}<rect class="mark" {x} {y} width="1" height="1" fill={MARK_COLOUR[m.colour]} opacity="0.6" />{/each}
  {#if p}<image class="mark" href={`vendor/chess-pieces/${p === p.toUpperCase() ? "w" : "b"}${PIECE[p.toLowerCase()]}.svg`} {x} {y} width="1" height="1" />{/if}
  {#if targets.includes(sq)}<circle class="mark" cx={x + 0.5} cy={y + 0.5} r={p ? 0.45 : 0.15} fill={p ? "none" : "rgb(0 0 0 / 0.3)"} stroke={p ? "rgb(0 0 0 / 0.35)" : "none"} stroke-width="0.07" />{/if}
  {#if y === 7}<text class="coord mark" x={x + 0.94} y={y + 0.95} text-anchor="end" fill={looks ? looks.colours[(rank + file) % 2 === 0 ? 0 : 1] : "#000"}>{"abcdefgh"[file]}</text>{/if}
  {#if x === 0}<text class="coord mark" x={x + 0.05} y={y + 0.22} fill={looks ? looks.colours[(rank + file) % 2 === 0 ? 0 : 1] : "#000"}>{rank + 1}</text>{/if}
{/snippet}

<!-- A shogi piece: a five-sided tile with its kanji (drawn for Coxswain, vendor/game-pieces),
     Gote's turned round; a promoted piece in red; the last move's squares tinted. -->
{#snippet sgPiece(p, i)}
  {@const c = flip ? 80 - i : i}
  {@const x = i % 9}
  {@const y = Math.floor(i / 9)}
  {#if last && (last.from === c || last.to === c)}<rect x={x + 0.03} y={y + 0.03} width="0.94" height="0.94" fill="rgb(246 224 90 / 0.5)" />{/if}
  {#if p}
    <image href={`vendor/game-pieces/shogi-${p.file}.svg`} x={x} y={y} width="1" height="1" transform={p.up ? "" : `rotate(180 ${x + 0.5} ${y + 0.5})`}><title>{p.k}</title></image>
  {/if}
{/snippet}

<!-- The xiangqi board's lines: 9 files, 10 ranks, the river between ranks 5 and 6 that only the
     edge files cross, the palaces' diagonals; the files numbered for both sides. -->
{#snippet xqLines(ink)}
  {#each Array(10) as _, y (y)}<line x1="0" y1={y} x2="8" y2={y} stroke={ink} stroke-width="0.04" />{/each}
  {#each Array(9) as _, x (x)}
    {#if x === 0 || x === 8}<line x1={x} y1="0" x2={x} y2="9" stroke={ink} stroke-width="0.04" />
    {:else}<line x1={x} y1="0" x2={x} y2="4" stroke={ink} stroke-width="0.04" /><line x1={x} y1="5" x2={x} y2="9" stroke={ink} stroke-width="0.04" />{/if}
    <text x={x} y="-0.45" class="coord" text-anchor="middle" fill={ink}>{flip ? "九八七六五四三二一"[x] : x + 1}</text>
    <text x={x} y="9.65" class="coord" text-anchor="middle" fill={ink}>{flip ? x + 1 : "九八七六五四三二一"[x]}</text>
  {/each}
  {#each [[3, 0, 5, 2], [5, 0, 3, 2], [3, 7, 5, 9], [5, 7, 3, 9]] as [a, b, c, d], k (k)}<line x1={a} y1={b} x2={c} y2={d} stroke={ink} stroke-width="0.03" />{/each}
  <text x="2" y="4.68" class="river" text-anchor="middle" fill={ink}>楚 河</text>
  <text x="6" y="4.68" class="river" text-anchor="middle" fill={ink}>漢 界</text>
{/snippet}

<!-- A xiangqi piece: a disc with its character, Red's in red, Black's in black (drawn for
     Coxswain, vendor/game-pieces); the last move's points ringed. -->
{#snippet xqPiece(p, i)}
  {@const c = flip ? 89 - i : i}
  {@const x = i % 9}
  {@const y = Math.floor(i / 9)}
  {@const lit = last && (last.from === c || last.to === c)}
  {#if lit}<circle cx={x} cy={y} r="0.47" fill="rgb(246 224 90 / 0.55)" />{/if}
  {#if p}
    <image href={`vendor/game-pieces/xiangqi-${p === p.toUpperCase() ? "r" : "b"}${p.toLowerCase()}.svg`} x={x - 0.46} y={y - 0.46} width="0.92" height="0.92" />
  {/if}
{/snippet}

<!-- A draughts square: dark ones numbered, a man as a disc, a king as two discs stacked. -->
{#snippet cell(d, i)}
  {@const col = i % d.w}
  {@const row = Math.floor(i / d.w)}
  {@const x = flip ? d.w - 1 - col : col}
  {@const y = flip ? d.h - 1 - row : row}
  {@const c = x + y * d.w}
  {@const dark = (x + y) % 2 === 1}
  {@const p = d.cells[c]}
  {@const lit = last && (last.from === c || last.to === c)}
  <rect x={col} y={row} width="1" height="1" fill={looks ? looks.colours[dark ? 1 : 0] : dark ? "#769656" : "#eeeed2"} />
  {#if lit}<rect x={col} y={row} width="1" height="1" fill="rgb(246 224 90 / 0.45)" />{/if}
  {#if dark && !p}<text x={col + 0.06} y={row + 0.24} class="coord" fill={looks ? looks.colours[0] : "#000"}>{y * (d.w / 2) + Math.floor(x / 2) + 1}</text>{/if}
  {#if p}<image href={`vendor/game-pieces/draughts-${p === p.toUpperCase() ? "w" : "b"}${p.toLowerCase() === "k" ? "k" : "m"}.svg`} x={col} y={row} width="1" height="1" />{/if}
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
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="game" onclick={(e) => keysToBoard(e.target, e.detail) && boardEl?.focus()}>
      <div class="side">
        <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
        {#if draughts}
          {@const d = cells(fen)}
          <svg class="board chess-keys" viewBox="0 0 {d.w} {d.h}" tabindex="0" role="img" aria-label={t("draughts.board")} bind:this={boardEl} onkeydown={onBoardKey}>
            {#each Array(d.w * d.h) as _, i (i)}{@render cell(d, i)}{/each}
          </svg>
        {:else if bridge}
          {@const st = bridgeState(fen)}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
          <div class="bridge chess-keys" tabindex="0" role="img" aria-label={t("bridge.board")} bind:this={boardEl} onkeydown={onBoardKey}>
            {#each [[0, "n"], [3, "w"], [1, "e"], [2, "s"]] as [s, area] (s)}
              <div class="hand {area}">
                <b>{t(SEAT[s])}</b> {tag(SEAT_TAG[s])}
                {#each [0, 1, 2, 3] as su (su)}<div><span class:red={su === 1 || su === 2}>{SUIT[su]}</span> {st.hands[s][su] || "–"}</div>{/each}
              </div>
            {/each}
            <div class="trick">
              {#each [[0, "n"], [3, "w"], [1, "e"], [2, "s"]] as [s, area] (s)}<span class="card {area}" class:red={st.trick[s] && "♥♦".includes(st.trick[s][0])}>{st.trick[s] ?? ""}</span>{/each}
            </div>
          </div>
          {#if tag("Contract")}<p class="meta">{t("bridge.contract", { contract: game.game.result.split(" ")[0], declarer: tag("Declarer") })}</p>{/if}
          <p class="meta">{t("bridge.facts", { dealer: tag("Dealer"), vulnerable: tag("Vulnerable") })}</p>
          <p class="meta">{t("bridge.tricks", { ns: st.won[0], ew: st.won[1] })}</p>
          {#if tag("Auction")}
            <p class="meta">{t("bridge.auction")}</p>
            <table class="auction">
              <thead><tr>{#each [3, 0, 1, 2] as s (s)}<th>{t(SEAT[s])}</th>{/each}</tr></thead>
              <tbody>
                {#each auctionRows(tag("Auction").split(" ")) as row, i (i)}<tr>{#each row as c, j (j)}<td class:red={c && /[♥♦]/.test(c)}>{c}</td>{/each}</tr>{/each}
              </tbody>
            </table>
          {/if}
        {:else if shogi}
          {@const sg = sgView(fen)}
          <p class="meta hand">{t(flip ? "shogi.hand_sente" : "shogi.hand_gote", { pieces: sg.hands[flip ? 0 : 1] || "–" })}</p>
          <svg class="board sg chess-keys" viewBox="-0.2 -0.6 9.8 9.8" tabindex="0" role="img" aria-label={t("shogi.board")} bind:this={boardEl} onkeydown={onBoardKey}>
            <rect x="-0.2" y="-0.6" width="9.8" height="9.8" fill={looks ? looks.colours[0] : "#e8c88a"} />
            {#each Array(10) as _, k (k)}
              <line x1={k} y1="0" x2={k} y2="9" stroke={looks ? looks.colours[1] : "#5a4020"} stroke-width="0.03" />
              <line x1="0" y1={k} x2="9" y2={k} stroke={looks ? looks.colours[1] : "#5a4020"} stroke-width="0.03" />
            {/each}
            {#each Array(9) as _, k (k)}
              <text x={k + 0.5} y="-0.18" class="coord" text-anchor="middle" fill={looks ? looks.colours[1] : "#5a4020"}>{"９８７６５４３２１"[flip ? 8 - k : k]}</text>
              <text x="9.3" y={k + 0.58} class="coord" text-anchor="middle" fill={looks ? looks.colours[1] : "#5a4020"}>{"一二三四五六七八九"[flip ? 8 - k : k]}</text>
            {/each}
            {#each sg.cells as p, i (i)}{@render sgPiece(p, i)}{/each}
          </svg>
          <p class="meta hand">{t(flip ? "shogi.hand_gote" : "shogi.hand_sente", { pieces: sg.hands[flip ? 1 : 0] || "–" })}</p>
        {:else if xiangqi}
          {@const xq = xqCells(fen)}
          <svg class="board xq chess-keys" viewBox="-0.8 -0.8 9.6 10.6" tabindex="0" role="img" aria-label={t("xiangqi.board")} bind:this={boardEl} onkeydown={onBoardKey}>
            <rect x="-0.8" y="-0.8" width="9.6" height="10.6" fill={looks ? looks.colours[0] : "#e8c88a"} />
            {@render xqLines(looks ? looks.colours[1] : "#5a4020")}
            {#each xq as p, i (i)}{@render xqPiece(p, i)}{/each}
          </svg>
        {:else}
        <div class="board-row">
          {#if evalNow != null}
            <div class="evalbar" class:flip title={shownEval(evalNow)}><div class="white" style:height="{share(evalNow) * 100}%"></div></div>
          {/if}
          <svg class="board chess-keys" viewBox="0 0 8 8" tabindex="0" role="img" aria-label={t("chess.board")} bind:this={boardEl} onkeydown={onBoardKey}>
            {#each Array(64) as _, i (i)}{@render square(i)}{/each}
            {#each drawn.filter((m) => m.from !== m.to) as m, k (k)}
              {@const [x1, y1] = centre(m.from)}
              {@const [x2, y2] = centre(m.to)}
              {@const len = Math.hypot(x2 - x1, y2 - y1)}
              {@const ux = (x2 - x1) / len}
              {@const uy = (y2 - y1) / len}
              <g class="mark" fill={MARK_COLOUR[m.colour]} stroke={MARK_COLOUR[m.colour]}>
                <line x1={x1} y1={y1} x2={x2 - ux * 0.35} y2={y2 - uy * 0.35} stroke-width="0.16" stroke-linecap="round" />
                <polygon points="{x2},{y2} {x2 - ux * 0.45 - uy * 0.25},{y2 - uy * 0.45 + ux * 0.25} {x2 - ux * 0.45 + uy * 0.25},{y2 - uy * 0.45 - ux * 0.25}" stroke="none" />
              </g>
            {/each}
          </svg>
        </div>
        {/if}
        <div class="bar">
          {#if table}<button title={t("chess.back_tip")} onclick={back}>{t("chess.back")}</button>{/if}
          <button class="icon" title={t("chess.start_tip")} aria-label={t("chess.start_tip")} onclick={() => step("start")}>|◀</button>
          <button class="icon" title={t("chess.prev_tip")} aria-label={t("chess.prev_tip")} onclick={() => step("prev")}>◀</button>
          <button class="icon" title={t("chess.next_tip")} aria-label={t("chess.next_tip")} onclick={() => step("next")}>▶</button>
          <button class="icon" title={t("chess.end_tip")} aria-label={t("chess.end_tip")} onclick={() => step("end")}>▶|</button>
          {#if !bridge}<button title={t("chess.flip_tip")} class:on={flip} onclick={() => (flip = !flip)}>{t("chess.flip")}</button>{/if}
          {#if !other}<button title={t("chess.notation_tip")} onclick={() => (letters = !letters)}>{#if letters}{pieces.join("")}{:else}<span class="fig">♔♘</span>{/if}</button>{/if}
          {#if !other}<button title={t("chess.engine_tip")} class:on={analysis} onclick={() => (analysis = !analysis)}>{t("chess.engine_btn")}</button>{/if}
          {#if !other}<button title={t("chess.position_tip")} onclick={findPosition}>{t("chess.position_btn")}</button>{/if}
          {#if format === "pgn" && !other}
            <button title={t("chess.review_tip")} class:on={review && !review.done} onclick={() => reviewAct(review && !review.done ? "stop" : "start")}>{review && !review.done ? t("chess.review_stop") : t("chess.review_btn")}</button>
            {#if review?.plies.length}<button title={t("chess.review_save_tip")} onclick={saveReview}>{t("chess.review_save")}</button>{/if}
          {/if}
          {#if !full}<button class="icon-btn" title={t("chess.window")} onclick={() => (ui.modal = { kind: "chess", path })}>{"\u{f065}"}</button>{/if}
        </div>
        {#if promo}
          <div class="bar promo">
            <span class="meta">{t("chess.promote")}</span>
            {#each promo as m (m.uci)}<button title={m.san} onclick={() => playMove(m)}><img alt={m.san} src={`vendor/chess-pieces/${m.white ? "w" : "b"}${m.promotion.toUpperCase()}.svg`} /></button>{/each}
          </div>
        {/if}
        {#if bridge}{:else if fen && shogi}<p class="meta">{fen.split(" ")[1] === "w" ? t("shogi.gote_to_move") : t("shogi.sente_to_move")}</p>
        {:else if fen}<p class="meta">{fen.split(" ")[1] === "b" ? t("chess.black_to_move") : xiangqi ? t("xiangqi.red_to_move") : t("chess.white_to_move")}{#if evalNow != null} · <b>{shownEval(evalNow)}</b>{/if}</p>{/if}
        {#if points.length}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
          <svg class="graph" viewBox="0 0 {Math.max(1, points.length - 1)} 100" preserveAspectRatio="none" onclick={graphClick} role="img" aria-label={t("chess.review_graph")}>
            <line x1="0" y1="50" x2={points.length - 1} y2="50" class="mid" />
            <polyline points={points.map((p, i) => `${i},${100 - share(p.v) * 100}`).join(" ")} />
            {#each points as p, i (i)}
              {#if p.mark && p.mark !== 5}<line x1={i} y1="82" x2={i} y2="100" class="tick m{p.mark}" />{/if}
              {#if !inLine && !browse && p.node === at}<line x1={i} y1="0" x2={i} y2="100" class="here" />{/if}
            {/each}
          </svg>
        {/if}
        {#if review}
          <p class="meta">{review.error ?? (review.done ? t("chess.review_done", { engine: review.engine, marked: review.plies.filter((p) => p.mark && p.mark !== 5).length }) : t("chess.review_running", { n: review.plies.length, total: review.total }))}</p>
        {/if}
      </div>
      <div class="moves" bind:this={movesEl}>
        {#if table}
          {#if bridge}<p class="players"><b>{[tag("North"), tag("South")].filter(known).join(", ")} – {[tag("East"), tag("West")].filter(known).join(", ")}</b> {game.game.result}</p>
          {#if tag("Board")}<p class="meta">{t("bridge.board_n", { n: tag("Board") })}</p>{/if}
          {:else}<p class="players"><b>{known(tag("Red")) || known(tag("White"))}{known(tag("WhiteElo")) ? ` (${tag("WhiteElo")})` : ""} – {known(tag("Black"))}{known(tag("BlackElo")) ? ` (${tag("BlackElo")})` : ""}</b> {game.game.result}</p>{/if}
          <p class="meta">{[known(tag("Event")), known(tag("Site")), known(tag("Date")), [tag("ECO"), tag("Opening")].join(" ").trim()].filter(Boolean).join(" · ")}</p>
          {#if game.variant}<p class="meta">{game.variant}</p>{/if}
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
        {#if engine}
          <p class="meta engine">{#if engine.thinking}{t("chess.engine_thinking")}{:else if engine.error}{engine.error}{:else}{t("chess.engine", { name: engine.a.engine, depth: engine.a.depth })}{#if engine.a.lines.length === 1} · {t("chess.engine_line_key")}{:else if engine.a.lines.length} · {t("chess.engine_lines_keys", { n: engine.a.lines.length })}{/if}{/if}</p>
          {#each engine.a?.lines ?? [] as l, k (k)}
            {@const on = browse?.k === k ? browse.at : 0}
            <p class="engine-line"><button class="mv line-no" class:here={on > 0} title={t("chess.engine_line_tip", { n: k + 1 })} onclick={() => browseLine(k)}>{k + 1}</button> <b class="eval">{l.eval}</b> {#each l.steps ?? [] as m, i (i)}<button class="mv" class:later={on ? i >= on : i > 0} class:here={on === i + 1} title={t("chess.engine_line_tip", { n: k + 1 })} onclick={() => browseLine(k, i + 1)}>{#each show(m.san) as [text, fig], j (j)}{#if fig}<span class="fig">{text}</span>{:else}{text}{/if}{/each}</button>{" "}{/each}</p>
          {/each}
          {#if browse}
            <p class="meta">{t("chess.engine_browse", { n: browse.k + 1, at: browse.at, total: browse.steps.length })}</p>
            <div class="bar">
              <button title={t("chess.engine_browse_play_tip")} onclick={playBrowsed}>{t("chess.engine_browse_play")}</button>
              <button title={t("chess.engine_browse_back_tip")} onclick={() => (browse = null)}>{t("chess.engine_browse_back")}</button>
            </div>
          {/if}
        {/if}
        <p class="text">
          {#each game.layout as tok, i (i)}
            {#if tok.t === "move"}
              {@const n = nodes[tok.node]}
              <button class="mv" data-tok={i} class:here={!inLine && at === tok.node} class:var={n.depth > 0} onclick={() => goto(tok.node)}>{tok.number ?? ""}{#each show(n.san) as [text, fig], j (j)}{#if fig}<span class="fig">{text}</span>{:else}{text}{/if}{/each}{glued(n.nags)}{#if marks.has(tok.node) && !n.nags.some((x) => x <= 6)}<span class="eng m{marks.get(tok.node)}">{MARK[marks.get(tok.node)]}</span>{/if}</button>{#if apart(n.nags)}<span class="nag">{apart(n.nags)}</span>{/if}{" "}
            {:else if tok.t === "comment"}
              <span class="comment" data-tok={i}>{tok.text}</span>{" "}
            {:else if tok.t === "open"}
              <span class="paren" data-tok={i}>(</span>
            {:else if tok.t === "close"}
              <span class="paren" data-tok={i}>)</span>{" "}
            {:else if tok.t === "result"}
              <b>{tok.text}</b>
            {/if}
          {/each}
        </p>
        {#if played}
          <p class="text played">
            <span class="comment">{t("chess.line_from", { move: baseName })}</span><br />
            {#each played.moves as m, k (k)}<button class="mv" class:here={played.at === k + 1} onclick={() => (played.at = k + 1)}>{numbered(m, k === 0)}{#each show(m.san) as [text, fig], j (j)}{#if fig}<span class="fig">{text}</span>{:else}{text}{/if}{/each}</button>{" "}{/each}
          </p>
          <div class="bar">
            <button title={t("chess.line_save_tip")} onclick={saveLine}>{t("chess.line_save")}</button>
            <button title={t("chess.line_clear_tip")} onclick={() => (played = null)}>{t("chess.line_clear")}</button>
          </div>
        {/if}
        {#if said}<p class="meta">{said}</p>{/if}
        {#if posGames}
          <p class="players"><b>{t("chess.position_title", { n: posGames.length })}</b> <button class="mv" title={t("chess.close")} onclick={() => (posGames = null)}>×</button></p>
          <ul class="found">
            {#each posGames as g, k (k)}
              <li><button class="mv" onclick={() => openFound(g)}>{t("chess.position_game", { file: g.name, n: g.index + 1 })}</button>{#if g.about}{" "}<span class="meta">{g.about}</span>{/if}</li>
            {/each}
          </ul>
        {/if}
        <p class="meta keys">{bridge ? t("bridge.keys_game_gui") : other ? t("draughts.keys_game_gui") : format === "pgn" ? t("chess.keys_game_gui") : format === "book" ? t("chess.keys_book_gui") : t("chess.keys_position_gui")}</p>
      </div>
    </div>
  {:else if table}
    <div class="filters">
      <input type="search" style={whole(t("chess.filter_player"))} placeholder={t("chess.filter_player")} aria-label={t("chess.filter_player")} bind:value={query.player} />
      {#if bridge}<input type="search" style={whole(t("bridge.col_contract"))} placeholder={t("bridge.col_contract")} aria-label={t("bridge.col_contract")} bind:value={query.opening} />{/if}
      {#if !other}<input type="search" style={whole(t("chess.filter_opening"))} placeholder={t("chess.filter_opening")} aria-label={t("chess.filter_opening")} bind:value={query.opening} />{/if}
      {#if !bridge}<select aria-label={t("chess.col_result")} bind:value={query.result}>
        <option value="">{t("chess.result_any")}</option>
        <option value="1-0">{draughts ? "2-0 · 1-0" : "1-0"}</option>
        <option value="0-1">{draughts ? "0-2 · 0-1" : "0-1"}</option>
        <option value="1/2-1/2">{draughts ? "1-1 · ½-½" : "½-½"}</option>
      </select>{/if}
      <input class="num" type="number" style:width={fits(t("chess.year_from"))} placeholder={t("chess.year_from")} aria-label={t("chess.year_from")} value={query.from ?? ""} onchange={(e) => (query.from = num(e.currentTarget.value))} />
      <input class="num" type="number" style:width={fits(t("chess.year_to"))} placeholder={t("chess.year_to")} aria-label={t("chess.year_to")} value={query.to ?? ""} onchange={(e) => (query.to = num(e.currentTarget.value))} />
      {#if !bridge}<input class="num" type="number" style:width={fits(t("chess.filter_elo"))} placeholder={t("chess.filter_elo")} aria-label={t("chess.filter_elo")} value={query.elo ?? ""} onchange={(e) => (query.elo = num(e.currentTarget.value))} />{/if}
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
            {#each columns as [col, label] (col)}
              <th class={col}><button onclick={() => sortBy(col)}>{label.includes(".") ? t(bridge && BRIDGE_COL[col] ? BRIDGE_COL[col] : xiangqi && col === "white" ? "xiangqi.col_red" : shogi && col === "white" ? "shogi.col_sente" : shogi && col === "black" ? "shogi.col_gote" : label) : label}{query.sort === col ? (query.desc ? " ↓" : " ↑") : ""}</button></th>
            {/each}
          </tr>
        </thead>
        <tbody>
          <tr style:height="{first * ROW}px"></tr>
          {#each visible as [index, r], k (index)}
            <tr class:cursor={first + k === cursor} onclick={() => (cursor = first + k)} ondblclick={() => openGame(index)}>
              <td class="number">{r.n}</td>
              <td>{r.white}</td>
              {#if !bridge}<td class="n">{r.white_elo ?? ""}</td>{/if}
              <td>{r.black}</td>
              {#if !bridge}<td class="n">{r.black_elo ?? ""}</td>{/if}
              <td>{r.result}</td>
              <td>{r.date}</td>
              <td>{r.event}</td>
              {#if !other}<td>{r.eco}</td>{/if}
              {#if !other || bridge}<td>{r.opening}</td>{/if}
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
    container-type: size;
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
    flex: 0 1 auto;
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
  /* The board beside the moves; each scrolls on its own, so the move shown and its variations
     can be kept in view without the board going out of it. */
  .game {
    display: flex;
    gap: 12px;
    min-height: 0;
    flex: 1;
    overflow: hidden;
  }
  .side {
    flex: 0 1 min(100%, 420px);
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
    overflow-y: auto;
  }
  /* A narrow pane (the preview): the board over the moves, at most half as high as the view,
     so the moves keep the rest. Before, the moves wrapped under a full-width board and showed
     two lines in a short window. */
  @container (max-width: 700px) {
    .game {
      flex-direction: column;
    }
    .side {
      flex: none;
      overflow: visible;
    }
    .side > .board,
    .board-row {
      width: min(100%, 50cqh);
      align-self: center;
    }
    .moves {
      flex: 1 1 0;
      min-height: 5em;
    }
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
  .bridge {
    display: grid;
    grid-template-areas: ". n ." "w t e" ". s .";
    grid-template-columns: 1fr 1fr 1fr;
    gap: 6px;
    padding: 8px;
    border: 1px solid var(--border-fg);
    border-radius: var(--r);
    outline: none;
    font-variant-numeric: tabular-nums;
  }
  .bridge:focus-visible {
    box-shadow: 0 0 0 2px var(--accent-bg);
  }
  .hand.n {
    grid-area: n;
  }
  .hand.w {
    grid-area: w;
  }
  .hand.e {
    grid-area: e;
  }
  .hand.s {
    grid-area: s;
  }
  .trick {
    grid-area: t;
    display: grid;
    grid-template-areas: ". n ." "w . e" ". s .";
    align-items: center;
    justify-items: center;
    font-weight: bold;
  }
  .card.n {
    grid-area: n;
  }
  .card.w {
    grid-area: w;
  }
  .card.e {
    grid-area: e;
  }
  .card.s {
    grid-area: s;
  }
  .red {
    color: #e0303f;
  }
  .auction td,
  .auction th {
    padding: 0 10px 0 0;
    text-align: start;
  }
  .board.sg {
    aspect-ratio: 1;
  }
  .hand {
    margin: 0;
  }
  .river {
    font-size: 0.55px;
    font-family: "Noto Serif CJK SC", "Noto Sans CJK SC", "Source Han Serif", "PingFang SC", "Microsoft YaHei", serif;
  }
  .board.xq {
    aspect-ratio: 9.6 / 10.6;
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
  /* The pieces and marks on the board let a click through to their square. */
  .mark {
    pointer-events: none;
  }
  .promo img {
    width: 2em;
    height: 2em;
    display: block;
  }
  .board-row {
    display: flex;
    gap: 4px;
  }
  .board-row .board {
    flex: 1;
    min-width: 0;
  }
  /* The evaluation: White's share from the bottom (from the top on a flipped board). */
  .evalbar {
    width: 12px;
    background: #222;
    border: 1px solid var(--border-fg);
    display: flex;
    flex-direction: column-reverse;
  }
  .evalbar.flip {
    flex-direction: column;
  }
  .evalbar .white {
    background: #eee;
    transition: height 0.3s;
  }
  .graph {
    width: 100%;
    height: 60px;
    cursor: pointer;
    border: 1px solid var(--border-fg);
  }
  .graph polyline {
    fill: none;
    stroke: var(--accent-bg, currentColor);
    stroke-width: 2;
    vector-effect: non-scaling-stroke;
  }
  .graph .mid,
  .graph .here {
    stroke: currentColor;
    opacity: 0.4;
    vector-effect: non-scaling-stroke;
  }
  .graph .here {
    opacity: 0.9;
  }
  .graph .tick {
    stroke: currentColor;
    stroke-width: 3;
    vector-effect: non-scaling-stroke;
  }
  .m6 {
    color: #e0c040;
    fill: #e0c040;
  }
  .m2 {
    color: #e08030;
    fill: #e08030;
  }
  .m4 {
    color: #e04040;
    fill: #e04040;
  }
  .m5 {
    color: #40a0e0;
  }
  .eng {
    font-weight: bold;
  }
  .found {
    margin: 0 0 6px;
    padding-inline-start: 1.2em;
    max-height: 14em;
    overflow: auto;
  }
  .engine-line {
    margin: 0;
    font-size: 0.9em;
  }
  .engine-line .mv.later {
    font-weight: normal;
    opacity: 0.75;
  }
  .engine-line .line-no {
    font-weight: normal;
    opacity: 0.75;
    min-width: 1.4em;
  }
  .engine-line .eval {
    display: inline-block;
    min-width: 4em;
    font-variant-numeric: tabular-nums;
  }
  .book {
    border-collapse: collapse;
    margin-block: 4px;
  }
  .book td {
    padding: 0 8px 0 0;
  }
</style>

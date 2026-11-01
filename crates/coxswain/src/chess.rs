//! Chess, full screen (F3 or Alt+K on a PGN, EPD or FEN file): a PGN file's games as a table
//! with a filter, filling while the file is read; a game as a board on coloured squares with
//! its moves beside it, variations and comments in place. An EPD or FEN file: its positions.
//! Reading and playing out are coxswain-core's `chess`; the desktop app shows the same.

use coxswain_core::chess::polyglot::{self, BookMove};
use coxswain_core::chess::play::{self, Analysis, Played};
use coxswain_core::chess::review::{self, Review, Reviewer};
use coxswain_core::chess::{self, Format, Library, Notation, Position, Query, Sort, Token};
use coxswain_core::config::{Action, Key, KeyCode, Theme};
use coxswain_core::mosaic::{self, Pieces};
use coxswain_core::{t, tn};
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Instant;
use crate::ui::Cols;

use crate::bom::faint;
use crate::ui::{fit, sty};

/// What the viewer wants done after a key.
pub enum Outcome {
    Stay,
    Close,
    /// Another games file, at a game (Find this position).
    Open(PathBuf, u32),
}

/// The filter field being typed into.
#[derive(Clone, Copy, PartialEq)]
enum Field {
    Player,
    Opening,
    Years,
    Elo,
}

/// A game open on the board.
struct Open {
    /// Its index in the file, from 0; for an EPD file, the position's.
    index: u32,
    game: chess::Game,
    layout: Vec<Token>,
    /// The move shown; None: the position before the first.
    at: Option<u32>,
}

/// A line played on from the board (`chess::play`), kept in the view until saved or cleared.
struct PlayedLine {
    /// The move it is played from (None: the start position).
    base: Option<u32>,
    moves: Vec<Played>,
    /// How many of its moves are shown: 0 is the base.
    at: usize,
}

/// A position on the board, and the squares of the move that led to it.
type Shown = (Option<String>, Option<(Option<u8>, Option<u8>)>);

/// One of the engine's lines stepped through without playing it (1–5, then ← →): the position
/// the engine was asked about, the line's place among its lines, its moves, and how many of them
/// are shown (1 at least). It ends when the game's position changes.
struct Browse {
    root: String,
    k: usize,
    steps: Vec<Played>,
    at: usize,
}

/// The engine asked about a position, on a thread: its position, the answer, and the flag
/// that stops it.
struct Thinking {
    fen: String,
    rx: Receiver<Result<Analysis, String>>,
    stop: Arc<AtomicBool>,
}

pub struct Viewer {
    pub path: PathBuf,
    lib: Option<Arc<Library>>,
    /// A game being read, on a thread of its own (a packed file is unpacked up to it).
    reading: Option<Receiver<(u32, std::io::Result<chess::Game>)>>,
    /// A Find hit's game: being looked for in the file, then waited for until the table has it.
    hit: Option<Receiver<Option<u32>>>,
    want: Option<u32>,
    positions: Vec<Position>,
    query: Query,
    /// The games the filter lets through, in order, and how many games there were then.
    ids: Vec<u32>,
    counted: usize,
    asked: Instant,
    cursor: usize,
    offset: usize,
    typing: Option<(Field, String)>,
    open: Option<Open>,
    flip: bool,
    notation: Notation,
    board: [Color; 2],
    /// The squares' own colours, light then dark: for pieces drawn from blocks, which have
    /// outlines of their own.
    squares: [Color; 2],
    pieces: Pieces,
    /// The pieces as pictures, made as the board needs them (`pictures`).
    pictures: Option<crate::pictures::Pieces>,
    said: Option<String>,
    /// The opening book whose moves are shown (`polyglot::find`), and the last position's.
    book: Option<PathBuf>,
    book_moves: Option<(String, Vec<BookMove>)>,
    /// The file is a book: its moves are played on the board from the start, `line` the
    /// positions before, `pick` the move chosen.
    book_only: bool,
    line: Vec<String>,
    pick: usize,
    /// The line played on from the board, and the move list being chosen from (what is typed
    /// to narrow it, the move chosen).
    played: Option<PlayedLine>,
    picking: Option<(String, usize)>,
    /// An engine line being stepped through: the board shows it, the engine stays on the game.
    browse: Option<Browse>,
    /// The engine: asked about each position while `analysis` is on.
    analysis: bool,
    /// The engine (`play::engine_for`), found when the view opens; or why there is none.
    program: Result<play::Program, String>,
    /// `[chess]`: the engine's limits, lines and options.
    chess: coxswain_core::config::ChessConfig,
    thinking: Option<Thinking>,
    analysed: Option<(String, Result<Analysis, String>)>,
    /// The game analysed as a whole (`g`).
    review: Option<Reviewer>,
    /// The search helper, and Find this position: the games it found (file, index) and the one
    /// chosen; asked on a thread.
    index: Option<Arc<coxswain_core::helper::Client>>,
    finding: Option<Receiver<Option<Vec<(PathBuf, u32)>>>>,
    found: Option<(Vec<(PathBuf, u32)>, usize)>,
    /// Whether the file was looked at for having one game only (`Some`), and whether it has:
    /// that game opens on its board, and Esc there closes the view.
    lone: Option<bool>,
}

impl Drop for Viewer {
    fn drop(&mut self) {
        if let Some(t) = &self.thinking {
            t.stop.store(true, Ordering::Relaxed);
        }
    }
}

fn rgb(hex: &str) -> Color {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x888888);
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// A light square two fifths of the way to the dark one.
fn mix(light: Color, dark: Color) -> Color {
    match (light, dark) {
        (Color::Rgb(a, b, c), Color::Rgb(x, y, z)) => {
            let m = |p: u8, q: u8| ((p as u16 * 3 + q as u16 * 2) / 5) as u8;
            Color::Rgb(m(a, x), m(b, y), m(c, z))
        }
        (l, _) => l,
    }
}

/// A colour halfway to `to`: the squares of the last move.
fn tint(c: Color, to: (u8, u8, u8)) -> Color {
    match c {
        Color::Rgb(r, g, b) => Color::Rgb(((r as u16 + to.0 as u16) / 2) as u8, ((g as u16 + to.1 as u16) / 2) as u8, ((b as u16 + to.2 as u16) / 2) as u8),
        c => c,
    }
}

/// A piece's glyph: the filled ones for both sides, told apart by their colour.
fn glyph(p: char) -> &'static str {
    match p.to_ascii_lowercase() {
        'k' => "♚",
        'q' => "♛",
        'r' => "♜",
        'b' => "♝",
        'n' => "♞",
        _ => "♟",
    }
}

impl Viewer {
    pub fn open(path: &Path, cfg: &coxswain_core::config::Config, theme: &Theme) -> std::io::Result<Viewer> {
        let mut v = Viewer {
            path: path.to_path_buf(),
            lib: None,
            positions: vec![],
            query: Query::default(),
            ids: vec![],
            counted: 0,
            asked: Instant::now(),
            cursor: 0,
            offset: 0,
            typing: None,
            open: None,
            flip: false,
            notation: Notation::from_config(&cfg.chess.notation),
            board: {
                // A character is small on its square: the light squares darker than the desktop
                // app's, so that a white piece stands out on them.
                let [light, dark] = chess::board_colours(theme, &cfg.chess.board).map(|c| rgb(&c));
                [mix(light, dark), dark]
            },
            squares: chess::board_colours(theme, &cfg.chess.board).map(|c| rgb(&c)),
            pieces: crate::pieces::pieces(cfg),
            pictures: crate::pictures::support().map(crate::pictures::Pieces::new),
            said: None,
            reading: None,
            hit: None,
            want: None,
            book: polyglot::find(path, &cfg.chess.book),
            book_moves: None,
            book_only: false,
            line: vec![],
            pick: 0,
            played: None,
            picking: None,
            browse: None,
            analysis: cfg.chess.analysis,
            program: play::engine_for(cfg),
            chess: cfg.chess.clone(),
            thinking: None,
            analysed: None,
            review: None,
            index: None,
            finding: None,
            found: None,
            lone: None,
        };
        match chess::sniff(path) {
            Some(Format::Pgn) => {
                let lib = Library::open(path)?;
                // Xiangqi in a PGN file: no opening book.
                if lib.format != Format::Pgn {
                    v.book = None;
                }
                v.lib = Some(Arc::new(lib));
            }
            // Draughts: the same table; no opening book.
            Some(Format::Pdn | Format::Kif | Format::Csa | Format::Pbn) => {
                v.lib = Some(Arc::new(Library::open(path)?));
                v.book = None;
            }
            Some(Format::Book) => {
                v.book_only = true;
                let game = chess::Game { start: chess::pgn::START.to_string(), ..Default::default() };
                v.open = Some(Open { index: 0, layout: vec![], game, at: None });
            }
            _ => {
                v.positions = chess::read_positions(path)?;
                v.show_position(0);
            }
        }
        if v.pieces != Pieces::Pictures {
            v.pictures = None;
            if cfg.chess.pieces == "pictures" {
                v.said = Some(t!("chess.pictures_none"));
            }
        }
        Ok(v)
    }

    /// Shows the game a Find hit's snippet is about, once it is found (on a thread: the file is
    /// read from its start for it) and the table has reached it.
    pub fn at_hit(&mut self, snippet: String) {
        let (tx, rx) = std::sync::mpsc::channel();
        let path = self.path.clone();
        std::thread::spawn(move || tx.send(chess::games::game_of_hit(&path, &snippet)));
        self.hit = Some(rx);
        self.said = Some(t!("common.loading"));
    }

    /// The table follows the reading: asked again when there are more games, twice a second.
    /// The engine is asked about the position shown, once per position.
    pub fn poll(&mut self) {
        self.poll_engine();
        if let Some(r) = self.finding.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.finding = None;
            match r {
                None => self.said = Some(t!("chess.position_off")),
                Some(v) if v.is_empty() => self.said = Some(t!("chess.position_none")),
                Some(v) => self.found = Some((v, 0)),
            }
        }
        if let Some(r) = self.hit.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.hit = None;
            self.said = None;
            self.want = r;
        }
        if let Some((index, r)) = self.reading.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.reading = None;
            self.said = None;
            match r {
                Ok(game) => {
                    let layout = game.layout();
                    self.said = game.error.as_ref().map(|e| t!("chess.cannot_play", "move" => e));
                    self.open = Some(Open { index, game, layout, at: None });
                    self.played = None;
                    self.browse = None;
                    self.review = None;
                }
                Err(e) => self.said = Some(e.to_string()),
            }
        }
        let Some(lib) = &self.lib else { return };
        let p = lib.progress();
        if p.games != self.counted && (self.asked.elapsed().as_millis() >= 500 || p.done || self.counted == 0) {
            self.ids = lib.query(&self.query);
            self.counted = p.games;
            self.asked = Instant::now();
            self.cursor = self.cursor.min(self.ids.len().saturating_sub(1));
        }
        // A file of one game opens on its board, once it is read to its end.
        if self.lone.is_none() && p.done {
            let one = p.games == 1 && self.want.is_none() && self.hit.is_none() && self.open.is_none();
            self.lone = Some(one);
            if one {
                self.show_game(0);
            }
        }
        // The hit's game, once the table has it.
        if let Some(n) = self.want
            && (p.games as u32 > n || p.done)
        {
            self.want = None;
            if let Some(i) = self.ids.iter().position(|&g| g == n) {
                self.cursor = i;
            }
            self.show_game(n);
        }
    }

    /// Draughts or xiangqi: a board of its own, without openings, figurines or a book.
    fn other_game(&self) -> bool {
        self.lib.as_ref().is_some_and(|l| l.format != Format::Pgn)
    }

    /// The feature F1 shows for it: chess, draughts or xiangqi.
    pub fn feature(&self) -> &'static str {
        match self.lib.as_ref().map(|l| l.format) {
            Some(Format::Pdn) => "draughts",
            Some(Format::Xiangqi) => "xiangqi",
            Some(Format::Kif | Format::Csa) => "shogi",
            Some(Format::Pbn) => "bridge",
            _ => "chess",
        }
    }

    /// A PBN file of bridge deals.
    fn bridge(&self) -> bool {
        self.lib.as_ref().is_some_and(|l| l.format == Format::Pbn)
    }

    /// A KIF or CSA file of shogi games.
    fn shogi(&self) -> bool {
        self.lib.as_ref().is_some_and(|l| matches!(l.format, Format::Kif | Format::Csa))
    }

    /// A PGN file of xiangqi games.
    fn xiangqi(&self) -> bool {
        self.lib.as_ref().is_some_and(|l| l.format == Format::Xiangqi)
    }

    /// A PDN file: draughts on its own board, without openings or figurines.
    fn draughts(&self) -> bool {
        self.lib.as_ref().is_some_and(|l| l.format == Format::Pdn)
    }

    /// The position on the board, and the squares of the move that led to it: an engine line's
    /// move while one is stepped through, else the game's (or your line's).
    fn now(&self) -> Shown {
        if let Some(b) = self.browsing() {
            let m = &b.steps[b.at - 1];
            return (Some(m.fen.clone()), Some((Some(m.from), Some(m.to))));
        }
        self.game_now()
    }

    /// The engine line stepped through, while the game is still at the position it was asked
    /// about.
    fn browsing(&self) -> Option<&Browse> {
        self.browse.as_ref().filter(|b| self.game_now().0.as_deref() == Some(b.root.as_str()))
    }

    /// The engine's line `k` (from 0) of the game's position, stepped into at its first move.
    fn browse_line(&mut self, k: usize) {
        let root = self.game_now().0;
        let (steps, lines) = match &self.analysed {
            Some((f, Ok(a))) if Some(f) == root.as_ref() => (a.lines.get(k).map(|l| l.steps.clone()).unwrap_or_default(), a.lines.len()),
            _ => (vec![], 0),
        };
        match root.filter(|_| !steps.is_empty()) {
            Some(root) => self.browse = Some(Browse { root, k, steps, at: 1 }),
            // The engine answered with fewer lines than the one asked for.
            None if k >= lines && lines > 0 => self.said = Some(tn!("chess.engine_only_lines", lines)),
            None => self.said = Some(t!("chess.engine_no_best")),
        }
    }

    /// The engine line stepped through, played into the game up to the move shown, as moves
    /// played on the board are.
    fn play_browsed(&mut self) {
        let Some(b) = self.browse.take().filter(|b| self.game_now().0.as_deref() == Some(b.root.as_str())) else { return };
        for m in b.steps.into_iter().take(b.at) {
            self.play(m);
        }
    }

    /// The game's position (or your line's), whatever engine line is stepped through.
    fn game_now(&self) -> Shown {
        if let Some(l) = self.played.as_ref().filter(|l| l.at > 0) {
            let m = &l.moves[l.at - 1];
            return (Some(m.fen.clone()), Some((Some(m.from), Some(m.to))));
        }
        let Some(o) = &self.open else { return (None, None) };
        match o.at {
            Some(n) => {
                let n = &o.game.nodes[n as usize];
                (n.fen.clone(), Some((n.from, n.to)))
            }
            None => (Some(o.game.start.clone()), None),
        }
    }

    /// The squares and arrows the game's comments draw on the position shown ([%csl], [%cal]).
    fn drawn(&self) -> Vec<chess::pgn::Mark> {
        if self.played.as_ref().is_some_and(|l| l.at > 0) || self.browsing().is_some() {
            return vec![];
        }
        let Some(o) = &self.open else { return vec![] };
        match o.at {
            Some(n) => o.game.nodes[n as usize].marks.clone(),
            None => o.game.marks.clone(),
        }
    }

    fn poll_engine(&mut self) {
        if let Some(t) = &self.thinking
            && let Ok(r) = t.rx.try_recv()
        {
            // Options set for the engine that it did not take: said once a session.
            if let Ok(note) = r.as_ref().map(|a| a.note.clone())
                && note.is_some()
            {
                self.said = note;
            }
            self.analysed = Some((t.fen.clone(), r));
            self.thinking = None;
        }
        // The game's position: an engine line stepped through leaves the engine on it.
        let fen = self.game_now().0.filter(|_| self.analysis && self.open.is_some());
        let Some(fen) = fen else {
            if let Some(t) = self.thinking.take() {
                t.stop.store(true, Ordering::Relaxed);
            }
            return;
        };
        if self.thinking.as_ref().is_some_and(|t| t.fen == fen) || self.analysed.as_ref().is_some_and(|a| a.0 == fen) {
            return;
        }
        if let Some(t) = self.thinking.take() {
            t.stop.store(true, Ordering::Relaxed);
        }
        let program = match &self.program {
            Ok(p) => p.clone(),
            Err(why) => {
                self.analysed = Some((fen, Err(why.clone())));
                return;
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (f, s, chess) = (fen.clone(), stop.clone(), self.chess.clone());
        std::thread::spawn(move || {
            let r = play::analyse(&program, &chess, &f, &|| s.load(Ordering::Relaxed)).map_err(|e| e.to_string());
            let _ = tx.send(r);
        });
        self.thinking = Some(Thinking { fen, rx, stop });
    }

    /// A move played from the position shown: the game's own next move (or a variation's) is
    /// stepped to; any other starts or goes on with the played line, cut after the move shown.
    fn play(&mut self, m: Played) {
        // From an engine line stepped through: the line up to there first.
        self.play_browsed();
        let in_line = self.played.as_ref().is_some_and(|l| l.at > 0);
        if self.book_only && !in_line {
            if let Some(o) = self.open.as_mut() {
                self.line.push(std::mem::replace(&mut o.game.start, m.fen));
                self.pick = 0;
            }
            return;
        }
        if !in_line && let Some(o) = self.open.as_mut() {
            let g = &o.game;
            let next: &[u32] = match o.at {
                Some(n) => &g.nodes[n as usize].children,
                None => &g.roots,
            };
            if let Some(&n) = next.iter().find(|&&n| g.nodes[n as usize].fen.as_deref() == Some(m.fen.as_str())) {
                o.at = Some(n);
                return;
            }
            let base = self.open.as_ref().and_then(|o| o.at);
            self.played = Some(PlayedLine { base, moves: vec![m], at: 1 });
            return;
        }
        if let Some(l) = self.played.as_mut() {
            l.moves.truncate(l.at);
            l.moves.push(m);
            l.at = l.moves.len();
        }
    }

    /// The played line as a new PGN file next to this one: the game's moves up to its base,
    /// then the line.
    fn save_line(&mut self) {
        let (Some(l), Some(o)) = (&self.played, &self.open) else {
            self.said = Some(t!("chess.line_none"));
            return;
        };
        let g = &o.game;
        let mut sans: Vec<String> = g.path_to(l.base).iter().map(|&n| g.nodes[n as usize].san.clone()).collect();
        sans.extend(l.moves.iter().map(|m| m.san.clone()));
        let text = play::line_pgn(&g.tags, &g.start, &sans);
        self.said = Some(match play::save_beside(&self.path, "line", &text) {
            Ok(p) => t!("chess.line_saved", "name" => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            Err(e) => t!("chess.line_not_saved", "error" => e),
        });
    }

    /// The search helper, for Find this position.
    pub fn with_index(mut self, index: Arc<coxswain_core::helper::Client>) -> Viewer {
        self.index = Some(index);
        self
    }

    /// `p`: the games in the search store that reach the position shown, asked on a thread.
    fn find_position(&mut self) {
        let (Some(index), Some(b)) = (self.index.clone(), self.now().0.as_deref().and_then(chess::pgn::board)) else { return };
        let key = polyglot::key(&b);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(index.games_at(key, 500));
        });
        self.finding = Some(rx);
        self.said = Some(t!("chess.position_searching"));
    }

    /// A key while the games found are listed: ↑ ↓ choose, Enter opens one, Esc closes.
    fn found_key(&mut self, key: Key) -> Outcome {
        let Some((list, mut sel)) = self.found.take() else { return Outcome::Stay };
        match key.code {
            KeyCode::Esc => return Outcome::Stay,
            KeyCode::Enter => {
                let Some((path, game)) = list.get(sel).cloned() else { return Outcome::Stay };
                if path == self.path {
                    if let Some(i) = self.ids.iter().position(|&g| g == game) {
                        self.cursor = i;
                    }
                    self.show_game(game);
                    return Outcome::Stay;
                }
                return Outcome::Open(path, game);
            }
            KeyCode::Down => sel = (sel + 1).min(list.len().saturating_sub(1)),
            KeyCode::Up => sel = sel.saturating_sub(1),
            KeyCode::PageDown => sel = (sel + 20).min(list.len().saturating_sub(1)),
            KeyCode::PageUp => sel = sel.saturating_sub(20),
            _ => {}
        }
        self.found = Some((list, sel));
        Outcome::Stay
    }

    /// Opens game `n` once the table has it, as a Find hit's.
    pub fn at_game(&mut self, n: u32) {
        self.want = Some(n);
    }

    /// `g`: the whole game analysed in the background, or stopped while it runs.
    fn review_key(&mut self) {
        if let Some(r) = &self.review
            && !r.now().done
        {
            r.stop();
            return;
        }
        let Some(o) = &self.open else { return };
        match &self.program {
            Ok(program) => self.review = Some(Reviewer::start(program, &self.chess, &o.game)),
            Err(why) => self.said = Some(why.clone()),
        }
    }

    /// `w`: the game with the analysis in it, as a new PGN file next to this one.
    fn save_review(&mut self) {
        let (Some(r), Some(o)) = (self.review.as_ref().map(|r| r.now()), &self.open) else {
            self.said = Some(t!("chess.review_first"));
            return;
        };
        if r.plies.is_empty() {
            self.said = Some(t!("chess.review_first"));
            return;
        }
        let text = review::annotated_pgn(&o.game, &r);
        self.said = Some(match play::save_beside(&self.path, "annotated", &text) {
            Ok(p) => t!("chess.line_saved", "name" => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            Err(e) => t!("chess.line_not_saved", "error" => e),
        });
    }

    /// The evaluation of the position shown, from White's side: the game's analysis, else the
    /// engine's for this position, else the one the game's comment gives.
    fn eval_now(&self, r: Option<&Review>, fen: Option<&str>) -> Option<i32> {
        let cp = |l: &play::EngineLine| match (l.mate, l.cp) {
            (Some(m), _) if m > 0 => Some(review::CAP + 100 - m.min(99)),
            (Some(m), _) if m < 0 => Some(-(review::CAP + 100 + m.max(-99))),
            (_, c) => c,
        };
        // An engine line stepped through: its own evaluation.
        if let Some(b) = self.browsing() {
            return match &self.analysed {
                Some((_, Ok(a))) => a.lines.get(b.k).and_then(cp),
                _ => None,
            };
        }
        let in_line = self.played.as_ref().is_some_and(|l| l.at > 0);
        if let (Some(r), Some(o), false) = (r, &self.open, in_line) {
            let v = match o.at {
                None => r.start,
                Some(n) => r.plies.iter().find(|p| p.node == n).map(|p| p.cp),
            };
            if v.is_some() {
                return v;
            }
        }
        let engine = match &self.analysed {
            Some((f, Ok(a))) if Some(f.as_str()) == fen => a.lines.first().and_then(cp),
            _ => None,
        };
        // Else the game's own, from its comment (`[%eval]`, a ChessBase evaluation).
        engine.or_else(|| self.open.as_ref().filter(|_| !in_line).and_then(|o| o.at).and_then(|n| self.open.as_ref()?.game.nodes.get(n as usize)?.eval))
    }

    /// A key while the move list is open: typing narrows it, ↑ ↓ choose, Enter plays.
    fn pick_key(&mut self, key: Key) -> Outcome {
        let Some((mut text, mut sel)) = self.picking.take() else { return Outcome::Stay };
        let list = self.pick_list(&text);
        match key.code {
            KeyCode::Esc => return Outcome::Stay,
            KeyCode::Enter => {
                if let Some(m) = list.get(sel) {
                    self.play(m.clone());
                }
                return Outcome::Stay;
            }
            KeyCode::Down | KeyCode::Right | KeyCode::Tab => sel = (sel + 1).min(list.len().saturating_sub(1)),
            KeyCode::Up | KeyCode::Left => sel = sel.saturating_sub(1),
            KeyCode::Backspace => {
                text.pop();
                sel = 0;
            }
            _ if crate::typed(&key).is_some() => {
                text.extend(crate::typed(&key));
                sel = 0;
            }
            _ => {}
        }
        self.picking = Some((text, sel));
        Outcome::Stay
    }

    /// The legal moves of the position shown that `typed` starts (in English SAN, as shown, or
    /// in UCI; `x`, `+` and `=` need not be typed).
    fn pick_list(&self, typed: &str) -> Vec<Played> {
        let Some(fen) = self.now().0 else { return vec![] };
        let pieces = chess::pieces(self.notation);
        let bare = |s: &str| s.chars().filter(|c| !"x+#=-".contains(*c)).collect::<String>().to_lowercase();
        let want = bare(typed);
        play::moves(&fen)
            .into_iter()
            .filter(|m| want.is_empty() || [bare(&m.san), bare(&chess::show_san(&m.san, &pieces)), m.uci.clone()].iter().any(|s| s.starts_with(&want)))
            .collect()
    }

    fn requery(&mut self) {
        if let Some(lib) = &self.lib {
            self.ids = lib.query(&self.query);
            self.counted = lib.progress().games;
            self.asked = Instant::now();
            self.cursor = 0;
            self.offset = 0;
        }
    }

    /// Reads game `index` on a thread; `poll` shows it.
    fn show_game(&mut self, index: u32) {
        let Some(lib) = self.lib.clone() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send((index, lib.game(index)));
        });
        self.reading = Some(rx);
        self.said = Some(t!("common.loading"));
    }

    fn show_position(&mut self, i: usize) {
        let Some(p) = self.positions.get(i) else { return };
        let game = chess::Game { start: p.fen.clone().unwrap_or_default(), comment: p.about.clone(), ..Default::default() };
        self.said = p.fen.is_none().then(|| t!("chess.no_position", "line" => p.line));
        self.played = None;
        self.open = Some(Open { index: i as u32, layout: game.layout(), game, at: None });
    }

    /// A key, and the action the keymap binds it to.
    pub fn key(&mut self, key: Key, action: Option<Action>) -> Outcome {
        if let Some((field, mut text)) = self.typing.take() {
            match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let num = |s: &str| s.trim().parse::<u16>().ok();
                    match field {
                        Field::Player => self.query.player = text,
                        Field::Opening => self.query.opening = text,
                        Field::Elo => self.query.elo = num(&text),
                        Field::Years => {
                            let (a, b) = text.split_once('-').unwrap_or((&text, &text));
                            (self.query.from, self.query.to) = (num(a), num(b));
                        }
                    }
                    self.requery();
                }
                KeyCode::Backspace => {
                    text.pop();
                    self.typing = Some((field, text));
                }
                _ if crate::typed(&key).is_some() => {
                    text.extend(crate::typed(&key));
                    self.typing = Some((field, text));
                }
                _ => self.typing = Some((field, text)),
            }
            return Outcome::Stay;
        }
        if action == Some(Action::Quit) {
            return Outcome::Close;
        }
        self.said = None;
        if self.open.is_some() {
            return self.game_key(key);
        }
        let n = self.ids.len();
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down => self.cursor = (self.cursor + 1).min(n.saturating_sub(1)),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(20),
            KeyCode::PageDown => self.cursor = (self.cursor + 20).min(n.saturating_sub(1)),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = n.saturating_sub(1),
            KeyCode::Enter => {
                if let Some(&g) = self.ids.get(self.cursor) {
                    self.show_game(g);
                }
            }
            KeyCode::Char('/') => self.typing = Some((Field::Player, self.query.player.clone())),
            KeyCode::Char('o') if !self.other_game() || self.bridge() => self.typing = Some((Field::Opening, self.query.opening.clone())),
            KeyCode::Char('e') => self.typing = Some((Field::Elo, self.query.elo.map(|e| e.to_string()).unwrap_or_default())),
            KeyCode::Char('y') => {
                let years = match (self.query.from, self.query.to) {
                    (None, None) => String::new(),
                    (a, b) => format!("{}-{}", a.map(|y| y.to_string()).unwrap_or_default(), b.map(|y| y.to_string()).unwrap_or_default()),
                };
                self.typing = Some((Field::Years, years));
            }
            KeyCode::Char('r') => {
                let all = ["", "1-0", "0-1", "1/2-1/2"];
                let at = all.iter().position(|r| *r == self.query.result).unwrap_or(0);
                self.query.result = all[(at + 1) % all.len()].to_string();
                self.requery();
            }
            KeyCode::Char('s') => {
                let at = Sort::ALL.iter().position(|s| *s == self.query.sort).unwrap_or(0);
                self.query.sort = Sort::ALL[(at + 1) % Sort::ALL.len()];
                self.requery();
            }
            KeyCode::Char('S') => {
                self.query.desc = !self.query.desc;
                self.requery();
            }
            KeyCode::Char('c') => {
                self.query = Query { sort: self.query.sort, desc: self.query.desc, ..Query::default() };
                self.requery();
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// The book's moves for `fen`, read once per position.
    fn book_moves(&mut self, fen: &str) -> &[BookMove] {
        if self.book_moves.as_ref().is_none_or(|(f, _)| f != fen) {
            let moves = self.book.as_deref().map(|b| polyglot::moves_at(b, fen)).unwrap_or_default();
            self.book_moves = Some((fen.to_string(), moves));
        }
        &self.book_moves.as_ref().expect("set above").1
    }

    /// A key in a book of its own: its moves are chosen and played.
    fn book_key(&mut self, key: Key) -> Outcome {
        let Some(fen) = self.open.as_ref().map(|o| o.game.start.clone()) else { return Outcome::Close };
        let n = self.book_moves(&fen).len();
        let mut to = None;
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Down => self.pick = (self.pick + 1).min(n.saturating_sub(1)),
            KeyCode::Up => self.pick = self.pick.saturating_sub(1),
            KeyCode::Enter | KeyCode::Right => {
                let pick = self.pick;
                if let Some(m) = self.book_moves(&fen).get(pick) {
                    to = Some(m.fen.clone());
                    self.line.push(fen);
                }
            }
            KeyCode::Left | KeyCode::Backspace => to = self.line.pop(),
            KeyCode::Home => {
                to = self.line.first().cloned();
                self.line.clear();
            }
            KeyCode::Char('f') => self.flip = !self.flip,
            KeyCode::Char('n') => {
                self.notation = if self.notation == Notation::Figurines { Notation::Letters } else { Notation::Figurines };
            }
            _ => {}
        }
        if let (Some(fen), Some(o)) = (to, self.open.as_mut()) {
            o.game.start = fen;
            self.pick = 0;
        }
        Outcome::Stay
    }

    fn game_key(&mut self, key: Key) -> Outcome {
        if self.picking.is_some() {
            return self.pick_key(key);
        }
        if self.found.is_some() {
            return self.found_key(key);
        }
        // An engine line stepped through: → ← step it, Enter plays it up to the move shown, Esc
        // (or ← on its first move) goes back to the game. f, n, m and 1–5 keep it; any other
        // key leaves it and does what it does in the game.
        if self.browsing().is_some()
            && let Some(b) = self.browse.as_mut()
        {
            match key.code {
                KeyCode::Right => {
                    b.at = (b.at + 1).min(b.steps.len());
                    return Outcome::Stay;
                }
                KeyCode::Left if b.at > 1 => {
                    b.at -= 1;
                    return Outcome::Stay;
                }
                KeyCode::Left | KeyCode::Esc => {
                    self.browse = None;
                    return Outcome::Stay;
                }
                KeyCode::Enter => {
                    self.play_browsed();
                    return Outcome::Stay;
                }
                KeyCode::Char('f' | 'n' | 'm' | '1'..='5') => {}
                _ => self.browse = None,
            }
        } else {
            self.browse = None;
        }
        // Playing on, the engine, the played line: the same in a game, a position and a book;
        // not on a draughts board.
        match key.code {
            _ if self.other_game() => {}
            KeyCode::Char(c @ '1'..='5') => {
                self.browse_line(c as usize - '1' as usize);
                return Outcome::Stay;
            }
            KeyCode::Char('m') => {
                self.picking = Some((String::new(), 0));
                if self.pick_list("").is_empty() {
                    self.picking = None;
                    self.said = Some(t!("chess.no_moves"));
                }
                return Outcome::Stay;
            }
            KeyCode::Char('a') => {
                self.analysis = !self.analysis;
                if !self.analysis {
                    self.analysed = None;
                }
                self.poll_engine();
                return Outcome::Stay;
            }
            KeyCode::Char('b') => {
                let fen = self.now().0;
                let best = match &self.analysed {
                    Some((f, Ok(a))) if Some(f) == fen.as_ref() => a.lines.first().and_then(|l| l.best.clone()),
                    _ => None,
                };
                match best {
                    Some(m) => self.play(m),
                    None => self.said = Some(t!("chess.engine_no_best")),
                }
                return Outcome::Stay;
            }
            KeyCode::Char('s') => {
                self.save_line();
                return Outcome::Stay;
            }
            KeyCode::Char('p') => {
                self.find_position();
                return Outcome::Stay;
            }
            KeyCode::Char('g') if self.lib.is_some() => {
                self.review_key();
                return Outcome::Stay;
            }
            KeyCode::Char('w') if self.lib.is_some() => {
                self.save_review();
                return Outcome::Stay;
            }
            KeyCode::Char('c') if self.played.is_some() => {
                self.played = None;
                return Outcome::Stay;
            }
            _ => {}
        }
        let epd = self.lib.is_none() && !self.book_only;
        // In the played line, ← → step it; → on the move it starts from enters it again. In a
        // position or a book, ← → step it whenever there is one.
        let base_here = self.played.as_ref().is_some_and(|l| l.at == 0 && (epd || self.book_only || self.open.as_ref().is_some_and(|o| o.at == l.base)));
        if let Some(l) = self.played.as_mut()
            && (l.at > 0 || base_here)
        {
            match key.code {
                KeyCode::Right => {
                    l.at = (l.at + 1).min(l.moves.len());
                    return Outcome::Stay;
                }
                KeyCode::Left if l.at > 0 => {
                    l.at -= 1;
                    return Outcome::Stay;
                }
                KeyCode::Left if epd || self.book_only => return Outcome::Stay,
                KeyCode::Home | KeyCode::End | KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => l.at = 0,
                _ => {}
            }
        }
        if self.book_only {
            return self.book_key(key);
        }
        let draughts = self.other_game();
        if epd {
            let i = self.open.as_ref().map_or(0, |o| o.index as usize);
            match key.code {
                KeyCode::Esc => return Outcome::Close,
                KeyCode::Right => self.show_position((i + 1).min(self.positions.len().saturating_sub(1))),
                KeyCode::Left => self.show_position(i.saturating_sub(1)),
                KeyCode::Home => self.show_position(0),
                KeyCode::End => self.show_position(self.positions.len().saturating_sub(1)),
                _ => {}
            }
        }
        let Some(o) = self.open.as_mut() else { return Outcome::Stay };
        let g = &o.game;
        let node = |n: u32| &g.nodes[n as usize];
        // The moves this one could be instead: its parent's children, or the first moves.
        let siblings = |at: u32| -> &[u32] {
            match node(at).parent {
                Some(p) => &node(p).children,
                None => &g.roots,
            }
        };
        match key.code {
            _ if epd && !matches!(key.code, KeyCode::Char(_)) => {}
            // A file of one game has no table to go back to.
            KeyCode::Esc if self.lone == Some(true) && self.played.is_none() && self.review.is_none() => return Outcome::Close,
            KeyCode::Esc => {
                self.open = None;
                self.played = None;
                self.review = None;
            }
            KeyCode::Right => {
                o.at = match o.at {
                    None => g.roots.first().copied(),
                    Some(n) => node(n).children.first().copied().or(Some(n)),
                }
            }
            KeyCode::Left => o.at = o.at.and_then(|n| node(n).parent),
            KeyCode::Home => o.at = None,
            KeyCode::End => {
                let mut at = o.at.or(g.roots.first().copied());
                while let Some(next) = at.and_then(|n| node(n).children.first().copied()) {
                    at = Some(next);
                }
                o.at = at;
            }
            // Into the variations of this move and back.
            KeyCode::Down | KeyCode::Up => {
                if let Some(n) = o.at {
                    let s = siblings(n);
                    let i = s.iter().position(|&x| x == n).unwrap_or(0);
                    let j = if key.code == KeyCode::Down { (i + 1).min(s.len() - 1) } else { i.saturating_sub(1) };
                    o.at = Some(s[j]);
                }
            }
            KeyCode::PageDown | KeyCode::PageUp if !epd => {
                let i = self.ids.iter().position(|&x| x == o.index).unwrap_or(0);
                let j = if key.code == KeyCode::PageDown { (i + 1).min(self.ids.len().saturating_sub(1)) } else { i.saturating_sub(1) };
                if let Some(&next) = self.ids.get(j) {
                    self.cursor = j;
                    self.show_game(next);
                }
            }
            KeyCode::Char('f') => self.flip = !self.flip,
            KeyCode::Char('n') if !draughts => {
                self.notation = if self.notation == Notation::Figurines { Notation::Letters } else { Notation::Figurines };
            }
            _ => {}
        }
        Outcome::Stay
    }

    pub fn draw(&mut self, f: &mut Frame, theme: &Theme) {
        let area = f.area().inner(Margin::new(1, 0));
        let base = sty(&theme.dialog);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
        let title = self.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(sty(&theme.dialog_border).bg(bg))
            .style(base)
            .title(Line::from(format!(" ♞ {title} ")).centered());
        f.render_widget(Clear, area);
        let inner = block.inner(area).inner(Margin::new(1, 0));
        f.render_widget(block, area);
        if inner.height < 4 || inner.width < 20 {
            return;
        }
        let foot = Rect { y: inner.bottom() - 1, height: 1, ..inner };
        let said = Rect { y: inner.bottom() - 2, height: 1, ..inner };
        let body = Rect { height: inner.height - 2, ..inner };
        let w = inner.width as usize;
        let keys = if self.picking.is_some() {
            t!("chess.keys_pick")
        } else if self.found.is_some() {
            t!("chess.keys_found")
        } else if self.book_only {
            t!("chess.keys_book")
        } else if self.open.is_some() {
            if self.bridge() {
                t!("bridge.keys_game")
            } else if self.other_game() {
                t!("draughts.keys_game")
            } else if self.lib.is_some() {
                t!("chess.keys_game")
            } else {
                t!("chess.keys_position")
            }
        } else if self.bridge() {
            t!("bridge.keys_list")
        } else if self.other_game() {
            t!("draughts.keys_list")
        } else {
            t!("chess.keys_list")
        };
        f.render_widget(Paragraph::new(fit(&keys, w)).style(dim), foot);
        let line = match &self.typing {
            Some((field, text)) => {
                let label = match field {
                    Field::Player => t!("chess.filter_player"),
                    Field::Opening => t!("chess.filter_opening"),
                    Field::Years => t!("chess.filter_years"),
                    Field::Elo => t!("chess.filter_elo"),
                };
                Some((format!("{label}: {text}▏"), sty(&theme.dialog_input)))
            }
            None => match &self.picking {
                Some((text, _)) => Some((format!("{}: {text}▏", t!("chess.pick")), sty(&theme.dialog_input))),
                None => self.said.clone().map(|s| (s, dim)),
            },
        };
        if let Some((s, style)) = line {
            f.render_widget(Paragraph::new(fit(&s, w)).style(style), said);
        }
        if self.open.is_some() {
            self.draw_game(f, theme, body);
        } else {
            self.draw_list(f, theme, body);
        }
    }

    fn draw_list(&mut self, f: &mut Frame, theme: &Theme, area: Rect) {
        let base = sty(&theme.dialog);
        let dim = faint(theme).bg(base.bg.unwrap_or(Color::Reset));
        let Some(lib) = &self.lib else { return };
        let p = lib.progress();
        let w = area.width as usize;
        // The state: how many, the filter, the order.
        let mut state = t!("chess.games", "shown" => self.ids.len(), "total" => p.games);
        if !p.done {
            let pct = (p.read * 100).checked_div(p.size).unwrap_or(0);
            state = format!("{state} · {}", t!("chess.reading", "percent" => pct));
        }
        if p.capped {
            state = format!("{state} · {}", t!("chess.capped", "n" => p.games));
        }
        if let Some(e) = &p.error {
            state = format!("{state} · {e}");
        }
        let q = &self.query;
        let mut filter = vec![];
        for (label, v) in [(t!("chess.filter_player"), q.player.clone()), (t!("chess.filter_opening"), q.opening.clone()), (t!("chess.col_result"), q.result.clone())] {
            if !v.is_empty() {
                filter.push(format!("{label} {v}"));
            }
        }
        if q.from.is_some() || q.to.is_some() {
            filter.push(format!("{} {}–{}", t!("chess.filter_years"), q.from.map(|y| y.to_string()).unwrap_or_default(), q.to.map(|y| y.to_string()).unwrap_or_default()));
        }
        if let Some(e) = q.elo {
            filter.push(format!("{} ≥ {e}", t!("chess.filter_elo")));
        }
        if !filter.is_empty() {
            state = format!("{state} · {}", filter.join(", "));
        }
        state = format!("{state} · {} {}{}", t!("chess.sorted_by"), column(q.sort), if q.desc { " ↓" } else { " ↑" });
        f.render_widget(Paragraph::new(fit(&state, w)).style(dim), Rect { height: 1, ..area });
        // Columns: fixed ones, and the names sharing the rest.
        // Draughts has no openings: their two columns go to the names and the event.
        let bridge = lib.format == Format::Pbn;
        let draughts = lib.format != Format::Pgn && !bridge;
        // Bridge: the pairs, no ratings, the contract where the opening stands, the tricks.
        let fixed = if bridge {
            [7usize, 0, 0, 12, 10, 0, 6]
        } else if draughts {
            [7usize, 5, 5, 7, 10, 0, 6]
        } else {
            [7usize, 5, 5, 7, 10, 4, 6]
        };
        let rest = w.saturating_sub(fixed.iter().sum::<usize>() + 10);
        let name = (rest * if draughts { 32 } else if bridge { 26 } else { 22 } / 100).max(6);
        let event = if draughts { rest.saturating_sub(2 * name).max(4) } else { (rest * 20 / 100).max(4) };
        let opening = if draughts { 0 } else if bridge { rest.saturating_sub(2 * name + event).max(6) } else { rest.saturating_sub(2 * name + event).max(4) };
        let row = |cells: [String; 11]| -> String {
            let widths = [fixed[0], name, fixed[1], name, fixed[2], fixed[3], fixed[4], event, fixed[5], opening, fixed[6]];
            let mut out = String::new();
            for (i, (c, cw)) in cells.iter().zip(widths).enumerate() {
                if cw == 0 {
                    continue;
                }
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&fit(c, cw));
            }
            out
        };
        let head = row([
            "#".into(),
            match lib.format {
                Format::Xiangqi => t!("xiangqi.col_red"),
                Format::Kif | Format::Csa => t!("shogi.col_sente"),
                Format::Pbn => t!("bridge.col_ns"),
                _ => t!("chess.col_white"),
            },
            t!("chess.col_elo"),
            match lib.format {
                Format::Kif | Format::Csa => t!("shogi.col_gote"),
                Format::Pbn => t!("bridge.col_ew"),
                _ => t!("chess.col_black"),
            },
            t!("chess.col_elo"),
            t!("chess.col_result"),
            t!("chess.col_date"),
            t!("chess.col_event"),
            "ECO".into(),
            if bridge { t!("bridge.col_contract") } else { t!("chess.col_opening") },
            if bridge { t!("bridge.col_tricks") } else { t!("chess.col_moves") },
        ]);
        f.render_widget(Paragraph::new(fit(&head, w)).style(base.add_modifier(Modifier::BOLD)), Rect { y: area.y + 1, height: 1, ..area });
        let h = area.height.saturating_sub(2) as usize;
        if self.ids.is_empty() {
            let s = if p.done { t!("chess.no_games") } else { t!("common.loading") };
            f.render_widget(Paragraph::new(fit(&s, w)).style(dim), Rect { y: area.y + 2, height: 1, ..area });
            return;
        }
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + h {
            self.offset = self.cursor + 1 - h;
        }
        for (k, &id) in self.ids.iter().enumerate().skip(self.offset).take(h) {
            let Some(r) = lib.row(id) else { continue };
            let elo = |e: Option<u16>| e.map(|e| e.to_string()).unwrap_or_default();
            let text = row([
                r.n.to_string(),
                r.white,
                elo(r.white_elo),
                r.black,
                elo(r.black_elo),
                r.result,
                r.date,
                r.event,
                r.eco,
                r.opening,
                r.moves.to_string(),
            ]);
            let style = if k == self.cursor { sty(&theme.dialog_input) } else { base };
            let y = area.y + 2 + (k - self.offset) as u16;
            f.render_widget(Paragraph::new(fit(&text, w)).style(style), Rect { y, height: 1, ..area });
        }
    }

    fn draw_game(&mut self, f: &mut Frame, theme: &Theme, area: Rect) {
        let base = sty(&theme.dialog);
        let bg = base.bg.unwrap_or(Color::Reset);
        let dim = faint(theme).bg(bg);
        if self.open.is_none() {
            return;
        }
        let (now, mut last) = self.now();
        let picks = self.picking.as_ref().map(|(text, sel)| (self.pick_list(text), *sel));
        if let Some((list, sel)) = &picks
            && let Some(m) = list.get(*sel)
        {
            last = Some((Some(m.from), Some(m.to)));
        }
        let book: Vec<BookMove> = match &now {
            Some(fen) if self.book.is_some() => self.book_moves(fen).to_vec(),
            _ => vec![],
        };
        if self.other_game() {
            let (lines, board_w, placed) = if self.bridge() {
                let (l, w) = self.bridge_table(dim);
                (l, w, vec![])
            } else if self.xiangqi() {
                self.xiangqi_board(area, dim)
            } else if self.shogi() {
                self.shogi_board(area, dim)
            } else {
                self.draughts_board(area, dim)
            };
            f.render_widget(Paragraph::new(lines), Rect { width: board_w.min(area.width), ..area });
            if let Some(pics) = self.pictures.as_mut() {
                pics.place(f.buffer_mut(), (area.x, area.y), area, placed);
            }
            return self.draw_moves(f, theme, area, board_w, vec![]);
        }
        // The board: as big as fits, a square twice as wide as high, the moves beside it.
        let (pieces, cell_w, cell_h) = crate::pieces::layout(self.pieces, &mosaic::CHESS, 8, 8, (area.width * 3 / 5).max(area.width.saturating_sub(40)).saturating_sub(2), area.height.saturating_sub(2));
        let fen = now.clone();
        let squares = chess::squares(fen.as_deref().unwrap_or(""));
        let drawn = self.drawn();
        let mark_rgb = |c: char| match c {
            'G' => (21, 160, 27),
            'R' => (220, 30, 30),
            'Y' => (240, 180, 0),
            _ => (40, 90, 220),
        };
        let mut lines: Vec<Line> = vec![];
        // The squares with a piece, for its picture: where, which and on what colour.
        let mut placed: Vec<crate::pictures::Placed> = vec![];
        for row in 0..8u16 {
            let rank = if self.flip { row } else { 7 - row };
            let mut rows: Vec<Vec<Span>> = (0..cell_h).map(|sub| vec![Span::styled(if sub == cell_h / 2 { format!("{} ", rank + 1) } else { "  ".into() }, dim)]).collect();
            for col in 0..8u16 {
                let file = if self.flip { 7 - col } else { col };
                let sq = (rank * 8 + file) as u8;
                let light = (rank + file) % 2 == 1;
                let mut colour = if pieces == Pieces::Figurines { self.board[!light as usize] } else { self.squares[!light as usize] };
                if last.is_some_and(|(a, b)| a == Some(sq) || b == Some(sq)) {
                    colour = tint(colour, (0xf6, 0xe0, 0x5a));
                }
                // A coloured square, or an arrow's end, in its colour; an arrow's start lighter.
                if let Some(m) = drawn.iter().find(|m| m.to == sq) {
                    colour = tint(colour, mark_rgb(m.colour));
                } else if let Some(m) = drawn.iter().find(|m| m.from == sq) {
                    colour = tint(tint(colour, mark_rgb(m.colour)), (0xff, 0xff, 0xff));
                }
                if let (Pieces::Pictures, Some(p)) = (pieces, squares[sq as usize]) {
                    placed.push((Rect::new(2 + col * cell_w, row * cell_h, cell_w, cell_h), crate::pictures::Picture::of(crate::pictures::chess(p)), colour));
                }
                let piece = squares[sq as usize].map(|p| crate::pieces::Piece {
                    name: p,
                    figurine: glyph(p),
                    colour: if p.is_ascii_uppercase() { Color::Rgb(255, 255, 255) } else { Color::Rgb(0, 0, 0) },
                });
                for (r, spans) in crate::pieces::square(pieces, &mosaic::CHESS, piece, cell_w, cell_h, colour).into_iter().enumerate() {
                    rows[r].extend(spans);
                }
            }
            lines.extend(rows.into_iter().map(Line::from));
        }
        let mut files = String::from("  ");
        for col in 0..8u16 {
            let file = if self.flip { 7 - col } else { col };
            let c = (b'a' + file as u8) as char;
            let pad = ((cell_w - 1) / 2) as usize;
            files.push_str(&format!("{}{c}{}", " ".repeat(pad), " ".repeat(cell_w as usize - 1 - pad)));
        }
        lines.push(Line::styled(files, dim));
        // Whose move, under the board.
        if let Some(fen) = &fen {
            let white = fen.split_whitespace().nth(1) == Some("w");
            lines.push(Line::styled(t!(if white { "chess.white_to_move" } else { "chess.black_to_move" }), dim));
        }
        // The arrows, which squares alone cannot show: `→ d2–d4` in their colours.
        let name = |sq: u8| format!("{}{}", (b'a' + sq % 8) as char, sq / 8 + 1);
        let arrows: Vec<Span> = drawn
            .iter()
            .filter(|m| m.from != m.to)
            .map(|m| {
                let (r, g, b) = mark_rgb(m.colour);
                Span::styled(format!(" {}–{}", name(m.from), name(m.to)), dim.fg(Color::Rgb(r, g, b)).add_modifier(Modifier::BOLD))
            })
            .collect();
        if !arrows.is_empty() {
            lines.push(Line::from([vec![Span::styled("→", dim)], arrows].concat()));
        }
        let board_w = 2 + cell_w * 8;
        // The evaluation as a bar, and the game's as a row of bars, one a move.
        let rev = self.review.as_ref().map(|r| r.now());
        if let Some(v) = self.eval_now(rev.as_ref(), fen.as_deref()) {
            let n = (board_w as usize).saturating_sub(10);
            let full = (review::share(v) * n as f64).round() as usize;
            lines.push(Line::from(vec![
                Span::styled(format!("{:>7} ", review::shown(v)), base.add_modifier(Modifier::BOLD)),
                Span::styled("█".repeat(full), Style::default().fg(Color::Rgb(240, 240, 240)).bg(Color::Rgb(30, 30, 30))),
                Span::styled("█".repeat(n - full), Style::default().fg(Color::Rgb(30, 30, 30))),
            ]));
        }
        if let Some(r) = &rev
            && let Some(start) = r.start
        {
            const BARS: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
            let values: Vec<(Option<u32>, i32)> = std::iter::once((None, start)).chain(r.plies.iter().map(|p| (Some(p.node), p.cp))).collect();
            let n = (board_w as usize).saturating_sub(2).max(1);
            let mut spans = vec![Span::styled("  ", dim)];
            for k in 0..values.len().min(n) {
                // More moves than room: every so many.
                let (node, v) = values[k * values.len() / values.len().min(n)];
                let here = self.open.as_ref().is_some_and(|o| o.at == node) && !self.played.as_ref().is_some_and(|l| l.at > 0) && self.browsing().is_none();
                let bar = BARS[((review::share(v) * 7.99) as usize).min(7)];
                spans.push(Span::styled(bar, if here { sty(&theme.dialog_input) } else { base }));
            }
            lines.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(lines), Rect { width: board_w.min(area.width), ..area });
        // Each picture over its square's figurine, which stays where one cannot be made.
        if let Some(pics) = self.pictures.as_mut() {
            pics.place(f.buffer_mut(), (area.x, area.y), area, placed);
        }
        self.draw_moves(f, theme, area, board_w, book);
    }

    /// A draughts board from the position shown: dark squares numbered, men ● and kings ♚ in
    /// white and black, the last move's squares tinted.
    fn draughts_board(&self, area: Rect, dim: Style) -> (Vec<Line<'static>>, u16, Vec<crate::pictures::Placed>) {
        let Some(o) = &self.open else { return (vec![], 0, vec![]) };
        let g = &o.game;
        let fen = match o.at {
            Some(n) => g.nodes[n as usize].fen.clone(),
            None => Some(g.start.clone()),
        }
        .unwrap_or_default();
        let last = o.at.map(|n| (g.nodes[n as usize].from, g.nodes[n as usize].to));
        let cells = coxswain_core::draughts::cells(&fen);
        let h = cells.len().max(1) as u16;
        let w = cells.iter().map(Vec::len).max().unwrap_or(0).max(1) as u16;
        let (pieces, cell_w, cell_h) = crate::pieces::layout(self.pieces, &mosaic::DRAUGHTS, w, h, (area.width * 3 / 5).max(area.width.saturating_sub(40)), area.height.saturating_sub(1));
        let mut lines = vec![];
        let mut placed = vec![];
        for row in 0..h {
            let y = if self.flip { h - 1 - row } else { row };
            let mut rows: Vec<Vec<Span>> = vec![vec![]; cell_h as usize];
            for col in 0..w {
                let x = if self.flip { w - 1 - col } else { col };
                let dark = (x + y) % 2 == 1;
                let cell = (x + y * w) as u8;
                let mut colour = if pieces == Pieces::Figurines { self.board[dark as usize] } else { self.squares[dark as usize] };
                if last.is_some_and(|(a, b)| a == Some(cell) || b == Some(cell)) {
                    colour = tint(colour, (0xf6, 0xe0, 0x5a));
                }
                let at = cells.get(y as usize).and_then(|r| r.get(x as usize)).copied().flatten();
                if let (Pieces::Pictures, Some(p)) = (pieces, at) {
                    placed.push((Rect::new(col * cell_w, row * cell_h, cell_w, cell_h), crate::pictures::Picture::of(crate::pictures::draughts(p)), colour));
                }
                let piece = at.map(|p| crate::pieces::Piece {
                    name: p,
                    figurine: if p.eq_ignore_ascii_case(&'k') { "♚" } else { "●" },
                    colour: if p.is_ascii_uppercase() { Color::Rgb(255, 255, 255) } else { Color::Rgb(0, 0, 0) },
                });
                let empty = piece.is_none();
                let mut square = crate::pieces::square(pieces, &mosaic::DRAUGHTS, piece, cell_w, cell_h, colour);
                // An empty dark square: its number at the top left.
                if empty && dark {
                    let n = coxswain_core::draughts::square_number(w as usize, x as usize, y as usize).unwrap_or(0);
                    square[0] = vec![Span::styled(format!("{n:<width$}", width = cell_w as usize), Style::default().bg(colour).fg(dim.fg.unwrap_or(Color::Gray)))];
                }
                for (r, spans) in square.into_iter().enumerate() {
                    rows[r].extend(spans);
                }
            }
            lines.extend(rows.into_iter().map(Line::from));
        }
        let white = fen.split_whitespace().nth(1) != Some("b");
        lines.push(Line::styled(t!(if white { "chess.white_to_move" } else { "chess.black_to_move" }), dim));
        (lines, cell_w * w, placed)
    }

    /// A xiangqi board: 9 files by 10 ranks of lines on the theme's wood, the river between
    /// the fifth and sixth ranks, the palaces' centres marked ╳, the pieces as their characters
    /// in red and black, the last move's points tinted. With room, the pieces are discs drawn
    /// from block characters with their character on them, or the desktop app's pictures.
    fn xiangqi_board(&self, area: Rect, dim: Style) -> (Vec<Line<'static>>, u16, Vec<crate::pictures::Placed>) {
        let Some(o) = &self.open else { return (vec![], 0, vec![]) };
        let g = &o.game;
        let fen = match o.at {
            Some(n) => g.nodes[n as usize].fen.clone(),
            None => Some(g.start.clone()),
        }
        .unwrap_or_default();
        let last = o.at.map(|n| (g.nodes[n as usize].from, g.nodes[n as usize].to));
        let cells = coxswain_core::xiangqi::cells(&fen);
        let wood = self.board[0];
        let ink = Style::default().bg(wood).fg(Color::Rgb(70, 50, 30));
        let (pieces, cols, rows) = crate::pieces::points(self.pieces, 9, 10, (area.width * 3 / 5).max(area.width.saturating_sub(40)), area.height.saturating_sub(3));
        if pieces != Pieces::Figurines {
            return self.xiangqi_drawn(&fen, &cells, last, pieces, cols, rows, dim);
        }
        let mut lines = vec![];
        let files = |red: bool| -> String {
            let n: Vec<String> = if red { ["九", "八", "七", "六", "五", "四", "三", "二", "一"].map(String::from).to_vec() } else { (1..=9).map(|n| format!("{n} ")).collect() };
            let n: Vec<String> = if self.flip { n.into_iter().rev().collect() } else { n };
            n.concat()
        };
        lines.push(Line::styled(files(self.flip), dim));
        for row in 0..10usize {
            let y = if self.flip { 9 - row } else { row };
            let mut spans = vec![];
            for col in 0..9usize {
                let x = if self.flip { 8 - col } else { col };
                let c = x + y * 9;
                let mut style = ink;
                if last.is_some_and(|(a, b)| a == Some(c as u8) || b == Some(c as u8)) {
                    style = style.bg(tint(wood, (0xf6, 0xe0, 0x5a)));
                }
                let text = match cells.get(c).copied().flatten() {
                    Some(p) => {
                        let fg = if p.is_ascii_uppercase() { Color::Rgb(200, 20, 30) } else { Color::Rgb(0, 0, 0) };
                        spans.push(Span::styled(coxswain_core::xiangqi::glyph(p).to_string(), style.fg(fg).add_modifier(Modifier::BOLD)));
                        continue;
                    }
                    None => {
                        let (top, bottom, left, right) = (row == 0, row == 9, col == 0, col == 8);
                        let g = match (top, bottom, left, right) {
                            (true, _, true, _) => "┌",
                            (true, _, _, true) => "┐",
                            (_, true, true, _) => "└",
                            (_, true, _, true) => "┘",
                            (true, ..) => "┬",
                            (_, true, ..) => "┴",
                            (_, _, true, _) => "├",
                            (_, _, _, true) => "┤",
                            // The river: the lines stop at its banks.
                            _ if row == 4 => "┴",
                            _ if row == 5 => "┬",
                            _ if x == 4 && (y == 1 || y == 8) => "╳",
                            _ => "┼",
                        };
                        format!("{g}{}", if col < 8 { "─" } else { " " })
                    }
                };
                spans.push(Span::styled(text, style));
            }
            lines.push(Line::from(spans));
            if row == 4 {
                lines.push(Line::styled("│  楚 河  漢 界 │".to_string(), ink));
            }
        }
        lines.push(Line::styled(files(!self.flip), dim));
        let red = fen.split_whitespace().nth(1) != Some("b");
        lines.push(Line::styled(t!(if red { "xiangqi.red_to_move" } else { "chess.black_to_move" }), dim));
        (lines, 18, vec![])
    }

    /// The xiangqi board with points of `cols` × `rows` cells: each piece a light disc with
    /// its character, or its picture; the river the gap in the lines between the fifth and
    /// sixth ranks.
    #[allow(clippy::too_many_arguments)]
    fn xiangqi_drawn(&self, fen: &str, cells: &[Option<char>], last: Option<(Option<u8>, Option<u8>)>, pieces: Pieces, cols: u16, rows: u16, dim: Style) -> (Vec<Line<'static>>, u16, Vec<crate::pictures::Placed>) {
        use crate::pieces::{DOWN, LEFT, RIGHT, UP};
        let wood = self.board[0];
        let ink_rgb = (70, 50, 30);
        let mut placed = vec![];
        let names = |red: bool| -> Line<'static> {
            let n: Vec<String> = if red { ["九", "八", "七", "六", "五", "四", "三", "二", "一"].map(String::from).to_vec() } else { (1..=9).map(|n| n.to_string()).collect() };
            let n: Vec<String> = if self.flip { n.into_iter().rev().collect() } else { n };
            // Each name over its file's line, in the middle of its point.
            let mut s = String::new();
            for name in n {
                let w = unicode_width::UnicodeWidthStr::width(name.as_str()) as u16;
                let left = ((cols - w) / 2) as usize;
                s.push_str(&format!("{}{name}{}", " ".repeat(left), " ".repeat((cols - w) as usize - left)));
            }
            Line::styled(s, dim)
        };
        let mut lines = vec![names(self.flip)];
        for row in 0..10u16 {
            let y = if self.flip { 9 - row } else { row } as usize;
            let mut grids = vec![];
            for col in 0..9u16 {
                let x = if self.flip { 8 - col } else { col } as usize;
                let c = x + y * 9;
                let mut bg = wood;
                if last.is_some_and(|(a, b)| a == Some(c as u8) || b == Some(c as u8)) {
                    bg = tint(wood, (0xf6, 0xe0, 0x5a));
                }
                let edge = col == 0 || col == 8;
                let mut lines = 0;
                if row > 0 && (row != 5 || edge) {
                    lines |= UP;
                }
                if row < 9 && (row != 4 || edge) {
                    lines |= DOWN;
                }
                if col > 0 {
                    lines |= LEFT;
                }
                if col < 8 {
                    lines |= RIGHT;
                }
                let mid = if x == 4 && (y == 1 || y == 8) { '╳' } else { crate::pieces::junction(lines) };
                let ink = Style::default().bg(bg).fg(Color::Rgb(ink_rgb.0, ink_rgb.1, ink_rgb.2));
                let mut grid = crate::pieces::point(cols, rows, lines, mid, ink);
                if let Some(p) = cells.get(c).copied().flatten() {
                    let disc = mosaic::Shape::Disc { dark: false }.draw(crate::pieces::glyphs(pieces), cols as usize, rows as usize);
                    crate::pieces::put(&mut grid, &disc, bg);
                    let fg = if p.is_ascii_uppercase() { Color::Rgb(200, 20, 30) } else { Color::Rgb(0, 0, 0) };
                    crate::pieces::label(&mut grid, coxswain_core::xiangqi::glyph(p), fg, crate::pieces::light());
                    if pieces == Pieces::Pictures {
                        let pic = crate::pictures::Picture { lines, ink: ink_rgb, ..crate::pictures::Picture::of(crate::pictures::xiangqi(p)) };
                        placed.push((Rect::new(col * cols, 1 + row * rows, cols, rows), pic, bg));
                    }
                }
                grids.push(grid);
            }
            for sub in 0..rows as usize {
                lines.push(Line::from(crate::pieces::spans(&grids, sub)));
            }
        }
        lines.push(names(!self.flip));
        let red = fen.split_whitespace().nth(1) != Some("b");
        lines.push(Line::styled(t!(if red { "xiangqi.red_to_move" } else { "chess.black_to_move" }), dim));
        (lines, cols * 9, placed)
    }

    /// A bridge deal round the table: North above, West and East beside, South below, each
    /// hand's cards still held by suit, the trick on the table in the middle; then the contract,
    /// dealer and vulnerability, the tricks won, and the auction in four columns.
    fn bridge_table(&self, dim: Style) -> (Vec<Line<'static>>, u16) {
        use coxswain_core::bridge::{self, State};
        let Some(o) = &self.open else { return (vec![], 0) };
        let g = &o.game;
        let fen = match o.at {
            Some(n) => g.nodes[n as usize].fen.clone(),
            None => Some(g.start.clone()),
        }
        .unwrap_or_default();
        let st = State::from_text(&fen);
        let tag = |k: &str| g.tag(k).filter(|v| !v.is_empty() && *v != "#" && *v != "?").unwrap_or("").to_string();
        let seat = [t!("bridge.north"), t!("bridge.east"), t!("bridge.south"), t!("bridge.west")];
        let names = ["North", "East", "South", "West"];
        let red = Style::default().fg(Color::Rgb(190, 20, 30));
        let suit_line = |s: usize, su: u8| -> Line<'static> {
            let mut cards: Vec<_> = st.hands[s].iter().filter(|c| c.suit == su).collect();
            cards.sort_by(|a, b| b.rank.cmp(&a.rank));
            let ranks: String = cards.iter().map(|c| c.shown().chars().skip(1).collect::<String>()).collect::<Vec<_>>().join(" ");
            let sym = ['♠', '♥', '♦', '♣'][su as usize].to_string();
            Line::from(vec![Span::styled(sym, if su == 1 || su == 2 { red } else { Style::default() }), Span::raw(format!(" {}", if ranks.is_empty() { "–".into() } else { ranks }))])
        };
        let head = |s: usize| format!("{} {}", seat[s], tag(names[s]));
        let pad = |l: Line<'static>, n: usize| -> Line<'static> {
            let mut spans = vec![Span::raw(" ".repeat(n))];
            spans.extend(l.spans);
            Line::from(spans)
        };
        let mut lines: Vec<Line<'static>> = vec![];
        lines.push(pad(Line::styled(head(0), dim), 16));
        for su in 0..4 {
            lines.push(pad(suit_line(0, su), 16));
        }
        // West and East side by side, the trick between them.
        let trick = |s: usize| st.trick.iter().find(|(w, _)| *w == s).map(|(_, c)| c.shown()).unwrap_or_default();
        let middle = [format!("   {:^6}   ", trick(0)), format!("{:<6}  {:>6}", trick(3), trick(1)), format!("   {:^6}   ", trick(2)), String::new()];
        lines.push(Line::from(vec![Span::styled(format!("{:<16}", head(3)), dim), Span::raw(" ".repeat(16)), Span::styled(head(1), dim)]));
        for su in 0..4u8 {
            let w = suit_line(3, su);
            let width: usize = w.spans.iter().map(|s| s.content.chars().count()).sum();
            let mut spans = w.spans;
            spans.push(Span::raw(" ".repeat(16usize.saturating_sub(width))));
            spans.push(Span::styled(format!("{:<16}", middle[su as usize]), Style::default().add_modifier(Modifier::BOLD)));
            spans.extend(suit_line(1, su).spans);
            lines.push(Line::from(spans));
        }
        lines.push(pad(Line::styled(head(2), dim), 16));
        for su in 0..4 {
            lines.push(pad(suit_line(2, su), 16));
        }
        lines.push(Line::from(""));
        let (contract, _, _) = bridge::contract(&tag("Contract"));
        if !contract.is_empty() {
            lines.push(Line::raw(t!("bridge.contract", "contract" => contract, "declarer" => tag("Declarer"))));
        }
        lines.push(Line::styled(t!("bridge.facts", "dealer" => tag("Dealer"), "vulnerable" => tag("Vulnerable")), dim));
        lines.push(Line::raw(t!("bridge.tricks", "ns" => st.won[0], "ew" => st.won[1])));
        if let Some(a) = g.tag("Auction") {
            let mut calls = a.split_whitespace();
            let first = calls.next().and_then(|c| "NESW".find(c)).unwrap_or(0);
            lines.push(Line::styled(t!("bridge.auction"), dim));
            // Columns West, North, East, South, as bridge players write them.
            let order = [3usize, 0, 1, 2];
            lines.push(Line::styled(order.iter().map(|&s| format!("{:<6}", seat[s].chars().take(5).collect::<String>())).collect::<String>(), dim));
            let mut row = vec![String::new(); order.iter().position(|&s| s == first).unwrap_or(0)];
            for c in calls {
                row.push(c.to_string());
                if row.len() == 4 {
                    lines.push(Line::raw(row.iter().map(|c| format!("{c:<6}")).collect::<String>()));
                    row.clear();
                }
            }
            if !row.is_empty() {
                lines.push(Line::raw(row.iter().map(|c| format!("{c:<6}")).collect::<String>()));
            }
        }
        (lines, 48)
    }

    /// A shogi board: 9×9 squares on the theme's wood, each piece its kanji, Gote's after a `v`
    /// (as KIF's board diagrams write them) and in another colour; the files numbered ９ to １
    /// above, the ranks 一 to 九 beside; the pieces in hand above (Gote's) and below (Sente's);
    /// the last move's squares tinted.
    fn shogi_board(&self, area: Rect, dim: Style) -> (Vec<Line<'static>>, u16, Vec<crate::pictures::Placed>) {
        use coxswain_core::shogi;
        let Some(o) = &self.open else { return (vec![], 0, vec![]) };
        let g = &o.game;
        let sfen = match o.at {
            Some(n) => g.nodes[n as usize].fen.clone(),
            None => Some(g.start.clone()),
        }
        .unwrap_or_default();
        let last = o.at.map(|n| (g.nodes[n as usize].from, g.nodes[n as usize].to));
        let (cells, hands) = shogi::view(&sfen);
        let wood = self.board[0];
        let ink = Style::default().bg(wood).fg(Color::Rgb(70, 50, 30));
        let hand = |side: usize| -> String {
            let list: Vec<String> = hands[side].iter().map(|&(k, n)| if n > 1 { format!("{k}{n}") } else { k.to_string() }).collect();
            let pieces = if list.is_empty() { "–".to_string() } else { list.join(" ") };
            if side == 0 { t!("shogi.hand_sente", "pieces" => pieces) } else { t!("shogi.hand_gote", "pieces" => pieces) }
        };
        let (top, bottom) = if self.flip { (0, 1) } else { (1, 0) };
        let mut lines = vec![Line::styled(hand(top), dim)];
        const FILES: [&str; 9] = ["９", "８", "７", "６", "５", "４", "３", "２", "１"];
        const RANKS: [&str; 9] = ["一", "二", "三", "四", "五", "六", "七", "八", "九"];
        let files: Vec<&str> = if self.flip { FILES.iter().rev().copied().collect() } else { FILES.to_vec() };
        let (pieces, cols, rows) = crate::pieces::tiles(self.pieces, 9, 9, (area.width * 3 / 5).max(area.width.saturating_sub(40)).saturating_sub(3), area.height.saturating_sub(5));
        if pieces != Pieces::Figurines {
            // Each piece a tile pointing away from its player, its name on it, or its picture;
            // the squares in two shades of the wood.
            let mut placed = vec![];
            let pad = " ".repeat(cols as usize / 2 - 1);
            lines.push(Line::styled(files.iter().map(|f| format!("{pad}{f}{pad}")).collect::<String>(), dim));
            for row in 0..9u16 {
                let y = if self.flip { 8 - row } else { row } as usize;
                let mut grids = vec![];
                for col in 0..9u16 {
                    let x = if self.flip { 8 - col } else { col } as usize;
                    let c = x + y * 9;
                    let mut bg = if (row + col).is_multiple_of(2) { wood } else { crate::pieces::darker(wood) };
                    if last.is_some_and(|(a, b)| a == Some(c as u8) || b == Some(c as u8)) {
                        bg = tint(wood, (0xf6, 0xe0, 0x5a));
                    }
                    let mut grid = crate::pieces::blank(cols, rows, bg);
                    if let Some(p) = cells.get(c).copied().flatten() {
                        let gote = p.sente == self.flip;
                        let tile = mosaic::Shape::Tile { up: !gote }.draw(crate::pieces::glyphs(pieces), cols as usize, rows as usize);
                        crate::pieces::put(&mut grid, &tile, bg);
                        // A promoted piece's name in red, as on the desktop app's tiles.
                        let fg = if p.promoted { Color::Rgb(160, 20, 30) } else { Color::Rgb(0, 0, 0) };
                        crate::pieces::label(&mut grid, shogi::glyph(p), fg, crate::pieces::light());
                        if pieces == Pieces::Pictures {
                            let pic = crate::pictures::Picture { turn: gote, ..crate::pictures::Picture::of(crate::pictures::shogi(p.kind, p.promoted)) };
                            placed.push((Rect::new(col * cols, 2 + row * rows, cols, rows), pic, bg));
                        }
                    }
                    grids.push(grid);
                }
                for sub in 0..rows as usize {
                    let mut spans = crate::pieces::spans(&grids, sub);
                    if sub == rows as usize / 2 {
                        spans.push(Span::styled(format!(" {}", RANKS[y]), dim));
                    }
                    lines.push(Line::from(spans));
                }
            }
            lines.push(Line::styled(hand(bottom), dim));
            let sente = sfen.split_whitespace().nth(1) != Some("w");
            lines.push(Line::styled(t!(if sente { "shogi.sente_to_move" } else { "shogi.gote_to_move" }), dim));
            return (lines, cols * 9 + 3, placed);
        }
        lines.push(Line::styled(files.iter().map(|f| format!(" {f}")).collect::<String>(), dim));
        for row in 0..9usize {
            let y = if self.flip { 8 - row } else { row };
            let mut spans = vec![];
            for col in 0..9usize {
                let x = if self.flip { 8 - col } else { col };
                let c = x + y * 9;
                let mut style = ink;
                if last.is_some_and(|(a, b)| a == Some(c as u8) || b == Some(c as u8)) {
                    style = style.bg(tint(wood, (0xf6, 0xe0, 0x5a)));
                }
                match cells.get(c).copied().flatten() {
                    Some(p) => {
                        // Gote's pieces point the other way: marked v, in red, from Sente's side.
                        let gote = p.sente == self.flip;
                        let fg = if gote { Color::Rgb(160, 20, 30) } else { Color::Rgb(0, 0, 0) };
                        spans.push(Span::styled(if gote { "v" } else { " " }, style.fg(fg)));
                        spans.push(Span::styled(shogi::glyph(p).to_string(), style.fg(fg).add_modifier(Modifier::BOLD)));
                    }
                    None => spans.push(Span::styled(" ・", style)),
                }
            }
            spans.push(Span::styled(format!(" {}", RANKS[y]), dim));
            lines.push(Line::from(spans));
        }
        lines.push(Line::styled(hand(bottom), dim));
        let sente = sfen.split_whitespace().nth(1) != Some("w");
        lines.push(Line::styled(t!(if sente { "shogi.sente_to_move" } else { "shogi.gote_to_move" }), dim));
        (lines, 30, vec![])
    }

    /// Beside the board: who played, then the moves.
    fn draw_moves(&mut self, f: &mut Frame, theme: &Theme, area: Rect, board_w: u16, book: Vec<BookMove>) {
        let picks = self.picking.as_ref().map(|(text, sel)| (self.pick_list(text), *sel));
        let base = sty(&theme.dialog);
        let dim = faint(theme).bg(base.bg.unwrap_or(Color::Reset));
        let draughts = self.draughts();
        let other = self.other_game();
        let rev = self.review.as_ref().map(|r| r.now());
        let Some(o) = &self.open else { return };
        let g = &o.game;
        let right = Rect { x: area.x + board_w + 2, width: area.width.saturating_sub(board_w + 2), ..area };
        if right.width < 10 {
            return;
        }
        let w = right.width as usize;
        let mut head: Vec<Line> = vec![];
        let tag = |k: &str| g.tag(k).filter(|v| !v.is_empty() && *v != "?").unwrap_or("").to_string();
        if self.lib.is_some() {
            let elo = |k: &str| {
                let e = tag(k);
                if e.is_empty() { e } else { format!(" ({e})") }
            };
            // Xiangqi names its first player Red; bridge has pairs.
            let first = if tag("Red").is_empty() { tag("White") } else { tag("Red") };
            let (first, second) = if self.bridge() {
                let (pairs, _) = coxswain_core::bridge::summary(g);
                (pairs[0].clone(), pairs[1].clone())
            } else {
                (first, tag("Black"))
            };
            head.push(Line::styled(fit(&format!("{}{} – {}{}  {}", first, elo("WhiteElo"), second, elo("BlackElo"), g.result), w), base.add_modifier(Modifier::BOLD)));
            let about = [tag("Event"), tag("Site"), tag("Date"), [tag("ECO"), tag("Opening")].join(" ").trim().to_string()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            head.push(Line::styled(fit(&about, w), dim));
            if draughts {
                head.push(Line::styled(fit(&coxswain_core::draughts::variant(g).name(), w), dim));
            }
            if let Some(b) = g.tag("Board").filter(|_| self.bridge()) {
                head.push(Line::styled(fit(&t!("bridge.board_n", "n" => b), w), dim));
            }
            let total = self.lib.as_ref().map_or(0, |l| l.progress().games);
            head.push(Line::styled(fit(&t!("chess.game_of", "n" => o.index + 1, "total" => total), w), dim));
        } else if self.book_only {
            let entries = polyglot::Book::open(&self.path).map_or(0, |b| b.entries());
            head.push(Line::styled(fit(&t!("chess.book_entries", "n" => entries), w), base.add_modifier(Modifier::BOLD)));
        } else {
            head.push(Line::styled(fit(&t!("chess.position_of", "n" => o.index + 1, "total" => self.positions.len()), w), base.add_modifier(Modifier::BOLD)));
        }
        let pieces = chess::pieces(self.notation);
        // The book's moves for this position: a list to choose from in a book of its own, a
        // line under the game's heading otherwise.
        let book_name = self.book.as_ref().and_then(|b| b.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if self.book_only && picks.is_none() {
            if self.analysis {
                head.extend(self.engine_lines(&pieces, theme, w));
            }
            head.push(Line::from(""));
            if book.is_empty() {
                head.push(Line::styled(fit(&t!("chess.book_none"), w), dim));
            }
            for (k, m) in book.iter().enumerate() {
                let learn = if m.learn > 0 { format!("  {}", t!("chess.book_learn", "n" => m.learn)) } else { String::new() };
                let text = format!("{:<8} {:>5.1} %  {:>5}{learn}", chess::show_san(&m.san, &pieces), m.share, m.weight);
                let style = if k == self.pick { sty(&theme.dialog_input) } else { base };
                head.push(Line::styled(fit(&text, w), style));
            }
            f.render_widget(Paragraph::new(head), right);
            return;
        }
        if self.book.is_some() {
            let text = if book.is_empty() {
                t!("chess.book_out", "name" => book_name)
            } else {
                let list: Vec<String> = book.iter().take(8).map(|m| format!("{} {:.0} %", chess::show_san(&m.san, &pieces), m.share)).collect();
                t!("chess.book", "name" => book_name, "moves" => list.join(" · "))
            };
            head.push(Line::styled(fit(&text, w), dim));
        }
        if self.analysis {
            head.extend(self.engine_lines(&pieces, theme, w));
        }
        let review_state = rev.as_ref().map(|r| {
            if let Some(e) = &r.error {
                e.clone()
            } else if r.done {
                t!("chess.review_done", "engine" => r.engine, "marked" => r.plies.iter().filter(|p| p.mark.is_some_and(|m| m != 5)).count())
            } else {
                t!("chess.review_running", "n" => r.plies.len(), "total" => r.total)
            }
        });
        if let Some(state) = review_state {
            head.push(Line::styled(fit(&state, w), dim));
        }
        head.push(Line::from(""));
        // The move list being chosen from, in place of the moves.
        if let Some((list, sel)) = picks {
            let mut words: Vec<(String, Style, bool)> = vec![(format!("{}:", t!("chess.pick_title")), dim, false)];
            for (k, m) in list.iter().enumerate() {
                let here = k == sel;
                words.push((chess::show_san(&m.san, &pieces), if here { sty(&theme.dialog_input) } else { base }, here));
            }
            if list.is_empty() {
                words.push((t!("chess.pick_none"), dim, false));
            }
            let room = (right.height as usize).saturating_sub(head.len());
            head.extend(wrap(words, w, base, room, None));
            f.render_widget(Paragraph::new(head), right);
            return;
        }
        // The games that reach this position, in place of the moves.
        if let Some((list, sel)) = &self.found {
            head.push(Line::styled(fit(&t!("chess.position_title", "n" => list.len()), w), base.add_modifier(Modifier::BOLD)));
            let room = (right.height as usize).saturating_sub(head.len());
            let top = sel.saturating_sub(room.saturating_sub(1));
            for (k, (path, game)) in list.iter().enumerate().skip(top).take(room) {
                let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let mut text = t!("chess.position_game", "file" => file, "n" => game + 1);
                if *path == self.path
                    && let Some(r) = self.lib.as_ref().and_then(|l| l.row(*game))
                {
                    text = format!("{text} · {} – {} {} {}", r.white, r.black, r.result, r.date);
                }
                head.push(Line::styled(fit(&text, w), if k == *sel { sty(&theme.dialog_input) } else { base }));
            }
            f.render_widget(Paragraph::new(head), right);
            return;
        }
        // The moves, word by word, wrapped; the line with the move shown is kept in view.
        let mut words: Vec<(String, Style, bool)> = vec![];
        // The move shown, its variations and the other moves from its position: kept in view.
        let end = o.layout.iter().position(|t| matches!(t, Token::Move { node, .. } if o.at == Some(*node))).map(|i| chess::pgn::view_end(&o.layout, i));
        let mut keep = None;
        for (i, tok) in o.layout.iter().enumerate() {
            if end == Some(i) {
                // Its last word, once this token's words are in.
                keep = Some(usize::MAX);
            }
            match tok {
                Token::Move { node, number } => {
                    let n = &g.nodes[*node as usize];
                    let mut list = n.nags.clone();
                    // The analysis's mark, when the file has none of its own.
                    if let Some(m) = rev.as_ref().and_then(|r| r.plies.iter().find(|p| p.node == *node)).and_then(|p| p.mark)
                        && !list.iter().any(|x| (1..=6).contains(x))
                    {
                        list.push(m);
                    }
                    let (glued, apart) = chess::nags(&list);
                    let san = if other { n.san.clone() } else { chess::show_san(&n.san, &pieces) };
                    let text = format!("{}{san}{glued}", number.clone().unwrap_or_default());
                    let here = o.at == Some(*node);
                    let mut style = if here { sty(&theme.dialog_input) } else if n.depth > 0 { dim } else { base };
                    if n.depth == 0 && !here {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    words.push((text, style, here));
                    if !apart.is_empty() {
                        words.push((apart, base, false));
                    }
                }
                Token::Comment { text } => {
                    for word in text.split_whitespace() {
                        words.push((word.to_string(), dim.add_modifier(Modifier::ITALIC), false));
                    }
                }
                Token::Open { .. } => words.push(("(".into(), dim, false)),
                Token::Close { .. } => words.push((")".into(), dim, false)),
                Token::Result { text } => words.push((text.clone(), base.add_modifier(Modifier::BOLD), false)),
            }
            if keep == Some(usize::MAX) {
                keep = words.len().checked_sub(1);
            }
        }
        // The line played on from the board, on lines of its own after the game.
        if let Some(l) = &self.played {
            words.push(("\n".into(), base, false));
            let from = match l.base {
                Some(n) => {
                    let n = &g.nodes[n as usize];
                    format!("{}{}{}", n.number, if n.white { "." } else { "…" }, chess::show_san(&n.san, &pieces))
                }
                None => t!("chess.line_start"),
            };
            words.push((t!("chess.line_from", "move" => from), dim.add_modifier(Modifier::ITALIC), false));
            words.push(("\n".into(), base, false));
            for (k, m) in l.moves.iter().enumerate() {
                let number = if m.white { format!("{}.", m.number) } else if k == 0 { format!("{}…", m.number) } else { String::new() };
                let here = l.at == k + 1;
                words.push((format!("{number}{}", chess::show_san(&m.san, &pieces)), if here { sty(&theme.dialog_input) } else { base }, here));
            }
            words.push(("\n".into(), base, false));
            words.push((t!("chess.line_keys"), dim, false));
        }
        let room = (right.height as usize).saturating_sub(head.len());
        head.extend(wrap(words, w, base, room, keep));
        f.render_widget(Paragraph::new(head), right);
    }

    /// What the engine says about the game's position: its name and depth, then its lines,
    /// numbered for 1–5, each with its evaluation and moves; the line stepped through lit up to
    /// the move shown, that move as the game's move shown is. Or that it thinks, or why it
    /// cannot.
    fn engine_lines(&self, pieces: &[String; 5], theme: &Theme, w: usize) -> Vec<Line<'static>> {
        let base = sty(&theme.dialog);
        let dim = faint(theme).bg(base.bg.unwrap_or(Color::Reset));
        let fen = self.game_now().0;
        match &self.analysed {
            Some((f, Ok(a))) if Some(f) == fen.as_ref() => {
                let browse = self.browsing();
                let mut head = t!("chess.engine", "name" => a.engine, "depth" => a.depth);
                match a.lines.len() {
                    0 => {}
                    1 => head = format!("{head} · {}", t!("chess.engine_line_key")),
                    n => head = format!("{head} · {}", t!("chess.engine_lines_keys", "n" => n)),
                }
                let mut out = vec![Line::styled(fit(&head, w), dim)];
                for (k, l) in a.lines.iter().enumerate() {
                    let at = browse.filter(|b| b.k == k).map_or(0, |b| b.at);
                    let mut spans = vec![Span::styled(format!("{} {:>6} ", k + 1, l.eval), if at > 0 { base } else { dim })];
                    for (i, san) in l.moves.iter().enumerate() {
                        let style = match (i + 1).cmp(&at) {
                            std::cmp::Ordering::Less => base,
                            std::cmp::Ordering::Equal => sty(&theme.dialog_input),
                            std::cmp::Ordering::Greater => dim,
                        };
                        spans.push(Span::styled(" ", dim));
                        spans.push(Span::styled(chess::show_san(san, pieces), style));
                    }
                    out.push(Line::from(spans));
                }
                if let Some(b) = browse {
                    out.push(Line::styled(fit(&t!("chess.engine_browse", "n" => b.k + 1, "at" => b.at, "total" => b.steps.len()), w), dim));
                }
                out
            }
            Some((f, Err(e))) if Some(f) == fen.as_ref() => vec![Line::styled(fit(e, w), dim)],
            _ => vec![Line::styled(fit(&t!("chess.engine_thinking"), w), dim)],
        }
    }
}

/// Words wrapped to lines `w` wide (`"\n"` starts a new one), `room` lines at most: the word
/// marked `here` kept in view, and the words after it up to the word `keep` while there is
/// room (the variations at the move shown).
fn wrap(words: Vec<(String, Style, bool)>, w: usize, base: Style, room: usize, keep: Option<usize>) -> Vec<Line<'static>> {
    let mut body: Vec<Vec<Span>> = vec![vec![]];
    let mut used = 0;
    let mut here_line = 0;
    let mut keep_line = 0;
    for (k, (text, style, here)) in words.into_iter().enumerate() {
        if text == "\n" {
            body.push(vec![]);
            used = 0;
            continue;
        }
        let tw = text.width();
        if used > 0 && used + 1 + tw > w {
            body.push(vec![]);
            used = 0;
        }
        let line = body.last_mut().expect("one line at least");
        if used > 0 && text != ")" {
            line.push(Span::styled(" ", base));
            used += 1;
        }
        if here {
            here_line = body.len() - 1;
        }
        if keep == Some(k) {
            keep_line = body.len() - 1;
        }
        used += tw;
        body.last_mut().expect("one line at least").push(Span::styled(text, style));
    }
    // The move shown a line from the bottom, or higher when what follows it needs the room;
    // never above the top.
    let top = here_line.saturating_sub(room.saturating_sub(2)).max((keep_line + 1).saturating_sub(room)).min(here_line);
    body.into_iter().skip(top).take(room).map(Line::from).collect()
}

/// A column's name, for the order line.
fn column(s: Sort) -> String {
    match s {
        Sort::Number => "#".into(),
        Sort::White => t!("chess.col_white"),
        Sort::Black => t!("chess.col_black"),
        Sort::WhiteElo => format!("{} {}", t!("chess.col_white"), t!("chess.col_elo")),
        Sort::BlackElo => format!("{} {}", t!("chess.col_black"), t!("chess.col_elo")),
        Sort::Result => t!("chess.col_result"),
        Sort::Date => t!("chess.col_date"),
        Sort::Event => t!("chess.col_event"),
        Sort::Eco => "ECO".into(),
        Sort::Opening => t!("chess.col_opening"),
        Sort::Moves => t!("chess.col_moves"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> Key {
        Key { code, ctrl: false, alt: false, shift: false }
    }

    /// The move shown keeps the variations after it in view when there is room: before, it
    /// sat a line from the bottom and they were cut off in a short window.
    #[test]
    fn the_variations_after_the_move_shown_stay_in_view() {
        let words = |here: usize| -> Vec<(String, Style, bool)> { (0..40).map(|k| (format!("w{k:02}"), Style::default(), k == here)).collect() };
        let text = |lines: Vec<Line<'static>>| lines.iter().map(|l| l.spans.iter().map(|s| s.content.to_string()).collect::<String>()).collect::<Vec<_>>();
        // 4 words a line (w00 w01 w02 w03), 5 lines of room; the move shown on the 7th line.
        let at = |here, keep| text(wrap(words(here), 15, Style::default(), 5, keep));
        assert_eq!(at(24, None)[3], "w24 w25 w26 w27", "without a variation: a line from the bottom");
        // Its variations up to w35 (the 9th line): scrolled on until w35 is on the last line.
        let shown = at(24, Some(35));
        assert_eq!((shown[2].as_str(), shown[4].as_str()), ("w24 w25 w26 w27", "w32 w33 w34 w35"));
        // More than there is room for: the move shown stays, on the top line.
        assert_eq!(at(4, Some(39))[0], "w04 w05 w06 w07");
        // Near the start: nothing scrolls.
        assert_eq!(at(1, Some(6))[0], "w00 w01 w02 w03");
    }

    /// Terminals report Shift+A as `a` with Shift: the player field types the capital.
    #[test]
    fn a_chess_field_types_capitals_with_shift() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-chess-shift-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("g.pgn");
        std::fs::write(&p, "[White \"A\"]\n\n1.e4 *\n").unwrap();
        let cfg = coxswain_core::config::Config::default();
        let mut v = Viewer::open(&p, &cfg, &Theme::default()).unwrap();
        v.key(key(KeyCode::Char('/')), None);
        v.key(Key { code: KeyCode::Char('m'), ctrl: false, alt: false, shift: true }, None);
        assert_eq!(v.typing.as_ref().map(|(_, t)| t.as_str()), Some("M"));
    }

    #[test]
    fn a_game_stepped_through_and_drawn() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-chess-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("g.pgn");
        std::fs::write(&p, "[White \"Morphy\"]\n[Black \"Duke\"]\n\n1.e4 e5 2.Nf3 d6 (2...Nc6 3.Bb5) 3.d4 {centre} 1-0\n\n[White \"B\"]\n\n1.d4 *\n").unwrap();
        let cfg = coxswain_core::config::Config::default();
        let mut v = Viewer::open(&p, &cfg, &Theme::default()).unwrap();
        while !v.lib.as_ref().unwrap().progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        v.poll();
        assert_eq!(v.ids, vec![0, 1]);
        v.key(key(KeyCode::Enter), None);
        while v.open.is_none() {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        for _ in 0..4 {
            v.key(key(KeyCode::Right), None);
        }
        // On 2…d6, Down goes into its variation 2…Nc6, Right on to 3.Bb5.
        v.key(key(KeyCode::Down), None);
        v.key(key(KeyCode::Right), None);
        let o = v.open.as_ref().unwrap();
        assert_eq!(o.game.nodes[o.at.unwrap() as usize].san, "Bb5");
        // A move played on from 2…d6: m, typed, Enter: a line of its own; its own next move
        // steps the game instead.
        v.key(key(KeyCode::Left), None);
        v.key(key(KeyCode::Up), None);
        let o = v.open.as_ref().unwrap();
        assert_eq!(o.game.nodes[o.at.unwrap() as usize].san, "d6");
        v.key(key(KeyCode::Char('m')), None);
        for c in "Nc3".chars() {
            v.key(key(KeyCode::Char(c)), None);
        }
        v.key(key(KeyCode::Enter), None);
        assert_eq!(v.played.as_ref().map(|l| l.moves[0].san.as_str()), Some("Nc3"));
        v.key(key(KeyCode::Char('s')), None);
        let saved = std::fs::read_to_string(d.join("g line.pgn")).unwrap();
        assert!(saved.contains("1. e4 e5 2. Nf3 d6 3. Nc3 *") && saved.contains("Morphy"), "{saved}");
        v.key(key(KeyCode::Left), None);
        v.key(key(KeyCode::Char('m')), None);
        for c in "d4".chars() {
            v.key(key(KeyCode::Char(c)), None);
        }
        v.key(key(KeyCode::Enter), None);
        let o = v.open.as_ref().unwrap();
        assert_eq!(o.game.nodes[o.at.unwrap() as usize].san, "d4");
        assert_eq!(v.played.as_ref().map(|l| l.at), Some(0));
        v.key(key(KeyCode::Left), None);
        v.key(key(KeyCode::Right), None);
        assert_eq!(v.played.as_ref().map(|l| l.at), Some(1));
        v.key(key(KeyCode::Char('c')), None);
        assert!(v.played.is_none());
        v.key(key(KeyCode::Down), None);
        v.key(key(KeyCode::Right), None);
        v.pieces = Pieces::Figurines;
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let screen: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        assert!(screen.contains("Morphy") && screen.contains("♘f3") && screen.contains("♗b5") && screen.contains("centre"), "{screen}");
        assert!(screen.contains('♚'));
        // Drawn from sextants on a board large enough; figurines on a small one.
        v.pieces = Pieces::Mosaic(coxswain_core::mosaic::Glyphs::Sextants);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 36)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let screen: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        assert!(screen.chars().any(|c| ('\u{1FB00}'..='\u{1FB3B}').contains(&c)) && !screen.contains('♚'), "{screen}");
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let screen: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        assert!(screen.contains('♚'), "{screen}");
        // Esc back to the table, a filter, and the table again.
        v.key(key(KeyCode::Esc), None);
        v.key(key(KeyCode::Char('/')), None);
        for c in "morph".chars() {
            v.key(key(KeyCode::Char(c)), None);
        }
        v.key(key(KeyCode::Enter), None);
        assert_eq!(v.ids, vec![0]);
        // A Find hit: found on a thread, then shown.
        let mut v = Viewer::open(&p, &cfg, &Theme::default()).unwrap();
        let (o, c) = coxswain_core::store::MARK;
        v.at_hit(format!("{o}B{c}"));
        let start = Instant::now();
        while v.open.is_none() && start.elapsed() < coxswain_core::test_limit(std::time::Duration::from_secs(10)) {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(v.open.as_ref().map(|o| o.index), Some(1));
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// An engine line stepped through without playing it: 1–5 choose it, → ← step it, the
    /// engine stays on the game's position, Esc goes back, Enter plays it into your line.
    #[test]
    fn an_engine_line_stepped_through() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-chess-lines-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("g.pgn");
        std::fs::write(&p, "1.e4 e5 2.Nf3 Nc6 *\n").unwrap();
        let cfg = coxswain_core::config::Config::default();
        let mut v = Viewer::open(&p, &cfg, &Theme::default()).unwrap();
        while !v.lib.as_ref().unwrap().progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        v.poll();
        v.key(key(KeyCode::Enter), None);
        while v.open.is_none() {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        v.key(key(KeyCode::Right), None);
        let root = v.now().0.unwrap();
        // The engine's two lines from 1.e4, as it would give them.
        let line = |ucis: &[&str]| {
            let mut f = root.clone();
            let steps: Vec<Played> = ucis
                .iter()
                .map(|u| {
                    let m = play::play(&f, u).unwrap();
                    f = m.fen.clone();
                    m
                })
                .collect();
            play::EngineLine { moves: steps.iter().map(|m| m.san.clone()).collect(), best: steps.first().cloned(), steps, ..Default::default() }
        };
        let a = Analysis { lines: vec![line(&["e7e5", "g1f3"]), line(&["c7c5", "g1f3", "d7d6"])], ..Default::default() };
        v.analysed = Some((root.clone(), Ok(a)));
        // The heading names the keys there are; a number past them says how many there are.
        let head: String = v.engine_lines(&chess::pieces(Notation::Letters), &Theme::default(), 100)[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(head.contains(&t!("chess.engine_lines_keys", "n" => 2)), "{head}");
        v.key(key(KeyCode::Char('5')), None);
        assert_eq!((v.browse.is_none(), v.said.take()), (true, Some(tn!("chess.engine_only_lines", 2))));
        // 2: the second line at its first move; → on, the game and the engine stay at 1.e4.
        v.key(key(KeyCode::Char('2')), None);
        assert_eq!(v.now().0.as_deref(), Some(v.analysed.as_ref().unwrap().1.as_ref().unwrap().lines[1].steps[0].fen.as_str()));
        v.key(key(KeyCode::Right), None);
        v.key(key(KeyCode::Right), None);
        v.key(key(KeyCode::Right), None);
        assert_eq!(v.browsing().map(|b| b.at), Some(3), "→ stops at its last move");
        assert_eq!(v.game_now().0.as_deref(), Some(root.as_str()));
        v.poll_engine();
        assert!(v.thinking.is_none(), "the engine is not asked about the line's positions");
        let o = v.open.as_ref().unwrap();
        assert_eq!(o.game.nodes[o.at.unwrap() as usize].san, "e4");
        // Numbered, the line stepped through lit up to the move shown, and where it is.
        let lines = v.engine_lines(&chess::pieces(Notation::Letters), &Theme::default(), 100);
        let text: Vec<String> = lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect();
        assert!(text[2].starts_with("2 ") && text[2].ends_with("c5 Nf3 d6"), "{text:?}");
        assert_eq!(lines[2].spans.last().unwrap().style, sty(&Theme::default().dialog_input), "d6, the move shown");
        assert_eq!(text.len(), 4, "{text:?}");
        // ← back a move, ← on its first move back to the game; Esc too.
        v.key(key(KeyCode::Left), None);
        assert_eq!(v.browsing().map(|b| b.at), Some(2));
        v.key(key(KeyCode::Left), None);
        v.key(key(KeyCode::Left), None);
        assert!(v.browse.is_none());
        assert_eq!(v.now().0.as_deref(), Some(root.as_str()));
        v.key(key(KeyCode::Char('2')), None);
        v.key(key(KeyCode::Esc), None);
        assert!(v.browse.is_none() && v.open.is_some(), "Esc leaves the line, not the game");
        // Enter plays it up to the move shown: c5 is no move of the game, so it is your line.
        v.key(key(KeyCode::Char('2')), None);
        v.key(key(KeyCode::Right), None);
        v.key(key(KeyCode::Enter), None);
        assert!(v.browse.is_none());
        assert_eq!(v.played.as_ref().map(|l| l.moves.iter().map(|m| m.san.as_str()).collect::<Vec<_>>()), Some(vec!["c5", "Nf3"]));
        // The first line is the game's own: played, it steps the game.
        v.played = None;
        v.key(key(KeyCode::Char('1')), None);
        v.key(key(KeyCode::Right), None);
        v.key(key(KeyCode::Enter), None);
        let o = v.open.as_ref().unwrap();
        assert_eq!((o.game.nodes[o.at.unwrap() as usize].san.as_str(), v.played.is_none()), ("Nf3", true));
        // A line the engine does not have: said, nothing stepped.
        v.key(key(KeyCode::Char('5')), None);
        assert!(v.browse.is_none() && v.said.is_some());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_book_played_through() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-book-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // Two moves from the start, one after 1.e4: (key, from, to, weight).
        let zobrist = |fen: &str| polyglot::key(&chess::pgn::board(fen).unwrap());
        let sq = |s: &str| (s.as_bytes()[0] - b'a') as u16 | (((s.as_bytes()[1] - b'1') as u16) << 3);
        let start = zobrist(chess::pgn::START);
        let e4 = zobrist("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1");
        let mut entries = vec![(start, "e2", "e4", 3u16), (start, "d2", "d4", 1), (e4, "c7", "c5", 1)];
        entries.sort_by_key(|e| e.0);
        let mut bytes = vec![];
        for (k, from, to, w) in entries {
            bytes.extend(k.to_be_bytes());
            bytes.extend((sq(to) | (sq(from) << 6)).to_be_bytes());
            bytes.extend(w.to_be_bytes());
            bytes.extend(0u32.to_be_bytes());
        }
        let book = d.join("book.bin");
        std::fs::write(&book, bytes).unwrap();
        let cfg = coxswain_core::config::Config::default();
        let mut v = Viewer::open(&book, &cfg, &Theme::default()).unwrap();
        let screen = |v: &mut Viewer| {
            let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
            term.draw(|f| v.draw(f, &Theme::default())).unwrap();
            term.backend().buffer().content.iter().map(|c| c.symbol()).collect::<String>()
        };
        let s = screen(&mut v);
        assert!(s.contains("e4") && s.contains("75.0 %") && s.contains("d4"), "{s}");
        v.key(key(KeyCode::Enter), None);
        assert!(screen(&mut v).contains("c5"));
        v.key(key(KeyCode::Left), None);
        assert!(screen(&mut v).contains("75.0 %"));
        // A game next to the book: its moves under the heading.
        let pgn = d.join("g.pgn");
        std::fs::write(&pgn, "[White \"A\"]\n\n1.e4 c5 *\n").unwrap();
        let mut v = Viewer::open(&pgn, &cfg, &Theme::default()).unwrap();
        while !v.lib.as_ref().unwrap().progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        v.poll();
        v.key(key(KeyCode::Enter), None);
        while v.open.is_none() {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let s = screen(&mut v);
        assert!(s.contains("book.bin") && s.contains("e4 75 %"), "{s}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_draughts_game_on_its_board() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-pdn-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("g.pdn");
        std::fs::write(&p, "[White \"Anna\"]\n[Black \"Bram\"]\n[Result \"2-0\"]\n\n1. 32-28 19-23 2. 28x19 {takes} 14x23 2-0\n").unwrap();
        let cfg = coxswain_core::config::Config::default();
        let mut v = Viewer::open(&p, &cfg, &Theme::default()).unwrap();
        assert!(v.draughts() && v.book.is_none());
        while !v.lib.as_ref().unwrap().progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        v.poll();
        let screen = |v: &mut Viewer| {
            let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 34)).unwrap();
            term.draw(|f| v.draw(f, &Theme::default())).unwrap();
            term.backend().buffer().content.iter().map(|c| c.symbol()).collect::<String>()
        };
        let s = screen(&mut v);
        assert!(s.contains("Anna") && s.contains("2-0") && !s.contains("ECO"), "{s}");
        v.key(key(KeyCode::Enter), None);
        while v.open.is_none() {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        for _ in 0..3 {
            v.key(key(KeyCode::Right), None);
        }
        v.pieces = Pieces::Figurines;
        let s = screen(&mut v);
        assert!(s.contains("28x19") && s.contains("takes") && s.contains('●') && s.contains("10×10"), "{s}");
        // Square numbers on the empty dark squares.
        assert!(s.contains("26"));
        // Men drawn from sextants on a board large enough, the numbers still there.
        v.pieces = Pieces::Mosaic(coxswain_core::mosaic::Glyphs::Sextants);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let s: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        assert!(s.chars().any(|c| ('\u{1FB00}'..='\u{1FB3B}').contains(&c)) && !s.contains('●') && s.contains("26"), "{s}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_xiangqi_game_on_its_board() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-xq-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("g.pgn");
        std::fs::write(&p, "[Game \"Chinese Chess\"]\n[Red \"Hu\"]\n[Black \"Lü\"]\n[Result \"1-0\"]\n\n1. 炮二平五 马８进７ 2. 马二进三 {centre} 车９平８ 1-0\n").unwrap();
        let cfg = coxswain_core::config::Config::default();
        let mut v = Viewer::open(&p, &cfg, &Theme::default()).unwrap();
        assert!(v.xiangqi() && v.book.is_none());
        while !v.lib.as_ref().unwrap().progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        v.poll();
        v.key(key(KeyCode::Enter), None);
        while v.open.is_none() {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        for _ in 0..3 {
            v.key(key(KeyCode::Right), None);
        }
        v.pieces = Pieces::Figurines;
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 34)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        // A wide character's second cell is drawn as a space here.
        let s: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect::<String>().replace(' ', "");
        assert!(s.contains("Hu") && s.contains("马二进三") && s.contains("centre") && s.contains('俥') && s.contains('將') && s.contains('楚'), "{s}");
        // With room: discs of block characters, each with its character, on points of 7 × 3.
        v.pieces = Pieces::Mosaic(coxswain_core::mosaic::Glyphs::Quadrants);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let b = term.backend().buffer();
        let rows: Vec<String> = (0..40).map(|y| (0..160).map(|x| b[(x, y)].symbol()).collect()).collect();
        let s = rows.join("\n");
        assert!(s.contains('俥') && s.contains('將') && s.contains('▐') && s.contains('╳') && !s.contains('楚'), "{s}");
        // The river: no line crosses it between the edges.
        let board = |r: &String| r.chars().take(66).filter(|c| !c.is_whitespace()).collect::<String>();
        assert!(rows.iter().any(|r| board(r) == "║││"), "{s}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_shogi_game_on_its_board() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-shogi-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("g.kif");
        std::fs::write(&p, "先手：羽生\n後手：佐藤\n手数----指手--\n   1 ７六歩(77)\n   2 ３四歩(33)\n*角道\n   3 ２二角成(88)\n   4 同　銀(31)\n").unwrap();
        let cfg = coxswain_core::config::Config::default();
        let mut v = Viewer::open(&p, &cfg, &Theme::default()).unwrap();
        assert!(v.shogi() && v.book.is_none() && v.feature() == "shogi");
        while !v.lib.as_ref().unwrap().progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        v.poll();
        v.key(key(KeyCode::Enter), None);
        while v.open.is_none() {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        for _ in 0..4 {
            v.key(key(KeyCode::Right), None);
        }
        v.pieces = Pieces::Figurines;
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 34)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let s: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect::<String>().replace(' ', "");
        assert!(s.contains("羽生") && s.contains("△同") && s.contains("角道") && s.contains("v香") && s.contains('角'), "{s}");
        // With room: tiles of block characters with their names, Gote's pointing down.
        v.pieces = Pieces::Mosaic(coxswain_core::mosaic::Glyphs::Halves);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let b = term.backend().buffer();
        let s: String = (0..40).map(|y| (0..160).map(|x| b[(x, y)].symbol()).collect::<String>() + "\n").collect();
        assert!(s.contains('香') && s.contains('▄') && s.contains('▀') && !s.contains("v香"), "{s}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_bridge_deal_at_the_table() {
        let d = std::env::temp_dir().join(format!("coxswain-tui-pbn-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("club.pbn");
        std::fs::write(&p, "[Board \"1\"]\n[North \"Ben\"]\n[South \"Di\"]\n[East \"Cy\"]\n[West \"Ada\"]\n[Dealer \"N\"]\n[Vulnerable \"None\"]\n[Deal \"N:AKQ5.T98.765.432 JT9.J765.432.AKQ 876.432.AKQJ.JT9 432.AKQ.T98.8765\"]\n[Declarer \"S\"]\n[Contract \"3NT\"]\n[Result \"9\"]\n[Auction \"N\"]\n1C Pass 1D Pass\n3NT AP\n[Play \"W\"]\nHA H8 H5 H2\n*\n").unwrap();
        let cfg = coxswain_core::config::Config::default();
        let mut v = Viewer::open(&p, &cfg, &Theme::default()).unwrap();
        assert!(v.bridge() && v.feature() == "bridge");
        while !v.lib.as_ref().unwrap().progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        v.poll();
        v.key(key(KeyCode::Enter), None);
        while v.open.is_none() {
            v.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        v.key(key(KeyCode::Right), None);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(130, 40)).unwrap();
        term.draw(|f| v.draw(f, &Theme::default())).unwrap();
        let s: String = term.backend().buffer().content.iter().map(|c| c.symbol()).collect();
        assert!(s.contains("Ben, Di") && s.contains("3NT S =") && s.contains("♥A") && s.contains("1♣") && s.contains("Ada"), "{s}");
        std::fs::remove_dir_all(&d).unwrap();
    }
}

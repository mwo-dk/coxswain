//! Chess in the desktop app (ChessView.svelte): a PGN file's games as a table that fills while
//! the file is read, a game played out, an EPD or FEN file's positions. Reading is
//! coxswain-core's `chess`; this keeps the open file and the table's last order.

use coxswain_core::chess::{self, Library, Position, Query, Row, Token};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type Res<T> = Result<T, String>;

/// The file in view and the one before, each reading its games on.
static LIBS: crate::recent::Recent<Library> = crate::recent::Recent::new();

/// The table's last order: file, query, games counted then, and the games it let through.
type Order = (PathBuf, Query, usize, Arc<Vec<u32>>);
static ORDER: Mutex<Option<Order>> = Mutex::new(None);

fn library(path: &Path) -> Res<Arc<Library>> {
    crate::here(path)?;
    LIBS.get(path, |_| true, || Library::open(path).map_err(|e| e.to_string()))
}

/// The last view of `path` closed: its games are let go, and the pass over the file stops
/// (`Library` stops its thread when dropped). Two views at most are kept in any case.
#[tauri::command]
pub fn chess_close(path: PathBuf) {
    LIBS.forget(&path);
    let mut review = REVIEW.lock().unwrap_or_else(|e| e.into_inner());
    if review.as_ref().is_some_and(|r| r.0 == path) {
        *review = None;
    }
    drop(review);
    let mut last = ORDER.lock().unwrap_or_else(|e| e.into_inner());
    if last.as_ref().is_some_and(|o| o.0 == path) {
        *last = None;
    }
}

/// How the board and the moves look: the square colours for `theme`, and the piece letters.
#[derive(Serialize)]
pub struct Looks {
    colours: [String; 2],
    figurines: [String; 5],
    letters: [String; 5],
    /// `figurines` or `letters`, from Settings.
    notation: String,
    /// Whether the engine is asked from the start (`[chess] analysis`).
    analysis: bool,
}

#[tauri::command]
pub fn chess_looks(theme: String, ctx: tauri::State<'_, crate::Ctx>) -> Looks {
    let cfg = ctx.cfg();
    Looks {
        colours: chess::board_colours(&cfg.gui_theme(Some(&theme)), &cfg.chess.board),
        figurines: chess::pieces(chess::Notation::Figurines),
        letters: chess::pieces(chess::Notation::Letters),
        notation: cfg.chess.notation.clone(),
        analysis: cfg.chess.analysis,
    }
}

#[derive(Serialize)]
pub struct Rows {
    progress: chess::Progress,
    /// Whose games: `pgn` (chess), `pdn` (draughts) or `xiangqi`.
    format: chess::Format,
    /// The games the query lets through, of `progress.games`.
    total: usize,
    /// From `offset`: each with its index in the file (from 0).
    rows: Vec<(u32, Row)>,
}

/// The table's rows `offset..offset + limit` for `query`; the order is worked out again only
/// when the query changed or more games were read.
#[tauri::command]
pub async fn chess_rows(path: PathBuf, query: Query, offset: usize, limit: usize) -> Res<Rows> {
    tauri::async_runtime::spawn_blocking(move || {
        let lib = library(&path)?;
        let progress = lib.progress();
        let ids = {
            let mut last = ORDER.lock().unwrap_or_else(|e| e.into_inner());
            match last.as_ref() {
                Some((p, q, n, ids)) if *p == path && *q == query && *n == progress.games => ids.clone(),
                _ => {
                    let ids = Arc::new(lib.query(&query));
                    *last = Some((path.clone(), query, progress.games, ids.clone()));
                    ids
                }
            }
        };
        let rows = ids.iter().skip(offset).take(limit.min(500)).filter_map(|&i| lib.row(i).map(|r| (i, r))).collect();
        Ok(Rows { progress, format: lib.format, total: ids.len(), rows })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct GameView {
    game: chess::Game,
    /// A draughts game's variant, as its heading says it: "International draughts, 10×10".
    variant: Option<String>,
    layout: Vec<Token>,
    /// Shown above the moves when one could not be played.
    error: Option<String>,
}

/// Game `index` (from 0) of the file, read and played out.
#[tauri::command]
pub async fn chess_game(path: PathBuf, index: u32) -> Res<GameView> {
    tauri::async_runtime::spawn_blocking(move || {
        let lib = library(&path)?;
        let game = lib.game(index).map_err(|e| e.to_string())?;
        let error = game.error.as_ref().map(|e| coxswain_core::t!("chess.cannot_play", "move" => e));
        let variant = (lib.format == chess::Format::Pdn).then(|| coxswain_core::draughts::variant(&game).name());
        Ok(GameView { layout: game.layout(), game, variant, error })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The positions of an EPD or FEN file.
#[tauri::command]
pub async fn chess_positions(path: PathBuf) -> Res<Vec<Position>> {
    crate::here(&path)?;
    tauri::async_runtime::spawn_blocking(move || {
        chess::read_positions(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The game a Find hit in a PGN file is about, from its snippet.
#[tauri::command]
pub async fn chess_hit(path: PathBuf, snippet: String) -> Option<u32> {
    tauri::async_runtime::spawn_blocking(move || chess::games::game_of_hit(&path, &snippet)).await.ok().flatten()
}

#[derive(Serialize)]
pub struct BookView {
    /// The book's file name.
    name: String,
    entries: u64,
    moves: Vec<chess::polyglot::BookMove>,
}

/// The opening book's moves for the position `fen` of a game in the file at `path`: the file
/// itself when it is a book, else the book Settings names or the first one next to the file.
#[tauri::command]
pub async fn chess_book(path: PathBuf, fen: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<Option<BookView>> {
    let configured = ctx.cfg().chess.book.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::here(&path)?;
        let Some(book) = chess::polyglot::find(&path, &configured) else { return Ok(None) };
        let entries = chess::polyglot::Book::open(&book).map_or(0, |b| b.entries());
        let name = book.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Some(BookView { name, entries, moves: chess::polyglot::moves_at(&book, &fen) }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Whether the file is a Polyglot opening book (`chess::sniff`), for the preview.
#[tauri::command]
pub async fn chess_is_book(path: PathBuf) -> bool {
    tauri::async_runtime::spawn_blocking(move || chess::polyglot::is_book(&path)).await.unwrap_or(false)
}

/// The legal moves of the position `fen`, each with the position after it (`chess::play`).
#[tauri::command]
pub fn chess_moves(fen: String) -> Vec<chess::play::Played> {
    chess::play::moves(&fen)
}

/// The last position the engine was asked about: an older question stops when a newer one comes.
static ASKED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What the installed engine (`[chess] engine`, else Stockfish) says about the position `fen`,
/// after `[chess] engine_time`. It runs on this machine; nothing is sent anywhere.
#[tauri::command]
pub async fn chess_analyse(fen: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<chess::play::Analysis> {
    use std::sync::atomic::Ordering;
    let cfg = ctx.cfg().clone();
    let (ms, lines) = (cfg.chess.engine_time, cfg.chess.engine_lines);
    let me = ASKED.fetch_add(1, Ordering::Relaxed) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let program = chess::play::engine_for(&cfg)?;
        chess::play::analyse(&program, &fen, ms, lines, &|| ASKED.load(Ordering::Relaxed) != me).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A line played on from the board, saved as a new PGN file next to the game file at `path`:
/// the game's `tags`, `sans` played from `start`. Its name, for the status line.
#[tauri::command]
pub async fn chess_save_line(path: PathBuf, tags: Vec<(String, String)>, start: String, sans: Vec<String>) -> Res<String> {
    crate::here(&path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let text = chess::play::line_pgn(&tags, &start, &sans);
        let p = chess::play::save_beside(&path, "line", &text).map_err(|e| coxswain_core::t!("chess.line_not_saved", "error" => e))?;
        Ok(coxswain_core::t!("chess.line_saved", "name" => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The game analysed as a whole, in the background: its file and index, and the analysis.
static REVIEW: Mutex<Option<(PathBuf, u32, chess::review::Reviewer)>> = Mutex::new(None);

/// The whole-game analysis of game `index` of `path`: `start` begins it (from the cache when
/// it was done before), `stop` stops it, `state` says how far it is. Another game's is let go.
#[tauri::command]
pub async fn chess_review(path: PathBuf, index: u32, act: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<Option<chess::review::Review>> {
    let cfg = ctx.cfg().clone();
    let ms = cfg.chess.review_time;
    tauri::async_runtime::spawn_blocking(move || {
        let mut cur = REVIEW.lock().unwrap_or_else(|e| e.into_inner());
        if cur.as_ref().is_some_and(|(p, i, _)| *p != path || *i != index) {
            *cur = None;
        }
        match act.as_str() {
            "start" => {
                let program = chess::play::engine_for(&cfg)?;
                let game = library(&path)?.game(index).map_err(|e| e.to_string())?;
                *cur = Some((path, index, chess::review::Reviewer::start(&program, ms, &game)));
            }
            "stop" => {
                if let Some((.., r)) = cur.as_ref() {
                    r.stop();
                }
            }
            _ => {}
        }
        Ok(cur.as_ref().map(|(.., r)| r.now()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Game `index` of `path` with its analysis in it, as a new PGN file next to `path`.
#[tauri::command]
pub async fn chess_review_save(path: PathBuf, index: u32) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let r = REVIEW
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .filter(|(p, i, _)| *p == path && *i == index)
            .map(|(.., r)| r.now())
            .filter(|r| !r.plies.is_empty())
            .ok_or_else(|| coxswain_core::t!("chess.review_first"))?;
        let game = library(&path)?.game(index).map_err(|e| e.to_string())?;
        let text = chess::review::annotated_pgn(&game, &r);
        let p = chess::play::save_beside(&path, "annotated", &text).map_err(|e| coxswain_core::t!("chess.line_not_saved", "error" => e))?;
        Ok(coxswain_core::t!("chess.line_saved", "name" => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
    })
    .await
    .map_err(|e| e.to_string())?
}

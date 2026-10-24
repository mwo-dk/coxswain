//! Go in the desktop app (GoView.svelte): an SGF file's games as a list, a game on the board,
//! each position played out by coxswain-core's `go` when it is shown.

use coxswain_core::go::{self, Collection, Game, Info, Node, Position};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type Res<T> = Result<T, String>;

/// The files in view, and the last game opened.
static FILES: crate::recent::Recent<Collection> = crate::recent::Recent::new();
static GAME: Mutex<Option<(PathBuf, usize, Arc<Game>)>> = Mutex::new(None);

fn collection(path: &Path) -> Res<Arc<Collection>> {
    crate::here(path)?;
    FILES.get(path, |_| true, || Collection::open(path).map_err(|e| e.to_string()))
}

fn game(path: &Path, index: usize) -> Res<Arc<Game>> {
    let mut last = GAME.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, i, g)) = last.as_ref()
        && p == path
        && *i == index
    {
        return Ok(g.clone());
    }
    let g = Arc::new(collection(path)?.game(index).map_err(|e| e.to_string())?);
    *last = Some((path.to_path_buf(), index, g.clone()));
    Ok(g)
}

#[derive(Serialize)]
pub struct Games {
    /// The games listed since the `from`th.
    games: Vec<Info>,
    progress: coxswain_core::chess::Progress,
}

/// Up to 2,000 of the games listed so far from the `from`th (from 0): the list fills as the
/// file is read, and the view asks again until it has them all and `progress.done`.
#[tauri::command]
pub async fn go_games(path: PathBuf, from: usize) -> Res<Games> {
    tauri::async_runtime::spawn_blocking(move || {
        let c = collection(&path)?;
        // The first answer waits a little for the first games, not for the whole file.
        c.wait_up_to(from.max(2), std::time::Duration::from_millis(250));
        let progress = c.progress();
        // ponytail: the table draws every row; 2,000 at a time keeps the window answering while
        // it grows. A table that draws only the rows in view would take 100,000 at once.
        Ok(Games { games: c.games(from, 2000), progress })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct GameView {
    info: Info,
    nodes: Vec<Node>,
    /// Shown above the board when the game is longer than Coxswain reads.
    note: Option<String>,
}

/// Game `index` (from 0): its facts and its tree of moves.
#[tauri::command]
pub async fn go_game(path: PathBuf, index: usize) -> Res<GameView> {
    tauri::async_runtime::spawn_blocking(move || {
        let g = game(&path, index)?;
        let note = g.capped.then(|| coxswain_core::t!("go.capped", "n" => g.nodes.len()));
        Ok(GameView { info: g.info.clone(), nodes: g.nodes.clone(), note })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct PositionView {
    #[serde(flatten)]
    position: Position,
    /// The node's move as shown ("37 ● Q16"), and the moves going on from it, A, B, …
    text: String,
    branches: Vec<(u32, String)>,
    /// The move number at the end of the line.
    total: u32,
    /// Column letters and row numbers of the board's edges.
    columns: Vec<String>,
    stars: Vec<(u8, u8)>,
}

/// The board after node `node` of game `index`.
#[tauri::command]
pub async fn go_position(path: PathBuf, index: usize, node: u32) -> Res<PositionView> {
    tauri::async_runtime::spawn_blocking(move || {
        let g = game(&path, index)?;
        let node = node.min(g.nodes.len().saturating_sub(1) as u32);
        let position = g.position(node);
        let (w, h) = (position.width, position.height);
        let columns = (0..w).map(|x| go::point_name(w, h, x, 0).trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('-').to_string()).collect();
        Ok(PositionView {
            text: g.move_text(node),
            branches: g.branches(node).into_iter().map(|c| (c, g.move_text(c))).collect(),
            total: g.nodes[g.end_of_line(node) as usize].number,
            columns,
            stars: go::stars(w, h),
            position,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The game a Find hit in an SGF file is about, from its snippet.
#[tauri::command]
pub async fn go_hit(path: PathBuf, snippet: String) -> Option<u32> {
    tauri::async_runtime::spawn_blocking(move || go::game_of_hit(&path, &snippet, &std::sync::atomic::AtomicBool::new(false))).await.ok().flatten()
}

/// The last view of `path` closed: its games are let go.
#[tauri::command]
pub fn go_close(path: PathBuf) {
    FILES.forget(&path);
    let mut last = GAME.lock().unwrap_or_else(|e| e.into_inner());
    if last.as_ref().is_some_and(|g| g.0 == path) {
        *last = None;
    }
}

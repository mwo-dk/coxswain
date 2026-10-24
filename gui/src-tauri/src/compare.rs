//! Compare files in the desktop app (Compare.svelte): the one compare open, read by
//! coxswain-core's `compare` on a thread of its own, and pulled from a screen at a time.

use coxswain_core::compare::bytes::{Count, HexRow};
use coxswain_core::compare::text::{Options, Row, Summary, View};
use coxswain_core::compare::{Done, Kind, Pictures, Session};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

type Res<T> = Result<T, String>;

static OPEN: Mutex<Option<Session>> = Mutex::new(None);

fn open() -> std::sync::MutexGuard<'static, Option<Session>> {
    OPEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// `f` on what the open compare made, if it is ready.
fn with<T>(f: impl FnOnce(&mut Done) -> T) -> Res<Option<T>> {
    let s = open();
    let s = s.as_ref().ok_or("no compare open")?;
    let mut done = s.done();
    Ok(done.as_mut().map(f))
}

/// Start comparing `left` with `right` (the reading before, if any, stops).
#[tauri::command]
pub fn compare_open(left: PathBuf, right: PathBuf, options: Options) -> Res<Kind> {
    crate::here(&left)?;
    crate::here(&right)?;
    let s = Session::open(&left, &right, options);
    let kind = s.kind;
    *open() = Some(s);
    Ok(kind)
}

#[tauri::command]
pub fn compare_close() {
    *open() = None;
}

#[derive(Serialize, Default)]
pub struct State {
    /// Reading, in percent, until `ready`.
    percent: usize,
    ready: bool,
    error: Option<String>,
    summary: Option<Summary>,
    /// Rows in all, of text or hex.
    total: usize,
    pictures: Option<Pictures>,
    count: Option<Count>,
    len: Option<[u64; 2]>,
}

#[tauri::command]
pub fn compare_state(view: View) -> Res<State> {
    let percent = open().as_ref().map_or(0, |s| s.percent());
    let state = with(|d| match d {
        Done::Text(c) => State { ready: true, summary: Some(c.summary()), total: c.total(view), ..State::default() },
        Done::Bytes(b) => State { ready: true, total: b.rows_total() as usize, count: Some(b.counted()), len: Some(b.len), ..State::default() },
        Done::Pictures(p) => State { ready: true, pictures: Some(p.clone()), ..State::default() },
        Done::Failed(e) => State { ready: true, error: Some(e.clone()), ..State::default() },
    })?;
    Ok(state.unwrap_or(State { percent, ..State::default() }))
}

#[tauri::command]
pub async fn compare_rows(view: View, from: usize, n: usize) -> Res<Vec<Row>> {
    crate::blocking(move || Ok(with(|d| if let Done::Text(c) = d { c.rows(view, from, n.min(500)) } else { vec![] })?.unwrap_or_default())).await
}

#[tauri::command]
pub async fn compare_hex(from: u64, n: usize) -> Res<Vec<HexRow>> {
    crate::blocking(move || with(|d| if let Done::Bytes(b) = d { b.rows(from, n.min(500)).map_err(|e| e.to_string()) } else { Ok(vec![]) })?.unwrap_or(Ok(vec![]))).await
}

/// The first row of the next change after `row` (before it, `back`).
#[tauri::command]
pub async fn compare_next(view: View, row: usize, back: bool) -> Res<Option<usize>> {
    crate::blocking(move || {
        let stop = open().as_ref().map(|s| s.stopped()).ok_or("no compare open")?;
        Ok(with(|d| match d {
            Done::Text(c) => c.next(view, row, back),
            Done::Bytes(b) => b.next(row as u64, back, &stop).ok().flatten().map(|r| r as usize),
            _ => None,
        })?
        .flatten())
    })
    .await
}

/// The change at `row` copied from side `from` to the other: the message to show.
#[tauri::command]
pub fn compare_take(view: View, row: usize, from: usize) -> Res<String> {
    Ok(with(|d| match d {
        Done::Text(c) => match c.hunk_at(view, row) {
            None => coxswain_core::t!("compare.not_a_change"),
            Some(h) => c.take(h, from.min(1)).map_or_else(|why| why, |()| coxswain_core::t!("compare.copied")),
        },
        _ => String::new(),
    })?
    .unwrap_or_default())
}

/// The last change copied, put back: its row.
#[tauri::command]
pub fn compare_undo(view: View) -> Res<Option<usize>> {
    Ok(with(|d| if let Done::Text(c) = d { c.undo(view) } else { None })?.flatten())
}

/// The sides with changes saved; `"changed"` when one changed on the disk since (none written
/// unless `force`).
#[tauri::command]
pub async fn compare_save(force: bool) -> Res<()> {
    use coxswain_core::edit::SaveError;
    crate::blocking(move || {
        with(|d| match d {
            Done::Text(c) => c.save(force).map_err(|e| match e {
                SaveError::Changed => "changed".to_string(),
                e => e.to_string(),
            }),
            _ => Ok(()),
        })?
        .unwrap_or(Ok(()))
    })
    .await
}

// ---------------------------------------------------------------- folders

use coxswain_core::compare::folders::{Folders, Mode, State as FileState};

/// The folders being compared (Compare folders, Ctrl+F2), shared with the panes.
static FOLDERS: Mutex<Option<std::sync::Arc<Folders>>> = Mutex::new(None);

pub(crate) fn folders() -> Res<std::sync::Arc<Folders>> {
    FOLDERS.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or_else(|| "no compare open".into())
}

/// Start comparing the folder `left` with `right` (one before stops).
#[tauri::command]
pub fn folders_open(left: PathBuf, right: PathBuf, mode: Mode) -> Res<()> {
    crate::here(&left)?;
    crate::here(&right)?;
    *FOLDERS.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::sync::Arc::new(Folders::open(&left, &right, mode)));
    Ok(())
}

#[tauri::command]
pub fn folders_close() {
    DIFF_ONLY.store(false, Ordering::Relaxed);
    if let Some(f) = FOLDERS.lock().unwrap_or_else(|e| e.into_inner()).take() {
        f.stop();
    }
}

/// Differences only: the tabs' listings leave out what is the same on both sides.
static DIFF_ONLY: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn folders_diff_only(on: bool) {
    DIFF_ONLY.store(on, Ordering::Relaxed);
}

/// With Differences only on, what is the same on both sides left out of a listing (`..` stays).
pub fn hide_same(entries: &mut Vec<coxswain_core::fs::Entry>) {
    if !DIFF_ONLY.load(Ordering::Relaxed) {
        return;
    }
    let Some(f) = FOLDERS.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
    entries.retain(|e| e.is_parent() || (0..2).all(|side| f.state(side, &e.path) != Some(FileState::Same)));
}

/// The entries of `dir` that side `side` has and the other lacks or has older or different
/// (`State::ahead`): what *Mark the differences* marks there.
#[tauri::command]
pub fn folders_ahead(dir: PathBuf, side: usize) -> Res<Vec<PathBuf>> {
    let side = side.min(1);
    Ok(folders_states(dir.clone(), side)?.into_iter().filter(|(_, s)| s.ahead(side)).map(|(n, _)| dir.join(n)).collect())
}

#[derive(Serialize)]
pub struct FoldersState {
    done: bool,
    seen: u64,
    /// The summary line once done (or why it stopped).
    summary: String,
    mode: Mode,
    roots: [PathBuf; 2],
}

#[tauri::command]
pub fn folders_state() -> Res<FoldersState> {
    let f = folders()?;
    let found = f.found();
    let summary = if found.done { found.error.clone().unwrap_or_else(|| found.summary()) } else { coxswain_core::t!("compare.comparing", "n" => coxswain_core::settings::count(f.seen.load(std::sync::atomic::Ordering::Relaxed) as usize)) };
    Ok(FoldersState { done: found.done, seen: f.seen.load(std::sync::atomic::Ordering::Relaxed), summary, mode: f.mode, roots: f.roots.clone() })
}

/// How each entry of `dir` stands, as side `side` sees it (`State::glyph`): name → state.
#[tauri::command]
pub fn folders_states(dir: PathBuf, side: usize) -> Res<std::collections::HashMap<String, FileState>> {
    let f = folders()?;
    let Some(rel) = f.rel(side.min(1), &dir) else { return Ok(Default::default()) };
    let prefix = if rel.is_empty() { String::new() } else { format!("{rel}/") };
    let found = f.found();
    Ok(found
        .states
        .iter()
        .filter_map(|(k, s)| k.strip_prefix(&prefix).filter(|n| !n.contains('/')).map(|n| (n.to_string(), *s)))
        .collect())
}

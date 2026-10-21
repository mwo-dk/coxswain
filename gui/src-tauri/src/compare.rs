//! Compare files in the desktop app (Compare.svelte): the one compare open, read by
//! coxswain-core's `compare` on a thread of its own, and pulled from a screen at a time.

use coxswain_core::compare::bytes::{Count, HexRow};
use coxswain_core::compare::text::{Options, Row, Summary, View};
use coxswain_core::compare::{Done, Kind, Pictures, Session};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;

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
            Some(h) if !c.take(h, from.min(1)) => coxswain_core::t!("compare.read_only"),
            Some(_) => coxswain_core::t!("compare.copied"),
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

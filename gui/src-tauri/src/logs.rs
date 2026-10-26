//! The preview pane's log view (LogView.svelte): logs, JSON Lines and any text too big to read
//! whole, through coxswain-core's `logs::View`, pulled from a screen at a time. Each view the
//! page opens is kept by its number until it is closed.

use coxswain_core::logs::{self, Format, Grew, Level, Shape};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::time::{Duration, Instant};

type Res<T> = Result<T, String>;

static OPEN: Mutex<Option<HashMap<u64, logs::View>>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

/// `f` on view `id`.
fn with<T>(id: u64, f: impl FnOnce(&mut logs::View) -> T) -> Res<T> {
    let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    open.get_or_insert_default().get_mut(&id).map(f).ok_or_else(|| "no such log view".to_string())
}

#[derive(Serialize)]
pub struct Opened {
    id: u64,
    /// A log (by its name or its lines): coloured by level, with the log controls.
    log: bool,
    /// `logs.format.<format>` names it.
    format: &'static str,
    /// Its columns, when its lines have fields, and their widths in characters.
    columns: Vec<String>,
    widths: Vec<usize>,
    /// Stack traces folded at the start (a log up to `logs::FOLD_AT_OPEN`).
    fold: bool,
}

/// Open `path` in a log view; `None` when it is no text read by lines (binary, UTF-16).
#[tauri::command]
pub async fn log_open(path: PathBuf) -> Res<Option<Opened>> {
    crate::here(&path)?;
    crate::blocking(move || {
        let Some(view) = logs::View::open(&path).map_err(|e| e.to_string())? else { return Ok(None) };
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let opened = Opened {
            id,
            log: view.log,
            format: view.format.id(),
            columns: if view.format.has_fields() { view.columns.iter().map(|(c, _)| c.clone()).collect() } else { vec![] },
            widths: if view.format.has_fields() { view.columns.iter().map(|(_, w)| *w).collect() } else { vec![] },
            fold: view.shape().fold,
        };
        OPEN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().insert(id, view);
        Ok(Some(opened))
    })
    .await
}

#[tauri::command]
pub fn log_close(id: u64) {
    if let Some(open) = OPEN.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        open.remove(&id);
    }
}

#[derive(Serialize)]
pub struct State {
    /// Rows known, and whether that is all of them.
    rows: usize,
    done: bool,
    /// Lines found, whether that is all, and whether it stopped at `logs::MOST_LINES`.
    lines: usize,
    lines_done: bool,
    capped: bool,
    /// Some lines are left out or folded: rows are not lines.
    filtered: bool,
    /// Bumped when the rows change: the rows the page holds are stale.
    era: u64,
}

#[tauri::command]
pub fn log_state(id: u64) -> Res<State> {
    with(id, |v| {
        let (rows, done) = v.count();
        let (lines, lines_done) = v.reader().count();
        State { rows, done, lines, lines_done, capped: v.reader().capped(), filtered: v.filtered(), era: v.era() }
    })
}

#[derive(Serialize)]
pub struct Row {
    /// The file's line, from 0.
    line: usize,
    text: String,
    level: Option<Level>,
    /// Lines of a stack trace folded under it.
    folded: u32,
    /// In columns: each column's value, and the fields with no column; none for a line without
    /// fields (the next line of a stack trace).
    cells: Option<Vec<String>>,
    rest: Option<Vec<(String, String)>>,
}

/// Rows `from..from+n`, at most 500; `columns` splits them into their fields.
#[tauri::command]
pub async fn log_rows(id: u64, from: usize, n: usize, columns: bool) -> Res<Vec<Row>> {
    crate::blocking(move || {
        with(id, |v| {
            let n = n.min(500);
            let rows = v.rows(from, from + n);
            let texts = v.texts(from, from + n, logs::LONGEST_READ);
            rows.into_iter()
                .zip(texts)
                .map(|((line, level, folded), text)| {
                    let (cells, rest) = match v.cells(&text).filter(|_| columns) {
                        Some((c, r)) => (Some(c), Some(r)),
                        None => (None, None),
                    };
                    Row { line, text, level, folded, cells, rest }
                })
                .collect()
        })
    })
    .await
}

/// Keep the entries at `level` and above (`None`: all), those with `filter` in them (or a
/// field's value, typed `name=value`), and fold stack traces under their line (`fold`).
#[tauri::command]
pub fn log_shape(id: u64, level: Option<String>, filter: String, fold: bool) -> Res<String> {
    with(id, |v| {
        let mut shape = Shape { level: level.as_deref().and_then(Level::from_name), fold, ..Shape::default() };
        let format = if v.log { v.format.clone() } else { Format::Plain };
        shape.typed(&format, &filter);
        let typed = shape.typed_text();
        v.set_shape(shape);
        typed
    })
}

/// The row of line `line` (from 0), or of the line above it the rows keep; `None` while the
/// rows have not reached it.
#[tauri::command]
pub fn log_row_of(id: u64, line: usize) -> Res<Option<usize>> {
    with(id, |v| v.row_of(line))
}

#[derive(Serialize)]
pub struct Followed {
    /// "no", "more" or "anew" (rotated or cut short: read again from the start).
    grew: &'static str,
    /// With the pause on errors: the first row at an error, of a line from `from` on.
    error: Option<usize>,
}

/// A followed log looked at for new lines; `pause` looks for an error among the rows of the
/// lines from `from` on.
#[tauri::command]
pub fn log_follow(id: u64, pause: bool, from: usize) -> Res<Followed> {
    with(id, |v| {
        let grew = match v.grow() {
            Grew::No => "no",
            Grew::More => "more",
            Grew::Anew => "anew",
        };
        let error = if pause && grew != "anew" { v.first_error(from) } else { None };
        Followed { grew, error }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_is_opened_filtered_followed_and_searched() {
        let d = std::env::temp_dir().join(format!("coxswain-gui-logs-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("app.jsonl");
        std::fs::write(&p, "{\"level\":\"info\",\"msg\":\"start\",\"user\":\"ada\"}\n{\"level\":\"error\",\"msg\":\"lost\",\"user\":\"bob\"}\n").unwrap();
        let o = tauri::async_runtime::block_on(log_open(p.clone())).unwrap().unwrap();
        assert!(o.log && o.format == "json_lines" && o.columns == ["level", "msg", "user"]);
        let wait = || {
            let limit = Instant::now() + coxswain_core::test_limit(Duration::from_secs(10));
            while !log_state(o.id).unwrap().done {
                assert!(Instant::now() < limit);
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        wait();
        let rows = tauri::async_runtime::block_on(log_rows(o.id, 0, 10, true)).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].cells.as_deref(), Some(&["error".to_string(), "lost".into(), "bob".into()][..]));
        assert_eq!(rows[1].level, Some(Level::Error));
        assert_eq!(log_shape(o.id, None, "user=BOB".into(), false).unwrap(), "user=BOB");
        wait();
        let rows = tauri::async_runtime::block_on(log_rows(o.id, 0, 10, false)).unwrap();
        assert_eq!(rows.iter().map(|r| r.line).collect::<Vec<_>>(), [1]);
        log_shape(o.id, Some("warn".into()), String::new(), false).unwrap();
        wait();
        assert_eq!(log_state(o.id).unwrap().rows, 1);
        log_shape(o.id, None, String::new(), false).unwrap();
        std::fs::write(&p, "{\"level\":\"info\",\"msg\":\"start\",\"user\":\"ada\"}\n{\"level\":\"error\",\"msg\":\"lost\",\"user\":\"bob\"}\n{\"level\":\"error\",\"msg\":\"again\"}\n").unwrap();
        let f = log_follow(o.id, true, 2).unwrap();
        assert_eq!(f.grew, "more");
        wait();
        assert_eq!(log_state(o.id).unwrap().rows, 3);
        assert_eq!(log_follow(o.id, true, 2).unwrap().error, Some(2));
        assert_eq!(log_follow(o.id, true, 3).unwrap().error, None, "only errors written since");
        log_close(o.id);
        assert!(log_state(o.id).is_err());
        std::fs::remove_dir_all(&d).ok();
    }
}

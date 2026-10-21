//! *Convert e-book* (`coxswain_core::ebook`): calibre makes an EPUB, AZW3 or PDF of each book,
//! next to it. Asked first (what it writes, which calibre runs, the first pull and its size),
//! one run at a time, the status line saying how far it is with a Stop button.

use coxswain_core::ebook;
use coxswain_core::t;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

type Res<T> = Result<T, String>;

/// What a run would do with `paths` as `to`, for the question before it.
#[derive(Serialize)]
pub struct Plan {
    /// The books among what was picked, and the name each one's copy gets.
    books: Vec<PathBuf>,
    targets: Vec<String>,
    /// Which calibre runs, and the first pull with its size.
    about: String,
    /// Why it cannot run here: turned off, what to install, or which setting to change.
    missing: Option<String>,
}

#[tauri::command]
pub async fn ebook_plan(paths: Vec<PathBuf>, to: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<Plan> {
    let cfg = ctx.cfg().preview.clone();
    crate::blocking(move || {
        // A book only in the cloud is not fetched to be read; one already in `to` has nothing to become.
        let books: Vec<PathBuf> = paths.into_iter().filter(|p| ebook::formats_for(p).contains(&to.as_str()) && ebook::is_ebook(p) && crate::here(p).is_ok() && p.is_file()).collect();
        let targets = books.iter().map(|p| ebook::target(p, &to).file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
        let (about, missing) = match ebook::how(&cfg) {
            Ok(how) => (ebook::about(&how).0, None),
            Err(e) => (String::new(), Some(e)),
        };
        Ok(Plan { books, targets, about, missing })
    })
    .await
}

/// How far a run is, for the status line.
#[derive(Serialize, Clone, Default)]
pub struct Progress {
    /// The book being worked on, counted from 0, of `all`.
    at: usize,
    all: usize,
    name: String,
    /// What calibre (or the pull) said last.
    line: String,
}

static RUN: Mutex<Option<Progress>> = Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);

/// Convert each of `paths` into `to`, one after the other. The files made, or what failed
/// (`name: why`, one per line).
#[tauri::command]
pub async fn ebook_run(paths: Vec<PathBuf>, to: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<Vec<PathBuf>> {
    for p in &paths {
        crate::here(p)?;
    }
    let cfg = ctx.cfg().preview.clone();
    {
        let mut s = RUN.lock().unwrap();
        if s.is_some() {
            return Err(t!("ebook.busy_already"));
        }
        *s = Some(Progress { all: paths.len(), ..Default::default() });
    }
    STOP.store(false, Ordering::Relaxed);
    let result = crate::blocking(move || {
        let set = |f: &dyn Fn(&mut Progress)| {
            if let Some(p) = RUN.lock().unwrap().as_mut() {
                f(p)
            }
        };
        let names: Vec<String> = paths.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
        let done = ebook::run_all(&cfg, &to, &paths, &STOP, |i| set(&|p| (p.at, p.name, p.line) = (i, names[i].clone(), String::new())), |said| {
            let line = match said {
                ebook::Said::Pulling(l) => format!("{} {l}", t!("convert.pulling", "image" => cfg.images.get(ebook::TOOL).cloned().unwrap_or_default())),
                ebook::Said::Line(l) => l.to_string(),
            };
            set(&|p| p.line = line.clone());
        });
        if let Ok(ebook::How::Container { image, .. }) = ebook::how(&cfg) {
            crate::convert::USED.lock().unwrap().insert(image);
        }
        // Done here, on this thread, so an app that is ending sees it (`end`).
        *RUN.lock().unwrap() = None;
        let (mut made, mut failed) = (vec![], vec![]);
        for (i, r) in done.into_iter().enumerate() {
            match r {
                Ok(p) => made.push(p),
                Err(e) => failed.push(format!("{}: {e}", names.get(i).cloned().unwrap_or_default())),
            }
        }
        if failed.is_empty() { Ok(made) } else { Err(failed.join("\n")) }
    })
    .await;
    *RUN.lock().unwrap() = None;
    let used = std::mem::take(&mut *crate::convert::USED.lock().unwrap());
    if !used.is_empty() {
        let now = coxswain_core::fs::now();
        let _ = ctx.edit(|st| used.iter().for_each(|i| coxswain_core::images::used(st, i, now)));
    }
    result
}

#[tauri::command]
pub fn ebook_progress() -> Option<Progress> {
    RUN.lock().unwrap().clone()
}

/// Stop the run under way: the book being worked on gets no copy, the rest are not started.
#[tauri::command]
pub fn ebook_stop() {
    STOP.store(true, Ordering::Relaxed);
}

/// Stop the run under way and wait for it, ten seconds at most: the app is ending.
pub fn end() {
    ebook_stop();
    for _ in 0..100 {
        if RUN.lock().unwrap().is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

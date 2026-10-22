//! Checksums (`coxswain_core::checksum`) on a worker thread, one run at a time: made for the
//! marked entries, or a list checked; how far it is for the status line, and Stop.

use coxswain_core::checksum::{self, Algo, Checked, Made, Progress};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

type Res<T> = Result<T, String>;

static WORK: Mutex<Option<Progress>> = Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);

/// Run `f` on a blocking thread as the one run under way, its progress in `WORK`.
async fn run<T: Send + 'static>(f: impl FnOnce(&AtomicBool, &mut dyn FnMut(&Progress)) -> T + Send + 'static) -> Res<T> {
    {
        let mut w = WORK.lock().unwrap();
        if w.is_some() {
            return Err(coxswain_core::t!("sums.busy_already"));
        }
        *w = Some(Progress::default());
    }
    STOP.store(false, Ordering::Relaxed);
    let r = tauri::async_runtime::spawn_blocking(move || {
        let r = f(&STOP, &mut |p| *WORK.lock().unwrap() = Some(p.clone()));
        *WORK.lock().unwrap() = None;
        r
    })
    .await
    .map_err(|e| e.to_string());
    *WORK.lock().unwrap() = None;
    r
}

/// The algorithms, each with the name its list would get in `dir`: `["sha256", "SHA-256", "SHA256SUMS"]`.
#[tauri::command]
pub fn sums_algos(dir: PathBuf) -> Vec<(String, String, String)> {
    Algo::ALL
        .into_iter()
        .map(|a| (a.id().into(), a.name().into(), checksum::target(&dir, a).file_name().unwrap_or_default().to_string_lossy().into_owned()))
        .collect()
}

#[tauri::command]
pub async fn sums_make(dir: PathBuf, paths: Vec<PathBuf>, algo: String) -> Res<Made> {
    let algo = Algo::from_id(&algo).ok_or("no such algorithm")?;
    for p in &paths {
        crate::here(p)?;
    }
    run(move |stop, at| checksum::make(&dir, &paths, algo, stop, at)).await?.map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct Verified {
    checked: Vec<Checked>,
    stopped: bool,
    summary: String,
    /// Each one's state in words, in the order of `checked`.
    words: Vec<String>,
}

#[tauri::command]
pub async fn sums_verify(list: PathBuf) -> Res<Verified> {
    let (checked, stopped) = run(move |stop, at| checksum::verify(&list, stop, at)).await?.map_err(|e| e.to_string())?;
    let words = checked.iter().map(|c| checksum::word(&c.state)).collect();
    Ok(Verified { summary: checksum::summary(&checked, stopped), checked, stopped, words })
}

/// How far the run is, with its status line already in words for `what`.
#[tauri::command]
pub fn work_progress(what: String) -> Option<String> {
    WORK.lock().unwrap().as_ref().map(|p| p.line(&what))
}

#[tauri::command]
pub fn work_stop() {
    STOP.store(true, Ordering::Relaxed);
}

//! *Scan for viruses* in the desktop app (coxswain-core's `viruses`): the question before it,
//! the scan on a thread with its progress for the status line and Stop, and what to do with
//! each file found.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use coxswain_core::t;
use coxswain_core::viruses::{self, Choice, Finding, Report, Said};
use serde::Serialize;

type Res<T> = Result<T, String>;

/// What a scan of `paths` would do, for the question before it; or why it cannot run.
#[derive(Serialize)]
pub struct Plan {
    files: usize,
    text: String,
    missing: Option<String>,
    /// Scanning is off in Settings.
    off: bool,
}

#[tauri::command]
pub async fn viruses_plan(paths: Vec<PathBuf>, ctx: tauri::State<'_, crate::Ctx>) -> Res<Plan> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || match viruses::plan(&cfg, &paths) {
        Ok(p) => Plan { files: p.files, text: p.text, missing: None, off: false },
        Err(e) => Plan { files: 0, text: String::new(), missing: Some(e), off: !cfg.virus_scan },
    })
    .await
    .map_err(|e| e.to_string())
}

/// What a scan under way said last, for the status line.
static PROGRESS: Mutex<Option<String>> = Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);

/// Scan `paths`; one scan at a time in the app. The report, with the database it used.
#[tauri::command]
pub async fn viruses_run(paths: Vec<PathBuf>, ctx: tauri::State<'_, crate::Ctx>) -> Res<Report> {
    let cfg = ctx.cfg().preview.clone();
    {
        let mut p = PROGRESS.lock().unwrap();
        if p.is_some() {
            return Err(t!("viruses.busy_already"));
        }
        *p = Some(t!("viruses.busy_start"));
    }
    STOP.store(false, Ordering::Relaxed);
    let image = cfg.images.get(viruses::TOOL).cloned().unwrap_or_default();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let r = viruses::scan(&cfg, &paths, &STOP, |s| {
            let line = match s {
                Said::Pulling(l) => format!("{} {l}", t!("convert.pulling", "image" => image)),
                Said::Updating(l) => format!("{} · {l}", t!("viruses.updating")),
                Said::Scanned(n, all) => t!("viruses.busy", "n" => n, "all" => all),
            };
            if let Some(p) = PROGRESS.lock().unwrap().as_mut() {
                *p = line;
            }
        });
        // Done here, on this thread, so an app that is ending sees it (`viruses_end`).
        *PROGRESS.lock().unwrap() = None;
        r
    })
    .await
    .map_err(|e| e.to_string());
    *PROGRESS.lock().unwrap() = None;
    if let Ok((_, _, image)) = viruses::how(&ctx.cfg().preview) {
        let now = coxswain_core::fs::now();
        let _ = ctx.edit(|st| coxswain_core::images::used(st, &image, now));
    }
    if STOP.load(Ordering::Relaxed) {
        return Err(t!("viruses.stopped"));
    }
    let report = result??;
    // What was found, each file as it was then, kept here: what is done to them is chosen in
    // the window, but which files they are is never taken from it.
    *FOUND.lock().unwrap() = report.found.clone();
    Ok(report)
}

/// The files the last scan found, with their identity (`Finding::id`).
static FOUND: Mutex<Vec<Finding>> = Mutex::new(Vec::new());

#[tauri::command]
pub fn viruses_progress() -> Option<String> {
    PROGRESS.lock().unwrap().clone()
}

/// Stop the scan under way: its container is killed.
#[tauri::command]
pub fn viruses_stop() {
    STOP.store(true, Ordering::Relaxed);
}

/// Stop the scan under way and wait for it, ten seconds at most: the app is ending.
pub fn viruses_end() {
    viruses_stop();
    for _ in 0..100 {
        if PROGRESS.lock().unwrap().is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// What was done with the files found.
#[derive(Serialize)]
pub struct Done {
    bin: usize,
    quarantine: usize,
    left: usize,
    failed: Vec<String>,
}

/// Do with each file the last scan found what the user chose for it, by its place in the list.
#[tauri::command]
pub fn viruses_act(choices: Vec<Choice>) -> Done {
    let mut d = Done { bin: 0, quarantine: 0, left: 0, failed: vec![] };
    let found = std::mem::take(&mut *FOUND.lock().unwrap());
    for (f, c) in found.iter().zip(&choices) {
        match viruses::act(f, *c) {
            Ok(_) if *c == Choice::Bin => d.bin += 1,
            Ok(_) if *c == Choice::Quarantine => d.quarantine += 1,
            Ok(_) => d.left += 1,
            Err(e) => {
                d.left += 1;
                d.failed.push(e);
            }
        }
    }
    d
}

/// The database line and the quarantine folder, for the dialog of what was found.
#[tauri::command]
pub fn viruses_facts(db: Option<u64>) -> (String, String) {
    (viruses::db_line(db), viruses::quarantine_dir().map(|d| d.display().to_string()).unwrap_or_default())
}

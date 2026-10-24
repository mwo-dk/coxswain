//! Make one side like the other (coxswain-core's `compare::sync`), from Compare folders' menu:
//! the plan, then the run, each on a thread, one at a time; the page asks how far it is and can
//! stop it. The two folders are the compare's own, never the page's.

use coxswain_core::compare::sync::{self, Plan, Progress};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::Res;

static RUN: Mutex<Option<(Arc<Progress>, Arc<AtomicBool>)>> = Mutex::new(None);

/// The last plan, as made here: the run takes what it saw, whatever the page sends.
static PLAN: Mutex<Option<Plan>> = Mutex::new(None);

fn begin() -> Res<(Arc<Progress>, Arc<AtomicBool>)> {
    let mut run = RUN.lock().map_err(|e| e.to_string())?;
    if run.is_some() {
        return Err(coxswain_core::t!("sync.busy"));
    }
    let pair = (Arc::new(Progress::default()), Arc::new(AtomicBool::new(false)));
    *run = Some(pair.clone());
    Ok(pair)
}

fn end() {
    if let Ok(mut run) = RUN.lock() {
        *run = None;
    }
}

/// A plan as the page shows it: its line, and each step's word, path, and whether it is a
/// folder or replaces something newer.
#[derive(Serialize)]
pub struct Shown {
    plan: Plan,
    summary: String,
    rows: Vec<(String, String, bool, bool)>,
}

/// Why the side `to` (0 left, 1 right) cannot be made like the other, if it cannot.
#[tauri::command]
pub fn sync_check(to: usize) -> Res<()> {
    let f = crate::compare::folders()?;
    let to = to.min(1);
    sync::check(&f.roots[1 - to], &f.roots[to])
}

/// Plan making side `to` like the other; with `delete`, what is only on `to` goes too.
#[tauri::command]
pub async fn sync_plan(to: usize, delete: bool) -> Res<Shown> {
    let f = crate::compare::folders()?;
    let to = to.min(1);
    let (from, dest, mode) = (f.roots[1 - to].clone(), f.roots[to].clone(), f.mode);
    let (progress, stop) = begin()?;
    let r = tauri::async_runtime::spawn_blocking(move || sync::plan(&from, &dest, mode, delete, &stop, &progress)).await;
    end();
    let plan = r.map_err(|e| e.to_string())??;
    *PLAN.lock().map_err(|e| e.to_string())? = Some(plan.clone());
    let rows = plan.rows.iter().map(|r| (r.act.label(), r.shown(), r.older, r.act == sync::Act::Delete)).collect();
    Ok(Shown { summary: plan.summary(), plan, rows })
}

#[derive(Serialize)]
pub struct Done {
    summary: String,
    failed: Vec<String>,
}

#[tauri::command]
pub async fn sync_apply(token: u64, ctx: tauri::State<'_, crate::Ctx>) -> Res<Done> {
    let (progress, stop) = begin()?;
    let Some(plan) = PLAN.lock().ok().and_then(|mut p| p.take_if(|p| p.token == token)) else {
        end();
        return Err(coxswain_core::t!("pinned.changed"));
    };
    let r = tauri::async_runtime::spawn_blocking(move || sync::apply(&plan, &stop, &progress)).await;
    end();
    let done = r.map_err(|e| e.to_string())??;
    let mut summary = done.summary();
    if let Some(k) = done.kept_text() {
        summary += &format!(" · {k}");
    }
    let out = Done { summary, failed: done.failed.clone() };
    if !done.record.is_empty() {
        ctx.remember(done.record);
    }
    Ok(out)
}

#[tauri::command]
pub fn sync_progress() -> Option<usize> {
    let run = RUN.lock().ok()?;
    Some(run.as_ref()?.0.files.load(Ordering::Relaxed))
}

#[tauri::command]
pub fn sync_stop() {
    if let Ok(run) = RUN.lock()
        && let Some((_, stop)) = run.as_ref()
    {
        stop.store(true, Ordering::Relaxed);
    }
}

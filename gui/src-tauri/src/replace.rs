//! Find and replace across files (coxswain-core's `replace`): the plan, then the run, each on a
//! thread, one at a time; the page asks how far it is and can stop it.

use coxswain_core::replace::{self, Pick, Plan, Progress, Query};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::Res;

static RUN: Mutex<Option<(Arc<Progress>, Arc<AtomicBool>)>> = Mutex::new(None);

/// Take the one run there may be, or say one is under way.
fn begin() -> Res<(Arc<Progress>, Arc<AtomicBool>)> {
    let mut run = RUN.lock().map_err(|e| e.to_string())?;
    if run.is_some() {
        return Err(coxswain_core::t!("replace.busy"));
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

/// The last plan, as made here: the run writes what it read, whatever the page sends back but
/// its ticks.
static PLAN: Mutex<Option<Plan>> = Mutex::new(None);

/// What `query` would change in `roots` (files, and folders walked through).
#[tauri::command]
pub async fn replace_plan(roots: Vec<PathBuf>, query: Query) -> Res<Plan> {
    let (progress, stop) = begin()?;
    let r = tauri::async_runtime::spawn_blocking(move || replace::plan(&roots, &query, &stop, &progress)).await;
    end();
    let plan = r.map_err(|e| e.to_string())??;
    *PLAN.lock().map_err(|e| e.to_string())? = Some(plan.clone());
    Ok(plan)
}

#[derive(Serialize)]
pub struct Replaced {
    summary: String,
    failed: Vec<String>,
    files: usize,
}

/// Write the ticked changes; what can be undone is kept for Ctrl+Z.
#[tauri::command]
pub async fn replace_apply(picks: Vec<Pick>, token: u64, ctx: tauri::State<'_, crate::Ctx>) -> Res<Replaced> {
    let (progress, stop) = begin()?;
    let Some(plan) = PLAN.lock().ok().and_then(|mut p| p.take_if(|p| p.token == token)) else {
        end();
        return Err(coxswain_core::t!("pinned.changed"));
    };
    let r = tauri::async_runtime::spawn_blocking(move || replace::apply(&plan, &picks, &stop, &progress)).await;
    end();
    let done = r.map_err(|e| e.to_string())??;
    let out = Replaced { summary: done.summary(), failed: done.failed.clone(), files: done.files };
    if !done.record.is_empty() {
        ctx.remember(done.record);
    }
    Ok(out)
}

/// Files looked at (or written) and changes found (or made) so far; nothing when none runs.
#[tauri::command]
pub fn replace_progress() -> Option<(usize, usize)> {
    let run = RUN.lock().ok()?;
    let (p, _) = run.as_ref()?;
    Some((p.files.load(Ordering::Relaxed), p.changes.load(Ordering::Relaxed)))
}

/// Stop the plan or run under way: a plan keeps what it found, a run the files it wrote.
#[tauri::command]
pub fn replace_stop() {
    if let Ok(run) = RUN.lock()
        && let Some((_, stop)) = run.as_ref()
    {
        stop.store(true, Ordering::Relaxed);
    }
}

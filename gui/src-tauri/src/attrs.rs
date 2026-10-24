//! Permissions and dates in bulk (coxswain-core's `attrs`): the plan, then the run, each on a
//! thread, one at a time; the page asks how far it is and can stop it.

use coxswain_core::attrs::{self, Plan, Progress, Spec};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::Res;

static RUN: Mutex<Option<(Arc<Progress>, Arc<AtomicBool>)>> = Mutex::new(None);

/// The last plan, as made here: the run changes what it saw, whatever the page sends.
static PLAN: Mutex<Option<Plan>> = Mutex::new(None);

fn begin() -> Res<(Arc<Progress>, Arc<AtomicBool>)> {
    let mut run = RUN.lock().map_err(|e| e.to_string())?;
    if run.is_some() {
        return Err(coxswain_core::t!("attrs.busy"));
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

/// A plan's rows as the page shows them: the path, and before → after as text.
#[derive(Serialize)]
pub struct Shown {
    plan: Plan,
    rows: Vec<(String, String, String)>,
}

#[tauri::command]
pub async fn attrs_plan(roots: Vec<PathBuf>, spec: Spec, base: PathBuf) -> Res<Shown> {
    let (progress, stop) = begin()?;
    let r = tauri::async_runtime::spawn_blocking(move || attrs::plan(&roots, &spec, &base, &stop, &progress)).await;
    end();
    let plan = r.map_err(|e| e.to_string())??;
    *PLAN.lock().map_err(|e| e.to_string())? = Some(plan.clone());
    let rows = plan
        .rows
        .iter()
        .map(|r| {
            let mode = if r.before.mode != r.after.mode { format!("{} → {}", attrs::show_mode(r.before.mode), attrs::show_mode(r.after.mode)) } else { String::new() };
            let date = if r.before.modified != r.after.modified { format!("{} → {}", attrs::show_date(r.before.modified), attrs::show_date(r.after.modified)) } else { String::new() };
            (r.path.to_string_lossy().into_owned(), mode, date)
        })
        .collect();
    Ok(Shown { plan, rows })
}

#[derive(Serialize)]
pub struct Changed {
    summary: String,
    failed: Vec<String>,
}

#[tauri::command]
pub async fn attrs_apply(token: u64, ctx: tauri::State<'_, crate::Ctx>) -> Res<Changed> {
    let (progress, stop) = begin()?;
    let Some(plan) = PLAN.lock().ok().and_then(|mut p| p.take_if(|p| p.token == token)) else {
        end();
        return Err(coxswain_core::t!("pinned.changed"));
    };
    let r = tauri::async_runtime::spawn_blocking(move || attrs::apply(&plan, &stop, &progress)).await;
    end();
    let done = r.map_err(|e| e.to_string())??;
    let out = Changed { summary: done.summary(), failed: done.failed.clone() };
    if !done.record.is_empty() {
        ctx.remember(done.record);
    }
    Ok(out)
}

#[tauri::command]
pub fn attrs_progress() -> Option<usize> {
    let run = RUN.lock().ok()?;
    Some(run.as_ref()?.0.files.load(Ordering::Relaxed))
}

#[tauri::command]
pub fn attrs_stop() {
    if let Ok(run) = RUN.lock()
        && let Some((_, stop)) = run.as_ref()
    {
        stop.store(true, Ordering::Relaxed);
    }
}

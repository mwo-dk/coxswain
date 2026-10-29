//! Fetch and Pull (`coxswain_core::remote`) on a worker thread, one at a time: the tool's last
//! progress line for the status line, and Stop (Esc).

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static LINE: Mutex<Option<String>> = Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);

/// `op` is `fetch` or `pull`. What came of it in words, or why not (the tool's reason under
/// ours); a stop says so too, as an error.
#[tauri::command]
pub async fn remote_run(dir: PathBuf, op: String) -> Result<String, String> {
    use coxswain_core::remote::{self, Op};
    let op = if op == "fetch" { Op::Fetch } else { Op::Pull };
    {
        let mut l = LINE.lock().unwrap();
        if l.is_some() {
            return Err(coxswain_core::t!("remote.busy_already"));
        }
        *l = Some(String::new());
    }
    STOP.store(false, Ordering::Relaxed);
    let r = tauri::async_runtime::spawn_blocking(move || remote::run(&dir, op, &STOP, &mut |l| *LINE.lock().unwrap() = Some(l.to_string())))
        .await
        .map_err(|e| e.to_string());
    *LINE.lock().unwrap() = None;
    r?.map_err(|e| e.to_string())
}

/// The tool's last progress line; none when nothing runs.
#[tauri::command]
pub fn remote_progress() -> Option<String> {
    LINE.lock().unwrap().clone()
}

#[tauri::command]
pub fn remote_stop() {
    STOP.store(true, Ordering::Relaxed);
}

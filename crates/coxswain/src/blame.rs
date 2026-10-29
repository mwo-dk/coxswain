//! Who changed each line and a function's history in the viewer and the editor: the work runs
//! on a thread (`coxswain_core::blame`), Esc stops it, and the answer comes on a later draw.

use coxswain_core::blame::{Blame, History};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

/// Work on a thread, with its stop.
pub(crate) struct Job<T> {
    rx: Receiver<std::io::Result<T>>,
    stop: Arc<AtomicBool>,
}

impl<T: Send + 'static> Job<T> {
    pub(crate) fn start(f: impl FnOnce(&AtomicBool) -> std::io::Result<T> + Send + 'static) -> Job<T> {
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f(&s));
        });
        Job { rx, stop }
    }

    /// Its answer, once there is one.
    pub(crate) fn poll(&self) -> Option<std::io::Result<T>> {
        self.rx.try_recv().ok()
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// What a viewer or the editor has of blame: the answer, and the work still going on.
#[derive(Default)]
pub(crate) struct Git {
    pub blame: Option<Blame>,
    blaming: Option<Job<Blame>>,
    tracing: Option<Job<History>>,
}

/// What came of the work since the last look.
pub(crate) enum Came {
    Nothing,
    Said(String),
    History(History),
}

impl Git {
    pub(crate) fn busy(&self) -> bool {
        self.blaming.is_some() || self.tracing.is_some()
    }

    /// Blame shown, or put away when it is: `Some` with what to say.
    pub(crate) fn toggle_blame(&mut self, path: &Path) -> String {
        if self.blame.take().is_some() {
            return coxswain_core::t!("blame.off");
        }
        let path = path.to_path_buf();
        self.blaming = Some(Job::start(move |stop| coxswain_core::blame::blame(&path, stop)));
        coxswain_core::t!("blame.reading")
    }

    /// The history of the function at `line` (lines `line..=to` when more are picked).
    pub(crate) fn trace(&mut self, path: &Path, text: String, line: usize, to: usize) -> String {
        let path = path.to_path_buf();
        self.tracing = Some(Job::start(move |stop| coxswain_core::blame::line_history(&path, &text, line, to, stop)));
        coxswain_core::t!("blame.tracing")
    }

    /// Esc while work goes on: it stops. `true` when there was some.
    pub(crate) fn stop(&mut self) -> bool {
        let busy = self.busy();
        self.blaming = None;
        self.tracing = None;
        busy
    }

    pub(crate) fn poll(&mut self) -> Came {
        if let Some(r) = self.blaming.as_ref().and_then(Job::poll) {
            self.blaming = None;
            return match r {
                Ok(b) => {
                    let said = if b.cut { coxswain_core::t!("blame.cut", "n" => coxswain_core::blame::MOST_LINES) } else { coxswain_core::t!("blame.shown") };
                    self.blame = Some(b);
                    Came::Said(said)
                }
                Err(e) => Came::Said(e.to_string()),
            };
        }
        if let Some(r) = self.tracing.as_ref().and_then(Job::poll) {
            self.tracing = None;
            return match r {
                Ok(h) if h.changes.is_empty() => Came::Said(coxswain_core::t!("blame.no_changes")),
                Ok(h) => Came::History(h),
                Err(e) => Came::Said(e.to_string()),
            };
        }
        Came::Nothing
    }

    /// Line `i`'s margin (from 0), when blame is shown: its width is `WIDTH`.
    pub(crate) fn margin(&self, i: usize) -> Option<String> {
        let b = self.blame.as_ref()?;
        Some(b.of(i).map_or_else(|| " ".repeat(WIDTH - 1), |w| w.margin.clone()) + " ")
    }
}

/// Columns the margin takes, its space after included.
pub(crate) const WIDTH: usize = 32;

/// A history's title in a viewer: the function's name, or the lines.
pub(crate) fn title(h: &History, file: &Path) -> String {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if h.name.is_empty() {
        coxswain_core::t!("blame.title_lines", "name" => name, "from" => h.from, "to" => h.to)
    } else {
        coxswain_core::t!("blame.title_function", "name" => name, "function" => &h.name)
    }
}

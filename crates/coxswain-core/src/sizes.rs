//! Folder sizes in the background, for both apps. The helper's store knows the home folder's
//! sizes, so those come at once. Other folders are measured: two threads so measuring never
//! takes the machine, a stop flag for when the folder is left, and a short memory so going back
//! to a folder shows its sizes at once.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::helper::Client;

use rayon::prelude::*;

/// How long a measured size is believed.
// ponytail: outside the store's folders, a change deep inside a folder goes unnoticed for this
// long; the store itself follows the file watcher.
const FRESH: Duration = Duration::from_secs(300);

/// Bytes and number of files.
pub type Size = (u64, u64);

pub struct Sizer {
    pool: rayon::ThreadPool,
    known: Mutex<HashMap<PathBuf, (Size, Instant)>>,
    /// The helper, whose store knows sizes.
    index: Option<Arc<Client>>,
    /// Paths the app changed, and when (seconds since the Unix epoch): the store's sizes of
    /// the folders they are in are old until its next walk.
    changed: Mutex<Vec<(PathBuf, u64)>>,
}

impl Default for Sizer {
    fn default() -> Self {
        Sizer::new(None)
    }
}

impl Sizer {
    pub fn new(index: Option<Arc<Client>>) -> Sizer {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(2).thread_name(|i| format!("coxswain-sizes-{i}")).build().expect("a thread pool");
        Sizer { pool, known: Mutex::default(), index, changed: Mutex::default() }
    }

    /// The size measured a short while ago, if any.
    pub fn known(&self, path: &Path) -> Option<Size> {
        let (size, at) = *self.known.lock().ok()?.get(path)?;
        (at.elapsed() < FRESH).then_some(size)
    }

    /// The size of `path`, remembered or measured now. `None` when `stop` was set meanwhile,
    /// and for folders that are not on a disk (`/proc`).
    pub fn measure(&self, path: &Path, stop: &AtomicBool) -> Option<Size> {
        if let Some(size) = self.known(path) {
            return Some(size);
        }
        if not_on_disk(path) {
            return None;
        }
        if let Some(size) = self.stored(path) {
            return Some(size);
        }
        let size = self.pool.install(|| walk(path, stop))?;
        self.known.lock().ok()?.insert(path.to_path_buf(), (size, Instant::now()));
        Some(size)
    }

    /// Something at `changed` was added, removed or written: forget every folder it is in.
    pub fn forget(&self, changed: &Path) {
        if let Ok(mut known) = self.known.lock() {
            known.retain(|folder, _| !changed.starts_with(folder));
        }
        if let Ok(mut all) = self.changed.lock() {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
            // The last ten minutes are enough: the store walks every ten.
            all.retain(|(_, at)| at + 900 > now);
            all.push((changed.to_path_buf(), now));
        }
    }

    /// The store's size of the folder, unless the app changed something in it since.
    fn stored(&self, dir: &Path) -> Option<Size> {
        if !fs::symlink_metadata(dir).ok()?.is_dir() {
            return None;
        }
        let (size, walked) = self.index.as_ref()?.size(dir)?;
        let changed = self.changed.lock().ok()?.iter().any(|(p, at)| p.starts_with(dir) && *at >= walked);
        (!changed).then_some(size)
    }
}

/// Kernel and device trees: endless, or sizes that mean nothing.
// ponytail: network mounts are measured like any folder, slowly; skip by file system type
// if that bites.
fn not_on_disk(path: &Path) -> bool {
    cfg!(unix) && ["/proc", "/sys", "/dev", "/run"].iter().any(|p| path.starts_with(p))
}

/// Symlinks are counted, not followed; unreadable parts are skipped.
pub(crate) fn walk(path: &Path, stop: &AtomicBool) -> Option<Size> {
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    let Ok(meta) = fs::symlink_metadata(path) else { return Some((0, 0)) };
    if !meta.is_dir() {
        return Some((meta.len(), 1));
    }
    let Ok(rd) = fs::read_dir(path) else { return Some((0, 0)) };
    rd.flatten()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|de| match de.file_type() {
            Ok(t) if t.is_dir() => walk(&de.path(), stop),
            _ => Some((de.metadata().map_or(0, |m| m.len()), 1)),
        })
        .try_reduce(|| (0, 0), |a, b| Some((a.0 + b.0, a.1 + b.1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_measured_remembered_forgotten_and_stopped() {
        let d = std::env::temp_dir().join(format!("coxswain-sizes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("a/b")).unwrap();
        fs::write(d.join("a/one"), [0u8; 100]).unwrap();
        fs::write(d.join("a/b/two"), [0u8; 50]).unwrap();
        let (sizer, go, stop) = (Sizer::default(), AtomicBool::new(false), AtomicBool::new(true));

        assert_eq!(sizer.measure(&d.join("a"), &stop), None, "stopped before it began");
        assert_eq!(sizer.known(&d.join("a")), None);
        assert_eq!(sizer.measure(&d.join("a"), &go), Some((150, 2)));
        assert_eq!(sizer.measure(&d.join("a"), &go), Some(crate::fs::dir_size(&d.join("a"))), "as the one-off measure");

        fs::write(d.join("a/b/three"), [0u8; 7]).unwrap();
        assert_eq!(sizer.measure(&d.join("a"), &stop), Some((150, 2)), "remembered, even when stopped");
        sizer.forget(&d.join("a/b/three"));
        assert_eq!(sizer.measure(&d.join("a"), &go), Some((157, 3)));

        assert_eq!(sizer.measure(Path::new("/proc"), &go).filter(|_| cfg!(unix)), None);
        fs::remove_dir_all(d).unwrap();
    }
}

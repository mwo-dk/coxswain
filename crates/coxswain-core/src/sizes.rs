//! Folder sizes in the background, for both apps. The helper's store knows the home folder's
//! sizes, so those come at once. Other folders are measured: two threads so measuring never
//! takes the machine, a stop flag for when the folder is left, and a short memory so going back
//! to a folder shows its sizes at once.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .thread_name(|i| format!("coxswain-sizes-{i}"))
            .build()
            .expect("a thread pool");
        Sizer {
            pool,
            known: Mutex::default(),
            index,
            changed: Mutex::default(),
        }
    }

    /// The size measured a short while ago, if any.
    pub fn known(&self, path: &Path) -> Option<Size> {
        let (size, at) = *self.known.lock().ok()?.get(path)?;
        (at.elapsed() < FRESH).then_some(size)
    }

    /// The size of `path` when it is cheap: measured a short while ago, or in the store.
    /// Never a walk; the store's answer is a round trip to the helper, so not on a UI thread.
    pub fn known_only(&self, path: &Path) -> Option<Size> {
        if let Some(size) = self.known(path) {
            return Some(size);
        }
        if not_on_disk(path) {
            return None;
        }
        self.stored(path)
    }

    /// The size of `path`, remembered or measured now. `None` when `stop` was set meanwhile,
    /// and for folders that are not on a disk (`/proc`).
    pub fn measure(&self, path: &Path, stop: &AtomicBool) -> Option<Size> {
        if let Some(size) = self.known_only(path) {
            return Some(size);
        }
        if not_on_disk(path) {
            return None;
        }
        let size = self.pool.install(|| walk(path, stop))?;
        self.known
            .lock()
            .ok()?
            .insert(path.to_path_buf(), (size, Instant::now()));
        Some(size)
    }

    /// Something at `changed` was added, removed or written: forget every folder it is in.
    pub fn forget(&self, changed: &Path) {
        if let Ok(mut known) = self.known.lock() {
            known.retain(|folder, _| !changed.starts_with(folder));
        }
        if let Ok(mut all) = self.changed.lock() {
            let now = crate::fs::now();
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
        let changed = self
            .changed
            .lock()
            .ok()?
            .iter()
            .any(|(p, at)| p.starts_with(dir) && *at >= walked);
        (!changed).then_some(size)
    }
}

/// Kernel and device trees: endless, or sizes that mean nothing.
// ponytail: network mounts are measured like any folder, slowly; skip by file system type
// if that bites.
pub(crate) fn not_on_disk(path: &Path) -> bool {
    cfg!(unix)
        && ["/proc", "/sys", "/dev", "/run"]
            .iter()
            .any(|p| path.starts_with(p))
}

/// The shared threads with room for deep folders: the name index's scan and the size walks
/// take a frame of their thread's stack per folder level, and rayon's 2 MiB overflowed at
/// about 1,900 nested folders. Called first thing by each app; later calls do nothing.
pub fn deep_stacks() {
    let _ = rayon::ThreadPoolBuilder::new().stack_size(64 << 20).build_global();
}

/// Symlinks are counted, not followed; unreadable parts, and other apps' data on a Mac below
/// `path` (`crate::fs::protected`), are skipped.
pub(crate) fn walk(path: &Path, stop: &AtomicBool) -> Option<Size> {
    walk_at(path, stop, 0)
}

/// Folders this deep and below are walked by one thread, in a loop: the parallel walk takes a
/// frame of its thread's stack per level, and thousands of nested folders would overflow it.
const DEEP: usize = 32;

fn walk_at(path: &Path, stop: &AtomicBool, depth: usize) -> Option<Size> {
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Some((0, 0));
    };
    if !meta.is_dir() {
        return Some((meta.len(), 1));
    }
    if depth >= DEEP {
        return walk_flat(path, stop);
    }
    let Ok(rd) = crate::fs::read_dir(path) else {
        return Some((0, 0));
    };
    rd.flatten()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|de| match de.file_type() {
            Ok(t) if t.is_dir() && crate::fs::protected(&de.path()) => Some((0, 0)),
            Ok(t) if t.is_dir() => walk_at(&de.path(), stop, depth + 1),
            _ => Some((de.metadata().map_or(0, |m| m.len()), 1)),
        })
        .try_reduce(|| (0, 0), |a, b| Some((a.0 + b.0, a.1 + b.1)))
}

/// The folder `path` and everything below it, with a list of folders still to read rather
/// than a frame per level.
fn walk_flat(path: &Path, stop: &AtomicBool) -> Option<Size> {
    let (mut total, mut todo) = ((0, 0), vec![path.to_path_buf()]);
    while let Some(dir) = todo.pop() {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        for de in crate::fs::read_dir(&dir).into_iter().flatten().flatten() {
            match de.file_type() {
                Ok(t) if t.is_dir() && crate::fs::protected(&de.path()) => {}
                Ok(t) if t.is_dir() => todo.push(de.path()),
                _ => total = (total.0 + de.metadata().map_or(0, |m| m.len()), total.1 + 1),
            }
        }
    }
    Some(total)
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
        let (sizer, go, stop) = (
            Sizer::default(),
            AtomicBool::new(false),
            AtomicBool::new(true),
        );

        assert_eq!(
            sizer.measure(&d.join("a"), &stop),
            None,
            "stopped before it began"
        );
        assert_eq!(sizer.known(&d.join("a")), None);
        assert_eq!(sizer.measure(&d.join("a"), &go), Some((150, 2)));
        assert_eq!(
            crate::fs::dir_size(&d.join("a")),
            (150, 2),
            "the one-off measure"
        );

        fs::write(d.join("a/b/three"), [0u8; 7]).unwrap();
        assert_eq!(
            sizer.measure(&d.join("a"), &stop),
            Some((150, 2)),
            "remembered, even when stopped"
        );
        sizer.forget(&d.join("a/b/three"));
        assert_eq!(sizer.measure(&d.join("a"), &go), Some((157, 3)));

        assert_eq!(
            sizer
                .measure(Path::new("/proc"), &go)
                .filter(|_| cfg!(unix)),
            None
        );
        fs::remove_dir_all(d).unwrap();
    }

    /// Cheap or nothing: a size measured a short while ago, else `None`, never a walk. The
    /// store's answer is tested with a helper in `helper::tests`.
    #[test]
    fn known_only_never_walks() {
        let d = std::env::temp_dir().join(format!("coxswain-known-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("a")).unwrap();
        fs::write(d.join("a/one"), [0u8; 10]).unwrap();
        let sizer = Sizer::default();
        assert_eq!(sizer.known_only(&d.join("a")), None, "neither: not walked");
        assert_eq!(sizer.known(&d.join("a")), None, "and nothing remembered");
        sizer.measure(&d.join("a"), &AtomicBool::new(false));
        assert_eq!(sizer.known_only(&d.join("a")), Some((10, 1)), "remembered");
        sizer.forget(&d.join("a/one"));
        assert_eq!(
            sizer.known_only(&d.join("a")),
            None,
            "forgotten, not walked again"
        );
        fs::remove_dir_all(d).unwrap();
    }
}

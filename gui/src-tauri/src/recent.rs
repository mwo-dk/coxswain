//! The last two files read and parsed, by path and modification time: the one in view and the
//! one it was compared with. Selecting, switching between the preview and its window, and
//! comparing two versions never read a file again.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const KEPT: usize = 2;

/// A file's path and modification time, and what was read of it.
type Kept<T> = (PathBuf, Option<SystemTime>, Arc<T>);

/// Most recent first.
pub struct Recent<T>(Mutex<Vec<Kept<T>>>);

impl<T> Recent<T> {
    pub const fn new() -> Self {
        Recent(Mutex::new(Vec::new()))
    }

    /// What `path` holds: kept, when the file has not changed and `fits` takes it, else `load`ed.
    pub fn get(&self, path: &Path, fits: impl Fn(&T) -> bool, load: impl FnOnce() -> Result<T, String>) -> Result<Arc<T>, String> {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        {
            let mut last = self.0.lock().unwrap();
            if let Some(at) = last.iter().position(|(p, m, v)| p == path && *m == modified && fits(v)) {
                let hit = last.remove(at);
                let v = hit.2.clone();
                last.insert(0, hit);
                return Ok(v);
            }
        }
        // Read outside the lock: a big file must not hold up the other view.
        let v = Arc::new(load()?);
        let mut last = self.0.lock().unwrap();
        last.retain(|(p, ..)| p != path);
        last.insert(0, (path.to_path_buf(), modified, v.clone()));
        last.truncate(KEPT);
        Ok(v)
    }
}

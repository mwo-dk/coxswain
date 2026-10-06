//! Folders watched with kqueue on the BSDs, one descriptor per folder and nothing more. notify's
//! kqueue backend opens a descriptor for every entry that appears in a watched folder, and for a
//! new folder every file below it, so a copy into a watched folder could use up the process's
//! open files (OpenBSD allows 512); it also hands the kernel every watch again on each one
//! added. Here a folder fires when an entry in it is added, removed or renamed; a watch reaches
//! the kernel at once, and many added through `paths_mut` reach it together.

use kqueue::{EventData, EventFilter, FilterFlag, Ident, Vnode};
use notify::{EventHandler, RecursiveMode, WatcherKind};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct Shared {
    kq: kqueue::Watcher,
}

pub struct KqueueWatcher {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
}

const FLAGS: FilterFlag = FilterFlag::NOTE_WRITE.union(FilterFlag::NOTE_DELETE).union(FilterFlag::NOTE_RENAME).union(FilterFlag::NOTE_REVOKE);

impl KqueueWatcher {
    fn wait(shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>, mut handler: Box<dyn EventHandler>) {
        let fd = shared.lock().map(|s| s.kq.as_raw_fd()).unwrap_or(-1);
        while !stop.load(Ordering::Relaxed) {
            // Wait outside the lock, so watches can be added meanwhile.
            let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
            // SAFETY: one pollfd, owned here.
            unsafe { libc::poll(&mut p, 1, 500) };
            let Ok(mut s) = shared.lock() else { break };
            let mut fired = vec![];
            while let Some(ev) = s.kq.poll(None) {
                if let (Ident::Filename(_, name), EventData::Vnode(v)) = (ev.ident, ev.data) {
                    fired.push((std::path::PathBuf::from(name), matches!(v, Vnode::Delete | Vnode::Rename | Vnode::Revoke)));
                }
            }
            for (dir, gone) in &fired {
                if *gone {
                    let _ = s.kq.remove_filename(dir, EventFilter::EVFILT_VNODE);
                }
            }
            drop(s);
            for (dir, gone) in fired {
                handler.handle_event(Ok(crate::folder_event(&dir, gone)));
            }
        }
    }
}

fn add(s: &mut Shared, path: &Path) -> notify::Result<()> {
    s.kq.add_filename(path, EventFilter::EVFILT_VNODE, FLAGS).map_err(|e| notify::Error::io(e).add_path(path.to_path_buf()))
}

impl notify::Watcher for KqueueWatcher {
    fn new<F: EventHandler>(handler: F, _config: notify::Config) -> notify::Result<Self> {
        let kq = kqueue::Watcher::new().map_err(notify::Error::io)?;
        let shared = Arc::new(Mutex::new(Shared { kq }));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, t) = (shared.clone(), stop.clone());
        std::thread::Builder::new().name("kqueue".into()).spawn(move || Self::wait(s, t, Box::new(handler))).map_err(notify::Error::io)?;
        Ok(KqueueWatcher { shared, stop })
    }

    /// Watches the folder `path` itself, never what is below it, whatever `_mode` says.
    fn watch(&mut self, path: &Path, _mode: RecursiveMode) -> notify::Result<()> {
        let mut s = self.shared.lock().map_err(|_| notify::Error::generic("poisoned"))?;
        add(&mut s, path)?;
        s.kq.watch().map_err(notify::Error::io)
    }

    /// Many folders, handed to the kernel in one call at `commit` (each `watch` hands it all).
    fn paths_mut<'me>(&'me mut self) -> Box<dyn notify::PathsMut + 'me> {
        struct Batch<'a>(Option<std::sync::MutexGuard<'a, Shared>>);
        impl notify::PathsMut for Batch<'_> {
            fn add(&mut self, path: &Path, _mode: RecursiveMode) -> notify::Result<()> {
                self.0.as_mut().map_or(Ok(()), |s| add(s, path))
            }
            fn remove(&mut self, path: &Path) -> notify::Result<()> {
                self.0.as_mut().map_or(Ok(()), |s| s.kq.remove_filename(path, EventFilter::EVFILT_VNODE).map_err(|_| notify::Error::watch_not_found()))
            }
            fn commit(mut self: Box<Self>) -> notify::Result<()> {
                self.0.take().map_or(Ok(()), |mut s| s.kq.watch().map_err(notify::Error::io))
            }
        }
        Box::new(Batch(self.shared.lock().ok()))
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        let mut s = self.shared.lock().map_err(|_| notify::Error::generic("poisoned"))?;
        s.kq.remove_filename(path, EventFilter::EVFILT_VNODE).map_err(|_| notify::Error::watch_not_found())
    }

    fn kind() -> WatcherKind {
        WatcherKind::Kqueue
    }
}

impl Drop for KqueueWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::Watcher;
    use std::time::Duration;

    #[test]
    fn kqueue_sees_a_folder_change_and_opens_nothing_below_it() {
        let dir = std::env::temp_dir().join(format!("coxswain-kq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut w = KqueueWatcher::new(tx, notify::Config::default()).unwrap();
        w.watch(&dir, RecursiveMode::NonRecursive).unwrap();
        let open = || std::fs::read_dir("/dev/fd").map(|d| d.count()).unwrap_or(0);
        let before = open();
        for i in 0..2 {
            // At once: a change right after `watch` is seen.
            std::fs::create_dir_all(dir.join(format!("tree{i}/a/b"))).unwrap();
            for f in 0..50 {
                std::fs::write(dir.join(format!("tree{i}/a/b/{f}")), "x").unwrap();
            }
            let e = rx.recv_timeout(Duration::from_secs(5)).expect("an event, every time").unwrap();
            assert!(e.paths.contains(&dir), "{e:?}");
            while rx.recv_timeout(Duration::from_millis(700)).is_ok() {}
        }
        assert!(open() <= before + 2, "no descriptor per new entry: {before} then {}", open());
        w.unwatch(&dir).unwrap();
        std::fs::write(dir.join("after"), "x").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(1200)).is_err(), "unwatched: quiet");
        let mut b = w.paths_mut();
        b.add(&dir.join("tree0"), RecursiveMode::NonRecursive).unwrap();
        b.add(&dir.join("tree1"), RecursiveMode::NonRecursive).unwrap();
        b.commit().unwrap();
        std::fs::write(dir.join("tree1/new"), "x").unwrap();
        let e = rx.recv_timeout(Duration::from_secs(5)).expect("a folder added in a batch").unwrap();
        assert!(e.paths.contains(&dir.join("tree1")), "{e:?}");
        drop(w);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

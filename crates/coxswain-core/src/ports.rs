//! Folders watched with illumos event ports: notify has no backend for them and would poll every
//! file under every watched folder. A folder is associated with the port and fires once when its
//! modification time changes (an entry added, removed or renamed), then is associated again. An
//! association holds no file descriptor, so many folders cost little. Folders only: a watch is
//! never recursive, which is all Coxswain asks of it (`index` watches folder by folder there).

use notify::{EventHandler, RecursiveMode, WatcherKind};
use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// `file_obj_t` from <sys/port.h>: the times the kernel compares, and the path.
#[repr(C)]
struct FileObj {
    atime: libc::timespec,
    mtime: libc::timespec,
    ctime: libc::timespec,
    pad: [libc::uintptr_t; 3],
    name: *mut libc::c_char,
}

const FILE_MODIFIED: libc::c_int = 0x2;
const FILE_NOFOLLOW: libc::c_int = 0x1000_0000;

/// One watched folder. Boxed, so the kernel's pointer to it stays valid while it is associated.
struct Assoc {
    obj: FileObj,
    _name: CString,
    path: PathBuf,
}

// SAFETY: `obj.name` points into `_name`, owned by the same value.
unsafe impl Send for Assoc {}

#[derive(Default)]
struct Watched {
    by_path: HashMap<PathBuf, Box<Assoc>>,
}

pub struct PortWatcher {
    port: libc::c_int,
    watched: Arc<Mutex<Watched>>,
}

/// Associate `a` with the port, with its folder's times as they are now: a change after this
/// fires. False when the folder is gone or cannot be watched.
fn associate(port: libc::c_int, a: &mut Assoc) -> bool {
    let Ok(md) = std::fs::metadata(&a.path) else { return false };
    use std::os::unix::fs::MetadataExt;
    let ts = |s: i64, ns: i64| libc::timespec { tv_sec: s as libc::time_t, tv_nsec: ns as libc::c_long };
    (a.obj.atime, a.obj.mtime, a.obj.ctime) = (ts(md.atime(), md.atime_nsec()), ts(md.mtime(), md.mtime_nsec()), ts(md.ctime(), md.ctime_nsec()));
    let obj = &raw mut a.obj as libc::uintptr_t;
    // SAFETY: `obj` lives in a Box kept in `Watched` until it is dissociated.
    unsafe { libc::port_associate(port, libc::PORT_SOURCE_FILE, obj, FILE_MODIFIED | FILE_NOFOLLOW, std::ptr::null_mut()) == 0 }
}

impl PortWatcher {
    fn wait(port: libc::c_int, watched: Arc<Mutex<Watched>>, mut handler: Box<dyn EventHandler>) {
        loop {
            // SAFETY: port_get fills the zeroed event it is given.
            let mut ev: libc::port_event = unsafe { std::mem::zeroed() };
            if unsafe { libc::port_get(port, &mut ev, std::ptr::null_mut()) } != 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            if ev.portev_source as libc::c_int == libc::PORT_SOURCE_USER {
                break; // dropped
            }
            let Ok(mut w) = watched.lock() else { break };
            // The folder, if still watched: an event may come for one unwatched meanwhile.
            let Some(path) = w.by_path.values().find(|a| &raw const a.obj as libc::uintptr_t == ev.portev_object).map(|a| a.path.clone()) else { continue };
            let again = ev.portev_events & FILE_MODIFIED != 0 && w.by_path.get_mut(&path).is_some_and(|a| associate(port, a));
            if !again {
                w.by_path.remove(&path);
            }
            drop(w);
            handler.handle_event(Ok(crate::folder_event(&path, !again)));
        }
        // SAFETY: the port is this thread's to close once the watcher is dropped.
        unsafe { libc::close(port) };
    }
}

impl notify::Watcher for PortWatcher {
    fn new<F: EventHandler>(handler: F, _config: notify::Config) -> notify::Result<Self> {
        // SAFETY: plain system call.
        let port = unsafe { libc::port_create() };
        if port < 0 {
            return Err(notify::Error::io(std::io::Error::last_os_error()));
        }
        let watched = Arc::new(Mutex::new(Watched::default()));
        let w = watched.clone();
        std::thread::Builder::new().name("event-ports".into()).spawn(move || Self::wait(port, w, Box::new(handler))).map_err(notify::Error::io)?;
        Ok(PortWatcher { port, watched })
    }

    fn watch(&mut self, path: &Path, _mode: RecursiveMode) -> notify::Result<()> {
        let mut w = self.watched.lock().map_err(|_| notify::Error::generic("poisoned"))?;
        if w.by_path.contains_key(path) {
            return Ok(());
        }
        let name = CString::new(path.as_os_str().as_bytes()).map_err(|_| notify::Error::path_not_found())?;
        let zero = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        let mut a = Box::new(Assoc { obj: FileObj { atime: zero, mtime: zero, ctime: zero, pad: [0; 3], name: name.as_ptr().cast_mut() }, _name: name, path: path.to_path_buf() });
        if !associate(self.port, &mut a) {
            return Err(notify::Error::io(std::io::Error::last_os_error()).add_path(path.to_path_buf()));
        }
        w.by_path.insert(path.to_path_buf(), a);
        Ok(())
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        let mut w = self.watched.lock().map_err(|_| notify::Error::generic("poisoned"))?;
        let a = w.by_path.remove(path).ok_or_else(notify::Error::watch_not_found)?;
        // SAFETY: the object was associated with this port; dissociating one that already fired is harmless.
        unsafe { libc::port_dissociate(self.port, libc::PORT_SOURCE_FILE, &raw const a.obj as libc::uintptr_t) };
        Ok(())
    }

    fn kind() -> WatcherKind {
        WatcherKind::PollWatcher
    }
}

impl Drop for PortWatcher {
    fn drop(&mut self) {
        // SAFETY: wakes the waiting thread, which closes the port.
        unsafe { libc::port_send(self.port, 0, std::ptr::null_mut()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::Watcher;

    #[test]
    fn ports_see_a_file_added_to_a_watched_folder() {
        let dir = std::env::temp_dir().join(format!("coxswain-ports-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut w = PortWatcher::new(tx, notify::Config::default()).unwrap();
        w.watch(&dir, RecursiveMode::NonRecursive).unwrap();
        for i in 0..2 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            std::fs::write(dir.join(format!("new{i}")), "x").unwrap();
            let e = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("an event, every time").unwrap();
            assert!(e.paths.contains(&dir), "{e:?}");
            assert!(e.paths.iter().any(|p| p.parent() == Some(dir.as_path())), "{e:?}");
        }
        w.unwatch(&dir).unwrap();
        std::fs::write(dir.join("after"), "x").unwrap();
        assert!(rx.recv_timeout(std::time::Duration::from_millis(500)).is_err(), "unwatched: quiet");
        drop(w);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

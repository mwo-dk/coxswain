//! A walk pinned to what it saw: the one way find and replace (`replace`) and permissions and
//! dates (`attrs`) reach the files they change, plan and run alike.
//!
//! The plan walks the tree through folder handles: each root's folder is opened once (its real
//! path, without following a link at its end), every folder below with `openat(…, O_DIRECTORY |
//! O_NOFOLLOW)` in the folder above, listed through its handle, and every entry looked at with
//! `fstatat(AT_SYMLINK_NOFOLLOW)`: after the root nothing is looked up by path. It keeps a `Set`:
//! every entry it will touch and every folder on the way to one, each as `(device, inode, ctime
//! with nanoseconds)` and the path it was met at.
//!
//! The run walks the same way, but goes only into folders in that set and hands on only entries
//! in it: an entry or folder whose identity, ctime or path is not exactly what the plan saw
//! (swapped, renamed in, renamed away, written meanwhile) is left as it is. The path counts on its
//! own: where a file system's clock ticks coarsely (OpenBSD's every 10 ms) an entry renamed in the
//! same tick keeps its ctime, but not its name. Nothing compares a ctime with the local clock. Files are opened in their held folder,
//! never through a link, never blocking on a pipe, and checked to be that very file and a file
//! or folder before a byte is read. A file is written anew only through its folder's handle: a
//! file beside it (0600, its group, then its mode) renamed over it in that folder (`renameat`).
//!
//! On Windows only the marked entries themselves are met, never what is inside a folder
//! (`RECURSES`), each checked by volume, file index and change time through a handle of its own
//! that never follows a link: good for a preview and for changing those entries through that
//! handle, not for writing files anew (`WRITES`).

use crate::t;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// How deep a walk goes at most: one folder handle per level is held open, and never more than
/// a quarter of what this process may open (`deepest`).
pub const DEEPEST: usize = 256;

/// How deep a walk goes here: `DEEPEST`, or less where few files may be open (macOS allows 256).
pub fn deepest() -> usize {
    #[cfg(unix)]
    {
        // SAFETY: zeroes are a valid rlimit, which getrlimit fills in.
        let mut r: libc::rlimit = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut r) } == 0 {
            return DEEPEST.min((r.rlim_cur / 4).max(16) as usize);
        }
    }
    DEEPEST
}
/// The lines of what was left out kept at most; the rest are counted.
pub const SAID: usize = 1_000;
/// Whether files can be written anew through their folder's handle: not on Windows yet.
pub const WRITES: bool = cfg!(unix);
/// Whether a walk goes into folders: not on Windows yet, where a folder could only be listed by
/// its path (a folder swapped for a junction would be followed). There only the roots are met.
pub const RECURSES: bool = cfg!(unix);

/// Which entry, as it was: device and inode (on Windows volume serial and file index), and when
/// its entry last changed (ctime; on Windows the change time), to the nanosecond.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Id {
    pub dev: u64,
    pub ino: u64,
    pub ctime: (i64, u32),
}

impl Id {
    /// The same file, whatever happened to it since.
    pub fn same_file(&self, other: &Id) -> bool {
        (self.dev, self.ino) == (other.dev, other.ino)
    }
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    File,
    Dir,
    Link,
    /// A pipe, a device, a socket: never opened.
    Other,
}

/// An entry as its folder lists it, never through a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stat {
    pub id: Id,
    pub kind: Kind,
    /// Unix permission bits (0o7777); on Windows its attributes (read-only 1, hidden 2).
    pub mode: u32,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    /// Seconds since 1970, and nanoseconds.
    pub modified: (i64, u32),
    pub accessed: (i64, u32),
    /// The BSDs' and macOS's file flags (`chflags`: nodump, hidden, append-only …); 0 elsewhere.
    pub flags: u32,
}

/// Where a change was made, for undo: the folder (device and inode) and the file as written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub dir: (u64, u64),
    pub file: Id,
    /// For a file written anew, its content (BLAKE3): undo then asks for the same file (device
    /// and inode) with that very content, not for its ctime, which some file systems move by
    /// themselves a little after a rename (FreeBSD's UFS with soft updates).
    #[serde(default)]
    pub sum: Option<String>,
}

/// The entries a plan will touch, and the folders on the way to them.
#[derive(Clone, Debug, Default)]
pub struct Set {
    touch: HashMap<Id, PathBuf>,
    route: HashMap<Id, PathBuf>,
}

impl Set {
    /// `node` is to be touched: it, and every folder the walk went through to reach it.
    pub fn add(&mut self, node: &Node) {
        self.touch.insert(node.stat.id, node.path.clone());
        let mut up = node.chain.as_deref();
        while let Some(l) = up {
            if self.route.insert(l.id, l.path.clone()).is_some() {
                break;
            }
            up = l.up.as_deref();
        }
    }

    /// `id` is already to be touched (a file reached twice).
    pub fn has(&self, id: &Id) -> bool {
        self.touch.contains_key(id)
    }

    /// Whether `id` met at `path` is in `map` as the plan saw it.
    fn met(map: &HashMap<Id, PathBuf>, id: &Id, path: &Path) -> bool {
        map.get(id).is_some_and(|p| p == path)
    }

    /// How many entries are to be touched.
    pub fn len(&self) -> usize {
        self.touch.len()
    }

    pub fn is_empty(&self) -> bool {
        self.touch.is_empty()
    }
}

/// The folders a node was reached through, nearest first.
struct Chain {
    id: Id,
    path: PathBuf,
    up: Option<Arc<Chain>>,
}

/// How to walk.
#[derive(Clone, Copy, Default)]
pub struct Walk<'a> {
    /// Into folders, all the way down (else only the roots).
    pub recursive: bool,
    /// Folders by these names are neither handed on nor gone into (`.git`).
    pub skip: &'a [&'a str],
    /// The run: only what this plan saw, exactly.
    pub only: Option<&'a Set>,
}

/// What a walk could not look at (`path: why`, at most `SAID`), and how many more.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Said {
    pub lines: Vec<(PathBuf, String)>,
    pub more: usize,
}

impl Said {
    pub fn say(&mut self, path: &Path, why: String) {
        if self.lines.len() < SAID {
            self.lines.push((path.to_path_buf(), why));
        } else {
            self.more += 1;
        }
    }
}

/// The error for an entry that is not what the plan saw.
pub fn changed() -> std::io::Error {
    std::io::Error::other(t!("pinned.changed"))
}

fn other(why: String) -> std::io::Error {
    std::io::Error::other(why)
}

/// An entry the walk reached.
pub struct Node {
    pub path: PathBuf,
    pub stat: Stat,
    pub depth: usize,
    /// Where it is: its folder, held open, and its name there (on Windows its path).
    pub place: Place,
    chain: Option<Arc<Chain>>,
}

impl Node {
    /// Where to change it and what to compare with afterwards, for undo.
    pub fn pin(&self) -> Pin {
        Pin { dir: self.place.dir_id(), file: self.stat.id, sum: None }
    }

    /// The entry, opened in its folder (never through a link, never waiting on a pipe) and
    /// checked to be this very file or folder, unchanged since it was listed.
    pub fn open(&self) -> std::io::Result<(std::fs::File, Stat)> {
        self.place.open(self.stat.id)
    }
}

/// A number of its own for each plan, so a run takes only the plan it was asked for.
pub fn token() -> u64 {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// `seconds, nanoseconds` since 1970 of a time, before 1970 negative.
pub fn secs(t: std::time::SystemTime) -> (i64, u32) {
    match t.duration_since(std::time::SystemTime::UNIX_EPOCH) {
        Ok(d) => (d.as_secs() as i64, d.subsec_nanos()),
        Err(e) => (-(e.duration().as_secs() as i64), 0),
    }
}

/// Every entry under `roots`, in name order within each folder, each handed to `each` (which
/// returns false to stop); what could not be looked at goes to `said`. With `w.only` only the
/// entries in that set are handed on, and only folders in it are gone into: what is not as the
/// plan saw it is passed over without a word. Returns how many entries were handed on, so the
/// caller can tell how many it missed.
pub fn walk(roots: &[PathBuf], w: &Walk, stop: &AtomicBool, said: &mut Said, mut each: impl FnMut(&Node) -> bool) -> usize {
    let (mut handed, deepest) = (0, deepest());
    // A root's folder is opened only when its turn comes: one handle at a time, however many.
    let mut todo: Vec<Todo> = roots.iter().rev().map(|r| Todo { path: r.clone(), place: None, depth: 0, chain: None }).collect();
    while let Some(t) = todo.pop() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let place = match t.place.map_or_else(|| imp::root(&t.path), Ok) {
            Ok(p) => p,
            Err(e) => {
                if w.only.is_none() {
                    said.say(&t.path, e.to_string());
                }
                continue;
            }
        };
        let stat = match place.stat() {
            Ok(s) => s,
            // Gone since it was listed: nothing to say for the run, which counts what it missed.
            Err(e) => {
                if w.only.is_none() {
                    said.say(&t.path, e.to_string());
                }
                continue;
            }
        };
        let skipped = stat.kind == Kind::Dir && t.path.file_name().is_some_and(|n| w.skip.iter().any(|s| n == *s));
        if skipped {
            continue;
        }
        let (touch, route) = match w.only {
            None => (true, true),
            Some(set) => (Set::met(&set.touch, &stat.id, &t.path), Set::met(&set.route, &stat.id, &t.path)),
        };
        let node = Node { path: t.path, stat, depth: t.depth, place, chain: t.chain };
        if touch {
            handed += 1;
            if !each(&node) {
                break;
            }
        }
        if !RECURSES && w.recursive && w.only.is_none() && stat.kind == Kind::Dir {
            said.say(&node.path, t!("pinned.no_folders"));
        }
        if !(RECURSES && w.recursive && route && stat.kind == Kind::Dir) {
            continue;
        }
        // Not onto another disk or mount (as `find -xdev`): a dead network mount would hang it.
        if node.chain.as_ref().is_some_and(|up| up.id.dev != stat.id.dev) {
            if w.only.is_none() {
                said.say(&node.path, t!("pinned.other_disk"));
            }
            continue;
        }
        if node.depth >= deepest {
            said.say(&node.path, t!("pinned.too_deep", "n" => deepest));
            continue;
        }
        match imp::list(&node) {
            Ok(children) => {
                let chain = Some(Arc::new(Chain { id: stat.id, path: node.path.clone(), up: node.chain.clone() }));
                todo.extend(children.into_iter().rev().map(|(path, place)| Todo { path, place: Some(place), depth: node.depth + 1, chain: chain.clone() }));
            }
            Err(e) => said.say(&node.path, e.to_string()),
        }
    }
    handed
}

/// An entry still to be looked at.
struct Todo {
    path: PathBuf,
    /// None for a root, until its turn.
    place: Option<Place>,
    depth: usize,
    chain: Option<Arc<Chain>>,
}

pub use imp::{Place, stat_of};
/// Folders held open to make, move and remove entries in (`compare::sync`).
#[cfg(unix)]
pub mod dir;
#[cfg(unix)]
pub use imp::{extra_permissions, foreign_flags};

#[cfg(unix)]
mod imp {
    use super::*;
    use std::ffi::{CStr, CString, OsStr};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::os::unix::ffi::OsStrExt;

    /// A folder held open, and what it is.
    pub struct Held {
        fd: OwnedFd,
        id: (u64, u64),
    }

    /// An entry's folder, held open, and its name there.
    #[derive(Clone)]
    pub struct Place {
        dir: Arc<Held>,
        name: CString,
    }

    fn last() -> std::io::Error {
        std::io::Error::last_os_error()
    }

    #[allow(clippy::unnecessary_cast)] // the stat fields differ in type between systems
    pub(super) fn from_stat(st: &libc::stat) -> Stat {
        #[cfg(target_os = "netbsd")]
        let (mn, an, cn) = (st.st_mtimensec, st.st_atimensec, st.st_ctimensec);
        #[cfg(not(target_os = "netbsd"))]
        let (mn, an, cn) = (st.st_mtime_nsec, st.st_atime_nsec, st.st_ctime_nsec);
        let kind = match st.st_mode & libc::S_IFMT {
            libc::S_IFREG => Kind::File,
            libc::S_IFDIR => Kind::Dir,
            libc::S_IFLNK => Kind::Link,
            _ => Kind::Other,
        };
        Stat {
            id: Id { dev: st.st_dev as u64, ino: st.st_ino as u64, ctime: (st.st_ctime as i64, cn as u32) },
            kind,
            mode: st.st_mode as u32 & 0o7777,
            nlink: st.st_nlink as u64,
            uid: st.st_uid as u32,
            gid: st.st_gid as u32,
            size: st.st_size as u64,
            modified: (st.st_mtime as i64, mn as u32),
            accessed: (st.st_atime as i64, an as u32),
            #[cfg(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "macos", target_os = "dragonfly"))]
            flags: st.st_flags as u32,
            #[cfg(not(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "macos", target_os = "dragonfly")))]
            flags: 0,
        }
    }

    /// errno set to 0, so that a call which says nothing on failure (readdir) can be told apart.
    pub(super) fn clear_errno() {
        // SAFETY: each returns this thread's errno.
        unsafe {
            #[cfg(target_os = "linux")]
            let e = libc::__errno_location();
            #[cfg(target_os = "android")]
            let e = libc::__errno();
            #[cfg(any(target_os = "freebsd", target_os = "macos"))]
            let e = libc::__error();
            #[cfg(target_os = "dragonfly")]
            let e = libc::__errno_location();
            #[cfg(any(target_os = "openbsd", target_os = "netbsd"))]
            let e = libc::__errno();
            #[cfg(any(target_os = "illumos", target_os = "solaris"))]
            let e = libc::___errno();
            *e = 0;
        }
    }

    /// What the open `f` is.
    pub fn stat_of(f: &std::fs::File) -> std::io::Result<Stat> {
        fstat(f.as_raw_fd())
    }

    fn fstat(fd: RawFd) -> std::io::Result<Stat> {
        // SAFETY: zeroes are a valid stat, which fstat fills in for an open descriptor.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        if unsafe { libc::fstat(fd, &mut st) } != 0 {
            return Err(last());
        }
        Ok(from_stat(&st))
    }

    /// The folder at `path`, opened as itself: its real path (a link on the way resolved once,
    /// here), opened as a folder without following a link at its end.
    fn open_dir(path: &Path) -> std::io::Result<Held> {
        use std::os::unix::fs::OpenOptionsExt;
        let path = if path.as_os_str().is_empty() { Path::new(".") } else { path };
        let f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(std::fs::canonicalize(path)?)?;
        let fd = OwnedFd::from(f);
        let s = fstat(fd.as_raw_fd())?;
        Ok(Held { fd, id: (s.id.dev, s.id.ino) })
    }

    fn split(path: &Path) -> std::io::Result<(&Path, CString)> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
        };
        Ok((parent, CString::new(name.as_bytes()).map_err(std::io::Error::other)?))
    }

    pub(super) fn root(path: &Path) -> std::io::Result<Place> {
        let (parent, name) = split(path)?;
        Ok(Place { dir: Arc::new(open_dir(parent)?), name })
    }

    /// The names in the folder `node` (opened in its own folder, checked to be it), in order.
    pub(super) fn list(node: &Node) -> std::io::Result<Vec<(PathBuf, Place)>> {
        let p = &node.place;
        // SAFETY: an open folder and a NUL-terminated name; never a link, never blocking.
        let fd = unsafe { libc::openat(p.dir.fd.as_raw_fd(), p.name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if fd < 0 {
            let e = last();
            return Err(if e.raw_os_error() == Some(libc::ELOOP) { other(t!("pinned.link")) } else { e });
        }
        // SAFETY: just opened, owned here.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let s = fstat(fd.as_raw_fd())?;
        // The very folder that was looked at, not one swapped in since.
        if !s.id.same_file(&node.stat.id) {
            return Err(changed());
        }
        let mut names = vec![];
        // SAFETY: a duplicate of the folder's handle goes to fdopendir, which owns and closes it.
        unsafe {
            // Closed on exec: no program started meanwhile on another thread inherits it.
            let dup = libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0);
            if dup < 0 {
                return Err(last());
            }
            let d = libc::fdopendir(dup);
            if d.is_null() {
                let e = last();
                libc::close(dup);
                return Err(e);
            }
            loop {
                clear_errno();
                let e = libc::readdir(d);
                if e.is_null() {
                    // The end, or a failure: a listing cut short is said, not taken as all.
                    let err = last();
                    if err.raw_os_error().is_some_and(|n| n != 0) {
                        libc::closedir(d);
                        return Err(err);
                    }
                    break;
                }
                let n = CStr::from_ptr((*e).d_name.as_ptr());
                if n.to_bytes() != b"." && n.to_bytes() != b".." {
                    names.push(n.to_owned());
                }
            }
            libc::closedir(d);
        }
        names.sort();
        let held = Arc::new(Held { fd, id: (s.id.dev, s.id.ino) });
        Ok(names.into_iter().map(|n| (node.path.join(OsStr::from_bytes(n.as_bytes())), Place { dir: held.clone(), name: n })).collect())
    }

    impl Place {
        /// The folder `path` is in, opened again (for undo) and checked to be the folder `dir`.
        pub fn reopen(path: &Path, dir: (u64, u64)) -> std::io::Result<Place> {
            let (parent, name) = split(path)?;
            let held = open_dir(parent)?;
            if held.id != dir {
                return Err(changed());
            }
            Ok(Place { dir: Arc::new(held), name })
        }

        pub fn dir_id(&self) -> (u64, u64) {
            self.dir.id
        }

        /// The folder's handle and the name in it, for a change made through them.
        pub fn at(&self) -> (RawFd, &CStr) {
            (self.dir.fd.as_raw_fd(), &self.name)
        }

        /// The entry as it is now, never through a link.
        pub fn stat(&self) -> std::io::Result<Stat> {
            // SAFETY: zeroes are a valid stat, which fstatat fills in.
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            // SAFETY: an open folder, a NUL-terminated name, `st` ours to fill.
            if unsafe { libc::fstatat(self.dir.fd.as_raw_fd(), self.name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) } != 0 {
                return Err(last());
            }
            Ok(from_stat(&st))
        }

        /// The entry opened read-only in its folder, checked through the handle to be `want`
        /// exactly (same file, same ctime) and a file or folder. A link, pipe, device or socket
        /// is refused before it is opened, and opening never follows a link or waits.
        pub fn open(&self, want: Id) -> std::io::Result<(std::fs::File, Stat)> {
            let before = self.stat()?;
            refuse_kind(before.kind)?;
            if before.id != want {
                return Err(changed());
            }
            // SAFETY: an open folder and a NUL-terminated name.
            let fd = unsafe { libc::openat(self.dir.fd.as_raw_fd(), self.name.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC) };
            if fd < 0 {
                let e = last();
                return Err(match e.raw_os_error() {
                    Some(libc::ELOOP) => other(t!("pinned.link")),
                    Some(libc::EACCES | libc::EPERM) => other(t!("pinned.no_open")),
                    _ => e,
                });
            }
            // SAFETY: just opened, owned here.
            let f = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
            let s = fstat(f.as_raw_fd())?;
            refuse_kind(s.kind)?;
            if s.id != want {
                return Err(changed());
            }
            Ok((f, s))
        }

        /// As `open`: a handle read-only is enough to change bits and times here.
        pub fn open_to_change(&self, want: Id) -> std::io::Result<(std::fs::File, Stat)> {
            self.open(want)
        }

        /// The file written anew with `bytes`: a file beside it in the same held folder, made
        /// for this user alone (0600, never one that is there, never a link), given the old
        /// file's group and then its mode, filled, synced and renamed over
        /// it there. Only while the name is still `want` exactly (a regular file of this user's
        /// with no other hard links). What it is now, for undo.
        pub fn rewrite(&self, want: Id, bytes: &[u8]) -> std::io::Result<Id> {
            use std::io::Write;
            // First exactly `want`; right before the rename the same file still (its ctime may
            // have moved by itself meanwhile: UFS settling an earlier rename).
            let check = |exact: bool| -> std::io::Result<Stat> {
                let s = self.stat()?;
                refuse_kind(s.kind)?;
                if s.kind != Kind::File || if exact { s.id != want } else { !s.id.same_file(&want) } {
                    return Err(changed());
                }
                if s.nlink > 1 {
                    return Err(other(t!("pinned.hard_link")));
                }
                // Set-user or set-group ID, or file flags: what a file written anew would lose.
                if s.mode & 0o6000 != 0 {
                    return Err(other(t!("pinned.setid")));
                }
                if foreign_flags(&s) {
                    return Err(other(t!("pinned.extra")));
                }
                // SAFETY: geteuid has no preconditions.
                if s.uid != unsafe { libc::geteuid() } {
                    return Err(other(t!("pinned.not_yours")));
                }
                Ok(s)
            };
            let old = check(true)?;
            let dir = self.dir.fd.as_raw_fd();
            // A name of its own (process, time), short whatever the file's name.
            let stem: String = String::from_utf8_lossy(self.name.as_bytes()).chars().take(40).collect();
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
            let tmp = CString::new(format!(".{stem}.coxswain-{}-{nanos}.tmp", std::process::id())).map_err(std::io::Error::other)?;
            // SAFETY: an open folder and a NUL-terminated name; a new file only.
            let fd = unsafe { libc::openat(dir, tmp.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600 as libc::c_uint) };
            if fd < 0 {
                return Err(last());
            }
            // SAFETY: just opened, owned here.
            let mut f = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
            let done = (|| {
                // Its group as the old file has it (when it is not already: changing it needs a
                // right a sandbox may lack), then its mode.
                // SAFETY: the file just made.
                if fstat(f.as_raw_fd())?.gid != old.gid && unsafe { libc::fchown(f.as_raw_fd(), u32::MAX as libc::uid_t, old.gid as libc::gid_t) } != 0 {
                    return Err(last());
                }
                // SAFETY: as above.
                if unsafe { libc::fchmod(f.as_raw_fd(), old.mode as libc::mode_t) } != 0 {
                    return Err(last());
                }
                // A folder's inherited ACL or attributes would open it to others the old file was
                // closed to: then it is not written at all.
                if extra_permissions(&f) {
                    return Err(other(t!("pinned.extra")));
                }
                f.write_all(bytes)?;
                // The flags it had that a user may set (nodump, hidden; ZFS sets archive by itself).
                #[cfg(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "macos", target_os = "dragonfly"))]
                // SAFETY: the file just made, and flags.
                if old.flags != 0 && unsafe { libc::fchflags(f.as_raw_fd(), old.flags as _) } != 0 {
                    return Err(last());
                }
                f.sync_all()?;
                // Still the file the plan saw, right before it is replaced.
                check(false)?;
                // SAFETY: both names in the one held folder.
                if unsafe { libc::renameat(dir, tmp.as_ptr(), dir, self.name.as_ptr()) } != 0 {
                    return Err(last());
                }
                Ok(())
            })();
            if let Err(e) = done {
                // SAFETY: as above.
                unsafe { libc::unlinkat(dir, tmp.as_ptr(), 0) };
                return Err(e);
            }
            // Written: what it is now, for undo. The file and its folder synced first, so a file
            // system that settles a rename later (FreeBSD's UFS with soft updates gives the file
            // a new ctime when the rename's link counts are written out) has done so; and never
            // an error here, which would cost the kept copy its undo. One not to be told is
            // recorded with no time, which undo refuses.
            let _ = f.sync_all();
            // SAFETY: the held folder.
            unsafe { libc::fsync(dir) };
            Ok(fstat(f.as_raw_fd()).map_or(Id { ctime: (-1, 0), ..want }, |s| s.id))
        }
    }

    /// File flags a file written anew may carry over: nodump, archive (ZFS sets it on every
    /// write), hidden, tracked (APFS). Any other (immutable, append-only, the system's own) and
    /// the file is left as it is.
    #[cfg(target_os = "freebsd")]
    const CARRIED: u32 = 0x1 | 0x800 | 0x8000;
    #[cfg(target_os = "macos")]
    const CARRIED: u32 = 0x1 | 0x40 | 0x8000;
    #[cfg(not(any(target_os = "freebsd", target_os = "macos")))]
    const CARRIED: u32 = 0x1;

    /// File flags a file written anew could not carry over (`CARRIED`).
    pub fn foreign_flags(s: &Stat) -> bool {
        s.flags & !CARRIED != 0
    }

    pub(super) fn refuse_kind(k: Kind) -> std::io::Result<()> {
        match k {
            Kind::File | Kind::Dir => Ok(()),
            Kind::Link => Err(other(t!("pinned.link"))),
            Kind::Other => Err(other(t!("pinned.special"))),
        }
    }

    /// The file has permissions beyond its bits (an ACL) or extended attributes: written anew it
    /// would lose them. It fails closed: an error, or a system where this cannot be told, counts
    /// as extra. SELinux's own label is no reason; a file system that cannot hold extended
    /// attributes at all has none to lose.
    pub fn extra_permissions(f: &std::fs::File) -> bool {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            // ACLs are the xattrs `system.posix_acl_*` here: listed with the rest.
            xattrs_listed(f)
        }
        #[cfg(target_os = "macos")]
        {
            unsafe extern "C" {
                fn acl_get_fd(fd: libc::c_int) -> *mut libc::c_void;
                fn acl_free(obj: *mut libc::c_void) -> libc::c_int;
            }
            // SAFETY: an open file; the ACL got is freed at once.
            let acl = unsafe { acl_get_fd(f.as_raw_fd()) };
            if !acl.is_null() {
                // SAFETY: as above.
                unsafe { acl_free(acl) };
                return true;
            }
            // No extended ACL is ENOENT; anything else is not knowing.
            last().raw_os_error() != Some(libc::ENOENT) || xattrs_listed(f)
        }
        #[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
        {
            unsafe extern "C" {
                fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
                fn acl_is_trivial_np(acl: *const libc::c_void, trivial: *mut libc::c_int) -> libc::c_int;
                fn acl_free(obj: *mut libc::c_void) -> libc::c_int;
            }
            // ACL_TYPE_NFS4 (ZFS), else ACL_TYPE_ACCESS (POSIX.1e, UFS): whichever it has.
            let mut acl = std::ptr::null_mut();
            for kind in [4, 2] {
                // SAFETY: an open file and an ACL type.
                acl = unsafe { acl_get_fd_np(f.as_raw_fd(), kind) };
                if !acl.is_null() {
                    break;
                }
            }
            if acl.is_null() {
                // No ACL support on this file system at all is no ACL; anything else is not knowing.
                if last().raw_os_error() != Some(libc::EOPNOTSUPP) {
                    return true;
                }
            } else {
                let mut trivial = 0;
                // SAFETY: the ACL got above, freed at once.
                let ok = unsafe { acl_is_trivial_np(acl, &mut trivial) } == 0;
                // SAFETY: as above.
                unsafe { acl_free(acl) };
                if !ok || trivial != 1 {
                    return true;
                }
            }
            // SAFETY: an open file; no buffer, only the length is asked for.
            let n = unsafe { libc::extattr_list_fd(f.as_raw_fd(), libc::EXTATTR_NAMESPACE_USER, std::ptr::null_mut(), 0) };
            n > 0 || n < 0 && last().raw_os_error() != Some(libc::EOPNOTSUPP)
        }
        // OpenBSD has neither ACLs nor extended attributes: nothing to lose.
        #[cfg(target_os = "openbsd")]
        {
            let _ = f;
            false
        }
        #[cfg(any(target_os = "illumos", target_os = "solaris"))]
        {
            #[link(name = "sec")]
            unsafe extern "C" {
                fn facl_get(fd: libc::c_int, flag: libc::c_int, aclp: *mut *mut libc::c_void) -> libc::c_int;
                fn acl_free(acl: *mut libc::c_void);
            }
            /// `facl_get` gives no ACL (a null one) when the file's is trivial: its mode bits alone.
            const ACL_NO_TRIVIAL: libc::c_int = 0x2;
            let mut acl = std::ptr::null_mut();
            // SAFETY: an open file and a place for the ACL got, freed at once.
            if unsafe { facl_get(f.as_raw_fd(), ACL_NO_TRIVIAL, &mut acl) } != 0 {
                return true;
            }
            if !acl.is_null() {
                // SAFETY: as above.
                unsafe { acl_free(acl) };
                return true;
            }
            // Extended attributes other than the system's own: 1 when there are, -1 not knowing.
            // SAFETY: an open file.
            unsafe { libc::fpathconf(f.as_raw_fd(), libc::_PC_XATTR_EXISTS) != 0 }
        }
        // Elsewhere it cannot be told here: left as it is.
        #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "illumos", target_os = "solaris")))]
        {
            let _ = f;
            true
        }
    }

    /// Extended attributes on the file, SELinux's label aside; an error but "none possible here"
    /// counts as some.
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
    fn xattrs_listed(f: &std::fs::File) -> bool {
        let mut names = vec![0u8; 64 * 1024];
        // SAFETY: an open file and a buffer of the length given.
        #[cfg(target_os = "macos")]
        let n = unsafe { libc::flistxattr(f.as_raw_fd(), names.as_mut_ptr().cast(), names.len(), 0) };
        // SAFETY: as above.
        #[cfg(not(target_os = "macos"))]
        let n = unsafe { libc::flistxattr(f.as_raw_fd(), names.as_mut_ptr().cast(), names.len()) };
        if n < 0 {
            return last().raw_os_error() != Some(libc::ENOTSUP);
        }
        // Labels the system puts on files by itself and a new file gets again: SELinux's, and on
        // macOS where a file came from and when it was last used.
        const OWN: [&[u8]; 3] = [b"security.selinux", b"com.apple.provenance", b"com.apple.lastuseddate#PS"];
        names[..n as usize].split(|&b| b == 0).any(|name| !name.is_empty() && !OWN.contains(&name))
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;

    /// FILE_FLAG_BACKUP_SEMANTICS (folders open too) | FILE_FLAG_OPEN_REPARSE_POINT (a link is
    /// opened itself, never what it points at).
    const FLAGS: u32 = 0x0200_0000 | 0x0020_0000;
    /// FILE_READ_ATTRIBUTES.
    const READ_ATTRIBUTES: u32 = 0x80;

    /// An entry's path: Windows has no handle-relative walk here yet.
    #[derive(Clone)]
    pub struct Place {
        path: PathBuf,
        dir: (u64, u64),
    }

    /// What the open `f` is, from its handle.
    pub fn stat_of(f: &std::fs::File) -> std::io::Result<Stat> {
        use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandle, GetFileInformationByHandleEx};
        let m = f.metadata()?;
        // SAFETY: zeroes are a valid BY_HANDLE_FILE_INFORMATION, which the call fills in.
        let mut i: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is open for the call and `i` outlives it.
        if unsafe { GetFileInformationByHandle(f.as_raw_handle() as _, &mut i) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: zeroes are a valid FILE_BASIC_INFO, which the call fills in.
        let mut b: FILE_BASIC_INFO = unsafe { std::mem::zeroed() };
        // SAFETY: as above, with the size of `b` given.
        if unsafe { GetFileInformationByHandleEx(f.as_raw_handle() as _, FileBasicInfo, std::ptr::from_mut(&mut b).cast(), std::mem::size_of::<FILE_BASIC_INFO>() as u32) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let ft = m.file_type();
        let kind = if ft.is_symlink() || m.file_attributes() & 0x400 != 0 {
            Kind::Link
        } else if ft.is_dir() {
            Kind::Dir
        } else if ft.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        let zero = (0, 0);
        Ok(Stat {
            // The change time, and (as NTFS may bring that up to date only later) the last write
            // and the size folded in: a file written meanwhile is not taken for the same.
            id: Id { dev: i.dwVolumeSerialNumber as u64, ino: (i.nFileIndexHigh as u64) << 32 | i.nFileIndexLow as u64, ctime: (b.ChangeTime, (b.LastWriteTime as u64 ^ m.len().wrapping_mul(0x9e37_79b9)) as u32) },
            kind,
            mode: m.file_attributes() & 3,
            nlink: i.nNumberOfLinks as u64,
            uid: 0,
            gid: 0,
            size: m.len(),
            modified: m.modified().map(secs).unwrap_or(zero),
            accessed: m.accessed().map(secs).unwrap_or(zero),
            flags: 0,
        })
    }

    /// The entry at `path` itself (a link is opened as the link), with `access`.
    pub fn open_path(path: &Path, access: u32) -> std::io::Result<(std::fs::File, Stat)> {
        let f = std::fs::OpenOptions::new().access_mode(access).custom_flags(FLAGS).open(path)?;
        let s = stat_of(&f)?;
        Ok((f, s))
    }

    fn dir_of(path: &Path) -> std::io::Result<(u64, u64)> {
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let (_, s) = open_path(parent, READ_ATTRIBUTES)?;
        Ok((s.id.dev, s.id.ino))
    }

    pub(super) fn root(path: &Path) -> std::io::Result<Place> {
        Ok(Place { path: path.to_path_buf(), dir: dir_of(path)? })
    }

    /// Never called: no walk goes into folders here (`RECURSES`).
    pub(super) fn list(_: &Node) -> std::io::Result<Vec<(PathBuf, Place)>> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    impl Place {
        pub fn reopen(path: &Path, dir: (u64, u64)) -> std::io::Result<Place> {
            if dir_of(path)? != dir {
                return Err(changed());
            }
            Ok(Place { path: path.to_path_buf(), dir })
        }

        pub fn dir_id(&self) -> (u64, u64) {
            self.dir
        }

        pub fn stat(&self) -> std::io::Result<Stat> {
            open_path(&self.path, READ_ATTRIBUTES).map(|(_, s)| s)
        }

        /// The entry opened to read (GENERIC_READ, FILE_READ_ATTRIBUTES) as itself and checked
        /// through that handle to be `want` exactly and a file or folder.
        pub fn open(&self, want: Id) -> std::io::Result<(std::fs::File, Stat)> {
            self.open_with(want, 0x8000_0000 | READ_ATTRIBUTES)
        }

        /// As `open`, to change its attributes and times (FILE_WRITE_ATTRIBUTES), not to read.
        pub fn open_to_change(&self, want: Id) -> std::io::Result<(std::fs::File, Stat)> {
            self.open_with(want, READ_ATTRIBUTES | 0x100)
        }

        fn open_with(&self, want: Id, access: u32) -> std::io::Result<(std::fs::File, Stat)> {
            let (f, s) = open_path(&self.path, access)?;
            match s.kind {
                Kind::Link => return Err(other(t!("pinned.link"))),
                Kind::Other => return Err(other(t!("pinned.special"))),
                _ => {}
            }
            if s.id != want {
                return Err(changed());
            }
            Ok((f, s))
        }

        /// Not on Windows yet (`WRITES`).
        pub fn rewrite(&self, _want: Id, _bytes: &[u8]) -> std::io::Result<Id> {
            Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(roots: &[PathBuf], recursive: bool, mut want: impl FnMut(&Node) -> bool) -> (Set, Said) {
        let (stop, mut said, mut set) = (AtomicBool::new(false), Said::default(), Set::default());
        walk(roots, &Walk { recursive, ..Default::default() }, &stop, &mut said, |n| {
            if want(n) {
                set.add(n);
            }
            true
        });
        (set, said)
    }

    fn run(roots: &[PathBuf], set: &Set) -> (Vec<PathBuf>, Said) {
        let (stop, mut said, mut seen) = (AtomicBool::new(false), Said::default(), vec![]);
        walk(roots, &Walk { recursive: true, only: Some(set), ..Default::default() }, &stop, &mut said, |n| {
            seen.push(n.path.clone());
            true
        });
        (seen, said)
    }

    fn files(n: &Node) -> bool {
        n.stat.kind == Kind::File
    }

    #[test]
    fn the_marked_entries_are_met_and_only_unchanged() {
        let d = crate::fs::Scratch::named("pinned-roots");
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/f"), "x").unwrap();
        std::fs::write(d.join("g"), "x").unwrap();
        let roots = [d.join("g"), d.join("sub")];
        let (set, said) = plan(&roots, false, |_| true);
        assert_eq!((set.len(), said.lines.len()), (2, 0));
        assert_eq!(run(&roots, &set).0.len(), 2);
        // Into folders only where a folder is walked through its handle.
        let (set, said) = plan(&[d.to_path_buf()], true, files);
        assert_eq!((set.len(), said.lines.len()), if RECURSES { (2, 0) } else { (0, 1) });
    }

    #[cfg(unix)]
    #[test]
    fn the_run_meets_only_what_the_plan_saw() {
        let d = crate::fs::Scratch::named("pinned-1");
        std::fs::create_dir_all(d.join("a/b")).unwrap();
        std::fs::write(d.join("a/b/f"), "x").unwrap();
        std::fs::write(d.join("a/g"), "x").unwrap();
        std::fs::write(d.join("h"), "x").unwrap();
        let (set, said) = plan(&[d.to_path_buf()], true, files);
        assert_eq!((set.len(), said.lines.len()), (3, 0));
        let (seen, _) = run(&[d.to_path_buf()], &set);
        assert_eq!(seen.len(), 3);
    }

    /// Time enough for a file system's timestamps (a tick of the clock) to move on.
    #[cfg(unix)]
    fn tick() {
        std::thread::sleep(std::time::Duration::from_millis(30));
    }

    #[cfg(unix)]
    #[test]
    fn what_changed_since_the_plan_is_not_met() {
        let d = crate::fs::Scratch::named("pinned-1b");
        std::fs::create_dir_all(d.join("a/b")).unwrap();
        std::fs::write(d.join("a/b/f"), "x").unwrap();
        std::fs::write(d.join("a/g"), "x").unwrap();
        std::fs::write(d.join("h"), "x").unwrap();
        // A file added later is not met; a file written meanwhile is not met either.
        std::fs::write(d.join("a/new"), "x").unwrap();
        let (set, _) = plan(&[d.to_path_buf()], true, files);
        tick();
        std::fs::write(d.join("h"), "changed").unwrap();
        std::fs::write(d.join("a/later"), "x").unwrap();
        let (seen, _) = run(&[d.to_path_buf()], &set);
        // `a` changed (a name came in): nothing under it is met; `h` was written.
        assert!(seen.is_empty(), "{seen:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_for_a_link_after_the_plan_leads_nowhere() {
        let (d, out) = (crate::fs::Scratch::named("pinned-swap"), crate::fs::Scratch::named("pinned-swap-out"));
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/f"), "x").unwrap();
        std::fs::write(out.join("f"), "x").unwrap();
        let (set, _) = plan(&[d.to_path_buf()], true, files);
        std::fs::rename(d.join("sub"), d.join("moved")).unwrap();
        std::os::unix::fs::symlink(&*out, d.join("sub")).unwrap();
        let (seen, _) = run(&[d.to_path_buf()], &set);
        assert!(seen.is_empty(), "{seen:?}");
        // The root itself swapped: passed over.
        let (set, _) = plan(&[d.join("moved")], true, files);
        std::fs::rename(d.join("moved"), d.join("moved2")).unwrap();
        std::os::unix::fs::symlink(&*out, d.join("moved")).unwrap();
        let (seen, _) = run(&[d.join("moved")], &set);
        assert!(seen.is_empty(), "{seen:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_renamed_in_after_the_plan_is_not_gone_into() {
        let (d, out) = (crate::fs::Scratch::named("pinned-in"), crate::fs::Scratch::named("pinned-in-out"));
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/f"), "x").unwrap();
        std::fs::create_dir(out.join("sub")).unwrap();
        std::fs::write(out.join("sub/f"), "x").unwrap();
        // Only `sub/f` planned; its folder then swapped for another of the same name.
        let (set, _) = plan(&[d.join("sub")], true, files);
        std::fs::rename(d.join("sub"), out.join("old")).unwrap();
        std::fs::rename(out.join("sub"), d.join("sub")).unwrap();
        let (seen, _) = run(&[d.join("sub")], &set);
        assert!(seen.is_empty(), "{seen:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_between_listing_and_opening_is_refused() {
        let (d, out) = (crate::fs::Scratch::named("pinned-race"), crate::fs::Scratch::named("pinned-race-out"));
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::create_dir(out.join("sub")).unwrap();
        std::fs::write(out.join("sub/secret"), "x").unwrap();
        let (stop, mut said, mut seen) = (AtomicBool::new(false), Said::default(), vec![]);
        let mut swapped = false;
        walk(&[d.to_path_buf()], &Walk { recursive: true, ..Default::default() }, &stop, &mut said, |n| {
            seen.push(n.path.clone());
            // Swapped right after it was looked at, before it is gone into.
            if n.path.ends_with("sub") && !swapped {
                std::fs::rename(d.join("sub"), d.join("gone")).unwrap();
                std::fs::rename(out.join("sub"), d.join("sub")).unwrap();
                swapped = true;
            }
            true
        });
        assert!(!seen.iter().any(|p| p.ends_with("secret")), "{seen:?}");
        assert_eq!(said.lines.len(), 1, "{said:?}");
    }

    #[cfg(unix)]
    #[test]
    fn pipes_devices_and_links_are_never_opened() {
        let d = crate::fs::Scratch::named("pinned-fifo");
        let fifo = std::ffi::CString::new(d.join("pipe").as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        std::os::unix::fs::symlink("/dev/null", d.join("null")).unwrap();
        std::fs::write(d.join("f"), "x").unwrap();
        let (stop, mut said) = (AtomicBool::new(false), Said::default());
        let mut opened = vec![];
        walk(&[d.to_path_buf()], &Walk { recursive: true, ..Default::default() }, &stop, &mut said, |n| {
            if n.depth > 0 {
                opened.push((n.path.file_name().unwrap().to_owned(), n.stat.kind, n.open().is_ok()));
            }
            true
        });
        opened.sort_by(|a, b| a.0.cmp(&b.0));
        let names: Vec<_> = opened.iter().map(|(n, k, ok)| (n.to_str().unwrap(), *k, *ok)).collect();
        assert_eq!(names, [("f", Kind::File, true), ("null", Kind::Link, false), ("pipe", Kind::Other, false)]);
        // A file swapped for a pipe after it was listed: refused, never waited on.
        let (set, _) = plan(&[d.join("f")], false, files);
        std::fs::remove_file(d.join("f")).unwrap();
        std::fs::rename(d.join("pipe"), d.join("f")).unwrap();
        let (seen, _) = run(&[d.join("f")], &set);
        assert!(seen.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn rewriting_goes_beside_keeps_the_mode_and_never_through_a_hard_link() {
        use std::os::unix::fs::PermissionsExt;
        let d = crate::fs::Scratch::named("pinned-rewrite");
        std::fs::write(d.join("a"), "old").unwrap();
        std::fs::set_permissions(d.join("a"), std::fs::Permissions::from_mode(0o640)).unwrap();
        let (stop, mut said) = (AtomicBool::new(false), Said::default());
        let mut pins = vec![];
        walk(&[d.join("a")], &Walk::default(), &stop, &mut said, |n| {
            let new = n.place.rewrite(n.stat.id, b"new").unwrap();
            assert!(!new.same_file(&n.stat.id));
            pins.push(Pin { dir: n.place.dir_id(), file: new, sum: None });
            // Not twice: it is not the file it was.
            assert!(n.place.rewrite(n.stat.id, b"again").is_err());
            true
        });
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "new");
        assert_eq!(std::fs::metadata(d.join("a")).unwrap().permissions().mode() & 0o777, 0o640);
        // Undo: back in the same folder, only to the file as written.
        let p = Place::reopen(&d.join("a"), pins[0].dir).unwrap();
        let now = p.stat().unwrap().id;
        assert!(now.same_file(&pins[0].file));
        p.rewrite(now, b"old").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "old");
        // A hard link to someone else's file swapped in: refused.
        std::fs::write(d.join("theirs"), "theirs").unwrap();
        let id = p.stat().unwrap().id;
        std::fs::remove_file(d.join("a")).unwrap();
        std::fs::hard_link(d.join("theirs"), d.join("a")).unwrap();
        assert!(p.rewrite(id, b"ours").is_err());
        assert!(p.rewrite(p.stat().unwrap().id, b"ours").is_err());
        assert_eq!(std::fs::read_to_string(d.join("theirs")).unwrap(), "theirs");
        // No file beside is left behind.
        assert_eq!(std::fs::read_dir(&*d).unwrap().count(), 2);
        // A setgid file is not written anew (it would lose the bit, or keep it for new content).
        std::fs::write(d.join("s"), "old").unwrap();
        // A sandbox may refuse the bit (Nix's build sandbox): then no such file can be there.
        let _ = std::fs::set_permissions(d.join("s"), std::fs::Permissions::from_mode(0o2755));
        let p = Place::reopen(&d.join("s"), pins[0].dir).unwrap();
        if p.stat().unwrap().mode & 0o2000 != 0 {
            assert!(p.rewrite(p.stat().unwrap().id, b"new").is_err());
            assert_eq!(std::fs::read_to_string(d.join("s")).unwrap(), "old");
        }
    }

    #[cfg(unix)]
    #[test]
    fn undo_refuses_another_folder() {
        let (d, out) = (crate::fs::Scratch::named("pinned-undo"), crate::fs::Scratch::named("pinned-undo-out"));
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/a"), "x").unwrap();
        std::fs::write(out.join("a"), "x").unwrap();
        let (stop, mut said, mut pin) = (AtomicBool::new(false), Said::default(), None);
        walk(&[d.join("sub/a")], &Walk::default(), &stop, &mut said, |n| {
            pin = Some(n.pin());
            true
        });
        let pin = pin.unwrap();
        std::fs::rename(d.join("sub"), d.join("moved")).unwrap();
        std::os::unix::fs::symlink(&*out, d.join("sub")).unwrap();
        assert!(Place::reopen(&d.join("sub/a"), pin.dir).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_s_inherited_acl_stops_a_rewrite() {
        let d = crate::fs::Scratch::named("pinned-inherit");
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/f"), "old").unwrap();
        // A default ACL given to the folder after the file was made: the file has none, a new one would.
        let given = std::process::Command::new("setfacl").args(["-d", "-m", "u:nobody:r"]).arg(d.join("sub")).output().is_ok_and(|o| o.status.success());
        if !given {
            return;
        }
        let (stop, mut said) = (AtomicBool::new(false), Said::default());
        walk(&[d.join("sub/f")], &Walk::default(), &stop, &mut said, |n| {
            assert!(!extra_permissions(&n.open().unwrap().0));
            assert!(n.place.rewrite(n.stat.id, b"new").is_err());
            true
        });
        assert_eq!(std::fs::read_to_string(d.join("sub/f")).unwrap(), "old");
        assert_eq!(std::fs::read_dir(d.join("sub")).unwrap().count(), 1);
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "illumos"))]
    #[test]
    fn extra_attributes_are_seen() {
        let d = crate::fs::Scratch::named("pinned-xattr");
        std::fs::write(d.join("a"), "x").unwrap();
        let f = std::fs::File::open(d.join("a")).unwrap();
        assert!(!extra_permissions(&f));
        let p = std::ffi::CString::new(d.join("a").as_os_str().as_encoded_bytes()).unwrap();
        #[cfg(target_os = "linux")]
        // SAFETY: NUL-terminated path and name, a value of the length given.
        let set = unsafe { libc::setxattr(p.as_ptr(), c"user.coxswain-test".as_ptr(), b"x".as_ptr().cast(), 1, 0) } == 0;
        #[cfg(target_os = "freebsd")]
        // SAFETY: as above.
        let set = unsafe { libc::extattr_set_file(p.as_ptr(), libc::EXTATTR_NAMESPACE_USER, c"coxswain-test".as_ptr(), b"x".as_ptr().cast(), 1) } == 1;
        #[cfg(target_os = "illumos")]
        let set = {
            let _ = p;
            false
        };
        if set {
            assert!(extra_permissions(&f));
        }
        // An ACL entry for someone else, as the system's own tool writes it.
        std::fs::write(d.join("b"), "x").unwrap();
        let entries: &[&[&str]] = if cfg!(target_os = "illumos") { &[&["chmod", "A+user:nobody:read_data:allow"]] } else { &[&["setfacl", "-m", "u:nobody:r"], &["setfacl", "-m", "u:nobody:read_data::allow"]] };
        let given = entries.iter().any(|c| std::process::Command::new(c[0]).args(&c[1..]).arg(d.join("b")).output().is_ok_and(|o| o.status.success()));
        if given {
            assert!(extra_permissions(&std::fs::File::open(d.join("b")).unwrap()));
        }
    }

    #[cfg(unix)]
    #[test]
    fn stop_and_depth() {
        let d = crate::fs::Scratch::named("pinned-deep");
        let mut p = d.to_path_buf();
        for _ in 0..deepest() + 2 {
            p = p.join("d");
        }
        std::fs::create_dir_all(&p).unwrap();
        let (set, said) = plan(&[d.to_path_buf()], true, |_| true);
        assert_eq!(set.len(), deepest() + 1);
        assert_eq!(said.lines.len(), 1, "{said:?}");
        let stop = AtomicBool::new(true);
        let mut said = Said::default();
        assert_eq!(walk(&[d.to_path_buf()], &Walk::default(), &stop, &mut said, |_| true), 0);
    }
}

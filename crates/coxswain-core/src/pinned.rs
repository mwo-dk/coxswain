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
//! On Windows the same, through NT handles: each folder opened relative to the one above
//! (`NtCreateFile` with its handle as the root, `FILE_OPEN_REPARSE_POINT`, so a junction or link
//! is opened as itself and never gone into), listed through its handle
//! (`FileIdBothDirectoryInfo`), every entry known by volume serial, file index, change time, last
//! write and size. A file is written anew beside itself in its held folder and renamed over it
//! there (`FileRenameInformation` relative to the folder's handle), only with the old file's
//! owner and permissions, and never when it has other streams or extended attributes.

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
    /// Where it is: its folder, held open, and its name there.
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
        if !(w.recursive && route && stat.kind == Kind::Dir) {
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
    fn clear_errno() {
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

    /// Whether the folder `dir` is on a local disk that NTFS or ReFS keeps, where an open relative
    /// to a handle stays one. Fails closed: anything not known to be so is not.
    fn local_disk(dir: HANDLE) -> bool {
        // FILE_FS_DEVICE_INFORMATION: device type, characteristics.
        let mut dev = [0u32; 2];
        // SAFETY: zeroes are a valid IO_STATUS_BLOCK.
        let mut io: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
        // SAFETY: the held folder, `dev` of the size given.
        let st = unsafe { NtQueryVolumeInformationFile(dir, &mut io, dev.as_mut_ptr().cast(), 8, FileFsDeviceInformation) };
        // FILE_DEVICE_DISK, and not FILE_REMOTE_DEVICE.
        if st < 0 || dev[0] != 0x07 || dev[1] & 0x10 != 0 {
            return false;
        }
        // A network redirector answers this; a local file system refuses it as invalid (87).
        // SAFETY: zeroes are a valid FILE_REMOTE_PROTOCOL_INFO, which the call fills in.
        let mut remote: FILE_REMOTE_PROTOCOL_INFO = unsafe { std::mem::zeroed() };
        // SAFETY: the held folder, `remote` of the size given.
        if unsafe { GetFileInformationByHandleEx(dir, FileRemoteProtocolInfo, std::ptr::from_mut(&mut remote).cast(), std::mem::size_of::<FILE_REMOTE_PROTOCOL_INFO>() as u32) } != 0 || last().raw_os_error() != Some(87) {
            return false;
        }
        let mut fs = [0u16; 64];
        // SAFETY: the held folder, and a buffer for the file system's name of the length given.
        if unsafe { GetVolumeInformationByHandleW(dir, std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), fs.as_mut_ptr(), fs.len() as u32) } == 0 {
            return false;
        }
        let name = String::from_utf16_lossy(&fs[..fs.iter().position(|&c| c == 0).unwrap_or(fs.len())]);
        name == "NTFS" || name == "ReFS"
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

    fn refuse_kind(k: Kind) -> std::io::Result<()> {
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
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{FILE_CREATE, FILE_DIRECTORY_FILE, FILE_EA_INFORMATION, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, FileEaInformation, FileRenameInformation, FileFsDeviceInformation, NtCreateFile, NtQueryInformationFile, NtQueryVolumeInformationFile, NtSetInformationFile};
    use windows_sys::Win32::Foundation::{HANDLE, LocalFree, RtlNtStatusToDosError, UNICODE_STRING};
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{ACL, EqualSid, GetSecurityDescriptorControl, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FILE_DISPOSITION_INFO, FILE_ID_BOTH_DIR_INFO, FILE_REMOTE_PROTOCOL_INFO, FILE_RENAME_INFO, FILE_STREAM_INFO, FileBasicInfo, FileRemoteProtocolInfo, FileDispositionInfo, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo, FileStreamInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, GetVolumeInformationByHandleW, SetFileInformationByHandle};
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    /// FILE_FLAG_BACKUP_SEMANTICS (folders open too) | FILE_FLAG_OPEN_REPARSE_POINT (a link is
    /// opened itself, never what it points at).
    const FLAGS: u32 = 0x0200_0000 | 0x0020_0000;
    const READ_ATTRIBUTES: u32 = 0x80;
    const WRITE_ATTRIBUTES: u32 = 0x100;
    const LIST_DIRECTORY: u32 = 0x1;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const DELETE: u32 = 0x1_0000;
    const READ_CONTROL: u32 = 0x2_0000;
    const SYNCHRONIZE: u32 = 0x10_0000;
    /// Read, write and delete: others may go on using the entry, as `std::fs` lets them.
    const SHARE_ALL: u32 = 7;
    /// Attributes a file written anew carries over: hidden, archive, normal, not indexed. Any
    /// other (system, compressed, encrypted, sparse, read-only …) and it is left as it is.
    const CARRIED: u32 = 0x2 | 0x20 | 0x80 | 0x2000;

    /// A folder held open, and what it is.
    pub struct Held {
        h: OwnedHandle,
        id: (u64, u64),
    }

    /// An entry's folder, held open, and its name there.
    #[derive(Clone)]
    pub struct Place {
        dir: Arc<Held>,
        name: Vec<u16>,
    }

    fn last() -> std::io::Error {
        std::io::Error::last_os_error()
    }

    fn nt(status: i32) -> std::io::Error {
        // SAFETY: a plain conversion of a status code.
        std::io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32)
    }

    /// `name` opened in the folder `dir` itself, never through a link (FILE_OPEN_REPARSE_POINT):
    /// nothing is looked up by path. A name that is no plain name (a separator, a stream, `..`)
    /// is refused, so it cannot lead out of the folder.
    fn open_in(dir: HANDLE, name: &[u16], access: u32, disposition: u32, options: u32, share: u32) -> std::io::Result<std::fs::File> {
        let plain = !name.is_empty() && name != [b'.' as u16] && name != [b'.' as u16, b'.' as u16] && !name.iter().any(|&c| c == b'\\' as u16 || c == b'/' as u16 || c == b':' as u16 || c == 0);
        if !plain || name.len() > 32_000 {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
        }
        let us = UNICODE_STRING { Length: (name.len() * 2) as u16, MaximumLength: (name.len() * 2) as u16, Buffer: name.as_ptr() as *mut u16 };
        // OBJ_CASE_INSENSITIVE, as CreateFile asks.
        let oa = OBJECT_ATTRIBUTES { Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32, RootDirectory: dir, ObjectName: &us, Attributes: 0x40, SecurityDescriptor: std::ptr::null(), SecurityQualityOfService: std::ptr::null() };
        let mut h: HANDLE = std::ptr::null_mut();
        // SAFETY: zeroes are a valid IO_STATUS_BLOCK, which the call fills in.
        let mut io: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
        // SAFETY: an open folder handle, a name that outlives the call, `h` and `io` ours to fill;
        // synchronous, so the handle works as a std File.
        let st = unsafe { NtCreateFile(&mut h, access | SYNCHRONIZE, &oa, &mut io, std::ptr::null(), 0x80, share, disposition, options | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT, std::ptr::null(), 0) };
        if st < 0 {
            return Err(nt(st));
        }
        // SAFETY: just opened, owned here.
        Ok(std::fs::File::from(unsafe { OwnedHandle::from_raw_handle(h as _) }))
    }

    /// What the open `f` is, from its handle.
    pub fn stat_of(f: &std::fs::File) -> std::io::Result<Stat> {
        let m = f.metadata()?;
        // SAFETY: zeroes are a valid BY_HANDLE_FILE_INFORMATION, which the call fills in.
        let mut i: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is open for the call and `i` outlives it.
        if unsafe { GetFileInformationByHandle(f.as_raw_handle() as _, &mut i) } == 0 {
            return Err(last());
        }
        let b = basic(f)?;
        let ft = m.file_type();
        // A reparse point of any kind (link, junction, mount point, cloud placeholder) is a link.
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

    fn basic(f: &std::fs::File) -> std::io::Result<FILE_BASIC_INFO> {
        // SAFETY: zeroes are a valid FILE_BASIC_INFO, which the call fills in.
        let mut b: FILE_BASIC_INFO = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is open for the call, `b` outlives it and its size is given.
        if unsafe { GetFileInformationByHandleEx(f.as_raw_handle() as _, FileBasicInfo, std::ptr::from_mut(&mut b).cast(), std::mem::size_of::<FILE_BASIC_INFO>() as u32) } == 0 {
            return Err(last());
        }
        Ok(b)
    }

    /// The folder at `path`, opened as itself: its real path (a link on the way resolved once,
    /// here), opened as a folder without following a link at its end.
    fn open_dir(path: &Path) -> std::io::Result<Held> {
        let path = if path.as_os_str().is_empty() { Path::new(".") } else { path };
        let f = std::fs::OpenOptions::new().access_mode(LIST_DIRECTORY | READ_ATTRIBUTES | SYNCHRONIZE).custom_flags(FLAGS).open(std::fs::canonicalize(path)?)?;
        let s = stat_of(&f)?;
        match s.kind {
            Kind::Dir => Ok(Held { h: f.into(), id: (s.id.dev, s.id.ino) }),
            Kind::Link => Err(other(t!("pinned.link"))),
            _ => Err(std::io::Error::from(std::io::ErrorKind::NotADirectory)),
        }
    }

    fn split(path: &Path) -> std::io::Result<(&Path, Vec<u16>)> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
        };
        Ok((parent, name.encode_wide().collect()))
    }

    pub(super) fn root(path: &Path) -> std::io::Result<Place> {
        let (parent, name) = split(path)?;
        Ok(Place { dir: Arc::new(open_dir(parent)?), name })
    }

    /// The names in the folder `node` (opened in its own folder, checked to be it), listed
    /// through that handle, in order.
    pub(super) fn list(node: &Node) -> std::io::Result<Vec<(PathBuf, Place)>> {
        let p = &node.place;
        let f = open_in(p.dir.h.as_raw_handle() as _, &p.name, LIST_DIRECTORY | READ_ATTRIBUTES, FILE_OPEN, FILE_DIRECTORY_FILE, SHARE_ALL)?;
        let s = stat_of(&f)?;
        if s.kind == Kind::Link {
            return Err(other(t!("pinned.link")));
        }
        // The very folder that was looked at, not one swapped in since.
        if s.kind != Kind::Dir || !s.id.same_file(&node.stat.id) {
            return Err(changed());
        }
        let mut names: Vec<Vec<u16>> = vec![];
        // 64 KiB, aligned for the records the call writes.
        let mut buf = vec![0u64; 8 * 1024];
        let mut class = FileIdBothDirectoryRestartInfo;
        loop {
            // SAFETY: an open folder, a buffer of the length given.
            if unsafe { GetFileInformationByHandleEx(f.as_raw_handle() as _, class, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32) } == 0 {
                let e = last();
                // ERROR_NO_MORE_FILES: the end. Anything else cuts the listing short: said.
                if e.raw_os_error() == Some(18) {
                    break;
                }
                return Err(e);
            }
            class = FileIdBothDirectoryInfo;
            let base = buf.as_ptr().cast::<u8>();
            let mut at = 0usize;
            loop {
                if at + std::mem::size_of::<FILE_ID_BOTH_DIR_INFO>() > buf.len() * 8 {
                    return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
                }
                // SAFETY: a record the call wrote at `at`, inside the buffer (checked above).
                let e = unsafe { base.add(at).cast::<FILE_ID_BOTH_DIR_INFO>() };
                // SAFETY: as above; unaligned reads, whatever the offsets.
                let (next, len) = unsafe { (std::ptr::addr_of!((*e).NextEntryOffset).read_unaligned(), std::ptr::addr_of!((*e).FileNameLength).read_unaligned()) };
                let name_at = at + std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
                if name_at + len as usize > buf.len() * 8 {
                    return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
                }
                // SAFETY: the name's bytes, inside the buffer (checked above), copied out.
                let name: Vec<u16> = unsafe { std::slice::from_raw_parts(base.add(name_at), len as usize) }.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                if name != [b'.' as u16] && name != [b'.' as u16, b'.' as u16] {
                    names.push(name);
                }
                if next == 0 {
                    break;
                }
                at += next as usize;
            }
        }
        names.sort();
        let held = Arc::new(Held { h: f.into(), id: (s.id.dev, s.id.ino) });
        Ok(names.into_iter().map(|n| (node.path.join(OsString::from_wide(&n)), Place { dir: held.clone(), name: n })).collect())
    }

    /// Something a file written anew would lose: other attributes than `CARRIED`, other data
    /// streams (the mark of the web among them), extended attributes. Fails closed.
    fn extra(f: &std::fs::File, attributes: u32) -> bool {
        if attributes & !CARRIED != 0 {
            return true;
        }
        // SAFETY: zeroes are a valid FILE_EA_INFORMATION and IO_STATUS_BLOCK, filled in by the call.
        let (mut ea, mut io): (FILE_EA_INFORMATION, IO_STATUS_BLOCK) = unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
        // SAFETY: an open file, `ea` of the size given.
        let st = unsafe { NtQueryInformationFile(f.as_raw_handle() as _, &mut io, std::ptr::from_mut(&mut ea).cast(), std::mem::size_of::<FILE_EA_INFORMATION>() as u32, FileEaInformation) };
        // A file system that keeps none (STATUS_INVALID_PARAMETER, _NOT_SUPPORTED,
        // _INVALID_DEVICE_REQUEST) has none to lose; any other failure counts as some.
        if st < 0 && !matches!(st as u32, 0xC000_000D | 0xC000_00BB | 0xC000_0010) || st >= 0 && ea.EaSize != 0 {
            return true;
        }
        let mut buf = vec![0u64; 8 * 1024];
        // SAFETY: an open file, a buffer of the length given.
        if unsafe { GetFileInformationByHandleEx(f.as_raw_handle() as _, FileStreamInfo, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32) } == 0 {
            // A file system with no streams at all (FAT) has none to lose.
            return !matches!(last().raw_os_error(), Some(1 | 38 | 50 | 87));
        }
        let base = buf.as_ptr().cast::<u8>();
        let mut at = 0usize;
        loop {
            if at + std::mem::size_of::<FILE_STREAM_INFO>() > buf.len() * 8 {
                return true;
            }
            // SAFETY: a record the call wrote, inside the buffer (checked above).
            let e = unsafe { base.add(at).cast::<FILE_STREAM_INFO>() };
            // SAFETY: as above.
            let (next, len) = unsafe { (std::ptr::addr_of!((*e).NextEntryOffset).read_unaligned(), std::ptr::addr_of!((*e).StreamNameLength).read_unaligned()) };
            let name_at = at + std::mem::offset_of!(FILE_STREAM_INFO, StreamName);
            if name_at + len as usize > buf.len() * 8 {
                return true;
            }
            // SAFETY: the name's bytes, inside the buffer.
            let name = unsafe { std::slice::from_raw_parts(base.add(name_at), len as usize) };
            let data: Vec<u8> = "::$DATA".encode_utf16().flat_map(u16::to_le_bytes).collect();
            if name != data.as_slice() {
                return true;
            }
            if next == 0 {
                return false;
            }
            at += next as usize;
        }
    }

    /// The owner and permissions (DACL) of `f`, to compare.
    fn security(f: &std::fs::File) -> std::io::Result<(PSID, Vec<u8>, PSECURITY_DESCRIPTOR, u16)> {
        let (mut owner, mut dacl, mut sd): (PSID, *mut ACL, PSECURITY_DESCRIPTOR) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
        // SAFETY: an open handle with READ_CONTROL; the pointers point into `sd`, freed by the caller.
        let e = unsafe { GetSecurityInfo(f.as_raw_handle() as _, SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION, &mut owner, std::ptr::null_mut(), &mut dacl, std::ptr::null_mut(), &mut sd) };
        if e != 0 {
            return Err(std::io::Error::from_raw_os_error(e as i32));
        }
        // SAFETY: a DACL inside `sd`, `AclSize` bytes long; a null one (everyone may) is empty.
        let bytes = if dacl.is_null() { vec![] } else { unsafe { std::slice::from_raw_parts(dacl.cast::<u8>(), (*dacl).AclSize as usize) }.to_vec() };
        // Whether the DACL is protected (inheritance turned off), from the descriptor's control.
        let (mut control, mut rev) = (0u16, 0u32);
        // SAFETY: the descriptor just got, and two values to fill.
        if unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut rev) } == 0 {
            // SAFETY: the descriptor GetSecurityInfo gave, not used again.
            unsafe { LocalFree(sd) };
            return Err(last());
        }
        Ok((owner, bytes, sd, control & 0x1000))
    }

    /// Whether `new` has the very owner and permissions `old` has: a file written anew must not
    /// be opened to others, or given to another owner.
    fn same_security(old: &std::fs::File, new: &std::fs::File) -> bool {
        let (a, b) = (security(old), security(new));
        // SAFETY: two SIDs inside their descriptors, still held.
        let same = matches!((&a, &b), (Ok(a), Ok(b)) if !a.0.is_null() && !b.0.is_null() && unsafe { EqualSid(a.0, b.0) } != 0 && a.1 == b.1 && !a.1.is_empty() && a.3 == b.3);
        // SAFETY: each descriptor GetSecurityInfo gave, freed once.
        for sd in [a, b].into_iter().flatten() {
            unsafe { LocalFree(sd.2) };
        }
        same
    }

    /// Whether the folder `dir` is on a local disk that NTFS or ReFS keeps, where an open relative
    /// to a handle stays one. Fails closed: anything not known to be so is not.
    fn local_disk(dir: HANDLE) -> bool {
        // FILE_FS_DEVICE_INFORMATION: device type, characteristics.
        let mut dev = [0u32; 2];
        // SAFETY: zeroes are a valid IO_STATUS_BLOCK.
        let mut io: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
        // SAFETY: the held folder, `dev` of the size given.
        let st = unsafe { NtQueryVolumeInformationFile(dir, &mut io, dev.as_mut_ptr().cast(), 8, FileFsDeviceInformation) };
        // FILE_DEVICE_DISK, and not FILE_REMOTE_DEVICE.
        if st < 0 || dev[0] != 0x07 || dev[1] & 0x10 != 0 {
            return false;
        }
        // A network redirector answers this; a local file system refuses it as invalid (87).
        // SAFETY: zeroes are a valid FILE_REMOTE_PROTOCOL_INFO, which the call fills in.
        let mut remote: FILE_REMOTE_PROTOCOL_INFO = unsafe { std::mem::zeroed() };
        // SAFETY: the held folder, `remote` of the size given.
        if unsafe { GetFileInformationByHandleEx(dir, FileRemoteProtocolInfo, std::ptr::from_mut(&mut remote).cast(), std::mem::size_of::<FILE_REMOTE_PROTOCOL_INFO>() as u32) } != 0 || last().raw_os_error() != Some(87) {
            return false;
        }
        let mut fs = [0u16; 64];
        // SAFETY: the held folder, and a buffer for the file system's name of the length given.
        if unsafe { GetVolumeInformationByHandleW(dir, std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), fs.as_mut_ptr(), fs.len() as u32) } == 0 {
            return false;
        }
        let name = String::from_utf16_lossy(&fs[..fs.iter().position(|&c| c == 0).unwrap_or(fs.len())]);
        name == "NTFS" || name == "ReFS"
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

        fn open_in(&self, access: u32, disposition: u32, options: u32, share: u32) -> std::io::Result<std::fs::File> {
            open_in(self.dir.h.as_raw_handle() as _, &self.name, access, disposition, options, share)
        }

        /// The entry as it is now, opened in its folder, never through a link.
        pub fn stat(&self) -> std::io::Result<Stat> {
            stat_of(&self.open_in(READ_ATTRIBUTES, FILE_OPEN, 0, SHARE_ALL)?)
        }

        /// The entry opened to read in its folder, as itself (a link is opened as the link), and
        /// checked through that handle to be `want` exactly and a file or folder.
        pub fn open(&self, want: Id) -> std::io::Result<(std::fs::File, Stat)> {
            self.open_with(want, GENERIC_READ | READ_ATTRIBUTES)
        }

        /// As `open`, to change its attributes and times, not to read.
        pub fn open_to_change(&self, want: Id) -> std::io::Result<(std::fs::File, Stat)> {
            self.open_with(want, READ_ATTRIBUTES | WRITE_ATTRIBUTES)
        }

        fn open_with(&self, want: Id, access: u32) -> std::io::Result<(std::fs::File, Stat)> {
            let f = self.open_in(access, FILE_OPEN, 0, SHARE_ALL).map_err(|e| if e.kind() == std::io::ErrorKind::PermissionDenied { other(t!("pinned.no_open")) } else { e })?;
            let s = stat_of(&f)?;
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

        /// The file written anew with `bytes`: a new file beside it, made in the same held folder
        /// (never one that is there, never through a link), with the old file's owner and
        /// permissions (else not written), its attributes and creation time, filled, flushed and
        /// renamed over it in that folder (`FileRenameInformation` relative to the folder's
        /// handle). Only while the name is still `want` exactly (a plain file with no other hard
        /// links). What it is now, for undo.
        pub fn rewrite(&self, want: Id, bytes: &[u8]) -> std::io::Result<Id> {
            use std::io::Write;
            // First exactly `want`; right before the rename the same file still.
            let check = |exact: bool| -> std::io::Result<Stat> {
                let s = self.stat()?;
                match s.kind {
                    Kind::Link => return Err(other(t!("pinned.link"))),
                    Kind::Other => return Err(other(t!("pinned.special"))),
                    _ => {}
                }
                if s.kind != Kind::File || if exact { s.id != want } else { !s.id.same_file(&want) } {
                    return Err(changed());
                }
                if s.nlink > 1 {
                    return Err(other(t!("pinned.hard_link")));
                }
                Ok(s)
            };
            check(true)?;
            // Only on a local NTFS or ReFS disk: a network share's server, or a file system in user
            // mode (WinFsp, Dokan), opens by path and could follow a junction swapped in there.
            if !local_disk(self.dir.h.as_raw_handle() as _) {
                return Err(other(t!("pinned.remote")));
            }
            let old = self.open_in(READ_CONTROL | READ_ATTRIBUTES, FILE_OPEN, FILE_NON_DIRECTORY_FILE, SHARE_ALL)?;
            if stat_of(&old)?.id != want {
                return Err(changed());
            }
            let was = basic(&old)?;
            if extra(&old, was.FileAttributes) {
                return Err(other(t!("pinned.extra")));
            }
            let dir = self.dir.h.as_raw_handle() as HANDLE;
            // A name of its own (process, time), short whatever the file's name.
            let stem: String = String::from_utf16_lossy(&self.name).chars().filter(|c| !matches!(c, '\\' | '/' | ':')).take(40).collect();
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
            let tmp: Vec<u16> = format!(".{stem}.coxswain-{}-{nanos}.tmp", std::process::id()).encode_utf16().collect();
            // A new file only, shared with no one while it is made.
            let mut f = open_in(dir, &tmp, GENERIC_WRITE | READ_ATTRIBUTES | DELETE | READ_CONTROL, FILE_CREATE, FILE_NON_DIRECTORY_FILE, 0)?;
            // The folder's inherited permissions would open it to others the old file was closed
            // to, or the old file is someone else's: then it is not written at all.
            let same = same_security(&old, &f);
            // Closed before the rename, which an open old file would stop.
            drop(old);
            let done = (|| {
                if !same {
                    return Err(other(t!("pinned.extra")));
                }
                f.write_all(bytes)?;
                // Its attributes and creation time as the old file has them; times of 0 are
                // left as they are.
                let info = FILE_BASIC_INFO { CreationTime: was.CreationTime, LastAccessTime: 0, LastWriteTime: 0, ChangeTime: 0, FileAttributes: if was.FileAttributes & CARRIED == 0 { 0x80 } else { was.FileAttributes & CARRIED } };
                // SAFETY: the file just made, `info` of the size given.
                if unsafe { SetFileInformationByHandle(f.as_raw_handle() as _, FileBasicInfo, std::ptr::from_ref(&info).cast(), std::mem::size_of::<FILE_BASIC_INFO>() as u32) } == 0 {
                    return Err(last());
                }
                f.sync_all()?;
                // Still the file the plan saw, right before it is replaced.
                check(false)?;
                // FILE_RENAME_INFORMATION: replace, in the held folder, under the old name.
                let head = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
                let size = head + self.name.len() * 2;
                let mut buf = vec![0u64; size.div_ceil(8) + 1];
                // SAFETY: the buffer holds the struct and the name (sized above), aligned for it.
                unsafe {
                    let r = buf.as_mut_ptr().cast::<FILE_RENAME_INFO>();
                    (*r).RootDirectory = dir;
                    (*r).FileNameLength = (self.name.len() * 2) as u32;
                    std::ptr::copy_nonoverlapping(self.name.as_ptr(), buf.as_mut_ptr().cast::<u8>().add(head).cast::<u16>(), self.name.len());
                }
                // POSIX semantics first (FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS):
                // the old file goes even while a scanner holds it open; where a file system or an
                // older Windows lacks it, the plain rename that replaces.
                for (class, flags) in [(65, 0x1 | 0x2), (FileRenameInformation, 0x1)] {
                    // SAFETY: as above; Flags overlays ReplaceIfExists (its low byte).
                    unsafe { (*buf.as_mut_ptr().cast::<FILE_RENAME_INFO>()).Anonymous.Flags = flags };
                    // SAFETY: zeroes are a valid IO_STATUS_BLOCK.
                    let mut io: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
                    // SAFETY: the file just made (opened with DELETE), the rename record of the size given.
                    // Never shorter than the record itself (a one-letter name).
                    let st = unsafe { NtSetInformationFile(f.as_raw_handle() as _, &mut io, buf.as_ptr().cast(), size.max(std::mem::size_of::<FILE_RENAME_INFO>()) as u32, class) };
                    if st >= 0 {
                        return Ok(());
                    }
                    // STATUS_INVALID_PARAMETER, STATUS_INVALID_INFO_CLASS, STATUS_NOT_SUPPORTED.
                    if class == FileRenameInformation || !matches!(st as u32, 0xC000_000D | 0xC000_0003 | 0xC000_00BB) {
                        return Err(nt(st));
                    }
                }
                Ok(())
            })();
            if let Err(e) = done {
                // Deleted through its own handle, never by name.
                let gone = FILE_DISPOSITION_INFO { DeleteFile: true };
                // SAFETY: the file just made, opened with DELETE.
                unsafe { SetFileInformationByHandle(f.as_raw_handle() as _, FileDispositionInfo, std::ptr::from_ref(&gone).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) };
                return Err(e);
            }
            // Written: what it is now, for undo; never an error here, which would cost the kept
            // copy its undo. One not to be told is recorded with no time, which undo refuses.
            Ok(stat_of(&f).map_or(Id { ctime: (-1, 0), ..want }, |s| s.id))
        }
    }
}

/// For tests: `link` made a link to the folder `target`: a symbolic link, on Windows a junction
/// (which needs no right of its own).
#[cfg(test)]
pub(crate) fn link_dir(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(link).arg(target).output().unwrap().status.success());
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
        let (set, said) = plan(&[d.to_path_buf()], true, files);
        assert_eq!((set.len(), said.lines.len()), (2, 0));
    }

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
    fn tick() {
        std::thread::sleep(std::time::Duration::from_millis(30));
    }

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

    #[test]
    fn a_folder_swapped_for_a_link_after_the_plan_leads_nowhere() {
        let (d, out) = (crate::fs::Scratch::named("pinned-swap"), crate::fs::Scratch::named("pinned-swap-out"));
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/f"), "x").unwrap();
        std::fs::write(out.join("f"), "x").unwrap();
        let (set, _) = plan(&[d.to_path_buf()], true, files);
        std::fs::rename(d.join("sub"), d.join("moved")).unwrap();
        link_dir(&out, &d.join("sub"));
        let (seen, _) = run(&[d.to_path_buf()], &set);
        assert!(seen.is_empty(), "{seen:?}");
        // The root itself swapped: passed over.
        let (set, _) = plan(&[d.join("moved")], true, files);
        std::fs::rename(d.join("moved"), d.join("moved2")).unwrap();
        link_dir(&out, &d.join("moved"));
        let (seen, _) = run(&[d.join("moved")], &set);
        assert!(seen.is_empty(), "{seen:?}");
    }

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
        // A link (on Windows a junction) swapped in at that moment: never gone into.
        let (stop, mut said, mut seen) = (AtomicBool::new(false), Said::default(), vec![]);
        let d = crate::fs::Scratch::named("pinned-race-2");
        std::fs::write(out.join("secret"), "x").unwrap();
        std::fs::create_dir(d.join("sub2")).unwrap();
        let mut swapped = false;
        walk(&[d.to_path_buf()], &Walk { recursive: true, ..Default::default() }, &stop, &mut said, |n| {
            seen.push(n.path.clone());
            if n.path.ends_with("sub2") && !swapped {
                std::fs::remove_dir(d.join("sub2")).unwrap();
                link_dir(&out, &d.join("sub2"));
                swapped = true;
            }
            true
        });
        assert!(swapped && !seen.iter().any(|p| p.ends_with("secret")), "{seen:?}");
        assert_eq!(said.lines.len(), 1, "{said:?}");
    }

    #[test]
    fn links_are_met_as_links_and_never_opened_or_gone_into() {
        let (d, out) = (crate::fs::Scratch::named("pinned-links"), crate::fs::Scratch::named("pinned-links-out"));
        std::fs::write(out.join("secret"), "x").unwrap();
        link_dir(&out, &d.join("j"));
        std::fs::write(d.join("f"), "x").unwrap();
        let (stop, mut said, mut met) = (AtomicBool::new(false), Said::default(), vec![]);
        walk(&[d.to_path_buf()], &Walk { recursive: true, ..Default::default() }, &stop, &mut said, |n| {
            met.push((n.path.file_name().unwrap().to_string_lossy().into_owned(), n.stat.kind, n.open().is_ok()));
            true
        });
        met.retain(|m| m.0 != d.file_name().unwrap().to_string_lossy());
        met.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(met, [("f".to_string(), Kind::File, true), ("j".to_string(), Kind::Link, false)]);
        // A file's name swapped for a link after the plan: not met.
        let (set, _) = plan(&[d.join("f")], false, files);
        std::fs::remove_file(d.join("f")).unwrap();
        link_dir(&out, &d.join("f"));
        assert!(run(&[d.join("f")], &set).0.is_empty());
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

    #[cfg(windows)]
    #[test]
    fn rewriting_on_windows_goes_beside_keeps_hidden_and_created_and_refuses_streams_and_hard_links() {
        use std::os::windows::fs::MetadataExt;
        let d = crate::fs::Scratch::named("pinned-rewrite-win");
        let a = d.join("a");
        std::fs::write(&a, "old").unwrap();
        assert!(std::process::Command::new("attrib").arg("+h").arg(&a).output().unwrap().status.success());
        let created = std::fs::metadata(&a).unwrap().creation_time();
        tick();
        let (stop, mut said) = (AtomicBool::new(false), Said::default());
        let mut pins = vec![];
        walk(std::slice::from_ref(&a), &Walk::default(), &stop, &mut said, |n| {
            let new = n.place.rewrite(n.stat.id, b"new").unwrap();
            assert!(!new.same_file(&n.stat.id));
            pins.push(Pin { dir: n.place.dir_id(), file: new, sum: None });
            // Not twice: it is not the file it was.
            assert!(n.place.rewrite(n.stat.id, b"again").is_err());
            true
        });
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "new");
        let m = std::fs::metadata(&a).unwrap();
        assert!(m.file_attributes() & 2 != 0, "{:x}", m.file_attributes());
        assert_eq!(m.creation_time(), created);
        // Undo: back in the same folder, only to the file as written.
        let p = Place::reopen(&a, pins[0].dir).unwrap();
        let now = p.stat().unwrap().id;
        assert!(now.same_file(&pins[0].file));
        p.rewrite(now, b"old").unwrap();
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "old");
        // Another data stream (as the mark of a download): refused, it would be lost.
        std::fs::write(d.join("s"), "old").unwrap();
        std::fs::write(d.join("s:Zone.Identifier"), "[ZoneTransfer]\r\nZoneId=3\r\n").unwrap();
        let p = Place::reopen(&d.join("s"), pins[0].dir).unwrap();
        assert_eq!(p.rewrite(p.stat().unwrap().id, b"new").unwrap_err().to_string(), t!("pinned.extra"));
        assert_eq!(std::fs::read_to_string(d.join("s")).unwrap(), "old");
        // A hard link: refused, never written through.
        std::fs::write(d.join("b"), "b").unwrap();
        std::fs::hard_link(d.join("b"), d.join("c")).unwrap();
        let p = Place::reopen(&d.join("b"), pins[0].dir).unwrap();
        assert!(p.rewrite(p.stat().unwrap().id, b"x").is_err());
        assert_eq!(std::fs::read_to_string(d.join("c")).unwrap(), "b");
        // A junction in a file's place: refused.
        std::fs::create_dir(d.join("t")).unwrap();
        link_dir(&d.join("t"), &d.join("j"));
        let p = Place::reopen(&d.join("j"), pins[0].dir).unwrap();
        assert!(p.rewrite(p.stat().unwrap().id, b"x").is_err());
        // No file beside is left behind: a, s, b, c, t, j.
        assert_eq!(std::fs::read_dir(&*d).unwrap().count(), 6);
    }

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
        link_dir(&out, &d.join("sub"));
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

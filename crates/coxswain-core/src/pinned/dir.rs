//! A folder held open by its handle, for the changes that make, move and remove entries
//! (`compare::sync`): every name is looked up in its held folder (`*at` calls), never through a
//! link, and every entry is checked through the handle to be exactly what a plan saw (`Id`)
//! before it is moved or removed. A folder below is opened only in the folder above, without
//! following a link at its end, and checked to be the very folder the plan saw.

use super::imp::{clear_errno, from_stat, refuse_kind};
use super::{Id, Kind, Stat, changed, other};
use crate::t;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// A folder held open.
pub struct Dir {
    fd: OwnedFd,
}

fn last() -> std::io::Error {
    std::io::Error::last_os_error()
}

fn c(name: &OsStr) -> std::io::Result<CString> {
    // One name in a folder: never a path, never `.` or `..`.
    if name.is_empty() || name == "." || name == ".." || name.as_bytes().contains(&b'/') {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    CString::new(name.as_bytes()).map_err(std::io::Error::other)
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

fn owned(fd: RawFd) -> std::io::Result<OwnedFd> {
    if fd < 0 {
        let e = last();
        return Err(if e.raw_os_error() == Some(libc::ELOOP) { other(t!("pinned.link")) } else { e });
    }
    // SAFETY: just opened, owned here.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

const DIR_FLAGS: libc::c_int = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;

/// A name of its own beside `name`, for a file being written (process, time, a count).
fn temp_name(name: &OsStr) -> CString {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stem: String = name.to_string_lossy().chars().take(40).collect();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    CString::new(format!(".{stem}.coxswain-{}-{nanos}-{n}.tmp", std::process::id()).replace('\0', "")).unwrap_or_default()
}

impl Dir {
    /// The folder at `path`, as itself: its real path (a link on the way resolved once, here),
    /// opened as a folder without following a link at its end.
    pub fn root(path: &Path) -> std::io::Result<Dir> {
        let real = std::fs::canonicalize(path)?;
        let p = CString::new(real.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
        // SAFETY: a NUL-terminated path.
        Ok(Dir { fd: owned(unsafe { libc::open(p.as_ptr(), DIR_FLAGS) })? })
    }

    /// The same folder, a handle of its own.
    pub fn try_clone(&self) -> std::io::Result<Dir> {
        Ok(Dir { fd: self.fd.try_clone()? })
    }

    /// Whether this folder is no longer in the folder `other` (it was moved elsewhere): its
    /// `..` looked at through its own handle.
    pub fn parent_is_not(&self, other: &Dir) -> bool {
        // SAFETY: the held folder and a NUL-terminated name; never a link, never blocking.
        let up = owned(unsafe { libc::openat(self.fd.as_raw_fd(), c"..".as_ptr(), DIR_FLAGS) });
        match (up.and_then(|u| fstat(u.as_raw_fd())), other.stat()) {
            (Ok(u), Ok(o)) => !u.id.same_file(&o.id),
            _ => false,
        }
    }

    /// What this folder is now.
    pub fn stat(&self) -> std::io::Result<Stat> {
        fstat(self.fd.as_raw_fd())
    }

    /// The entry `name` here, as it is now, never through a link.
    pub fn stat_at(&self, name: &OsStr) -> std::io::Result<Stat> {
        let n = c(name)?;
        // SAFETY: zeroes are a valid stat, which fstatat fills in.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: an open folder, a NUL-terminated name, `st` ours to fill.
        if unsafe { libc::fstatat(self.fd.as_raw_fd(), n.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) } != 0 {
            return Err(last());
        }
        Ok(from_stat(&st))
    }

    /// The folder `name` here, opened here without following a link, and checked through its
    /// handle to be `want` exactly (same folder, same ctime); with `same_file`, the same folder
    /// whatever its ctime (undo, after the run changed it).
    pub fn sub(&self, name: &OsStr, want: Id, same_file: bool) -> std::io::Result<Dir> {
        let n = c(name)?;
        // SAFETY: an open folder and a NUL-terminated name; never a link, never blocking.
        let fd = owned(unsafe { libc::openat(self.fd.as_raw_fd(), n.as_ptr(), DIR_FLAGS) })?;
        let s = fstat(fd.as_raw_fd())?;
        if s.kind != Kind::Dir || if same_file { !s.id.same_file(&want) } else { s.id != want } {
            return Err(changed());
        }
        Ok(Dir { fd })
    }

    /// The names here, `.` and `..` left out, in order.
    pub fn names(&self) -> std::io::Result<Vec<OsString>> {
        let mut names = vec![];
        // SAFETY: a duplicate of the folder's handle goes to fdopendir, which owns and closes it.
        unsafe {
            let dup = libc::fcntl(self.fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0);
            if dup < 0 {
                return Err(last());
            }
            let d = libc::fdopendir(dup);
            if d.is_null() {
                let e = last();
                libc::close(dup);
                return Err(e);
            }
            // From the start, whoever read this handle before.
            libc::rewinddir(d);
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
                let n = CStr::from_ptr((*e).d_name.as_ptr()).to_bytes();
                if n != b"." && n != b".." {
                    names.push(OsString::from_vec(n.to_vec()));
                }
            }
            libc::closedir(d);
        }
        names.sort();
        Ok(names)
    }

    /// The file `name` here opened to read, checked to be `want` exactly and a file: a link,
    /// pipe, device or socket is never opened, and opening never follows a link nor waits.
    pub fn open_file(&self, name: &OsStr, want: Id) -> std::io::Result<(std::fs::File, Stat)> {
        let before = self.stat_at(name)?;
        refuse_kind(before.kind)?;
        if before.kind != Kind::File || before.id != want {
            return Err(changed());
        }
        let n = c(name)?;
        // SAFETY: an open folder and a NUL-terminated name.
        let fd = unsafe { libc::openat(self.fd.as_raw_fd(), n.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC) };
        let f = std::fs::File::from(owned(fd).map_err(|e| if e.raw_os_error() == Some(libc::EACCES) { other(t!("pinned.no_open")) } else { e })?);
        let s = fstat(f.as_raw_fd())?;
        if s.kind != Kind::File || s.id != want {
            return Err(changed());
        }
        Ok((f, s))
    }

    /// Where the link `name` here points, checked to be `want` exactly.
    pub fn read_link(&self, name: &OsStr, want: Id) -> std::io::Result<OsString> {
        let s = self.stat_at(name)?;
        if s.kind != Kind::Link || s.id != want {
            return Err(changed());
        }
        let n = c(name)?;
        let mut buf = vec![0u8; 4096];
        // SAFETY: an open folder, a NUL-terminated name, and a buffer of that length.
        let len = unsafe { libc::readlinkat(self.fd.as_raw_fd(), n.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if len < 0 {
            return Err(last());
        }
        // As long as the buffer: cut short, never taken as the whole.
        if len as usize >= buf.len() {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
        }
        buf.truncate(len as usize);
        // Still the very link read.
        if self.stat_at(name)?.id != want {
            return Err(changed());
        }
        Ok(OsString::from_vec(buf))
    }

    /// Two entries here the same, without following a link: links by where they point, files
    /// by their bytes (read through their handles, a megabyte at a time).
    pub fn same_bytes(&self, name: &OsStr, a: Id, there: &Dir, b: Id, stop: &AtomicBool) -> std::io::Result<bool> {
        let (mut fa, _) = self.open_file(name, a)?;
        let (mut fb, _) = there.open_file(name, b)?;
        let (mut x, mut y) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
        loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(false);
            }
            let (n, m) = (read_full(&mut fa, &mut x)?, read_full(&mut fb, &mut y)?);
            if n != m || x[..n] != y[..m] {
                return Ok(false);
            }
            if n == 0 {
                return Ok(true);
            }
        }
    }

    /// A new file `name` here, with the bytes of `from` (as `was`): written beside under a name
    /// of its own (this user's alone, never one that is there, never through a link), given
    /// `was`'s permission bits (not set-user or set-group ID) and times, then linked in under
    /// `name` only while nothing is there. Stopped half way, nothing is left. What it is now.
    pub fn create_file(&self, name: &OsStr, from: &mut std::fs::File, was: &Stat, stop: &AtomicBool) -> std::io::Result<Stat> {
        let n = c(name)?;
        let tmp = temp_name(name);
        let dir = self.fd.as_raw_fd();
        // SAFETY: an open folder and a NUL-terminated name; a new file only.
        let fd = unsafe { libc::openat(dir, tmp.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600 as libc::c_uint) };
        let mut f = std::fs::File::from(owned(fd)?);
        let done = (|| {
            let mut buf = vec![0u8; 1 << 20];
            // At most the size it had, and one byte more to tell it grew: a file still being
            // written is not copied without end, nor torn.
            let mut left = was.size + 1;
            while left > 0 {
                if stop.load(Ordering::Relaxed) {
                    return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                }
                let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                let k = read_full(from, &mut buf[..want])?;
                if k == 0 {
                    break;
                }
                f.write_all(&buf[..k])?;
                left -= k as u64;
            }
            if fstat(from.as_raw_fd())?.id != was.id || fstat(f.as_raw_fd())?.size != was.size {
                return Err(changed());
            }
            set_owner_bits(f.as_raw_fd(), was)?;
            set_times(f.as_raw_fd(), was)?;
            self.link_in(&tmp, &n)
        })();
        // SAFETY: the name beside, in the held folder: gone whether it was linked in or not.
        unsafe { libc::unlinkat(dir, tmp.as_ptr(), 0) };
        done?;
        fstat(f.as_raw_fd())
    }

    /// `tmp` here given the name `name` too, never over what is there: a hard link where the
    /// file system has them, else a rename after `name` was found free.
    fn link_in(&self, tmp: &CStr, name: &CStr) -> std::io::Result<()> {
        let dir = self.fd.as_raw_fd();
        // SAFETY: both names in the one held folder; a link never follows one.
        if unsafe { libc::linkat(dir, tmp.as_ptr(), dir, name.as_ptr(), 0) } == 0 {
            return Ok(());
        }
        let e = last();
        if e.raw_os_error() == Some(libc::EEXIST) {
            return Err(other(t!("sync.taken")));
        }
        // No hard links here (FAT, some network file systems): renamed in, while the name is free.
        if !e.raw_os_error().is_some_and(|n| [libc::EPERM, libc::ENOTSUP, libc::EOPNOTSUPP, libc::EMLINK, libc::ENOSYS].contains(&n)) {
            return Err(e);
        }
        rename_new(dir, tmp, dir, name)
    }

    /// A new, empty folder `name` here, held open: this user's, made here and nothing else (one
    /// that is there, or swapped in since, is refused). Its bits and times come last
    /// (`finish`), once what goes into it is in.
    pub fn make_dir(&self, name: &OsStr) -> std::io::Result<Dir> {
        let n = c(name)?;
        // SAFETY: an open folder and a NUL-terminated name.
        if unsafe { libc::mkdirat(self.fd.as_raw_fd(), n.as_ptr(), 0o700) } != 0 {
            let e = last();
            return Err(if e.raw_os_error() == Some(libc::EEXIST) { other(t!("sync.taken")) } else { e });
        }
        let opened = (|| {
            // SAFETY: as above; never a link, never blocking.
            let fd = owned(unsafe { libc::openat(self.fd.as_raw_fd(), n.as_ptr(), DIR_FLAGS) })?;
            let s = fstat(fd.as_raw_fd())?;
            let d = Dir { fd };
            // SAFETY: geteuid has no preconditions.
            if s.kind != Kind::Dir || s.uid != unsafe { libc::geteuid() } || !d.names()?.is_empty() {
                return Err(changed());
            }
            Ok(d)
        })();
        if opened.is_err() {
            // The folder just made is not left behind: only an empty one of this user's goes.
            // SAFETY: geteuid has no preconditions.
            if self.stat_at(name).is_ok_and(|s| s.kind == Kind::Dir && s.uid == unsafe { libc::geteuid() }) {
                // SAFETY: an open folder and a NUL-terminated name; only an empty folder goes.
                unsafe { libc::unlinkat(self.fd.as_raw_fd(), n.as_ptr(), libc::AT_REMOVEDIR) };
            }
        }
        opened
    }

    /// This folder (one `make_dir` made) given `was`'s bits and times, now that it is filled.
    pub fn finish(&self, was: &Stat) -> std::io::Result<()> {
        set_owner_bits(self.fd.as_raw_fd(), was)?;
        set_times(self.fd.as_raw_fd(), was)
    }

    /// A new link `name` here pointing at `to` (never followed), only while nothing is there.
    pub fn make_link(&self, name: &OsStr, to: &OsStr) -> std::io::Result<Stat> {
        let n = c(name)?;
        let to = CString::new(to.as_bytes()).map_err(std::io::Error::other)?;
        // SAFETY: a NUL-terminated target, an open folder and a NUL-terminated name.
        if unsafe { libc::symlinkat(to.as_ptr(), self.fd.as_raw_fd(), n.as_ptr()) } != 0 {
            let e = last();
            return Err(if e.raw_os_error() == Some(libc::EEXIST) { other(t!("sync.taken")) } else { e });
        }
        // The link made here, nothing swapped in since: only that is recorded (undo removes it).
        let s = self.stat_at(name)?;
        // SAFETY: geteuid has no preconditions.
        if s.kind != Kind::Link || s.uid != unsafe { libc::geteuid() } {
            return Err(changed());
        }
        Ok(s)
    }

    /// The entry `name` here moved into the folder `to` as `new` (the same disk, renamed, never
    /// copied), only while it is `want` exactly (any kind: it is never opened) and `new` is free
    /// there. Checked again after: should another have slipped in between, it is moved back.
    pub fn move_to(&self, name: &OsStr, want: Id, to: &Dir, new: &OsStr) -> std::io::Result<()> {
        self.move_checked(name, want, false, to, new)
    }

    /// As `move_to`, for undo: the same entry, whatever its ctime (moving it moved that).
    pub fn move_back(&self, name: &OsStr, want: Id, to: &Dir, new: &OsStr) -> std::io::Result<()> {
        self.move_checked(name, want, true, to, new)
    }

    fn move_checked(&self, name: &OsStr, want: Id, same_file: bool, to: &Dir, new: &OsStr) -> std::io::Result<()> {
        let (n, m) = (c(name)?, c(new)?);
        let ok = |s: &Stat| if same_file { s.id.same_file(&want) } else { s.id == want };
        if !ok(&self.stat_at(name)?) {
            return Err(changed());
        }
        rename_new(self.fd.as_raw_fd(), &n, to.fd.as_raw_fd(), &m)?;
        if !to.stat_at(new).is_ok_and(|s| s.id.same_file(&want)) {
            // Swapped in the instant between the check and the rename: put back where it was.
            let _ = rename_new(to.fd.as_raw_fd(), &m, self.fd.as_raw_fd(), &n);
            return Err(changed());
        }
        Ok(())
    }

    /// The entry `name` here removed for good (what a run made, for undo), only while it is the
    /// very one (`same_file`) with the size and time of change it was left with; a folder only
    /// when empty.
    pub fn remove(&self, name: &OsStr, want: &Stat) -> std::io::Result<()> {
        let n = c(name)?;
        let s = self.stat_at(name)?;
        let same = s.id.same_file(&want.id) && s.kind == want.kind && (s.kind == Kind::Dir || (s.size, s.modified) == (want.size, want.modified));
        if !same {
            return Err(changed());
        }
        let flag = if s.kind == Kind::Dir { libc::AT_REMOVEDIR } else { 0 };
        // SAFETY: an open folder and a NUL-terminated name; never follows a link.
        if unsafe { libc::unlinkat(self.fd.as_raw_fd(), n.as_ptr(), flag) } != 0 {
            return Err(last());
        }
        Ok(())
    }
}

/// `from` in the folder `a` renamed to `to` in the folder `b`, never over what is there: in one
/// call where the system has one (Linux, macOS), else after `to` was found free.
fn rename_new(a: RawFd, from: &CStr, b: RawFd, to: &CStr) -> std::io::Result<()> {
    let taken = || other(t!("sync.taken"));
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // The system call itself: the static musl build's C library has no renameat2.
        // SAFETY: two open folders and two NUL-terminated names; never over what is there.
        if unsafe { libc::syscall(libc::SYS_renameat2, a, from.as_ptr(), b, to.as_ptr(), libc::RENAME_NOREPLACE) } == 0 {
            return Ok(());
        }
        let e = last();
        match e.raw_os_error() {
            Some(libc::EEXIST) => return Err(taken()),
            // A file system without it: as elsewhere.
            Some(libc::EINVAL | libc::ENOSYS) => {}
            _ => return Err(e),
        }
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: as above.
        if unsafe { libc::renameatx_np(a, from.as_ptr(), b, to.as_ptr(), libc::RENAME_EXCL) } == 0 {
            return Ok(());
        }
        let e = last();
        match e.raw_os_error() {
            Some(libc::EEXIST) => return Err(taken()),
            Some(libc::ENOTSUP | libc::EINVAL) => {}
            _ => return Err(e),
        }
    }
    // ponytail: check, then rename; a name taken in between is written over (no atomic
    // no-replace rename on the BSDs and illumos).
    // SAFETY: zeroes are a valid stat, which fstatat fills in.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: an open folder, a NUL-terminated name, `st` ours to fill.
    if unsafe { libc::fstatat(b, to.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) } == 0 {
        return Err(taken());
    }
    // SAFETY: two open folders and two NUL-terminated names; rename never follows a link.
    if unsafe { libc::renameat(a, from.as_ptr(), b, to.as_ptr()) } != 0 {
        return Err(last());
    }
    Ok(())
}

/// The group `was` had, then its bits (not set-user or set-group ID); where the group cannot be
/// given (another user's group), no bits for the group, so it reaches no group it did not.
fn set_owner_bits(fd: RawFd, was: &Stat) -> std::io::Result<()> {
    let mut mode = was.mode & 0o1777;
    // SAFETY: an open entry; the owner left as it is.
    let grouped = fstat(fd)?.gid == was.gid || unsafe { libc::fchown(fd, u32::MAX as libc::uid_t, was.gid as libc::gid_t) } == 0;
    if !grouped || fstat(fd)?.gid != was.gid {
        mode &= !0o070;
    }
    // SAFETY: an open entry, and its bits.
    if unsafe { libc::fchmod(fd, mode as libc::mode_t) } != 0 {
        return Err(last());
    }
    Ok(())
}

fn set_times(fd: RawFd, was: &Stat) -> std::io::Result<()> {
    let ts = |(s, n): (i64, u32)| libc::timespec { tv_sec: s as libc::time_t, tv_nsec: n as _ };
    let times = [ts(was.accessed), ts(was.modified)];
    // SAFETY: an open entry, and two times.
    if unsafe { libc::futimens(fd, times.as_ptr()) } != 0 {
        return Err(last());
    }
    Ok(())
}

fn read_full(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

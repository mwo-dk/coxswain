//! Permissions and dates in bulk (docs/files/permissions-and-dates.md), in both apps: on the
//! marked files, and with `recursive` everything in the marked folders. Permission bits are set
//! as chmod takes them (`644`, `u+x,go-w`, `a+X`); on Windows `-w` and `+w` make a file read-only
//! or writable again, `+h` and `-h` hide it or show it. The time of last change (and on Unix of
//! last access) is set to now, to a typed date and time, or to the one of another file.
//!
//! First a plan, every entry that would change with before and after, then the run. Both reach
//! the entries through `pinned` (folder handles, never a link), and the run changes only the
//! very entries the plan saw, unchanged since (same file, same ctime), each through a handle of
//! its own opened in its held folder (`fchmod`, `futimens`). Hard links, pipes and devices are
//! left as they are. One undo puts back the old bits and times of the whole batch.

use crate::pinned::{self, Id, Kind as Entry, Node, Pin, Place, Said, Set, Walk, secs};
use crate::undo::{Kind, Record};
use crate::t;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::SystemTime;

/// The plan shows at most this many rows; the run changes them all.
pub const SHOWN: usize = 2_000;
/// The entries a plan changes at most: past them it stops, and says so.
pub const MOST: usize = 100_000;

/// What to change, as typed.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Spec {
    /// `644`, `u+x,go-w`, `a+X`; on Windows `-w`, `+w`, `+h`, `-h`. Empty: not changed.
    pub mode: String,
    /// `now`, `2026-10-21 14:30` (local time), or a file (its name in `base`, or a path) whose
    /// time is taken. Empty: not changed.
    pub date: String,
    /// The time of last access too (Unix; Windows keeps its own).
    pub accessed: bool,
    /// Into the marked folders, all the way down.
    pub recursive: bool,
}

/// A file's bits and times, before or after.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Was {
    /// Unix permission bits; on Windows its attributes (read-only 1, hidden 2).
    pub mode: u32,
    /// Seconds since 1970, and nanoseconds.
    pub modified: (i64, u32),
    pub accessed: (i64, u32),
}

impl Was {
    pub fn of(s: &pinned::Stat) -> Was {
        Was { mode: s.mode, modified: s.modified, accessed: s.accessed }
    }
}

/// One entry that changes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Row {
    pub path: PathBuf,
    pub is_dir: bool,
    pub before: Was,
    pub after: Was,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Plan {
    /// The first `SHOWN` changes.
    pub rows: Vec<Row>,
    /// How many change in all.
    pub total: usize,
    /// Left out, and why (`path: why`).
    pub refused: Vec<String>,
    pub stopped: bool,
    /// Which plan this is: the page sends it back with the run, which takes only that plan.
    pub token: u64,
    /// What the run takes from the plan, never from the page: where, what, the time worked out
    /// once, and what was seen.
    #[serde(skip)]
    roots: Vec<PathBuf>,
    #[serde(skip)]
    spec: Spec,
    #[serde(skip)]
    at: Option<(i64, u32)>,
    #[serde(skip)]
    set: Arc<Set>,
}

#[derive(Default, Debug)]
pub struct Progress {
    pub files: AtomicUsize,
}

/// A date and time as a person writes it, in local time: `2026-10-21 14:30`, with seconds, or a
/// day alone (midnight).
pub fn parse_date(s: &str) -> Option<SystemTime> {
    use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
    let s = s.trim().replace('T', " ");
    let naive = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"]
        .iter()
        .find_map(|f| NaiveDateTime::parse_from_str(&s, f).ok())
        .or_else(|| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)))?;
    // Within what every system's file times hold (Windows counts from 1601).
    if !(1601..=9999).contains(&chrono::Datelike::year(&naive)) {
        return None;
    }
    Local.from_local_datetime(&naive).earliest().map(SystemTime::from)
}

/// A date for a person: `2026-10-21 14:30:05`, local time.
pub fn show_date((s, n): (i64, u32)) -> String {
    use chrono::{Local, TimeZone};
    Local.timestamp_opt(s, n).single().map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default()
}

/// Bits as `ls` shows them (`rwxr-xr-x`); on Windows the attributes (`read-only, hidden`).
pub fn show_mode(m: u32) -> String {
    if cfg!(windows) {
        let mut v = vec![];
        if m & 1 != 0 {
            v.push(t!("attrs.read_only"));
        }
        if m & 2 != 0 {
            v.push(t!("attrs.hidden"));
        }
        return if v.is_empty() { "–".into() } else { v.join(", ") };
    }
    let mut s = String::new();
    for shift in [6, 3, 0] {
        for (bit, c) in [(4, 'r'), (2, 'w'), (1, 'x')] {
            s.push(if m >> shift & bit != 0 { c } else { '-' });
        }
    }
    if m & 0o7000 != 0 { format!("{:o} {s}", m & 0o7777) } else { s }
}

/// A mode as typed, applied to `old` of a file or (`dir`) a folder; why it is wrong when it is.
pub fn apply_mode(spec: &str, old: u32, dir: bool) -> Result<u32, String> {
    let spec = spec.trim();
    let bad = || t!("attrs.bad_mode", "mode" => spec);
    if cfg!(windows) {
        let mut m = old;
        for part in spec.split(',').map(str::trim) {
            m = match part {
                "-w" => m | 1,
                "+w" => m & !1,
                "+h" => m | 2,
                "-h" => m & !2,
                _ => return Err(bad()),
            };
        }
        return Ok(m);
    }
    if !spec.is_empty() && spec.chars().all(|c| c.is_digit(8)) {
        return u32::from_str_radix(spec, 8).ok().filter(|m| *m <= 0o7777 && spec.len() <= 4).ok_or_else(bad);
    }
    let mut m = old & 0o7777;
    for part in spec.split(',') {
        let at = part.find(['+', '-', '=']).ok_or_else(bad)?;
        let (who, rest) = part.split_at(at);
        let (op, perms) = rest.split_at(1);
        let mut mask = 0;
        for c in if who.is_empty() { "a".chars() } else { who.chars() } {
            mask |= match c {
                'u' => 0o4700,
                'g' => 0o2070,
                'o' => 0o0007,
                'a' => 0o6777,
                _ => return Err(bad()),
            };
        }
        let mut bits = 0;
        for c in perms.chars() {
            bits |= match c {
                'r' => 0o444,
                'w' => 0o222,
                'x' => 0o111,
                // Execute only for folders, and files someone may run already.
                'X' if dir || old & 0o111 != 0 => 0o111,
                'X' => 0,
                's' => 0o6000,
                't' => 0o1000,
                _ => return Err(bad()),
            };
        }
        let bits = bits & (mask | if who.is_empty() || who.contains(['a', 'o']) { 0o1000 } else { 0 });
        m = match op {
            "+" => m | bits,
            "-" => m & !bits,
            // As chmod: `=` for a file clears its setuid and setgid too (a folder keeps setgid).
            _ => (m & !(mask & 0o777) & !(mask & if dir { 0o4000 } else { 0o6000 })) | bits,
        };
    }
    Ok(m)
}

/// The time asked for, worked out once: now, a typed date, or a file's.
fn when(spec: &Spec, base: &Path) -> Result<Option<(i64, u32)>, String> {
    let d = spec.date.trim();
    if d.is_empty() {
        return Ok(None);
    }
    if d.eq_ignore_ascii_case("now") || d == t!("attrs.now") {
        return Ok(Some(secs(SystemTime::now())));
    }
    if let Some(t) = parse_date(d) {
        return Ok(Some(secs(t)));
    }
    let like = base.join(d);
    match std::fs::metadata(&like).and_then(|m| m.modified()) {
        Ok(t) => Ok(Some(secs(t))),
        Err(_) => Err(t!("attrs.bad_date", "date" => d)),
    }
}

fn walk_spec<'a>(spec: &Spec, only: Option<&'a Set>) -> Walk<'a> {
    Walk { recursive: spec.recursive, skip: &[], only }
}

/// What `said` holds, as lines, with "… and N more".
fn lines(said: Said, out: &mut Vec<String>) {
    out.extend(said.lines.into_iter().map(|(p, why)| format!("{}: {why}", p.display())));
    if said.more > 0 {
        out.push(crate::tn!("attrs.more", said.more));
    }
}

/// What `spec` would change under `roots`; `base` is where a file named in the date is.
pub fn plan(roots: &[PathBuf], spec: &Spec, base: &Path, stop: &AtomicBool, progress: &Progress) -> Result<Plan, String> {
    check(spec, base)?;
    let at = when(spec, base)?;
    let mut plan = Plan { roots: roots.to_vec(), spec: spec.clone(), at, token: pinned::token(), ..Default::default() };
    let (mut set, mut said, mut bad, mut n) = (Set::default(), Said::default(), Said::default(), 0);
    pinned::walk(roots, &walk_spec(spec, None), stop, &mut said, |node| {
        n += 1;
        progress.files.store(n, Ordering::Relaxed);
        match refusal(node) {
            Some(why) => bad.say(&node.path, why),
            None if set.has(&node.stat.id) => {}
            None => {
                let before = Was::of(&node.stat);
                let is_dir = node.stat.kind == Entry::Dir;
                match after(spec, at, before, is_dir) {
                    Ok(a) if a != before => {
                        set.add(node);
                        plan.total += 1;
                        if plan.rows.len() < SHOWN {
                            plan.rows.push(Row { path: node.path.clone(), is_dir, before, after: a });
                        }
                        if plan.total >= MOST {
                            bad.say(&node.path, t!("pinned.full", "n" => MOST));
                            return false;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => bad.say(&node.path, e),
                }
            }
        }
        true
    });
    lines(said, &mut plan.refused);
    lines(bad, &mut plan.refused);
    plan.stopped = stop.load(Ordering::Relaxed);
    plan.set = Arc::new(set);
    Ok(plan)
}

/// Why `node` is left as it is, if it is: a link (never followed nor changed), a pipe, device or
/// socket (never opened), a file with other names (changing it would change it under those too,
/// outside what was chosen).
fn refusal(node: &Node) -> Option<String> {
    match node.stat.kind {
        Entry::Link => Some(t!("pinned.link")),
        Entry::Other => Some(t!("pinned.special")),
        Entry::File if node.stat.nlink > 1 => Some(t!("pinned.hard_link")),
        _ => None,
    }
}

/// Give the entry at `place` the bits and times of `w` (it had `old`), through one handle opened
/// in its held folder and checked to be `want` exactly: the one way apply and undo both change
/// an entry. What it is afterwards, for undo.
fn set_at(place: &Place, want: Id, old: Was, w: Was) -> std::io::Result<Id> {
    let (f, now) = match place.open_to_change(want) {
        Ok(x) => x,
        // One the owner may not even read (`chmod 000`): on Linux through a path handle instead.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        Err(e) if e.to_string() == t!("pinned.no_open") => return set_through_path(place, want, old, w),
        Err(e) => return Err(e),
    };
    if now.kind != Entry::Dir && now.nlink > 1 {
        return Err(std::io::Error::other(t!("pinned.hard_link")));
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let last = std::io::Error::last_os_error;
        // SAFETY: the open entry and its new bits.
        if w.mode != now.mode && unsafe { libc::fchmod(f.as_raw_fd(), w.mode as libc::mode_t) } != 0 {
            return Err(last());
        }
        // Only the times that change; the others are left (UTIME_OMIT).
        let ts = |(s, n): (i64, u32), keep: bool| libc::timespec { tv_sec: s as libc::time_t, tv_nsec: if keep { libc::UTIME_OMIT } else { n as _ } };
        let times = [ts(w.accessed, w.accessed == old.accessed), ts(w.modified, w.modified == old.modified)];
        // SAFETY: the open entry, and two times.
        if (w.modified != old.modified || w.accessed != old.accessed) && unsafe { libc::futimens(f.as_raw_fd(), times.as_ptr()) } != 0 {
            return Err(last());
        }
        // Settled before it is read for undo: some file systems set the ctime lazily (UFS).
        let _ = f.sync_all();
        Ok(pinned::stat_of(&f)?.id)
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, FILE_BASIC_INFO, FileBasicInfo, SetFileInformationByHandle};
        if w.mode != now.mode {
            let mut a = std::os::windows::fs::MetadataExt::file_attributes(&f.metadata()?) & !(FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_HIDDEN);
            if w.mode & 1 != 0 {
                a |= FILE_ATTRIBUTE_READONLY;
            }
            if w.mode & 2 != 0 {
                a |= FILE_ATTRIBUTE_HIDDEN;
            }
            if a == 0 {
                a = FILE_ATTRIBUTE_NORMAL;
            }
            // Times of 0 are left as they are; only the attributes change.
            let info = FILE_BASIC_INFO { CreationTime: 0, LastAccessTime: 0, LastWriteTime: 0, ChangeTime: 0, FileAttributes: a };
            // SAFETY: the handle is open for this call, `info` lives through it and its size is given.
            let ok = unsafe { SetFileInformationByHandle(f.as_raw_handle() as _, FileBasicInfo, std::ptr::from_ref(&info).cast(), std::mem::size_of::<FILE_BASIC_INFO>() as u32) };
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        if w.modified != old.modified || w.accessed != old.accessed {
            let time = |(s, n): (i64, u32)| {
                let t = if s >= 0 { SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::new(s as u64, n)) } else { SystemTime::UNIX_EPOCH.checked_sub(std::time::Duration::new(s.unsigned_abs(), 0)) };
                t.ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))
            };
            let mut t = std::fs::FileTimes::new();
            if w.modified != old.modified {
                t = t.set_modified(time(w.modified)?);
            }
            if w.accessed != old.accessed {
                t = t.set_accessed(time(w.accessed)?);
            }
            f.set_times(t)?;
        }
        // What it is now, from the very handle it was changed through.
        Ok(pinned::stat_of(&f)?.id)
    }
}

/// As `set_at`, for an entry that cannot be opened to read: an `O_PATH` handle opened in its
/// held folder without following a link, checked to be `want` exactly, and changed through
/// `/proc/self/fd`, which leads to that very inode.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn set_through_path(place: &Place, want: Id, old: Was, w: Was) -> std::io::Result<Id> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let last = std::io::Error::last_os_error;
    let (dir, name) = place.at();
    // SAFETY: an open folder and a NUL-terminated name; a path handle only, never a link followed.
    let fd = unsafe { libc::openat(dir, name.as_ptr(), libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(last());
    }
    // SAFETY: just opened, owned here.
    let f = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
    let now = pinned::stat_of(&f)?;
    if !matches!(now.kind, Entry::File | Entry::Dir) || now.id != want {
        return Err(pinned::changed());
    }
    if now.kind != Entry::Dir && now.nlink > 1 {
        return Err(std::io::Error::other(t!("pinned.hard_link")));
    }
    // Only a real /proc leads to the handle's own inode; anything else at that path is refused.
    // SAFETY: zeroes are a valid statfs, which the call fills in.
    let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: a NUL-terminated path and `fs` ours to fill.
    if unsafe { libc::statfs(c"/proc".as_ptr(), &mut fs) } != 0 || fs.f_type as i64 != 0x9fa0 {
        return Err(std::io::Error::other(t!("pinned.no_open")));
    }
    let proc = std::ffi::CString::new(format!("/proc/self/fd/{}", f.as_raw_fd())).map_err(std::io::Error::other)?;
    // SAFETY: the path of the handle just checked, and its new bits.
    if w.mode != now.mode && unsafe { libc::chmod(proc.as_ptr(), w.mode as libc::mode_t) } != 0 {
        return Err(last());
    }
    let ts = |(s, n): (i64, u32), keep: bool| libc::timespec { tv_sec: s as libc::time_t, tv_nsec: if keep { libc::UTIME_OMIT } else { n as _ } };
    let times = [ts(w.accessed, w.accessed == old.accessed), ts(w.modified, w.modified == old.modified)];
    // SAFETY: as above, and two times.
    if (w.modified != old.modified || w.accessed != old.accessed) && unsafe { libc::utimensat(libc::AT_FDCWD, proc.as_ptr(), times.as_ptr(), 0) } != 0 {
        return Err(last());
    }
    Ok(pinned::stat_of(&f)?.id)
}

/// Undo: give the entry at `path`, in the folder and as the very file `pin` names (unchanged
/// since), back what the change changed: of `before` → `after`, only the fields that differ.
pub(crate) fn undo(path: &Path, pin: Pin, before: Was, after: Was) -> std::io::Result<()> {
    let place = Place::reopen(path, pin.dir)?;
    let now = place.stat()?;
    if now.id != pin.file {
        return Err(pinned::changed());
    }
    let now = Was::of(&now);
    fn pick<T: PartialEq>(b: T, a: T, n: T) -> T {
        if b != a { b } else { n }
    }
    let back = Was { mode: pick(before.mode, after.mode, now.mode), modified: pick(before.modified, after.modified, now.modified), accessed: pick(before.accessed, after.accessed, now.accessed) };
    set_at(&place, pin.file, now, back).map(drop)
}

/// What a run did, and the record that undoes it.
pub struct Applied {
    pub changed: usize,
    pub failed: Vec<String>,
    pub record: Record,
}

impl Applied {
    pub fn summary(&self) -> String {
        crate::tn!("attrs.done", self.changed)
    }
}

/// Make the changes of `plan`, each entry as the plan saw it: one changed since (written,
/// swapped, moved in) is left as it is, and counted.
pub fn apply(plan: &Plan, stop: &AtomicBool, progress: &Progress) -> Result<Applied, String> {
    let mut done = Applied { changed: 0, failed: vec![], record: Record::new(Kind::Attrs) };
    let (mut said, mut bad, mut n) = (Said::default(), Said::default(), 0);
    let met = pinned::walk(&plan.roots, &walk_spec(&plan.spec, Some(&plan.set)), stop, &mut said, |node| {
        n += 1;
        progress.files.store(n, Ordering::Relaxed);
        let before = Was::of(&node.stat);
        let r = after(&plan.spec, plan.at, before, node.stat.kind == Entry::Dir)
            .and_then(|a| if a == before { Ok(None) } else { set_at(&node.place, node.stat.id, before, a).map(|id| Some((a, id))).map_err(|e| e.to_string()) });
        match r {
            Ok(Some((a, id))) => {
                done.record.done_attrs(&node.path, before, a, Pin { dir: node.place.dir_id(), file: id, sum: None });
                done.changed += 1;
            }
            Ok(None) => {}
            Err(e) => bad.say(&node.path, e),
        }
        true
    });
    lines(said, &mut done.failed);
    lines(bad, &mut done.failed);
    let missed = plan.set.len().saturating_sub(met);
    if missed > 0 && !stop.load(Ordering::Relaxed) {
        done.failed.push(t!("pinned.missed", "n" => missed));
    }
    Ok(done)
}

/// What `spec` makes of a file that was `before`.
fn after(spec: &Spec, at: Option<(i64, u32)>, before: Was, dir: bool) -> Result<Was, String> {
    let mut a = before;
    if !spec.mode.trim().is_empty() {
        a.mode = apply_mode(&spec.mode, before.mode, dir)?;
    }
    if let Some(at) = at {
        a.modified = at;
        if spec.accessed {
            a.accessed = at;
        }
    }
    Ok(a)
}

/// Check what was typed, before anything runs: the mode on a made-up file, and the date.
pub fn check(spec: &Spec, base: &Path) -> Result<(), String> {
    if spec.mode.trim().is_empty() && spec.date.trim().is_empty() {
        return Err(t!("attrs.nothing"));
    }
    if !spec.mode.trim().is_empty() {
        apply_mode(&spec.mode, 0o644, false)?;
    }
    when(spec, base).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_of(p: &Path) -> Was {
        let m = std::fs::symlink_metadata(p).unwrap();
        #[cfg(unix)]
        let mode = std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o7777;
        #[cfg(windows)]
        let mode = std::os::windows::fs::MetadataExt::file_attributes(&m) & 3;
        Was { mode, modified: secs(m.modified().unwrap()), accessed: secs(m.accessed().unwrap()) }
    }

    fn plan_of(roots: &[PathBuf], spec: &Spec, base: &Path) -> Plan {
        plan(roots, spec, base, &AtomicBool::new(false), &Progress::default()).unwrap()
    }

    fn apply_all(plan: &Plan) -> Applied {
        apply(plan, &AtomicBool::new(false), &Progress::default()).unwrap()
    }

    #[cfg(unix)]
    fn mode(p: &Path) -> u32 {
        now_of(p).mode & 0o777
    }

    #[cfg(unix)]
    fn chmod(p: &Path, m: u32) {
        std::fs::set_permissions(p, std::os::unix::fs::PermissionsExt::from_mode(m)).unwrap();
    }

    /// Time enough for a file system's timestamps to move on.
    #[cfg(unix)]
    fn tick() {
        std::thread::sleep(std::time::Duration::from_millis(30));
    }

    #[cfg(unix)]
    #[test]
    fn modes_as_chmod_takes_them() {
        assert_eq!(apply_mode("644", 0o755, false), Ok(0o644));
        assert_eq!(apply_mode("u+x,go-w", 0o666, false), Ok(0o744));
        assert_eq!(apply_mode("a+X", 0o644, true), Ok(0o755));
        assert_eq!(apply_mode("a+X", 0o644, false), Ok(0o644));
        assert_eq!(apply_mode("+x", 0o600, false), Ok(0o711));
        assert_eq!(apply_mode("o=r", 0o777, false), Ok(0o774));
        assert_eq!(apply_mode("u+s", 0o755, false), Ok(0o4755));
        assert_eq!(apply_mode("u=rwx,go=rx", 0o6755, false), Ok(0o755));
        assert_eq!(apply_mode("g=rx", 0o2775, true), Ok(0o2755));
        assert!(apply_mode("q+x", 0o644, false).is_err());
        assert!(apply_mode("999", 0o644, false).is_err());
        assert_eq!(show_mode(0o754), "rwxr-xr--");
    }

    #[test]
    fn dates_as_people_write_them() {
        let a = parse_date("2026-10-21 14:30").unwrap();
        assert_eq!(show_date(secs(a)), "2026-10-21 14:30:00");
        assert!(parse_date("2026-10-21").is_some());
        assert!(parse_date("21/10/2026").is_none());
        assert!(parse_date("1600-01-01").is_none());
    }

    #[test]
    fn plan_apply_and_undo_a_batch() {
        let d = crate::fs::Scratch::named("attrs-batch");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        for f in ["a", "sub/b"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(d.join("a"), d.join("link")).unwrap();
        let spec = Spec { mode: if cfg!(windows) { "-w".into() } else { "u+x".into() }, date: "2020-01-02 03:04".into(), accessed: true, recursive: true };
        let roots = vec![d.to_path_buf()];
        let plan = plan_of(&roots, &spec, &d);
        assert_eq!(plan.total, 4, "{plan:?}");
        #[cfg(unix)]
        assert_eq!(plan.refused.len(), 1);
        let before = now_of(&d.join("sub/b"));
        let done = apply_all(&plan);
        assert_eq!((done.changed, done.failed.len()), (plan.total, 0), "{:?}", done.failed);
        let now = now_of(&d.join("sub/b"));
        assert_eq!(show_date(now.modified), "2020-01-02 03:04:00");
        assert_ne!(now.mode, before.mode);
        // A file changed since is left as it is; the rest goes back.
        #[cfg(windows)]
        {
            let mut perm = std::fs::metadata(d.join("a")).unwrap().permissions();
            perm.set_readonly(false);
            std::fs::set_permissions(d.join("a"), perm).unwrap();
        }
        #[cfg(unix)]
        tick();
        std::fs::write(d.join("a"), "changed").unwrap();
        let undone = done.record.undo(|_| {});
        assert_eq!((undone.done, undone.refused.len()), (plan.total - 1, 1), "{:?}", undone.refused);
        assert_eq!(now_of(&d.join("sub/b")).mode, before.mode);
        assert_eq!(now_of(&d.join("sub/b")).modified, before.modified);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_swapped_in_after_the_plan_is_not_followed() {
        let (d, out) = (crate::fs::Scratch::named("attrs-swap"), crate::fs::Scratch::named("attrs-swap-out"));
        let (a, far) = (d.join("a"), out.join("far"));
        std::fs::write(&a, "x").unwrap();
        std::fs::write(&far, "x").unwrap();
        chmod(&far, 0o600);
        let plan = plan_of(&[a.clone()], &Spec { mode: "777".into(), ..Default::default() }, &d);
        std::fs::remove_file(&a).unwrap();
        std::os::unix::fs::symlink(&far, &a).unwrap();
        let done = apply_all(&plan);
        assert_eq!((done.changed, done.failed.len()), (0, 1), "{:?}", done.failed);
        assert_eq!(mode(&far), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_for_a_link_leads_nowhere_and_undo_keeps_to_its_folder() {
        let (d, out) = (crate::fs::Scratch::named("attrs-dirswap"), crate::fs::Scratch::named("attrs-dirswap-out"));
        std::fs::create_dir(d.join("sub")).unwrap();
        for f in [d.join("sub/f"), out.join("f")] {
            std::fs::write(&f, "x").unwrap();
            chmod(&f, 0o600);
        }
        // Group may read: the files change, the folders (which have it) do not.
        let spec = Spec { mode: "g+r".into(), recursive: true, ..Default::default() };
        let plan = plan_of(&[d.join("sub")], &spec, &d);
        std::fs::rename(d.join("sub"), d.join("moved")).unwrap();
        std::os::unix::fs::symlink(&*out, d.join("sub")).unwrap();
        assert_eq!(apply_all(&plan).changed, 0);
        assert_eq!(mode(&out.join("f")), 0o600);
        assert_eq!(mode(&d.join("moved/f")), 0o600);
        // Changed where it is, then its folder swapped for a link: undo goes nowhere.
        let done = apply_all(&plan_of(&[d.join("moved")], &spec, &d));
        assert_eq!(mode(&d.join("moved/f")), 0o640);
        std::fs::rename(d.join("moved"), d.join("moved2")).unwrap();
        std::os::unix::fs::symlink(&*out, d.join("moved")).unwrap();
        let undone = done.record.undo(|_| {});
        assert_eq!(undone.done, 0);
        assert_eq!(mode(&out.join("f")), 0o600);
        assert_eq!(mode(&d.join("moved2/f")), 0o640);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_renamed_in_after_the_plan_is_not_gone_into() {
        let (d, out) = (crate::fs::Scratch::named("attrs-renamed-in"), crate::fs::Scratch::named("attrs-renamed-in-out"));
        for root in [&d, &out] {
            std::fs::create_dir(root.join("sub")).unwrap();
            std::fs::write(root.join("sub/f"), "x").unwrap();
            chmod(&root.join("sub/f"), 0o600);
        }
        let plan = plan_of(&[d.to_path_buf()], &Spec { mode: "g+r".into(), recursive: true, ..Default::default() }, &d);
        std::fs::rename(d.join("sub"), out.join("was")).unwrap();
        std::fs::rename(out.join("sub"), d.join("sub")).unwrap();
        let done = apply_all(&plan);
        assert_eq!(done.changed, 0);
        assert_eq!(done.failed, [t!("pinned.missed", "n" => plan.total)]);
        assert_eq!(mode(&d.join("sub/f")), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn pipes_hard_links_and_files_that_cannot_be_opened_are_left_out() {
        let (d, out) = (crate::fs::Scratch::named("attrs-hard"), crate::fs::Scratch::named("attrs-hard-out"));
        std::fs::write(out.join("f"), "x").unwrap();
        chmod(&out.join("f"), 0o600);
        std::fs::hard_link(out.join("f"), d.join("f")).unwrap();
        std::fs::write(d.join("locked"), "x").unwrap();
        chmod(&d.join("locked"), 0o000);
        let fifo = std::ffi::CString::new(d.join("pipe").as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let spec = Spec { mode: "u+rw".into(), recursive: true, ..Default::default() };
        let plan = plan_of(&[d.to_path_buf()], &spec, &d);
        assert!(plan.refused.iter().any(|r| r.contains(&t!("pinned.hard_link"))), "{:?}", plan.refused);
        assert!(plan.refused.iter().any(|r| r.contains(&t!("pinned.special"))), "{:?}", plan.refused);
        let done = apply_all(&plan);
        assert_eq!(mode(&out.join("f")), 0o600);
        // One that cannot be opened is left as it is (root opens it anyway).
        // SAFETY: geteuid has no preconditions.
        if cfg!(target_os = "linux") {
            // Through a path handle: it can be opened up again.
            assert_eq!(mode(&d.join("locked")), 0o600, "{:?}", done.failed);
        } else if unsafe { libc::geteuid() } != 0 {
            assert!(done.failed.iter().any(|f| f.contains(&t!("pinned.no_open"))), "{:?}", done.failed);
            assert_eq!(mode(&d.join("locked")), 0);
        }
        chmod(&d.join("locked"), 0o600);
        // A file swapped for a pipe after the plan: never opened, never waited on.
        std::fs::write(d.join("g"), "x").unwrap();
        let plan = plan_of(&[d.join("g")], &Spec { mode: "640".into(), ..Default::default() }, &d);
        std::fs::remove_file(d.join("g")).unwrap();
        std::fs::rename(d.join("pipe"), d.join("g")).unwrap();
        assert_eq!(apply_all(&plan).changed, 0);
    }

    #[cfg(unix)]
    #[test]
    fn undo_gives_back_only_what_the_change_changed_and_only_while_unchanged() {
        let d = crate::fs::Scratch::named("attrs-partial");
        std::fs::write(d.join("a"), "x").unwrap();
        chmod(&d.join("a"), 0o644);
        let done = apply_all(&plan_of(&[d.join("a")], &Spec { date: "2020-01-02 03:04".into(), ..Default::default() }, &d));
        assert_eq!(done.changed, 1);
        // Locked down since: undo must not open it up again.
        tick();
        chmod(&d.join("a"), 0o600);
        assert_eq!(done.record.undo(|_| {}).refused.len(), 1);
        assert_eq!(mode(&d.join("a")), 0o600);
        // Unchanged since: the date goes back, the bits stay as they are.
        let before = now_of(&d.join("a"));
        let done = apply_all(&plan_of(&[d.join("a")], &Spec { date: "2021-01-02 03:04".into(), ..Default::default() }, &d));
        assert_eq!(done.record.undo(|_| {}).done, 1);
        assert_eq!(now_of(&d.join("a")).modified, before.modified);
        assert_eq!(mode(&d.join("a")), 0o600);
        // An entry that changed after the plan is not changed by the run.
        let plan = plan_of(&[d.join("a")], &Spec { mode: "644".into(), ..Default::default() }, &d);
        tick();
        chmod(&d.join("a"), 0o640);
        let done = apply_all(&plan);
        assert_eq!((done.changed, done.failed.len()), (0, 1));
        assert_eq!(mode(&d.join("a")), 0o640);
    }

    #[test]
    fn a_file_date_is_taken_and_nonsense_is_refused() {
        let d = crate::fs::Scratch::named("attrs-like");
        std::fs::write(d.join("old"), "x").unwrap();
        std::fs::write(d.join("new"), "x").unwrap();
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        std::fs::File::options().write(true).open(d.join("old")).unwrap().set_times(std::fs::FileTimes::new().set_modified(t).set_accessed(t)).unwrap();
        apply_all(&plan_of(&[d.join("new")], &Spec { date: "old".into(), ..Default::default() }, &d));
        assert_eq!(now_of(&d.join("new")).modified, (1_000_000_000, 0));
        assert!(check(&Spec { date: "nonsense".into(), ..Default::default() }, &d).is_err());
        assert!(check(&Spec::default(), &d).is_err());
    }
}

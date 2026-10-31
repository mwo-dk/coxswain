//! The system's bin (en-CA: trash) as a place the panels show: `<home>/@bin` lists what is in
//! it, each entry by its real path inside the bin (so the preview, F3 and Copy work), with the
//! folder it was deleted from and the date it was deleted. Its files are restored, deleted for
//! good, or all emptied, in both apps.
//!
//! - Linux, the BSDs, illumos and Termux: the freedesktop.org bin, the home one
//!   (`$XDG_DATA_HOME/Trash`) and each disk's (`.Trash/<uid>`, `.Trash-<uid>`): `files/<name>`
//!   with its `info/<name>.trashinfo`. In a Flatpak only the app's own home bin.
//! - Windows: the Recycle Bin of every drive, through the shell (`trash::os_limited`).
//! - macOS: `~/.Trash`, which needs Full Disk Access to be listed; the system gives apps no way
//!   to put a file back, and the whole bin is emptied by Finder.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;

use crate::fs::Entry;

/// The last segment of the bin's path: `<home>/@bin`.
pub const MARKER: &str = "@bin";

/// Where the panels show the bin.
pub fn path() -> Option<PathBuf> {
    crate::dirs::home_dir().map(|h| h.join(MARKER))
}

/// Whether `dir` is the bin's path (and not a real folder of that name).
pub fn is_bin(dir: &Path) -> bool {
    dir.file_name().is_some_and(|n| n == MARKER)
        && path().is_some_and(|p| p == dir)
        && fs::symlink_metadata(dir).is_err()
}

/// Whether `path` is the bin's path or under it: nothing is put there, as a real `@bin` folder
/// made by a copy would hide the bin from then on.
pub fn inside(path: &Path) -> bool {
    path.ancestors().any(is_bin)
}

/// Where `..` leads from `dir` when it is an item of a freedesktop bin (`<bin>/files/<item>`):
/// back to the bin.
pub fn up_from(dir: &Path) -> Option<PathBuf> {
    let files = dir.parent()?;
    let bin = files.parent()?;
    let name = bin.file_name()?.to_string_lossy();
    let named = name == "Trash"
        || name.starts_with(".Trash-")
        || bin
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n == ".Trash");
    (files.file_name()? == "files" && named && bin.join("info").is_dir()).then(path)?
}

/// One thing in the bin.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Its name where it came from.
    pub name: String,
    /// Where it is now, inside the bin.
    pub path: PathBuf,
    /// Where it was: the folder and the name. Empty where the system does not say (macOS).
    pub origin: PathBuf,
    /// When it was deleted, in seconds since the epoch.
    pub deleted: i64,
    pub is_dir: bool,
    pub is_symlink: bool,
    /// Its bytes, a folder's all of them.
    pub size: u64,
}

/// Files can be put back where they were: not on a Mac.
pub fn restores() -> bool {
    !cfg!(any(target_os = "macos", target_os = "ios"))
}

/// Set when the bin was looked at where Coxswain sees only part of it, or none: the notice
/// says why, once.
pub static LIMITED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What is in the bin, newest first.
pub fn list() -> io::Result<Vec<Item>> {
    if limited_here() {
        LIMITED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let mut items = sys::list()?;
    items.sort_by(|a, b| b.deleted.cmp(&a.deleted).then_with(|| a.name.cmp(&b.name)));
    Ok(items)
}

/// A Flatpak sees its own bin only; a Mac's bin is the user's but cannot be put back from.
fn limited_here() -> bool {
    crate::tools::flatpak().is_some() || !restores()
}

/// The bin as `fs::list` gives a folder: `..` back home, then each item.
pub fn entries(show_hidden: bool) -> io::Result<Vec<Entry>> {
    let up = Entry {
        name: "..".into(),
        path: crate::dirs::home_dir().unwrap_or_default(),
        is_dir: true,
        ..Entry::default()
    };
    Ok(std::iter::once(up)
        .chain(list()?.into_iter().filter_map(|i| {
            let hidden = i.name.starts_with('.');
            (show_hidden || !hidden).then(|| Entry {
                hidden,
                is_dir: i.is_dir,
                is_symlink: i.is_symlink,
                // A folder's size is measured, as on disk.
                size: if i.is_dir { 0 } else { i.size },
                modified: i.deleted.max(0) as u64,
                origin: i.origin.parent().map(Path::to_path_buf).unwrap_or_default(),
                name: i.name,
                path: i.path,
                ..Entry::default()
            })
        }))
        .collect())
}

/// Find in the bin: every word of `query` in an item's name, case aside; the snippet says
/// where it was deleted from.
pub fn find(query: &str, max: usize) -> crate::find::Group {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let hits: Vec<crate::index::Hit> = list()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| {
            let low = i.name.to_lowercase();
            !words.is_empty() && words.iter().all(|w| low.contains(w.as_str()))
        })
        .map(|i| crate::index::Hit {
            is_dir: i.is_dir,
            snippet: Some(i.origin.display().to_string()).filter(|s| !s.is_empty()),
            path: i.path,
            similar: None,
        })
        .collect();
    let total = hits.len();
    crate::find::Group {
        hits: hits.into_iter().take(max).collect(),
        total,
    }
}

/// What the sidebar's row says: how many items, their bytes, and whether the bin can be
/// listed here (`false`: a Mac without Full Disk Access, where the row opens Finder).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Summary {
    pub count: usize,
    pub bytes: u64,
    pub open: bool,
    /// Files can be put back (`restores`).
    pub restores: bool,
    /// The bin's path in the panels (`path`).
    pub path: PathBuf,
    /// The folders that change when something goes in or out: watched where they can be.
    pub watch: Vec<PathBuf>,
}

pub fn summary() -> Summary {
    let path = path().unwrap_or_default();
    match list() {
        Ok(items) => Summary {
            count: items.len(),
            bytes: items.iter().map(|i| i.size).sum(),
            open: true,
            restores: restores(),
            path,
            watch: sys::watch(),
        },
        Err(_) => Summary {
            path,
            ..Summary::default()
        },
    }
}

/// The row's text: "3 items · 1.2 MB", or "Empty".
pub fn summary_text(s: &Summary) -> String {
    if s.count == 0 {
        crate::t!("bin.empty")
    } else {
        format!(
            "{} · {}",
            crate::tn!("items", s.count),
            crate::settings::human(s.bytes)
        )
    }
}

/// A folder's bytes, measured once per item: the bin is listed every half minute.
fn measured(path: &Path, deleted: i64) -> u64 {
    static SIZES: Mutex<Option<HashMap<(PathBuf, i64), u64>>> = Mutex::new(None);
    let key = (path.to_path_buf(), deleted);
    if let Some(n) = SIZES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_default()
        .get(&key)
    {
        return *n;
    }
    let n = crate::fs::dir_size(path).0;
    SIZES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_default()
        .insert(key, n);
    n
}

fn sized(path: &Path, deleted: i64) -> (bool, bool, u64) {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => (true, false, measured(path, deleted)),
        Ok(m) => (false, m.file_type().is_symlink(), m.len()),
        Err(_) => (false, false, 0),
    }
}

/// What a question is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Restore,
    Delete,
    Empty,
}

/// The question asked before `op` on `paths` (the bin's items; all of them to empty it):
/// how many, their size, and that it cannot be undone. `None` when none of them is in the bin.
pub fn question(op: Op, paths: &[PathBuf]) -> Option<String> {
    let all = list().ok()?;
    let chosen: Vec<&Item> = all
        .iter()
        .filter(|i| op == Op::Empty || paths.contains(&i.path))
        .collect();
    if chosen.is_empty() {
        return None;
    }
    let what = if chosen.len() == 1 {
        chosen[0].name.clone()
    } else {
        crate::tn!("items", chosen.len())
    };
    let size = crate::settings::human(chosen.iter().map(|i| i.size).sum());
    Some(match op {
        Op::Restore => crate::t!("bin.ask_restore", "what" => what, "size" => size),
        Op::Delete => crate::t!("bin.ask_delete", "what" => what, "size" => size),
        // Finder empties every disk's bin, not only the one listed (`~/.Trash`).
        Op::Empty if cfg!(target_os = "macos") => format!(
            "{} {}",
            crate::t!("bin.ask_empty", "what" => what, "size" => size),
            crate::t!("bin.finder_all_disks")
        ),
        Op::Empty => crate::t!("bin.ask_empty", "what" => what, "size" => size),
    })
}

/// What a restore did: how many came back, the items whose place is taken (left in the bin,
/// to be asked about), and what failed, each with its name.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Done {
    pub done: usize,
    pub taken: Vec<PathBuf>,
    pub failed: Vec<String>,
}

/// Put `paths` (items of the bin) back where they were. One whose place is taken stays, and is
/// listed in `taken`; with `free_name` it comes back beside it, as `name (2)`.
pub fn restore(paths: &[PathBuf], free_name: bool) -> Done {
    if !restores() {
        return Done {
            failed: vec![crate::t!("bin.no_restore")],
            ..Done::default()
        };
    }
    sys::restore(paths, free_name)
}

/// Delete `paths` (items of the bin) for good; how many went, and what failed.
pub fn delete(paths: &[PathBuf]) -> Done {
    sys::delete(paths)
}

/// Everything in the bin deleted for good.
pub fn empty() -> Done {
    sys::empty()
}

/// The free name for `name` in `dir`: itself, else `name (2)`, `name (3)` …
fn free_in(dir: &Path, name: &str) -> PathBuf {
    crate::fs::name_candidates(name)
        .map(|n| dir.join(n))
        .find(|p| fs::symlink_metadata(p).is_err())
        .expect("some name is free")
}

fn failed(path: &Path, e: impl std::fmt::Display) -> String {
    format!(
        "{}: {e}",
        path.file_name().unwrap_or_default().to_string_lossy()
    )
}

// ---------------------------------------------------------------- freedesktop.org

/// The freedesktop.org bin: Linux, the BSDs, illumos, Termux.
#[cfg_attr(any(windows, target_os = "macos", target_os = "ios"), allow(dead_code))]
pub(crate) mod fd {
    use super::*;

    #[cfg(test)]
    thread_local! {
        /// A test's other program: runs where one could act in the middle of an operation.
        pub(crate) static MIDWAY: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
            const { std::cell::RefCell::new(None) };
        /// A test's disk without hard links (Termux, where Android refuses them; FAT).
        pub(crate) static NO_LINK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    pub(crate) fn midway() {
        #[cfg(test)]
        MIDWAY.with_borrow_mut(|f| {
            if let Some(f) = f {
                f()
            }
        });
    }

    /// The lock file in a bin that a put, a restore, a delete for good or an Empty holds.
    pub const LOCK: &str = ".coxswain-lock";

    /// Held while this program restores, deletes for good or empties `bins`, so no other
    /// Coxswain takes an item away between this one's look at it and its removal, after which
    /// the item's name may hold a file put in since, and no Coxswain's put copies an item in
    /// meanwhile. Let go when dropped or when the program dies. A bin where no lock file can be
    /// made goes unlocked. When another Coxswain holds it: `bin.busy`, at once (the terminal
    /// app runs these on its UI thread).
    pub fn hold(bins: &[Bin]) -> io::Result<Vec<fs::File>> {
        lock_all(bins, false)
    }

    /// `hold` on the home bin, waiting while another Coxswain holds it: for a put, which runs
    /// as a job. The home bin is the one a put may copy into (from another disk), and a copy
    /// half made is listed: an Empty then would delete it, and the put the original.
    pub fn wait_home() -> Vec<fs::File> {
        let home: Vec<Bin> = crate::dirs::data_dir()
            .map(|d| Bin::at(d.join("Trash")))
            .into_iter()
            .collect();
        lock_all(&home, true).unwrap_or_default()
    }

    fn lock_all(bins: &[Bin], wait: bool) -> io::Result<Vec<fs::File>> {
        let mut held = vec![];
        for b in bins.iter().filter(|b| real(b)) {
            let mut o = fs::OpenOptions::new();
            o.write(true).create(true).truncate(false);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, libc::O_NOFOLLOW);
            let Ok(f) = o.open(b.dir.join(LOCK)) else {
                continue;
            };
            let locked = if wait {
                f.lock().is_ok()
            } else {
                match f.try_lock() {
                    Ok(()) => true,
                    Err(fs::TryLockError::WouldBlock) => {
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            crate::t!("bin.busy"),
                        ));
                    }
                    Err(_) => false,
                }
            };
            if locked {
                held.push(f);
            }
        }
        Ok(held)
    }

    fn busy(e: io::Error) -> Done {
        Done {
            failed: vec![e.to_string()],
            ..Done::default()
        }
    }

    /// The item in `files/` at `path` has no item's info file: none, or one that does not parse.
    fn unlisted(path: &Path) -> bool {
        fs::read_to_string(info_of(path))
            .ok()
            .and_then(|t| parse(&t, None))
            .is_none()
    }

    /// What tells one file from another that took its name since.
    fn id(path: &Path) -> Option<(u64, u64)> {
        let m = fs::symlink_metadata(path).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some((m.dev(), m.ino()))
        }
        #[cfg(not(unix))]
        Some((0, m.len()))
    }

    /// A bin folder and the top of the disk it is on, which the paths in its info files are
    /// relative to (`None`: the home bin, whose paths are whole).
    #[derive(Clone, Debug, PartialEq)]
    pub struct Bin {
        pub dir: PathBuf,
        pub top: Option<PathBuf>,
    }

    impl Bin {
        /// The bin `dir` is, with its top told by its name: `<top>/.Trash-<uid>`,
        /// `<top>/.Trash/<uid>`, else the home bin.
        pub fn at(dir: PathBuf) -> Bin {
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let top = if name.starts_with(".Trash-") {
                dir.parent().map(Path::to_path_buf)
            } else if dir
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|n| n == ".Trash")
            {
                dir.parent().and_then(Path::parent).map(Path::to_path_buf)
            } else {
                None
            };
            Bin { dir, top }
        }
    }

    /// The bins of this machine: the home one, then each disk's.
    pub fn bins() -> Vec<Bin> {
        let mut dirs: Vec<PathBuf> = crate::dirs::data_dir()
            .map(|d| d.join("Trash"))
            .into_iter()
            .collect();
        // The disks' bins, as the trash crate finds them; never in tests, which have a home of
        // their own and must not look at the machine's disks. A Flatpak sees only its own.
        #[cfg(all(
            not(any(test, feature = "test-home")),
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android"),
            not(target_os = "illumos"),
            not(target_os = "solaris")
        ))]
        if crate::tools::flatpak().is_none() {
            dirs.extend(trash::os_limited::trash_folders().unwrap_or_default());
        }
        #[cfg(all(
            not(any(test, feature = "test-home")),
            any(target_os = "illumos", target_os = "solaris")
        ))]
        dirs.extend(disk_bins(
            crate::machine::mounted().into_iter().map(|m| m.mount),
        ));
        dirs.sort();
        dirs.dedup();
        dirs.into_iter().map(Bin::at).filter(real).collect()
    }

    /// A bin that can be used: the bin folder, its `info` and its `files` (when there; none is
    /// an empty bin) are real folders, never links, and so is `.Trash` above `.Trash/<uid>`. A
    /// link there (a stick with `.Trash-1000/files -> /home/you`) would have Empty delete
    /// whatever it points at.
    pub fn real(b: &Bin) -> bool {
        let folder = |p: &Path| fs::symlink_metadata(p).is_ok_and(|m| m.is_dir());
        let files = b.dir.join("files");
        let shared = match b.dir.parent() {
            Some(p) if p.file_name().is_some_and(|n| n == ".Trash") => folder(p),
            _ => true,
        };
        shared
            && folder(&b.dir)
            && folder(&b.dir.join("info"))
            && (folder(&files) || fs::symlink_metadata(&files).is_err())
    }

    /// The bins on these disks' tops that are there: `.Trash/<uid>` (when `.Trash` is a sticky
    /// folder of its own, not a link) and `.Trash-<uid>`.
    #[cfg(unix)]
    #[cfg_attr(
        any(
            test,
            feature = "test-home",
            not(any(target_os = "illumos", target_os = "solaris"))
        ),
        allow(dead_code)
    )]
    pub fn disk_bins(tops: impl Iterator<Item = PathBuf>) -> Vec<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        // SAFETY: getuid cannot fail.
        let uid = unsafe { libc::getuid() };
        let mut out = vec![];
        for top in tops {
            let shared = top.join(".Trash");
            if fs::symlink_metadata(&shared)
                .is_ok_and(|m| m.is_dir() && m.permissions().mode() & 0o1000 != 0)
            {
                out.push(shared.join(uid.to_string()));
            }
            out.push(top.join(format!(".Trash-{uid}")));
        }
        out.retain(|d| real(&Bin::at(d.clone())));
        out
    }

    /// The bins' `info` folders: what changes when something goes in or comes out.
    pub fn watch() -> Vec<PathBuf> {
        bins()
            .into_iter()
            .map(|b| b.dir.join("info"))
            .filter(|d| d.is_dir())
            .collect()
    }

    /// Everything in `bins`: each info file whose item is there.
    pub fn list_in(bins: &[Bin]) -> Vec<Item> {
        let mut out = vec![];
        for b in bins.iter().filter(|b| real(b)) {
            let Ok(rd) = fs::read_dir(b.dir.join("info")) else {
                continue;
            };
            for e in rd.flatten() {
                let file = e.file_name().to_string_lossy().into_owned();
                // `.`, `..` or nothing would name `files` or the bin itself.
                let Some(name) = file
                    .strip_suffix(".trashinfo")
                    .filter(|n| !["", ".", ".."].contains(n))
                else {
                    continue;
                };
                let path = b.dir.join("files").join(name);
                if fs::symlink_metadata(&path).is_err() {
                    continue;
                }
                let Some((origin, deleted)) = fs::read_to_string(e.path())
                    .ok()
                    .and_then(|t| parse(&t, b.top.as_deref()))
                else {
                    continue;
                };
                let (is_dir, is_symlink, size) = sized(&path, deleted);
                out.push(Item {
                    name: origin
                        .file_name()
                        .map_or_else(|| name.to_string(), |n| n.to_string_lossy().into_owned()),
                    path,
                    origin,
                    deleted,
                    is_dir,
                    is_symlink,
                    size,
                });
            }
        }
        out
    }

    /// An info file's `Path` (whole: joined to `top` when relative) and `DeletionDate` (the
    /// local time it says; 0 when it says none).
    pub fn parse(text: &str, top: Option<&Path>) -> Option<(PathBuf, i64)> {
        let mut path = None;
        let mut date = 0;
        for l in text.lines() {
            if let Some(p) = l.strip_prefix("Path=") {
                path = Some(decode(p.trim_end()));
            } else if let Some(d) = l.strip_prefix("DeletionDate=") {
                date = chrono::NaiveDateTime::parse_from_str(d.trim(), "%Y-%m-%dT%H:%M:%S")
                    .ok()
                    .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
                    .map_or(0, |t| t.timestamp());
            }
        }
        let path = path?;
        let path = if path.is_absolute() {
            path
        } else {
            top.unwrap_or(Path::new("/")).join(path)
        };
        Some((path, date))
    }

    /// The spec's URI escaping undone: `%XX` is that byte.
    pub fn decode(s: &str) -> PathBuf {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            match (
                b[i],
                b.get(i + 1).copied().and_then(hex),
                b.get(i + 2).copied().and_then(hex),
            ) {
                (b'%', Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                }
                (c, ..) => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        #[cfg(unix)]
        return PathBuf::from(<std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(&out));
        #[cfg(not(unix))]
        PathBuf::from(String::from_utf8_lossy(&out).into_owned())
    }

    /// The item at `path`, if it is one of `bins`' (never a path elsewhere).
    fn find(bins: &[Bin], path: &Path) -> Option<Item> {
        let b = bins
            .iter()
            .find(|b| path.parent() == Some(b.dir.join("files").as_path()) && real(b))?;
        list_in(std::slice::from_ref(b))
            .into_iter()
            .find(|i| i.path == path)
    }

    fn info_of(path: &Path) -> PathBuf {
        let bin = path
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new(""));
        bin.join("info").join(format!(
            "{}.trashinfo",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))
    }

    pub fn restore_in(bins: &[Bin], paths: &[PathBuf], free_name: bool) -> Done {
        let _held = match hold(bins) {
            Ok(h) => h,
            Err(e) => return busy(e),
        };
        let mut done = Done::default();
        for p in paths {
            let Some(item) = find(bins, p) else {
                done.failed.push(failed(p, crate::t!("err.not_in_trash")));
                continue;
            };
            let mut to = item.origin.clone();
            if fs::symlink_metadata(&to).is_ok() {
                if !free_name {
                    done.taken.push(p.clone());
                    continue;
                }
                to = free_in(to.parent().unwrap_or(Path::new("/")), &item.name);
            }
            let moved = to
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|_| move_new(&item.path, &to));
            match moved {
                Ok(()) => {
                    let _ = fs::remove_file(info_of(&item.path));
                    done.done += 1;
                }
                Err(e) => done.failed.push(failed(&to, e)),
            }
        }
        done
    }

    /// `from` to `to`, never over what is there by then: a file by a hard link, which fails when
    /// `to` exists, then its name in the bin taken away; where no hard link is made (Termux,
    /// where Android refuses them; FAT), by a rename onto an empty file made at `to` with
    /// `create_new`, which fails when `to` exists; a folder by a rename, which cannot replace a
    /// file or a folder with something in it. Across disks (the home bin holds what was moved in
    /// from another), the move F6 does.
    // ponytail: that move checks, then moves: a file made in that instant at `to` is replaced.
    // Windows restores through the shell, with the same check first.
    pub fn move_new(from: &Path, to: &Path) -> io::Result<()> {
        let across = |e: &io::Error| {
            #[cfg(test)]
            if crate::fs::ACROSS.get() {
                return true;
            }
            matches!(
                e.kind(),
                io::ErrorKind::CrossesDevices
                    | io::ErrorKind::PermissionDenied
                    | io::ErrorKind::Unsupported
            )
        };
        #[cfg(test)]
        let simulated = || {
            if crate::fs::ACROSS.get() {
                Err(io::Error::from(io::ErrorKind::CrossesDevices))
            } else {
                Ok(())
            }
        };
        #[cfg(not(test))]
        let simulated = || Ok(());
        let first = if fs::symlink_metadata(from)?.is_dir() {
            if fs::symlink_metadata(to).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    crate::t!("err.exists", "path" => to.display()),
                ));
            }
            simulated().and_then(|_| fs::rename(from, to))
        } else {
            #[cfg(test)]
            let link = |f: &Path, t: &Path| {
                if NO_LINK.get() {
                    Err(io::ErrorKind::PermissionDenied.into())
                } else {
                    fs::hard_link(f, t)
                }
            };
            #[cfg(not(test))]
            let link = fs::hard_link;
            match simulated().and_then(|_| link(from, to)) {
                Ok(()) => return fs::remove_file(from),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                    ) =>
                {
                    // No hard link here (Termux, FAT, or Linux's fs.protected_hardlinks on a file
                    // not ours): the empty file claims `to`, and the rename replaces only that.
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(to)?;
                    fs::rename(from, to).inspect_err(|_| {
                        if fs::symlink_metadata(to).is_ok_and(|m| m.len() == 0) {
                            let _ = fs::remove_file(to);
                        }
                    })
                }
                Err(e) => Err(e),
            }
        };
        match first {
            Err(e) if e.kind() != io::ErrorKind::AlreadyExists && across(&e) => {
                crate::fs::rename(from, to).map(drop)
            }
            r => r,
        }
    }

    fn remove(path: &Path) -> io::Result<()> {
        match fs::symlink_metadata(path) {
            Ok(m) if m.is_dir() => fs::remove_dir_all(path),
            Ok(_) => fs::remove_file(path),
            Err(e) => Err(e),
        }
    }

    pub fn delete_in(bins: &[Bin], paths: &[PathBuf]) -> Done {
        let _held = match hold(bins) {
            Ok(h) => h,
            Err(e) => return busy(e),
        };
        delete_held(bins, paths)
    }

    fn delete_held(bins: &[Bin], paths: &[PathBuf]) -> Done {
        let mut done = Done::default();
        for p in paths {
            if find(bins, p).is_none() {
                done.failed.push(failed(p, crate::t!("err.not_in_trash")));
                continue;
            }
            midway();
            // The item first: an info file left without it is no item.
            match remove(p) {
                Ok(()) => {
                    let _ = fs::remove_file(info_of(p));
                    done.done += 1;
                }
                Err(e) => done.failed.push(failed(p, e)),
            }
        }
        done
    }

    /// What was listed, and the strays: an item with no info file, or one that does not parse
    /// (left by a crash), then and still, and still the same file. Nothing put in while it runs
    /// goes: a put, by a move or by a copy, writes its info file before the item is in `files/`,
    /// and Coxswain's own put waits for the lock. Info files without their item stay, as one may
    /// be such a put under way.
    pub fn empty_in(bins: &[Bin]) -> Done {
        let _held = match hold(bins) {
            Ok(h) => h,
            Err(e) => return busy(e),
        };
        let all: Vec<PathBuf> = list_in(bins).into_iter().map(|i| i.path).collect();
        let strays: Vec<(PathBuf, Option<(u64, u64)>)> = bins
            .iter()
            .filter(|b| real(b))
            .flat_map(|b| {
                fs::read_dir(b.dir.join("files"))
                    .into_iter()
                    .flatten()
                    .flatten()
            })
            .map(|e| e.path())
            .filter(|p| unlisted(p))
            .map(|p| {
                let id = id(&p);
                (p, id)
            })
            .collect();
        let mut done = delete_held(bins, &all);
        midway();
        for (p, was) in strays {
            if unlisted(&p) && id(&p) == was && remove(&p).is_ok() {
                done.done += 1;
            }
        }
        for b in bins.iter().filter(|b| real(b)) {
            let _ = fs::remove_file(b.dir.join("directorysizes"));
        }
        done
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios")))]
mod sys {
    use super::*;

    pub fn list() -> io::Result<Vec<Item>> {
        Ok(fd::list_in(&fd::bins()))
    }
    pub fn watch() -> Vec<PathBuf> {
        fd::watch()
    }
    pub fn restore(paths: &[PathBuf], free_name: bool) -> Done {
        fd::restore_in(&fd::bins(), paths, free_name)
    }
    pub fn delete(paths: &[PathBuf]) -> Done {
        fd::delete_in(&fd::bins(), paths)
    }
    pub fn empty() -> Done {
        fd::empty_in(&fd::bins())
    }
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
mod sys {
    use super::*;
    use trash::os_limited;

    /// Each item of every drive's Recycle Bin, with its place there.
    fn items() -> io::Result<Vec<(trash::TrashItem, Item)>> {
        let list = os_limited::list().map_err(io::Error::other)?;
        Ok(list
            .into_iter()
            .map(|t| {
                let path = PathBuf::from(&t.id);
                // The shell names a file as Explorer shows it, without a known ending; the
                // ending is still on its place in the bin.
                let mut name = t.name.to_string_lossy().into_owned();
                if let Some(ext) = path.extension().map(|e| e.to_string_lossy().into_owned())
                    && Path::new(&name)
                        .extension()
                        .is_none_or(|e| e.to_string_lossy() != ext)
                    && path.is_file()
                {
                    name = format!("{name}.{ext}");
                }
                let (is_dir, is_symlink, size) = sized(&path, t.time_deleted);
                let origin = t.original_parent.join(&name);
                let item = Item {
                    name,
                    path,
                    origin,
                    deleted: t.time_deleted,
                    is_dir,
                    is_symlink,
                    size,
                };
                (t, item)
            })
            .collect())
    }

    pub fn list() -> io::Result<Vec<Item>> {
        Ok(items()?.into_iter().map(|(_, i)| i).collect())
    }
    // ponytail: the Recycle Bin's folders are named by the user's SID; listed again on focus
    // and every half minute instead of watched.
    pub fn watch() -> Vec<PathBuf> {
        vec![]
    }

    fn chosen(paths: &[PathBuf]) -> Result<Vec<(trash::TrashItem, Item)>, String> {
        Ok(items()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|(_, i)| paths.contains(&i.path))
            .collect())
    }

    pub fn restore(paths: &[PathBuf], free_name: bool) -> Done {
        let mut done = Done::default();
        let list = match chosen(paths) {
            Ok(l) => l,
            Err(e) => {
                return Done {
                    failed: vec![e],
                    ..Done::default()
                };
            }
        };
        for (mut t, i) in list {
            let mut to = i.origin.clone();
            if fs::symlink_metadata(&to).is_ok() {
                if !free_name {
                    done.taken.push(i.path);
                    continue;
                }
                to = free_in(&t.original_parent, &i.name);
            }
            // Restored under the name asked for, which is also what the clash check looks at.
            t.name = to.file_name().unwrap_or_default().to_os_string();
            match os_limited::restore_all([t]) {
                Ok(()) => done.done += 1,
                Err(e) => done.failed.push(failed(&to, e)),
            }
        }
        done
    }

    pub fn delete(paths: &[PathBuf]) -> Done {
        match chosen(paths) {
            Ok(l) => {
                let n = l.len();
                match os_limited::purge_all(l.into_iter().map(|(t, _)| t)) {
                    Ok(()) => Done {
                        done: n,
                        ..Done::default()
                    },
                    Err(e) => Done {
                        failed: vec![e.to_string()],
                        ..Done::default()
                    },
                }
            }
            Err(e) => Done {
                failed: vec![e],
                ..Done::default()
            },
        }
    }

    pub fn empty() -> Done {
        match os_limited::list() {
            Ok(l) => {
                let n = l.len();
                match os_limited::purge_all(l) {
                    Ok(()) => Done {
                        done: n,
                        ..Done::default()
                    },
                    Err(e) => Done {
                        failed: vec![e.to_string()],
                        ..Done::default()
                    },
                }
            }
            Err(e) => Done {
                failed: vec![e.to_string()],
                ..Done::default()
            },
        }
    }
}

// ---------------------------------------------------------------- macOS

/// What runs Finder: to open the bin in a window, and to empty it (macOS asks once whether
/// Coxswain may tell Finder what to do).
pub fn finder(empty: bool) -> (&'static str, Vec<&'static str>) {
    let script = if empty {
        "tell application \"Finder\" to empty trash"
    } else {
        "tell application \"Finder\"\nopen trash\nactivate\nend tell"
    };
    ("osascript", vec!["-e", script])
}

/// Run `finder`: on a Mac only.
pub fn run_finder(empty: bool) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err(crate::t!("bin.finder_mac_only"));
    }
    let (program, args) = finder(empty);
    let mut c = crate::tools::command(Path::new(program));
    c.args(args);
    let out = crate::tools::capture(c, std::time::Duration::from_secs(120), 1 << 16)
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod sys {
    use super::*;

    fn dir() -> Option<PathBuf> {
        crate::dirs::home_dir().map(|h| h.join(".Trash"))
    }

    /// `~/.Trash`'s entries; without Full Disk Access the folder cannot be read.
    pub fn list() -> io::Result<Vec<Item>> {
        use std::os::unix::fs::MetadataExt;
        let d = dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home folder"))?;
        let mut out = vec![];
        // None yet is an empty bin; one that cannot be read needs Full Disk Access.
        let rd = match fs::read_dir(&d) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            r => r?,
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == ".DS_Store" {
                continue;
            }
            let path = e.path();
            // Put in the bin: the moment its entry last changed.
            let deleted = fs::symlink_metadata(&path).map_or(0, |m| m.ctime());
            let (is_dir, is_symlink, size) = sized(&path, deleted);
            out.push(Item {
                name,
                path,
                origin: PathBuf::new(),
                deleted,
                is_dir,
                is_symlink,
                size,
            });
        }
        Ok(out)
    }
    pub fn watch() -> Vec<PathBuf> {
        dir()
            .into_iter()
            .filter(|d| fs::read_dir(d).is_ok())
            .collect()
    }
    pub fn restore(_: &[PathBuf], _: bool) -> Done {
        Done {
            failed: vec![crate::t!("bin.no_restore")],
            ..Done::default()
        }
    }
    pub fn delete(paths: &[PathBuf]) -> Done {
        let mut done = Done::default();
        // Only what the bin lists: never a path elsewhere, nor `.DS_Store`.
        let listed: Vec<PathBuf> = list()
            .map(|l| l.into_iter().map(|i| i.path).collect())
            .unwrap_or_default();
        for p in paths {
            let r = if !listed.contains(p) {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    crate::t!("err.not_in_trash"),
                ))
            } else if fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()) {
                fs::remove_dir_all(p)
            } else {
                fs::remove_file(p)
            };
            match r {
                Ok(()) => done.done += 1,
                Err(e) => done.failed.push(failed(p, e)),
            }
        }
        done
    }
    /// By Finder, which empties every disk's bin and knows what is locked.
    pub fn empty() -> Done {
        let n = list().map_or(0, |l| l.len());
        match run_finder(true) {
            Ok(()) => Done {
                done: n,
                ..Done::default()
            },
            Err(e) => Done {
                failed: vec![e],
                ..Done::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fd::*;
    use super::*;

    /// A bin in `dir` with `items` (name in the bin, original path, date), as the spec lays
    /// it out.
    fn bin(dir: &Path, items: &[(&str, &str, &str)]) -> Bin {
        let b = dir.join("Trash");
        fs::create_dir_all(b.join("files")).unwrap();
        fs::create_dir_all(b.join("info")).unwrap();
        for (name, path, date) in items {
            if name.ends_with('/') {
                fs::create_dir_all(b.join("files").join(name).join("sub")).unwrap();
                fs::write(b.join("files").join(name).join("sub/x"), "12345").unwrap();
            } else {
                fs::write(b.join("files").join(name), "abc").unwrap();
            }
            let name = name.trim_end_matches('/');
            fs::write(
                b.join("info").join(format!("{name}.trashinfo")),
                format!("[Trash Info]\nPath={path}\nDeletionDate={date}\n"),
            )
            .unwrap();
        }
        Bin::at(b)
    }

    #[test]
    fn bin_lists_names_places_dates_and_sizes() {
        let d = crate::fs::Scratch::named("bin-list");
        let home = format!("{}/home", d.display());
        let b = bin(
            &d,
            &[
                (
                    "a.txt",
                    &format!("{home}/docs/a.txt"),
                    "2026-10-01T12:00:00",
                ),
                (
                    "a.2.txt",
                    &format!("{home}/other/a%20b.txt"),
                    "2026-10-02T08:30:00",
                ),
                ("dir/", &format!("{home}/dir"), "2026-10-03T00:00:00"),
            ],
        );
        // An info file without its item, and an item without its info file: neither is listed.
        fs::write(
            b.dir.join("info/gone.trashinfo"),
            "[Trash Info]\nPath=/x/gone\n",
        )
        .unwrap();
        fs::write(b.dir.join("files/stray"), "").unwrap();
        let mut l = list_in(std::slice::from_ref(&b));
        l.sort_by_key(|i| i.deleted);
        assert_eq!(
            l.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
            ["a.txt", "a b.txt", "dir"]
        );
        assert_eq!(l[1].origin, Path::new(&home).join("other/a b.txt"));
        assert_eq!(l[1].path, b.dir.join("files/a.2.txt"));
        assert_eq!((l[0].size, l[2].size, l[2].is_dir), (3, 5, true));
        let want =
            chrono::NaiveDateTime::parse_from_str("2026-10-01T12:00:00", "%Y-%m-%dT%H:%M:%S")
                .unwrap()
                .and_local_timezone(chrono::Local)
                .unwrap()
                .timestamp();
        assert_eq!(l[0].deleted, want);
    }

    #[test]
    fn bin_on_a_disk_takes_paths_from_its_top() {
        let d = crate::fs::Scratch::named("bin-top");
        let b = Bin::at(d.join(".Trash-1000"));
        assert_eq!(b.top.as_deref(), Some(&*d as &Path));
        assert_eq!(
            Bin::at(d.join(".Trash/1000")).top.as_deref(),
            Some(&*d as &Path)
        );
        assert_eq!(Bin::at(d.join("Trash")).top, None);
        assert_eq!(
            parse("Path=photos/x%2Fy.jpg\n", Some(&d)).unwrap().0,
            d.join("photos/x/y.jpg")
        );
        assert_eq!(
            parse(
                "[Trash Info]\nPath=/a/%C3%B8\nDeletionDate=nonsense\n",
                None
            ),
            Some((PathBuf::from("/a/ø"), 0))
        );
        assert_eq!(parse("[Trash Info]\n", None), None);
    }

    #[test]
    fn bin_restores_and_asks_when_the_place_is_taken() {
        let d = crate::fs::Scratch::named("bin-restore");
        let home = d.join("home");
        fs::create_dir_all(home.join("docs")).unwrap();
        let b = bin(
            &d,
            &[
                (
                    "a.txt",
                    &format!("{}/docs/a.txt", home.display()),
                    "2026-10-01T12:00:00",
                ),
                (
                    "n.txt",
                    &format!("{}/new/deep/n.txt", home.display()),
                    "2026-10-01T12:00:00",
                ),
            ],
        );
        let bins = [b.clone()];
        fs::write(home.join("docs/a.txt"), "here now").unwrap();
        let (a, n) = (b.dir.join("files/a.txt"), b.dir.join("files/n.txt"));
        let r = restore_in(&bins, &[a.clone(), n.clone()], false);
        assert_eq!(
            (r.done, r.taken.clone(), r.failed.len()),
            (1, vec![a.clone()], 0)
        );
        // Its folder made again.
        assert_eq!(
            fs::read_to_string(home.join("new/deep/n.txt")).unwrap(),
            "abc"
        );
        assert!(!b.dir.join("info/n.txt.trashinfo").exists());
        // Never over what is there: beside it, under a free name.
        let r = restore_in(&bins, &[a], true);
        assert_eq!(r.done, 1);
        assert_eq!(
            fs::read_to_string(home.join("docs/a.txt")).unwrap(),
            "here now"
        );
        assert_eq!(
            fs::read_to_string(home.join("docs/a (2).txt")).unwrap(),
            "abc"
        );
        assert!(list_in(&bins).is_empty());
        // A path outside the bin is none of its items.
        let r = restore_in(&bins, &[home.join("docs/a.txt")], true);
        assert_eq!((r.done, r.failed.len()), (0, 1));
    }

    #[test]
    fn bin_deletes_for_good_only_its_own_items_and_empties() {
        let d = crate::fs::Scratch::named("bin-delete");
        let b = bin(
            &d,
            &[
                ("a.txt", "/x/a.txt", "2026-10-01T12:00:00"),
                ("b.txt", "/x/b.txt", "2026-10-01T12:00:00"),
                ("dir/", "/x/dir", "2026-10-01T12:00:00"),
            ],
        );
        let bins = [b.clone()];
        let outside = d.join("keep.txt");
        fs::write(&outside, "keep").unwrap();
        let r = delete_in(
            &bins,
            &[
                b.dir.join("files/a.txt"),
                outside.clone(),
                b.dir.join("files/dir"),
            ],
        );
        assert_eq!((r.done, r.failed.len()), (2, 1));
        assert!(outside.exists(), "nothing outside the bin is touched");
        assert!(
            !b.dir.join("files/a.txt").exists() && !b.dir.join("info/a.txt.trashinfo").exists()
        );
        assert!(!b.dir.join("files/dir").exists());
        assert_eq!(list_in(&bins).len(), 1);
        fs::write(b.dir.join("files/stray"), "").unwrap();
        let r = empty_in(&bins);
        assert_eq!(r.done, 2);
        assert_eq!(
            fs::read_dir(b.dir.join("files")).unwrap().count()
                + fs::read_dir(b.dir.join("info")).unwrap().count(),
            0
        );
        assert!(outside.exists());
    }

    /// Empty takes what it listed and what had no info file then, never a file put in while
    /// it ran (#720).
    #[test]
    fn bin_empty_keeps_what_is_put_in_meanwhile() {
        let d = crate::fs::Scratch::named("bin-empty-meanwhile");
        let b = bin(&d, &[("a.txt", "/x/a.txt", "2026-10-01T12:00:00")]);
        fs::write(b.dir.join("files/stray"), "").unwrap();
        let bins = [b.clone()];
        let dir = b.dir.clone();
        MIDWAY.set(Some(Box::new(move || {
            // Another program's put: the info file, then the file.
            fs::write(
                dir.join("info/a.txt.trashinfo"),
                "[Trash Info]\nPath=/y/a.txt\nDeletionDate=2026-10-02T12:00:00\n",
            )
            .unwrap();
            fs::write(dir.join("files/a.txt"), "new").unwrap();
        })));
        let r = empty_in(&bins);
        MIDWAY.set(None);
        assert_eq!(r.done, 2, "a.txt as listed, and the stray");
        assert!(!b.dir.join("files/stray").exists());
        let l = list_in(&bins);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].origin, Path::new("/y/a.txt"));
        assert_eq!(fs::read_to_string(&l[0].path).unwrap(), "new");
    }

    /// Between its look at an item and its removal, a delete for good or an Empty holds the
    /// bin's lock, so another Coxswain cannot free the name for a new file to take (#720).
    #[test]
    fn bin_delete_and_empty_hold_the_bin() {
        let d = crate::fs::Scratch::named("bin-held");
        let b = bin(&d, &[("a.txt", "/x/a.txt", "2026-10-01T12:00:00")]);
        let bins = [b.clone()];
        let lock = b.dir.join(LOCK);
        let seen = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
        let s = seen.clone();
        MIDWAY.set(Some(Box::new(move || {
            let other = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock)
                .unwrap();
            s.borrow_mut().push(matches!(
                other.try_lock(),
                Err(fs::TryLockError::WouldBlock)
            ));
        })));
        delete_in(&bins, &[b.dir.join("files/a.txt")]);
        empty_in(&bins);
        MIDWAY.set(None);
        assert_eq!(*seen.borrow(), [true, true]);
    }

    /// While another Coxswain holds the bin, restore, delete for good and Empty say so at once
    /// and touch nothing: the terminal app runs them on its UI thread.
    #[test]
    fn bin_busy_says_so_and_waits_for_nothing() {
        let d = crate::fs::Scratch::named("bin-busy");
        let b = bin(&d, &[("a.txt", "/x/a.txt", "2026-10-01T12:00:00")]);
        let bins = [b.clone()];
        let other = hold(&bins).unwrap();
        let a = b.dir.join("files/a.txt");
        let busy = vec![crate::t!("bin.busy").to_string()];
        assert_eq!(delete_in(&bins, std::slice::from_ref(&a)).failed, busy);
        assert_eq!(
            restore_in(&bins, std::slice::from_ref(&a), true).failed,
            busy
        );
        assert_eq!(empty_in(&bins).failed, busy);
        assert_eq!(list_in(&bins).len(), 1);
        drop(other);
        assert_eq!(empty_in(&bins).done, 1);
    }

    /// A stray (no info file, or one that does not parse) goes with Empty, but not a file that
    /// took its name after the listing. Unix: the freedesktop.org bin's systems, where a file
    /// is told by its inode.
    #[cfg(unix)]
    #[test]
    fn bin_empty_takes_strays_it_saw_only() {
        let d = crate::fs::Scratch::named("bin-strays");
        let b = bin(&d, &[]);
        let bins = [b.clone()];
        fs::write(b.dir.join("files/broken"), "x").unwrap();
        fs::write(b.dir.join("info/broken.trashinfo"), "not an info file").unwrap();
        fs::write(b.dir.join("files/swapped"), "old").unwrap();
        let dir = b.dir.clone();
        MIDWAY.set(Some(Box::new(move || {
            // Another program's file takes the stray's name: made first, so a new file.
            fs::write(dir.join("new"), "new").unwrap();
            fs::rename(dir.join("new"), dir.join("files/swapped")).unwrap();
        })));
        let r = empty_in(&bins);
        MIDWAY.set(None);
        assert_eq!(r.done, 1);
        assert!(!b.dir.join("files/broken").exists());
        assert_eq!(
            fs::read_to_string(b.dir.join("files/swapped")).unwrap(),
            "new"
        );
    }

    /// A bin whose `files` is a link to a folder outside it (a stick with
    /// `.Trash-1000/files -> /home/you`): nothing done in the bin reaches that folder.
    #[cfg(unix)]
    #[test]
    fn bin_never_reaches_through_a_link() {
        let d = crate::fs::Scratch::named("bin-link");
        let outside = d.join("outside");
        fs::create_dir_all(outside.join("docs")).unwrap();
        fs::write(outside.join("docs/keep.txt"), "keep").unwrap();
        fs::write(outside.join("a.txt"), "keep").unwrap();
        let b = d.join(".Trash-1000");
        fs::create_dir_all(b.join("info")).unwrap();
        std::os::unix::fs::symlink(&outside, b.join("files")).unwrap();
        fs::write(b.join("info/docs.trashinfo"), "[Trash Info]\nPath=docs\n").unwrap();
        fs::write(
            b.join("info/a.txt.trashinfo"),
            "[Trash Info]\nPath=back/a.txt\n",
        )
        .unwrap();
        let bins = [Bin::at(b.clone())];
        assert!(list_in(&bins).is_empty());
        restore_in(&bins, &[b.join("files/a.txt")], true);
        delete_in(&bins, &[b.join("files/docs")]);
        empty_in(&bins);
        assert_eq!(
            fs::read_to_string(outside.join("docs/keep.txt")).unwrap(),
            "keep"
        );
        assert_eq!(fs::read_to_string(outside.join("a.txt")).unwrap(), "keep");
        // `info` a link, the bin itself a link, `.Trash` above `<uid>` a link: none is a bin.
        let kept = |dir: PathBuf| real(&Bin::at(dir));
        assert!(!kept(b.clone()));
        let c = d.join("c/.Trash-1000");
        fs::create_dir_all(c.join("files")).unwrap();
        std::os::unix::fs::symlink(&outside, c.join("info")).unwrap();
        assert!(!kept(c));
        std::os::unix::fs::symlink(d.join("real"), d.join("linked")).unwrap();
        fs::create_dir_all(d.join("real/info")).unwrap();
        assert!(!kept(d.join("linked")));
        fs::create_dir_all(d.join("t/.Trash-target/1000/info")).unwrap();
        fs::create_dir_all(d.join("t2")).unwrap();
        std::os::unix::fs::symlink(d.join("t/.Trash-target"), d.join("t2/.Trash")).unwrap();
        assert!(!kept(d.join("t2/.Trash/1000")));
        // A bin with no `files` yet is a bin, and empty.
        assert!(kept(d.join("real")));
        assert!(disk_bins([d.join("c"), d.join("t2")].into_iter()).is_empty());
    }

    /// An info file named `..`, `.` or nothing names `files` or the bin itself: never an item.
    #[cfg(unix)]
    #[test]
    fn bin_odd_names_are_no_items() {
        let d = crate::fs::Scratch::named("bin-dots");
        let b = bin(&d, &[("a.txt", "/x/a.txt", "2026-10-01T12:00:00")]);
        for n in ["", ".", ".."] {
            fs::write(
                b.dir.join(format!("info/{n}.trashinfo")),
                "[Trash Info]\nPath=/x/y\n",
            )
            .unwrap();
        }
        let bins = [b.clone()];
        assert_eq!(list_in(&bins).len(), 1);
        empty_in(&bins);
        assert!(
            b.dir.join("files").is_dir() && b.dir.join("info").is_dir(),
            "the bin stays"
        );
    }

    /// Restore never replaces what is at the place by the time it moves.
    #[test]
    fn bin_move_never_replaces() {
        let d = crate::fs::Scratch::named("bin-noreplace");
        fs::write(d.join("from"), "bin").unwrap();
        fs::write(d.join("to"), "new").unwrap();
        assert_eq!(
            move_new(&d.join("from"), &d.join("to")).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_to_string(d.join("to")).unwrap(), "new");
        assert_eq!(fs::read_to_string(d.join("from")).unwrap(), "bin");
        fs::create_dir_all(d.join("dir/x")).unwrap();
        assert_eq!(
            move_new(&d.join("dir"), &d.join("to")).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        move_new(&d.join("from"), &d.join("back")).unwrap();
        assert!(!d.join("from").exists() && fs::read_to_string(d.join("back")).unwrap() == "bin");
    }

    /// The home bin holds what was moved in from another disk: it goes back across.
    #[cfg(unix)]
    #[test]
    fn bin_restores_across_disks() {
        let d = crate::fs::Scratch::named("bin-across");
        // Another file system when there is one here (Linux's /dev/shm), else as across.
        let shm = Path::new("/dev/shm");
        use std::os::unix::fs::MetadataExt;
        let real = shm.is_dir()
            && fs::metadata(shm).ok().map(|m| m.dev()) != fs::metadata(&*d).ok().map(|m| m.dev());
        let home = if real {
            shm.join(format!("coxswain-test-bin-{}", std::process::id()))
        } else {
            d.join("home")
        };
        fs::create_dir_all(&home).unwrap();
        let b = bin(
            &d,
            &[
                (
                    "a.txt",
                    &format!("{}/a.txt", home.display()),
                    "2026-10-01T12:00:00",
                ),
                (
                    "dir/",
                    &format!("{}/dir", home.display()),
                    "2026-10-01T12:00:00",
                ),
            ],
        );
        crate::fs::ACROSS.set(!real);
        let r = restore_in(
            &[b.clone()],
            &[b.dir.join("files/a.txt"), b.dir.join("files/dir")],
            false,
        );
        crate::fs::ACROSS.set(false);
        let got = (
            fs::read_to_string(home.join("a.txt")).ok(),
            fs::read_to_string(home.join("dir/sub/x")).ok(),
        );
        if real {
            let _ = fs::remove_dir_all(&home);
        }
        assert_eq!((r.done, r.failed), (2, Vec::<String>::new()));
        assert_eq!(got, (Some("abc".into()), Some("12345".into())));
        assert!(list_in(&[b]).is_empty());
    }

    /// Nothing is put into the bin's panel: a real `@bin` would hide the bin.
    #[test]
    fn bin_takes_no_copy_move_or_pack() {
        let d = crate::fs::Scratch::named("bin-into");
        fs::write(d.join("a.txt"), "a").unwrap();
        let bin = path().unwrap();
        assert!(inside(&bin) && inside(&bin.join("x.zip")) && !inside(&d));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        for op in [
            crate::undo::Op::Copy,
            crate::undo::Op::Move,
            crate::undo::Op::Pack { hide_names: false },
        ] {
            let to = if matches!(op, crate::undo::Op::Pack { .. }) {
                bin.join("a.zip")
            } else {
                bin.clone()
            };
            let r = crate::undo::run(op, &[d.join("a.txt")], &to, None, &stop, |_| {});
            assert_eq!(r.failed.len(), 1);
        }
        assert!(fs::symlink_metadata(&bin).is_err(), "no @bin made");
        assert!(d.join("a.txt").exists());
        assert!(
            crate::fs::copy(&d.join("a.txt"), &bin).is_err()
                && crate::fs::mkdir(&bin.join("n")).is_err()
        );
        assert!(fs::symlink_metadata(&bin).is_err());
        assert_eq!(crate::drag::read_only(&bin), Some("bin.read_only"));
    }

    #[cfg(unix)]
    #[test]
    fn bin_finds_a_disks_own_bins() {
        use std::os::unix::fs::PermissionsExt;
        let d = crate::fs::Scratch::named("bin-disks");
        // SAFETY: getuid cannot fail.
        let uid = unsafe { libc::getuid() };
        let (a, b, c) = (d.join("a"), d.join("b"), d.join("c"));
        fs::create_dir_all(a.join(format!(".Trash-{uid}/info"))).unwrap();
        fs::create_dir_all(b.join(format!(".Trash/{uid}/info"))).unwrap();
        fs::set_permissions(b.join(".Trash"), fs::Permissions::from_mode(0o1777)).unwrap();
        // Not sticky: anyone could have put it there; left out.
        fs::create_dir_all(c.join(format!(".Trash/{uid}/info"))).unwrap();
        let found = disk_bins([a.clone(), b.clone(), c].into_iter());
        assert_eq!(
            found,
            [
                a.join(format!(".Trash-{uid}")),
                b.join(format!(".Trash/{uid}"))
            ]
        );
    }

    #[test]
    fn bin_path_is_home_and_lists_as_a_folder() {
        let p = path().unwrap();
        assert!(is_bin(&p) && !is_bin(p.parent().unwrap()));
        // The test home's bin: `..` first, then what is in it.
        let e = entries(true).unwrap();
        assert!(e[0].is_parent());
    }

    #[test]
    fn bin_finder_commands() {
        assert_eq!(
            finder(true),
            (
                "osascript",
                vec!["-e", "tell application \"Finder\" to empty trash"]
            )
        );
        let (p, a) = finder(false);
        assert_eq!(p, "osascript");
        assert!(a[1].contains("open trash") && a[1].contains("activate"));
    }
}

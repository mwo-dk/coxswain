//! Undo (Ctrl+Z) for the last file operations: a short history, for the app run, of what each
//! operation left, and the way back. Before anything goes back, each item is checked to be still
//! as the operation left it; what changed since is refused, with a reason, and the rest is
//! undone. Nothing is overwritten, and what undo takes away goes to the trash.

use crate::{fs as cfs, t, tn};
use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

/// How many operations the history keeps.
pub const KEEP: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Copy,
    /// Moved or renamed, one by one (F6, drag and drop, cut and paste).
    Move,
    /// Renamed together in one folder (batch rename): undone together, so swaps go back too.
    Rename,
    Mkdir,
    Trash,
    Pack,
    Extract,
    /// Text replaced in files (`replace`): `from` is the file as it was, kept aside, `to` the file.
    Replace,
    /// Permissions and dates changed (`attrs`): each item keeps what its file had before.
    Attrs,
}

/// What a file or folder was. `deep` (for what undo would take away): every entry inside a
/// folder counts too, so a file changed in a copied folder is noticed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
    entries: u64,
}

fn stamp(path: &Path, deep: bool) -> Option<Stamp> {
    let m = fs::symlink_metadata(path).ok()?;
    let mut s = Stamp {
        size: m.len(),
        modified: m.modified().ok(),
        entries: 1,
    };
    if deep && m.is_dir() {
        for e in fs::read_dir(path).ok()?.flatten() {
            let inner = stamp(&e.path(), true)?;
            s.size += inner.size;
            s.entries += inner.entries;
            s.modified = s.modified.max(inner.modified);
        }
    }
    Some(s)
}

#[derive(Clone, Debug)]
struct Item {
    /// Where it came from (for a new folder, a pack or a copy: what was made).
    from: PathBuf,
    /// What the operation left: the new place, the copy, the archive, the folder. For the trash,
    /// its name in Coxswain's own trash (Termux, illumos), else empty.
    to: PathBuf,
    stamp: Option<Stamp>,
    /// For `Attrs`: the bits and times the file had before.
    was: Option<(crate::attrs::Was, crate::attrs::Was)>,
    /// Where a change in place was made (`pinned`): the folder, and the file as it was left.
    /// Undo goes back only in that folder, and only to that very file, unchanged since.
    pin: Option<crate::pinned::Pin>,
}

/// One operation, as far as it can be undone.
#[derive(Clone, Debug)]
pub struct Record {
    kind: Kind,
    items: Vec<Item>,
    /// Folders a move across disks made and that it left with something in them when it ended
    /// half way: removed by undo once empty again, the innermost first.
    made: Vec<PathBuf>,
    /// When it started, in seconds since the epoch: what the trash holds from before is not its.
    since: i64,
}

fn name(p: &Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The trash gives back what went into it: Windows, the freedesktop.org trash (Linux, the
/// BSDs), and Coxswain's own (Termux, illumos). Not on a Mac, where the system offers no way back to an app,
/// nor in a Flatpak, whose trash is the host's.
pub fn trash_restores() -> bool {
    !cfg!(any(target_os = "macos", target_os = "ios")) && crate::tools::flatpak().is_none()
}

/// Put `path` back from the trash; `name`: its name in Coxswain's own trash (Termux, illumos).
#[cfg(any(
    windows,
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android"),
        not(target_os = "illumos"),
        not(target_os = "solaris")
    )
))]
fn restore(path: &Path, _name: &Path, since: i64) -> io::Result<()> {
    use trash::os_limited;
    let want = comparable(path);
    // ponytail: two trashings of one path within a second are told apart by nothing; the newest
    // listed wins.
    let file = path.file_name().unwrap_or_default();
    // Windows lists a name as Explorer shows it, without a known ending (`report` for
    // `report.pdf`); the ending is still on the item's place in the Recycle Bin.
    let shown_short = |i: &trash::TrashItem| {
        cfg!(windows)
            && Path::new(&i.name).file_stem().is_some()
            && Some(i.name.as_os_str()) == Path::new(file).file_stem()
            && Path::new(&i.id).extension() == Path::new(file).extension()
    };
    let mut item = os_limited::list()
        .map_err(io::Error::other)?
        .into_iter()
        .filter(|i| {
            i.time_deleted >= since
                && comparable(&i.original_parent.join(if shown_short(i) {
                    file
                } else {
                    i.name.as_os_str()
                })) == want
        })
        .max_by_key(|i| i.time_deleted)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, t!("undo.not_in_trash")))?;
    // Restored under its whole name, which is also what the collision check looks at.
    item.name = file.to_os_string();
    os_limited::restore_all([item]).map_err(|e| match e {
        trash::Error::RestoreCollision { .. } => {
            io::Error::new(io::ErrorKind::AlreadyExists, t!("undo.taken"))
        }
        e => io::Error::other(e),
    })
}

#[cfg(any(target_os = "android", target_os = "illumos", target_os = "solaris"))]
fn restore(path: &Path, name: &Path, _since: i64) -> io::Result<()> {
    crate::xdg_trash::restore(&name.to_string_lossy(), path)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn restore(_path: &Path, _name: &Path, _since: i64) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        t!("undo.no_restore"),
    ))
}

/// A path as the trash records it: the folder resolved, as the trash crate does; on Windows
/// without `\\?\` and in one case.
#[cfg(any(
    windows,
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android"),
        not(target_os = "illumos"),
        not(target_os = "solaris")
    )
))]
fn comparable(p: &Path) -> PathBuf {
    let p = match (
        p.parent().and_then(|d| fs::canonicalize(d).ok()),
        p.file_name(),
    ) {
        (Some(d), Some(n)) => d.join(n),
        _ => p.to_path_buf(),
    };
    #[cfg(windows)]
    let p = {
        let s = p.to_string_lossy();
        let s = s
            .strip_prefix(r"\\?\UNC\")
            .map(|r| format!(r"\\{r}"))
            .or_else(|| s.strip_prefix(r"\\?\").map(str::to_string))
            .unwrap_or_else(|| s.to_string());
        PathBuf::from(s.to_lowercase())
    };
    p
}

impl Record {
    pub fn new(kind: Kind) -> Record {
        Record {
            kind,
            items: vec![],
            made: vec![],
            since: crate::fs::now() as i64 - 1,
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.made.is_empty()
    }

    /// One item done: `from` is where it came from (what was made, for a new folder or a pack),
    /// `to` what the operation left (nothing for the trash). Kept when it can be undone.
    pub fn done(&mut self, from: &Path, to: &Path) {
        let stamp = match self.kind {
            Kind::Trash => None,
            k => match stamp(to, matches!(k, Kind::Copy | Kind::Pack | Kind::Extract)) {
                Some(s) => Some(s),
                None => return,
            },
        };
        self.items.push(Item {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            stamp,
            was: None,
            pin: None,
        });
    }

    /// A file changed where it is (`replace`, `attrs`): `from` what undo takes back (the file
    /// as it was, kept aside), `file` the file, `pin` where and what it is now.
    pub fn done_pinned(&mut self, from: &Path, file: &Path, pin: crate::pinned::Pin) {
        self.items.push(Item { from: from.to_path_buf(), to: file.to_path_buf(), stamp: None, was: None, pin: Some(pin) });
    }

    /// A file whose bits or times were changed (`attrs`), and what it had before.
    pub fn done_attrs(&mut self, path: &Path, before: crate::attrs::Was, after: crate::attrs::Was, pin: crate::pinned::Pin) {
        self.items.push(Item { from: path.to_path_buf(), to: path.to_path_buf(), stamp: None, was: Some((before, after)), pin: Some(pin) });
    }

    /// The operation on `src`, as `fs` and `archive` do it (`dst`: where to, for a copy, a move
    /// or an extract), keeping what it left when that can be undone: not what went into or came
    /// out of an archive for a move, not what was merged into a folder that was there. Stopped
    /// or failed half way, what it did so far is kept too: a folder copied or extracted in part,
    /// the files a move across disks moved.
    pub fn run(&mut self, src: &Path, dst: &Path, password: Option<&str>) -> io::Result<()> {
        let on_disk = |p: &Path| fs::symlink_metadata(p).is_ok();
        match self.kind {
            Kind::Copy => {
                let target = cfs::target(src, dst);
                let fresh = !on_disk(&target);
                match cfs::copy_locked(src, dst, password) {
                    Ok(to) if fresh && on_disk(&to) => self.done(src, &to),
                    Ok(_) => {}
                    Err(e) => {
                        if fresh && crate::stop::asked() && on_disk(&target) {
                            self.done(src, &target);
                        }
                        return Err(e);
                    }
                }
            }
            Kind::Move | Kind::Rename => {
                let real = on_disk(src);
                let mut log = cfs::Moved::default();
                match cfs::rename_locked_logged(src, dst, password, &mut log) {
                    Ok(to) if real && on_disk(&to) => self.done(src, &to),
                    Ok(_) => {}
                    Err(e) => {
                        for (from, to) in &log.done {
                            self.done(from, to);
                        }
                        self.made.extend(log.made);
                        return Err(e);
                    }
                }
            }
            Kind::Extract => {
                let to = crate::archive::extract_target(src, dst);
                // A folder of that name that was there is the user's, never the extract's.
                let fresh = !on_disk(&to);
                match crate::archive::extract_locked(src, dst, password) {
                    Ok(to) => self.done(src, &to),
                    Err(e) => {
                        if fresh && crate::stop::asked() && on_disk(&to) {
                            self.done(src, &to);
                        }
                        return Err(e);
                    }
                }
            }
            Kind::Trash => {
                let real = on_disk(src);
                let name = cfs::trash_named(src, password)?.unwrap_or_default();
                if real && trash_restores() {
                    self.done(src, Path::new(&name));
                }
            }
            Kind::Mkdir => {
                // Each folder made, the outermost first, so undo removes the innermost first.
                let mut made: Vec<PathBuf> = src
                    .ancestors()
                    .take_while(|a| !a.as_os_str().is_empty() && !on_disk(a))
                    .map(Path::to_path_buf)
                    .collect();
                cfs::mkdir_locked(src, password)?;
                if src.is_dir() {
                    made.reverse();
                    made.iter().for_each(|p| self.done(p, p));
                }
            }
            Kind::Pack | Kind::Replace | Kind::Attrs => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a pack is done with archive::create_locked, a replace with replace::apply",
                ));
            }
        }
        Ok(())
    }

    /// The paths it touches, to read again after it is undone.
    pub fn touched(&self) -> Vec<PathBuf> {
        let trash = self.kind == Kind::Trash;
        let kept = self.kind == Kind::Replace;
        self.items
            .iter()
            .flat_map(|i| [(!kept).then(|| i.from.clone()), (!trash).then(|| i.to.clone())])
            .flatten()
            .collect()
    }

    /// What it was, for "Undo: …" and "Undone: …": *rename a.txt → b.txt*, *move 3 items to
    /// Documents*.
    pub fn label(&self) -> String {
        let n = self.items.len();
        let what = |p: &Path| {
            if n == 1 {
                format!("\"{}\"", name(p))
            } else {
                tn!("items", n)
            }
        };
        let Some(first) = self.items.first() else {
            return String::new();
        };
        let into = |p: &Path| p.parent().map(name).unwrap_or_default();
        match self.kind {
            Kind::Move | Kind::Rename
                if self.items.iter().all(|i| i.from.parent() == i.to.parent()) =>
            {
                if n == 1 {
                    t!("undo.what.rename_one", "from" => name(&first.from), "to" => name(&first.to))
                } else {
                    t!("undo.what.rename", "what" => what(&first.from))
                }
            }
            Kind::Move | Kind::Rename => {
                t!("undo.what.move", "what" => what(&first.from), "dir" => into(&first.to))
            }
            Kind::Copy => {
                t!("undo.what.copy", "what" => what(&first.from), "dir" => into(&first.to))
            }
            Kind::Mkdir => t!("undo.what.mkdir", "what" => format!("\"{}\"", name(&first.from))),
            Kind::Trash => t!("undo.what.trash", "what" => what(&first.from)),
            Kind::Pack => t!("undo.what.pack", "what" => format!("\"{}\"", name(&first.to))),
            Kind::Attrs => t!("undo.what.attrs", "what" => what(&first.to)),
            Kind::Extract => {
                t!("undo.what.extract", "what" => what(&first.from), "dir" => into(&first.to))
            }
            Kind::Replace => t!("undo.what.replace", "what" => what(&first.to)),
        }
    }

    /// What it kept aside to be undone (a replace's files as they were), removed: once it can
    /// no longer be undone.
    pub fn forget(&self) {
        if self.kind == Kind::Replace {
            for i in &self.items {
                let _ = fs::remove_file(&i.from);
                if let Some(dir) = i.from.parent() {
                    crate::replace::release(dir);
                    let _ = fs::remove_file(dir.join(".lock"));
                    let _ = fs::remove_dir(dir);
                }
            }
        }
    }

    /// Undo it, the last item first; `at` hears which item is next. What is no longer as the
    /// operation left it is refused, each with a line saying why.
    pub fn undo(&self, mut at: impl FnMut(usize)) -> Undone {
        let mut refused = vec![];
        let mut done = 0;
        // What came back where it was, for the apps to put the cursor on.
        let mut back = vec![];
        if self.kind == Kind::Rename {
            let (n, r) = self.undo_renames();
            (done, refused) = (n, r);
            // Back under its old name: that name is there and the new one is not.
            back = (self.items.iter())
                .filter(|i| {
                    fs::symlink_metadata(&i.from).is_ok()
                        && (fs::symlink_metadata(&i.to).is_err() || cfs::same_file(&i.from, &i.to))
                })
                .map(|i| i.from.clone())
                .collect();
        } else {
            for (i, item) in self.items.iter().enumerate().rev() {
                at(self.items.len() - 1 - i);
                match self.undo_one(item) {
                    Ok(()) => {
                        done += 1;
                        if matches!(self.kind, Kind::Move | Kind::Trash) {
                            back.push(item.from.clone());
                        }
                    }
                    Err(why) => refused.push(why),
                }
            }
            // Undone once, never again: what it kept aside for that goes, refused or not.
            self.forget();
            for m in self.made.iter().rev() {
                let _ = fs::remove_dir(m);
            }
        }
        Undone {
            label: self.label(),
            done,
            refused,
            back,
            touched: self.touched(),
        }
    }

    /// Whether `item`'s result is still as the operation left it.
    fn unchanged(&self, item: &Item) -> Result<(), String> {
        let deep = matches!(self.kind, Kind::Copy | Kind::Pack | Kind::Extract);
        match stamp(&item.to, deep) {
            None => Err(format!("{}: {}", item.to.display(), t!("undo.gone"))),
            Some(s) if Some(s) != item.stamp => {
                Err(format!("{}: {}", item.to.display(), t!("undo.changed")))
            }
            Some(_) => Ok(()),
        }
    }

    fn free(path: &Path) -> Result<(), String> {
        if fs::symlink_metadata(path).is_ok() {
            Err(format!("{}: {}", path.display(), t!("undo.taken")))
        } else {
            Ok(())
        }
    }

    /// Where `item` goes back to is free, or is the item itself: a rename that changed only the
    /// case, where case does not count.
    fn free_for(item: &Item) -> Result<(), String> {
        if cfs::same_file(&item.from, &item.to) {
            Ok(())
        } else {
            Self::free(&item.from)
        }
    }

    fn undo_one(&self, item: &Item) -> Result<(), String> {
        let err = |p: &Path, e: io::Error| format!("{}: {e}", p.display());
        match self.kind {
            Kind::Move | Kind::Rename => {
                self.unchanged(item)?;
                Self::free_for(item)?;
                cfs::rename(&item.to, &item.from)
                    .map(drop)
                    .map_err(|e| err(&item.to, e))
            }
            Kind::Copy | Kind::Pack | Kind::Extract => {
                self.unchanged(item)?;
                if !cfs::trash_takes(std::slice::from_ref(&item.to)) {
                    return Err(format!("{}: {}", item.to.display(), t!("undo.no_trash")));
                }
                cfs::trash(&item.to).map_err(|e| err(&item.to, e))
            }
            Kind::Mkdir => {
                if fs::symlink_metadata(&item.to).is_err() {
                    return Err(format!("{}: {}", item.to.display(), t!("undo.gone")));
                }
                if !fs::read_dir(&item.to).is_ok_and(|mut d| d.next().is_none()) {
                    return Err(format!("{}: {}", item.to.display(), t!("undo.not_empty")));
                }
                fs::remove_dir(&item.to).map_err(|e| err(&item.to, e))
            }
            Kind::Trash => {
                Self::free(&item.from)?;
                restore(&item.from, &item.to, self.since).map_err(|e| err(&item.from, e))
            }
            // The file as it was, written back over it; the copy kept aside goes.
            Kind::Replace => {
                let pin = item.pin.clone().ok_or_else(|| format!("{}: {}", item.to.display(), t!("undo.changed")))?;
                let was = crate::replace::read_kept(&item.from).map_err(|e| err(&item.from, e))?;
                crate::replace::write_back(&item.to, &was, pin).map_err(|e| err(&item.to, e))?;
                let _ = fs::remove_file(&item.from);
                if let Some(dir) = item.from.parent() {
                    let _ = fs::remove_dir(dir);
                }
                Ok(())
            }
            // The bits and times it had, given back: only what the change changed, and only to
            // the very entry it left, unchanged since.
            Kind::Attrs => {
                let (Some((before, after)), Some(pin)) = (item.was, item.pin.clone()) else {
                    return Err(format!("{}: {}", item.to.display(), t!("undo.gone")));
                };
                crate::attrs::undo(&item.to, pin, before, after).map_err(|e| err(&item.to, e))
            }
        }
    }

    /// A batch rename goes back together, through `rename::apply`, so that names that were
    /// swapped are swapped back. A name taken since is refused; one that another item gives up
    /// is free.
    fn undo_renames(&self) -> (usize, Vec<String>) {
        let mut refused = vec![];
        let mut ok: Vec<&Item> = self
            .items
            .iter()
            .filter(|i| self.unchanged(i).map_err(|e| refused.push(e)).is_ok())
            .collect();
        // Until none is dropped: a name is free only while the item leaving it still goes.
        loop {
            let leaving: Vec<PathBuf> = ok.iter().map(|i| i.to.clone()).collect();
            let before = ok.len();
            ok.retain(|i| {
                let free = Self::free_for(i).is_ok() || leaving.contains(&i.from);
                if !free {
                    refused.push(format!("{}: {}", i.from.display(), t!("undo.taken")));
                }
                free
            });
            if ok.len() == before {
                break;
            }
        }
        let Some(dir) = ok.first().and_then(|i| i.to.parent()) else {
            return (0, refused);
        };
        let plan: Vec<crate::rename::Planned> = ok
            .iter()
            .map(|i| crate::rename::Planned {
                from: name(&i.to),
                to: name(&i.from),
                conflict: None,
            })
            .collect();
        match crate::rename::apply(dir, &plan) {
            Ok(()) => (ok.len(), refused),
            Err(e) => {
                refused.push(e);
                (0, refused)
            }
        }
    }
}

/// A file operation as both apps run it on a list of items.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Copy,
    /// Moved, or renamed in place.
    Move,
    Extract,
    Trash,
    /// Deleted for good: the one that cannot be undone.
    Forever,
    /// A new folder, the one item given.
    Mkdir,
    /// Packed into the archive `dst`; a 7z's names hidden too with `hide_names`.
    Pack {
        hide_names: bool,
    },
}

impl Op {
    /// The folders an operation from `src` to `dst` changes the size of, to forget while it
    /// runs and measure again when it ends. A move, delete or new folder changes `src`; a
    /// copy, move or extract what is in `dst`, a pack `dst` itself.
    pub fn touches(self, src: &[PathBuf], dst: &Path) -> Vec<PathBuf> {
        let mut out = if matches!(self, Op::Move | Op::Trash | Op::Forever | Op::Mkdir) {
            src.to_vec()
        } else {
            vec![]
        };
        if !dst.as_os_str().is_empty() {
            out.push(match self {
                Op::Pack { .. } => dst.to_path_buf(),
                _ => dst.join("new"),
            });
        }
        out
    }
}

/// The title over what failed when `op` ran on `all` items (`what` names them) and `failed` of
/// them failed: for one item "Could not copy “a.txt”", for several how many went through and
/// how many did not, "3 of 4 copied; 1 could not be copied". A pack is one archive: when it
/// fails, none of its items went in.
pub fn failure_title(op: Op, what: &str, all: usize, failed: usize) -> String {
    let failed = if matches!(op, Op::Pack { .. }) { all } else { failed.min(all) };
    if all <= 1 {
        return match op {
            Op::Copy => crate::t!("error.copy", "what" => what),
            Op::Move => crate::t!("error.move", "what" => what),
            Op::Extract => crate::t!("error.extract", "what" => what),
            Op::Trash => crate::t!("error.trash", "what" => what),
            Op::Forever => crate::t!("error.delete", "what" => what),
            Op::Mkdir => crate::t!("error.create", "what" => what),
            Op::Pack { .. } => crate::t!("error.pack", "what" => what),
        };
    }
    let done = all - failed;
    match op {
        Op::Copy => crate::tn!("error.counted_copy", failed, "done" => crate::i18n::digits(done.to_string()), "all" => crate::i18n::digits(all.to_string())),
        Op::Move => crate::tn!("error.counted_move", failed, "done" => crate::i18n::digits(done.to_string()), "all" => crate::i18n::digits(all.to_string())),
        Op::Extract => crate::tn!("error.counted_extract", failed, "done" => crate::i18n::digits(done.to_string()), "all" => crate::i18n::digits(all.to_string())),
        Op::Trash => crate::tn!("error.counted_trash", failed, "done" => crate::i18n::digits(done.to_string()), "all" => crate::i18n::digits(all.to_string())),
        Op::Forever => crate::tn!("error.counted_delete", failed, "done" => crate::i18n::digits(done.to_string()), "all" => crate::i18n::digits(all.to_string())),
        Op::Mkdir => crate::t!("error.create", "what" => what),
        Op::Pack { .. } => crate::tn!("error.counted_pack", failed, "done" => crate::i18n::digits(done.to_string()), "all" => crate::i18n::digits(all.to_string())),
    }
}

/// What `run` did: what failed, with why; the record of what can be undone (empty for
/// `Forever`); and, when it was stopped, the line that says how far it got ("Stopped: 120 of
/// 400 copied").
pub struct Ran {
    pub failed: Vec<(PathBuf, io::Error)>,
    pub rec: Record,
    pub stopped: Option<String>,
}

/// Run `op` on every item in turn (`dst`: where to, or the archive for a pack), `at(i)`
/// before item `i`; `password` opens a locked archive. `stop`, set by the app's Stop, ends it
/// between items and inside a large file: the file it was on is removed (a move keeps its
/// original), what was done stays and can be undone, the rest is not started.
pub fn run(
    op: Op,
    items: &[PathBuf],
    dst: &Path,
    password: Option<&str>,
    stop: &Arc<AtomicBool>,
    at: impl FnMut(usize),
) -> Ran {
    crate::stop::during(stop, || run_items(op, items, dst, password, at))
}

fn run_items(
    op: Op,
    items: &[PathBuf],
    dst: &Path,
    password: Option<&str>,
    mut at: impl FnMut(usize),
) -> Ran {
    let mut rec = Record::new(match op {
        Op::Copy => Kind::Copy,
        Op::Move => Kind::Move,
        Op::Extract => Kind::Extract,
        Op::Trash | Op::Forever => Kind::Trash,
        Op::Mkdir => Kind::Mkdir,
        Op::Pack { .. } => Kind::Pack,
    });
    let stopped = |done: usize| Some(stopped_line(op, done, items.len()));
    if let Op::Pack { hide_names } = op {
        at(0);
        return match crate::archive::create_locked(dst, items, password, hide_names) {
            Ok(()) => {
                rec.done(dst, dst);
                Ran { failed: vec![], rec, stopped: None }
            }
            Err(e) if crate::stop::is(&e) => Ran { failed: vec![], rec, stopped: stopped(0) },
            // Failed for a cause of its own while Stop was pressed: both are said.
            Err(e) if crate::stop::asked() => Ran { failed: vec![(dst.to_path_buf(), e)], rec, stopped: stopped(0) },
            Err(e) => Ran { failed: vec![(dst.to_path_buf(), e)], rec, stopped: None },
        };
    }
    let mut failed = vec![];
    for (i, p) in items.iter().enumerate() {
        if crate::stop::asked() {
            return Ran { stopped: stopped(i - failed.len()), failed, rec };
        }
        at(i);
        let r = match op {
            Op::Forever => cfs::delete_locked(p, password),
            _ => rec.run(p, dst, password),
        };
        match r {
            Err(e) if crate::stop::is(&e) => {
                return Ran { stopped: stopped(i - failed.len()), failed, rec };
            }
            // Failed for a cause of its own while Stop was pressed: both are said.
            Err(e) if crate::stop::asked() => {
                let done = i - failed.len();
                failed.push((p.clone(), e));
                return Ran { stopped: stopped(done), failed, rec };
            }
            Err(e) => failed.push((p.clone(), e)),
            Ok(()) => {}
        }
    }
    Ran { failed, rec, stopped: None }
}

/// "Stopped: 120 of 400 copied": how far `op` got of `all` items when it was stopped.
pub fn stopped_line(op: Op, done: usize, all: usize) -> String {
    let key = match op {
        Op::Copy => "stop.done.copy",
        Op::Move => "stop.done.move",
        Op::Extract => "stop.done.extract",
        Op::Trash => "stop.done.trash",
        Op::Forever => "stop.done.delete",
        Op::Pack { .. } => "stop.done.pack",
        Op::Mkdir => return t!("stop.stopped"),
    };
    tn!(key, done, "all" => crate::i18n::digits(all.to_string()))
}

/// What undo did.
#[derive(Debug, serde::Serialize)]
pub struct Undone {
    /// What was undone, as `Record::label` says it.
    pub label: String,
    pub done: usize,
    /// One line for each item left as it is: its path and why.
    pub refused: Vec<String>,
    /// The items moved, renamed or trashed that are back where they were.
    pub back: Vec<PathBuf>,
    /// What it changed: `Record::touched`, for the sizes measured again.
    pub touched: Vec<PathBuf>,
}

/// The last operations, the newest last; for the app run only.
#[derive(Default, Debug)]
pub struct History {
    list: VecDeque<Record>,
    /// How many were kept so far: tells the desktop app's page that a new one came.
    pub pushed: u64,
}

impl History {
    /// Keep `r` when anything in it can be undone; says whether it was kept.
    pub fn push(&mut self, r: Record) -> bool {
        if r.is_empty() {
            return false;
        }
        if self.list.len() == KEEP
            && let Some(old) = self.list.pop_front()
        {
            old.forget();
        }
        self.list.push_back(r);
        self.pushed += 1;
        true
    }

    pub fn pop(&mut self) -> Option<Record> {
        self.list.pop_back()
    }

    /// What Ctrl+Z would undo.
    pub fn next(&self) -> Option<&Record> {
        self.list.back()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> crate::fs::Scratch {
        crate::fs::Scratch::named(&format!("undo-{name}"))
    }

    /// `super::run`, never stopped, as the tests before Stop took it.
    fn run(
        op: Op,
        items: &[PathBuf],
        dst: &Path,
        password: Option<&str>,
        at: impl FnMut(usize),
    ) -> (Vec<(PathBuf, io::Error)>, Record) {
        let r = super::run(op, items, dst, password, &Arc::default(), at);
        (r.failed, r.rec)
    }

    #[test]
    fn run_goes_on_past_a_failed_item() {
        let d = tmp("run");
        fs::create_dir(d.join("to")).unwrap();
        fs::write(d.join("a.txt"), "a").unwrap();
        let (items, missing) = (
            vec![d.join("a.txt"), d.join("gone.txt")],
            d.join("gone.txt"),
        );
        let mut seen = vec![];
        let (failed, rec) = run(Op::Copy, &items, &d.join("to"), None, |i| seen.push(i));
        assert_eq!(seen, [0, 1]);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, missing);
        assert!(d.join("to/a.txt").is_file());
        assert_eq!((rec.kind(), rec.items.len()), (Kind::Copy, 1));

        let (failed, rec) = run(Op::Move, &items, &d.join("to/sub"), None, |_| {});
        assert_eq!(failed.len(), 1);
        assert!(!d.join("a.txt").exists());
        assert_eq!((rec.kind(), rec.items.len()), (Kind::Move, 1));

        let zip = d.join("p.zip");
        let (failed, rec) = run(
            Op::Pack { hide_names: true },
            &[d.join("to")],
            &zip,
            None,
            |_| {},
        );
        assert!(failed.is_empty() && zip.is_file());
        assert_eq!((rec.kind(), rec.items.len()), (Kind::Pack, 1));
        let (failed, rec) = run(
            Op::Pack { hide_names: true },
            std::slice::from_ref(&missing),
            &d.join("q.zip"),
            None,
            |_| {},
        );
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, d.join("q.zip"));
        assert!(rec.is_empty());

        let (failed, rec) = run(
            Op::Forever,
            &[zip.clone(), missing],
            Path::new(""),
            None,
            |_| {},
        );
        assert_eq!(failed.len(), 1);
        assert!(!zip.exists());
        assert!(rec.is_empty(), "for good is not undone");

        let (failed, rec) = run(
            Op::Mkdir,
            &[d.join("new/deep")],
            Path::new(""),
            None,
            |_| {},
        );
        assert!(failed.is_empty() && d.join("new/deep").is_dir());
        assert_eq!(rec.items.len(), 2);
    }

    #[test]
    fn touches_names_the_folders_that_change() {
        let (src, dst) = (vec![PathBuf::from("/a/x")], Path::new("/b"));
        assert_eq!(Op::Copy.touches(&src, dst), [PathBuf::from("/b/new")]);
        assert_eq!(
            Op::Move.touches(&src, dst),
            [PathBuf::from("/a/x"), PathBuf::from("/b/new")]
        );
        assert_eq!(Op::Forever.touches(&src, Path::new("")), src);
        assert_eq!(
            Op::Pack { hide_names: false }.touches(&src, Path::new("/b/p.zip")),
            [PathBuf::from("/b/p.zip")]
        );
    }

    #[test]
    fn undo_move_and_its_refusals() {
        let d = tmp("move");
        fs::create_dir(d.join("docs")).unwrap();
        for f in ["a.txt", "b.txt", "c.txt"] {
            fs::write(d.join(f), f).unwrap();
        }
        let mut r = Record::new(Kind::Move);
        for f in ["a.txt", "b.txt", "c.txt"] {
            r.run(&d.join(f), &d.join("docs"), None).unwrap();
        }
        assert_eq!(r.label(), "move 3 items to docs");
        // b changed since, the place c came from is taken again.
        fs::write(d.join("docs/b.txt"), "changed").unwrap();
        fs::write(d.join("c.txt"), "new c").unwrap();
        let u = r.undo(|_| {});
        assert_eq!(u.done, 1);
        assert_eq!(u.back, [d.join("a.txt")], "only what came back");
        assert_eq!(u.refused.len(), 2, "{:?}", u.refused);
        assert!(
            u.refused
                .iter()
                .any(|e| e.contains("b.txt") && e.contains(&t!("undo.changed")))
        );
        assert!(
            u.refused
                .iter()
                .any(|e| e.contains("c.txt") && e.contains(&t!("undo.taken")))
        );
        assert_eq!(fs::read_to_string(d.join("a.txt")).unwrap(), "a.txt");
        assert_eq!(
            fs::read_to_string(d.join("c.txt")).unwrap(),
            "new c",
            "never overwritten"
        );
        assert!(d.join("docs/c.txt").exists() && d.join("docs/b.txt").exists());
    }

    #[test]
    fn undo_rename_in_place() {
        let d = tmp("rename");
        fs::write(d.join("a.txt"), "x").unwrap();
        let mut r = Record::new(Kind::Move);
        r.run(&d.join("a.txt"), &d.join("b.txt"), None).unwrap();
        assert_eq!(r.label(), "rename a.txt → b.txt");
        assert_eq!(r.undo(|_| {}).done, 1);
        assert!(d.join("a.txt").exists() && !d.join("b.txt").exists());
    }

    #[test]
    fn undo_batch_rename_swaps_back() {
        let d = tmp("batch");
        fs::write(d.join("a"), "A").unwrap();
        fs::write(d.join("b"), "B").unwrap();
        fs::write(d.join("c"), "C").unwrap();
        let plan = [("a", "b"), ("b", "a"), ("c", "d")].map(|(f, t)| crate::rename::Planned {
            from: f.into(),
            to: t.into(),
            conflict: None,
        });
        crate::rename::apply(&d, &plan).unwrap();
        let mut r = Record::new(Kind::Rename);
        plan.iter()
            .for_each(|p| r.done(&d.join(&p.from), &d.join(&p.to)));
        // The name c gave up is taken since.
        fs::write(d.join("c"), "new").unwrap();
        let u = r.undo(|_| {});
        assert_eq!((u.done, u.refused.len()), (2, 1), "{:?}", u.refused);
        assert_eq!(fs::read_to_string(d.join("a")).unwrap(), "A");
        assert_eq!(fs::read_to_string(d.join("b")).unwrap(), "B");
        assert_eq!(fs::read_to_string(d.join("c")).unwrap(), "new");
        assert!(d.join("d").exists());
    }

    #[test]
    fn undo_mkdir_only_when_empty() {
        let d = tmp("mkdir");
        let mut r = Record::new(Kind::Mkdir);
        r.run(&d.join("x/y"), Path::new(""), None).unwrap();
        assert_eq!(r.label(), "new folder \"x\"");
        let u = r.undo(|_| {});
        assert_eq!(u.done, 2);
        assert!(!d.join("x").exists());
        let mut r = Record::new(Kind::Mkdir);
        r.run(&d.join("z"), Path::new(""), None).unwrap();
        fs::write(d.join("z/f"), "").unwrap();
        let u = r.undo(|_| {});
        assert_eq!(u.done, 0);
        assert!(u.refused[0].contains(&t!("undo.not_empty")));
        assert!(d.join("z/f").exists());
    }

    /// What undo takes away goes to the trash, so these touch the real one: only in CI.
    fn in_ci() -> bool {
        std::env::var_os("CI").is_some()
    }

    #[test]
    fn undo_copy_refuses_a_changed_copy() {
        if !in_ci() {
            return;
        }
        let d = tmp("copy");
        if !cfs::trash_takes(std::slice::from_ref(&d.to_path_buf())) {
            return;
        }
        fs::create_dir_all(d.join("src/deep")).unwrap();
        fs::create_dir(d.join("out")).unwrap();
        fs::write(d.join("src/deep/f.txt"), "one").unwrap();
        fs::write(d.join("src/g.txt"), "two").unwrap();
        let mut r = Record::new(Kind::Copy);
        r.run(&d.join("src/deep"), &d.join("out"), None).unwrap();
        r.run(&d.join("src/g.txt"), &d.join("out"), None).unwrap();
        // A file inside the copied folder changed: that copy stays.
        fs::write(d.join("out/deep/f.txt"), "edited").unwrap();
        let u = r.undo(|_| {});
        assert_eq!(u.done, 1, "{:?}", u.refused);
        assert!(!d.join("out/g.txt").exists() && d.join("out/deep/f.txt").exists());
        assert!(d.join("src/g.txt").exists(), "the original stays");
    }

    #[test]
    fn copy_into_a_folder_that_was_there_is_not_undone() {
        let d = tmp("merge");
        fs::create_dir_all(d.join("src/deep")).unwrap();
        fs::create_dir_all(d.join("out/deep")).unwrap();
        fs::write(d.join("out/deep/mine.txt"), "keep").unwrap();
        let mut r = Record::new(Kind::Copy);
        r.run(&d.join("src/deep"), &d.join("out"), None).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn undo_trash_restores() {
        if !in_ci() || !trash_restores() {
            return;
        }
        // In the folder of the tests, which is on the disk the user's trash is on.
        let d = std::env::current_dir()
            .unwrap()
            .join(format!("target-undo-trash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        if !cfs::trash_takes(std::slice::from_ref(&d)) {
            return fs::remove_dir_all(&d).unwrap();
        }
        fs::write(d.join("gone.txt"), "back").unwrap();
        let mut r = Record::new(Kind::Trash);
        r.run(&d.join("gone.txt"), Path::new(""), None).unwrap();
        assert!(!d.join("gone.txt").exists());
        let u = r.undo(|_| {});
        assert_eq!(u.done, 1, "{:?}", u.refused);
        assert_eq!(
            fs::read_to_string(d.join("gone.txt")).unwrap(),
            "back",
            "back under its whole name"
        );
        // Trashed again and its place taken: refused, nothing overwritten.
        let mut r = Record::new(Kind::Trash);
        r.run(&d.join("gone.txt"), Path::new(""), None).unwrap();
        fs::write(d.join("gone.txt"), "new").unwrap();
        let u = r.undo(|_| {});
        assert_eq!((u.done, u.refused.len()), (0, 1));
        assert_eq!(fs::read_to_string(d.join("gone.txt")).unwrap(), "new");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn history_keeps_the_last_ones() {
        let d = tmp("history");
        let mut h = History::default();
        assert!(
            !h.push(Record::new(Kind::Copy)),
            "nothing to undo is not kept"
        );
        for i in 0..KEEP + 3 {
            let mut r = Record::new(Kind::Mkdir);
            r.run(&d.join(i.to_string()), Path::new(""), None).unwrap();
            assert!(h.push(r));
        }
        assert_eq!(h.list.len(), KEEP);
        assert_eq!(
            h.next().unwrap().label(),
            format!("new folder \"{}\"", KEEP + 2)
        );
        assert_eq!(
            h.pop().unwrap().label(),
            format!("new folder \"{}\"", KEEP + 2)
        );
        assert_eq!(h.pushed, KEEP as u64 + 3);
    }

    #[test]
    fn failure_title_counts_what_went_through() {
        crate::i18n::set_language("en-GB");
        assert_eq!(failure_title(Op::Copy, "\"a\"", 1, 1), "Could not copy \"a\"");
        assert_eq!(failure_title(Op::Copy, "4 items", 4, 1), "3 of 4 copied; 1 could not be copied");
        assert_eq!(failure_title(Op::Trash, "2 items", 2, 2), "0 of 2 moved to the bin; 2 could not be moved to the bin");
        // A pack is one archive: all or nothing.
        assert_eq!(failure_title(Op::Pack { hide_names: true }, "3 items", 3, 1), "0 of 3 packed; 3 could not be packed");
    }

    // ------------------------------------------------------------ Stop

    /// A tree to work on: small files, a folder, and a file large enough to be copied in
    /// steps (`fs::STEP`), with zeros in it (a hole) and bytes around them.
    fn stop_tree(d: &Path) -> Vec<PathBuf> {
        let src = d.join("src");
        fs::create_dir_all(src.join("sub/deep")).unwrap();
        fs::write(src.join("a.txt"), "a").unwrap();
        fs::write(src.join("sub/b.txt"), "bb").unwrap();
        fs::write(src.join("sub/deep/d.txt"), "ddd").unwrap();
        let mut big = vec![7u8; 3 << 20];
        big.extend(vec![0u8; 6 << 20]);
        big.extend(vec![9u8; 3 << 20]);
        fs::write(src.join("sub/big.bin"), &big).unwrap();
        fs::write(src.join("c.txt"), "c").unwrap();
        vec![src.join("a.txt"), src.join("sub"), src.join("c.txt")]
    }

    /// Every file below `root`, by its path inside, with its bytes' hash.
    fn files(root: &Path) -> std::collections::BTreeMap<PathBuf, blake3::Hash> {
        let mut out = std::collections::BTreeMap::new();
        fn walk(root: &Path, at: &Path, out: &mut std::collections::BTreeMap<PathBuf, blake3::Hash>) {
            for e in fs::read_dir(at).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    out.insert(p.strip_prefix(root).unwrap().to_path_buf(), blake3::hash(&fs::read(&p).unwrap()));
                }
            }
        }
        if root.exists() {
            walk(root, root, &mut out);
        }
        out
    }

    /// Stop at the `k`th look, for every `k` until the operation ends before it: 1, 2, 3, 5, 8 …
    fn each_stop(mut f: impl FnMut(u64) -> bool) {
        let mut k = 1;
        while f(k) {
            k = k * 3 / 2 + 1;
            assert!(k < 1 << 20, "never ended");
        }
    }

    fn stopped_at(k: u64, op: Op, items: &[PathBuf], dst: &Path) -> Ran {
        crate::stop::TRIP.set(k);
        let r = super::run(op, items, dst, None, &Arc::default(), |_| {});
        crate::stop::TRIP.set(0);
        r
    }

    #[test]
    fn a_stopped_copy_leaves_no_half_file_and_keeps_what_is_done() {
        let mut cut_in_big = false;
        each_stop(|k| {
            let d = tmp(&format!("stop-copy-{k}"));
            let items = stop_tree(&d);
            let (src, to) = (d.join("src"), d.join("to"));
            fs::create_dir(&to).unwrap();
            let before = files(&src);
            let r = stopped_at(k, Op::Copy, &items, &to);
            assert!(r.failed.is_empty(), "{:?}", r.failed);
            let copied = files(&to);
            // What is there is whole: no file half copied, no folder under a passing name.
            for (p, h) in &copied {
                assert_eq!(before.get(p), Some(h), "{k}: {p:?} is not the original");
            }
            assert!(fs::read_dir(&to).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("coxswain-tmp")));
            // Undo has each item it left.
            assert_eq!(r.rec.items.len(), fs::read_dir(&to).unwrap().count(), "{k}");
            cut_in_big |= r.stopped.is_some() && to.join("sub").exists() && !to.join("sub/big.bin").exists();
            let go_on = r.stopped.is_some();
            if !go_on {
                assert_eq!(copied, before, "all of it when not stopped");
            }
            go_on
        });
        assert!(cut_in_big, "one stop came inside the folder, before the large file was whole");
    }

    /// A private file's copy is never readable by others: not while it is written, not when it
    /// is stopped half way, not when it is whole.
    #[cfg(unix)]
    #[test]
    fn a_private_file_copied_or_stopped_stays_private() {
        use std::os::unix::fs::PermissionsExt;
        each_stop(|k| {
            let d = tmp(&format!("stop-private-{k}"));
            let key = d.join("key.bin");
            fs::write(&key, vec![5u8; 20 << 20]).unwrap();
            fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
            let to = d.join("to");
            fs::create_dir(&to).unwrap();
            // Looked at from inside the copy: each Stop look sees the half copy as it is.
            let copy = to.join("key.bin");
            let seen = to.clone();
            crate::stop::LOOK.set(Some(Box::new(move || {
                for e in fs::read_dir(&seen).unwrap().flatten() {
                    let mode = e.metadata().unwrap().permissions().mode();
                    assert_eq!(mode & 0o077, 0, "{:?} readable by others while it is written", e.file_name());
                }
            })));
            let r = stopped_at(k, Op::Copy, std::slice::from_ref(&key), &to);
            crate::stop::LOOK.set(None);
            if let Ok(m) = fs::metadata(&copy) {
                assert_eq!(m.permissions().mode() & 0o077, 0, "{k}: readable by others");
            }
            if r.stopped.is_some() {
                assert_eq!(fs::read_dir(&to).unwrap().count(), 0, "{k}: no half copy, under any name");
            } else {
                assert_eq!(fs::metadata(&copy).unwrap().permissions().mode() & 0o777, 0o600);
            }
            r.stopped.is_some()
        });
    }

    #[test]
    fn a_stopped_move_across_disks_has_every_file_in_one_place_and_undo_takes_back_the_rest() {
        cfs::ACROSS.set(true);
        let mut halves = 0;
        each_stop(|k| {
            let d = tmp(&format!("stop-move-{k}"));
            let items = stop_tree(&d);
            let (src, to) = (d.join("src"), d.join("to"));
            fs::create_dir(&to).unwrap();
            let before = files(&src);
            let r = stopped_at(k, Op::Move, &items, &to);
            assert!(r.failed.is_empty(), "{:?}", r.failed);
            let (left, moved) = (files(&src), files(&to));
            // Each file in one place, whole.
            for (p, h) in &before {
                match (left.get(p), moved.get(p)) {
                    (Some(a), None) | (None, Some(a)) => assert_eq!(a, h, "{k}: {p:?}"),
                    other => panic!("{k}: {p:?} is in {other:?}"),
                }
            }
            assert_eq!(left.len() + moved.len(), before.len(), "{k}: nothing more");
            halves += usize::from(!left.is_empty() && !moved.is_empty());
            let go_on = r.stopped.is_some();
            // Undo puts back exactly what went, and the folders made for it go.
            let u = r.rec.undo(|_| {});
            assert!(u.refused.is_empty(), "{k}: {:?}", u.refused);
            assert_eq!(files(&src), before, "{k}: all back");
            assert_eq!(fs::read_dir(&to).unwrap().count(), 0, "{k}: nothing left over there");
            go_on
        });
        cfs::ACROSS.set(false);
        assert!(halves > 0, "some stops came half way");
    }

    /// A move across disks of the folder `src` in `d` into `d/to`, with copy number `n` failing
    /// for each bit `n` of `full` (a full disk).
    fn move_with_full_disk(d: &Path, full: u64) -> Ran {
        fs::create_dir(d.join("to")).unwrap();
        cfs::ACROSS.set(true);
        cfs::COPIES.set(0);
        cfs::FULL.set(full);
        let r = super::run(Op::Move, &[d.join("src")], &d.join("to"), None, &Arc::default(), |_| {});
        cfs::FULL.set(0);
        r
    }

    #[test]
    fn a_move_across_disks_that_fails_puts_back_what_went() {
        // Five files: the disk is full at each of them in turn.
        for n in 1..=5 {
            let d = tmp(&format!("full-move-{n}"));
            stop_tree(&d);
            let before = files(&d.join("src"));
            let r = move_with_full_disk(&d, 1 << n);
            cfs::ACROSS.set(false);
            assert_eq!(r.failed.len(), 1, "{n}");
            assert_eq!(r.failed[0].1.kind(), io::ErrorKind::StorageFull, "{n}");
            assert_eq!(files(&d.join("src")), before, "{n}: the source is whole again");
            assert_eq!(fs::read_dir(d.join("to")).unwrap().count(), 0, "{n}: nothing left over there");
            assert!(r.rec.items.is_empty() && r.rec.made.is_empty(), "{n}: nothing to undo");
        }
    }

    #[test]
    fn a_failed_move_across_disks_that_cannot_all_go_back_says_where_and_undo_has_the_rest() {
        let d = tmp("full-move-back");
        stop_tree(&d);
        let before = files(&d.join("src"));
        // The third file fails, and so does the first one put back.
        let r = move_with_full_disk(&d, (1 << 3) | (1 << 4));
        assert_eq!(r.failed.len(), 1);
        let msg = r.failed[0].1.to_string();
        assert!(msg.contains(&d.join("src").display().to_string()), "{msg}");
        assert!(msg.contains(&d.join("to").join("src").display().to_string()), "{msg}");
        let (left, moved) = (files(&d.join("src")), files(&d.join("to/src")));
        assert!(!moved.is_empty(), "some of it stayed over there");
        for (p, h) in &before {
            match (left.get(p), moved.get(p)) {
                (Some(a), None) | (None, Some(a)) => assert_eq!(a, h, "{p:?}"),
                other => panic!("{p:?} is in {other:?}"),
            }
        }
        assert!(!r.rec.items.is_empty(), "undo has what stayed over there");
        let u = r.rec.undo(|_| {});
        cfs::ACROSS.set(false);
        assert!(u.refused.is_empty(), "{:?}", u.refused);
        assert_eq!(files(&d.join("src")), before, "all back");
        assert_eq!(fs::read_dir(d.join("to")).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_move_across_disks_of_a_tree_with_a_pipe_leaves_the_tree_whole() {
        use std::os::unix::{ffi::OsStrExt, fs::FileTypeExt};
        let d = tmp("pipe-move");
        stop_tree(&d);
        let before = files(&d.join("src"));
        let pipe = d.join("src/sub/zz.pipe");
        let c = std::ffi::CString::new(pipe.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let r = move_with_full_disk(&d, 0);
        cfs::ACROSS.set(false);
        assert_eq!(r.failed.len(), 1);
        assert_eq!(r.failed[0].1.kind(), io::ErrorKind::Unsupported);
        assert!(fs::symlink_metadata(&pipe).unwrap().file_type().is_fifo());
        fs::remove_file(&pipe).unwrap();
        assert_eq!(files(&d.join("src")), before);
        assert_eq!(fs::read_dir(d.join("to")).unwrap().count(), 0);
        assert!(r.rec.items.is_empty());
    }

    /// The same file under a name of its own (a hard link here, another mount point for real):
    /// a move across disks onto it is refused, never a copy onto itself that empties it.
    #[cfg(unix)]
    #[test]
    fn a_move_across_disks_onto_the_same_file_keeps_it() {
        let d = tmp("same-inode");
        fs::write(d.join("a.txt"), "keep me").unwrap();
        fs::hard_link(d.join("a.txt"), d.join("b.txt")).unwrap();
        cfs::ACROSS.set(true);
        let r = cfs::rename(&d.join("a.txt"), &d.join("b.txt"));
        cfs::ACROSS.set(false);
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(d.join("a.txt")).unwrap(), "keep me");
        assert_eq!(fs::read_to_string(d.join("b.txt")).unwrap(), "keep me");
    }

    #[test]
    fn a_stopped_extract_keeps_what_came_out_whole() {
        let d = tmp("stop-extract");
        let items = stop_tree(&d);
        // Each archive holds each item under its name, as the source folder has it.
        let before = files(&d.join("src"));
        for name in ["tree.zip", "tree.tar.gz", "tree.7z"] {
            let archive = d.join(name);
            crate::archive::create(&archive, &items).unwrap();
            each_stop(|k| {
                let to = d.join(format!("out-{name}-{k}"));
                fs::create_dir(&to).unwrap();
                let r = stopped_at(k, Op::Extract, std::slice::from_ref(&archive), &to);
                assert!(r.failed.is_empty(), "{name} {k}: {:?}", r.failed);
                for (p, h) in files(&to.join("tree")) {
                    assert_eq!(before.get(&p), Some(&h), "{name} {k}: {p:?} came out half");
                }
                assert_eq!(r.rec.items.len(), usize::from(to.join("tree").exists()), "{name} {k}: undo has it");
                r.stopped.is_some()
            });
        }
    }

    #[test]
    fn a_stopped_pack_leaves_the_archive_as_it_was() {
        let d = tmp("stop-pack");
        let items = stop_tree(&d);
        let zip = d.join("old.zip");
        crate::archive::create(&zip, &items[..1]).unwrap();
        let old = fs::read(&zip).unwrap();
        let leftovers = || {
            fs::read_dir(&*d).unwrap().flatten().filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.ends_with(".coxswain-tmp") || n.ends_with(".coxswain-tar")
            }).count()
        };
        // Added to one that is there.
        let mut stops = 0;
        each_stop(|k| {
            let r = stopped_at(k, Op::Copy, &items[1..2], &zip);
            if r.stopped.is_none() {
                return false;
            }
            stops += 1;
            assert!(r.failed.is_empty(), "{:?}", r.failed);
            assert!(fs::read(&zip).unwrap() == old, "{k}: the archive is as it was");
            assert_eq!(leftovers(), 0, "{k}");
            assert!(r.rec.is_empty());
            true
        });
        assert!(stops > 2);
        // A new one: not made at all, of every kind.
        for name in ["new.zip", "new.7z", "new.tar.gz"] {
            each_stop(|k| {
                let to = d.join(name);
                let r = stopped_at(k, Op::Pack { hide_names: false }, &items, &to);
                if r.stopped.is_none() {
                    assert!(to.is_file());
                    fs::remove_file(&to).unwrap();
                    return false;
                }
                assert!(!to.exists(), "{name} {k}");
                assert_eq!(leftovers(), 0, "{name} {k}");
                assert!(r.stopped.unwrap().contains('3'));
                true
            });
        }
    }

    #[test]
    fn a_stopped_delete_stops_between_items() {
        let d = tmp("stop-delete");
        let items: Vec<PathBuf> = ["a", "b", "c"].iter().map(|n| d.join(n)).collect();
        for p in &items {
            fs::write(p, "x").unwrap();
        }
        let stop = Arc::new(AtomicBool::new(false));
        let r = super::run(Op::Forever, &items, Path::new(""), None, &stop, |i| {
            if i == 1 {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        // The one it was on is done; the next is not started.
        assert!(!items[0].exists() && !items[1].exists() && items[2].exists());
        assert_eq!(r.stopped, Some(stopped_line(Op::Forever, 2, 3)));
        // To the bin, where the trash gives back (in CI: the developer's own trash is left alone).
        if !in_ci() || !trash_restores() || !cfs::trash_takes(&items) {
            return;
        }
        let d = std::env::current_dir().unwrap().join(format!("target-stop-trash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let items: Vec<PathBuf> = ["a", "b", "c"].iter().map(|n| d.join(n)).collect();
        for p in &items {
            fs::write(p, "x").unwrap();
        }
        let stop = Arc::new(AtomicBool::new(false));
        let r = super::run(Op::Trash, &items, Path::new(""), None, &stop, |i| {
            if i == 1 {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        assert!(!items[1].exists() && items[2].exists());
        assert_eq!(r.stopped, Some(stopped_line(Op::Trash, 2, 3)));
        assert_eq!(r.rec.undo(|_| {}).done, 2, "undo brings back the two");
        assert!(items.iter().all(|p| p.exists()));
        fs::remove_dir_all(&d).unwrap();
    }

}

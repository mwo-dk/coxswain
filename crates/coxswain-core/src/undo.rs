//! Undo (Ctrl+Z) for the last file operations: a short history, for the app run, of what each
//! operation left, and the way back. Before anything goes back, each item is checked to be still
//! as the operation left it; what changed since is refused, with a reason, and the rest is
//! undone. Nothing is overwritten, and what undo takes away goes to the trash.

use crate::{fs as cfs, t, tn};
use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
    let mut s = Stamp { size: m.len(), modified: m.modified().ok(), entries: 1 };
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
    /// its name in Termux's trash, else empty.
    to: PathBuf,
    stamp: Option<Stamp>,
}

/// One operation, as far as it can be undone.
#[derive(Clone, Debug)]
pub struct Record {
    kind: Kind,
    items: Vec<Item>,
    /// When it started, in seconds since the epoch: what the trash holds from before is not its.
    since: i64,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The trash gives back what went into it: Windows, and the freedesktop.org trash (Linux, the
/// BSDs, illumos), and Termux's own. Not on a Mac, where the system offers no way back to an app,
/// nor in a Flatpak, whose trash is the host's.
pub fn trash_restores() -> bool {
    !cfg!(any(target_os = "macos", target_os = "ios")) && crate::tools::flatpak().is_none()
}

/// Put `path` back from the trash; `name`: its name in Termux's trash.
#[cfg(any(windows, all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android"))))]
fn restore(path: &Path, _name: &Path, since: i64) -> io::Result<()> {
    use trash::os_limited;
    let want = comparable(path);
    // ponytail: two trashings of one path within a second are told apart by nothing; the newest
    // listed wins.
    let file = path.file_name().unwrap_or_default();
    // Windows lists a name as Explorer shows it, without a known ending (`report` for
    // `report.pdf`); the ending is still on the item's place in the Recycle Bin.
    let shown_short = |i: &trash::TrashItem| {
        cfg!(windows) && Path::new(&i.name).file_stem().is_some() && Some(i.name.as_os_str()) == Path::new(file).file_stem() && Path::new(&i.id).extension() == Path::new(file).extension()
    };
    let mut item = os_limited::list()
        .map_err(io::Error::other)?
        .into_iter()
        .filter(|i| i.time_deleted >= since && comparable(&i.original_parent.join(if shown_short(i) { file } else { i.name.as_os_str() })) == want)
        .max_by_key(|i| i.time_deleted)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, t!("undo.not_in_trash")))?;
    // Restored under its whole name, which is also what the collision check looks at.
    item.name = file.to_os_string();
    os_limited::restore_all([item]).map_err(|e| match e {
        trash::Error::RestoreCollision { .. } => io::Error::new(io::ErrorKind::AlreadyExists, t!("undo.taken")),
        e => io::Error::other(e),
    })
}

#[cfg(target_os = "android")]
fn restore(path: &Path, name: &Path, _since: i64) -> io::Result<()> {
    crate::xdg_trash::restore(&name.to_string_lossy(), path)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn restore(_path: &Path, _name: &Path, _since: i64) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, t!("undo.no_restore")))
}

/// A path as the trash records it: the folder resolved, as the trash crate does; on Windows
/// without `\\?\` and in one case.
#[cfg(any(windows, all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android"))))]
fn comparable(p: &Path) -> PathBuf {
    let p = match (p.parent().and_then(|d| fs::canonicalize(d).ok()), p.file_name()) {
        (Some(d), Some(n)) => d.join(n),
        _ => p.to_path_buf(),
    };
    #[cfg(windows)]
    let p = {
        let s = p.to_string_lossy();
        let s = s.strip_prefix(r"\\?\UNC\").map(|r| format!(r"\\{r}")).or_else(|| s.strip_prefix(r"\\?\").map(str::to_string)).unwrap_or_else(|| s.to_string());
        PathBuf::from(s.to_lowercase())
    };
    p
}

impl Record {
    pub fn new(kind: Kind) -> Record {
        Record { kind, items: vec![], since: now() - 1 }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
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
        self.items.push(Item { from: from.to_path_buf(), to: to.to_path_buf(), stamp });
    }

    /// The operation on `src`, as `fs` and `archive` do it (`dst`: where to, for a copy, a move
    /// or an extract), keeping what it left when that can be undone: not what went into or came
    /// out of an archive for a move, not what was merged into a folder that was there.
    pub fn run(&mut self, src: &Path, dst: &Path, password: Option<&str>) -> io::Result<()> {
        let on_disk = |p: &Path| fs::symlink_metadata(p).is_ok();
        match self.kind {
            Kind::Copy => {
                let fresh = !on_disk(&cfs::target(src, dst));
                let to = cfs::copy_locked(src, dst, password)?;
                if fresh && on_disk(&to) {
                    self.done(src, &to);
                }
            }
            Kind::Move | Kind::Rename => {
                let real = on_disk(src);
                let to = cfs::rename_locked(src, dst, password)?;
                if real && on_disk(&to) {
                    self.done(src, &to);
                }
            }
            Kind::Extract => {
                let to = crate::archive::extract_locked(src, dst, password)?;
                self.done(src, &to);
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
                let mut made: Vec<PathBuf> = src.ancestors().take_while(|a| !a.as_os_str().is_empty() && !on_disk(a)).map(Path::to_path_buf).collect();
                cfs::mkdir_locked(src, password)?;
                if src.is_dir() {
                    made.reverse();
                    made.iter().for_each(|p| self.done(p, p));
                }
            }
            Kind::Pack => return Err(io::Error::new(io::ErrorKind::InvalidInput, "a pack is done with archive::create_locked")),
        }
        Ok(())
    }

    /// The paths it touches, to read again after it is undone.
    pub fn touched(&self) -> Vec<PathBuf> {
        let trash = self.kind == Kind::Trash;
        self.items.iter().flat_map(|i| [Some(i.from.clone()), (!trash).then(|| i.to.clone())]).flatten().collect()
    }

    /// What it was, for "Undo: …" and "Undone: …": *rename a.txt → b.txt*, *move 3 items to
    /// Documents*.
    pub fn label(&self) -> String {
        let n = self.items.len();
        let what = |p: &Path| if n == 1 { format!("\"{}\"", name(p)) } else { tn!("items", n) };
        let Some(first) = self.items.first() else { return String::new() };
        let into = |p: &Path| p.parent().map(name).unwrap_or_default();
        match self.kind {
            Kind::Move | Kind::Rename if self.items.iter().all(|i| i.from.parent() == i.to.parent()) => {
                if n == 1 {
                    t!("undo.what.rename_one", "from" => name(&first.from), "to" => name(&first.to))
                } else {
                    t!("undo.what.rename", "what" => what(&first.from))
                }
            }
            Kind::Move | Kind::Rename => t!("undo.what.move", "what" => what(&first.from), "dir" => into(&first.to)),
            Kind::Copy => t!("undo.what.copy", "what" => what(&first.from), "dir" => into(&first.to)),
            Kind::Mkdir => t!("undo.what.mkdir", "what" => format!("\"{}\"", name(&first.from))),
            Kind::Trash => t!("undo.what.trash", "what" => what(&first.from)),
            Kind::Pack => t!("undo.what.pack", "what" => format!("\"{}\"", name(&first.to))),
            Kind::Extract => t!("undo.what.extract", "what" => what(&first.from), "dir" => into(&first.to)),
        }
    }

    /// Undo it, the last item first; `at` hears which item is next. What is no longer as the
    /// operation left it is refused, each with a line saying why.
    pub fn undo(&self, mut at: impl FnMut(usize)) -> Undone {
        let mut refused = vec![];
        let mut done = 0;
        if self.kind == Kind::Rename {
            let (n, r) = self.undo_renames();
            (done, refused) = (n, r);
        } else {
            for (i, item) in self.items.iter().enumerate().rev() {
                at(self.items.len() - 1 - i);
                match self.undo_one(item) {
                    Ok(()) => done += 1,
                    Err(why) => refused.push(why),
                }
            }
        }
        Undone { label: self.label(), done, refused }
    }

    /// Whether `item`'s result is still as the operation left it.
    fn unchanged(&self, item: &Item) -> Result<(), String> {
        let deep = matches!(self.kind, Kind::Copy | Kind::Pack | Kind::Extract);
        match stamp(&item.to, deep) {
            None => Err(format!("{}: {}", item.to.display(), t!("undo.gone"))),
            Some(s) if Some(s) != item.stamp => Err(format!("{}: {}", item.to.display(), t!("undo.changed"))),
            Some(_) => Ok(()),
        }
    }

    fn free(path: &Path) -> Result<(), String> {
        if fs::symlink_metadata(path).is_ok() { Err(format!("{}: {}", path.display(), t!("undo.taken"))) } else { Ok(()) }
    }

    /// Where `item` goes back to is free, or is the item itself: a rename that changed only the
    /// case, where case does not count.
    fn free_for(item: &Item) -> Result<(), String> {
        if cfs::same_file(&item.from, &item.to) { Ok(()) } else { Self::free(&item.from) }
    }

    fn undo_one(&self, item: &Item) -> Result<(), String> {
        let err = |p: &Path, e: io::Error| format!("{}: {e}", p.display());
        match self.kind {
            Kind::Move | Kind::Rename => {
                self.unchanged(item)?;
                Self::free_for(item)?;
                cfs::rename(&item.to, &item.from).map(drop).map_err(|e| err(&item.to, e))
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
        }
    }

    /// A batch rename goes back together, through `rename::apply`, so that names that were
    /// swapped are swapped back. A name taken since is refused; one that another item gives up
    /// is free.
    fn undo_renames(&self) -> (usize, Vec<String>) {
        let mut refused = vec![];
        let mut ok: Vec<&Item> = self.items.iter().filter(|i| self.unchanged(i).map_err(|e| refused.push(e)).is_ok()).collect();
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
        let Some(dir) = ok.first().and_then(|i| i.to.parent()) else { return (0, refused) };
        let plan: Vec<crate::rename::Planned> = ok.iter().map(|i| crate::rename::Planned { from: name(&i.to), to: name(&i.from), conflict: None }).collect();
        match crate::rename::apply(dir, &plan) {
            Ok(()) => (ok.len(), refused),
            Err(e) => {
                refused.push(e);
                (0, refused)
            }
        }
    }
}

/// What undo did.
#[derive(Debug)]
pub struct Undone {
    /// What was undone, as `Record::label` says it.
    pub label: String,
    pub done: usize,
    /// One line for each item left as it is: its path and why.
    pub refused: Vec<String>,
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
        if self.list.len() == KEEP {
            self.list.pop_front();
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

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-undo-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
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
        assert_eq!(u.refused.len(), 2, "{:?}", u.refused);
        assert!(u.refused.iter().any(|e| e.contains("b.txt") && e.contains(&t!("undo.changed"))));
        assert!(u.refused.iter().any(|e| e.contains("c.txt") && e.contains(&t!("undo.taken"))));
        assert_eq!(fs::read_to_string(d.join("a.txt")).unwrap(), "a.txt");
        assert_eq!(fs::read_to_string(d.join("c.txt")).unwrap(), "new c", "never overwritten");
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
        let plan = [("a", "b"), ("b", "a"), ("c", "d")].map(|(f, t)| crate::rename::Planned { from: f.into(), to: t.into(), conflict: None });
        crate::rename::apply(&d, &plan).unwrap();
        let mut r = Record::new(Kind::Rename);
        plan.iter().for_each(|p| r.done(&d.join(&p.from), &d.join(&p.to)));
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
        let d = std::env::current_dir().unwrap().join(format!("target-undo-trash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("gone.txt"), "back").unwrap();
        let mut r = Record::new(Kind::Trash);
        r.run(&d.join("gone.txt"), Path::new(""), None).unwrap();
        assert!(!d.join("gone.txt").exists());
        let u = r.undo(|_| {});
        assert_eq!(u.done, 1, "{:?}", u.refused);
        assert_eq!(fs::read_to_string(d.join("gone.txt")).unwrap(), "back", "back under its whole name");
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
        assert!(!h.push(Record::new(Kind::Copy)), "nothing to undo is not kept");
        for i in 0..KEEP + 3 {
            let mut r = Record::new(Kind::Mkdir);
            r.run(&d.join(i.to_string()), Path::new(""), None).unwrap();
            assert!(h.push(r));
        }
        assert_eq!(h.list.len(), KEEP);
        assert_eq!(h.next().unwrap().label(), format!("new folder \"{}\"", KEEP + 2));
        assert_eq!(h.pop().unwrap().label(), format!("new folder \"{}\"", KEEP + 2));
        assert_eq!(h.pushed, KEEP as u64 + 3);
    }
}

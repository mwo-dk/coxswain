//! Make one side like the other (docs/files/compare-folders.md#making-one-side-like-the-other):
//! after Compare folders, *Make right like left* or *Make left like right*. First a plan, then the
//! run, both through `pinned::dir`: folder handles, never a link followed, never a path looked up
//! again.
//!
//! The plan walks both trees side by side from their two roots, each folder opened in the one
//! above it, and lists every step: a file, link or folder to copy over, an entry on the target
//! side to replace, and (only when asked) one only on the target side to delete. Each step keeps
//! its path and the identity (device, inode, ctime) of what it reads and of what it replaces or
//! deletes, and so does every folder on the way.
//!
//! The run takes only those steps. It goes down from the two roots again, opening each folder in
//! the one above and checking it to be the very folder the plan saw (the same device and inode:
//! what is in it may have changed, and each entry is checked by itself); a step whose entry is not
//! exactly as planned (written, swapped, renamed in, turned into a link) is left as it is and
//! said. What it replaces or deletes is renamed, in its held folder, into one folder of the run's
//! own at the target's top, which then goes to the bin as one item. New files are written beside
//! under a name of their own and linked in only where nothing is. Ctrl+Z takes away what the run
//! made and puts back what it set aside. Pipes, devices and sockets are never opened nor copied.
//!
//! On Windows not yet: its walk does not go into folders through handles (`pinned::RECURSES`).

#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use super::folders::Mode;
use crate::pinned::{Id, Kind, Said, Stat};
use crate::t;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The plan shows at most this many rows; the run takes them all.
pub const SHOWN: usize = 2_000;
/// The steps a plan holds at most: past them it stops, and says so.
pub const MOST: usize = 100_000;
/// The name the run's own folder of what it set aside starts with; never compared nor copied.
pub const ASIDE: &str = ".coxswain-sync-";
/// Times of change two seconds apart are the same (FAT), as Compare folders has it.
const SLACK: i64 = 2;

/// The run's own entries, never compared nor copied (here and in Compare folders): a folder
/// aside (`.coxswain-sync-…`), at any depth, and a file being written that could not be removed
/// (`.name.coxswain-….tmp`).
pub fn ours(n: &str) -> bool {
    n.starts_with(ASIDE) || (n.starts_with('.') && n.contains(".coxswain-") && n.ends_with(".tmp"))
}

/// What a step does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Act {
    /// Only on the source side: copied over.
    Copy,
    /// On both, not alike: the target's goes to the bin, the source's is copied over.
    Replace,
    /// Only on the target side: to the bin.
    Delete,
}

/// One step, as the plan shows it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Row {
    /// Under the two roots, `/` between names.
    pub path: String,
    pub act: Act,
    pub is_dir: bool,
    pub size: u64,
    /// What is replaced is newer than what replaces it.
    pub older: bool,
    /// What goes to the bin is a folder: how many entries are in it.
    pub inside: Option<usize>,
}

/// The path of an entry under the roots, name by name.
type Rel = Vec<OsString>;
/// Every entry in a folder, by its path under it, as the plan saw it.
type Inside = Arc<Vec<(Rel, Id)>>;

#[derive(Clone, Debug)]
struct Step {
    rel: Rel,
    act: Act,
    /// What is copied over, as the plan saw it.
    src: Option<Stat>,
    /// What is replaced or deleted, as the plan saw it.
    dst: Option<Stat>,
    /// When that is a folder: every entry in it, by its path under it, as the plan saw it.
    inside: Option<Inside>,
    /// Its own name, where it differs from the source's in case only (Windows): what it is set
    /// aside and put back as.
    dst_name: Option<OsString>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Plan {
    /// The first `SHOWN` steps.
    pub rows: Vec<Row>,
    /// How many steps in all, and of each kind.
    pub total: usize,
    pub copies: usize,
    pub replaces: usize,
    pub deletes: usize,
    /// Bytes to copy over.
    pub bytes: u64,
    /// Entries only on the target side, kept (deleting was not asked for).
    pub extra: usize,
    /// Left out, and why (`path: why`).
    pub refused: Vec<String>,
    pub stopped: bool,
    /// Which plan this is: the page sends it back with the run, which takes only that plan.
    pub token: u64,
    /// The two folders, for the page to name them.
    pub from: PathBuf,
    pub to: PathBuf,
    /// What the run takes from the plan, never from the page.
    #[serde(skip)]
    roots: [Option<Stat>; 2],
    #[serde(skip)]
    steps: Arc<Vec<Step>>,
    /// Every folder gone into, on each side, as the plan saw it.
    #[serde(skip)]
    dirs: Arc<[HashMap<Rel, Id>; 2]>,
}

impl Plan {
    /// What it holds, in one line: the steps of each kind, what is kept, what was left out.
    pub fn summary(&self) -> String {
        let to = self.to.file_name().map_or_else(|| self.to.display().to_string(), |n| n.to_string_lossy().into_owned());
        let mut s = t!("sync.planned", "n" => self.total, "copies" => self.copies, "replaces" => self.replaces, "deletes" => self.deletes, "size" => crate::settings::human(self.bytes));
        if self.extra > 0 {
            s += &format!(" · {}", t!("sync.extra", "n" => self.extra, "dir" => to));
        }
        if !self.refused.is_empty() {
            s += &format!(" · {}", t!("attrs.left_out", "n" => self.refused.len()));
        }
        if self.stopped {
            s += &format!(" · {}", t!("attrs.stopped_early"));
        }
        s
    }
}

impl Row {
    /// Its path as a plan shows it: a folder with `/`, and when one goes to the bin, how many
    /// entries go with it.
    pub fn shown(&self) -> String {
        let mut s = self.path.clone();
        if self.is_dir {
            s.push('/');
        }
        if let Some(n) = self.inside.filter(|&n| n > 0) {
            s += &format!(" ({})", crate::tn!("items", n));
        }
        s
    }
}

impl Act {
    /// Its word in a plan's row.
    pub fn label(self) -> String {
        t!(match self {
            Act::Copy => "sync.act_copy",
            Act::Replace => "sync.act_replace",
            Act::Delete => "sync.act_delete",
        })
    }
}

#[derive(Default, Debug)]
pub struct Progress {
    pub files: AtomicUsize,
}

fn shown(rel: &[OsString]) -> String {
    rel.iter().map(|n| n.to_string_lossy()).collect::<Vec<_>>().join("/")
}

fn lines(said: Said, out: &mut Vec<String>) {
    out.extend(said.lines.into_iter().map(|(p, why)| format!("{}: {why}", p.display())));
    if said.more > 0 {
        out.push(t!("attrs.left_out", "n" => said.more));
    }
}

/// Why `from` and `to` cannot be made alike, if they cannot: not two folders on this computer
/// (an archive, an image, a snapshot, a history), the same folder, or one inside the other.
pub fn check(from: &Path, to: &Path) -> Result<(), String> {
    let real = |p: &Path| std::fs::canonicalize(p).ok().filter(|r| std::fs::symlink_metadata(r).is_ok_and(|m| m.is_dir()));
    let (Some(a), Some(b)) = (real(from), real(to)) else { return Err(t!("sync.not_here")) };
    if a.starts_with(&b) || b.starts_with(&a) {
        return Err(t!("sync.nested"));
    }
    Ok(())
}

/// What a run did, and the record that undoes it.
pub struct Applied {
    pub copied: usize,
    pub replaced: usize,
    pub deleted: usize,
    pub failed: Vec<String>,
    pub record: crate::undo::Record,
    /// What was set aside, when the bin could not take it: still there, in the target.
    pub kept_at: Option<PathBuf>,
}

impl Applied {
    pub fn summary(&self) -> String {
        t!("sync.done", "copied" => self.copied, "replaced" => self.replaced, "deleted" => self.deleted)
    }

    /// Where what was replaced or deleted is, when it did not go to the bin, as the status line
    /// says it.
    pub fn kept_text(&self) -> Option<String> {
        let p = self.kept_at.as_ref()?;
        Some(if cfg!(windows) { t!("sync.kept_windows", "path" => p.display()) } else { t!("sync.kept", "path" => p.display()) })
    }
}

/// What undo needs: where the run made what, and where it set what aside.
#[derive(Clone, Debug)]
pub struct Undo {
    /// The target folder, and its folders by their path (device and inode).
    to: PathBuf,
    dirs: HashMap<Rel, Id>,
    /// Made by the run, in the order made, as left.
    made: Vec<(Rel, Stat)>,
    /// Set aside: where it was, its name in the folder aside, what it was.
    kept: Vec<(Rel, OsString, Stat)>,
    /// The folder aside, its name at the target's top, and whether it went to the bin (with its
    /// name in Coxswain's own trash, where that is used).
    aside: Option<(OsString, Stat)>,
    trashed: Option<String>,
}

pub use imp::{apply, plan};

impl Undo {
    /// The target folder, to read again once undone.
    pub fn touched(&self) -> PathBuf {
        self.to.clone()
    }

    /// Undo the run: what it made, taken away (only the very entries, unchanged since, a folder
    /// only once empty), then what it set aside put back where it was, never over what is there.
    /// `restore` brings the folder aside back out of the bin. How many were undone, and why the
    /// others were not.
    pub fn undo(&self, restore: impl Fn(&Path, &Path) -> std::io::Result<()>) -> (usize, Vec<String>) {
        imp::undo(self, restore)
    }
}

mod imp {
    use super::*;
    use crate::pinned::dir::Dir;
    use crate::undo::{Kind as UndoKind, Record};

    struct Planner<'a> {
        mode: Mode,
        delete: bool,
        stop: &'a AtomicBool,
        progress: &'a Progress,
        plan: Plan,
        steps: Vec<Step>,
        dirs: [HashMap<Rel, Id>; 2],
        devs: [u64; 2],
        said: Said,
        deepest: usize,
        full: bool,
    }

    impl Planner<'_> {
        fn path(&self, side: usize, rel: &[OsString]) -> PathBuf {
            let root = if side == 0 { &self.plan.from } else { &self.plan.to };
            rel.iter().fold(root.clone(), |p, n| p.join(n))
        }

        fn push(&mut self, rel: Rel, act: Act, src: Option<Stat>, dst: Option<Stat>, inside: Option<Inside>, dst_name: Option<OsString>) {
            if self.plan.total >= MOST {
                if !self.full {
                    self.said.say(&self.path(1, &rel), t!("pinned.full", "n" => MOST));
                    self.full = true;
                }
                return;
            }
            self.plan.total += 1;
            match act {
                Act::Copy => self.plan.copies += 1,
                Act::Replace => self.plan.replaces += 1,
                Act::Delete => self.plan.deletes += 1,
            }
            let shown_stat = src.or(dst).expect("a step reads or replaces something");
            if shown_stat.kind == Kind::File && act != Act::Delete {
                self.plan.bytes += shown_stat.size;
            }
            if self.plan.rows.len() < SHOWN {
                let older = matches!((src, dst), (Some(a), Some(b)) if a.kind == Kind::File && b.kind == Kind::File && a.modified.0 + SLACK < b.modified.0);
                let is_dir = shown_stat.kind == Kind::Dir || inside.is_some();
                self.plan.rows.push(Row { path: shown(&rel), act, is_dir, size: shown_stat.size, older, inside: inside.as_ref().map(|i| i.len()) });
            }
            let dst_name = dst_name.filter(|n| Some(n) != rel.last());
            self.steps.push(Step { rel, act, src, dst, inside, dst_name });
        }

        fn busy(&self) -> bool {
            self.stop.load(Ordering::Relaxed) || self.full
        }

        /// Whether `id` is one of the two folders compared: never gone into from the other.
        fn a_root(&self, id: &Id) -> bool {
            self.plan.roots.iter().flatten().any(|r| r.id.same_file(id))
        }

        /// A target folder that goes to the bin: every entry in it, so the run can tell that
        /// nothing came in or changed since. None (and said) when it cannot be told.
        fn inside(&mut self, d: &Dir, rel: &Rel, y: Stat) -> Result<Option<Inside>, ()> {
            if y.kind != Kind::Dir {
                return Ok(None);
            }
            let name = rel.last().cloned().unwrap_or_default();
            let r = d.sub(&name, y.id, false).and_then(|sub| tree(&sub, self.devs[1], self.deepest.saturating_sub(rel.len()), self.stop));
            match r {
                Ok(v) => Ok(Some(Arc::new(v))),
                Err(e) => {
                    self.said.say(&self.path(1, rel), e.to_string());
                    Err(())
                }
            }
        }

        /// A source entry to copy over (`dst`: what it replaces, in `d`, and its own name), and
        /// all inside it when a folder.
        fn copy(&mut self, s: &Dir, d: Option<&Dir>, rel: Rel, a: Stat, dst: Option<(Stat, OsString)>) {
            match a.kind {
                Kind::Other => return self.said.say(&self.path(0, &rel), t!("pinned.special")),
                // Making a link needs a right of its own on Windows: links are left out there.
                Kind::Link if cfg!(windows) => return self.said.say(&self.path(0, &rel), t!("pinned.link")),
                // Windows lets anyone link a file they cannot read into a folder they can write:
                // its bytes would be copied with this user's rights. Another name of a file is
                // left out there.
                Kind::File if cfg!(windows) && a.nlink > 1 => return self.said.say(&self.path(0, &rel), t!("pinned.hard_link")),
                Kind::Dir if a.id.dev != self.devs[0] => return self.said.say(&self.path(0, &rel), t!("pinned.other_disk")),
                Kind::Dir if self.a_root(&a.id) => return self.said.say(&self.path(0, &rel), t!("sync.nested")),
                _ => {}
            }
            let (dst, dst_name) = dst.unzip();
            let inside = match (d, dst) {
                (Some(d), Some(y)) => match self.inside(d, &rel, y) {
                    Ok(i) => i,
                    Err(()) => return,
                },
                _ => None,
            };
            let act = if dst.is_some() { Act::Replace } else { Act::Copy };
            self.push(rel.clone(), act, Some(a), dst, inside, dst_name);
            if a.kind == Kind::Dir {
                let name = rel.last().cloned().unwrap_or_default();
                match s.sub(&name, a.id, false) {
                    Ok(sub) => self.walk(&sub, None, rel),
                    Err(e) => self.said.say(&self.path(0, &rel), e.to_string()),
                }
            }
        }

        /// The folder `s` (source) against `d` (target; none when it is to be made), both at `rel`.
        fn walk(&mut self, s: &Dir, d: Option<&Dir>, rel: Rel) {
            if self.busy() {
                return;
            }
            if rel.len() >= self.deepest {
                return self.said.say(&self.path(0, &rel), t!("pinned.too_deep", "n" => self.deepest));
            }
            let fold = folds(s, d);
            let list = |side: usize, dir: &Dir, me: &mut Self| match dir.names() {
                Ok(n) => {
                    let mut n = n.into_iter().filter(|n| !ours(n)).collect::<Vec<_>>();
                    n.sort_by(|x, y| by_name(fold, x, y).then_with(|| x.cmp(y)));
                    Some(n)
                }
                Err(e) => {
                    me.said.say(&me.path(side, &rel), e.to_string());
                    None
                }
            };
            let Some(a) = list(0, s, self) else { return };
            let b = match d {
                Some(d) => match list(1, d, self) {
                    Some(b) => b,
                    None => return,
                },
                None => vec![],
            };
            let (mut i, mut j) = (0, 0);
            while (i < a.len() || j < b.len()) && !self.busy() {
                let order = match (a.get(i), b.get(j)) {
                    (Some(x), Some(y)) => by_name(fold, x, y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    _ => std::cmp::Ordering::Greater,
                };
                let name = if order == std::cmp::Ordering::Greater { b[j].clone() } else { a[i].clone() };
                let dst_name = (order != std::cmp::Ordering::Less).then(|| b[j].clone());
                let mut sub = rel.clone();
                sub.push(name.clone());
                self.progress.files.fetch_add(1, Ordering::Relaxed);
                let sa = (order != std::cmp::Ordering::Greater).then(|| s.stat_at(&name));
                let sb = (order != std::cmp::Ordering::Less).then(|| d.map(|d| d.stat_at(&name))).flatten();
                if order != std::cmp::Ordering::Greater {
                    i += 1;
                }
                if order != std::cmp::Ordering::Less {
                    j += 1;
                }
                let sa = match sa.transpose() {
                    Ok(x) => x,
                    Err(e) => {
                        self.said.say(&self.path(0, &sub), e.to_string());
                        continue;
                    }
                };
                let sb = match sb.transpose() {
                    Ok(x) => x,
                    Err(e) => {
                        self.said.say(&self.path(1, &sub), e.to_string());
                        continue;
                    }
                };
                match (sa, sb) {
                    (Some(x), None) => self.copy(s, None, sub, x, None),
                    (None, Some(y)) => {
                        if y.kind == Kind::Dir && y.id.dev != self.devs[1] {
                            self.said.say(&self.path(1, &sub), t!("pinned.other_disk"));
                        } else if y.kind == Kind::Other {
                            self.said.say(&self.path(1, &sub), t!("pinned.special"));
                        } else if self.delete {
                            if let Ok(inside) = self.inside(d.expect("listed"), &sub, y) {
                                self.push(sub, Act::Delete, None, Some(y), inside, None);
                            }
                        } else {
                            self.plan.extra += 1;
                        }
                    }
                    (Some(x), Some(y)) => self.both(s, d.expect("listed"), sub, x, (y, dst_name.expect("on both"))),
                    (None, None) => {}
                }
            }
        }

        /// An entry on both sides.
        fn both(&mut self, s: &Dir, d: &Dir, rel: Rel, x: Stat, (y, dst_name): (Stat, OsString)) {
            let name = rel.last().cloned().unwrap_or_default();
            if x.kind == Kind::Other {
                return self.said.say(&self.path(0, &rel), t!("pinned.special"));
            }
            if y.kind == Kind::Other {
                return self.said.say(&self.path(1, &rel), t!("pinned.special"));
            }
            if y.kind == Kind::Dir && y.id.dev != self.devs[1] {
                return self.said.say(&self.path(1, &rel), t!("pinned.other_disk"));
            }
            let alike = match (x.kind, y.kind) {
                (Kind::Dir, Kind::Dir) => {
                    if x.id.dev != self.devs[0] {
                        return self.said.say(&self.path(0, &rel), t!("pinned.other_disk"));
                    }
                    if self.a_root(&x.id) || self.a_root(&y.id) {
                        return self.said.say(&self.path(0, &rel), t!("sync.nested"));
                    }
                    match (s.sub(&name, x.id, false), d.sub(&name, y.id, false)) {
                        (Ok(a), Ok(b)) => {
                            self.dirs[0].insert(rel.clone(), x.id);
                            self.dirs[1].insert(rel.clone(), y.id);
                            self.walk(&a, Some(&b), rel);
                        }
                        (Err(e), _) => self.said.say(&self.path(0, &rel), e.to_string()),
                        (_, Err(e)) => self.said.say(&self.path(1, &rel), e.to_string()),
                    }
                    return;
                }
                (Kind::File, Kind::File) => {
                    let near = x.modified.0.abs_diff(y.modified.0) <= SLACK as u64;
                    x.size == y.size
                        && match self.mode {
                            Mode::Quick => near,
                            Mode::Content => match s.same_bytes(&name, x.id, d, y.id, self.stop) {
                                Ok(same) => same,
                                Err(e) => return self.said.say(&self.path(0, &rel), e.to_string()),
                            },
                        }
                }
                (Kind::Link, Kind::Link) => match (s.read_link(&name, x.id), d.read_link(&name, y.id)) {
                    (Ok(a), Ok(b)) => a == b,
                    (Err(e), _) => return self.said.say(&self.path(0, &rel), e.to_string()),
                    (_, Err(e)) => return self.said.say(&self.path(1, &rel), e.to_string()),
                },
                _ => false,
            };
            if !alike {
                self.copy(s, Some(d), rel, x, Some((y, dst_name)));
            }
        }
    }

    /// Whether names pair up whatever their case: on Windows, unless either folder is marked
    /// case-sensitive (where `README` and `Readme` are two entries).
    fn folds(s: &Dir, d: Option<&Dir>) -> bool {
        #[cfg(windows)]
        return !s.case_sensitive() && !d.is_some_and(Dir::case_sensitive);
        #[cfg(not(windows))]
        {
            let _ = (s, d);
            false
        }
    }

    /// How two names pair up and sort: as they are; with `fold` (Windows), whose file systems
    /// take a name whatever its case, as Windows compares them (ordinal, case ignored), so
    /// `README` on one side and `Readme` on the other are one entry (replaced, the copy taking
    /// the source's name) rather than a copy that finds its name taken and a deletion.
    fn by_name(fold: bool, a: &OsStr, b: &OsStr) -> std::cmp::Ordering {
        #[cfg(windows)]
        if fold {
            use std::os::windows::ffi::OsStrExt;
            let (x, y): (Vec<u16>, Vec<u16>) = (a.encode_wide().collect(), b.encode_wide().collect());
            // SAFETY: two buffers of the lengths given. CSTR_LESS_THAN 1, CSTR_EQUAL 2,
            // CSTR_GREATER_THAN 3; 0 on failure, never for names this short.
            let r = unsafe { windows_sys::Win32::Globalization::CompareStringOrdinal(x.as_ptr(), x.len() as i32, y.as_ptr(), y.len() as i32, 1) };
            return (r - 2).cmp(&0);
        }
        let _ = fold;
        a.cmp(b)
    }

    fn ours(n: &OsStr) -> bool {
        super::ours(&n.to_string_lossy())
    }

    /// Every entry in the folder `d`, by its path under it, through folder handles, never
    /// through a link; refused when a folder in it is on another disk or more than `deepest`
    /// down.
    fn tree(d: &Dir, dev: u64, deepest: usize, stop: &AtomicBool) -> std::io::Result<Vec<(Rel, Id)>> {
        fn go(d: &Dir, rel: &mut Rel, dev: u64, left: usize, stop: &AtomicBool, out: &mut Vec<(Rel, Id)>) -> std::io::Result<()> {
            if left == 0 {
                return Err(std::io::Error::other(t!("pinned.too_deep", "n" => crate::pinned::deepest())));
            }
            for name in d.names()? {
                if stop.load(Ordering::Relaxed) {
                    return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                }
                let st = d.stat_at(&name)?;
                rel.push(name.clone());
                // A pipe set aside would hang a bin that copies (another disk): left as it is.
                if st.kind == Kind::Other {
                    let at = rel.iter().fold(PathBuf::new(), |p, n| p.join(n));
                    return Err(std::io::Error::other(format!("{}: {}", at.display(), t!("pinned.special"))));
                }
                out.push((rel.clone(), st.id));
                if st.kind == Kind::Dir {
                    if st.id.dev != dev {
                        return Err(std::io::Error::other(t!("pinned.other_disk")));
                    }
                    go(&d.sub(&name, st.id, false)?, rel, dev, left - 1, stop, out)?;
                }
                rel.pop();
            }
            Ok(())
        }
        let mut out = vec![];
        go(d, &mut vec![], dev, deepest.max(1), stop, &mut out)?;
        Ok(out)
    }

    /// Plan making `to` like `from`: every entry to copy over and to replace, and with `delete`
    /// every one only in `to` to delete (to the bin). Nothing is changed.
    pub fn plan(from: &Path, to: &Path, mode: Mode, delete: bool, stop: &AtomicBool, progress: &Progress) -> Result<Plan, String> {
        check(from, to)?;
        let (s, d) = (Dir::root(from).map_err(|e| format!("{}: {e}", from.display()))?, Dir::root(to).map_err(|e| format!("{}: {e}", to.display()))?);
        let (a, b) = (s.stat().map_err(|e| e.to_string())?, d.stat().map_err(|e| e.to_string())?);
        // One folder by two paths (a bind mount): never made like itself.
        if a.id.same_file(&b.id) {
            return Err(t!("sync.nested"));
        }
        let mut p = Planner {
            mode,
            delete,
            stop,
            progress,
            plan: Plan { token: crate::pinned::token(), from: from.to_path_buf(), to: to.to_path_buf(), roots: [Some(a), Some(b)], ..Default::default() },
            steps: vec![],
            dirs: [HashMap::from([(vec![], a.id)]), HashMap::from([(vec![], b.id)])],
            devs: [a.id.dev, b.id.dev],
            said: Said::default(),
            deepest: crate::pinned::deepest(),
            full: false,
        };
        p.walk(&s, Some(&d), vec![]);
        // The folders a copied folder holds are gone into too: the run opens them as seen.
        for st in &p.steps {
            if let Some(x) = st.src.filter(|x| x.kind == Kind::Dir) {
                p.dirs[0].insert(st.rel.clone(), x.id);
            }
        }
        let mut plan = p.plan;
        lines(p.said, &mut plan.refused);
        plan.stopped = stop.load(Ordering::Relaxed);
        plan.steps = Arc::new(p.steps);
        plan.dirs = Arc::new(p.dirs);
        Ok(plan)
    }

    /// A side's folders held open, from its root down to the folder last used: a step's folder
    /// is reached from the nearest held one, each folder below opened in the one above and
    /// checked to be the folder the plan saw. A folder the run made stays held until the run
    /// leaves it, then gets its bits and times.
    struct Held {
        stack: Vec<(Rel, Dir, Option<Stat>)>,
        /// Folders that could not be reached: nothing under them is tried.
        lost: std::collections::HashSet<Rel>,
    }

    impl Held {
        fn new(root: Dir) -> Held {
            Held { stack: vec![(vec![], root, None)], lost: Default::default() }
        }

        fn leave_to(&mut self, rel: &[OsString], said: &mut Vec<String>, root: &Path) {
            while self.stack.len() > 1 && !rel.starts_with(&self.stack.last().expect("held").0) {
                let (r, dir, made) = self.stack.pop().expect("held");
                if let Some(was) = made
                    && let Err(e) = dir.finish(&was)
                {
                    said.push(format!("{}: {e}", r.iter().fold(root.to_path_buf(), |p, n| p.join(n)).display()));
                }
            }
        }

        fn lost(&mut self, rel: &[OsString]) {
            self.lost.insert(rel.to_vec());
        }

        fn at(&mut self, rel: &[OsString], ids: impl Fn(&[OsString]) -> Option<Id>, said: &mut Vec<String>, root: &Path) -> Option<&Dir> {
            if (0..=rel.len()).any(|n| self.lost.contains(&rel[..n])) {
                return None;
            }
            self.leave_to(rel, said, root);
            while self.stack.last().expect("held").0.len() < rel.len() {
                let here = &self.stack.last().expect("held").1;
                let next: Rel = rel[..self.stack.last().expect("held").0.len() + 1].to_vec();
                let name = next.last().expect("a name");
                let opened = ids(&next).ok_or_else(crate::pinned::changed).and_then(|id| here.sub(name, id, true));
                match opened {
                    Ok(d) => self.stack.push((next, d, None)),
                    Err(e) => {
                        said.push(format!("{}: {e}", next.iter().fold(root.to_path_buf(), |p, n| p.join(n)).display()));
                        self.lost.insert(next);
                        return None;
                    }
                }
            }
            Some(&self.stack.last().expect("held").1)
        }

        fn top(&self) -> &Dir {
            &self.stack.last().expect("held").1
        }
    }

    /// The name of the folder aside: when, and which run.
    fn aside_name() -> OsString {
        let now = chrono::Local::now().format("%Y%m%d-%H%M%S");
        OsString::from(format!("{ASIDE}{now}-{}-{}", std::process::id(), crate::pinned::token()))
    }

    /// The name of the `k`th entry set aside: its number, and as much of its own name as fits.
    pub(super) fn kept_name(k: usize, name: &OsString) -> OsString {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::{OsStrExt, OsStringExt};
            let mut v = format!("{k}-").into_bytes();
            let b = name.as_bytes();
            // Cut between characters: half a one is refused where names must be UTF-8 (APFS, ZFS).
            let mut end = b.len().min(200);
            if let Ok(s) = std::str::from_utf8(b) {
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
            }
            v.extend(&b[..end]);
            OsString::from_vec(v)
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::{OsStrExt, OsStringExt};
            let mut v: Vec<u16> = format!("{k}-").encode_utf16().collect();
            v.extend(name.encode_wide().take(200));
            OsString::from_wide(&v)
        }
    }

    /// Take the steps of `plan`, each only on the entries exactly as the plan saw them.
    pub fn apply(plan: &Plan, stop: &AtomicBool, progress: &Progress) -> Result<Applied, String> {
        // On Windows the folder aside stays at the target's top: the bin takes and gives back
        // only by path (the shell's), which a junction swapped in there would lead elsewhere,
        // and the folder held by its handle is what undo works through.
        if cfg!(windows) {
            return apply_to(plan, stop, progress, |_| Err(std::io::Error::other(t!("undo.no_trash"))));
        }
        apply_to(plan, stop, progress, |p| if crate::fs::trash_takes(&[p.to_path_buf()]) && crate::fs::trash_renames(p) { crate::fs::trash_named(p, None) } else { Err(std::io::Error::other(t!("undo.no_trash"))) })
    }

    /// What is copied over, reached as planned.
    enum From {
        File(std::fs::File),
        Link(OsString),
        Dir,
    }

    /// `apply`, the folder aside going to the bin through `trash` (its name in Coxswain's own).
    pub(super) fn apply_to(plan: &Plan, stop: &AtomicBool, progress: &Progress, trash: impl Fn(&Path) -> std::io::Result<Option<String>>) -> Result<Applied, String> {
        let (from, to) = (&plan.from, &plan.to);
        let open = |p: &Path, want: Option<Stat>| -> Result<Dir, String> {
            let d = Dir::root(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let now = d.stat().map_err(|e| e.to_string())?;
            if !want.is_some_and(|w| now.id.same_file(&w.id)) {
                return Err(format!("{}: {}", p.display(), crate::pinned::changed()));
            }
            Ok(d)
        };
        let (s, d) = (open(from, plan.roots[0])?, open(to, plan.roots[1])?);
        let root = d.try_clone().map_err(|e| e.to_string())?;
        let mut failed = vec![];
        let mut undo = Undo { to: to.clone(), dirs: plan.dirs[1].clone(), made: vec![], kept: vec![], aside: None, trashed: None };
        // What is replaced or deleted goes into a folder of the run's own at the target's top.
        let aside = if plan.steps.iter().any(|s| s.act != Act::Copy) {
            let name = aside_name();
            let a = root.make_dir(&name).map_err(|e| format!("{}: {e}", to.join(&name).display()))?;
            let st = a.stat().map_err(|e| e.to_string())?;
            undo.aside = Some((name, st));
            Some(a)
        } else {
            None
        };
        let (mut src, mut dst) = (Held::new(s), Held::new(d));
        let (mut copied, mut replaced, mut deleted) = (0, 0, 0);
        let mut made_dirs: HashMap<Rel, Id> = HashMap::new();
        for (k, step) in plan.steps.iter().enumerate() {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            progress.files.store(k + 1, Ordering::Relaxed);
            let (parent, name) = (&step.rel[..step.rel.len() - 1], step.rel.last().expect("a name"));
            let at = |side: &Path| step.rel.iter().fold(side.to_path_buf(), |p, n| p.join(n));
            let ids = |r: &[OsString]| made_dirs.get(r).or_else(|| plan.dirs[1].get(r)).copied();
            if dst.at(parent, ids, &mut failed, to).is_none() {
                continue;
            }
            // What is copied over is reached first, exactly as planned: nothing goes aside for
            // a source that is not there any more.
            let source = match step.src {
                None => None,
                Some(was) => {
                    let Some(sd) = src.at(parent, |r| plan.dirs[0].get(r).copied(), &mut failed, from) else {
                        dst.lost(&step.rel);
                        continue;
                    };
                    let got = match was.kind {
                        Kind::File => sd.open_file(name, was.id).map(|(f, _)| From::File(f)),
                        Kind::Link => sd.read_link(name, was.id).map(From::Link),
                        Kind::Dir => sd.sub(name, was.id, false).map(|_| From::Dir),
                        Kind::Other => Err(std::io::Error::other(t!("pinned.special"))),
                    };
                    match got {
                        Ok(g) => Some((was, g)),
                        Err(e) => {
                            failed.push(format!("{}: {e}", at(from).display()));
                            dst.lost(&step.rel);
                            continue;
                        }
                    }
                }
            };
            let dd = dst.top();
            // Then what is there goes aside: a folder only with nothing in it changed since.
            let mut kept = None;
            // The target's entry by its own name (it may differ in case), and where it is put back.
            let there = step.dst_name.as_ref().unwrap_or(name);
            let mut back = step.rel.clone();
            *back.last_mut().expect("a name") = there.clone();
            if let (Some(was), Some(a)) = (step.dst, aside.as_ref()) {
                let k_name = kept_name(k, there);
                let unchanged = match &step.inside {
                    None => Ok(()),
                    Some(inside) => dd
                        .sub(there, was.id, false)
                        .and_then(|sub| tree(&sub, was.id.dev, usize::MAX, stop))
                        .and_then(|now| if now == **inside { Ok(()) } else { Err(crate::pinned::changed()) }),
                };
                if let Err(e) = unchanged.and_then(|()| dd.move_to(there, was.id, a, &k_name)) {
                    failed.push(format!("{}: {e}", at(to).display()));
                    dst.lost(&step.rel);
                    continue;
                }
                if step.act == Act::Delete {
                    undo.kept.push((back, k_name, was));
                    deleted += 1;
                    continue;
                }
                kept = Some((k_name, was));
            }
            let Some((was, source)) = source else { continue };
            let made = match source {
                From::File(mut f) => dd.create_file(name, &mut f, &was, stop).map(|(m, whole)| {
                    if !whole {
                        failed.push(format!("{}: {}", at(to).display(), t!("sync.no_mark")));
                    }
                    (m, None)
                }),
                From::Link(l) => dd.make_link(name, &l).map(|m| (m, None)),
                From::Dir => dd.make_dir(name).and_then(|nd| Ok((nd.stat()?, Some(nd)))),
            };
            match made {
                Ok((m, nd)) => {
                    undo.made.push((step.rel.clone(), m));
                    if let Some(k) = kept {
                        undo.kept.push((back, k.0, k.1));
                    }
                    if let Some(nd) = nd {
                        made_dirs.insert(step.rel.clone(), m.id);
                        undo.dirs.insert(step.rel.clone(), m.id);
                        dst.stack.push((step.rel.clone(), nd, Some(was)));
                    }
                    if step.act == Act::Replace { replaced += 1 } else { copied += 1 }
                }
                Err(e) => {
                    // A replace that could not be made: what was there comes back at once.
                    if let (Some((k_name, old)), Some(a)) = (kept, aside.as_ref())
                        && let Err(e) = a.move_back(&k_name, old.id, dd, there)
                    {
                        undo.kept.push((back, k_name, old));
                        failed.push(format!("{}: {e}", at(to).display()));
                    }
                    let stopped = stop.load(Ordering::Relaxed) && e.kind() == std::io::ErrorKind::Interrupted;
                    if !stopped || step.act == Act::Replace {
                        failed.push(format!("{}: {e}", at(to).display()));
                    }
                    // Nothing under a folder that was not made.
                    dst.lost(&step.rel);
                }
            }
        }
        // Every folder made gets its bits and times, now that it is filled.
        dst.leave_to(&[], &mut failed, to);
        let done = copied + replaced + deleted;
        if done < plan.total && !stop.load(Ordering::Relaxed) && failed.is_empty() {
            failed.push(t!("pinned.missed", "n" => plan.total - done));
        }
        // The folder aside: removed when empty, else to the bin as one item.
        let mut kept_at = None;
        if let (Some((name, st)), Some(a)) = (undo.aside.clone(), aside) {
            let path = to.join(&name);
            if undo.kept.is_empty() {
                let _ = root.remove(&name, &st);
                undo.aside = None;
            } else {
                // Only while the target's path still leads to the very folder held, and the
                // name in it to the very folder aside.
                let same = Dir::root(to).and_then(|r| r.stat()).is_ok_and(|now| now.id.same_file(&st_root(plan)))
                    && root.stat_at(&name).is_ok_and(|now| now.id.same_file(&st.id));
                match if same { trash(&path).ok() } else { None } {
                    // Gone from the target's top, and not to another place in it.
                    Some(n) if root.stat_at(&name).is_err() && (a.parent_is_not(&root) || a.stat().is_ok_and(|s| s.nlink == 0)) => undo.trashed = Some(n.unwrap_or_default()),
                    Some(_) => failed.push(t!("sync.moved_away", "path" => path.display())),
                    None => kept_at = Some(path),
                }
            }
        }
        let mut record = Record::new(UndoKind::Sync);
        if !undo.made.is_empty() || !undo.kept.is_empty() {
            record.done_sync(from, Arc::new(undo));
        }
        Ok(Applied { copied, replaced, deleted, failed, record, kept_at })
    }

    fn st_root(plan: &Plan) -> Id {
        plan.roots[1].map(|s| s.id).unwrap_or_default()
    }

    pub(super) fn undo(u: &Undo, restore: impl Fn(&Path, &Path) -> std::io::Result<()>) -> (usize, Vec<String>) {
        let mut refused = vec![];
        let mut done = 0;
        let path = |rel: &[OsString]| rel.iter().fold(u.to.clone(), |p, n| p.join(n));
        let root = match Dir::root(&u.to) {
            Ok(r) if r.stat().is_ok_and(|s| u.dirs.get(&vec![]).is_some_and(|w| s.id.same_file(w))) => r,
            Ok(_) => return (0, vec![format!("{}: {}", u.to.display(), crate::pinned::changed())]),
            Err(e) => return (0, vec![format!("{}: {e}", u.to.display())]),
        };
        // A folder of the target by its path, each opened in the one above from the root
        // checked here, the very folder.
        let open = |rel: &[OsString]| -> std::io::Result<Dir> {
            let mut d = root.try_clone()?;
            for i in 0..rel.len() {
                let id = u.dirs.get(&rel[..=i]).ok_or_else(crate::pinned::changed)?;
                d = d.sub(&rel[i], *id, true)?;
            }
            Ok(d)
        };
        // First what was set aside is made sure of: back out of the bin, before anything the
        // run made is taken away.
        let aside = match &u.aside {
            None => None,
            Some((name, st)) => {
                if u.trashed.is_some() && !crate::undo::trash_restores() {
                    return (0, vec![format!("{}: {}", u.to.join(name).display(), t!("undo.no_restore"))]);
                }
                if let Some(trash_name) = &u.trashed
                    && let Err(e) = restore(&u.to.join(name), Path::new(trash_name))
                {
                    return (0, vec![format!("{}: {e}", u.to.join(name).display())]);
                }
                match root.sub(name, st.id, true) {
                    Ok(a) => Some((a, name, st)),
                    Err(e) => return (0, vec![format!("{}: {e}", u.to.join(name).display())]),
                }
            }
        };
        // A folder the run made got its source's bits (perhaps no w): writable again, so
        // what is in it can be taken out. (Windows' read-only folder does not keep it in.)
        #[cfg(unix)]
        for (rel, st) in &u.made {
            if st.kind == Kind::Dir {
                let _ = open(rel).and_then(|d| d.writable());
            }
        }
        // What the run made, the last first: a folder after what is in it.
        for (rel, st) in u.made.iter().rev() {
            let r = open(&rel[..rel.len() - 1]).and_then(|d| d.remove(rel.last().expect("a name"), st));
            match r {
                Ok(()) => done += 1,
                Err(e) => refused.push(format!("{}: {e}", path(rel).display())),
            }
        }
        let Some((aside, name, st)) = aside else { return (done, refused) };
        for (rel, kept, was) in u.kept.iter().rev() {
            let r = open(&rel[..rel.len() - 1]).and_then(|d| aside.move_back(kept, was.id, &d, rel.last().expect("a name")));
            match r {
                Ok(()) => done += 1,
                Err(e) => refused.push(format!("{}: {e}", path(rel).display())),
            }
        }
        // Gone when all came back; else it stays, holding what could not.
        if root.remove(name, st).is_err() && !refused.is_empty() {
            refused.push(t!("sync.left_in", "path" => u.to.join(name).display()));
        }
        (done, refused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pinned::link_dir;
    use std::fs;

    /// A link to a folder (a junction on Windows) removed, not what it points at.
    fn unlink_dir(p: &Path) {
        #[cfg(unix)]
        fs::remove_file(p).unwrap();
        #[cfg(windows)]
        fs::remove_dir(p).unwrap();
    }

    fn plan_of(l: &Path, r: &Path, delete: bool) -> Plan {
        plan(l, r, Mode::Quick, delete, &AtomicBool::new(false), &Progress::default()).unwrap()
    }

    fn run(p: &Plan) -> Applied {
        run_kept(p)
    }

    fn pair(name: &str) -> (crate::fs::Scratch, PathBuf, PathBuf) {
        let d = crate::fs::Scratch::named(name);
        let (l, r) = (d.join("l"), d.join("r"));
        fs::create_dir_all(&l).unwrap();
        fs::create_dir_all(&r).unwrap();
        (d, l, r)
    }

    fn old(p: &Path) {
        let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
    }

    /// The run, the folder aside left where it is: nothing reaches the user's bin.
    fn run_kept(p: &Plan) -> Applied {
        imp::apply_to(p, &AtomicBool::new(false), &Progress::default(), |_| Err(std::io::Error::other("no bin in a test"))).unwrap()
    }

    #[test]
    fn the_target_is_made_like_the_source_and_undo_puts_it_back() {
        let (_d, l, r) = pair("sync-basic");
        fs::create_dir_all(l.join("new/deep")).unwrap();
        fs::write(l.join("new/deep/f"), "deep").unwrap();
        fs::write(l.join("same"), "s").unwrap();
        fs::write(r.join("same"), "s").unwrap();
        fs::write(l.join("changed"), "new text").unwrap();
        fs::write(r.join("changed"), "old").unwrap();
        old(&r.join("changed"));
        fs::write(r.join("extra"), "only right").unwrap();
        // A link is copied as a link; on Windows it is left out, and said.
        #[cfg(unix)]
        std::os::unix::fs::symlink("same", l.join("link")).unwrap();
        #[cfg(windows)]
        link_dir(&l.join("new"), &l.join("link"));
        let copies = if cfg!(unix) { 4 } else { 3 };
        // Kept unless deleting is asked for.
        let p = plan_of(&l, &r, false);
        assert_eq!((p.copies, p.replaces, p.deletes, p.extra), (copies, 1, 0, 1), "{:?}", p.rows);
        assert_eq!(p.refused.len(), usize::from(cfg!(windows)), "{:?}", p.refused);
        let p = plan_of(&l, &r, true);
        assert_eq!(p.deletes, 1);
        let done = run(&p);
        assert!(done.failed.is_empty(), "{:?}", done.failed);
        assert_eq!((done.copied, done.replaced, done.deleted), (copies, 1, 1));
        assert_eq!(fs::read_to_string(r.join("new/deep/f")).unwrap(), "deep");
        assert_eq!(fs::read_to_string(r.join("changed")).unwrap(), "new text");
        #[cfg(unix)]
        assert_eq!(fs::read_link(r.join("link")).unwrap(), Path::new("same"));
        assert!(!r.join("extra").exists());
        // What was replaced or deleted is in the folder aside, which the plan never shows.
        let kept = done.kept_at.clone().expect("kept where the bin cannot take it");
        assert_eq!(fs::read_dir(&kept).unwrap().count(), 2);
        // Alike now: nothing more to do.
        assert_eq!(plan_of(&l, &r, true).total, 0);
        // Undo: as it was.
        let u = done.record.undo(|_| {});
        assert!(u.refused.is_empty(), "{:?}", u.refused);
        assert_eq!(fs::read_to_string(r.join("changed")).unwrap(), "old");
        assert_eq!(fs::read_to_string(r.join("extra")).unwrap(), "only right");
        assert!(!r.join("new").exists() && !r.join("link").exists());
        let names: Vec<_> = fs::read_dir(&r).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 3, "the folder aside is gone too: {names:?}");
    }

    /// With the real bin, in CI only: the folder aside goes there as one item, and undo brings
    /// it back out.
    #[cfg(not(windows))]
    #[test]
    fn the_bin_takes_the_folder_aside_and_undo_brings_it_back() {
        if std::env::var_os("CI").is_none() || !crate::undo::trash_restores() {
            return;
        }
        // In the folder of the tests, which is on the disk the user's trash is on.
        let d = std::env::current_dir().unwrap().join(format!("target-sync-bin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let (l, r) = (d.join("l"), d.join("r"));
        fs::create_dir_all(&l).unwrap();
        fs::create_dir_all(&r).unwrap();
        if !crate::fs::trash_takes(std::slice::from_ref(&r)) {
            return fs::remove_dir_all(&d).unwrap();
        }
        fs::write(r.join("extra"), "kept in the bin").unwrap();
        let done = apply(&plan_of(&l, &r, true), &AtomicBool::new(false), &Progress::default()).unwrap();
        assert_eq!(done.deleted, 1, "{:?}", done.failed);
        assert!(done.kept_at.is_none());
        assert_eq!(fs::read_dir(&r).unwrap().count(), 0, "the folder aside went to the bin");
        let u = done.record.undo(|_| {});
        assert!(u.refused.is_empty(), "{:?}", u.refused);
        assert_eq!(fs::read_to_string(r.join("extra")).unwrap(), "kept in the bin");
        assert_eq!(fs::read_dir(&r).unwrap().count(), 1);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_folder_to_the_bin_goes_only_with_what_the_plan_saw_in_it() {
        let (_d, l, r) = pair("sync-deep-change");
        fs::create_dir_all(r.join("gone/deep")).unwrap();
        fs::write(r.join("gone/deep/f"), "x").unwrap();
        // A folder against a file: the folder goes, with the two entries counted.
        fs::create_dir_all(r.join("clash/in")).unwrap();
        fs::write(l.join("clash"), "a file now").unwrap();
        let p = plan_of(&l, &r, true);
        assert_eq!((p.deletes, p.replaces), (1, 1), "{:?}", p.rows);
        assert!(p.rows.iter().all(|r| r.is_dir && r.inside.is_some()), "{:?}", p.rows);
        // Something put deep inside after the plan: the folders stay.
        fs::write(r.join("gone/deep/late"), "theirs").unwrap();
        fs::write(r.join("clash/in/late"), "theirs").unwrap();
        let done = run(&p);
        assert_eq!(done.deleted + done.replaced, 0, "{:?}", done.failed);
        assert!(r.join("gone/deep/late").exists() && r.join("clash/in/late").exists());
        assert!(!done.failed.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn set_id_bits_are_not_copied() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, l, r) = pair("sync-setid");
        fs::write(l.join("tool"), "#!/bin/sh").unwrap();
        // Nix's build sandbox refuses setting the bit (EPERM): then there is nothing to test.
        if let Err(e) = fs::set_permissions(l.join("tool"), fs::Permissions::from_mode(0o4755)) {
            assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
            return eprintln!("skipped: setting the setuid bit is refused here ({e})");
        }
        let done = run(&plan_of(&l, &r, false));
        assert_eq!(done.copied, 1, "{:?}", done.failed);
        assert_eq!(fs::metadata(r.join("tool")).unwrap().permissions().mode() & 0o7777, 0o755);
    }

    #[test]
    fn nested_or_same_folders_are_refused() {
        let (_d, l, r) = pair("sync-nested");
        fs::create_dir_all(l.join("in")).unwrap();
        assert!(check(&l, &l.join("in")).is_err());
        assert!(check(&l.join("in"), &l).is_err());
        assert!(check(&l, &l).is_err());
        assert!(check(&l, &r).is_ok());
    }

    #[test]
    fn a_target_folder_swapped_for_a_link_after_the_plan_leads_nowhere() {
        let (d, l, r) = pair("sync-swap");
        let out = d.join("outside");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("f"), "precious").unwrap();
        fs::create_dir_all(l.join("sub")).unwrap();
        fs::create_dir_all(r.join("sub")).unwrap();
        fs::write(l.join("sub/f"), "from the left").unwrap();
        fs::write(r.join("sub/f"), "old").unwrap();
        old(&r.join("sub/f"));
        fs::write(l.join("sub/g"), "new").unwrap();
        let p = plan_of(&l, &r, true);
        assert_eq!((p.copies, p.replaces), (1, 1));
        fs::rename(r.join("sub"), d.join("moved")).unwrap();
        link_dir(&out, &r.join("sub"));
        let done = run(&p);
        assert_eq!(done.copied + done.replaced, 0);
        assert!(!done.failed.is_empty());
        assert_eq!(fs::read_to_string(out.join("f")).unwrap(), "precious");
        assert!(!out.join("g").exists());
        // The source side swapped: nothing read from outside.
        unlink_dir(&r.join("sub"));
        fs::rename(d.join("moved"), r.join("sub")).unwrap();
        let p = plan_of(&l, &r, true);
        fs::rename(l.join("sub"), d.join("lmoved")).unwrap();
        link_dir(&out, &l.join("sub"));
        let done = run(&p);
        assert_eq!(done.copied + done.replaced, 0, "{:?}", done.failed);
        assert_eq!(fs::read_to_string(r.join("sub/f")).unwrap(), "old");
        assert!(!r.join("sub/g").exists());
    }

    #[test]
    fn what_was_renamed_in_after_the_plan_is_left_alone() {
        let (d, l, r) = pair("sync-rename-in");
        fs::write(l.join("a"), "new a").unwrap();
        fs::write(r.join("a"), "old a").unwrap();
        old(&r.join("a"));
        fs::write(r.join("extra"), "x").unwrap();
        fs::write(l.join("b"), "new b").unwrap();
        let p = plan_of(&l, &r, true);
        assert_eq!(p.total, 3);
        // Others renamed in over the planned names, on both sides.
        fs::write(d.join("victim"), "victim's").unwrap();
        fs::rename(d.join("victim"), r.join("a")).unwrap();
        fs::write(d.join("victim2"), "victim's 2").unwrap();
        fs::rename(d.join("victim2"), r.join("extra")).unwrap();
        fs::write(d.join("secret"), "secret").unwrap();
        fs::rename(d.join("secret"), l.join("b")).unwrap();
        let done = run(&p);
        assert_eq!(done.copied + done.replaced + done.deleted, 0, "{:?}", done.failed);
        assert_eq!(fs::read_to_string(r.join("a")).unwrap(), "victim's");
        assert_eq!(fs::read_to_string(r.join("extra")).unwrap(), "victim's 2");
        assert!(!r.join("b").exists(), "a file renamed in on the source side is not read");
    }

    #[cfg(unix)]
    #[test]
    fn pipes_are_never_opened_nor_copied() {
        let (_d, l, r) = pair("sync-fifo");
        let fifo = std::ffi::CString::new(l.join("pipe").as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        fs::write(l.join("f"), "x").unwrap();
        let p = plan_of(&l, &r, true);
        assert_eq!(p.copies, 1, "{:?}", p.rows);
        assert_eq!(p.refused.len(), 1, "{:?}", p.refused);
        // A file planned, then swapped for a pipe: never opened (which would hang).
        fs::remove_file(l.join("f")).unwrap();
        let f2 = std::ffi::CString::new(l.join("f").as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: as above.
        assert_eq!(unsafe { libc::mkfifo(f2.as_ptr(), 0o600) }, 0);
        let done = run(&p);
        assert_eq!(done.copied, 0);
        assert!(!r.join("f").exists() && !r.join("pipe").exists());
    }

    #[cfg(unix)]
    fn mkfifo(p: &Path) {
        let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    }

    /// A pipe in the target is never set aside: a bin that copies (another disk) would hang
    /// on it. Only in the target, against a file, or deep in a folder that would go: left.
    #[cfg(unix)]
    #[test]
    fn pipes_in_the_target_are_left_where_they_are() {
        let (_d, l, r) = pair("sync-fifo-target");
        mkfifo(&r.join("only"));
        fs::write(l.join("clash"), "x").unwrap();
        mkfifo(&r.join("clash"));
        fs::create_dir_all(r.join("gone/deep")).unwrap();
        mkfifo(&r.join("gone/deep/pipe"));
        let p = plan_of(&l, &r, true);
        assert_eq!(p.total, 0, "{:?}", p.rows);
        assert_eq!(p.refused.len(), 3, "{:?}", p.refused);
        assert!(p.refused.iter().any(|l| l.contains("deep/pipe")), "{:?}", p.refused);
    }

    /// The bin copied the folder aside over and deleted it (another disk): it is in the bin,
    /// not said to have gone somewhere else. Only a freedesktop.org bin copies: macOS's and
    /// Windows' move (and a folder removed while held need not show 0 links on APFS).
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "illumos", target_os = "solaris"))))]
    #[test]
    fn a_folder_aside_the_bin_copied_counts_as_in_the_bin() {
        let (_d, l, r) = pair("sync-bin-copied");
        fs::write(r.join("extra"), "x").unwrap();
        let done = imp::apply_to(&plan_of(&l, &r, true), &AtomicBool::new(false), &Progress::default(), |p| fs::remove_dir_all(p).map(|()| None)).unwrap();
        assert!(done.failed.is_empty(), "{:?}", done.failed);
        assert!(done.kept_at.is_none() && done.deleted == 1);
    }

    /// A copied folder without write bits (0555) is taken away by undo, with what is in it.
    #[cfg(unix)]
    #[test]
    fn undo_takes_away_a_copied_read_only_folder() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, l, r) = pair("sync-ro-dir");
        fs::create_dir_all(l.join("ro/in")).unwrap();
        fs::write(l.join("ro/in/f"), "x").unwrap();
        fs::set_permissions(l.join("ro/in"), fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(l.join("ro"), fs::Permissions::from_mode(0o555)).unwrap();
        let done = run(&plan_of(&l, &r, false));
        assert_eq!(done.copied, 3, "{:?}", done.failed);
        assert_eq!(fs::metadata(r.join("ro")).unwrap().permissions().mode() & 0o777, 0o555);
        let u = done.record.undo(|_| {});
        assert!(u.refused.is_empty(), "{:?}", u.refused);
        assert!(!r.join("ro").exists());
        fs::set_permissions(l.join("ro"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(l.join("ro/in"), fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A long name set aside is cut between characters, never inside one.
    #[cfg(unix)]
    #[test]
    fn a_kept_name_is_cut_between_characters() {
        let name = OsString::from("€".repeat(100));
        let kept = imp::kept_name(7, &name);
        let s = kept.to_str().expect("still UTF-8");
        assert!(s.starts_with("7-") && s.len() <= 202 && s.len() >= 200, "{}", s.len());
    }

    #[test]
    fn a_hard_link_to_a_file_outside_is_replaced_not_written_through() {
        let (d, l, r) = pair("sync-hard");
        let out = d.join("outside");
        fs::write(&out, "precious").unwrap();
        old(&out);
        fs::hard_link(&out, r.join("f")).unwrap();
        fs::write(l.join("f"), "from the left, longer").unwrap();
        let done = run(&plan_of(&l, &r, true));
        assert_eq!(done.replaced, 1, "{:?}", done.failed);
        assert_eq!(fs::read_to_string(&out).unwrap(), "precious");
        assert_eq!(fs::read_to_string(r.join("f")).unwrap(), "from the left, longer");
        // Undo brings the very link back.
        let u = done.record.undo(|_| {});
        assert!(u.refused.is_empty(), "{:?}", u.refused);
        assert_eq!(fs::read_to_string(r.join("f")).unwrap(), "precious");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(r.join("f")).unwrap().ino(), fs::metadata(&out).unwrap().ino());
        }
    }

    #[test]
    fn a_folder_renamed_in_on_the_target_is_not_gone_into() {
        let (d, l, r) = pair("sync-dir-in");
        fs::create_dir_all(l.join("sub")).unwrap();
        fs::create_dir_all(r.join("sub")).unwrap();
        fs::write(l.join("sub/new"), "x").unwrap();
        let p = plan_of(&l, &r, true);
        let other = d.join("other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("keep"), "theirs").unwrap();
        fs::rename(r.join("sub"), d.join("gone")).unwrap();
        fs::rename(&other, r.join("sub")).unwrap();
        let done = run(&p);
        assert_eq!(done.copied, 0);
        assert!(!r.join("sub/new").exists());
    }

    #[test]
    fn stop_ends_the_run_and_leaves_no_half_file() {
        let (_d, l, r) = pair("sync-stop");
        for i in 0..20 {
            fs::write(l.join(format!("f{i:02}")), "x").unwrap();
        }
        let p = plan_of(&l, &r, true);
        let stop = AtomicBool::new(true);
        let done = imp::apply_to(&p, &stop, &Progress::default(), |_| Err(std::io::Error::other("no bin"))).unwrap();
        assert_eq!(done.copied, 0);
        assert!(done.failed.is_empty());
        assert_eq!(fs::read_dir(&r).unwrap().count(), 0, "nothing left behind");
        // Stopping the plan: what was looked at until then.
        let p = plan(&l, &r, Mode::Quick, true, &stop, &Progress::default()).unwrap();
        assert!(p.stopped && p.total == 0);
    }

    /// A junction in the target, there at the plan: deleted means moved aside itself, never
    /// gone into; what it points at is untouched, and undo puts the junction back.
    #[test]
    fn a_link_in_the_target_goes_aside_itself() {
        let (d, l, r) = pair("sync-link-aside");
        let out = d.join("outside");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("f"), "precious").unwrap();
        link_dir(&out, &r.join("j"));
        let p = plan_of(&l, &r, true);
        assert_eq!(p.deletes, 1, "{:?}", p.rows);
        let done = run(&p);
        assert_eq!(done.deleted, 1, "{:?}", done.failed);
        assert!(fs::symlink_metadata(r.join("j")).is_err());
        assert_eq!(fs::read_to_string(out.join("f")).unwrap(), "precious");
        let u = done.record.undo(|_| {});
        assert!(u.refused.is_empty(), "{:?}", u.refused);
        assert!(fs::symlink_metadata(r.join("j")).is_ok());
        assert_eq!(fs::read_to_string(r.join("j/f")).unwrap(), "precious");
        assert_eq!(fs::read_dir(&out).unwrap().count(), 1);
    }

    /// The folder for what goes aside is the run's own: a junction swapped in for a folder the
    /// run is about to fill, after the plan, is not followed, on the target's top or deeper.
    #[test]
    fn a_junction_swapped_in_deep_down_leads_nowhere() {
        let (d, l, r) = pair("sync-deep-swap");
        let out = d.join("outside");
        fs::create_dir_all(&out).unwrap();
        fs::create_dir_all(l.join("a/b")).unwrap();
        fs::create_dir_all(r.join("a/b")).unwrap();
        fs::write(l.join("a/b/new"), "new").unwrap();
        fs::write(r.join("a/b/gone"), "x").unwrap();
        let p = plan_of(&l, &r, true);
        assert_eq!((p.copies, p.deletes), (1, 1), "{:?}", p.rows);
        // The middle folder swapped for a junction to a copy of it outside.
        fs::create_dir_all(out.join("b")).unwrap();
        fs::write(out.join("b/gone"), "x").unwrap();
        fs::rename(r.join("a"), d.join("moved")).unwrap();
        link_dir(&out, &r.join("a"));
        let done = run(&p);
        assert_eq!(done.copied + done.deleted, 0, "{:?}", done.failed);
        assert!(!out.join("b/new").exists());
        assert_eq!(fs::read_to_string(out.join("b/gone")).unwrap(), "x");
    }

    /// On Windows a name is one name whatever its case: `README.TXT` on the target is replaced
    /// by the source's `Readme.txt`, which keeps the source's case; undo brings the old one back.
    #[cfg(windows)]
    #[test]
    fn names_that_differ_in_case_only_are_one_entry_on_windows() {
        let (_d, l, r) = pair("sync-case");
        fs::write(l.join("Readme.txt"), "new text").unwrap();
        fs::write(r.join("README.TXT"), "old").unwrap();
        old(&r.join("README.TXT"));
        fs::create_dir_all(l.join("Src")).unwrap();
        fs::create_dir_all(r.join("src")).unwrap();
        fs::write(l.join("Src/a"), "a").unwrap();
        let p = plan_of(&l, &r, true);
        assert_eq!((p.copies, p.replaces, p.deletes), (1, 1, 0), "{:?}", p.rows);
        let done = run(&p);
        assert!(done.failed.is_empty(), "{:?}", done.failed);
        let names: Vec<_> = fs::read_dir(&r).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).filter(|n| !n.starts_with(ASIDE)).collect();
        assert!(names.contains(&"Readme.txt".to_string()) && names.contains(&"src".to_string()), "{names:?}");
        assert_eq!(fs::read_to_string(r.join("Readme.txt")).unwrap(), "new text");
        assert_eq!(fs::read_to_string(r.join("src/a")).unwrap(), "a");
        // Set aside under its own name, and put back as it was named.
        let aside = fs::read_dir(&r).unwrap().map(|e| e.unwrap().path()).find(|p| p.file_name().unwrap().to_string_lossy().starts_with(ASIDE)).unwrap();
        let kept: Vec<_> = fs::read_dir(&aside).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert!(kept.iter().all(|k| k.ends_with("README.TXT")), "{kept:?}");
        let u = done.record.undo(|_| {});
        assert!(u.refused.is_empty(), "{:?}", u.refused);
        assert_eq!(fs::read_to_string(r.join("README.TXT")).unwrap(), "old");
        let names: Vec<_> = fs::read_dir(&r).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert!(names.contains(&"README.TXT".to_string()), "{names:?}");
        assert!(!r.join("src/a").exists());
    }

    /// Names pair up as the file system takes them, case aside: where it takes two names for
    /// one (Greek final sigma, title case), so does the plan, and the other way round.
    #[cfg(windows)]
    #[test]
    fn names_pair_up_as_windows_compares_them() {
        let (_d, l, r) = pair("sync-case-fold");
        let pairs = [("a\u{3c2}", "a\u{3c3}"), ("\u{1c5}", "\u{1c6}"), ("x\u{df}", "x\u{1e9e}"), ("\u{130}", "i"), ("caf\u{e9}", "CAF\u{c9}")];
        let mut same = 0;
        for (a, b) in pairs {
            fs::write(r.join(b), "old").unwrap();
            old(&r.join(b));
            same += usize::from(r.join(a).exists());
            fs::write(l.join(a), "new").unwrap();
        }
        assert!(same >= 1, "the plain accent pair is one name");
        let p = plan_of(&l, &r, false);
        assert_eq!((p.replaces, p.copies, p.extra), (same, pairs.len() - same, pairs.len() - same), "{:?}", p.rows);
    }

    /// In a folder marked case-sensitive (as WSL makes them) two names that differ in case are
    /// two entries.
    #[cfg(windows)]
    #[test]
    fn a_case_sensitive_folder_pairs_names_as_they_are() {
        let (_d, l, r) = pair("sync-case-sensitive");
        let on = std::process::Command::new("fsutil").args(["file", "setCaseSensitiveInfo"]).arg(&r).arg("enable").output();
        if !on.is_ok_and(|o| o.status.success()) {
            eprintln!("fsutil cannot mark a folder case-sensitive here: not tested");
            return;
        }
        fs::write(l.join("Readme.txt"), "new").unwrap();
        fs::write(r.join("README.TXT"), "old").unwrap();
        let p = plan_of(&l, &r, true);
        assert_eq!((p.copies, p.replaces, p.deletes), (1, 0, 1), "{:?}", p.rows);
        let done = run(&p);
        assert!(done.failed.is_empty(), "{:?}", done.failed);
        assert_eq!(fs::read_to_string(r.join("Readme.txt")).unwrap(), "new");
        assert!(!r.join("README.TXT").exists());
    }

    /// What a run leaves of its own (a folder aside, a file half written it could not remove)
    /// is never compared, copied nor deleted, at any depth.
    #[test]
    fn the_runs_own_leftovers_are_left_alone_at_any_depth() {
        let (_d, l, r) = pair("sync-leftovers");
        fs::create_dir_all(l.join("sub")).unwrap();
        fs::create_dir_all(r.join(format!("sub/{ASIDE}20261024-1-2"))).unwrap();
        fs::write(r.join(format!("sub/{ASIDE}20261024-1-2/0-f")), "x").unwrap();
        fs::write(r.join("sub/.f.coxswain-1-2-3.tmp"), "half").unwrap();
        fs::write(l.join(".g.coxswain-4-5-6.tmp"), "half").unwrap();
        let p = plan_of(&l, &r, true);
        assert_eq!(p.total, 0, "{:?}", p.rows);
    }

    /// On Windows a copy keeps the mark of a download, and another name of a file (a hard link,
    /// which anyone may make to a file they cannot read) is left out, not read.
    #[cfg(windows)]
    #[test]
    fn windows_keeps_the_mark_of_a_download_and_leaves_hard_links_out() {
        let (d, l, r) = pair("sync-zone");
        fs::write(l.join("setup.exe"), "MZ").unwrap();
        fs::write(PathBuf::from(format!("{}:Zone.Identifier", l.join("setup.exe").display())), "[ZoneTransfer]\r\nZoneId=3\r\n").unwrap();
        fs::write(d.join("secret"), "secret").unwrap();
        fs::hard_link(d.join("secret"), l.join("linked")).unwrap();
        let p = plan_of(&l, &r, true);
        assert_eq!(p.copies, 1, "{:?}", p.rows);
        assert_eq!(p.refused.len(), 1, "{:?}", p.refused);
        let done = run(&p);
        assert!(done.failed.is_empty(), "{:?}", done.failed);
        assert!(done.kept_at.is_none());
        assert_eq!(fs::read_to_string(PathBuf::from(format!("{}:Zone.Identifier", r.join("setup.exe").display()))).unwrap(), "[ZoneTransfer]\r\nZoneId=3\r\n");
        assert!(!r.join("linked").exists());
    }

    /// Every other data stream comes along, the mark of a download among them, a large one too.
    #[cfg(windows)]
    #[test]
    fn windows_copies_other_data_streams() {
        let (_d, l, r) = pair("sync-streams");
        let stream = |p: &Path, s: &str| PathBuf::from(format!("{}:{s}", p.display()));
        let big: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        fs::write(l.join("f"), "main").unwrap();
        fs::write(stream(&l.join("f"), "notes"), "a note").unwrap();
        fs::write(stream(&l.join("f"), "big"), &big).unwrap();
        fs::write(stream(&l.join("f"), "Zone.Identifier"), "[ZoneTransfer]\r\nZoneId=3\r\n").unwrap();
        let mut ro = fs::metadata(l.join("f")).unwrap().permissions();
        ro.set_readonly(true);
        fs::set_permissions(l.join("f"), ro).unwrap();
        let done = run(&plan_of(&l, &r, false));
        assert!(done.failed.is_empty(), "{:?}", done.failed);
        let t = r.join("f");
        assert_eq!(fs::read_to_string(&t).unwrap(), "main");
        assert_eq!(fs::read_to_string(stream(&t, "notes")).unwrap(), "a note");
        assert!(fs::read(stream(&t, "big")).unwrap() == big);
        assert!(fs::read_to_string(stream(&t, "Zone.Identifier")).unwrap().contains("ZoneId=3"));
        assert!(fs::metadata(&t).unwrap().permissions().readonly(), "read-only after the streams");
        // Undo as an older Windows deletes (no POSIX semantics): read-only is taken off first.
        crate::pinned::dir::OLD_DELETE.set(true);
        let u = done.record.undo(|_| {});
        crate::pinned::dir::OLD_DELETE.set(false);
        assert!(u.refused.is_empty(), "{:?}", u.refused);
        assert!(!t.exists());
        let mut rw = fs::metadata(l.join("f")).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        rw.set_readonly(false);
        fs::set_permissions(l.join("f"), rw).unwrap();
    }

    /// An encrypted file (EFS) is copied encrypted, or not at all: never as plain data.
    #[cfg(windows)]
    #[test]
    fn windows_copies_an_encrypted_file_encrypted_or_not_at_all() {
        use std::os::windows::fs::MetadataExt;
        let (_d, l, r) = pair("sync-efs");
        fs::write(l.join("secret.txt"), "secret").unwrap();
        let ok = std::process::Command::new("cipher").arg("/e").arg(l.join("secret.txt")).output().is_ok_and(|o| o.status.success());
        if !ok || fs::metadata(l.join("secret.txt")).unwrap().file_attributes() & 0x4000 == 0 {
            eprintln!("EFS is not available here: not tested");
            return;
        }
        let done = run(&plan_of(&l, &r, false));
        let t = r.join("secret.txt");
        if t.exists() {
            assert!(done.failed.is_empty(), "{:?}", done.failed);
            assert_ne!(fs::metadata(&t).unwrap().file_attributes() & 0x4000, 0, "the copy is encrypted");
            assert_eq!(fs::read_to_string(&t).unwrap(), "secret");
        } else {
            assert!(done.failed.iter().any(|f| f.contains(&t!("pinned.efs"))), "{:?}", done.failed);
        }
        let leftovers: Vec<_> = fs::read_dir(&r).unwrap().map(|e| e.unwrap().file_name()).filter(|n| n != "secret.txt").collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    /// On Windows the folder aside stays at the target's top (never handed to the shell's bin
    /// by its path), and the status line says where.
    #[cfg(windows)]
    #[test]
    fn windows_keeps_the_folder_aside_in_the_target() {
        let (_d, l, r) = pair("sync-aside-win");
        fs::write(r.join("extra"), "x").unwrap();
        let done = apply(&plan_of(&l, &r, true), &AtomicBool::new(false), &Progress::default()).unwrap();
        assert_eq!(done.deleted, 1, "{:?}", done.failed);
        let kept = done.kept_at.clone().expect("kept in the target");
        assert!(kept.starts_with(&r));
        assert!(done.kept_text().unwrap().contains(&kept.display().to_string()));
        let u = done.record.undo(|_| {});
        assert!(u.refused.is_empty(), "{:?}", u.refused);
        assert_eq!(fs::read_to_string(r.join("extra")).unwrap(), "x");
        assert_eq!(fs::read_dir(&r).unwrap().count(), 1);
    }
}

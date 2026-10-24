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
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Whether one side can be made like the other here.
pub const AVAILABLE: bool = cfg!(unix);
/// The plan shows at most this many rows; the run takes them all.
pub const SHOWN: usize = 2_000;
/// The steps a plan holds at most: past them it stops, and says so.
pub const MOST: usize = 100_000;
/// The name the run's own folder of what it set aside starts with; never compared nor copied.
pub const ASIDE: &str = ".coxswain-sync-";
/// Times of change two seconds apart are the same (FAT), as Compare folders has it.
const SLACK: i64 = 2;

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
    if !AVAILABLE {
        return Err(t!("sync.not_windows"));
    }
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

#[cfg(unix)]
pub use imp::{apply, plan};

#[cfg(not(unix))]
pub fn plan(_from: &Path, _to: &Path, _mode: Mode, _delete: bool, _stop: &AtomicBool, _progress: &Progress) -> Result<Plan, String> {
    Err(t!("sync.not_windows"))
}

#[cfg(not(unix))]
pub fn apply(_plan: &Plan, _stop: &AtomicBool, _progress: &Progress) -> Result<Applied, String> {
    Err(t!("sync.not_windows"))
}

impl Undo {
    /// The target folder, to read again once undone.
    pub fn touched(&self) -> PathBuf {
        self.to.clone()
    }

    /// Undo the run: what it made, taken away (only the very entries, unchanged since, a folder
    /// only once empty), then what it set aside put back where it was, never over what is there.
    /// `restore` brings the folder aside back out of the bin. How many were undone, and why the
    /// others were not.
    #[cfg(unix)]
    pub fn undo(&self, restore: impl Fn(&Path, &Path) -> std::io::Result<()>) -> (usize, Vec<String>) {
        imp::undo(self, restore)
    }

    #[cfg(not(unix))]
    pub fn undo(&self, _restore: impl Fn(&Path, &Path) -> std::io::Result<()>) -> (usize, Vec<String>) {
        (0, vec![t!("sync.not_windows")])
    }
}

#[cfg(unix)]
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

        fn push(&mut self, rel: Rel, act: Act, src: Option<Stat>, dst: Option<Stat>, inside: Option<Inside>) {
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
            self.steps.push(Step { rel, act, src, dst, inside });
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

        /// A source entry to copy over (`dst`: what it replaces, in `d`), and all inside it when
        /// a folder.
        fn copy(&mut self, s: &Dir, d: Option<&Dir>, rel: Rel, a: Stat, dst: Option<Stat>) {
            match a.kind {
                Kind::Other => return self.said.say(&self.path(0, &rel), t!("pinned.special")),
                Kind::Dir if a.id.dev != self.devs[0] => return self.said.say(&self.path(0, &rel), t!("pinned.other_disk")),
                Kind::Dir if self.a_root(&a.id) => return self.said.say(&self.path(0, &rel), t!("sync.nested")),
                _ => {}
            }
            let inside = match (d, dst) {
                (Some(d), Some(y)) => match self.inside(d, &rel, y) {
                    Ok(i) => i,
                    Err(()) => return,
                },
                _ => None,
            };
            let act = if dst.is_some() { Act::Replace } else { Act::Copy };
            self.push(rel.clone(), act, Some(a), dst, inside);
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
            // The run's own folder aside, at the top, is no one's entry.
            let top = rel.is_empty();
            let list = |side: usize, dir: &Dir, me: &mut Self| match dir.names() {
                Ok(n) => Some(n.into_iter().filter(|n| !(top && n.to_string_lossy().starts_with(ASIDE))).collect::<Vec<_>>()),
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
                    (Some(x), Some(y)) => x.cmp(y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    _ => std::cmp::Ordering::Greater,
                };
                let name = if order == std::cmp::Ordering::Greater { b[j].clone() } else { a[i].clone() };
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
                        } else if self.delete {
                            if let Ok(inside) = self.inside(d.expect("listed"), &sub, y) {
                                self.push(sub, Act::Delete, None, Some(y), inside);
                            }
                        } else {
                            self.plan.extra += 1;
                        }
                    }
                    (Some(x), Some(y)) => self.both(s, d.expect("listed"), sub, x, y),
                    (None, None) => {}
                }
            }
        }

        /// An entry on both sides.
        fn both(&mut self, s: &Dir, d: &Dir, rel: Rel, x: Stat, y: Stat) {
            let name = rel.last().cloned().unwrap_or_default();
            if x.kind == Kind::Other {
                return self.said.say(&self.path(0, &rel), t!("pinned.special"));
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
                self.copy(s, Some(d), rel, x, Some(y));
            }
        }
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
    fn kept_name(k: usize, name: &OsString) -> OsString {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let mut v = format!("{k}-").into_bytes();
        v.extend(name.as_bytes().iter().take(200));
        OsString::from_vec(v)
    }

    /// Take the steps of `plan`, each only on the entries exactly as the plan saw them.
    pub fn apply(plan: &Plan, stop: &AtomicBool, progress: &Progress) -> Result<Applied, String> {
        apply_to(plan, stop, progress, |p| if crate::fs::trash_takes(&[p.to_path_buf()]) { crate::fs::trash_named(p, None) } else { Err(std::io::Error::other(t!("undo.no_trash"))) })
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
            if let (Some(was), Some(a)) = (step.dst, aside.as_ref()) {
                let k_name = kept_name(k, name);
                let unchanged = match &step.inside {
                    None => Ok(()),
                    Some(inside) => dd
                        .sub(name, was.id, false)
                        .and_then(|sub| tree(&sub, was.id.dev, usize::MAX, stop))
                        .and_then(|now| if now == **inside { Ok(()) } else { Err(crate::pinned::changed()) }),
                };
                if let Err(e) = unchanged.and_then(|()| dd.move_to(name, was.id, a, &k_name)) {
                    failed.push(format!("{}: {e}", at(to).display()));
                    dst.lost(&step.rel);
                    continue;
                }
                if step.act == Act::Delete {
                    undo.kept.push((step.rel.clone(), k_name, was));
                    deleted += 1;
                    continue;
                }
                kept = Some((k_name, was));
            }
            let Some((was, source)) = source else { continue };
            let made = match source {
                From::File(mut f) => dd.create_file(name, &mut f, &was, stop).map(|m| (m, None)),
                From::Link(l) => dd.make_link(name, &l).map(|m| (m, None)),
                From::Dir => dd.make_dir(name).and_then(|nd| Ok((nd.stat()?, Some(nd)))),
            };
            match made {
                Ok((m, nd)) => {
                    undo.made.push((step.rel.clone(), m));
                    if let Some(k) = kept {
                        undo.kept.push((step.rel.clone(), k.0, k.1));
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
                        && let Err(back) = a.move_back(&k_name, old.id, dd, name)
                    {
                        undo.kept.push((step.rel.clone(), k_name, old));
                        failed.push(format!("{}: {back}", at(to).display()));
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
                    Some(n) if root.stat_at(&name).is_err() && a.parent_is_not(&root) => undo.trashed = Some(n.unwrap_or_default()),
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;

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
        std::os::unix::fs::symlink("same", l.join("link")).unwrap();
        // Kept unless deleting is asked for.
        let p = plan_of(&l, &r, false);
        assert_eq!((p.copies, p.replaces, p.deletes, p.extra), (4, 1, 0, 1), "{:?}", p.rows);
        let p = plan_of(&l, &r, true);
        assert_eq!(p.deletes, 1);
        let done = run(&p);
        assert!(done.failed.is_empty(), "{:?}", done.failed);
        assert_eq!((done.copied, done.replaced, done.deleted), (4, 1, 1));
        assert_eq!(fs::read_to_string(r.join("new/deep/f")).unwrap(), "deep");
        assert_eq!(fs::read_to_string(r.join("changed")).unwrap(), "new text");
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

    #[test]
    fn set_id_bits_are_not_copied() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, l, r) = pair("sync-setid");
        fs::write(l.join("tool"), "#!/bin/sh").unwrap();
        fs::set_permissions(l.join("tool"), fs::Permissions::from_mode(0o4755)).unwrap();
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
        std::os::unix::fs::symlink(&out, r.join("sub")).unwrap();
        let done = run(&p);
        assert_eq!(done.copied + done.replaced, 0);
        assert!(!done.failed.is_empty());
        assert_eq!(fs::read_to_string(out.join("f")).unwrap(), "precious");
        assert!(!out.join("g").exists());
        // The source side swapped: nothing read from outside.
        fs::remove_file(r.join("sub")).unwrap();
        fs::rename(d.join("moved"), r.join("sub")).unwrap();
        let p = plan_of(&l, &r, true);
        fs::rename(l.join("sub"), d.join("lmoved")).unwrap();
        std::os::unix::fs::symlink(&out, l.join("sub")).unwrap();
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
        use std::os::unix::fs::MetadataExt;
        assert_eq!(fs::metadata(r.join("f")).unwrap().ino(), fs::metadata(&out).unwrap().ino());
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
}

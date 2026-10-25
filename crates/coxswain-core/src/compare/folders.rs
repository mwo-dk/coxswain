//! Compare folders (docs/files/compare-folders.md): the left panel's folder against the right
//! one's, every file and folder in them by its path under each, on a thread of its own. Each
//! folder is listed as the panels list it (`fs::list`), so an archive, a container image, a
//! snapshot or a git history is compared too, read-only. Two folders are walked side by side,
//! a folder at a time, their names in order: nothing is held but what was found, which the
//! panels show as it comes in. Stop (or dropping it) ends the walk.
//!
//! Files match by size and time of change (`Quick`, two seconds apart still the same, as FAT
//! keeps them), or byte by byte (`Content`, a megabyte at a time, stopping at the first
//! difference; inside an archive, where bytes are not at hand, by size and time).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// How two files with the same path are matched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Size and time of change.
    #[default]
    Quick,
    /// Every byte.
    Content,
}

/// How an entry stands against the other side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    OnlyLeft,
    OnlyRight,
    NewerLeft,
    NewerRight,
    /// Not the same, and neither is newer (or a file against a folder).
    Different,
    Same,
}

impl State {
    pub const ALL: [State; 6] = [State::OnlyLeft, State::OnlyRight, State::NewerLeft, State::NewerRight, State::Different, State::Same];

    /// What the other side has of it: only-left is only-right there, newer-left older.
    pub fn flip(self) -> State {
        match self {
            State::OnlyLeft => State::OnlyRight,
            State::OnlyRight => State::OnlyLeft,
            State::NewerLeft => State::NewerRight,
            State::NewerRight => State::NewerLeft,
            s => s,
        }
    }

    /// Side `side` (0 left, 1 right) has something the other lacks or has older: what
    /// *Mark differences* marks there, and what copying from there brings across.
    pub fn ahead(self, side: usize) -> bool {
        let s = if side == 0 { self } else { self.flip() };
        matches!(s, State::OnlyLeft | State::NewerLeft | State::Different)
    }
}

/// What was found, by path under the two folders (`/` between names).
#[derive(Default)]
pub struct Found {
    pub states: HashMap<String, State>,
    /// Files counted in each state (a folder only on one side counts as one), and their bytes
    /// (the left side's, or the only side's).
    pub counts: HashMap<State, (u64, u64)>,
    pub done: bool,
    pub error: Option<String>,
    /// Folders below `DEEPEST` were not compared (marked different): the summary says so.
    pub too_deep: bool,
    /// Something was found that is not the same, whatever the counts say.
    pub differ: bool,
}

/// How deep the walk goes: below this a folder is marked different, not walked into, so a tree
/// nested without end (or archives in archives) cannot run the walk out of stack.
pub const DEEPEST: usize = 1024;

impl Found {
    /// The summary line: how many files are in each state, and their size, the empty ones
    /// left out; "The folders are the same" only when the walk found nothing that is not.
    pub fn summary(&self) -> String {
        let parts: Vec<String> = State::ALL
            .iter()
            .filter_map(|s| self.counts.get(s).filter(|c| c.0 > 0).map(|c| (s, c)))
            .map(|(s, &(n, bytes))| crate::t!("compare.state_count", "n" => crate::settings::count(n as usize), "state" => s.label(), "size" => crate::settings::human(bytes)))
            .collect();
        let deep = self.too_deep.then(|| crate::t!("compare.too_deep", "n" => DEEPEST));
        if !self.too_deep && !self.differ {
            return crate::t!("compare.folders_same");
        }
        let parts = if parts.is_empty() && deep.is_none() { vec![State::Different.label()] } else { parts };
        parts.into_iter().chain(deep).collect::<Vec<_>>().join(" · ")
    }
}

impl State {
    /// Its name, as the summary and the legend say it.
    pub fn label(self) -> String {
        crate::t!(match self {
            State::OnlyLeft => "compare.only_left",
            State::OnlyRight => "compare.only_right",
            State::NewerLeft => "compare.newer_left",
            State::NewerRight => "compare.newer_right",
            State::Different => "compare.different",
            State::Same => "compare.same_state",
        })
    }

    /// The mark beside a name in a panel on side `side`: + only here, > newer here, < older
    /// here, ≠ different, = the same.
    pub fn glyph(self, side: usize) -> &'static str {
        match if side == 0 { self } else { self.flip() } {
            State::OnlyLeft => "+",
            State::NewerLeft => ">",
            State::NewerRight => "<",
            State::Different => "≠",
            State::Same => "=",
            State::OnlyRight => "-",
        }
    }
}

/// A compare of two folders, running or done.
pub struct Folders {
    pub roots: [PathBuf; 2],
    pub mode: Mode,
    found: Arc<Mutex<Found>>,
    stop: Arc<AtomicBool>,
    /// Entries looked at so far.
    pub seen: Arc<AtomicU64>,
}

/// The time of change two seconds apart is the same: FAT keeps it to two seconds.
const SLACK: u64 = 2;

fn same_bytes(a: &Path, b: &Path, stop: &AtomicBool) -> std::io::Result<bool> {
    let (mut fa, mut fb) = (std::fs::File::open(a)?, std::fs::File::open(b)?);
    let (mut x, mut y) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let n = read_full(&mut fa, &mut x)?;
        let m = read_full(&mut fb, &mut y)?;
        if n != m || x[..n] != y[..m] {
            return Ok(false);
        }
        if n == 0 {
            return Ok(true);
        }
    }
}

fn read_full(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// Two files with the same path matched.
fn file_state(a: &crate::fs::Entry, b: &crate::fs::Entry, mode: Mode, stop: &AtomicBool) -> State {
    let newer = || match a.modified.abs_diff(b.modified) <= SLACK {
        true => State::Different,
        false if a.modified > b.modified => State::NewerLeft,
        false => State::NewerRight,
    };
    if a.size != b.size {
        return newer();
    }
    // Bytes only where both are on the disk.
    if mode == Mode::Content && a.path.is_file() && b.path.is_file() {
        return if same_bytes(&a.path, &b.path, stop).unwrap_or(false) { State::Same } else { newer() };
    }
    if a.modified.abs_diff(b.modified) <= SLACK { State::Same } else { newer() }
}

/// The entries of `dir` by name, `..` and a sync run's own leftovers (`sync::ours`) left out:
/// those are Coxswain's, not the user's, and would keep two folders made alike from being the same.
fn entries(dir: &Path) -> Vec<crate::fs::Entry> {
    let mut v: Vec<_> = crate::fs::list(dir, true).unwrap_or_default().into_iter().filter(|e| !e.is_parent() && !super::sync::ours(&e.name)).collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

struct Walk<'a> {
    mode: Mode,
    found: &'a Mutex<Found>,
    stop: &'a AtomicBool,
    seen: &'a AtomicU64,
    /// `DEEPEST`, but in a test.
    deepest: usize,
}

impl Walk<'_> {
    fn put(&self, rel: &str, state: State, files: u64, bytes: u64) {
        let mut f = self.found.lock().unwrap_or_else(|e| e.into_inner());
        f.differ |= state != State::Same;
        f.states.insert(rel.to_string(), state);
        if files > 0 {
            let c = f.counts.entry(state).or_default();
            c.0 += files;
            c.1 += bytes;
        }
    }

    /// Folders `l` and `r` (by their path `rel` under the roots): whether all in them is the same.
    fn folder(&self, l: &Path, r: &Path, rel: &str, depth: usize) -> bool {
        if depth >= self.deepest {
            self.found.lock().unwrap_or_else(|e| e.into_inner()).too_deep = true;
            return false;
        }
        let (a, b) = (entries(l), entries(r));
        let (mut i, mut j, mut same) = (0, 0, true);
        while (i < a.len() || j < b.len()) && !self.stop.load(Ordering::Relaxed) {
            let order = match (a.get(i), b.get(j)) {
                (Some(x), Some(y)) => x.name.cmp(&y.name),
                (Some(_), None) => std::cmp::Ordering::Less,
                _ => std::cmp::Ordering::Greater,
            };
            self.seen.fetch_add(1, Ordering::Relaxed);
            let name = if order == std::cmp::Ordering::Greater { &b[j].name } else { &a[i].name };
            let sub = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let state = match order {
                std::cmp::Ordering::Less => {
                    let e = &a[i];
                    i += 1;
                    self.put(&sub, State::OnlyLeft, 1, e.size);
                    State::OnlyLeft
                }
                std::cmp::Ordering::Greater => {
                    let e = &b[j];
                    j += 1;
                    self.put(&sub, State::OnlyRight, 1, e.size);
                    State::OnlyRight
                }
                std::cmp::Ordering::Equal => {
                    let (x, y) = (&a[i], &b[j]);
                    i += 1;
                    j += 1;
                    // A link is compared as a link (where it points), never followed: the walk
                    // stays inside the two folders.
                    let (xd, yd) = (x.is_dir && !x.is_symlink, y.is_dir && !y.is_symlink);
                    let s = match (xd, yd) {
                        _ if x.is_symlink || y.is_symlink => match (std::fs::read_link(&x.path), std::fs::read_link(&y.path)) {
                            (Ok(a), Ok(b)) if a == b => State::Same,
                            _ => State::Different,
                        },
                        (true, true) => {
                            if self.folder(&x.path, &y.path, &sub, depth + 1) { State::Same } else { State::Different }
                        }
                        (false, false) => file_state(x, y, self.mode, self.stop),
                        _ => State::Different,
                    };
                    // A folder against a folder counts what is in them; anything else (a file
                    // or a link against a folder too) is one item, sized by its file side.
                    self.put(&sub, s, u64::from(!(xd && yd)), if xd { y.size } else { x.size });
                    s
                }
            };
            same &= state == State::Same;
        }
        same
    }
}

impl Folders {
    /// Start comparing folder `left` with `right`.
    pub fn open(left: &Path, right: &Path, mode: Mode) -> Folders {
        let f = Folders { roots: [left.to_path_buf(), right.to_path_buf()], mode, found: Arc::default(), stop: Arc::default(), seen: Arc::default() };
        let (l, r, found, stop, seen) = (left.to_path_buf(), right.to_path_buf(), f.found.clone(), f.stop.clone(), f.seen.clone());
        std::thread::spawn(move || {
            Walk { mode, found: &found, stop: &stop, seen: &seen, deepest: DEEPEST }.folder(&l, &r, "", 0);
            let mut f = found.lock().unwrap_or_else(|e| e.into_inner());
            if stop.load(Ordering::Relaxed) {
                f.error = Some(crate::t!("compare.stopped"));
            }
            f.done = true;
        });
        f
    }

    pub fn found(&self) -> MutexGuard<'_, Found> {
        self.found.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// The path under the roots of `path`, shown on side `side`; none when it is not under it.
    pub fn rel(&self, side: usize, path: &Path) -> Option<String> {
        let r = path.strip_prefix(&self.roots[side]).ok()?;
        Some(r.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
    }

    /// How `path` on side `side` stands (as that side sees it: only-left is only-here on the left).
    pub fn state(&self, side: usize, path: &Path) -> Option<State> {
        let rel = self.rel(side, path)?;
        self.found().states.get(&rel).copied()
    }

    /// The other side's path for `rel`.
    pub fn path(&self, side: usize, rel: &str) -> PathBuf {
        if rel.is_empty() { self.roots[side].clone() } else { self.roots[side].join(rel) }
    }


}

impl Drop for Folders {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_compared_entry_by_entry() {
        let d = super::super::tests::tempfile_dir("folders");
        let (l, r) = (d.join("l"), d.join("r"));
        for p in [&l, &r] {
            std::fs::create_dir_all(p.join("same")).unwrap();
            std::fs::write(p.join("same/a.txt"), "a").unwrap();
            std::fs::write(p.join("both.txt"), "one").unwrap();
        }
        std::fs::write(r.join("both.txt"), "two!").unwrap();
        std::fs::write(l.join("only-left.txt"), "x").unwrap();
        std::fs::create_dir_all(r.join("extra/deep")).unwrap();
        std::fs::write(r.join("extra/deep/z"), "z").unwrap();
        let f = Folders::open(&l, &r, Mode::Content);
        while !f.found().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let st = |p: &str| f.found().states.get(p).copied();
        assert_eq!(st("same"), Some(State::Same));
        assert_eq!(st("same/a.txt"), Some(State::Same));
        assert_eq!(st("only-left.txt"), Some(State::OnlyLeft));
        assert_eq!(st("extra"), Some(State::OnlyRight));
        assert!(matches!(st("both.txt"), Some(State::Different | State::NewerRight | State::NewerLeft)));
        assert_eq!(f.state(1, &r.join("extra")), Some(State::OnlyRight));
        assert_eq!(f.found().counts.get(&State::OnlyRight).map(|c| c.0), Some(1), "a folder only there counts");
        assert!(State::OnlyLeft.ahead(0) && !State::OnlyLeft.ahead(1) && State::NewerRight.ahead(1));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_sync_runs_leftovers_are_not_compared() {
        let d = super::super::tests::tempfile_dir("folders-leftovers");
        let (l, r) = (d.join("l"), d.join("r"));
        for p in [&l, &r] {
            std::fs::create_dir_all(p.join("sub")).unwrap();
            std::fs::write(p.join("sub/a.txt"), "a").unwrap();
        }
        std::fs::create_dir_all(r.join(".coxswain-sync-2026-10-25-120000/k-a.txt")).unwrap();
        std::fs::write(r.join("sub/.a.txt.coxswain-1-2-3.tmp"), "half").unwrap();
        std::fs::write(l.join(".coxswain-notes"), "the user's").unwrap();
        let (found, stop, seen) = (Mutex::default(), AtomicBool::default(), AtomicU64::default());
        Walk { mode: Mode::Content, found: &found, stop: &stop, seen: &seen, deepest: DEEPEST }.folder(&l, &r, "", 0);
        let found = found.into_inner().unwrap();
        assert_eq!(found.states.get("sub"), Some(&State::Same), "a half-written file is not compared");
        assert!(!found.states.keys().any(|k| k.contains("coxswain-sync") || k.ends_with(".tmp")), "{:?}", found.states);
        assert_eq!(found.states.get(".coxswain-notes"), Some(&State::OnlyLeft), "only the run's own names are left out");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_file_against_a_folder_is_one_difference() {
        let d = super::super::tests::tempfile_dir("folders-clash");
        let (l, r) = (d.join("l"), d.join("r"));
        std::fs::create_dir_all(&l).unwrap();
        std::fs::create_dir_all(r.join("x")).unwrap();
        std::fs::write(l.join("x"), "a file").unwrap();
        let (found, stop, seen) = (Mutex::default(), AtomicBool::default(), AtomicU64::default());
        assert!(!Walk { mode: Mode::Quick, found: &found, stop: &stop, seen: &seen, deepest: DEEPEST }.folder(&l, &r, "", 0));
        let found = found.into_inner().unwrap();
        assert_eq!(found.states.get("x"), Some(&State::Different));
        assert_eq!(found.counts.get(&State::Different), Some(&(1, 6)));
        assert_ne!(found.summary(), crate::t!("compare.folders_same"));
        // Nothing counted, but something differs: never "the same".
        let odd = Found { differ: true, ..Found::default() };
        assert_ne!(odd.summary(), crate::t!("compare.folders_same"));
        assert_eq!(Found::default().summary(), crate::t!("compare.folders_same"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_tree_deeper_than_the_cap_is_marked_not_walked() {
        let d = super::super::tests::tempfile_dir("folders-deep");
        let (l, r) = (d.join("l"), d.join("r"));
        // The cap made small: a real path 1,024 folders deep is longer than macOS allows.
        let deepest = 4;
        let rel: PathBuf = std::iter::repeat_n("d", deepest + 6).collect();
        for root in [&l, &r] {
            std::fs::create_dir_all(root.join(&rel)).unwrap();
        }
        std::fs::write(l.join(&rel).join("f"), "x").unwrap();
        let (found, stop, seen) = (Mutex::default(), AtomicBool::default(), AtomicU64::default());
        let same = Walk { mode: Mode::Quick, found: &found, stop: &stop, seen: &seen, deepest }.folder(&l, &r, "", 0);
        assert!(!same);
        let found = found.into_inner().unwrap();
        assert!(found.too_deep);
        let at = |n: usize| vec!["d"; n].join("/");
        assert_eq!(found.states.get(&at(deepest)), Some(&State::Different), "the deepest compared is marked different");
        assert_eq!(found.states.get(&at(deepest + 1)), None, "nothing below it is walked");
        assert_eq!(found.states.get("d"), Some(&State::Different));
        assert!(found.summary().contains(&DEEPEST.to_string()), "{}", found.summary());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn links_are_compared_as_links_and_not_walked_into() {
        use std::os::unix::fs::symlink;
        let d = super::super::tests::tempfile_dir("folders-links");
        let (l, r, out) = (d.join("l"), d.join("r"), d.join("outside"));
        for p in [&l, &r, &out] {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(out.join("f"), "outside").unwrap();
        // Left: a link out of the tree where the right has a folder; right: the other way round.
        symlink(&out, l.join("a")).unwrap();
        std::fs::create_dir_all(r.join("a")).unwrap();
        std::fs::write(r.join("a/f"), "right").unwrap();
        std::fs::create_dir_all(l.join("b")).unwrap();
        std::fs::write(l.join("b/f"), "left").unwrap();
        symlink(&out, r.join("b")).unwrap();
        symlink(&out, l.join("c")).unwrap();
        symlink(&out, r.join("c")).unwrap();
        let f = Folders::open(&l, &r, Mode::Content);
        while !f.found().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let st = |p: &str| f.found().states.get(p).copied();
        assert_eq!(st("a"), Some(State::Different));
        assert_eq!(st("b"), Some(State::Different));
        assert_eq!(st("c"), Some(State::Same), "two links to the same place");
        assert_eq!(st("a/f"), None, "a link is not walked into");
        assert_eq!(st("b/f"), None, "a link is not walked into");
        std::fs::remove_dir_all(d).unwrap();
    }
}

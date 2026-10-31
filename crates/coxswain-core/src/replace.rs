//! Find and replace across files (docs/files/find-and-replace.md), in both apps: on Find's
//! results or the marked files and folders, text or a regular expression. First a plan, every
//! change with its file, line, before and after, of which any change or file can be left out;
//! then the ticked ones are written, each file whole or not at all: in its encoding (UTF-8 with
//! or without its mark, UTF-16, else Windows-1252, the superset of Latin-1), with its line
//! endings untouched and its permissions. The file as it was is kept aside, so one undo puts the
//! whole batch back.
//!
//! The plan and the run reach the files through `pinned`: folder handles, never a link, and the
//! run writes only the very files the plan read (same file, same ctime, same content by its
//! BLAKE3), each anew beside itself in its held folder. Binary files, files over `edit::MOST`,
//! read-only ones, hard links, other users' files and files with ACLs or extended attributes are
//! left out, and so is version control's own folder.

use crate::edit;
use crate::pinned::{self, Id, Kind as Entry, Node, Pin, Said, Set, Walk};
use crate::undo::{Kind, Record};
use crate::{t, tn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The changes a plan keeps at most: past them it stops looking, and says so.
pub const MOST_CHANGES: usize = 20_000;
/// How much of a long line a change shows on each side of what it replaces.
const AROUND: usize = 60;
/// How much of a long match (or what replaces it) a change shows: cut in the middle.
const MATCH_SHOWN: usize = 200;
/// Folders of version control, never looked into: a change there would break the repository.
const SKIP_DIRS: [&str; 5] = [".git", ".hg", ".svn", ".jj", "_darcs"];

/// What to find and what to put in its place.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Query {
    pub find: String,
    pub with: String,
    /// `find` is a regular expression, and `with` may name its groups (`$1`, `${name}`).
    pub regex: bool,
    /// Letters match only in the same case.
    pub case: bool,
    /// Only whole words match.
    pub word: bool,
}

impl Query {
    fn compile(&self) -> Result<regex::Regex, String> {
        if self.find.is_empty() {
            return Err(t!("replace.empty"));
        }
        let pat = if self.regex { self.find.clone() } else { regex::escape(&self.find) };
        let pat = if self.word { format!(r"\b(?:{pat})\b") } else { pat };
        regex::RegexBuilder::new(&pat)
            .case_insensitive(!self.case)
            .multi_line(true)
            .crlf(true)
            .build()
            .map_err(|e| e.to_string())
    }

    /// `with` as `Captures::expand` wants it: `$2_x` would mean a group named "2_x"; people mean
    /// group 2, so it is braced (as batch rename does).
    fn template(&self) -> String {
        if !self.regex {
            return String::new();
        }
        // `$$` stays a dollar sign, as it is.
        regex::Regex::new(r"\$\$|\$(\d+)").expect("valid").replace_all(&self.with, |c: &regex::Captures| c.get(1).map_or_else(|| "$$".to_string(), |n| format!("${{{}}}", n.as_str()))).into_owned()
    }
}

/// One match, and its line before and after.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// Counted from 1.
    pub line: usize,
    pub before: String,
    pub after: String,
}

/// A file with matches.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilePlan {
    pub path: PathBuf,
    pub encoding: edit::Encoding,
    /// The file as the plan read it: the run writes only this very file (same device, inode and
    /// ctime) with this very content (BLAKE3).
    pub id: Id,
    pub hash: String,
    pub changes: Vec<Change>,
}

/// A file left out, and why.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Skipped {
    pub path: PathBuf,
    pub why: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Plan {
    pub files: Vec<FilePlan>,
    /// The first `pinned::SAID` left out, and how many more.
    pub skipped: Vec<Skipped>,
    pub more: usize,
    /// Files looked at, and of them binary (left out without a line each).
    pub looked: usize,
    pub binary: usize,
    /// It stopped at `MOST_CHANGES`, or when asked: what is there is what it found until then.
    pub full: bool,
    pub stopped: bool,
    /// Which plan this is: the page sends it back with the run, which takes only that plan.
    pub token: u64,
    /// What the run takes from the plan, never from the page: where, what, and what was seen.
    #[serde(skip)]
    roots: Vec<PathBuf>,
    #[serde(skip)]
    query: Query,
    #[serde(skip)]
    set: Arc<Set>,
}

impl Plan {
    pub fn changes(&self) -> usize {
        self.files.iter().map(|f| f.changes.len()).sum()
    }

    fn skip(&mut self, path: &Path, why: String) {
        if self.skipped.len() < pinned::SAID {
            self.skipped.push(Skipped { path: path.to_path_buf(), why });
        } else {
            self.more += 1;
        }
    }
}

/// How far a plan or a run is: files looked at (or written) and changes found (or made).
#[derive(Default, Debug)]
pub struct Progress {
    pub files: AtomicUsize,
    pub changes: AtomicUsize,
}

fn floor(s: &str, mut at: usize) -> usize {
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn ceil(s: &str, mut at: usize) -> usize {
    while !s.is_char_boundary(at) {
        at += 1;
    }
    at
}

/// The line before `at`, at most `AROUND` bytes of it, with … where it was cut; only that much
/// is ever looked through, however long the line.
fn head(text: &str, at: usize) -> String {
    let lo = floor(text, at.saturating_sub(AROUND));
    match text[lo..at].rfind('\n') {
        Some(i) => text[lo + i + 1..at].to_string(),
        None if lo == 0 => text[..at].to_string(),
        None => format!("…{}", &text[lo..at]),
    }
}

/// The line after `at`, as `head` does it.
fn tail(text: &str, at: usize) -> String {
    let hi = ceil(text, (at + AROUND).min(text.len()));
    match text[at..hi].find('\n') {
        Some(i) => text[at..at + i].trim_end_matches('\r').to_string(),
        None if hi == text.len() => text[at..].to_string(),
        None => format!("{}…", &text[at..hi]),
    }
}

/// A match or what replaces it, as a change shows it: on one line, cut in the middle when long.
fn shown(s: &str) -> String {
    let s = if s.len() > MATCH_SHOWN {
        let half = MATCH_SHOWN / 2;
        format!("{}…{}", &s[..floor(s, half)], &s[ceil(s, s.len() - half)..])
    } else {
        s.to_string()
    };
    s.replace('\r', "").replace('\n', "⏎")
}

/// The BLAKE3 of a file's bytes, as text.
fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// The file `node`, read through one handle opened in its held folder (never a link, never
/// waiting on a pipe, checked to be the very file listed), and what it is. A file with other
/// hard links is left out (writing it would change it under its other names too), and so are
/// big ones and, where it is written, files with permissions beyond their bits.
fn read(node: &Node) -> Result<(Vec<u8>, pinned::Stat), String> {
    use std::io::Read as _;
    let (f, s) = node.open().map_err(|e| e.to_string())?;
    if s.kind != Entry::File {
        return Err(t!("replace.binary"));
    }
    if s.nlink > 1 {
        return Err(t!("pinned.hard_link"));
    }
    // Written anew it would lose these, or keep them for content they were not set for.
    if s.mode & 0o6000 != 0 && cfg!(unix) {
        return Err(t!("pinned.setid"));
    }
    if s.size > edit::MOST {
        return Err(t!("replace.too_big", "size" => crate::settings::human(s.size)));
    }
    #[cfg(unix)]
    if pinned::foreign_flags(&s) {
        return Err(t!("pinned.extra"));
    }
    #[cfg(unix)]
    if pinned::extra_permissions(&f) {
        return Err(t!("pinned.extra"));
    }
    let mut bytes = Vec::with_capacity(s.size as usize);
    f.take(edit::MOST + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > edit::MOST {
        return Err(t!("replace.too_big", "size" => crate::settings::human(bytes.len() as u64)));
    }
    Ok((bytes, s))
}

/// No one may write it: its bits (or on Windows its read-only attribute) say so.
fn read_only(s: &pinned::Stat) -> bool {
    if cfg!(windows) { s.mode & 1 != 0 } else { s.mode & 0o222 == 0 }
}

/// The changes in `text`, numbered from 0: every match but an empty one. The plan and the run
/// count through this one list, so a change left out is the very one the preview showed.
fn matches<'t>(re: &'t regex::Regex, text: &'t str) -> impl Iterator<Item = (usize, regex::Captures<'t>)> + 't {
    re.captures_iter(text).filter(|c| c.get(0).is_some_and(|m| !m.is_empty())).enumerate()
}

/// `text` with the first `listed` changes replaced, but those numbered in `skip`; how many were.
/// None when the result would be bigger than a file may be (`edit::MOST`).
fn replaced(text: &str, re: &regex::Regex, q: &Query, template: &str, skip: &HashSet<usize>, listed: usize) -> Option<(String, usize)> {
    let mut out = String::with_capacity(text.len());
    let (mut last, mut n) = (0, 0);
    for (i, caps) in matches(re, text) {
        if i >= listed {
            break;
        }
        let m = caps.get(0).expect("the whole match");
        if skip.contains(&i) {
            continue;
        }
        out.push_str(&text[last..m.start()]);
        // What the expansion could grow to, checked before it is made.
        if (out.len() + bound(q, template, m.len())) as u64 > edit::MOST {
            return None;
        }
        if q.regex {
            caps.expand(template, &mut out);
        } else {
            out.push_str(&q.with);
        }
        last = m.end();
        n += 1;
        // Grown past what a file may be: not made at all (`too_big`).
        if out.len() as u64 > edit::MOST {
            return None;
        }
    }
    out.push_str(&text[last..]);
    (out.len() as u64 <= edit::MOST).then_some((out, n))
}

/// The most one replacement can be: the text, and each `$` group as long as the whole match.
fn bound(q: &Query, template: &str, matched: usize) -> usize {
    if q.regex { template.len() + template.matches('$').count() * matched } else { q.with.len() }
}

/// The text a change would make, too big to be written.
fn too_big() -> String {
    t!("replace.too_big", "size" => crate::settings::human(edit::MOST + 1))
}

/// The changes in `text`, each with its line; `room` the most to keep.
fn changes(text: &str, re: &regex::Regex, q: &Query, template: &str, room: usize) -> Vec<Change> {
    let mut out = vec![];
    let (mut line, mut counted) = (1, 0);
    for (_, caps) in matches(re, text) {
        if out.len() >= room {
            break;
        }
        let m = caps.get(0).expect("the whole match");
        line += text[counted..m.start()].matches('\n').count();
        counted = m.start();
        let mut with = String::new();
        if bound(q, template, m.len()) as u64 > edit::MOST {
            with.push('…');
        } else if q.regex {
            caps.expand(template, &mut with);
        } else {
            with.push_str(&q.with);
        }
        let (head, tail) = (head(text, m.start()), tail(text, m.end()));
        out.push(Change { line, before: format!("{head}{}{tail}", shown(m.as_str())), after: format!("{head}{}{tail}", shown(&with)) });
    }
    out
}

fn walk_spec<'a>(only: Option<&'a Set>) -> Walk<'a> {
    Walk { recursive: true, skip: &SKIP_DIRS, only }
}

/// What `q` would change in the files under `roots` (files, and folders walked through). `stop`
/// ends it early with what was found so far; `progress` counts as it goes.
pub fn plan(roots: &[PathBuf], q: &Query, stop: &AtomicBool, progress: &Progress) -> Result<Plan, String> {
    let re = q.compile()?;
    let template = q.template();
    let mut plan = Plan { roots: roots.to_vec(), query: q.clone(), token: pinned::token(), ..Default::default() };
    let (mut set, mut said, mut counted) = (Set::default(), Said::default(), 0);
    pinned::walk(roots, &walk_spec(None), stop, &mut said, |node| {
        match node.stat.kind {
            Entry::Dir | Entry::Other => return true,
            Entry::Link => {
                plan.skip(&node.path, t!("pinned.link"));
                return true;
            }
            Entry::File => {}
        }
        // Reached twice (a file marked and its folder too): once is enough.
        if set.has(&node.stat.id) {
            return true;
        }
        plan.looked += 1;
        progress.files.store(plan.looked, Ordering::Relaxed);
        // A file only in the cloud is not downloaded to be read.
        if crate::cloud::unasked(&node.path) {
            plan.skip(&node.path, t!("cloud.online_only"));
            return true;
        }
        let (bytes, stat) = match read(node) {
            Ok(r) => r,
            Err(why) => {
                plan.skip(&node.path, why);
                return true;
            }
        };
        let Some((text, encoding)) = edit::decode(&bytes) else {
            plan.binary += 1;
            return true;
        };
        let hash = hash(&bytes);
        drop(bytes);
        let found = changes(&text, &re, q, &template, MOST_CHANGES - counted);
        if found.is_empty() {
            return true;
        }
        // Only a read-only file with a match is worth a line.
        if read_only(&stat) {
            plan.skip(&node.path, t!("replace.read_only"));
            return true;
        }
        // A character the file's encoding has no byte for: the file is left as it is.
        let Some((new, _)) = replaced(&text, &re, q, &template, &HashSet::new(), found.len()) else {
            plan.skip(&node.path, too_big());
            return true;
        };
        if let Err(c) = edit::encode(&new, encoding, false) {
            plan.skip(&node.path, t!("replace.unencodable", "char" => c));
            return true;
        }
        set.add(node);
        counted += found.len();
        plan.files.push(FilePlan { path: node.path.clone(), encoding, id: stat.id, hash, changes: found });
        progress.changes.store(counted, Ordering::Relaxed);
        if counted >= MOST_CHANGES {
            plan.full = true;
            return false;
        }
        true
    });
    for (path, why) in said.lines {
        plan.skip(&path, why);
    }
    plan.more += said.more;
    plan.stopped = stop.load(Ordering::Relaxed);
    plan.set = Arc::new(set);
    Ok(plan)
}

/// A file of the plan to write (its number in `Plan::files`), and the numbers of its changes
/// left out.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pick {
    pub file: usize,
    #[serde(default)]
    pub skip: Vec<usize>,
}

/// What a run did: the changes made, the files written, what failed (`path: why`) and the
/// record that undoes it.
pub struct Applied {
    pub replaced: usize,
    pub files: usize,
    pub failed: Vec<String>,
    pub record: Record,
}

impl Applied {
    /// "Replaced 42 in 7 files".
    pub fn summary(&self) -> String {
        tn!("replace.done", self.replaced, "files" => tn!("preview.files", self.files))
    }
}

/// The most a run keeps aside for undo: past it, files are left as they are (`no_keep`).
const KEPT_MOST: u64 = 256 << 20;

/// The handles that hold this app's kept folders locked while it runs: `sweep` leaves those.
static HELD: std::sync::Mutex<Vec<(PathBuf, std::fs::File)>> = std::sync::Mutex::new(vec![]);

/// A kept folder no longer needed (undone, or dropped from the undo history): its lock let go.
pub(crate) fn release(dir: &Path) {
    if let Ok(mut held) = HELD.lock() {
        held.retain(|(d, _)| d != dir);
    }
}

/// A folder of this run's own in the system's temporary folder, for the files as they were:
/// made for this user alone (0700, never one that is there, under a name not to be guessed), and
/// locked for as long as this app runs. On unix it is made and locked under a passing name and
/// only then given its own, so a folder under a `coxswain-replace-` name is always locked while
/// its run lives, and `sweep` can take one that is not at once.
fn keep_dir() -> std::io::Result<PathBuf> {
    static RUN: AtomicUsize = AtomicUsize::new(0);
    let mut last = std::io::Error::from(std::io::ErrorKind::AlreadyExists);
    for _ in 0..16 {
        let n = RUN.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let salt = blake3::hash(format!("{}-{n}-{nanos}-{:p}", std::process::id(), &last).as_bytes()).to_hex();
        let name = format!("coxswain-replace-{}-{}", crate::spill::run(), &salt[..16]);
        let dir = std::env::temp_dir().join(&name);
        #[cfg(unix)]
        let making = std::env::temp_dir().join(format!(".making-{name}"));
        #[cfg(not(unix))]
        let making = dir.clone();
        match private_dir(&making) {
            Ok(()) => {
                let mut o = std::fs::OpenOptions::new();
                o.write(true).create_new(true);
                #[cfg(unix)]
                std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
                let f = o.open(making.join(".lock"))?;
                #[cfg(unix)]
                {
                    use std::os::fd::AsRawFd;
                    // SAFETY: the file just made.
                    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    // shortcut: a crash between these lines leaves an empty `.making-…` folder;
                    // name it in `sweep` if one is ever seen.
                    std::fs::rename(&making, &dir)?;
                }
                HELD.lock().map_err(|_| std::io::Error::other("lock"))?.push((dir.clone(), f));
                return Ok(dir);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Every folder of undo copies in the temporary folder that is this user's own, a live run's
/// too (Disk use shows them all; its Remove takes only those of runs that ended, by `sweep`).
pub(crate) fn kept_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(std::env::temp_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("coxswain-replace-"))
        .filter(|e| {
            std::fs::symlink_metadata(e.path()).is_ok_and(|m| {
                #[cfg(unix)]
                // SAFETY: geteuid has no preconditions.
                let mine = std::os::unix::fs::MetadataExt::uid(&m) == unsafe { libc::geteuid() };
                #[cfg(not(unix))]
                let mine = true;
                m.is_dir() && mine
            })
        })
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

/// Folders kept by runs of an app that has ended (it can undo them no more): removed, when they
/// are this user's own and no running app holds their lock.
pub(crate) fn sweep() {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let tmp = std::env::temp_dir();
        // Only where no one else may rename what is ours: a sticky folder, or one only we write.
        let Ok(t) = std::fs::metadata(&tmp) else { return };
        if t.mode() & 0o1000 == 0 && t.mode() & 0o022 != 0 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(&tmp) else { return };
        // SAFETY: geteuid has no preconditions.
        let me = unsafe { libc::geteuid() };
        let own = format!("coxswain-replace-{}-", crate::spill::run());
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            // This app's own folders are never swept, whatever its locks say (NFS, Solaris).
            if !name.starts_with("coxswain-replace-") || name.starts_with(&own) {
                continue;
            }
            let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
            if !m.is_dir() || m.uid() != me {
                continue;
            }
            let lock = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(e.path().join(".lock"));
            let free = match &lock {
                // A lock no one holds: its run has ended (`keep_dir` locks a folder before it
                // gives it this name).
                // SAFETY: an open file; the lock, if got, goes with the handle.
                Ok(f) => (unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }) == 0,
                // No lock at all: an older version's folder it was making this moment is left
                // (it named the folder before locking it); an old one goes.
                Err(_) => m.mtime() < crate::fs::now() as i64 - 600,
            };
            if free {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    // Windows: the temp folder is this user's own; a folder goes when its run ended
    // (`spill::in_use`), and one a live run holds cannot be removed while its lock file is open.
    #[cfg(not(unix))]
    for e in std::fs::read_dir(std::env::temp_dir()).into_iter().flatten().flatten() {
        let p = e.path();
        if e.file_name().to_string_lossy().starts_with("coxswain-replace-")
            && std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
            && !crate::spill::in_use(&p)
        {
            let _ = std::fs::remove_dir_all(&p);
        }
    }
}

/// A new folder for this user alone, made so (0700) from the start, never one that is there.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

/// Undo: the file as it was, kept in this user's own folder: read without following a link, and
/// only when it and its folder are still this user's own, the folder closed to others.
pub(crate) fn read_kept(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let mut f = o.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no preconditions.
        let me = unsafe { libc::geteuid() };
        let (m, d) = (f.metadata()?, std::fs::symlink_metadata(path.parent().unwrap_or(Path::new("/")))?);
        if !m.is_file() || m.uid() != me || m.nlink() != 1 || !d.is_dir() || d.uid() != me || d.mode() & 0o077 != 0 {
            return Err(pinned::changed());
        }
    }
    let mut bytes = vec![];
    f.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Undo: `bytes` back into the file at `path`, in the folder it was written in, and only while
/// it is still the very file written then, unchanged since.
pub(crate) fn write_back(path: &Path, bytes: &[u8], pin: Pin) -> std::io::Result<()> {
    use std::io::Read as _;
    let p = pinned::Place::reopen(path, pin.dir)?;
    let now = p.stat()?;
    // The very file written, with the very content written (its ctime may have moved by itself).
    let Some(sum) = pin.sum.filter(|_| now.id.same_file(&pin.file)) else { return Err(pinned::changed()) };
    let (f, _) = p.open(now.id)?;
    let mut was = vec![];
    f.take(edit::MOST + 1).read_to_end(&mut was)?;
    if hash(&was) != sum {
        return Err(pinned::changed());
    }
    p.rewrite(now.id, bytes).map(drop)
}

/// Write the ticked changes of `picks` in the files of `plan`, as the plan read them; `stop`
/// ends it between files.
pub fn apply(plan: &Plan, picks: &[Pick], stop: &AtomicBool, progress: &Progress) -> Result<Applied, String> {
    let q = &plan.query;
    let re = q.compile()?;
    let template = q.template();
    let by_id: HashMap<Id, usize> = plan.files.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
    let picked: HashMap<usize, HashSet<usize>> = picks.iter().filter(|p| p.file < plan.files.len()).map(|p| (p.file, p.skip.iter().copied().collect())).collect();
    let mut done = Applied { replaced: 0, files: 0, failed: vec![], record: Record::new(Kind::Replace) };
    sweep();
    let (mut keep, mut kept) = (None::<PathBuf>, 0u64);
    let (mut reached, mut said) = (HashSet::new(), Said::default());
    pinned::walk(&plan.roots, &walk_spec(Some(&plan.set)), stop, &mut said, |node| {
        let Some(i) = by_id.get(&node.stat.id).copied().filter(|i| picked.contains_key(i)) else { return true };
        if !reached.insert(i) {
            return true;
        }
        progress.files.store(reached.len(), Ordering::Relaxed);
        let file = &plan.files[i];
        let fail = |why: String| format!("{}: {why}", node.path.display());
        let written = (|| {
            let (raw, _) = read(node)?;
            // The very content the plan showed.
            if hash(&raw) != file.hash {
                return Err(t!("pinned.changed"));
            }
            let Some((text, encoding)) = edit::decode(&raw) else { return Err(t!("replace.binary")) };
            let (new, n) = replaced(&text, &re, q, &template, &picked[&i], file.changes.len()).ok_or_else(too_big)?;
            if n == 0 {
                return Ok(None);
            }
            let bytes = edit::encode(&new, encoding, false).map_err(|c| t!("replace.unencodable", "char" => c))?;
            // The file as it was read, kept from the bytes read, in a folder made here for this
            // user alone: the temp folder is everyone's.
            if keep.is_none() {
                keep = Some(keep_dir().map_err(|_| t!("replace.no_keep"))?);
            }
            kept += raw.len() as u64;
            if kept > KEPT_MOST {
                return Err(t!("replace.no_keep"));
            }
            let was = keep.as_ref().expect("made above").join(i.to_string());
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
            let written = o
                .open(&was)
                .and_then(|mut f| std::io::Write::write_all(&mut f, &raw).and_then(|_| f.sync_all()))
                .and_then(|_| node.place.rewrite(node.stat.id, &bytes));
            match written {
                Ok(id) => Ok(Some((n, was, Pin { dir: node.place.dir_id(), file: id, sum: Some(hash(&bytes)) }))),
                Err(e) => {
                    // Nothing half kept is left behind.
                    let _ = std::fs::remove_file(&was);
                    Err(e.to_string())
                }
            }
        })();
        match written {
            Ok(Some((n, was, pin))) => {
                done.record.done_pinned(&was, &node.path, pin);
                done.replaced += n;
                done.files += 1;
                progress.changes.store(done.replaced, Ordering::Relaxed);
            }
            Ok(None) => {}
            Err(why) => done.failed.push(fail(why)),
        }
        true
    });
    // What the plan read but the run did not find as it was: left as it is.
    if !stop.load(Ordering::Relaxed) {
        let mut missed: Vec<usize> = picked.keys().filter(|i| !reached.contains(*i)).copied().collect();
        missed.sort();
        done.failed.extend(missed.into_iter().map(|i| format!("{}: {}", plan.files[i].path.display(), t!("pinned.changed"))));
    }
    done.failed.extend(said.lines.into_iter().map(|(p, why)| format!("{}: {why}", p.display())));
    if done.files == 0
        && let Some(keep) = &keep
    {
        release(keep);
        let _ = std::fs::remove_dir_all(keep);
    }
    Ok(done)
}

/// Every change of a plan ticked: what a run takes when nothing is left out.
pub fn picks(plan: &Plan) -> Vec<Pick> {
    (0..plan.files.len()).map(|file| Pick { file, skip: vec![] }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(find: &str, with: &str) -> Query {
        Query { find: find.into(), with: with.into(), case: true, ..Default::default() }
    }

    fn plan_of(roots: &[PathBuf], q: &Query) -> Plan {
        plan(roots, q, &AtomicBool::new(false), &Progress::default()).unwrap()
    }

    fn apply_all(plan: &Plan) -> Applied {
        apply(plan, &picks(plan), &AtomicBool::new(false), &Progress::default()).unwrap()
    }

    fn run(roots: &[PathBuf], q: &Query) -> (Plan, Applied) {
        let plan = plan_of(roots, q);
        let done = apply_all(&plan);
        (plan, done)
    }

    #[test]
    fn plans_then_writes_keeping_encoding_line_endings_and_permissions() {
        let d = crate::fs::Scratch::named("replace-1");
        let a = d.join("a.txt");
        std::fs::write(&a, "one foo\r\ntwo\nfoo foo\r\n").unwrap();
        let utf16 = d.join("w.txt");
        let bytes: Vec<u8> = [0xff, 0xfe].into_iter().chain("say foo\r\n".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        std::fs::write(&utf16, &bytes).unwrap();
        let latin = d.join("l.txt");
        std::fs::write(&latin, b"caf\xe9 foo\n").unwrap();
        std::fs::write(d.join("bin.dat"), b"foo\0foo").unwrap();
        std::fs::create_dir(d.join(".git")).unwrap();
        std::fs::write(d.join(".git/config"), "foo").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&a, std::os::unix::fs::PermissionsExt::from_mode(0o751)).unwrap();
        let (plan, done) = run(&[d.to_path_buf()], &q("foo", "bär"));
        assert_eq!(plan.files.len(), 3);
        assert_eq!(plan.binary, 1);
        let first = &plan.files[0];
        assert_eq!(first.changes.iter().map(|c| c.line).collect::<Vec<_>>(), [1, 3, 3]);
        assert_eq!(first.changes[0].before, "one foo");
        assert_eq!(first.changes[0].after, "one bär");
        assert_eq!((done.replaced, done.files), (5, 3), "{:?}", done.failed);
        assert_eq!(done.summary(), "Replaced 5 in 3 files");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "one bär\r\ntwo\nbär bär\r\n");
        let want: Vec<u8> = [0xff, 0xfe].into_iter().chain("say bär\r\n".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(std::fs::read(&utf16).unwrap(), want);
        assert_eq!(std::fs::read(&latin).unwrap(), b"caf\xe9 b\xe4r\n");
        assert_eq!(std::fs::read(d.join(".git/config")).unwrap(), b"foo");
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&a).unwrap().permissions()) & 0o777, 0o751);
        // One undo puts every file back as it was.
        let undone = done.record.undo(|_| {});
        assert_eq!((undone.done, undone.refused.len()), (3, 0), "{:?}", undone.refused);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "one foo\r\ntwo\nfoo foo\r\n");
        assert_eq!(std::fs::read(&utf16).unwrap(), bytes);
    }

    #[test]
    fn unticked_changes_stay_and_a_file_changed_since_is_left() {
        let d = crate::fs::Scratch::named("replace-2");
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::write(&a, "x x x").unwrap();
        std::fs::write(&b, "x").unwrap();
        let plan = plan_of(&[a.clone(), b.clone()], &q("x", "y"));
        let mut picks = picks(&plan);
        picks[0].skip = vec![1];
        std::fs::write(&b, "x changed").unwrap();
        let done = apply(&plan, &picks, &AtomicBool::new(false), &Progress::default()).unwrap();
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "y x y");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "x changed");
        assert_eq!((done.replaced, done.files, done.failed.len()), (2, 1, 1), "{:?}", done.failed);
        // Changed after the run: its undo is refused.
        std::fs::write(&a, "mine now").unwrap();
        let undone = done.record.undo(|_| {});
        assert_eq!((undone.done, undone.refused.len()), (0, 1));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "mine now");
    }

    #[test]
    fn regex_case_and_whole_words() {
        let d = crate::fs::Scratch::named("replace-3");
        let a = d.join("a");
        std::fs::write(&a, "Cat cat concat\nv1.2 v3.4\n").unwrap();
        let mut query = q("cat", "dog");
        query.word = true;
        query.case = false;
        run(&[a.clone()], &query);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "dog dog concat\nv1.2 v3.4\n");
        let query = Query { find: r"v(\d)\.(\d)$".into(), with: "$2_$1".into(), regex: true, case: true, word: false };
        run(&[a.clone()], &query);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "dog dog concat\nv1.2 4_3\n");
        // `$$` is a dollar sign: `$$1` writes `$1`.
        run(&[a.clone()], &Query { find: "4_3".into(), with: "$$1".into(), regex: true, case: true, word: false });
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "dog dog concat\nv1.2 $1\n");
        assert!(Query { find: "(".into(), regex: true, ..Default::default() }.compile().is_err());
        assert!(Query::default().compile().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn links_are_not_followed_and_read_only_files_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let (d, out) = (crate::fs::Scratch::named("replace-4"), crate::fs::Scratch::named("replace-5"));
        let far = out.join("far.txt");
        std::fs::write(&far, "foo").unwrap();
        std::os::unix::fs::symlink(&far, d.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(&*out, d.join("dir")).unwrap();
        let ro = d.join("ro.txt");
        std::fs::write(&ro, "foo").unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o444)).unwrap();
        let (plan, done) = run(&[d.to_path_buf()], &q("foo", "bar"));
        assert_eq!(done.files, 0);
        assert_eq!(plan.skipped.len(), 3, "{:?}", plan.skipped);
        assert_eq!(std::fs::read_to_string(&far).unwrap(), "foo");
        assert_eq!(std::fs::read_to_string(&ro).unwrap(), "foo");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_swapped_in_after_the_plan_is_not_written_through() {
        let (d, out) = (crate::fs::Scratch::named("replace-swap"), crate::fs::Scratch::named("replace-swap-out"));
        let (a, far) = (d.join("a.txt"), out.join("far.txt"));
        std::fs::write(&a, "foo").unwrap();
        std::fs::write(&far, "foo").unwrap();
        let plan = plan_of(&[a.clone()], &q("foo", "bar"));
        std::fs::remove_file(&a).unwrap();
        std::os::unix::fs::symlink(&far, &a).unwrap();
        let done = apply_all(&plan);
        assert_eq!((done.files, done.failed.len()), (0, 1), "{:?}", done.failed);
        assert_eq!(std::fs::read_to_string(&far).unwrap(), "foo");
        assert!(std::fs::symlink_metadata(&a).unwrap().file_type().is_symlink());
    }

    #[test]
    fn hard_links_are_left_out_and_never_written_through() {
        let d = crate::fs::Scratch::named("replace-hardlink");
        let (a, b) = (d.join("a.txt"), d.join("elsewhere.txt"));
        std::fs::write(&a, "foo").unwrap();
        std::fs::hard_link(&a, &b).unwrap();
        let (plan, done) = run(&[a.clone()], &q("foo", "bar"));
        assert_eq!((plan.skipped.len(), done.files), (1, 0), "{:?}", plan.skipped);
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "foo");
        // One swapped in after the plan: not written either.
        std::fs::remove_file(&b).unwrap();
        let plan = plan_of(&[a.clone()], &q("foo", "bar"));
        std::fs::write(d.join("theirs"), "foo").unwrap();
        std::fs::remove_file(&a).unwrap();
        std::fs::hard_link(d.join("theirs"), &a).unwrap();
        assert_eq!(apply_all(&plan).files, 0);
        assert_eq!(std::fs::read_to_string(d.join("theirs")).unwrap(), "foo");
    }

    #[test]
    fn a_file_moved_away_and_replaced_after_the_plan_is_left() {
        let d = crate::fs::Scratch::named("replace-moved");
        let a = d.join("a.txt");
        std::fs::write(&a, "foo").unwrap();
        let plan = plan_of(&[d.to_path_buf()], &q("foo", "bar"));
        // Another file of the same content put in its place: not the file read.
        std::fs::rename(&a, d.join("old.txt")).unwrap();
        std::fs::write(&a, "foo").unwrap();
        assert_eq!(apply_all(&plan).files, 0);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "foo");
        assert_eq!(std::fs::read_to_string(d.join("old.txt")).unwrap(), "foo");
    }

    #[test]
    fn empty_matches_do_not_shift_which_change_is_left_out() {
        let d = crate::fs::Scratch::named("replace-empty");
        let a = d.join("a");
        std::fs::write(&a, "x y x").unwrap();
        // `x*` also matches empty between the letters; the changes are the two x's only.
        let query = Query { find: "x*".into(), with: "Z".into(), regex: true, case: true, word: false };
        let plan = plan_of(&[a.clone()], &query);
        assert_eq!(plan.files[0].changes.len(), 2);
        let mut picks = picks(&plan);
        picks[0].skip = vec![1];
        apply(&plan, &picks, &AtomicBool::new(false), &Progress::default()).unwrap();
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "Z y x");
    }

    #[test]
    fn a_full_plan_writes_only_the_changes_it_listed() {
        let d = crate::fs::Scratch::named("replace-listed");
        let a = d.join("a");
        std::fs::write(&a, "x x x x").unwrap();
        let mut plan = plan_of(&[a.clone()], &q("x", "y"));
        plan.files[0].changes.truncate(2);
        let done = apply_all(&plan);
        assert_eq!(done.replaced, 2);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "y y x x");
    }

    #[test]
    fn a_folder_swapped_for_a_link_after_the_plan_is_not_written_and_undo_keeps_to_its_folder() {
        let (d, out) = (crate::fs::Scratch::named("replace-dirswap"), crate::fs::Scratch::named("replace-dirswap-out"));
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/a"), "foo").unwrap();
        std::fs::write(out.join("a"), "foo").unwrap();
        // The outside file given the very same time.
        let t = std::fs::metadata(d.join("sub/a")).unwrap().modified().unwrap();
        std::fs::File::options().write(true).open(out.join("a")).unwrap().set_modified(t).unwrap();
        let plan = plan_of(&[d.to_path_buf()], &q("foo", "bar"));
        std::fs::rename(d.join("sub"), d.join("moved")).unwrap();
        crate::pinned::link_dir(&out, &d.join("sub"));
        let done = apply_all(&plan);
        assert_eq!((done.files, done.failed.len()), (0, 1), "{:?}", done.failed);
        assert_eq!(std::fs::read_to_string(out.join("a")).unwrap(), "foo");
        // Written where it is: the file beside takes the file's own permissions.
        #[cfg(unix)]
        std::fs::set_permissions(d.join("moved/a"), std::os::unix::fs::PermissionsExt::from_mode(0o640)).unwrap();
        let (_, done) = run(&[d.join("moved/a")], &q("foo", "bar"));
        assert_eq!(done.files, 1, "{:?}", done.failed);
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(d.join("moved/a")).unwrap().permissions()) & 0o777, 0o640);
        // Its folder swapped for a link before the undo: refused, nothing written outside.
        std::fs::rename(d.join("moved"), d.join("moved2")).unwrap();
        crate::pinned::link_dir(&out, &d.join("moved"));
        let undone = done.record.undo(|_| {});
        assert_eq!((undone.done, undone.refused.len()), (0, 1));
        assert_eq!(std::fs::read_to_string(out.join("a")).unwrap(), "foo");
        assert_eq!(std::fs::read_to_string(d.join("moved2/a")).unwrap(), "bar");
    }

    #[test]
    fn a_folder_renamed_in_after_the_plan_is_not_written() {
        let (d, out) = (crate::fs::Scratch::named("replace-renamed-in"), crate::fs::Scratch::named("replace-renamed-in-out"));
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/a"), "foo").unwrap();
        std::fs::create_dir(out.join("sub")).unwrap();
        std::fs::write(out.join("sub/a"), "foo").unwrap();
        let plan = plan_of(&[d.to_path_buf()], &q("foo", "bar"));
        std::fs::rename(d.join("sub"), out.join("was")).unwrap();
        std::fs::rename(out.join("sub"), d.join("sub")).unwrap();
        let done = apply_all(&plan);
        assert_eq!((done.files, done.failed.len()), (0, 1), "{:?}", done.failed);
        assert_eq!(std::fs::read_to_string(d.join("sub/a")).unwrap(), "foo");
        assert_eq!(std::fs::read_to_string(out.join("was/a")).unwrap(), "foo");
    }

    #[test]
    fn the_run_writes_only_the_content_the_plan_read_and_undo_only_the_file_it_wrote() {
        let d = crate::fs::Scratch::named("replace-pinned");
        std::fs::write(d.join("a"), "foo").unwrap();
        // Content changed with length and time kept: refused.
        let plan = plan_of(&[d.join("a")], &q("foo", "bar"));
        let t = std::fs::metadata(d.join("a")).unwrap().modified().unwrap();
        let f = std::fs::File::options().write(true).open(d.join("a")).unwrap();
        std::io::Write::write_all(&mut &f, b"fo0").unwrap();
        f.set_modified(t).unwrap();
        drop(f);
        assert_eq!(apply_all(&plan).files, 0);
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "fo0");
        // Written, then another file put in its place with the very same content: undo leaves it.
        std::fs::write(d.join("a"), "foo").unwrap();
        let (_, done) = run(&[d.join("a")], &q("foo", "bar"));
        std::fs::write(d.join("other"), "bar").unwrap();
        std::fs::rename(d.join("other"), d.join("a")).unwrap();
        let undone = done.record.undo(|_| {});
        assert_eq!((undone.done, undone.refused.len()), (0, 1));
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "bar");
    }

    #[cfg(unix)]
    #[test]
    fn a_pipe_never_hangs_it_and_extra_permissions_are_kept_out() {
        let d = crate::fs::Scratch::named("replace-fifo");
        let fifo = std::ffi::CString::new(d.join("pipe").as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        std::fs::write(d.join("a"), "foo").unwrap();
        let plan = plan_of(&[d.join("pipe"), d.join("a")], &q("foo", "bar"));
        assert_eq!(plan.files.len(), 1);
        // A file swapped for a pipe after the plan: the run never waits on it.
        std::fs::rename(d.join("a"), d.join("was")).unwrap();
        std::fs::rename(d.join("pipe"), d.join("a")).unwrap();
        assert_eq!(apply_all(&plan).files, 0);
        // An ACL, where setfacl can give one: left out.
        std::fs::write(d.join("acl"), "foo").unwrap();
        // POSIX.1e (Linux, UFS), else NFSv4 (ZFS on FreeBSD).
        let acl = ["u:nobody:r", "u:nobody:read_data::allow"].iter().any(|e| std::process::Command::new("setfacl").args(["-m", e]).arg(d.join("acl")).output().is_ok_and(|o| o.status.success()));
        if acl {
            let plan = plan_of(&[d.join("acl")], &q("foo", "bar"));
            assert_eq!((plan.files.len(), plan.skipped.len()), (0, 1), "{:?}", plan.skipped);
            assert_eq!(plan.skipped[0].why, t!("pinned.extra"));
        }
        #[cfg(target_os = "linux")]
        {
            let p = std::ffi::CString::new(d.join("was").as_os_str().as_encoded_bytes()).unwrap();
            // SAFETY: NUL-terminated path and name, a value of the length given.
            if unsafe { libc::setxattr(p.as_ptr(), c"user.coxswain-test".as_ptr(), b"x".as_ptr().cast(), 1, 0) } == 0 {
                let plan = plan_of(&[d.join("was")], &q("foo", "bar"));
                assert_eq!(plan.skipped.first().map(|s| s.why.clone()), Some(t!("pinned.extra")));
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn kept_folders_of_ended_runs_are_swept_and_ours_are_kept() {
        let held = keep_dir().unwrap();
        let dead = std::env::temp_dir().join(format!("coxswain-replace-test-dead-{}", std::process::id()));
        private_dir(&dead).unwrap();
        std::fs::write(dead.join(".lock"), "").unwrap();
        std::fs::write(dead.join("0"), "was").unwrap();
        // A lock no one holds is a run that ended, however new it is: gone at the first sweep
        // (Remove in Disk use), not ten minutes later. Another test's run may hold the dead
        // folder's lock for a moment while it sweeps it too.
        for _ in 0..250 {
            sweep();
            if !dead.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(held.is_dir());
        assert!(!dead.exists());
        let _ = std::fs::remove_dir_all(&held);
    }

    #[test]
    fn long_lines_and_long_matches_are_clipped_and_a_plan_stops_when_full() {
        let d = crate::fs::Scratch::named("replace-6");
        let a = d.join("a");
        let long = format!("{}foo{}\n", "é".repeat(100), "z".repeat(100));
        std::fs::write(&a, long.repeat(3)).unwrap();
        let plan = plan_of(&[a.clone()], &q("foo", "bar"));
        let c = &plan.files[0].changes[2];
        assert_eq!(c.line, 3);
        assert!(c.before.starts_with('…') && c.before.ends_with('…') && c.before.contains("foo"));
        assert!(c.before.len() < 2 * AROUND + 20);
        // A match of the whole file shows only its ends.
        std::fs::write(&a, "x".repeat(1 << 20)).unwrap();
        let plan = plan_of(&[a.clone()], &Query { find: "x+".into(), with: "$0$0".into(), regex: true, case: true, word: false });
        let c = &plan.files[0].changes[0];
        assert!(c.before.len() <= MATCH_SHOWN + 10 && c.after.len() <= MATCH_SHOWN + 10, "{} {}", c.before.len(), c.after.len());
        std::fs::write(&a, "x ".repeat(MOST_CHANGES + 5)).unwrap();
        let plan = plan_of(&[a.clone()], &q("x", "y"));
        assert!(plan.full);
        assert_eq!(plan.changes(), MOST_CHANGES);
        let stop = AtomicBool::new(true);
        let plan = super::plan(&[a], &q("x", "y"), &stop, &Progress::default()).unwrap();
        assert!(plan.stopped && plan.files.is_empty());
    }
}

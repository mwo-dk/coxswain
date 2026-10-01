//! Git history as folders, the way archives are: `<file or folder>/@history` lists the commits
//! that touched it, newest first, and `<file or folder>/@history/<commit>/…` is the folder it
//! is in as that commit had it, read-only. Also the last commit of each entry in a folder.
//! Everything through the git CLI, as `git` does for the status.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::fs::Entry;

/// The path segment after a file or folder that leads into its history.
pub const MARKER: &str = "@history";
/// Commits a history lists at most.
pub const MAX_COMMITS: usize = 2000;
/// Commits the last-change walk reads at most: an entry not changed in them is "older".
pub const LAST_WALK: usize = 5000;
/// How long one git run may take before it is stopped, so a huge repository stays usable.
const BUDGET: Duration = Duration::from_secs(4);
/// Characters of a commit id in paths: unique in any repository there is.
const ID: usize = 12;

fn git(dir: &Path) -> Command {
    let mut c = crate::git::command(dir);
    // Paths are names, never patterns.
    c.env("GIT_LITERAL_PATHSPECS", "1");
    c
}

/// What `c` prints, or its complaint as the error.
fn run(mut c: Command) -> io::Result<Vec<u8>> {
    let o = c.output()?;
    if !o.status.success() {
        let why = String::from_utf8_lossy(&o.stderr).trim().to_string();
        return Err(io::Error::other(if why.is_empty() { "git failed".into() } else { why }));
    }
    Ok(o.stdout)
}

/// Run `c` and give `f` what it prints, record by record (split at `sep`), until `f` says
/// enough, the output ends, or `BUDGET` passes. `true` when it was cut short.
fn records(mut c: Command, sep: u8, mut f: impl FnMut(&[u8]) -> bool) -> io::Result<bool> {
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let out = child.stdout.take().ok_or_else(|| io::Error::other("no output"))?;
    let (done, wait) = std::sync::mpsc::channel::<()>();
    let watch = std::thread::spawn(move || {
        let late = wait.recv_timeout(BUDGET) == Err(std::sync::mpsc::RecvTimeoutError::Timeout);
        let _ = child.kill();
        let _ = child.wait();
        late
    });
    let (mut r, mut buf, mut cut) = (BufReader::new(out), vec![], false);
    loop {
        buf.clear();
        if r.read_until(sep, &mut buf)? == 0 {
            break;
        }
        if buf.last() == Some(&sep) {
            buf.pop();
        }
        if !f(&buf) {
            cut = true;
            break;
        }
    }
    drop(done);
    Ok(watch.join().unwrap_or(false) || cut)
}

/// Where a path goes through a history: `target` is the file or folder on disk, `base` the
/// folder git runs in (`target`, or the folder holding it), `commit` the commit looked into
/// (none: the list of commits), `inner` the path below `base` at that commit, `/`-separated.
#[derive(Clone, Debug, PartialEq)]
pub struct At {
    pub target: PathBuf,
    pub base: PathBuf,
    pub commit: Option<String>,
    pub inner: String,
}

/// Whether `path` has the history marker in it, without looking at the disk.
pub fn is_history(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == MARKER)
}

/// The path of `target`'s history, or of the folder it is in at `commit`.
pub fn path(target: &Path, commit: Option<&str>) -> PathBuf {
    let p = target.join(MARKER);
    match commit {
        Some(c) => p.join(&c[..c.len().min(ID)]),
        None => p,
    }
}

/// Where `path` goes through a history, if it does: the marker after a file or folder that is
/// on disk (and is not itself a real entry named so).
pub fn split(path: &Path) -> Option<At> {
    let comps: Vec<Component> = path.components().collect();
    let i = comps.iter().position(|c| c.as_os_str() == MARKER)?;
    let target: PathBuf = comps[..i].iter().collect();
    let meta = std::fs::metadata(&target).ok()?;
    if std::fs::symlink_metadata(target.join(MARKER)).is_ok() {
        return None;
    }
    let base = if meta.is_dir() { target.clone() } else { target.parent()?.to_path_buf() };
    let mut rest = vec![];
    for c in &comps[i + 1..] {
        let Component::Normal(s) = c else { return None };
        rest.push(s.to_str()?.to_string());
    }
    let commit = match rest.first() {
        Some(c) if (4..=40).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_hexdigit()) => Some(c.clone()),
        Some(_) => return None,
        None => None,
    };
    let inner = rest.get(1..).unwrap_or_default().join("/");
    // A file's history at a commit starts in its folder.
    Some(At { target, base, commit, inner })
}

/// A commit: its id, time (seconds), author and subject.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Commit {
    pub hash: String,
    pub time: u64,
    pub author: String,
    pub subject: String,
}

impl Commit {
    pub fn short(&self) -> &str {
        &self.hash[..self.hash.len().min(7)]
    }
}

/// `%H %at %an %s`, separated by unit separators.
const FORMAT: &str = "--format=%H%x1f%at%x1f%an%x1f%s";

fn commit(rec: &[u8]) -> Option<Commit> {
    let s = String::from_utf8_lossy(rec);
    let mut f = s.splitn(4, '\x1f');
    let hash = f.next()?.trim_start_matches(['\n', '\x1e']).to_string();
    let time = f.next()?.parse().ok()?;
    Some(Commit { hash, time, author: f.next()?.to_string(), subject: f.next().unwrap_or("").to_string() })
}

/// What git calls `target` from `base`: its name, or `.` for the folder itself.
fn spec(at: &At) -> String {
    if at.target == at.base { ".".into() } else { at.target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into()) }
}

/// The commits that touched `spec` (from `base`) up to `rev`, newest first: at most `max`, and
/// whether there were more (or the walk took too long to see).
pub fn log(base: &Path, rev: &str, spec: &str, max: usize) -> io::Result<(Vec<Commit>, bool)> {
    let mut c = git(base);
    c.args(["log", "-z", FORMAT, "-n", &(max + 1).to_string(), rev, "--", spec]);
    let mut out = vec![];
    let cut = records(c, 0, |rec| {
        out.extend(commit(rec));
        true
    })?;
    if out.is_empty() && !cut {
        // Nothing: say why, when git has a reason (not a repository, no commits yet).
        run({
            let mut c = git(base);
            c.args(["rev-parse", "--verify", "-q", rev]);
            c
        })?;
    }
    let more = cut || out.len() > max;
    out.truncate(max);
    Ok((out, more))
}

/// One commit, by its id or name.
pub fn show(base: &Path, rev: &str) -> io::Result<Commit> {
    let mut c = git(base);
    c.args(["log", "-1", "-z", FORMAT, rev, "--"]);
    commit(&run(c)?).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no commit {rev}")))
}

/// A name for a commit in a listing: short id and subject, without path separators.
fn entry_name(c: &Commit) -> String {
    let subject: String = c.subject.chars().map(|ch| if matches!(ch, '/' | '\\') || ch.is_control() { '∕' } else { ch }).collect();
    format!("{} {subject}", c.short())
}

/// One line of `git ls-tree -l -z`: mode, kind, id, size, path.
fn tree_line(rec: &[u8]) -> Option<(String, &str, u64, String)> {
    let s = std::str::from_utf8(rec).ok()?;
    let (meta, path) = s.split_once('\t')?;
    let mut f = meta.split_whitespace();
    let (mode, kind, oid, size) = (f.next()?, f.next()?, f.next()?, f.next()?);
    let kind = match kind {
        "tree" => "tree",
        "blob" if mode == "120000" => "link",
        "blob" if mode == "100755" => "exec",
        "blob" => "blob",
        _ => "commit",
    };
    Some((path.to_string(), kind, size.parse().unwrap_or(0), oid.to_string()))
}

/// What git calls `inner` below the base: `.` for the base itself.
fn inner_spec(inner: &str) -> String {
    if inner.is_empty() { ".".into() } else { format!("{inner}/") }
}

/// A folder of the listing, as `fs::list` gives it: the commits of a history, or the folder at
/// a commit. `..` leads up: from the commits, back to the folder on disk.
pub fn list(dir: &Path, at: &At) -> io::Result<Vec<Entry>> {
    let up = |path: PathBuf| Entry { name: "..".into(), path, is_dir: true, is_symlink: false, is_exec: false, hidden: false, size: 0, modified: 0, created: 0 };
    let Some(rev) = &at.commit else {
        let (commits, _) = log(&at.base, "HEAD", &spec(at), MAX_COMMITS)?;
        let mut out = vec![up(at.base.clone())];
        out.extend(commits.iter().map(|c| Entry {
            name: entry_name(c),
            path: path(&at.target, Some(&c.hash)),
            is_dir: true,
            is_symlink: false,
            is_exec: false,
            hidden: false,
            size: 0,
            modified: c.time,
            created: c.time,
        }));
        return Ok(out);
    };
    let when = show(&at.base, rev)?.time;
    let mut c = git(&at.base);
    c.args(["ls-tree", "-z", "-l", rev, "--", &inner_spec(&at.inner)]);
    let prefix = if at.inner.is_empty() { String::new() } else { format!("{}/", at.inner) };
    let mut out = vec![up(dir.parent().unwrap_or(dir).to_path_buf())];
    for rec in run(c)?.split(|&b| b == 0) {
        let Some((p, kind, size, _)) = tree_line(rec) else { continue };
        let Some(name) = p.strip_prefix(&prefix).filter(|n| !n.is_empty() && !n.contains('/')) else { continue };
        out.push(Entry {
            hidden: name.starts_with('.'),
            path: dir.join(name),
            name: name.to_string(),
            is_dir: kind == "tree",
            is_symlink: kind == "link",
            is_exec: kind == "exec",
            size,
            modified: when,
            created: when,
        });
    }
    if out.len() == 1 && !at.inner.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} is not in commit {rev}", at.inner)));
    }
    Ok(out)
}

/// The first `max` bytes of a file at a commit, and whether there was more.
pub fn read(at: &At, max: usize) -> io::Result<(Vec<u8>, bool)> {
    let rev = at.commit.as_deref().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pick a commit first"))?;
    let mut c = git(&at.base);
    c.args(["cat-file", "blob", &format!("{rev}:./{}", at.inner)]);
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut buf = vec![];
    child.stdout.take().ok_or_else(|| io::Error::other("no output"))?.take(max as u64 + 1).read_to_end(&mut buf)?;
    let more = buf.len() > max;
    buf.truncate(max);
    if more {
        let _ = child.kill();
        let _ = child.wait();
        return Ok((buf, true));
    }
    let o = child.wait_with_output()?;
    if !o.status.success() {
        return Err(io::Error::other(String::from_utf8_lossy(&o.stderr).trim().to_string()));
    }
    Ok((buf, false))
}

/// A copy of a file as it was at a commit, to look at (the preview pane, F3), as
/// `archive::peek` makes one of a file in an archive: it keeps the file's name.
pub fn peek(path: &Path) -> io::Result<PathBuf> {
    let at = split(path).filter(|at| at.commit.is_some() && !at.inner.is_empty()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a file in a history"))?;
    let (bytes, more) = read(&at, 64 << 20)?;
    if more {
        return Err(io::Error::new(io::ErrorKind::FileTooLarge, format!("{} is too large to look at here; copy it out with F5", at.inner)));
    }
    let to = crate::archive::peek_folder()?.join(at.inner.rsplit('/').next().unwrap_or("file"));
    std::fs::write(&to, bytes)?;
    Ok(to)
}

/// What the commit changed in the file (or folder): its diff against the commit before.
pub fn diff(at: &At) -> io::Result<String> {
    let rev = at.commit.as_deref().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pick a commit first"))?;
    let mut c = git(&at.base);
    c.args(["show", "--format=", "--no-color", "--no-ext-diff", "--no-textconv", rev, "--", inner_spec(&at.inner).trim_end_matches('/')]);
    Ok(String::from_utf8_lossy(&run(c)?).chars().take(512 * 1024).collect())
}

/// Copy a file or folder as it was at a commit (the whole folder for a commit itself) out to
/// `dest`, as `fs::copy` would: into it when it is a folder. Returns where it landed; whatever
/// was written is removed when it fails.
pub fn copy_out(at: &At, dest: &Path) -> io::Result<PathBuf> {
    let rev = at.commit.as_deref().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pick a commit first"))?;
    let name = match at.inner.rsplit('/').next().filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => format!("{}-{}", at.base.file_name().map_or("commit".into(), |n| n.to_string_lossy()), &rev[..rev.len().min(7)]),
    };
    let to = if dest.is_dir() { dest.join(&name) } else { dest.to_path_buf() };
    if std::fs::symlink_metadata(&to).is_ok() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", to.display())));
    }
    let r = write_tree(at, rev, &to);
    if r.is_err() {
        let _ = if to.is_dir() { std::fs::remove_dir_all(&to) } else { std::fs::remove_file(&to) };
    }
    r.map(|_| to)
}

fn write_tree(at: &At, rev: &str, to: &Path) -> io::Result<()> {
    let mut c = git(&at.base);
    let spec = if at.inner.is_empty() { ".".to_string() } else { at.inner.clone() };
    c.args(["ls-tree", "-r", "-z", "-l", rev, "--", &spec]);
    let listed = run(c)?;
    let mut files = vec![];
    for rec in listed.split(|&b| b == 0) {
        let Some((p, kind, _, oid)) = tree_line(rec) else { continue };
        let rel = if at.inner.is_empty() { p.as_str() } else if p == at.inner { "" } else { match p.strip_prefix(&format!("{}/", at.inner)) { Some(r) => r, None => continue } };
        // git never stores `..` or `.git`; refuse them anyway (and a `\\` or `C:` that Windows
        // reads as a path), so nothing lands outside `to` or makes a repository there.
        let normal = Path::new(rel).components().all(|c| matches!(c, Component::Normal(_)));
        if !normal || rel.split('/').any(|s| matches!(s, "." | "..") || s.eq_ignore_ascii_case(".git")) {
            continue;
        }
        let dst = if rel.is_empty() { to.to_path_buf() } else { to.join(rel) };
        if kind == "commit" {
            std::fs::create_dir_all(&dst)?;
        } else {
            files.push((oid, kind, dst));
        }
    }
    if files.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} is not in commit {rev}", if at.inner.is_empty() { "." } else { &at.inner })));
    }
    // One git for every file: the ids in, the contents out.
    let mut c = git(&at.base);
    c.args(["cat-file", "--batch"]);
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let mut input = child.stdin.take().ok_or_else(|| io::Error::other("no input"))?;
    let ids: String = files.iter().map(|(oid, ..)| format!("{oid}\n")).collect();
    let feed = std::thread::spawn(move || input.write_all(ids.as_bytes()));
    let mut out = BufReader::new(child.stdout.take().ok_or_else(|| io::Error::other("no output"))?);
    // Links are made last: a file listed after a link must not be written through it.
    let mut links = vec![];
    let r = (|| -> io::Result<()> {
        for (_, kind, dst) in &files {
            let mut head = String::new();
            out.read_line(&mut head)?;
            let size: u64 = head.split_whitespace().nth(2).and_then(|s| s.parse().ok()).ok_or_else(|| io::Error::other(format!("git: {}", head.trim())))?;
            let mut body = (&mut out).take(size);
            if let Some(dir) = dst.parent() {
                std::fs::create_dir_all(dir)?;
            }
            if *kind == "link" {
                let mut target = String::new();
                body.read_to_string(&mut target)?;
                links.push((target, dst));
            } else {
                io::copy(&mut body, &mut std::fs::File::create_new(dst)?)?;
                #[cfg(unix)]
                if *kind == "exec" {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o755))?;
                }
            }
            out.read_exact(&mut [0u8; 1])?;
        }
        for (target, dst) in links {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, dst)?;
            #[cfg(not(unix))]
            std::fs::write(dst, &target)?;
        }
        Ok(())
    })();
    let _ = feed.join();
    let _ = child.kill();
    let _ = child.wait();
    r
}

// ---------------------------------------------------------------- last commit per entry

/// The last commit that changed an entry: when, by whom, its short id and subject.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Last {
    pub time: u64,
    pub author: String,
    pub hash: String,
    pub subject: String,
}

impl From<&Commit> for Last {
    fn from(c: &Commit) -> Last {
        Last { time: c.time, author: c.author.clone(), hash: c.short().to_string(), subject: c.subject.clone() }
    }
}

/// Entry name -> its last commit; `None`: not within the last `LAST_WALK` commits ("older").
/// Names git does not track are not in it.
pub type Lasts = HashMap<String, Option<Last>>;

/// Walks done, by (folder git ran in, commit id, path below it): a commit never changes, so
/// they hold until HEAD moves.
type Cache = HashMap<(PathBuf, String, String), Arc<Lasts>>;
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// The last commit of each entry in `dir`: a folder of a work tree (up to HEAD), a folder at a
/// commit in a history, or a history's list of commits (each its own). One `git log` for the
/// whole folder, cached by commit. `None` outside a repository.
pub fn last_changes(dir: &Path) -> Option<Arc<Lasts>> {
    let (base, rev, inner) = match split(dir) {
        Some(at) if at.commit.is_none() => {
            let (commits, _) = log(&at.base, "HEAD", &spec(&at), MAX_COMMITS).ok()?;
            return Some(Arc::new(commits.iter().map(|c| (entry_name(c), Some(Last::from(c)))).collect()));
        }
        Some(at) => (at.base, at.commit.unwrap_or_default(), at.inner),
        None => (dir.to_path_buf(), "HEAD".to_string(), String::new()),
    };
    let oid = String::from_utf8(run({
        let mut c = git(&base);
        c.args(["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")]);
        c
    }).ok()?).ok()?.trim().to_string();
    let key = (base.clone(), oid.clone(), inner.clone());
    if let Some(hit) = CACHE.lock().unwrap().as_ref().and_then(|c| c.get(&key)) {
        return Some(hit.clone());
    }
    let lasts = Arc::new(walk(&base, &oid, &inner).ok()?);
    let mut cache = CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if cache.len() >= 64 {
        cache.clear();
    }
    cache.insert(key, lasts.clone());
    Some(lasts)
}

/// The walk: the names `rev` has in the folder, then the commits from `rev` back, each name
/// taking the first that changed something at or below it, until every name has one.
fn walk(base: &Path, rev: &str, inner: &str) -> io::Result<Lasts> {
    let spec = inner_spec(inner);
    let prefix = if inner.is_empty() { String::new() } else { format!("{inner}/") };
    let mut c = git(base);
    c.args(["ls-tree", "-z", rev, "--", &spec]);
    let wanted: HashSet<String> = run(c)?
        .split(|&b| b == 0)
        .filter_map(|rec| Some(String::from_utf8_lossy(rec).split_once('\t')?.1.strip_prefix(&prefix)?.to_string()))
        .filter(|n| !n.is_empty())
        .collect();
    let mut found: Lasts = wanted.iter().map(|n| (n.clone(), None)).collect();
    let mut left = wanted.len();
    if left == 0 {
        return Ok(found);
    }
    let mut c = git(base);
    c.args(["log", "-z", "--name-only", "--relative", "--no-renames", "--format=%x1e%H%x1f%at%x1f%an%x1f%s", "-n", &LAST_WALK.to_string(), rev, "--", &spec]);
    let mut now: Option<Last> = None;
    records(c, 0, |rec| {
        if rec.first() == Some(&0x1e) {
            now = commit(rec).as_ref().map(Last::from);
            return true;
        }
        let name = String::from_utf8_lossy(rec);
        let name = name.trim_start_matches('\n');
        let Some(first) = name.strip_prefix(&prefix).and_then(|r| r.split('/').next()) else { return true };
        if let (Some(slot @ None), Some(c)) = (found.get_mut(first), &now) {
            *slot = Some(c.clone());
            left -= 1;
        }
        left > 0
    })?;
    Ok(found)
}

/// Sort entries by their last commit, newest first (`..` first, folders before files, as
/// `fs::sort` does); entries without one last.
pub fn sort_by_last(entries: &mut [Entry], lasts: &Lasts, reverse: bool) {
    let when = |e: &Entry| lasts.get(&e.name).and_then(|l| l.as_ref()).map_or(0, |l| l.time);
    entries.sort_by(|a, b| {
        let group = |e: &Entry| (!e.is_parent(), !e.is_dir);
        group(a).cmp(&group(b)).then_with(|| {
            let ord = when(b).cmp(&when(a)).then_with(|| a.name.cmp(&b.name));
            if reverse { ord.reverse() } else { ord }
        })
    });
}

// ---------------------------------------------------------------- for the search store

/// Commits of the repository at `root` for the search store, newest first: those after `since`
/// (all when `None`), at most `max`. Each is (id, time, text): a first line that says which
/// commit it is ("commit a1b2c3d · author · 2026-09-30"), the whole message, and the paths it
/// changed (at most 200).
pub fn for_search(root: &Path, since: Option<&str>, max: usize) -> io::Result<Vec<(String, u64, String)>> {
    let mut c = git(root);
    c.args(["log", "-z", "--name-only", "--no-renames", "--date=short", "--format=%x1e%H%x1f%at%x1f%an%x1f%ad%x1f%B", "-n", &max.to_string(), "HEAD"]);
    if let Some(s) = since {
        c.arg(format!("^{s}"));
    }
    c.arg("--");
    let mut out: Vec<(String, u64, String, usize)> = vec![];
    records(c, 0, |rec| {
        if rec.first() == Some(&0x1e) {
            let s = String::from_utf8_lossy(&rec[1..]);
            let f: Vec<&str> = s.splitn(5, '\x1f').collect();
            if let [hash, time, author, date, body] = f[..] {
                let text = format!("commit {} · {author} · {date}\n{}", &hash[..hash.len().min(7)], body.trim_end());
                out.push((hash.to_string(), time.parse().unwrap_or(0), text, 0));
            }
            return true;
        }
        if let Some(last) = out.last_mut().filter(|l| l.3 < 200) {
            let name = String::from_utf8_lossy(rec);
            let name = name.trim_start_matches('\n');
            if !name.is_empty() {
                last.2.push('\n');
                last.2.push_str(name);
                last.3 += 1;
            }
        }
        true
    })?;
    Ok(out.into_iter().map(|(h, t, text, _)| (h, t, text)).collect())
}

/// HEAD's commit id at `root`, and whether `old` is an ancestor of it (history only grew).
pub fn head(root: &Path, old: Option<&str>) -> Option<(String, bool)> {
    let mut c = git(root);
    c.args(["rev-parse", "--verify", "-q", "HEAD^{commit}"]);
    let now = String::from_utf8(run(c).ok()?).ok()?.trim().to_string();
    let grew = old.is_some_and(|old| {
        let mut c = git(root);
        c.args(["merge-base", "--is-ancestor", old, &now]).stdout(Stdio::null()).stderr(Stdio::null());
        c.status().is_ok_and(|s| s.success())
    });
    Some((now, grew))
}

/// The store's key of a commit of the repository at `root`: not a path on disk, so no folder
/// counts it as a file. `None` back from `from_key` for any other key.
pub fn key(root: &Path, hash: &str) -> String {
    format!("git:{}", path(root, Some(hash)).display())
}

/// The history path a store key names.
pub fn from_key(key: &str) -> Option<PathBuf> {
    key.strip_prefix("git:").map(PathBuf::from)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A repository made with the git CLI, or `None` when there is no git.
    pub(crate) fn repo(name: &str) -> Option<PathBuf> {
        let d = std::env::temp_dir().join(format!("coxswain-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).ok()?;
        crate::tools::command("git").arg("-C").arg(&d).args(["init", "-q", "-b", "main"]).status().ok().filter(|s| s.success())?;
        Some(d)
    }

    pub(crate) fn commit_as(d: &Path, who: &str, when: u64, msg: &str) {
        let date = format!("{when} +0000");
        let ok = crate::tools::command("git")
            .arg("-C")
            .arg(d)
            .args(["-c", "commit.gpgsign=false", "-c", &format!("user.name={who}"), "-c", "user.email=t@t", "commit", "-q", "-a", "-m", msg])
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .status()
            .unwrap();
        assert!(ok.success());
    }

    pub(crate) fn add(d: &Path) {
        assert!(crate::tools::command("git").arg("-C").arg(d).args(["add", "-A"]).status().unwrap().success());
    }

    #[test]
    fn history_paths_split_and_build() {
        let d = std::env::temp_dir().join(format!("coxswain-history-split-{}", std::process::id()));
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src/main.rs"), "").unwrap();
        let p = path(&d.join("src/main.rs"), Some("0123456789abcdef0123"));
        assert_eq!(p, d.join("src/main.rs/@history/0123456789ab"));
        let at = split(&p.join("deep/x.rs")).unwrap();
        assert_eq!((at.target, at.base, at.commit.as_deref(), at.inner.as_str()), (d.join("src/main.rs"), d.join("src"), Some("0123456789ab"), "deep/x.rs"));
        let at = split(&path(&d.join("src"), None)).unwrap();
        assert_eq!((at.base, at.commit), (d.join("src"), None));
        assert!(split(&d.join("src/@history/not-a-commit")).is_none());
        assert!(split(&d.join("src/@history/0123456789ab/../../x")).is_none());
        assert!(split(&d.join("gone/@history")).is_none(), "the target must be there");
        assert!(split(&d.join("src")).is_none());
        // A real folder of that name is just a folder.
        std::fs::create_dir_all(d.join("src/@history")).unwrap();
        assert!(split(&d.join("src/@history")).is_none());
        assert!(is_history(&p) && !is_history(&d));
        assert_eq!(from_key(&key(&d, "abcdef0123456789")), Some(d.join("@history/abcdef012345")));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn history_lists_commits_trees_files_and_copies_out() {
        let Some(d) = repo("tree") else { return };
        std::fs::create_dir_all(d.join("src/deep")).unwrap();
        std::fs::write(d.join("src/main.rs"), "one\n").unwrap();
        std::fs::write(d.join("src/deep/a b.txt"), "deep\n").unwrap();
        std::fs::write(d.join("README"), "hi\n").unwrap();
        add(&d);
        commit_as(&d, "Ada", 1_700_000_000, "First: the start");
        std::fs::write(d.join("src/main.rs"), "two\n").unwrap();
        commit_as(&d, "Bob", 1_700_100_000, "Second/with a slash\n\nThe body.");
        std::fs::write(d.join("README"), "hello\n").unwrap();
        commit_as(&d, "Cy", 1_700_200_000, "Third");

        // The commits of a file, newest first, and `..` back to its folder.
        let file = d.join("src/main.rs");
        let list = crate::fs::list(&path(&file, None), true).unwrap();
        let names: Vec<_> = list.iter().map(|e| e.name.split_once(' ').map_or(e.name.as_str(), |n| n.1)).collect();
        assert_eq!(names, ["..", "Second∕with a slash", "First: the start"]);
        assert_eq!(list[0].path, d.join("src"));
        assert_eq!(list[2].modified, 1_700_000_000);
        // The folder at the first commit: the file's folder as it was then.
        let at_first = list[2].path.clone();
        let tree = crate::fs::list(&at_first, true).unwrap();
        let names: Vec<_> = tree.iter().map(|e| (e.name.as_str(), e.is_dir, e.size)).collect();
        assert_eq!(names, [("..", true, 0), ("deep", true, 0), ("main.rs", false, 4)]);
        assert_eq!(tree[0].path, path(&file, None));
        let deep = crate::fs::list(&at_first.join("deep"), true).unwrap();
        assert_eq!(deep[1].name, "a b.txt");
        assert!(crate::fs::list(&at_first.join("nothing"), true).is_err());

        // The file then, and what that commit changed in it.
        let at = split(&at_first.join("main.rs")).unwrap();
        assert_eq!(read(&at, 100).unwrap(), (b"one\n".to_vec(), false));
        assert_eq!(read(&at, 2).unwrap(), (b"on".to_vec(), true));
        let copy = peek(&at_first.join("main.rs")).unwrap();
        assert_eq!((copy.file_name().unwrap().to_str(), std::fs::read_to_string(&copy).unwrap().as_str()), (Some("main.rs"), "one\n"));
        assert!(peek(&at_first).is_err(), "a folder has no copy to look at");
        let second = split(&list[1].path.join("main.rs")).unwrap();
        assert!(diff(&second).unwrap().contains("-one\n+two"));

        // Copied out: a file, a folder, and the whole folder of a commit; never over a file.
        let out = d.join("out");
        std::fs::create_dir_all(&out).unwrap();
        assert_eq!(crate::fs::copy(&at_first.join("main.rs"), &out).unwrap(), out.join("main.rs"));
        assert_eq!(std::fs::read_to_string(out.join("main.rs")).unwrap(), "one\n");
        assert!(crate::fs::copy(&at_first.join("main.rs"), &out).is_err(), "it is there now");
        crate::fs::copy(&at_first.join("deep"), &out).unwrap();
        assert_eq!(std::fs::read_to_string(out.join("deep/a b.txt")).unwrap(), "deep\n");
        let whole = crate::fs::copy(&at_first, &out).unwrap();
        assert!(whole.join("deep/a b.txt").is_file() && whole.join("main.rs").is_file());

        // Read-only: nothing goes in, nothing is made or taken out there.
        assert!(crate::fs::copy(&d.join("README"), &at_first).is_err());
        assert!(crate::fs::mkdir(&at_first.join("new")).is_err());
        assert!(crate::fs::delete(&at_first.join("main.rs")).is_err());
        assert!(crate::fs::rename(&at_first.join("main.rs"), &out.join("x")).is_err());
        assert!(!d.join("src/main.rs/@history").exists() && !d.join("src/@history").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn history_last_commit_per_entry_one_walk_cached() {
        let Some(d) = repo("last") else { return };
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src/a.rs"), "a").unwrap();
        std::fs::write(d.join("README"), "r").unwrap();
        add(&d);
        commit_as(&d, "Ada", 1_700_000_000, "start");
        std::fs::write(d.join("src/a.rs"), "aa").unwrap();
        commit_as(&d, "Bob", 1_700_100_000, "change a");
        std::fs::write(d.join("untracked"), "u").unwrap();

        let l = last_changes(&d).unwrap();
        assert_eq!(l["src"].as_ref().map(|c| (c.author.as_str(), c.time)), Some(("Bob", 1_700_100_000)), "a folder takes its newest");
        assert_eq!(l["README"].as_ref().map(|c| c.subject.as_str()), Some("start"));
        assert!(!l.contains_key("untracked"), "git does not track it");
        assert!(Arc::ptr_eq(&l, &last_changes(&d).unwrap()), "the same HEAD: from the cache");
        let src = last_changes(&d.join("src")).unwrap();
        assert_eq!(src["a.rs"].as_ref().unwrap().author, "Bob");

        // At a commit: as of then.
        let commits = crate::fs::list(&path(&d, None), true).unwrap();
        let first = &commits[2].path;
        let then = last_changes(&first.join("src")).unwrap();
        assert_eq!(then["a.rs"].as_ref().unwrap().author, "Ada");
        // The list of commits: each its own.
        let own = last_changes(&path(&d, None)).unwrap();
        assert_eq!(own[&commits[1].name].as_ref().unwrap().author, "Bob");

        // HEAD moves: walked again.
        std::fs::write(d.join("README"), "rr").unwrap();
        commit_as(&d, "Cy", 1_700_200_000, "readme");
        assert_eq!(last_changes(&d).unwrap()["README"].as_ref().unwrap().author, "Cy");

        let mut entries = crate::fs::list(&d, true).unwrap();
        sort_by_last(&mut entries, &last_changes(&d).unwrap(), false);
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).filter(|n| *n != ".git").collect();
        assert_eq!(names[..4], ["..", "src", "README", "untracked"]);
        assert!(last_changes(&std::env::temp_dir()).is_none_or(|l| l.is_empty()));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn history_for_the_search_store() {
        let Some(d) = repo("search") else { return };
        std::fs::write(d.join("engine.rs"), "x").unwrap();
        add(&d);
        commit_as(&d, "Ada", 1_700_000_000, "Tune the rocket engine\n\nThrust was low.");
        let (one, _) = head(&d, None).unwrap();
        std::fs::write(d.join("engine.rs"), "y").unwrap();
        commit_as(&d, "Bob", 1_700_100_000, "Fix the fuel valve");
        let (two, grew) = head(&d, Some(&one)).unwrap();
        assert!(grew);
        let all = for_search(&d, None, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].2, format!("commit {} · Ada · 2023-11-14\nTune the rocket engine\n\nThrust was low.\nengine.rs", &one[..7]));
        let new = for_search(&d, Some(&one), 10).unwrap();
        assert_eq!((new.len(), new[0].0.as_str()), (1, two.as_str()));
        assert!(for_search(&d, Some(&two), 10).unwrap().is_empty());
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A repository's config names a "gpg" to run on every commit shown: it never runs.
    #[cfg(unix)]
    #[test]
    fn history_never_runs_programs_from_the_repository_config() {
        let Some(d) = repo("config") else { return };
        std::fs::write(d.join("a.txt"), "a\n").unwrap();
        add(&d);
        commit_as(&d, "Ada", 1_700_000_000, "one");
        let (fake, mark) = (d.join("fake-gpg"), d.join("ran"));
        std::fs::write(&fake, format!("#!/bin/sh\ntouch '{}'\nexit 0\n", mark.display())).unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        std::fs::write(d.join(".git/config"), format!("[log]\n\tshowSignature = true\n[gpg]\n\tprogram = {}\n[core]\n\tfsmonitor = {}\n", fake.display(), fake.display())).unwrap();
        let at = split(&path(&d.join("a.txt"), None)).unwrap();
        assert_eq!(list(&path(&d.join("a.txt"), None), &at).unwrap().len(), 2);
        let commit = log(&d, "HEAD", ".", 10).unwrap().0.remove(0);
        assert!(show(&d, &commit.hash).is_ok() && diff(&split(&path(&d.join("a.txt"), Some(&commit.hash)).join("a.txt")).unwrap()).is_ok());
        assert!(last_changes(&d).is_some() && for_search(&d, None, 10).is_ok());
        assert!(!mark.exists(), "the repository's gpg.program ran");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A made-up tree with a link `a` to a folder outside and a folder `a` of the same name,
    /// and a `.git` folder: copied out, nothing is written through the link, and no `.git`.
    #[cfg(unix)]
    #[test]
    fn history_copy_out_never_writes_through_a_link() {
        let Some(d) = repo("link") else { return };
        let outside = d.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let git_in = |args: &[&str], input: &str| {
            let mut c = crate::tools::command("git");
            c.arg("-C").arg(&d).args(args).stdin(Stdio::piped()).stdout(Stdio::piped());
            let mut child = c.spawn().unwrap();
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
            String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap().trim().to_string()
        };
        let blob = git_in(&["hash-object", "-w", "--stdin"], "evil\n");
        let link = git_in(&["hash-object", "-w", "--stdin"], &outside.to_string_lossy());
        let sub = git_in(&["mktree"], &format!("100644 blob {blob}\tevil\n"));
        let tree = git_in(&["mktree"], &format!("120000 blob {link}\ta\n040000 tree {sub}\ta\n040000 tree {sub}\t.git\n100644 blob {blob}\tok\n"));
        let commit = git_in(&["-c", "user.name=t", "-c", "user.email=t@t", "commit-tree", &tree, "-m", "x"], "");
        let dest = d.join("dest");
        std::fs::create_dir(&dest).unwrap();
        let at = At { target: d.clone(), base: d.clone(), commit: Some(commit), inner: String::new() };
        let _ = copy_out(&at, &dest);
        assert!(!outside.join("evil").exists(), "written through the link");
        assert!(std::fs::read_dir(&dest).unwrap().flatten().all(|e| !e.path().join(".git").exists()));
        std::fs::remove_dir_all(d).unwrap();
    }

}

/// Timings on a big repository: `COXSWAIN_BIG_REPO=<path> cargo test -p coxswain-core --release history_timings -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn history_timings() {
    let Ok(root) = std::env::var("COXSWAIN_BIG_REPO") else { return };
    let root = PathBuf::from(root);
    let time = |what: &str, f: &dyn Fn() -> usize| {
        let t = std::time::Instant::now();
        let n = f();
        println!("{what}: {n} in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    };
    time("last commit, top folder", &|| last_changes(&root).map_or(0, |l| l.values().filter(|v| v.is_some()).count()));
    time("last commit, top folder again (cache)", &|| last_changes(&root).map_or(0, |l| l.len()));
    let sub = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).find(|p| p.is_dir() && !p.ends_with(".git")).unwrap();
    time("last commit, a subfolder", &|| last_changes(&sub).map_or(0, |l| l.values().filter(|v| v.is_some()).count()));
    time("commits of the top folder", &|| crate::fs::list(&path(&root, None), true).unwrap().len());
    time("commits of a rarely changed file", &|| crate::fs::list(&path(&root.join("LICENSE"), None), true).unwrap().len());
    let commits = crate::fs::list(&path(&root, None), true).unwrap();
    time("the top folder at a commit", &|| crate::fs::list(&commits[1000].path, true).unwrap().len());
    time("last commit at a commit", &|| last_changes(&commits[1000].path).map_or(0, |l| l.len()));
    time("for the search store, 2000", &|| for_search(&root, None, 2000).unwrap().len());
}

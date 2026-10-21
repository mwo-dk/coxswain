//! Version control systems behind one seam: what the git line, the history and the list of
//! branches ask of a repository. Git; Jujutsu through its `jj` CLI, only read
//! (`--ignore-working-copy`: jj takes no snapshot and writes no operation; where a folder has
//! both `.jj` and `.git`, Jujutsu is asked unless `[git] prefer_git` says otherwise); and
//! Mercurial through its `hg` CLI, never with the repository's own `.hg/hgrc` (its hooks and
//! extensions would run).

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::branches::Branch;
use crate::git::{FileStatus, Kind, Status};
use crate::history::{At, Commit};

/// What a repository is asked, whatever keeps it.
pub trait Vcs: Sync {
    /// Its program's name, as the line shows it: `git`, `jj`.
    fn name(&self) -> &'static str;
    /// The working copy's status. `None` outside a repository or without the program.
    fn status(&self, dir: &Path) -> Option<Status>;
    /// The commits that touched `spec` (`.` or a name in `base`), newest first: at most `max`,
    /// and whether there were more.
    fn log(&self, base: &Path, spec: &str, max: usize) -> io::Result<(Vec<Commit>, bool)>;
    /// The branches (Jujutsu's bookmarks), the local ones first.
    fn branches(&self, dir: &Path) -> io::Result<Vec<Branch>>;
    /// Switch to the branch an entry of the list names. Refused where Coxswain only reads.
    fn switch(&self, _dir: &Path, _entry: &str) -> io::Result<String> {
        Err(self.read_only())
    }
    /// A new branch, switched to. Refused where Coxswain only reads.
    fn create(&self, _dir: &Path, _name: &str, _from: Option<&str>) -> io::Result<String> {
        Err(self.read_only())
    }
    fn read_only(&self) -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, crate::t!("vcs.jj_read_only"))
    }
    /// One commit, by its id or name.
    fn show(&self, base: &Path, rev: &str) -> io::Result<Commit> {
        crate::history::git_show(base, rev)
    }
    /// The entries of the folder `inner` (below `base`, `/`-separated) at `rev`: name, kind
    /// (`tree`, `blob`, `exec`, `link`, `commit`) and size.
    fn tree(&self, base: &Path, rev: &str, inner: &str) -> io::Result<Vec<(String, &'static str, u64)>> {
        crate::history::git_tree(base, rev, inner)
    }
    /// The first `max` bytes of the file `inner` at `rev`, and whether there was more.
    fn read(&self, base: &Path, rev: &str, inner: &str, max: usize) -> io::Result<(Vec<u8>, bool)> {
        crate::history::git_read(base, rev, inner, max)
    }
    /// What `rev` changed in `inner` (the folder itself when empty), as a diff.
    fn diff(&self, base: &Path, rev: &str, inner: &str) -> io::Result<Vec<u8>> {
        crate::history::git_diff(base, rev, inner)
    }
    /// The file or folder `at` names, as it was at `rev`, written to `to` (not there yet).
    fn export(&self, at: &At, rev: &str, to: &Path) -> io::Result<()> {
        crate::history::git_write_tree(at, rev, to)
    }
}

/// `[git] prefer_git`: where a folder has both, git is asked, not Jujutsu.
static PREFER_GIT: AtomicBool = AtomicBool::new(false);
/// A Jujutsu repository was found and `jj` is not installed: a notice says what to install.
pub static JJ_MISSING: AtomicBool = AtomicBool::new(false);
/// A Mercurial repository was found and `hg` is not installed.
pub static HG_MISSING: AtomicBool = AtomicBool::new(false);

/// Follow the settings: every config that is loaded passes here.
pub fn follow(cfg: &crate::config::GitConfig) {
    PREFER_GIT.store(cfg.prefer_git, Ordering::Relaxed);
}

/// The system that keeps the repository `dir` is in: Jujutsu when the nearest folder above
/// with `.jj` or `.git` has `.jj` (and the settings do not prefer git there), else git, which
/// finds its repository itself.
pub fn of(dir: &Path) -> &'static dyn Vcs {
    if jj_root(dir).is_some() {
        &Jj
    } else if hg_root(dir).is_some() {
        &Hg
    } else {
        &Git
    }
}

/// The nearest folder at or above `dir` that is a repository's top.
fn top(dir: &Path) -> Option<&Path> {
    dir.ancestors()
        .find(|a| a.join(".jj").is_dir() || a.join(".git").exists() || a.join(".hg").is_dir())
}

/// The root of the Mercurial repository `dir` is in (one with no `.git` or `.jj` of its own).
fn hg_root(dir: &Path) -> Option<PathBuf> {
    let root = top(dir)?;
    (root.join(".hg").is_dir() && !root.join(".git").exists() && !root.join(".jj").is_dir())
        .then(|| root.to_path_buf())
}

/// The root of the Jujutsu repository `dir` is in, when it is one Jujutsu is asked about.
fn jj_root(dir: &Path) -> Option<PathBuf> {
    let root = top(dir)?;
    let both = root.join(".git").exists();
    (root.join(".jj").is_dir() && !(both && PREFER_GIT.load(Ordering::Relaxed)))
        .then(|| root.to_path_buf())
}

/// Where a Jujutsu repository without a `.git` of its own keeps its commits: a git store
/// below `.jj` (`git_target` names it). git reads the commits from there, for the history's
/// folders; `None` where git finds its own.
pub(crate) fn jj_store(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let root = jj_root(dir).filter(|r| !r.join(".git").exists())?;
    let dot = root.join(".jj");
    // A second workspace's `repo` is a file naming the first one's.
    let repo = match std::fs::read_to_string(dot.join("repo")) {
        Ok(to) => dot.join(to.trim()),
        Err(_) => dot.join("repo"),
    };
    let store = repo.join("store");
    let target = std::fs::read_to_string(store.join("git_target")).ok()?;
    Some((store.join(target.trim()), root))
}

/// git pointed at the store `jj_store` finds for `dir`, when it has one.
pub(crate) fn use_store(c: &mut Command, dir: &Path) {
    if let Some((store, root)) = jj_store(dir) {
        c.env("GIT_DIR", store).env("GIT_WORK_TREE", root);
    }
}

struct Git;

impl Vcs for Git {
    fn name(&self) -> &'static str {
        "git"
    }
    fn status(&self, dir: &Path) -> Option<Status> {
        Status::read_git(dir)
    }
    fn log(&self, base: &Path, spec: &str, max: usize) -> io::Result<(Vec<Commit>, bool)> {
        // A history of this branch, as the list shows it (`history::graph_log`).
        let at = At {
            target: if spec == "." { base.to_path_buf() } else { base.join(spec) },
            base: base.to_path_buf(),
            commit: None,
            inner: String::new(),
            view: crate::history::View::History,
            scope: crate::history::Scope::Branch,
        };
        crate::history::graph_log(&at, max)
    }
    fn branches(&self, dir: &Path) -> io::Result<Vec<Branch>> {
        crate::branches::git_branches(dir)
    }
    fn switch(&self, dir: &Path, entry: &str) -> io::Result<String> {
        crate::branches::git_switch(dir, entry)
    }
    fn create(&self, dir: &Path, name: &str, from: Option<&str>) -> io::Result<String> {
        crate::branches::git_create(dir, name, from)
    }
}

struct Jj;

/// `jj` in `dir`, reading only: no snapshot of the working copy (which would write an
/// operation, and import git's refs), no pager, no colour.
fn jj(dir: &Path) -> Command {
    let mut c = crate::tools::command("jj");
    c.current_dir(dir)
        .args(["--ignore-working-copy", "--no-pager", "--color=never"])
        .stdin(Stdio::null());
    c
}

/// Whether `jj` runs here; when not, the notice is due and git is asked instead.
fn jj_here() -> bool {
    let here = crate::user_menu::installed("jj");
    if !here {
        JJ_MISSING.store(true, Ordering::Relaxed);
    }
    here
}

/// A Jujutsu string literal: `"…"` with `\` and `"` escaped.
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

const LOG: &str = r#"commit_id ++ "\x1f" ++ author.timestamp().format("%s") ++ "\x1f" ++ author.name() ++ "\x1f" ++ description.first_line() ++ "\x00""#;

/// The working-copy commit and the nearest commits below it with bookmarks: whether it is the
/// working copy, the change id, the local bookmarks, whether it has conflicts.
const HEAD: &str = r#"current_working_copy ++ "\x1f" ++ change_id.shortest(8) ++ "\x1f" ++ local_bookmarks.map(|b| b.name()).join(",") ++ "\x1f" ++ conflict ++ "\n""#;

const BOOKMARKS: &str = r#"name ++ "\x1f" ++ remote ++ "\x1f" ++ if(normal_target, normal_target.commit_id() ++ "\x1f" ++ normal_target.author().timestamp().format("%s") ++ "\x1f" ++ normal_target.author().name() ++ "\x1f" ++ normal_target.contained_in("@ | @-") ++ "\x1f" ++ normal_target.description().first_line()) ++ "\n""#;

impl Vcs for Jj {
    fn name(&self) -> &'static str {
        "jj"
    }

    fn status(&self, dir: &Path) -> Option<Status> {
        let root = jj_root(dir)?;
        if !jj_here() {
            return Status::read_git(dir);
        }
        // Next to git, git's own status is live (jj's is as of the last jj command), and git
        // sees the parent of the working-copy commit as HEAD: the same changes.
        let mut st = if root.join(".git").exists() {
            // From the root: `dir` may be a history's or the bookmarks' folder, not on disk.
            Status::read_git(&root).unwrap_or_default()
        } else {
            let out = crate::history::run({
                let mut c = jj(&root);
                c.args(["diff", "--summary"]);
                c
            })
            .ok()?;
            parse_summary(&root, &String::from_utf8_lossy(&out))
        };
        st.root = root.clone();
        let heads = crate::history::run({
            let mut c = jj(&root);
            c.args(["log", "--no-graph", "-r", "@ | heads(::@ & bookmarks())", "-T", HEAD]);
            c
        })
        .ok()?;
        let s = &mut st.summary;
        (s.detached, s.upstream, s.ahead, s.behind, s.worktree) = (None, None, 0, 0, None);
        s.system = "jj";
        s.branch.clear();
        s.operation = None;
        for line in String::from_utf8_lossy(&heads).lines() {
            let f: Vec<&str> = line.split('\x1f').collect();
            let [current, change, bookmarks, conflict] = f[..] else { continue };
            if current == "true" {
                s.change = Some(change.to_string());
                if conflict == "true" {
                    s.operation = Some("conflict");
                }
            }
            if s.branch.is_empty() {
                s.branch = bookmarks.to_string();
            }
        }
        Some(st)
    }

    fn log(&self, base: &Path, spec: &str, max: usize) -> io::Result<(Vec<Commit>, bool)> {
        if !jj_here() {
            return Git.log(base, spec, max);
        }
        let mut c = jj(base);
        c.args([
            "log",
            "--no-graph",
            "-n",
            &(max + 1).to_string(),
            "-r",
            &format!("::@ & files({})", quoted(spec)),
            "-T",
            LOG,
        ]);
        let mut out = vec![];
        let cut = crate::history::records(c, 0, |rec| {
            out.extend(commit(rec));
            true
        })?;
        let more = cut || out.len() > max;
        out.truncate(max);
        Ok((out, more))
    }

    fn branches(&self, dir: &Path) -> io::Result<Vec<Branch>> {
        if !jj_here() {
            return Git.branches(dir);
        }
        let mut c = jj(dir);
        c.args(["bookmark", "list", "--all-remotes", "-T", BOOKMARKS]);
        Ok(parse_bookmarks(&String::from_utf8_lossy(&crate::history::run(c)?)))
    }
}

fn commit(rec: &[u8]) -> Option<Commit> {
    let s = String::from_utf8_lossy(rec);
    let f: Vec<&str> = s.trim_start_matches('\n').splitn(4, '\x1f').collect();
    let [hash, time, author, subject] = f[..] else { return None };
    Some(Commit {
        hash: hash.to_string(),
        time: time.parse().ok()?,
        author: author.to_string(),
        subject: subject.to_string(),
        ..Commit::default()
    })
}

/// The lines of `jj diff --summary` (`M path`, `A path`, `D path`, `R dir/{old => new}`,
/// paths from `root`) as a status.
pub fn parse_summary(root: &Path, out: &str) -> Status {
    let mut st = Status {
        root: root.to_path_buf(),
        ..Default::default()
    };
    for line in out.lines() {
        let Some((tag, path)) = line.split_once(' ') else { continue };
        let (kind, path) = match tag {
            "M" => (Kind::Modified, path.to_string()),
            "A" => (Kind::Added, path.to_string()),
            "D" => (Kind::Deleted, path.to_string()),
            // `dir/{old => new}/rest`, or `{old => new}` for the whole path: the new one.
            "R" | "C" => (Kind::Renamed, renamed_to(path)),
            _ => continue,
        };
        let s = &mut st.summary;
        match kind {
            Kind::Modified => s.modified += 1,
            Kind::Deleted => s.deleted += 1,
            _ => s.staged += 1,
        }
        let abs = root.join(&path);
        st.files.insert(abs.clone(), FileStatus { kind, staged: false });
        for dir in abs
            .ancestors()
            .skip(1)
            .take_while(|d| d.starts_with(root) && *d != root)
        {
            let e = st
                .files
                .entry(dir.to_path_buf())
                .or_insert(FileStatus { kind, staged: false });
            e.kind = e.kind.max(kind);
        }
    }
    st
}

fn renamed_to(path: &str) -> String {
    match (path.find('{'), path.find(" => "), path.find('}')) {
        (Some(a), Some(m), Some(b)) if a < m && m < b => {
            let joined = format!("{}{}{}", &path[..a], &path[m + 4..b], &path[b + 1..]);
            // `{a => }` for a name moved up a folder leaves `//`.
            joined.replace("//", "/").trim_start_matches('/').to_string()
        }
        _ => path.to_string(),
    }
}

/// The lines of `jj bookmark list --all-remotes -T BOOKMARKS`: the local bookmarks, then the
/// remote ones as `name@remote` (git's own refs, `@git`, left out, and a bookmark in conflict,
/// which points at no one commit).
pub fn parse_bookmarks(out: &str) -> Vec<Branch> {
    let mut all: Vec<Branch> = out
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(7, '\x1f').collect();
            let [name, remote, oid, time, author, current, subject] = f[..] else {
                return None;
            };
            if remote == "git" {
                return None;
            }
            Some(Branch {
                name: if remote.is_empty() {
                    name.to_string()
                } else {
                    format!("{name}@{remote}")
                },
                remote: !remote.is_empty(),
                current: current == "true" && remote.is_empty(),
                oid: oid.to_string(),
                time: time.parse().unwrap_or(0),
                author: author.to_string(),
                subject: subject.to_string(),
                ahead: 0,
                behind: 0,
            })
        })
        .collect();
    all.sort_by_key(|b| b.remote);
    all
}

// ---------------------------------------------------------------- Mercurial

struct Hg;

/// `hg` in `dir`: plain output (`HGPLAIN`: no aliases, colours, pager or localised words) and
/// never the repository's own `.hg/hgrc` (`HGRCSKIPREPO`), whose hooks and extensions would
/// run; the user's and the system's config still hold.
fn hg(dir: &Path) -> Command {
    let mut c = crate::tools::command("hg");
    c.current_dir(dir)
        .env("HGPLAIN", "1")
        .env("HGRCSKIPREPO", "1")
        .env("HGENCODING", "utf-8")
        .arg("--noninteractive")
        .stdin(Stdio::null());
    c
}

/// Whether `hg` runs here and is new enough to leave a repository's own config unread
/// (`HGRCSKIPREPO`, Mercurial 5.4): an older one is as good as missing.
fn hg_here() -> bool {
    static NEW: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    #[cfg(test)]
    if NO_HG.with(|n| n.get()) {
        return false;
    }
    let here = crate::user_menu::installed("hg")
        && *NEW.get_or_init(|| {
            let out = crate::tools::command("hg")
                .args(["--version", "--quiet"])
                .current_dir(std::env::temp_dir())
                .env("HGPLAIN", "1")
                .stdin(Stdio::null())
                .output();
            let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
            hg_new_enough(&text)
        });
    if !here {
        HG_MISSING.store(true, Ordering::Relaxed);
    }
    here
}

#[cfg(test)]
thread_local! {
    /// A test's way to have no hg.
    static NO_HG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// hg is here, or the notice's reason why not: everything but the line needs it, and nothing
/// is ever asked of git instead (git's branch names must never reach `hg update`).
fn need_hg() -> io::Result<()> {
    if hg_here() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, crate::t!("notice.hg_missing")))
    }
}

/// `hg --version --quiet` (`Mercurial Distributed SCM (version 6.7.2)`) names 5.4 or later.
fn hg_new_enough(text: &str) -> bool {
    let v = text.split("version ").nth(1).unwrap_or_default();
    let mut n = v.split(|c: char| !c.is_ascii_digit()).filter_map(|x| x.parse::<u32>().ok());
    (n.next().unwrap_or(0), n.next().unwrap_or(0)) >= (5, 4)
}

/// A commit by its id or the start of it: `id()`, so a prefix of digits is never taken for a
/// local revision number.
fn hg_rev(rev: &str) -> String {
    if rev.bytes().all(|b| b.is_ascii_hexdigit()) {
        format!("id({rev})")
    } else {
        rev.to_string()
    }
}

/// A Mercurial pattern for exactly the path `rel` (from the root): `path:` takes no globs.
fn hg_path(rel: &str) -> String {
    format!("path:{}", if rel.is_empty() { "." } else { rel })
}

/// `inner` (below `base`) from the repository's root, `/`-separated.
fn from_root(root: &Path, base: &Path, inner: &str) -> String {
    let rel = base
        .strip_prefix(root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    [rel.as_str(), inner]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

const HG_LOG: &str = r"{node}\x1f{date|hgdate}\x1f{author|person}\x1f{desc|firstline}\0";

fn hg_commit(rec: &[u8]) -> Option<Commit> {
    let s = String::from_utf8_lossy(rec);
    let f: Vec<&str> = s.splitn(4, '\x1f').collect();
    let [hash, date, author, subject] = f[..] else { return None };
    Some(Commit {
        hash: hash.to_string(),
        time: date.split(' ').next()?.parse().ok()?,
        author: author.to_string(),
        subject: subject.to_string(),
    })
}

/// The lines of `hg status` (`M`, `A`, `R`, `!`, `?`, root-relative paths) as a status.
pub fn parse_hg_status(root: &Path, out: &str) -> Status {
    let mut st = Status {
        root: root.to_path_buf(),
        ..Default::default()
    };
    for line in out.lines() {
        let Some((tag, path)) = line.split_once(' ') else { continue };
        let kind = match tag {
            "M" => Kind::Modified,
            "A" => Kind::Added,
            "R" | "!" => Kind::Deleted,
            "?" => Kind::Untracked,
            _ => continue,
        };
        mark(&mut st, root, path, kind, tag == "A" || tag == "R");
    }
    st
}

/// One file's status, counted, and its folders up to the root marked.
fn mark(st: &mut Status, root: &Path, path: &str, kind: Kind, staged: bool) {
    let s = &mut st.summary;
    match kind {
        Kind::Modified => s.modified += 1,
        Kind::Deleted => s.deleted += 1,
        Kind::Untracked => s.untracked += 1,
        Kind::Conflict => s.conflicts += 1,
        _ => s.staged += 1,
    }
    let abs = root.join(path);
    st.files.insert(abs.clone(), FileStatus { kind, staged });
    for dir in abs
        .ancestors()
        .skip(1)
        .take_while(|d| d.starts_with(root) && *d != root)
    {
        let e = st
            .files
            .entry(dir.to_path_buf())
            .or_insert(FileStatus { kind, staged });
        e.kind = e.kind.max(kind);
        e.staged |= staged;
    }
}

/// What Mercurial says is under way, by the files it keeps while it is.
fn hg_operation(dot: &Path) -> Option<&'static str> {
    [
        ("rebasestate", "rebase"),
        ("merge/state2", "merge"),
        ("graftstate", "cherry_pick"),
        ("bisect.state", "bisect"),
    ]
    .into_iter()
    .find(|(f, _)| dot.join(f).exists())
    .map(|(_, op)| op)
}

/// Whether the repository's own config asks for what changes files as they are written
/// (`[extensions]` such as largefiles, `[encode]`, `[decode]`): Coxswain never reads that
/// config, so it leaves writing files to hg itself.
fn hg_writes_files_safely(root: &Path) -> io::Result<()> {
    let cfg = std::fs::read_to_string(root.join(".hg/hgrc")).unwrap_or_default();
    let requires = std::fs::read_to_string(root.join(".hg/requires")).unwrap_or_default();
    let own = cfg.lines().any(|l| l.trim_start().starts_with("%include"))
        || requires.lines().any(|l| matches!(l.trim(), "largefiles" | "lfs"))
        || root.join(".hg/sharedpath").exists()
        || cfg.lines().any(|l| {
        matches!(
            l.trim().trim_start_matches('[').trim_end_matches(']'),
            "extensions" | "encode" | "decode"
        ) && l.trim().starts_with('[')
    });
    if own {
        Err(io::Error::other(crate::t!("vcs.hg_has_filters")))
    } else {
        Ok(())
    }
}

impl Vcs for Hg {
    fn name(&self) -> &'static str {
        "hg"
    }

    fn status(&self, dir: &Path) -> Option<Status> {
        let root = hg_root(dir)?;
        if !hg_here() {
            // A `.hg` inside a git work tree (a fixture, a vendored copy): git's line, as before.
            return Status::read_git(dir);
        }
        let out = crate::history::run({
            let mut c = hg(&root);
            c.args(["status", "-mard", "-u"]);
            c
        })
        .ok()?;
        let mut st = parse_hg_status(&root, &String::from_utf8_lossy(&out));
        let dot = root.join(".hg");
        let s = &mut st.summary;
        s.system = "hg";
        // The working folder's branch (`default` when the file is missing), or its bookmark.
        let branch = std::fs::read_to_string(dot.join("branch"))
            .map(|b| b.trim().to_string())
            .unwrap_or_default();
        let bookmark = std::fs::read_to_string(dot.join("bookmarks.current"))
            .map(|b| b.trim().to_string())
            .unwrap_or_default();
        s.branch = match (bookmark.is_empty(), branch.is_empty()) {
            (false, _) => bookmark,
            (true, false) => branch,
            (true, true) => "default".into(),
        };
        s.operation = hg_operation(&dot);
        if dot.join("merge/state2").exists() {
            let unresolved = crate::history::run({
                let mut c = hg(&root);
                c.args(["resolve", "--list"]);
                c
            })
            .unwrap_or_default();
            for line in String::from_utf8_lossy(&unresolved).lines() {
                if let Some(path) = line.strip_prefix("U ") {
                    // `hg status` said `M` for it: a conflict only.
                    if st.files.get(&root.join(path)).is_some_and(|f| f.kind == Kind::Modified) {
                        st.summary.modified = st.summary.modified.saturating_sub(1);
                    }
                    mark(&mut st, &root, path, Kind::Conflict, false);
                }
            }
        }
        Some(st)
    }

    fn log(&self, base: &Path, spec: &str, max: usize) -> io::Result<(Vec<Commit>, bool)> {
        need_hg()?;
        let root = hg_root(base).ok_or_else(|| io::Error::other("no repository"))?;
        let rel = from_root(&root, base, if spec == "." { "" } else { spec });
        let mut c = hg(&root);
        c.args(["log", "-T", HG_LOG, "-l", &(max + 1).to_string()]);
        // A file's history goes on past its renames; a folder's is its path as it is.
        if base.join(spec).is_file() {
            c.args(["--follow", "--", &hg_path(&rel)]);
        } else {
            c.args(["-r", "reverse(::.)", "--", &hg_path(&rel)]);
        }
        let mut out = vec![];
        let cut = crate::history::records(c, 0, |rec| {
            out.extend(hg_commit(rec));
            true
        })?;
        let more = cut || out.len() > max;
        out.truncate(max);
        Ok((out, more))
    }

    fn branches(&self, dir: &Path) -> io::Result<Vec<Branch>> {
        need_hg()?;
        let root = hg_root(dir).unwrap_or_else(|| dir.to_path_buf());
        let tail = r"\x1f{node}\x1f{date|hgdate}\x1f{author|person}\x1f{desc|firstline}\n";
        let marks = crate::history::run({
            let mut c = hg(&root);
            c.args(["bookmarks", "-T", &format!("{{bookmark}}\\x1f{{active}}{tail}")]);
            c
        })?;
        let named = crate::history::run({
            let mut c = hg(&root);
            c.args(["branches", "-T", &format!("{{branch}}\\x1f{{active}}{tail}")]);
            c
        })?;
        let here = std::fs::read_to_string(root.join(".hg/branch"))
            .map(|b| b.trim().to_string())
            .unwrap_or_else(|_| "default".into());
        Ok(parse_hg_branches(
            &String::from_utf8_lossy(&marks),
            &String::from_utf8_lossy(&named),
            &here,
        ))
    }

    fn switch(&self, dir: &Path, entry: &str) -> io::Result<String> {
        need_hg()?;
        let root = hg_root(dir).ok_or_else(|| io::Error::other("no repository"))?;
        let name = crate::branches::name_of(entry);
        let b = self
            .branches(&root)?
            .into_iter()
            .find(|b| b.name == name)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, crate::t!("branches.gone", "name" => name))
            })?;
        hg_writes_files_safely(&root)?;
        // `--check`: refused while anything is changed, never merged into or thrown away. By
        // its name: a bookmark becomes the active one, a named branch goes to its head.
        let mut c = hg(&root);
        // Never a subrepository fetched from wherever `.hgsub` names.
        c.args(["--config", "subrepos.allowed=false", "update", "--check", "-r", &b.name]);
        said(c)?;
        Ok(crate::t!("vcs.hg_switched", "name" => b.name))
    }

    fn create(&self, dir: &Path, name: &str, from: Option<&str>) -> io::Result<String> {
        need_hg()?;
        let root = hg_root(dir).ok_or_else(|| io::Error::other("no repository"))?;
        let name = name.trim();
        if name.is_empty()
            || name.starts_with('-')
            || name.chars().any(|c| c.is_whitespace() || c.is_control() || c == ':')
            || name.chars().all(|c| c.is_ascii_digit())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                crate::t!("vcs.hg_bad_name", "name" => name),
            ));
        }
        let in_list = crate::history::split(dir).is_some_and(|at| at.view == crate::history::View::Branches && at.commit.is_none());
        if let Some(from) = from.filter(|e| *e != ".." && in_list) {
            self.switch(dir, from)?;
        }
        let mut c = hg(&root);
        c.args(["bookmark", "--", name]);
        said(c)?;
        Ok(crate::t!("vcs.hg_created", "name" => name))
    }

    fn show(&self, base: &Path, rev: &str) -> io::Result<Commit> {
        need_hg()?;
        let mut c = hg(base);
        c.args(["log", "-r", &hg_rev(rev), "-T", HG_LOG]);
        hg_commit(crate::history::run(c)?.split(|&b| b == 0).next().unwrap_or_default())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no commit {rev}")))
    }

    fn tree(&self, base: &Path, rev: &str, inner: &str) -> io::Result<Vec<(String, &'static str, u64)>> {
        need_hg()?;
        let root = hg_root(base).ok_or_else(|| io::Error::other("no repository"))?;
        let rel = from_root(&root, base, inner);
        let mut c = hg(&root);
        c.args(["files", "-r", &hg_rev(rev), "-T", r"{size}\x1f{flags}\x1f{path}\0", "--", &hg_path(&rel)]);
        let prefix = if rel.is_empty() { String::new() } else { format!("{rel}/") };
        let mut out: Vec<(String, &'static str, u64)> = vec![];
        let mut dirs = std::collections::BTreeSet::new();
        for rec in crate::history::run(c).unwrap_or_default().split(|&b| b == 0) {
            let s = String::from_utf8_lossy(rec);
            let f: Vec<&str> = s.splitn(3, '\x1f').collect();
            let [size, flags, path] = f[..] else { continue };
            let Some(rest) = path.strip_prefix(&prefix) else { continue };
            match rest.split_once('/') {
                Some((d, _)) => {
                    dirs.insert(d.to_string());
                }
                None => out.push((
                    rest.to_string(),
                    if flags.contains('l') {
                        "link"
                    } else if flags.contains('x') {
                        "exec"
                    } else {
                        "blob"
                    },
                    size.parse().unwrap_or(0),
                )),
            }
        }
        out.extend(dirs.into_iter().map(|d| (d, "tree", 0)));
        Ok(out)
    }

    fn read(&self, base: &Path, rev: &str, inner: &str, max: usize) -> io::Result<(Vec<u8>, bool)> {
        need_hg()?;
        let root = hg_root(base).ok_or_else(|| io::Error::other("no repository"))?;
        let mut c = hg(&root);
        c.args(["cat", "-r", &hg_rev(rev), "--", &hg_path(&from_root(&root, base, inner))]);
        crate::history::read_out(c, max)
    }

    fn diff(&self, base: &Path, rev: &str, inner: &str) -> io::Result<Vec<u8>> {
        need_hg()?;
        let root = hg_root(base).ok_or_else(|| io::Error::other("no repository"))?;
        let mut c = hg(&root);
        c.args(["diff", "--git", "-c", &hg_rev(rev), "--", &hg_path(&from_root(&root, base, inner))]);
        crate::history::run(c)
    }

    fn export(&self, at: &At, rev: &str, to: &Path) -> io::Result<()> {
        need_hg()?;
        let root = hg_root(&at.base).ok_or_else(|| io::Error::other("no repository"))?;
        let rel = from_root(&root, &at.base, &at.inner);
        let rev = hg_rev(rev);
        let mut c = hg(&root);
        c.args(["files", "-r", &rev, "-T", r"{flags}\x1f{path}\0", "--", &hg_path(&rel)]);
        let listed = crate::history::run(c)?;
        let mut files = vec![];
        for rec in listed.split(|&b| b == 0) {
            let rec = String::from_utf8_lossy(rec);
            let Some((flags, path)) = rec.split_once('\x1f') else { continue };
            let rest = if path == rel {
                ""
            } else if rel.is_empty() {
                path
            } else {
                match path.strip_prefix(&format!("{rel}/")) {
                    Some(r) => r,
                    None => continue,
                }
            };
            // Never outside `to`, and never a repository made there, whatever the commit holds
            // (as `git_write_tree` refuses them).
            let normal = Path::new(rest).components().all(|c| matches!(c, std::path::Component::Normal(_)));
            let repo = |s: &str| {
                let s = s.trim_end_matches(['.', ' ']).to_ascii_lowercase();
                matches!(s.as_str(), ".git" | ".hg" | ".jj" | "git~1")
            };
            if !normal || rest.contains('\\') || rest.split('/').any(|s| matches!(s, "." | "..") || repo(s)) {
                continue;
            }
            files.push((flags.to_string(), path.to_string(), if rest.is_empty() { to.to_path_buf() } else { to.join(rest) }));
        }
        if files.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                crate::t!("history.not_in_commit", "path" => if at.inner.is_empty() { "." } else { &at.inner }, "rev" => &rev),
            ));
        }
        let when = crate::fs::epoch(self.show(&at.base, &rev)?.time);
        let mut links = vec![];
        for (flags, path, dst) in &files {
            if let Some(dir) = dst.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut c = hg(&root);
            c.args(["cat", "-r", &rev, "--", &hg_path(path)]);
            let bytes = crate::history::run(c)?;
            if flags.contains('l') {
                // Links last: a file listed after one must not be written through it.
                links.push((String::from_utf8_lossy(&bytes).into_owned(), dst));
                continue;
            }
            let mut f = std::fs::File::create_new(dst)?;
            io::Write::write_all(&mut f, &bytes)?;
            let _ = f.set_modified(when);
            #[cfg(unix)]
            if flags.contains('x') {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o755))?;
            }
        }
        for (target, dst) in links {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, dst)?;
            #[cfg(not(unix))]
            std::fs::write(dst, &target)?;
        }
        // Folders last: writing into one sets its time to now.
        let dirs: HashSet<&Path> = files
            .iter()
            .flat_map(|(_, _, dst)| dst.ancestors().skip(1).take_while(|d| d.starts_with(to)))
            .collect();
        for dir in dirs {
            let mut o = std::fs::OpenOptions::new();
            o.read(true);
            #[cfg(windows)]
            std::os::windows::fs::OpenOptionsExt::custom_flags(o.write(true), 0x0200_0000); // FILE_FLAG_BACKUP_SEMANTICS: a folder
            let _ = o.open(dir).and_then(|f| f.set_modified(when));
        }
        Ok(())
    }
}

/// hg's message, or its complaint as the error.
fn said(mut c: Command) -> io::Result<String> {
    let o = c.output()?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    if !o.status.success() {
        let why = text(&o.stderr);
        return Err(io::Error::other(if why.is_empty() { "hg failed".into() } else { why }));
    }
    Ok(text(&o.stdout))
}

/// The bookmarks (`hg bookmarks -T …`) and then the named branches (`hg branches -T …`) that
/// no bookmark has the name of; `here` is the working folder's branch, current when no
/// bookmark is active.
pub fn parse_hg_branches(marks: &str, named: &str, here: &str) -> Vec<Branch> {
    let parse = |out: &str, is_branch: bool| -> Vec<Branch> {
        out.lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.splitn(6, '\x1f').collect();
                let [name, active, oid, date, author, subject] = f[..] else { return None };
                Some(Branch {
                    name: name.to_string(),
                    remote: false,
                    current: !is_branch && active == "True",
                    oid: oid.to_string(),
                    time: date.split(' ').next()?.parse().unwrap_or(0),
                    author: author.to_string(),
                    subject: subject.to_string(),
                    ahead: 0,
                    behind: 0,
                })
            })
            .collect()
    };
    let mut all = parse(marks, false);
    let any_active = all.iter().any(|b| b.current);
    for mut b in parse(named, true) {
        if all.iter().any(|m| m.name == b.name) {
            continue;
        }
        b.current = !any_active && b.name == here;
        all.push(b);
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vcs_jj_summary_and_renames() {
        let root = Path::new("/r");
        let st = parse_summary(
            root,
            "M a.txt\nA sub/new.txt\nD old.txt\nR sub/{b.txt => c.txt}\nR {x => y/x}\n",
        );
        let k = |p: &str| st.get(&root.join(p)).map(|f| f.kind);
        assert_eq!(k("a.txt"), Some(Kind::Modified));
        assert_eq!(k("sub/new.txt"), Some(Kind::Added));
        assert_eq!(k("sub/c.txt"), Some(Kind::Renamed));
        assert_eq!(k("y/x"), Some(Kind::Renamed));
        assert_eq!(k("sub"), Some(Kind::Renamed));
        let s = &st.summary;
        assert_eq!((s.modified, s.staged, s.deleted), (1, 3, 1));
        assert_eq!(renamed_to("a/{b => }/c"), "a/c");
    }

    #[test]
    fn vcs_jj_bookmarks() {
        let out = "main\x1f\x1fabc\x1f17\x1fAda\x1ftrue\x1fone\n\
                   main\x1fgit\x1fabc\x1f17\x1fAda\x1ftrue\x1fone\n\
                   main\x1forigin\x1fabd\x1f16\x1fAda\x1ffalse\x1fzero\n\
                   wip\x1f\x1fabe\x1f18\x1fBo\x1ffalse\x1ftwo\n\
                   torn\x1f\x1f\n";
        let b = parse_bookmarks(out);
        let names: Vec<&str> = b.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["main", "wip", "main@origin"]);
        assert!(b[0].current && !b[1].current && b[2].remote);
        assert_eq!((b[1].time, b[1].author.as_str()), (18, "Bo"));
    }

    /// The jj CLI, when installed, with a fake home: `None` (the test passes) without it.
    fn jj_repo(name: &str, colocate: bool) -> Option<(PathBuf, impl Fn(&[&str]))> {
        if !crate::user_menu::installed("jj") {
            return None;
        }
        let top =
            std::env::temp_dir().join(format!("coxswain-jj-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let (home, d) = (top.join("home"), top.join("repo"));
        std::fs::create_dir_all(&home).ok()?;
        std::fs::create_dir_all(&d).ok()?;
        let (h, r) = (home.clone(), d.clone());
        let run = move |args: &[&str]| {
            let ok = Command::new("jj")
                .current_dir(&r)
                .args(args)
                .env("HOME", &h)
                .env("XDG_CONFIG_HOME", h.join(".config"))
                .env("JJ_USER", "Ada")
                .env("JJ_EMAIL", "ada@example.org")
                .env("JJ_CONFIG", h.join("jj.toml"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(ok.success(), "jj {args:?}");
        };
        run(&[
            "git",
            "init",
            if colocate { "--colocate" } else { "--no-colocate" },
        ]);
        Some((d, run))
    }

    /// A Jujutsu repository, with and without git next to it: the line names jj, the change
    /// and the bookmark; the files carry their marks; the history and the bookmarks are jj's,
    /// and the folder at a commit is read from its store. Nothing is written by the reads.
    #[test]
    fn vcs_jj_repository_is_read() {
        for colocate in [false, true] {
            let Some((d, jj)) = jj_repo(if colocate { "co" } else { "alone" }, colocate) else {
                return;
            };
            std::fs::write(d.join("a.txt"), "a\n").unwrap();
            std::fs::create_dir(d.join("sub")).unwrap();
            std::fs::write(d.join("sub/b.txt"), "b\n").unwrap();
            jj(&["commit", "-m", "first one"]);
            jj(&["bookmark", "create", "main", "-r", "@-"]);
            std::fs::write(d.join("a.txt"), "a\nc\n").unwrap();
            jj(&["status"]);
            let ops = || std::fs::read_dir(d.join(".jj/repo/op_store/operations")).unwrap().count();
            let before = ops();
            assert_eq!(of(&d).name(), "jj");
            let st = Status::read(&d.join("sub")).expect("status");
            assert_eq!(st.summary.system, "jj");
            assert_eq!(st.summary.branch, "main");
            assert!(st.summary.change.as_ref().is_some_and(|c| c.len() >= 8));
            assert_eq!(st.get(&d.join("a.txt")).map(|f| f.kind), Some(Kind::Modified));
            let line = st.prompt(&crate::config::Glyphs::ascii());
            assert!(line.starts_with("jj: ") && line.contains(" main") && line.contains("~1"), "{line}");
            let at = crate::history::split(&d.join("a.txt").join(crate::history::MARKER)).unwrap();
            let list = crate::history::list(&at.target.join(crate::history::MARKER), &at).unwrap();
            // `..`, the working-copy commit (a.txt changed) and the first one.
            assert_eq!(list.len(), 3, "{list:?}");
            assert!(list[2].name.ends_with("first one"));
            let b = crate::branches::branches(&d).unwrap();
            assert_eq!(b.len(), 1);
            assert!(b[0].current && b[0].name == "main");
            let tree = crate::history::path(&d, Some(&b[0].oid));
            let at = crate::history::split(&tree).unwrap();
            let names: Vec<String> = crate::history::list(&tree, &at).unwrap().into_iter().map(|e| e.name).collect();
            assert_eq!(names, ["..", "a.txt", "sub"]);
            let at = crate::history::split(&tree.join("a.txt")).unwrap();
            assert_eq!(crate::history::read(&at, 100).unwrap().0, b"a\n");
            assert!(crate::branches::switch(&crate::history::path_of(&d, crate::history::View::Branches), "main").is_err());
            assert_eq!(ops(), before, "a read wrote an operation");
            std::fs::remove_dir_all(d.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn vcs_hg_status_and_branches() {
        let root = Path::new("/r");
        let st = parse_hg_status(root, "M a.txt\nA sub/new.txt\nR old.txt\n! gone.txt\n? notes.txt\n");
        let k = |p: &str| st.get(&root.join(p)).map(|f| f.kind);
        assert_eq!(k("a.txt"), Some(Kind::Modified));
        assert_eq!(k("sub"), Some(Kind::Added));
        assert_eq!((k("old.txt"), k("gone.txt")), (Some(Kind::Deleted), Some(Kind::Deleted)));
        let s = &st.summary;
        assert_eq!((s.modified, s.staged, s.deleted, s.untracked), (1, 1, 2, 1));
        let b = parse_hg_branches(
            "main\x1fTrue\x1fabc\x1f17 0\x1fAda\x1fone\n",
            "default\x1fTrue\x1fabc\x1f17 0\x1fAda\x1fone\nmain\x1fTrue\x1fabd\x1f16 0\x1fBo\x1ftwo\nstable\x1fFalse\x1fabe\x1f18 -3600\x1fCy\x1fthree\n",
            "default",
        );
        let names: Vec<&str> = b.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["main", "default", "stable"]);
        assert!(b[0].current && !b[1].current && !b[2].current);
        assert_eq!(b[2].time, 18);
        assert!(hg_new_enough("Mercurial Distributed SCM (version 6.7.2)\n"));
        assert!(!hg_new_enough("Mercurial Distributed SCM (version 5.3)\n") && !hg_new_enough(""));
        assert_eq!((hg_rev("012345678901"), hg_rev("main")), ("id(012345678901)".into(), "main".into()));
    }

    /// A Mercurial repository made with the hg CLI and a fake home: the line, the history, a
    /// folder at a commit, a file's diff and copy, the bookmarks, a switch refused while a
    /// file is changed and done when it is not, and a new bookmark. `None` without hg.
    #[test]
    fn vcs_hg_repository() {
        if !crate::user_menu::installed("hg") {
            return;
        }
        let top = std::env::temp_dir().join(format!("coxswain-hg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let (home, d) = (top.join("home"), top.join("repo"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(d.join("sub")).unwrap();
        let hg = |args: &[&str]| {
            let ok = Command::new("hg")
                .current_dir(&d)
                .args(args)
                .env("HOME", &home)
                .env("HGRCPATH", "")
                .env("HGUSER", "Ada <ada@example.org>")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(ok.success(), "hg {args:?}");
        };
        hg(&["init"]);
        std::fs::write(d.join("a.txt"), "a\n").unwrap();
        std::fs::write(d.join("sub/b.txt"), "b\n").unwrap();
        hg(&["add"]);
        hg(&["commit", "-m", "first one", "-d", "1700000000 0"]);
        hg(&["bookmark", "main"]);
        std::fs::write(d.join("a.txt"), "a\nc\n").unwrap();
        hg(&["commit", "-m", "second", "-d", "1700000100 0"]);
        hg(&["bookmark", "-r", "0", "old"]);
        std::fs::write(d.join("a.txt"), "changed\n").unwrap();
        // A hook in the repository's own config never runs.
        let mark = top.join("ran");
        std::fs::write(d.join(".hg/hgrc"), format!("[hooks]\npre-status = touch {}\npre-log = touch {}\n", mark.display(), mark.display())).unwrap();
        assert_eq!(of(&d).name(), "hg");
        let st = Status::read(&d.join("sub")).expect("status");
        assert_eq!((st.summary.system, st.summary.branch.as_str()), ("hg", "main"));
        assert_eq!(st.get(&d.join("a.txt")).map(|f| f.kind), Some(Kind::Modified));
        assert!(st.prompt(&crate::config::Glyphs::ascii()).starts_with("hg: main"));
        let at = crate::history::split(&d.join("a.txt").join(crate::history::MARKER)).unwrap();
        let list = crate::history::list(&at.target.join(crate::history::MARKER), &at).unwrap();
        assert_eq!(list.len(), 3, "{list:?}");
        assert!(list[1].name.ends_with("second") && list[2].name.ends_with("first one"));
        let first = list[2].path.clone();
        let names: Vec<String> = crate::history::list(&first, &crate::history::split(&first).unwrap()).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["..", "a.txt", "sub"]);
        let at = crate::history::split(&first.join("a.txt")).unwrap();
        assert_eq!(crate::history::read(&at, 100).unwrap().0, b"a\n");
        assert!(crate::history::diff(&at).unwrap().contains("+a"));
        let out = top.join("out");
        std::fs::create_dir(&out).unwrap();
        let at = crate::history::split(&first.join("sub")).unwrap();
        let copied = crate::history::copy_out(&at, &out).unwrap();
        assert_eq!(std::fs::read(copied.join("b.txt")).unwrap(), b"b\n");
        let b = crate::branches::branches(&d).unwrap();
        let names: Vec<&str> = b.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["main", "old", "default"]);
        assert!(b[0].current);
        let list = crate::history::path_of(&d, crate::history::View::Branches);
        assert!(crate::branches::switch(&list, "old").is_err(), "a change would be lost");
        std::fs::write(d.join("a.txt"), "a\nc\n").unwrap();
        crate::branches::switch(&list, "old").expect("switch");
        assert_eq!(std::fs::read(d.join("a.txt")).unwrap(), b"a\n");
        assert_eq!(Status::read(&d).unwrap().summary.branch, "old");
        crate::branches::create(&d, "feature", None).expect("new bookmark");
        assert_eq!(Status::read(&d).unwrap().summary.branch, "feature");
        assert!(crate::branches::create(&d, "has space", None).is_err());
        assert!(!mark.exists(), "the repository's hook ran");
        std::fs::remove_dir_all(top).unwrap();
    }


    /// Without hg, nothing in a Mercurial repository is asked of git instead, and nothing that
    /// writes runs: each says hg is missing.
    #[test]
    fn vcs_hg_missing_refuses_all() {
        NO_HG.with(|n| n.set(true));
        let top = std::env::temp_dir().join(format!("coxswain-hg-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        std::fs::create_dir_all(top.join(".hg")).unwrap();
        let list = crate::history::path_of(&top, crate::history::View::Branches);
        let at = At { target: top.clone(), base: top.clone(), commit: Some("abc".into()), inner: String::new(), view: crate::history::View::History };
        let missing = |e: io::Error| assert_eq!(e.kind(), io::ErrorKind::NotFound, "{e}");
        assert_eq!(of(&top).name(), "hg");
        missing(Hg.log(&top, ".", 10).unwrap_err());
        missing(Hg.branches(&top).unwrap_err());
        missing(Hg.switch(&list, "main").unwrap_err());
        missing(Hg.create(&top, "feature", None).unwrap_err());
        missing(Hg.show(&top, "abc").unwrap_err());
        missing(Hg.tree(&top, "abc", "").unwrap_err());
        missing(Hg.read(&top, "abc", "a", 10).unwrap_err());
        missing(Hg.diff(&top, "abc", "").unwrap_err());
        missing(Hg.export(&at, "abc", &top.join("out")).unwrap_err());
        missing(crate::branches::switch(&list, "main").unwrap_err());
        NO_HG.with(|n| n.set(false));
        std::fs::remove_dir_all(top).unwrap();
    }

}

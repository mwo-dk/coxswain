//! A repository's branches and worktrees as folders, the way `history` shows commits:
//! `<repo>/@branches` lists the local branches, then the remote-tracking ones, and
//! `<repo>/@branches/<commit>/…` is the tree at a branch's commit, read-only (`history` lists
//! it); `<repo>/@worktrees` lists the worktrees, each leading to its folder on disk. A switch
//! and a new branch go through `git switch`, which refuses what would lose changes.

use std::io;
use std::path::{Path, PathBuf};

use crate::fs::Entry;
use crate::history::{self, Last, Lasts, View};

/// A branch, as `git for-each-ref` tells it.
#[derive(Clone, Debug, PartialEq)]
pub struct Branch {
    /// `main`, or `origin/main` for a remote-tracking one.
    pub name: String,
    pub remote: bool,
    /// HEAD is on it.
    pub current: bool,
    pub oid: String,
    pub time: u64,
    pub author: String,
    pub subject: String,
    pub ahead: u32,
    pub behind: u32,
}

const REFS: &str = "--format=%(HEAD)%1f%(refname)%1f%(objectname)%1f%(authordate:unix)%1f%(authorname)%1f%(upstream:track,nobracket)%1f%(symref)%1f%(subject)";

/// The lines of `git for-each-ref --format=REFS refs/heads refs/remotes`, in git's order (by
/// name: the local ones first); a remote's `HEAD`, which only points at a branch, left out.
pub fn parse_refs(out: &str) -> Vec<Branch> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(8, '\x1f').collect();
            let [head, refname, oid, time, author, track, symref, subject] = f[..] else { return None };
            let (name, remote) = match (refname.strip_prefix("refs/heads/"), refname.strip_prefix("refs/remotes/")) {
                (Some(n), _) => (n, false),
                (_, Some(n)) if symref.is_empty() => (n, true),
                _ => return None,
            };
            let count = |what: &str| track.split(", ").find_map(|p| p.strip_prefix(what)?.trim().parse().ok()).unwrap_or(0);
            Some(Branch {
                name: name.to_string(),
                remote,
                current: head == "*",
                oid: oid.to_string(),
                time: time.parse().unwrap_or(0),
                author: author.to_string(),
                subject: subject.to_string(),
                ahead: count("ahead "),
                behind: count("behind "),
            })
        })
        .collect()
}

/// The branches of the repository `dir` is in.
pub fn branches(dir: &Path) -> io::Result<Vec<Branch>> {
    let mut c = history::git(dir);
    c.args(["for-each-ref", REFS, "refs/heads", "refs/remotes"]);
    Ok(parse_refs(&String::from_utf8_lossy(&history::run(c)?)))
}

/// A branch's name in a listing: `*` before the current one, `∕` for `/` (a name is one path
/// segment), how far ahead and behind its upstream it is, and its last commit's subject. A
/// branch name never has a space, so the name is all up to the first one.
pub fn entry_name(b: &Branch) -> String {
    let mut s = format!("{}{}", if b.current { "* " } else { "" }, b.name.replace('/', "∕"));
    for (n, arrow) in [(b.ahead, '↑'), (b.behind, '↓')] {
        if n > 0 {
            s += &format!(" {arrow}{n}");
        }
    }
    let subject: String = b.subject.chars().map(|ch| if matches!(ch, '/' | '\\') || ch.is_control() { '∕' } else { ch }).collect();
    format!("{s} · {subject}")
}

/// The branch an entry of the list is, read again.
fn find(dir: &Path, entry: &str) -> io::Result<Branch> {
    let name = entry.trim_start_matches("* ").split(' ').next().unwrap_or_default();
    branches(dir)?
        .into_iter()
        .find(|b| b.name.replace('/', "∕") == name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, crate::t!("branches.gone", "name" => name)))
}

fn dir_entry(name: String, path: PathBuf, time: u64) -> Entry {
    Entry { hidden: false, name, path, is_dir: true, is_symlink: false, is_exec: false, size: 0, modified: time, created: time, online: false }
}

/// The list of branches as folders: each leads to the tree at its commit. Two branches on one
/// commit get ids of different lengths, so each has a path of its own (the apps mark and draw
/// entries by path).
pub fn entries(dir: &Path) -> io::Result<Vec<Entry>> {
    let list = history::path_of(dir, View::Branches);
    let mut seen = std::collections::HashMap::<&str, usize>::new();
    let all = branches(dir)?;
    Ok(all
        .iter()
        .map(|b| {
            let n = seen.entry(b.oid.as_str()).or_insert(11);
            *n += 1;
            dir_entry(entry_name(b), list.join(&b.oid[..b.oid.len().min(*n)]), b.time)
        })
        .collect())
}

/// Each branch's last commit, by its name in the list.
pub fn lasts(dir: &Path) -> io::Result<Lasts> {
    Ok(branches(dir)?.iter().map(|b| (entry_name(b), Some(Last { time: b.time, author: b.author.clone(), hash: b.oid[..b.oid.len().min(7)].to_string(), subject: b.subject.clone() }))).collect())
}

/// git's message, or its complaint as the error.
fn said(mut c: std::process::Command) -> io::Result<String> {
    let o = c.output()?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    if !o.status.success() {
        let why = text(&o.stderr);
        return Err(io::Error::other(if why.is_empty() { "git failed".into() } else { why }));
    }
    Ok([text(&o.stderr), text(&o.stdout)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n"))
}

/// Files are written with the repository's own filters turned off (`git::command`): where it
/// has some (git-crypt, say), the files would come out wrong, so git itself is asked to do it.
fn writes_files_safely(dir: &Path) -> io::Result<()> {
    if crate::git::repo_filters(dir).is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(crate::t!("branches.has_filters")))
    }
}

/// The folder git runs in for `dir`: the folder a list of branches is of, or `dir` itself.
fn base(dir: &Path) -> PathBuf {
    history::split(dir).map_or_else(|| dir.to_path_buf(), |at| at.base)
}

/// Switch to the branch `entry` names in the list of branches at `dir`; a remote one gets a
/// local branch that tracks it. git refuses when changes here would be overwritten, and its
/// message says which: it is never forced.
pub fn switch(dir: &Path, entry: &str) -> io::Result<String> {
    let at = history::split(dir).filter(|at| at.view == View::Branches && at.commit.is_none()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, crate::t!("branches.switch_where")))?;
    let b = find(&at.base, entry)?;
    // A ref made by hand may start with `-`: never an option.
    if b.name.starts_with('-') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, crate::t!("branches.bad_name", "name" => b.name)));
    }
    writes_files_safely(&at.base)?;
    let mut c = history::git(&at.base);
    c.arg("switch");
    if b.remote {
        c.arg("--track");
    }
    c.arg(&b.name);
    said(c)
}

/// A new branch `name`, switched to: from the branch `from` names in the list of branches at
/// `dir`, or from the current commit.
pub fn create(dir: &Path, name: &str, from: Option<&str>) -> io::Result<String> {
    let base = base(dir);
    let name = name.trim();
    let mut check = history::git(&base);
    check.args(["check-ref-format", "--branch", name]);
    if name.is_empty() || name.starts_with('-') || history::run(check).is_err() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, crate::t!("branches.bad_name", "name" => name)));
    }
    let start = match from.filter(|e| *e != ".." && history::split(dir).is_some_and(|at| at.view == View::Branches && at.commit.is_none())) {
        Some(e) => {
            writes_files_safely(&base)?;
            Some(find(&base, e)?.name)
        }
        None => None,
    };
    let mut c = history::git(&base);
    c.args(["switch", "-c", name]);
    if let Some(s) = start.filter(|s| !s.starts_with('-')) {
        c.arg(s);
    }
    said(c)
}

/// A worktree, as `git worktree list --porcelain -z` tells it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: String,
    /// `None`: detached (or bare).
    pub branch: Option<String>,
    pub bare: bool,
    pub locked: bool,
    /// Its folder is gone: `git worktree prune` would forget it.
    pub prunable: bool,
}

/// The records of `git worktree list --porcelain -z`: fields ended by NUL, records by an
/// empty one.
pub fn parse_worktrees(out: &str) -> Vec<Worktree> {
    let mut all = vec![];
    let mut w: Option<Worktree> = None;
    for f in out.split('\0') {
        let (k, v) = f.split_once(' ').unwrap_or((f, ""));
        match k {
            "worktree" => all.extend(w.replace(Worktree { path: PathBuf::from(v), ..Default::default() })),
            "HEAD" => w.iter_mut().for_each(|w| w.head = v.to_string()),
            "branch" => w.iter_mut().for_each(|w| w.branch = Some(v.strip_prefix("refs/heads/").unwrap_or(v).to_string())),
            "bare" => w.iter_mut().for_each(|w| w.bare = true),
            "locked" => w.iter_mut().for_each(|w| w.locked = true),
            "prunable" => w.iter_mut().for_each(|w| w.prunable = true),
            _ => {}
        }
    }
    all.extend(w);
    all
}

/// The worktrees of the repository `dir` is in, the main one first.
pub fn worktrees(dir: &Path) -> io::Result<Vec<Worktree>> {
    let mut c = history::git(dir);
    c.args(["worktree", "list", "--porcelain", "-z"]);
    Ok(parse_worktrees(&String::from_utf8_lossy(&history::run(c)?)))
}

/// Whether the worktree at `path` has changes, untracked files too: `None` when git cannot say.
fn dirty(path: &Path) -> Option<bool> {
    let mut c = history::git(path);
    c.args(["status", "--porcelain", "-unormal", "--ignore-submodules=dirty"]);
    history::run(c).ok().map(|o| !o.is_empty())
}

/// The list of worktrees as folders: each is its folder on disk. Its name says its branch (or
/// the commit it is detached at), whether it has changes, and whether it is locked or gone; `*`
/// before the one `dir` is in.
pub fn worktree_entries(dir: &Path) -> io::Result<Vec<Entry>> {
    let here = dir.canonicalize().ok();
    Ok(worktrees(dir)?
        .into_iter()
        .map(|w| {
            let folder = w.path.file_name().map_or_else(|| w.path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
            let head = match &w.branch {
                Some(b) => b.clone(),
                None if w.bare => crate::t!("worktrees.bare"),
                None => crate::t!("git.detached", "commit" => &w.head[..w.head.len().min(7)]),
            };
            let mut name = format!("{}{folder} [{}]", if w.path.canonicalize().ok() == here { "* " } else { "" }, head.replace('/', "∕"));
            if !w.bare && !w.prunable {
                match dirty(&w.path) {
                    Some(true) => name += &format!(" {}", crate::t!("worktrees.dirty")),
                    Some(false) => name += &format!(" {}", crate::t!("worktrees.clean")),
                    None => {}
                }
            }
            for (on, key) in [(w.locked, "worktrees.locked"), (w.prunable, "worktrees.prunable")] {
                if on {
                    name += &format!(" {}", crate::t!(key));
                }
            }
            let time = std::fs::metadata(&w.path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            dir_entry(name, w.path, time)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{add, commit_as, repo};

    fn git(d: &Path, args: &[&str]) -> bool {
        crate::tools::command("git").arg("-C").arg(d).args(args).output().unwrap().status.success()
    }

    #[test]
    fn branches_parse_for_each_ref() {
        let out = concat!(
            "*\x1frefs/heads/main\x1faaaa111122223333\x1f1700000000\x1fAda\x1fahead 1, behind 2\x1f\x1fFix the valve\n",
            " \x1frefs/heads/feature/x\x1fbbbb\x1f1700000100\x1fBob\x1f\x1f\x1fA ∕ subject\n",
            " \x1frefs/remotes/origin/HEAD\x1fcccc\x1f1\x1fCy\x1f\x1frefs/remotes/origin/main\x1fx\n",
            " \x1frefs/remotes/origin/main\x1fcccc\x1f1700000200\x1fCy\x1f\x1f\x1fOn the server\n",
        );
        let b = parse_refs(out);
        assert_eq!(b.iter().map(|b| (b.name.as_str(), b.remote, b.current)).collect::<Vec<_>>(), [("main", false, true), ("feature/x", false, false), ("origin/main", true, false)]);
        assert_eq!((b[0].ahead, b[0].behind, b[0].time, b[0].author.as_str()), (1, 2, 1_700_000_000, "Ada"));
        assert_eq!(entry_name(&b[0]), "* main ↑1 ↓2 · Fix the valve");
        assert_eq!(entry_name(&b[1]), "feature∕x · A ∕ subject");
    }

    #[test]
    fn branches_parse_worktree_porcelain() {
        let out = "worktree /r/main\0HEAD aaaa\0branch refs/heads/main\0\0worktree /r/feat\0HEAD bbbb\0detached\0locked because\0\0worktree /r/gone\0HEAD cccc\0branch refs/heads/old\0prunable gitdir file points to non-existent location\0\0";
        let w = parse_worktrees(out);
        assert_eq!(w.len(), 3);
        assert_eq!((w[0].path.as_path(), w[0].branch.as_deref()), (Path::new("/r/main"), Some("main")));
        assert_eq!((w[1].branch.as_deref(), w[1].head.as_str(), w[1].locked), (None, "bbbb", true));
        assert!(w[2].prunable && !w[2].locked);
    }

    #[test]
    fn branches_operation_from_state_files() {
        let d = std::env::temp_dir().join(format!("coxswain-op-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(crate::git::operation(&d), None);
        for (f, op) in [("BISECT_LOG", "bisect"), ("REVERT_HEAD", "revert"), ("CHERRY_PICK_HEAD", "cherry_pick"), ("MERGE_HEAD", "merge")] {
            std::fs::write(d.join(f), "").unwrap();
            assert_eq!(crate::git::operation(&d), Some(op));
        }
        std::fs::create_dir(d.join("rebase-merge")).unwrap();
        assert_eq!(crate::git::operation(&d), Some("rebase"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn branches_virtual_paths() {
        let d = std::env::temp_dir().join(format!("coxswain-views-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("f"), "").unwrap();
        let at = history::split(&d.join("@branches")).unwrap();
        assert_eq!((at.view, at.commit, at.base.as_path()), (View::Branches, None, d.as_path()));
        let at = history::split(&d.join("@branches/0123456789ab/src")).unwrap();
        assert_eq!((at.view, at.commit.as_deref(), at.inner.as_str()), (View::Branches, Some("0123456789ab"), "src"));
        assert_eq!(history::split(&d.join("@worktrees")).unwrap().view, View::Worktrees);
        assert!(history::split(&d.join("@worktrees/x")).is_none(), "a worktree is its own folder");
        assert!(history::split(&d.join("f/@branches")).is_none(), "of a folder only");
        assert!(history::is_history(&d.join("@worktrees")) && history::is_history(&d.join("@branches/ab12")));
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Branches listed, browsed, created and switched to with the git CLI; a switch that would
    /// overwrite changes is refused with git's own message, and nothing is lost. Worktrees.
    #[test]
    fn branches_list_switch_create_and_worktrees() {
        let Some(d) = repo("branches") else { return };
        std::fs::write(d.join("a.txt"), "one\n").unwrap();
        add(&d);
        commit_as(&d, "Ada", 1_700_000_000, "first");
        assert!(git(&d, &["branch", "feature/x"]));
        std::fs::write(d.join("a.txt"), "two\n").unwrap();
        commit_as(&d, "Bob", 1_700_100_000, "second");
        // A remote-tracking branch, as a fetch would make it.
        assert!(git(&d, &["update-ref", "refs/remotes/origin/main", "HEAD~1"]));

        let list = crate::fs::list(&history::path_of(&d, View::Branches), true).unwrap();
        let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["..", "feature∕x · first", "* main · second", "origin∕main · first"]);
        assert_eq!((list[0].path.as_path(), list[2].modified), (d.as_path(), 1_700_100_000));
        // Two branches on one commit: each a path of its own, both leading to that commit.
        assert_ne!(list[1].path, list[3].path);
        assert_eq!(crate::fs::list(&list[3].path, true).unwrap().len(), 2);
        // Into a branch: its tree at its commit, read-only.
        let tree = crate::fs::list(&list[1].path, true).unwrap();
        assert_eq!(tree.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["..", "a.txt"]);
        assert_eq!(history::read(&history::split(&list[1].path.join("a.txt")).unwrap(), 100).unwrap().0, b"one\n");
        let l = history::last_changes(&history::path_of(&d, View::Branches)).unwrap();
        assert_eq!(l[&list[2].name].as_ref().unwrap().author, "Bob");

        // A change the switch would overwrite: refused, in git's words, the change kept.
        let branches = history::path_of(&d, View::Branches);
        std::fs::write(d.join("a.txt"), "mine\n").unwrap();
        let err = switch(&branches, &list[1].name).unwrap_err().to_string();
        assert!(err.contains("a.txt"), "{err}");
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "mine\n");
        std::fs::write(d.join("a.txt"), "two\n").unwrap();
        switch(&branches, &list[1].name).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "one\n");
        // A remote one gets a local branch that tracks it: here `main` is there already.
        assert!(switch(&branches, &list[3].name).unwrap_err().to_string().contains("main"));

        // New branches: from the current commit, and from a branch in the list.
        assert!(create(&d, "-x", None).is_err() && create(&d, "a b", None).is_err());
        create(&d, "new", None).unwrap();
        assert!(super::branches(&d).unwrap().iter().any(|b| b.current && b.name == "new"));
        create(&branches, "from-main", Some(&list[2].name)).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "two\n");

        // A linked worktree, detached.
        let wt = d.with_file_name(format!("{}-wt", d.file_name().unwrap().to_string_lossy()));
        let _ = std::fs::remove_dir_all(&wt);
        assert!(git(&d, &["worktree", "add", "-q", "--detach", &wt.to_string_lossy(), "main"]));
        std::fs::write(wt.join("b.txt"), "").unwrap();
        let list = crate::fs::list(&history::path_of(&d, View::Worktrees), true).unwrap();
        assert_eq!(list.len(), 3);
        assert!(list[1].name.starts_with("* ") && list[1].name.contains("[from-main]") && list[1].name.contains("clean"), "{}", list[1].name);
        assert!(list[2].name.contains("detached at") && list[2].name.contains("dirty"), "{}", list[2].name);
        assert_eq!(list[2].path.canonicalize().unwrap(), wt.canonicalize().unwrap());
        let st = crate::git::Status::read(&wt).unwrap();
        assert_eq!(st.summary.worktree.as_deref(), wt.file_name().and_then(|n| n.to_str()));
        assert!(st.prompt(&crate::config::Glyphs::ascii()).contains("detached at"));
        assert_eq!(crate::git::Status::read(&d).unwrap().summary.worktree, None);
        std::fs::remove_dir_all(&wt).unwrap();
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A merge that stopped on a conflict: the status says a merge is under way.
    #[test]
    fn branches_status_names_a_merge_in_progress() {
        let Some(d) = repo("merging") else { return };
        std::fs::write(d.join("a.txt"), "one\n").unwrap();
        add(&d);
        commit_as(&d, "Ada", 1_700_000_000, "first");
        assert!(git(&d, &["switch", "-q", "-c", "other"]));
        std::fs::write(d.join("a.txt"), "other\n").unwrap();
        commit_as(&d, "Bob", 1_700_100_000, "other");
        assert!(git(&d, &["switch", "-q", "main"]));
        std::fs::write(d.join("a.txt"), "main\n").unwrap();
        commit_as(&d, "Cy", 1_700_200_000, "main");
        assert!(!git(&d, &["-c", "user.name=t", "-c", "user.email=t@t", "merge", "-q", "other"]));
        let st = crate::git::Status::read(&d).unwrap();
        assert_eq!(st.summary.operation, Some("merge"));
        assert!(st.prompt(&crate::config::Glyphs::ascii()).ends_with("merging"), "{}", st.prompt(&crate::config::Glyphs::ascii()));
        std::fs::remove_dir_all(d).unwrap();
    }
}

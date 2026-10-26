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
            let [head, refname, oid, time, author, track, symref, subject] = f[..] else {
                return None;
            };
            let (name, remote) = match (
                refname.strip_prefix("refs/heads/"),
                refname.strip_prefix("refs/remotes/"),
            ) {
                (Some(n), _) => (n, false),
                (_, Some(n)) if symref.is_empty() => (n, true),
                _ => return None,
            };
            let count = |what: &str| {
                track
                    .split(", ")
                    .find_map(|p| p.strip_prefix(what)?.trim().parse().ok())
                    .unwrap_or(0)
            };
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

/// The branches of the repository `dir` is in (Jujutsu's bookmarks in one).
pub fn branches(dir: &Path) -> io::Result<Vec<Branch>> {
    crate::vcs::of(dir).branches(dir)
}

pub(crate) fn git_branches(dir: &Path) -> io::Result<Vec<Branch>> {
    let mut c = history::git(dir);
    c.args(["for-each-ref", REFS, "refs/heads", "refs/remotes"]);
    Ok(parse_refs(&String::from_utf8_lossy(&history::run(c)?)))
}

/// A branch's name in a listing: `*` before the current one, `∕` for `/` (a name is one path
/// segment), how far ahead and behind its upstream it is, and its last commit's subject. A
/// branch name never has a space, so the name is all up to the first one.
pub fn entry_name(b: &Branch) -> String {
    let mut s = format!(
        "{}{}",
        if b.current { "* " } else { "" },
        b.name.replace('/', "∕")
    );
    for (n, arrow) in [(b.ahead, '↑'), (b.behind, '↓')] {
        if n > 0 {
            s += &format!(" {arrow}{n}");
        }
    }
    format!("{s} · {}", crate::history::safe_name(&b.subject))
}

/// The branch an entry of the list names, `entry_name`'s inverse.
pub fn name_of(entry: &str) -> String {
    segment(entry).replace('∕', "/")
}

/// The name part of an entry of the list, as `entry_name` wrote it (`∕` for `/`).
fn segment(entry: &str) -> &str {
    entry
        .trim_start_matches("* ")
        .split(' ')
        .next()
        .unwrap_or_default()
}

/// The branch an entry of the list is, read again.
fn find(dir: &Path, entry: &str) -> io::Result<Branch> {
    let name = segment(entry);
    branches(dir)?
        .into_iter()
        .find(|b| b.name.replace('/', "∕") == name)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                crate::t!("branches.gone", "name" => name),
            )
        })
}

fn dir_entry(name: String, path: PathBuf, time: u64) -> Entry {
    Entry {
        name,
        path,
        is_dir: true,
        modified: time,
        created: time,
        ..Entry::default()
    }
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
            dir_entry(
                entry_name(b),
                list.join(&b.oid[..b.oid.len().min(*n)]),
                b.time,
            )
        })
        .collect())
}

/// Each branch's last commit, by its name in the list.
pub fn lasts(dir: &Path) -> io::Result<Lasts> {
    Ok(branches(dir)?
        .iter()
        .map(|b| {
            (
                entry_name(b),
                Some(Last {
                    time: b.time,
                    author: b.author.clone(),
                    hash: b.oid[..b.oid.len().min(7)].to_string(),
                    subject: b.subject.clone(),
                }),
            )
        })
        .collect())
}

/// git's message, or its complaint as the error.
fn said(mut c: std::process::Command) -> io::Result<String> {
    let o = c.output()?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    if !o.status.success() {
        let why = text(&o.stderr);
        return Err(io::Error::other(if why.is_empty() {
            "git failed".into()
        } else {
            why
        }));
    }
    Ok([text(&o.stderr), text(&o.stdout)]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Files are written with the repository's own filters turned off (`git::command`): where it
/// has some (git-crypt, say), the files would come out wrong, so git itself is asked to do it.
/// The user's own filters still run, git-lfs's among them, and git-lfs reads the repository's
/// own config: a repository from elsewhere could have it run a program or send the user's login
/// through a proxy while it writes the files of `target` (`lfs_risk`), so that is refused too.
fn writes_files_safely(dir: &Path, target: &str) -> io::Result<()> {
    if !crate::git::repo_filters(dir).is_empty() {
        return Err(io::Error::other(crate::t!("branches.has_filters")));
    }
    match lfs_risk(dir, target) {
        Some(key) => Err(io::Error::other(crate::t!("branches.lfs_risk", "key" => key))),
        None => Ok(()),
    }
}

/// A switch or a new branch updates refs and the work tree: refused where the repository's own
/// config would have git run a program for it, by a hook set in config (`hook.<name>.command`,
/// which `core.hooksPath` does not stop), or where it puts the work tree somewhere else
/// (`core.worktree`), so a checkout would write outside the folder the app shows.
fn changes_refs_safely(base: &Path) -> io::Result<()> {
    let mut c = crate::tools::command("git");
    c.arg("-C")
        .arg(base)
        .args(["config", "--show-scope", "--name-only", "--get-regexp", r"^hook\."])
        .env("GIT_CONFIG_COUNT", "0")
        .stdin(std::process::Stdio::null());
    let out = c.output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    if let Some(key) = out.lines().find_map(|l| {
        let (scope, key) = l.split_once('\t')?;
        matches!(scope, "local" | "worktree").then(|| key.to_string())
    }) {
        return Err(io::Error::other(crate::t!("branches.config_hook", "key" => key)));
    }
    // The folder that holds `.git` (a folder, or a file in a submodule or a linked worktree) is
    // the work tree; `core.worktree` may name another.
    let here = dunce::canonicalize(base)?;
    let Some(root) = here.ancestors().find(|a| a.join(".git").exists()) else { return Ok(()) };
    let mut top = history::git(base);
    top.args(["rev-parse", "--show-toplevel"]);
    let top = String::from_utf8_lossy(&history::run(top)?).trim().to_string();
    if dunce::canonicalize(&top).ok().as_deref() != Some(root) {
        return Err(io::Error::other(crate::t!("branches.worktree_elsewhere", "dir" => top, "root" => root.display().to_string())));
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// A test's own user config for `lfs_risk`, in place of the machine's.
    static TEST_GLOBAL: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// What git-lfs would follow from the repository itself while it writes files, when the user
/// has git-lfs set up (`filter.lfs`): in its own git config, a transfer agent or extension
/// (each a program), the ssh or askpass program, a credential helper that is a command (`!…` or
/// a path), a proxy, certificate or address override; in the `.lfsconfig` of the work tree or
/// of `target`, an LFS server that is not https.
fn lfs_risk(dir: &Path, target: &str) -> Option<String> {
    let config = |args: &[&str]| -> String {
        let mut c = crate::tools::command("git");
        c.arg("-C").arg(dir).arg("config").args(args).env("GIT_CONFIG_COUNT", "0").stdin(std::process::Stdio::null());
        #[cfg(test)]
        if let Some(g) = TEST_GLOBAL.with(|g| g.borrow().clone()) {
            c.env("GIT_CONFIG_GLOBAL", g).env("GIT_CONFIG_NOSYSTEM", "1");
        }
        c.output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
    };
    // Without git-lfs as a filter, nothing of it runs.
    if config(&["--get-regexp", r"^filter\.lfs\.(smudge|process)$"]).trim().is_empty() {
        return None;
    }
    let own = config(&[
        "--show-scope",
        "--get-regexp",
        r"^(lfs\.customtransfer\..*|lfs\.standalonetransferagent|lfs\.extension\..*|core\.(sshcommand|askpass)|credential\.(.*\.)?helper|remote\..*\.proxy|http\.(.*\.)?(proxy.*|ssl.*|curloptresolve|pinnedpubkey))$",
    ]);
    let found = own.lines().find_map(|l| {
        let (scope, rest) = l.split_once('\t')?;
        let (key, value) = rest.split_once(' ').unwrap_or((rest, ""));
        // A helper by name (`store`, `manager`) is git's own `git-credential-<name>`.
        let named_helper = key.ends_with("helper") && !value.is_empty() && !value.starts_with('!') && !value.contains(['/', '\\']);
        (matches!(scope, "local" | "worktree") && !named_helper).then(|| key.to_string())
    });
    if found.is_some() {
        return found;
    }
    let lfs_url = r"^(lfs\.(.*\.)?url|remote\..*\.lfsurl)$";
    let mut sources = vec![];
    if std::fs::symlink_metadata(dir.join(".lfsconfig")).is_ok_and(|m| m.is_file()) {
        sources.push("--file=.lfsconfig".to_string());
    }
    if !target.starts_with('-') {
        sources.push(format!("--blob={target}:.lfsconfig"));
    }
    sources.iter().find_map(|src| {
        config(&[src, "--get-regexp", lfs_url])
            .lines()
            .find(|l| l.split_once(' ').is_some_and(|(_, url)| !url.trim().starts_with("https://")))
            .map(|_| "lfs.url (.lfsconfig)".to_string())
    })
}

/// The folder git runs in for `dir`: the folder a list of branches is of, or `dir` itself.
fn base(dir: &Path) -> PathBuf {
    history::split(dir).map_or_else(|| dir.to_path_buf(), |at| at.base)
}

/// Switch to the branch `entry` names in the list of branches at `dir`; a remote one gets a
/// local branch that tracks it. git refuses when changes here would be overwritten, and its
/// message says which: it is never forced. Refused where Coxswain only reads (Jujutsu).
pub fn switch(dir: &Path, entry: &str) -> io::Result<String> {
    crate::vcs::of(&base(dir)).switch(dir, entry)
}

pub(crate) fn git_switch(dir: &Path, entry: &str) -> io::Result<String> {
    let at = history::split(dir)
        .filter(|at| at.view == View::Branches && at.commit.is_none())
        .ok_or_else(|| {
            // Rare (the apps ask first): the key as config.toml has it.
            let cfg = crate::config::Config::load().unwrap_or_default();
            io::Error::new(
                io::ErrorKind::InvalidInput,
                crate::t!("branches.switch_where", "key" => cfg.keys_shown(crate::config::Action::Branches)),
            )
        })?;
    let b = find(&at.base, entry)?;
    // A ref made by hand may start with `-`: never an option.
    if b.name.starts_with('-') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::t!("branches.bad_name", "name" => b.name),
        ));
    }
    changes_refs_safely(&at.base)?;
    writes_files_safely(&at.base, &b.name)?;
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
    crate::vcs::of(&base(dir)).create(dir, name, from)
}

pub(crate) fn git_create(dir: &Path, name: &str, from: Option<&str>) -> io::Result<String> {
    let base = base(dir);
    let name = name.trim();
    let mut check = history::git(&base);
    check.args(["check-ref-format", "--branch", name]);
    if name.is_empty() || name.starts_with('-') || history::run(check).is_err() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::t!("branches.bad_name", "name" => name),
        ));
    }
    changes_refs_safely(&base)?;
    let start = match from.filter(|e| {
        *e != ".."
            && history::split(dir)
                .is_some_and(|at| at.view == View::Branches && at.commit.is_none())
    }) {
        Some(e) => {
            let b = find(&base, e)?;
            writes_files_safely(&base, &b.name)?;
            Some(b.name)
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
            "worktree" => all.extend(w.replace(Worktree {
                path: PathBuf::from(v),
                ..Default::default()
            })),
            "HEAD" => w.iter_mut().for_each(|w| w.head = v.to_string()),
            "branch" => w.iter_mut().for_each(|w| {
                w.branch = Some(v.strip_prefix("refs/heads/").unwrap_or(v).to_string())
            }),
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
    c.args([
        "status",
        "--porcelain",
        "-unormal",
        "--ignore-submodules=dirty",
    ]);
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
            let folder = w.path.file_name().map_or_else(
                || w.path.to_string_lossy().into_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
            let head = match &w.branch {
                Some(b) => b.clone(),
                None if w.bare => crate::t!("worktrees.bare"),
                None => crate::t!("git.detached", "commit" => &w.head[..w.head.len().min(7)]),
            };
            let mut name = format!(
                "{}{folder} [{}]",
                if w.path.canonicalize().ok() == here {
                    "* "
                } else {
                    ""
                },
                head.replace('/', "∕")
            );
            if !w.bare && !w.prunable {
                match dirty(&w.path) {
                    Some(true) => name += &format!(" {}", crate::t!("worktrees.dirty")),
                    Some(false) => name += &format!(" {}", crate::t!("worktrees.clean")),
                    None => {}
                }
            }
            for (on, key) in [
                (w.locked, "worktrees.locked"),
                (w.prunable, "worktrees.prunable"),
            ] {
                if on {
                    name += &format!(" {}", crate::t!(key));
                }
            }
            let time =
                crate::fs::secs(std::fs::metadata(&w.path).and_then(|m| m.modified())).unwrap_or(0);
            dir_entry(name, w.path, time)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::{add, commit_as, repo};

    fn git(d: &Path, args: &[&str]) -> bool {
        let out = crate::tools::command("git").arg("-C").arg(d).args(args).output().unwrap();
        if !out.status.success() {
            eprintln!("git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        }
        out.status.success()
    }

    #[test]
    fn name_of_reads_entry_name_back() {
        for (name, current, ahead, behind) in [
            ("main", true, 0, 0),
            ("origin/feature/x", false, 3, 1),
            ("fix", false, 0, 2),
        ] {
            let b = Branch {
                name: name.into(),
                remote: false,
                current,
                oid: String::new(),
                time: 0,
                author: String::new(),
                subject: "A subject with spaces".into(),
                ahead,
                behind,
            };
            assert_eq!(name_of(&entry_name(&b)), name);
        }
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
        assert_eq!(
            b.iter()
                .map(|b| (b.name.as_str(), b.remote, b.current))
                .collect::<Vec<_>>(),
            [
                ("main", false, true),
                ("feature/x", false, false),
                ("origin/main", true, false)
            ]
        );
        assert_eq!(
            (b[0].ahead, b[0].behind, b[0].time, b[0].author.as_str()),
            (1, 2, 1_700_000_000, "Ada")
        );
        assert_eq!(entry_name(&b[0]), "* main ↑1 ↓2 · Fix the valve");
        assert_eq!(entry_name(&b[1]), "feature∕x · A ∕ subject");
    }

    #[test]
    fn branches_parse_worktree_porcelain() {
        let out = "worktree /r/main\0HEAD aaaa\0branch refs/heads/main\0\0worktree /r/feat\0HEAD bbbb\0detached\0locked because\0\0worktree /r/gone\0HEAD cccc\0branch refs/heads/old\0prunable gitdir file points to non-existent location\0\0";
        let w = parse_worktrees(out);
        assert_eq!(w.len(), 3);
        assert_eq!(
            (w[0].path.as_path(), w[0].branch.as_deref()),
            (Path::new("/r/main"), Some("main"))
        );
        assert_eq!(
            (w[1].branch.as_deref(), w[1].head.as_str(), w[1].locked),
            (None, "bbbb", true)
        );
        assert!(w[2].prunable && !w[2].locked);
    }

    #[test]
    fn branches_operation_from_state_files() {
        let d = std::env::temp_dir().join(format!("coxswain-op-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(crate::git::operation(&d), None);
        for (f, op) in [
            ("BISECT_LOG", "bisect"),
            ("REVERT_HEAD", "revert"),
            ("CHERRY_PICK_HEAD", "cherry_pick"),
            ("MERGE_HEAD", "merge"),
        ] {
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
        assert_eq!(
            (at.view, at.commit, at.base.as_path()),
            (View::Branches, None, d.as_path())
        );
        let at = history::split(&d.join("@branches/0123456789ab/src")).unwrap();
        assert_eq!(
            (at.view, at.commit.as_deref(), at.inner.as_str()),
            (View::Branches, Some("0123456789ab"), "src")
        );
        assert_eq!(
            history::split(&d.join("@worktrees")).unwrap().view,
            View::Worktrees
        );
        assert!(
            history::split(&d.join("@worktrees/x")).is_none(),
            "a worktree is its own folder"
        );
        assert!(
            history::split(&d.join("f/@branches")).is_none(),
            "of a folder only"
        );
        assert!(
            history::is_history(&d.join("@worktrees"))
                && history::is_history(&d.join("@branches/ab12"))
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Branches listed, browsed, created and switched to with the git CLI; a switch that would
    /// overwrite changes is refused with git's own message, and nothing is lost. Worktrees.
    /// A repository from elsewhere with an LFS file on a branch, and the user's own git-lfs: a
    /// stand-in for it, set up as `git lfs install` does, in a home of its own, that runs the
    /// repository's transfer agent as git-lfs does when the file's object is missing. git as
    /// Coxswain runs it does run the agent; a switch and a new branch from it are refused first.
    #[cfg(unix)]
    #[test]
    fn a_switch_is_refused_where_git_lfs_would_run_the_repositorys_own_program() {
        use std::os::unix::fs::PermissionsExt;
        let Some(d) = repo("lfs-risk") else { return };
        let home = d.with_file_name(format!("{}-home", d.file_name().unwrap().to_string_lossy()));
        std::fs::create_dir_all(&home).unwrap();
        let lfs = home.join("git-lfs-standin");
        std::fs::write(&lfs, "#!/bin/sh\nagent=$(git config lfs.customtransfer.evil.path)\n[ -n \"$agent\" ] && sh -c \"$agent\"\ncat\n").unwrap();
        std::fs::set_permissions(&lfs, std::fs::Permissions::from_mode(0o755)).unwrap();
        let global = home.join("gitconfig");
        std::fs::write(&global, format!("[filter \"lfs\"]\n\tsmudge = {} smudge %f\n\tclean = cat\n\trequired = true\n[user]\n\tname = Ada\n\temail = ada@example.org\n", lfs.display())).unwrap();
        // The setting up, with that home's config only (not the machine's own git-lfs).
        let g = |args: &[&str]| {
            let ok = crate::tools::command("git").arg("-C").arg(&d).args(args).env("GIT_CONFIG_GLOBAL", &global).env("GIT_CONFIG_NOSYSTEM", "1").status().unwrap().success();
            assert!(ok, "git {args:?}");
        };
        let commit = |msg: &str| {
            g(&["add", "-A"]);
            g(&["commit", "-q", "-m", msg]);
        };
        std::fs::write(d.join("a.txt"), "one\n").unwrap();
        commit("first");
        g(&["switch", "-q", "-c", "assets"]);
        std::fs::write(d.join(".gitattributes"), "*.bin filter=lfs\n").unwrap();
        std::fs::write(d.join("big.bin"), "version https://git-lfs.github.com/spec/v1\n").unwrap();
        commit("an LFS file");
        g(&["switch", "-q", "main"]);
        let marker = home.join("PWNED");
        g(&["config", "lfs.customtransfer.evil.path", &format!("touch {}", marker.display())]);
        g(&["config", "lfs.standalonetransferagent", "evil"]);

        TEST_GLOBAL.with(|g| *g.borrow_mut() = Some(global.clone()));
        // Refused before git runs.
        let list = history::path_of(&d, View::Branches);
        let e = switch(&list, "assets").unwrap_err().to_string();
        assert_eq!(e, crate::t!("branches.lfs_risk", "key" => "lfs.customtransfer.evil.path"));
        let e = create(&list, "copy", Some("assets")).unwrap_err().to_string();
        assert!(e.contains("lfs.customtransfer"), "{e}");
        assert!(!marker.exists());
        // Without the refusal, git as Coxswain runs it, with that git-lfs, runs the program.
        let mut c = history::git(&d);
        c.env("GIT_CONFIG_GLOBAL", &global).env("GIT_CONFIG_NOSYSTEM", "1").args(["switch", "-q", "assets"]);
        assert!(c.status().unwrap().success());
        assert!(marker.exists(), "the stand-in did not run the agent: the test proves nothing");
        g(&["switch", "-q", "main"]);

        // A plain http LFS server in the branch's .lfsconfig is refused; https is not.
        g(&["config", "--unset", "lfs.customtransfer.evil.path"]);
        g(&["config", "--unset", "lfs.standalonetransferagent"]);
        assert_eq!(lfs_risk(&d, "assets"), None);
        // A credential helper by name is git's own; one that is a command is not.
        g(&["config", "credential.helper", "store"]);
        assert_eq!(lfs_risk(&d, "assets"), None);
        g(&["config", "credential.helper", "!touch pwned"]);
        assert_eq!(lfs_risk(&d, "assets").as_deref(), Some("credential.helper"));
        g(&["config", "--unset", "credential.helper"]);
        g(&["switch", "-q", "assets"]);
        std::fs::write(d.join(".lfsconfig"), "[lfs]\n\turl = http://192.0.2.1/lfs\n").unwrap();
        commit("lfsconfig");
        g(&["switch", "-q", "main"]);
        assert_eq!(switch(&list, "assets").unwrap_err().to_string(), crate::t!("branches.lfs_risk", "key" => "lfs.url (.lfsconfig)"));
        // In the work tree too.
        std::fs::write(d.join(".lfsconfig"), "[lfs]\n\turl = git@192.0.2.1:lfs\n").unwrap();
        assert!(lfs_risk(&d, "main").is_some());
        std::fs::remove_file(d.join(".lfsconfig")).unwrap();
        g(&["switch", "-q", "assets"]);
        std::fs::write(d.join(".lfsconfig"), "[lfs]\n\turl = https://example.org/lfs\n").unwrap();
        commit("https");
        g(&["switch", "-q", "main"]);
        assert_eq!(lfs_risk(&d, "assets"), None);
        // Without git-lfs as a filter, nothing of it runs: nothing to refuse.
        std::fs::write(&global, "").unwrap();
        g(&["config", "lfs.standalonetransferagent", "evil"]);
        assert_eq!(lfs_risk(&d, "assets"), None);
        TEST_GLOBAL.with(|g| *g.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A hook set in the repository's own config (`hook.<name>.command`), which `core.hooksPath`
    /// does not stop, and a `core.worktree` that points outside: a switch and a new branch are
    /// refused before git runs.
    #[test]
    fn a_switch_is_refused_where_the_repository_sets_a_hook_or_a_work_tree_elsewhere() {
        let Some(d) = repo("config-hook") else { return };
        let g = |args: &[&str]| {
            let ok = crate::tools::command("git")
                .arg("-C")
                .arg(&d)
                .args(["-c", "user.name=Ada", "-c", "user.email=ada@example.org"])
                .args(args)
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?}");
        };
        std::fs::write(d.join("a.txt"), "one\n").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-q", "-m", "first"]);
        g(&["branch", "other"]);
        let marker = d.with_file_name(format!("{}-PWNED", d.file_name().unwrap().to_string_lossy()));
        let _ = std::fs::remove_file(&marker);
        g(&["config", "hook.x.command", &format!("touch {}", marker.display())]);
        g(&["config", "--add", "hook.x.event", "post-checkout"]);
        g(&["config", "--add", "hook.x.event", "reference-transaction"]);
        let list = history::path_of(&d, View::Branches);
        let e = switch(&list, "other").unwrap_err().to_string();
        assert!(e.contains("hook.x."), "{e}");
        assert!(create(&list, "new", None).is_err());
        assert!(create(&d, "new", None).is_err());
        assert!(!marker.exists(), "a hook from the repository's config ran");
        // The premise, where git runs hooks from config (2.46 and later): git as Coxswain runs
        // it, without the refusal, does run it; core.hooksPath=/dev/null does not stop it.
        let mut c = history::git(&d);
        c.args(["switch", "-q", "other"]);
        assert!(c.status().unwrap().success());
        eprintln!("this git runs hooks from config: {}", marker.exists());
        let _ = std::fs::remove_file(&marker);
        g(&["switch", "-q", "main"]);
        g(&["config", "--remove-section", "hook.x"]);

        // core.worktree pointing outside the folder.
        let outside = d.with_file_name(format!("{}-outside", d.file_name().unwrap().to_string_lossy()));
        std::fs::create_dir_all(&outside).unwrap();
        g(&["config", "core.worktree", &outside.display().to_string()]);
        let e = switch(&list, "other").unwrap_err().to_string();
        assert!(e.contains(&*outside.file_name().unwrap().to_string_lossy()), "{e}");
        assert!(create(&d, "new2", None).is_err());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0, "a checkout wrote outside");
        g(&["config", "--unset", "core.worktree"]);
        switch(&list, "other").unwrap();
        let _ = std::fs::remove_dir_all(&outside);
        let _ = std::fs::remove_dir_all(&d);
    }

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
        assert!(git(
            &d,
            &["update-ref", "refs/remotes/origin/main", "HEAD~1"]
        ));

        let list = crate::fs::list(&history::path_of(&d, View::Branches), true).unwrap();
        let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "..",
                "feature∕x · first",
                "* main · second",
                "origin∕main · first"
            ]
        );
        assert_eq!(
            (list[0].path.as_path(), list[2].modified),
            (d.as_path(), 1_700_100_000)
        );
        // Two branches on one commit: each a path of its own, both leading to that commit.
        assert_ne!(list[1].path, list[3].path);
        assert_eq!(crate::fs::list(&list[3].path, true).unwrap().len(), 2);
        // Into a branch: its tree at its commit, read-only.
        let tree = crate::fs::list(&list[1].path, true).unwrap();
        assert_eq!(
            tree.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["..", "a.txt"]
        );
        assert_eq!(
            history::read(&history::split(&list[1].path.join("a.txt")).unwrap(), 100)
                .unwrap()
                .0,
            b"one\n"
        );
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
        assert_eq!(
            std::fs::read_to_string(d.join("a.txt")).unwrap().trim_end(),
            "one"
        );
        // A remote one gets a local branch that tracks it: here `main` is there already.
        assert!(
            switch(&branches, &list[3].name)
                .unwrap_err()
                .to_string()
                .contains("main")
        );

        // New branches: from the current commit, and from a branch in the list.
        assert!(create(&d, "-x", None).is_err() && create(&d, "a b", None).is_err());
        create(&d, "new", None).unwrap();
        assert!(
            super::branches(&d)
                .unwrap()
                .iter()
                .any(|b| b.current && b.name == "new")
        );
        create(&branches, "from-main", Some(&list[2].name)).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join("a.txt")).unwrap().trim_end(),
            "two"
        );

        // A linked worktree, detached.
        let wt = d.with_file_name(format!("{}-wt", d.file_name().unwrap().to_string_lossy()));
        let _ = std::fs::remove_dir_all(&wt);
        assert!(git(
            &d,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                &wt.to_string_lossy(),
                "main"
            ]
        ));
        std::fs::write(wt.join("b.txt"), "").unwrap();
        let list = crate::fs::list(&history::path_of(&d, View::Worktrees), true).unwrap();
        assert_eq!(list.len(), 3);
        assert!(
            list[1].name.starts_with("* ")
                && list[1].name.contains("[from-main]")
                && list[1].name.contains("clean"),
            "{}",
            list[1].name
        );
        assert!(
            list[2].name.contains("detached at") && list[2].name.contains("dirty"),
            "{}",
            list[2].name
        );
        assert_eq!(
            list[2].path.canonicalize().unwrap(),
            wt.canonicalize().unwrap()
        );
        let st = crate::git::Status::read(&wt).unwrap();
        assert_eq!(
            st.summary.worktree.as_deref(),
            wt.file_name().and_then(|n| n.to_str())
        );
        assert!(
            st.prompt(&crate::config::Glyphs::ascii())
                .contains("detached at")
        );
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
        assert!(!git(
            &d,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "merge",
                "-q",
                "other"
            ]
        ));
        let st = crate::git::Status::read(&d).unwrap();
        assert_eq!(st.summary.operation, Some("merge"));
        assert!(
            st.prompt(&crate::config::Glyphs::ascii())
                .ends_with("merging"),
            "{}",
            st.prompt(&crate::config::Glyphs::ascii())
        );
        std::fs::remove_dir_all(d).unwrap();
    }
}

//! Fetch and Pull: besides a Subversion or CVS history read after a yes, the only things Coxswain
//! asks of a repository's remote, and only when the user picks them: git `fetch --all --prune`,
//! Mercurial `pull`, Jujutsu `git fetch`; and a
//! pull that only fast-forwards (git `merge --ff-only` after the fetch, Mercurial `update` when
//! the branch has one head, Subversion and CVS `update`). Nothing is merged or rebased, and a
//! pull that would touch uncommitted changes is refused before anything is written. The tool
//! never asks on the terminal behind the app: no prompt (`GIT_TERMINAL_PROMPT=0`,
//! `--noninteractive`, `--non-interactive`) and, on Unix, a session of its own, so ssh finds
//! no terminal to ask a passphrase on; its configured helpers and ssh-agent still work, and
//! when they cannot, the tool's own words come back as the error.

use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::t;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Fetch,
    Pull,
}

/// Whether the system fetches without changing the working copy (git, Jujutsu, Mercurial):
/// Subversion and CVS only update, so they get Pull alone. `system` is `Summary::system`.
pub fn fetches(system: &str) -> bool {
    matches!(system, "" | "git" | "jj" | "hg")
}

/// Fetch or pull the repository `dir` is in (`dir` may be its list of branches). `stop` set
/// (Esc) ends the tool; `at` gets the last line of its progress. What came of it, in words, or
/// why not: the tool's own reason under ours.
pub fn run(dir: &Path, op: Op, stop: &AtomicBool, at: &mut dyn FnMut(&str)) -> io::Result<String> {
    let base = crate::history::split(dir).map_or_else(|| dir.to_path_buf(), |a| a.base);
    match crate::vcs::of(&base).name() {
        "jj" => jj(&base, op, stop, at),
        "hg" => hg(&base, op, stop, at),
        "svn" | "cvs" if op == Op::Fetch => Err(io::Error::new(io::ErrorKind::Unsupported, t!("remote.no_fetch"))),
        "svn" => svn(&base, stop, at),
        "cvs" => cvs(&base, stop, at),
        _ => git(&base, op, stop, at),
    }
}

// ---------------------------------------------------------------- running

/// What a tool printed: stdout and stderr, each its last 64 KB.
struct Said {
    ok: bool,
    out: String,
    err: String,
}

impl Said {
    /// Its complaint: the lines that say what failed (`fatal:`, `error:`, `abort:`, `svn: E…`)
    /// first, as the apps show the first line, then all it said on stderr (else stdout).
    fn why(&self) -> String {
        let all = [&self.err, &self.out].into_iter().map(|s| s.trim()).find(|s| !s.is_empty()).unwrap_or("failed");
        let bad: Vec<&str> = all
            .lines()
            .filter(|l| ["fatal:", "error:", "abort:", "svn: E", "cvs [", "Error:"].iter().any(|p| l.trim_start().starts_with(p)))
            .collect();
        if bad.is_empty() { all.to_string() } else { format!("{}\n{all}", bad.join("\n")) }
    }
    fn into_result(self) -> io::Result<Said> {
        if self.ok { Ok(self) } else { Err(io::Error::other(self.why())) }
    }
}

/// Kept of each stream.
const KEEP: usize = 64 << 10;

/// Runs `c` to its end, or until `stop`: never on the terminal (no stdin, no prompt, a session
/// of its own), its progress lines to `at`.
fn tool(mut c: Command, stop: &AtomicBool, at: &mut dyn FnMut(&str)) -> io::Result<Said> {
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A credential helper or ssh's askpass may still ask in a window of its own.
        .env("GIT_TERMINAL_PROMPT", "0");
    #[cfg(unix)]
    unsafe {
        // No controlling terminal: ssh cannot ask for a passphrase or a host key on the one the
        // terminal app draws on. setsid is async-signal-safe.
        std::os::unix::process::CommandExt::pre_exec(&mut c, || {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = c.spawn()?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let reader = |r: Option<Box<dyn Read + Send>>, tx: std::sync::mpsc::Sender<String>| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let Some(r) = r else { return String::new() };
            let mut r = io::BufReader::new(r);
            let mut line = Vec::new();
            // git ends a progress line with `\r`, the rest with `\n`.
            loop {
                line.clear();
                let mut b = [0u8];
                let mut n = 0;
                while r.read(&mut b).unwrap_or(0) == 1 {
                    n += 1;
                    kept.push(b[0]);
                    if b[0] == b'\n' || b[0] == b'\r' {
                        break;
                    }
                    line.push(b[0]);
                }
                if kept.len() > 2 * KEEP {
                    kept.drain(..kept.len() - KEEP);
                }
                if n == 0 {
                    break;
                }
                let l = String::from_utf8_lossy(&line).trim().to_string();
                if !l.is_empty() {
                    let _ = tx.send(l);
                }
            }
            let start = kept.len().saturating_sub(KEEP);
            String::from_utf8_lossy(&kept[start..]).replace('\r', "\n")
        })
    };
    let out = reader(child.stdout.take().map(|s| Box::new(s) as _), tx.clone());
    let err = reader(child.stderr.take().map(|s| Box::new(s) as _), tx);
    let status = loop {
        while let Ok(l) = rx.try_recv() {
            at(&l);
        }
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::Interrupted, t!("remote.stopped")));
        }
        match child.try_wait()? {
            Some(s) => break s,
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    Ok(Said { ok: status.success(), out, err })
}

/// `ours`, with the tool's words under it.
fn with_reason(ours: String, theirs: &str) -> io::Error {
    let theirs = theirs.trim();
    io::Error::other(if theirs.is_empty() { ours } else { format!("{ours}\n{theirs}") })
}

/// The files a pull would change that hold uncommitted changes: refused, with their names.
fn touched(files: &[String]) -> io::Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let mut list = files.iter().take(10).cloned().collect::<Vec<_>>().join(", ");
    if files.len() > 10 {
        list += ", …";
    }
    Err(io::Error::other(t!("remote.would_touch", "files" => list)))
}

// ---------------------------------------------------------------- git

/// git as Coxswain runs it (`git::command`), let reach the remote: the transports git knows
/// by name only (never `ext::`, which runs a command), no prompt, no maintenance after.
fn git_remote(base: &Path) -> Command {
    let mut c = crate::history::git(base);
    c.args(["-c", "protocol.allow=never"]);
    for p in ["https", "http", "ssh", "git", "file"] {
        c.args(["-c", &format!("protocol.{p}.allow=always")]);
    }
    c.args(["-c", "maintenance.auto=false", "-c", "gc.auto=0", "-c", "fetch.recurseSubmodules=false"]);
    c
}

/// git's output, or its complaint as the error (local commands, no remote).
fn git_local(base: &Path, args: &[&str]) -> io::Result<String> {
    let mut c = crate::history::git(base);
    c.args(args);
    let o = c.output()?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        Err(io::Error::other(String::from_utf8_lossy(&o.stderr).trim().to_string()))
    }
}

/// Settings of the repository's own config that name a program git would run to reach the
/// remote: a repository from elsewhere (a download, an archive) could run anything by them.
/// The user's and the system's config are theirs and hold.
pub(crate) fn git_repo_commands(base: &Path) -> Vec<String> {
    let mut c = crate::tools::command("git");
    c.arg("-C").arg(base).args([
        "config",
        "--show-scope",
        "--name-only",
        "--get-regexp",
        r"^(core\.(sshcommand|gitproxy|askpass)|remote\..*\.(uploadpack|receivepack)|credential\..*helper|credential\.helper)$",
    ]);
    c.env("GIT_CONFIG_COUNT", "0").stdin(Stdio::null());
    let Ok(o) = c.output() else { return vec![] };
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|l| {
            let (scope, key) = l.split_once('\t')?;
            matches!(scope, "local" | "worktree").then(|| key.to_string())
        })
        .collect()
}

fn git_fetch(base: &Path, stop: &AtomicBool, at: &mut dyn FnMut(&str)) -> io::Result<()> {
    if let Some(key) = git_repo_commands(base).first() {
        return Err(io::Error::other(t!("remote.repo_command", "key" => key)));
    }
    if git_local(base, &["remote"])?.is_empty() {
        return Err(io::Error::other(t!("remote.no_remote")));
    }
    let mut c = git_remote(base);
    c.args(["fetch", "--all", "--prune", "--progress", "--no-recurse-submodules"]);
    tool(c, stop, at)?.into_result().map(drop)
}

fn git(base: &Path, op: Op, stop: &AtomicBool, at: &mut dyn FnMut(&str)) -> io::Result<String> {
    if op == Op::Fetch {
        git_fetch(base, stop, at)?;
        let st = crate::git::Status::read_git(base).unwrap_or_default();
        let s = &st.summary;
        return Ok(match &s.upstream {
            Some(u) => t!("remote.fetched", "upstream" => u, "behind" => crate::i18n::digits(s.behind.to_string()), "ahead" => crate::i18n::digits(s.ahead.to_string())),
            None => t!("remote.fetched_no_upstream"),
        });
    }
    // Pull: the upstream first, before anything is fetched.
    let upstream = git_local(base, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
        .map_err(|e| with_reason(t!("remote.no_upstream"), &e.to_string()))?;
    git_fetch(base, stop, at)?;
    let head = git_local(base, &["rev-parse", "HEAD"])?;
    let up = git_local(base, &["rev-parse", "@{u}"])?;
    if head == up {
        return Ok(t!("remote.up_to_date", "upstream" => upstream));
    }
    let is_ancestor = |a: &str, b: &str| git_local(base, &["merge-base", "--is-ancestor", a, b]).is_ok();
    if is_ancestor(&up, &head) {
        return Ok(t!("remote.ahead_only", "upstream" => upstream));
    }
    if !is_ancestor(&head, &up) {
        return Err(io::Error::other(t!("remote.diverged", "upstream" => upstream)));
    }
    // The files the fast-forward writes, against the ones with changes not committed.
    let changing: std::collections::HashSet<String> =
        git_local(base, &["diff", "--name-only", "-z", &head, &up])?.split('\0').filter(|s| !s.is_empty()).map(String::from).collect();
    let dirty = git_local(base, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    let mut hit = vec![];
    let mut parts = dirty.split('\0');
    while let Some(e) = parts.next() {
        if e.len() < 4 {
            continue;
        }
        let renamed = matches!(&e[..1], "R" | "C");
        let path = &e[3..];
        if changing.contains(path) {
            hit.push(path.to_string());
        }
        if renamed && let Some(from) = parts.next() && changing.contains(from) {
            hit.push(from.to_string());
        }
    }
    touched(&hit)?;
    crate::branches::writes_files_safely(base)?;
    let mut c = crate::history::git(base);
    c.args(["merge", "--ff-only", "--no-autostash", "--no-stat", "--no-edit", "@{u}"]);
    tool(c, stop, at)?.into_result().map_err(|e| with_reason(t!("remote.pull_failed"), &e.to_string()))?;
    Ok(t!("remote.pulled", "upstream" => upstream, "from" => &head[..7.min(head.len())], "to" => &up[..7.min(up.len())]))
}

// ---------------------------------------------------------------- Mercurial

/// The repository's `paths.default`, read from its own `.hg/hgrc` (which hg is told never to
/// read, `vcs::hg`), the one place hg keeps where it was cloned from.
fn hg_default(root: &Path) -> Option<String> {
    let f = std::fs::File::open(root.join(".hg/hgrc")).ok()?;
    let mut in_paths = false;
    for line in io::BufReader::new(f.take(1 << 20)).lines() {
        let line = line.ok()?;
        let l = line.trim();
        if l.starts_with('[') {
            in_paths = l == "[paths]";
        } else if in_paths && let Some((k, v)) = l.split_once('=') && k.trim() == "default" {
            let v = v.trim();
            return (!v.is_empty() && !v.starts_with('-')).then(|| v.to_string());
        }
    }
    None
}

fn hg(base: &Path, op: Op, stop: &AtomicBool, at: &mut dyn FnMut(&str)) -> io::Result<String> {
    crate::vcs::need_hg()?;
    let root = crate::vcs::hg_root(base).unwrap_or_else(|| base.to_path_buf());
    let source = hg_default(&root).ok_or_else(|| io::Error::other(t!("remote.no_remote")))?;
    let local = |args: &[&str]| -> io::Result<String> {
        let mut c = crate::vcs::hg(&root);
        c.args(args);
        Ok(String::from_utf8_lossy(&crate::history::run(c)?).trim().to_string())
    };
    if op == Op::Pull {
        let dirty: Vec<String> = local(&["status", "-mard", "-n"])?.lines().map(String::from).collect();
        touched(&dirty)?;
        crate::vcs::hg_writes_files_safely(&root)?;
    }
    let mut c = crate::vcs::hg(&root);
    c.args(["pull", "--", &source]);
    tool(c, stop, at)?.into_result()?;
    if op == Op::Fetch {
        return Ok(t!("remote.fetched_hg"));
    }
    let heads = local(&["heads", "-T", "{node|short}\n", "."])?;
    let heads: Vec<&str> = heads.lines().collect();
    if heads.len() > 1 {
        return Err(io::Error::other(t!("remote.hg_heads", "n" => heads.len().to_string())));
    }
    let mut c = crate::vcs::hg(&root);
    // Never a subrepository fetched from wherever `.hgsub` names.
    c.args(["--config", "subrepos.allowed=false", "update", "--check"]);
    tool(c, stop, at)?.into_result().map_err(|e| with_reason(t!("remote.pull_failed"), &e.to_string()))?;
    Ok(t!("remote.pulled_to", "rev" => local(&["log", "-r", ".", "-T", "{node|short}"])?))
}

// ---------------------------------------------------------------- Jujutsu

fn jj(base: &Path, op: Op, stop: &AtomicBool, at: &mut dyn FnMut(&str)) -> io::Result<String> {
    let root = crate::vcs::jj_root(base).unwrap_or_else(|| base.to_path_buf());
    if !crate::vcs::jj_here(&root) {
        return Err(io::Error::other(t!("remote.jj_not_here")));
    }
    let mut c = crate::vcs::jj_writing(&root);
    c.args(["git", "fetch"]);
    tool(c, stop, at)?.into_result()?;
    Ok(match op {
        Op::Fetch => t!("remote.fetched_jj"),
        // Jujutsu moves the tracked bookmarks itself; the working copy stays where it is, as
        // moving it onto them is a rebase.
        Op::Pull => t!("remote.pulled_jj"),
    })
}

// ---------------------------------------------------------------- Subversion and CVS

fn svn(base: &Path, stop: &AtomicBool, at: &mut dyn FnMut(&str)) -> io::Result<String> {
    crate::wc::need("svn", "Subversion", &crate::wc::SVN_MISSING)?;
    let root = crate::wc::svn_root(base).unwrap_or_else(|| base.to_path_buf());
    // Asks the server which files are out of date (`*` in the ninth column): one with changes
    // here too would be merged into.
    let mut c = crate::wc::svn();
    c.current_dir(&root).args(["status", "--show-updates", "--quiet", "--ignore-externals"]);
    let st = tool(c, stop, at)?.into_result()?;
    touched(&parse_svn_out_of_date(&st.out))?;
    let mut c = crate::wc::svn();
    c.current_dir(&root).args(["update", "--accept", "postpone"]);
    let said = tool(c, stop, at)?.into_result()?;
    Ok(said.out.trim().lines().last().unwrap_or_default().to_string())
}

/// The paths of `svn status --show-updates --quiet` that have changes here and a newer version
/// on the server.
pub fn parse_svn_out_of_date(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| {
            let b = l.as_bytes();
            let local = b.first().is_some_and(|c| b"MADRC~!".contains(c)) || b.get(1) == Some(&b'M');
            (b.len() > 9 && b[8] == b'*' && local).then(|| l[9..].split_whitespace().last().unwrap_or_default().to_string())
        })
        .collect()
}

fn cvs(base: &Path, stop: &AtomicBool, at: &mut dyn FnMut(&str)) -> io::Result<String> {
    crate::wc::need("cvs", "CVS", &crate::wc::CVS_MISSING)?;
    let root = crate::wc::cvs_root(base).unwrap_or_else(|| base.to_path_buf());
    let dir = crate::wc::cvs_dir(&root).ok_or_else(|| io::Error::other(t!("remote.no_remote")))?;
    let (_, cvsroot) = crate::wc::cvs_root_of(&dir).ok_or_else(|| io::Error::other(t!("remote.no_remote")))?;
    // A folder's `CVS/Update.prog` is a program `cvs update` runs: one from elsewhere could be
    // anything.
    if has_update_prog(&root) {
        return Err(io::Error::other(t!("remote.repo_command", "key" => "CVS/Update.prog")));
    }
    let cvs = || {
        let mut c = crate::tools::command("cvs");
        c.current_dir(&root).args(["-f", "-q", "-d", &cvsroot]);
        c
    };
    // What an update would do, without doing it: `M` and `C` are files changed here.
    let mut c = cvs();
    c.args(["-n", "update"]);
    let plan = tool(c, stop, at)?;
    let dirty: Vec<String> = plan
        .out
        .lines()
        .filter_map(|l| l.strip_prefix("M ").or_else(|| l.strip_prefix("C ")).map(String::from))
        .collect();
    touched(&dirty)?;
    let mut c = cvs();
    c.args(["update", "-d"]);
    let said = tool(c, stop, at)?.into_result()?;
    let n = said.out.lines().filter(|l| l.starts_with("U ") || l.starts_with("P ")).count();
    Ok(t!("remote.updated_cvs", "n" => crate::i18n::digits(n.to_string())))
}

/// Whether a folder of the CVS working copy at `root` names a program for `cvs update`.
fn has_update_prog(root: &Path) -> bool {
    let mut todo: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(d) = todo.pop() {
        if d.join("CVS/Update.prog").exists() {
            return true;
        }
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) && e.path().join("CVS").is_dir() {
                    todo.push(e.path());
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_in(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=Ada", "-c", "user.email=ada@example.org", "-c", "init.defaultBranch=main", "-c", "protocol.file.allow=always"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn commit(dir: &Path, file: &str, text: &str) {
        std::fs::write(dir.join(file), text).unwrap();
        git_in(dir, &["add", "-A"]);
        git_in(dir, &["commit", "-q", "-m", file]);
    }

    fn head(dir: &Path) -> String {
        git_local(dir, &["rev-parse", "HEAD"]).unwrap()
    }

    /// A bare remote, a clone of it (`here`), and a second clone that pushes (`there`).
    fn setup(name: &str) -> Option<(crate::fs::Scratch, PathBuf, PathBuf)> {
        if !crate::user_menu::installed("git") {
            return None;
        }
        let t = crate::fs::Scratch::named(&format!("remote-{name}"));
        let (bare, here, there) = (t.join("remote.git"), t.join("here"), t.join("there"));
        git_in(&t, &["init", "-q", "--bare", "remote.git"]);
        git_in(&t, &["clone", "-q", bare.to_str().unwrap(), "there"]);
        commit(&there, "a.txt", "one\n");
        git_in(&there, &["push", "-q", "origin", "HEAD:main"]);
        git_in(&t, &["clone", "-q", bare.to_str().unwrap(), "here"]);
        Some((t, here, there))
    }

    fn go(dir: &Path, op: Op) -> io::Result<String> {
        run(dir, op, &AtomicBool::new(false), &mut |_| {})
    }

    #[test]
    fn fetch_brings_a_new_commit_and_pull_moves_the_branch() {
        let Some((_t, here, there)) = setup("ff") else { return };
        let before = head(&here);
        commit(&there, "b.txt", "two\n");
        git_in(&there, &["push", "-q", "origin", "HEAD:main"]);
        let said = go(&here, Op::Fetch).unwrap();
        assert!(!said.is_empty());
        assert_eq!(head(&here), before, "a fetch moves no branch");
        assert_eq!(git_local(&here, &["rev-parse", "origin/main"]).unwrap(), head(&there));
        assert!(here.join(".git/FETCH_HEAD").exists(), "Last fetch has a time");
        go(&here, Op::Pull).unwrap();
        assert_eq!(head(&here), head(&there));
        // Windows runners check files out with CRLF (core.autocrlf in the system config).
        assert_eq!(std::fs::read_to_string(here.join("b.txt")).unwrap().trim_end(), "two");
        // Again: nothing to pull.
        go(&here, Op::Pull).unwrap();
    }

    #[test]
    fn a_pull_that_cannot_fast_forward_is_refused_with_nothing_changed() {
        let Some((_t, here, there)) = setup("nonff") else { return };
        commit(&there, "b.txt", "theirs\n");
        git_in(&there, &["push", "-q", "origin", "HEAD:main"]);
        commit(&here, "c.txt", "mine\n");
        let before = head(&here);
        let e = go(&here, Op::Pull).unwrap_err().to_string();
        assert_eq!(e, t!("remote.diverged", "upstream" => "origin/main"));
        assert_eq!(head(&here), before);
        assert!(!here.join("b.txt").exists());
    }

    #[test]
    fn a_pull_that_would_touch_uncommitted_changes_is_refused() {
        let Some((_t, here, there)) = setup("dirty") else { return };
        commit(&there, "a.txt", "theirs\n");
        git_in(&there, &["push", "-q", "origin", "HEAD:main"]);
        std::fs::write(here.join("a.txt"), "my edit\n").unwrap();
        let before = head(&here);
        let e = go(&here, Op::Pull).unwrap_err().to_string();
        assert!(e.contains("a.txt"), "{e}");
        assert_eq!(head(&here), before);
        assert_eq!(std::fs::read_to_string(here.join("a.txt")).unwrap(), "my edit\n");
        // A change to another file is left alone and the pull goes ahead.
        std::fs::write(here.join("a.txt"), "one\n").unwrap();
        std::fs::write(here.join("new.txt"), "untracked\n").unwrap();
        go(&here, Op::Pull).unwrap();
        assert_eq!(head(&here), head(&there));
        assert_eq!(std::fs::read_to_string(here.join("new.txt")).unwrap(), "untracked\n");
    }

    #[test]
    fn no_upstream_and_no_remote_say_so() {
        let Some((t, here, _there)) = setup("noup") else { return };
        git_in(&here, &["switch", "-q", "-c", "local-only"]);
        let e = go(&here, Op::Pull).unwrap_err().to_string();
        assert!(e.starts_with(&t!("remote.no_upstream")), "{e}");
        let lone = t.join("lone");
        std::fs::create_dir(&lone).unwrap();
        git_in(&lone, &["init", "-q"]);
        commit(&lone, "a.txt", "x\n");
        assert_eq!(go(&lone, Op::Fetch).unwrap_err().to_string(), t!("remote.no_remote"));
    }

    #[test]
    fn a_repository_naming_its_own_ssh_command_is_not_fetched() {
        let Some((_t, here, _there)) = setup("sshcmd") else { return };
        git_in(&here, &["config", "core.sshCommand", "touch pwned"]);
        let e = go(&here, Op::Fetch).unwrap_err().to_string();
        assert!(e.contains("core.sshcommand"), "{e}");
    }

    #[test]
    fn stop_ends_the_tool() {
        let Some((_t, here, _there)) = setup("stop") else { return };
        let e = run(&here, Op::Fetch, &AtomicBool::new(true), &mut |_| {}).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn svn_out_of_date_lines() {
        let out = "M       *        5   src/a.c\n        *        5   src/b.c\nM                5   src/c.c\nStatus against revision:      7\n";
        assert_eq!(parse_svn_out_of_date(out), vec!["src/a.c".to_string()]);
    }

    #[test]
    fn hg_fetch_and_pull() {
        if !crate::user_menu::installed("hg") {
            return;
        }
        let t = crate::fs::Scratch::named("remote-hg");
        let hg_in = |dir: &Path, args: &[&str]| {
            let ok = Command::new("hg")
                .current_dir(dir)
                .args(args)
                .env("HOME", &*t)
                .env("HGRCPATH", "")
                .env("HGUSER", "Ada <ada@example.org>")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success();
            assert!(ok, "hg {args:?}");
        };
        let (a, b) = (t.join("a"), t.join("b"));
        hg_in(&t, &["init", "a"]);
        std::fs::write(a.join("f.txt"), "one\n").unwrap();
        hg_in(&a, &["commit", "-A", "-m", "one"]);
        hg_in(&t, &["clone", "-q", "a", "b"]);
        std::fs::write(a.join("f.txt"), "two\n").unwrap();
        hg_in(&a, &["commit", "-m", "two"]);
        go(&b, Op::Fetch).unwrap();
        assert_eq!(std::fs::read_to_string(b.join("f.txt")).unwrap(), "one\n", "a fetch updates nothing");
        // Dirty: refused.
        std::fs::write(b.join("f.txt"), "mine\n").unwrap();
        assert!(go(&b, Op::Pull).unwrap_err().to_string().contains("f.txt"));
        std::fs::write(b.join("f.txt"), "one\n").unwrap();
        go(&b, Op::Pull).unwrap();
        assert_eq!(std::fs::read_to_string(b.join("f.txt")).unwrap(), "two\n");
        // A commit here and one there: two heads, refused.
        std::fs::write(a.join("g.txt"), "theirs\n").unwrap();
        hg_in(&a, &["commit", "-A", "-m", "theirs"]);
        std::fs::write(b.join("h.txt"), "mine\n").unwrap();
        hg_in(&b, &["commit", "-A", "-m", "mine"]);
        let e = go(&b, Op::Pull).unwrap_err().to_string();
        assert_eq!(e, t!("remote.hg_heads", "n" => "2"));
        assert!(!b.join("g.txt").exists());
    }
}

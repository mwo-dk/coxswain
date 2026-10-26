//! Fetch and Pull: besides a Subversion or CVS history read after a yes, the only things Coxswain
//! asks of a repository's remote, and only when the user picks them: git `fetch --prune` of the
//! branch's remote, Mercurial `pull`, Jujutsu `git fetch`; and a pull that only fast-forwards (git
//! `merge --ff-only` after the fetch, Mercurial `update` when the branch has one head, Subversion
//! and CVS `update`). Nothing is merged or rebased; a pull that would touch uncommitted changes is
//! refused before the work tree is written (the fetch before it is kept).
//!
//! A repository may come from anywhere, so its own config is not trusted to reach the network:
//! only the transports git knows by name (`GIT_ALLOW_PROTOCOL`, never `ext::`), and a repository
//! whose own config names a program to run, a proxy or other `http.*` settings, LFS agents, or a
//! fetch refspec that writes outside `refs/remotes/<remote>/` is refused. The tool never asks on
//! the terminal behind the app: no prompt (`GIT_TERMINAL_PROMPT=0`, `--noninteractive`,
//! `--non-interactive`), on Unix a session of its own (ssh finds no terminal; Esc ends the whole
//! session), on Windows a detached process; configured helpers and ssh-agent still work, and when
//! they cannot, the tool's own words come back as the error.

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
        .env("GIT_TERMINAL_PROMPT", "0")
        // Overrides every `protocol.*` setting, a repository's own too (jj's git as well).
        .env("GIT_ALLOW_PROTOCOL", ALLOWED);
    #[cfg(unix)]
    unsafe {
        // No controlling terminal: ssh cannot ask for a passphrase or a host key on the one the
        // terminal app draws on; and a process group of its own, so Stop ends ssh and
        // git-remote-https with it. setsid is async-signal-safe.
        std::os::unix::process::CommandExt::pre_exec(&mut c, || {
            libc::setsid();
            Ok(())
        });
    }
    // Not on the terminal app's console, which ssh would ask on.
    #[cfg(windows)]
    {
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        std::os::windows::process::CommandExt::creation_flags(&mut c, DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
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
            // The whole session setsid made: its id is the child's.
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            // ponytail: on Windows only the tool itself is ended; its helpers end when their pipes
            // close. A Job object would end them at once.
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

/// The transports a fetch may use: never `ext::` (a command) or a remote helper of another name.
const ALLOWED: &str = "https:http:ssh:git:file";

/// git in `base` as Coxswain runs it (`git::command`), but without first asking git for the
/// repository's filters: every git here goes through `tool`, so Esc stops it, whatever a hostile
/// `[include]` (a FIFO) does. A pull writes files only once `repo_config` found no filters.
fn git_plain(base: &Path) -> Command {
    let mut c = crate::git::command_with(base, &[]);
    crate::vcs::use_store(&mut c, base);
    c.env("GIT_LITERAL_PATHSPECS", "1");
    c
}

/// git's output, or its complaint as the error.
fn git_local(base: &Path, args: &[&str], stop: &AtomicBool) -> io::Result<String> {
    let mut c = git_plain(base);
    c.args(args);
    Ok(tool(c, stop, &mut |_| {})?.into_result()?.out.trim().to_string())
}

/// The repository's own settings (not the user's or the system's, which are theirs) that would
/// let a repository from elsewhere act when it is fetched: a program to run (ssh, proxy,
/// askpass, alternate refs, upload-pack, a credential helper, LFS agents and extensions), its
/// own `http.*` (a proxy, certificates: a man in the middle gets the credential helper's token)
/// or proxy; and its filters, which a pull would have to run. `git_dir`: the git store to read
/// (Jujutsu's), else the one git finds in `base`. Scope and key, lower case.
const RISKY: &str = r"^(core\.(sshcommand|gitproxy|askpass|alternaterefscommand)|remote\..*\.(uploadpack|receivepack|proxy)|credential\.(.*\.)?helper|http\..*|lfs\.customtransfer\..*\.(path|args)|lfs\.standalonetransferagent|lfs\.extension\..*\.(clean|smudge)|filter\..*)$";

fn repo_config(base: &Path, git_dir: Option<&Path>, regex: &str, names_only: bool, stop: &AtomicBool) -> io::Result<Vec<(String, String)>> {
    let mut c = crate::tools::command("git");
    c.arg("-C").arg(base).args(["config", "--show-scope"]);
    if names_only {
        c.arg("--name-only");
    }
    c.args(["--get-regexp", regex]).env("GIT_CONFIG_COUNT", "0");
    if let Some(d) = git_dir {
        c.env("GIT_DIR", d);
    }
    // No match is exit 1: what it printed is the answer either way.
    let said = tool(c, stop, &mut |_| {})?;
    Ok(said
        .out
        .lines()
        .filter_map(|l| {
            let (scope, rest) = l.split_once('\t')?;
            matches!(scope, "local" | "worktree").then(|| (scope.to_string(), rest.to_string()))
        })
        .collect())
}

/// Refuses a repository whose own config makes fetching from it unsafe (`RISKY`, filters aside
/// unless `writes`), or whose fetch refspecs write outside `refs/remotes/<remote>/`: a mapping
/// into `refs/heads/*` or `refs/tags/*`, with `--prune`, would delete the user's own branches.
fn fetches_safely(base: &Path, git_dir: Option<&Path>, writes: bool, stop: &AtomicBool) -> io::Result<()> {
    for (_, key) in repo_config(base, git_dir, RISKY, true, stop)? {
        if key.starts_with("filter.") {
            if writes {
                return Err(io::Error::other(t!("branches.has_filters")));
            }
            continue;
        }
        return Err(io::Error::other(t!("remote.repo_command", "key" => key)));
    }
    // Every remote's, in any scope: the user's own could be as wrong.
    let mut c = crate::tools::command("git");
    c.arg("-C").arg(base).args(["config", "--get-regexp", r"^remote\..*\.fetch$"]).env("GIT_CONFIG_COUNT", "0");
    if let Some(d) = git_dir {
        c.env("GIT_DIR", d);
    }
    for line in tool(c, stop, &mut |_| {})?.out.lines() {
        let Some((key, spec)) = line.split_once(' ') else { continue };
        let remote = key.strip_prefix("remote.").and_then(|k| k.strip_suffix(".fetch")).unwrap_or_default();
        if let Some(dst) = refspec_outside(spec, remote) {
            return Err(io::Error::other(t!("remote.bad_refspec", "remote" => remote, "dst" => dst)));
        }
    }
    Ok(())
}

/// Where a fetch refspec of `remote` writes, when that is outside `refs/remotes/<remote>/`. A
/// negative one (`^…`) and one without a destination (only `FETCH_HEAD`) write nothing.
pub fn refspec_outside(spec: &str, remote: &str) -> Option<String> {
    let s = spec.trim().trim_start_matches('+');
    if s.starts_with('^') {
        return None;
    }
    let (_, dst) = s.split_once(':')?;
    let ok = dst.is_empty() || (dst.starts_with(&format!("refs/remotes/{remote}/")) && !dst.contains(".."));
    (!ok).then(|| dst.to_string())
}

/// The remote to fetch: the branch's own, else `origin`, else the only one.
fn fetch_remote(base: &Path, stop: &AtomicBool) -> io::Result<String> {
    let remotes: Vec<String> = git_local(base, &["remote"], stop)?.lines().map(String::from).collect();
    let branch = git_local(base, &["symbolic-ref", "--short", "-q", "HEAD"], stop).unwrap_or_default();
    let own = (!branch.is_empty())
        .then(|| git_local(base, &["config", "--get", &format!("branch.{branch}.remote")], stop).ok())
        .flatten()
        .filter(|r| remotes.contains(r));
    let remote = match own {
        Some(r) => r,
        None if remotes.iter().any(|r| r == "origin") => "origin".into(),
        None if remotes.len() == 1 => remotes[0].clone(),
        None if remotes.is_empty() => return Err(io::Error::other(t!("remote.no_remote"))),
        None => return Err(io::Error::other(t!("remote.ambiguous"))),
    };
    // Given as an argument and in a `-c` key: never an option, never a `=`.
    if remote.starts_with('-') || remote.contains('=') {
        return Err(io::Error::other(t!("remote.no_remote")));
    }
    Ok(remote)
}

fn git_fetch(base: &Path, writes: bool, stop: &AtomicBool, at: &mut dyn FnMut(&str)) -> io::Result<()> {
    fetches_safely(base, None, writes, stop)?;
    let remote = fetch_remote(base, stop)?;
    let mut c = git_plain(base);
    c.args(["-c", "maintenance.auto=false", "-c", "gc.auto=0", "-c", "fetch.recurseSubmodules=false"])
        // `--prune` removes only what the checked refspecs wrote; never tags.
        .args(["-c", "fetch.pruneTags=false", "-c", &format!("remote.{remote}.pruneTags=false")])
        .args(["fetch", "--prune", "--progress", "--no-recurse-submodules", &remote]);
    tool(c, stop, at)?.into_result().map(drop)
}

fn git(base: &Path, op: Op, stop: &AtomicBool, at: &mut dyn FnMut(&str)) -> io::Result<String> {
    let upstream = || git_local(base, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"], stop);
    if op == Op::Fetch {
        git_fetch(base, false, stop, at)?;
        let Ok(u) = upstream() else { return Ok(t!("remote.fetched_no_upstream")) };
        let counts = git_local(base, &["rev-list", "--left-right", "--count", "HEAD...@{u}"], stop)?;
        let (ahead, behind) = counts.split_once('\t').unwrap_or(("0", "0"));
        return Ok(t!("remote.fetched", "upstream" => u, "behind" => crate::i18n::digits(behind.trim().to_string()), "ahead" => crate::i18n::digits(ahead.trim().to_string())));
    }
    // Pull: the upstream first, before anything is fetched.
    let upstream = upstream().map_err(|e| with_reason(t!("remote.no_upstream"), &e.to_string()))?;
    git_fetch(base, true, stop, at)?;
    let head = git_local(base, &["rev-parse", "HEAD"], stop)?;
    let up = git_local(base, &["rev-parse", "@{u}"], stop)?;
    if head == up {
        return Ok(t!("remote.up_to_date", "upstream" => upstream));
    }
    let is_ancestor = |a: &str, b: &str| git_local(base, &["merge-base", "--is-ancestor", a, b], stop).is_ok();
    if is_ancestor(&up, &head) {
        return Ok(t!("remote.ahead_only", "upstream" => upstream));
    }
    if !is_ancestor(&head, &up) {
        return Err(io::Error::other(t!("remote.diverged", "upstream" => upstream)));
    }
    // The files the fast-forward writes, against the ones with changes not committed.
    let changing: std::collections::HashSet<String> =
        git_local(base, &["diff", "--name-only", "-z", &head, &up], stop)?.split('\0').filter(|s| !s.is_empty()).map(String::from).collect();
    let dirty = git_local(base, &["status", "--porcelain=v1", "-z", "--untracked-files=all"], stop)?;
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
    // No filters of the repository's own (`fetches_safely` refused them): none to turn off.
    let mut c = git_plain(base);
    c.args(["merge", "--ff-only", "--no-autostash", "--no-stat", "--no-edit", "@{u}"]);
    tool(c, stop, at)?.into_result().map_err(|e| with_reason(t!("remote.pull_failed"), &e.to_string()))?;
    Ok(t!("remote.pulled", "upstream" => upstream, "from" => &head[..7.min(head.len())], "to" => &up[..7.min(up.len())]))
}

// ---------------------------------------------------------------- Mercurial

/// The repository's `paths.default`, read from its own `.hg/hgrc` (which hg is told never to
/// read, `vcs::hg`), the one place hg keeps where it was cloned from.
fn hg_default(root: &Path) -> Option<String> {
    // A plain file only: a FIFO or a link to `/dev/zero` would hold the reading thread.
    let path = root.join(".hg/hgrc");
    if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
        return None;
    }
    let f = std::fs::File::open(path).ok()?;
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
        Ok(tool(c, stop, &mut |_| {})?.into_result()?.out.trim().to_string())
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
    // jj's fetch runs the user's git against the repository's git store: the same checks.
    let store = if root.join(".git").exists() { None } else { crate::vcs::jj_store(&root).map(|(s, _)| s) };
    fetches_safely(&root, store.as_deref(), false, stop)?;
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
    cvs_safely(&root)?;
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
    // No `-d`: no folder is made from what the server lists.
    c.args(["update"]);
    let said = tool(c, stop, at)?.into_result()?;
    let n = said.out.lines().filter(|l| l.starts_with("U ") || l.starts_with("P ")).count();
    Ok(t!("remote.updated_cvs", "n" => crate::i18n::digits(n.to_string())))
}

/// Refuses a CVS working copy where `cvs update` would run a program it names (a folder's
/// `CVS/Update.prog`) or could write outside it, by a symbolic link in it (`evil -> ~/.ssh`).
pub(crate) fn cvs_safely(root: &Path) -> io::Result<()> {
    let mut todo: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(d) = todo.pop() {
        if std::fs::symlink_metadata(d.join("CVS/Update.prog")).is_ok() {
            return Err(io::Error::other(t!("remote.repo_command", "key" => "CVS/Update.prog")));
        }
        for e in std::fs::read_dir(&d)?.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_symlink() {
                let name = e.path().strip_prefix(root).unwrap_or(&e.path()).display().to_string();
                return Err(io::Error::other(t!("remote.symlink", "name" => name)));
            }
            // Only the folders CVS keeps: `cvs update` enters no other.
            if kind.is_dir() && e.file_name() != "CVS" && e.path().join("CVS").is_dir() {
                todo.push(e.path());
            }
        }
    }
    Ok(())
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
        git_local(dir, &["rev-parse", "HEAD"], &AtomicBool::new(false)).unwrap()
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
        assert_eq!(git_local(&here, &["rev-parse", "origin/main"], &AtomicBool::new(false)).unwrap(), head(&there));
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

    fn stopped() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[cfg(unix)]
    #[test]
    fn an_ext_remote_never_runs_even_when_the_repository_allows_it() {
        let Some((t, here, _there)) = setup("ext") else { return };
        let marker = t.join("PWNED");
        git_in(&here, &["config", "protocol.ext.allow", "always"]);
        git_in(&here, &["remote", "set-url", "origin", &format!("ext::sh -c touch% {}", marker.display())]);
        assert!(go(&here, Op::Fetch).is_err());
        assert!(!marker.exists(), "ext:: ran a command");
    }

    #[cfg(unix)]
    #[test]
    fn an_alternate_refs_command_is_refused() {
        let Some((t, here, there)) = setup("altrefs") else { return };
        let marker = t.join("PWNED");
        git_in(&here, &["config", "core.alternateRefsCommand", &format!("touch {}", marker.display())]);
        std::fs::write(here.join(".git/objects/info/alternates"), format!("{}\n", there.join(".git/objects").display())).unwrap();
        let e = go(&here, Op::Fetch).unwrap_err().to_string();
        assert!(e.contains("core.alternaterefscommand"), "{e}");
        assert!(!marker.exists());
    }

    #[test]
    fn proxies_http_settings_and_lfs_agents_of_the_repository_are_refused() {
        let Some((_t, here, _there)) = setup("http") else { return };
        for (key, value) in [("http.proxy", "http://127.0.0.1:9"), ("http.https://example.org.sslVerify", "false"), ("remote.origin.proxy", "http://127.0.0.1:9"), ("lfs.customtransfer.x.path", "sh"), ("lfs.standalonetransferagent", "x"), ("credential.helper", "!sh")] {
            git_in(&here, &["config", key, value]);
            let e = go(&here, Op::Fetch).unwrap_err().to_string();
            assert!(e.starts_with(&t!("remote.repo_command", "key" => "").split('(').next().unwrap().to_string()), "{key}: {e}");
            git_in(&here, &["config", "--unset", key]);
        }
        go(&here, Op::Fetch).unwrap();
    }

    #[test]
    fn a_refspec_into_local_branches_is_refused_and_tags_are_never_pruned() {
        let Some((_t, here, _there)) = setup("refspec") else { return };
        git_in(&here, &["branch", "keep"]);
        git_in(&here, &["tag", "v1"]);
        git_in(&here, &["config", "--replace-all", "remote.origin.fetch", "+refs/heads/*:refs/heads/*"]);
        git_in(&here, &["config", "--add", "remote.origin.fetch", "^refs/heads/main"]);
        let e = go(&here, Op::Fetch).unwrap_err().to_string();
        assert_eq!(e, t!("remote.bad_refspec", "remote" => "origin", "dst" => "refs/heads/*"));
        git_in(&here, &["config", "--unset-all", "remote.origin.fetch"]);
        git_in(&here, &["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"]);
        git_in(&here, &["config", "fetch.pruneTags", "true"]);
        git_in(&here, &["config", "remote.origin.pruneTags", "true"]);
        go(&here, Op::Fetch).unwrap();
        let s = &stopped();
        assert!(git_local(&here, &["rev-parse", "--verify", "refs/heads/keep"], s).is_ok());
        assert!(git_local(&here, &["rev-parse", "--verify", "refs/tags/v1"], s).is_ok(), "a local tag was pruned");
        assert_eq!(refspec_outside("+refs/heads/*:refs/remotes/origin/*", "origin"), None);
        assert_eq!(refspec_outside("refs/heads/main", "origin"), None);
        assert_eq!(refspec_outside("+refs/heads/*:refs/remotes/other/*", "origin").as_deref(), Some("refs/remotes/other/*"));
        assert!(refspec_outside("refs/heads/*:refs/remotes/origin/../../heads/*", "origin").is_some());
    }

    #[test]
    fn only_the_branchs_remote_is_fetched() {
        let Some((t, here, _there)) = setup("oneremote") else { return };
        git_in(&here, &["remote", "add", "other", &t.join("nowhere.git").display().to_string()]);
        // `other` would fail: it is not asked.
        go(&here, Op::Fetch).unwrap();
    }

    #[test]
    fn a_jujutsu_store_is_checked_as_git_is() {
        if !crate::user_menu::installed("git") {
            return;
        }
        let t = crate::fs::Scratch::named("remote-jj-store");
        let store = t.join(".jj/repo/store");
        std::fs::create_dir_all(&store).unwrap();
        git_in(&store, &["init", "-q", "--bare", "git"]);
        std::fs::write(store.join("git_target"), "git").unwrap();
        git_in(&store.join("git"), &["config", "core.sshCommand", "touch pwned"]);
        let (dir, _) = crate::vcs::jj_store(&t).unwrap();
        let e = fetches_safely(&t, Some(&dir), false, &stopped()).unwrap_err().to_string();
        assert!(e.contains("core.sshcommand"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_jujutsu_config_counts_as_the_repositorys_own() {
        let t = crate::fs::Scratch::named("remote-jj-link");
        std::fs::create_dir_all(t.join(".jj/repo")).unwrap();
        std::fs::write(t.join("elsewhere.toml"), "").unwrap();
        std::os::unix::fs::symlink(t.join("elsewhere.toml"), t.join(".jj/repo/config.toml")).unwrap();
        assert!(crate::vcs::jj_config_inside(&t));
        std::fs::write(t.join(".jj/repo/config-id"), "x").unwrap();
        assert!(!crate::vcs::jj_config_inside(&t));
    }

    #[cfg(unix)]
    #[test]
    fn a_cvs_working_copy_with_a_link_or_an_update_program_is_refused() {
        let t = crate::fs::Scratch::named("remote-cvs-link");
        for d in ["CVS", "sub/CVS", "untracked"] {
            std::fs::create_dir_all(t.join(d)).unwrap();
        }
        // A link in a folder CVS does not keep is no matter.
        std::os::unix::fs::symlink("/", t.join("untracked/link")).unwrap();
        cvs_safely(&t).unwrap();
        std::os::unix::fs::symlink("/", t.join("sub/evil")).unwrap();
        assert_eq!(cvs_safely(&t).unwrap_err().to_string(), t!("remote.symlink", "name" => "sub/evil"));
        std::fs::remove_file(t.join("sub/evil")).unwrap();
        std::fs::write(t.join("sub/CVS/Update.prog"), "touch pwned").unwrap();
        assert!(cvs_safely(&t).unwrap_err().to_string().contains("CVS/Update.prog"));
    }

    #[cfg(unix)]
    #[test]
    fn stop_ends_what_the_tool_started_too() {
        let t = crate::fs::Scratch::named("remote-group");
        let pid_file = t.join("pid");
        let mut c = Command::new("sh");
        c.arg("-c").arg(format!("sleep 600 & echo $! > {}; wait", pid_file.display()));
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let f = pid_file.clone();
        let limit = std::time::Instant::now() + crate::test_limit(Duration::from_secs(10));
        let presser = std::thread::spawn(move || {
            while !f.exists() || std::fs::read_to_string(&f).unwrap_or_default().trim().is_empty() {
                assert!(std::time::Instant::now() < limit, "the tool never started");
                std::thread::sleep(Duration::from_millis(10));
            }
            flag.store(true, Ordering::Relaxed);
        });
        assert_eq!(tool(c, &stop, &mut |_| {}).err().map(|e| e.kind()), Some(io::ErrorKind::Interrupted));
        presser.join().unwrap();
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(std::time::Instant::now() < limit, "the tool's child lives on after Stop");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_as_hgrc_is_never_read() {
        let t = crate::fs::Scratch::named("remote-hg-fifo");
        std::fs::create_dir_all(t.join(".hg")).unwrap();
        let path = std::ffi::CString::new(t.join(".hg/hgrc").display().to_string()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert_eq!(hg_default(&t), None);
        std::fs::remove_file(t.join(".hg/hgrc")).unwrap();
        std::fs::write(t.join(".hg/hgrc"), "[paths]\ndefault = https://example.org/r\n").unwrap();
        assert_eq!(hg_default(&t).as_deref(), Some("https://example.org/r"));
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

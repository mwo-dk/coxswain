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
//! session), on Windows a detached process in a job of its own (Esc ends the job); configured
//! helpers and ssh-agent still work, and when they cannot, the tool's own words come back as the
//! error.

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
    // Everything the tool starts goes into a job of its own, so Stop ends the whole tree: Git
    // Credential Manager's sign-in window too, which never writes to a pipe. git starts its
    // helpers only after it has read the config, well after this.
    #[cfg(windows)]
    let job = Job::of(&child);
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
            #[cfg(windows)]
            if let Some(j) = &job {
                j.end();
            }
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

/// A Windows job holding one tool and all it starts. Ended with `end`, or when dropped (kill on
/// close), so nothing it started outlives `tool`, even when the app ends first.
#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Job {
    /// `child` put in a new job; None when Windows refuses (the tool is then only ended itself).
    fn of(child: &std::process::Child) -> Option<Job> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return None;
        }
        let job = Job(job);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = std::mem::size_of_val(&limits) as u32;
        let set = unsafe { SetInformationJobObject(job.0, JobObjectExtendedLimitInformation, (&raw const limits).cast(), size) };
        let put = set != 0 && unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) } != 0;
        put.then_some(job)
    }

    fn end(&self) {
        unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1) };
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
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
const RISKY: &str = r"^(core\.(sshcommand|gitproxy|askpass|alternaterefscommand)|remote\..*\.(uploadpack|receivepack|proxy)|credential\.(.*\.)?helper|http\..*|gpg\..*|hook\..*|fetch\.bundleuri|gc\.recentobjectshook|lfs\.customtransfer\..*\.(path|args)|lfs\.standalonetransferagent|lfs\.extension\..*\.(clean|smudge)|filter\..*)$";

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

/// The work tree is the folder that holds `.git` (a folder, or a file in a submodule or a linked
/// worktree): a repository's `core.worktree` may put it elsewhere, and a pull would write there.
fn work_tree_here(base: &Path, stop: &AtomicBool) -> io::Result<()> {
    let here = dunce::canonicalize(base)?;
    let Some(root) = here.ancestors().find(|a| a.join(".git").exists()) else { return Ok(()) };
    let top = git_local(base, &["rev-parse", "--show-toplevel"], stop)?;
    if dunce::canonicalize(&top).ok().as_deref() != Some(root) {
        return Err(io::Error::other(t!("remote.worktree_elsewhere", "dir" => top, "root" => root.display().to_string())));
    }
    Ok(())
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
    // Pull: the upstream first, before anything is fetched, and the work tree where it is shown.
    work_tree_here(base, stop)?;
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
    // A submodule's own changes are not looked into: that would run its filters.
    let dirty = git_local(base, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignore-submodules=dirty"], stop)?;
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
    // No signature check: `merge.verifySignatures`/`pull.verifySignatures` plus the repository's
    // own `gpg.*` (refused above too) would run a program it names. `--no-edit` means no editor.
    c.args(["-c", "merge.verifySignatures=false", "-c", "log.showSignature=false", "-c", "maintenance.auto=false", "-c", "gc.auto=0"]);
    c.args(["merge", "--ff-only", "--no-verify-signatures", "--no-autostash", "--no-stat", "--no-edit", "@{u}"]);
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
    // A look that failed (the server refused, or did not answer) found nothing: no update after it.
    if !plan.ok {
        return Err(io::Error::other(plan.why()));
    }
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

    /// Whether process `pid` has ended, waiting at most `limit` (only against a hang).
    fn ended(pid: u32, limit: Duration) -> bool {
        #[cfg(unix)]
        {
            // Its parent is gone too, so init reaps it and the pid stops answering.
            let start = std::time::Instant::now();
            while unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
                if start.elapsed() > limit {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            true
        }
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
            use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
            let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if h.is_null() {
                return true;
            }
            let gone = WaitForSingleObject(h, limit.as_millis() as u32) == WAIT_OBJECT_0;
            CloseHandle(h);
            gone
        }
    }

    /// Stop ends what the tool started, not only the tool: a helper that never writes to its
    /// pipe (a sign-in window) is ended with it.
    #[test]
    fn stop_ends_the_tools_helpers() {
        #[cfg(unix)]
        let c = {
            let mut c = Command::new("sh");
            c.args(["-c", "sleep 1000 & echo $!; wait"]);
            c
        };
        #[cfg(windows)]
        let c = {
            let mut c = Command::new("powershell");
            c.args([
                "-NoProfile",
                "-Command",
                "$p = Start-Process -PassThru -NoNewWindow ping -ArgumentList '-n','1000','127.0.0.1'; Write-Output $p.Id; Wait-Process -Id $p.Id",
            ]);
            c
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let ran = std::thread::spawn(move || {
            let stop = AtomicBool::new(false);
            tool(c, &stop, &mut |l| {
                if let Ok(pid) = l.parse::<u32>() {
                    let _ = tx.send(pid);
                    stop.store(true, Ordering::Relaxed);
                }
            })
            .err()
            .map(|e| e.kind())
        });
        let limit = crate::test_limit(Duration::from_secs(20));
        let helper = rx.recv_timeout(limit).expect("the helper's pid");
        assert_eq!(ran.join().unwrap(), Some(io::ErrorKind::Interrupted));
        assert!(ended(helper, limit), "helper {helper} still runs");
    }

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

    #[cfg(unix)]
    #[test]
    fn a_signature_check_the_repository_asks_for_never_runs_its_program() {
        let Some((t, here, there)) = setup("gpg") else { return };
        let marker = t.join("PWNED");
        // The repository asks for signatures and names the program that would check them.
        git_in(&here, &["config", "merge.verifySignatures", "true"]);
        git_in(&here, &["config", "gpg.program", &format!("sh -c \"touch {} #\"", marker.display())]);
        // A commit to fast-forward to.
        commit(&there, "b.txt", "two\n");
        git_in(&there, &["push", "-q", "origin", "HEAD:main"]);
        // Refused at the config gate, before the merge.
        let e = go(&here, Op::Pull).unwrap_err().to_string();
        assert!(e.contains("gpg.program"), "{e}");
        assert!(!marker.exists());
        // Even past the gate, the merge itself asks for no signature: unset the gpg key, keep the
        // request, and run the pull's own merge args by hand with a gpg.program that would fire.
        git_in(&here, &["config", "--unset", "gpg.program"]);
        let mut c = git_plain(&here);
        c.args(["-c", &format!("gpg.program=sh -c \"touch {} #\"", marker.display())]);
        c.args(["-c", "merge.verifySignatures=false", "-c", "log.showSignature=false"]);
        c.args(["merge", "--ff-only", "--no-verify-signatures", "--no-autostash", "--no-stat", "--no-edit", "@{u}"]);
        assert!(c.status().unwrap().success());
        assert!(!marker.exists(), "the merge ran gpg.program");
    }

    #[test]
    fn hooks_set_in_config_and_bundle_uris_are_refused() {
        let Some((t, here, _there)) = setup("cfghook") else { return };
        let marker = t.join("PWNED");
        git_in(&here, &["config", "hook.x.command", &format!("touch {}", marker.display())]);
        git_in(&here, &["config", "--add", "hook.x.event", "reference-transaction"]);
        let e = go(&here, Op::Fetch).unwrap_err().to_string();
        assert!(e.contains("hook.x."), "{e}");
        assert!(!marker.exists());
        git_in(&here, &["config", "--remove-section", "hook.x"]);
        git_in(&here, &["config", "fetch.bundleURI", &format!("file://{}", t.join("nowhere.bundle").display())]);
        let e = go(&here, Op::Fetch).unwrap_err().to_string();
        assert!(e.contains("fetch.bundleuri"), "{e}");
    }

    #[test]
    fn a_pull_into_a_work_tree_elsewhere_is_refused() {
        let Some((t, here, there)) = setup("worktree") else { return };
        commit(&there, "b.txt", "two\n");
        git_in(&there, &["push", "-q", "origin", "HEAD:main"]);
        let outside = t.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        git_in(&here, &["config", "core.worktree", &outside.display().to_string()]);
        let e = go(&here, Op::Pull).unwrap_err().to_string();
        assert!(e.contains(&*outside.file_name().unwrap().to_string_lossy()), "{e}");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        git_in(&here, &["config", "--unset", "core.worktree"]);
        go(&here, Op::Pull).unwrap();
    }

    #[test]
    fn the_pull_runs_no_filter_of_a_submodule() {
        let Some((t, here, there)) = setup("submodule") else { return };
        // A submodule whose own config defines a filter for its files, with a changed file:
        // looking into it for changes would run that filter.
        let sub = t.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        git_in(&sub, &["init", "-q"]);
        commit(&sub, "s.txt", "one\n");
        git_in(&here, &["submodule", "add", "-q", sub.to_str().unwrap(), "sub"]);
        git_in(&here, &["commit", "-q", "-m", "sub"]);
        git_in(&here, &["push", "-q", "origin", "HEAD:main"]);
        git_in(&there, &["pull", "-q", "--no-rebase", "origin", "main"]);
        commit(&there, "b.txt", "two\n");
        git_in(&there, &["push", "-q", "origin", "HEAD:main"]);
        let marker = t.join("PWNED");
        let inner = here.join("sub");
        git_in(&inner, &["config", "filter.evil.clean", &format!("sh -c \"touch {}; cat\"", marker.display())]);
        std::fs::write(inner.join(".gitattributes"), "* filter=evil\n").unwrap();
        // The same size: git has to read it (through the filter) to know it changed.
        std::fs::write(inner.join("s.txt"), "two\n").unwrap();
        go(&here, Op::Pull).unwrap();
        assert_eq!(head(&here), head(&there));
        assert!(!marker.exists(), "a submodule's own filter ran");
    }

    #[test]
    fn a_gc_hook_of_the_repository_is_refused() {
        let Some((t, here, _there)) = setup("gchook") else { return };
        git_in(&here, &["config", "gc.recentObjectsHook", &format!("touch {}", t.join("PWNED").display())]);
        let e = go(&here, Op::Fetch).unwrap_err().to_string();
        assert!(e.contains("gc.recentobjectshook"), "{e}");
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

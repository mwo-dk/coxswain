//! Who changed each line of a file (`git blame`, Mercurial's `hg annotate`), and the commits
//! that changed one function or a few lines of it (`git log -L`). Both through Coxswain's own
//! git and hg commands (`history::git`, `vcs::hg`): no hooks, no filters, no text conversion
//! a repository names. Bounded (lines, bytes, time) and stopped when asked.

use serde::Serialize;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::history::Commit;
use crate::t;

/// Lines of a file blamed at most: the rest is said to be left out.
pub const MOST_LINES: usize = 100_000;
/// A file bigger than this is not blamed at all.
const MOST_BYTES: u64 = 32 << 20;
/// Commits of a function's history at most.
pub const MOST_CHANGES: usize = 200;
/// Bytes of a function's history at most.
const MOST_HISTORY: usize = 4 << 20;
/// How long git may take before it is stopped.
const BUDGET: Duration = Duration::from_secs(60);

/// A commit as blame names it: its short id ("" for lines not committed yet), author, time
/// (seconds) and subject.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Who {
    pub hash: String,
    pub author: String,
    pub time: u64,
    pub subject: String,
    /// Its margin as both apps show it (`margin`).
    pub margin: String,
}

/// Each line's commit (`lines[i]` is an index into `commits`), and whether the file had more
/// lines than were blamed.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Blame {
    pub commits: Vec<Who>,
    pub lines: Vec<u32>,
    pub cut: bool,
}

impl Blame {
    /// Line `i`'s commit (from 0).
    pub fn of(&self, i: usize) -> Option<&Who> {
        self.commits.get(*self.lines.get(i)? as usize)
    }
}

/// One line's margin, as both apps show it: `a1b2c3d Ada          2026-09-30`, the author cut to
/// 12 columns; lines not committed say so.
pub fn margin(w: &Who) -> String {
    if w.hash.is_empty() {
        return format!("{:<31}", t!("blame.not_committed"));
    }
    let author: String = w.author.chars().take(12).collect();
    format!("{:<7} {author:<12} {}", w.hash, crate::i18n::date(w.time))
}

/// Run `c` to its end, at most `max` bytes of what it prints kept: stopped when `stop` is
/// set or `BUDGET` passes. `true` with the output when there was more.
fn run(mut c: Command, max: usize, stop: &AtomicBool) -> io::Result<(Vec<u8>, bool)> {
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null()).spawn()?;
    let out = child.stdout.take().ok_or_else(|| io::Error::other("no output"))?;
    let mut err = child.stderr.take().ok_or_else(|| io::Error::other("no output"))?;
    let reader = std::thread::spawn(move || {
        let mut buf = vec![];
        let r = out.take(max as u64 + 1).read_to_end(&mut buf);
        r.map(|_| buf)
    });
    let errs = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break Some(s);
        }
        if reader.is_finished() {
            // All it may print is read: it is stopped rather than waited for.
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        if stop.load(Ordering::Relaxed) || start.elapsed() > BUDGET {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(if stop.load(Ordering::Relaxed) {
                io::Error::new(io::ErrorKind::Interrupted, t!("blame.stopped"))
            } else {
                io::Error::new(io::ErrorKind::TimedOut, t!("blame.too_long"))
            });
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut buf = reader.join().map_err(|_| io::Error::other("reader"))??;
    let why = errs.join().unwrap_or_default();
    let more = buf.len() > max;
    buf.truncate(max);
    match status {
        Some(s) if !s.success() && !more => Err(io::Error::other(if why.trim().is_empty() { "git failed".into() } else { why.trim().to_string() })),
        _ => Ok((buf, more)),
    }
}

/// Whether `dir` is in a repository blame can ask: one with `.git`, `.jj` or `.hg` above.
pub fn in_repository(dir: &Path) -> bool {
    dir.ancestors().any(|a| a.join(".git").exists() || a.join(".jj").is_dir() || a.join(".hg").is_dir())
}

/// The folder git runs in and the file's name in it.
fn parts(path: &Path) -> io::Result<(&Path, String)> {
    let base = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, t!("blame.not_here")))?;
    let name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, t!("blame.not_here")))?;
    Ok((base, name.to_string_lossy().into_owned()))
}

/// Which system keeps `dir`, for what this module asks: git (Jujutsu with its git store too),
/// Mercurial, or none that can answer.
fn system(dir: &Path) -> io::Result<&'static str> {
    if !in_repository(dir) {
        return Err(io::Error::new(io::ErrorKind::Unsupported, t!("blame.not_here")));
    }
    match crate::vcs::of(dir).name() {
        "git" | "jj" => Ok("git"),
        "hg" => Ok("hg"),
        _ => Err(io::Error::new(io::ErrorKind::Unsupported, t!("blame.not_here"))),
    }
}

/// Who last changed each line of the file at `path`, as it is on the disk: lines changed
/// since the last commit have no commit. At most `MOST_LINES` lines.
pub fn blame(path: &Path, stop: &AtomicBool) -> io::Result<Blame> {
    let (base, name) = parts(path)?;
    let size = std::fs::metadata(path)?.len();
    if size > MOST_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, t!("blame.too_big")));
    }
    let lines = bytecount(&std::fs::read(path)?);
    let cut = lines > MOST_LINES;
    if system(base)? == "hg" {
        return hg_blame(base, &name, cut, stop);
    }
    let mut c = crate::history::git(base);
    c.args(["blame", "--porcelain", "--no-textconv"]);
    if cut {
        c.arg(format!("-L1,{MOST_LINES}"));
    }
    c.args(["--", &name]);
    let (out, _) = run(c, 512 << 20, stop)?;
    let mut b = parse_porcelain(&String::from_utf8_lossy(&out));
    b.cut = cut;
    Ok(with_margins(b))
}

/// Lines of `bytes`, a last one without a newline counted.
fn bytecount(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b == b'\n').count() + usize::from(bytes.last().is_some_and(|&b| b != b'\n'))
}

/// `git blame --porcelain`: a header (`<id> <old line> <line> [<lines>]`) for each line, the
/// commit's fields the first time it comes, then the line itself after a tab.
pub fn parse_porcelain(out: &str) -> Blame {
    let mut b = Blame::default();
    let mut ids: std::collections::HashMap<String, u32> = Default::default();
    let mut cur: Option<(u32, usize)> = None;
    for l in out.lines() {
        if l.starts_with('\t') {
            if let Some((c, line)) = cur.take() {
                if b.lines.len() <= line {
                    b.lines.resize(line + 1, c);
                }
                b.lines[line] = c;
            }
            continue;
        }
        let mut f = l.splitn(4, ' ');
        let first = f.next().unwrap_or("");
        if first.len() >= 40 && first.bytes().all(|c| c.is_ascii_hexdigit()) {
            let line: usize = f.nth(1).and_then(|n| n.parse().ok()).unwrap_or(1);
            let n = b.commits.len() as u32;
            let c = *ids.entry(first.to_string()).or_insert_with(|| {
                let hash = if first.bytes().all(|c| c == b'0') { String::new() } else { first[..7].to_string() };
                b.commits.push(Who { hash, ..Who::default() });
                n
            });
            cur = Some((c, line.saturating_sub(1)));
            continue;
        }
        let Some((c, _)) = cur else { continue };
        let w = &mut b.commits[c as usize];
        if w.hash.is_empty() {
            continue;
        }
        let rest = l.split_once(' ').map_or("", |(_, r)| r);
        match first {
            "author" => w.author = rest.to_string(),
            "author-time" => w.time = rest.parse().unwrap_or(0),
            "summary" => w.subject = rest.to_string(),
            _ => {}
        }
    }
    b
}

/// `hg annotate` of the working copy: every line's commit, author and date.
fn hg_blame(base: &Path, name: &str, cut: bool, stop: &AtomicBool) -> io::Result<Blame> {
    let mut c = crate::vcs::hg_command(base)?;
    c.args([
        "annotate",
        "-r",
        "wdir()",
        "-T",
        r"{lines % '{node}\x1f{date|hgdate}\x1f{user|person}\x1f{line}'}",
        "--",
        // `relpath:`, as hg runs in the file's folder (`path:` is from the root).
        &format!("relpath:{name}"),
    ]);
    let (out, _) = run(c, 512 << 20, stop)?;
    let mut b = parse_hg(&String::from_utf8_lossy(&out));
    if cut {
        b.lines.truncate(MOST_LINES);
    }
    b.cut = cut;
    Ok(with_margins(b))
}

fn with_margins(mut b: Blame) -> Blame {
    for w in &mut b.commits {
        w.margin = margin(w);
    }
    b
}

/// `node \x1f secs tz \x1f person \x1f line` for each line.
pub fn parse_hg(out: &str) -> Blame {
    let mut b = Blame::default();
    let mut ids: std::collections::HashMap<String, u32> = Default::default();
    for l in out.lines() {
        let f: Vec<&str> = l.splitn(4, '\x1f').collect();
        let [node, date, user, _] = f[..] else { continue };
        let n = b.commits.len() as u32;
        let c = *ids.entry(node.to_string()).or_insert_with(|| {
            let wdir = node.bytes().all(|c| c == b'f');
            b.commits.push(Who {
                hash: if wdir { String::new() } else { node.chars().take(7).collect() },
                author: if wdir { String::new() } else { user.to_string() },
                time: date.split(' ').next().and_then(|s| s.parse().ok()).unwrap_or(0),
                ..Who::default()
            });
            n
        });
        b.lines.push(c);
    }
    b
}

/// One commit of a function's history and what it changed there.
#[derive(Clone, Debug, Serialize)]
pub struct Change {
    pub commit: Commit,
    pub diff: String,
}

/// The commits that changed some lines of a file, newest first.
#[derive(Clone, Debug, Default, Serialize)]
pub struct History {
    /// The function's name ("impl Store › fn open"), or "" for lines picked.
    pub name: String,
    /// The lines followed, from 1, as the last commit has them.
    pub from: usize,
    pub to: usize,
    pub changes: Vec<Change>,
    /// There were more than were kept.
    pub more: bool,
}

impl History {
    /// The whole history as text, for a viewer: each commit's line, its subject, its change.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for c in &self.changes {
            s += &format!(
                "commit {} · {} · {}\n    {}\n\n{}\n\n",
                c.commit.short(),
                c.commit.author,
                crate::i18n::date(c.commit.time),
                c.commit.subject,
                c.diff.trim_end()
            );
        }
        if self.more {
            s += &t!("blame.more_changes", "n" => MOST_CHANGES);
            s.push('\n');
        }
        s
    }
}

/// The commits that changed the function holding line `line` (from 0) of the file at `path`,
/// or, for a range picked (`to` past `line`) or a line in no function, those lines. `text` is
/// the file as the app has it (the editor's, maybe not saved); the lines followed are found
/// again in the file as the last commit has it, the function by its name.
pub fn line_history(path: &Path, text: &str, line: usize, to: usize, stop: &AtomicBool) -> io::Result<History> {
    let (base, name) = parts(path)?;
    let hg = system(base)? == "hg";
    let head = if hg { crate::vcs::of(base).read(base, ".", &name, MOST_BYTES as usize) } else { crate::history::git_read(base, "HEAD", &name, MOST_BYTES as usize) };
    let (head, _) = head
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, t!("blame.not_committed_file")))?;
    let head = String::from_utf8_lossy(&head).into_owned();
    let p = path.to_string_lossy();
    let item = (to <= line).then(|| crate::code::item_at(text, &p, line)).flatten();
    let (from, to, label) = match item {
        Some((a, b, label)) if text == head => (a, b - 1, label),
        Some((a, _, label)) => {
            // The same function in the last commit: the one of that name nearest to where it is.
            let there = crate::code::items(&head, &p)
                .into_iter()
                .filter(|(_, _, n)| *n == label)
                .min_by_key(|(s, _, _)| s.abs_diff(a))
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, t!("blame.not_in_head", "name" => &label)))?;
            (there.0, there.1 - 1, label)
        }
        None => (line, to.max(line), String::new()),
    };
    let last = bytecount(head.as_bytes()).max(1);
    let (from, to) = (from.min(last - 1) + 1, to.min(last - 1) + 1);
    let c = if hg {
        // Mercurial's `log -L` (marked experimental): from the working copy's commit back.
        let mut c = crate::vcs::hg_command(base)?;
        c.args([
            "log",
            "--follow",
            "-p",
            "-T",
            r"\x1e{node}\x1f{word(0, date|hgdate)}\x1f{author|person}\x1f{desc|firstline}\n",
            "-l",
            &MOST_CHANGES.to_string(),
            "-L",
            &format!("relpath:{name},{from}:{to}"),
        ]);
        c
    } else {
        let mut c = crate::history::git(base);
        c.args([
            "log",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--format=%x1e%H%x1f%at%x1f%an%x1f%s",
            "-n",
            &MOST_CHANGES.to_string(),
            &format!("-L{from},{to}:./{name}"),
            "HEAD",
        ]);
        c
    };
    let (out, cut) = run(c, MOST_HISTORY, stop)?;
    let mut changes = parse_log(&String::from_utf8_lossy(&out));
    if cut {
        changes.pop();
    }
    let more = cut || changes.len() >= MOST_CHANGES;
    Ok(History { name: label, from, to, changes, more })
}

/// `git log -L` with each commit after a record separator: its fields on the first line, the
/// change after.
pub fn parse_log(out: &str) -> Vec<Change> {
    out.split('\x1e')
        .filter_map(|rec| {
            let (head, diff) = rec.split_once('\n').unwrap_or((rec, ""));
            let f: Vec<&str> = head.splitn(4, '\x1f').collect();
            let [hash, time, author, subject] = f[..] else { return None };
            Some(Change {
                commit: Commit {
                    hash: hash.to_string(),
                    time: time.parse().unwrap_or(0),
                    author: author.to_string(),
                    subject: subject.to_string(),
                    ..Commit::default()
                },
                diff: diff.trim_matches('\n').to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let ok = crate::tools::command("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=Ada", "-c", "user.email=ada@example.com", "-c", "commit.gpgsign=false"])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn blames_lines_and_follows_a_function() {
        let Some(d) = crate::history::tests::repo("blame") else { return };
        let f = d.join("engine.rs");
        std::fs::write(&f, "fn start() {\n    ignite();\n}\n\nfn stop() {\n    cut();\n}\n").unwrap();
        git(&d, &["add", "."]);
        git(&d, &["commit", "-q", "-m", "First engine"]);
        std::fs::write(&f, "fn start() {\n    ignite();\n}\n\nfn stop() {\n    cut();\n    vent();\n}\n").unwrap();
        git(&d, &["commit", "-q", "-am", "Vent on stop"]);
        // Not committed: the last line.
        std::fs::write(&f, "fn start() {\n    ignite();\n}\n\nfn stop() {\n    cut();\n    vent();\n}\n// later\n").unwrap();
        let stop = AtomicBool::new(false);
        let b = blame(&f, &stop).unwrap();
        assert_eq!(b.lines.len(), 9);
        assert_eq!(b.of(0).unwrap().subject, "First engine");
        assert_eq!(b.of(6).unwrap().subject, "Vent on stop");
        assert_eq!(b.of(0).unwrap().author, "Ada");
        assert!(b.of(8).unwrap().hash.is_empty(), "not committed yet");
        assert!(!b.cut);

        let text = std::fs::read_to_string(&f).unwrap();
        let h = line_history(&f, &text, 5, 5, &stop).unwrap();
        assert_eq!(h.name, "fn stop");
        assert_eq!((h.from, h.to), (5, 8));
        let subjects: Vec<&str> = h.changes.iter().map(|c| c.commit.subject.as_str()).collect();
        assert_eq!(subjects, ["Vent on stop", "First engine"]);
        assert!(h.changes[0].diff.contains("+    vent();"));
        // `start` never changed after it came.
        let h = line_history(&f, &text, 1, 1, &stop).unwrap();
        assert_eq!(h.name, "fn start");
        assert_eq!(h.changes.len(), 1);
        // Lines picked: those lines.
        let h = line_history(&f, &text, 0, 1, &stop).unwrap();
        assert_eq!((h.name.as_str(), h.from, h.to), ("", 1, 2));
        assert!(h.text().contains("First engine"));
        // Stopped before it starts: nothing but the stop.
        stop.store(true, Ordering::Relaxed);
        assert_eq!(blame(&f, &stop).unwrap_err().kind(), io::ErrorKind::Interrupted);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A Mercurial repository made with the hg CLI: who changed each line of a file in a
    /// folder (not the root's file of the same name), a function's history through `hg log -L`,
    /// and for search the names of the functions each commit changed, its changes and their
    /// cost. Nothing without hg.
    #[test]
    fn hg_blames_lines_and_follows_a_function() {
        if !crate::user_menu::installed("hg") {
            return;
        }
        let top = std::env::temp_dir().join(format!("coxswain-blame-hg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let (home, d) = (top.join("home"), top.join("repo"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(d.join("sub")).unwrap();
        let hg = |args: &[&str]| {
            let ok = crate::tools::command("hg")
                .current_dir(&d)
                .args(args)
                .env("HOME", &home)
                .env("HGRCPATH", "")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "hg {args:?}");
        };
        hg(&["init"]);
        let f = d.join("sub/engine.rs");
        std::fs::write(d.join("engine.rs"), "// the root's own\n").unwrap();
        std::fs::write(&f, "fn start() {\n    ignite();\n}\n\nfn stop() {\n    cut();\n}\n").unwrap();
        hg(&["add"]);
        hg(&["commit", "-u", "Ada <ada@example.org>", "-d", "1700000000 0", "-m", "First engine"]);
        std::fs::write(&f, "fn start() {\n    ignite();\n}\n\nfn stop() {\n    cut();\n    vent();\n}\n").unwrap();
        hg(&["commit", "-u", "Bob <bob@example.org>", "-d", "1700000100 0", "-m", "Vent on stop"]);
        std::fs::write(&f, "fn start() {\n    ignite();\n}\n\nfn stop() {\n    cut();\n    vent();\n}\n// later\n").unwrap();
        assert_eq!(crate::vcs::of(&d).name(), "hg");
        let stop = AtomicBool::new(false);
        let b = blame(&f, &stop).unwrap();
        assert_eq!(b.lines.len(), 9, "the folder's file, not the root's");
        assert_eq!(b.of(0).unwrap().author, "Ada");
        assert_eq!(b.of(0).unwrap().time, 1_700_000_000);
        assert_eq!(b.of(6).unwrap().author, "Bob");
        assert_eq!(b.of(6).unwrap().hash.len(), 7);
        assert!(b.of(8).unwrap().hash.is_empty(), "not committed yet");

        let text = std::fs::read_to_string(&f).unwrap();
        let h = line_history(&f, &text, 5, 5, &stop).unwrap();
        assert_eq!((h.name.as_str(), h.from, h.to), ("fn stop", 5, 8));
        let subjects: Vec<&str> = h.changes.iter().map(|c| c.commit.subject.as_str()).collect();
        assert_eq!(subjects, ["Vent on stop", "First engine"]);
        assert_eq!((h.changes[0].commit.author.as_str(), h.changes[0].commit.time), ("Bob", 1_700_000_100));
        assert!(h.changes[0].diff.contains("+    vent();"), "{}", h.changes[0].diff);
        let h = line_history(&f, &text, 1, 1, &stop).unwrap();
        assert_eq!((h.name.as_str(), h.changes.len()), ("fn start", 1));

        let head = crate::vcs::of(&d).commit_id(&d, "HEAD").unwrap();
        let commits = crate::history::for_search(&d, &head, None, 10).unwrap();
        assert_eq!(commits.len(), 2);
        assert!(commits[0].2.contains("Changed functions: fn stop"), "{}", commits[0].2);
        let changes = crate::history::changes_for_search(&d, &head, None, 10).unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!((changes[0].0.as_str(), changes[0].1), (head.as_str(), 1_700_000_100));
        assert!(changes[0].2.starts_with(&format!("commit {} · Bob · 2023-11-14 · changes\n", &head[..7])), "{}", changes[0].2);
        assert!(changes[0].2.contains("+    vent();"));
        let (n, bytes) = crate::history::changes_cost(&d, 10);
        assert_eq!(n, 2);
        assert!(bytes >= 700, "{bytes}");
        let _ = std::fs::remove_dir_all(&top);
    }

    #[test]
    fn a_file_outside_a_repository_says_so() {
        let d = std::env::temp_dir().join(format!("coxswain-blame-none-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.txt");
        std::fs::write(&f, "a\n").unwrap();
        // Unless the temporary folder is itself in a repository.
        if !in_repository(&d) {
            assert_eq!(blame(&f, &AtomicBool::new(false)).unwrap_err().kind(), io::ErrorKind::Unsupported);
            assert_eq!(line_history(&f, "a\n", 0, 0, &AtomicBool::new(false)).unwrap_err().kind(), io::ErrorKind::Unsupported);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reads_annotate() {
        let b = parse_hg("abcdef1234\x1f1700000000 0\x1fAda\x1fone\nffffffffffff\x1f1700000001 0\x1fBob\x1ftwo\nabcdef1234\x1f1700000000 0\x1fAda\x1fthree\n");
        assert_eq!(b.lines, [0, 1, 0]);
        assert_eq!(b.commits[0].hash, "abcdef1");
        assert!(b.commits[1].hash.is_empty());
    }
}

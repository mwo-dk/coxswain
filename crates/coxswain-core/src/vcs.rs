//! Version control systems behind one seam: what the git line, the history and the list of
//! branches ask of a repository. Git, and Jujutsu through its `jj` CLI. Jujutsu is only read
//! (`--ignore-working-copy`: jj takes no snapshot and writes no operation); where a folder
//! has both `.jj` and `.git`, Jujutsu is asked unless `[git] prefer_git` says otherwise.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::branches::Branch;
use crate::git::{FileStatus, Kind, Status};
use crate::history::Commit;

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
}

/// `[git] prefer_git`: where a folder has both, git is asked, not Jujutsu.
static PREFER_GIT: AtomicBool = AtomicBool::new(false);
/// A Jujutsu repository was found and `jj` is not installed: a notice says what to install.
pub static JJ_MISSING: AtomicBool = AtomicBool::new(false);

/// Follow the settings: every config that is loaded passes here.
pub fn follow(cfg: &crate::config::GitConfig) {
    PREFER_GIT.store(cfg.prefer_git, Ordering::Relaxed);
}

/// The system that keeps the repository `dir` is in: Jujutsu when the nearest folder above
/// with `.jj` or `.git` has `.jj` (and the settings do not prefer git there), else git, which
/// finds its repository itself.
pub fn of(dir: &Path) -> &'static dyn Vcs {
    match jj_root(dir) {
        Some(_) => &Jj,
        None => &Git,
    }
}

/// The root of the Jujutsu repository `dir` is in, when it is one Jujutsu is asked about.
fn jj_root(dir: &Path) -> Option<PathBuf> {
    let root = dir
        .ancestors()
        .find(|a| a.join(".jj").is_dir() || a.join(".git").exists())?;
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
        crate::history::log(base, "HEAD", spec, max)
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
        parents: vec![],
        refs: vec![],
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
}

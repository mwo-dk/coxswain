//! Subversion and CVS working copies, read from their own records only: `.svn/wc.db` (a
//! SQLite database) and the `CVS/Entries` files. No server is asked and no program run, so a
//! working copy of a server far away, or gone, shows at once. Only the status: history, update
//! and commit are the server's, and go to a terminal (`handoff`).

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use crate::branches::Branch;
use crate::git::{FileStatus, Kind, Status};
use crate::history::Commit;
use crate::vcs::Vcs;

/// One file's status, counted, and its folders up to the root marked.
fn mark(st: &mut Status, path: PathBuf, kind: Kind) {
    let s = &mut st.summary;
    match kind {
        Kind::Modified => s.modified += 1,
        Kind::Deleted => s.deleted += 1,
        Kind::Untracked => s.untracked += 1,
        Kind::Conflict => s.conflicts += 1,
        Kind::Ignored => {}
        _ => s.staged += 1,
    }
    let root = st.root.clone();
    st.files.insert(path.clone(), FileStatus { kind, staged: false });
    for dir in path
        .ancestors()
        .skip(1)
        .take_while(|d| d.starts_with(&root) && *d != root)
    {
        let e = st
            .files
            .entry(dir.to_path_buf())
            .or_insert(FileStatus { kind, staged: false });
        e.kind = e.kind.max(kind);
    }
}

/// History and branches live on the server: never asked.
fn on_server(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        crate::t!("vcs.on_server", "system" => name),
    )
}

/// Whether `name` matches one of the ignore patterns (`*` and `?`).
fn ignored(name: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .any(|p| crate::index::glob(p.as_bytes(), name.as_bytes()))
}

/// A file's modification time in whole seconds and in microseconds since 1970.
fn mtime(meta: &std::fs::Metadata) -> Option<(i64, i64)> {
    let d = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((d.as_secs() as i64, d.as_micros() as i64))
}

// ---------------------------------------------------------------- Subversion

/// What Subversion ignores unless told otherwise (`global-ignores`).
const SVN_IGNORES: &[&str] = &[
    "*.o", "*.lo", "*.la", "*.al", ".libs", "*.so", "*.a", "*.pyc", "*.pyo", "__pycache__",
    "*.rej", "*~", "#*#", ".#*", ".*.swp", ".DS_Store", "Thumbs.db", "thumbs.db",
];

/// The top of the Subversion working copy `dir` is in: the nearest folder with `.svn` holding
/// `wc.db` (one for the whole working copy since Subversion 1.7).
pub fn svn_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|a| a.join(".svn/wc.db").is_file())
        .map(Path::to_path_buf)
}

/// The atoms of a Subversion skel, `(svn:ignore 12 build\n*.log\n)`: a length and a space
/// before raw bytes, or a bare word.
fn skel(b: &[u8]) -> Vec<String> {
    let (mut out, mut i) = (vec![], 0);
    while i < b.len() {
        match b[i] {
            b'(' | b')' | b' ' | b'\n' | b'\t' => i += 1,
            b'0'..=b'9' => {
                let start = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                let n: usize = std::str::from_utf8(&b[start..i]).ok().and_then(|s| s.parse().ok()).unwrap_or(0);
                if b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
                    i += 1;
                    let end = (i + n).min(b.len());
                    out.push(String::from_utf8_lossy(&b[i..end]).into_owned());
                    i = end;
                } else {
                    out.push(String::from_utf8_lossy(&b[start..i]).into_owned());
                }
            }
            _ => {
                let start = i;
                while i < b.len() && !matches!(b[i], b'(' | b')' | b' ' | b'\n' | b'\t') {
                    i += 1;
                }
                out.push(String::from_utf8_lossy(&b[start..i]).into_owned());
            }
        }
    }
    out
}

/// A property of a skel of properties.
fn prop(props: &[u8], name: &str) -> Option<String> {
    let atoms = skel(props);
    atoms.chunks(2).find(|kv| kv[0] == name).and_then(|kv| kv.get(1).cloned())
}

/// What the line says of a repository path: `trunk`, a branch's or tag's name, else its last
/// folder.
fn svn_branch(repos_path: &str) -> String {
    let parts: Vec<&str> = repos_path.split('/').filter(|s| !s.is_empty()).collect();
    if let Some(i) = parts.iter().position(|p| matches!(*p, "branches" | "tags"))
        && let Some(name) = parts.get(i + 1)
    {
        return name.to_string();
    }
    parts.last().map_or_else(|| "/".to_string(), |s| s.to_string())
}

struct Node {
    op_depth: i64,
    presence: String,
    kind: String,
    size: Option<i64>,
    mtime: Option<i64>,
    checksum: Option<String>,
    props: Option<Vec<u8>>,
    base: bool,
}

/// The status of the Subversion working copy at `root`, from `wc.db` and the files on disk.
pub fn svn_status(root: &Path) -> Option<Status> {
    let db = root.join(".svn/wc.db");
    let conn = rusqlite::Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
    let mut nodes: HashMap<String, Node> = HashMap::new();
    let mut root_path = (String::new(), 0i64);
    {
        let mut q = conn
            .prepare(
                "SELECT local_relpath, op_depth, presence, kind, translated_size, last_mod_time, \
                 checksum, properties, repos_path, revision FROM nodes ORDER BY local_relpath, op_depth",
            )
            .ok()?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    Node {
                        op_depth: r.get(1)?,
                        presence: r.get(2)?,
                        kind: r.get(3)?,
                        size: r.get(4)?,
                        mtime: r.get(5)?,
                        checksum: r.get(6)?,
                        props: r.get(7)?,
                        base: false,
                    },
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, Option<i64>>(9)?,
                ))
            })
            .ok()?;
        for (path, mut node, repos_path, rev) in rows.flatten() {
            if path.is_empty() && node.op_depth == 0 {
                root_path = (repos_path.unwrap_or_default(), rev.unwrap_or(0));
            }
            // Rows come by depth: the last is the working one; a depth-0 row is the base.
            let base = nodes.get(&path).is_some_and(|n| n.base || n.op_depth == 0);
            node.base = base || node.op_depth == 0;
            // The base's properties, unless the working row has its own.
            if node.props.is_none() {
                node.props = nodes.get(&path).and_then(|n| n.props.clone());
            }
            nodes.insert(path, node);
        }
    }
    let mut actual_props: HashMap<String, Vec<u8>> = HashMap::new();
    let mut conflicts: HashSet<String> = HashSet::new();
    if let Ok(mut q) = conn.prepare(
        "SELECT local_relpath, properties, conflict_data IS NOT NULL OR tree_conflict_data IS NOT NULL FROM actual_node",
    ) && let Ok(rows) = q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<Vec<u8>>>(1)?,
            r.get::<_, bool>(2)?,
        ))
    }) {
        for (path, props, conflict) in rows.flatten() {
            if conflict {
                conflicts.insert(path.clone());
            }
            if let Some(p) = props {
                actual_props.insert(path, p);
            }
        }
    }
    let mut st = Status {
        root: root.to_path_buf(),
        ..Default::default()
    };
    let abs = |rel: &str| {
        if rel.is_empty() {
            root.to_path_buf()
        } else {
            root.join(rel)
        }
    };
    let mut dirs = vec![];
    for (rel, n) in &nodes {
        let path = abs(rel);
        if conflicts.contains(rel) {
            mark(&mut st, path, Kind::Conflict);
            continue;
        }
        let kind = match n.presence.as_str() {
            "base-deleted" => Some(Kind::Deleted),
            "normal" if n.op_depth > 0 && n.base => Some(Kind::Modified),
            "normal" if n.op_depth > 0 => Some(Kind::Added),
            "normal" if n.kind == "dir" => {
                if path.is_dir() {
                    dirs.push(rel.clone());
                    None
                } else {
                    Some(Kind::Deleted)
                }
            }
            "normal" => match std::fs::symlink_metadata(&path) {
                Err(_) => Some(Kind::Deleted),
                Ok(meta) if n.kind == "file" => svn_changed(root, n, &path, &meta).then_some(Kind::Modified),
                Ok(_) => None,
            },
            _ => None,
        };
        if n.presence == "normal" && n.op_depth > 0 && n.kind == "dir" && path.is_dir() {
            dirs.push(rel.clone());
        }
        if let Some(kind) = kind {
            mark(&mut st, path, kind);
        }
    }
    // What is on disk and not in the working copy, by folder; an unknown folder is one entry.
    for rel in dirs {
        let dir = abs(&rel);
        let props = actual_props
            .get(&rel)
            .or_else(|| nodes.get(&rel).and_then(|n| n.props.as_ref()));
        let mut patterns: Vec<String> = SVN_IGNORES.iter().map(|s| s.to_string()).collect();
        if let Some(ignore) = props.and_then(|p| prop(p, "svn:ignore")) {
            patterns.extend(ignore.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from));
        }
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == ".svn" {
                continue;
            }
            let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if nodes.contains_key(&child) {
                continue;
            }
            let kind = if ignored(&name, &patterns) { Kind::Ignored } else { Kind::Untracked };
            mark(&mut st, e.path(), kind);
        }
    }
    let s = &mut st.summary;
    s.system = "svn";
    s.branch = svn_branch(&root_path.0);
    s.change = Some(format!("r{}", root_path.1));
    Some(st)
}

/// Whether a file differs from its pristine copy: by its size, else (when its time moved) by
/// its bytes.
fn svn_changed(root: &Path, n: &Node, path: &Path, meta: &std::fs::Metadata) -> bool {
    if n.size.is_some_and(|s| s != meta.len() as i64) {
        return true;
    }
    if mtime(meta).map(|m| m.1) == n.mtime {
        return false;
    }
    let Some(sha) = n.checksum.as_deref().and_then(|c| c.strip_prefix("$sha1$")) else {
        return true;
    };
    let pristine = root
        .join(".svn/pristine")
        .join(sha.get(..2).unwrap_or_default())
        .join(format!("{sha}.svn-base"));
    match (std::fs::read(&pristine), std::fs::read(path)) {
        (Ok(a), Ok(b)) => a != b,
        _ => true,
    }
}

pub(crate) struct Svn;

impl Vcs for Svn {
    fn name(&self) -> &'static str {
        "svn"
    }
    fn status(&self, dir: &Path) -> Option<Status> {
        svn_status(&svn_root(dir)?)
    }
    fn log(&self, _: &Path, _: &str, _: usize) -> io::Result<(Vec<Commit>, bool)> {
        Err(on_server("Subversion"))
    }
    fn branches(&self, _: &Path) -> io::Result<Vec<Branch>> {
        Err(on_server("Subversion"))
    }
    fn read_only(&self) -> io::Error {
        on_server("Subversion")
    }
}

// ---------------------------------------------------------------- CVS

/// What CVS ignores unless told otherwise.
const CVS_IGNORES: &[&str] = &[
    "RCS", "SCCS", "CVS", "CVS.adm", "RCSLOG", "cvslog.*", "tags", "TAGS", ".make.state",
    ".nse_depinfo", "*~", "#*", ".#*", ",*", "_$*", "*$", "*.old", "*.bak", "*.BAK", "*.orig",
    "*.rej", ".del-*", "*.a", "*.olb", "*.o", "*.obj", "*.so", "*.exe", "*.Z", "*.elc", "*.ln",
    "core",
];

/// The top of the CVS working copy `dir` is in: the highest folder of the unbroken run of
/// folders with `CVS/Entries` above it.
pub fn cvs_root(dir: &Path) -> Option<PathBuf> {
    let has = |d: &Path| d.join("CVS/Entries").is_file();
    let mut top = dir.ancestors().find(|d| has(d))?;
    while let Some(up) = top.parent().filter(|u| has(u)) {
        top = up;
    }
    Some(top.to_path_buf())
}

/// One line of `CVS/Entries`: a file (`/name/rev/time/options/tag`) or a folder (`D/name////`).
enum Entry {
    File { name: String, rev: String, time: String },
    Dir(String),
}

fn cvs_entries(dir: &Path) -> Vec<Entry> {
    let mut out = vec![];
    let mut removed: HashSet<String> = HashSet::new();
    let lines = std::fs::read_to_string(dir.join("CVS/Entries")).unwrap_or_default();
    // `Entries.Log` holds what was added (`A …`) and taken out (`R …`) since.
    let log = std::fs::read_to_string(dir.join("CVS/Entries.Log")).unwrap_or_default();
    let mut all: Vec<String> = lines.lines().map(String::from).collect();
    for l in log.lines() {
        if let Some(a) = l.strip_prefix("A ") {
            all.push(a.to_string());
        } else if let Some(r) = l.strip_prefix("R ") {
            removed.insert(r.to_string());
        }
    }
    for l in all.into_iter().filter(|l| !removed.contains(l)) {
        let (dir_entry, rest) = match l.strip_prefix("D/") {
            Some(r) => (true, r),
            None => match l.strip_prefix('/') {
                Some(r) => (false, r),
                None => continue,
            },
        };
        let f: Vec<&str> = rest.split('/').collect();
        if dir_entry {
            if let Some(n) = f.first().filter(|n| !n.is_empty()) {
                out.push(Entry::Dir(n.to_string()));
            }
        } else if let [name, rev, time, ..] = f[..] {
            out.push(Entry::File {
                name: name.to_string(),
                rev: rev.to_string(),
                time: time.to_string(),
            });
        }
    }
    out
}

/// `Wed Oct 21 10:00:00 2026` (UTC, as CVS writes it) in seconds since 1970.
fn cvs_time(s: &str) -> Option<i64> {
    let f: Vec<&str> = s.split_whitespace().collect();
    let [_, mon, day, hms, year] = f[..] else { return None };
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        .iter()
        .position(|m| *m == mon)? as i64
        + 1;
    let (day, year): (i64, i64) = (day.parse().ok()?, year.parse().ok()?);
    let t: Vec<i64> = hms.split(':').filter_map(|x| x.parse().ok()).collect();
    let [h, m, sec] = t[..] else { return None };
    // Days from 1970 by the civil calendar (Howard Hinnant's days_from_civil).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + m * 60 + sec)
}

/// The status of the CVS working copy at `root`, from its `CVS/Entries` and the files on disk.
pub fn cvs_status(root: &Path) -> Option<Status> {
    let mut st = Status {
        root: root.to_path_buf(),
        ..Default::default()
    };
    let home_ignores = crate::dirs::home_dir()
        .and_then(|h| std::fs::read_to_string(h.join(".cvsignore")).ok())
        .unwrap_or_default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = cvs_entries(&dir);
        let mut known: HashSet<String> = HashSet::from(["CVS".to_string()]);
        for e in &entries {
            match e {
                Entry::Dir(n) => {
                    known.insert(n.clone());
                    let d = dir.join(n);
                    if d.join("CVS/Entries").is_file() {
                        stack.push(d);
                    }
                }
                Entry::File { name, rev, time } => {
                    known.insert(name.clone());
                    let path = dir.join(name);
                    let kind = if rev == "0" {
                        Some(Kind::Added)
                    } else if rev.starts_with('-') {
                        Some(Kind::Deleted)
                    } else if time.contains('+') {
                        Some(Kind::Conflict)
                    } else {
                        match std::fs::metadata(&path) {
                            Err(_) => Some(Kind::Deleted),
                            Ok(meta) => {
                                let changed = time.starts_with("Result of merge")
                                    || cvs_time(time) != mtime(&meta).map(|m| m.0);
                                changed.then_some(Kind::Modified)
                            }
                        }
                    };
                    if let Some(kind) = kind {
                        mark(&mut st, path, kind);
                    }
                }
            }
        }
        let mut patterns: Vec<String> = CVS_IGNORES.iter().map(|s| s.to_string()).collect();
        for text in [home_ignores.clone(), std::fs::read_to_string(dir.join(".cvsignore")).unwrap_or_default()] {
            for p in text.split_whitespace() {
                if p == "!" {
                    patterns.clear();
                } else {
                    patterns.push(p.to_string());
                }
            }
        }
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if known.contains(&name) {
                continue;
            }
            let kind = if ignored(&name, &patterns) { Kind::Ignored } else { Kind::Untracked };
            mark(&mut st, e.path(), kind);
        }
    }
    let s = &mut st.summary;
    s.system = "cvs";
    // `CVS/Tag`: `T` a branch, `N` a tag, `D` a date the working copy is held at.
    s.branch = std::fs::read_to_string(root.join("CVS/Tag"))
        .ok()
        .and_then(|t| t.lines().next().map(|l| l.get(1..).unwrap_or_default().to_string()))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "HEAD".into());
    Some(st)
}

pub(crate) struct Cvs;

impl Vcs for Cvs {
    fn name(&self) -> &'static str {
        "cvs"
    }
    fn status(&self, dir: &Path) -> Option<Status> {
        cvs_status(&cvs_root(dir)?)
    }
    fn log(&self, _: &Path, _: &str, _: usize) -> io::Result<(Vec<Commit>, bool)> {
        Err(on_server("CVS"))
    }
    fn branches(&self, _: &Path) -> io::Result<Vec<Branch>> {
        Err(on_server("CVS"))
    }
    fn read_only(&self) -> io::Error {
        on_server("CVS")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wc_skel_and_branch_names() {
        assert_eq!(skel(b"(svn:ignore 12 build\n*.log\n)"), ["svn:ignore", "build\n*.log\n"]);
        assert_eq!(prop(b"(svn:eol-style 6 native svn:ignore 5 build)", "svn:ignore").as_deref(), Some("build"));
        assert_eq!(svn_branch("trunk"), "trunk");
        assert_eq!(svn_branch("project/branches/feature-x/src"), "feature-x");
        assert_eq!(svn_branch("tags/1.0"), "1.0");
        assert_eq!(svn_branch(""), "/");
        assert_eq!(cvs_time("Thu Jan  1 00:00:10 1970"), Some(10));
        assert_eq!(cvs_time("Wed Oct 21 10:00:00 2026"), Some(1_792_576_800));
    }

    fn top(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-wc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A CVS working copy as CVS leaves it: the files' states from `CVS/Entries` and the disk.
    #[test]
    fn wc_cvs_working_copy() {
        let d = top("cvs");
        let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_792_576_800);
        let file = |p: &str, text: &str| {
            let path = d.join(p);
            std::fs::write(&path, text).unwrap();
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
        };
        std::fs::create_dir_all(d.join("CVS")).unwrap();
        std::fs::create_dir_all(d.join("sub/CVS")).unwrap();
        let t = "Wed Oct 21 10:00:00 2026";
        std::fs::write(
            d.join("CVS/Entries"),
            format!("/same.txt/1.1/{t}//\n/changed.txt/1.2/{t}//\n/gone.txt/1.1/{t}//\n/new.txt/0/dummy timestamp//\n/old.txt/-1.3/{t}//\n/clash.txt/1.4/Result of merge+{t}//\nD/sub////\n"),
        )
        .unwrap();
        std::fs::write(d.join("CVS/Tag"), "Tstable-1\n").unwrap();
        std::fs::write(d.join("sub/CVS/Entries"), format!("/inner.txt/1.1/{t}//\nD\n")).unwrap();
        file("same.txt", "a");
        file("changed.txt", "b");
        std::fs::write(d.join("changed.txt"), "b2").unwrap();
        file("new.txt", "n");
        file("clash.txt", "c");
        file("sub/inner.txt", "i");
        file("notes.txt", "?");
        file("build.o", "o");
        let st = cvs_status(&cvs_root(&d.join("sub")).unwrap()).unwrap();
        let k = |p: &str| st.get(&d.join(p)).map(|f| f.kind);
        assert_eq!(k("same.txt"), None);
        assert_eq!(k("sub/inner.txt"), None);
        assert_eq!(k("changed.txt"), Some(Kind::Modified));
        assert_eq!(k("gone.txt"), Some(Kind::Deleted));
        assert_eq!(k("new.txt"), Some(Kind::Added));
        assert_eq!(k("old.txt"), Some(Kind::Deleted));
        assert_eq!(k("clash.txt"), Some(Kind::Conflict));
        assert_eq!(k("notes.txt"), Some(Kind::Untracked));
        assert_eq!(k("build.o"), Some(Kind::Ignored));
        assert_eq!(st.prompt(&crate::config::Glyphs::ascii()), "cvs: stable-1 +1 ~1 -2 ?1 !1");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A Subversion working copy made by the svn CLI, when it is installed (else the test
    /// passes): its status read from wc.db alone matches what `svn status` says.
    #[test]
    fn wc_svn_working_copy() {
        if !crate::user_menu::installed("svn") || !crate::user_menu::installed("svnadmin") {
            return;
        }
        let d = top("svn");
        let run = |dir: &Path, args: &[&str]| {
            let ok = std::process::Command::new(args[0])
                .current_dir(dir)
                .args(&args[1..])
                .env("HOME", &d)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(ok.success(), "{args:?}");
        };
        run(&d, &["svnadmin", "create", "repo"]);
        let url = format!("file://{}/repo", d.display());
        run(&d, &["svn", "mkdir", "-q", "-m", "layout", &format!("{url}/trunk")]);
        run(&d, &["svn", "checkout", "-q", &format!("{url}/trunk"), "wc"]);
        let wc = d.join("wc");
        std::fs::write(wc.join("a.txt"), "a\n").unwrap();
        std::fs::create_dir(wc.join("sub")).unwrap();
        std::fs::write(wc.join("sub/b.txt"), "b\n").unwrap();
        std::fs::write(wc.join("same.txt"), "s\n").unwrap();
        run(&wc, &["svn", "add", "-q", "a.txt", "sub", "same.txt"]);
        run(&wc, &["svn", "commit", "-q", "-m", "first one"]);
        run(&wc, &["svn", "update", "-q"]);
        run(&wc, &["svn", "propset", "-q", "svn:ignore", "build", "."]);
        std::fs::write(wc.join("a.txt"), "a\nc\n").unwrap();
        std::fs::write(wc.join("new.txt"), "n\n").unwrap();
        run(&wc, &["svn", "add", "-q", "new.txt"]);
        run(&wc, &["svn", "rm", "-q", "sub/b.txt"]);
        std::fs::write(wc.join("notes.txt"), "?\n").unwrap();
        std::fs::create_dir(wc.join("build")).unwrap();
        // Touched, not changed: the bytes are compared.
        std::fs::File::options().write(true).open(wc.join("same.txt")).unwrap().set_modified(std::time::SystemTime::now()).unwrap();
        let st = Svn.status(&wc.join("sub")).expect("status");
        let k = |p: &str| st.get(&wc.join(p)).map(|f| f.kind);
        assert_eq!(k("a.txt"), Some(Kind::Modified));
        assert_eq!(k("same.txt"), None);
        assert_eq!(k("new.txt"), Some(Kind::Added));
        assert_eq!(k("sub/b.txt"), Some(Kind::Deleted));
        assert_eq!(k("notes.txt"), Some(Kind::Untracked));
        assert_eq!(k("build"), Some(Kind::Ignored));
        assert_eq!(st.prompt(&crate::config::Glyphs::ascii()), "svn: r2 trunk +1 ~1 -1 ?1");
        std::fs::remove_dir_all(d).unwrap();
    }
}

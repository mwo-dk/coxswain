//! Subversion and CVS working copies, read from their own records only: `.svn/wc.db` (a
//! SQLite database) and the `CVS/Entries` files. No server is asked and no program run, so a
//! working copy of a server far away, or gone, shows at once. The history is the server's: it is
//! read with `svn log` or `cvs log` only once the user has said yes to that server (`server`,
//! `allow`); then a commit opened is read from it too (`svn list`/`cat`/`diff`, `cvs update -p`
//! and `diff`), only reading. Update and commit go to a terminal (`handoff`).

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use crate::branches::Branch;
use crate::git::{FileStatus, Kind, Status};
use crate::history::{At, Commit};
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

/// `wc.db`, opened to be read only and trusting nothing in it: defensive mode, no functions
/// from its schema, `nodes` and `actual_node` plain tables (not views), and every statement
/// stopped after two seconds.
fn svn_db(db: &Path) -> Option<rusqlite::Connection> {
    use rusqlite::config::DbConfig;
    // `immutable`: SQLite takes no lock and opens no `-wal`, `-shm` or `-journal` beside it,
    // which a hostile working copy could make pipes or links.
    let conn = rusqlite::Connection::open_with_flags(
        format!("{}?immutable=1&mode=ro", file_uri(db)?),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
            | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .ok()?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true).ok()?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false).ok()?;
    conn.execute_batch("PRAGMA trusted_schema = OFF; PRAGMA query_only = ON;").ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
    let start = std::time::Instant::now();
    conn.progress_handler(10_000, Some(move || start.elapsed() > std::time::Duration::from_secs(2)))
        .ok()?;
    let plain: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND lower(name) IN ('nodes', 'actual_node')",
            [],
            |r| r.get(0),
        )
        .ok()?;
    let other: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type <> 'table' AND lower(name) IN ('nodes', 'actual_node')",
            [],
            |r| r.get(0),
        )
        .ok()?;
    (plain == 2 && other == 0).then_some(conn)
}

/// `path` as a SQLite `file:` URI: every byte but letters, digits, `-._~/` percent-encoded,
/// `\\` as `/`, and a drive (`C:`) after a `/`. None for a path that is not UTF-8: its URI
/// would name another file.
fn file_uri(path: &Path) -> Option<String> {
    let p = path.to_str()?.replace('\\', "/");
    let p = if p.starts_with('/') { p } else { format!("/{p}") };
    let mut out = String::from("file:");
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) || (cfg!(windows) && b == b':') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    Some(out)
}

/// A path as `wc.db` holds it: `/`-separated, relative, nothing that climbs out or names a
/// drive.
fn safe_relpath(rel: &str) -> bool {
    rel.is_empty()
        || (!rel.contains(['\\', '\0'])
            && !(cfg!(windows) && rel.contains(':'))
            && rel.split('/').all(|c| !c.is_empty() && c != "." && c != ".."))
}

/// The SHA-1 of a `$sha1$…` checksum: 40 hex digits, or none.
fn sha1_of(checksum: &str) -> Option<&str> {
    checksum
        .strip_prefix("$sha1$")
        .filter(|h| h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The status of the Subversion working copy at `root`, from `wc.db` and the files on disk.
pub fn svn_status(root: &Path) -> Option<Status> {
    // `.svn` and its database are the working copy's own, not links to elsewhere.
    let real_dir = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir());
    let db = root.join(".svn/wc.db");
    if !real_dir(&root.join(".svn")) || !std::fs::symlink_metadata(&db).is_ok_and(|m| m.is_file()) {
        return None;
    }
    let conn = svn_db(&db)?;
    let mut nodes: HashMap<String, Node> = HashMap::new();
    let mut root_path = (String::new(), 0i64);
    {
        let mut q = conn
            .prepare(
                "SELECT local_relpath, op_depth, presence, kind, translated_size, last_mod_time, \
                 checksum, properties, repos_path, revision FROM nodes ORDER BY local_relpath, op_depth \
                 LIMIT 1000000",
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
            // A working copy may be someone else's (an unpacked archive): a path that would
            // leave it, or a checksum that is not one, is not believed.
            if !safe_relpath(&path) {
                continue;
            }
            if node.checksum.as_deref().is_some_and(|c| sha1_of(c).is_none()) {
                node.checksum = None;
            }
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
        "SELECT local_relpath, properties, conflict_data IS NOT NULL OR tree_conflict_data IS NOT NULL FROM actual_node LIMIT 1000000",
    ) && let Ok(rows) = q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<Vec<u8>>>(1)?,
            r.get::<_, bool>(2)?,
        ))
    }) {
        for (path, props, conflict) in rows.flatten().filter(|(p, ..)| safe_relpath(p)) {
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
    // `wc.db` names paths with `/`; on Windows the files map holds `\\`.
    let abs = |rel: &str| {
        if rel.is_empty() {
            root.to_path_buf()
        } else {
            root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
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
        // ponytail: 100k unknown entries a folder at most; enough for any working copy one browses.
        for e in read.flatten().take(100_000) {
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
    // Only a plain file is compared, never what a link points at.
    if !meta.is_file() {
        return true;
    }
    let Some(sha) = n.checksum.as_deref().and_then(sha1_of) else {
        return true;
    };
    let store = root.join(".svn/pristine");
    if !std::fs::symlink_metadata(&store).is_ok_and(|m| m.is_dir()) {
        return true;
    }
    let pristine = store
        .join(&sha[..2])
        .join(format!("{sha}.svn-base"));
    let cap = n.size.map_or(meta.len(), |s| s.max(0) as u64);
    match (open_plain(&pristine), open_plain(path)) {
        (Ok(a), Ok(b)) => !same_bytes(a, b, cap),
        _ => true,
    }
}

/// A plain file opened for reading, never through a link.
fn open_plain(path: &Path) -> io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    // Never through a link, and never waiting on a pipe put where a file was.
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let f = o.open(path)?;
    if f.metadata()?.is_file() {
        Ok(f)
    } else {
        Err(io::Error::other("not a file"))
    }
}

/// Whether two files hold the same bytes, read side by side, at most `cap` and one more.
fn same_bytes(a: std::fs::File, b: std::fs::File, cap: u64) -> bool {
    use std::io::Read;
    let (mut a, mut b) = (a.take(cap + 1), b.take(cap + 1));
    let (mut x, mut y) = ([0u8; 64 * 1024], [0u8; 64 * 1024]);
    loop {
        let n = match a.read(&mut x) {
            Ok(n) => n,
            Err(_) => return false,
        };
        let mut got = 0;
        while got < n {
            match b.read(&mut y[got..n]) {
                Ok(0) | Err(_) => return false,
                Ok(m) => got += m,
            }
        }
        if x[..n] != y[..n] {
            return false;
        }
        if n == 0 {
            return matches!(b.read(&mut y[..1]), Ok(0));
        }
    }
}

/// The URL of the repository the working copy at `root` is from, as `wc.db` has it.
fn svn_url(root: &Path) -> Option<String> {
    let db = root.join(".svn/wc.db");
    if !std::fs::symlink_metadata(&db).is_ok_and(|m| m.is_file()) {
        return None;
    }
    let conn = svn_db(&db)?;
    let mut q = conn.prepare("SELECT root FROM repository LIMIT 2").ok()?;
    let roots: Vec<String> = q.query_map([], |r| r.get(0)).ok()?.flatten().collect();
    match &roots[..] {
        [one] => Some(one.clone()),
        _ => None,
    }
}

/// A host name as a server is named: letters, digits, `.`, `-`, `_`, never starting with `-`
/// (an option to ssh), or an IPv6 address in brackets.
fn host_ok(h: &str) -> bool {
    let v6 = h.starts_with('[') && h.ends_with(']') && h.len() > 2 && h[1..h.len() - 1].bytes().all(|b| b.is_ascii_hexdigit() || b == b':');
    v6 || (!h.is_empty()
        && !h.starts_with('-')
        && h.len() <= 253
        && h.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')))
}

/// The server of a Subversion repository URL, as the question names it: `https://host[:port]`
/// (no user), or the whole `file://` URL for a repository on this machine. `None` for a URL
/// Coxswain will not hand to svn: another scheme, or a host that is no host name.
pub fn svn_server_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if url.chars().any(|c| c.is_control()) {
        return None;
    }
    if scheme == "file" {
        return Some(url.trim_end_matches('/').to_string());
    }
    if !matches!(scheme, "http" | "https" | "svn" | "svn+ssh") {
        return None;
    }
    let auth = rest.split('/').next()?;
    let (user, hostport) = auth.rsplit_once('@').map_or((None, auth), |(u, h)| (Some(u), h));
    // A user is no option to ssh, written plainly or `%`-escaped.
    if user.is_some_and(|u| u.starts_with('-') || u.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("%2d"))) {
        return None;
    }
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(':') && !p.contains(']') => (h, Some(p)),
        _ => (hostport, None),
    };
    if !host_ok(host) || port.is_some_and(|p| p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    Some(match port {
        Some(p) => format!("{scheme}://{host}:{p}"),
        None => format!("{scheme}://{host}"),
    })
}

/// Servers said yes to (`AppState::history_servers`), as the apps hand them over.
static ALLOWED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The servers whose history may be read: the apps pass them at start and after each change.
pub fn allow(servers: &[String]) {
    *ALLOWED.lock().unwrap_or_else(|e| e.into_inner()) = servers.to_vec();
}

fn allowed(server: &str) -> bool {
    ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|s| s == server)
}

/// The server the history of the Subversion or CVS working copy `path` is in is read from,
/// named as the question and Settings name it. `None` elsewhere, and for a working copy whose
/// server Coxswain would not contact (a scheme or a host it does not take).
pub fn server(path: &Path) -> Option<String> {
    match crate::vcs::of(path).name() {
        "svn" => svn_server_of(&svn_url(&svn_root(path)?)?),
        "cvs" => cvs_root_of(&cvs_dir(path)?).map(|(id, _)| id),
        _ => None,
    }
}

/// The question asked before a Subversion or CVS history is read: the server, and the system
/// and its program, as the question names them.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Ask {
    pub server: String,
    pub system: &'static str,
    pub program: &'static str,
}

impl Ask {
    /// The question's text.
    pub fn text(&self) -> String {
        crate::t!("history.ask_server_text", "system" => self.system, "server" => &self.server, "program" => self.program)
    }
}

/// The question is due before the history of `path` is opened: when it is a Subversion or CVS
/// working copy whose server was not said yes to.
pub fn ask_first(path: &Path) -> Option<Ask> {
    let server = server(path).filter(|s| !allowed(s))?;
    let (system, program) = match crate::vcs::of(path).name() {
        "svn" => ("Subversion", "svn"),
        _ => ("CVS", "cvs"),
    };
    Some(Ask { server, system, program })
}

/// Not said yes to: refused, naming the server.
fn not_asked(server: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        crate::t!("vcs.server_not_asked", "server" => server),
    )
}

/// Subversion or CVS is not installed: the notice is due.
pub static SVN_MISSING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static CVS_MISSING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn need(program: &str, system: &str, flag: &std::sync::atomic::AtomicBool) -> io::Result<()> {
    if crate::user_menu::installed(program) {
        return Ok(());
    }
    flag.store(true, std::sync::atomic::Ordering::Relaxed);
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        crate::t!("notice.wc_tool_missing", "system" => system, "program" => program),
    ))
}

/// What a server read may print: more is cut.
const SERVER_MAX: usize = 32 << 20;
/// How long a server read may take.
const SERVER_TIME: std::time::Duration = std::time::Duration::from_secs(30);

/// Runs `c`: at most `max` bytes of what it prints go to `out`, within `SERVER_TIME`. Returns
/// whether it was cut, and its complaint when it failed (a run cut short was stopped, so that
/// is no failure).
fn run_server(mut c: std::process::Command, out: &mut dyn io::Write, max: usize) -> io::Result<(bool, Option<String>)> {
    use std::io::Read;
    c.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // Messages in English, dates as the tools write them by default.
        .env("LC_ALL", "C");
    let mut child = c.spawn()?;
    let (mut stdout, mut err) = (child.stdout.take().ok_or_else(|| io::Error::other("no output"))?, child.stderr.take());
    let errs = std::thread::spawn(move || {
        let mut e = vec![];
        if let Some(err) = err.as_mut() {
            let _ = err.take(64 << 10).read_to_end(&mut e);
        }
        e
    });
    // Stopped when it takes too long: the read below then ends.
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let watch = std::thread::spawn(move || {
        let late = rx.recv_timeout(SERVER_TIME) == Err(std::sync::mpsc::RecvTimeoutError::Timeout);
        let _ = child.kill();
        (late, child.wait())
    });
    let copied = io::copy(&mut (&mut stdout).take(max as u64), out);
    let cut = copied.is_ok() && stdout.read(&mut [0u8]).is_ok_and(|n| n > 0);
    drop(stdout);
    drop(tx);
    let (late, status) = watch.join().map_err(|_| io::Error::other("watch"))?;
    let status = status?;
    let errs = errs.join().unwrap_or_default();
    // Writing failed (a full disk): that is the error.
    copied?;
    if late {
        return Err(io::Error::new(io::ErrorKind::TimedOut, crate::t!("vcs.server_slow")));
    }
    let failed = (!status.success() && !cut).then(|| {
        let why = String::from_utf8_lossy(&errs).trim().lines().last().unwrap_or_default().to_string();
        if why.is_empty() { "failed".to_string() } else { why }
    });
    Ok((cut, failed))
}

/// What `c` prints, at most `max` bytes, and whether it was cut: kept even when it then fails
/// (`cvs diff` fails when files differ, `cvs log` on a file it cannot read); its complaint is
/// the error only when it printed nothing. Never for a file's bytes: `file_from_server`.
fn from_server(c: std::process::Command, max: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut buf = vec![];
    match run_server(c, &mut buf, max)? {
        (_, Some(why)) if buf.is_empty() => Err(io::Error::other(why)),
        (cut, _) => Ok((buf, cut)),
    }
}

/// As `from_server`, but a run that fails is an error even after it printed part: a file's
/// bytes, a listing of what is copied out.
fn file_from_server(c: std::process::Command, max: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut buf = vec![];
    match run_server(c, &mut buf, max)? {
        (_, Some(why)) => Err(io::Error::other(why)),
        (cut, None) => Ok((buf, cut)),
    }
}

/// A file copied out, written as it comes into `out`, never held whole: refused when the run
/// fails or the file is larger than `OUT_MAX`.
fn file_to(c: std::process::Command, out: &mut std::fs::File, name: &str, cap: u64) -> io::Result<()> {
    let cap = cap.min(OUT_MAX as u64) as usize;
    match run_server(c, out, cap)? {
        (_, Some(why)) => Err(io::Error::other(why)),
        // Cut by what a drag may take in all, not by the size of one file.
        (true, None) if cap < OUT_MAX => Err(io::Error::new(io::ErrorKind::FileTooLarge, crate::t!("drag.too_much"))),
        (true, None) => Err(io::Error::new(io::ErrorKind::FileTooLarge, format!("{name}: larger than 1 GB"))),
        (false, None) => Ok(()),
    }
}

/// What svn's XML holds, as `svn_xml` walks it: an element's start with its attributes, and its
/// end with its text.
enum X<'a> {
    Start(&'a str, Vec<(String, String)>),
    End(&'a str, &'a str),
}

/// Walk svn's XML (`--xml`). A DTD ends it: svn never writes one, and refusing it rules out
/// entity expansion.
fn svn_xml(xml: &[u8], mut on: impl FnMut(X)) {
    use quick_xml::events::Event;
    let xml = String::from_utf8_lossy(xml);
    let mut r = quick_xml::Reader::from_str(&xml);
    let mut text = String::new();
    let name = |n: &str| n.to_string();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                text.clear();
                let attrs = e
                    .attributes()
                    .flatten()
                    .map(|a| (name(a.key.local_name().as_ref()), a.value.to_string()))
                    .collect();
                on(X::Start(&name(e.local_name().as_ref()), attrs));
            }
            Ok(Event::Text(t)) => text.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0)),
            Ok(Event::GeneralRef(g)) => match g.resolve_char_ref() {
                Ok(Some(c)) => text.push(c),
                _ => text.push_str(quick_xml::escape::resolve_xml_entity(&g).unwrap_or_default()),
            },
            Ok(Event::End(e)) => {
                on(X::End(&name(e.local_name().as_ref()), text.trim()));
                text.clear();
            }
            Ok(Event::DocType(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// `svn log --xml` as commits, newest first: `r<revision>`, its time, author and first line.
pub fn parse_svn_log(xml: &[u8]) -> Vec<Commit> {
    let (mut out, mut now) = (vec![], None::<Commit>);
    svn_xml(xml, |x| match x {
        X::Start("logentry", a) => {
            now = attr(&a, "revision")
                .and_then(|r| r.parse::<u64>().ok())
                .map(|rev| Commit { hash: format!("r{rev}"), ..Default::default() })
        }
        X::End(tag, text) => match (tag, now.as_mut()) {
            ("author", Some(c)) => c.author = text.to_string(),
            ("date", Some(c)) => {
                c.time = chrono::DateTime::parse_from_rfc3339(text).map_or(0, |d| d.timestamp().max(0) as u64)
            }
            ("msg", Some(c)) => c.subject = text.lines().next().unwrap_or_default().to_string(),
            ("logentry", _) => out.extend(now.take()),
            _ => {}
        },
        _ => {}
    });
    out
}

/// `svn list --xml` (or `info --xml`) as entries: path, whether a folder, size.
pub fn parse_svn_list(xml: &[u8]) -> Vec<(String, bool, u64)> {
    let (mut out, mut now) = (vec![], None::<(String, bool, u64)>);
    svn_xml(xml, |x| match x {
        X::Start("entry", a) => now = Some((String::new(), attr(&a, "kind") == Some("dir"), 0)),
        X::End(tag, text) => match (tag, now.as_mut()) {
            ("name" | "url", Some(e)) => e.0 = text.to_string(),
            ("size", Some(e)) => e.2 = text.parse().unwrap_or(0),
            ("entry", _) => out.extend(now.take()),
            _ => {}
        },
        _ => {}
    });
    out
}

/// Commits read from servers, by the working copy folder and id: a commit never changes, and
/// a listing inside one asks for it each time.
static SEEN: std::sync::Mutex<Vec<(PathBuf, Commit)>> = std::sync::Mutex::new(Vec::new());

fn seen(base: &Path, commits: &[Commit]) {
    let mut s = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    // ponytail: dropped whole past 20,000; an LRU if long sessions over many servers need it.
    if s.len() > 20_000 {
        s.clear();
    }
    for c in commits {
        if !s.iter().any(|(b, k)| b == base && k.hash == c.hash) {
            s.push((base.to_path_buf(), c.clone()));
        }
    }
}

fn seen_before(base: &Path, rev: &str) -> Option<Commit> {
    let s = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    s.iter().find(|(b, c)| b == base && !rev.is_empty() && c.hash.starts_with(rev)).map(|(_, c)| c.clone())
}

fn no_commit(rev: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("no commit {rev}"))
}

fn not_in(inner: &str, rev: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        crate::t!("history.not_in_commit", "path" => if inner.is_empty() { "." } else { inner }, "rev" => rev),
    )
}

/// A path for a URL: every byte but letters, digits and `-._~/` percent-encoded.
fn uri_path(p: &str) -> String {
    p.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// A name a listing may show: one path part, no control characters.
fn name_ok(n: &str) -> bool {
    !n.is_empty() && n != "." && n != ".." && !n.contains(['/', '\\']) && !n.chars().any(char::is_control)
}

/// The Subversion working copy `base` is in, once its server was said yes to: the
/// repository's URL, `base`'s URL and the revision it was checked out at, from `wc.db`.
fn svn_here(base: &Path) -> io::Result<(String, String, u64)> {
    let root = svn_root(base).ok_or_else(|| on_server("Subversion"))?;
    let repo = svn_url(&root).ok_or_else(|| on_server("Subversion"))?;
    let server = svn_server_of(&repo).ok_or_else(|| on_server("Subversion"))?;
    if !allowed(&server) {
        return Err(not_asked(&server));
    }
    need("svn", "Subversion", &SVN_MISSING)?;
    let rel = base.strip_prefix(&root).ok().and_then(Path::to_str).ok_or_else(|| on_server("Subversion"))?.replace('\\', "/");
    let conn = svn_db(&root.join(".svn/wc.db")).ok_or_else(|| on_server("Subversion"))?;
    let (path, rev): (String, i64) = conn
        .query_row(
            "SELECT repos_path, revision FROM nodes WHERE local_relpath = ?1 AND op_depth = 0",
            [&rel],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| not_in(&rel, "BASE"))?;
    if !safe_relpath(&path) {
        return Err(on_server("Subversion"));
    }
    let url = match path.as_str() {
        "" => repo.clone(),
        p => format!("{}/{}", repo.trim_end_matches('/'), uri_path(p)),
    };
    Ok((repo, url, rev.max(0) as u64))
}

/// svn, never asking anything on the terminal.
fn svn() -> std::process::Command {
    let mut c = crate::tools::command("svn");
    c.arg("--non-interactive");
    c
}

/// The revision a Subversion commit id names: `r12` is 12.
fn svn_rev(rev: &str) -> io::Result<u64> {
    rev.strip_prefix('r').and_then(|n| n.parse().ok()).ok_or_else(|| no_commit(rev))
}

/// Where the folder `base` was at revision `n`: its URL then, traced from where it is now
/// (it may have moved since), in the same repository. Kept: it never changes.
fn svn_at(base: &Path, n: u64) -> io::Result<String> {
    static AT: std::sync::Mutex<Vec<((PathBuf, u64), String)>> = std::sync::Mutex::new(Vec::new());
    let (repo, url, peg) = svn_here(base)?;
    let key = (base.to_path_buf(), n);
    if let Some((_, u)) = AT.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| *k == key) {
        return Ok(u.clone());
    }
    let mut c = svn();
    c.args(["info", "--xml", "-r", &n.to_string(), "--", &format!("{url}@{peg}")]);
    let then = from_server(c, 1 << 20)
        .ok()
        .and_then(|(xml, _)| parse_svn_list(&xml).into_iter().next())
        .map(|(u, ..)| u)
        // In the same repository: never another server.
        .filter(|u| {
            let repo = repo.trim_end_matches('/');
            !u.chars().any(char::is_control) && (u == repo || u.strip_prefix(repo).is_some_and(|r| r.starts_with('/')))
        })
        .unwrap_or(url);
    let mut at = AT.lock().unwrap_or_else(|e| e.into_inner());
    if at.len() > 10_000 {
        at.clear();
    }
    at.push((key, then.clone()));
    Ok(then)
}

/// The URL of `inner` below `base` at revision `n`, with `n` as its peg: `…/inner@n`.
fn svn_url_at(base: &Path, n: u64, inner: &str) -> io::Result<String> {
    let at = svn_at(base, n)?;
    Ok(match inner {
        "" => format!("{at}@{n}"),
        i => format!("{at}/{}@{n}", uri_path(i)),
    })
}

/// The bytes of the file at `url` (with its peg), at most `max`, and whether there was more.
fn svn_cat(url: &str, max: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut c = svn();
    c.args(["cat", "--", url]);
    file_from_server(c, max)
}

/// The history of `spec` (`.` or a name in `base`) from the Subversion server, once said yes to.
fn svn_log(base: &Path, spec: &str, max: usize) -> io::Result<(Vec<Commit>, bool)> {
    let root = svn_root(base).ok_or_else(|| on_server("Subversion"))?;
    let server = svn_url(&root).and_then(|u| svn_server_of(&u)).ok_or_else(|| on_server("Subversion"))?;
    if !allowed(&server) {
        return Err(not_asked(&server));
    }
    need("svn", "Subversion", &SVN_MISSING)?;
    let mut c = svn();
    c.current_dir(base).args(["log", "--xml", "--limit", &(max + 1).to_string(), "--"]);
    // A trailing `@`: a name with `@` in it is never taken for a revision.
    c.arg(format!("{spec}@"));
    let (out, cut) = from_server(c, SERVER_MAX)?;
    let mut commits = parse_svn_log(&out);
    let more = cut || commits.len() > max;
    commits.truncate(max);
    seen(base, &commits);
    Ok((commits, more))
}

/// Files fetched one by one into `to` (not there yet): (the path below `to`, `""` for `to`
/// itself; what `cat` fetches it by). Never outside `to` nor a repository's folder there
/// (`vcs::safe_out`); each file and folder gets `when` as its time. Files only: links and the
/// executable bit are not carried over. `cat` gets the bytes still allowed of `max` in all.
fn write_out(to: &Path, when: std::time::SystemTime, files: Vec<(String, String)>, max: u64, cat: impl Fn(&str, &mut std::fs::File, u64) -> io::Result<()>) -> io::Result<()> {
    let files: Vec<(PathBuf, String)> = files
        .into_iter()
        .filter(|(rest, _)| crate::vcs::safe_out(rest))
        .map(|(rest, src)| (if rest.is_empty() { to.to_path_buf() } else { to.join(rest) }, src))
        .collect();
    // ponytail: one program run a file; one batch read if large folders copied out are slow.
    let mut used = 0;
    for (dst, src) in &files {
        if let Some(dir) = dst.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::File::create_new(dst)?;
        cat(src, &mut f, max - used)?;
        used += f.metadata()?.len().min(max - used);
        let _ = f.set_modified(when);
    }
    crate::vcs::date_folders(to, files.iter().map(|(d, _)| d.as_path()), when);
    Ok(())
}

/// What a Subversion or CVS commit's diff may hold, all its files together: more is cut.
/// `history::diff` shows 512K characters.
const DIFF_MAX: usize = 2 << 20;

/// A diff cut short says so on its first line.
fn cut_note(diff: Vec<u8>, cut: bool) -> Vec<u8> {
    if !cut {
        return diff;
    }
    let mut out = format!("{}\n", crate::t!("history.diff_cut")).into_bytes();
    out.extend(diff);
    out
}

/// What a file copied out may hold: more is refused, not cut.
const OUT_MAX: usize = 1 << 30;

/// What copying `at` out at `rev` writes: (the path below the copy, `""` for a file itself;
/// its URL with the revision; its size). Refused when the server's list fails or is cut.
fn svn_out(at: &At, rev: &str) -> io::Result<Vec<(String, String, u64)>> {
    let n = svn_rev(rev)?;
    let url = svn_url_at(&at.base, n, &at.inner)?;
    let mut c = svn();
    c.args(["info", "--xml", "--", &url]);
    let is_dir = parse_svn_list(&file_from_server(c, 1 << 20)?.0).first().is_some_and(|e| e.1);
    let mut c = svn();
    c.args(["list", "--xml"]);
    if is_dir {
        c.arg("-R");
    }
    c.args(["--", &url]);
    let (xml, cut) = file_from_server(c, SERVER_MAX)?;
    if cut {
        return Err(io::Error::new(io::ErrorKind::FileTooLarge, "svn list: more than 32 MB"));
    }
    let list = parse_svn_list(&xml);
    let files: Vec<(String, String, u64)> = if is_dir {
        let (at_url, _) = url.rsplit_once('@').unwrap_or((&url, ""));
        list.into_iter()
            .filter(|(p, dir, _)| !dir && p.split('/').all(name_ok) && crate::vcs::safe_out(p))
            .map(|(p, _, size)| (p.clone(), format!("{at_url}/{}@{n}", uri_path(&p)), size))
            .collect()
    } else {
        list.into_iter().take(1).map(|(.., size)| (String::new(), url.clone(), size)).collect()
    };
    if files.is_empty() {
        return Err(not_in(&at.inner, rev));
    }
    Ok(files)
}

/// What copying a Subversion or CVS `at` out at `rev` takes: bytes and files. CVS lists no
/// sizes without reading every file, so its bytes are 0; a file copied out is written as it
/// comes, never held whole.
pub(crate) fn measure(at: &At, rev: &str) -> io::Result<(u64, usize)> {
    if crate::vcs::of(&at.base).name() == "svn" {
        return Ok(svn_out(at, rev)?.iter().fold((0, 0), |(b, n), f| (b + f.2, n + 1)));
    }
    Ok((0, cvs_out(&*cvs_known(&at.base)?, at, rev)?.len()))
}

pub(crate) struct Svn;

impl Vcs for Svn {
    fn name(&self) -> &'static str {
        "svn"
    }
    fn status(&self, dir: &Path) -> Option<Status> {
        svn_status(&svn_root(dir)?)
    }
    fn log(&self, base: &Path, spec: &str, max: usize) -> io::Result<(Vec<Commit>, bool)> {
        svn_log(base, spec, max)
    }
    fn show(&self, base: &Path, rev: &str) -> io::Result<Commit> {
        let n = svn_rev(rev)?;
        if let Some(c) = seen_before(base, &format!("r{n}")).filter(|c| c.hash == format!("r{n}")) {
            return Ok(c);
        }
        let (repo, ..) = svn_here(base)?;
        let mut c = svn();
        c.args(["log", "--xml", "-r", &n.to_string(), "--limit", "1", "--", &format!("{repo}@{n}")]);
        let found = parse_svn_log(&from_server(c, 1 << 20)?.0);
        seen(base, &found);
        found.into_iter().next().ok_or_else(|| no_commit(rev))
    }
    fn tree(&self, base: &Path, rev: &str, inner: &str) -> io::Result<Vec<(String, &'static str, u64)>> {
        let n = svn_rev(rev)?;
        let mut c = svn();
        c.args(["list", "--xml", "--", &svn_url_at(base, n, inner)?]);
        let (xml, _) = from_server(c, SERVER_MAX)?;
        Ok(parse_svn_list(&xml)
            .into_iter()
            .filter(|(name, ..)| name_ok(name))
            .map(|(name, dir, size)| (name, if dir { "tree" } else { "blob" }, size))
            .collect())
    }
    fn read(&self, base: &Path, rev: &str, inner: &str, max: usize) -> io::Result<(Vec<u8>, bool)> {
        svn_cat(&svn_url_at(base, svn_rev(rev)?, inner)?, max)
    }
    fn diff(&self, base: &Path, rev: &str, inner: &str) -> io::Result<Vec<u8>> {
        let n = svn_rev(rev)?;
        let mut c = svn();
        // `--internal-diff`: never the diff program the user's own config names.
        c.args(["diff", "--internal-diff", "-c", &n.to_string(), "--", &svn_url_at(base, n, inner)?]);
        let (out, cut) = from_server(c, DIFF_MAX)?;
        Ok(cut_note(out, cut))
    }
    fn export(&self, at: &At, rev: &str, to: &Path, max: u64) -> io::Result<()> {
        let when = crate::fs::epoch(self.show(&at.base, rev)?.time);
        let files = svn_out(at, rev)?.into_iter().map(|(rest, src, _)| (rest, src)).collect();
        write_out(to, when, files, max, |src, f, room| {
            let mut c = svn();
            c.args(["cat", "--", src]);
            file_to(c, f, src, room)
        })
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

/// How deep, and over how many entries, a CVS working copy is walked at most.
const CVS_MAX_DEPTH: usize = 64;
const CVS_MAX_ENTRIES: usize = 500_000;

/// A name in `CVS/Entries` that is one plain name in its folder.
fn cvs_name_ok(n: &str) -> bool {
    !n.is_empty() && n != "." && n != ".." && !n.contains(['/', '\\', '\0']) && !(cfg!(windows) && n.contains(':'))
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
    // A working copy may be someone else's: names are single folder names, a link to a folder
    // is never followed, each folder is read once, and the walk is bounded.
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut budget = CVS_MAX_ENTRIES;
    while let Some((dir, depth)) = stack.pop() {
        if !seen.insert(dir.canonicalize().unwrap_or_else(|_| dir.clone())) {
            continue;
        }
        let entries: Vec<Entry> = cvs_entries(&dir)
            .into_iter()
            .filter(|e| match e {
                Entry::Dir(n) | Entry::File { name: n, .. } => cvs_name_ok(n),
            })
            .take(budget)
            .collect();
        budget -= entries.len();
        let mut known: HashSet<String> = HashSet::from(["CVS".to_string()]);
        for e in &entries {
            match e {
                Entry::Dir(n) => {
                    known.insert(n.clone());
                    let d = dir.join(n);
                    let plain_dir = std::fs::symlink_metadata(&d).is_ok_and(|m| m.is_dir());
                    if plain_dir && depth < CVS_MAX_DEPTH && d.join("CVS/Entries").is_file() {
                        stack.push((d, depth + 1));
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
        // ponytail: 100k unknown entries a folder at most; enough for any working copy one browses.
        for e in read.flatten().take(100_000) {
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

/// The folder whose `CVS/Root` names the server for `path`: the folder itself, or the one a
/// file is in.
fn cvs_dir(path: &Path) -> Option<PathBuf> {
    let has = |d: &Path| d.join("CVS/Root").is_file();
    [path, path.parent()?].into_iter().find(|d| has(d)).map(Path::to_path_buf)
}

/// The server of the CVS working copy folder `dir`, from its `CVS/Root`: how the question
/// names it (`pserver://host`, `ext://host:port`, or the folder of a repository on this
/// machine), and the root to hand to `cvs -d`. `None` for a root Coxswain will not hand over:
/// another method, options after the method (`:ext;CVS_RSH=…:`), or a host or user that is
/// no name (one starting with `-` would be an option to ssh).
pub fn cvs_root_of(dir: &Path) -> Option<(String, String)> {
    let f = std::fs::File::open(dir.join("CVS/Root")).ok()?;
    let mut line = String::new();
    io::BufRead::read_line(&mut io::BufReader::new(io::Read::take(f, 4096)), &mut line).ok()?;
    parse_cvs_root(line.trim_end_matches(['\n', '\r']))
}

/// `parse_cvs_root` of the text of a `CVS/Root`.
pub fn parse_cvs_root(root: &str) -> Option<(String, String)> {
    if root.is_empty() || root.chars().any(|c| c.is_control()) || root.contains(';') {
        return None;
    }
    if root.starts_with('/') {
        return Some((root.to_string(), root.to_string()));
    }
    let (method, rest) = match root.strip_prefix(':') {
        Some(r) => r.split_once(':')?,
        None => ("ext", root),
    };
    match method {
        "local" | "fork" => return rest.starts_with('/').then(|| (rest.to_string(), root.to_string())),
        "pserver" | "ext" => {}
        _ => return None,
    }
    let slash = rest.find('/')?;
    let (before, path) = (&rest[..slash], &rest[slash..]);
    if path.len() < 2 {
        return None;
    }
    let (user, hostport) = match before.rsplit_once('@') {
        Some((u, h)) => (Some(u.split(':').next().unwrap_or_default()), h),
        None => (None, before),
    };
    let hostport = hostport.trim_end_matches(':');
    let (host, port) = match hostport.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (hostport, None),
    };
    let name_ok = |u: &str| !u.is_empty() && !u.starts_with('-') && u.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    if !host_ok(host) || host.starts_with('[') || user.is_some_and(|u| !name_ok(u)) || port.is_some_and(|p| p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let id = match port {
        Some(p) => format!("{method}://{host}:{p}"),
        None => format!("{method}://{host}"),
    };
    Some((id, root.to_string()))
}

/// A CVS date: `2026/10/01 12:00:00` (CVS 1.11) or `2026-10-01 12:00:00 +0000` (1.12), in UTC.
fn cvs_log_time(s: &str) -> Option<u64> {
    let s = s.trim();
    let fixed = chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S %z").map(|d| d.timestamp());
    let t = fixed.or_else(|_| {
        chrono::NaiveDateTime::parse_from_str(s, "%Y/%m/%d %H:%M:%S").map(|d| d.and_utc().timestamp())
    });
    t.ok().filter(|t| *t >= 0).map(|t| t as u64)
}

/// One revision of one file, as `cvs log` has it.
#[derive(Clone, Debug)]
struct CvsRev {
    /// The working file, `/`-separated, below the folder the log was read in.
    file: String,
    num: String,
    time: u64,
    dead: bool,
    author: String,
    msg: String,
    id: Option<String>,
}

/// The revisions in `cvs log`'s output, newest first.
fn parse_cvs_revs(out: &str) -> Vec<CvsRev> {
    let mut revs = vec![];
    let mut file = String::new();
    let mut lines = out.lines().peekable();
    while let Some(line) = lines.next() {
        if let Some(f) = line.strip_prefix("Working file: ") {
            file = f.replace('\\', "/");
            continue;
        }
        let Some(num) = line.strip_prefix("revision ").and_then(|n| n.split_whitespace().next()).filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit() || b == b'.')) else {
            continue;
        };
        let Some(date) = lines.next().filter(|l| l.starts_with("date: ")) else { continue };
        let mut fields = HashMap::new();
        for part in date.split(';') {
            if let Some((k, v)) = part.trim().split_once(": ") {
                fields.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        let Some(time) = fields.get("date").and_then(|d| cvs_log_time(d)) else { continue };
        let mut msg = vec![];
        while let Some(l) = lines.peek() {
            if *l == "----------------------------" || l.starts_with("=============") {
                break;
            }
            let l = lines.next().unwrap_or_default();
            if !l.starts_with("branches: ") || !msg.is_empty() {
                msg.push(l);
            }
        }
        revs.push(CvsRev {
            file: file.clone(),
            num: num.trim().to_string(),
            time,
            dead: fields.get("state").is_some_and(|s| s == "dead"),
            author: fields.remove("author").unwrap_or_default(),
            msg: msg.join("\n"),
            id: fields.remove("commitid"),
        });
    }
    revs.sort_by_key(|r| std::cmp::Reverse(r.time));
    revs
}

/// The revisions of a log grouped into commits, newest first, each with its revisions (indices
/// into `revs`). CVS keeps a history per file: revisions with the same commit id (CVS 1.12),
/// or else the same author and message within five minutes, are one commit, named by that id
/// or a hash of the three.
fn cvs_commits(revs: &[CvsRev]) -> Vec<(Commit, Vec<usize>)> {
    let mut out: Vec<(Commit, Vec<usize>)> = vec![];
    // The commits by their id, and those without one by author and message (newest first).
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    let mut by_text: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (i, r) in revs.iter().enumerate() {
        // Revisions come newest first, so a commit is never older than `r`: the first one
        // within five minutes is the first whose time is at most five minutes later.
        let same = match &r.id {
            Some(id) => by_id.get(id.as_str()).copied(),
            None => by_text.get(&(r.author.as_str(), r.msg.as_str())).and_then(|cs| {
                let at = cs.partition_point(|&c| out[c].0.time.abs_diff(r.time) > 300);
                cs.get(at).copied()
            }),
        };
        if let Some(at) = same {
            out[at].1.push(i);
            continue;
        }
        let hash = r.id.clone().unwrap_or_else(|| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (r.time, &r.author, &r.msg).hash(&mut h);
            format!("{:016x}", h.finish())
        });
        match &r.id {
            Some(id) => {
                by_id.insert(id, out.len());
            }
            None => by_text.entry((&r.author, &r.msg)).or_default().push(out.len()),
        }
        let subject = r.msg.lines().next().unwrap_or_default().to_string();
        out.push((Commit { hash, time: r.time, author: r.author.clone(), subject, ..Default::default() }, vec![i]));
    }
    out
}

/// `cvs log` as commits, newest first (`cvs_commits`).
pub fn parse_cvs_log(out: &str) -> Vec<Commit> {
    cvs_commits(&parse_cvs_revs(out)).into_iter().map(|(c, _)| c).collect()
}

/// A folder's `cvs log`, read: its revisions and commits.
struct CvsLog {
    revs: Vec<CvsRev>,
    commits: Vec<(Commit, Vec<usize>)>,
}

/// The last log read of each folder: opening a commit, its files and its diff read from it.
static CVS_LOGS: std::sync::Mutex<Vec<(PathBuf, std::sync::Arc<CvsLog>)>> = std::sync::Mutex::new(Vec::new());

/// The root to hand to `cvs -d` for the working copy folder `base`, once its server was said
/// yes to.
fn cvs_here(base: &Path) -> io::Result<String> {
    let dir = cvs_dir(base).ok_or_else(|| on_server("CVS"))?;
    let (server, root) = cvs_root_of(&dir).ok_or_else(|| on_server("CVS"))?;
    if !allowed(&server) {
        return Err(not_asked(&server));
    }
    need("cvs", "CVS", &CVS_MISSING)?;
    Ok(root)
}

/// cvs in `base`, as `cvs_log` runs it: `-f`: no ~/.cvsrc; `-d`: the root asked about, for every
/// folder below too, whatever their own `CVS/Root` says. Only `log`, `update -p` and `diff`
/// are run: none of them runs a program a working copy or repository names (`update` runs
/// `CVS/Update.prog` only when it writes files, never with `-p`), and none looks at
/// `CVSROOT/modules`.
fn cvs(base: &Path, root: &str) -> std::process::Command {
    let mut c = crate::tools::command("cvs");
    c.current_dir(base).args(["-f", "-Q", "-d", root]);
    c
}

/// The log of the folder `base` and all below it, read from the server and kept.
fn cvs_read_log(base: &Path) -> io::Result<(std::sync::Arc<CvsLog>, bool)> {
    let root = cvs_here(base)?;
    let mut c = cvs(base, &root);
    c.args(["log", "-N"]);
    let (out, cut) = from_server(c, SERVER_MAX)?;
    let revs = parse_cvs_revs(&String::from_utf8_lossy(&out));
    let log = std::sync::Arc::new(CvsLog { commits: cvs_commits(&revs), revs });
    let mut logs = CVS_LOGS.lock().unwrap_or_else(|e| e.into_inner());
    logs.retain(|(b, _)| b != base);
    if logs.len() >= 16 {
        logs.remove(0);
    }
    logs.push((base.to_path_buf(), log.clone()));
    Ok((log, cut))
}

/// The log of `base` as last read, or read now.
fn cvs_known(base: &Path) -> io::Result<std::sync::Arc<CvsLog>> {
    cvs_here(base)?;
    let kept = CVS_LOGS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(b, _)| b == base).map(|(_, l)| l.clone());
    match kept {
        Some(l) => Ok(l),
        None => Ok(cvs_read_log(base)?.0),
    }
}

impl CvsLog {
    fn commit(&self, rev: &str) -> io::Result<&(Commit, Vec<usize>)> {
        self.commits.iter().find(|(c, _)| !rev.is_empty() && c.hash.starts_with(rev)).ok_or_else(|| no_commit(rev))
    }
    /// The files as they were at the commit `rev`: each one's revision in it, or else the last
    /// one before it on the trunk (or the vendor branch `1.1.1` an import leaves); none that
    /// were removed then.
    fn files_at(&self, rev: &str) -> io::Result<Vec<&CvsRev>> {
        let (c, members) = self.commit(rev)?;
        let main = |n: &str| n.matches('.').count() == 1 || n.starts_with("1.1.1.");
        let mut by: HashMap<&str, &CvsRev> = HashMap::new();
        // Newest first: the first found is the last one before the commit.
        for r in self.revs.iter().filter(|r| r.time <= c.time && main(&r.num)) {
            by.entry(&r.file).or_insert(r);
        }
        for &i in members {
            by.insert(&self.revs[i].file, &self.revs[i]);
        }
        let mut out: Vec<&CvsRev> = by.into_values().filter(|r| !r.dead && r.file.split('/').all(name_ok)).collect();
        out.sort_by(|a, b| a.file.cmp(&b.file));
        Ok(out)
    }
}

/// The revision before `num`: `1.3` → `1.2`, `1.2.2.1` → `1.2`; none before `1.1`.
fn cvs_before(num: &str) -> Option<String> {
    let (head, last) = num.rsplit_once('.')?;
    match last.parse::<u64>().ok()? {
        0 => None,
        1 if head.contains('.') => head.rsplit_once('.').map(|(h, _)| h.to_string()),
        1 => None,
        n => Some(format!("{head}.{}", n - 1)),
    }
}

/// The file `r` at its revision, at most `max` bytes, and whether there was more.
fn cvs_cat(base: &Path, r: &CvsRev, max: usize) -> io::Result<(Vec<u8>, bool)> {
    file_from_server(cvs_cat_cmd(base, r)?, max)
}

/// The run that prints the file `r` at its revision.
fn cvs_cat_cmd(base: &Path, r: &CvsRev) -> io::Result<std::process::Command> {
    let mut c = cvs(base, &cvs_here(base)?);
    c.args(["update", "-p", "-r", &r.num, "--", &r.file]);
    Ok(c)
}

/// The history of `spec` (`.` or a name in `base`) from the CVS server, once said yes to: the
/// commits of the whole folder that touched it, so a commit is named alike in both.
fn cvs_log(base: &Path, spec: &str, max: usize) -> io::Result<(Vec<Commit>, bool)> {
    let (log, cut) = cvs_read_log(base)?;
    let mine = |f: &str| spec == "." || f == spec || f.strip_prefix(spec).is_some_and(|r| r.starts_with('/'));
    let mut commits: Vec<Commit> = log
        .commits
        .iter()
        .filter(|(_, m)| m.iter().any(|&i| mine(&log.revs[i].file)))
        .map(|(c, _)| c.clone())
        .collect();
    let more = cut || commits.len() > max;
    commits.truncate(max);
    Ok((commits, more))
}

/// What copying `at` out at `rev` writes: (the path below the copy, `""` for a file itself;
/// the file in the log).
fn cvs_out(log: &CvsLog, at: &At, rev: &str) -> io::Result<Vec<(String, String)>> {
    let prefix = format!("{}/", at.inner);
    let files: Vec<(String, String)> = log
        .files_at(rev)?
        .into_iter()
        .filter_map(|r| {
            let rest = if at.inner.is_empty() { r.file.as_str() } else if r.file == at.inner { "" } else { r.file.strip_prefix(&prefix)? };
            crate::vcs::safe_out(rest).then(|| (rest.to_string(), r.file.clone()))
        })
        .collect();
    if files.is_empty() {
        return Err(not_in(&at.inner, rev));
    }
    Ok(files)
}

pub(crate) struct Cvs;

impl Vcs for Cvs {
    fn name(&self) -> &'static str {
        "cvs"
    }
    fn status(&self, dir: &Path) -> Option<Status> {
        cvs_status(&cvs_root(dir)?)
    }
    fn log(&self, base: &Path, spec: &str, max: usize) -> io::Result<(Vec<Commit>, bool)> {
        cvs_log(base, spec, max)
    }
    fn show(&self, base: &Path, rev: &str) -> io::Result<Commit> {
        Ok(cvs_known(base)?.commit(rev)?.0.clone())
    }
    fn tree(&self, base: &Path, rev: &str, inner: &str) -> io::Result<Vec<(String, &'static str, u64)>> {
        let log = cvs_known(base)?;
        let prefix = if inner.is_empty() { String::new() } else { format!("{inner}/") };
        // CVS lists no sizes without reading every file: they show as 0.
        let mut out = std::collections::BTreeMap::new();
        for r in log.files_at(rev)? {
            let Some(rest) = r.file.strip_prefix(&prefix) else { continue };
            match rest.split_once('/') {
                Some((dir, _)) => out.insert(dir.to_string(), "tree"),
                None => out.insert(rest.to_string(), "blob"),
            };
        }
        Ok(out.into_iter().map(|(n, k)| (n, k, 0)).collect())
    }
    fn read(&self, base: &Path, rev: &str, inner: &str, max: usize) -> io::Result<(Vec<u8>, bool)> {
        let log = cvs_known(base)?;
        let files = log.files_at(rev)?;
        let r = files.iter().find(|r| r.file == inner).ok_or_else(|| not_in(inner, rev))?;
        cvs_cat(base, r, max)
    }
    fn diff(&self, base: &Path, rev: &str, inner: &str) -> io::Result<Vec<u8>> {
        let log = cvs_known(base)?;
        let root = cvs_here(base)?;
        let (_, members) = log.commit(rev)?;
        let mut revs: Vec<&CvsRev> = members
            .iter()
            .map(|&i| &log.revs[i])
            .filter(|r| r.file.split('/').all(name_ok))
            .filter(|r| inner.is_empty() || r.file == inner || r.file.strip_prefix(inner).is_some_and(|x| x.starts_with('/')))
            .collect();
        revs.sort_by(|a, b| a.file.cmp(&b.file));
        let mut out = vec![];
        // At most 200 files, `DIFF_MAX` bytes and about `SERVER_TIME` in all; more is cut, and
        // the diff says so at its top.
        // ponytail: one cvs run a file; one `cvs rdiff` if large commits matter.
        let until = std::time::Instant::now() + SERVER_TIME;
        let mut cut = revs.len() > 200;
        for r in revs.iter().take(200) {
            let room = DIFF_MAX.saturating_sub(out.len());
            if room == 0 || std::time::Instant::now() > until {
                cut = true;
                break;
            }
            match cvs_before(&r.num) {
                Some(before) => {
                    let mut c = cvs(base, &root);
                    c.args(["diff", "-N", "-u", "-r", &before, "-r", &r.num, "--", &r.file]);
                    let (bytes, more) = from_server(c, room)?;
                    out.extend(bytes);
                    cut |= more;
                }
                // The first revision: all of it is new.
                None => {
                    let (text, _) = cvs_cat(base, r, room.min(1 << 20))?;
                    let text = String::from_utf8_lossy(&text);
                    let lines: Vec<&str> = text.lines().collect();
                    out.extend(format!("Index: {f}\n--- /dev/null\n+++ {f}\t{num}\n@@ -0,0 +1,{n} @@\n", f = r.file, num = r.num, n = lines.len()).bytes());
                    for l in lines {
                        out.extend(format!("+{l}\n").bytes());
                    }
                }
            }
        }
        Ok(cut_note(out, cut))
    }
    fn export(&self, at: &At, rev: &str, to: &Path, max: u64) -> io::Result<()> {
        let log = cvs_known(&at.base)?;
        let when = crate::fs::epoch(log.commit(rev)?.0.time);
        let files = cvs_out(&log, at, rev)?;
        let files_at = log.files_at(rev)?;
        write_out(to, when, files, max, |file, f, room| {
            let r = files_at.iter().find(|r| r.file == file).ok_or_else(|| not_in(file, rev))?;
            file_to(cvs_cat_cmd(&at.base, r)?, f, file, room)
        })
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

    fn allow_also(server: &str) {
        ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).push(server.to_string());
    }

    #[test]
    fn wc_servers_are_named_and_hostile_ones_refused() {
        assert_eq!(svn_server_of("https://ada@svn.example.org/repos/x").as_deref(), Some("https://svn.example.org"));
        assert_eq!(svn_server_of("svn://svn.example.org:3690/x").as_deref(), Some("svn://svn.example.org:3690"));
        assert_eq!(svn_server_of("svn+ssh://[::1]/x").as_deref(), Some("svn+ssh://[::1]"));
        assert_eq!(svn_server_of("file:///srv/repo/").as_deref(), Some("file:///srv/repo"));
        assert_eq!(svn_server_of("svn+ssh://-oProxyCommand=x@svn.example.org/x"), None);
        assert_eq!(svn_server_of("svn+ssh://%2DoProxyCommand=x@svn.example.org/x"), None);
        for bad in ["svn+ssh://-oProxyCommand=x/r", "svn+evil://host/r", "https://ho st/r", "https://h:99999999/r", "https://h\n/r", "nonsense"] {
            assert_eq!(svn_server_of(bad), None, "{bad}");
        }
        let id = |r: &str| parse_cvs_root(r).map(|(id, _)| id);
        assert_eq!(id(":pserver:anon@cvs.example.org:/cvsroot").as_deref(), Some("pserver://cvs.example.org"));
        assert_eq!(id(":pserver:anon:secret@cvs.example.org:2401/cvsroot").as_deref(), Some("pserver://cvs.example.org:2401"));
        assert_eq!(id(":ext:ada@cvs.example.org:/cvs").as_deref(), Some("ext://cvs.example.org"));
        assert_eq!(id("ada@cvs.example.org:/cvs").as_deref(), Some("ext://cvs.example.org"));
        assert_eq!(id("/var/cvs").as_deref(), Some("/var/cvs"));
        assert_eq!(id(":local:/var/cvs").as_deref(), Some("/var/cvs"));
        for bad in [":ext:-oProxyCommand=x:/cvs", ":ext:-l@host:/cvs", ":ext;CVS_RSH=sh:host:/cvs", ":gserver:host:/cvs", ":ext:host", ":fork:relative", ":ext:ho st:/cvs", ""] {
            assert_eq!(parse_cvs_root(bad), None, "{bad}");
        }
    }

    #[test]
    fn wc_logs_are_read() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<log>
<logentry revision="3"><author>ada</author><date>2026-10-01T12:00:00.123456Z</date><msg>Fix &amp; tidy &#x263A;
more</msg></logentry>
<logentry revision="2"><date>2026-09-30T08:00:00.000000Z</date><msg></msg></logentry>
</log>"#;
        let c = parse_svn_log(xml);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].hash.as_str(), c[0].author.as_str(), c[0].subject.as_str()), ("r3", "ada", "Fix & tidy \u{263A}"));
        assert_eq!(c[0].time, 1_790_856_000);
        assert_eq!((c[1].hash.as_str(), c[1].author.as_str()), ("r2", ""));
        assert!(parse_svn_log(b"<!DOCTYPE x [<!ENTITY a \"b\">]><log/>").is_empty());
        let log = "\nRCS file: /cvs/p/a.txt,v\nWorking file: a.txt\nhead: 1.2\n----------------------------\nrevision 1.2\ndate: 2026/10/01 12:00:05;  author: ada;  state: Exp;  lines: +1 -0;\nsecond\nwith more\n----------------------------\nrevision 1.1\ndate: 2026/09/30 08:00:00;  author: bob;  state: Exp;\nbranches:  1.1.2;\nfirst one\n=============================================================================\n\nRCS file: /cvs/p/b.txt,v\nWorking file: b.txt\n----------------------------\nrevision 1.2\ndate: 2026/10/01 12:00:00;  author: ada;  state: Exp;  lines: +1 -0;\nsecond\nwith more\n----------------------------\nrevision 1.1\ndate: 2026-09-29 10:00:00 +0000;  author: cy;  state: Exp;  commitid: 10065A1B2C3;\nthird\n=============================================================================\n";
        let c = parse_cvs_log(log);
        let subjects: Vec<&str> = c.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, ["second", "first one", "third"], "{c:?}");
        assert_eq!((c[0].time, c[0].author.as_str()), (1_790_856_005, "ada"));
        assert_eq!(c[2].hash, "10065A1B2C3");
    }

    /// A real Subversion repository (`file://`) and a real CVS one (local): nothing is read
    /// until the server was said yes to, then the history of the working copy and of a file
    /// lists its commits, newest first; opening a commit says it is on the server.
    #[test]
    fn wc_history_from_the_server_once_allowed() {
        if crate::user_menu::installed("svn") && crate::user_menu::installed("svnadmin") {
            let d = top("svn-log");
            let run = |dir: &Path, args: &[&str]| {
                let ok = std::process::Command::new(args[0]).current_dir(dir).args(&args[1..]).env("HOME", &d)
                    .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
                assert!(ok.success(), "{args:?}");
            };
            run(&d, &["svnadmin", "create", "repo"]);
            let url = format!("file://{}/repo", d.display());
            run(&d, &["svn", "mkdir", "-q", "-m", "layout", &format!("{url}/trunk")]);
            run(&d, &["svn", "checkout", "-q", &format!("{url}/trunk"), "wc"]);
            let wc = d.join("wc");
            std::fs::write(wc.join("a@b.txt"), "a\n").unwrap();
            run(&wc, &["svn", "add", "-q", "a@b.txt@"]);
            run(&wc, &["svn", "commit", "-q", "-m", "first one"]);
            std::fs::write(wc.join("a@b.txt"), "a\nc\n").unwrap();
            run(&wc, &["svn", "commit", "-q", "-m", "second\nmore"]);
            run(&wc, &["svn", "update", "-q"]);
            let server = server(&wc).expect("a server");
            assert_eq!(server, url);
            assert_eq!(ask_first(&wc.join("a@b.txt")).map(|a| (a.server, a.program)), Some((url.clone(), "svn")));
            let e = Svn.log(&wc, ".", 10).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::PermissionDenied, "{e}");
            allow_also(&server);
            assert_eq!(ask_first(&wc), None);
            let (all, more) = Svn.log(&wc, ".", 10).unwrap();
            let names: Vec<&str> = all.iter().map(|c| c.hash.as_str()).collect();
            assert_eq!((names, more), (vec!["r3", "r2", "r1"], false));
            assert_eq!(all[0].subject, "second");
            let (one, more) = Svn.log(&wc, "a@b.txt", 1).unwrap();
            assert_eq!((one.len(), more), (1, true));
            let at = crate::history::split(&wc.join("a@b.txt").join(crate::history::MARKER)).unwrap();
            let list = crate::history::list(&at.target.join(crate::history::MARKER), &at).unwrap();
            assert_eq!(list.len(), 3, "{list:?}");
            assert!(list[1].name.contains("second"), "{list:?}");
            // A commit opened: the files as they were, what it changed, a copy out.
            let hist = wc.join("a@b.txt").join(crate::history::MARKER);
            let r2 = crate::history::split(&hist.join("r2")).expect("r2 is a commit");
            let files = crate::history::list(&hist.join("r2"), &r2).unwrap();
            assert!(files.iter().any(|e| e.name == "a@b.txt" && e.size == 2 && !e.is_dir), "{files:?}");
            let file = crate::history::split(&hist.join("r2/a@b.txt")).unwrap();
            assert_eq!(crate::history::read(&file, 100).unwrap().0, b"a\n");
            let file3 = crate::history::split(&hist.join("r3/a@b.txt")).unwrap();
            assert!(crate::history::diff(&file3).unwrap().contains("+c"));
            assert_eq!(crate::history::show(&wc, "r3").unwrap().subject, "second");
            // A folder moved since is found where it was; a `.git` in it never comes out.
            std::fs::create_dir_all(wc.join("old/sub/.git")).unwrap();
            std::fs::write(wc.join("old/x.txt"), "x\n").unwrap();
            std::fs::write(wc.join("old/sub/.git/config"), "[core]\n").unwrap();
            run(&wc, &["svn", "add", "-q", "old"]);
            run(&wc, &["svn", "commit", "-q", "-m", "an old folder"]);
            run(&wc, &["svn", "update", "-q"]);
            run(&wc, &["svn", "mv", "-q", "old", "new"]);
            run(&wc, &["svn", "commit", "-q", "-m", "moved"]);
            run(&wc, &["svn", "update", "-q"]);
            let r4 = crate::history::split(&wc.join("new").join(crate::history::MARKER).join("r4")).unwrap();
            let names: Vec<String> = crate::history::list(&wc.join("new/@history/r4"), &r4).unwrap().into_iter().map(|e| e.name).collect();
            assert!(names.contains(&"x.txt".to_string()), "{names:?}");
            let out = d.join("out");
            std::fs::create_dir(&out).unwrap();
            let copied = crate::history::copy_out(&r4, &out).unwrap();
            assert_eq!(std::fs::read(copied.join("x.txt")).unwrap(), b"x\n");
            assert!(!copied.join("sub/.git").exists());
            let one = crate::history::copy_out(&file, &out).unwrap();
            assert_eq!(std::fs::read(one).unwrap(), b"a\n");
            // Sizes as the server lists them, before a drag.
            assert_eq!(crate::history::measure(&file).unwrap(), (2, 1));
            assert_eq!(crate::history::measure(&r4).unwrap(), (2, 1));
            // A diff longer than is shown is cut, and says so on its first line.
            std::fs::write(wc.join("big.txt"), "a line of text to make a long diff\n".repeat(80_000)).unwrap();
            run(&wc, &["svn", "add", "-q", "big.txt"]);
            run(&wc, &["svn", "commit", "-q", "-m", "big"]);
            let big = crate::history::split(&wc.join("big.txt").join(crate::history::MARKER).join("r6")).unwrap();
            let diff = crate::history::diff(&big).unwrap();
            assert!(diff.starts_with(&crate::t!("history.diff_cut")), "{}", &diff[..200]);
            assert!(!crate::history::diff(&file3).unwrap().starts_with(&crate::t!("history.diff_cut")));
            std::fs::remove_dir_all(d).unwrap();
        }
        if crate::user_menu::installed("cvs") {
            let d = top("cvs-log");
            let root = d.join("cvsroot");
            let run = |dir: &Path, args: &[&str]| {
                let o = std::process::Command::new(args[0]).current_dir(dir).args(&args[1..]).env("HOME", &d).env("CVSROOT", &root)
                    .stdin(std::process::Stdio::null()).output().unwrap();
                assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
            };
            run(&d, &["cvs", "-f", "-q", "init"]);
            std::fs::create_dir(d.join("import")).unwrap();
            std::fs::write(d.join("import/a.txt"), "a\n").unwrap();
            std::fs::write(d.join("import/b.txt"), "b\n").unwrap();
            run(&d.join("import"), &["cvs", "-f", "-q", "import", "-m", "first one", "proj", "vendor", "start"]);
            run(&d, &["cvs", "-f", "-q", "checkout", "proj"]);
            let wc = d.join("proj");
            // A second revision by a second import, not a commit: GNU CVS refuses to commit as
            // root (CI's virtual machines run the tests as root). Revisions are dated in whole
            // seconds, so the second is a second later.
            std::thread::sleep(std::time::Duration::from_millis(1100));
            std::fs::write(d.join("import/a.txt"), "a\nc\n").unwrap();
            std::fs::write(d.join("import/b.txt"), "b\nc\n").unwrap();
            run(&d.join("import"), &["cvs", "-f", "-q", "import", "-m", "both changed", "proj", "vendor", "second"]);
            let server = server(&wc).expect("a server");
            assert_eq!(server, root.display().to_string());
            assert_eq!(Cvs.log(&wc, ".", 10).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
            allow_also(&server);
            let (all, _) = Cvs.log(&wc, ".", 10).unwrap();
            let subjects: Vec<&str> = all.iter().map(|c| c.subject.as_str()).collect();
            assert_eq!(subjects[0], "both changed", "{all:?}");
            assert!(subjects.contains(&"first one") || subjects.contains(&"Initial revision"), "{all:?}");
            let (one, _) = Cvs.log(&wc, "a.txt", 10).unwrap();
            assert_eq!(one[0].subject, "both changed");
            // The commits opened: the files as they were in each, what the last one changed,
            // a copy out.
            let hist = wc.join(crate::history::MARKER);
            let last = crate::history::path(&wc, Some(&all[0].hash));
            let first = crate::history::path(&wc, Some(&all[all.len() - 1].hash));
            let at = crate::history::split(&last).expect("a commit");
            let names: Vec<String> = crate::history::list(&last, &at).unwrap().into_iter().map(|e| e.name).collect();
            assert_eq!(names, ["..", "a.txt", "b.txt"]);
            // A program the working copy names for updates never runs.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let prog = d.join("prog.sh");
                std::fs::write(&prog, format!("#!/bin/sh\ntouch {}\n", d.join("ran").display())).unwrap();
                std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
                std::fs::write(wc.join("CVS/Update.prog"), format!("{}\n", prog.display())).unwrap();
            }
            let read = |p: &Path| crate::history::read(&crate::history::split(p).unwrap(), 100).unwrap().0;
            assert_eq!(read(&last.join("a.txt")), b"a\nc\n");
            assert_eq!(read(&first.join("a.txt")), b"a\n");
            let diff = crate::history::diff(&crate::history::split(&last.join("b.txt")).unwrap()).unwrap();
            assert!(diff.contains("+c") && !diff.contains("a.txt"), "{diff}");
            let out = d.join("out");
            std::fs::create_dir(&out).unwrap();
            let copied = crate::history::copy_out(&crate::history::split(&first).unwrap(), &out).unwrap();
            assert_eq!(std::fs::read(copied.join("b.txt")).unwrap(), b"b\n");
            assert_eq!(crate::history::measure(&crate::history::split(&first).unwrap()).unwrap(), (0, 2));
            // CVS lists no sizes: a drag's limit is counted while writing, and passing it
            // leaves nothing.
            let out2 = d.join("out2");
            std::fs::create_dir(&out2).unwrap();
            let e = crate::history::copy_out_within(&crate::history::split(&first).unwrap(), &out2, 3).unwrap_err();
            assert_eq!(e.to_string(), crate::t!("drag.too_much"));
            assert_eq!(std::fs::read_dir(&out2).unwrap().count(), 0);
            crate::history::copy_out_within(&crate::history::split(&first).unwrap(), &out2, 4).unwrap();
            assert!(hist.starts_with(&wc));
            assert!(!d.join("ran").exists(), "CVS/Update.prog ran");
            // A commit of more than 200 files: its diff is cut, and says so at its top.
            std::fs::create_dir(d.join("many-in")).unwrap();
            for i in 0..201 {
                std::fs::write(d.join(format!("many-in/f{i}.txt")), "x\n").unwrap();
            }
            run(&d.join("many-in"), &["cvs", "-f", "-q", "import", "-m", "many", "many", "vendor", "one"]);
            run(&d, &["cvs", "-f", "-q", "checkout", "many"]);
            let many = d.join("many");
            let (all, _) = Cvs.log(&many, ".", 10).unwrap();
            let at = crate::history::split(&crate::history::path(&many, Some(&all[0].hash))).unwrap();
            let diff = crate::history::diff(&at).unwrap();
            assert!(diff.starts_with(&crate::t!("history.diff_cut")), "{}", &diff[..diff.len().min(200)]);
            std::fs::remove_dir_all(d).unwrap();
        }
    }

    /// A server run that fails after printing part: a listing keeps what came, a file's
    /// bytes are an error, a file copied out is refused; a run cut short is no failure.
    #[cfg(unix)]
    #[test]
    fn wc_server_run_that_fails_is_not_a_whole_file() {
        let sh = |script: &str| {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", script]);
            c
        };
        assert_eq!(from_server(sh("printf abc; exit 1"), 100).unwrap(), (b"abc".to_vec(), false));
        assert!(from_server(sh("echo no >&2; exit 1"), 100).unwrap_err().to_string().contains("no"));
        assert!(file_from_server(sh("printf abc; echo half >&2; exit 1"), 100).unwrap_err().to_string().contains("half"));
        assert_eq!(file_from_server(sh("printf abcdef; exit 1"), 3).unwrap(), (b"abc".to_vec(), true));
        assert_eq!(file_from_server(sh("printf abc"), 3).unwrap(), (b"abc".to_vec(), false));
        let d = top("server-run");
        let mut f = std::fs::File::create(d.join("f")).unwrap();
        assert!(file_to(sh("printf abc; exit 1"), &mut f, "f", u64::MAX).is_err());
        let mut f = std::fs::File::create(d.join("g")).unwrap();
        file_to(sh("printf abc"), &mut f, "g", u64::MAX).unwrap();
        let mut f = std::fs::File::create(d.join("h")).unwrap();
        assert_eq!(file_to(sh("printf abc"), &mut f, "h", 2).unwrap_err().to_string(), crate::t!("drag.too_much"));
        assert_eq!(std::fs::read(d.join("g")).unwrap(), b"abc");
        // A copy out that fails half way leaves nothing (`history::copy_out` removes it).
        let out = d.join("out");
        let files = vec![("a".to_string(), "printf a".to_string()), ("b/c".to_string(), "printf c; exit 1".to_string())];
        assert!(write_out(&out, std::time::SystemTime::now(), files, u64::MAX, |src, f, room| file_to(sh(src), f, src, room)).is_err());
        // What a drag may take in all: the second file finds no room left.
        let files = vec![("a".to_string(), "printf ab".to_string()), ("b".to_string(), "printf cd".to_string())];
        let e = write_out(&d.join("out2"), std::time::SystemTime::now(), files, 3, |src, f, room| file_to(sh(src), f, src, room)).unwrap_err();
        assert_eq!(e.to_string(), crate::t!("drag.too_much"));
        assert_eq!(std::fs::read(out.join("a")).unwrap(), b"a");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// CVS revisions grouped into commits: by commit id, or by author and message within five
    /// minutes of a commit's newest revision, the first such commit when two are.
    #[test]
    fn wc_cvs_commits_grouped() {
        let rev = |file: &str, time: u64, author: &str, msg: &str, id: Option<&str>| CvsRev {
            file: file.into(),
            num: "1.1".into(),
            time,
            dead: false,
            author: author.into(),
            msg: msg.into(),
            id: id.map(Into::into),
        };
        let revs = vec![
            rev("a", 2000, "ada", "x", None),
            rev("b", 1990, "ada", "x", Some("ID")),
            rev("c", 1800, "ada", "x", None),
            rev("d", 1650, "ada", "x", None),
            rev("e", 1600, "bob", "x", None),
            rev("f", 1500, "ada", "x", Some("ID")),
            rev("g", 1400, "ada", "x", None),
        ];
        let groups: Vec<Vec<usize>> = cvs_commits(&revs).into_iter().map(|(_, m)| m).collect();
        assert_eq!(groups, [vec![0, 2], vec![1, 5], vec![3, 6], vec![4]]);
    }

    /// A hostile wc.db (an unpacked archive's): paths that climb out or are absolute are never
    /// looked at, a bad checksum is not used, and `nodes` as a view is refused outright.
    #[test]
    fn wc_svn_hostile_db_is_not_believed() {
        let d = top("svn-hostile");
        std::fs::create_dir_all(d.join(".svn")).unwrap();
        let make = |nodes_view: bool| {
            let db = d.join(".svn/wc.db");
            let _ = std::fs::remove_file(&db);
            let c = rusqlite::Connection::open(&db).unwrap();
            let nodes = "wc_id, local_relpath, op_depth, presence, kind, translated_size, last_mod_time, checksum, properties, repos_path, revision";
            if nodes_view {
                c.execute_batch(&format!("CREATE TABLE real_nodes ({nodes}); CREATE VIEW nodes AS SELECT * FROM real_nodes;")).unwrap();
            } else {
                c.execute_batch(&format!("CREATE TABLE nodes ({nodes});")).unwrap();
            }
            c.execute_batch("CREATE TABLE actual_node (local_relpath, properties, conflict_data, tree_conflict_data);").unwrap();
            let table = if nodes_view { "real_nodes" } else { "nodes" };
            for (rel, sum) in [("", ""), ("../outside.txt", ""), ("/etc/passwd", ""), ("a/./b", ""), ("ok.txt", "$sha1$../../x")] {
                c.execute(
                    &format!("INSERT INTO {table} VALUES (1, ?1, 0, 'normal', ?2, 1, 1, ?3, NULL, 'trunk', 3)"),
                    rusqlite::params![rel, if rel.is_empty() { "dir" } else { "file" }, if sum.is_empty() { None } else { Some(sum) }],
                )
                .unwrap();
            }
            c.execute("INSERT INTO actual_node VALUES ('../../conflict', NULL, 'x', NULL)", []).unwrap();
        };
        make(false);
        std::fs::write(d.join("ok.txt"), "changed and longer").unwrap();
        let st = svn_status(&d).expect("status");
        assert!(st.files.keys().all(|p| p.starts_with(&d) && !p.components().any(|c| c == std::path::Component::ParentDir)), "{:?}", st.files);
        assert_eq!(st.get(&d.join("ok.txt")).map(|f| f.kind), Some(Kind::Modified));
        assert_eq!(st.summary.conflicts, 0);
        assert!(safe_relpath("a/b") && safe_relpath("") && !safe_relpath("a//b") && !safe_relpath("/a") && !safe_relpath("a/.."));
        assert!(sha1_of("$sha1$3f786850e387550fdab836ed7e6dc881de23001b").is_some() && sha1_of("$sha1$../x").is_none());
        assert_eq!(file_uri(Path::new("/a b/c?#%.db")).as_deref(), Some("file:/a%20b/c%3F%23%25.db"));
        #[cfg(unix)]
        assert_eq!(file_uri(Path::new(<std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(b"/a\xff"))), None);
        make(true);
        assert!(svn_status(&d).is_none(), "nodes as a view");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A hostile CVS working copy: names that climb out are skipped, a link to a folder is not
    /// followed, and a loop ends.
    #[test]
    fn wc_cvs_hostile_entries_are_skipped() {
        let d = top("cvs-hostile");
        std::fs::create_dir_all(d.join("CVS")).unwrap();
        std::fs::create_dir_all(d.join("sub/CVS")).unwrap();
        std::fs::write(d.join("CVS/Entries"), "/../escape/1.1/x//\nD/..////\nD/.////\nD/sub////\nD/loop////\n/a\\b/1.1/x//\n").unwrap();
        std::fs::write(d.join("sub/CVS/Entries"), "D/up////\n").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&d, d.join("loop")).unwrap();
            std::os::unix::fs::symlink(&d, d.join("sub/up")).unwrap();
        }
        let st = cvs_status(&d).expect("status");
        assert!(st.files.keys().all(|p| p.starts_with(&d) && !p.components().any(|c| c == std::path::Component::ParentDir)), "{:?}", st.files);
        assert!(!cvs_name_ok("..") && !cvs_name_ok("a/b") && !cvs_name_ok("") && cvs_name_ok("main.c"));
        std::fs::remove_dir_all(d).unwrap();
    }


    /// A pipe where a file was never hangs the read, and a `.svn` that is a link is no working
    /// copy.
    #[cfg(unix)]
    #[test]
    fn wc_svn_pipes_and_links_are_refused() {
        let d = top("svn-fifo");
        std::fs::write(d.join("f"), "x").unwrap();
        let fifo = d.join("pipe");
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        let t = std::time::Instant::now();
        assert!(open_plain(&fifo).is_err());
        assert!(t.elapsed() < crate::test_limit(std::time::Duration::from_secs(2)));
        assert!(open_plain(&d.join("f")).is_ok());
        let wc = d.join("wc");
        std::fs::create_dir_all(d.join("elsewhere")).unwrap();
        std::fs::write(d.join("elsewhere/wc.db"), "").unwrap();
        std::fs::create_dir(&wc).unwrap();
        std::os::unix::fs::symlink(d.join("elsewhere"), wc.join(".svn")).unwrap();
        assert!(svn_status(&wc).is_none());
        // Pipes where SQLite's journal files would be are never opened.
        let real = d.join("real");
        std::fs::create_dir_all(real.join(".svn")).unwrap();
        let c = rusqlite::Connection::open(real.join(".svn/wc.db")).unwrap();
        c.execute_batch("CREATE TABLE nodes (wc_id, local_relpath, op_depth, presence, kind, translated_size, last_mod_time, checksum, properties, repos_path, revision); CREATE TABLE actual_node (local_relpath, properties, conflict_data, tree_conflict_data); INSERT INTO nodes VALUES (1, '', 0, 'normal', 'dir', NULL, NULL, NULL, NULL, 'trunk', 1);").unwrap();
        drop(c);
        for side in ["wc.db-wal", "wc.db-shm", "wc.db-journal"] {
            assert!(std::process::Command::new("mkfifo").arg(real.join(".svn").join(side)).status().unwrap().success());
        }
        let t = std::time::Instant::now();
        assert!(svn_status(&real).is_some());
        assert!(t.elapsed() < crate::test_limit(std::time::Duration::from_secs(2)));
        std::fs::remove_dir_all(d).unwrap();
    }

}

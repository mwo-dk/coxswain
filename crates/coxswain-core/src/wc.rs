//! Subversion and CVS working copies, read from their own records only: `.svn/wc.db` (a
//! SQLite database) and the `CVS/Entries` files. No server is asked and no program run, so a
//! working copy of a server far away, or gone, shows at once. The history is the server's: it is
//! read with `svn log` or `cvs log` only once the user has said yes to that server (`server`,
//! `allow`), and nothing else is asked of it. Update and commit go to a terminal (`handoff`).

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
    let hostport = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
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

/// What `c` prints, at most `SERVER_MAX` bytes within `SERVER_TIME`, and whether it was cut.
/// Its complaint is the error when it fails without printing.
fn from_server(mut c: std::process::Command) -> io::Result<(Vec<u8>, bool)> {
    use std::io::Read;
    c.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // Messages in English, dates as the tools write them by default.
        .env("LC_ALL", "C");
    let mut child = c.spawn()?;
    let (mut out, mut err) = (child.stdout.take().ok_or_else(|| io::Error::other("no output"))?, child.stderr.take());
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
    let mut buf = vec![];
    let _ = (&mut out).take(SERVER_MAX as u64 + 1).read_to_end(&mut buf);
    let cut = buf.len() > SERVER_MAX;
    buf.truncate(SERVER_MAX);
    drop(out);
    drop(tx);
    let (late, status) = watch.join().map_err(|_| io::Error::other("watch"))?;
    let status = status?;
    let errs = errs.join().unwrap_or_default();
    if late {
        return Err(io::Error::new(io::ErrorKind::TimedOut, crate::t!("vcs.server_slow")));
    }
    if !status.success() && !cut && buf.is_empty() {
        let why = String::from_utf8_lossy(&errs).trim().lines().last().unwrap_or_default().to_string();
        return Err(io::Error::other(if why.is_empty() { "failed".to_string() } else { why }));
    }
    Ok((buf, cut))
}

/// `svn log --xml` as commits, newest first: `r<revision>`, its time, author and first line.
pub fn parse_svn_log(xml: &[u8]) -> Vec<Commit> {
    use quick_xml::events::Event;
    let xml = String::from_utf8_lossy(xml);
    let mut r = quick_xml::Reader::from_str(&xml);
    let (mut out, mut now, mut text) = (vec![], None::<Commit>, String::new());
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                text.clear();
                if e.local_name().as_ref() == "logentry" {
                    let rev = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == "revision")
                        .and_then(|a| a.value.parse::<u64>().ok());
                    now = rev.map(|rev| Commit { hash: format!("r{rev}"), ..Default::default() });
                }
            }
            Ok(Event::Text(t)) => text.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0)),
            Ok(Event::GeneralRef(g)) => match g.resolve_char_ref() {
                Ok(Some(c)) => text.push(c),
                _ => text.push_str(quick_xml::escape::resolve_xml_entity(&g).unwrap_or_default()),
            },
            Ok(Event::End(e)) => {
                match (e.local_name().as_ref(), now.as_mut()) {
                    ("author", Some(c)) => c.author = text.trim().to_string(),
                    ("date", Some(c)) => {
                        c.time = chrono::DateTime::parse_from_rfc3339(text.trim()).map_or(0, |d| d.timestamp().max(0) as u64)
                    }
                    ("msg", Some(c)) => c.subject = text.trim().lines().next().unwrap_or_default().to_string(),
                    ("logentry", _) => out.extend(now.take()),
                    _ => {}
                }
                text.clear();
            }
            // No DTD: svn never writes one, and refusing it rules out entity expansion.
            Ok(Event::DocType(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The history of `spec` (`.` or a name in `base`) from the Subversion server, once said yes to.
fn svn_log(base: &Path, spec: &str, max: usize) -> io::Result<(Vec<Commit>, bool)> {
    let root = svn_root(base).ok_or_else(|| on_server("Subversion"))?;
    let server = svn_url(&root).and_then(|u| svn_server_of(&u)).ok_or_else(|| on_server("Subversion"))?;
    if !allowed(&server) {
        return Err(not_asked(&server));
    }
    need("svn", "Subversion", &SVN_MISSING)?;
    let mut c = crate::tools::command("svn");
    c.current_dir(base).args(["log", "--xml", "--non-interactive", "--limit", &(max + 1).to_string(), "--"]);
    // A trailing `@`: a name with `@` in it is never taken for a revision.
    c.arg(format!("{spec}@"));
    let (out, cut) = from_server(c)?;
    let mut commits = parse_svn_log(&out);
    let more = cut || commits.len() > max;
    commits.truncate(max);
    Ok((commits, more))
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
    fn show(&self, _: &Path, _: &str) -> io::Result<Commit> {
        Err(on_server("Subversion"))
    }
    fn tree(&self, _: &Path, _: &str, _: &str) -> io::Result<Vec<(String, &'static str, u64)>> {
        Err(on_server("Subversion"))
    }
    fn read(&self, _: &Path, _: &str, _: &str, _: usize) -> io::Result<(Vec<u8>, bool)> {
        Err(on_server("Subversion"))
    }
    fn diff(&self, _: &Path, _: &str, _: &str) -> io::Result<Vec<u8>> {
        Err(on_server("Subversion"))
    }
    fn export(&self, _: &At, _: &str, _: &Path) -> io::Result<()> {
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

/// `cvs log` as commits, newest first. CVS keeps a history per file: revisions with the same
/// commit id (CVS 1.12), or else the same author and message within five minutes, are one
/// commit, named by that id or a hash of the three.
pub fn parse_cvs_log(out: &str) -> Vec<Commit> {
    // (time, author, message, commit id)
    let mut revs: Vec<(u64, String, String, Option<String>)> = vec![];
    let mut lines = out.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with("revision ") {
            continue;
        }
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
        let author = fields.remove("author").unwrap_or_default();
        revs.push((time, author, msg.join("\n"), fields.remove("commitid")));
    }
    revs.sort_by(|a, b| b.0.cmp(&a.0));
    // Each commit with its whole message, to tell commits apart.
    let mut out: Vec<(Commit, String, Option<String>)> = vec![];
    for (time, author, msg, id) in revs {
        let same = out.iter().position(|(c, m, i)| match (&id, i) {
            (Some(id), Some(i)) => id == i,
            (None, None) => c.author == author && *m == msg && c.time.abs_diff(time) <= 300,
            _ => false,
        });
        if same.is_some() {
            continue;
        }
        let hash = id.clone().unwrap_or_else(|| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (time, &author, &msg).hash(&mut h);
            format!("{:016x}", h.finish())
        });
        let subject = msg.lines().next().unwrap_or_default().to_string();
        out.push((Commit { hash, time, author, subject, ..Default::default() }, msg, id));
    }
    out.into_iter().map(|(c, ..)| c).collect()
}

/// The history of `spec` (`.` or a name in `base`) from the CVS server, once said yes to.
fn cvs_log(base: &Path, spec: &str, max: usize) -> io::Result<(Vec<Commit>, bool)> {
    let dir = cvs_dir(&base.join(spec)).ok_or_else(|| on_server("CVS"))?;
    let (server, root) = cvs_root_of(&dir).ok_or_else(|| on_server("CVS"))?;
    if !allowed(&server) {
        return Err(not_asked(&server));
    }
    need("cvs", "CVS", &CVS_MISSING)?;
    let mut c = crate::tools::command("cvs");
    // `-f`: no ~/.cvsrc; `-d`: the root asked about, for every folder below too, whatever their
    // own `CVS/Root` says.
    c.current_dir(base).args(["-f", "-q", "-d", &root, "log", "-N"]);
    if spec != "." {
        c.args(["--", spec]);
    }
    let (out, cut) = from_server(c)?;
    let mut commits = parse_cvs_log(&String::from_utf8_lossy(&out));
    let more = cut || commits.len() > max;
    commits.truncate(max);
    Ok((commits, more))
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
    fn show(&self, _: &Path, _: &str) -> io::Result<Commit> {
        Err(on_server("CVS"))
    }
    fn tree(&self, _: &Path, _: &str, _: &str) -> io::Result<Vec<(String, &'static str, u64)>> {
        Err(on_server("CVS"))
    }
    fn read(&self, _: &Path, _: &str, _: &str, _: usize) -> io::Result<(Vec<u8>, bool)> {
        Err(on_server("CVS"))
    }
    fn diff(&self, _: &Path, _: &str, _: &str) -> io::Result<Vec<u8>> {
        Err(on_server("CVS"))
    }
    fn export(&self, _: &At, _: &str, _: &Path) -> io::Result<()> {
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

    fn allow_also(server: &str) {
        ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).push(server.to_string());
    }

    #[test]
    fn wc_servers_are_named_and_hostile_ones_refused() {
        assert_eq!(svn_server_of("https://ada@svn.example.org/repos/x").as_deref(), Some("https://svn.example.org"));
        assert_eq!(svn_server_of("svn://svn.example.org:3690/x").as_deref(), Some("svn://svn.example.org:3690"));
        assert_eq!(svn_server_of("svn+ssh://[::1]/x").as_deref(), Some("svn+ssh://[::1]"));
        assert_eq!(svn_server_of("file:///srv/repo/").as_deref(), Some("file:///srv/repo"));
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
            assert!(Svn.tree(&wc, "r2", "").is_err());
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
            // CVS sees a change by the file's time, in whole seconds.
            std::thread::sleep(std::time::Duration::from_millis(1100));
            std::fs::write(wc.join("a.txt"), "a\nc\n").unwrap();
            std::fs::write(wc.join("b.txt"), "b\nc\n").unwrap();
            run(&wc, &["cvs", "-f", "-q", "commit", "-m", "both changed"]);
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
            std::fs::remove_dir_all(d).unwrap();
        }
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
        assert!(t.elapsed() < std::time::Duration::from_secs(2));
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
        assert!(t.elapsed() < std::time::Duration::from_secs(2));
        std::fs::remove_dir_all(d).unwrap();
    }

}

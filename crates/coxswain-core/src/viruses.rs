//! *Scan for viruses*, in both apps, with ClamAV in a container: only when asked, on a file,
//! the marked files or a folder, never in the background, and only when `[preview]
//! virus_scan` is on (off by default).
//!
//! Two runs, each in a container of its own. When the virus database is missing or more than a
//! day old, `freshclam` brings it up to date from ClamAV's server (`SERVER`): the only run that
//! has network, and it sees nothing but the database's folder. Then `clamscan` reads the files
//! with no network at all, the files read-only, the database read-only. Its output is not
//! trusted: a finding counts only for a path Coxswain itself put on the list it scanned.
//!
//! Nothing found is ever deleted by itself: for each finding the user chooses to leave it, put
//! it in the bin, or move it into the quarantine folder (`quarantine_dir`), where it can no
//! longer be run.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use crate::config::PreviewConfig;
use crate::t;

/// Its name under `[preview.images]`.
pub const TOOL: &str = "clamav";
/// Where `freshclam` fetches the database from: the one thing that leaves the machine.
pub const SERVER: &str = "database.clamav.net";
/// A database older than this is brought up to date before a scan.
pub const FRESH: u64 = 24 * 3600;
/// Files with a line break in their name, mounted one by one (a list of names is one per
/// line); past this many they are left out, and the report says so.
const ODD_MAX: usize = 100;
/// Findings kept from one scan; the report says when there were more.
const FOUND_MAX: usize = 10_000;

/// The runtime and the image a scan uses, or why it cannot run.
pub fn how(cfg: &PreviewConfig) -> Result<(&'static str, PathBuf, String), String> {
    if !cfg.virus_scan {
        return Err(t!("viruses.off"));
    }
    // ClamAV's image is a Linux one: no container runs it on the BSDs or illumos.
    if cfg!(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "illumos", target_os = "solaris")) {
        return Err(t!("viruses.not_here"));
    }
    let image = cfg.images.get(TOOL).filter(|i| !i.is_empty()).cloned().ok_or_else(|| t!("convert.no_image", "tool" => TOOL))?;
    if cfg.container == "off" {
        return Err(format!("{} {}", t!("viruses.needs"), t!("searchable.containers_off")));
    }
    let (rt, path) = crate::tools::container_runtime(cfg).ok_or_else(|| format!("{} {}", t!("viruses.needs"), crate::tools::no_runtime()))?;
    Ok((rt, path, image))
}

/// The folder ClamAV's database is kept in (in the cache: it can always be fetched again).
pub fn db_dir() -> Option<PathBuf> {
    crate::dirs::cache_dir().map(|d| d.join("coxswain").join("clamav"))
}

/// The folder found files are moved into when the user chooses quarantine.
pub fn quarantine_dir() -> Option<PathBuf> {
    crate::dirs::data_dir().map(|d| d.join("coxswain").join("quarantine"))
}

/// When the database was last brought up to date, in seconds since 1970; none when there is
/// no database.
pub fn db_updated() -> Option<u64> {
    let dir = db_dir()?;
    let has = ["main.cvd", "main.cld"].iter().any(|f| dir.join(f).is_file());
    let stamp = std::fs::metadata(dir.join("updated")).and_then(|m| m.modified()).ok()?;
    has.then(|| stamp.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
}

/// Does a scan at `now` bring the database up to date first?
pub fn due(updated: Option<u64>, now: u64) -> bool {
    updated.is_none_or(|u| now.saturating_sub(u) > FRESH)
}

/// The database's age as a line: when it was updated, or that there is none yet.
pub fn db_line(updated: Option<u64>) -> String {
    match updated {
        Some(u) => t!("viruses.db_from", "when" => crate::i18n::date_time(u)),
        None => t!("viruses.db_none"),
    }
}

/// What a scan of `targets` would do, for the question before it: how many files, the
/// database and its update, and the first pull with its size.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Plan {
    pub files: usize,
    pub text: String,
}

pub fn plan(cfg: &PreviewConfig, targets: &[PathBuf]) -> Result<Plan, String> {
    let (rt, path, image) = how(cfg)?;
    let files = walk(targets, &AtomicBool::new(false)).0.len();
    let what = match targets {
        [one] if one.is_dir() => t!("viruses.confirm_folder", "name" => shown(one), "n" => crate::i18n::digits(files.to_string())),
        [one] => t!("viruses.confirm_one", "name" => shown(one)),
        _ => t!("viruses.confirm_many", "n" => crate::i18n::digits(files.to_string())),
    };
    let updated = db_updated();
    let mut text = vec![what, t!("viruses.sealed", "runtime" => rt), db_line(updated)];
    if due(updated, crate::fs::now()) {
        text.push(t!(if updated.is_some() { "viruses.update" } else { "viruses.update_first" }, "server" => SERVER));
    }
    if crate::tools::image_size(&path, &image).is_none() {
        text.push(match crate::images::download(&image) {
            Some(size) => t!("convert.first_run_pulls_large", "image" => crate::images::short(&image), "size" => size),
            None => t!("convert.first_run_pulls", "image" => crate::images::short(&image)),
        } + ".");
    }
    text.push(t!("viruses.never_deletes"));
    Ok(Plan { files, text: text.join(" ") })
}

fn shown(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The regular files under `targets`, each once, links and special files left out (a pipe
/// would hold the scan up), and those only in the cloud too (they are not downloaded to be
/// read). The second number: files left out for that.
fn walk(targets: &[PathBuf], stop: &AtomicBool) -> (Vec<PathBuf>, usize) {
    let (mut files, mut left) = (vec![], 0);
    let mut todo: Vec<PathBuf> = targets.to_vec();
    while let Some(p) = todo.pop() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let Ok(m) = std::fs::symlink_metadata(&p) else { continue };
        if m.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                todo.extend(rd.flatten().map(|e| e.path()));
            }
        } else if m.is_file() {
            if crate::cloud::unasked(&p) {
                left += 1;
            } else {
                files.push(p);
            }
        }
    }
    files.sort();
    files.dedup();
    (files, left)
}

/// What a scan says while it goes.
pub enum Said<'a> {
    /// The image is being pulled: its progress line.
    Pulling(&'a str),
    /// freshclam is bringing the database up to date: a line it wrote.
    Updating(&'a str),
    /// Files read, of all.
    Scanned(usize, usize),
}

/// A file ClamAV found something in, and what (its signature's name, as `Win.Test.EICAR`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Finding {
    pub path: PathBuf,
    pub name: String,
    /// The file as it was when it was found: a bin or quarantine acts only on that same file,
    /// never on what was put in its place since (a link, a folder, another file). Kept by the
    /// app itself, never taken from what a window sends.
    #[serde(skip)]
    pub id: Option<Ident>,
}

/// Which file a path was. On Unix its device and inode, size, and times of change (content and
/// inode); on Windows its volume and file index, size and time of change. Elsewhere none can
/// be told, and nothing found is moved there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ident {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

impl Ident {
    /// Of a regular file at `p`, not following a link; none for anything else, or where the
    /// file's identity cannot be told.
    pub fn of(p: &Path) -> Option<Ident> {
        #[cfg(unix)]
        {
            let m = std::fs::symlink_metadata(p).ok().filter(|m| m.file_type().is_file())?;
            Some(Self::from(&m))
        }
        #[cfg(windows)]
        {
            win::ident(p)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = p;
            None
        }
    }

    #[cfg(unix)]
    fn from(m: &std::fs::Metadata) -> Ident {
        use std::os::unix::fs::MetadataExt;
        Ident { dev: m.dev(), ino: m.ino(), len: m.size(), mtime: (m.mtime(), m.mtime_nsec()), ctime: (m.ctime(), m.ctime_nsec()) }
    }

    /// The same file, whatever happened to it since (a rename changes its inode's time).
    #[cfg(unix)]
    fn same_inode(&self, o: &Ident) -> bool {
        (self.dev, self.ino) == (o.dev, o.ino)
    }
}

/// The file's identity on Windows, from a handle that opens a link itself, never its target:
/// a link or a folder has none.
#[cfg(windows)]
mod win {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, GetFileInformationByHandle,
    };

    pub fn ident(p: &std::path::Path) -> Option<super::Ident> {
        let f = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(p)
            .ok()?;
        // SAFETY: an open handle and a zeroed plain struct for the call to fill.
        let mut i: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut i) } == 0 {
            return None;
        }
        if i.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
            return None;
        }
        let write = (u64::from(i.ftLastWriteTime.dwHighDateTime) << 32) | u64::from(i.ftLastWriteTime.dwLowDateTime);
        Some(super::Ident {
            dev: u64::from(i.dwVolumeSerialNumber),
            ino: (u64::from(i.nFileIndexHigh) << 32) | u64::from(i.nFileIndexLow),
            len: (u64::from(i.nFileSizeHigh) << 32) | u64::from(i.nFileSizeLow),
            mtime: (write as i64, 0),
            ctime: (0, 0),
        })
    }
}

/// What a scan came to.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Report {
    pub found: Vec<Finding>,
    /// Files read, of `files` listed.
    pub scanned: usize,
    pub files: usize,
    /// Files not read: only in the cloud, or too many with a line break in their name.
    pub left_out: usize,
    /// Lines ClamAV wrote about files it could not read, at most twenty.
    pub problems: Vec<String>,
    /// More was found than is kept (`FOUND_MAX`).
    pub more: bool,
    /// When the database it used was updated.
    pub db: Option<u64>,
    /// The update before it failed: why (the scan used the database there was).
    pub update_failed: Option<String>,
}

static N: AtomicUsize = AtomicUsize::new(0);

/// Scan `targets`: the database brought up to date when it is due, then every file under them
/// read. `stop` ends it (and its container); a stopped scan is an error saying so.
pub fn scan(cfg: &PreviewConfig, targets: &[PathBuf], stop: &AtomicBool, mut said: impl FnMut(Said)) -> Result<Report, String> {
    let (rt, rt_path, image) = how(cfg)?;
    let who = who(rt, &rt_path)?;
    let db = db_dir().ok_or_else(|| t!("err.no_cache_folder"))?;
    std::fs::create_dir_all(&db).map_err(|e| format!("{}: {e}", db.display()))?;
    if crate::tools::image_size(&rt_path, &image).is_none() {
        // Stop ends the pull too: it runs on a scoped thread that watches `stop`.
        std::thread::scope(|scope| {
            let puller = scope.spawn(|| crate::images::pull_until(&rt_path, &image, stop));
            while !puller.is_finished() {
                if let Some(line) = crate::images::pulling().get(image.as_str()) {
                    said(Said::Pulling(line));
                }
                std::thread::sleep(Duration::from_millis(300));
            }
            puller.join().map_err(|_| t!("convert.failed", "status" => "pull"))?
        })?;
    }
    let bounds = crate::images::bounds_work(&rt_path, &image);
    let scratch = Scratch::new()?;
    let mut report = Report::default();
    if due(db_updated(), crate::fs::now())
        && let Err(e) = update(&who, &rt_path, &image, &db, &scratch.0, bounds, stop, &mut said)
    {
        if stop.load(Ordering::Relaxed) {
            return Err(t!("viruses.stopped"));
        }
        if db_updated().is_none() {
            return Err(t!("viruses.no_db", "server" => SERVER, "error" => e));
        }
        report.update_failed = Some(e);
    }
    report.db = db_updated();
    let (files, left) = walk(targets, stop);
    if stop.load(Ordering::Relaxed) {
        return Err(t!("viruses.stopped"));
    }
    report.files = files.len();
    report.left_out = left;
    if files.is_empty() {
        return Ok(report);
    }
    let root = root(targets);
    // The list clamscan reads, one name a line, as the container sees each; and back from
    // that name to the file here. A name with a line break gets a mount of its own.
    let mut back: HashMap<Vec<u8>, PathBuf> = HashMap::new();
    let (mut list, mut odd) = (Vec::<u8>::new(), vec![]);
    for f in &files {
        let inside = match f.strip_prefix(&root) {
            Ok(rel) if !bytes(rel).iter().any(|b| matches!(b, b'\n' | b'\r')) => [b"/src/".as_slice(), &bytes(rel)].concat(),
            _ if odd.len() < ODD_MAX => {
                odd.push(f.clone());
                format!("/odd/{}", odd.len()).into_bytes()
            }
            _ => {
                report.left_out += 1;
                continue;
            }
        };
        list.extend_from_slice(&inside);
        list.push(b'\n');
        back.insert(inside, f.clone());
    }
    let all = back.len();
    std::fs::write(scratch.0.join("files"), &list).map_err(|e| e.to_string())?;
    let name = format!("coxswain-clamscan-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
    let mut c = sealed(&rt_path, &name, &who, bounds, false);
    c.arg("--mount").arg(crate::images::bind(&root, "/src", true));
    c.arg("--mount").arg(crate::images::bind(&db, "/db", true));
    c.arg("--mount").arg(crate::images::bind(&scratch.0, "/list", true));
    for (i, f) in odd.iter().enumerate() {
        c.arg("--mount").arg(crate::images::bind(f, &format!("/odd/{}", i + 1), true));
    }
    c.args(["--entrypoint", "clamscan", image.as_str(), "--database=/db", "--file-list=/list/files", "--tempdir=/tmp", "--no-summary", "--stdout"]);
    said(Said::Scanned(0, all));
    let (status, err) = follow(c, &rt_path, &name, stop, |line| {
        let Some((path, verdict)) = parse(line) else { return };
        let Some(file) = back.get(path) else { return };
        report.scanned += 1;
        if let Some(sig) = verdict.strip_suffix(" FOUND") {
            if report.found.len() < FOUND_MAX {
                report.found.push(Finding { path: file.clone(), name: clean(sig), id: Ident::of(file) });
            } else {
                report.more = true;
            }
        } else if verdict != "OK" && report.problems.len() < 20 {
            report.problems.push(clean(&format!("{}: {}", file.display(), verdict)));
        }
        said(Said::Scanned(report.scanned.min(all), all));
    })?;
    // 0: nothing found, 1: something found, 2: some files could not be read.
    match status {
        Some(0 | 1) => Ok(report),
        Some(2) if report.scanned > 0 => {
            report.problems.extend(err.into_iter().take(20usize.saturating_sub(report.problems.len())));
            Ok(report)
        }
        s => {
            let tail = err.join("\n");
            Err(if tail.is_empty() { t!("convert.failed", "status" => s.map_or("?".into(), |s| s.to_string())) } else { tail })
        }
    }
}

/// The folder mounted at /src: the one folder scanned, else the folder the targets are in.
fn root(targets: &[PathBuf]) -> PathBuf {
    match targets {
        [one] if std::fs::symlink_metadata(one).is_ok_and(|m| m.is_dir()) => one.clone(),
        [first, ..] => {
            let mut r = first.parent().unwrap_or(Path::new("/")).to_path_buf();
            while !targets.iter().all(|t| t.starts_with(&r)) {
                if !r.pop() {
                    break;
                }
            }
            r
        }
        [] => PathBuf::from("/"),
    }
}

/// A line of clamscan's: the name it read and what it says of it (`OK`, `<sig> FOUND`, an
/// error). The verdict comes last and has no `: ` in it, so a name with `: ` in it stays whole.
fn parse(line: &[u8]) -> Option<(&[u8], &str)> {
    let at = line.windows(2).rposition(|w| w == b": ")?;
    Some((&line[..at], std::str::from_utf8(&line[at + 2..]).ok()?.trim_end()))
}

/// Text from a container, safe to show: control characters (escape sequences) become spaces,
/// and it is cut at 200 characters.
fn clean(s: &str) -> String {
    s.chars().take(200).map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

#[cfg(unix)]
fn bytes(p: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn bytes(p: &Path) -> Vec<u8> {
    p.to_string_lossy().replace('\\', "/").into_bytes()
}

/// `<rt> run` as both runs start: removed when it ends, a read-only root with a small `/tmp`
/// in memory, no capabilities, no gaining of privileges, bounded memory and processes where
/// the runtime can bound them, and as `who` says (`who`): you, so freshclam need not change
/// user and clamscan reads what you can. `net`: network (freshclam only); else none.
fn sealed(rt_path: &Path, name: &str, who: &[String], bounds: bool, net: bool) -> Command {
    let mut c = crate::tools::command(rt_path);
    c.args(["run", "--rm", "-i", "--security-opt", "label=disable", "--name", name]);
    if !net {
        c.arg("--network=none");
    }
    // Every container's options (`images::bounded`), with ClamAV's own bounds instead of theirs.
    c.args(crate::images::bounded(false));
    if bounds {
        c.args(["--memory=4g", "--pids-limit=64"]);
    }
    c.args(who);
    c
}

/// Who the containers run as: you. In rootless podman your own user id inside too
/// (`keep-id`); with docker or podman run as root, your user and group ids (`--user`), never
/// those of the folder's owner. Run as root itself, it does not scan: the containers would be
/// root.
fn who(rt: &str, rt_path: &Path) -> Result<Vec<String>, String> {
    if rt == "podman" && crate::images::rootless(rt_path) {
        return Ok(vec!["--userns=keep-id".into()]);
    }
    #[cfg(unix)]
    {
        // SAFETY: getuid and getgid cannot fail and touch no memory.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        if uid == 0 {
            return Err(t!("viruses.root"));
        }
        Ok(vec!["--user".into(), format!("{uid}:{gid}"), "-e".into(), "HOME=/tmp".into()])
    }
    #[cfg(not(unix))]
    {
        let _ = rt_path;
        Ok(vec![])
    }
}

/// Bring the database in `db` up to date with freshclam, from `SERVER`. The container sees
/// only `db`, and a config file of Coxswain's read-only; ten minutes at most.
#[allow(clippy::too_many_arguments)]
fn update(who: &[String], rt_path: &Path, image: &str, db: &Path, scratch: &Path, bounds: bool, stop: &AtomicBool, said: &mut impl FnMut(Said)) -> Result<(), String> {
    let conf = scratch.join("conf");
    std::fs::create_dir_all(&conf).map_err(|e| e.to_string())?;
    std::fs::write(conf.join("freshclam.conf"), format!("DatabaseDirectory /db\nDatabaseMirror {SERVER}\nForeground yes\nConnectTimeout 30\n")).map_err(|e| e.to_string())?;
    let name = format!("coxswain-freshclam-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
    let mut c = sealed(rt_path, &name, who, bounds, true);
    c.arg("--mount").arg(crate::images::bind(db, "/db", false));
    c.arg("--mount").arg(crate::images::bind(&conf, "/conf", true));
    c.args(["--entrypoint", "freshclam", image, "--config-file=/conf/freshclam.conf", "--stdout", "--log=/dev/null"]);
    let started = std::time::Instant::now();
    let watch = AtomicBool::new(false);
    let mut last = vec![];
    let done = AtomicBool::new(false);
    let (status, err) = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                if stop.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(600) {
                    watch.store(true, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let r = follow(c, rt_path, &name, &watch, |line| {
            let l = clean(&String::from_utf8_lossy(line));
            if !l.is_empty() {
                said(Said::Updating(&l));
                last.push(l);
                if last.len() > 6 {
                    last.remove(0);
                }
            }
        });
        done.store(true, Ordering::Relaxed);
        r
    })?;
    if status == Some(0) {
        let _ = std::fs::write(db.join("updated"), "");
        return Ok(());
    }
    last.extend(err);
    Err(if last.is_empty() { t!("convert.failed", "status" => status.map_or("?".into(), |s| s.to_string())) } else { last.join("\n") })
}

/// Run `c`, each line of its output to `line` as it comes (64 KB at most a line), its last
/// complaints kept. `stop` kills it and its container named `name`, and is an error. The exit
/// code, and the last lines it complained.
fn follow(c: Command, rt: &Path, name: &str, stop: &AtomicBool, mut line: impl FnMut(&[u8])) -> Result<(Option<i32>, Vec<String>), String> {
    let mut c = crate::tools::hosted(c);
    let mut child = c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let err = child.stderr.take();
    let complaints = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err {
            let _ = e.take(1 << 20).read_to_string(&mut s);
        }
        s.lines().map(clean).filter(|l| !l.is_empty()).collect::<Vec<_>>().into_iter().rev().take(8).rev().collect::<Vec<_>>()
    });
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let mut out = child.stdout.take().expect("piped");
    std::thread::spawn(move || {
        let (mut buf, mut acc) = ([0u8; 8192], Vec::new());
        while let Ok(n) = out.read(&mut buf) {
            if n == 0 {
                break;
            }
            acc.extend_from_slice(&buf[..n]);
            while let Some(i) = acc.iter().position(|&b| b == b'\n') {
                let l: Vec<u8> = acc.drain(..=i).take(i).collect();
                if tx.send(l).is_err() {
                    return;
                }
            }
            if acc.len() > 64 << 10 {
                acc.clear();
            }
        }
        if !acc.is_empty() {
            let _ = tx.send(acc);
        }
    });
    let status = loop {
        while let Ok(l) = rx.try_recv() {
            line(&l);
        }
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = crate::tools::command(rt).args(["kill", name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            let _ = child.wait();
            return Err(t!("viruses.stopped"));
        }
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    // What it wrote last, after it ended.
    while let Ok(l) = rx.recv_timeout(Duration::from_millis(500)) {
        line(&l);
    }
    Ok((status.code(), complaints.join().unwrap_or_default()))
}

/// What to do with a file something was found in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// Leave it where it is.
    Nothing,
    /// Into the bin, from where it can be put back.
    Bin,
    /// Into the quarantine folder, where it cannot be run.
    Quarantine,
}

/// Do `choice` with the file of `f`: where it went, for quarantine. Only the file that was
/// found is acted on: when what is at its path now is another (a link, a folder, a new file),
/// or its identity cannot be told, it is left alone and the error says so.
pub fn act(f: &Finding, choice: Choice) -> Result<Option<PathBuf>, String> {
    if choice == Choice::Nothing {
        return Ok(None);
    }
    let Some(was) = f.id else { return Err(format!("{}: {}", f.path.display(), t!("viruses.unsure"))) };
    same(f, was)?;
    match choice {
        Choice::Nothing => Ok(None),
        // ponytail: the bin is reached by path (the desktop's trash takes no folder handle), so
        // a file swapped in the instant between this check and the move would go to the bin;
        // it can be put back from there. Quarantine moves through the folder's handle.
        Choice::Bin => crate::fs::trash(&f.path).map(|_| None).map_err(|e| format!("{}: {e}", f.path.display())),
        Choice::Quarantine => {
            let dir = quarantine_dir().ok_or_else(|| t!("viruses.no_quarantine"))?;
            quarantine(f, was, &dir).map(Some)
        }
    }
}

/// Is the file at `f.path` still the regular file that was found?
fn same(f: &Finding, was: Ident) -> Result<(), String> {
    match Ident::of(&f.path) {
        Some(now) if now == was => Ok(()),
        _ => Err(changed(&f.path)),
    }
}

fn changed(p: &Path) -> String {
    format!("{}: {}", p.display(), t!("viruses.changed"))
}

/// Move the file of `f` into `dir` under a name of its own (nothing there is written over),
/// readable by you alone and by nobody run, and add a line to `dir/quarantine.log`: when, what
/// was found, and where it came from, so it can be put back.
fn quarantine(f: &Finding, was: Ident, dir: &Path) -> Result<PathBuf, String> {
    let err = |e: std::io::Error| format!("{}: {e}", f.path.display());
    std::fs::create_dir_all(dir).map_err(err)?;
    let name = f.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let now = crate::fs::now();
    let to = (1..)
        .map(|n| dir.join(if n == 1 { format!("{now}-{name}") } else { format!("{now}-{n}-{name}") }))
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .expect("a free name");
    #[cfg(unix)]
    at::quarantine(&f.path, was, &to).map_err(|e| if e.kind() == std::io::ErrorKind::InvalidData { changed(&f.path) } else { err(e) })?;
    #[cfg(not(unix))]
    {
        // ponytail: by path, checked again just before: Windows has no rename relative to an
        // open folder in the standard library; the window is the instant between the two.
        same(f, was)?;
        std::fs::rename(&f.path, &to).map_err(err)?;
    }
    use std::io::Write;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("quarantine.log"));
    if let Ok(mut log) = log {
        let _ = writeln!(log, "{}\t{}\t{}\t{}", crate::i18n::date_time(now), f.name, f.path.display(), to.display());
    }
    Ok(to)
}

/// The quarantine's moves on Unix, relative to the folders' own handles (opened without
/// following a link), so the name checked is the name moved: `renameat`, `unlinkat`, and the
/// file opened with `O_NOFOLLOW` from its folder. A file that is not the one found is an
/// `InvalidData` error.
#[cfg(unix)]
mod at {
    use super::Ident;
    use std::ffi::CString;
    use std::fs::File;
    use std::io::{self, Error, ErrorKind};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    fn c(s: &std::ffi::OsStr) -> io::Result<CString> {
        CString::new(s.as_bytes()).map_err(|_| Error::from(ErrorKind::InvalidInput))
    }

    /// The folder `p` is in, opened as a folder without following a link, and its name there.
    fn parent(p: &Path) -> io::Result<(OwnedFd, CString)> {
        let dir = c(p.parent().ok_or(ErrorKind::InvalidInput)?.as_os_str())?;
        let name = c(p.file_name().ok_or(ErrorKind::InvalidInput)?)?;
        // SAFETY: a valid C string; the descriptor is owned from here on.
        let fd = unsafe { libc::open(dir.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(Error::last_os_error());
        }
        Ok((unsafe { OwnedFd::from_raw_fd(fd) }, name))
    }

    /// `name` in `dir`, opened without following a link and without waiting on a pipe; only a
    /// regular file.
    fn open(dir: &OwnedFd, name: &CString) -> io::Result<File> {
        // SAFETY: an open folder and a valid C string.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(Error::new(ErrorKind::InvalidData, Error::last_os_error()));
        }
        let f = unsafe { File::from_raw_fd(fd) };
        if !f.metadata()?.file_type().is_file() {
            return Err(ErrorKind::InvalidData.into());
        }
        Ok(f)
    }

    /// The open file is the one found.
    fn check(f: &File, was: &Ident) -> io::Result<()> {
        if Ident::from(&f.metadata()?) == *was { Ok(()) } else { Err(ErrorKind::InvalidData.into()) }
    }

    pub fn quarantine(from: &Path, was: Ident, to: &Path) -> io::Result<()> {
        let (src, name) = parent(from)?;
        let (dst, new) = parent(to)?;
        let file = open(&src, &name)?;
        check(&file, &was)?;
        // SAFETY: open folders and valid C strings.
        let r = unsafe { libc::renameat(src.as_raw_fd(), name.as_ptr(), dst.as_raw_fd(), new.as_ptr()) };
        if r == 0 {
            // What was moved is the file found (a rename changes only its inode's time); else
            // it goes back where it was.
            let moved = open(&dst, &new)?;
            let m = Ident::from(&moved.metadata()?);
            if !m.same_inode(&was) {
                unsafe { libc::renameat(dst.as_raw_fd(), new.as_ptr(), src.as_raw_fd(), name.as_ptr()) };
                return Err(ErrorKind::InvalidData.into());
            }
            return lock(&moved);
        }
        let e = Error::last_os_error();
        if e.raw_os_error() != Some(libc::EXDEV) {
            return Err(e);
        }
        // Another disk: copied from the file opened above, then the name removed only when it
        // still is that file.
        let mut o = std::fs::OpenOptions::new();
        use std::os::unix::fs::OpenOptionsExt;
        o.write(true).create_new(true).mode(0o600);
        let mut out = o.open(to)?;
        let copied = std::io::copy(&mut &file, &mut out).and_then(|_| {
            let again = open(&src, &name)?;
            check(&again, &Ident::from(&file.metadata()?))
        });
        if let Err(e) = copied {
            let _ = std::fs::remove_file(to);
            return Err(e);
        }
        // SAFETY: an open folder and a valid C string.
        if unsafe { libc::unlinkat(src.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            let e = Error::last_os_error();
            let _ = std::fs::remove_file(to);
            return Err(e);
        }
        lock(&out)
    }

    /// Read-only, by you alone, never run: on the open file itself (`fchmod`), no link followed.
    fn lock(f: &File) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o400))
    }
}

/// A folder of its own in the cache for one run (the list of files, freshclam's config),
/// removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Scratch, String> {
        let base = crate::dirs::cache_dir().ok_or_else(|| t!("err.no_cache_folder"))?.join("coxswain").join("clamscan");
        // What a run of an app that was killed left behind, a day on.
        for e in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            if e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|a| a > Duration::from_secs(24 * 3600))) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
        let dir = base.join(format!("{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The EICAR test file: no virus, but every scanner reports it as one.
    pub(crate) fn eicar() -> String {
        ["X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR", "-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*"].concat()
    }

    #[test]
    fn viruses_lines_are_read_from_the_right_and_only_for_listed_files() {
        assert_eq!(parse(b"/src/a: b: Eicar-Test-Signature FOUND"), Some((b"/src/a: b".as_slice(), "Eicar-Test-Signature FOUND")));
        assert_eq!(parse(b"/src/x: OK: OK"), Some((b"/src/x: OK".as_slice(), "OK")));
        assert_eq!(parse(b"no verdict"), None);
        assert_eq!(clean("Bad\x1b[31mName\n"), "Bad [31mName");
    }

    #[test]
    fn viruses_database_is_updated_when_older_than_a_day() {
        assert!(due(None, 100));
        assert!(!due(Some(1000), 1000 + FRESH));
        assert!(due(Some(1000), 1001 + FRESH));
    }

    #[test]
    fn viruses_are_off_by_default_and_say_so() {
        crate::i18n::set_language("en-GB");
        let cfg = PreviewConfig::default();
        assert!(!cfg.virus_scan);
        assert_eq!(how(&cfg).unwrap_err(), t!("viruses.off"));
        let on = PreviewConfig { virus_scan: true, container: "off".into(), ..Default::default() };
        // On the BSDs and illumos no container runs at all, and that is what it says.
        let want = if cfg!(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "illumos", target_os = "solaris")) { t!("viruses.not_here") } else { t!("searchable.containers_off") };
        assert!(how(&on).unwrap_err().contains(&want));
    }

    #[test]
    fn viruses_walk_skips_links_and_mounts_the_folder() {
        let d = std::env::temp_dir().join(format!("coxswain-viruses-walk-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a"), "a").unwrap();
        std::fs::write(d.join("sub/b"), "b").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/passwd", d.join("link")).unwrap();
        let (files, left) = walk(std::slice::from_ref(&d), &AtomicBool::new(false));
        assert_eq!((files, left), (vec![d.join("a"), d.join("sub/b")], 0));
        assert_eq!(root(std::slice::from_ref(&d)), d);
        assert_eq!(root(&[d.join("a"), d.join("sub")]), d);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn viruses_quarantine_never_writes_over_and_logs_where_from() {
        let d = std::env::temp_dir().join(format!("coxswain-viruses-q-{}", std::process::id()));
        let q = d.join("q");
        std::fs::create_dir_all(&d).unwrap();
        let mut put = vec![];
        for _ in 0..2 {
            std::fs::write(d.join("eicar.com"), eicar()).unwrap();
            let f = Finding { path: d.join("eicar.com"), name: "Eicar-Test-Signature".into(), id: Ident::of(&d.join("eicar.com")) };
            put.push(quarantine(&f, f.id.unwrap(), &q).unwrap());
            assert!(!f.path.exists());
        }
        assert_ne!(put[0], put[1], "the second one gets a name of its own");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&put[0]).unwrap().permissions().mode() & 0o777, 0o400);
        }
        let log = std::fs::read_to_string(q.join("quarantine.log")).unwrap();
        assert_eq!(log.lines().count(), 2);
        assert!(log.contains("Eicar-Test-Signature") && log.contains(&d.join("eicar.com").display().to_string()), "{log}");
        #[cfg(unix)]
        for p in &put {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn viruses_act_refuses_a_file_put_in_the_found_ones_place() {
        crate::i18n::set_language("en-GB");
        let d = std::env::temp_dir().join(format!("coxswain-viruses-swap-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let found = d.join("eicar.com");
        std::fs::write(&found, eicar()).unwrap();
        let f = Finding { path: found.clone(), name: "Eicar-Test-Signature".into(), id: Ident::of(&found) };
        // Another file, the same name: refused.
        std::fs::remove_file(&found).unwrap();
        std::fs::write(&found, "something else, longer than before").unwrap();
        assert!(act(&f, Choice::Quarantine).unwrap_err().contains(&t!("viruses.changed")));
        // A link to a file of yours: refused, and that file's mode is as it was.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let other = d.join("mine.txt");
            std::fs::write(&other, "mine").unwrap();
            std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o644)).unwrap();
            std::fs::remove_file(&found).unwrap();
            std::os::unix::fs::symlink(&other, &found).unwrap();
            for c in [Choice::Quarantine, Choice::Bin] {
                assert!(act(&f, c).unwrap_err().contains(&t!("viruses.changed")));
            }
            assert_eq!(std::fs::metadata(&other).unwrap().permissions().mode() & 0o777, 0o644);
            assert!(std::fs::symlink_metadata(&found).unwrap().file_type().is_symlink(), "left where it was");
        }
        // One whose identity could not be told (no file id on this system): left alone, saying so.
        let none = Finding { id: None, ..f };
        for c in [Choice::Quarantine, Choice::Bin] {
            assert!(act(&none, c).unwrap_err().contains(&t!("viruses.unsure")));
        }
        assert_eq!(act(&none, Choice::Nothing), Ok(None));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn viruses_identity_is_of_a_regular_file_only() {
        let d = std::env::temp_dir().join(format!("coxswain-viruses-id-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a"), "a").unwrap();
        let id = Ident::of(&d.join("a")).expect("a file has one");
        assert_eq!(Ident::of(&d.join("a")), Some(id));
        assert_eq!(Ident::of(&d), None, "a folder has none");
        std::fs::write(d.join("b"), "a").unwrap();
        assert_ne!(Ident::of(&d.join("b")), Some(id), "another file, the same content");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Moved onto another disk (`/dev/shm` when it is one), the quarantine copies the file found
    /// and removes it only then.
    #[test]
    #[cfg(target_os = "linux")]
    fn viruses_quarantine_across_disks() {
        let shm = std::path::Path::new("/dev/shm");
        let d = std::env::temp_dir().join(format!("coxswain-viruses-xdev-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let q = shm.join(format!("coxswain-viruses-q-{}", std::process::id()));
        use std::os::unix::fs::MetadataExt;
        if !shm.is_dir() || std::fs::metadata(shm).unwrap().dev() == std::fs::metadata(&d).unwrap().dev() {
            return std::fs::remove_dir_all(d).unwrap();
        }
        let path = d.join("eicar.com");
        std::fs::write(&path, eicar()).unwrap();
        let f = Finding { path: path.clone(), name: "Eicar-Test-Signature".into(), id: Ident::of(&path) };
        let to = quarantine(&f, f.id.unwrap(), &q).unwrap();
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(&to).unwrap(), eicar());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&to).unwrap().permissions().mode() & 0o777, 0o400);
        std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::remove_dir_all(q).unwrap();
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn viruses_containers_run_as_you_never_as_a_folders_owner() {
        // SAFETY: as in `who`.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        let w = who("docker", Path::new("/nonexistent/docker"));
        if uid == 0 {
            assert!(w.is_err(), "as root: no scan");
        } else {
            assert_eq!(w.unwrap()[..2], ["--user".to_string(), format!("{uid}:{gid}")]);
        }
    }

    /// Needs podman (or a `podman` on PATH that adds its own store) and the network, with a
    /// fake HOME: `cargo test -p coxswain-core -- --ignored real_virus_scan`. The EICAR test
    /// file, alone, in a zip, and under a name with a line break, among clean files.
    #[test]
    #[ignore]
    fn real_virus_scan() {
        let d = std::env::temp_dir().join(format!("coxswain-real-viruses-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("eicar: odd OK.com"), eicar()).unwrap();
        std::fs::write(d.join("sub/two\nlines.com"), eicar()).unwrap();
        std::fs::write(d.join("sub/clean.txt"), "hello").unwrap();
        let cfg = PreviewConfig { virus_scan: true, ..Default::default() };
        let start = std::time::Instant::now();
        let r = scan(&cfg, std::slice::from_ref(&d), &AtomicBool::new(false), |s| match s {
            Said::Updating(l) | Said::Pulling(l) => eprintln!("{l}"),
            Said::Scanned(n, all) => eprintln!("{n} of {all}"),
        })
        .unwrap();
        eprintln!("{r:?} in {:?}", start.elapsed());
        assert_eq!((r.files, r.scanned, r.left_out), (3, 3, 0));
        let mut found: Vec<_> = r.found.iter().map(|f| f.path.clone()).collect();
        found.sort();
        assert_eq!(found, [d.join("eicar: odd OK.com"), d.join("sub/two\nlines.com")]);
        assert!(r.found.iter().all(|f| f.name.contains("Eicar")), "{:?}", r.found);
        assert!(r.db.is_some() && r.update_failed.is_none());
        // Stopped at once: an error, and no container left.
        let r = scan(&cfg, std::slice::from_ref(&d), &AtomicBool::new(true), |_| {});
        assert_eq!(r.unwrap_err(), t!("viruses.stopped"));
        std::fs::remove_dir_all(d).unwrap();
    }
}

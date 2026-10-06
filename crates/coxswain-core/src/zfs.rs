//! ZFS: the dataset a folder is in, its facts (compression, space, quota), and its snapshots as
//! read-only folders. `<folder>/@snapshots` lists the snapshots of the folder's dataset, newest
//! first; each leads to the folder as that snapshot has it, at
//! `<mountpoint>/.zfs/snapshot/<name>/<folder below the mountpoint>`, which ZFS itself serves,
//! read-only and without root, whether `snapdir` is `hidden` or `visible`. Everything through
//! the `zfs` command, as an administrator would run it; nothing is ever changed.
//! On FreeBSD, Linux and macOS with OpenZFS; elsewhere, or without ZFS, nothing is found.

use serde::Serialize;
use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::fs::Entry;

/// The path segment after a folder that leads to its dataset's snapshots.
pub const MARKER: &str = "@snapshots";
/// How long one `zfs` run may take.
const BUDGET: Duration = Duration::from_secs(4);

/// A ZFS file system as it is mounted.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Dataset {
    pub name: String,
    pub mountpoint: PathBuf,
}

/// The `zfs` program: on PATH, or where the systems put it (a user's PATH on Linux has no
/// `/sbin`; OpenZFS on macOS installs in `/usr/local/zfs/bin`).
fn zfs_program() -> Option<PathBuf> {
    crate::tools::which("zfs").or_else(|| ["/sbin/zfs", "/usr/sbin/zfs", "/usr/local/sbin/zfs", "/usr/local/zfs/bin/zfs"].iter().map(PathBuf::from).find(|p| p.is_file()))
}

/// What `zfs` with these arguments prints; its complaint as the error.
fn zfs(args: &[&str]) -> io::Result<String> {
    let program = zfs_program().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, crate::t!("zfs.no_program")))?;
    let mut c = crate::tools::command(program);
    c.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = c.spawn()?;
    // Read while it runs: a long list would fill the pipe and never end.
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let reader = std::thread::spawn(move || {
        let mut v = String::new();
        if let Some(o) = out.as_mut() {
            let _ = io::Read::read_to_string(o, &mut v);
        }
        v
    });
    let status = crate::tools::wait(&mut child, BUDGET)?;
    let text = reader.join().unwrap_or_default();
    if !status.success() {
        let mut why = String::new();
        if let Some(e) = err.as_mut() {
            let _ = io::Read::read_to_string(e, &mut why);
        }
        return Err(io::Error::other(if why.trim().is_empty() { status.to_string() } else { why.trim().to_string() }));
    }
    Ok(text)
}

// ---------------------------------------------------------------- which dataset

/// The ZFS mounts in `/proc/self/mountinfo`'s text (Linux), snapshots left out.
pub fn parse_mountinfo(mountinfo: &str) -> Vec<Dataset> {
    let unescape = |s: &str| s.replace("\\040", " ").replace("\\011", "\t").replace("\\012", "\n").replace("\\134", "\\");
    mountinfo
        .lines()
        .filter_map(|l| {
            let (left, right) = l.split_once(" - ")?;
            let point = left.split(' ').nth(4)?;
            let mut r = right.split(' ');
            let (fstype, source) = (r.next()?, r.next()?);
            (fstype == "zfs" && !source.contains('@')).then(|| Dataset { name: unescape(source), mountpoint: PathBuf::from(unescape(point)) })
        })
        .collect()
}

/// The dataset `dir` is on, and where it is mounted. A folder inside a snapshot is on the
/// dataset the snapshot is of. Cheap: the kernel's mount table, no `zfs` run.
pub fn dataset_of(dir: &Path) -> Option<Dataset> {
    if let Some(s) = in_snapshot(dir) {
        return dataset_of(&s.mountpoint);
    }
    mounted(dir).filter(|d| !d.name.contains('@'))
}

#[cfg(target_os = "linux")]
fn mounted(dir: &Path) -> Option<Dataset> {
    let text = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    parse_mountinfo(&text).into_iter().filter(|d| dir.starts_with(&d.mountpoint)).max_by_key(|d| d.mountpoint.as_os_str().len())
}

/// statfs(2) names the file system a path is on: its type, what is mounted and where.
#[cfg(any(target_os = "freebsd", target_os = "macos"))]
fn mounted(dir: &Path) -> Option<Dataset> {
    use std::ffi::{CStr, CString};
    let c = CString::new(dir.as_os_str().as_encoded_bytes()).ok()?;
    // SAFETY: `c` is a NUL-terminated path; statfs fills the zeroed struct it is given.
    let st = unsafe {
        let mut st: libc::statfs = std::mem::zeroed();
        if libc::statfs(c.as_ptr(), &mut st) != 0 {
            return None;
        }
        st
    };
    // SAFETY: the kernel ends each name with a NUL inside its array.
    let text = |a: &[libc::c_char]| unsafe { CStr::from_ptr(a.as_ptr()) }.to_string_lossy().into_owned();
    (text(&st.f_fstypename) == "zfs").then(|| Dataset { name: text(&st.f_mntfromname), mountpoint: PathBuf::from(text(&st.f_mntonname)) })
}

/// /etc/mnttab names the dataset mounted at each mount point (illumos).
#[cfg(any(target_os = "illumos", target_os = "solaris"))]
fn mounted(dir: &Path) -> Option<Dataset> {
    let text = std::fs::read_to_string("/etc/mnttab").ok()?;
    crate::machine::parse_mnttab(&text).into_iter().filter(|m| m.fstype == "zfs" && dir.starts_with(&m.mount)).max_by_key(|m| m.mount.as_os_str().len()).map(|m| Dataset { name: m.device, mountpoint: m.mount })
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "macos", target_os = "illumos", target_os = "solaris")))]
fn mounted(_dir: &Path) -> Option<Dataset> {
    None
}

// ---------------------------------------------------------------- facts

/// What the footer and Properties tell of a dataset. Sizes in bytes; 0 for no quota.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Facts {
    pub dataset: String,
    pub mountpoint: PathBuf,
    /// The snapshot looked into, when the folder is inside one.
    pub snapshot: Option<String>,
    pub compression: String,
    /// As ZFS gives it, `1.52`: data written against space taken.
    pub compressratio: String,
    pub used: u64,
    pub available: u64,
    pub referenced: u64,
    pub quota: u64,
    pub refquota: u64,
}

const PROPS: &str = "used,available,referenced,compressratio,compression,quota,refquota";

/// `zfs get -H -p -o property,value …`'s text, as facts of `ds`.
pub fn parse_props(ds: &Dataset, text: &str) -> Facts {
    let p: HashMap<&str, &str> = text.lines().filter_map(|l| l.split_once('\t')).collect();
    let n = |k: &str| p.get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    Facts {
        dataset: ds.name.clone(),
        mountpoint: ds.mountpoint.clone(),
        snapshot: None,
        compression: p.get("compression").unwrap_or(&"").trim().to_string(),
        compressratio: p.get("compressratio").unwrap_or(&"").trim().trim_end_matches('x').to_string(),
        used: n("used"),
        available: n("available"),
        referenced: n("referenced"),
        quota: n("quota"),
        refquota: n("refquota"),
    }
}

/// The facts of the dataset `dir` is on (or whose snapshots it lists); `None` off ZFS. Kept for
/// ten seconds a dataset, so moving between its folders runs `zfs` once.
pub fn facts(dir: &Path) -> Option<Facts> {
    type Cache = HashMap<String, (Instant, Facts)>;
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    let of = split(dir);
    let dir = of.as_deref().unwrap_or(dir);
    let ds = dataset_of(dir)?;
    let snapshot = in_snapshot(dir).map(|s| s.name);
    let cached = CACHE.lock().ok()?.get_or_insert_default().get(&ds.name).filter(|(at, _)| at.elapsed() < Duration::from_secs(10)).map(|(_, f)| f.clone());
    let mut f = match cached {
        Some(f) => f,
        None => {
            let f = parse_props(&ds, &zfs(&["get", "-H", "-p", "-o", "property,value", PROPS, &ds.name]).ok()?);
            if let Ok(mut c) = CACHE.lock() {
                c.get_or_insert_default().insert(ds.name.clone(), (Instant::now(), f.clone()));
            }
            f
        }
    };
    f.snapshot = snapshot;
    Some(f)
}

// ---------------------------------------------------------------- snapshots

/// A snapshot: its name after the `@`, when it was taken (seconds), the space only it holds
/// (`used`: what destroying it frees) and the data it refers to.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Snapshot {
    pub name: String,
    pub created: u64,
    pub used: u64,
    pub referenced: u64,
}

/// `zfs list -H -p -o name,creation,used,refer -t snapshot`'s text: the snapshots of `dataset`
/// itself (not of the datasets below it), newest first.
pub fn parse_snapshots(dataset: &str, text: &str) -> Vec<Snapshot> {
    let mut v: Vec<Snapshot> = text
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let name = f.next()?.strip_prefix(dataset)?.strip_prefix('@')?.to_string();
            let mut n = || f.next().and_then(|s| s.trim().parse::<u64>().ok());
            Some(Snapshot { name, created: n()?, used: n().unwrap_or(0), referenced: n().unwrap_or(0) })
        })
        .collect();
    // Stable: snapshots taken in the same second keep ZFS's order (the order they were taken).
    v.reverse();
    v.sort_by_key(|s| std::cmp::Reverse(s.created));
    v
}

/// The snapshots of `dataset`, newest first.
pub fn snapshots(dataset: &str) -> io::Result<Vec<Snapshot>> {
    let text = zfs(&["list", "-H", "-p", "-o", "name,creation,used,refer", "-t", "snapshot", "-s", "creation", "-d", "1", dataset])?;
    Ok(parse_snapshots(dataset, &text))
}

/// Where a path is inside a snapshot: the dataset's mountpoint, the snapshot's name and the
/// path below the snapshot's top.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InSnapshot {
    pub mountpoint: PathBuf,
    pub name: String,
    pub rel: PathBuf,
}

/// Whether `path` is inside a snapshot (`…/.zfs/snapshot/<name>/…`), from the path alone.
pub fn in_snapshot(path: &Path) -> Option<InSnapshot> {
    let comps: Vec<Component> = path.components().collect();
    let i = comps.windows(3).position(|w| w[0].as_os_str() == ".zfs" && w[1].as_os_str() == "snapshot" && matches!(w[2], Component::Normal(_)))?;
    Some(InSnapshot {
        mountpoint: comps[..i].iter().collect(),
        name: comps[i + 2].as_os_str().to_string_lossy().into_owned(),
        rel: comps[i + 3..].iter().collect(),
    })
}

impl InSnapshot {
    /// The same file or folder as it is now.
    pub fn live(&self) -> PathBuf {
        if self.rel.as_os_str().is_empty() { self.mountpoint.clone() } else { self.mountpoint.join(&self.rel) }
    }
}

/// The folder `dir` (on the dataset mounted at `mountpoint`) as the snapshot `name` has it.
pub fn snapshot_path(mountpoint: &Path, name: &str, dir: &Path) -> PathBuf {
    let top = mountpoint.join(".zfs").join("snapshot").join(name);
    match dir.strip_prefix(mountpoint) {
        Ok(rel) if !rel.as_os_str().is_empty() => top.join(rel),
        _ => top,
    }
}

/// The folder whose snapshots `path` lists, when it is `<folder>/@snapshots` (and not a real
/// entry of that name).
pub fn split(path: &Path) -> Option<PathBuf> {
    if path.file_name()? != MARKER || std::fs::symlink_metadata(path).is_ok() {
        return None;
    }
    let dir = path.parent()?;
    dir.is_dir().then(|| dir.to_path_buf())
}

/// Whether `path` is the list of snapshots or inside a snapshot: nothing is written there.
pub fn is_read_only(path: &Path) -> bool {
    in_snapshot(path).is_some() || path.ancestors().any(|a| split(a).is_some())
}

/// The list of snapshots of the folder `dir`'s dataset, as `fs::list` gives it: `..` back to
/// the folder, then one folder a snapshot, newest first, leading to `dir` as it was then. Its
/// size is the snapshot's own space (`used`); `referenced` is the data it refers to.
pub fn list(dir: &Path) -> io::Result<Vec<Entry>> {
    let ds = dataset_of(dir).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, crate::t!("zfs.not_zfs", "dir" => dir.display())))?;
    let up = Entry { name: "..".into(), path: dir.to_path_buf(), is_dir: true, ..Entry::default() };
    let snaps = snapshots(&ds.name)?;
    Ok(std::iter::once(up)
        .chain(snaps.into_iter().map(|s| Entry {
            path: snapshot_path(&ds.mountpoint, &s.name, dir),
            name: s.name,
            is_dir: true,
            size: s.used,
            modified: s.created,
            created: s.created,
            referenced: s.referenced,
            ..Entry::default()
        }))
        .collect())
}

/// Where a folder is among a dataset's snapshots, for its badge and tint: the dataset, the
/// snapshot looked into (none: the list of snapshots) and the folder as it is now.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct At {
    pub dataset: String,
    pub snapshot: Option<String>,
    pub live: PathBuf,
}

/// `dir` among the snapshots, if it is: inside one, or their list.
pub fn at(dir: &Path) -> Option<At> {
    if let Some(of) = split(dir) {
        return Some(At { dataset: dataset_of(&of)?.name, snapshot: None, live: of });
    }
    let s = in_snapshot(dir)?;
    // Not ZFS after all: a folder that happens to be called so.
    let ds = dataset_of(&s.mountpoint)?;
    Some(At { dataset: ds.name, live: s.live(), snapshot: Some(s.name) })
}

/// The path of `dir`'s list of snapshots.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(MARKER)
}

/// What changed in a file since the snapshot: `diff -u` of the snapshot's copy against the
/// file now (a file gone since, or new since, is diffed against nothing).
pub fn diff(path: &Path) -> io::Result<String> {
    let at = in_snapshot(path).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not in a snapshot"))?;
    let now = at.live();
    let label = |p: &Path, what: &str| format!("{} ({what})", p.display());
    let mut c = crate::tools::command("diff");
    c.arg("-u").arg("-N").arg("-L").arg(label(&now, &format!("@{}", at.name))).arg("-L").arg(label(&now, &crate::t!("zfs.now"))).arg(path).arg(&now);
    let o = c.output()?;
    // 0: the same; 1: differences; 2: trouble.
    if o.status.code() == Some(2) {
        return Err(io::Error::other(String::from_utf8_lossy(&o.stderr).trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).chars().take(512 * 1024).collect())
}

/// `diff` of a file in a snapshot against now, written next to the copies the viewer reads:
/// `None` when it is the same now.
pub fn diff_file(path: &Path) -> io::Result<Option<PathBuf>> {
    let text = diff(path)?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let to = crate::archive::peek_folder()?.join(format!("{}.diff", path.file_name().unwrap_or_default().to_string_lossy()));
    std::fs::write(&to, text)?;
    Ok(Some(to))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zfs_mounts_are_read_from_mountinfo() {
        let text = "\
29 1 0:24 / / rw,relatime shared:1 - zfs rpool/ROOT/ubuntu rw,xattr,posixacl
30 29 0:25 / /home rw,relatime shared:2 - zfs rpool/home rw,xattr
31 30 0:26 / /home/my\\040files rw - zfs rpool/home/files rw
32 30 0:27 / /home/.zfs/snapshot/daily rw - zfs rpool/home@daily ro
40 29 8:1 / /boot rw - ext4 /dev/sda1 rw";
        let v = parse_mountinfo(text);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], Dataset { name: "rpool/ROOT/ubuntu".into(), mountpoint: "/".into() });
        assert_eq!(v[2].mountpoint, Path::new("/home/my files"));
    }

    #[test]
    fn zfs_facts_are_parsed_from_zfs_get() {
        // Recorded on FreeBSD 14.5: `zfs get -H -p -o property,value … tank/home/demo`.
        let text = "used\t37888\navailable\t524250112\nreferenced\t25600\ncompressratio\t1.08\ncompression\tlz4\nquota\t524288000\nrefquota\t0\n";
        let ds = Dataset { name: "tank/home/demo".into(), mountpoint: "/tank/home/demo".into() };
        let f = parse_props(&ds, text);
        assert_eq!((f.used, f.available, f.referenced, f.quota, f.refquota), (37888, 524250112, 25600, 524288000, 0));
        assert_eq!((f.compression.as_str(), f.compressratio.as_str()), ("lz4", "1.08"));
        // Without -p the ratio has its x.
        assert_eq!(parse_props(&ds, "compressratio\t1.52x").compressratio, "1.52");
    }

    #[test]
    fn zfs_snapshots_are_parsed_newest_first() {
        // Recorded on FreeBSD 14.5, with a child dataset's snapshot that is not this one's.
        let text = "tank/home/demo@first\t1791245434\t12288\t25600\ntank/home/demo@second\t1791245435\t0\t25600\ntank/home/demo/sub@x\t1791245436\t0\t1\ntank/home/demo@same\t1791245435\t0\t1\n";
        let v = parse_snapshots("tank/home/demo", text);
        let names: Vec<&str> = v.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["same", "second", "first"]);
        assert_eq!(v[2], Snapshot { name: "first".into(), created: 1791245434, used: 12288, referenced: 25600 });
        assert!(parse_snapshots("tank/home/demo", "garbage\n").is_empty());
    }

    #[test]
    fn zfs_snapshot_paths() {
        let p = Path::new("/tank/home/demo/.zfs/snapshot/first/docs/note.md");
        let at = in_snapshot(p).unwrap();
        assert_eq!(at.mountpoint, Path::new("/tank/home/demo"));
        assert_eq!(at.name, "first");
        assert_eq!(at.live(), Path::new("/tank/home/demo/docs/note.md"));
        assert_eq!(in_snapshot(Path::new("/z/.zfs/snapshot/s")).unwrap().live(), Path::new("/z"));
        assert!(in_snapshot(Path::new("/z/.zfs/snapshot")).is_none());
        assert!(in_snapshot(Path::new("/z/zfs/snapshot/s")).is_none());
        assert_eq!(snapshot_path(Path::new("/tank/home/demo"), "first", Path::new("/tank/home/demo/docs")), Path::new("/tank/home/demo/.zfs/snapshot/first/docs"));
        assert_eq!(snapshot_path(Path::new("/"), "s", Path::new("/")), Path::new("/.zfs/snapshot/s"));
        assert!(is_read_only(p));
    }

    #[test]
    fn zfs_list_path_is_virtual_and_read_only() {
        let d = std::env::temp_dir().join(format!("coxswain-zfs-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(split(&path(&d)), Some(d.clone()));
        assert!(is_read_only(&path(&d)));
        assert!(!is_read_only(&d.join("x")));
        // A real folder of that name is a folder.
        std::fs::create_dir(path(&d)).unwrap();
        assert_eq!(split(&path(&d)), None);
        // Nothing is written into a snapshot or its list.
        let e = crate::fs::mkdir(&d.join(".zfs/snapshot/s/new")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert!(!d.join(".zfs").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }
}

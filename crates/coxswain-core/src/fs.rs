//! Directory listing, sorting and file operations.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Entry {
    pub name: String,
    /// Left out when empty: the desktop app's listing sends a path only where the page cannot
    /// join it from the folder and the name.
    #[serde(skip_serializing_if = "crate::fs::no_path")]
    pub path: PathBuf,
    pub is_dir: bool,
    // Left out when false: most entries are none of these, and a big folder's JSON is smaller.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_symlink: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_exec: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: u64,
    /// Seconds since the Unix epoch; 0 where the file system does not record it.
    pub created: u64,
    /// Only in the cloud (OneDrive, Dropbox, iCloud …): reading it would download it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub online: bool,
    /// A ZFS snapshot in a list of snapshots: the data it refers to (its size is its own space).
    #[serde(skip_serializing_if = "is_zero")]
    pub referenced: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn no_path(p: &Path) -> bool {
    p.as_os_str().is_empty()
}

/// At every start: the cache folder (name index, search store, previews, archive copies) and
/// the state folder (notes, tags, favourites) made readable by the user alone, with what is in
/// them, so installs from before this keep nothing open to other users either.
pub fn lock_down() {
    let state = crate::state::AppState::path().and_then(|p| p.parent().map(Path::to_path_buf));
    for dir in [crate::helper::folder(), state].into_iter().flatten() {
        let _ = lock_down_in(&dir);
    }
}

/// `lock_down` for one folder: it and the folders in it 0700, the files in it 0600.
fn lock_down_in(dir: &Path) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    private(dir, None)?;
    for e in fs::read_dir(dir)?.flatten() {
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            private(&e.path(), None)?;
        } else if t.is_file() {
            private(dir, Some(&e.path()))?;
        }
    }
    Ok(())
}

/// Make `dir` (created if missing) readable by the user alone, and `file` in it too: what
/// Coxswain keeps about your files (names, notes, tags, text) is yours.
pub fn private(dir: &Path, file: Option<&Path>) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        if let Some(f) = file.filter(|f| f.exists()) {
            fs::set_permissions(f, fs::Permissions::from_mode(0o600))?;
        }
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

/// `t` in seconds since 1970; `None` before then, or when the system keeps no such time.
pub fn secs(t: io::Result<std::time::SystemTime>) -> Option<u64> {
    Some(t.ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

/// The moment `secs` seconds after 1970, as far as the year 9999. An archive, an index or a
/// commit may say any number, and past that adding it to 1970 or showing it as a date panics.
pub fn epoch(secs: u64) -> std::time::SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs.min(253_402_300_799))
}

/// Now, in seconds since 1970.
pub fn now() -> u64 {
    secs(Ok(std::time::SystemTime::now())).unwrap_or(0)
}

/// `bytes` as the new content of `path`, through a file beside it, so a failed write never
/// leaves a half-written `path` (and the file beside it is removed); with `own`, readable by the
/// user alone, as `private` makes it.
pub fn replace(path: &Path, bytes: &[u8], own: bool) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    // Its own name, so two apps saving at once (the terminal and the desktop app) never write
    // into one file beside it.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(match path.extension() {
        Some(e) => format!("{}.{}-{n}.tmp", e.to_string_lossy(), std::process::id()),
        None => format!("{}-{n}.tmp", std::process::id()),
    });
    let done = (|| {
        if own {
            private(dir, None)?;
        } else {
            fs::create_dir_all(dir)?;
        }
        // On the disk before it takes the name: a crash never leaves an empty file there.
        let mut f = fs::File::create(&tmp)?;
        io::Write::write_all(&mut f, bytes)?;
        f.sync_all()?;
        drop(f);
        if own {
            private(dir, Some(&tmp))?;
        }
        fs::rename(&tmp, path)
    })();
    if done.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    done
}

impl Entry {
    pub fn is_parent(&self) -> bool {
        self.name == ".."
    }

    pub fn ext(&self) -> &str {
        match self.name.rfind('.') {
            Some(i) if i > 0 && !self.is_dir => &self.name[i + 1..],
            _ => "",
        }
    }

    /// `lmeta`: the entry's own metadata, from the folder's listing (on Windows that opens
    /// nothing, so a file only in the cloud is not downloaded by looking at it).
    fn from_path(
        path: PathBuf,
        name: String,
        lmeta: io::Result<fs::Metadata>,
    ) -> io::Result<Entry> {
        let lmeta = lmeta?;
        let is_symlink = lmeta.file_type().is_symlink();
        // Follow links for type and size; a dangling link stays a plain entry, and never a
        // program: its own mode (0777) says nothing about what it would run.
        let (meta, dangling) = match is_symlink.then(|| fs::metadata(&path)) {
            Some(Ok(m)) => (m, false),
            Some(Err(_)) => (lmeta, true),
            None => (lmeta, false),
        };
        Ok(Entry {
            hidden: name.starts_with('.') || is_hidden_attr(&meta),
            is_exec: !dangling && !meta.is_dir() && is_exec(&meta, &name),
            is_dir: meta.is_dir(),
            is_symlink,
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified: secs(meta.modified()).unwrap_or(0),
            created: secs(meta.created()).unwrap_or(0),
            online: !meta.is_dir() && crate::cloud::online_meta(&meta, &path),
            referenced: 0,
            name,
            path,
        })
    }
}

#[cfg(unix)]
fn is_exec(meta: &fs::Metadata, _name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(windows)]
fn is_exec(_meta: &fs::Metadata, name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [".exe", ".bat", ".cmd", ".ps1", ".com"]
        .iter()
        .any(|e| n.ends_with(e))
}

#[cfg(windows)]
fn is_hidden_attr(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x2 != 0
}

#[cfg(not(windows))]
fn is_hidden_attr(_meta: &fs::Metadata) -> bool {
    false
}

/// List a directory, or a folder inside an archive (`…/tools.zip/bin`). The first entry is
/// `..` unless `dir` is a root.
pub fn list(dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
    list_with_archive(dir, show_hidden).map(|(entries, _)| entries)
}

/// The archive a folder is inside, and whether anything in the folder is locked.
pub type InArchive = (PathBuf, bool);

/// `list`, and when `dir` is inside an archive, which and whether anything there is locked.
pub fn list_with_archive(
    dir: &Path,
    show_hidden: bool,
) -> io::Result<(Vec<Entry>, Option<InArchive>)> {
    if !dir.is_dir()
        && let Some(at) = crate::history::split(dir)
    {
        let mut all = crate::history::list(dir, &at)?;
        all.retain(|e| show_hidden || !e.hidden || e.is_parent());
        return Ok((all, None));
    }
    // A dataset's snapshots, and the files of a package: lists that lead to folders and files
    // on disk.
    if let Some(of) = crate::zfs::split(dir) {
        return Ok((crate::zfs::list(&of)?, None));
    }
    if let Some(file) = crate::bsd::split(dir) {
        let mut all = crate::bsd::list(&file)?;
        all.retain(|e| show_hidden || !e.hidden || e.is_parent());
        return Ok((all, None));
    }
    if !dir.is_dir()
        && let Some((archive, inner)) = crate::archive::split(dir)
    {
        let (mut all, locked) = crate::archive::listing(&archive, &inner)?;
        all.retain(|e| show_hidden || !e.hidden || e.is_parent());
        return Ok((all, Some((archive, locked))));
    }
    let mut out = Vec::new();
    // At the top of a snapshot, `..` leads back to the list of snapshots.
    let up = crate::zfs::in_snapshot(dir)
        .filter(|s| s.rel.as_os_str().is_empty())
        .map(|s| crate::zfs::path(&s.mountpoint));
    if let Some(parent) = up.or_else(|| dir.parent().map(Path::to_path_buf)) {
        out.push(Entry {
            name: "..".into(),
            path: parent,
            is_dir: true,
            ..Entry::default()
        });
    }
    for de in fs::read_dir(dir)? {
        let de = de?;
        let name = de.file_name().to_string_lossy().into_owned();
        // Entries can vanish between readdir and stat; skip them.
        if let Ok(e) = Entry::from_path(de.path(), name, de.metadata())
            && (show_hidden || !e.hidden)
        {
            out.push(e);
        }
    }
    Ok((out, None))
}

/// What a folder holds, for the preview of a folder: its folders, its files, their bytes and
/// the newest date (seconds since the Unix epoch), without `..`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Summary {
    pub folders: usize,
    pub files: usize,
    pub bytes: u64,
    pub newest: u64,
}

/// `dir`'s `Summary`, from the same listing the panel has (an archive's, a history's too).
pub fn summary(dir: &Path, show_hidden: bool) -> io::Result<Summary> {
    let mut s = Summary::default();
    for e in list_with_archive(dir, show_hidden)?.0 {
        if e.is_parent() {
            continue;
        }
        if e.is_dir {
            s.folders += 1;
        } else {
            s.files += 1;
            s.bytes += e.size;
        }
        s.newest = s.newest.max(e.modified);
    }
    Ok(s)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Name,
    Ext,
    Time,
    Size,
    /// By the last commit (`history::sort_by_last`); by name until git has said.
    Commit,
}

/// `..` first, then directories, then files; each group ordered by `key`.
/// `sort` for the listing of `dir`. A history's list of commits keeps git's order, newest first
/// with every commit below those made on top of it, as its graph is drawn: its names start with
/// commit ids, which would put it in a random order. The branches and the worktrees keep git's
/// order too: local branches before remote ones. A dataset's snapshots go by date, newest first.
/// By size a folder goes by its measured size (`measured`; inside an archive its listed one), and comes after the others, by name, while that is not known.
pub fn sort_in(
    dir: &Path,
    entries: &mut [Entry],
    key: SortKey,
    reverse: bool,
    measured: &std::collections::HashMap<PathBuf, u64>,
) {
    let name = |p: &Path| p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
    let commits = name(dir) == crate::history::MARKER
        || name(dir) == crate::history::ALL && dir.parent().is_some_and(|p| name(p) == crate::history::MARKER);
    if commits
        || crate::history::split(dir)
            .is_some_and(|at| at.commit.is_none() && at.view != crate::history::View::History)
    {
        entries.sort_by_key(|e| !e.is_parent());
    } else if dir.file_name().is_some_and(|n| n == crate::zfs::MARKER) {
        entries.sort_by(|a, b| {
            let ord = b.modified.cmp(&a.modified);
            (!a.is_parent())
                .cmp(&!b.is_parent())
                .then(if reverse { ord.reverse() } else { ord })
        });
    } else {
        let listed = crate::archive::split(dir).is_some();
        sort_sized(entries, key, reverse, |e| {
            if !e.is_dir || listed {
                Some(e.size)
            } else {
                measured.get(&e.path).copied()
            }
        });
    }
}

/// `sort` with no folder measured: by size, folders in name order. Names in the language's
/// alphabet (`collate`), numbers by their value: "file2" < "file10".
pub fn sort(entries: &mut [Entry], key: SortKey, reverse: bool) {
    sort_sized(entries, key, reverse, |e| (!e.is_dir).then_some(e.size));
}

/// `size` is what a sort by size goes by; `None` (a folder not measured) after the rest.
fn sort_sized(
    entries: &mut [Entry],
    key: SortKey,
    reverse: bool,
    size: impl Fn(&Entry) -> Option<u64>,
) {
    sort_keyed(entries, key == SortKey::Ext, |a, ka, b, kb| {
        let group = |e: &Entry| (!e.is_parent(), !e.is_dir);
        group(a).cmp(&group(b)).then_with(|| {
            let name = || ka.name.cmp(&kb.name);
            let sizes = (key == SortKey::Size).then(|| (size(a), size(b)));
            if let Some((x, y)) = sizes
                && (x.is_none() || y.is_none())
            {
                // Not measured yet, or folder sizes off: last, by name, whichever way round.
                return x.is_none().cmp(&y.is_none()).then_with(name);
            }
            let ord = match key {
                SortKey::Name | SortKey::Commit => name(),
                SortKey::Ext => ka.ext.cmp(&kb.ext).then_with(name),
                SortKey::Time => b.modified.cmp(&a.modified).then_with(name),
                SortKey::Size => {
                    let (x, y) = sizes.unwrap_or_default();
                    y.cmp(&x).then_with(name)
                }
            };
            if reverse { ord.reverse() } else { ord }
        })
    });
}

/// An entry's collation keys (`collate::key`): its name's, and its extension's when asked.
pub(crate) struct Keys {
    pub name: Vec<u8>,
    pub ext: Vec<u8>,
}

/// `entries` sorted by `cmp`, which gets each entry with its `Keys`, made once per entry (on
/// every core): a sort of 200,000 names compares them some four million times.
pub(crate) fn sort_keyed(
    entries: &mut [Entry],
    ext: bool,
    cmp: impl Fn(&Entry, &Keys, &Entry, &Keys) -> std::cmp::Ordering,
) {
    use rayon::prelude::*;
    let keys: Vec<Keys> = entries
        .par_iter()
        .map(|e| Keys {
            name: crate::collate::key(&e.name),
            ext: if ext { crate::collate::key(e.ext()) } else { Vec::new() },
        })
        .collect();
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|&i, &j| cmp(&entries[i], &keys[i], &entries[j], &keys[j]));
    let mut taken: Vec<Entry> = entries.iter_mut().map(std::mem::take).collect();
    for (slot, i) in entries.iter_mut().zip(order) {
        *slot = std::mem::take(&mut taken[i]);
    }
}

/// Where `src` lands when copied or moved to `dst`: inside it if `dst` is a directory (and not
/// `src` itself, spelt in another case).
pub fn target(src: &Path, dst: &Path) -> PathBuf {
    match (dst.is_dir() && !same_file(src, dst), src.file_name()) {
        (true, Some(name)) => dst.join(name),
        _ => dst.to_path_buf(),
    }
}

/// Whether `a` and `b` are one file: the same path in another case on a file system that
/// does not mind the case, say.
pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (fs::symlink_metadata(a), fs::symlink_metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        // Canonical paths carry the case the disk has.
        matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
    }
}

/// Copy a file, symlink or directory tree. Returns the created path.
pub fn copy(src: &Path, dst: &Path) -> io::Result<PathBuf> {
    copy_locked(src, dst, None)
}

/// The archive and the folder inside it that `dst` names, when it is inside one.
fn into_archive(dst: &Path) -> Option<(PathBuf, String)> {
    if dst.is_dir() {
        None
    } else {
        crate::archive::split(dst)
    }
}

/// `src` inside an archive, when it is.
fn out_of_archive(src: &Path) -> Option<(PathBuf, String)> {
    if src.exists() {
        None
    } else {
        crate::archive::split(src)
    }
}

/// A folder of its own under the temp folder, removed when dropped.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> io::Result<Scratch> {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "coxswain-archive-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        // Made here, never one that is there: the temp folder is everyone's.
        fs::create_dir(&dir)?;
        private_dir(&dir)?;
        Ok(Scratch(dir))
    }

    /// A test's folder, named after the test, removed with it.
    #[cfg(test)]
    pub(crate) fn named(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("coxswain-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

/// A folder made for this user alone: the temp folder is everyone's.
pub(crate) fn private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    Ok(())
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A history is read-only: nothing goes into it, nothing is made or taken out there. So are a
/// ZFS snapshot, the lists of snapshots and of a package's files, and a disk image.
fn writable(path: &Path) -> io::Result<()> {
    let denied = |key: &str| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            crate::t!(key),
        ))
    };
    if crate::zfs::is_read_only(path) {
        return denied("zfs.read_only");
    }
    if path.ancestors().any(|a| crate::bsd::split(a).is_some()) {
        return denied("pkg.read_only");
    }
    // Inside a disk image nothing is made, changed or taken out (the image itself is a file).
    if !path.exists()
        && crate::archive::split(path)
            .is_some_and(|(image, inner)| !inner.is_empty() && crate::archive::read_only(&image))
    {
        return denied("disk_image.read_only");
    }
    // Inside an archive inside an archive, which is looked into from a copy in the cache.
    if !path.exists()
        && crate::archive::split(path)
            .is_some_and(|(a, inner)| crate::archive::nested(&a, &inner, false))
    {
        return denied("archive.nested_read_only");
    }
    match crate::history::split(path) {
        Some(_) if !path.exists() => denied("history.read_only"),
        _ => Ok(()),
    }
}

/// `copy`, with the password of the archive `src` is inside, when it is locked. Into an
/// archive it is added; from one archive to another it goes through a folder of its own. Out
/// of a history it is the file or folder as it was at that commit.
pub fn copy_locked(src: &Path, dst: &Path, password: Option<&str>) -> io::Result<PathBuf> {
    copy_out_or_in(src, dst, password, false)
}

/// `copy_locked`; `whole`: out of an archive everything comes, or nothing (for a move).
fn copy_out_or_in(
    src: &Path,
    dst: &Path,
    password: Option<&str>,
    whole: bool,
) -> io::Result<PathBuf> {
    writable(dst)?;
    if !src.exists()
        && let Some(at) = crate::history::split(src)
    {
        return crate::history::copy_out(&at, dst);
    }
    if let Some((archive, inner)) = into_archive(dst) {
        if crate::archive::read_only(&archive) {
            return Err(crate::disk_image::read_only());
        }
        let name = src
            .file_name()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    crate::t!("err.nothing_to_copy"),
                )
            })?
            .to_string_lossy();
        // Into a folder of the archive under its own name, or under the name `dst` gives.
        let (to, inner) = if crate::archive::is_folder(&archive, &inner)? {
            (
                dst.join(&*name),
                if inner.is_empty() {
                    name.into_owned()
                } else {
                    format!("{inner}/{name}")
                },
            )
        } else {
            (dst.to_path_buf(), inner)
        };
        let scratch;
        let from = match out_of_archive(src) {
            Some((a, i)) => {
                scratch = Scratch::new()?;
                crate::archive::copy_out(&a, &i, &scratch.0, password, whole)?
            }
            None => src.to_path_buf(),
        };
        crate::archive::add(&archive, &[(inner, from)], password)?;
        return Ok(to);
    }
    if let Some((archive, inner)) = out_of_archive(src) {
        return crate::archive::copy_out(&archive, &inner, dst, password, whole);
    }
    let to = target(src, dst);
    if src.is_dir() && inside(&to, src) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::t!("err.copy_into_itself"),
        ));
    }
    copy_tree(src, &to)?;
    Ok(to)
}

/// Whether `to` lies in `dir`, also when one of them is reached through a link (`/home` is a
/// link to `/usr/home` on FreeBSD). The last parts are not followed: `to` need not exist yet.
fn inside(to: &Path, dir: &Path) -> bool {
    let real = |p: &Path| match (p.parent().map(fs::canonicalize), p.file_name()) {
        (Some(Ok(parent)), Some(name)) => parent.join(name),
        _ => p.to_path_buf(),
    };
    real(to).starts_with(real(dir))
}

/// `src` to `to`. A folder that is not there yet is made under a passing name beside it and
/// gets its name when complete, so a copy that fails half way leaves nothing and can be tried
/// again; one that is there is merged into, file by file. Stopped (`stop`), the folder gets its
/// name with what was copied so far: the file it was on is removed, the rest stays.
fn copy_tree(src: &Path, to: &Path) -> io::Result<()> {
    if !fs::symlink_metadata(src)?.is_dir() || fs::symlink_metadata(to).is_ok() {
        return copy_entry(src, to);
    }
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    // Made here, never one that is there (nor a link to one): the destination may be shared.
    let mut tries = 0;
    let tmp = loop {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let tmp = to.with_file_name(format!(
            ".{}.coxswain-tmp-{}-{}-{nanos:x}",
            to.file_name().unwrap_or_default().to_string_lossy(),
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        match fs::create_dir(&tmp) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && tries < 8 => tries += 1,
            r => break r.map(|()| tmp)?,
        }
    };
    let r = (|| {
        for de in fs::read_dir(src)? {
            let de = de?;
            copy_entry(&de.path(), &tmp.join(de.file_name()))?;
        }
        // Never onto one that came meanwhile: rename would take an empty folder's place.
        if fs::symlink_metadata(to).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                crate::t!("err.exists", "path" => to.display()),
            ));
        }
        fs::rename(&tmp, to)
    })();
    if r.is_err() {
        // Never onto one that came meanwhile, as above.
        if !(crate::stop::asked() && fs::symlink_metadata(to).is_err() && fs::rename(&tmp, to).is_ok()) {
            let _ = fs::remove_dir_all(&tmp);
        }
    }
    r
}

fn copy_entry(src: &Path, to: &Path) -> io::Result<()> {
    crate::stop::check()?;
    let ft = fs::symlink_metadata(src)?.file_type();
    if ft.is_symlink() {
        let link = fs::read_link(src)?;
        #[cfg(unix)]
        return std::os::unix::fs::symlink(link, to);
        #[cfg(windows)]
        return if src.is_dir() {
            std::os::windows::fs::symlink_dir(link, to)
        } else {
            std::os::windows::fs::symlink_file(link, to)
        };
    }
    if ft.is_dir() {
        fs::create_dir_all(to)?;
        for de in fs::read_dir(src)? {
            let de = de?;
            copy_entry(&de.path(), &to.join(de.file_name()))?;
        }
        Ok(())
    } else if !ft.is_file() {
        // A pipe would wait for a writer for ever; a device would be read without end.
        Err(special(src))
    } else if fs::symlink_metadata(to).is_ok() {
        // Never overwrite: copying a file onto itself would truncate it.
        // ponytail: no overwrite prompt yet; delete the target first.
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            crate::t!("err.exists", "path" => to.display()),
        ))
    } else {
        copy_file(src, to, false)
    }
}

/// Files up to this size are copied by the system in one go (a clone where the disk can), in
/// well under a second; larger ones in steps, with Stop looked at between them.
const STEP: u64 = 8 << 20;

/// The file `src` to `to`, which is not there (`copy_entry` looked). `sync`: written through to
/// the disk before it returns, for a move that deletes the original next. Not there before: a
/// copy that fails half way (a full disk) or is stopped leaves nothing, so trying again is not
/// refused as "exists".
fn copy_file(src: &Path, to: &Path, sync: bool) -> io::Result<()> {
    let meta = fs::metadata(src)?;
    // Windows' own copy is written to the disk later, by its cache: a move takes the steps below,
    // which are.
    if meta.len() <= STEP && !(sync && cfg!(windows)) {
        let r = fs::copy(src, to).and_then(|_| {
            #[cfg(unix)]
            if sync {
                fs::File::open(to)?.sync_all()?;
            }
            Ok(())
        });
        if r.is_err() {
            let _ = fs::remove_file(to);
        }
        r?;
    } else {
        copy_in_steps(src, to, &meta, sync)?;
    }
    // The original's date (Windows and macOS copy it themselves). As the owner, a file
    // open for reading may be dated, a read-only one too.
    #[cfg(unix)]
    let _ = meta.modified().and_then(|t| fs::File::open(to)?.set_modified(t));
    Ok(())
}

/// A large file to `to`, in steps of a megabyte with Stop looked at between them, under a
/// passing name beside it (`.name.coxswain-tmp-…`) until it is whole: a copy cut short, even
/// by a crash, never stands under the real name.
// ponytail: the system's clone, and on macOS and Windows a file's extended attributes and
// streams, are given up for Stop; copyfile(3) and CopyFileExW with their progress callbacks
// would keep them.
fn copy_in_steps(src: &Path, to: &Path, meta: &fs::Metadata, sync: bool) -> io::Result<()> {
    use std::io::{Read, Seek, Write};
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let tmp = to.with_file_name(format!(
        ".{}.coxswain-tmp-{}-{}",
        to.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut from = fs::File::open(src)?;
    // For this user alone while it is written: a private file's half copy is never readable
    // by others. Its own mode once it is whole, below.
    let mut new = fs::OpenOptions::new();
    new.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut new, 0o600);
    let mut out = new.open(&tmp)?;
    let r = (|| {
        let mut buf = vec![0u8; 1 << 20];
        loop {
            crate::stop::check()?;
            let n = match from.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            // A run of zeros is skipped, not written: a sparse file stays sparse.
            if buf[..n].iter().all(|&b| b == 0) {
                out.seek(io::SeekFrom::Current(n as i64))?;
            } else {
                out.write_all(&buf[..n])?;
            }
        }
        let end = out.stream_position()?;
        out.set_len(end)?;
        if sync {
            out.sync_all()?;
        }
        out.set_modified(meta.modified()?)?;
        // The original's permission bits, not its set-user or set-group ones; on Windows only
        // its read-only flag, not its other attributes (compressed, a cloud placeholder's).
        #[cfg(unix)]
        out.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(
            std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o777,
        ))?;
        #[cfg(not(unix))]
        {
            let mut p = out.metadata()?.permissions();
            p.set_readonly(meta.permissions().readonly());
            out.set_permissions(p)?;
        }
        drop(out);
        // Never onto one that came meanwhile: rename would replace it.
        if fs::symlink_metadata(to).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                crate::t!("err.exists", "path" => to.display()),
            ));
        }
        fs::rename(&tmp, to)
    })();
    if r.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    r
}

/// What a move across disks did when it ended half way (stopped, or failed): the files and
/// folders that went over whole, and the folders it made for them that are not empty.
#[derive(Default, Debug)]
pub(crate) struct Moved {
    pub(crate) done: Vec<(PathBuf, PathBuf)>,
    pub(crate) made: Vec<PathBuf>,
}

/// Move `src` to `to` on another disk, file by file: each copied, checked (written to the disk,
/// as long as the original) and only then its original deleted, so every file is in one
/// place at any time. A folder is made under its own name and its original removed once empty.
fn move_across(src: &Path, to: &Path, log: &mut Moved) -> io::Result<()> {
    crate::stop::check()?;
    let meta = fs::symlink_metadata(src)?;
    if !meta.is_dir() {
        move_file(src, to, &meta)?;
        log.done.push((src.to_path_buf(), to.to_path_buf()));
        return Ok(());
    }
    fs::create_dir(to)?;
    log.made.push(to.to_path_buf());
    let mark = log.done.len();
    let inside: Vec<_> = fs::read_dir(src)?.collect::<io::Result<_>>()?;
    for de in inside {
        move_across(&de.path(), &to.join(de.file_name()), log)?;
    }
    // Its own permissions once the files are in: a read-only folder would have kept them out.
    let _ = fs::set_permissions(to, meta.permissions());
    fs::remove_dir(src)?;
    // Gone over whole: one item, the folder, instead of what is in it.
    log.done.truncate(mark);
    log.made.retain(|m| !m.starts_with(to));
    log.done.push((src.to_path_buf(), to.to_path_buf()));
    Ok(())
}

/// One file (or link) of `move_across`. When anything fails, the original stays and the copy
/// goes.
fn move_file(src: &Path, to: &Path, before: &fs::Metadata) -> io::Result<()> {
    if before.is_file() {
        copy_file(src, to, true)?;
    } else {
        copy_entry(src, to)?;
    }
    let checked = (|| {
        let (now, copy) = (fs::symlink_metadata(src)?, fs::symlink_metadata(to)?);
        // Changed while it was copied, or the copy is short: the original stays.
        if before.is_file()
            && (copy.len() != before.len()
                || now.len() != before.len()
                || now.modified().ok() != before.modified().ok())
        {
            return Err(io::Error::other(
                crate::t!("err.move_check", "path" => src.display()),
            ));
        }
        fs::remove_file(src)
    })();
    if checked.is_err() {
        let _ = fs::remove_file(to);
    }
    checked
}

/// The error for a pipe, a socket or a device, which are not copied or moved.
fn special(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        crate::t!("err.special_file", "path" => path.display()),
    )
}

/// Refused when anything in `src` cannot go into an archive (a link, a pipe, a device): an
/// archive leaves those out, and a move would then delete what it never packed.
fn packable(src: &Path) -> io::Result<()> {
    let ft = fs::symlink_metadata(src)?.file_type();
    if ft.is_dir() {
        fs::read_dir(src)?.try_for_each(|de| packable(&de?.path()))
    } else if ft.is_file() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            crate::t!("err.not_packable", "path" => src.display()),
        ))
    }
}

/// Move or rename. Falls back to copy + delete across filesystems.
pub fn rename(src: &Path, dst: &Path) -> io::Result<PathBuf> {
    rename_locked(src, dst, None)
}

/// `rename`, with the password of the archive `src` is inside: out of an archive it is copied
/// out, then taken out of the archive.
pub fn rename_locked(src: &Path, dst: &Path, password: Option<&str>) -> io::Result<PathBuf> {
    rename_locked_logged(src, dst, password, &mut Moved::default())
}

/// `rename_locked`; `log`: what a move across disks that ended half way left, for undo.
pub(crate) fn rename_locked_logged(
    src: &Path,
    dst: &Path,
    password: Option<&str>,
    log: &mut Moved,
) -> io::Result<PathBuf> {
    writable(src)?;
    writable(dst)?;
    match (out_of_archive(src), into_archive(dst)) {
        // Within one archive: renamed there, the folder `dst` names, or a new name.
        (Some((a, from)), Some((b, to))) if a == b => {
            let name = src.file_name().unwrap_or_default().to_string_lossy();
            let to = if crate::archive::is_folder(&a, &to)? {
                if to.is_empty() {
                    name.to_string()
                } else {
                    format!("{to}/{name}")
                }
            } else {
                to
            };
            crate::archive::rename_in(&a, &from, &to, password)?;
            Ok(a.join(to))
        }
        (Some((a, from)), _) => {
            // Taken out of the archive only when all of it came out.
            let to = copy_out_or_in(src, dst, password, true)?;
            // Once out whole it is taken out of the archive, Stop or not: never in two places.
            crate::stop::during(&Default::default(), || {
                crate::archive::remove(&a, &[from], password)
            })?;
            Ok(to)
        }
        (None, Some(_)) => {
            packable(src)?;
            let to = copy_locked(src, dst, password)?;
            delete(src)?;
            Ok(to)
        }
        (None, None) => rename_logged(src, dst, log),
    }
}

#[cfg(test)]
thread_local! {
    /// A test's move goes as across disks.
    pub(crate) static ACROSS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A move on disk: a rename, or across disks file by file; `log`: what a move across disks that
/// ended half way left, for undo.
fn rename_logged(src: &Path, dst: &Path, log: &mut Moved) -> io::Result<PathBuf> {
    let to = target(src, dst);
    if src.is_dir() && inside(&to, src) && !same_file(src, &to) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::t!("err.move_into_itself"),
        ));
    }
    // `to` may be `src` itself in another case (`Notes` to `notes` where case does not count).
    // A link counts as there, even one to nothing: rename would replace it.
    if fs::symlink_metadata(&to).is_ok() && !same_file(src, &to) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            crate::t!("err.exists", "path" => to.display()),
        ));
    }
    #[cfg(test)]
    let moved = if ACROSS.get() {
        Err(io::ErrorKind::CrossesDevices.into())
    } else {
        fs::rename(src, &to)
    };
    #[cfg(not(test))]
    let moved = fs::rename(src, &to);
    match moved {
        // Another disk: file by file. Other failures are failures.
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            if let Err(e) = move_across(src, &to, log) {
                // The folders made that nothing went into go again, the innermost first.
                for m in log.made.iter().rev() {
                    let _ = fs::remove_dir(m);
                }
                log.made.retain(|m| m.exists());
                return Err(e);
            }
        }
        r => r?,
    }
    Ok(to)
}

/// Delete a file, symlink (not its target) or directory tree; inside an archive, take it out of
/// the archive.
pub fn delete(path: &Path) -> io::Result<()> {
    delete_locked(path, None)
}

/// `delete`, with the password of the locked 7z `path` is inside.
pub fn delete_locked(path: &Path, password: Option<&str>) -> io::Result<()> {
    writable(path)?;
    if !path.exists()
        && let Some((archive, inner)) = crate::archive::split(path)
    {
        return crate::archive::remove(&archive, &[inner], password);
    }
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Move to the desktop's trash (Recycle Bin on Windows). Inside an archive there is no trash:
/// it is taken out of the archive, as `delete` does.
pub fn trash(path: &Path) -> io::Result<()> {
    trash_locked(path, None)
}

/// `trash`, with the password of the locked 7z `path` is inside.
pub fn trash_locked(path: &Path, password: Option<&str>) -> io::Result<()> {
    trash_named(path, password).map(drop)
}

/// `trash_locked`; in Termux's own trash, the name it has there (for undo to put it back).
pub(crate) fn trash_named(path: &Path, password: Option<&str>) -> io::Result<Option<String>> {
    writable(path)?;
    if !path.exists() && crate::archive::split(path).is_some() {
        return delete_locked(path, password).map(|_| None);
    }
    // A Flatpak's XDG_DATA_HOME is its own folder, so the trash crate would fill a trash the
    // desktop never shows: the host's gio puts it in the desktop's.
    if crate::tools::flatpak().is_some() {
        let out = crate::tools::user_command("gio")
            .arg("trash")
            .arg(path)
            .stdin(std::process::Stdio::null())
            .output()?;
        return if out.status.success() {
            Ok(None)
        } else {
            Err(io::Error::other(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        };
    }
    #[cfg(not(any(target_os = "android", target_os = "illumos", target_os = "solaris")))]
    return trash::delete(path).map(|_| None).map_err(io::Error::other);
    #[cfg(any(target_os = "android", target_os = "illumos", target_os = "solaris"))]
    crate::xdg_trash::put(path).map(Some)
}

/// Whether `trash` can take every one of `paths`. Only Coxswain's own trash (Termux, illumos)
/// cannot: it takes nothing from another filesystem, such as the phone's storage or another
/// ZFS dataset.
pub fn trash_takes(paths: &[PathBuf]) -> bool {
    #[cfg(any(target_os = "android", target_os = "illumos", target_os = "solaris"))]
    return paths.iter().all(|p| crate::xdg_trash::takes(p));
    #[cfg(not(any(target_os = "android", target_os = "illumos", target_os = "solaris")))]
    {
        let _ = paths;
        true
    }
}

/// What deleting does, decided the same in both apps.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub struct DeletePlan {
    /// Deleted for good, not moved to the trash.
    pub forever: bool,
    /// Asked even when deleting is not confirmed: the trash cannot take these.
    pub must_confirm: bool,
    /// The locale key of the question (its `{what}`, and `{archive}` inside one).
    pub question: &'static str,
}

/// How `paths` are deleted: for good with `forever`; inside an archive always, as there is no
/// trash there (they are taken out, the archive written anew). Coxswain's own trash (Termux,
/// illumos) takes nothing from another filesystem: that is deleted for good, and always asked.
pub fn delete_plan(paths: &[PathBuf], forever: bool, inside_archive: bool) -> DeletePlan {
    let takes = forever || inside_archive || trash_takes(paths);
    plan_delete(forever, inside_archive, takes, crate::termux::active())
}

fn plan_delete(forever: bool, inside: bool, takes: bool, termux: bool) -> DeletePlan {
    let no_trash = !takes;
    DeletePlan {
        forever: forever || inside || no_trash,
        must_confirm: no_trash,
        question: match () {
            _ if inside => "confirm.archive_remove",
            _ if no_trash && termux => "termux.no_trash_storage",
            _ if no_trash => "confirm.no_trash_here",
            _ if forever => "confirm.delete_forever",
            _ => "confirm.trash",
        },
    }
}

/// A free name for `name` in `dir`: `name`, else `stem (2).ext`, `stem (3).ext`, ...
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    name_candidates(name)
        .map(|n| dir.join(n))
        .find(|p| fs::symlink_metadata(p).is_err())
        .expect("some name is free")
}

/// `name`, then `stem (2).ext`, `stem (3).ext`, ...
pub(crate) fn name_candidates(name: &str) -> impl Iterator<Item = String> + '_ {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    };
    std::iter::once(name.to_string()).chain((2..).map(move |n| format!("{stem} ({n}){ext}")))
}

pub fn mkdir(path: &Path) -> io::Result<()> {
    mkdir_locked(path, None)
}

/// `mkdir`, with the password of the locked 7z `path` is inside.
pub fn mkdir_locked(path: &Path, password: Option<&str>) -> io::Result<()> {
    writable(path)?;
    if let Some((archive, inner)) = into_archive(path) {
        return crate::archive::mkdir(&archive, &inner, password);
    }
    fs::create_dir_all(path)
}

// ---------------------------------------------------------------- macOS's protected folders

/// Other apps' data on a Mac, which no walk of Coxswain's opens or stats: each touch can raise
/// "Coxswain would like to access data from other apps", again after every update of an app
/// that is not signed with a Developer ID. That is `~/Library` but its cloud folders, the Data
/// volume's second view of the disk, the per-user temporary folders, the Trash, and the
/// libraries of Photos, Music and TV. On every system, ZFS's control folder `.zfs` too: with
/// `snapdir=visible` every snapshot of the dataset is below it, and a walk would read the whole
/// dataset once for each. A folder the user opens is listed all the same: this is for walks.
pub fn protected(path: &Path) -> bool {
    if path.file_name().is_some_and(|n| n == ".zfs") && path.join("snapshot").is_dir() {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        static HOME: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
        if let Some(home) = HOME.get_or_init(crate::dirs::home_dir) {
            return protected_in(home, path);
        }
    }
    let _ = path;
    false
}

/// `protected` for Space, which the user asked to see whole: `~/Library` is walked (its caches,
/// Developer and Application Support count), but not the folders whose every touch asks for
/// another app's permission: Containers, Group Containers and the data vaults of Mail, Messages,
/// Safari and the like. The rest of `protected` holds (the Photos libraries, the Trash, `.zfs`).
pub fn protected_for_space(path: &Path) -> bool {
    if path.file_name().is_some_and(|n| n == ".zfs") && path.join("snapshot").is_dir() {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        static HOME: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
        if let Some(home) = HOME.get_or_init(crate::dirs::home_dir) {
            return protected_for_space_in(home, path);
        }
    }
    let _ = path;
    false
}

/// `protected_for_space`, for the home folder `home`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn protected_for_space_in(home: &Path, path: &Path) -> bool {
    const VAULTS: [&str; 16] = [
        "Containers",
        "Group Containers",
        "Mail",
        "Messages",
        "Safari",
        "Calendars",
        "Reminders",
        "HomeKit",
        "Suggestions",
        "Cookies",
        "IdentityServices",
        "Biome",
        "Metadata/CoreSpotlight",
        "Application Support/AddressBook",
        "Application Support/CallHistoryDB",
        "Application Support/com.apple.TCC",
    ];
    match path.strip_prefix(home.join("Library")) {
        Ok(rest) => VAULTS.iter().any(|v| rest.starts_with(v)),
        Err(_) => protected_in(home, path),
    }
}

/// `protected`, for the home folder `home`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn protected_in(home: &Path, path: &Path) -> bool {
    // iCloud Drive and the cloud apps' folders: found by name as the cloud rules say.
    const KEEP: [&str; 2] = ["CloudStorage", "Mobile Documents"];
    const BUNDLES: [&str; 6] = [
        "photoslibrary",
        "photolibrary",
        "migratedphotolibrary",
        "aplibrary",
        "musiclibrary",
        "tvlibrary",
    ];
    if let Ok(rest) = path.strip_prefix(home.join("Library")) {
        return rest
            .components()
            .next()
            .is_some_and(|c| !KEEP.iter().any(|k| c.as_os_str() == *k));
    }
    path.starts_with("/System/Volumes")
        || path.starts_with("/private/var/folders")
        || path == home.join(".Trash")
        || path
            .extension()
            .is_some_and(|e| BUNDLES.iter().any(|b| e.eq_ignore_ascii_case(b)))
}

/// `fs::read_dir` for walks. On a Mac a folder refused once (Desktop, Documents, Downloads, a
/// removable or network volume the user did not allow) is not asked for again while this
/// process runs: one privacy prompt per folder, not one per walk.
pub fn read_dir(dir: &Path) -> io::Result<fs::ReadDir> {
    static REFUSED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::LazyLock::new(Default::default);
    if cfg!(target_os = "macos") {
        read_dir_once(&REFUSED, dir)
    } else {
        fs::read_dir(dir)
    }
}

fn read_dir_once(
    refused: &std::sync::Mutex<std::collections::HashSet<PathBuf>>,
    dir: &Path,
) -> io::Result<fs::ReadDir> {
    if refused
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(dir)
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let rd = fs::read_dir(dir);
    if rd
        .as_ref()
        .is_err_and(|e| e.kind() == io::ErrorKind::PermissionDenied)
    {
        refused
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(dir.to_path_buf());
    }
    rd
}

/// The space free to this user on the disk `dir` is on, when the system says.
pub fn free_space(dir: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        // SAFETY: statvfs fills the zeroed struct it is given.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
            return None;
        }
        #[allow(clippy::unnecessary_cast)] // the types differ between systems
        Some((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
        let mut free = 0u64;
        // SAFETY: the path ends with a NUL; the counts not wanted are null.
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(free)
    }
}

/// Total bytes and file count under `path`, in parallel. Symlinks are counted, not followed;
/// unreadable parts, and other apps' data on a Mac (`protected`) below it, are skipped.
pub fn dir_size(path: &Path) -> (u64, u64) {
    // Never stopped, so always measured.
    crate::sizes::walk(path, &std::sync::atomic::AtomicBool::new(false)).unwrap_or_default()
}

/// What Properties shows of a file or folder: the basics, and where the system has them, its
/// ZFS dataset, its package and its flags.
#[derive(Clone, Debug, Serialize)]
pub struct Props {
    pub path: PathBuf,
    /// "file", "folder" or "symlink"; the apps show it in their language.
    pub kind: &'static str,
    pub link_target: Option<PathBuf>,
    pub size: u64,
    pub files: u64,
    /// A folder that could not be listed: why, in plain words. Its size and count are then
    /// unknown, not 0.
    pub unreadable: Option<String>,
    pub created: Option<u64>,
    pub modified: Option<u64>,
    pub accessed: Option<u64>,
    pub readonly: bool,
    /// Unix permission bits, e.g. 0o644, and owner ids.
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    /// The owner's and group's names, as `ls -l` shows them: `user:group` (an id stays where it
    /// has no name).
    pub owner: Option<String>,
    /// The ZFS dataset it is on, and how many snapshots that has.
    pub zfs: Option<crate::zfs::Facts>,
    pub snapshots: Option<usize>,
    /// The FreeBSD package it belongs to.
    pub package: Option<String>,
    pub flags: Option<crate::flags::Flags>,
    /// A disk image's facts (`disk_image::facts`), as (label, value) lines, and what can be done
    /// with it next.
    pub disk_image: Option<Vec<(String, String)>>,
    pub disk_next: Option<crate::disk_image::Next>,
    /// What an old picture, a tracker module, a mailbox, a comic or a dBase table holds
    /// (`facts::of`), as (label, value) lines.
    pub facts: Option<Vec<(String, String)>>,
}

/// Properties of `path`: a folder's size is a walk of it, and the facts run `zfs` and `pkg`,
/// so not on a thread that draws.
pub fn properties(path: &Path) -> io::Result<Props> {
    let lmeta = fs::symlink_metadata(path)?;
    let meta = fs::metadata(path).unwrap_or_else(|_| lmeta.clone());
    let (size, files) = dir_size(path);
    let unreadable = meta
        .is_dir()
        .then(|| fs::read_dir(path).err())
        .flatten()
        .map(|e| plain_kind(e.kind()).unwrap_or_else(|| cause(&e.to_string())));
    #[cfg(unix)]
    let (mode, uid, gid) = {
        use std::os::unix::fs::MetadataExt;
        (
            Some(meta.mode() & 0o7777),
            Some(meta.uid()),
            Some(meta.gid()),
        )
    };
    #[cfg(not(unix))]
    let (mode, uid, gid) = (None, None, None);
    #[cfg(unix)]
    let owner = uid.zip(gid).map(|(u, g)| owner_names(u, g));
    #[cfg(not(unix))]
    let owner = None;
    let disk = (meta.is_file() && crate::disk_image::named(path))
        .then(|| crate::disk_image::facts(path))
        .flatten();
    let zfs = crate::zfs::facts(path);
    let snapshots = zfs
        .as_ref()
        .and_then(|f| crate::zfs::snapshots(&f.dataset).ok())
        .map(|v| v.len());
    Ok(Props {
        kind: if lmeta.is_symlink() {
            "symlink"
        } else if meta.is_dir() {
            "folder"
        } else {
            "file"
        },
        link_target: fs::read_link(path).ok(),
        size,
        files,
        unreadable,
        created: secs(meta.created()),
        modified: secs(meta.modified()),
        accessed: secs(meta.accessed()),
        readonly: meta.permissions().readonly(),
        mode,
        uid,
        gid,
        owner,
        zfs,
        snapshots,
        package: if lmeta.is_dir() {
            None
        } else {
            crate::bsd::package_of(path)
        },
        flags: crate::flags::read(path),
        facts: ((meta.is_file() || crate::neural::named(path)) && disk.is_none()).then(|| crate::facts::of(path)).flatten(),
        disk_image: disk.as_ref().map(crate::disk_image::lines),
        disk_next: disk.map(|_| crate::disk_image::next()),
        path: path.to_path_buf(),
    })
}

/// `user:group` for the ids, from the system's user and group databases; an id without a name
/// stays a number.
#[cfg(unix)]
fn owner_names(uid: u32, gid: u32) -> String {
    use std::ffi::CStr;
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    // The name a lookup found (`rc` 0, `found` set), or the number.
    let named = |rc: libc::c_int, found: bool, name: *const libc::c_char, id: u32| {
        if rc == 0 && found {
            // SAFETY: a found entry's name is a NUL-ended string in `buf`, read before `buf`
            // is used again.
            unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned()
        } else {
            id.to_string()
        }
    };
    // SAFETY: getpwuid_r and getgrgid_r fill the zeroed struct they are given, pointing into
    // `buf`, and set `found` to it or to null.
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found = std::ptr::null_mut();
    let rc = unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut found) };
    let user = named(rc, !found.is_null(), pw.pw_name, uid);
    let mut gr: libc::group = unsafe { std::mem::zeroed() };
    let mut found = std::ptr::null_mut();
    let rc = unsafe { libc::getgrgid_r(gid, &mut gr, buf.as_mut_ptr(), buf.len(), &mut found) };
    let group = named(rc, !found.is_null(), gr.gr_name, gid);
    format!("{user}:{group}")
}

/// Open with the desktop's default application, detached.
pub fn open_default(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows_open(path)
    }
    #[cfg(not(windows))]
    {
        let opener = crate::termux::opener().unwrap_or_else(|| {
            (if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            })
            .into()
        });
        let mut c = crate::tools::command(opener);
        c.arg(path);
        // An opener that finds no application says so and stops at once: that is an error, not
        // "opened".
        crate::tools::spawn_watched(c, std::time::Duration::from_secs(1))
    }
}

/// The shell's own "open", called directly: no `cmd /C start`, which would expand `%NAME%` in
/// the file's name and act on `&`.
#[cfg(windows)]
fn windows_open(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain([0]).collect::<Vec<u16>>();
    let (verb, file) = (wide("open".as_ref()), wide(path.as_os_str()));
    // SAFETY: both strings end in a NUL and outlive the call; COM is released only when this
    // call initialised it.
    let code = unsafe {
        let com = CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
        let r = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        if com >= 0 {
            CoUninitialize();
        }
        r as isize
    };
    // Above 32 is success; 31 is "no association"; else the code is a Win32 error number.
    match code {
        33.. => Ok(()),
        31 => Err(io::Error::other(crate::t!("err.no_application"))),
        _ => Err(io::Error::from_raw_os_error(code as i32)),
    }
}

/// `%XX` undone, as in a `file://` link: a `%` without two digits of sixteen after it stays.
pub fn percent_decode(bytes: &[u8]) -> Vec<u8> {
    let digit = |at: usize| bytes.get(at).and_then(|b| (*b as char).to_digit(16));
    let (mut out, mut at) = (Vec::with_capacity(bytes.len()), 0);
    while at < bytes.len() {
        match (bytes[at], digit(at + 1), digit(at + 2)) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                at += 3;
            }
            (byte, ..) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    out
}

/// `s` as a path: `~` is the home folder, and a relative path starts from `base`.
pub fn resolve(base: &Path, s: &str) -> PathBuf {
    let s = s.trim();
    let p = match s.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => crate::dirs::home_dir()
            .unwrap_or_default()
            .join(rest.trim_start_matches(['/', '\\'])),
        _ => PathBuf::from(s),
    };
    if p.is_absolute() { p } else { base.join(p) }
}

/// A path a file names (a BOM's source file, a provenance's subject) as an existing file or
/// folder in `dir`, or below it. Never one that climbs out: `..` is refused, a leading `/` is
/// dropped, and a part that would replace `dir` (a drive such as `C:`) makes it none.
pub fn beneath(dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim_start_matches(['/', '\\']);
    if rel.is_empty() || rel.split(['/', '\\']).any(|s| s == "..") {
        return None;
    }
    // One part at a time, so the path has the system's separators (a file's are mostly `/`).
    let mut p = dir.to_path_buf();
    p.extend(
        rel.split(['/', '\\'])
            .filter(|s| !s.is_empty() && *s != "."),
    );
    (p.starts_with(dir) && p != dir && p.exists()).then_some(p)
}

/// Folders watched for changes (not their subfolders), for an app that reads them again when
/// something in them changes. The changes are gathered until they settle (`Settle`).
pub struct Watch {
    watcher: Option<crate::DirWatcher>,
    rx: std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
    watched: Vec<PathBuf>,
    settle: Settle,
}

impl Default for Watch {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Watch {
            watcher: notify::Watcher::new(tx, notify::Config::default()).ok(),
            rx,
            watched: vec![],
            settle: Settle::default(),
        }
    }
}

impl Watch {
    /// Watch these folders and no others. A folder that cannot be watched (gone, inside an
    /// archive or a history) is left out.
    pub fn set(&mut self, dirs: Vec<PathBuf>) {
        use notify::Watcher;
        if dirs == self.watched {
            return;
        }
        let Some(w) = self.watcher.as_mut() else {
            return;
        };
        for d in self.watched.iter().filter(|d| !dirs.contains(d)) {
            let _ = w.unwatch(d);
        }
        for d in dirs.iter().filter(|d| !self.watched.contains(d)) {
            let _ = w.watch(d, notify::RecursiveMode::NonRecursive);
        }
        self.watched = dirs;
    }

    /// The paths changed since the last time, once they have settled; empty until then.
    pub fn changed(&mut self) -> Vec<PathBuf> {
        let now = std::time::Instant::now();
        let paths: Vec<PathBuf> = self
            .rx
            .try_iter()
            .flatten()
            .filter(|e| !e.kind.is_access())
            .flat_map(|e| e.paths)
            .collect();
        self.settle.add(paths, now);
        self.settle.take(now)
    }
}

/// Changes gathered until none has come for `QUIET`, or for `MOST` at most while they keep
/// coming: a build or a copy writes many files, and they are read again once.
#[derive(Default)]
pub struct Settle {
    paths: std::collections::HashSet<PathBuf>,
    first: Option<std::time::Instant>,
    last: Option<std::time::Instant>,
}

impl Settle {
    const QUIET: std::time::Duration = std::time::Duration::from_millis(250);
    const MOST: std::time::Duration = std::time::Duration::from_secs(2);

    pub fn add(&mut self, paths: Vec<PathBuf>, now: std::time::Instant) {
        if !paths.is_empty() {
            self.first.get_or_insert(now);
            self.last = Some(now);
            self.paths.extend(paths);
        }
    }

    pub fn take(&mut self, now: std::time::Instant) -> Vec<PathBuf> {
        match (self.first, self.last) {
            (Some(first), Some(last)) if now - last >= Self::QUIET || now - first >= Self::MOST => {
                (self.first, self.last) = (None, None);
                self.paths.drain().collect()
            }
            _ => vec![],
        }
    }
}

/// An error's cause in one line, as a dialog says it under a title that names what failed: the
/// first line, without the path before it and the OS error number. The raw text goes under
/// Details. (The desktop app's `errors.js` does the same.)
pub fn cause(err: &str) -> String {
    let first = err.lines().next().unwrap_or_default().trim();
    let first = first
        .rsplit_once(" (os error ")
        .filter(|(_, n)| n.ends_with(')'))
        .map_or(first, |(t, _)| t)
        .trim();
    match first
        .rsplit_once(": ")
        .map(|(_, tail)| tail.trim())
        .filter(|t| !t.is_empty())
    {
        Some(tail) => {
            let first = tail.chars().next().map_or(0, char::len_utf8);
            crate::i18n::caps(&tail[..first]) + &tail[first..]
        }
        None => first.to_string(),
    }
}

/// A system error in plain words, in the current language ("No permission", "Not found"), or
/// `None` for one that has none.
pub fn plain_kind(kind: io::ErrorKind) -> Option<String> {
    use io::ErrorKind as K;
    let key = match kind {
        K::PermissionDenied => "oserr.permission",
        K::NotFound => "oserr.not_found",
        K::AlreadyExists => "oserr.exists",
        K::StorageFull | K::QuotaExceeded => "oserr.full",
        K::ReadOnlyFilesystem => "oserr.read_only",
        K::DirectoryNotEmpty => "oserr.not_empty",
        K::NotADirectory => "oserr.not_folder",
        K::IsADirectory => "oserr.is_folder",
        K::ResourceBusy | K::ExecutableFileBusy => "oserr.busy",
        K::InvalidFilename => "oserr.bad_name",
        K::FileTooLarge => "oserr.too_large",
        K::TimedOut => "oserr.timed_out",
        K::StaleNetworkFileHandle | K::NetworkDown | K::NetworkUnreachable | K::HostUnreachable => "oserr.network",
        K::ConnectionRefused => "oserr.refused",
        K::CrossesDevices => "oserr.other_disk",
        _ => return None,
    };
    Some(crate::t!(key))
}

/// A status line's error in plain words: its first line, with what the system said
/// ("Permission denied (os error 13)") as `plain_kind` says it. A text it changed goes, whole,
/// to helper.log.
pub fn plain(raw: &str) -> String {
    let out = plain_words(raw);
    if out != raw.trim() {
        crate::helper::app_log(raw.trim());
    }
    out
}

/// `plain` without the log: for a text drawn again and again.
pub fn plain_words(raw: &str) -> String {
    plain_line(raw.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim())
}

fn plain_line(line: &str) -> String {
    // "…: Permission denied (os error 13)": the number says what it is on every system.
    let (said, kind) = match line.rsplit_once(" (os error ").and_then(|(t, n)| Some((t, n.strip_suffix(')')?.parse::<i32>().ok()?))) {
        Some((t, n)) => (t, io::Error::from_raw_os_error(n).kind()),
        None => {
            // An error made from a kind alone says it in English: "entity not found".
            let tail = line.rsplit_once(": ").map_or(line, |(_, t)| t);
            match KINDS.iter().find(|k| io::Error::from(**k).to_string() == tail) {
                Some(k) => (line, *k),
                None => return line.to_string(),
            }
        }
    };
    let Some(words) = plain_kind(kind) else { return line.to_string() };
    match said.rsplit_once(": ") {
        Some((head, _)) => format!("{head}: {words}"),
        None => words,
    }
}

/// The kinds `plain_kind` has words for, to know them by their English text.
const KINDS: &[io::ErrorKind] = &[
    io::ErrorKind::PermissionDenied,
    io::ErrorKind::NotFound,
    io::ErrorKind::AlreadyExists,
    io::ErrorKind::StorageFull,
    io::ErrorKind::ReadOnlyFilesystem,
    io::ErrorKind::DirectoryNotEmpty,
    io::ErrorKind::NotADirectory,
    io::ErrorKind::IsADirectory,
    io::ErrorKind::ResourceBusy,
    io::ErrorKind::InvalidFilename,
    io::ErrorKind::FileTooLarge,
    io::ErrorKind::TimedOut,
];

#[cfg(test)]
mod tests {
    #[test]
    fn plain_says_system_errors_in_words() {
        let e = io::Error::from_raw_os_error(if cfg!(windows) { 5 } else { 13 });
        assert_eq!(plain_line(&format!("Could not open: {e}")), format!("Could not open: {}", crate::t!("oserr.permission")));
        assert_eq!(plain_line(&io::Error::from(io::ErrorKind::NotFound).to_string()), crate::t!("oserr.not_found"));
        assert_eq!(plain_line("/a/b: entity not found"), format!("/a/b: {}", crate::t!("oserr.not_found")));
        // Only the first line: an opener's stderr can run long.
        assert_eq!(plain_words("\nxdg-open: no method available\nmore\n"), "xdg-open: no method available");
        // Not a system error: left as it is.
        assert_eq!(plain_words("Copied 3 items"), "Copied 3 items");
    }

    #[test]
    fn percent_decode_undoes_two_digits_and_leaves_the_rest() {
        assert_eq!(super::percent_decode(b"a%C3%A6%2f"), "aæ/".as_bytes());
        assert_eq!(super::percent_decode(b"%zz%+1%4"), b"%zz%+1%4");
    }

    use super::*;

    /// A link to nothing (or to itself) is never a program: Enter would try to run it.
    #[cfg(unix)]
    #[test]
    fn dangling_and_looping_links_are_no_programs() {
        let d = std::env::temp_dir().join(format!("coxswain-dangling-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        std::os::unix::fs::symlink("/nonexistent", d.join("gone")).unwrap();
        std::os::unix::fs::symlink("loop2", d.join("loop1")).unwrap();
        std::os::unix::fs::symlink("loop1", d.join("loop2")).unwrap();
        fs::write(d.join("run.sh"), b"#!/bin/sh\n").unwrap();
        fs::set_permissions(d.join("run.sh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("run.sh", d.join("run-link")).unwrap();
        let all = list(&d, true).unwrap();
        let exec = |n: &str| all.iter().find(|e| e.name == n).unwrap().is_exec;
        assert!(!exec("gone") && !exec("loop1") && !exec("loop2"));
        assert!(exec("run.sh") && exec("run-link"), "a link to a program is one");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn summary_counts_folders_files_and_bytes() {
        let d = std::env::temp_dir().join(format!("coxswain-summary-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("sub")).unwrap();
        fs::write(d.join("a.txt"), b"abc").unwrap();
        fs::write(d.join(".hid"), b"12345").unwrap();
        let s = summary(&d, false).unwrap();
        assert_eq!((s.folders, s.files, s.bytes), (1, 1, 3));
        assert!(s.newest > 0);
        assert_eq!(summary(&d, true).unwrap().bytes, 8);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn delete_plan_asks_and_deletes_for_good_where_there_is_no_trash() {
        let plan = |forever, inside, takes| plan_delete(forever, inside, takes, false);
        let trash = plan(false, false, true);
        assert!(!trash.forever && !trash.must_confirm);
        assert_eq!(trash.question, "confirm.trash");
        let gone = plan(true, false, true);
        assert!(gone.forever && !gone.must_confirm);
        assert_eq!(gone.question, "confirm.delete_forever");
        let none = plan(false, false, false);
        assert!(none.forever && none.must_confirm);
        assert_eq!(none.question, "confirm.no_trash_here");
        assert_eq!(
            plan_delete(false, false, false, true).question,
            "termux.no_trash_storage"
        );
        let inside = plan(false, true, true);
        assert!(inside.forever && !inside.must_confirm);
        assert_eq!(inside.question, "confirm.archive_remove");
        // Asked for good or inside an archive, the trash is not asked about.
        assert!(!delete_plan(&[PathBuf::from("/nowhere")], true, false).must_confirm);
    }

    #[test]
    #[cfg(unix)]
    fn owner_ids_get_their_names() {
        assert!(
            owner_names(0, 0).starts_with("root:"),
            "{}",
            owner_names(0, 0)
        );
        assert_eq!(
            owner_names(4_000_000_001, 4_000_000_002),
            "4000000001:4000000002"
        );
    }

    /// On a Mac the walks leave other apps' data alone: `~/Library` but iCloud Drive and the
    /// cloud folders, the second view of the disk, and the photo and music libraries.
    #[test]
    fn other_apps_data_is_protected() {
        let home = Path::new("/Users/me");
        let p = |s: &str| protected_in(home, Path::new(s));
        for yes in [
            "/Users/me/Library/Containers",
            "/Users/me/Library/Group Containers/x/a.db",
            "/Users/me/Library/Mail/V10",
            "/Users/me/Library/Messages",
            "/Users/me/Library/Safari",
            "/Users/me/Library/Application Support/Slack",
            "/System/Volumes/Data/Users/me/Library/Containers",
            "/private/var/folders/xy/T",
            "/Users/me/.Trash",
            "/Users/me/Pictures/Photos Library.photoslibrary",
            "/Users/me/Music/Music/Music Library.musiclibrary",
        ] {
            assert!(p(yes), "{yes}");
        }
        for no in [
            "/Users/me",
            "/Users/me/Library",
            "/Users/me/Library/CloudStorage/OneDrive-Personal/a.docx",
            "/Users/me/Library/Mobile Documents/com~apple~CloudDocs",
            "/Users/me/Documents",
            "/Users/me/Desktop/Library/notes.txt",
            "/Volumes/USB",
            "/Users/me/Pictures/holiday.jpg",
        ] {
            assert!(!p(no), "{no}");
        }
        assert_eq!(
            protected(Path::new("/Users/me/Library/Containers")),
            cfg!(target_os = "macos") && crate::dirs::home_dir().as_deref() == Some(home)
        );
    }

    /// ZFS's `.zfs` with `snapdir=visible` holds every snapshot again: no walk goes in.
    #[test]
    fn walks_leave_zfs_snapshots_alone() {
        let dir = std::env::temp_dir().join(format!("coxswain-zfsdir-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".zfs/snapshot/daily")).unwrap();
        std::fs::create_dir_all(dir.join("other/.zfs")).unwrap();
        assert!(protected(&dir.join(".zfs")));
        assert!(
            !protected(&dir.join(".zfs/snapshot/daily")),
            "opened by the user, a snapshot lists as usual"
        );
        assert!(
            !protected(&dir.join("other/.zfs")),
            "a folder of that name without snapshots is a folder"
        );
        assert!(!protected(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A folder refused once is not asked for again: on a Mac, each ask can be a prompt.
    #[cfg(unix)]
    #[test]
    fn a_refused_folder_is_asked_once() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-refused-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("shut")).unwrap();
        fs::set_permissions(d.join("shut"), fs::Permissions::from_mode(0o000)).unwrap();
        let refused = std::sync::Mutex::default();
        let first = read_dir_once(&refused, &d.join("shut"));
        fs::set_permissions(d.join("shut"), fs::Permissions::from_mode(0o755)).unwrap();
        // Root reads any folder: nothing is refused then.
        if first.is_err() {
            assert!(
                read_dir_once(&refused, &d.join("shut")).is_err(),
                "not asked again"
            );
        }
        assert!(read_dir_once(&refused, &d).is_ok());
        fs::remove_dir_all(&d).unwrap();
    }

    /// Properties of a folder it may not list say so, not "0 B in 0 files".
    #[cfg(unix)]
    #[test]
    fn properties_of_an_unreadable_folder_say_why() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-props-shut-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("shut")).unwrap();
        fs::set_permissions(d.join("shut"), fs::Permissions::from_mode(0o000)).unwrap();
        let p = properties(&d.join("shut")).unwrap();
        let root = fs::read_dir(d.join("shut")).is_ok();
        fs::set_permissions(d.join("shut"), fs::Permissions::from_mode(0o755)).unwrap();
        // Root lists any folder.
        if !root {
            assert_eq!(p.unreadable, Some(crate::t!("oserr.permission")));
        }
        assert_eq!(properties(&d).unwrap().unreadable, None);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn an_error_is_said_in_one_line() {
        assert_eq!(
            cause("/home/me/budget.txt: Permission denied (os error 13)"),
            "Permission denied"
        );
        assert_eq!(
            cause("Neither podman nor docker is installed"),
            "Neither podman nor docker is installed"
        );
        assert_eq!(
            cause("a.txt: locked: wrong password\nb.txt: No space left on device (os error 28)"),
            "Wrong password"
        );
        assert_eq!(cause("a.txt: ნებართვა უარყოფილია"), "ნებართვა უარყოფილია");
    }

    #[test]
    fn watch_settles_changes_before_giving_them_out() {
        use std::time::{Duration, Instant};
        let (t0, ms) = (Instant::now(), |n| Duration::from_millis(n));
        let mut s = Settle::default();
        assert!(s.take(t0).is_empty(), "nothing yet");
        s.add(vec![PathBuf::from("/d/a")], t0);
        s.add(
            vec![PathBuf::from("/d/a"), PathBuf::from("/d/b")],
            t0 + ms(100),
        );
        s.add(vec![], t0 + ms(300));
        assert!(s.take(t0 + ms(300)).is_empty(), "still coming 200 ms ago");
        let mut got = s.take(t0 + ms(350));
        got.sort();
        assert_eq!(
            got,
            [PathBuf::from("/d/a"), PathBuf::from("/d/b")],
            "once, together"
        );
        assert!(s.take(t0 + ms(5000)).is_empty(), "and not again");
        // Changes that never stop are given out every two seconds all the same.
        let mut out = 0;
        for i in 0..50 {
            let now = t0 + ms(10_000 + i * 100);
            s.add(vec![PathBuf::from(format!("/d/log{i}"))], now);
            out += usize::from(!s.take(now).is_empty());
        }
        assert_eq!(out, 2, "in five seconds of changes");
    }

    #[test]
    fn watch_hears_a_folder_change() {
        let d = std::env::temp_dir().join(format!("coxswain-test-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut w = Watch::default();
        w.set(vec![d.clone()]);
        std::fs::write(d.join("new.txt"), "x").unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut got = vec![];
        while got.is_empty() && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(50));
            got = w.changed();
        }
        assert!(
            got.iter()
                .any(|p| p.file_name().is_some_and(|n| n == "new.txt")),
            "{got:?}"
        );
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn fs_resolve_paths() {
        let base = Path::new("/base");
        assert_eq!(resolve(base, " sub/x "), PathBuf::from("/base/sub/x"));
        assert_eq!(resolve(base, "/abs"), PathBuf::from("/abs"));
        let home = crate::dirs::home_dir().unwrap_or_default();
        assert_eq!(resolve(base, "~"), home);
        assert_eq!(resolve(base, "~/x"), home.join("x"));
        assert_eq!(resolve(base, "~x"), PathBuf::from("/base/~x"));
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn fs_list_and_sort() {
        let d = tmp("list");
        for f in ["b10.txt", "b2.txt", "a.rs", ".hidden"] {
            fs::write(d.join(f), f).unwrap();
        }
        fs::create_dir(d.join("zdir")).unwrap();
        let mut v = list(&d, false).unwrap();
        sort(&mut v, SortKey::Name, false);
        let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["..", "zdir", "a.rs", "b2.txt", "b10.txt"]);
        sort(&mut v, SortKey::Ext, false);
        assert_eq!(v[2].name, "a.rs");
        assert_eq!(list(&d, true).unwrap().len(), 6);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn by_size_folders_go_by_their_measured_size() {
        let e = |name: &str, is_dir: bool, size: u64| Entry {
            name: name.into(),
            path: PathBuf::from("/d").join(name),
            is_dir,
            size,
            ..Entry::default()
        };
        // A folder's own entry size (4096 on ext4) says nothing about what is in it.
        let all = [
            e("..", true, 4096),
            e("one", true, 4096),
            e("fifty", true, 4096),
            e("five-g", true, 4096),
            e("b-new", true, 9999),
            e("a-new", true, 0),
            e("x.txt", false, 10),
            e("y.txt", false, 20),
        ];
        let measured: std::collections::HashMap<PathBuf, u64> = [("one", 1 << 20), ("fifty", 50 << 20), ("five-g", 5 << 30)]
            .map(|(n, b)| (PathBuf::from("/d").join(n), b))
            .into();
        let order = |measured: &std::collections::HashMap<PathBuf, u64>, reverse: bool| {
            let mut v = all.to_vec();
            sort_in(Path::new("/d"), &mut v, SortKey::Size, reverse, measured);
            v.iter().map(|e| e.name.clone()).collect::<Vec<_>>()
        };
        assert_eq!(order(&measured, false), ["..", "five-g", "fifty", "one", "a-new", "b-new", "y.txt", "x.txt"], "unmeasured after, by name");
        assert_eq!(order(&measured, true), ["..", "one", "fifty", "five-g", "a-new", "b-new", "x.txt", "y.txt"], "unmeasured last either way");
        // Folder sizes off: nothing measured, folders by name, never by their entry size.
        assert_eq!(order(&Default::default(), false), ["..", "a-new", "b-new", "fifty", "five-g", "one", "y.txt", "x.txt"]);
        let mut v = all.to_vec();
        sort(&mut v, SortKey::Size, false);
        assert_eq!(v[1].name, "a-new");
        // Inside an archive a folder's listed size is its real one.
        let d = tmp("size-sort");
        fs::write(d.join("a.zip"), "").unwrap();
        let mut v = all.to_vec();
        sort_in(&d.join("a.zip"), &mut v, SortKey::Size, false, &Default::default());
        assert_eq!(v[1].name, "b-new");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_list_of_commits_stays_newest_first() {
        let e = |name: &str, modified: u64| Entry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir: true,
            modified,
            created: modified,
            ..Entry::default()
        };
        // As git lists them: newest first; two made in the same second.
        let git = [
            e("..", 0),
            e("f00d123 Third", 30),
            e("a11c0de Second", 20),
            e("beef000 First, again", 10),
            e("0ddba11 First", 10),
        ];
        let order = |v: &[Entry]| {
            v.iter()
                .map(|e| e.name[..2].to_string())
                .collect::<Vec<_>>()
        };
        for key in [
            SortKey::Name,
            SortKey::Ext,
            SortKey::Size,
            SortKey::Time,
            SortKey::Commit,
        ] {
            let mut v = git.to_vec();
            sort_in(Path::new("/r/src/main.rs/@history"), &mut v, key, false, &Default::default());
            assert_eq!(order(&v), ["..", "f0", "a1", "be", "0d"], "{key:?}");
        }
        // Reversed too, and in every branch's history: its graph is drawn in git's order.
        for dir in ["/r/src/main.rs/@history", "/r/src/@history/@all"] {
            let mut v = git.to_vec();
            sort_in(Path::new(dir), &mut v, SortKey::Name, true, &Default::default());
            assert_eq!(order(&v), ["..", "f0", "a1", "be", "0d"]);
        }
        // Anywhere else, as asked.
        let mut v = git.to_vec();
        sort_in(Path::new("/r/src"), &mut v, SortKey::Name, false, &Default::default());
        assert_eq!(order(&v), ["..", "0d", "a1", "be", "f0"]);
    }

    #[test]
    fn fs_natural_order() {
        let names = |v: &[Entry]| v.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        let mut v: Vec<Entry> = ["x9z", "file10", "B", "file010a", "ab", "file2", "a", "x9y", "abc"]
            .into_iter()
            .map(|n| Entry {
                name: n.into(),
                ..Default::default()
            })
            .collect();
        sort(&mut v, SortKey::Name, false);
        let want = ["a", "ab", "abc", "B", "file2", "file10", "file010a", "x9y", "x9z"];
        assert_eq!(names(&v), want);
        sort(&mut v, SortKey::Name, true);
        assert_eq!(names(&v), want.iter().rev().copied().collect::<Vec<_>>());
    }

    #[test]
    fn fs_copy_move_delete() {
        let d = tmp("ops");
        mkdir(&d.join("src/sub")).unwrap();
        fs::write(d.join("src/sub/f"), "x").unwrap();
        mkdir(&d.join("dst")).unwrap();
        assert_eq!(
            copy(&d.join("src"), &d.join("dst")).unwrap(),
            d.join("dst/src")
        );
        assert_eq!(fs::read_to_string(d.join("dst/src/sub/f")).unwrap(), "x");
        assert!(copy(&d.join("src"), &d.join("src/sub")).is_err());
        // Through a link to the same place (`/home` is `/usr/home` on FreeBSD): refused too, and
        // so is a move.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&d, d.join("alias")).unwrap();
            assert!(copy(&d.join("alias/src"), &d.join("src/sub")).is_err());
            assert!(copy(&d.join("src"), &d.join("alias/src/sub")).is_err());
            assert!(rename(&d.join("alias/src"), &d.join("src/sub")).is_err());
            assert!(!d.join("src/sub/src").exists());
            fs::remove_file(d.join("alias")).unwrap();
        }
        // Copying a file onto itself or over another file is refused, not truncated.
        assert!(copy(&d.join("src/sub/f"), &d.join("src/sub")).is_err());
        assert_eq!(fs::read_to_string(d.join("src/sub/f")).unwrap(), "x");
        rename(&d.join("src"), &d.join("moved")).unwrap();
        assert!(!d.join("src").exists() && d.join("moved/sub/f").exists());
        fs::write(d.join("dst/moved"), "").unwrap();
        assert!(rename(&d.join("moved"), &d.join("dst")).is_err());
        // Renamed onto itself (as a case change is where case does not count): nothing lost.
        assert_eq!(
            rename(&d.join("moved"), &d.join("moved")).unwrap(),
            d.join("moved")
        );
        assert!(d.join("moved/sub/f").exists());
        assert_eq!(dir_size(&d.join("moved")), (1, 1));
        // A link to nothing at the target is there all the same: not replaced.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(d.join("nowhere"), d.join("link")).unwrap();
            fs::write(d.join("b.txt"), "b").unwrap();
            assert!(rename(&d.join("b.txt"), &d.join("link")).is_err());
            assert!(fs::symlink_metadata(d.join("link")).unwrap().is_symlink());
            fs::remove_file(d.join("link")).unwrap();
            fs::remove_file(d.join("b.txt")).unwrap();
        }
        fs::write(d.join("a.txt"), "").unwrap();
        assert_eq!(free_name(&d, "a.txt"), d.join("a (2).txt"));
        assert_eq!(free_name(&d, "b.txt"), d.join("b.txt"));
        assert_eq!(free_name(&d, "moved"), d.join("moved (2)"));
        let scratch = Scratch::new().unwrap();
        #[cfg(unix)]
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(
                &fs::metadata(&scratch.0).unwrap().permissions()
            ) & 0o777,
            0o700
        );
        let kept = scratch.0.clone();
        drop(scratch);
        assert!(!kept.exists());
        delete(&d.join("moved")).unwrap();
        assert!(!d.join("moved").exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn fs_copy_refuses_pipes_keeps_dates_and_a_move_into_an_archive_keeps_links() {
        use std::os::unix::ffi::OsStrExt;
        let d = tmp("special");
        mkdir(&d.join("src")).unwrap();
        fs::write(d.join("src/real.txt"), "x").unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        fs::File::open(d.join("src/real.txt")).unwrap().set_modified(old).unwrap();
        let pipe = std::ffi::CString::new(d.join("src/pipe").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
        // A pipe is refused, not waited on for ever; the folder it is in leaves nothing behind,
        // so trying again is not refused as "exists".
        let err = |r: io::Result<PathBuf>| r.unwrap_err().kind();
        assert_eq!(err(copy(&d.join("src/pipe"), &d)), io::ErrorKind::Unsupported);
        assert_eq!(err(copy(&d.join("src"), &d.join("out"))), io::ErrorKind::Unsupported);
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1, "no copy, no passing name");
        fs::remove_file(d.join("src/pipe")).unwrap();
        copy(&d.join("src"), &d.join("out")).unwrap();
        assert_eq!(fs::metadata(d.join("out/real.txt")).unwrap().modified().unwrap(), old);
        // Moved into a zip, a link the zip would leave out stops the move before anything is
        // deleted.
        std::os::unix::fs::symlink("real.txt", d.join("src/lnk")).unwrap();
        crate::archive::create(&d.join("a.zip"), &[d.join("out/real.txt")]).unwrap();
        assert_eq!(err(rename(&d.join("src"), &d.join("a.zip"))), io::ErrorKind::Unsupported);
        assert!(fs::symlink_metadata(d.join("src/lnk")).unwrap().is_symlink());
        assert!(d.join("src/real.txt").exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn fs_lock_down_makes_old_installs_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-lockdown-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("previews")).unwrap();
        fs::write(d.join("index.bin"), b"names").unwrap();
        for p in [&d, &d.join("previews")] {
            fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::set_permissions(d.join("index.bin"), fs::Permissions::from_mode(0o644)).unwrap();
        lock_down_in(&d).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            (
                mode(&d),
                mode(&d.join("previews")),
                mode(&d.join("index.bin"))
            ),
            (0o700, 0o700, 0o600)
        );
        fs::remove_dir_all(d).unwrap();
    }
}

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
    fn from_path(path: PathBuf, name: String, lmeta: io::Result<fs::Metadata>) -> io::Result<Entry> {
        let lmeta = lmeta?;
        let is_symlink = lmeta.file_type().is_symlink();
        // Follow links for type and size; a dangling link stays a plain entry.
        let meta = if is_symlink { fs::metadata(&path).unwrap_or(lmeta) } else { lmeta };
        Ok(Entry {
            hidden: name.starts_with('.') || is_hidden_attr(&meta),
            is_exec: !meta.is_dir() && is_exec(&meta, &name),
            is_dir: meta.is_dir(),
            is_symlink,
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified: meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()),
            created: meta.created().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()),
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
    [".exe", ".bat", ".cmd", ".ps1", ".com"].iter().any(|e| n.ends_with(e))
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
pub fn list_with_archive(dir: &Path, show_hidden: bool) -> io::Result<(Vec<Entry>, Option<InArchive>)> {
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
    let up = crate::zfs::in_snapshot(dir).filter(|s| s.rel.as_os_str().is_empty()).map(|s| crate::zfs::path(&s.mountpoint));
    if let Some(parent) = up.or_else(|| dir.parent().map(Path::to_path_buf)) {
        out.push(Entry {
            name: "..".into(),
            path: parent,
            is_dir: true,
            is_symlink: false,
            is_exec: false,
            hidden: false,
            size: 0,
            modified: 0,
            created: 0,
            online: false,
            referenced: 0,
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
/// `sort` for the listing of `dir`. A history's list of commits is always by date, newest
/// first, and in git's order where two have the same time: its names start with commit ids,
/// which would put it in a random order.
/// The branches and the worktrees keep git's order: local branches before remote ones.
pub fn sort_in(dir: &Path, entries: &mut [Entry], key: SortKey, reverse: bool) {
    if crate::history::split(dir).is_some_and(|at| at.commit.is_none() && at.view != crate::history::View::History) {
        entries.sort_by_key(|e| !e.is_parent());
    } else if dir.file_name().is_some_and(|n| n == crate::history::MARKER || n == crate::zfs::MARKER) {
        entries.sort_by(|a, b| {
            let ord = b.modified.cmp(&a.modified);
            (!a.is_parent()).cmp(&!b.is_parent()).then(if reverse { ord.reverse() } else { ord })
        });
    } else {
        sort(entries, key, reverse);
    }
}

pub fn sort(entries: &mut [Entry], key: SortKey, reverse: bool) {
    entries.sort_by(|a, b| {
        let group = |e: &Entry| (!e.is_parent(), !e.is_dir);
        group(a).cmp(&group(b)).then_with(|| {
            let name = || natord(&a.name, &b.name);
            let ord = match key {
                SortKey::Name | SortKey::Commit => name(),
                SortKey::Ext => natord(a.ext(), b.ext()).then_with(name),
                SortKey::Time => b.modified.cmp(&a.modified).then_with(name),
                SortKey::Size => b.size.cmp(&a.size).then_with(name),
            };
            if reverse { ord.reverse() } else { ord }
        })
    });
}

/// Case-insensitive natural order: "file2" < "file10". No allocation: a sort of 100,000
/// names calls this two million times.
fn natord(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a, b);
    loop {
        let (Some(x), Some(y)) = (a.chars().next(), b.chars().next()) else {
            return a.len().cmp(&b.len());
        };
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let digits = |s: &str| s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
            let (na, nb) = (&a[..digits(a)], &b[..digits(b)]);
            let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
            let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
            if ord.is_ne() {
                return ord;
            }
            a = &a[na.len()..];
            b = &b[nb.len()..];
        } else {
            let ord = if x.is_ascii() && y.is_ascii() { x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase()) } else { x.to_lowercase().cmp(y.to_lowercase()) };
            if ord.is_ne() {
                return ord;
            }
            a = &a[x.len_utf8()..];
            b = &b[y.len_utf8()..];
        }
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
fn same_file(a: &Path, b: &Path) -> bool {
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
    if dst.is_dir() { None } else { crate::archive::split(dst) }
}

/// `src` inside an archive, when it is.
fn out_of_archive(src: &Path) -> Option<(PathBuf, String)> {
    if src.exists() { None } else { crate::archive::split(src) }
}

/// A folder of its own under the temp folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> io::Result<Scratch> {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("coxswain-archive-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        // Made here, never one that is there: the temp folder is everyone's.
        fs::create_dir(&dir)?;
        private_dir(&dir)?;
        Ok(Scratch(dir))
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
/// ZFS snapshot and the lists of snapshots and of a package's files.
fn writable(path: &Path) -> io::Result<()> {
    let denied = |key: &str| Err(io::Error::new(io::ErrorKind::PermissionDenied, crate::t!(key)));
    if crate::zfs::is_read_only(path) {
        return denied("zfs.read_only");
    }
    if path.ancestors().any(|a| crate::bsd::split(a).is_some()) {
        return denied("pkg.read_only");
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
fn copy_out_or_in(src: &Path, dst: &Path, password: Option<&str>, whole: bool) -> io::Result<PathBuf> {
    writable(dst)?;
    if !src.exists()
        && let Some(at) = crate::history::split(src)
    {
        return crate::history::copy_out(&at, dst);
    }
    if let Some((archive, inner)) = into_archive(dst) {
        let name = src.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nothing to copy"))?.to_string_lossy();
        // Into a folder of the archive under its own name, or under the name `dst` gives.
        let (to, inner) = if crate::archive::is_folder(&archive, &inner)? {
            (dst.join(&*name), if inner.is_empty() { name.into_owned() } else { format!("{inner}/{name}") })
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
    if to.starts_with(src) && src.is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "cannot copy a folder into itself"));
    }
    copy_tree(src, &to)?;
    Ok(to)
}

fn copy_tree(src: &Path, to: &Path) -> io::Result<()> {
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
            copy_tree(&de.path(), &to.join(de.file_name()))?;
        }
        Ok(())
    } else if fs::symlink_metadata(to).is_ok() {
        // Never overwrite: copying a file onto itself would truncate it.
        // ponytail: no overwrite prompt yet; delete the target first.
        Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", to.display())))
    } else {
        fs::copy(src, to).map(drop)
    }
}

/// Move or rename. Falls back to copy + delete across filesystems.
pub fn rename(src: &Path, dst: &Path) -> io::Result<PathBuf> {
    rename_locked(src, dst, None)
}

/// `rename`, with the password of the archive `src` is inside: out of an archive it is copied
/// out, then taken out of the archive.
pub fn rename_locked(src: &Path, dst: &Path, password: Option<&str>) -> io::Result<PathBuf> {
    writable(src)?;
    writable(dst)?;
    match (out_of_archive(src), into_archive(dst)) {
        // Within one archive: renamed there, the folder `dst` names, or a new name.
        (Some((a, from)), Some((b, to))) if a == b => {
            let name = src.file_name().unwrap_or_default().to_string_lossy();
            let to = if crate::archive::is_folder(&a, &to)? { if to.is_empty() { name.to_string() } else { format!("{to}/{name}") } } else { to };
            crate::archive::rename_in(&a, &from, &to, password)?;
            Ok(a.join(to))
        }
        (Some((a, from)), _) => {
            // Taken out of the archive only when all of it came out.
            let to = copy_out_or_in(src, dst, password, true)?;
            crate::archive::remove(&a, &[from], password)?;
            Ok(to)
        }
        (None, Some(_)) => {
            let to = copy_locked(src, dst, password)?;
            delete(src)?;
            Ok(to)
        }
        (None, None) => rename_plain(src, dst),
    }
}

fn rename_plain(src: &Path, dst: &Path) -> io::Result<PathBuf> {
    let to = target(src, dst);
    // `to` may be `src` itself in another case (`Notes` to `notes` where case does not count).
    if to.exists() && !same_file(src, &to) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", to.display())));
    }
    match fs::rename(src, &to) {
        // Another disk: copied over, then the original goes. Other failures are failures.
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            copy(src, &to)?;
            delete(src)?;
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
    writable(path)?;
    if !path.exists() && crate::archive::split(path).is_some() {
        return delete_locked(path, password);
    }
    // A Flatpak's XDG_DATA_HOME is its own folder, so the trash crate would fill a trash the
    // desktop never shows: the host's gio puts it in the desktop's.
    if crate::tools::flatpak().is_some() {
        let out = crate::tools::user_command("gio").arg("trash").arg(path).stdin(std::process::Stdio::null()).output()?;
        return if out.status.success() { Ok(()) } else { Err(io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_string())) };
    }
    #[cfg(not(target_os = "android"))]
    return trash::delete(path).map_err(io::Error::other);
    #[cfg(target_os = "android")]
    Err(io::Error::other(crate::t!("termux.no_trash")))
}

/// A free name for `name` in `dir`: `name`, else `stem (2).ext`, `stem (3).ext`, ...
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    };
    std::iter::once(dir.join(name))
        .chain((2..).map(|n| dir.join(format!("{stem} ({n}){ext}"))))
        .find(|p| fs::symlink_metadata(p).is_err())
        .expect("some name is free")
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
        if let Some(home) = HOME.get_or_init(std::env::home_dir) {
            return protected_in(home, path);
        }
    }
    let _ = path;
    false
}

/// `protected`, for the home folder `home`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn protected_in(home: &Path, path: &Path) -> bool {
    // iCloud Drive and the cloud apps' folders: found by name as the cloud rules say.
    const KEEP: [&str; 2] = ["CloudStorage", "Mobile Documents"];
    const BUNDLES: [&str; 6] = ["photoslibrary", "photolibrary", "migratedphotolibrary", "aplibrary", "musiclibrary", "tvlibrary"];
    if let Ok(rest) = path.strip_prefix(home.join("Library")) {
        return rest.components().next().is_some_and(|c| !KEEP.iter().any(|k| c.as_os_str() == *k));
    }
    path.starts_with("/System/Volumes")
        || path.starts_with("/private/var/folders")
        || path == home.join(".Trash")
        || path.extension().is_some_and(|e| BUNDLES.iter().any(|b| e.eq_ignore_ascii_case(b)))
}

/// `fs::read_dir` for walks. On a Mac a folder refused once (Desktop, Documents, Downloads, a
/// removable or network volume the user did not allow) is not asked for again while this
/// process runs: one privacy prompt per folder, not one per walk.
pub fn read_dir(dir: &Path) -> io::Result<fs::ReadDir> {
    static REFUSED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> = std::sync::LazyLock::new(Default::default);
    if cfg!(target_os = "macos") { read_dir_once(&REFUSED, dir) } else { fs::read_dir(dir) }
}

fn read_dir_once(refused: &std::sync::Mutex<std::collections::HashSet<PathBuf>>, dir: &Path) -> io::Result<fs::ReadDir> {
    if refused.lock().unwrap_or_else(|e| e.into_inner()).contains(dir) {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let rd = fs::read_dir(dir);
    if rd.as_ref().is_err_and(|e| e.kind() == io::ErrorKind::PermissionDenied) {
        refused.lock().unwrap_or_else(|e| e.into_inner()).insert(dir.to_path_buf());
    }
    rd
}

/// Total bytes and file count under `path`, in parallel. Symlinks are counted, not followed;
/// unreadable parts, and other apps' data on a Mac (`protected`) below it, are skipped.
pub fn dir_size(path: &Path) -> (u64, u64) {
    use rayon::prelude::*;
    let Ok(meta) = fs::symlink_metadata(path) else { return (0, 0) };
    if !meta.is_dir() {
        return (meta.len(), 1);
    }
    let Ok(rd) = read_dir(path) else { return (0, 0) };
    rd.flatten()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|de| match de.file_type() {
            Ok(t) if t.is_dir() && protected(&de.path()) => (0, 0),
            Ok(t) if t.is_dir() => dir_size(&de.path()),
            _ => (de.metadata().map_or(0, |m| m.len()), 1),
        })
        .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
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
    pub created: Option<u64>,
    pub modified: Option<u64>,
    pub accessed: Option<u64>,
    pub readonly: bool,
    /// Unix permission bits, e.g. 0o644, and owner ids.
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    /// The ZFS dataset it is on, and how many snapshots that has.
    pub zfs: Option<crate::zfs::Facts>,
    pub snapshots: Option<usize>,
    /// The FreeBSD package it belongs to.
    pub package: Option<String>,
    pub flags: Option<crate::flags::Flags>,
}

/// Properties of `path`: a folder's size is a walk of it, and the facts run `zfs` and `pkg`,
/// so not on a thread that draws.
pub fn properties(path: &Path) -> io::Result<Props> {
    let secs = |t: io::Result<std::time::SystemTime>| Some(t.ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs());
    let lmeta = fs::symlink_metadata(path)?;
    let meta = fs::metadata(path).unwrap_or_else(|_| lmeta.clone());
    let (size, files) = dir_size(path);
    #[cfg(unix)]
    let (mode, uid, gid) = {
        use std::os::unix::fs::MetadataExt;
        (Some(meta.mode() & 0o7777), Some(meta.uid()), Some(meta.gid()))
    };
    #[cfg(not(unix))]
    let (mode, uid, gid) = (None, None, None);
    let zfs = crate::zfs::facts(path);
    let snapshots = zfs.as_ref().and_then(|f| crate::zfs::snapshots(&f.dataset).ok()).map(|v| v.len());
    Ok(Props {
        kind: if lmeta.is_symlink() { "symlink" } else if meta.is_dir() { "folder" } else { "file" },
        link_target: fs::read_link(path).ok(),
        size,
        files,
        created: secs(meta.created()),
        modified: secs(meta.modified()),
        accessed: secs(meta.accessed()),
        readonly: meta.permissions().readonly(),
        mode,
        uid,
        gid,
        zfs,
        snapshots,
        package: if lmeta.is_dir() { None } else { crate::bsd::package_of(path) },
        flags: crate::flags::read(path),
        path: path.to_path_buf(),
    })
}

/// Open with the desktop's default application, detached.
pub fn open_default(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows_open(path)
    }
    #[cfg(not(windows))]
    {
        let opener = crate::termux::opener().unwrap_or_else(|| (if cfg!(target_os = "macos") { "open" } else { "xdg-open" }).into());
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
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize};
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain([0]).collect::<Vec<u16>>();
    let (verb, file) = (wide("open".as_ref()), wide(path.as_os_str()));
    // SAFETY: both strings end in a NUL and outlive the call; COM is released only when this
    // call initialised it.
    let code = unsafe {
        let com = CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32);
        let r = ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
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

/// `s` as a path: `~` is the home folder, and a relative path starts from `base`.
pub fn resolve(base: &Path, s: &str) -> PathBuf {
    let s = s.trim();
    let p = match s.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => std::env::home_dir().unwrap_or_default().join(rest.trim_start_matches(['/', '\\'])),
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
    p.extend(rel.split(['/', '\\']).filter(|s| !s.is_empty() && *s != "."));
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
        Watch { watcher: notify::Watcher::new(tx, notify::Config::default()).ok(), rx, watched: vec![], settle: Settle::default() }
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
        let Some(w) = self.watcher.as_mut() else { return };
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
        let paths: Vec<PathBuf> = self.rx.try_iter().flatten().filter(|e| !e.kind.is_access()).flat_map(|e| e.paths).collect();
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
    let first = first.rsplit_once(" (os error ").filter(|(_, n)| n.ends_with(')')).map_or(first, |(t, _)| t).trim();
    match first.rsplit_once(": ").map(|(_, tail)| tail.trim()).filter(|t| !t.is_empty()) {
        Some(tail) => {
            let first = tail.chars().next().map_or(0, char::len_utf8);
            crate::i18n::caps(&tail[..first]) + &tail[first..]
        }
        None => first.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On a Mac the walks leave other apps' data alone: `~/Library` but iCloud Drive and the
    /// cloud folders, the second view of the disk, and the photo and music libraries.
    #[test]
    fn other_apps_data_is_protected() {
        let home = Path::new("/Users/me");
        let p = |s: &str| protected_in(home, Path::new(s));
        for yes in ["/Users/me/Library/Containers", "/Users/me/Library/Group Containers/x/a.db", "/Users/me/Library/Mail/V10", "/Users/me/Library/Messages", "/Users/me/Library/Safari", "/Users/me/Library/Application Support/Slack", "/System/Volumes/Data/Users/me/Library/Containers", "/private/var/folders/xy/T", "/Users/me/.Trash", "/Users/me/Pictures/Photos Library.photoslibrary", "/Users/me/Music/Music/Music Library.musiclibrary"] {
            assert!(p(yes), "{yes}");
        }
        for no in ["/Users/me", "/Users/me/Library", "/Users/me/Library/CloudStorage/OneDrive-Personal/a.docx", "/Users/me/Library/Mobile Documents/com~apple~CloudDocs", "/Users/me/Documents", "/Users/me/Desktop/Library/notes.txt", "/Volumes/USB", "/Users/me/Pictures/holiday.jpg"] {
            assert!(!p(no), "{no}");
        }
        assert_eq!(protected(Path::new("/Users/me/Library/Containers")), cfg!(target_os = "macos") && std::env::home_dir().as_deref() == Some(home));
    }

    /// ZFS's `.zfs` with `snapdir=visible` holds every snapshot again: no walk goes in.
    #[test]
    fn walks_leave_zfs_snapshots_alone() {
        let dir = std::env::temp_dir().join(format!("coxswain-zfsdir-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".zfs/snapshot/daily")).unwrap();
        std::fs::create_dir_all(dir.join("other/.zfs")).unwrap();
        assert!(protected(&dir.join(".zfs")));
        assert!(!protected(&dir.join(".zfs/snapshot/daily")), "opened by the user, a snapshot lists as usual");
        assert!(!protected(&dir.join("other/.zfs")), "a folder of that name without snapshots is a folder");
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
            assert!(read_dir_once(&refused, &d.join("shut")).is_err(), "not asked again");
        }
        assert!(read_dir_once(&refused, &d).is_ok());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn an_error_is_said_in_one_line() {
        assert_eq!(cause("/home/me/budget.txt: Permission denied (os error 13)"), "Permission denied");
        assert_eq!(cause("Neither podman nor docker is installed"), "Neither podman nor docker is installed");
        assert_eq!(cause("a.txt: locked: wrong password\nb.txt: No space left on device (os error 28)"), "Wrong password");
        assert_eq!(cause("a.txt: ნებართვა უარყოფილია"), "ნებართვა უარყოფილია");
    }

    #[test]
    fn watch_settles_changes_before_giving_them_out() {
        use std::time::{Duration, Instant};
        let (t0, ms) = (Instant::now(), |n| Duration::from_millis(n));
        let mut s = Settle::default();
        assert!(s.take(t0).is_empty(), "nothing yet");
        s.add(vec![PathBuf::from("/d/a")], t0);
        s.add(vec![PathBuf::from("/d/a"), PathBuf::from("/d/b")], t0 + ms(100));
        s.add(vec![], t0 + ms(300));
        assert!(s.take(t0 + ms(300)).is_empty(), "still coming 200 ms ago");
        let mut got = s.take(t0 + ms(350));
        got.sort();
        assert_eq!(got, [PathBuf::from("/d/a"), PathBuf::from("/d/b")], "once, together");
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
        assert!(got.iter().any(|p| p.file_name().is_some_and(|n| n == "new.txt")), "{got:?}");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn fs_resolve_paths() {
        let base = Path::new("/base");
        assert_eq!(resolve(base, " sub/x "), PathBuf::from("/base/sub/x"));
        assert_eq!(resolve(base, "/abs"), PathBuf::from("/abs"));
        let home = std::env::home_dir().unwrap_or_default();
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
    fn a_list_of_commits_stays_newest_first() {
        let e = |name: &str, modified: u64| Entry { name: name.into(), path: PathBuf::from(name), is_dir: true, is_symlink: false, is_exec: false, hidden: false, size: 0, modified, created: modified, online: false, referenced: 0 };
        // As git lists them: newest first; two made in the same second.
        let git = [e("..", 0), e("f00d123 Third", 30), e("a11c0de Second", 20), e("beef000 First, again", 10), e("0ddba11 First", 10)];
        let order = |v: &[Entry]| v.iter().map(|e| e.name[..2].to_string()).collect::<Vec<_>>();
        for key in [SortKey::Name, SortKey::Ext, SortKey::Size, SortKey::Time, SortKey::Commit] {
            let mut v = git.to_vec();
            sort_in(Path::new("/r/src/main.rs/@history"), &mut v, key, false);
            assert_eq!(order(&v), ["..", "f0", "a1", "be", "0d"], "{key:?}");
        }
        let mut v = git.to_vec();
        sort_in(Path::new("/r/src/main.rs/@history"), &mut v, SortKey::Name, true);
        assert_eq!(order(&v), ["..", "be", "0d", "a1", "f0"]);
        // Anywhere else, as asked.
        let mut v = git.to_vec();
        sort_in(Path::new("/r/src"), &mut v, SortKey::Name, false);
        assert_eq!(order(&v), ["..", "0d", "a1", "be", "f0"]);
    }

    #[test]
    fn fs_natural_order() {
        use std::cmp::Ordering::*;
        assert_eq!(natord("file2", "file10"), Less);
        assert_eq!(natord("file010", "file10"), Equal);
        assert_eq!(natord("file10", "file010a"), Less);
        assert_eq!(natord("a", "B"), Less);
        assert_eq!(natord("B", "a"), Greater);
        assert_eq!(natord("abc", "ab"), Greater);
        assert_eq!(natord("", "a"), Less);
        assert_eq!(natord("Ærø 2", "ærø 10"), Less);
        assert_eq!(natord("x9y", "x9z"), Less);
    }

    #[test]
    fn fs_copy_move_delete() {
        let d = tmp("ops");
        mkdir(&d.join("src/sub")).unwrap();
        fs::write(d.join("src/sub/f"), "x").unwrap();
        mkdir(&d.join("dst")).unwrap();
        assert_eq!(copy(&d.join("src"), &d.join("dst")).unwrap(), d.join("dst/src"));
        assert_eq!(fs::read_to_string(d.join("dst/src/sub/f")).unwrap(), "x");
        assert!(copy(&d.join("src"), &d.join("src/sub")).is_err());
        // Copying a file onto itself or over another file is refused, not truncated.
        assert!(copy(&d.join("src/sub/f"), &d.join("src/sub")).is_err());
        assert_eq!(fs::read_to_string(d.join("src/sub/f")).unwrap(), "x");
        rename(&d.join("src"), &d.join("moved")).unwrap();
        assert!(!d.join("src").exists() && d.join("moved/sub/f").exists());
        fs::write(d.join("dst/moved"), "").unwrap();
        assert!(rename(&d.join("moved"), &d.join("dst")).is_err());
        // Renamed onto itself (as a case change is where case does not count): nothing lost.
        assert_eq!(rename(&d.join("moved"), &d.join("moved")).unwrap(), d.join("moved"));
        assert!(d.join("moved/sub/f").exists());
        assert_eq!(dir_size(&d.join("moved")), (1, 1));
        fs::write(d.join("a.txt"), "").unwrap();
        assert_eq!(free_name(&d, "a.txt"), d.join("a (2).txt"));
        assert_eq!(free_name(&d, "b.txt"), d.join("b.txt"));
        assert_eq!(free_name(&d, "moved"), d.join("moved (2)"));
        let scratch = Scratch::new().unwrap();
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&fs::metadata(&scratch.0).unwrap().permissions()) & 0o777, 0o700);
        let kept = scratch.0.clone();
        drop(scratch);
        assert!(!kept.exists());
        delete(&d.join("moved")).unwrap();
        assert!(!d.join("moved").exists());
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
        assert_eq!((mode(&d), mode(&d.join("previews")), mode(&d.join("index.bin"))), (0o700, 0o700, 0o600));
        fs::remove_dir_all(d).unwrap();
    }
}

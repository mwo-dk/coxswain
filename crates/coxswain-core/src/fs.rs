//! Directory listing, sorting and file operations.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug, Serialize)]
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

    fn from_path(path: PathBuf, name: String) -> io::Result<Entry> {
        let lmeta = fs::symlink_metadata(&path)?;
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
    if !dir.is_dir()
        && let Some((archive, inner)) = crate::archive::split(dir)
    {
        let (mut all, locked) = crate::archive::listing(&archive, &inner)?;
        all.retain(|e| show_hidden || !e.hidden || e.is_parent());
        return Ok((all, Some((archive, locked))));
    }
    let mut out = Vec::new();
    if let Some(parent) = dir.parent() {
        out.push(Entry {
            name: "..".into(),
            path: parent.to_path_buf(),
            is_dir: true,
            is_symlink: false,
            is_exec: false,
            hidden: false,
            size: 0,
            modified: 0,
            created: 0,
            online: false,
        });
    }
    for de in fs::read_dir(dir)? {
        let de = de?;
        let name = de.file_name().to_string_lossy().into_owned();
        // Entries can vanish between readdir and stat; skip them.
        if let Ok(e) = Entry::from_path(de.path(), name)
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

/// A history is read-only: nothing goes into it, nothing is made or taken out there.
fn writable(path: &Path) -> io::Result<()> {
    match crate::history::split(path) {
        Some(_) if !path.exists() => Err(io::Error::new(io::ErrorKind::PermissionDenied, crate::t!("history.read_only"))),
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
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "cannot copy a directory into itself"));
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
    trash::delete(path).map_err(io::Error::other)
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

/// Total bytes and file count under `path`, in parallel. Symlinks are counted, not followed;
/// unreadable parts are skipped.
pub fn dir_size(path: &Path) -> (u64, u64) {
    use rayon::prelude::*;
    let Ok(meta) = fs::symlink_metadata(path) else { return (0, 0) };
    if !meta.is_dir() {
        return (meta.len(), 1);
    }
    let Ok(rd) = fs::read_dir(path) else { return (0, 0) };
    rd.flatten()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|de| match de.file_type() {
            Ok(t) if t.is_dir() => dir_size(&de.path()),
            _ => (de.metadata().map_or(0, |m| m.len()), 1),
        })
        .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
}

/// Open with the desktop's default application, detached.
pub fn open_default(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows_open(path)
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
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

#[cfg(test)]
mod tests {
    use super::*;

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
//! Directory listing, sorting and file operations.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_exec: bool,
    pub hidden: bool,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: u64,
    /// Seconds since the Unix epoch; 0 where the file system does not record it.
    pub created: u64,
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

/// List a directory. The first entry is `..` unless `dir` is a root.
pub fn list(dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
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
        });
    }
    for de in fs::read_dir(dir)? {
        let de = de?;
        let name = de.file_name().to_string_lossy().into_owned();
        // Entries can vanish between readdir and stat; skip them.
        if let Ok(e) = Entry::from_path(de.path(), name) {
            if show_hidden || !e.hidden {
                out.push(e);
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Name,
    Ext,
    Time,
    Size,
}

/// `..` first, then directories, then files; each group ordered by `key`.
pub fn sort(entries: &mut [Entry], key: SortKey, reverse: bool) {
    entries.sort_by(|a, b| {
        let group = |e: &Entry| (!e.is_parent(), !e.is_dir);
        group(a).cmp(&group(b)).then_with(|| {
            let name = || natord(&a.name, &b.name);
            let ord = match key {
                SortKey::Name => name(),
                SortKey::Ext => natord(a.ext(), b.ext()).then_with(name),
                SortKey::Time => b.modified.cmp(&a.modified).then_with(name),
                SortKey::Size => b.size.cmp(&a.size).then_with(name),
            };
            if reverse { ord.reverse() } else { ord }
        })
    });
}

/// Case-insensitive natural order: "file2" < "file10".
fn natord(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let num = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.next_if(|c| c.is_ascii_digit()) {
                        s.push(c);
                    }
                    s
                };
                let (na, nb) = (num(&mut a), num(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord.is_ne() {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord.is_ne() {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Where `src` lands when copied or moved to `dst`: inside it if `dst` is a directory.
pub fn target(src: &Path, dst: &Path) -> PathBuf {
    match (dst.is_dir(), src.file_name()) {
        (true, Some(name)) => dst.join(name),
        _ => dst.to_path_buf(),
    }
}

/// Copy a file, symlink or directory tree. Returns the created path.
pub fn copy(src: &Path, dst: &Path) -> io::Result<PathBuf> {
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
    let to = target(src, dst);
    if to.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", to.display())));
    }
    if fs::rename(src, &to).is_err() {
        copy(src, &to)?;
        delete(src)?;
    }
    Ok(to)
}

/// Delete a file, symlink (not its target) or directory tree.
pub fn delete(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Move to the desktop's trash (Recycle Bin on Windows).
pub fn trash(path: &Path) -> io::Result<()> {
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
    let opener: &[&str] = if cfg!(target_os = "macos") {
        &["open"]
    } else if cfg!(windows) {
        &["cmd", "/C", "start", ""]
    } else {
        &["xdg-open"]
    };
    std::process::Command::new(opener[0])
        .args(&opener[1..])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bosum-test-{name}-{}", std::process::id()));
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
        assert_eq!(dir_size(&d.join("moved")), (1, 1));
        fs::write(d.join("a.txt"), "").unwrap();
        assert_eq!(free_name(&d, "a.txt"), d.join("a (2).txt"));
        assert_eq!(free_name(&d, "b.txt"), d.join("b.txt"));
        assert_eq!(free_name(&d, "moved"), d.join("moved (2)"));
        delete(&d.join("moved")).unwrap();
        assert!(!d.join("moved").exists());
        fs::remove_dir_all(d).unwrap();
    }
}

//! A trash of our own for Termux, where the `trash` crate does not build: the freedesktop.org
//! layout in `$XDG_DATA_HOME/Trash` (`~/.local/share/Trash`), `files/<name>` with its
//! `info/<name>.trashinfo`. Files are only renamed in, never copied: one on another filesystem
//! (the phone's storage under `~/storage`) is refused, and the caller asks to delete it for good.

#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::fs;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// `$XDG_DATA_HOME/Trash`, else `~/.local/share/Trash`.
pub fn dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::home_dir().map(|h| h.join(".local/share")))
        .map(|d| d.join("Trash"))
}

/// Whether `path` is on the trash's filesystem, so it can be renamed in.
pub fn takes(path: &Path) -> bool {
    dir().is_some_and(|t| same_fs(&t, path))
}

/// Move `path` into the trash.
pub fn put(path: &Path) -> io::Result<()> {
    let trash = dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home folder"))?;
    put_in(&trash, path, same_fs).map(|_| ())
}

/// The same device as `path` itself (a link is where it sits) and the trash, or the nearest
/// folder of it that exists.
fn same_fs(trash: &Path, path: &Path) -> bool {
    let dev = |m: fs::Metadata| m.dev();
    let t = trash.ancestors().find_map(|a| fs::metadata(a).ok()).map(dev);
    t.is_some() && t == fs::symlink_metadata(path).ok().map(dev)
}

/// Move `path` into `trash` and give its name there; `same` says whether a rename can reach it.
fn put_in(trash: &Path, path: &Path, same: fn(&Path, &Path) -> bool) -> io::Result<String> {
    let path = std::path::absolute(path)?;
    fs::symlink_metadata(&path)?;
    if !same(trash, &path) {
        return Err(io::Error::new(io::ErrorKind::CrossesDevices, "the trash is on another filesystem"));
    }
    let (files, info) = (trash.join("files"), trash.join("info"));
    fs::create_dir_all(&files)?;
    fs::create_dir_all(&info)?;
    let name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no name"))?.to_string_lossy().into_owned();
    let body = format!("[Trash Info]\nPath={}\nDeletionDate={}\n", encode(&path), chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"));
    // The info file is made first and exclusively: it claims the name.
    for n in crate::fs::name_candidates(&name) {
        if fs::symlink_metadata(files.join(&n)).is_ok() {
            continue;
        }
        let i = info.join(format!("{n}.trashinfo"));
        let mut f = match fs::OpenOptions::new().write(true).create_new(true).open(&i) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            r => r?,
        };
        let moved = f.write_all(body.as_bytes()).and_then(|_| fs::rename(&path, files.join(&n)));
        return match moved {
            Ok(()) => Ok(n),
            Err(e) => {
                let _ = fs::remove_file(&i);
                Err(e)
            }
        };
    }
    unreachable!("some name is free")
}

/// The path as the spec's URI escaping: unreserved characters and `/` stay, every other byte
/// is `%XX`.
fn encode(path: &Path) -> String {
    let mut s = String::new();
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-test-trash-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn info(trash: &Path, n: &str) -> String {
        fs::read_to_string(trash.join("info").join(format!("{n}.trashinfo"))).unwrap()
    }

    #[test]
    fn moves_in_with_info_and_a_free_name_on_a_clash() {
        let d = tmp("clash");
        let trash = d.join("Trash");
        for _ in 0..2 {
            fs::write(d.join("a b.txt"), "x").unwrap();
            put_in(&trash, &d.join("a b.txt"), same_fs).unwrap();
        }
        assert!(!d.join("a b.txt").exists());
        assert!(trash.join("files/a b.txt").exists() && trash.join("files/a b (2).txt").exists());
        let i = info(&trash, "a b (2).txt");
        assert!(i.starts_with(&format!("[Trash Info]\nPath={}/a%20b.txt\nDeletionDate=", encode(&d))), "{i}");
        let date = i.lines().nth(2).unwrap().strip_prefix("DeletionDate=").unwrap();
        assert!(chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%S").is_ok(), "{date}");
        // A folder goes in whole.
        fs::create_dir_all(d.join("dir/sub")).unwrap();
        assert_eq!(put_in(&trash, &d.join("dir"), same_fs).unwrap(), "dir");
        assert!(trash.join("files/dir/sub").is_dir());
    }

    #[test]
    fn encodes_what_a_uri_cannot_hold() {
        assert_eq!(encode(Path::new("/home/ø 100%/#a~b-c_d.e")), "/home/%C3%B8%20100%25/%23a~b-c_d.e");
    }

    #[test]
    fn another_filesystem_is_refused_and_left_alone() {
        let d = tmp("device");
        let f = d.join("photo.jpg");
        fs::write(&f, "x").unwrap();
        let e = put_in(&d.join("Trash"), &f, |_, _| false).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::CrossesDevices);
        assert!(f.exists() && !d.join("Trash").exists());
        assert!(same_fs(&d.join("Trash/not/yet"), &f));
    }
}

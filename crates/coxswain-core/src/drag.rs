//! Drag and drop in the desktop app: what a drag out hands to other applications, and what a
//! drop does by the keys held.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Most a drag unpacks by itself out of an archive or a git history; more is copied with F5.
pub const MAX_BYTES: u64 = 200 * 1024 * 1024;
pub const MAX_FILES: usize = 2000;

/// The last drag's paths, what they were copied out to, and when, so dragging the same again
/// soon takes it as it is.
type Last = (Vec<PathBuf>, Vec<PathBuf>, Instant);
static LAST: Mutex<Option<Last>> = Mutex::new(None);

/// Files on disk for a drag out to other applications: `paths` themselves where they are on
/// disk, and what is only inside an archive (a disk image, a mailbox …) or a git history copied
/// out into a fresh folder of this run's cache, as F5 would. The folders go when the app closes
/// (`done`), or after a day. Too much, or a file locked with a password not given yet, is
/// refused: F5 copies those, and asks for the password.
pub fn out(paths: &[PathBuf]) -> io::Result<Vec<PathBuf>> {
    let virt = |p: &PathBuf| fs::symlink_metadata(p).is_err();
    if !paths.iter().any(virt) {
        return Ok(paths.to_vec());
    }
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((from, to, at)) = last.as_ref()
        && from == paths
        && at.elapsed() < Duration::from_secs(60)
        && to.iter().all(|p| p.exists())
    {
        return Ok(to.clone());
    }
    let (mut bytes, mut files) = (0, 0);
    for p in paths.iter().filter(|p| virt(p)) {
        let (b, n) = measure(p)?;
        bytes += b;
        files += n;
    }
    if bytes > MAX_BYTES || files > MAX_FILES {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            crate::t!("drag.too_much"),
        ));
    }
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = crate::archive::run_folder("drag", false)?.join(
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .to_string(),
    );
    // One a crashed run with the same process id may have left.
    let _ = fs::remove_dir_all(&dir);
    crate::fs::private_dir(&dir)?;
    let to = paths
        .iter()
        .map(|p| {
            if virt(p) {
                crate::fs::copy_locked(p, &dir, None)
            } else {
                Ok(p.clone())
            }
        })
        .collect::<io::Result<Vec<_>>>()?;
    *last = Some((paths.to_vec(), to.clone(), Instant::now()));
    Ok(to)
}

/// What copying `p` (in a history or an archive) out takes: bytes and files.
fn measure(p: &Path) -> io::Result<(u64, usize)> {
    if let Some(at) = crate::history::split(p) {
        return crate::history::measure(&at);
    }
    let (archive, inner) = crate::archive::split(p)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, p.display().to_string()))?;
    let (bytes, files, locked) = crate::archive::measure(&archive, &inner)?;
    if locked {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            crate::t!("drag.locked"),
        ));
    }
    Ok((bytes, files))
}

/// This run's drag folders, when the app closes.
pub fn done() {
    if let Some(f) = crate::helper::folder() {
        let _ = fs::remove_dir_all(f.join("drag").join(std::process::id().to_string()));
    }
}

/// What a drop does by the keys held at the drop: `Some(false)` copies, `Some(true)` moves, and
/// `None` asks. As other file managers have it: Ctrl copies and Shift moves (on macOS Option
/// copies and Command moves); both together ask.
pub fn by_keys(copy: bool, mv: bool) -> Option<bool> {
    match (copy, mv) {
        (true, false) => Some(false),
        (false, true) => Some(true),
        _ => None,
    }
}

/// Why nothing can be dropped into `dir` (the key of the message), if it is read-only: a git
/// history, a ZFS snapshot, a package's list of files, a disk image.
pub fn read_only(dir: &Path) -> Option<&'static str> {
    if crate::history::is_history(dir) {
        Some("history.read_only")
    } else if crate::zfs::is_read_only(dir) {
        Some("zfs.read_only")
    } else if dir.ancestors().any(|a| crate::bsd::split(a).is_some()) {
        Some("pkg.read_only")
    } else if crate::archive::split(dir).is_some_and(|(a, _)| crate::archive::read_only(&a)) {
        Some("disk_image.read_only")
    } else {
        None
    }
}

/// The files a terminal typed in for a drop, or pasted: one or more absolute paths that exist,
/// each as the terminal writes it (`'quoted'`, `"quoted"`, with `\ ` before a space, or a
/// `file://` URI), apart by spaces or lines. Anything else is no drop.
pub fn dropped(text: &str) -> Option<Vec<PathBuf>> {
    let mut words = vec![];
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = text.trim().chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            // Windows paths are made of backslashes.
            (None, '\\') if !cfg!(windows) => word.extend(chars.next()),
            (None, c) if c.is_whitespace() => {
                if started || !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                started = false;
            }
            (None, c) => word.push(c),
        }
    }
    if quote.is_some() {
        return None;
    }
    if started || !word.is_empty() {
        words.push(word);
    }
    let paths: Vec<PathBuf> = words
        .into_iter()
        .map(|w| match w.strip_prefix("file://") {
            Some(rest) => PathBuf::from(percent_decode(rest)),
            None => PathBuf::from(w),
        })
        .collect();
    (!paths.is_empty()
        && paths
            .iter()
            .all(|p| p.is_absolute() && fs::symlink_metadata(p).is_ok()))
    .then_some(paths)
}

/// `%20` and the like as the bytes they stand for.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (
            b[i],
            b.get(i + 1).copied().and_then(hex),
            b.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn keys_pick_copy_or_move_and_both_ask() {
        assert_eq!(by_keys(true, false), Some(false));
        assert_eq!(by_keys(false, true), Some(true));
        assert_eq!(by_keys(true, true), None);
        assert_eq!(by_keys(false, false), None);
    }

    #[test]
    fn a_terminal_drop_is_read_in_each_way_terminals_write_it() {
        let d = scratch("drag-dropped");
        let a = d.join("a b.txt");
        let c = d.join("c");
        fs::write(&a, "a").unwrap();
        fs::write(&c, "c").unwrap();
        let (ad, cd) = (a.display().to_string(), c.display().to_string());
        let want = Some(vec![a.clone(), c.clone()]);
        assert_eq!(dropped(&format!("'{ad}' '{cd}' ")), want);
        assert_eq!(dropped(&format!("\"{ad}\"\n{cd}")), want);
        if !cfg!(windows) {
            assert_eq!(dropped(&format!("{} {cd}", ad.replace(' ', "\\ "))), want);
            assert_eq!(
                dropped(&format!("file://{} {cd}", ad.replace(' ', "%20"))),
                want
            );
        }
        assert_eq!(dropped("hello world"), None);
        assert_eq!(dropped(&format!("{cd} missing-file")), None);
        assert_eq!(dropped(&format!("'{cd}")), None);
        assert_eq!(dropped(""), None);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn files_on_disk_are_dragged_as_they_are() {
        let d = scratch("drag-on-disk");
        let f = d.join("a.txt");
        fs::write(&f, "a").unwrap();
        assert_eq!(out(std::slice::from_ref(&f)).unwrap(), vec![f]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_in_a_zip_is_copied_out_under_its_name() {
        let d = scratch("drag-zip");
        let src = d.join("b c#1 50%.txt");
        fs::write(&src, "inside").unwrap();
        let zip = d.join("x.zip");
        crate::archive::create(&zip, std::slice::from_ref(&src)).unwrap();
        let got = out(&[zip.join("b c#1 50%.txt")]).unwrap();
        assert_eq!(got[0].file_name().unwrap(), "b c#1 50%.txt");
        assert_eq!(fs::read_to_string(&got[0]).unwrap(), "inside");
        // The same drag again soon takes the same copy.
        assert_eq!(out(&[zip.join("b c#1 50%.txt")]).unwrap(), got);
        let _ = fs::remove_dir_all(&d);
    }
}

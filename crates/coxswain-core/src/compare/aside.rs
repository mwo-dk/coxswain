//! What a folder sync set aside (its `.coxswain-sync-…` folder) once its run has ended
//! (docs/files/compare-folders.md#where-did-my-replaced-files-go): a run notes the folder when
//! it makes it, and forgets it once the folder went to the bin. A run that was killed, crashed
//! or lost its power, and a run whose bin could not take the folder (Windows, a disk without a
//! bin of its own), leave the note; the next start of either app or the search helper sends
//! that folder to the bin as a finished run would, or where the bin cannot take it keeps it,
//! listed in Disk use with Remove. Either way a notice says so, once.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The notes and what came of them: `sync-aside/` in the cache folder.
fn dir() -> Option<PathBuf> {
    Some(crate::helper::folder()?.join("sync-aside"))
}

/// A path as the hex of its own bytes, so a name that is not UTF-8 comes back as it was.
fn enc(p: &Path) -> String {
    #[cfg(unix)]
    let b: Vec<u8> = std::os::unix::ffi::OsStrExt::as_bytes(p.as_os_str()).to_vec();
    #[cfg(windows)]
    let b: Vec<u8> = std::os::windows::ffi::OsStrExt::encode_wide(p.as_os_str()).flat_map(u16::to_le_bytes).collect();
    #[cfg(not(any(unix, windows)))]
    let b: Vec<u8> = p.to_string_lossy().into_owned().into_bytes();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn dec(s: &str) -> Option<PathBuf> {
    let b: Vec<u8> = (0..s.len()).step_by(2).map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok())).collect::<Option<_>>()?;
    #[cfg(unix)]
    return Some(PathBuf::from(<std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(b)));
    #[cfg(windows)]
    return Some(PathBuf::from(<std::ffi::OsString as std::os::windows::ffi::OsStringExt>::from_wide(
        &b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>(),
    )));
    #[cfg(not(any(unix, windows)))]
    Some(PathBuf::from(String::from_utf8(b).ok()?))
}

/// The run made the folder aside `aside`: noted, named by the run's id as its other folders are
/// (`spill::in_use`), so the next start knows when the run has ended. The note, to give `done`.
pub(crate) fn note(aside: &Path) -> Option<PathBuf> {
    note_in(&dir()?, aside)
}

fn note_in(d: &Path, aside: &Path) -> Option<PathBuf> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    crate::fs::private_dir(d).ok()?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.subsec_nanos());
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let f = d.join(format!("{}-{n}-{nanos:x}", std::process::id()));
    std::fs::write(&f, enc(aside)).ok()?;
    Some(f)
}

/// The run is through with its folder aside: it went to the bin, or held nothing.
pub(crate) fn done(note: Option<&Path>) {
    if let Some(n) = note {
        let _ = std::fs::remove_file(n);
    }
}

/// A folder aside of a run that ended, as the next start found it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Left {
    /// The folder aside, as `enc` has it.
    aside: String,
    pub files: u64,
    pub bytes: u64,
    /// It went to the bin; else it is still where it was.
    pub binned: bool,
}

impl Left {
    /// The folder aside, at the target's top (where it was, when it went to the bin).
    pub fn path(&self) -> PathBuf {
        dec(&self.aside).unwrap_or_default()
    }

    /// The folder that was synced: the one the folder aside is in.
    pub fn folder(&self) -> PathBuf {
        self.path().parent().map(Path::to_path_buf).unwrap_or_default()
    }

    /// The id of its notice, and of its row in Disk use.
    pub fn id(&self) -> String {
        format!("sync-aside:{}", self.aside)
    }
}

const RECORDS: &str = "left.json";

fn load(d: &Path) -> Vec<Left> {
    std::fs::read(d.join(RECORDS)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(d: &Path, v: &[Left]) {
    let tmp = d.join(format!("{RECORDS}.{}", std::process::id()));
    if std::fs::write(&tmp, serde_json::to_vec(v).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, d.join(RECORDS));
    }
}

/// Held while the notes and records change: two starts at once never take one note twice.
fn locked(d: &Path) -> Option<std::fs::File> {
    let f = std::fs::OpenOptions::new().create(true).read(true).write(true).truncate(false).open(d.join("lock")).ok()?;
    f.lock().ok()?;
    Some(f)
}

/// What the runs that ended left, as the notices and Disk use show it: those that went to the
/// bin, and those still in place.
pub fn left() -> Vec<Left> {
    dir().map(|d| load(&d)).unwrap_or_default()
}

/// The bin takes `p` as a finished run sends it: never on Windows (whose bin goes by path), and
/// only by a move.
fn bin(p: &Path) -> bool {
    !cfg!(windows) && crate::fs::trash_takes(&[p.to_path_buf()]) && crate::fs::trash_renames(p) && crate::fs::trash_named(p, None).is_ok()
}

/// At start: the folder aside of each run that ended, to the bin (or kept where the bin cannot
/// take it), and the notices of those already dismissed forgotten.
pub fn sweep() {
    let Some(d) = dir().filter(|d| d.is_dir()) else { return };
    let dismissed = crate::state::AppState::load().notices_dismissed;
    sweep_in(&d, bin, &dismissed);
}

fn sweep_in(d: &Path, bin: impl Fn(&Path) -> bool, dismissed: &[String]) {
    let Some(_held) = locked(d) else { return };
    let mut v = load(d);
    let before = v.clone();
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        let p = e.path();
        // Notes only (named by a run's id), of runs that ended.
        if !e.file_name().to_string_lossy().starts_with(|c: char| c.is_ascii_digit()) || crate::spill::in_use(&p) {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let _ = std::fs::remove_file(&p);
        let Some(aside) = dec(text.trim()) else { continue };
        // Only a folder of a run's own name, never through a link.
        let ours = aside.file_name().is_some_and(|n| n.to_string_lossy().starts_with(super::sync::ASIDE));
        if !ours || !std::fs::symlink_metadata(&aside).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        if std::fs::read_dir(&aside).is_ok_and(|mut r| r.next().is_none()) {
            let _ = std::fs::remove_dir(&aside);
            continue;
        }
        let (bytes, files) = crate::fs::dir_size(&aside);
        let mut l = Left { aside: enc(&aside), files, bytes, binned: false };
        l.binned = bin(&aside);
        v.retain(|o| o.aside != l.aside);
        v.push(l);
    }
    // Forgotten: what went to the bin once its notice was dismissed, and a kept folder no longer
    // there (removed in Disk use, or by hand).
    v.retain(|l| if l.binned { !dismissed.contains(&l.id()) } else { std::fs::symlink_metadata(l.path()).is_ok_and(|m| m.is_dir()) });
    if v != before {
        save(d, &v);
    }
}

/// Remove a kept folder aside for good (Disk use's Remove): only one the records name, of a
/// run's own name, never through a link.
pub fn remove(path: &Path) -> std::io::Result<()> {
    let d = dir().ok_or_else(|| std::io::Error::other(crate::t!("err.no_cache_folder")))?;
    let _held = locked(&d);
    let mut v = load(&d);
    let known = v.iter().any(|l| !l.binned && l.path() == path);
    let ours = path.file_name().is_some_and(|n| n.to_string_lossy().starts_with(super::sync::ASIDE));
    if !known || !ours || !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, path.display().to_string()));
    }
    std::fs::remove_dir_all(path)?;
    v.retain(|l| l.path() != path);
    save(&d, &v);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-aside-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_path_comes_back_as_it_was() {
        #[cfg(unix)]
        let p = PathBuf::from(<std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(b"/t/\xff\xfe odd".to_vec()));
        #[cfg(not(unix))]
        let p = PathBuf::from("C:\\t\\odd ø");
        assert_eq!(dec(&enc(&p)), Some(p));
    }

    /// The crash case: a run noted its folder aside and died (no list of its own holds it). The
    /// next start sends the folder to the bin, or keeps it where the bin cannot take it; a
    /// note of a live run (this one) is left; a dismissed notice is forgotten.
    #[test]
    fn an_ended_runs_folder_aside_goes_to_the_bin_or_is_kept_and_said() {
        let d = folder("sweep");
        let notes = d.join("notes");
        let (binned, kept, mine, empty) = (d.join(".coxswain-sync-1-4000011-a"), d.join(".coxswain-sync-1-4000012-b"), d.join(".coxswain-sync-1-mine"), d.join(".coxswain-sync-1-4000013-c"));
        for a in [&binned, &kept, &mine] {
            std::fs::create_dir_all(a.join("sub")).unwrap();
            std::fs::write(a.join("0-a.txt"), "old").unwrap();
            std::fs::write(a.join("sub/1-b.txt"), "older").unwrap();
        }
        std::fs::create_dir(&empty).unwrap();
        // The user's own folder, named in a note: never touched.
        let user = d.join("photos");
        std::fs::create_dir(&user).unwrap();
        crate::fs::private_dir(&notes).unwrap();
        // Ids no real process of this test has, with no list: ended (their notes a day old).
        for (n, a) in [(11, &binned), (12, &kept), (13, &empty), (14, &user)] {
            let f = notes.join(format!("40000{n}-0-1"));
            std::fs::write(&f, enc(a)).unwrap();
            std::fs::File::options().write(true).open(&f).unwrap().set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 3600)).unwrap();
        }
        let live = note_in(&notes, &mine).unwrap();
        sweep_in(&notes, |p| p == binned && std::fs::remove_dir_all(p).is_ok(), &[]);
        let v = load(&notes);
        assert_eq!(v.len(), 2, "{v:?}");
        let b = v.iter().find(|l| l.binned).unwrap();
        assert_eq!((b.path(), b.folder(), b.files), (binned.clone(), d.clone(), 2));
        let k = v.iter().find(|l| !l.binned).unwrap();
        assert_eq!((k.path(), k.files, k.bytes), (kept.clone(), 2, 8));
        assert!(kept.join("sub/1-b.txt").exists() && user.exists() && !empty.exists());
        assert!(live.exists() && mine.join("0-a.txt").exists(), "a live run's: left");
        // Its notice dismissed, the binned one is forgotten; the kept one stays while it is there.
        sweep_in(&notes, |_| false, &[b.id()]);
        assert_eq!(load(&notes), vec![k.clone()]);
        std::fs::remove_dir_all(&kept).unwrap();
        sweep_in(&notes, |_| false, &[]);
        assert!(load(&notes).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}

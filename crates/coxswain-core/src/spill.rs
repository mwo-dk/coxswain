//! What Coxswain leaves on the disk by itself, kept in bounds: the previews no one looked at for
//! a month go, and the passing names a copy writes beside its destination
//! (`.name.coxswain-tmp-…`, a disk image's `.name.<pid>.coxswain-tmp`, the editor's
//! `.name.coxswain-<pid>-….tmp`) are noted, so the next start removes what a run that was killed
//! or crashed left in the user's folders.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// A preview folder not shown for this long goes (`prune_previews`).
pub const PREVIEWS_KEPT: Duration = Duration::from_secs(30 * 24 * 3600);
/// The most the preview cache keeps: past it, the folders shown longest ago go first.
pub const PREVIEWS_MOST: u64 = 4 << 30;
/// A folder shown again is marked used at most this often, so thumbnails write nothing on each look.
const MARK_EVERY: Duration = Duration::from_secs(24 * 3600);
/// `prune_previews` looks through the folder at most this often in one run.
const PRUNE_EVERY: Duration = Duration::from_secs(6 * 3600);
/// The file that says a preview is being made: one there means the run making it never ended
/// (killed, the power gone), so what is beside it is not a whole preview.
pub const MAKING: &str = ".making";

/// The preview cache: `previews/` in the cache folder.
pub fn previews_dir() -> Option<PathBuf> {
    Some(crate::helper::folder()?.join("previews"))
}

/// The preview folder `dir` was shown now: its date moves (by a file made and removed in it, which
/// works on every system), so `prune_previews` keeps it. Nothing when it is not there.
pub fn used(dir: &Path) {
    let old = std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().unwrap_or_default() > MARK_EVERY);
    if old {
        let mark = dir.join(".used");
        if std::fs::File::create(&mark).is_ok() {
            let _ = std::fs::remove_file(&mark);
        }
    }
}

/// `dir` is being made: `.making` in it until the guard goes, so a run cut short is made again
/// the next time, never shown half written.
pub struct Making(PathBuf);

impl Making {
    pub fn new(dir: &Path) -> Making {
        let _ = std::fs::write(dir.join(MAKING), "");
        Making(dir.join(MAKING))
    }
    /// Marked again, after its folder was emptied and made anew.
    pub fn again(&self) {
        let _ = std::fs::write(&self.0, "");
    }
}

impl Drop for Making {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A run that made a preview in `dir` never ended.
pub fn half_made(dir: &Path) -> bool {
    dir.join(MAKING).exists()
}

/// The preview folders not shown for `PREVIEWS_KEPT`, removed; at most every `PRUNE_EVERY` in a
/// run. A folder being made now has today's date, so it stays.
pub fn prune_previews() {
    static LAST: Mutex<Option<SystemTime>> = Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| t.elapsed().unwrap_or_default() < PRUNE_EVERY) {
            return;
        }
        *last = Some(SystemTime::now());
    }
    if let Some(dir) = previews_dir() {
        prune_in(&dir, PREVIEWS_KEPT, PREVIEWS_MOST);
    }
}

/// The folders in `dir` not shown for `age` removed, then the ones shown longest ago until all
/// of them take no more than `most`.
fn prune_in(dir: &Path, age: Duration, most: u64) {
    let mut kept: Vec<(SystemTime, u64, PathBuf)> = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(m) = std::fs::symlink_metadata(e.path()) else {
            continue;
        };
        let Ok(when) = m.modified() else { continue };
        if !m.is_dir() {
            continue;
        }
        if when.elapsed().unwrap_or_default() > age {
            let _ = std::fs::remove_dir_all(e.path());
        } else {
            kept.push((when, crate::fs::dir_size(&e.path()).0, e.path()));
        }
    }
    let mut total: u64 = kept.iter().map(|k| k.1).sum();
    kept.sort();
    for (_, bytes, path) in kept {
        if total <= most {
            break;
        }
        if std::fs::remove_dir_all(&path).is_ok() {
            total = total.saturating_sub(bytes);
        }
    }
}

// ---------------------------------------------------------------- passing names beside copies

/// This run's list of the folders it wrote passing names in, `staging/<pid>` in the cache, held
/// locked while the run lives: a list no one holds is a run that ended.
struct Journal {
    file: std::fs::File,
    seen: std::collections::HashSet<PathBuf>,
}

static JOURNAL: Mutex<Option<Journal>> = Mutex::new(None);

fn staging_dir() -> Option<PathBuf> {
    Some(crate::helper::folder()?.join("staging"))
}

/// A passing name is about to be written in `dir`: noted once per folder in this run, so a crash
/// leaves a list of where to look. Folders inside a passing one go with it and are not noted.
pub(crate) fn staging_in(dir: &Path) {
    if dir
        .components()
        .any(|c| ours(&c.as_os_str().to_string_lossy(), None))
    {
        return;
    }
    let mut j = JOURNAL.lock().unwrap_or_else(|e| e.into_inner());
    if j.is_none() {
        *j = open_journal();
    }
    let Some(j) = j.as_mut() else { return };
    if j.seen.insert(dir.to_path_buf()) {
        let line = serde_json::to_string(&dir.to_string_lossy()).unwrap_or_default();
        let _ = writeln!(j.file, "{line}");
    }
}

fn open_journal() -> Option<Journal> {
    let dir = staging_dir()?;
    crate::fs::private_dir(&dir).ok()?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(std::process::id().to_string()))
        .ok()?;
    file.try_lock().ok()?;
    Some(Journal {
        file,
        seen: Default::default(),
    })
}

/// A passing name: a copy's `.name.coxswain-tmp-<pid>-…` (a file or a folder), a disk image's
/// `.name.<pid>.coxswain-tmp`, a save's `.name.coxswain-<pid>-….tmp`; with `pid`, only that run's.
fn ours(name: &str, pid: Option<&str>) -> bool {
    if !name.starts_with('.') {
        return false;
    }
    match pid {
        Some(pid) => {
            name.contains(&format!(".coxswain-tmp-{pid}-"))
                || name.ends_with(&format!(".{pid}.coxswain-tmp"))
                || (name.contains(&format!(".coxswain-{pid}-")) && name.ends_with(".tmp"))
        }
        None => name.contains(".coxswain-tmp-") || name.ends_with(".coxswain-tmp"),
    }
}

/// At start: what runs that ended without tidying (a crash, a kill, the power gone) left beside
/// their copies, removed: only names of `ours` with that run's id, in the folders its list names,
/// never through a link. A list a running app holds is left.
pub fn sweep_staging() {
    if let Some(dir) = staging_dir() {
        sweep_staging_in(&dir);
    }
}

fn sweep_staging_in(dir: &Path) {
    let me = std::process::id().to_string();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let pid = e.file_name().to_string_lossy().into_owned();
        if pid == me || pid.parse::<u32>().is_err() {
            continue;
        }
        let Ok(list) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(e.path())
        else {
            continue;
        };
        if list.try_lock().is_err() {
            continue;
        }
        let text = std::fs::read_to_string(e.path()).unwrap_or_default();
        for folder in text
            .lines()
            .filter_map(|l| serde_json::from_str::<String>(l).ok())
        {
            for left in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                if !ours(&left.file_name().to_string_lossy(), Some(&pid)) {
                    continue;
                }
                let _ = match std::fs::symlink_metadata(left.path()) {
                    Ok(m) if m.is_dir() => std::fs::remove_dir_all(left.path()),
                    Ok(m) if m.is_file() => std::fs::remove_file(left.path()),
                    _ => Ok(()),
                };
            }
        }
        drop(list);
        let _ = std::fs::remove_file(e.path());
    }
}

/// At each start of either app: both of the above.
pub fn at_start() {
    sweep_staging();
    prune_previews();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-spill-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A folder's date set back (unix: Windows cannot open a folder for it, so those tests are unix's).
    #[cfg(unix)]
    fn age(p: &Path, days: u64) {
        let t = SystemTime::now() - Duration::from_secs(days * 24 * 3600);
        std::fs::File::open(p)
            .and_then(|f| f.set_modified(t))
            .unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn previews_not_shown_for_a_month_go_and_used_ones_stay() {
        let d = folder("prune");
        for n in ["old", "shown", "new"] {
            std::fs::create_dir(d.join(n)).unwrap();
            std::fs::write(d.join(n).join("out.pdf"), "x").unwrap();
        }
        age(&d.join("old"), 40);
        age(&d.join("shown"), 40);
        used(&d.join("shown"));
        prune_in(&d, PREVIEWS_KEPT, PREVIEWS_MOST);
        assert!(!d.join("old").exists());
        assert!(
            d.join("shown/out.pdf").exists(),
            "shown again: its date moved"
        );
        assert!(d.join("new/out.pdf").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Past the most it keeps, the folders shown longest ago go first.
    #[test]
    #[cfg(unix)]
    fn past_its_size_the_cache_drops_the_least_recently_shown() {
        let d = folder("cap");
        for (n, days) in [("a", 3), ("b", 2), ("c", 1)] {
            std::fs::create_dir(d.join(n)).unwrap();
            std::fs::write(d.join(n).join("out.pdf"), vec![1u8; 1000]).unwrap();
            age(&d.join(n), days);
        }
        prune_in(&d, PREVIEWS_KEPT, 2500);
        assert!(!d.join("a").exists(), "shown longest ago");
        assert!(d.join("b").exists() && d.join("c").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_preview_cut_short_is_half_made_until_its_guard_goes() {
        let d = folder("making");
        {
            let _m = Making::new(&d);
            assert!(half_made(&d));
        }
        assert!(!half_made(&d));
        // A kill: the guard never drops.
        std::mem::forget(Making::new(&d));
        assert!(half_made(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn only_passing_names_of_that_run_are_ours() {
        assert!(ours(".a.txt.coxswain-tmp-77-3", Some("77")));
        assert!(ours(".photos.coxswain-tmp-77-0-1f", Some("77")));
        assert!(ours(".disk.77.coxswain-tmp", Some("77")));
        assert!(!ours(".a.txt.coxswain-tmp-770-3", Some("77")));
        assert!(!ours(".a.txt.coxswain-tmp-78-3", Some("77")));
        assert!(
            !ours("a.txt.coxswain-tmp-77-3", Some("77")),
            "no leading dot: the user's"
        );
        assert!(!ours(".a.77.coxswain-tmp.txt", Some("77")));
        assert!(ours(".notes.txt.coxswain-77-0-1f.tmp", Some("77")));
        assert!(!ours(".notes.txt.coxswain-770-0-1f.tmp", Some("77")));
        assert!(!ours(".notes.txt.coxswain-77-0-1f", Some("77")));
    }

    /// The crash case: a run noted a folder and left a half copy there, then died (its list is
    /// not locked); the next start removes the half copy and the list, and nothing else.
    #[test]
    fn a_dead_runs_half_copies_are_removed_at_start() {
        let d = folder("sweep");
        let (target, lists) = (d.join("target"), d.join("staging"));
        std::fs::create_dir_all(target.join(".photos.coxswain-tmp-4242-0-a/sub")).unwrap();
        std::fs::write(target.join(".photos.coxswain-tmp-4242-0-a/sub/x.jpg"), "x").unwrap();
        std::fs::write(target.join(".a.txt.coxswain-tmp-4242-1"), "half").unwrap();
        std::fs::create_dir(target.join(".disk.4242.coxswain-tmp")).unwrap();
        // Another run's, a live one's and the user's own stay.
        std::fs::write(target.join(".a.txt.coxswain-tmp-4243-1"), "other").unwrap();
        std::fs::write(target.join("a.txt"), "mine").unwrap();
        std::fs::create_dir(&lists).unwrap();
        let line = serde_json::to_string(&target.to_string_lossy()).unwrap();
        std::fs::write(lists.join("4242"), format!("{line}\n")).unwrap();
        // A live run's list, held: left alone with what it names.
        let live = lists.join("4243");
        std::fs::write(&live, format!("{line}\n")).unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&live)
            .unwrap();
        held.lock().unwrap();
        sweep_staging_in(&lists);
        assert!(!target.join(".photos.coxswain-tmp-4242-0-a").exists());
        assert!(!target.join(".a.txt.coxswain-tmp-4242-1").exists());
        assert!(!target.join(".disk.4242.coxswain-tmp").exists());
        assert!(!lists.join("4242").exists());
        assert!(target.join(".a.txt.coxswain-tmp-4243-1").exists());
        assert!(target.join("a.txt").exists());
        assert!(live.exists());
        drop(held);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// What a run notes is what the next start reads: each folder once, as the sweep parses it.
    #[test]
    fn a_noted_folder_is_listed_once_in_this_runs_list() {
        // Quotes and a line break, where the system allows them in a name.
        let d = folder("noted").join(if cfg!(windows) { "odd name" } else { "odd \"name\"\nline" });
        std::fs::create_dir_all(&d).unwrap();
        staging_in(&d);
        staging_in(&d);
        staging_in(&d.join(".x.coxswain-tmp-1-0-a"));
        let list = staging_dir().unwrap().join(std::process::id().to_string());
        let text = std::fs::read_to_string(list).unwrap();
        let ours: Vec<String> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .filter(|l: &String| l.contains("coxswain-spill-noted"))
            .collect();
        assert_eq!(ours, vec![d.to_string_lossy().into_owned()], "{text}");
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    /// A link named like a passing name is removed as a link, never followed.
    #[test]
    #[cfg(unix)]
    fn a_link_with_our_name_is_never_followed() {
        let d = folder("link");
        let (target, lists, outside) = (d.join("target"), d.join("staging"), d.join("outside"));
        std::fs::create_dir_all(outside.join("keep")).unwrap();
        std::fs::write(outside.join("keep/f"), "x").unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&outside, target.join(".x.coxswain-tmp-4244-0-a")).unwrap();
        std::fs::create_dir(&lists).unwrap();
        let line = serde_json::to_string(&target.to_string_lossy()).unwrap();
        std::fs::write(lists.join("4244"), format!("{line}\n")).unwrap();
        sweep_staging_in(&lists);
        assert!(outside.join("keep/f").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}

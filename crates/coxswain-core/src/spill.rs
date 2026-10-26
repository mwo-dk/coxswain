//! What Coxswain leaves on the disk by itself, kept in bounds: the previews no one looked at for
//! a month go, and the passing names a copy writes beside its destination
//! (`.name.coxswain-tmp-…`, a disk image's `.name.<pid>.coxswain-tmp`, the editor's
//! `.name.coxswain-<pid>-….tmp`) are noted, so the next start removes what a run that was killed
//! or crashed left in the user's folders.

use std::io::{Read, Write};
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

/// `dir` is being made: `.making` in it until `whole` says the result is complete. A run cut
/// short (killed, the power gone) or one that failed leaves it, so what is there is made again
/// the next time, never shown as a whole preview.
pub fn begin(dir: &Path) {
    let _ = std::fs::write(dir.join(MAKING), "");
}

/// The preview in `dir` is complete: `begin`'s mark goes.
pub fn whole(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(MAKING));
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
/// of them take no more than `most`. Each is first renamed `.gone-…`, so a look meanwhile finds
/// none and makes it anew, never a folder half removed; a `.gone-…` a cut prune left goes too.
fn prune_in(dir: &Path, age: Duration, most: u64) {
    let mut kept: Vec<(SystemTime, u64, PathBuf)> = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(m) = std::fs::symlink_metadata(e.path()) else {
            continue;
        };
        if !m.is_dir() {
            continue;
        }
        if name.starts_with(".gone-") {
            let _ = std::fs::remove_dir_all(e.path());
            continue;
        }
        let Ok(when) = m.modified() else { continue };
        if when.elapsed().unwrap_or_default() > age {
            gone(&e.path());
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
        gone(&path);
        // Counted as gone even when it could not go, so no newer one goes in its stead.
        total = total.saturating_sub(bytes);
    }
}

/// A preview folder renamed out of the way, then removed.
fn gone(path: &Path) {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let aside = path.with_file_name(format!(".gone-{name}-{}", std::process::id()));
    if std::fs::rename(path, &aside).is_ok() {
        let _ = std::fs::remove_dir_all(&aside);
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
/// The list could not be made (no cache folder, a lock refused): not tried again for each file.
static NO_JOURNAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
    use std::sync::atomic::Ordering;
    if NO_JOURNAL.load(Ordering::Relaxed) {
        return;
    }
    let mut j = JOURNAL.lock().unwrap_or_else(|e| e.into_inner());
    if j.is_none() {
        *j = open_journal();
        if j.is_none() {
            NO_JOURNAL.store(true, Ordering::Relaxed);
        }
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
    // Readable too: Windows locks only a handle that may read or write the whole file.
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
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
        let Ok(mut list) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(e.path())
        else {
            continue;
        };
        if list.try_lock().is_err() {
            continue;
        }
        // Read through the handle that holds the lock: Windows refuses any other.
        let mut text = String::new();
        let _ = list.read_to_string(&mut text);
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
        // Removed while still held, so no run takes it up between.
        let _ = std::fs::remove_file(e.path());
        drop(list);
    }
}

// ---------------------------------------------------------------- runs alive and ended

/// This run's list, made now and held while it lives, so other runs can tell it is alive
/// (`alive`); at the start of both apps and the search helper.
pub fn register() {
    let mut j = JOURNAL.lock().unwrap_or_else(|e| e.into_inner());
    if j.is_none() && !NO_JOURNAL.load(std::sync::atomic::Ordering::Relaxed) {
        *j = open_journal();
        if j.is_none() {
            NO_JOURNAL.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Whether the run with process id `pid` lives: `Some(true)` while it holds its list,
/// `Some(false)` when its list is there and free (it ended), `None` when it has none (it ran
/// before lists were kept, or its list was swept).
pub fn alive(pid: u32) -> Option<bool> {
    if pid == std::process::id() {
        return Some(true);
    }
    let list = staging_dir()?.join(pid.to_string());
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(list)
        .ok()?;
    Some(f.try_lock().is_err())
}

/// The process id a run's folder is named by: `<pid>`, `<pid>-<n>`, `inside-<pid>`,
/// `coxswain-archive-<pid>-<n>`.
fn pid_of(name: &str) -> Option<u32> {
    let rest = name
        .strip_prefix("inside-")
        .or_else(|| name.strip_prefix("coxswain-archive-"))
        .or_else(|| name.strip_prefix("coxswain-replace-"))
        .unwrap_or(name);
    rest.split('-').next()?.parse().ok()
}

/// `alive` for a run's folder named `name`; `None` for a name no run's id gives.
pub(crate) fn alive_by_name(name: &str) -> Option<bool> {
    alive(pid_of(name)?)
}

/// A day: how long the folder of a run that keeps no list (an older version) is left alone.
const DAY: Duration = Duration::from_secs(24 * 3600);

/// A run's folder at `path` that a live run may be using: its run holds its list, or it keeps
/// none and the folder changed within a day. Names no run's id gives are never in use.
pub(crate) fn in_use(path: &Path) -> bool {
    let Some(pid) = path.file_name().and_then(|n| pid_of(&n.to_string_lossy())) else {
        return false;
    };
    match alive(pid) {
        Some(live) => live,
        None => std::fs::metadata(path)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().unwrap_or_default() <= DAY),
    }
}

/// The folders in the cache's `sub` named by runs: what goes when their run ended.
const RUN_FOLDERS: &[&str] = &[
    "scratch",
    "media",
    "geo",
    "ocr",
    "export",
    "clamscan",
    "peek",
    "nested",
    "cbv",
    "provenance",
];

/// Every run's folder in `dir` whose run ended, removed (never through a link).
pub(crate) fn clear_ended_in(dir: &Path) -> std::io::Result<()> {
    let mut last = Ok(());
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if !std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir()) || in_use(&p) {
            continue;
        }
        if let Err(err) = std::fs::remove_dir_all(&p)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            last = Err(err);
        }
    }
    last
}

/// Whether `name` in the cache folder is one of `RUN_FOLDERS`: cleared run by run.
pub(crate) fn by_run(name: &str) -> bool {
    RUN_FOLDERS.contains(&name)
}

/// At start: the work folders of runs that ended (a crash, a kill, the power gone) removed: in
/// the cache, `RUN_FOLDERS` and the search helper's `inside-<pid>`; in the temp folder, the
/// `coxswain-archive-…` an older version kept there, and find-and-replace's undo copies of
/// runs that ended. A folder a live run uses stays (`in_use`).
pub fn sweep_ended() {
    if let Some(cache) = crate::helper::folder() {
        for sub in RUN_FOLDERS {
            let _ = clear_ended_in(&cache.join(sub));
        }
        for e in std::fs::read_dir(&cache).into_iter().flatten().flatten() {
            let p = e.path();
            if e.file_name().to_string_lossy().starts_with("inside-")
                && std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir())
                && !in_use(&p)
            {
                let _ = std::fs::remove_dir_all(&p);
            }
        }
    }
    sweep_old_temp(&std::env::temp_dir());
    crate::replace::sweep();
}

/// `coxswain-archive-<pid>-<n>` in the temp folder (where versions before this one kept their
/// scratch), of runs that ended: only this user's own folders, never through a link.
fn sweep_old_temp(tmp: &Path) {
    for e in std::fs::read_dir(tmp).into_iter().flatten().flatten() {
        let p = e.path();
        if !e
            .file_name()
            .to_string_lossy()
            .starts_with("coxswain-archive-")
        {
            continue;
        }
        let Ok(m) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        #[cfg(unix)]
        // SAFETY: geteuid has no preconditions.
        let mine = std::os::unix::fs::MetadataExt::uid(&m) == unsafe { libc::geteuid() };
        #[cfg(not(unix))]
        let mine = true;
        if m.is_dir() && mine && !in_use(&p) {
            let _ = std::fs::remove_dir_all(&p);
        }
    }
}

/// At each start of either app and the search helper: this run's list made, what runs that
/// ended left removed, and the preview cache kept in bounds. Runs that ended are swept before
/// their lists go (`sweep_staging`), so their folders are known as theirs.
pub fn at_start() {
    register();
    sweep_ended();
    // Before the lists go: a sync's folder aside is known as an ended run's by its list.
    crate::compare::aside::sweep();
    sweep_staging();
    prune_previews();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Seek;

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
    fn a_preview_cut_short_or_failed_is_half_made_until_it_is_whole() {
        let d = folder("making");
        begin(&d);
        // Killed here, or the build failed: the mark stays.
        assert!(half_made(&d));
        whole(&d);
        assert!(!half_made(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(unix)]
    fn a_cut_prune_leaves_nothing_half_removed() {
        let d = folder("gone");
        std::fs::create_dir_all(d.join(".gone-abc-1/x")).unwrap();
        std::fs::create_dir(d.join("kept")).unwrap();
        prune_in(&d, PREVIEWS_KEPT, PREVIEWS_MOST);
        assert!(!d.join(".gone-abc-1").exists());
        assert!(d.join("kept").exists());
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
        let d = folder("noted").join(if cfg!(windows) {
            "odd name"
        } else {
            "odd \"name\"\nline"
        });
        std::fs::create_dir_all(&d).unwrap();
        staging_in(&d);
        staging_in(&d);
        staging_in(&d.join(".x.coxswain-tmp-1-0-a"));
        // Through the handle that holds it, as Windows wants.
        let mut text = String::new();
        {
            let mut j = JOURNAL.lock().unwrap();
            let f = &mut j.as_mut().unwrap().file;
            f.seek(std::io::SeekFrom::Start(0)).unwrap();
            f.read_to_string(&mut text).unwrap();
        }
        let ours: Vec<String> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .filter(|l: &String| l.contains("coxswain-spill-noted"))
            .collect();
        assert_eq!(ours, vec![d.to_string_lossy().into_owned()], "{text}");
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn a_runs_folders_are_known_by_their_id() {
        assert_eq!(pid_of("4242"), Some(4242));
        assert_eq!(pid_of("4242-7"), Some(4242));
        assert_eq!(pid_of("inside-4242"), Some(4242));
        assert_eq!(pid_of("coxswain-archive-4242-0"), Some(4242));
        assert_eq!(pid_of("coxswain-replace-4242-0123abcd"), Some(4242));
        assert_eq!(pid_of("part-3"), None);
        assert_eq!(pid_of("0123abcd.json"), None);
    }

    /// The crash case for work folders: a run that ended (its list free) loses its folder at
    /// once, a live run (its list held) keeps its own however old, and one with no list keeps
    /// it for a day.
    #[test]
    #[cfg(unix)]
    fn an_ended_runs_work_goes_and_a_live_runs_stays_however_old() {
        let lists = staging_dir().unwrap();
        crate::fs::private_dir(&lists).unwrap();
        let d = folder("ended");
        // Ids no real process of this test has: far above any pid_max.
        let (ended, live, none_new, none_old) =
            (4_000_001u32, 4_000_002u32, 4_000_003u32, 4_000_004u32);
        std::fs::write(lists.join(ended.to_string()), "").unwrap();
        std::fs::write(lists.join(live.to_string()), "").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lists.join(live.to_string()))
            .unwrap();
        held.lock().unwrap();
        for pid in [ended, live, none_new, none_old] {
            std::fs::create_dir_all(d.join(format!("{pid}-0")).join("in")).unwrap();
        }
        age(&d.join(format!("{live}-0")), 3);
        age(&d.join(format!("{none_old}-0")), 3);
        std::fs::write(d.join("not-a-run"), "x").unwrap();
        clear_ended_in(&d).unwrap();
        assert!(
            !d.join(format!("{ended}-0")).exists(),
            "ended: gone at once"
        );
        assert!(
            d.join(format!("{live}-0/in")).exists(),
            "live: kept, however old"
        );
        assert!(
            d.join(format!("{none_new}-0/in")).exists(),
            "no list, new: kept"
        );
        assert!(
            !d.join(format!("{none_old}-0")).exists(),
            "no list, a day old: gone"
        );
        assert!(d.join("not-a-run").exists());
        drop(held);
        for pid in [ended, live] {
            let _ = std::fs::remove_file(lists.join(pid.to_string()));
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// An older version's scratch in the temp folder: a link is never followed.
    #[test]
    #[cfg(unix)]
    fn old_scratch_in_the_temp_folder_goes_but_never_through_a_link() {
        let d = folder("oldtemp");
        let outside = d.join("outside");
        std::fs::create_dir_all(outside.join("k")).unwrap();
        std::fs::create_dir_all(d.join("coxswain-archive-4000005-0/a")).unwrap();
        age(&d.join("coxswain-archive-4000005-0"), 3);
        std::os::unix::fs::symlink(&outside, d.join("coxswain-archive-4000006-0")).unwrap();
        sweep_old_temp(&d);
        assert!(!d.join("coxswain-archive-4000005-0").exists());
        assert!(outside.join("k").exists());
        let _ = std::fs::remove_dir_all(&d);
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

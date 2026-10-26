//! What Coxswain leaves on the disk by itself, kept in bounds: the previews no one looked at for
//! a month go, and the passing names a copy writes beside its destination
//! (`.name.coxswain-tmp-…`, a disk image's `.name.<run>.coxswain-tmp`, the editor's
//! `.name.coxswain-<run>-….tmp`) are noted, so the next start removes what a run that was killed
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

/// This run's name in the names of its lists, folders and passing names: `<pid>_<8 hex>`, its
/// process id and a random part, so runs that share a process id (each launch in a Flatpak
/// sandbox is the same process there) never take each other's for their own.
pub fn run() -> &'static str {
    static RUN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    RUN.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        // Seeded at random for each process.
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        format!("{}_{:08x}", std::process::id(), h.finish() as u32)
    })
}

/// This run's list of the folders it wrote passing names in, `staging/<run>` in the cache, held
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
        let _ = writeln!(j.file, "{}", line_of(dir));
    }
}

/// A folder as a line of a run's list: a JSON string, or where the name is not UTF-8 (possible
/// on the BSDs, illumos and Linux, and Windows' unpaired surrogates) its raw bytes (unix) or
/// UTF-16 units (Windows) as a JSON array, so the sweep finds the very folder.
fn line_of(dir: &Path) -> String {
    if let Some(s) = dir.to_str() {
        return serde_json::to_string(s).unwrap_or_default();
    }
    #[cfg(unix)]
    let raw: Vec<u16> = std::os::unix::ffi::OsStrExt::as_bytes(dir.as_os_str())
        .iter()
        .map(|&b| u16::from(b))
        .collect();
    #[cfg(windows)]
    let raw: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(dir.as_os_str()).collect();
    serde_json::to_string(&raw).unwrap_or_default()
}

/// `line_of` read back; `None` for a line it does not give.
fn folder_of(line: &str) -> Option<PathBuf> {
    match serde_json::from_str::<serde_json::Value>(line).ok()? {
        serde_json::Value::String(s) => Some(PathBuf::from(s)),
        serde_json::Value::Array(units) => {
            let units: Vec<u16> = units
                .iter()
                .map(|u| u.as_u64().and_then(|u| u16::try_from(u).ok()))
                .collect::<Option<_>>()?;
            #[cfg(unix)]
            let os = <std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(
                units
                    .iter()
                    .map(|&u| u8::try_from(u).ok())
                    .collect::<Option<Vec<u8>>>()?,
            );
            #[cfg(windows)]
            let os = <std::ffi::OsString as std::os::windows::ffi::OsStringExt>::from_wide(&units);
            Some(PathBuf::from(os))
        }
        _ => None,
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
        .open(dir.join(run()))
        .ok()?;
    file.try_lock().ok()?;
    Some(Journal {
        file,
        seen: Default::default(),
    })
}

/// A passing name: a copy's `.name.coxswain-tmp-<run>-…` (a file or a folder), a disk image's
/// `.name.<run>.coxswain-tmp`, a save's `.name.coxswain-<run>-….tmp`; with `run`, only that
/// run's (`run()`, or a bare process id from an older version).
fn ours(name: &str, run: Option<&str>) -> bool {
    if !name.starts_with('.') {
        return false;
    }
    match run {
        Some(run) => {
            name.contains(&format!(".coxswain-tmp-{run}-"))
                || name.ends_with(&format!(".{run}.coxswain-tmp"))
                || (name.contains(&format!(".coxswain-{run}-")) && name.ends_with(".tmp"))
        }
        None => name.contains(".coxswain-tmp-") || name.ends_with(".coxswain-tmp"),
    }
}

/// At start: what runs that ended without tidying (a crash, a kill, the power gone) left beside
/// their copies, removed: only names of `ours` with that run's name, in the folders its list
/// names, never through a link. A list a running app holds is left.
pub fn sweep_staging() {
    if let Some(dir) = staging_dir() {
        sweep_staging_in(&dir, SWEEP_WAIT);
    }
}

/// How long the start waits for the lists' sweeps; one still going on (a network folder that
/// does not answer) goes on by itself, and the others are not held behind it.
const SWEEP_WAIT: Duration = Duration::from_secs(60);

/// Each ended run's list swept on a thread of its own, waited for up to `wait` in all.
fn sweep_staging_in(dir: &Path, wait: Duration) {
    let (done, all_done) = std::sync::mpsc::channel();
    let mut started = 0;
    let end = std::time::Instant::now() + wait;
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == run() || run_of(&name) != Some(name.as_str()) {
            continue;
        }
        let done = done.clone();
        started += 1;
        std::thread::spawn(move || {
            sweep_list(&e.path(), &name, end);
            let _ = done.send(());
        });
    }
    for _ in 0..started {
        let left = end.saturating_duration_since(std::time::Instant::now());
        if all_done.recv_timeout(left).is_err() {
            break;
        }
    }
}

/// One ended run's list at `path`: each folder it names swept on a thread of its own, so a
/// folder that does not answer (a network mount gone) holds back none of the others; when all
/// were swept by `end`, the list is removed, else it stays for the next start.
// ponytail: a thread per folder named; a hung one stays blocked until the run ends. Lists name
// a few folders each (where files were written), so that is a few threads at most.
fn sweep_list(path: &Path, run: &str, end: std::time::Instant) {
    let Ok(mut list) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    else {
        return;
    };
    if list.try_lock().is_err() {
        return;
    }
    // Read through the handle that holds the lock: Windows refuses any other.
    let mut text = String::new();
    let _ = list.read_to_string(&mut text);
    let (done, all_done) = std::sync::mpsc::channel();
    let mut started = 0;
    for folder in text.lines().filter_map(folder_of) {
        let (done, run) = (done.clone(), run.to_string());
        started += 1;
        std::thread::spawn(move || {
            sweep_folder(&folder, &run);
            let _ = done.send(());
        });
    }
    let whole = (0..started).all(|_| all_done.recv_timeout(end.saturating_duration_since(std::time::Instant::now())).is_ok());
    if whole {
        // Removed while still held, so no run takes it up between.
        let _ = std::fs::remove_file(path);
    }
    drop(list);
}

/// A folder that hangs, for a test: its sweep sleeps this long first.
#[cfg(test)]
static HANG: std::sync::Mutex<Option<(PathBuf, Duration)>> = std::sync::Mutex::new(None);

/// The run's passing names in `folder` removed: folders whole, files, links as links.
fn sweep_folder(folder: &Path, run: &str) {
    #[cfg(test)]
    {
        let hang = HANG.lock().unwrap().clone();
        if let Some((_, d)) = hang.filter(|(f, _)| f == folder) {
            std::thread::sleep(d);
        }
    }
    for left in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        if !ours(&left.file_name().to_string_lossy(), Some(run)) {
            continue;
        }
        let _ = match std::fs::symlink_metadata(left.path()) {
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(left.path()),
            Ok(m) if m.is_file() => std::fs::remove_file(left.path()),
            _ => Ok(()),
        };
    }
}

/// At a clean exit of either app or the search helper, as the last thing it does: this run's
/// work folders in the cache and its list go, and nothing more is noted.
pub fn at_exit() {
    if let Some(cache) = crate::helper::folder() {
        remove_own_in(&cache, run());
    }
    let mut j = JOURNAL.lock().unwrap_or_else(|e| e.into_inner());
    NO_JOURNAL.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(held) = j.take() {
        if let Some(dir) = staging_dir() {
            // Removed while still held, as the sweep does.
            let _ = std::fs::remove_file(dir.join(run()));
        }
        drop(held);
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

/// Whether the run named `run` (`run()`, or an older version's process id) lives: `Some(true)`
/// while it holds its list, `Some(false)` when its list is there and free (it ended), `None`
/// when it has none (it ran before lists were kept, or its list was swept).
pub fn alive(run: &str) -> Option<bool> {
    if run == self::run() {
        return Some(true);
    }
    let list = staging_dir()?.join(run);
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(list)
        .ok()?;
    Some(f.try_lock().is_err())
}

/// The run a run's folder is named by: `<run>`, `<run>-<n>`, `inside-<run>`,
/// `coxswain-archive-<run>-<n>`, `coxswain-replace-<run>-…`; `<run>` is `run()`'s
/// `<pid>_<hex>`, or an older version's bare process id.
fn run_of(name: &str) -> Option<&str> {
    let rest = name
        .strip_prefix("inside-")
        .or_else(|| name.strip_prefix("coxswain-archive-"))
        .or_else(|| name.strip_prefix("coxswain-replace-"))
        .unwrap_or(name);
    let run = rest.split('-').next()?;
    let (pid, part) = run.split_once('_').unwrap_or((run, "0"));
    let ok = !pid.is_empty()
        && pid.bytes().all(|b| b.is_ascii_digit())
        && !part.is_empty()
        && part.bytes().all(|b| b.is_ascii_hexdigit());
    ok.then_some(run)
}

/// `alive` for a run's folder named `name`; `None` for a name no run gives.
pub(crate) fn alive_by_name(name: &str) -> Option<bool> {
    alive(run_of(name)?)
}

/// A day: how long the folder of a run that keeps no list (an older version) is left alone.
const DAY: Duration = Duration::from_secs(24 * 3600);

/// A run's folder at `path` that a live run may be using: its run holds its list, or it keeps
/// none and the folder changed within a day. Names no run's id gives are never in use.
pub(crate) fn in_use(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy()) else {
        return false;
    };
    let Some(run) = run_of(&name) else {
        return false;
    };
    match alive(run) {
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

/// The work folders of the run `run` in the cache folder `cache` (`RUN_FOLDERS`, `inside-<run>`)
/// removed, never through a link.
fn remove_own_in(cache: &Path, run: &str) {
    let subs = RUN_FOLDERS.iter().map(|s| cache.join(s));
    for dir in subs.chain([cache.to_path_buf()]) {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if run_of(&e.file_name().to_string_lossy()) == Some(run)
                && std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir())
            {
                let _ = std::fs::remove_dir_all(&p);
            }
        }
    }
}

/// Whether `name` in the cache folder is one of `RUN_FOLDERS`: cleared run by run.
pub(crate) fn by_run(name: &str) -> bool {
    RUN_FOLDERS.contains(&name)
}

/// At start: the work folders of runs that ended (a crash, a kill, the power gone) removed: in
/// the cache, `RUN_FOLDERS` and the search helper's `inside-<run>`; in the temp folder, the
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
        sweep_staging_in(&lists, SWEEP_WAIT);
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

    /// A folder that does not answer holds back none of the others named in the same list:
    /// they are swept within the wait, and the list stays to be tried again at the next start.
    #[test]
    fn a_hung_folder_holds_back_none_of_its_list() {
        let d = folder("hung");
        let (hung, fine, lists) = (d.join("hung"), d.join("fine"), d.join("staging"));
        for f in [&hung, &fine] {
            std::fs::create_dir_all(f).unwrap();
            std::fs::write(f.join(".a.txt.coxswain-tmp-4247-1"), "half").unwrap();
        }
        std::fs::create_dir(&lists).unwrap();
        // The hung one first, as a list read one after another would be stuck on it.
        std::fs::write(lists.join("4247"), format!("{}\n{}\n", line_of(&hung), line_of(&fine))).unwrap();
        *HANG.lock().unwrap() = Some((hung.clone(), Duration::from_secs(60)));
        let t = std::time::Instant::now();
        sweep_staging_in(&lists, Duration::from_secs(5));
        assert!(t.elapsed() < Duration::from_secs(40), "{:?}", t.elapsed());
        assert!(!fine.join(".a.txt.coxswain-tmp-4247-1").exists());
        assert!(hung.join(".a.txt.coxswain-tmp-4247-1").exists());
        assert!(lists.join("4247").exists(), "kept for the next start");
        // At the next start it answers: swept, and the list goes.
        *HANG.lock().unwrap() = None;
        // The first sweep's list thread may still be letting go of the list's lock.
        let t = std::time::Instant::now();
        while lists.join("4247").exists() && t.elapsed() < Duration::from_secs(30) {
            sweep_staging_in(&lists, SWEEP_WAIT);
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!hung.join(".a.txt.coxswain-tmp-4247-1").exists());
        assert!(!lists.join("4247").exists());
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
        let ours: Vec<PathBuf> = text
            .lines()
            .filter_map(folder_of)
            .filter(|l| l.to_string_lossy().contains("coxswain-spill-noted"))
            .collect();
        assert_eq!(ours, vec![d.clone()], "{text}");
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    /// A folder whose name is not UTF-8 is listed as it is, so the sweep finds it.
    #[test]
    #[cfg(unix)]
    fn a_folder_not_named_in_utf8_is_swept_too() {
        use std::os::unix::ffi::OsStrExt;
        let d = folder("bytes");
        let target = d.join(std::ffi::OsStr::from_bytes(b"caf\xe9"));
        let lists = d.join("staging");
        // Some file systems (macOS's) take only UTF-8 names.
        if std::fs::create_dir(&target).is_err() {
            return;
        }
        assert_eq!(folder_of(&line_of(&target)), Some(target.clone()));
        assert_eq!(
            folder_of(&line_of(Path::new("/a/ok"))),
            Some(PathBuf::from("/a/ok"))
        );
        std::fs::write(target.join(".a.txt.coxswain-tmp-4245-1"), "half").unwrap();
        std::fs::create_dir(&lists).unwrap();
        std::fs::write(lists.join("4245"), format!("{}\n", line_of(&target))).unwrap();
        sweep_staging_in(&lists, SWEEP_WAIT);
        assert!(!target.join(".a.txt.coxswain-tmp-4245-1").exists());
        assert!(!lists.join("4245").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A clean exit takes this run's own work folders with it, and no other run's.
    #[test]
    fn a_clean_exit_leaves_none_of_its_own_work() {
        let d = folder("exit");
        let me = "4246_0a0b0c0d";
        for p in [
            format!("scratch/{me}-0/x"),
            format!("peek/{me}/x"),
            format!("inside-{me}/x"),
            "scratch/4246_ffffffff-0/x".into(),
            "peek/4247/x".into(),
        ] {
            std::fs::create_dir_all(d.join(p)).unwrap();
        }
        remove_own_in(&d, me);
        assert!(!d.join(format!("scratch/{me}-0")).exists());
        assert!(!d.join(format!("peek/{me}")).exists());
        assert!(!d.join(format!("inside-{me}")).exists());
        assert!(
            d.join("scratch/4246_ffffffff-0/x").exists(),
            "same pid, another run"
        );
        assert!(d.join("peek/4247/x").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_runs_folders_are_known_by_their_run() {
        assert_eq!(run_of("4242"), Some("4242"));
        assert_eq!(run_of("4242-7"), Some("4242"));
        assert_eq!(run_of("4242_0a1b2c3d-7"), Some("4242_0a1b2c3d"));
        assert_eq!(run_of("inside-4242_0a1b2c3d"), Some("4242_0a1b2c3d"));
        assert_eq!(run_of("coxswain-archive-4242-0"), Some("4242"));
        assert_eq!(run_of("coxswain-replace-4242_ff-0123abcd"), Some("4242_ff"));
        assert_eq!(run_of("part-3"), None);
        assert_eq!(run_of("0123abcd.json"), None);
        assert_eq!(run_of("4242_"), None);
        assert_eq!(run_of("4242_xyz"), None);
        assert_eq!(run_of(run()), Some(run()));
    }

    /// Two launches with the same process id (a Flatpak sandbox) are two runs.
    #[test]
    fn a_run_is_more_than_its_process_id() {
        let me = run();
        let (pid, part) = me.split_once('_').unwrap();
        assert_eq!(pid, std::process::id().to_string());
        assert_eq!(part.len(), 8);
        let other = format!("{pid}_{:08x}", u32::from_str_radix(part, 16).unwrap() ^ 1);
        let lists = staging_dir().unwrap();
        crate::fs::private_dir(&lists).unwrap();
        std::fs::write(lists.join(&other), "").unwrap();
        assert_eq!(alive(&other), Some(false), "same pid, another run: ended");
        assert_eq!(alive(me), Some(true));
        let _ = std::fs::remove_file(lists.join(&other));
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
        sweep_staging_in(&lists, SWEEP_WAIT);
        assert!(outside.join("keep/f").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}

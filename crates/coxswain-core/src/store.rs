//! The search store: what is known about the files in the user's folders, and the text of
//! those that have text, in one SQLite file in the cache folder. Only the helper writes it.
//!
//! It also knows every file's size, and the total of each folder it leaves out, so a folder's
//! size is a sum; and the content hash of files that share a size, for the duplicate finder.
//!
//! It is a cache: a store from another version is thrown away and filled again.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use rusqlite::{params, Connection};

use crate::config::SearchConfig;
use crate::index::{Hit, Results};
use crate::sizes::Size;

/// Seconds since the Unix epoch the file was modified.
fn secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64)
}

/// `path` as the walk writes it: on Windows `C:\a/b` is stored as `C:\a\b`.
fn key(path: &Path) -> Option<String> {
    Some(path.components().collect::<PathBuf>().to_str()?.to_string())
}

/// A row's path as the apps go to it: a commit's is the history folder of that commit.
fn shown(path: String) -> PathBuf {
    crate::history::from_key(&path).unwrap_or_else(|| PathBuf::from(path))
}

/// The range of paths below `dir`: every path that starts with it and a separator.
fn below(dir: &str) -> (String, String) {
    let sep = std::path::MAIN_SEPARATOR;
    let dir = dir.trim_end_matches(sep);
    (format!("{dir}{sep}"), format!("{dir}{}", char::from(sep as u8 + 1)))
}

/// Bumped when the tables change, and when the readers learn formats: files the store has
/// marked as without text are only read again when they change.
const VERSION: i32 = 6;
/// A snippet marks the words it found with these; the apps turn them into a highlight.
pub const MARK: (char, char) = ('\u{1}', '\u{2}');

pub struct Store {
    db: Mutex<Connection>,
    /// Files still to be read in the scan under way.
    pub pending: AtomicUsize,
    /// "Index now": no rests until the backlog is done.
    pub hurry: AtomicBool,
    /// Set by `clear`: the scan under way stops, and a new one begins.
    cleared: AtomicBool,
    /// The roots of the last walk, and when it began (seconds since the Unix epoch): sizes are
    /// known once one has finished.
    walked: Mutex<Option<(Vec<PathBuf>, u64)>>,
    /// Where the rows of roots on disks that are not plugged in are: kept, not searched.
    offline: Mutex<Vec<String>>,
    /// Reading waits for the mains.
    pub paused: AtomicBool,
    /// The roots of the settings, as the last scan had them.
    configured: Mutex<Vec<PathBuf>>,
    /// Search by meaning is on (and the model is downloaded).
    pub meaning: AtomicBool,
    /// Where the vectors come from, when search by meaning is on: the built-in model or a server.
    engine: Mutex<Option<Arc<crate::meaning::Engine>>>,
    /// Why the last vectors could not be had (a server that did not answer), for Settings.
    pub meaning_error: Mutex<Option<String>>,
    /// Why the last scan failed, for Settings and the terminal app: until one succeeds, no
    /// file further on is read and none gets its vectors.
    pub error: Mutex<Option<String>>,
    /// The signs of every passage's vector, read from `chunks` for the first search by meaning
    /// and kept up to date after.
    signs: Mutex<Option<Signs>>,
    /// Where the store is: the signs are read on a connection of their own.
    path: PathBuf,
    /// Files whose vectors were dropped for a new way of making passages, while they get new
    /// ones; and how long a file took, in milliseconds, as measured, for the time left.
    pub renewing: AtomicUsize,
    pub ms_per_file: AtomicUsize,
    /// The clouds the walks came upon (files only in the cloud): (name, folder).
    clouds: Mutex<Vec<(String, PathBuf)>>,
    /// How many files' text a search by meaning read, for the tests: per store, so tests that
    /// run at the same time do not count each other's.
    #[cfg(test)]
    bodies_read: AtomicUsize,
}

/// A file as the store knows it: size, date, and whether it was left in the cloud.
type Known = HashMap<String, (u64, i64, bool)>;
/// A file that changed: path, size, date, left in the cloud.
type Changed = (String, u64, i64, bool);
/// A file inside an archive: its path as a key, size, modified.
type Member = (String, u64, u64);
/// The signs of passages' vectors, one after the other, `width` words each, with the file and
/// number of each: 9 bytes and the signs (48 bytes for 384 numbers, 128 for 1024) a passage.
#[derive(Default)]
struct Signs {
    files: Vec<i64>,
    ns: Vec<u8>,
    bits: Vec<u64>,
    width: usize,
}

impl Signs {
    /// A passage's packed vector; one of another length (a broken row) is left out.
    fn push(&mut self, file: i64, n: u8, packed: &[u8]) {
        let s = crate::meaning::signs(packed);
        if self.width == 0 {
            self.width = s.len();
        }
        if s.len() == self.width && self.width > 0 {
            self.files.push(file);
            self.ns.push(n);
            self.bits.extend_from_slice(&s);
        }
    }

    /// Keep the passages of the files `keep` takes, in place.
    fn retain(&mut self, keep: impl Fn(i64) -> bool) {
        let mut to = 0;
        for i in 0..self.files.len() {
            if keep(self.files[i]) {
                self.files[to] = self.files[i];
                self.ns[to] = self.ns[i];
                self.bits.copy_within(i * self.width..(i + 1) * self.width, to * self.width);
                to += 1;
            }
        }
        self.files.truncate(to);
        self.ns.truncate(to);
        self.bits.truncate(to * self.width);
    }

    /// The `keep` passages whose signs are most like `wanted`'s: (file, number). Parts of
    /// 65,536 passages each are gone through on all cores, each keeping its best.
    fn closest(&self, wanted: &[u64], keep: usize) -> Vec<(i64, u8)> {
        use rayon::prelude::*;
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        const PART: usize = 1 << 16;
        if self.width == 0 || wanted.len() != self.width || keep == 0 {
            return vec![];
        }
        let mut best: Vec<(u32, usize)> = self
            .bits
            .par_chunks(PART * self.width)
            .enumerate()
            .flat_map_iter(|(p, part)| {
                // The least of the best so far on top: most passages are not let in at all.
                let mut best: BinaryHeap<Reverse<(u32, usize)>> = BinaryHeap::with_capacity(keep + 1);
                for (i, s) in part.chunks_exact(self.width).enumerate() {
                    let a = crate::meaning::alike(wanted, s);
                    if best.len() < keep {
                        best.push(Reverse((a, p * PART + i)));
                    } else if best.peek().is_some_and(|Reverse((low, _))| a > *low) {
                        best.pop();
                        best.push(Reverse((a, p * PART + i)));
                    }
                }
                best.into_iter().map(|Reverse(x)| x)
            })
            .collect();
        if best.len() > keep {
            best.select_nth_unstable_by(keep, |a, b| b.0.cmp(&a.0));
            best.truncate(keep);
        }
        best.into_iter().map(|(_, i)| (self.files[i], self.ns[i])).collect()
    }
}

impl Store {
    pub fn path() -> Option<PathBuf> {
        Some(crate::helper::folder()?.join("search.db"))
    }

    pub fn open(path: &Path) -> rusqlite::Result<Store> {
        let db = Connection::open(path)?;
        #[cfg(unix)]
        let _ = std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600));
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        if db.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))? != VERSION {
            db.execute_batch("DROP TABLE IF EXISTS files; DROP TABLE IF EXISTS text; DROP TABLE IF EXISTS skipped; DROP TABLE IF EXISTS hashes; DROP TABLE IF EXISTS roots; DROP TABLE IF EXISTS meta;")?;
            db.pragma_update(None, "user_version", VERSION)?;
        }
        // The duplicate finder writes hashes from the app while the helper scans.
        db.busy_timeout(Duration::from_secs(10))?;
        // `has_text` is NULL until the file has been read. `files_path_size` answers a folder's
        // total from the index alone: twenty times faster over a million files. `roots` knows each root's disk and
        // where on it the root is, and where its rows are (`at`): a disk mounted elsewhere
        // keeps its rows. `skipped` holds the folders left
        // out of the walk with their bytes and files; `hashes`, files' BLAKE3 as they were.
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS files(id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL, modified INTEGER NOT NULL, has_text INTEGER);
             CREATE INDEX IF NOT EXISTS files_size ON files(size);
             CREATE VIRTUAL TABLE IF NOT EXISTS text USING fts5(body, tokenize = 'unicode61 remove_diacritics 2');
             CREATE TABLE IF NOT EXISTS skipped(path TEXT PRIMARY KEY, bytes INTEGER NOT NULL, files INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS hashes(path TEXT PRIMARY KEY, size INTEGER NOT NULL, modified INTEGER NOT NULL, hash TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS roots(path TEXT PRIMARY KEY, volume TEXT, inside TEXT, at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS chunks(file INTEGER NOT NULL, n INTEGER NOT NULL, vector BLOB NOT NULL, PRIMARY KEY(file, n));
             CREATE TABLE IF NOT EXISTS errors(at INTEGER NOT NULL, path TEXT, what TEXT NOT NULL);",
        )?;
        // Search by meaning came later: a store from before gets the column, and keeps its text.
        // `embedded` is NULL until the file's passages have their vectors in `chunks`, 1 when
        // they have, 0 when the server refused the file (tried again at the next scan).
        if db.prepare("SELECT embedded FROM files LIMIT 0").is_err() {
            db.execute_batch("ALTER TABLE files ADD COLUMN embedded INTEGER")?;
        }
        // Files inside archives came later too: `inside` is 1 for them. They have text, and no
        // part in sizes, hashes or the walk; the index of sizes leaves them out.
        if db.prepare("SELECT inside FROM files LIMIT 0").is_err() {
            db.execute_batch("ALTER TABLE files ADD COLUMN inside INTEGER; DROP INDEX IF EXISTS files_path_size;")?;
        }
        db.execute_batch("CREATE INDEX IF NOT EXISTS files_path_size ON files(path, size, inside) WHERE inside IS NULL")?;
        // Files left in the cloud came later still: `cloud` is 1 for a file only in the cloud
        // that the settings do not read. It has no text, vectors or hash.
        if db.prepare("SELECT cloud FROM files LIMIT 0").is_err() {
            db.execute_batch("ALTER TABLE files ADD COLUMN cloud INTEGER")?;
        }
        // The files still to get their vectors, newest first, without a look at the rest.
        db.execute_batch("CREATE INDEX IF NOT EXISTS files_unembedded ON files(modified) WHERE has_text = 1 AND embedded IS NULL")?;
        let renewing = passages_are(&db, crate::meaning::SCHEME)?;
        Ok(Store { db: Mutex::new(db), pending: AtomicUsize::new(0), hurry: AtomicBool::new(false), cleared: AtomicBool::new(false), walked: Mutex::default(), offline: Mutex::default(), paused: AtomicBool::new(false), configured: Mutex::default(), meaning: AtomicBool::new(false), engine: Mutex::default(), meaning_error: Mutex::default(), error: Mutex::default(), signs: Mutex::default(), path: path.to_path_buf(), renewing: AtomicUsize::new(renewing), ms_per_file: AtomicUsize::new(0), clouds: Mutex::default(), #[cfg(test)] bodies_read: AtomicUsize::new(0) })
    }

    /// Bytes and files below `dir`, and when the walk they come from began. `None` until a
    /// walk has finished, and for folders outside it.
    pub fn size(&self, dir: &Path) -> Option<(Size, u64)> {
        let dir = &PathBuf::from(key(dir)?);
        let (roots, at) = self.walked.lock().unwrap().clone()?;
        let root = roots.iter().find(|r| dir.starts_with(r))?;
        let db = self.db.lock().unwrap();
        let skipped = |p: &Path| db.query_row("SELECT bytes, files FROM skipped WHERE path = ?1", [p.to_str()?], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))).ok();
        if let Some(size) = skipped(dir) {
            return Some((size, at));
        }
        // Inside a folder that was left out: only its total is known.
        if dir.ancestors().take_while(|a| a.starts_with(root)).any(|a| skipped(a).is_some()) {
            return None;
        }
        let (from, to) = below(dir.to_str()?);
        let sum = |table: &str, bytes: &str, files: &str| {
            db.query_row(&format!("SELECT coalesce(sum({bytes}), 0), coalesce(sum({files}), 0) FROM {table} WHERE path > ?1 AND path < ?2{}", if table == "files" { " AND inside IS NULL" } else { "" }), [&from, &to], |r| {
                Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))
            })
        };
        let (a, b) = (sum("files", "size", "1").ok()?, sum("skipped", "bytes", "files").ok()?);
        Some(((a.0 + b.0, a.1 + b.1), at))
    }

    /// The hash of `path` when it had this size and date.
    pub fn hash(&self, path: &Path, size: u64, modified: u64) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT hash FROM hashes WHERE path = ?1 AND size = ?2 AND modified = ?3", params![key(path)?, size as i64, modified as i64], |r| r.get(0)).ok()
    }

    /// Hashes as (path, size, modified, hash), in one transaction.
    pub fn put_hashes(&self, rows: &[(PathBuf, u64, u64, String)]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for (path, size, modified, hash) in rows {
            let Some(path) = key(path) else { continue };
            tx.execute("INSERT OR REPLACE INTO hashes(path, size, modified, hash) VALUES (?1, ?2, ?3, ?4)", params![path, *size as i64, *modified as i64, hash])?;
        }
        tx.commit()
    }

    /// Forget everything; the store fills again from the start.
    pub fn clear(&self) -> rusqlite::Result<()> {
        *self.walked.lock().unwrap() = None;
        self.cleared.store(true, Ordering::SeqCst);
        let db = self.db.lock().unwrap();
        *self.signs.lock().unwrap() = None;
        // The vacuum goes to the write-ahead log first: written back and emptied, the room is free.
        db.execute_batch("DELETE FROM text; DELETE FROM files; DELETE FROM skipped; DELETE FROM hashes; DELETE FROM chunks; DELETE FROM errors; VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")
    }

    /// What went wrong, with the file it was about, kept for the store's preview: the latest 50,
    /// the same one again not twice in a row.
    pub(crate) fn note_error(&self, path: Option<&str>, what: &str) {
        let db = self.db.lock().unwrap();
        let last: Option<(Option<String>, String)> = db.query_row("SELECT path, what FROM errors ORDER BY rowid DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?))).ok();
        if last.as_ref().is_some_and(|(p, w)| p.as_deref() == path && w == what) {
            return;
        }
        let _ = db.execute("INSERT INTO errors(at, path, what) VALUES (?1, ?2, ?3)", params![now() as i64, path, what]);
        let _ = db.execute("DELETE FROM errors WHERE rowid <= (SELECT max(rowid) - 50 FROM errors)", []);
    }

    /// Bytes the store takes on disk.
    pub fn bytes(&self) -> u64 {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as u64
    }

    /// Rest as long as the work since `start` took, unless in a hurry; on battery, until the
    /// mains is back.
    fn rest(&self, start: Instant, stop: &AtomicBool) {
        self.rest_after(start.elapsed(), stop);
    }

    /// The rest after `work`, never on battery unless *Index now*.
    fn rest_after(&self, work: Duration, stop: &AtomicBool) {
        if self.hurry.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(work.min(Duration::from_secs(2)));
        while !self.hurry.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) && crate::machine::on_battery() {
            self.paused.store(true, Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(1));
        }
        self.paused.store(false, Ordering::Relaxed);
    }

    /// Files found without text are read again when an installed program can now read their
    /// kind (`exts`, e.g. tesseract for pictures), or no longer can.
    fn tools_changed(&self, exts: &[&str]) -> rusqlite::Result<()> {
        let now = exts.join(",");
        let db = self.db.lock().unwrap();
        let before: String = db.query_row("SELECT value FROM meta WHERE key = 'tools'", [], |r| r.get(0)).unwrap_or_default();
        if before == now {
            return Ok(());
        }
        let kinds: Vec<&str> = before.split(',').chain(now.split(',')).filter(|e| !e.is_empty()).collect();
        for ext in kinds {
            db.execute("UPDATE files SET has_text = NULL WHERE has_text = 0 AND lower(path) LIKE ?1", [format!("%.{ext}")])?;
        }
        db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('tools', ?1)", [now])?;
        Ok(())
    }

    /// A reader learnt to get more of some kinds of files: those files are read again, once,
    /// and the rest of the store stays. `(tag, extensions)`: the tag names what changed.
    fn readers_changed(&self, tag: &str, exts: &[&str]) -> rusqlite::Result<()> {
        let db = self.db.lock().unwrap();
        let before: String = db.query_row("SELECT value FROM meta WHERE key = 'readers'", [], |r| r.get(0)).unwrap_or_default();
        if before == tag {
            return Ok(());
        }
        for ext in exts {
            db.execute("UPDATE files SET has_text = NULL, embedded = NULL WHERE lower(path) LIKE ?1", [format!("%.{ext}")])?;
        }
        db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('readers', ?1)", [tag])?;
        Ok(())
    }

    /// Where each root's rows are, as the store last saw it: path -> (disk, place on it, rows at).
    fn roots(&self) -> rusqlite::Result<HashMap<String, (Option<String>, Option<String>, String)>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare("SELECT path, volume, inside, at FROM roots")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?;
        rows.collect()
    }

    /// The root at `path` now has its rows at `at`, on this disk. Rows kept at `moved` (the
    /// disk's earlier place) are moved there first.
    fn place(&self, path: &str, volume: Option<(String, PathBuf)>, at: &str, moved: Option<&str>) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        if let Some(old) = moved.filter(|old| *old != at) {
            let (from, to) = below(old);
            for table in ["files", "hashes", "skipped"] {
                tx.execute(&format!("UPDATE {table} SET path = ?4 || substr(path, length(?1) + 1) WHERE path = ?1 OR (path > ?2 AND path < ?3)"), [old, &from, &to, at])?;
            }
        }
        let (volume, inside) = volume.map_or((None, None), |(v, i)| (Some(v), i.to_str().map(str::to_string)));
        tx.execute("INSERT OR REPLACE INTO roots(path, volume, inside, at) VALUES (?1, ?2, ?3, ?4)", params![path, volume, inside, at])?;
        tx.commit()
    }

    /// Each root: where it is now (`None` while its disk is not plugged in), and its bytes and
    /// files in the store.
    pub fn root_sizes(&self) -> Vec<(PathBuf, Option<PathBuf>, Size)> {
        let placed = self.roots().unwrap_or_default();
        let configured = self.configured.lock().unwrap().clone();
        let db = self.db.lock().unwrap();
        configured
            .into_iter()
            .map(|root| {
                let text = key(&root).unwrap_or_default();
                let at = placed.get(&text).map_or(text.clone(), |p| p.2.clone());
                let (from, to) = below(&at);
                let size = db
                    .query_row("SELECT coalesce(sum(size), 0), count(*) FROM files WHERE path > ?1 AND path < ?2 AND inside IS NULL", [&from, &to], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)))
                    .unwrap_or_default();
                let online = !self.offline.lock().unwrap().contains(&at) && Path::new(&at).is_dir();
                (root, online.then(|| PathBuf::from(at)), size)
            })
            .collect()
    }

    /// How many files have their text in the store.
    pub fn texts(&self) -> usize {
        self.db.lock().unwrap().query_row("SELECT count(*) FROM files WHERE has_text", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    }

    /// Every file the store knows: path -> (size, modified, left in the cloud).
    fn known(&self) -> rusqlite::Result<Known> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare("SELECT path, size, modified, cloud IS NOT NULL FROM files WHERE inside IS NULL")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, (r.get::<_, i64>(1)? as u64, r.get(2)?, r.get(3)?))))?;
        rows.collect()
    }

    /// The clouds the walks came upon: (name, folder).
    pub fn clouds(&self) -> Vec<(String, PathBuf)> {
        self.clouds.lock().unwrap().clone()
    }

    /// A file only in the cloud was seen at `path`: its cloud is remembered, for Settings.
    fn saw_cloud(&self, path: &Path) {
        let mut all = self.clouds.lock().unwrap();
        if all.len() < 20 && !all.iter().any(|(_, root)| path.starts_with(root)) {
            all.push(crate::cloud::place(path));
        }
    }

    /// What a walk found, in one transaction: rows at and below each of `gone` go, with their
    /// text and hashes; new or changed files wait to be read again, and with `archives` (the
    /// folders read, when archives are looked into) the files inside a changed archive out of
    /// caches are those it has now, the members that kept their size and time keeping their
    /// text and vectors; the folders left out get their totals. `all` is a walk of every root,
    /// which knows every folder left out.
    fn apply(&self, gone: &[String], changed: &[Changed], skipped: &[(String, Size)], all: bool, archives: Option<&[PathBuf]>) -> rusqlite::Result<()> {
        // Archives are listed before the store is locked: searches go on meanwhile.
        let inside: Vec<(&str, Vec<Member>)> = changed
            .iter()
            .filter(|(path, ..)| crate::archive::is_archive(Path::new(path)))
            .map(|(path, size, ..)| {
                let entries = if archives.is_some_and(|roots| !in_cache(Path::new(path), roots)) { crate::archive::search_entries(Path::new(path), *size).unwrap_or_default() } else { vec![] };
                (path.as_str(), entries.into_iter().filter(|e| !e.is_dir).filter_map(|e| Some((key(&Path::new(path).join(&e.name))?, e.size, e.modified))).collect())
            })
            .collect();
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        if all {
            tx.execute("DELETE FROM skipped", [])?;
        }
        // The files whose passages lose their vectors, for the signs kept in memory.
        let mut stale: HashSet<i64> = HashSet::new();
        for path in gone {
            let (from, to) = below(path);
            let at = "(path = ?1 OR (path > ?2 AND path < ?3))";
            let mut q = tx.prepare(&format!("SELECT id FROM files WHERE {at}"))?;
            stale.extend(q.query_map([path, &from, &to], |r| r.get::<_, i64>(0))?.flatten());
            drop(q);
            tx.execute(&format!("DELETE FROM text WHERE rowid IN (SELECT id FROM files WHERE {at})"), [path, &from, &to])?;
            tx.execute(&format!("DELETE FROM chunks WHERE file IN (SELECT id FROM files WHERE {at})"), [path, &from, &to])?;
            for table in ["files", "hashes", "skipped"] {
                tx.execute(&format!("DELETE FROM {table} WHERE {at}"), [path, &from, &to])?;
            }
        }
        // One row goes, with its text and vectors.
        let mut drop_row = |id: i64, whole: bool| -> rusqlite::Result<()> {
            stale.insert(id);
            tx.prepare_cached("DELETE FROM text WHERE rowid = ?1")?.execute([id])?;
            tx.prepare_cached("DELETE FROM chunks WHERE file = ?1")?.execute([id])?;
            if whole {
                tx.prepare_cached("DELETE FROM files WHERE id = ?1")?.execute([id])?;
            }
            Ok(())
        };
        for (path, size, modified, cloud) in changed {
            if let Ok(id) = tx.prepare_cached("SELECT id FROM files WHERE path = ?1")?.query_row([path], |r| r.get::<_, i64>(0)) {
                drop_row(id, false)?;
            }
            // Gone back to the cloud: its hash goes with its text.
            if *cloud {
                tx.prepare_cached("DELETE FROM hashes WHERE path = ?1")?.execute([path])?;
            }
            tx.prepare_cached(
                "INSERT INTO files(path, size, modified, has_text, cloud) VALUES (?1, ?2, ?3, NULL, ?4)
                 ON CONFLICT(path) DO UPDATE SET size = excluded.size, modified = excluded.modified, has_text = NULL, embedded = NULL, cloud = excluded.cloud",
            )?
            .execute(params![path, *size as i64, modified, cloud.then_some(1)])?;
        }
        for (archive, entries) in &inside {
            let (from, to) = below(archive);
            // The members as the store has them: those with the same size and time stay as they are.
            let mut had: HashMap<String, (i64, i64, i64)> = tx
                .prepare_cached("SELECT path, id, size, modified FROM files WHERE path > ?1 AND path < ?2")?
                .query_map([&from, &to], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?
                .collect::<rusqlite::Result<_>>()?;
            for (path, size, modified) in entries {
                match had.remove(path) {
                    Some((_, s, m)) if s == *size as i64 && m == *modified as i64 => continue,
                    Some((id, ..)) => drop_row(id, false)?,
                    None => {}
                }
                tx.prepare_cached(
                    "INSERT INTO files(path, size, modified, has_text, inside) VALUES (?1, ?2, ?3, NULL, 1)
                     ON CONFLICT(path) DO UPDATE SET size = excluded.size, modified = excluded.modified, has_text = NULL, embedded = NULL, inside = 1",
                )?
                .execute(params![path, *size as i64, *modified as i64])?;
            }
            for (id, ..) in had.into_values() {
                drop_row(id, true)?;
            }
        }
        for (path, (bytes, files)) in skipped {
            tx.execute("INSERT OR REPLACE INTO skipped(path, bytes, files) VALUES (?1, ?2, ?3)", params![path, *bytes as i64, *files as i64])?;
        }
        // Committed with the signs held, so signs being read meanwhile have all of it or none.
        let mut signs = self.signs.lock().unwrap();
        tx.commit()?;
        if let Some(signs) = signs.as_mut().filter(|_| !stale.is_empty()) {
            signs.retain(|file| !stale.contains(&file));
        }
        Ok(())
    }

    /// The size and date the store has for this file, and whether it was left in the cloud.
    fn row(&self, path: &str) -> Option<(u64, i64, bool)> {
        self.db.lock().unwrap().query_row("SELECT size, modified, cloud IS NOT NULL FROM files WHERE path = ?1", [path], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?))).ok()
    }

    /// Whether the store has anything below this folder, or has it as a folder left out.
    fn has(&self, dir: &str) -> bool {
        let (from, to) = below(dir);
        let db = self.db.lock().unwrap();
        db.query_row("SELECT 1 FROM files WHERE path > ?1 AND path < ?2 LIMIT 1", [&from, &to], |_| Ok(())).is_ok()
            || db.query_row("SELECT 1 FROM skipped WHERE path = ?1 OR (path > ?2 AND path < ?3) LIMIT 1", [dir, &from, &to], |_| Ok(())).is_ok()
    }

    /// Files still to be read: (id, path, size, inside an archive).
    fn unread(&self) -> rusqlite::Result<Vec<(i64, String, u64, bool)>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare("SELECT id, path, size, inside IS NOT NULL FROM files WHERE has_text IS NULL")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64, r.get(3)?)))?;
        rows.collect()
    }

    /// Search inside archives was turned on or off, or which archives changed: the files
    /// inside go, and when it is on, every archive is listed again.
    fn archives_changed(&self, on: bool) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        // "2": archives in caches are left out.
        let now = if on { "2" } else { "0" };
        let before: String = db.query_row("SELECT value FROM meta WHERE key = 'archives'", [], |r| r.get(0)).unwrap_or_default();
        if before == now {
            return Ok(());
        }
        let tx = db.transaction()?;
        tx.execute_batch(
            "DELETE FROM text WHERE rowid IN (SELECT id FROM files WHERE inside IS NOT NULL);
             DELETE FROM chunks WHERE file IN (SELECT id FROM files WHERE inside IS NOT NULL);
             DELETE FROM files WHERE inside IS NOT NULL;",
        )?;
        if on {
            let paths: Vec<String> = tx.prepare("SELECT path FROM files")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            for path in paths.into_iter().filter(|p| crate::archive::is_archive(Path::new(p))) {
                // Not what is on disk: the next walk takes it as changed.
                tx.execute("UPDATE files SET modified = -1 WHERE path = ?1", [path])?;
            }
        }
        tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('archives', ?1)", [now])?;
        tx.commit()?;
        *self.signs.lock().unwrap() = None;
        Ok(())
    }

    /// `text_exclude` changed: files a new pattern leaves out lose their text and vectors, and
    /// files a pattern no longer leaves out are read again. Folders follow from the walk.
    fn exclude_changed(&self, now: &[String]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let before: String = db.query_row("SELECT value FROM meta WHERE key = 'exclude'", [], |r| r.get(0)).unwrap_or_default();
        let joined = now.join("\n");
        if before == joined {
            return Ok(());
        }
        let tx = db.transaction()?;
        // SQLite's GLOB knows `*` and `?` as `text_exclude` does; brackets mean the same too.
        let named = "(path GLOB '*/' || ?1 OR path GLOB '*\\' || ?1)";
        for gone in before.split('\n').filter(|p| for_files(p) && !now.iter().any(|n| n == p)) {
            tx.execute(&format!("UPDATE files SET has_text = NULL WHERE has_text = 0 AND {named}"), [gone])?;
        }
        for new in now.iter().filter(|n| for_files(n) && !before.split('\n').any(|p| p == n.as_str())) {
            tx.execute(&format!("DELETE FROM text WHERE rowid IN (SELECT id FROM files WHERE has_text = 1 AND {named})"), [new])?;
            tx.execute(&format!("DELETE FROM chunks WHERE file IN (SELECT id FROM files WHERE {named})"), [new])?;
            tx.execute(&format!("UPDATE files SET has_text = 0, embedded = NULL WHERE has_text = 1 AND {named}"), [new])?;
        }
        tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('exclude', ?1)", [joined])?;
        tx.commit()?;
        *self.signs.lock().unwrap() = None;
        Ok(())
    }

    /// Files read, in one transaction. `text` is `None` for a file without text.
    fn read(&self, files: &[(i64, Option<String>)]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for (id, text) in files {
            tx.prepare_cached("UPDATE files SET has_text = ?2 WHERE id = ?1")?.execute(params![id, text.is_some()])?;
            // A file read again may still have its text from before (a reader that learnt more
            // keeps it until now): the new one takes its place, or the insert fails the scan.
            tx.prepare_cached("DELETE FROM text WHERE rowid = ?1")?.execute([id])?;
            if let Some(text) = text {
                tx.prepare_cached("INSERT INTO text(rowid, body) VALUES (?1, ?2)")?.execute(params![id, text])?;
            }
        }
        if !files.is_empty() {
            tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('read_at', ?1)", [now().to_string()])?;
        }
        tx.commit()
    }

    /// Commits of the repository at `root`, (id, time, text), as rows whose text is searched;
    /// then only its newest `max` stay.
    fn put_commits(&self, root: &Path, commits: &[(String, u64, String)], max: usize) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for (hash, time, text) in commits {
            let path = crate::history::key(root, hash);
            if tx.execute("INSERT OR IGNORE INTO files(path, size, modified, has_text) VALUES (?1, 0, ?2, 1)", params![path, *time as i64])? == 1 {
                tx.execute("INSERT INTO text(rowid, body) VALUES (?1, ?2)", params![tx.last_insert_rowid(), text])?;
            }
        }
        tx.commit()?;
        let (from, to) = below(crate::history::key(root, "").trim_end_matches(std::path::MAIN_SEPARATOR));
        let mut q = db.prepare("SELECT path FROM files WHERE path > ?1 AND path < ?2 ORDER BY modified DESC LIMIT -1 OFFSET ?3")?;
        let old: Vec<String> = q.query_map(params![from, to, max as i64], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        drop(q);
        drop(db);
        if old.is_empty() { Ok(()) } else { self.apply(&old, &[], &[], false, None) }
    }

    /// Files that share their size with another and have no current hash: (path, size, modified).
    /// Files left in the cloud are neither.
    fn unhashed(&self) -> rusqlite::Result<Vec<(PathBuf, u64, u64)>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare(
            "SELECT f.path, f.size, f.modified FROM files f
             WHERE f.inside IS NULL AND f.cloud IS NULL AND f.size > 0 AND f.size IN (SELECT size FROM files WHERE inside IS NULL AND cloud IS NULL GROUP BY size HAVING count(*) > 1)
             AND NOT EXISTS (SELECT 1 FROM hashes h WHERE h.path = f.path AND h.size = f.size AND h.modified = f.modified)",
        )?;
        let rows = q.query_map([], |r| Ok((PathBuf::from(r.get::<_, String>(0)?), r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)))?;
        rows.collect()
    }

    /// Hashes of files that are gone or changed, wherever they are, go.
    fn prune_hashes(&self) -> rusqlite::Result<()> {
        let rows: Vec<(String, u64, u64)> = {
            let db = self.db.lock().unwrap();
            let mut q = db.prepare("SELECT path, size, modified FROM hashes")?;
            let rows = q.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        // So do those of files gone back to the cloud, which are not read again.
        let stale: Vec<String> = rows
            .into_iter()
            .filter(|(path, size, modified)| crate::cloud::keep_out(Path::new(path)) || Path::new(path).ancestors().any(crate::fs::protected) || std::fs::metadata(path).map_or(true, |m| (m.len(), secs(&m) as u64) != (*size, *modified)))
            .map(|r| r.0)
            .collect();
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for path in stale {
            tx.execute("DELETE FROM hashes WHERE path = ?1", [path])?;
        }
        tx.commit()
    }

    /// Search by meaning with this engine, or none.
    pub fn set_engine(&self, engine: Option<crate::meaning::Engine>) {
        self.meaning.store(engine.is_some(), Ordering::Relaxed);
        *self.engine.lock().unwrap() = engine.map(Arc::new);
    }

    /// Which model makes the vectors, when search by meaning is on.
    pub fn engine_id(&self) -> Option<String> {
        self.engine().map(|e| e.id())
    }

    /// Where the built-in model runs, when it makes the vectors.
    pub fn engine_runs(&self) -> Option<crate::meaning::Runs> {
        self.engine()?.runs()
    }

    /// The engine, when search by meaning is on.
    fn engine(&self) -> Option<Arc<crate::meaning::Engine>> {
        self.engine.lock().unwrap().clone().filter(|_| self.meaning.load(Ordering::Relaxed))
    }

    /// The vectors in the store were made by another model: they go, and every file gets new
    /// ones, since vectors of two models cannot be compared.
    /// The same model written another way (`bge-m3`, `bge-m3:latest`), or the same weights under
    /// another name (by `digest`), keeps them.
    fn model_is(&self, id: &str, digest: Option<&str>) -> rusqlite::Result<()> {
        let db = self.db.lock().unwrap();
        let meta = |key: &str| db.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get::<_, String>(0)).unwrap_or_default();
        let (before, weights) = (meta("meaning_model"), meta("meaning_digest"));
        if !crate::meaning::same_model(&before, id) && digest.is_none_or(|d| d != weights) {
            db.execute_batch("DELETE FROM chunks; UPDATE files SET embedded = NULL;")?;
            *self.signs.lock().unwrap() = None;
        }
        db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('meaning_model', ?1)", [id])?;
        if let Some(d) = digest {
            db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('meaning_digest', ?1)", [d])?;
        }
        Ok(())
    }

    /// Files with text whose passages have no vectors yet, and files that have them.
    pub fn meaning_counts(&self) -> (usize, usize) {
        let db = self.db.lock().unwrap();
        let count = |q: &str| db.query_row(q, [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize;
        (count("SELECT count(*) FROM files WHERE has_text = 1 AND embedded IS NULL"), count("SELECT count(*) FROM files WHERE embedded = 1"))
    }

    /// Passages that have their vectors.
    pub fn passage_count(&self) -> usize {
        self.db.lock().unwrap().query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    }

    /// Up to `n` files still to get their vectors, the last changed first: (id, path, text).
    fn unembedded(&self, n: usize) -> rusqlite::Result<Vec<(i64, String, String)>> {
        let db = self.db.lock().unwrap();
        // The files first, by `files_unembedded`, then their text: joined before the limit, the
        // text of every file still to go was read for each few (2 s a time with 300,000).
        let mut q = db.prepare(
            "SELECT f.id, f.path, coalesce(t.body, '') FROM (SELECT id, path, modified FROM files WHERE has_text = 1 AND embedded IS NULL ORDER BY modified DESC LIMIT ?1) f
             LEFT JOIN text t ON t.rowid = f.id ORDER BY f.modified DESC",
        )?;
        let rows = q.query_map([n as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    /// A file's passages' vectors, packed.
    fn put_vectors(&self, file: i64, vectors: &[Vec<u8>]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let had = tx.execute("DELETE FROM chunks WHERE file = ?1", [file])? > 0;
        for (n, v) in vectors.iter().enumerate() {
            tx.execute("INSERT INTO chunks(file, n, vector) VALUES (?1, ?2, ?3)", params![file, n as i64, v])?;
        }
        tx.execute("UPDATE files SET embedded = 1 WHERE id = ?1", [file])?;
        let mut signs = self.signs.lock().unwrap();
        tx.commit()?;
        if let Some(signs) = signs.as_mut() {
            // A file that changed lost its passages already: no pass over them all for it.
            if had {
                signs.retain(|f| f != file);
            }
            for (n, v) in vectors.iter().enumerate() {
                signs.push(file, n as u8, v);
            }
        }
        Ok(())
    }

    /// `f` on the signs of every passage, read first if they are not yet: on a connection of
    /// their own, so the store goes on answering meanwhile (two million passages take seconds).
    /// Writes that change them commit while holding them.
    fn with_signs<T>(&self, f: impl FnOnce(&Signs) -> T) -> T {
        let mut known = self.signs.lock().unwrap();
        if known.is_none() {
            let read = || -> rusqlite::Result<Signs> {
                let db = Connection::open_with_flags(&self.path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                db.busy_timeout(Duration::from_secs(10))?;
                let mut q = db.prepare("SELECT file, n, vector FROM chunks")?;
                let mut rows = q.query([])?;
                let mut out = Signs::default();
                while let Some(r) = rows.next()? {
                    out.push(r.get(0)?, r.get::<_, i64>(1)? as u8, r.get_ref(2)?.as_blob()?);
                }
                Ok(out)
            };
            *known = Some(read().unwrap_or_default());
        }
        f(known.as_ref().unwrap())
    }

    /// Read the signs ahead of the first search by meaning, when it is on.
    pub fn warm_signs(&self) {
        if self.engine().is_some() {
            self.with_signs(|_| ());
        }
    }

    /// Files whose passages mean what `query` asks, closest first, each with the start of
    /// the passage that was closest. A file's score is its best passage's, and a little more
    /// for each further passage that matches, up to four. Only files below `scope`, when given.
    /// Nothing while search by meaning is off.
    // ponytail: the sieve keeps the 1000 closest passages of every folder before the scope
    // is applied; a narrow scope can come up short. Sieve per scope if users notice.
    pub fn similar(&self, query: &str, scope: Option<&Path>, max: usize) -> Vec<Hit> {
        self.closest(query, max, true, |p| scope.is_none_or(|s| p.starts_with(s)))
            .into_iter()
            .map(|(path, s, n, all)| {
                let words: Vec<&str> = all[n].text.split_whitespace().collect();
                let snippet = if words.len() > 24 { format!("{} …", words[..24].join(" ")) } else { words.join(" ") };
                Hit { path, is_dir: false, snippet: Some(snippet), similar: Some(s) }
            })
            .collect()
    }

    /// What Ask gives the chat model to answer from: excerpts of the files closest to what
    /// `question` asks, about `bytes` long in all, one source per file with its excerpts in the
    /// file's order. Each hit comes with the passages before and after it, so it reads as a
    /// whole, and the strongest files give more of their hits (four from the first, three from
    /// the second, …) than weak ones, which give one. Only files below `scope`, when given.
    // ponytail: per-file passage ranges by position only; a file cut to `PASSAGES` passages
    // may join two that were not next to each other (`join` then keeps both whole).
    pub fn passages(&self, question: &str, scope: Option<&Path>, bytes: usize) -> Vec<(PathBuf, String)> {
        let mut per_file: HashMap<PathBuf, usize> = HashMap::new();
        let hits = self.closest(question, ASK_HITS, false, |path| {
            if scope.is_some_and(|s| !path.starts_with(s)) {
                return false;
            }
            // Only the files that can give an excerpt have their text read.
            if per_file.len() >= ASK_FILES && !per_file.contains_key(path) {
                return false;
            }
            let n = per_file.entry(path.to_path_buf()).or_default();
            *n += 1;
            *n <= 4
        });
        // The files in the order of their best hit, each with its hits, best first.
        let mut files: Vec<(PathBuf, std::rc::Rc<Vec<crate::meaning::Passage>>, Vec<usize>)> = vec![];
        for (path, _, n, all) in hits {
            match files.iter_mut().find(|f| f.0 == path) {
                Some(f) => f.2.push(n),
                None => files.push((path, all, vec![n])),
            }
        }
        let mut left = bytes;
        let mut seen = HashSet::new();
        let mut out = vec![];
        for (rank, (path, all, hits)) in files.into_iter().enumerate() {
            let header = path.as_os_str().len() + 8;
            let mut taken: Vec<usize> = vec![];
            for &n in hits.iter().take(4usize.saturating_sub(rank).max(1)) {
                // With its neighbours if they fit, else alone.
                for window in [n.saturating_sub(1)..=(n + 1).min(all.len() - 1), n..=n] {
                    let new: Vec<usize> = window.filter(|i| !taken.contains(i)).collect();
                    let cost = new.iter().map(|&i| all[i].text.len() + 1).sum::<usize>() + if taken.is_empty() { header } else { 0 };
                    if cost <= left {
                        left -= cost;
                        taken.extend(new);
                        break;
                    }
                }
            }
            taken.sort_unstable();
            // Runs of neighbouring passages read as one excerpt; the same text in two files
            // (a copy, a translation left as it was) is given once.
            let mut excerpts: Vec<String> = vec![];
            let mut i = 0;
            while i < taken.len() {
                let mut text = all[taken[i]].text.clone();
                while i + 1 < taken.len() && taken[i + 1] == taken[i] + 1 {
                    i += 1;
                    crate::meaning::join(&mut text, &all[taken[i]].text);
                }
                i += 1;
                if seen.insert(text.clone()) {
                    excerpts.push(text);
                }
            }
            if !excerpts.is_empty() {
                out.push((path, excerpts.join(crate::ask::GAP)));
            }
        }
        out
    }

    /// Up to `want` passages near what `query` means, closest first, with their file and
    /// score: those near the best one, above what unrelated text scores, and of files `accept`
    /// takes; with `by_file`, the best passage of each file, ranked by the file's score. Only
    /// the files that give a passage have their text read.
    fn closest(&self, query: &str, want: usize, by_file: bool, mut accept: impl FnMut(&Path) -> bool) -> Vec<(PathBuf, f32, usize, std::rc::Rc<Vec<crate::meaning::Passage>>)> {
        use crate::meaning::{pack, score, signs};
        let Some(engine) = self.engine() else { return vec![] };
        let q = match engine.query(query) {
            Ok(q) => q,
            Err(e) => {
                *self.meaning_error.lock().unwrap() = Some(e);
                return vec![];
            }
        };
        // The signs sieve out all but the thousand closest; their vectors say how close.
        let close = self.with_signs(|s| s.closest(&signs(&pack(&q)), 1000));
        let db = self.db.lock().unwrap();
        let Ok(mut vec) = db.prepare_cached("SELECT vector FROM chunks WHERE file = ?1 AND n = ?2") else { return vec![] };
        let mut ranked: Vec<(i64, f32, u8)> = close.into_iter().filter_map(|(file, n)| vec.query_row(params![file, n as i64], |r| Ok(score(r.get_ref(0)?.as_blob()?, &q))).ok().map(|s| (file, s, n))).collect();
        drop(vec);
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        // Near the best one, and above what unrelated text scores, by the model's scale.
        let top = ranked.first().map_or(0.0, |r| r.1);
        let floor = engine.floor().max(top - engine.window());
        ranked.retain(|r| r.1 >= floor);
        if by_file {
            ranked = by_files(ranked);
        }
        let offline: Vec<(String, String)> = self.offline.lock().unwrap().iter().map(|at| below(at)).collect();
        // A file's path as shown, and whether it is Markdown (its headings cut its passages).
        let mut paths: HashMap<i64, Option<(PathBuf, bool)>> = HashMap::new();
        let mut texts: HashMap<i64, std::rc::Rc<Vec<crate::meaning::Passage>>> = HashMap::new();
        let mut out = vec![];
        for (file, s, n) in ranked {
            if out.len() >= want {
                break;
            }
            let path = paths.entry(file).or_insert_with(|| {
                let path: String = db.query_row("SELECT path FROM files WHERE id = ?1", [file], |r| r.get(0)).ok()?;
                let markdown = crate::meaning::is_markdown(&path);
                (!offline.iter().any(|(from, to)| path > *from && path < *to)).then(|| (shown(path), markdown))
            });
            let Some((path, markdown)) = path.clone().filter(|(p, _)| accept(p)) else { continue };
            let passages = texts.entry(file).or_insert_with(|| {
                #[cfg(test)]
                self.bodies_read.fetch_add(1, Ordering::Relaxed);
                std::rc::Rc::new(db.query_row("SELECT body FROM text WHERE rowid = ?1", [file], |r| r.get::<_, String>(0)).map(|b| crate::meaning::passages(&b, markdown)).unwrap_or_default())
            });
            if (n as usize) < passages.len() {
                out.push((path, s, n as usize, passages.clone()));
            }
        }
        out
    }

    /// Files whose text has every word of `query`; the last word may be the start of one. When
    /// that finds fewer than `ENOUGH` files and there are two words or more, files with any of
    /// the words of four letters or more come after them, so a question typed as a question
    /// finds its file. Best matches first, each with the passage that matched; only files
    /// below `scope`, when given.
    pub fn search(&self, query: &str, scope: Option<&Path>, max: usize) -> Results {
        self.search_words(query, scope, max).0
    }

    /// `search`, and how many of the hits, at the top, have every word.
    pub fn search_words(&self, query: &str, scope: Option<&Path>, max: usize) -> (Results, usize) {
        /// Fewer files than this with every word: files with any of them follow.
        const ENOUGH: usize = 10;
        let start = Instant::now();
        let words: Vec<String> = query.split_whitespace().map(|w| w.replace('"', "")).filter(|w| !w.is_empty()).collect();
        if words.is_empty() {
            return (Results::default(), 0);
        }
        let last = words.len() - 1;
        let term = |i: usize, w: &String| format!("\"{w}\"{}", if i == last { "*" } else { "" });
        let all = words.iter().enumerate().map(|(i, w)| term(i, w)).collect::<Vec<_>>().join(" ");
        let mut found = self.matching(&all, scope, max);
        let every = found.0.len();
        let any: Vec<String> = words.iter().enumerate().filter(|(_, w)| w.chars().count() > 3).map(|(i, w)| term(i, w)).collect();
        if found.0.len() < ENOUGH && words.len() > 1 && !any.is_empty() {
            let (more, total) = self.matching(&any.join(" OR "), scope, max);
            // Every file with all the words has some of them: the count of these holds both.
            found.1 = found.1.max(total);
            for hit in more {
                if found.0.len() >= max {
                    break;
                }
                if !found.0.iter().any(|h| h.path == hit.path) {
                    found.0.push(hit);
                }
            }
        }
        (Results { hits: found.0, total: found.1, micros: start.elapsed().as_micros() as u64 }, every)
    }

    /// The files whose text matches the FTS query `ask`, best first, up to `max`, and how many
    /// there are; only below `scope`, when given.
    fn matching(&self, ask: &str, scope: Option<&Path>, max: usize) -> (Vec<Hit>, usize) {
        let db = self.db.lock().unwrap();
        // Disks that are not plugged in keep their rows, out of sight.
        let offline: Vec<(String, String)> = self.offline.lock().unwrap().iter().map(|at| below(at)).collect();
        let mut filter: String = (0..offline.len()).map(|i| format!(" AND NOT (f.path > ?{} AND f.path < ?{})", 2 + 2 * i, 3 + 2 * i)).collect();
        let mut args: Vec<rusqlite::types::Value> = std::iter::once(ask.to_string()).chain(offline.into_iter().flat_map(|(from, to)| [from, to])).map(Into::into).collect();
        // A scope: the files below it, and the commits of the repositories below it.
        if let Some((from, to)) = scope.and_then(key).map(|k| below(&k)) {
            let n = args.len();
            filter += &format!(" AND ((f.path > ?{a} AND f.path < ?{b}) OR (f.path > ?{c} AND f.path < ?{d}))", a = n + 1, b = n + 2, c = n + 3, d = n + 4);
            args.extend([from.clone(), to.clone(), format!("git:{from}"), format!("git:{to}")].map(Into::into));
        }
        let found = || -> rusqlite::Result<(Vec<Hit>, usize)> {
            let total = db.query_row(&format!("SELECT count(*) FROM text JOIN files f ON f.id = text.rowid WHERE text MATCH ?1{filter}"), rusqlite::params_from_iter(&args), |r| r.get::<_, i64>(0))? as usize;
            let mut q = db.prepare(&format!(
                "SELECT f.path, snippet(text, 0, char(1), char(2), '…', 18), CASE WHEN f.path LIKE 'git:%' THEN substr(text.body, 1, instr(text.body, char(10)) - 1) END FROM text JOIN files f ON f.id = text.rowid
                 WHERE text MATCH ?1{filter} ORDER BY rank LIMIT ?{}",
                args.len() + 1
            ))?;
            let hits = q.query_map(rusqlite::params_from_iter(args.iter().cloned().chain([(max as i64).into()])), |r| {
                let mut snippet = r.get::<_, String>(1)?.split_whitespace().collect::<Vec<_>>().join(" ");
                // A commit says which it is ("commit a1b2c3d · author · date") before what matched.
                if let Some(head) = r.get::<_, Option<String>>(2)?.filter(|h| !snippet.replace([MARK.0, MARK.1], "").contains(h.as_str())) {
                    snippet = format!("{head} · {snippet}");
                }
                Ok(Hit { path: shown(r.get(0)?), is_dir: false, snippet: Some(snippet), similar: None })
            })?;
            Ok((hits.collect::<rusqlite::Result<_>>()?, total))
        };
        found().unwrap_or_default()
    }
}

// ---------------------------------------------------------------- what it holds

/// What a search store holds, for its preview: read without writing anything.
#[derive(Debug, Default)]
pub struct Facts {
    /// Files known (with their sizes), and of them those read for their text.
    pub files: usize,
    pub read: usize,
    /// Files read, by extension ("" for none), most first; commits and files inside archives
    /// on their own.
    pub kinds: Vec<(String, usize)>,
    pub commits: usize,
    pub inside: usize,
    /// Files with no text to read, files only in the cloud (not read), files still to read.
    pub no_text: usize,
    pub cloud: usize,
    pub waiting: usize,
    /// Passages with their vectors, the files they are of, the files still to get them, and the
    /// model that made them.
    pub passages: usize,
    pub with_vectors: usize,
    pub vectors_waiting: usize,
    pub model: String,
    /// The latest errors, newest first: when, the file, what.
    pub errors: Vec<(u64, Option<String>, String)>,
    /// Bytes of each part: text, vectors, hashes, files, the rest.
    pub parts: Vec<(&'static str, u64)>,
    /// When a file was last read, in seconds since 1970.
    pub read_at: Option<u64>,
}

/// The facts of the store in `path`, opened read-only: an error when it is no search store.
pub fn facts(path: &Path) -> Result<Facts, String> {
    use rusqlite::OpenFlags;
    let mut head = [0u8; 16];
    std::io::Read::read_exact(&mut std::fs::File::open(path).map_err(|e| e.to_string())?, &mut head).map_err(|e| e.to_string())?;
    if &head != b"SQLite format 3\0" {
        return Err("not a database".into());
    }
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|e| e.to_string())?;
    let tables: HashSet<String> = db.prepare("SELECT name FROM sqlite_master WHERE type = 'table'").and_then(|mut q| q.query_map([], |r| r.get(0))?.collect()).map_err(|e| e.to_string())?;
    if !["files", "text", "chunks", "meta", "hashes", "skipped", "roots"].iter().all(|t| tables.contains(*t)) {
        return Err("not a search store".into());
    }
    let count = |q: &str| db.query_row(q, [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize;
    let meta = |k: &str| db.query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get::<_, String>(0)).ok();
    let mut f = Facts {
        files: count("SELECT count(*) FROM files WHERE path NOT LIKE 'git:%' AND inside IS NULL"),
        read: count("SELECT count(*) FROM files WHERE has_text = 1"),
        commits: count("SELECT count(*) FROM files WHERE path LIKE 'git:%'"),
        inside: count("SELECT count(*) FROM files WHERE has_text = 1 AND inside IS NOT NULL"),
        no_text: count("SELECT count(*) FROM files WHERE has_text = 0 AND cloud IS NULL"),
        cloud: count("SELECT count(*) FROM files WHERE cloud = 1"),
        waiting: count("SELECT count(*) FROM files WHERE has_text IS NULL"),
        passages: count("SELECT count(*) FROM chunks"),
        with_vectors: count("SELECT count(*) FROM files WHERE embedded = 1"),
        model: meta("meaning_model").unwrap_or_default(),
        read_at: meta("read_at").and_then(|v| v.parse().ok()),
        ..Default::default()
    };
    // Vectors wait only where a model made some, or is set to.
    if !f.model.is_empty() {
        f.vectors_waiting = count("SELECT count(*) FROM files WHERE has_text = 1 AND embedded IS NULL");
    }
    let mut kinds: HashMap<String, usize> = HashMap::new();
    if let Ok(mut q) = db.prepare("SELECT path FROM files WHERE has_text = 1 AND path NOT LIKE 'git:%'") {
        let paths = q.query_map([], |r| r.get::<_, String>(0)).map(|rows| rows.flatten().collect::<Vec<_>>()).unwrap_or_default();
        for p in paths {
            let ext = Path::new(&p).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            *kinds.entry(ext).or_default() += 1;
        }
    }
    f.kinds = kinds.into_iter().collect();
    f.kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    if tables.contains("errors") {
        f.errors = db
            .prepare("SELECT at, path, what FROM errors ORDER BY rowid DESC LIMIT 10")
            .and_then(|mut q| q.query_map([], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?)))?.collect())
            .unwrap_or_default();
    }
    // Each table and index's pages, by part: SQLite's own count of them.
    let mut parts: Vec<(&'static str, u64)> = ["text", "vectors", "hashes", "files", "rest"].map(|p| (p, 0)).to_vec();
    if let Ok(mut q) = db.prepare("SELECT name, sum(pgsize) FROM dbstat GROUP BY name") {
        let rows = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))).map(|r| r.flatten().collect::<Vec<_>>()).unwrap_or_default();
        for (name, bytes) in rows {
            let part = match name.trim_start_matches("sqlite_autoindex_") {
                n if n.starts_with("text") => 0,
                n if n.starts_with("chunks") => 1,
                n if n.starts_with("hashes") => 2,
                n if n.starts_with("files") => 3,
                _ => 4,
            };
            parts[part].1 += bytes;
        }
        f.parts = parts;
    }
    Ok(f)
}

// ---------------------------------------------------------------- filling it

/// The folders whose text is kept: the ones in the config, or the home folder.
pub(crate) fn roots(cfg: &SearchConfig) -> Vec<PathBuf> {
    if cfg.text_roots.is_empty() { std::env::home_dir().into_iter().collect() } else { cfg.text_roots.clone() }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Hidden folders, folders a tool made and can make again, folders marked "names only" and
/// folders holding `.nosearch`.
pub(crate) fn left_out(dir: &Path, cfg: &SearchConfig) -> bool {
    let name = dir.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    // Coxswain's own cache (the store, previews, files unpacked to be read) is never read: on
    // macOS and Windows it is not a hidden folder.
    static OWN: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    OWN.get_or_init(crate::helper::folder).as_deref() == Some(dir)
        || name.starts_with('.') || excluded(&name, cfg) || cfg.names_only.iter().any(|p| p == dir) || dir.join(".nosearch").exists() || cache_tagged(dir)
}

/// A folder a program marked as its cache, as backup tools know it (Cargo's `target`, whatever
/// its name, and other build and cache folders): a `CACHEDIR.TAG` that starts with the
/// signature of the Cache Directory Tagging Specification.
fn cache_tagged(dir: &Path) -> bool {
    const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";
    let mut start = [0; SIGNATURE.len()];
    std::fs::File::open(dir.join("CACHEDIR.TAG")).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut start)).is_ok_and(|_| start == SIGNATURE)
}

/// Folders of caches, package stores, build output and programs' data: the archives in them
/// are a program's, not the user's, and are not looked into (unless `archives_everywhere`, for
/// their names), wherever the folders read are.
pub(crate) fn cache_folder(dir: &Path) -> bool {
    const NAMES: &[&str] = &[".cache", ".cargo", ".npm", ".m2", ".gradle", ".rustup", ".pnpm-store", "node_modules", "target"];
    static AT: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    let at = AT.get_or_init(|| {
        let home = std::env::home_dir().unwrap_or_default();
        let mut v: Vec<PathBuf> = [dirs::cache_dir(), dirs::data_dir(), dirs::data_local_dir()].into_iter().flatten().collect();
        v.extend([".local/share", "Library/Caches", "AppData/Local"].map(|p| home.join(p)));
        v.extend(["/var/cache", "/var/lib"].map(PathBuf::from));
        v
    });
    dir.file_name().is_some_and(|n| NAMES.iter().any(|x| n == *x)) || at.iter().any(|p| p == dir)
}

/// Whether the archive at `path` is in a cache folder at or below one of `roots`.
pub(crate) fn in_cache(path: &Path, roots: &[PathBuf]) -> bool {
    path.ancestors().skip(1).take_while(|a| roots.iter().any(|r| a.starts_with(r))).any(cache_folder)
}

/// Whether a folder or file of this name is left out by `text_exclude`: a name (`node_modules`)
/// or a pattern with `*` and `?` (`*.log`, `secret*`).
pub(crate) fn excluded(name: &str, cfg: &SearchConfig) -> bool {
    let name: Vec<char> = name.chars().collect();
    cfg.text_exclude.iter().any(|x| crate::index::glob(&x.chars().collect::<Vec<_>>(), &name))
}

/// Whether a `text_exclude` entry is about files too: one with `*`, `?` or a dot (`*.log`,
/// `notes.txt`). A plain name (`build`) leaves out folders only.
fn for_files(entry: &str) -> bool {
    entry.contains(['*', '?', '.'])
}

/// A file whose name `text_exclude` leaves out: found by name and counted, never read.
fn left_out_file(path: &str, cfg: &SearchConfig) -> bool {
    let name: Vec<char> = path.rsplit(['/', '\\']).next().unwrap_or(path).chars().collect();
    cfg.text_exclude.iter().filter(|x| for_files(x)).any(|x| crate::index::glob(&x.chars().collect::<Vec<_>>(), &name))
}

/// What a walk of some folders found.
#[derive(Default)]
struct Walk {
    /// Files that differ from what the store knew.
    changed: Vec<Changed>,
    /// Folders left out, with their totals.
    skipped: Vec<(String, Size)>,
    /// Every file.
    seen: HashSet<String>,
    /// Folders that are git work trees (they hold a `.git`).
    repos: Vec<PathBuf>,
}

/// Whether the file `meta` describes is only in the cloud and the settings leave it there: it is
/// found by name, never read. Its cloud is remembered either way.
fn left_in_cloud(store: &Store, meta: &std::fs::Metadata, path: &Path) -> bool {
    if !crate::cloud::online_meta(meta, path) {
        return false;
    }
    store.saw_cloud(path);
    crate::cloud::keep_out_meta(meta, path)
}

/// Walk `dirs` and everything below them. `None` when `stop` was set meanwhile.
fn walk(store: &Store, dirs: Vec<PathBuf>, cfg: &SearchConfig, known: &Known, stop: &AtomicBool) -> Option<Walk> {
    let mut found = Walk::default();
    // Folders left out are only measured, one thread, like the rest of the scan.
    let slow = rayon::ThreadPoolBuilder::new().num_threads(1).build().expect("a thread");
    let mut stack = dirs;
    while let Some(dir) = stack.pop() {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        for entry in crate::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let (Ok(kind), path) = (entry.file_type(), entry.path()) else { continue };
            // Other apps' data on a Mac: not read, not measured, not even looked at.
            if kind.is_dir() && crate::fs::protected(&path) {
                continue;
            }
            if entry.file_name() == ".git" {
                found.repos.push(dir.clone());
            }
            if kind.is_dir() {
                // A cloud mount whose files are not read is not walked either: listing it may
                // be the network's work. Its names are the name index's.
                if crate::cloud::unread_mount(&path) {
                    store.saw_cloud(&path);
                } else if !left_out(&path, cfg) {
                    stack.push(path);
                } else if let (Some(text), Some(size)) = (path.to_str(), slow.install(|| crate::sizes::walk(&path, stop))) {
                    found.skipped.push((text.to_string(), size));
                }
                continue;
            }
            let (true, Some(text), Ok(meta)) = (kind.is_file(), path.to_str(), entry.metadata()) else { continue };
            let modified = secs(&meta);
            let cloud = left_in_cloud(store, &meta, &path);
            if known.get(text).is_none_or(|k| *k != (meta.len(), modified, cloud)) {
                found.changed.push((text.to_string(), meta.len(), modified, cloud));
            }
            found.seen.insert(text.to_string());
        }
    }
    Some(found)
}

/// Hits Ask looks at, four of a file at most, of `ASK_FILES` files at most.
const ASK_HITS: usize = 48;
const ASK_FILES: usize = 16;

/// Passages ranked one per file: the file's best, its score raised by 0.005 for each further
/// passage of it that matches, four at most, so a document that keeps coming back to a
/// question goes ahead of one that mentions it once.
fn by_files(ranked: Vec<(i64, f32, u8)>) -> Vec<(i64, f32, u8)> {
    let mut files: HashMap<i64, (f32, u8, usize)> = HashMap::new();
    for (file, s, n) in ranked {
        // Ranked best first: the first passage of a file is its best.
        files.entry(file).or_insert((s, n, 0)).2 += 1;
    }
    let mut out: Vec<(i64, f32, u8)> = files.into_iter().map(|(file, (s, n, count))| (file, s + 0.005 * (count - 1).min(4) as f32, n)).collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// Vectors of passages made another way than `scheme` go, the text stays, and the files get
/// new ones in the background. Checked when the store is opened, by whichever process opens
/// it first: how many files are being renewed, kept in the meta (`renew`) until they are done.
fn passages_are(db: &Connection, scheme: &str) -> rusqlite::Result<usize> {
    let meta = |key: &str| db.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get::<_, String>(0)).unwrap_or_default();
    if meta("passages") != scheme {
        let tx = db.unchecked_transaction()?;
        let had = tx.execute("UPDATE files SET embedded = NULL WHERE embedded IS NOT NULL", [])?;
        tx.execute("DELETE FROM chunks", [])?;
        tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('passages', ?1)", [scheme])?;
        if had > 0 {
            tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('renew', ?1)", [had.to_string()])?;
        }
        tx.commit()?;
    }
    Ok(meta("renew").parse().unwrap_or(0))
}

/// Bring the store up to date with the disk, once. Reads and hashes at half speed, one file
/// at a time, so the machine stays the user's. Stops early when `stop` is set.
pub fn scan(store: &Store, cfg: &SearchConfig, stop: &AtomicBool) -> rusqlite::Result<()> {
    let began = now();
    let (roots, offline) = place(store, cfg)?;
    store.tools_changed(&crate::extract::installed::extensions())?;
    // Diagrams got a sentence per arrow.
    store.readers_changed("diagrams-2", &["drawio", "dio", "mmd", "mermaid", "dot", "gv", "puml", "plantuml", "pu", "iuml", "wsd", "md", "markdown", "mdx"])?;
    store.archives_changed(cfg.archives)?;
    store.exclude_changed(&cfg.text_exclude)?;
    let known = store.known()?;
    let Some(found) = walk(store, roots.clone(), cfg, &known, stop) else { return Ok(()) };
    // Rows of a disk that is not plugged in stay.
    let kept: Vec<(String, String)> = offline.iter().map(|at| below(at)).collect();
    // Commits are not files on disk: `history` keeps them.
    let gone: Vec<String> = known.into_keys().filter(|path| !path.starts_with("git:") && !found.seen.contains(path) && !kept.iter().any(|(from, to)| path > from && path < to)).collect();
    *store.offline.lock().unwrap() = offline;
    store.apply(&gone, &found.changed, &found.skipped, true, cfg.archives.then_some(&roots[..]))?;
    *store.walked.lock().unwrap() = Some((roots, began));
    if read(store, cfg, stop)? {
        return Ok(());
    }
    // Git in a repository left in the cloud would download it.
    let repos = if cfg.history { found.repos.into_iter().filter(|r| !crate::cloud::git_kept_out(r)).collect() } else { vec![] };
    if history(store, &repos, &kept, stop)? {
        return Ok(());
    }

    store.prune_hashes()?;
    if hash(store, stop)? {
        return Ok(());
    }
    embed(store, stop)?;
    store.hurry.store(false, Ordering::Relaxed);
    Ok(())
}

/// Hash the files that share a size. `true` when stopped.
fn hash(store: &Store, stop: &AtomicBool) -> rusqlite::Result<bool> {
    for batch in store.unhashed()?.chunks(50) {
        let start = Instant::now();
        let rows: Vec<_> = batch.iter().filter_map(|(path, size, modified)| Some((path.clone(), *size, *modified, crate::dupes::hash_file(path, None, stop, None).ok()?.to_hex().to_string()))).collect();
        store.put_hashes(&rows)?;
        if stop.load(Ordering::Relaxed) {
            return Ok(true);
        }
        store.rest(start, stop);
    }
    Ok(false)
}

/// Give the passages of files with text their vectors, when search by meaning is on: a few
/// files at a time, resting as long as the work took, never on battery unless *Index now*.
// ponytail: files the watcher brings get theirs at the next scan, within ten minutes; the
// text is searchable by its words at once.
fn embed(store: &Store, stop: &AtomicBool) -> rusqlite::Result<()> {
    use crate::meaning::NoVectors;
    let Some(engine) = store.engine() else { return Ok(()) };
    store.model_is(&engine.id(), engine.digest().as_deref())?;
    // Files the server refused last time get one more try per scan; one it refuses for good
    // costs one quick answer every ten minutes, and never holds up the rest.
    store.db.lock().unwrap().execute("UPDATE files SET embedded = NULL WHERE embedded = 0", [])?;
    loop {
        let files = store.unembedded(8)?;
        if files.is_empty() {
            if store.renewing.swap(0, Ordering::Relaxed) > 0 {
                store.db.lock().unwrap().execute("DELETE FROM meta WHERE key = 'renew'", [])?;
            }
            return Ok(());
        }
        let (start, count) = (Instant::now(), files.len());
        for (id, path, body) in files {
            let texts: Vec<String> = crate::meaning::passages(&body, crate::meaning::is_markdown(&path)).iter().map(|p| crate::meaning::shown_to_model(&path, p)).collect();
            if texts.is_empty() {
                store.put_vectors(id, &[])?;
                continue;
            }
            // A server that does not answer: the file waits for the next scan, word search goes on.
            let vectors = match engine.passages(&texts) {
                Ok(v) => v,
                Err(NoVectors::Down(e)) => {
                    store.note_error(Some(path.as_str()), &e);
                    *store.meaning_error.lock().unwrap() = Some(e);
                    return Ok(());
                }
                Err(NoVectors::Refused(e)) => {
                    store.note_error(Some(path.as_str()), &e);
                    *store.meaning_error.lock().unwrap() = Some(e);
                    store.db.lock().unwrap().execute("UPDATE files SET embedded = 0 WHERE id = ?1", [id])?;
                    continue;
                }
            };
            *store.meaning_error.lock().unwrap() = None;
            store.put_vectors(id, &vectors.iter().map(|v| crate::meaning::pack(v)).collect::<Vec<_>>())?;
            if stop.load(Ordering::Relaxed) || store.cleared.load(Ordering::Relaxed) {
                return Ok(());
            }
        }
        // A file's time with the rest after it (not a wait for the mains), for the time the
        // files still to go take.
        // The built-in model works on this processor and rests as long as it worked; a server
        // works on its own (most often a GPU) and the helper only waits for it, so it rests a
        // quarter of that.
        let work = start.elapsed();
        let rest = if matches!(*engine, crate::meaning::Engine::Builtin(_)) { work } else { work / 4 };
        let ms = (work + rest.min(Duration::from_secs(2))).as_millis() as usize / count;
        store.rest_after(rest, stop);
        let before = store.ms_per_file.load(Ordering::Relaxed);
        store.ms_per_file.store(if before == 0 { ms } else { (before * 7 + ms) / 8 }, Ordering::Relaxed);
    }
}

/// Where the roots are now: the ones to walk, and where the rows of those whose disk is not
/// plugged in are. A disk mounted somewhere else than before has its rows moved there.
fn place(store: &Store, cfg: &SearchConfig) -> rusqlite::Result<(Vec<PathBuf>, Vec<String>)> {
    let placed = store.roots()?;
    *store.configured.lock().unwrap() = roots(cfg);
    let (mut online, mut offline) = (vec![], vec![]);
    for root in roots(cfg) {
        let Some(text) = key(&root) else { continue };
        let known = placed.get(&text);
        // The folder is there and on the disk it was on: an empty mount point, with the disk
        // not plugged in, is on another disk.
        let volume = crate::machine::volume(&root);
        let same = match (known.and_then(|k| k.0.as_deref()), &volume) {
            (Some(was), Some((now, _))) => was == now,
            _ => true,
        };
        let at = if root.is_dir() && same {
            Some(root.clone())
        } else {
            // Not where it was set: its disk may be mounted elsewhere. A folder with files in it
            // on another disk is taken as it is (the disk was replaced).
            let elsewhere = known.and_then(|(volume, inside, _)| crate::machine::locate(volume.as_deref()?, Path::new(inside.as_deref()?))).filter(|p| p.is_dir());
            let full = || std::fs::read_dir(&root).is_ok_and(|mut d| d.next().is_some());
            elsewhere.or_else(|| full().then(|| root.clone()))
        };
        match at {
            Some(at) => {
                let Some(at_text) = key(&at) else { continue };
                let volume = if at == root { volume.clone() } else { crate::machine::volume(&at) };
                store.place(&text, volume, &at_text, known.map(|k| k.2.as_str()))?;
                online.push(at);
            }
            None => offline.extend(known.map(|k| k.2.clone())),
        }
    }
    Ok((online, offline))
}

/// Read the text of the files waiting for it: those on disk, then those inside archives, one
/// archive at a time and in one pass through it. `true` when stopped.
fn read(store: &Store, cfg: &SearchConfig, stop: &AtomicBool) -> rusqlite::Result<bool> {
    let (inside, unread): (Vec<_>, Vec<_>) = store.unread()?.into_iter().partition(|f| f.3);
    store.pending.store(unread.len() + inside.len(), Ordering::Relaxed);
    let done = |n: usize| {
        store.pending.fetch_sub(n, Ordering::Relaxed);
        stop.load(Ordering::Relaxed) || store.cleared.load(Ordering::Relaxed)
    };
    for batch in unread.chunks(200) {
        let start = Instant::now();
        let rows: Vec<_> = batch.iter().map(|(id, path, size, _)| (*id, if left_out_file(path, cfg) { None } else { crate::extract::text_of(Path::new(path), *size, cfg.text_max_size) })).collect();
        store.read(&rows)?;
        if done(batch.len()) {
            return Ok(true);
        }
        store.rest(start, stop);
    }
    let mut archives: HashMap<PathBuf, Vec<(i64, String)>> = HashMap::new();
    let mut lost = vec![];
    // Which archive a folder inside one is in: found once per folder, not per member.
    let mut holders: HashMap<PathBuf, Option<(PathBuf, String)>> = HashMap::new();
    for (id, path, ..) in inside {
        let path = Path::new(&path);
        let holder = path.parent().map(|dir| holders.entry(dir.to_path_buf()).or_insert_with(|| crate::archive::split(dir)).clone());
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        match (holder.flatten(), name) {
            (Some((archive, inner)), Some(name)) if !left_out_file(&path.to_string_lossy(), cfg) => {
                let inner = if inner.is_empty() { name } else { format!("{inner}/{name}") };
                archives.entry(archive).or_default().push((id, inner));
            }
            _ => lost.push((id, None)),
        }
    }
    store.read(&lost)?;
    for (archive, files) in archives {
        let start = Instant::now();
        let n = files.len();
        store.read(&read_inside(&archive, files, cfg.text_max_size))?;
        if done(n) {
            return Ok(true);
        }
        store.rest(start, stop);
    }
    Ok(false)
}

/// The text of `files` (id, path inside) of `archive`, as `text_of` reads them from disk, each
/// no larger than `max`, and no more than `archive::SEARCH_READ` bytes from the archive. Each
/// is unpacked into a folder of the cache only the user can read, and is gone once read.
fn read_inside(archive: &Path, files: Vec<(i64, String)>, max: u64) -> Vec<(i64, Option<String>)> {
    let want: HashMap<String, i64> = files.iter().map(|(id, inner)| (inner.clone(), *id)).collect();
    let mut texts: HashMap<i64, Option<String>> = files.iter().map(|(id, _)| (*id, None)).collect();
    let dir = crate::helper::folder().map(|d| d.join(format!("inside-{}", std::process::id())));
    if let Some(dir) = dir.as_ref().filter(|d| std::fs::create_dir_all(d).is_ok()) {
        #[cfg(unix)]
        let _ = std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
        let left = std::cell::Cell::new(crate::archive::SEARCH_READ);
        let size = std::fs::metadata(archive).map_or(0, |m| m.len());
        let wanted = |name: &str, size: u64| size > 0 && size <= max.min(left.get()) && want.contains_key(name);
        let _ = crate::archive::search_read(archive, size, &wanted, &mut |name, from| {
            let limit = max.min(left.get());
            let file = dir.join(name.rsplit('/').next().unwrap_or(name));
            let copied = std::fs::File::create(&file).and_then(|mut to| std::io::copy(&mut std::io::Read::take(from, limit + 1), &mut to));
            if let Ok(n) = copied {
                left.set(left.get().saturating_sub(n));
                if n <= limit {
                    texts.insert(want[name], crate::extract::text_of(&file, n, max));
                }
            }
            let _ = std::fs::remove_file(&file);
            left.get() > 0
        });
        let _ = std::fs::remove_dir(dir);
    }
    texts.into_iter().collect()
}

/// Commits a repository keeps in the store at most, newest first.
const HISTORY_MAX: usize = 2000;

/// The history of the repositories in `repos`: commit messages, authors and changed paths,
/// searched like the files' text. A repository whose HEAD moved on gets the new commits only;
/// one whose history was rewritten is read again; one that is gone (and not on a disk that is
/// not plugged in, `kept`) loses its commits. `true` when stopped.
fn history(store: &Store, repos: &[PathBuf], kept: &[(String, String)], stop: &AtomicBool) -> rusqlite::Result<bool> {
    let meta = |k: &str| -> Option<String> { store.db.lock().unwrap().query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get(0)).ok() };
    let set = |k: &str, v: Option<&str>| -> rusqlite::Result<()> {
        let db = store.db.lock().unwrap();
        match v {
            Some(v) => db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)", [k, v]),
            None => db.execute("DELETE FROM meta WHERE key = ?1", [k]),
        }
        .map(drop)
    };
    // Where each repository's commits are: below this key.
    let rows = |root: &Path| crate::history::key(root, "").trim_end_matches(std::path::MAIN_SEPARATOR).to_string();
    let had: Vec<String> = {
        let db = store.db.lock().unwrap();
        let mut q = db.prepare("SELECT substr(key, 9) FROM meta WHERE key LIKE 'history:%'")?;
        q.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let now: HashSet<String> = repos.iter().filter_map(|r| key(r)).collect();
    for root in had.iter().filter(|r| !now.contains(*r) && !kept.iter().any(|(from, to)| *r > from && *r < to)) {
        store.apply(&[rows(Path::new(root))], &[], &[], false, None)?;
        set(&format!("history:{root}"), None)?;
    }
    for repo in repos {
        if stop.load(Ordering::Relaxed) || store.cleared.load(Ordering::Relaxed) {
            return Ok(true);
        }
        let Some(root) = key(repo) else { continue };
        let old = meta(&format!("history:{root}"));
        let Some((head, grew)) = crate::history::head(repo, old.as_deref()) else { continue };
        if old.as_deref() == Some(head.as_str()) {
            continue;
        }
        let start = Instant::now();
        // Rewritten (or new): read from the start.
        if !grew {
            store.apply(&[rows(repo)], &[], &[], false, None)?;
        }
        let Ok(commits) = crate::history::for_search(repo, old.as_deref().filter(|_| grew), HISTORY_MAX) else { continue };
        store.put_commits(repo, &commits, HISTORY_MAX)?;
        set(&format!("history:{root}"), Some(&head))?;
        store.rest(start, stop);
    }
    Ok(false)
}

/// The store's sizes are as of now.
fn walked_now(store: &Store, began: u64) {
    if let Some((_, at)) = store.walked.lock().unwrap().as_mut() {
        *at = began;
    }
}

/// An archive written this recently (a download under way) is not listed yet: each change
/// would unpack it again. It waits in `paths` for the next round.
const SETTLE: Duration = Duration::from_secs(3);

/// Follow what the file watcher saw at `paths`: files that changed, came or went, and folders
/// that came. A change inside a folder left out only puts that folder in `later`, as measuring
/// one takes a while: `measure` does it. Nothing happens before a first scan has finished.
/// What is left in `paths` after is to be looked at again: archives still being written.
pub fn refresh(store: &Store, cfg: &SearchConfig, paths: &mut HashSet<PathBuf>, later: &mut HashSet<PathBuf>, stop: &AtomicBool) -> rusqlite::Result<()> {
    let Some((roots, _)) = store.walked.lock().unwrap().clone() else { return Ok(()) };
    let began = now();
    let (mut gone, mut changed, mut new, mut again) = (vec![], vec![], vec![], HashSet::new());
    for path in paths.iter() {
        let Some(text) = key(path) else { continue };
        let path = PathBuf::from(&text);
        let Some(root) = roots.iter().find(|r| path.starts_with(r) && path != **r) else { continue };
        // The topmost folder left out on the way down, this one included if it is a folder.
        let mut down: Vec<&Path> = path.ancestors().take_while(|a| a != root).collect();
        down.reverse();
        if down.iter().any(|a| crate::fs::protected(a)) {
            continue;
        }
        let is_dir = path.is_dir();
        if let Some(out) = down.iter().find(|a| (**a != path || is_dir) && left_out(a, cfg)) {
            later.insert(out.to_path_buf());
            continue;
        }
        // A `.nosearch` or `CACHEDIR.TAG` that came or went changes what its folder is.
        if path.file_name().is_some_and(|n| n == ".nosearch" || n == "CACHEDIR.TAG") {
            later.extend(path.parent().map(Path::to_path_buf));
        }
        match std::fs::symlink_metadata(&path) {
            Err(_) => gone.push(text),
            Ok(meta) if meta.is_file() => {
                let now = (meta.len(), secs(&meta), left_in_cloud(store, &meta, &path));
                if store.row(&text) != Some(now) {
                    let settling = cfg.archives && crate::archive::is_archive(&path) && meta.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < SETTLE);
                    if settling {
                        again.insert(path);
                    } else {
                        changed.push((text, now.0, now.1, now.2));
                    }
                }
            }
            // A folder that came, or was moved in: its files have not been seen.
            Ok(meta) if meta.is_dir() && !store.has(&text) => new.push(path),
            Ok(_) => {}
        }
    }
    *paths = again;
    let Some(found) = walk(store, new, cfg, &Known::default(), stop) else { return Ok(()) };
    changed.extend(found.changed);
    store.apply(&gone, &changed, &found.skipped, false, cfg.archives.then_some(&roots[..]))?;
    walked_now(store, began);
    read(store, cfg, stop).map(|_| ())
}

/// Measure the folders in `later` again, and take them from it: folders left out that
/// changed, and folders that stopped or started being left out.
pub fn measure(store: &Store, cfg: &SearchConfig, later: &mut HashSet<PathBuf>, stop: &AtomicBool) -> rusqlite::Result<()> {
    let began = now();
    let (mut gone, mut skipped, mut again) = (vec![], vec![], vec![]);
    for dir in later.drain() {
        let Some(text) = key(&dir) else { continue };
        gone.push(text.clone());
        if !dir.is_dir() {
            continue;
        }
        if !left_out(&dir, cfg) {
            again.push(dir);
        } else if let Some(size) = crate::sizes::walk(&dir, stop) {
            skipped.push((text, size));
        }
    }
    let Some(found) = walk(store, again, cfg, &Known::default(), stop) else { return Ok(()) };
    skipped.extend(found.skipped);
    store.apply(&gone, &found.changed, &skipped, false, cfg.archives.then(|| roots(cfg)).as_deref())?;
    walked_now(store, began);
    read(store, cfg, stop).map(|_| ())
}

/// Keep the store current until `stop`: a scan, then the watcher's `changes` within seconds
/// (inside folders left out within a minute), and a scan every ten minutes for what the
/// watcher missed.
pub fn keep_current(store: &Store, cfg: &SearchConfig, stop: &AtomicBool, changes: Option<std::sync::mpsc::Receiver<Vec<PathBuf>>>) {
    let report = |r: rusqlite::Result<()>| {
        if let Err(e) = r {
            eprintln!("coxswain: search store: {e}");
            store.note_error(None, &e.to_string());
            *store.error.lock().unwrap() = Some(e.to_string());
        }
    };
    while !stop.load(Ordering::Relaxed) {
        store.cleared.store(false, Ordering::SeqCst);
        let scanned = scan(store, cfg, stop);
        if scanned.is_ok() {
            *store.error.lock().unwrap() = None;
        }
        store.warm_signs();
        report(scanned);
        let (rest, mut refreshed, mut measured) = (Instant::now(), Instant::now(), Instant::now());
        let (mut now, mut later) = (HashSet::new(), HashSet::new());
        let again = || rest.elapsed() > Duration::from_secs(600) || store.hurry.load(Ordering::Relaxed) || store.cleared.load(Ordering::Relaxed);
        while !again() && !stop.load(Ordering::Relaxed) {
            match changes.as_ref().map(|c| c.recv_timeout(Duration::from_millis(200))) {
                Some(Ok(paths)) => now.extend(paths),
                Some(Err(std::sync::mpsc::RecvTimeoutError::Timeout)) => {}
                _ => std::thread::sleep(Duration::from_millis(200)),
            }
            if !now.is_empty() && refreshed.elapsed() > Duration::from_secs(2) {
                report(refresh(store, cfg, &mut now, &mut later, stop));
                refreshed = Instant::now();
            }
            if !later.is_empty() && measured.elapsed() > Duration::from_secs(60) {
                report(measure(store, cfg, &mut later, stop));
                measured = Instant::now();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_finds_text_follows_changes_and_forgets() {
        let d = std::env::temp_dir().join(format!("coxswain-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for sub in ["docs", "node_modules/pkg", ".hidden", "private", "mail", "build-x", "not-a-cache"] {
            std::fs::create_dir_all(d.join("home").join(sub)).unwrap();
        }
        let write = |name: &str, text: &[u8]| std::fs::write(d.join("home").join(name), text).unwrap();
        write("docs/budget.md", "# Rocket budget\n\nFuel is the largest cost of flight seven.\n".as_bytes());
        write("docs/notes.txt", "Book the ferry. Renew the domain.\n".as_bytes());
        write("docs/photo.bin", b"\x89PNG\x00\x00fuel");
        write("node_modules/pkg/index.js", b"const fuel = 1;\n");
        write(".hidden/secret.txt", b"fuel\n");
        write("private/diary.txt", b"fuel\n");
        write("private/.nosearch", b"");
        write("build-x/CACHEDIR.TAG", b"Signature: 8a477f597d28d172789f06886806bc55\n# Cargo's\n");
        write("build-x/notes.md", b"fuel for the cache");
        write("not-a-cache/CACHEDIR.TAG", b"anything else");
        write("not-a-cache/plan.md", b"harbour schedule");
        write("mail/inbox.txt", b"fuel\n");

        let cfg = SearchConfig { text_roots: vec![d.join("home")], names_only: vec![d.join("home/mail")], ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        scan(&store, &cfg, &go).unwrap();

        let names = |q: &str| store.search(q, None, 10).hits.iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names("fuel"), ["budget.md"], "not the binary, node_modules, the hidden folder, the .nosearch one, the names-only one or the cache-tagged one");
        assert_eq!(names("harbour"), ["plan.md"], "a CACHEDIR.TAG without the signature leaves its folder in");
        assert_eq!(names("rocket bud"), ["budget.md"], "every word, the last one begun");
        // No file has both words: the files with either come instead.
        let mut either = names("ferry fuel");
        either.sort();
        assert_eq!(either, ["budget.md", "notes.txt"]);
        assert_eq!(names("ferry and fuel"), names("ferry fuel"), "short words are left out of either");
        // A scope: the files below it alone.
        let docs = d.join("home/docs");
        assert_eq!(store.search("fuel", Some(&docs), 10).hits.len(), 1);
        assert_eq!(store.search("fuel", Some(&d.join("home/doc")), 10).total, 0, "a folder whose name starts the same is not below");
        assert_eq!(store.search("fuel", Some(&d.join("home/private")), 10).total, 0);
        assert_eq!(names("\"; DROP TABLE files"), [""; 0], "a query is words, never SQL");
        let hit = &store.search("largest", None, 10).hits[0];
        assert_eq!(hit.snippet.as_deref(), Some(format!("# Rocket budget Fuel is the {}largest{} cost of flight seven.", MARK.0, MARK.1).as_str()));
        assert_eq!(store.texts(), 4, "budget.md, notes.txt and both files of not-a-cache");

        // Sizes: the files' rows and the totals of the folders left out, as a walk counts them.
        let home = d.join("home");
        assert_eq!(store.size(&home).map(|s| s.0), Some(crate::fs::dir_size(&home)));
        assert_eq!(store.size(&home.join("node_modules")).map(|s| s.0), Some((16, 1)), "a folder left out has its total");
        assert_eq!(store.size(&home.join("node_modules/pkg")), None, "inside one, nothing is known");
        assert_eq!(store.size(&d), None, "outside the roots neither");

        // Hashes: files sharing a size get one; a file that goes loses it.
        let hash = |name: &str| {
            let meta = std::fs::metadata(home.join(name)).unwrap();
            store.hash(&home.join(name), meta.len(), secs(&meta) as u64)
        };
        assert!(hash("docs/notes.txt").is_none(), "its size is its own");
        write("docs/copy.txt", "Book the ferry. Renew the domain.\n".as_bytes());
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(hash("docs/notes.txt").unwrap(), hash("docs/copy.txt").unwrap());
        std::fs::remove_file(home.join("docs/copy.txt")).unwrap();

        // A file changes, one goes, one comes.
        std::thread::sleep(Duration::from_millis(1100));
        write("docs/notes.txt", "Book the train instead.\n".as_bytes());
        write("docs/new.rs", b"fn launch() {}\n");
        std::fs::remove_file(d.join("home/docs/budget.md")).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!((names("ferry"), names("train"), names("launch"), names("fuel")), (vec![], vec!["notes.txt".to_string()], vec!["new.rs".to_string()], vec![]));
        assert_eq!(store.pending.load(Ordering::Relaxed), 0);
        assert_eq!(store.db.lock().unwrap().query_row("SELECT count(*) FROM hashes", [], |r| r.get::<_, i64>(0)).unwrap(), 0, "the copy's hash went with it");

        // Deleting the index forgets everything, sizes included, until the next scan.
        store.clear().unwrap();
        assert_eq!((store.texts(), store.size(&home)), (0, None));

        // The store of another version is filled afresh.
        drop(store);
        Connection::open(d.join("search.db")).unwrap().pragma_update(None, "user_version", VERSION + 1).unwrap();
        assert_eq!(Store::open(&d.join("search.db")).unwrap().texts(), 0);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn store_leaves_files_only_in_the_cloud_there() {
        let d = std::env::temp_dir().join(format!("coxswain-store-cloud-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let home = d.join("home");
        std::fs::create_dir_all(home.join("OneDrive")).unwrap();
        let (plan, copy) = (home.join("OneDrive/plan.txt"), home.join("OneDrive/copy.txt"));
        std::fs::write(&plan, "Orbit at dawn, land by noon.\n").unwrap();
        std::fs::write(&copy, "Orbit at dawn, land by noon.\n").unwrap();
        let cfg = SearchConfig { text_roots: vec![home.clone()], ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        let found = |q: &str| store.search(q, None, 10).total;
        let hashes = || store.db.lock().unwrap().query_row("SELECT count(*) FROM hashes", [], |r| r.get::<_, i64>(0)).unwrap();
        let id = || store.db.lock().unwrap().query_row("SELECT id FROM files WHERE path = ?1", [key(&plan).unwrap()], |r| r.get::<_, i64>(0)).unwrap();
        let vectors = || store.db.lock().unwrap().query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0)).unwrap();

        // Kept on the device: read, hashed, given vectors like any file.
        scan(&store, &cfg, &go).unwrap();
        assert_eq!((found("orbit"), hashes()), (2, 2));
        store.put_vectors(id(), &[vec![0u8; 8]]).unwrap();
        assert_eq!(vectors(), 1);

        // "Free up space": both go back to the cloud. Their text, vectors and hashes go; their
        // names and sizes stay.
        crate::cloud::pretend(&plan, true);
        crate::cloud::pretend(&copy, true);
        scan(&store, &cfg, &go).unwrap();
        assert_eq!((found("orbit"), hashes(), vectors()), (0, 0, 0));
        assert_eq!(store.size(&home).map(|s| s.0), Some(crate::fs::dir_size(&home)));
        assert_eq!(store.clouds(), [("OneDrive".to_string(), home.join("OneDrive"))]);
        assert!(crate::extract::text_of(&plan, 30, 1 << 20).is_none(), "never read");
        assert!(store.unhashed().unwrap().is_empty(), "never hashed");

        // Downloaded again (opened, or kept on this device): read at the next scan.
        crate::cloud::pretend(&plan, false);
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(found("orbit"), 1);
        crate::cloud::pretend(&copy, false);
        // Windows lets nobody delete a database that is open.
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn store_keeps_the_history_of_repositories() {
        use crate::history::tests::{add, commit_as, repo};
        let Some(home) = repo("store") else { return };
        std::fs::write(home.join("engine.rs"), "fn main() {}\n").unwrap();
        add(&home);
        commit_as(&home, "Ada", 1_700_000_000, "Tune the rocket engine");
        let cfg = SearchConfig { text_roots: vec![home.clone()], ..SearchConfig::default() };
        let db = home.with_extension("db");
        let (store, go) = (Store::open(&db).unwrap(), AtomicBool::new(false));
        scan(&store, &cfg, &go).unwrap();
        let hits = |q: &str| store.search(q, None, 10).hits;
        let found = hits("rocket");
        assert_eq!(found.len(), 1);
        let at = crate::history::split(&found[0].path).expect("a commit's history folder");
        assert_eq!((at.target.as_path(), at.commit.as_ref().map(String::len)), (home.as_path(), Some(12)));
        assert!(found[0].snippet.as_deref().unwrap().starts_with("commit "), "{:?}", found[0].snippet);
        assert!(found[0].snippet.as_deref().unwrap().contains("· Ada ·"));
        assert_eq!(hits("Ada").len(), 1, "by its author");
        assert_eq!(hits("engine.rs").len(), 1, "by a path it changed");
        assert_eq!(store.size(&home).map(|s| s.0), Some(crate::fs::dir_size(&home)), "commits are no files in sizes");

        // HEAD moves on: only the new commit is added, the old one stays.
        std::fs::write(home.join("engine.rs"), "fn main() { valve(); }\n").unwrap();
        commit_as(&home, "Bob", 1_700_100_000, "Fix the fuel valve");
        scan(&store, &cfg, &go).unwrap();
        assert_eq!((hits("fuel").len(), hits("rocket").len()), (1, 1));
        let rows = || store.db.lock().unwrap().query_row("SELECT count(*) FROM files WHERE path LIKE 'git:%'", [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(rows(), 2);
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(rows(), 2, "nothing new: nothing read again");

        // Turned off, its commits go.
        scan(&store, &SearchConfig { history: false, ..cfg.clone() }, &go).unwrap();
        assert_eq!((rows(), hits("fuel").len()), (0, 0));
        drop(store);
        std::fs::remove_dir_all(home).unwrap();
        for end in ["db", "db-wal", "db-shm"] {
            let _ = std::fs::remove_file(db.with_extension(end));
        }
    }

    #[test]
    fn store_follows_the_watcher() {
        let d = std::env::temp_dir().join(format!("coxswain-store-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let home = d.join("home");
        for sub in ["docs", "node_modules"] {
            std::fs::create_dir_all(home.join(sub)).unwrap();
        }
        let write = |name: &str, text: &[u8]| std::fs::write(home.join(name), text).unwrap();
        write("docs/plan.txt", b"launch window in march\n");
        write("node_modules/a.js", b"x");
        let cfg = SearchConfig { text_roots: vec![home.clone()], ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        scan(&store, &cfg, &go).unwrap();
        let names = |q: &str| store.search(q, None, 10).hits.iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        let size = |p: &Path| store.size(p).map(|s| s.0);
        let mut later = HashSet::new();
        let saw = |paths: &[PathBuf], later: &mut HashSet<PathBuf>| refresh(&store, &cfg, &mut paths.iter().cloned().collect(), later, &go).unwrap();

        // A file comes, one goes, a folder is moved in.
        write("docs/budget.txt", b"fuel for march\n");
        std::fs::remove_file(home.join("docs/plan.txt")).unwrap();
        std::fs::create_dir_all(d.join("elsewhere/deep")).unwrap();
        std::fs::write(d.join("elsewhere/deep/orbit.txt"), b"orbit\n").unwrap();
        std::fs::rename(d.join("elsewhere"), home.join("moved")).unwrap();
        saw(&[home.join("docs/budget.txt"), home.join("docs/plan.txt"), home.join("moved")], &mut later);
        assert_eq!((names("march"), names("orbit")), (vec!["budget.txt".to_string()], vec!["orbit.txt".to_string()]));
        assert_eq!(size(&home), Some(crate::fs::dir_size(&home)));
        assert!(later.is_empty());

        // Inside a folder left out, and a folder that becomes one: measured later.
        write("node_modules/b.js", b"yy");
        write("moved/.nosearch", b"");
        saw(&[home.join("node_modules/b.js"), home.join("moved/.nosearch")], &mut later);
        assert_eq!(later, [home.join("node_modules"), home.join("moved")].into_iter().collect());
        measure(&store, &cfg, &mut later, &go).unwrap();
        assert_eq!(size(&home.join("node_modules")), Some((3, 2)));
        assert_eq!(names("orbit"), [""; 0], "a .nosearch folder loses its text");
        assert_eq!(size(&home), Some(crate::fs::dir_size(&home)));

        // A folder removed.
        std::fs::remove_dir_all(home.join("docs")).unwrap();
        saw(&[home.join("docs")], &mut later);
        assert_eq!(names("fuel"), [""; 0]);
        assert_eq!(size(&home), Some(crate::fs::dir_size(&home)));
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn store_leaves_out_archives_in_caches() {
        let roots = [PathBuf::from("/h"), PathBuf::from("/w/target/keep")];
        for (path, cached) in [("/h/p/a.zip", false), ("/h/p/node_modules/x/a.zip", true), ("/h/.cargo/registry/a.tar.gz", true), ("/h/target.zip", false), ("/w/target/keep/a.zip", false), ("/elsewhere/target/a.zip", false)] {
            assert_eq!(in_cache(Path::new(path), &roots), cached, "{path}");
        }
        let cache = dirs::cache_dir().unwrap();
        assert!(in_cache(&cache.join("a.zip"), std::slice::from_ref(&cache)), "a folder read that is one");
    }

    #[test]
    fn store_reads_inside_archives_and_follows_their_changes() {
        let d = std::env::temp_dir().join(format!("coxswain-store-arc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (home, src) = (d.join("home"), d.join("src"));
        std::fs::create_dir_all(&home).unwrap();
        for (f, text) in [("docs/plan.txt", "launch window in march"), ("docs/notes.md", "# Fuel\n\nTanks refilled twice."), ("budget.txt", "the orbit budget"), ("big.txt", "colossal words that go on and on and on")] {
            std::fs::create_dir_all(src.join(f).parent().unwrap()).unwrap();
            std::fs::write(src.join(f), text).unwrap();
        }
        let (zip, tgz) = (home.join("website.zip"), home.join("site.tar.gz"));
        crate::archive::create(&zip, &[src.join("docs")]).unwrap();
        crate::archive::create(&tgz, &[src.join("big.txt")]).unwrap();
        // A zip whose file is locked: its name is seen, its text is never read.
        let locked = home.join("locked.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&locked).unwrap());
        w.start_file("secret.txt", zip::write::SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "hunter2")).unwrap();
        std::io::Write::write_all(&mut w, b"the secret rendezvous").unwrap();
        w.finish().unwrap();

        let mut cfg = SearchConfig { text_roots: vec![home.clone()], text_max_size: 30, ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        let paths = |q: &str| store.search(q, None, 10).hits.into_iter().map(|h| h.path).collect::<Vec<_>>();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(paths("colossal"), Vec::<PathBuf>::new(), "larger than text_max_size");
        assert_eq!(store.db.lock().unwrap().query_row("SELECT count(*) FROM files WHERE inside", [], |r| r.get::<_, i64>(0)).unwrap(), 4, "but known");
        assert_eq!(paths("launch"), [zip.join("docs").join("plan.txt")]);
        let hit = &store.search("refilled", None, 10).hits[0];
        assert_eq!((hit.path.clone(), hit.snippet.as_deref()), (zip.join("docs").join("notes.md"), Some(format!("# Fuel Tanks {}refilled{} twice.", MARK.0, MARK.1).as_str())));
        assert_eq!(paths("rendezvous"), Vec::<PathBuf>::new(), "locked");
        assert_eq!(store.size(&home).map(|s| s.0), Some(crate::fs::dir_size(&home)), "files inside are not counted twice");
        assert_eq!(store.pending.load(Ordering::Relaxed), 0);
        assert!(store.unhashed().unwrap().iter().all(|(p, ..)| crate::archive::split(p).is_none()));

        // Changed by Coxswain, seen by the watcher: a file in, one out. Just written, it may
        // still be growing (a download): it waits for the next round, and nothing is read.
        std::thread::sleep(Duration::from_millis(1100));
        crate::archive::add(&zip, &[("docs/budget.txt".into(), src.join("budget.txt"))], None).unwrap();
        crate::archive::remove(&zip, &["docs/plan.txt".into()], None).unwrap();
        let mut seen: HashSet<PathBuf> = [zip.clone()].into_iter().collect();
        refresh(&store, &cfg, &mut seen, &mut HashSet::new(), &go).unwrap();
        assert!(seen.contains(&zip) && paths("launch") == [zip.join("docs").join("plan.txt")], "still settling");
        std::thread::sleep(SETTLE);
        let id_of = |name: &str| store.db.lock().unwrap().query_row("SELECT id FROM files WHERE path = ?1", [zip.join("docs").join(name).to_str().unwrap()], |r| r.get::<_, i64>(0)).unwrap();
        let notes = id_of("notes.md");
        refresh(&store, &cfg, &mut seen, &mut HashSet::new(), &go).unwrap();
        assert!(seen.is_empty());
        assert_eq!((paths("orbit"), paths("launch")), (vec![zip.join("docs").join("budget.txt")], vec![]));
        assert_eq!(id_of("notes.md"), notes, "a member that did not change keeps its row and text");
        // Renamed inside, then written anew by something else, seen by the next scan.
        std::thread::sleep(Duration::from_millis(1100));
        crate::archive::rename_in(&zip, "docs/budget.txt", "docs/costs.txt", None).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(paths("orbit"), [zip.join("docs").join("costs.txt")]);
        std::fs::remove_file(&zip).unwrap();
        crate::archive::create(&zip, &[src.join("docs/plan.txt")]).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!((paths("orbit"), paths("launch")), (vec![], vec![zip.join("plan.txt")]));
        // An archive that goes takes its files with it.
        std::fs::remove_file(&tgz).unwrap();
        refresh(&store, &cfg, &mut [tgz.clone()].into_iter().collect(), &mut HashSet::new(), &go).unwrap();
        assert_eq!(store.db.lock().unwrap().query_row("SELECT count(*) FROM files WHERE inside", [], |r| r.get::<_, i64>(0)).unwrap(), 2);

        // Switched off: the files inside go; on again, they come back.
        cfg.archives = false;
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(paths("launch"), Vec::<PathBuf>::new());
        cfg.archives = true;
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(paths("launch"), [zip.join("plan.txt")]);
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn store_keeps_a_disk_that_is_not_plugged_in() {
        let d = std::env::temp_dir().join(format!("coxswain-store-disk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("disk/photos")).unwrap();
        std::fs::write(d.join("disk/photos/notes.txt"), b"orbit plan\n").unwrap();
        let cfg = SearchConfig { text_roots: vec![d.join("disk")], ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        let found = || store.search("orbit", None, 10).hits.len();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(found(), 1);

        // Unplugged: the rows stay, out of sight, and Settings shows the disk as away.
        std::fs::rename(d.join("disk"), d.join("away")).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!((found(), store.texts()), (0, 1));
        let roots = store.root_sizes();
        assert_eq!((roots[0].1.clone(), roots[0].2.1), (None, 1));

        // Plugged in again: found again, nothing read twice.
        std::fs::rename(d.join("away"), d.join("disk")).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(found(), 1);
        assert_eq!(store.root_sizes()[0].1.as_deref(), Some(d.join("disk").as_path()));
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn store_reads_again_what_a_new_program_can_read() {
        let d = std::env::temp_dir().join(format!("coxswain-store-tools-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let store = Store::open(&d.join("search.db")).unwrap();
        store.apply(&[], &[("/h/scan.PNG".into(), 1, 1, false), ("/h/a.txt".into(), 1, 1, false)], &[], false, None).unwrap();
        store.read(&store.unread().unwrap().iter().map(|(id, ..)| (*id, None)).collect::<Vec<_>>()).unwrap();
        assert!(store.unread().unwrap().is_empty());
        store.tools_changed(&[]).unwrap();
        assert!(store.unread().unwrap().is_empty(), "nothing new");
        store.tools_changed(&["png", "jpg"]).unwrap();
        assert_eq!(store.unread().unwrap().iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["/h/scan.PNG"], "tesseract came");
        store.tools_changed(&["png", "jpg"]).unwrap();

        // A reader that learnt more: its files are read again over the text they had. Before
        // 1.26.4 the old text made every scan fail there, and no file got its vectors after.
        let id = store.unread().unwrap()[0].0;
        store.read(&[(id, Some("old words".into()))]).unwrap();
        store.readers_changed("test-2", &["png"]).unwrap();
        assert_eq!(store.unread().unwrap().len(), 1);
        store.read(&[(id, Some("new words".into()))]).unwrap();
        assert_eq!(store.search("new", None, 10).total, 1);
        assert_eq!(store.search("old", None, 10).total, 0);
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// `text_exclude` leaves out folders by name and files by pattern, and follows its changes:
    /// a file a new pattern covers loses its text, one no pattern covers any more is read again.
    #[test]
    fn store_leaves_out_what_text_exclude_names() {
        let d = std::env::temp_dir().join(format!("coxswain-store-exclude-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home/drafts")).unwrap();
        for (name, text) in [("app.log", "fuel log"), ("notes.txt", "fuel notes"), ("drafts/a.txt", "fuel draft"), ("build", "fuel script")] {
            std::fs::write(d.join("home").join(name), text).unwrap();
        }
        let mut cfg = SearchConfig { text_roots: vec![d.join("home")], text_exclude: vec!["*.log".into(), "drafts".into(), "build".into()], ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        let found = |store: &Store| {
            let mut v: Vec<String> = store.search("fuel", None, 10).hits.iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
            v.sort();
            v
        };
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(found(&store), ["build", "notes.txt"], "a plain name leaves out folders only, a pattern files");
        assert_eq!(store.size(&d.join("home")).map(|s| s.0 .1), Some(4), "left out files still count in sizes");
        cfg.text_exclude = vec!["notes.*".into()];
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(found(&store), ["a.txt", "app.log", "build"]);
        drop(store);
        let _ = std::fs::remove_dir_all(d);
    }

    /// Coxswain's own cache (the store itself, previews) is never read, wherever it is.
    #[test]
    fn store_never_reads_its_own_cache() {
        let cfg = SearchConfig::default();
        let own = crate::helper::folder().unwrap();
        assert!(left_out(&own, &cfg));
        assert!(!left_out(own.parent().unwrap(), &cfg) || own.parent().unwrap().file_name().unwrap().to_string_lossy().starts_with('.'));
    }

    /// With the model downloaded: files are found by what they are about, in any language,
    /// and a file that changes loses its old vectors.
    #[test]
    fn store_finds_files_by_meaning() {
        if !crate::meaning::installed() {
            return;
        }
        let d = std::env::temp_dir().join(format!("coxswain-store-meaning-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home")).unwrap();
        let write = |name: &str, text: &str| std::fs::write(d.join("home").join(name), text).unwrap();
        write("budget.txt", "The fuel budget for flight seven is the largest cost of the launch, and the tanks are refilled twice before lift-off.");
        write("budget-da.txt", "Brændstofbudgettet for flyvning syv er den største udgift ved opsendelsen, og tankene fyldes to gange før afgang.");
        write("cake.txt", "Opskrift på æblekage: smør, sukker, mel, æbler og kanel. Bag kagen i en time ved 180 grader og server med flødeskum.");
        let cfg = SearchConfig { text_roots: vec![d.join("home")], ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        store.hurry.store(true, Ordering::Relaxed);
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (0, 3));
        let names = |q: &str| store.similar(q, None, 10).iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        let found = names("how much does it cost to fuel the rocket");
        assert!(found.contains(&"budget.txt".to_string()) && found.contains(&"budget-da.txt".to_string()), "{found:?}");
        assert!(!found.contains(&"cake.txt".to_string()), "{found:?}");
        assert_eq!(names("apple cake recipe").first().map(String::as_str), Some("cake.txt"));
        assert!(store.similar("apple cake recipe", Some(&d.join("elsewhere")), 10).is_empty(), "a scope with none of the files");
        assert!(store.passages("apple cake recipe", Some(&d.join("elsewhere")), 10_000).is_empty());
        // What Ask answers from: whole passages, closest first.
        let passages = store.passages("what does the fuel cost", None, 10_000);
        assert!(passages.first().is_some_and(|(p, text)| p.ends_with("budget.txt") || p.ends_with("budget-da.txt") && text.contains("syv")), "{passages:?}");

        // Changed: its vectors go with its old text, and come again for the new one.
        std::thread::sleep(Duration::from_millis(1100));
        write("cake.txt", "Minutes of the board meeting: the budget was approved and the fuel supplier was changed.");
        scan(&store, &cfg, &go).unwrap();
        // Its name still says cake, which the model is shown too: the passage is the new one.
        let hits = store.similar("apple cake recipe", None, 10);
        assert!(hits.iter().filter(|h| h.path.ends_with("cake.txt")).all(|h| h.snippet.as_ref().is_some_and(|s| s.contains("board meeting"))), "{hits:?}");
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// An embedding server that answers like Ollama (`/api/embed`), or like the OpenAI API
    /// (`/v1/embeddings`), with a vector of letter counts per text.
    fn fake_embed_server() -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for mut c in listener.incoming().flatten() {
                let (mut got, mut buf) = (Vec::new(), [0u8; 65536]);
                loop {
                    let n = c.read(&mut buf).unwrap_or(0);
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got);
                    let Some(h) = text.find("\r\n\r\n") else { continue };
                    let len: usize = text[..h].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:")?.trim().parse().ok()).unwrap_or(0);
                    if n == 0 || got.len() >= h + 4 + len {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&got);
                let v: serde_json::Value = serde_json::from_str(text.split_once("\r\n\r\n").map_or("", |x| x.1)).unwrap_or_default();
                let vectors: Vec<Vec<f32>> = v["input"].as_array().map_or(vec![], |a| {
                    a.iter()
                        .map(|t| {
                            let mut v = vec![0.0f32; 26];
                            for b in t.as_str().unwrap_or("").bytes().filter(u8::is_ascii_alphabetic) {
                                v[(b.to_ascii_lowercase() - b'a') as usize] += 1.0;
                            }
                            v
                        })
                        .collect()
                });
                let openai = text.lines().next().is_some_and(|l| l.contains("/embeddings "));
                let reply = if openai { serde_json::json!({ "data": vectors.iter().map(|v| serde_json::json!({ "embedding": v })).collect::<Vec<_>>() }) } else { serde_json::json!({ "embeddings": vectors }) }.to_string();
                let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
            }
        });
        url
    }

    /// A search by meaning reads the text of the files it shows, not of every candidate.
    #[test]
    fn store_meaning_reads_only_the_files_it_shows() {
        let d = std::env::temp_dir().join(format!("coxswain-store-lazy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home")).unwrap();
        for i in 0..40 {
            std::fs::write(d.join("home").join(format!("doc-{i}.txt")), format!("zebra quartz {i} ").repeat(300)).unwrap();
        }
        let cfg = SearchConfig { text_roots: vec![d.join("home")], meaning: true, meaning_engine: "ollama".into(), meaning_url: fake_embed_server(), meaning_model: "fake".into(), ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        store.hurry.store(true, Ordering::Relaxed);
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (0, 40));
        store.bodies_read.store(0, Ordering::Relaxed);
        assert_eq!(store.similar("zebra quartz", None, 5).len(), 5);
        assert_eq!(store.bodies_read.load(Ordering::Relaxed), 5, "five files shown, five read");
        store.bodies_read.store(0, Ordering::Relaxed);
        let found = store.passages("zebra quartz", None, 6000);
        assert!(!found.is_empty() && found.iter().map(|(p, t)| p.as_os_str().len() + 8 + t.len()).sum::<usize>() <= 6000, "{found:?}");
        assert!(store.bodies_read.load(Ordering::Relaxed) <= ASK_FILES, "only the files that can give an excerpt are read");
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Ask's excerpts: a hit with the passages before and after it, in the file's order, the
    /// strongest file first; the same text in a second file once; neighbours left out when
    /// they do not fit.
    #[test]
    fn ask_excerpts_read_as_a_whole() {
        let d = std::env::temp_dir().join(format!("coxswain-store-excerpts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home")).unwrap();
        // Letters the question does not have, but for the hit: the fake model counts letters.
        let text = format!("# One\n{}\n# Two\n{}\n# Three\n{}\n# Four\n{}\n", "mill kiln ".repeat(30), "zebra quartz ".repeat(30), "milk hill ".repeat(30), "lily silk ".repeat(30));
        std::fs::write(d.join("home/a.md"), &text).unwrap();
        std::fs::write(d.join("home/b.md"), &text).unwrap();
        let cfg = SearchConfig { text_roots: vec![d.join("home")], meaning: true, meaning_engine: "ollama".into(), meaning_url: fake_embed_server(), meaning_model: "fake".into(), ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        store.hurry.store(true, Ordering::Relaxed);
        scan(&store, &cfg, &go).unwrap();
        let found = store.passages("zebra quartz", None, 10_000);
        assert_eq!(found.len(), 1, "the copy's text is given once: {found:?}");
        let t = &found[0].1;
        let (before, hit, after) = (t.find("kiln").unwrap(), t.find("zebra").unwrap(), t.find("milk").unwrap());
        assert!(before < hit && hit < after && !t.contains("lily"), "the hit and its neighbours, in order: {t}");
        let tight = store.passages("zebra quartz", None, 600);
        assert!(tight[0].1.contains("zebra") && !tight[0].1.contains("kiln"), "{tight:?}");
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Vectors of passages cut the old way (scheme 1: the first 960 words) are dropped when the
    /// store is opened, the text stays, and the files get new ones, the last changed first;
    /// vectors of this scheme stay, whatever model tag the store names.
    #[test]
    fn store_renews_vectors_of_an_older_passage_scheme() {
        let d = std::env::temp_dir().join(format!("coxswain-store-renew-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home")).unwrap();
        for i in 0..3 {
            std::fs::write(d.join("home").join(format!("doc-{i}.txt")), format!("zebra quartz {i} ").repeat(600)).unwrap();
        }
        let cfg = SearchConfig { text_roots: vec![d.join("home")], meaning: true, meaning_engine: "ollama".into(), meaning_url: fake_embed_server(), meaning_model: "fake".into(), ..SearchConfig::default() };
        let go = AtomicBool::new(false);
        let open = || {
            let store = Store::open(&d.join("search.db")).unwrap();
            store.set_engine(crate::meaning::Engine::from_config(&cfg));
            store.hurry.store(true, Ordering::Relaxed);
            store
        };
        let store = open();
        assert_eq!(store.renewing.load(Ordering::Relaxed), 0, "a new store has nothing to renew");
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (0, 3));
        let chunks = |s: &Store| s.db.lock().unwrap().query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0)).unwrap();
        assert!(chunks(&store) > 3 * 8, "the whole of each file, not its first eight passages");
        // Scheme 1, as a store from before had it (no `passages` in its meta).
        store.db.lock().unwrap().execute_batch("DELETE FROM meta WHERE key = 'passages'; UPDATE files SET modified = 5 WHERE path LIKE '%doc-1.txt'").unwrap();
        drop(store);

        let store = open();
        assert_eq!(store.renewing.load(Ordering::Relaxed), 3);
        drop(store);
        let store = open();
        assert_eq!(store.renewing.load(Ordering::Relaxed), 3, "still renewing after a restart, or when another process opened the store first");
        assert_eq!((store.meaning_counts(), chunks(&store)), ((3, 0), 0), "the vectors went");
        assert_eq!(store.search("zebra", None, 10).total, 3, "the text stayed");
        let first = store.unembedded(3).unwrap().into_iter().map(|(_, path, _)| path).collect::<Vec<_>>();
        assert!(first[2].ends_with("doc-1.txt"), "the last changed first: {first:?}");
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (0, 3));
        assert_eq!(store.renewing.load(Ordering::Relaxed), 0, "renewed");
        let had = chunks(&store);
        drop(store);

        // This scheme: left alone, also when the model's id gains a tag (that is the model's
        // check, not this one).
        let store = open();
        store.db.lock().unwrap().execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('meaning_model', 'ollama:fake:latest')", []).unwrap();
        drop(store);
        let store = open();
        assert_eq!((store.renewing.load(Ordering::Relaxed), store.meaning_counts(), chunks(&store)), (0, (0, 3), had));
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// `bge-m3` picked again from Ollama's list as `bge-m3:latest`, or the same weights under
    /// another name, keep the vectors; another model does not.
    #[test]
    fn vectors_stay_when_only_the_way_the_model_is_written_changes() {
        assert!(crate::meaning::same_model("ollama:bge-m3", "ollama:bge-m3:latest"));
        assert!(!crate::meaning::same_model("ollama:bge-m3", "ollama:nomic-embed-text"));
        assert!(!crate::meaning::same_model("openai:bge-m3", "ollama:bge-m3"));
        let d = std::env::temp_dir().join(format!("coxswain-store-model-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let store = Store::open(&d.join("search.db")).unwrap();
        let vectors = || {
            let db = store.db.lock().unwrap();
            db.execute("INSERT OR IGNORE INTO files(id, path, size, modified, has_text) VALUES (1, '/a.txt', 1, 1, 1)", []).unwrap();
            db.execute("INSERT OR REPLACE INTO chunks(file, n, vector) VALUES (1, 0, x'00')", []).unwrap();
            db.execute("UPDATE files SET embedded = 1", []).unwrap();
        };
        store.model_is("ollama:bge-m3", None).unwrap();
        vectors();
        store.model_is("ollama:bge-m3:latest", Some("sha256:1")).unwrap();
        assert_eq!(store.meaning_counts(), (0, 1), "the same model with its tag");
        store.model_is("ollama:m3-renamed:latest", Some("sha256:1")).unwrap();
        assert_eq!(store.meaning_counts(), (0, 1), "the same weights under another name");
        store.model_is("ollama:nomic-embed-text:latest", Some("sha256:2")).unwrap();
        assert_eq!(store.meaning_counts(), (1, 0), "another model: the vectors go");
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A large store: 20,000 files of 100 passages, 1,024 numbers each (bge-m3's), made once in
    /// `COXSWAIN_BENCH_DIR` (the temp folder by default). What the signs take in memory, and
    /// what a search costs: `cargo test --release -p coxswain-core --lib perf_meaning_large --
    /// --ignored --nocapture`. The numbers are in docs/reference/performance.md.
    #[test]
    #[ignore]
    fn perf_meaning_large_store() {
        const FILES: i64 = 20_000;
        const PER: i64 = 100;
        const DIMS: usize = 1024;
        let dir = std::env::var_os("COXSWAIN_BENCH_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("meaning-large.db");
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut unit = || {
            let v: Vec<f32> = (0..DIMS)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    (seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5
                })
                .collect();
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.into_iter().map(|x| x / n).collect::<Vec<f32>>()
        };
        let store = Store::open(&path).unwrap();
        if store.meaning_counts().1 < FILES as usize {
            let t = Instant::now();
            let mut db = store.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            let words = "the launch plan and the fuel budget for the flight, written out in plain words ".repeat(8);
            for f in 1..=FILES {
                tx.execute("INSERT INTO files(id, path, size, modified, has_text, embedded) VALUES (?1, ?2, 1, 1, 1, 1)", params![f, format!("/home/u/docs/doc-{f}.txt")]).unwrap();
                tx.execute("INSERT INTO text(rowid, body) VALUES (?1, ?2)", params![f, (0..PER).map(|_| words.as_str()).collect::<Vec<_>>().join("\n\n")]).unwrap();
                for n in 0..PER {
                    tx.execute("INSERT INTO chunks(file, n, vector) VALUES (?1, ?2, ?3)", params![f, n, crate::meaning::pack(&unit())]).unwrap();
                }
            }
            tx.commit().unwrap();
            println!("made in {:.0} s", t.elapsed().as_secs_f64());
        }
        println!("store: {} MB", store.bytes() >> 20);
        let q = unit();
        let t = Instant::now();
        let (count, bytes) = store.with_signs(|s| (s.files.len(), s.bits.capacity() * 8 + s.files.capacity() * 8 + s.ns.capacity()));
        println!("signs of {count} passages: {} MB, read in {:.0} ms", bytes >> 20, t.elapsed().as_secs_f64() * 1000.0);
        let wanted = crate::meaning::signs(&crate::meaning::pack(&q));
        let db = store.db.lock().unwrap();
        for _ in 0..3 {
            let t = Instant::now();
            let close = store.with_signs(|s| s.closest(&wanted, 1000));
            let sieve = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let mut vec = db.prepare_cached("SELECT vector FROM chunks WHERE file = ?1 AND n = ?2").unwrap();
            let scored: Vec<f32> = close.iter().filter_map(|(f, n)| vec.query_row(params![f, *n as i64], |r| Ok(crate::meaning::score(r.get_ref(0)?.as_blob()?, &q))).ok()).collect();
            println!("sieve {sieve:.1} ms, {} vectors scored in {:.1} ms", scored.len(), t.elapsed().as_secs_f64() * 1000.0);
        }
        let long = "the launch plan and the fuel budget for the flight, written out in plain words ".repeat(2000);
        for _ in 0..3 {
            let t = Instant::now();
            let n = crate::meaning::passages(&long, false).len();
            println!("passages of a 30,000-word text: {n} in {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        }
    }

    /// The vectors come from a server (a fake one, so the test does not depend on what runs on
    /// the machine), through Ollama's API and the OpenAI one; a switch of model redoes them, and
    /// a server that does not answer leaves the files to wait.
    #[test]
    fn store_takes_its_vectors_from_a_server() {
        takes_vectors_from(&fake_embed_server(), "fake", "fake");
    }

    /// The same against the Ollama on this machine, with a small embedding model pulled
    /// (`ollama pull all-minilm`): `cargo test -p coxswain-core -- --ignored real_ollama`.
    /// Ignored, as it depends on what runs on the machine.
    #[test]
    #[ignore = "needs Ollama on localhost:11434 with all-minilm pulled"]
    fn store_takes_its_vectors_from_a_real_ollama() {
        takes_vectors_from(crate::meaning::OLLAMA, "all-minilm", "real");
    }

    fn takes_vectors_from(server: &str, model: &str, name: &str) {
        let d = std::env::temp_dir().join(format!("coxswain-store-server-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home")).unwrap();
        std::fs::write(d.join("home/budget.txt"), "The fuel budget for flight seven is the largest cost of the launch, and the tanks are refilled twice before lift-off.").unwrap();
        std::fs::write(d.join("home/cake.txt"), "An apple cake: butter, sugar, flour, apples and cinnamon. Bake it for an hour and serve it with whipped cream.").unwrap();
        let mut cfg = SearchConfig { text_roots: vec![d.join("home")], meaning: true, meaning_engine: "ollama".into(), meaning_url: server.into(), meaning_model: model.into(), ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        store.hurry.store(true, Ordering::Relaxed);
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (0, 2));
        let first = |q: &str| store.similar(q, None, 10).first().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned());
        assert_eq!(first("how much does fuelling the rocket cost").as_deref(), Some("budget.txt"));
        assert_eq!(first("a recipe for baking").as_deref(), Some("cake.txt"));

        // The same model through the OpenAI API (Ollama speaks it too, as Lemonade does).
        cfg.meaning_engine = "openai".into();
        cfg.meaning_url = format!("{server}/v1");
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (0, 2));
        assert_eq!(first("a recipe for baking").as_deref(), Some("cake.txt"));

        // Nobody answers there: the files wait, and Settings is told why.
        cfg.meaning_url = "http://127.0.0.1:9".into();
        cfg.meaning_model = "other".into();
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (2, 0), "another model: the old vectors went");
        assert!(store.meaning_error.lock().unwrap().is_some());
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }
}

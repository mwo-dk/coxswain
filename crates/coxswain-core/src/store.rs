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
    /// The signs of every passage's vector, (file, passage, signs), read from `chunks` for the
    /// first search by meaning and kept up to date after.
    signs: Mutex<Option<Signs>>,
}

type Known = HashMap<String, (u64, i64)>;
/// (file, passage, the signs of its vector).
type Signs = Vec<(i64, u8, Box<[u64]>)>;

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
             CREATE INDEX IF NOT EXISTS files_path_size ON files(path, size);
             CREATE VIRTUAL TABLE IF NOT EXISTS text USING fts5(body, tokenize = 'unicode61 remove_diacritics 2');
             CREATE TABLE IF NOT EXISTS skipped(path TEXT PRIMARY KEY, bytes INTEGER NOT NULL, files INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS hashes(path TEXT PRIMARY KEY, size INTEGER NOT NULL, modified INTEGER NOT NULL, hash TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS roots(path TEXT PRIMARY KEY, volume TEXT, inside TEXT, at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS chunks(file INTEGER NOT NULL, n INTEGER NOT NULL, vector BLOB NOT NULL, PRIMARY KEY(file, n));",
        )?;
        // Search by meaning came later: a store from before gets the column, and keeps its text.
        // `embedded` is NULL until the file's passages have their vectors in `chunks`, 1 when
        // they have, 0 when the server refused the file (tried again at the next scan).
        if db.prepare("SELECT embedded FROM files LIMIT 0").is_err() {
            db.execute_batch("ALTER TABLE files ADD COLUMN embedded INTEGER")?;
        }
        Ok(Store { db: Mutex::new(db), pending: AtomicUsize::new(0), hurry: AtomicBool::new(false), cleared: AtomicBool::new(false), walked: Mutex::default(), offline: Mutex::default(), paused: AtomicBool::new(false), configured: Mutex::default(), meaning: AtomicBool::new(false), engine: Mutex::default(), meaning_error: Mutex::default(), signs: Mutex::default() })
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
            db.query_row(&format!("SELECT coalesce(sum({bytes}), 0), coalesce(sum({files}), 0) FROM {table} WHERE path > ?1 AND path < ?2"), [&from, &to], |r| {
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
        db.execute_batch("DELETE FROM text; DELETE FROM files; DELETE FROM skipped; DELETE FROM hashes; DELETE FROM chunks; VACUUM;")
    }

    /// Bytes the store takes on disk.
    pub fn bytes(&self) -> u64 {
        let db = self.db.lock().unwrap();
        db.query_row("SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as u64
    }

    /// Rest as long as the work since `start` took, unless in a hurry; on battery, until the
    /// mains is back.
    fn rest(&self, start: Instant, stop: &AtomicBool) {
        if self.hurry.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(start.elapsed().min(Duration::from_secs(2)));
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
                    .query_row("SELECT coalesce(sum(size), 0), count(*) FROM files WHERE path > ?1 AND path < ?2", [&from, &to], |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)))
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

    /// Every file the store knows: path -> (size, modified).
    fn known(&self) -> rusqlite::Result<Known> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare("SELECT path, size, modified FROM files")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, (r.get::<_, i64>(1)? as u64, r.get(2)?))))?;
        rows.collect()
    }

    /// What a walk found, in one transaction: rows at and below each of `gone` go, with their
    /// text and hashes; new or changed files wait to be read again; the folders left out get
    /// their totals. `all` is a walk of every root, which knows every folder left out.
    fn apply(&self, gone: &[String], changed: &[(String, u64, i64)], skipped: &[(String, Size)], all: bool) -> rusqlite::Result<()> {
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
        for (path, size, modified) in changed {
            tx.execute("DELETE FROM text WHERE rowid = (SELECT id FROM files WHERE path = ?1)", [path])?;
            stale.extend(tx.query_row("SELECT id FROM files WHERE path = ?1", [path], |r| r.get::<_, i64>(0)).ok());
            tx.execute("DELETE FROM chunks WHERE file = (SELECT id FROM files WHERE path = ?1)", [path])?;
            tx.execute(
                "INSERT INTO files(path, size, modified, has_text) VALUES (?1, ?2, ?3, NULL)
                 ON CONFLICT(path) DO UPDATE SET size = excluded.size, modified = excluded.modified, has_text = NULL, embedded = NULL",
                params![path, *size as i64, modified],
            )?;
        }
        for (path, (bytes, files)) in skipped {
            tx.execute("INSERT OR REPLACE INTO skipped(path, bytes, files) VALUES (?1, ?2, ?3)", params![path, *bytes as i64, *files as i64])?;
        }
        tx.commit()?;
        if let Some(signs) = self.signs.lock().unwrap().as_mut().filter(|_| !stale.is_empty()) {
            signs.retain(|(file, ..)| !stale.contains(file));
        }
        Ok(())
    }

    /// The size and date the store has for this file.
    fn row(&self, path: &str) -> Option<(u64, i64)> {
        self.db.lock().unwrap().query_row("SELECT size, modified FROM files WHERE path = ?1", [path], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?))).ok()
    }

    /// Whether the store has anything below this folder, or has it as a folder left out.
    fn has(&self, dir: &str) -> bool {
        let (from, to) = below(dir);
        let db = self.db.lock().unwrap();
        db.query_row("SELECT 1 FROM files WHERE path > ?1 AND path < ?2 LIMIT 1", [&from, &to], |_| Ok(())).is_ok()
            || db.query_row("SELECT 1 FROM skipped WHERE path = ?1 OR (path > ?2 AND path < ?3) LIMIT 1", [dir, &from, &to], |_| Ok(())).is_ok()
    }

    /// Files still to be read: (id, path, size).
    fn unread(&self) -> rusqlite::Result<Vec<(i64, String, u64)>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare("SELECT id, path, size FROM files WHERE has_text IS NULL")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64)))?;
        rows.collect()
    }

    /// Files read, in one transaction. `text` is `None` for a file without text.
    fn read(&self, files: &[(i64, Option<String>)]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for (id, text) in files {
            tx.execute("UPDATE files SET has_text = ?2 WHERE id = ?1", params![id, text.is_some()])?;
            if let Some(text) = text {
                tx.execute("INSERT INTO text(rowid, body) VALUES (?1, ?2)", params![id, text])?;
            }
        }
        tx.commit()
    }

    /// Files that share their size with another and have no current hash: (path, size, modified).
    fn unhashed(&self) -> rusqlite::Result<Vec<(PathBuf, u64, u64)>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare(
            "SELECT f.path, f.size, f.modified FROM files f
             WHERE f.size > 0 AND f.size IN (SELECT size FROM files GROUP BY size HAVING count(*) > 1)
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
        let stale: Vec<String> = rows.into_iter().filter(|(path, size, modified)| std::fs::metadata(path).map_or(true, |m| (m.len(), secs(&m) as u64) != (*size, *modified))).map(|r| r.0).collect();
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

    /// The engine, when search by meaning is on.
    fn engine(&self) -> Option<Arc<crate::meaning::Engine>> {
        self.engine.lock().unwrap().clone().filter(|_| self.meaning.load(Ordering::Relaxed))
    }

    /// The vectors in the store were made by another model: they go, and every file gets new
    /// ones, since vectors of two models cannot be compared.
    fn model_is(&self, id: &str) -> rusqlite::Result<()> {
        let db = self.db.lock().unwrap();
        let before: String = db.query_row("SELECT value FROM meta WHERE key = 'meaning_model'", [], |r| r.get(0)).unwrap_or_default();
        if before != id {
            db.execute_batch("DELETE FROM chunks; UPDATE files SET embedded = NULL;")?;
            db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('meaning_model', ?1)", [id])?;
            *self.signs.lock().unwrap() = None;
        }
        Ok(())
    }

    /// Files with text whose passages have no vectors yet, and files that have them.
    pub fn meaning_counts(&self) -> (usize, usize) {
        let db = self.db.lock().unwrap();
        let count = |q: &str| db.query_row(q, [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize;
        (count("SELECT count(*) FROM files WHERE has_text = 1 AND embedded IS NULL"), count("SELECT count(*) FROM files WHERE embedded = 1"))
    }

    /// Up to `n` files still to get their vectors: (id, text).
    fn unembedded(&self, n: usize) -> rusqlite::Result<Vec<(i64, String)>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare("SELECT f.id, t.body FROM files f JOIN text t ON t.rowid = f.id WHERE f.has_text = 1 AND f.embedded IS NULL LIMIT ?1")?;
        let rows = q.query_map([n as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// A file's passages' vectors, packed.
    fn put_vectors(&self, file: i64, vectors: &[Vec<u8>]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM chunks WHERE file = ?1", [file])?;
        for (n, v) in vectors.iter().enumerate() {
            tx.execute("INSERT INTO chunks(file, n, vector) VALUES (?1, ?2, ?3)", params![file, n as i64, v])?;
        }
        tx.execute("UPDATE files SET embedded = 1 WHERE id = ?1", [file])?;
        tx.commit()?;
        if let Some(signs) = self.signs.lock().unwrap().as_mut() {
            signs.retain(|(f, ..)| *f != file);
            signs.extend(vectors.iter().enumerate().map(|(n, v)| (file, n as u8, crate::meaning::signs(v))));
        }
        Ok(())
    }

    /// Files whose passages mean what `query` asks, closest first, each with the start of
    /// the passage that was closest. Nothing while search by meaning is off.
    pub fn similar(&self, query: &str, max: usize) -> Vec<Hit> {
        let mut seen = HashSet::new();
        self.closest(query, 400)
            .into_iter()
            .filter(|(path, ..)| seen.insert(path.clone()))
            .take(max)
            .map(|(path, s, passage)| {
                let words: Vec<&str> = passage.split_whitespace().collect();
                let snippet = if words.len() > 24 { format!("{} …", words[..24].join(" ")) } else { words.join(" ") };
                Hit { path, is_dir: false, snippet: Some(snippet), similar: Some(s) }
            })
            .collect()
    }

    /// The `max` passages closest to what `question` asks, whole, with their files: what Ask
    /// gives the chat model to answer from. A file may give more than one; at most three, so
    /// one long document does not crowd out the rest.
    pub fn passages(&self, question: &str, max: usize) -> Vec<(PathBuf, String)> {
        let mut per_file: HashMap<PathBuf, usize> = HashMap::new();
        self.closest(question, 400)
            .into_iter()
            .filter(|(path, ..)| {
                let n = per_file.entry(path.clone()).or_default();
                *n += 1;
                *n <= 3
            })
            .take(max)
            .map(|(path, _, passage)| (path, passage))
            .collect()
    }

    /// Passages near what `query` means, closest first, with their file and score. The
    /// passages near the best one, and above what unrelated text scores.
    fn closest(&self, query: &str, keep: usize) -> Vec<(PathBuf, f32, String)> {
        use crate::meaning::{pack, score, signs, alike};
        let Some(engine) = self.engine() else { return vec![] };
        let q = match engine.query(query) {
            Ok(q) => q,
            Err(e) => {
                *self.meaning_error.lock().unwrap() = Some(e);
                return vec![];
            }
        };
        let wanted = signs(&pack(&q));
        let db = self.db.lock().unwrap();
        let mut known = self.signs.lock().unwrap();
        if known.is_none() {
            let read = || -> rusqlite::Result<Vec<_>> {
                let mut q = db.prepare("SELECT file, n, vector FROM chunks")?;
                let rows = q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u8, signs(&r.get::<_, Vec<u8>>(2)?))))?;
                rows.collect()
            };
            *known = Some(read().unwrap_or_default());
        }
        // The signs sieve out all but the few hundred closest; their vectors say how close.
        let mut close: Vec<(u32, i64, u8)> = known.as_ref().unwrap().iter().map(|(f, n, s)| (alike(&wanted, s), *f, *n)).collect();
        drop(known);
        let keep = close.len().min(keep);
        if keep < close.len() {
            close.select_nth_unstable_by(keep, |a, b| b.0.cmp(&a.0));
            close.truncate(keep);
        }
        let Ok(mut vec) = db.prepare("SELECT vector FROM chunks WHERE file = ?1 AND n = ?2") else { return vec![] };
        let mut ranked: Vec<(i64, f32, u8)> = close.into_iter().filter_map(|(_, file, n)| vec.query_row(params![file, n as i64], |r| r.get::<_, Vec<u8>>(0)).ok().map(|v| (file, score(&v, &q), n))).collect();
        drop(vec);
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        // e5's scores sit close together: near the best one, and above what unrelated text scores.
        let top = ranked.first().map_or(0.0, |r| r.1);
        let offline: Vec<(String, String)> = self.offline.lock().unwrap().iter().map(|at| below(at)).collect();
        let mut texts: HashMap<i64, Option<(String, Vec<String>)>> = HashMap::new();
        ranked
            .into_iter()
            .take_while(|r| r.1 >= engine.floor().max(top - 0.10))
            .filter_map(|(file, s, n)| {
                let found = texts.entry(file).or_insert_with(|| {
                    let (path, body): (String, String) = db.query_row("SELECT f.path, t.body FROM files f JOIN text t ON t.rowid = f.id WHERE f.id = ?1", [file], |r| Ok((r.get(0)?, r.get(1)?))).ok()?;
                    (!offline.iter().any(|(from, to)| path > *from && path < *to)).then(|| (path, crate::meaning::passages(&body)))
                });
                let (path, passages) = found.as_ref()?;
                Some((PathBuf::from(path), s, passages.get(n as usize)?.clone()))
            })
            .collect()
    }

    /// Files whose text has every word of `query`; the last word may be the start of one.
    /// Best matches first, each with the passage that matched.
    pub fn search(&self, query: &str, max: usize) -> Results {
        let start = Instant::now();
        let words: Vec<String> = query.split_whitespace().map(|w| w.replace('"', "")).filter(|w| !w.is_empty()).collect();
        if words.is_empty() {
            return Results::default();
        }
        let ask = words.iter().enumerate().map(|(i, w)| format!("\"{w}\"{}", if i + 1 == words.len() { "*" } else { "" })).collect::<Vec<_>>().join(" ");
        let db = self.db.lock().unwrap();
        // Disks that are not plugged in keep their rows, out of sight.
        let offline: Vec<(String, String)> = self.offline.lock().unwrap().iter().map(|at| below(at)).collect();
        let hidden: String = (0..offline.len()).map(|i| format!(" AND NOT (f.path > ?{} AND f.path < ?{})", 2 + 2 * i, 3 + 2 * i)).collect();
        let args: Vec<rusqlite::types::Value> = std::iter::once(&ask).chain(offline.iter().flat_map(|(from, to)| [from, to])).map(|s| s.clone().into()).collect();
        let found = || -> rusqlite::Result<(Vec<Hit>, usize)> {
            let total = db.query_row(&format!("SELECT count(*) FROM text JOIN files f ON f.id = text.rowid WHERE text MATCH ?1{hidden}"), rusqlite::params_from_iter(&args), |r| r.get::<_, i64>(0))? as usize;
            let mut q = db.prepare(&format!(
                "SELECT f.path, snippet(text, 0, char(1), char(2), '…', 18) FROM text JOIN files f ON f.id = text.rowid
                 WHERE text MATCH ?1{hidden} ORDER BY rank LIMIT ?{}",
                2 + 2 * offline.len()
            ))?;
            let hits = q.query_map(rusqlite::params_from_iter(args.iter().cloned().chain([(max as i64).into()])), |r| {
                let snippet: String = r.get(1)?;
                Ok(Hit { path: PathBuf::from(r.get::<_, String>(0)?), is_dir: false, snippet: Some(snippet.split_whitespace().collect::<Vec<_>>().join(" ")), similar: None })
            })?;
            Ok((hits.collect::<rusqlite::Result<_>>()?, total))
        };
        let (hits, total) = found().unwrap_or_default();
        Results { hits, total, micros: start.elapsed().as_micros() as u64 }
    }
}

// ---------------------------------------------------------------- filling it

/// The folders whose text is kept: the ones in the config, or the home folder.
fn roots(cfg: &SearchConfig) -> Vec<PathBuf> {
    if cfg.text_roots.is_empty() { std::env::home_dir().into_iter().collect() } else { cfg.text_roots.clone() }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Hidden folders, folders a tool made and can make again, folders marked "names only" and
/// folders holding `.nosearch`.
fn left_out(dir: &Path, cfg: &SearchConfig) -> bool {
    let name = dir.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    name.starts_with('.') || cfg.text_exclude.iter().any(|x| *x == name) || cfg.names_only.iter().any(|p| p == dir) || dir.join(".nosearch").exists()
}

/// What a walk of some folders found.
#[derive(Default)]
struct Walk {
    /// Files that differ from what the store knew.
    changed: Vec<(String, u64, i64)>,
    /// Folders left out, with their totals.
    skipped: Vec<(String, Size)>,
    /// Every file.
    seen: HashSet<String>,
}

/// Walk `dirs` and everything below them. `None` when `stop` was set meanwhile.
fn walk(dirs: Vec<PathBuf>, cfg: &SearchConfig, known: &Known, stop: &AtomicBool) -> Option<Walk> {
    let mut found = Walk::default();
    // Folders left out are only measured, one thread, like the rest of the scan.
    let slow = rayon::ThreadPoolBuilder::new().num_threads(1).build().expect("a thread");
    let mut stack = dirs;
    while let Some(dir) = stack.pop() {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let (Ok(kind), path) = (entry.file_type(), entry.path()) else { continue };
            if kind.is_dir() {
                if !left_out(&path, cfg) {
                    stack.push(path);
                } else if let (Some(text), Some(size)) = (path.to_str(), slow.install(|| crate::sizes::walk(&path, stop))) {
                    found.skipped.push((text.to_string(), size));
                }
                continue;
            }
            let (true, Some(text), Ok(meta)) = (kind.is_file(), path.to_str(), entry.metadata()) else { continue };
            let modified = secs(&meta);
            if known.get(text).is_none_or(|k| *k != (meta.len(), modified)) {
                found.changed.push((text.to_string(), meta.len(), modified));
            }
            found.seen.insert(text.to_string());
        }
    }
    Some(found)
}

/// Bring the store up to date with the disk, once. Reads and hashes at half speed, one file
/// at a time, so the machine stays the user's. Stops early when `stop` is set.
pub fn scan(store: &Store, cfg: &SearchConfig, stop: &AtomicBool) -> rusqlite::Result<()> {
    let began = now();
    let (roots, offline) = place(store, cfg)?;
    store.tools_changed(&crate::extract::installed::extensions())?;
    // Diagrams got a sentence per arrow.
    store.readers_changed("diagrams-2", &["drawio", "dio", "mmd", "mermaid", "dot", "gv", "puml", "plantuml", "pu", "iuml", "wsd", "md", "markdown", "mdx"])?;
    let known = store.known()?;
    let Some(found) = walk(roots.clone(), cfg, &known, stop) else { return Ok(()) };
    // Rows of a disk that is not plugged in stay.
    let kept: Vec<(String, String)> = offline.iter().map(|at| below(at)).collect();
    let gone: Vec<String> = known.into_keys().filter(|path| !found.seen.contains(path) && !kept.iter().any(|(from, to)| path > from && path < to)).collect();
    *store.offline.lock().unwrap() = offline;
    store.apply(&gone, &found.changed, &found.skipped, true)?;
    *store.walked.lock().unwrap() = Some((roots, began));
    if read(store, cfg, stop)? {
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
    store.model_is(&engine.id())?;
    // Files the server refused last time get one more try per scan; one it refuses for good
    // costs one quick answer every ten minutes, and never holds up the rest.
    store.db.lock().unwrap().execute("UPDATE files SET embedded = NULL WHERE embedded = 0", [])?;
    loop {
        let files = store.unembedded(8)?;
        if files.is_empty() {
            return Ok(());
        }
        let start = Instant::now();
        for (id, body) in files {
            // A server that does not answer: the file waits for the next scan, word search goes on.
            let vectors = match engine.passages(&crate::meaning::passages(&body)) {
                Ok(v) => v,
                Err(NoVectors::Down(e)) => {
                    *store.meaning_error.lock().unwrap() = Some(e);
                    return Ok(());
                }
                Err(NoVectors::Refused(e)) => {
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
        store.rest(start, stop);
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

/// Read the text of the files waiting for it. `true` when stopped.
fn read(store: &Store, cfg: &SearchConfig, stop: &AtomicBool) -> rusqlite::Result<bool> {
    let unread = store.unread()?;
    store.pending.store(unread.len(), Ordering::Relaxed);
    for batch in unread.chunks(200) {
        let start = Instant::now();
        let rows: Vec<_> = batch.iter().map(|(id, path, size)| (*id, crate::extract::text_of(Path::new(path), *size, cfg.text_max_size))).collect();
        store.read(&rows)?;
        store.pending.fetch_sub(batch.len(), Ordering::Relaxed);
        if stop.load(Ordering::Relaxed) || store.cleared.load(Ordering::Relaxed) {
            return Ok(true);
        }
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

/// Follow what the file watcher saw at `paths`: files that changed, came or went, and folders
/// that came. A change inside a folder left out only puts that folder in `later`, as measuring
/// one takes a while: `measure` does it. Nothing happens before a first scan has finished.
pub fn refresh(store: &Store, cfg: &SearchConfig, paths: &HashSet<PathBuf>, later: &mut HashSet<PathBuf>, stop: &AtomicBool) -> rusqlite::Result<()> {
    let Some((roots, _)) = store.walked.lock().unwrap().clone() else { return Ok(()) };
    let began = now();
    let (mut gone, mut changed, mut new) = (vec![], vec![], vec![]);
    for path in paths {
        let Some(text) = key(path) else { continue };
        let path = PathBuf::from(&text);
        let Some(root) = roots.iter().find(|r| path.starts_with(r) && path != **r) else { continue };
        // The topmost folder left out on the way down, this one included if it is a folder.
        let mut down: Vec<&Path> = path.ancestors().take_while(|a| a != root).collect();
        down.reverse();
        let is_dir = path.is_dir();
        if let Some(out) = down.iter().find(|a| (**a != path || is_dir) && left_out(a, cfg)) {
            later.insert(out.to_path_buf());
            continue;
        }
        // A `.nosearch` that came or went changes what its folder is.
        if path.file_name().is_some_and(|n| n == ".nosearch") {
            later.extend(path.parent().map(Path::to_path_buf));
        }
        match std::fs::symlink_metadata(&path) {
            Err(_) => gone.push(text),
            Ok(meta) if meta.is_file() => {
                let now = (meta.len(), secs(&meta));
                if store.row(&text) != Some(now) {
                    changed.push((text, now.0, now.1));
                }
            }
            // A folder that came, or was moved in: its files have not been seen.
            Ok(meta) if meta.is_dir() && !store.has(&text) => new.push(path),
            Ok(_) => {}
        }
    }
    let Some(found) = walk(new, cfg, &Known::default(), stop) else { return Ok(()) };
    changed.extend(found.changed);
    store.apply(&gone, &changed, &found.skipped, false)?;
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
    let Some(found) = walk(again, cfg, &Known::default(), stop) else { return Ok(()) };
    skipped.extend(found.skipped);
    store.apply(&gone, &found.changed, &skipped, false)?;
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
        }
    };
    while !stop.load(Ordering::Relaxed) {
        store.cleared.store(false, Ordering::SeqCst);
        report(scan(store, cfg, stop));
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
                report(refresh(store, cfg, &std::mem::take(&mut now), &mut later, stop));
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
        for sub in ["docs", "node_modules/pkg", ".hidden", "private", "mail"] {
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
        write("mail/inbox.txt", b"fuel\n");

        let cfg = SearchConfig { text_roots: vec![d.join("home")], names_only: vec![d.join("home/mail")], ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        scan(&store, &cfg, &go).unwrap();

        let names = |q: &str| store.search(q, 10).hits.iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names("fuel"), ["budget.md"], "not the binary, node_modules, the hidden folder, the .nosearch one or the names-only one");
        assert_eq!(names("rocket bud"), ["budget.md"], "every word, the last one begun");
        assert_eq!(names("ferry fuel"), [""; 0]);
        assert_eq!(names("\"; DROP TABLE files"), [""; 0], "a query is words, never SQL");
        let hit = &store.search("largest", 10).hits[0];
        assert_eq!(hit.snippet.as_deref(), Some(format!("# Rocket budget Fuel is the {}largest{} cost of flight seven.", MARK.0, MARK.1).as_str()));
        assert_eq!(store.texts(), 2);

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
        let names = |q: &str| store.search(q, 10).hits.iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        let size = |p: &Path| store.size(p).map(|s| s.0);
        let mut later = HashSet::new();
        let saw = |paths: &[PathBuf], later: &mut HashSet<PathBuf>| refresh(&store, &cfg, &paths.iter().cloned().collect(), later, &go).unwrap();

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
    fn store_keeps_a_disk_that_is_not_plugged_in() {
        let d = std::env::temp_dir().join(format!("coxswain-store-disk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("disk/photos")).unwrap();
        std::fs::write(d.join("disk/photos/notes.txt"), b"orbit plan\n").unwrap();
        let cfg = SearchConfig { text_roots: vec![d.join("disk")], ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        let found = || store.search("orbit", 10).hits.len();
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
        store.apply(&[], &[("/h/scan.PNG".into(), 1, 1), ("/h/a.txt".into(), 1, 1)], &[], false).unwrap();
        store.read(&store.unread().unwrap().iter().map(|(id, ..)| (*id, None)).collect::<Vec<_>>()).unwrap();
        assert!(store.unread().unwrap().is_empty());
        store.tools_changed(&[]).unwrap();
        assert!(store.unread().unwrap().is_empty(), "nothing new");
        store.tools_changed(&["png", "jpg"]).unwrap();
        assert_eq!(store.unread().unwrap().iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["/h/scan.PNG"], "tesseract came");
        store.tools_changed(&["png", "jpg"]).unwrap();
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
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
        let names = |q: &str| store.similar(q, 10).iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        let found = names("how much does it cost to fuel the rocket");
        assert!(found.contains(&"budget.txt".to_string()) && found.contains(&"budget-da.txt".to_string()), "{found:?}");
        assert!(!found.contains(&"cake.txt".to_string()), "{found:?}");
        assert_eq!(names("apple cake recipe").first().map(String::as_str), Some("cake.txt"));
        // What Ask answers from: whole passages, closest first.
        let passages = store.passages("what does the fuel cost", 12);
        assert!(passages.first().is_some_and(|(p, text)| p.ends_with("budget.txt") || p.ends_with("budget-da.txt") && text.contains("syv")), "{passages:?}");

        // Changed: its vectors go with its old text, and come again for the new one.
        std::thread::sleep(Duration::from_millis(1100));
        write("cake.txt", "Minutes of the board meeting: the budget was approved and the fuel supplier was changed.");
        scan(&store, &cfg, &go).unwrap();
        assert!(names("apple cake recipe").first().is_none_or(|n| n != "cake.txt"));
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// With Ollama running and a small embedding model pulled (`ollama pull all-minilm`): the
    /// vectors come from the server, a switch of model redoes them, and a server that does not
    /// answer leaves the files to wait.
    #[test]
    fn store_takes_its_vectors_from_a_server() {
        let models = crate::meaning::server_models(false, "", None).unwrap_or_default();
        if !models.iter().any(|m| m.starts_with("all-minilm")) {
            return;
        }
        let d = std::env::temp_dir().join(format!("coxswain-store-server-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home")).unwrap();
        std::fs::write(d.join("home/budget.txt"), "The fuel budget for flight seven is the largest cost of the launch, and the tanks are refilled twice before lift-off.").unwrap();
        std::fs::write(d.join("home/cake.txt"), "An apple cake: butter, sugar, flour, apples and cinnamon. Bake it for an hour and serve it with whipped cream.").unwrap();
        let mut cfg = SearchConfig { text_roots: vec![d.join("home")], meaning: true, meaning_engine: "ollama".into(), meaning_model: "all-minilm".into(), ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        store.hurry.store(true, Ordering::Relaxed);
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (0, 2));
        let first = |q: &str| store.similar(q, 10).first().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned());
        assert_eq!(first("how much does fuelling the rocket cost").as_deref(), Some("budget.txt"));
        assert_eq!(first("a recipe for baking").as_deref(), Some("cake.txt"));

        // The same model through the OpenAI API (Ollama speaks it too, as Lemonade does).
        cfg.meaning_engine = "openai".into();
        cfg.meaning_url = format!("{}/v1", crate::meaning::OLLAMA);
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

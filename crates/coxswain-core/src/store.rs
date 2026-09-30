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
use std::sync::Mutex;
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
const VERSION: i32 = 5;
/// A snippet marks the words it found with these; the apps turn them into a highlight.
pub const MARK: (char, char) = ('\u{1}', '\u{2}');

pub struct Store {
    db: Mutex<Connection>,
    /// Files still to be read in the scan under way.
    pub pending: AtomicUsize,
    /// The roots of the last walk, and when it began (seconds since the Unix epoch): sizes are
    /// known once one has finished.
    walked: Mutex<Option<(Vec<PathBuf>, u64)>>,
}

type Known = HashMap<String, (u64, i64)>;

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
            db.execute_batch("DROP TABLE IF EXISTS files; DROP TABLE IF EXISTS text; DROP TABLE IF EXISTS skipped; DROP TABLE IF EXISTS hashes;")?;
            db.pragma_update(None, "user_version", VERSION)?;
        }
        // The duplicate finder writes hashes from the app while the helper scans.
        db.busy_timeout(Duration::from_secs(10))?;
        // `has_text` is NULL until the file has been read. `skipped` holds the folders left
        // out of the walk with their bytes and files; `hashes`, files' BLAKE3 as they were.
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS files(id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL, modified INTEGER NOT NULL, has_text INTEGER);
             CREATE INDEX IF NOT EXISTS files_size ON files(size);
             CREATE VIRTUAL TABLE IF NOT EXISTS text USING fts5(body, tokenize = 'unicode61 remove_diacritics 2');
             CREATE TABLE IF NOT EXISTS skipped(path TEXT PRIMARY KEY, bytes INTEGER NOT NULL, files INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS hashes(path TEXT PRIMARY KEY, size INTEGER NOT NULL, modified INTEGER NOT NULL, hash TEXT NOT NULL);",
        )?;
        Ok(Store { db: Mutex::new(db), pending: AtomicUsize::new(0), walked: Mutex::default() })
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

    /// What one walk found, in one transaction: files gone lose their row, text and hash;
    /// new or changed files wait to be read again; the folders left out get their totals.
    fn walked(&self, gone: &[&String], changed: &[(String, u64, i64)], skipped: &[(String, Size)]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for path in gone {
            tx.execute("DELETE FROM text WHERE rowid = (SELECT id FROM files WHERE path = ?1)", [path])?;
            tx.execute("DELETE FROM files WHERE path = ?1", [path])?;
            tx.execute("DELETE FROM hashes WHERE path = ?1", [path])?;
        }
        for (path, size, modified) in changed {
            tx.execute("DELETE FROM text WHERE rowid = (SELECT id FROM files WHERE path = ?1)", [path])?;
            tx.execute(
                "INSERT INTO files(path, size, modified, has_text) VALUES (?1, ?2, ?3, NULL)
                 ON CONFLICT(path) DO UPDATE SET size = excluded.size, modified = excluded.modified, has_text = NULL",
                params![path, *size as i64, modified],
            )?;
        }
        tx.execute("DELETE FROM skipped", [])?;
        for (path, (bytes, files)) in skipped {
            tx.execute("INSERT INTO skipped(path, bytes, files) VALUES (?1, ?2, ?3)", params![path, *bytes as i64, *files as i64])?;
        }
        tx.commit()
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
        let found = || -> rusqlite::Result<(Vec<Hit>, usize)> {
            let total = db.query_row("SELECT count(*) FROM text WHERE text MATCH ?1", [&ask], |r| r.get::<_, i64>(0))? as usize;
            let mut q = db.prepare(
                "SELECT f.path, snippet(text, 0, char(1), char(2), '…', 18) FROM text JOIN files f ON f.id = text.rowid
                 WHERE text MATCH ?1 ORDER BY rank LIMIT ?2",
            )?;
            let hits = q.query_map(params![ask, max as i64], |r| {
                let snippet: String = r.get(1)?;
                Ok(Hit { path: PathBuf::from(r.get::<_, String>(0)?), is_dir: false, snippet: Some(snippet.split_whitespace().collect::<Vec<_>>().join(" ")) })
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

/// Bring the store up to date with the disk, once. Reads and hashes at half speed, one file
/// at a time, so the machine stays the user's. Stops early when `stop` is set.
pub fn scan(store: &Store, cfg: &SearchConfig, stop: &AtomicBool) -> rusqlite::Result<()> {
    let began = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let known = store.known()?;
    let mut seen: HashSet<String> = HashSet::new();
    let (mut changed, mut skipped) = (vec![], vec![]);
    // Folders left out are only measured, one thread, like the rest of the scan.
    let slow = rayon::ThreadPoolBuilder::new().num_threads(1).build().expect("a thread");
    let roots = roots(cfg);
    let mut stack = roots.clone();
    while let Some(dir) = stack.pop() {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let (Ok(kind), path) = (entry.file_type(), entry.path()) else { continue };
            if kind.is_dir() {
                // Hidden folders, and folders a tool made and can make again.
                if !name.starts_with('.') && !cfg.text_exclude.iter().any(|x| *x == name) && !path.join(".nosearch").exists() {
                    stack.push(path);
                } else if let (Some(text), Some(size)) = (path.to_str(), slow.install(|| crate::sizes::walk(&path, stop))) {
                    skipped.push((text.to_string(), size));
                }
                continue;
            }
            let (true, Some(text), Ok(meta)) = (kind.is_file(), path.to_str(), entry.metadata()) else { continue };
            let modified = secs(&meta);
            if known.get(text).is_none_or(|k| *k != (meta.len(), modified)) {
                changed.push((text.to_string(), meta.len(), modified));
            }
            seen.insert(text.to_string());
        }
    }
    let gone: Vec<&String> = known.keys().filter(|path| !seen.contains(*path)).collect();
    store.walked(&gone, &changed, &skipped)?;
    *store.walked.lock().unwrap() = Some((roots, began));

    let unread = store.unread()?;
    store.pending.store(unread.len(), Ordering::Relaxed);
    for batch in unread.chunks(200) {
        let start = Instant::now();
        let rows: Vec<_> = batch.iter().map(|(id, path, size)| (*id, crate::extract::text_of(Path::new(path), *size, cfg.text_max_size))).collect();
        store.read(&rows)?;
        store.pending.fetch_sub(batch.len(), Ordering::Relaxed);
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        // As long a rest as the work took.
        std::thread::sleep(start.elapsed().min(Duration::from_secs(2)));
    }

    store.prune_hashes()?;
    for batch in store.unhashed()?.chunks(50) {
        let start = Instant::now();
        let rows: Vec<_> = batch.iter().filter_map(|(path, size, modified)| Some((path.clone(), *size, *modified, crate::dupes::hash_file(path, None, stop, None).ok()?.to_hex().to_string()))).collect();
        store.put_hashes(&rows)?;
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        std::thread::sleep(start.elapsed().min(Duration::from_secs(2)));
    }
    Ok(())
}

/// Keep the store current until `stop`: a scan, then one every ten minutes.
// ponytail: a change shows up in searches within ten minutes; following the file watcher's
// events, as the name index does, makes it seconds.
pub fn keep_current(store: &Store, cfg: &SearchConfig, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        if let Err(e) = scan(store, cfg, stop) {
            eprintln!("coxswain: search store: {e}");
        }
        let rest = Instant::now();
        while rest.elapsed() < Duration::from_secs(600) && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
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
        for sub in ["docs", "node_modules/pkg", ".hidden", "private"] {
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

        let cfg = SearchConfig { text_roots: vec![d.join("home")], ..SearchConfig::default() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        scan(&store, &cfg, &go).unwrap();

        let names = |q: &str| store.search(q, 10).hits.iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names("fuel"), ["budget.md"], "not the binary, node_modules, the hidden folder or the .nosearch one");
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

        // The store of another version is filled afresh.
        drop(store);
        Connection::open(d.join("search.db")).unwrap().pragma_update(None, "user_version", VERSION + 1).unwrap();
        assert_eq!(Store::open(&d.join("search.db")).unwrap().texts(), 0);
        std::fs::remove_dir_all(d).unwrap();
    }
}

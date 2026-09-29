//! The search store: what is known about the files in the user's folders, and the text of
//! those that have text, in one SQLite file in the cache folder. Only the helper writes it.
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

/// Bumped when the tables change.
const VERSION: i32 = 2;
/// A snippet marks the words it found with these; the apps turn them into a highlight.
pub const MARK: (char, char) = ('\u{1}', '\u{2}');

pub struct Store {
    db: Mutex<Connection>,
    /// Files still to be read in the scan under way.
    pub pending: AtomicUsize,
}

type Known = HashMap<String, (i64, u64, i64)>;

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
            db.execute_batch("DROP TABLE IF EXISTS files; DROP TABLE IF EXISTS text;")?;
            db.pragma_update(None, "user_version", VERSION)?;
        }
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS files(id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL, modified INTEGER NOT NULL, has_text INTEGER NOT NULL);
             CREATE VIRTUAL TABLE IF NOT EXISTS text USING fts5(body, tokenize = 'unicode61 remove_diacritics 2');",
        )?;
        Ok(Store { db: Mutex::new(db), pending: AtomicUsize::new(0) })
    }

    /// How many files have their text in the store.
    pub fn texts(&self) -> usize {
        self.db.lock().unwrap().query_row("SELECT count(*) FROM files WHERE has_text", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    }

    /// Every file the store knows: path -> (id, size, modified).
    fn known(&self) -> rusqlite::Result<Known> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare("SELECT path, id, size, modified FROM files")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get::<_, i64>(2)? as u64, r.get(3)?))))?;
        rows.collect()
    }

    /// New or changed files, in one transaction. `text` is `None` for a file without text.
    fn put(&self, files: &[(String, u64, i64, Option<String>)]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for (path, size, modified, text) in files {
            if let Ok(id) = tx.query_row("SELECT id FROM files WHERE path = ?1", [path], |r| r.get::<_, i64>(0)) {
                tx.execute("DELETE FROM text WHERE rowid = ?1", [id])?;
                tx.execute("DELETE FROM files WHERE id = ?1", [id])?;
            }
            tx.execute("INSERT INTO files(path, size, modified, has_text) VALUES (?1, ?2, ?3, ?4)", params![path, *size as i64, modified, text.is_some()])?;
            if let Some(text) = text {
                tx.execute("INSERT INTO text(rowid, body) VALUES (?1, ?2)", params![tx.last_insert_rowid(), text])?;
            }
        }
        tx.commit()
    }

    /// Files that are gone lose their row and their text.
    fn forget(&self, ids: &[i64]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for id in ids {
            tx.execute("DELETE FROM text WHERE rowid = ?1", [id])?;
            tx.execute("DELETE FROM files WHERE id = ?1", [id])?;
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

/// Bring the store up to date with the disk, once. Reads at half speed, one file at a time,
/// so the machine stays the user's. Stops early when `stop` is set.
pub fn scan(store: &Store, cfg: &SearchConfig, stop: &AtomicBool) -> rusqlite::Result<()> {
    let known = store.known()?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut changed: Vec<(PathBuf, String, u64, i64)> = vec![];
    let mut stack = roots(cfg);
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
                }
                continue;
            }
            let (true, Some(text), Ok(meta)) = (kind.is_file(), path.to_str(), entry.metadata()) else { continue };
            let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
            if known.get(text).is_none_or(|(_, size, at)| (*size, *at) != (meta.len(), modified)) {
                changed.push((path.clone(), text.to_string(), meta.len(), modified));
            }
            seen.insert(text.to_string());
        }
    }
    let gone: Vec<i64> = known.iter().filter(|(path, _)| !seen.contains(*path)).map(|(_, (id, ..))| *id).collect();
    store.forget(&gone)?;

    store.pending.store(changed.len(), Ordering::Relaxed);
    for batch in changed.chunks(200) {
        let start = Instant::now();
        let rows: Vec<_> = batch.iter().map(|(path, text, size, at)| (text.clone(), *size, *at, crate::extract::text_of(path, *size, cfg.text_max_size))).collect();
        store.put(&rows)?;
        store.pending.fetch_sub(batch.len(), Ordering::Relaxed);
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        // As long a rest as the work took.
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

        // A file changes, one goes, one comes.
        std::thread::sleep(Duration::from_millis(1100));
        write("docs/notes.txt", "Book the train instead.\n".as_bytes());
        write("docs/new.rs", b"fn launch() {}\n");
        std::fs::remove_file(d.join("home/docs/budget.md")).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!((names("ferry"), names("train"), names("launch"), names("fuel")), (vec![], vec!["notes.txt".to_string()], vec!["new.rs".to_string()], vec![]));
        assert_eq!(store.pending.load(Ordering::Relaxed), 0);

        // The store of another version is filled afresh.
        drop(store);
        Connection::open(d.join("search.db")).unwrap().pragma_update(None, "user_version", VERSION + 1).unwrap();
        assert_eq!(Store::open(&d.join("search.db")).unwrap().texts(), 0);
        std::fs::remove_dir_all(d).unwrap();
    }
}

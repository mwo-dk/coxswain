//! The search store: what is known about the files in the user's folders, and the text of
//! those that have text, in one SQLite file in the cache folder. Only the helper writes it.
//!
//! It also knows every file's size, and the total of each folder it leaves out, so a folder's
//! size is a sum; and the content hash of files that share a size, for the duplicate finder.
//!
//! It is a cache: a store from an older version is thrown away and filled again; one from a
//! newer version is left alone.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use rusqlite::{Connection, params};

use crate::config::SearchConfig;
use crate::index::{Hit, Results};
use crate::sizes::Size;

/// Seconds since the Unix epoch the file was modified.
fn secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
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
    (
        format!("{dir}{sep}"),
        format!("{dir}{}", char::from(sep as u8 + 1)),
    )
}

/// Bumped when the tables change, and when the readers learn formats: files the store has
/// marked as without text are only read again when they change.
const VERSION: i32 = 6;
/// A snippet marks the words it found with these; the apps turn them into a highlight.
pub const MARK: (char, char) = ('\u{1}', '\u{2}');
/// Seconds a file stays unchanged before it gets its vectors: one an app keeps saving
/// (`~/.claude.json`, a browser's session) is found by its words at once, and counted as
/// changing, not as to go, until it has been quiet this long. A date in the future is
/// changing until it is this far past.
pub const QUIET: u64 = 300;

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
    /// Changes to the signs that came while they were held (being read, or a search going
    /// through them): a write never waits for them while it holds the store.
    later: Mutex<Vec<SignsChange>>,
    /// Where the store is: the signs are read on a connection of their own.
    path: PathBuf,
    /// Files whose vectors were dropped for a new way of making passages, while they get new
    /// ones; and how long a passage takes, in milliseconds, as measured, for the time left.
    pub renewing: AtomicUsize,
    pub ms_per_passage: AtomicUsize,
    /// `QUIET`, which the tests and evaluations set to 0 for files they have just written.
    pub quiet: AtomicU64,
    /// The clouds the walks came upon (files only in the cloud): (name, folder).
    clouds: Mutex<Vec<(String, PathBuf)>>,
    /// How many files' text a search by meaning read, for the tests: per store, so tests that
    /// run at the same time do not count each other's.
    #[cfg(test)]
    bodies_read: AtomicUsize,
}

/// A file as the store knows it: size, date, and whether it was left in the cloud; and whether
/// the walk met it.
type Known = HashMap<String, ((u64, i64, bool), bool)>;
/// A file that changed: path, size, date, left in the cloud.
type Changed = (String, u64, i64, bool);
/// A file inside an archive: its path as a key, size, modified.
type Member = (String, u64, u64);
/// What a write did to the signs: all to be read again, these files' passages gone, or a file's
/// passages (anew: it had some before) with their packed vectors.
enum SignsChange {
    Reset,
    Drop(HashSet<i64>),
    Put(i64, Vec<Vec<u8>>, bool),
}

impl SignsChange {
    /// On `signs`, if they are read. `queued`: it waited, so the signs may have it already
    /// (read meanwhile), and a file's passages are put in their place, never twice.
    fn on(self, signs: &mut Option<Signs>, queued: bool) {
        match (self, signs.as_mut()) {
            (SignsChange::Reset, _) => *signs = None,
            (SignsChange::Drop(files), Some(s)) if !files.is_empty() => {
                s.retain(|f| !files.contains(&f))
            }
            (SignsChange::Put(file, vectors, had), Some(s)) => {
                // A file that changed lost its passages already: no pass over them all for it.
                if had || queued {
                    s.retain(|f| f != file);
                }
                for (n, v) in vectors.iter().enumerate() {
                    s.push(file, n as u32, v);
                }
            }
            _ => {}
        }
    }
}

/// The signs of passages' vectors, one after the other, `width` words each, with the file and
/// number of each: 9 bytes and the signs (48 bytes for 384 numbers, 128 for 1024) a passage.
#[derive(Default)]
struct Signs {
    files: Vec<i64>,
    /// The passage's number: past 255 for a file kept whole (`long_files`).
    ns: Vec<u32>,
    bits: Vec<u64>,
    width: usize,
}

impl Signs {
    /// A passage's packed vector; one of another length (a broken row) is left out.
    fn push(&mut self, file: i64, n: u32, packed: &[u8]) {
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
                self.bits
                    .copy_within(i * self.width..(i + 1) * self.width, to * self.width);
                to += 1;
            }
        }
        self.files.truncate(to);
        self.ns.truncate(to);
        self.bits.truncate(to * self.width);
    }

    /// The `keep` passages whose signs are most like `wanted`'s: (file, number). Parts of
    /// 65,536 passages each are gone through on all cores, each keeping its best.
    fn closest(&self, wanted: &[u64], keep: usize) -> Vec<(i64, u32)> {
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
                let mut best: BinaryHeap<Reverse<(u32, usize)>> =
                    BinaryHeap::with_capacity(keep + 1);
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
        best.into_iter()
            .map(|(_, i)| (self.files[i], self.ns[i]))
            .collect()
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
        // A new store gives back the room of what it drops bit by bit (`compact`); one from
        // before gets it at its first compaction.
        db.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        let version = db.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))?;
        // A newer Coxswain made this store: an older one never throws it away, it leaves it
        // alone, so going back to the newer version finds everything it had read.
        if version > VERSION {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                Some(format!("made by a newer Coxswain (store version {version}, this one reads {VERSION}): kept as it is; update Coxswain to search inside files")),
            ));
        }
        if version != VERSION {
            db.execute_batch("DROP TABLE IF EXISTS files; DROP TABLE IF EXISTS text; DROP TABLE IF EXISTS skipped; DROP TABLE IF EXISTS hashes; DROP TABLE IF EXISTS roots; DROP TABLE IF EXISTS meta; DROP TABLE IF EXISTS chunks; DROP TABLE IF EXISTS errors; DROP TABLE IF EXISTS positions; DROP TABLE IF EXISTS slow;")?;
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
             CREATE TABLE IF NOT EXISTS errors(at INTEGER NOT NULL, path TEXT, what TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS transcripts(path TEXT PRIMARY KEY, size INTEGER NOT NULL, modified INTEGER NOT NULL, length INTEGER, done INTEGER NOT NULL DEFAULT 0, text TEXT NOT NULL DEFAULT '', finished INTEGER NOT NULL DEFAULT 0, asked INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS positions(hash INTEGER NOT NULL, file INTEGER NOT NULL, game INTEGER NOT NULL, PRIMARY KEY(hash, file, game)) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS positions_file ON positions(file);
             CREATE TABLE IF NOT EXISTS slow(path TEXT PRIMARY KEY, size INTEGER NOT NULL, modified INTEGER NOT NULL);",
        )?;
        // Search by meaning came later: a store from before gets the column, and keeps its text.
        // `embedded` is NULL until the file's passages have their vectors in `chunks`, 1 when
        // they have, 0 when the server refused the file (tried again at the next scan).
        if db.prepare("SELECT embedded FROM files LIMIT 0").is_err() {
            db.execute_batch("ALTER TABLE files ADD COLUMN embedded INTEGER")?;
        }
        // The passages of a file whose text is longer than what is kept of it (2.33) are made
        // from the whole file, and kept with their vectors: they cannot be cut again from the
        // text kept. NULL for every other file, whose passages are cut from its text.
        // Each on its own: a store left with one of them (stopped between the two) gets the other.
        for column in ["passage", "heading"] {
            if db.prepare(&format!("SELECT {column} FROM chunks LIMIT 0")).is_err() {
                db.execute_batch(&format!("ALTER TABLE chunks ADD COLUMN {column} TEXT"))?;
            }
        }
        // *Everything* came later still (2.34, `long_files`): a file kept whole has `whole` set
        // to the date it had when it was, so a file another version read again since (and
        // cut) counts as sampled; its passages' `starts` say which start a section, so going
        // back to *Sample* keeps the passages `meaning::passages` would. An older version
        // leaves both be.
        if db.prepare("SELECT whole FROM files LIMIT 0").is_err() {
            db.execute_batch("ALTER TABLE files ADD COLUMN whole INTEGER")?;
        }
        if db.prepare("SELECT starts FROM chunks LIMIT 0").is_err() {
            db.execute_batch("ALTER TABLE chunks ADD COLUMN starts INTEGER")?;
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
        // A file dated in the future came later: `settled` is the date it had when a scan found
        // it unchanged since the one before, so it counts as quiet while that date stays.
        if db.prepare("SELECT settled FROM files LIMIT 0").is_err() {
            db.execute_batch("ALTER TABLE files ADD COLUMN settled INTEGER")?;
        }
        // The files still to get their vectors, newest first, without a look at the rest.
        db.execute_batch("CREATE INDEX IF NOT EXISTS files_unembedded ON files(modified) WHERE has_text = 1 AND embedded IS NULL")?;
        // The files still to be read, so that a burst of changes does not scan every row.
        db.execute_batch(
            "CREATE INDEX IF NOT EXISTS files_unread ON files(id) WHERE has_text IS NULL",
        )?;
        let renewing = passages_are(&db, crate::meaning::SCHEME)?;
        // What a passage took last time, kept: the cost of *Everything* is said before any
        // vector is made in this run.
        let measured: usize = db
            .query_row("SELECT value FROM meta WHERE key = 'ms_per_passage'", [], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Ok(Store {
            db: Mutex::new(db),
            pending: AtomicUsize::new(0),
            hurry: AtomicBool::new(false),
            cleared: AtomicBool::new(false),
            walked: Mutex::default(),
            offline: Mutex::default(),
            paused: AtomicBool::new(false),
            configured: Mutex::default(),
            meaning: AtomicBool::new(false),
            engine: Mutex::default(),
            meaning_error: Mutex::default(),
            error: Mutex::default(),
            signs: Mutex::default(),
            later: Mutex::default(),
            path: path.to_path_buf(),
            renewing: AtomicUsize::new(renewing),
            ms_per_passage: AtomicUsize::new(measured),
            quiet: AtomicU64::new(if cfg!(test) { 0 } else { QUIET }),
            clouds: Mutex::default(),
            #[cfg(test)]
            bodies_read: AtomicUsize::new(0),
        })
    }

    /// Bytes and files below `dir`, and when the walk they come from began. `None` until a
    /// walk has finished, and for folders outside it.
    pub fn size(&self, dir: &Path) -> Option<(Size, u64)> {
        let dir = &PathBuf::from(key(dir)?);
        let (roots, at) = self.walked.lock().unwrap().clone()?;
        let root = roots.iter().find(|r| dir.starts_with(r))?;
        let db = self.db.lock().unwrap();
        let skipped = |p: &Path| {
            db.query_row(
                "SELECT bytes, files FROM skipped WHERE path = ?1",
                [p.to_str()?],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
            )
            .ok()
        };
        if let Some(size) = skipped(dir) {
            return Some((size, at));
        }
        // Inside a folder that was left out: only its total is known.
        if dir
            .ancestors()
            .take_while(|a| a.starts_with(root))
            .any(|a| skipped(a).is_some())
        {
            return None;
        }
        let (from, to) = below(dir.to_str()?);
        let sum = |table: &str, bytes: &str, files: &str| {
            db.query_row(&format!("SELECT coalesce(sum({bytes}), 0), coalesce(sum({files}), 0) FROM {table} WHERE path > ?1 AND path < ?2{}", if table == "files" { " AND inside IS NULL" } else { "" }), [&from, &to], |r| {
                Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))
            })
        };
        let (a, b) = (
            sum("files", "size", "1").ok()?,
            sum("skipped", "bytes", "files").ok()?,
        );
        Some(((a.0 + b.0, a.1 + b.1), at))
    }

    /// The hash of `path` when it had this size and date.
    pub fn hash(&self, path: &Path, size: u64, modified: u64) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT hash FROM hashes WHERE path = ?1 AND size = ?2 AND modified = ?3",
            params![key(path)?, size as i64, modified as i64],
            |r| r.get(0),
        )
        .ok()
    }

    /// Hashes as (path, size, modified, hash), in one transaction.
    pub fn put_hashes(&self, rows: &[(PathBuf, u64, u64, String)]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for (path, size, modified, hash) in rows {
            let Some(path) = key(path) else { continue };
            tx.execute(
                "INSERT OR REPLACE INTO hashes(path, size, modified, hash) VALUES (?1, ?2, ?3, ?4)",
                params![path, *size as i64, *modified as i64, hash],
            )?;
        }
        tx.commit()
    }

    /// Forget everything; the store fills again from the start.
    /// Transcripts stay: they are made only on request, and each comes back as its file's text
    /// when the file is read again.
    pub fn clear(&self) -> rusqlite::Result<()> {
        *self.walked.lock().unwrap() = None;
        self.cleared.store(true, Ordering::SeqCst);
        let db = self.db.lock().unwrap();
        // The vacuum goes to the write-ahead log first: written back and emptied, the room is free.
        let done = db.execute_batch("DELETE FROM text; DELETE FROM files; DELETE FROM skipped; DELETE FROM hashes; DELETE FROM chunks; DELETE FROM positions; DELETE FROM errors; DELETE FROM slow; VACUUM; PRAGMA wal_checkpoint(TRUNCATE);");
        // Read again whatever came of it: each statement commits on its own, so a vacuum that
        // fails still leaves the rows gone.
        self.commit_signs(|| Ok(()), SignsChange::Reset)?;
        done
    }

    /// What went wrong, with the file it was about, kept for the store's preview: the latest 50,
    /// the same one again not twice in a row.
    pub(crate) fn note_error(&self, path: Option<&str>, what: &str) {
        let db = self.db.lock().unwrap();
        let last: Option<(Option<String>, String)> = db
            .query_row(
                "SELECT path, what FROM errors ORDER BY rowid DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        if last
            .as_ref()
            .is_some_and(|(p, w)| p.as_deref() == path && w == what)
        {
            return;
        }
        let _ = db.execute(
            "INSERT INTO errors(at, path, what) VALUES (?1, ?2, ?3)",
            params![now() as i64, path, what],
        );
        let _ = db.execute(
            "DELETE FROM errors WHERE rowid <= (SELECT max(rowid) - 50 FROM errors)",
            [],
        );
    }

    /// Bytes the store takes on disk.
    pub fn bytes(&self) -> u64 {
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as u64
    }

    /// Rest as long as the work since `start` took, unless in a hurry; on battery, until the
    /// mains is back.
    fn rest(&self, start: Instant, stop: &AtomicBool) {
        self.rest_after(start.elapsed(), stop, None);
    }

    /// The rest after `work`, never on battery unless *Index now*; on battery no longer than
    /// `until`, when given.
    fn rest_after(&self, work: Duration, stop: &AtomicBool, until: Option<Instant>) {
        if self.hurry.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(work.min(Duration::from_secs(2)));
        while !self.hurry.load(Ordering::Relaxed)
            && !stop.load(Ordering::Relaxed)
            && until.is_none_or(|u| Instant::now() < u)
            && crate::machine::on_battery()
        {
            self.paused.store(true, Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(1));
        }
        self.paused.store(false, Ordering::Relaxed);
    }

    /// Files found without text are read again when an installed program can now read their
    /// kind (`exts`, e.g. tesseract for pictures), or no longer can. A kind that a program can
    /// read now and could not before is read again whole, text and vectors, as another reader
    /// now reads it (Ghostscript a PostScript file that was read as plain text); only the files
    /// of that kind, the first time. A kind no program reads any more keeps what was read of it:
    /// the program may only be missing from `PATH` for a while, and when it is back only that
    /// kind's files without text are read again.
    ///
    /// `known` are the kinds this app can read with a program at all. The list is kept under
    /// `tools.2`, which apps before 2.32.2 leave alone (they write a shorter `tools`): a kind
    /// missing only because an older or newer app wrote the list, not because its program was
    /// missing, is not read again, so going back and forth between versions reads nothing again.
    fn tools_changed(&self, exts: &[&str], known: &[&str]) -> rusqlite::Result<()> {
        let now = exts.join(",");
        let db = self.db.lock().unwrap();
        let meta = |key: &str| -> Option<String> {
            db.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).ok()
        };
        // Written by this app or a later one; else the list of an app from before (`tools`).
        let ours = meta("tools.2");
        let before = ours.clone().or_else(|| meta("tools")).unwrap_or_default();
        let knew = ours.and(meta("tools.known")).unwrap_or_default();
        let now_known = known.join(",");
        if before == now && knew == now_known {
            return Ok(());
        }
        let knew: Vec<&str> = knew.split(',').filter(|e| !e.is_empty()).collect();
        // The kinds a program has read here before (`tools.had`, since 2.33; before that the
        // list itself): one that comes back (a container runtime that started late, a program
        // back on `PATH`) is not read again whole, only its files without text.
        // ponytail: a file of such a kind read another way while its program was away (a
        // PostScript file as plain text) keeps that text until it changes.
        let had_list = meta("tools.had").unwrap_or_else(|| before.clone());
        let had: Vec<&str> = had_list.split(',').filter(|e| !e.is_empty()).collect();
        let kinds: Vec<&str> = before
            .split(',')
            .chain(now.split(','))
            .filter(|e| !e.is_empty())
            .collect();
        let (was, is): (Vec<&str>, Vec<&str>) = (before.split(',').collect(), now.split(',').collect());
        for ext in kinds {
            let pattern = format!("%.{ext}");
            // New to the list: its program came, or the app that wrote the list did not know
            // the kind (then a reader's own version in `reader_changed` says what to read).
            let came = (knew.is_empty() || knew.contains(&ext)) && !had.contains(&ext);
            if is.contains(&ext) && !was.contains(&ext) && came {
                db.execute(
                    "UPDATE files SET has_text = NULL, embedded = NULL WHERE lower(path) LIKE ?1",
                    [&pattern],
                )?;
            } else {
                db.execute(
                    "UPDATE files SET has_text = NULL WHERE has_text = 0 AND lower(path) LIKE ?1",
                    [&pattern],
                )?;
            }
        }
        let mut had_now = had.clone();
        had_now.extend(is.iter().filter(|e| !e.is_empty() && !had.contains(e)));
        let had_now = had_now.join(",");
        // `tools` too, for the apps from before.
        for (key, value) in [("tools", &now), ("tools.2", &now), ("tools.known", &now_known), ("tools.had", &had_now)] {
            db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)", [key, value])?;
        }
        Ok(())
    }

    /// A reader learnt to get more of some kinds of files: those files are read again, once,
    /// and the rest of the store stays. `(tag, ends)`: the tag names what changed, the ends
    /// are those of the files' names (`.md`, `/Makefile`).
    fn readers_changed<S: AsRef<str>>(&self, tag: &str, ends: &[S]) -> rusqlite::Result<()> {
        self.reader_changed("readers", tag, ends)
    }

    /// Files of a `kind` whose text was cut at `extract::MAX_TEXT` (no more than `margin` below
    /// it) get their passages and vectors made again, once (under `key`, at `tag`; an older app
    /// leaves a newer tag be): they are now made from the whole file. Their text stays.
    /// Gone through `PART` files at a time, the store's lock let go between them: a search
    /// waits for one part, not for the whole store's text to be measured.
    fn long_passages(&self, key: &str, tag: &str, margin: usize, kind: fn(&Path) -> bool) -> rusqlite::Result<()> {
        const PART: i64 = 2000;
        let before: String = self
            .db
            .lock()
            .unwrap()
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .unwrap_or_default();
        if before == tag || matches!((before.parse::<u32>(), tag.parse::<u32>()), (Ok(a), Ok(b)) if a > b) {
            return Ok(());
        }
        let mut after = 0i64;
        loop {
            let mut db = self.db.lock().unwrap();
            let part: Vec<(i64, String, bool)> = db
                .prepare_cached("SELECT f.id, f.path, length(CAST(t.body AS BLOB)) + ?1 >= ?2 FROM files f JOIN text t ON t.rowid = f.id WHERE f.embedded = 1 AND f.id > ?3 ORDER BY f.id LIMIT ?4")?
                .query_map(rusqlite::params![margin as i64, crate::extract::MAX_TEXT as i64, after, PART], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let Some(last) = part.last().map(|p| p.0) else { break };
            after = last;
            let tx = db.transaction()?;
            for (id, path, long) in part {
                if long && kind(Path::new(&path)) {
                    tx.execute("UPDATE files SET embedded = NULL WHERE id = ?1", [id])?;
                }
            }
            tx.commit()?;
        }
        self.db.lock().unwrap().execute("INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)", rusqlite::params![key, tag])?;
        Ok(())
    }

    /// What search keeps of long files follows `rule` (`long_files`), once after each change
    /// of it (the rule is kept in `meta`). Files kept whole that are no longer go back to
    /// *Sample* (`to_sample`, nothing read again) and the store gives the room back. Files to
    /// be kept whole are marked; the long ones among them are read again (those whose text was
    /// cut at `extract::MAX_TEXT`) or get their vectors again (those with more passages than
    /// `meaning::PASSAGES`). A short file is the same either way, and only marked.
    fn long_files_changed(&self, rule: &crate::long_files::Rule) -> rusqlite::Result<()> {
        let sig = serde_json::to_string(rule).unwrap_or_default();
        let before: Option<String> = self
            .db
            .lock()
            .unwrap()
            .query_row("SELECT value FROM meta WHERE key = 'long_files'", [], |r| r.get(0))
            .ok();
        if before.as_deref() == Some(sig.as_str()) {
            return Ok(());
        }
        // Nothing kept whole, before or now: nothing to do but note the rule.
        let none_whole = || -> rusqlite::Result<bool> {
            Ok(self.db.lock().unwrap().query_row("SELECT count(*) FROM files WHERE whole IS NOT NULL", [], |r| r.get::<_, i64>(0))? == 0)
        };
        let rows = if !rule.any() && none_whole()? { vec![] } else { self.long_rows()? };
        let capped = if rule.any() { self.capped()? } else { HashMap::new() };
        let mut back = vec![];
        {
            let mut db = self.db.lock().unwrap();
            let tx = db.transaction()?;
            for r in &rows {
                if !rule.whole(Path::new(&r.path)) {
                    if r.whole.is_some() {
                        back.push((r.id, r.path.clone()));
                    }
                    continue;
                }
                if r.whole == Some(r.modified) {
                    continue;
                }
                tx.execute("UPDATE files SET whole = modified WHERE id = ?1", [r.id])?;
                if r.has_text && r.may_be_cut() && text_len(&tx, r.id) + LONG_MARGIN >= crate::extract::MAX_TEXT {
                    tx.execute("UPDATE files SET has_text = NULL, embedded = NULL WHERE id = ?1", [r.id])?;
                } else if capped.contains_key(&r.id) {
                    tx.execute("UPDATE files SET embedded = NULL WHERE id = ?1", [r.id])?;
                }
            }
            tx.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('long_files', ?1)", [&sig])?;
            tx.commit()?;
        }
        if !back.is_empty() {
            self.to_sample(&back)?;
            self.compact()?;
        }
        Ok(())
    }

    /// The files on disk (not inside archives, not commits) as `long_files_changed` and
    /// `long_files_cost` go through them.
    fn long_rows(&self) -> rusqlite::Result<Vec<LongRow>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare("SELECT id, path, modified, whole, size, coalesce(has_text, 0) FROM files WHERE inside IS NULL AND path NOT LIKE 'git:%'")?;
        q.query_map([], |r| {
            Ok(LongRow {
                id: r.get(0)?,
                path: r.get(1)?,
                modified: r.get(2)?,
                whole: r.get(3)?,
                size: r.get::<_, i64>(4)? as u64,
                has_text: r.get(5)?,
            })
        })?
        .collect()
    }

    /// The files with as many passages as `meaning::PASSAGES` keeps or more, and how many.
    fn capped(&self) -> rusqlite::Result<HashMap<i64, usize>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare("SELECT file, count(*) FROM chunks GROUP BY file HAVING count(*) >= ?1")?;
        q.query_map([crate::meaning::PASSAGES as i64], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as usize)))?
            .collect()
    }

    /// Files kept whole (`ids`, with their paths) back to *Sample*, without reading anything:
    /// of their passages those `meaning::passages` keeps (`meaning::chosen`, by where sections
    /// start), numbered anew, the rest gone with their vectors; their text cut back to what is
    /// kept of a file (`extract::cut_back`).
    fn to_sample(&self, ids: &[(i64, String)]) -> rusqlite::Result<()> {
        for (id, path) in ids {
            let mut db = self.db.lock().unwrap();
            let tx = db.transaction()?;
            let rows: Vec<(i64, bool)> = tx
                .prepare("SELECT n, coalesce(starts, 0) FROM chunks WHERE file = ?1 ORDER BY n")?
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let heads: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].1).collect();
            let mut change = None;
            if let Some(keep) = crate::meaning::chosen(rows.len(), &heads) {
                // Upwards: the number a kept passage takes is free by the time it is reached.
                let mut k = 0i64;
                for (i, (n, _)) in rows.iter().enumerate() {
                    if !keep[i] {
                        tx.execute("DELETE FROM chunks WHERE file = ?1 AND n = ?2", [*id, *n])?;
                        continue;
                    }
                    if *n != k {
                        tx.execute("UPDATE chunks SET n = ?3 WHERE file = ?1 AND n = ?2", [*id, *n, k])?;
                    }
                    k += 1;
                }
                let vectors: Vec<Vec<u8>> = tx
                    .prepare("SELECT vector FROM chunks WHERE file = ?1 ORDER BY n")?
                    .query_map([id], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?;
                change = Some(SignsChange::Put(*id, vectors, true));
            }
            let body: Option<String> = tx.query_row("SELECT body FROM text WHERE rowid = ?1", [id], |r| r.get(0)).ok();
            if let Some(cut) = body.and_then(|b| crate::extract::cut_back(&b, crate::code::is_code(path))) {
                tx.execute("DELETE FROM text WHERE rowid = ?1", [id])?;
                tx.execute("INSERT INTO text(rowid, body) VALUES (?1, ?2)", params![id, cut])?;
            }
            tx.execute("UPDATE files SET whole = NULL WHERE id = ?1", [id])?;
            match change {
                Some(change) => self.commit_signs(|| tx.commit(), change)?,
                None => tx.commit()?,
            }
        }
        Ok(())
    }

    /// The room of what was dropped given back to the disk: bit by bit once the store vacuums
    /// incrementally; a store from before is vacuumed whole once, to that.
    fn compact(&self) -> rusqlite::Result<()> {
        let db = self.db.lock().unwrap();
        let mode: i64 = db.pragma_query_value(None, "auto_vacuum", |r| r.get(0))?;
        db.execute_batch(if mode == 2 {
            "PRAGMA incremental_vacuum;"
        } else {
            "PRAGMA auto_vacuum = INCREMENTAL; VACUUM;"
        })?;
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
    }

    /// What following `rule` would cost (`long_files::Cost`): of the long files, those to be
    /// kept whole that are not, with the passages they would get besides theirs (about one
    /// per `PASSAGE_BYTES` of text), the bytes that adds (their text twice over, text and
    /// index, and each passage's vector and text) and the bytes read again; and those kept
    /// whole that would not be, with the bytes that frees.
    pub fn long_files_cost(&self, rule: &crate::long_files::Rule) -> rusqlite::Result<crate::long_files::Cost> {
        let rows = self.long_rows()?;
        let capped = self.capped()?;
        let meaning = self.meaning.load(Ordering::Relaxed);
        let db = self.db.lock().unwrap();
        let dims: u64 = db
            .query_row("SELECT length(vector) FROM chunks LIMIT 1", [], |r| r.get::<_, i64>(0))
            .map_or(384, |n| n as u64);
        let per_passage = dims + PASSAGE_ROW;
        let mut c = crate::long_files::Cost::default();
        for r in rows {
            let count = capped.get(&r.id).copied().unwrap_or(0);
            let now = r.whole == Some(r.modified);
            if rule.whole(Path::new(&r.path)) && !now {
                if count == 0 && !r.may_be_cut() {
                    continue;
                }
                let len = text_len(&db, r.id);
                let cut = r.has_text && r.may_be_cut() && len + LONG_MARGIN >= crate::extract::MAX_TEXT;
                if !cut && count == 0 {
                    continue;
                }
                c.files += 1;
                let all = if cut { r.size as usize } else { len } / PASSAGE_BYTES;
                if cut {
                    c.read += r.size;
                    c.bytes += 2 * ((r.size as usize).min(crate::extract::EVERYTHING_TEXT).saturating_sub(len)) as u64;
                }
                if meaning {
                    let more = all.saturating_sub(count);
                    c.passages += more;
                    c.bytes += more as u64 * per_passage;
                }
            } else if !rule.whole(Path::new(&r.path)) && r.whole.is_some() {
                let len = text_len(&db, r.id);
                let fewer = count.saturating_sub(crate::meaning::PASSAGES) as u64;
                let less = if crate::extract::streams(Path::new(&r.path)) { len.saturating_sub(crate::extract::MAX_TEXT) as u64 } else { 0 };
                if fewer + less == 0 {
                    continue;
                }
                c.back += 1;
                c.freed += fewer * per_passage + 2 * less;
            }
        }
        Ok(c)
    }

    /// Files left without text and larger than `max` are read again, once (under `key`, at
    /// `tag`, as `reader_changed`): a reader now reads files of any size of some kinds.
    fn large_unread(&self, key: &str, tag: &str, max: u64) -> rusqlite::Result<()> {
        let db = self.db.lock().unwrap();
        let before: String = db
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .unwrap_or_default();
        if before == tag || matches!((before.parse::<u32>(), tag.parse::<u32>()), (Ok(a), Ok(b)) if a > b) {
            return Ok(());
        }
        db.execute(
            "UPDATE files SET has_text = NULL WHERE has_text = 0 AND size > ?1",
            [max as i64],
        )?;
        db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)", rusqlite::params![key, tag])?;
        Ok(())
    }

    /// `readers_changed` for one reader, under its own `key` in the store, so a change to it
    /// reads only its own files again. Apps that know no such key leave it alone, and their
    /// own `readers` tag stays as they wrote it.
    fn reader_changed<S: AsRef<str>>(&self, key: &str, tag: &str, ends: &[S]) -> rusqlite::Result<()> {
        self.reader_changed_to(key, tag, ends, false)
    }

    /// `reader_changed` for a reader that now leaves out what it read before (hidden text):
    /// the files' old text and the vectors of their passages go at once, so nothing of it is
    /// found, by words or by meaning, nor given to Ask, while the files wait to be read again.
    fn reader_dropped<S: AsRef<str>>(&self, key: &str, tag: &str, ends: &[S]) -> rusqlite::Result<()> {
        self.reader_changed_to(key, tag, ends, true)
    }

    fn reader_changed_to<S: AsRef<str>>(
        &self,
        key: &str,
        tag: &str,
        ends: &[S],
        forget: bool,
    ) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let before: String = db
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .unwrap_or_default();
        // A newer Coxswain read these files with a newer reader: an older one leaves them be.
        let newer = matches!((before.parse::<u32>(), tag.parse::<u32>()), (Ok(a), Ok(b)) if a > b);
        if before == tag || newer {
            return Ok(());
        }
        let tx = db.transaction()?;
        for end in ends {
            let like = format!("%{}", end.as_ref());
            if forget {
                tx.execute("DELETE FROM text WHERE rowid IN (SELECT id FROM files WHERE path LIKE ?1)", [&like])?;
                tx.execute("DELETE FROM chunks WHERE file IN (SELECT id FROM files WHERE path LIKE ?1)", [&like])?;
            }
            tx.execute(
                "UPDATE files SET has_text = NULL, embedded = NULL WHERE path LIKE ?1",
                [like],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            [key, tag],
        )?;
        if forget {
            self.commit_signs(|| tx.commit(), SignsChange::Reset)
        } else {
            tx.commit()
        }
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
    fn place(
        &self,
        path: &str,
        volume: Option<(String, PathBuf)>,
        at: &str,
        moved: Option<&str>,
    ) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        if let Some(old) = moved.filter(|old| *old != at) {
            let (from, to) = below(old);
            for table in ["files", "hashes", "skipped"] {
                tx.execute(&format!("UPDATE {table} SET path = ?4 || substr(path, length(?1) + 1) WHERE path = ?1 OR (path > ?2 AND path < ?3)"), [old, &from, &to, at])?;
            }
        }
        let (volume, inside) = volume.map_or((None, None), |(v, i)| {
            (Some(v), i.to_str().map(str::to_string))
        });
        tx.execute(
            "INSERT OR REPLACE INTO roots(path, volume, inside, at) VALUES (?1, ?2, ?3, ?4)",
            params![path, volume, inside, at],
        )?;
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

    /// Rows written so far (all writes go through this one connection): what was counted
    /// from the store holds while this stays the same.
    pub fn writes(&self) -> u64 {
        self.db.lock().unwrap().total_changes()
    }

    /// How many files have their text in the store.
    pub fn texts(&self) -> usize {
        self.db
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM files WHERE has_text", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap_or(0) as usize
    }

    /// Every file the store knows: path -> (size, modified, left in the cloud).
    fn known(&self) -> rusqlite::Result<Known> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare(
            "SELECT path, size, modified, cloud IS NOT NULL FROM files WHERE inside IS NULL",
        )?;
        let rows = q.query_map([], |r| {
            Ok((
                r.get(0)?,
                ((r.get::<_, i64>(1)? as u64, r.get(2)?, r.get(3)?), false),
            ))
        })?;
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
    fn apply(
        &self,
        gone: &[String],
        changed: &[Changed],
        skipped: &[(String, Size)],
        all: bool,
        archives: Option<&[PathBuf]>,
    ) -> rusqlite::Result<()> {
        // Archives are listed before the store is locked: searches go on meanwhile.
        let inside: Vec<(&str, Vec<Member>)> = changed
            .iter()
            .filter(|(path, ..)| crate::archive::is_archive(Path::new(path)))
            .map(|(path, size, ..)| {
                let entries = if archives.is_some_and(|roots| !in_cache(Path::new(path), roots)) {
                    crate::archive::search_entries(Path::new(path), *size).unwrap_or_default()
                } else {
                    vec![]
                };
                (
                    path.as_str(),
                    entries
                        .into_iter()
                        .filter(|e| !e.is_dir)
                        .filter_map(|e| {
                            Some((key(&Path::new(path).join(&e.name))?, e.size, e.modified))
                        })
                        .collect(),
                )
            })
            .collect();
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        if all {
            tx.execute("DELETE FROM skipped", [])?;
        }
        // The files whose passages lose their vectors, for the signs kept in memory.
        let mut stale: HashSet<i64> = HashSet::new();
        let at = "(path = ?1 OR (path > ?2 AND path < ?3))";
        let ids = format!("SELECT id FROM files WHERE {at}");
        let deletes = [
            format!("DELETE FROM text WHERE rowid IN ({ids})"),
            format!("DELETE FROM chunks WHERE file IN ({ids})"),
            format!("DELETE FROM positions WHERE file IN ({ids})"),
            format!("DELETE FROM files WHERE {at}"),
            format!("DELETE FROM hashes WHERE {at}"),
            format!("DELETE FROM skipped WHERE {at}"),
        ];
        for path in gone {
            let (from, to) = below(path);
            stale.extend(
                tx.prepare_cached(&ids)?
                    .query_map([path, &from, &to], |r| r.get::<_, i64>(0))?
                    .flatten(),
            );
            for sql in &deletes {
                tx.prepare_cached(sql)?.execute([path, &from, &to])?;
            }
        }
        // One row goes, with its text and vectors.
        let mut drop_row = |id: i64, whole: bool| -> rusqlite::Result<()> {
            stale.insert(id);
            tx.prepare_cached("DELETE FROM text WHERE rowid = ?1")?
                .execute([id])?;
            tx.prepare_cached("DELETE FROM chunks WHERE file = ?1")?
                .execute([id])?;
            tx.prepare_cached("DELETE FROM positions WHERE file = ?1")?
                .execute([id])?;
            if whole {
                tx.prepare_cached("DELETE FROM files WHERE id = ?1")?
                    .execute([id])?;
            }
            Ok(())
        };
        for (path, size, modified, cloud) in changed {
            if let Ok(id) = tx
                .prepare_cached("SELECT id FROM files WHERE path = ?1")?
                .query_row([path], |r| r.get::<_, i64>(0))
            {
                drop_row(id, false)?;
            }
            // Gone back to the cloud: its hash goes with its text.
            if *cloud {
                tx.prepare_cached("DELETE FROM hashes WHERE path = ?1")?
                    .execute([path])?;
            }
            tx.prepare_cached(
                "INSERT INTO files(path, size, modified, has_text, cloud) VALUES (?1, ?2, ?3, NULL, ?4)
                 ON CONFLICT(path) DO UPDATE SET size = excluded.size, modified = excluded.modified, has_text = NULL, embedded = NULL, cloud = excluded.cloud, settled = NULL",
            )?
            .execute(params![path, *size as i64, modified, cloud.then_some(1)])?;
        }
        for (archive, entries) in &inside {
            let (from, to) = below(archive);
            // The members as the store has them: those with the same size and time stay as they are.
            let mut had: HashMap<String, (i64, i64, i64)> = tx
                .prepare_cached(
                    "SELECT path, id, size, modified FROM files WHERE path > ?1 AND path < ?2",
                )?
                .query_map([&from, &to], |r| {
                    Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
                })?
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
            tx.execute(
                "INSERT OR REPLACE INTO skipped(path, bytes, files) VALUES (?1, ?2, ?3)",
                params![path, *bytes as i64, *files as i64],
            )?;
        }
        self.commit_signs(|| tx.commit(), SignsChange::Drop(stale))
    }

    /// The size and date the store has for this file, and whether it was left in the cloud.
    fn row(&self, path: &str) -> Option<(u64, i64, bool)> {
        self.db
            .lock()
            .unwrap()
            .query_row(
                "SELECT size, modified, cloud IS NOT NULL FROM files WHERE path = ?1",
                [path],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?)),
            )
            .ok()
    }

    /// Whether the store has anything below this folder, or has it as a folder left out.
    fn has(&self, dir: &str) -> bool {
        let (from, to) = below(dir);
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT 1 FROM files WHERE path > ?1 AND path < ?2 LIMIT 1",
            [&from, &to],
            |_| Ok(()),
        )
        .is_ok()
            || db
                .query_row(
                    "SELECT 1 FROM skipped WHERE path = ?1 OR (path > ?2 AND path < ?3) LIMIT 1",
                    [dir, &from, &to],
                    |_| Ok(()),
                )
                .is_ok()
    }

    /// How many files are still to be read.
    fn unread_count(&self) -> rusqlite::Result<usize> {
        self.db.lock().unwrap().query_row(
            "SELECT count(*) FROM files WHERE has_text IS NULL",
            [],
            |r| Ok(r.get::<_, i64>(0)? as usize),
        )
    }

    /// Up to `n` files on disk still to be read after the id `after`: (id, path, size).
    fn unread_disk(&self, after: i64, n: usize) -> rusqlite::Result<Vec<(i64, String, u64)>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare_cached(
            "SELECT id, path, size FROM files WHERE has_text IS NULL AND inside IS NULL AND id > ?1 ORDER BY id LIMIT ?2",
        )?;
        let rows = q.query_map(rusqlite::params![after, n as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64))
        })?;
        rows.collect()
    }

    /// The files inside archives still to be read: (id, path), all of them, so that each
    /// archive is read in one pass.
    fn unread_inside(&self) -> rusqlite::Result<Vec<(i64, String)>> {
        let db = self.db.lock().unwrap();
        let mut q =
            db.prepare("SELECT id, path FROM files WHERE has_text IS NULL AND inside IS NOT NULL")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// Search inside archives was turned on or off, or which archives changed: the files
    /// inside go, and when it is on, every archive is listed again.
    fn archives_changed(&self, on: bool) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        // "2": archives in caches are left out.
        let now = if on { "2" } else { "0" };
        let before: String = db
            .query_row("SELECT value FROM meta WHERE key = 'archives'", [], |r| {
                r.get(0)
            })
            .unwrap_or_default();
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
            let paths: Vec<String> = tx
                .prepare("SELECT path FROM files")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for path in paths
                .into_iter()
                .filter(|p| crate::archive::is_archive(Path::new(p)))
            {
                // Not what is on disk: the next walk takes it as changed.
                tx.execute("UPDATE files SET modified = -1 WHERE path = ?1", [path])?;
            }
        }
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('archives', ?1)",
            [now],
        )?;
        self.commit_signs(|| tx.commit(), SignsChange::Reset)
    }

    /// `text_exclude` changed: files a new pattern leaves out lose their text and vectors, and
    /// files a pattern no longer leaves out are read again. Folders follow from the walk.
    fn exclude_changed(&self, now: &[String]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let before: String = db
            .query_row("SELECT value FROM meta WHERE key = 'exclude'", [], |r| {
                r.get(0)
            })
            .unwrap_or_default();
        let joined = now.join("\n");
        if before == joined {
            return Ok(());
        }
        let tx = db.transaction()?;
        // SQLite's GLOB knows `*` and `?` as `text_exclude` does; brackets mean the same too.
        let named = "(path GLOB '*/' || ?1 OR path GLOB '*\\' || ?1)";
        for gone in before
            .split('\n')
            .filter(|p| for_files(p) && !now.iter().any(|n| n == p))
        {
            tx.execute(
                &format!("UPDATE files SET has_text = NULL WHERE has_text = 0 AND {named}"),
                [gone],
            )?;
        }
        for new in now
            .iter()
            .filter(|n| for_files(n) && !before.split('\n').any(|p| p == n.as_str()))
        {
            tx.execute(&format!("DELETE FROM text WHERE rowid IN (SELECT id FROM files WHERE has_text = 1 AND {named})"), [new])?;
            tx.execute(
                &format!("DELETE FROM chunks WHERE file IN (SELECT id FROM files WHERE {named})"),
                [new],
            )?;
            tx.execute(
                &format!("DELETE FROM positions WHERE file IN (SELECT id FROM files WHERE {named})"),
                [new],
            )?;
            tx.execute(
                &format!(
                    "UPDATE files SET has_text = 0, embedded = NULL WHERE has_text = 1 AND {named}"
                ),
                [new],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('exclude', ?1)",
            [joined],
        )?;
        self.commit_signs(|| tx.commit(), SignsChange::Reset)
    }

    /// Whether each file just read (`id`, kept whole, with text) is kept whole: marked with
    /// the date it has now (`long_files_changed`), unmarked otherwise.
    fn mark_whole(&self, files: &[(i64, bool, bool)]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for (id, whole, _) in files {
            tx.prepare_cached("UPDATE files SET whole = CASE WHEN ?2 THEN modified END WHERE id = ?1")?
                .execute(params![id, whole])?;
        }
        tx.commit()
    }

    /// Whether the file `id` ran past a program's time limit as it is now: it is not read
    /// again until it changes (`slow`, 2.33), however often its program comes and goes.
    fn too_long(&self, id: i64) -> bool {
        self.db.lock().unwrap().query_row(
            "SELECT 1 FROM files f JOIN slow s ON s.path = f.path AND s.size = f.size AND s.modified = f.modified WHERE f.id = ?1",
            [id],
            |_| Ok(()),
        ).is_ok()
    }

    /// The file `id` ran past a program's time limit: kept as it is now, for `too_long`.
    // ponytail: rows of files gone since stay; a few bytes each, cleared by Forget.
    fn mark_too_long(&self, id: i64) -> rusqlite::Result<()> {
        self.db.lock().unwrap().execute(
            "INSERT OR REPLACE INTO slow(path, size, modified) SELECT path, size, modified FROM files WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    /// Files read, in one transaction. `text` is `None` for a file without text.
    fn read(&self, files: &[(i64, Option<String>)]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for (id, text) in files {
            tx.prepare_cached("UPDATE files SET has_text = ?2 WHERE id = ?1")?
                .execute(params![id, text.is_some()])?;
            // A file read again may still have its text from before (a reader that learnt more
            // keeps it until now): the new one takes its place, or the insert fails the scan.
            tx.prepare_cached("DELETE FROM text WHERE rowid = ?1")?
                .execute([id])?;
            if let Some(text) = text {
                tx.prepare_cached("INSERT INTO text(rowid, body) VALUES (?1, ?2)")?
                    .execute(params![id, text])?;
            }
        }
        if !files.is_empty() {
            tx.execute(
                "INSERT OR REPLACE INTO meta(key, value) VALUES ('read_at', ?1)",
                [now().to_string()],
            )?;
        }
        tx.commit()
    }

    /// The positions a PGN file's games reach (`chess::games::positions`), in place of those it
    /// had: by key, with the game's index, so Find this position finds the games. The file is
    /// read game by game without the store's lock (`stop` asked between games), and the rows go
    /// in, in transactions of their own a few thousand games at a time.
    fn put_positions(&self, id: i64, path: &Path, stop: &dyn Fn() -> bool) -> rusqlite::Result<()> {
        // The old rows a part at a time, the lock let go between parts: a database of millions
        // of games holds up no search.
        while self.db.lock().unwrap().execute(
            "DELETE FROM positions WHERE file = ?1 AND (hash, game) IN (SELECT hash, game FROM positions WHERE file = ?1 LIMIT 50000)",
            [id],
        )? > 0
        {}
        let mut batch: Vec<(u64, u32)> = vec![];
        let flush = |batch: &mut Vec<(u64, u32)>| -> rusqlite::Result<()> {
            let mut db = self.db.lock().unwrap();
            let tx = db.transaction()?;
            {
                let mut put = tx.prepare_cached("INSERT OR IGNORE INTO positions(hash, file, game) VALUES (?1, ?2, ?3)")?;
                for (k, game) in batch.drain(..) {
                    put.execute(params![k as i64, id, game])?;
                }
            }
            tx.commit()
        };
        let mut failed = None;
        let _ = crate::chess::games::positions(path, stop, |game, keys| {
            batch.extend(keys.iter().map(|&k| (k, game)));
            if batch.len() >= 100_000
                && failed.is_none()
                && let Err(e) = flush(&mut batch)
            {
                failed = Some(e);
            }
        });
        if let Some(e) = failed {
            return Err(e);
        }
        // Stopped part way (the app quits, or *Forget* cleared the store): what came in is not
        // all, so the file is read again, its positions with it; after a clear none go in.
        if stop() {
            self.db.lock().unwrap().execute("UPDATE files SET has_text = NULL WHERE id = ?1", [id])?;
            return Ok(());
        }
        flush(&mut batch)
    }

    /// The games that reach the position with the Polyglot key `hash`: (file, game index from
    /// 0), at most `max`, by their files' paths. The first `max` rows of the key's in the order
    /// they are kept (by file, as it came into the store), so a position millions of games
    /// reach (the start, 1. e4) is answered as quickly as a rare one: never all of them sorted.
    pub fn games_at(&self, hash: u64, max: usize) -> rusqlite::Result<Vec<(PathBuf, u32)>> {
        let db = self.db.lock().unwrap();
        let mut q = db.prepare_cached("SELECT f.path, p.game FROM (SELECT file, game FROM positions WHERE hash = ?1 ORDER BY file, game LIMIT ?2) p JOIN files f ON f.id = p.file")?;
        let mut games: Vec<(PathBuf, u32)> = q.query_map(params![hash as i64, max as i64], |r| Ok((PathBuf::from(r.get::<_, String>(0)?), r.get::<_, u32>(1)?)))?.collect::<rusqlite::Result<_>>()?;
        games.sort();
        Ok(games)
    }

    /// Commits of the repository at `root`, (id, time, text), as rows whose text is searched;
    /// then only its newest `max` stay.
    fn put_commits(
        &self,
        root: &Path,
        commits: &[(String, u64, String)],
        max: usize,
    ) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for (hash, time, text) in commits {
            let path = crate::history::key(root, hash);
            if tx.execute(
                "INSERT OR IGNORE INTO files(path, size, modified, has_text) VALUES (?1, 0, ?2, 1)",
                params![path, *time as i64],
            )? == 1
            {
                tx.execute(
                    "INSERT INTO text(rowid, body) VALUES (?1, ?2)",
                    params![tx.last_insert_rowid(), text],
                )?;
            }
        }
        tx.commit()?;
        let (from, to) =
            below(crate::history::key(root, "").trim_end_matches(std::path::MAIN_SEPARATOR));
        let mut q = db.prepare("SELECT path FROM files WHERE path > ?1 AND path < ?2 ORDER BY modified DESC LIMIT -1 OFFSET ?3")?;
        let old: Vec<String> = q
            .query_map(params![from, to, max as i64], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(q);
        drop(db);
        if old.is_empty() {
            Ok(())
        } else {
            self.apply(&old, &[], &[], false, None)
        }
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
        let rows = q.query_map([], |r| {
            Ok((
                PathBuf::from(r.get::<_, String>(0)?),
                r.get::<_, i64>(1)? as u64,
                r.get::<_, i64>(2)? as u64,
            ))
        })?;
        rows.collect()
    }

    /// Hashes of files that are gone or changed, wherever they are, go.
    fn prune_hashes(&self) -> rusqlite::Result<()> {
        let rows: Vec<(String, u64, u64)> = {
            let db = self.db.lock().unwrap();
            let mut q = db.prepare("SELECT path, size, modified FROM hashes")?;
            let rows = q.query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, i64>(1)? as u64,
                    r.get::<_, i64>(2)? as u64,
                ))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        // So do those of files gone back to the cloud, which are not read again.
        let stale: Vec<String> = rows
            .into_iter()
            .filter(|(path, size, modified)| {
                crate::cloud::keep_out(Path::new(path))
                    || Path::new(path).ancestors().any(crate::fs::protected)
                    || std::fs::metadata(path)
                        .map_or(true, |m| (m.len(), secs(&m) as u64) != (*size, *modified))
            })
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
        self.engine
            .lock()
            .unwrap()
            .clone()
            .filter(|_| self.meaning.load(Ordering::Relaxed))
    }

    /// The vectors in the store were made by another model: they go, and every file gets new
    /// ones, since vectors of two models cannot be compared.
    /// The same model written another way (`bge-m3`, `bge-m3:latest`), or the same weights under
    /// another name (by `digest`), keeps them.
    fn model_is(&self, id: &str, digest: Option<&str>) -> rusqlite::Result<()> {
        let db = self.db.lock().unwrap();
        let meta = |key: &str| {
            db.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .unwrap_or_default()
        };
        let (before, weights) = (meta("meaning_model"), meta("meaning_digest"));
        if !crate::meaning::same_model(&before, id) && digest.is_none_or(|d| d != weights) {
            let done = db.execute_batch("DELETE FROM chunks; UPDATE files SET embedded = NULL;");
            // Read again whatever came of it: each statement commits on its own.
            self.commit_signs(|| Ok(()), SignsChange::Reset)?;
            done?;
        }
        db.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('meaning_model', ?1)",
            [id],
        )?;
        if let Some(d) = digest {
            db.execute(
                "INSERT OR REPLACE INTO meta(key, value) VALUES ('meaning_digest', ?1)",
                [d],
            )?;
        }
        Ok(())
    }

    /// The latest `modified` of a file quiet enough for its vectors.
    fn quiet_since(&self) -> i64 {
        quiet_since(self.quiet.load(Ordering::Relaxed))
    }

    /// Files dated in the future that a walk found unchanged: quiet while they keep that date.
    fn settle(&self, files: &[(String, i64)]) -> rusqlite::Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        for (path, at) in files {
            tx.prepare_cached("UPDATE files SET settled = ?2 WHERE path = ?1 AND modified = ?2")?
                .execute(params![path, at])?;
        }
        tx.commit()
    }

    /// Files with text, quiet, whose passages have no vectors yet, and files that have them.
    pub fn meaning_counts(&self) -> (usize, usize) {
        let db = self.db.lock().unwrap();
        let count = |q: &str| db.query_row(q, [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize;
        (
            db.query_row(TO_GO, [self.quiet_since()], |r| r.get::<_, i64>(0))
                .unwrap_or(0) as usize,
            count("SELECT count(*) FROM files WHERE embedded = 1"),
        )
    }

    /// Files with text still to get their vectors that changed within `quiet`.
    pub fn meaning_changing(&self) -> usize {
        self.db
            .lock()
            .unwrap()
            .query_row(CHANGING, [self.quiet_since()], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    /// Passages that have their vectors.
    pub fn passage_count(&self) -> usize {
        self.db
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    /// Up to `n` quiet files still to get their vectors, the last changed first: (id, path, text).
    fn unembedded(&self, n: usize) -> rusqlite::Result<Vec<(i64, String, String)>> {
        Ok(self.unembedded_whole(n)?.into_iter().map(|(id, path, body, _)| (id, path, body)).collect())
    }

    /// `unembedded`, with whether each file is kept whole (*Everything*).
    fn unembedded_whole(&self, n: usize) -> rusqlite::Result<Vec<(i64, String, String, bool)>> {
        let db = self.db.lock().unwrap();
        // The files first, by `files_unembedded`, then their text: joined before the limit, the
        // text of every file still to go was read for each few (2 s a time with 300,000).
        let mut q = db.prepare(
            "SELECT f.id, f.path, coalesce(t.body, ''), f.whole IS NOT NULL AND f.whole = f.modified FROM (SELECT id, path, modified, whole FROM files WHERE has_text = 1 AND embedded IS NULL AND (modified <= ?2 OR settled = modified) ORDER BY modified DESC LIMIT ?1) f
             LEFT JOIN text t ON t.rowid = f.id ORDER BY f.modified DESC",
        )?;
        let rows = q.query_map([n as i64, self.quiet_since()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        rows.collect()
    }

    /// A file's passages' vectors, packed.
    fn put_vectors(&self, file: i64, vectors: &[Vec<u8>]) -> rusqlite::Result<()> {
        self.put_passages(file, vectors, None, None)
    }

    /// `put_vectors`, with the passages themselves when they were made from the whole file
    /// (`whole_passages`), not from the text kept.
    fn put_passages(
        &self,
        file: i64,
        vectors: &[Vec<u8>],
        passages: Option<&[crate::meaning::Passage]>,
        starts: Option<&[bool]>,
    ) -> rusqlite::Result<()> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let had = tx.execute("DELETE FROM chunks WHERE file = ?1", [file])? > 0;
        for (n, v) in vectors.iter().enumerate() {
            let p = passages.and_then(|p| p.get(n));
            tx.execute(
                "INSERT INTO chunks(file, n, vector, passage, heading, starts) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![file, n as i64, v, p.map(|p| p.text.as_str()), p.map(|p| p.heading.as_str()), starts.and_then(|s| s.get(n))],
            )?;
        }
        tx.execute("UPDATE files SET embedded = 1 WHERE id = ?1", [file])?;
        self.commit_signs(
            || tx.commit(),
            SignsChange::Put(file, vectors.to_vec(), had),
        )
    }

    /// `commit`, made while the store is held, and `change` on the signs. When they are free
    /// it commits holding them, so signs read meanwhile have all of it or none. When they are
    /// held (read, or a search going through two million) it does not wait with the store
    /// held: the change waits in `later` for whoever holds them next.
    fn commit_signs(
        &self,
        commit: impl FnOnce() -> rusqlite::Result<()>,
        change: SignsChange,
    ) -> rusqlite::Result<()> {
        match self.signs.try_lock() {
            Ok(mut signs) => {
                commit()?;
                for c in std::mem::take(&mut *self.later.lock().unwrap()) {
                    c.on(&mut signs, true);
                }
                change.on(&mut signs, false);
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                commit()?;
                self.later.lock().unwrap().push(change);
            }
            Err(std::sync::TryLockError::Poisoned(e)) => panic!("{e}"),
        }
        Ok(())
    }

    /// `f` on the signs of every passage, read first if they are not yet: on a connection of
    /// their own, so the store goes on answering meanwhile (two million passages take seconds).
    /// Writes meanwhile leave their changes in `later`, taken in here.
    fn with_signs<T>(&self, f: impl FnOnce(&Signs) -> T) -> T {
        let mut known = self.signs.lock().unwrap();
        // A change that came while they were read may be in them already or not: each is put
        // in its place again; one that wants them all read again has them read again.
        loop {
            for c in std::mem::take(&mut *self.later.lock().unwrap()) {
                c.on(&mut known, true);
            }
            if known.is_some() {
                break;
            }
            let read = || -> rusqlite::Result<Signs> {
                let db = Connection::open_with_flags(
                    &self.path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )?;
                db.busy_timeout(Duration::from_secs(10))?;
                let mut q = db.prepare("SELECT file, n, vector FROM chunks")?;
                let mut rows = q.query([])?;
                let mut out = Signs::default();
                while let Some(r) = rows.next()? {
                    out.push(
                        r.get(0)?,
                        r.get::<_, i64>(1)? as u32,
                        r.get_ref(2)?.as_blob()?,
                    );
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
        self.closest(query, max, true, None, |p| scope.is_none_or(|s| p.starts_with(s)))
            .into_iter()
            .map(|(path, s, n, all)| {
                let words: Vec<&str> = all[n].text.split_whitespace().collect();
                let snippet = if words.len() > 24 {
                    format!("{} …", words[..24].join(" "))
                } else {
                    words.join(" ")
                };
                Hit {
                    path,
                    is_dir: false,
                    snippet: Some(snippet),
                    similar: Some(s),
                }
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
    pub fn passages(
        &self,
        question: &str,
        scope: Option<&Path>,
        bytes: usize,
    ) -> Vec<(PathBuf, String, usize)> {
        let mut per_file: HashMap<PathBuf, usize> = HashMap::new();
        // Passages near the same as one already taken, of other files (copies, versions of one
        // document), fold into it: the copies take no room, and the file says how many it has.
        let mut folded: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();
        let hits = self.closest(question, ASK_HITS, false, Some(&mut folded), |path| {
            if scope.is_some_and(|s| !path.starts_with(s)) || crate::ask::string_table(path) {
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
        let mut files: Vec<(
            PathBuf,
            std::rc::Rc<Vec<crate::meaning::Passage>>,
            Vec<usize>,
        )> = vec![];
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
            let header = path.as_os_str().len() + 32;
            let mut taken: Vec<usize> = vec![];
            for &n in hits.iter().take(4usize.saturating_sub(rank).max(1)) {
                // With its neighbours if they fit, else alone.
                for window in [n.saturating_sub(1)..=(n + 1).min(all.len() - 1), n..=n] {
                    let new: Vec<usize> = window.filter(|i| !taken.contains(i)).collect();
                    let cost = new.iter().map(|&i| all[i].text.len() + 1).sum::<usize>()
                        + if taken.is_empty() { header } else { 0 };
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
            // An excerpt of code says the item it starts in: "› impl Store › fn passages".
            let code = crate::code::is_code(&path.to_string_lossy());
            while i < taken.len() {
                let first = &all[taken[i]];
                let mut text = if code && !first.heading.is_empty() {
                    format!("› {}\n{}", first.heading, first.text)
                } else {
                    first.text.clone()
                };
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
                out.push((path, excerpts.join(crate::ask::GAP), 0));
            }
        }
        // The similar files a source stands for, but those that are sources of their own.
        let own: HashSet<PathBuf> = out.iter().map(|(p, ..)| p.clone()).collect();
        for (path, _, similar) in &mut out {
            *similar = folded.get(path).map_or(0, |f| {
                f.iter()
                    .filter(|p| !own.contains(*p) && scope.is_none_or(|s| p.starts_with(s)))
                    .count()
            });
        }
        out
    }

    /// Up to `want` passages near what `query` means, closest first, with their file and
    /// score: those near the best one, above what unrelated text scores, and of files `accept`
    /// takes; with `by_file`, the best passage of each file, ranked by the file's score. Only
    /// the files that give a passage have their text read.
    fn closest(
        &self,
        query: &str,
        want: usize,
        by_file: bool,
        mut folded: Option<&mut HashMap<PathBuf, HashSet<PathBuf>>>,
        mut accept: impl FnMut(&Path) -> bool,
    ) -> Vec<(
        PathBuf,
        f32,
        usize,
        std::rc::Rc<Vec<crate::meaning::Passage>>,
    )> {
        use crate::meaning::{pack, score, signs};
        let Some(engine) = self.engine() else {
            return vec![];
        };
        // The server answering again takes back what a failed query said.
        let q = match engine.query(query) {
            Ok(q) => {
                *self.meaning_error.lock().unwrap() = None;
                q
            }
            Err(e) => {
                *self.meaning_error.lock().unwrap() = Some(e);
                return vec![];
            }
        };
        // The signs sieve out all but the thousand closest; their vectors say how close.
        let close = self.with_signs(|s| s.closest(&signs(&pack(&q)), 1000));
        let db = self.db.lock().unwrap();
        let Ok(mut vec) = db.prepare_cached("SELECT vector FROM chunks WHERE file = ?1 AND n = ?2")
        else {
            return vec![];
        };
        // With the vectors themselves when near copies are folded.
        let mut ranked: Vec<(i64, f32, u32, Vec<u8>)> = close
            .into_iter()
            .filter_map(|(file, n)| {
                vec.query_row(params![file, n as i64], |r| {
                    let blob = r.get_ref(0)?.as_blob()?;
                    let kept = if folded.is_some() { blob.to_vec() } else { vec![] };
                    Ok((score(blob, &q), kept))
                })
                .ok()
                .map(|(s, v)| (file, s, n, v))
            })
            .collect();
        drop(vec);
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        // Near the best one, and above what unrelated text scores, by the model's scale.
        let top = ranked.first().map_or(0.0, |r| r.1);
        let floor = engine.floor().max(top - engine.window());
        ranked.retain(|r| r.1 >= floor);
        if by_file {
            ranked = by_files(ranked);
        }
        // The vectors of the passages given, with their file and where in `out` they are.
        let mut given: Vec<(i64, usize, Vec<u8>)> = vec![];
        let offline: Vec<(String, String)> = self
            .offline
            .lock()
            .unwrap()
            .iter()
            .map(|at| below(at))
            .collect();
        // A file's path as shown, and as kept (its kind says how it is cut into passages).
        let mut paths: HashMap<i64, Option<(PathBuf, String)>> = HashMap::new();
        let mut texts: HashMap<i64, Vec<crate::meaning::Passage>> = HashMap::new();
        // Files kept whole with more passages than are read at once: only the passages a hit
        // needs (it and its neighbours) are read, the rest left empty.
        let mut sparse: HashSet<i64> = HashSet::new();
        let mut out: Vec<(PathBuf, f32, usize, i64)> = vec![];
        for (file, s, n, v) in ranked {
            if out.len() >= want {
                break;
            }
            let path = paths.entry(file).or_insert_with(|| {
                let path: String = db
                    .query_row("SELECT path FROM files WHERE id = ?1", [file], |r| r.get(0))
                    .ok()?;
                (!offline.iter().any(|(from, to)| path > *from && path < *to))
                    .then(|| (shown(path.clone()), path))
            });
            // Pairwise only with the passages given so far, `want` at most.
            if let (Some(folded), Some((shown, _))) = (folded.as_deref_mut(), path.as_ref())
                && let Some((_, at, _)) = given
                    .iter()
                    .find(|(of, _, g)| *of != file && crate::meaning::packed_cosine(g, &v) > NEAR_COPY)
            {
                let into: &PathBuf = &out[*at].0;
                folded.entry(into.clone()).or_default().insert(shown.clone());
                continue;
            }
            let Some((path, kept)) = path.clone().filter(|(p, _)| accept(p)) else {
                continue;
            };
            let passages = texts.entry(file).or_insert_with(|| {
                #[cfg(test)]
                self.bodies_read.fetch_add(1, Ordering::Relaxed);
                let stored: usize = db
                    .prepare_cached("SELECT count(*) FROM chunks WHERE file = ?1 AND passage IS NOT NULL")
                    .and_then(|mut q| q.query_row([file], |r| r.get::<_, i64>(0)))
                    .map_or(0, |c| c as usize);
                if stored > READ_WHOLE {
                    sparse.insert(file);
                    return vec![crate::meaning::Passage::default(); stored];
                }
                // Made from the whole file: kept with the vectors. Else cut from the text.
                let whole: Vec<crate::meaning::Passage> = db
                    .prepare_cached("SELECT passage, coalesce(heading, '') FROM chunks WHERE file = ?1 AND passage IS NOT NULL ORDER BY n")
                    .and_then(|mut q| {
                        q.query_map([file], |r| Ok(crate::meaning::Passage { text: r.get(0)?, heading: r.get(1)? }))?
                            .collect()
                    })
                    .unwrap_or_default();
                if whole.is_empty() {
                    db.query_row("SELECT body FROM text WHERE rowid = ?1", [file], |r| {
                        r.get::<_, String>(0)
                    })
                    .map(|b| crate::meaning::passages(&b, &kept))
                    .unwrap_or_default()
                } else {
                    whole
                }
            });
            if sparse.contains(&file) {
                let mut q = match db.prepare_cached("SELECT n, passage, coalesce(heading, '') FROM chunks WHERE file = ?1 AND n BETWEEN ?2 AND ?3 AND passage IS NOT NULL") {
                    Ok(q) => q,
                    Err(_) => continue,
                };
                let rows = q.query_map([file, i64::from(n.saturating_sub(1)), i64::from(n) + 1], |r| {
                    Ok((r.get::<_, i64>(0)? as usize, crate::meaning::Passage { text: r.get(1)?, heading: r.get(2)? }))
                });
                for (i, p) in rows.into_iter().flatten().flatten() {
                    if let Some(at) = passages.get_mut(i) {
                        *at = p;
                    }
                }
            }
            if (n as usize) < passages.len() {
                if folded.is_some() {
                    given.push((file, out.len(), v));
                }
                out.push((path, s, n as usize, file));
            }
        }
        let texts: HashMap<i64, std::rc::Rc<Vec<crate::meaning::Passage>>> =
            texts.into_iter().map(|(f, p)| (f, std::rc::Rc::new(p))).collect();
        out.into_iter().map(|(path, s, n, file)| (path, s, n, texts[&file].clone())).collect()
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
        let words: Vec<String> = query
            .split_whitespace()
            .map(|w| w.replace('"', ""))
            .filter(|w| !w.is_empty())
            .collect();
        if words.is_empty() {
            return (Results::default(), 0);
        }
        let last = words.len() - 1;
        let term = |i: usize, w: &String| format!("\"{w}\"{}", if i == last { "*" } else { "" });
        let all = words
            .iter()
            .enumerate()
            .map(|(i, w)| term(i, w))
            .collect::<Vec<_>>()
            .join(" ");
        let mut found = self.matching(&all, scope, max);
        let every = found.0.len();
        let any: Vec<String> = words
            .iter()
            .enumerate()
            .filter(|(_, w)| w.chars().count() > 3)
            .map(|(i, w)| term(i, w))
            .collect();
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
        (
            Results {
                hits: found.0,
                total: found.1,
                micros: start.elapsed().as_micros() as u64,
            },
            every,
        )
    }

    /// The files whose text matches the FTS query `ask`, best first, up to `max`, and how many
    /// there are; only below `scope`, when given.
    fn matching(&self, ask: &str, scope: Option<&Path>, max: usize) -> (Vec<Hit>, usize) {
        let db = self.db.lock().unwrap();
        // Disks that are not plugged in keep their rows, out of sight.
        let offline: Vec<(String, String)> = self
            .offline
            .lock()
            .unwrap()
            .iter()
            .map(|at| below(at))
            .collect();
        let mut filter: String = (0..offline.len())
            .map(|i| {
                format!(
                    " AND NOT (f.path > ?{} AND f.path < ?{})",
                    2 + 2 * i,
                    3 + 2 * i
                )
            })
            .collect();
        let mut args: Vec<rusqlite::types::Value> = std::iter::once(ask.to_string())
            .chain(offline.into_iter().flat_map(|(from, to)| [from, to]))
            .map(Into::into)
            .collect();
        // A scope: the files below it, and the commits of the repositories below it.
        if let Some((from, to)) = scope.and_then(key).map(|k| below(&k)) {
            let n = args.len();
            filter += &format!(
                " AND ((f.path > ?{a} AND f.path < ?{b}) OR (f.path > ?{c} AND f.path < ?{d}))",
                a = n + 1,
                b = n + 2,
                c = n + 3,
                d = n + 4
            );
            args.extend(
                [
                    from.clone(),
                    to.clone(),
                    format!("git:{from}"),
                    format!("git:{to}"),
                ]
                .map(Into::into),
            );
        }
        let found = || -> rusqlite::Result<(Vec<Hit>, usize)> {
            let total = db.query_row(&format!("SELECT count(*) FROM text JOIN files f ON f.id = text.rowid WHERE text MATCH ?1{filter}"), rusqlite::params_from_iter(&args), |r| r.get::<_, i64>(0))? as usize;
            let mut q = db.prepare(&format!(
                "SELECT f.path, snippet(text, 0, char(1), char(2), '…', 18), CASE WHEN f.path LIKE 'git:%' THEN substr(text.body, 1, instr(text.body, char(10)) - 1) END FROM text JOIN files f ON f.id = text.rowid
                 WHERE text MATCH ?1{filter} ORDER BY rank LIMIT ?{}",
                args.len() + 1
            ))?;
            let hits = q.query_map(
                rusqlite::params_from_iter(args.iter().cloned().chain([(max as i64).into()])),
                |r| {
                    let mut snippet = r
                        .get::<_, String>(1)?
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    // A commit says which it is ("commit a1b2c3d · author · date") before what matched.
                    if let Some(head) = r
                        .get::<_, Option<String>>(2)?
                        .filter(|h| !snippet.replace([MARK.0, MARK.1], "").contains(h.as_str()))
                    {
                        snippet = format!("{head} · {snippet}");
                    }
                    Ok(Hit {
                        path: shown(r.get(0)?),
                        is_dir: false,
                        snippet: Some(snippet),
                        similar: None,
                    })
                },
            )?;
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
    /// Files waiting that changed within `QUIET`: they get theirs once quiet.
    pub vectors_changing: usize,
    pub model: String,
    /// The latest errors, newest first: when, the file, what.
    pub errors: Vec<(u64, Option<String>, String)>,
    /// Bytes of each part: text, vectors, hashes, files, the rest.
    pub parts: Vec<(&'static str, u64)>,
    /// When a file was last read, in seconds since 1970.
    pub read_at: Option<u64>,
}

/// The files still to get their vectors, by `files_unembedded`: quiet since `?1` or dated in
/// the future and unchanged across two scans (to go), or changed after it (changing).
const TO_GO: &str =
    "SELECT count(*) FROM files WHERE has_text = 1 AND embedded IS NULL AND (modified <= ?1 OR settled = modified)";
const CHANGING: &str =
    "SELECT count(*) FROM files WHERE has_text = 1 AND embedded IS NULL AND modified > ?1 AND settled IS NOT modified";

/// The latest `modified` of a file unchanged for `quiet` seconds now.
fn quiet_since(quiet: u64) -> i64 {
    crate::fs::now().saturating_sub(quiet) as i64
}

/// The facts of the store in `path`, opened read-only: an error when it is no search store.
pub fn facts(path: &Path) -> Result<Facts, String> {
    use rusqlite::OpenFlags;
    let mut head = [0u8; 16];
    std::io::Read::read_exact(
        &mut std::fs::File::open(path).map_err(|e| e.to_string())?,
        &mut head,
    )
    .map_err(|e| e.to_string())?;
    if &head != b"SQLite format 3\0" {
        return Err("not a database".into());
    }
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    let tables: HashSet<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .and_then(|mut q| q.query_map([], |r| r.get(0))?.collect())
        .map_err(|e| e.to_string())?;
    if ![
        "files", "text", "chunks", "meta", "hashes", "skipped", "roots",
    ]
    .iter()
    .all(|t| tables.contains(*t))
    {
        return Err("not a search store".into());
    }
    let count = |q: &str| db.query_row(q, [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize;
    let meta = |k: &str| {
        db.query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| {
            r.get::<_, String>(0)
        })
        .ok()
    };
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
        let since = quiet_since(QUIET);
        let count = |q: &str| {
            db.query_row(q, [since], |r| r.get::<_, i64>(0))
                .unwrap_or(0) as usize
        };
        (f.vectors_waiting, f.vectors_changing) = (count(TO_GO), count(CHANGING));
    }
    let mut kinds: HashMap<String, usize> = HashMap::new();
    if let Ok(mut q) =
        db.prepare("SELECT path FROM files WHERE has_text = 1 AND path NOT LIKE 'git:%'")
    {
        let paths = q
            .query_map([], |r| r.get::<_, String>(0))
            .map(|rows| rows.flatten().collect::<Vec<_>>())
            .unwrap_or_default();
        for p in paths {
            let ext = Path::new(&p)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            *kinds.entry(ext).or_default() += 1;
        }
    }
    f.kinds = kinds.into_iter().collect();
    f.kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    if tables.contains("errors") {
        f.errors = db
            .prepare("SELECT at, path, what FROM errors ORDER BY rowid DESC LIMIT 10")
            .and_then(|mut q| {
                q.query_map([], |r| {
                    Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?))
                })?
                .collect()
            })
            .unwrap_or_default();
    }
    // Each table and index's pages, by part: SQLite's own count of them.
    let mut parts: Vec<(&'static str, u64)> = ["text", "vectors", "hashes", "files", "rest"]
        .map(|p| (p, 0))
        .to_vec();
    if let Ok(mut q) = db.prepare("SELECT name, sum(pgsize) FROM dbstat GROUP BY name") {
        let rows = q
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })
            .map(|r| r.flatten().collect::<Vec<_>>())
            .unwrap_or_default();
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
    if cfg.text_roots.is_empty() {
        crate::dirs::home_dir().into_iter().collect()
    } else {
        cfg.text_roots.clone()
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Hidden folders, folders a tool made and can make again, folders marked "names only" and
/// folders holding `.nosearch`.
pub(crate) fn left_out(dir: &Path, cfg: &SearchConfig) -> bool {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    // Coxswain's own cache (the store, previews, files unpacked to be read) is never read: on
    // macOS and Windows it is not a hidden folder.
    static OWN: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    OWN.get_or_init(crate::helper::folder).as_deref() == Some(dir)
        || name.starts_with('.')
        || excluded(&name, cfg)
        || cfg.names_only.iter().any(|p| p == dir)
        || dir.join(".nosearch").exists()
        || cache_tagged(dir)
}

/// A folder a program marked as its cache, as backup tools know it (Cargo's `target`, whatever
/// its name, and other build and cache folders): a `CACHEDIR.TAG` that starts with the
/// signature of the Cache Directory Tagging Specification.
fn cache_tagged(dir: &Path) -> bool {
    const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";
    let mut start = [0; SIGNATURE.len()];
    std::fs::File::open(dir.join("CACHEDIR.TAG"))
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut start))
        .is_ok_and(|_| start == SIGNATURE)
}

/// Folders of caches, package stores, build output and programs' data: the archives in them
/// are a program's, not the user's, and are not looked into (unless `archives_everywhere`, for
/// their names), wherever the folders read are.
pub(crate) fn cache_folder(dir: &Path) -> bool {
    const NAMES: &[&str] = &[
        ".cache",
        ".cargo",
        ".npm",
        ".m2",
        ".gradle",
        ".rustup",
        ".pnpm-store",
        "node_modules",
        "target",
    ];
    static AT: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    let at = AT.get_or_init(|| {
        let home = crate::dirs::home_dir().unwrap_or_default();
        let mut v: Vec<PathBuf> = [crate::dirs::cache_dir(), crate::dirs::data_dir(), crate::dirs::data_local_dir()]
            .into_iter()
            .flatten()
            .collect();
        v.extend([".local/share", "Library/Caches", "AppData/Local"].map(|p| home.join(p)));
        v.extend(["/var/cache", "/var/lib"].map(PathBuf::from));
        v
    });
    dir.file_name()
        .is_some_and(|n| NAMES.iter().any(|x| n == *x))
        || at.iter().any(|p| p == dir)
}

/// Whether the archive at `path` is in a cache folder at or below one of `roots`.
pub(crate) fn in_cache(path: &Path, roots: &[PathBuf]) -> bool {
    path.ancestors()
        .skip(1)
        .take_while(|a| roots.iter().any(|r| a.starts_with(r)))
        .any(cache_folder)
}

/// Whether a folder or file of this name is left out by `text_exclude`: a name (`node_modules`)
/// or a pattern with `*` and `?` (`*.log`, `secret*`).
pub(crate) fn excluded(name: &str, cfg: &SearchConfig) -> bool {
    let name: Vec<char> = name.chars().collect();
    cfg.text_exclude
        .iter()
        .any(|x| crate::index::glob(&x.chars().collect::<Vec<_>>(), &name))
}

/// Whether a `text_exclude` entry is about files too: one with `*`, `?` or a dot (`*.log`,
/// `notes.txt`). A plain name (`build`) leaves out folders only.
fn for_files(entry: &str) -> bool {
    entry.contains(['*', '?', '.'])
}

/// A file whose name `text_exclude` leaves out: found by name and counted, never read.
fn left_out_file(path: &str, cfg: &SearchConfig) -> bool {
    let name: Vec<char> = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .chars()
        .collect();
    cfg.text_exclude
        .iter()
        .filter(|x| for_files(x))
        .any(|x| crate::index::glob(&x.chars().collect::<Vec<_>>(), &name))
}

/// What a walk of some folders found.
#[derive(Default)]
struct Walk {
    /// Files that differ from what the store knew.
    changed: Vec<Changed>,
    /// Folders left out, with their totals.
    skipped: Vec<(String, Size)>,
    /// Folders that are git work trees (they hold a `.git`).
    repos: Vec<PathBuf>,
    /// Files dated in the future and unchanged since the last walk: (path, date).
    settled: Vec<(String, i64)>,
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

/// Walk `dirs` and everything below them, marking in `known` the files met. `None` when
/// `stop` was set meanwhile.
fn walk(
    store: &Store,
    dirs: Vec<PathBuf>,
    cfg: &SearchConfig,
    known: &mut Known,
    stop: &AtomicBool,
) -> Option<Walk> {
    let mut found = Walk::default();
    // Folders left out are only measured, one thread, like the rest of the scan.
    let slow = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("a thread");
    let mut stack = dirs;
    while let Some(dir) = stack.pop() {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        for entry in crate::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let (Ok(kind), path) = (entry.file_type(), entry.path()) else {
                continue;
            };
            // Other apps' data on a Mac: not read, not measured, not even looked at.
            if kind.is_dir() && crate::fs::protected(&path) {
                continue;
            }
            // A repository's history is searched too: git's, Mercurial's, and Jujutsu's where
            // it has no `.git` of its own (with one, it is git's).
            let name = entry.file_name();
            if name == ".git"
                || (kind.is_dir() && name == ".hg")
                || (kind.is_dir() && name == ".jj" && !dir.join(".git").exists())
            {
                found.repos.push(dir.clone());
            }
            if kind.is_dir() {
                // A cloud mount whose files are not read is not walked either: listing it may
                // be the network's work. Its names are the name index's.
                if crate::cloud::unread_mount(&path) {
                    store.saw_cloud(&path);
                } else if !left_out(&path, cfg) {
                    stack.push(path);
                } else if let (Some(text), Some(size)) = (
                    path.to_str(),
                    slow.install(|| crate::sizes::walk(&path, stop)),
                ) {
                    found.skipped.push((text.to_string(), size));
                }
                continue;
            }
            let (true, Some(text), Ok(meta)) = (kind.is_file(), path.to_str(), entry.metadata())
            else {
                continue;
            };
            let modified = secs(&meta);
            let cloud = left_in_cloud(store, &meta, &path);
            // Roots may overlap: a file met twice still matches its row.
            match known.get_mut(text) {
                Some((k, seen)) if *k == (meta.len(), modified, cloud) => {
                    *seen = true;
                    // A date in the future never gets quiet by itself: unchanged is quiet.
                    if modified > now() as i64 {
                        found.settled.push((text.to_string(), modified));
                    }
                }
                Some((_, seen)) => {
                    *seen = true;
                    found
                        .changed
                        .push((text.to_string(), meta.len(), modified, cloud));
                }
                None => found
                    .changed
                    .push((text.to_string(), meta.len(), modified, cloud)),
            }
        }
    }
    Some(found)
}

/// Two passages whose vectors are this alike (cosine) or more are near copies: Ask gives one.
/// Measured: a copy, the same text with another date or number, or saved again as another
/// format, scores 0.98 and up; two passages of one document on the same subject 0.80 to 0.95.
const NEAR_COPY: f32 = 0.97;

/// Hits Ask looks at, four of a file at most, of `ASK_FILES` files at most.
const ASK_HITS: usize = 48;
const ASK_FILES: usize = 16;

/// Passages ranked one per file: the file's best, its score raised by 0.005 for each further
/// passage of it that matches, four at most, so a document that keeps coming back to a
/// question goes ahead of one that mentions it once.
fn by_files(ranked: Vec<(i64, f32, u32, Vec<u8>)>) -> Vec<(i64, f32, u32, Vec<u8>)> {
    let mut files: HashMap<i64, (f32, u32, usize, Vec<u8>)> = HashMap::new();
    for (file, s, n, v) in ranked {
        // Ranked best first: the first passage of a file is its best.
        files.entry(file).or_insert((s, n, 0, v)).2 += 1;
    }
    let mut out: Vec<(i64, f32, u32, Vec<u8>)> = files
        .into_iter()
        .map(|(file, (s, n, count, v))| (file, s + 0.005 * (count - 1).min(4) as f32, n, v))
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// Vectors of passages made another way than `scheme` go, the text stays, and the files get
/// new ones in the background. Checked when the store is opened, by whichever process opens
/// it first: how many files are being renewed, kept in the meta (`renew`) until they are done.
fn passages_are(db: &Connection, scheme: &str) -> rusqlite::Result<usize> {
    let meta = |key: &str| {
        db.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
            r.get::<_, String>(0)
        })
        .unwrap_or_default()
    };
    let stored = meta("passages");
    // A newer Coxswain cut these passages: an older one leaves its vectors alone instead of
    // throwing them away, so going back and forth between versions costs no new vectors.
    let newer = matches!((stored.parse::<u32>(), scheme.parse::<u32>()), (Ok(a), Ok(b)) if a > b);
    if stored != scheme && !newer {
        let tx = db.unchecked_transaction()?;
        let had = tx.execute(
            "UPDATE files SET embedded = NULL WHERE embedded IS NOT NULL",
            [],
        )?;
        tx.execute("DELETE FROM chunks", [])?;
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('passages', ?1)",
            [scheme],
        )?;
        if had > 0 {
            tx.execute(
                "INSERT OR REPLACE INTO meta(key, value) VALUES ('renew', ?1)",
                [had.to_string()],
            )?;
        }
        tx.commit()?;
    }
    Ok(meta("renew").parse().unwrap_or(0))
}

/// Bring the store up to date with the disk, once. Reads and hashes at half speed, one file
/// at a time, so the machine stays the user's. Stops early when `stop` is set.
pub fn scan(store: &Store, cfg: &SearchConfig, stop: &AtomicBool) -> rusqlite::Result<()> {
    scan_for(store, cfg, stop, None).map(|_| ())
}

/// `scan`, with hashes and vectors for no longer than `slice` (when given): `true` when there
/// are more of them to do, for `backlog`.
fn scan_for(
    store: &Store,
    cfg: &SearchConfig,
    stop: &AtomicBool,
    slice: Option<Duration>,
) -> rusqlite::Result<bool> {
    let began = now();
    let (roots, offline) = place(store, cfg)?;
    let tika = crate::extract::tika::extensions();
    let ebooks = crate::ebook::extensions();
    let tools: Vec<&str> = crate::extract::installed::extensions().into_iter().chain(tika.iter().copied()).chain(ebooks.iter().copied()).collect();
    let known: Vec<&str> = crate::extract::installed::kinds().into_iter().chain(crate::extract::tika::FORMATS.iter().copied()).chain(crate::ebook::FORMATS.iter().copied()).collect();
    store.tools_changed(&tools, &known)?;
    // Diagrams got a sentence per arrow; code keeps its indentation and blank lines.
    let mut ends: Vec<String> = [
        "drawio", "dio", "mmd", "mermaid", "dot", "gv", "puml", "plantuml", "pu", "iuml", "wsd",
        "md", "markdown", "mdx",
    ]
    .iter()
    .map(|e| format!(".{e}"))
    .collect();
    ends.extend(crate::code::endings());
    store.readers_changed("code-1", &ends)?;
    // PostScript has a reader of its own (Ghostscript, else plain text): read once again.
    store.reader_changed("readers.postscript", "1", &[".ps", ".eps"])?;
    // Code in more languages (2.32): only the files with the endings new then are read again.
    let added: Vec<String> = crate::code::SINCE_CODE_1.iter().map(|e| format!(".{e}")).collect();
    store.reader_changed("readers.code", "2", &added)?;
    // PGN files: a line per game (players, event, site, date, opening, result), read once
    // again; at 2, the positions of their games too (Find this position).
    store.reader_changed("readers.pgn", "2", &crate::chess::PGN_ENDINGS)?;
    // SCID databases (2.33): their index files are read as a line per game and its positions.
    store.reader_changed("readers.scid", "1", &[".si4"])?;
    // ChessBase databases (2.33): a line per game and their positions, as PGN files; read once.
    store.reader_changed("readers.cbh", "1", &[".cbh"])?;
    // SGF files: a line per game (players, ranks, event, place, date, result), read once again.
    store.reader_changed("readers.sgf", "1", &crate::go::ENDINGS)?;
    // PDN files: a line per draughts game (players, event, place, date, result), read once again.
    store.reader_changed("readers.pdn", "1", &crate::draughts::ENDINGS)?;
    // KIF and CSA files: a line per shogi game (players, event, place, date, result), read once again.
    store.reader_changed("readers.shogi", "1", &crate::shogi::ENDINGS)?;
    // PBN files: a line per bridge deal (the pairs, event, place, date, contract and result), read once again.
    store.reader_changed("readers.pbn", "1", &crate::bridge::ENDINGS)?;
    // Outlook messages and dBase tables have readers of their own: read once again.
    store.reader_changed("readers.msg-dbf", "1", &[".msg", ".dbf"])?;
    // Apache Tika, once it is turned on and its image is there: its own version, so a change
    // to what it gets reads only its files again. Nothing while it is off.
    if !tika.is_empty() {
        let ends: Vec<String> = tika.iter().map(|e| format!(".{e}")).collect();
        store.reader_changed("readers.tika", crate::extract::tika::READER, &ends)?;
    }
    // Plain text, code, CSV and logs of any size are read line by line (2.33): those left
    // without text for being larger than `text_max_size` are read, once. Only files without
    // text and that large; a larger file of another kind is passed over again at once.
    store.large_unread("readers.large", "1", cfg.text_max_size)?;
    // Their passages and those of every text, code, CSV or log file with more text than is
    // kept are made from the whole file (2.33): only those files get their vectors again, once.
    store.long_passages("passages.whole", "1", LONG_MARGIN, crate::extract::streams)?;
    // So are those of Word, OpenDocument, workbook and presentation files with nearly as much
    // text as is kept, or more (2.33): only those get their vectors again, once.
    store.long_passages("passages.documents", "1", DOCUMENT_MARGIN, crate::extract::whole_document)?;
    // What is kept of long files, *Sample* or *Everything* (2.34): only the long files whose
    // folder's choice changed are read again, get their vectors again, or are cut back.
    store.long_files_changed(&crate::long_files::Rule::of(cfg))?;
    // PDF and Word files leave out hidden text (2.33): their old text and passages go, and
    // they are read and their passages made again, once.
    store.reader_dropped("readers.pdf", "1", &[".pdf"])?;
    store.reader_dropped("readers.word", "1", &[".docx", ".docm", ".dotx", ".odt", ".ott"])?;
    // E-books through calibre, once they are turned on and calibre can run: a version of their
    // own, so a change to what it gets reads only these books again. Nothing while it is off.
    if !ebooks.is_empty() {
        let ends: Vec<String> = ebooks.iter().map(|e| format!(".{e}")).collect();
        store.reader_changed("readers.ebooks", crate::ebook::READER, &ends)?;
    }
    store.archives_changed(cfg.archives)?;
    store.exclude_changed(&cfg.text_exclude)?;
    let mut known = store.known()?;
    let Some(found) = walk(store, roots.clone(), cfg, &mut known, stop) else {
        return Ok(false);
    };
    // Rows of a disk that is not plugged in stay.
    let kept: Vec<(String, String)> = offline.iter().map(|at| below(at)).collect();
    // Commits are not files on disk: `history` keeps them.
    let gone: Vec<String> = known
        .into_iter()
        .filter(|(path, (_, seen))| {
            !seen
                && !path.starts_with("git:")
                && !kept.iter().any(|(from, to)| path > from && path < to)
        })
        .map(|(path, _)| path)
        .collect();
    *store.offline.lock().unwrap() = offline;
    store.apply(
        &gone,
        &found.changed,
        &found.skipped,
        true,
        cfg.archives.then_some(&roots[..]),
    )?;
    store.settle(&found.settled)?;
    *store.walked.lock().unwrap() = Some((roots, began));
    if read(store, cfg, stop)? {
        return Ok(false);
    }
    // Git in a repository left in the cloud would download it.
    let repos = if cfg.history {
        found
            .repos
            .into_iter()
            .filter(|r| !crate::cloud::git_kept_out(r))
            .collect()
    } else {
        vec![]
    };
    if history(store, &repos, &kept, stop)? {
        return Ok(false);
    }

    store.prune_hashes()?;
    // Files the server refused last time get one more try per scan; one it refuses for good
    // costs one quick answer every ten minutes, and never holds up the rest.
    if store.engine().is_some() {
        store
            .db
            .lock()
            .unwrap()
            .execute("UPDATE files SET embedded = NULL WHERE embedded = 0", [])?;
    }
    backlog(store, stop, slice)
}

/// Hashes, then vectors, for no longer than `slice` when given, so what the watcher saw is
/// followed in between (a saved file found by its words within a minute, not after hours of
/// vectors). `true` when there is more to do.
fn backlog(store: &Store, stop: &AtomicBool, slice: Option<Duration>) -> rusqlite::Result<bool> {
    // On battery the work waits for the mains here, a second at a time, the watcher followed;
    // with none left the backlog is done, not paused.
    if slice.is_some() && !store.hurry.load(Ordering::Relaxed) && crate::machine::on_battery() {
        if !has_backlog(store)? {
            store.paused.store(false, Ordering::Relaxed);
            return Ok(false);
        }
        store.paused.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_secs(1));
        return Ok(true);
    }
    store.paused.store(false, Ordering::Relaxed);
    let until = slice.map(|s| Instant::now() + s);
    if hash(store, stop, until)? {
        return Ok(!stop.load(Ordering::Relaxed));
    }
    embed(store, stop, until)
}

/// Whether there are files to hash, or (with search by meaning on) to give their vectors.
fn has_backlog(store: &Store) -> rusqlite::Result<bool> {
    Ok(store.engine().is_some() && !store.unembedded(1)?.is_empty()
        || !store.unhashed()?.is_empty())
}

/// Hash the files that share a size. `true` when stopped, or at `until`.
fn hash(store: &Store, stop: &AtomicBool, until: Option<Instant>) -> rusqlite::Result<bool> {
    for batch in store.unhashed()?.chunks(50) {
        let start = Instant::now();
        let rows: Vec<_> = batch
            .iter()
            .filter_map(|(path, size, modified)| {
                // Hashed again next time, once its date can tell a change.
                if !crate::dupes::settled(*modified) {
                    return None;
                }
                Some((
                    path.clone(),
                    *size,
                    *modified,
                    crate::dupes::hash_file(path, None, stop, None)
                        .ok()?
                        .to_hex()
                        .to_string(),
                ))
            })
            .collect();
        store.put_hashes(&rows)?;
        if stop.load(Ordering::Relaxed) {
            return Ok(true);
        }
        store.rest_after(start.elapsed(), stop, until);
        if until.is_some_and(|u| Instant::now() >= u) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The passages of a file whose text is longer than what is kept of it (`body`, cut at
/// `extract::MAX_TEXT`), made from the whole file (its first gigabyte), read twice line by
/// line: `cap`'s choice spread over all of it, not only its start. `None` for every other file (its passages are
/// cut from `body`), and for one that cannot be read now. Text, code, CSV and logs, the
/// kinds read line by line (`extract::streams`), and Word, OpenDocument, workbook and
/// presentation files, read as they are unpacked (`extract::whole_lines`).
fn whole_passages(path: &str, body: &str) -> Option<Vec<crate::meaning::Passage>> {
    // Near the limit: the next line did not fit. A text that was not cut gives the same
    // passages read whole, so a file taken for cut costs a reading, never other passages.
    if !long_text(path, body) {
        return None;
    }
    crate::meaning::passages_streamed(path, &mut |f| whole_lines(path, f))
}

/// Each line of the whole text of the file at `path`, as `whole_passages` reads it, to `f`.
/// `false` when it cannot be read now.
fn whole_lines(path: &str, f: &mut dyn FnMut(&str)) -> bool {
    let p = Path::new(path);
    if crate::extract::whole_document(p) {
        crate::extract::whole_lines(p, f).is_some()
    } else {
        crate::extract::plain::each_line(p, crate::code::is_code(path), f).is_some()
    }
}

/// Whether `body`, the text kept of the file at `path`, is as long as what is kept of most files
/// or longer, and of a kind read line by line or a document read as it is unpacked
/// (`extract::whole_document`): its passages come from the file itself. A document's text is
/// cut before it is tidied, so it may end further below the limit.
fn long_text(path: &str, body: &str) -> bool {
    let p = Path::new(path);
    (body.len() + LONG_MARGIN >= crate::extract::MAX_TEXT && crate::extract::streams(p))
        || (body.len() + DOCUMENT_MARGIN >= crate::extract::MAX_TEXT && crate::extract::whole_document(p))
}

/// How a file kept whole got its vectors (`embed_streamed`).
enum Streamed {
    /// With this many passages.
    Done(usize),
    Stopped,
    Failed(crate::meaning::NoVectors),
}

/// Passages that go to the model at a time, for a file kept whole.
const STREAM_BATCH: usize = 64;

/// The vectors of every passage of the file at `path` (`id`), kept whole: read line by line,
/// a batch at a time to the model, each batch written with its passages and where sections
/// start; the file is done (`embedded`) once the last is in. Stopped or refused, the rows
/// written go again.
fn embed_streamed(
    store: &Store,
    engine: &crate::meaning::Engine,
    id: i64,
    path: &str,
    stop: &AtomicBool,
) -> rusqlite::Result<Streamed> {
    let had = store.db.lock().unwrap().execute("DELETE FROM chunks WHERE file = ?1", [id])? > 0;
    let (mut batch, mut packed) = (Vec::with_capacity(STREAM_BATCH), vec![]);
    let mut halt: Option<rusqlite::Result<Streamed>> = None;
    // A file that cannot be read now has no passages, as one without text.
    crate::meaning::every_passage_streamed(
        path,
        &mut |f| whole_lines(path, f),
        &mut |p, starts| {
            batch.push((p, starts));
            if batch.len() < STREAM_BATCH {
                return true;
            }
            match put_batch(store, engine, id, path, &mut batch, &mut packed, stop) {
                Ok(None) => true,
                h => {
                    halt = Some(h.map(|h| h.unwrap_or(Streamed::Stopped)));
                    false
                }
            }
        },
    );
    let end = match halt {
        Some(h) => h?,
        None => match put_batch(store, engine, id, path, &mut batch, &mut packed, stop)? {
            Some(h) => h,
            None => Streamed::Done(packed.len()),
        },
    };
    let mut db = store.db.lock().unwrap();
    if !matches!(end, Streamed::Done(_)) {
        db.execute("DELETE FROM chunks WHERE file = ?1", [id])?;
        drop(db);
        if had {
            store.commit_signs(|| Ok(()), SignsChange::Drop(HashSet::from([id])))?;
        }
        return Ok(end);
    }
    let tx = db.transaction()?;
    tx.execute("UPDATE files SET embedded = 1 WHERE id = ?1", [id])?;
    store.commit_signs(|| tx.commit(), SignsChange::Put(id, packed, had))?;
    Ok(end)
}

/// `batch` to the model and into `chunks` after the `packed` ones (whose vectors it joins);
/// `Some` when it stopped there.
fn put_batch(
    store: &Store,
    engine: &crate::meaning::Engine,
    id: i64,
    path: &str,
    batch: &mut Vec<(crate::meaning::Passage, bool)>,
    packed: &mut Vec<Vec<u8>>,
    stop: &AtomicBool,
) -> rusqlite::Result<Option<Streamed>> {
    if stop.load(Ordering::Relaxed) || store.cleared.load(Ordering::Relaxed) {
        return Ok(Some(Streamed::Stopped));
    }
    if batch.is_empty() {
        return Ok(None);
    }
    let texts: Vec<String> = batch.iter().map(|(p, _)| crate::meaning::shown_to_model(path, p)).collect();
    let vectors = match engine.passages(&texts) {
        Ok(v) => v,
        Err(e) => return Ok(Some(Streamed::Failed(e))),
    };
    let mut db = store.db.lock().unwrap();
    let tx = db.transaction()?;
    for ((p, starts), v) in batch.drain(..).zip(vectors) {
        let v = crate::meaning::pack(&v);
        tx.execute(
            "INSERT INTO chunks(file, n, vector, passage, heading, starts) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, packed.len() as i64, v, p.text, p.heading, starts],
        )?;
        packed.push(v);
    }
    tx.commit()?;
    Ok(None)
}

/// A file on disk as `Store::long_rows` gives it.
struct LongRow {
    id: i64,
    path: String,
    modified: i64,
    whole: Option<i64>,
    size: u64,
    has_text: bool,
}

impl LongRow {
    /// Read line by line and large enough that its text may have been cut at `MAX_TEXT`.
    fn may_be_cut(&self) -> bool {
        self.size as usize + LONG_MARGIN >= crate::extract::MAX_TEXT && crate::extract::streams(Path::new(&self.path))
    }
}

/// The bytes of a file's text in the store.
fn text_len(db: &Connection, id: i64) -> usize {
    db.query_row("SELECT length(CAST(body AS BLOB)) FROM text WHERE rowid = ?1", [id], |r| r.get::<_, i64>(0))
        .map_or(0, |n| n as usize)
}

/// About how many bytes of text a passage stands for (`meaning::WORDS` words less the
/// `OVERLAP`), and what a passage's row holds besides its vector (its text, its numbers).
const PASSAGE_BYTES: usize = 600;
const PASSAGE_ROW: u64 = 800;

/// A file kept whole with more passages than this has only those a search needs read.
const READ_WHOLE: usize = 4 * crate::meaning::PASSAGES;

/// The most text `read` holds before it writes what it has.
const HELD_TEXT: usize = 64 << 20;

/// How far below `extract::MAX_TEXT` a text may end and still be one that was cut.
const LONG_MARGIN: usize = 64 << 10;
/// The same for a document, whose text is cut before it is tidied: blank space the tidying
/// takes away is little: the line breaks between paragraphs, the spaces after cells.
const DOCUMENT_MARGIN: usize = crate::extract::MAX_TEXT / 4;

/// Give the passages of files with text their vectors, when search by meaning is on: a few
/// files at a time, resting as long as the work took, never on battery unless *Index now*.
/// `true` when stopped at `until` with files still to go.
// ponytail: files the watcher brings get theirs in the next slice, or at the next scan within
// ten minutes, once quiet (`QUIET`); the text is searchable by its words at once.
fn embed(store: &Store, stop: &AtomicBool, until: Option<Instant>) -> rusqlite::Result<bool> {
    use crate::meaning::NoVectors;
    let Some(engine) = store.engine() else {
        return Ok(false);
    };
    store.model_is(&engine.id(), engine.digest().as_deref())?;
    loop {
        let files = store.unembedded_whole(8)?;
        if files.is_empty() {
            if store.renewing.swap(0, Ordering::Relaxed) > 0 {
                store
                    .db
                    .lock()
                    .unwrap()
                    .execute("DELETE FROM meta WHERE key = 'renew'", [])?;
            }
            return Ok(false);
        }
        let (start, mut count) = (Instant::now(), 0);
        for (id, path, body, everything) in files {
            // Kept whole, longer than what is kept of most files: read line by line again and
            // a passage for every part of it, a batch at a time, never held whole.
            if everything && long_text(&path, &body) {
                match embed_streamed(store, &engine, id, &path, stop)? {
                    Streamed::Done(n) => count += n,
                    Streamed::Stopped => return Ok(false),
                    Streamed::Failed(NoVectors::Down(e)) => {
                        store.note_error(Some(path.as_str()), &e);
                        *store.meaning_error.lock().unwrap() = Some(e);
                        return Ok(false);
                    }
                    Streamed::Failed(NoVectors::Refused(e)) => {
                        store.note_error(Some(path.as_str()), &e);
                        *store.meaning_error.lock().unwrap() = Some(e);
                        store.db.lock().unwrap().execute("UPDATE files SET embedded = 0 WHERE id = ?1", [id])?;
                    }
                }
                *store.meaning_error.lock().unwrap() = None;
                continue;
            }
            // Kept whole: every passage, kept with the vectors when there are more than
            // `meaning::passages` keeps (they are no longer what it cuts from the text), with
            // where sections start for going back to *Sample*.
            let (whole, starts) = if everything {
                let (all, starts): (Vec<_>, Vec<_>) = crate::meaning::every_passage(&body, &path).into_iter().unzip();
                if all.len() > crate::meaning::PASSAGES {
                    (Some(all), Some(starts))
                } else {
                    (None, None)
                }
            } else {
                (whole_passages(&path, &body), None)
            };
            let passages = whole.clone().unwrap_or_else(|| crate::meaning::passages(&body, &path));
            let texts: Vec<String> = passages
                .iter()
                .map(|p| crate::meaning::shown_to_model(&path, p))
                .collect();
            count += texts.len();
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
                    return Ok(false);
                }
                Err(NoVectors::Refused(e)) => {
                    store.note_error(Some(path.as_str()), &e);
                    *store.meaning_error.lock().unwrap() = Some(e);
                    store
                        .db
                        .lock()
                        .unwrap()
                        .execute("UPDATE files SET embedded = 0 WHERE id = ?1", [id])?;
                    continue;
                }
            };
            *store.meaning_error.lock().unwrap() = None;
            store.put_passages(
                id,
                &vectors
                    .iter()
                    .map(|v| crate::meaning::pack(v))
                    .collect::<Vec<_>>(),
                whole.as_deref(),
                starts.as_deref(),
            )?;
            if stop.load(Ordering::Relaxed) || store.cleared.load(Ordering::Relaxed) {
                return Ok(false);
            }
        }
        // A passage's time with the rest after it (not a wait for the mains), for the time the
        // files still to go take: per passage, not per file, since the newest files go first
        // and a few long ones would set the pace for all.
        // The built-in model works on this processor and rests as long as it worked; a server
        // works on its own (most often a GPU) and the helper only waits for it, so it rests a
        // quarter of that.
        let work = start.elapsed();
        let rest = if matches!(*engine, crate::meaning::Engine::Builtin(_)) {
            work
        } else {
            work / 4
        };
        let ms = (work + rest.min(Duration::from_secs(2))).as_millis() as usize / count.max(1);
        store.rest_after(rest, stop, until);
        let before = store.ms_per_passage.load(Ordering::Relaxed);
        let ms = if before == 0 { ms } else { (before * 7 + ms) / 8 };
        store.ms_per_passage.store(ms, Ordering::Relaxed);
        store.db.lock().unwrap().execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('ms_per_passage', ?1)", [ms.to_string()])?;
        if until.is_some_and(|u| Instant::now() >= u) {
            return Ok(true);
        }
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
            let elsewhere = known
                .and_then(|(volume, inside, _)| {
                    crate::machine::locate(volume.as_deref()?, Path::new(inside.as_deref()?))
                })
                .filter(|p| p.is_dir());
            let full = || std::fs::read_dir(&root).is_ok_and(|mut d| d.next().is_some());
            elsewhere.or_else(|| full().then(|| root.clone()))
        };
        match at {
            Some(at) => {
                let Some(at_text) = key(&at) else { continue };
                let volume = if at == root {
                    volume.clone()
                } else {
                    crate::machine::volume(&at)
                };
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
    store
        .pending
        .store(store.unread_count()?, Ordering::Relaxed);
    let done = |n: usize| {
        store.pending.fetch_sub(n, Ordering::Relaxed);
        stop.load(Ordering::Relaxed) || store.cleared.load(Ordering::Relaxed)
    };
    // In batches by id: every row read gets its `has_text`, and the cursor ends the loop even
    // if one did not. A file in a folder kept whole (`long_files`) keeps all its text.
    let rule = crate::long_files::Rule::of(cfg);
    let mut after = 0;
    // A file that is not there now (its disk unplugged, or it went since the scan) is not read
    // as one without text: it keeps what it had, and waits for its disk or for the next scan.
    let away: Vec<(String, String)> = store.offline.lock().unwrap().iter().map(|at| below(at)).collect();
    let away = |p: &str| away.iter().any(|(from, to)| p > from.as_str() && p < to.as_str());
    loop {
        let batch = store.unread_disk(after, 200)?;
        let Some(last) = batch.last() else { break };
        after = last.0;
        let n = batch.len();
        let batch: Vec<_> = batch
            .into_iter()
            .filter(|(_, path, _)| !away(path) && std::fs::symlink_metadata(path).is_ok())
            .collect();
        let start = Instant::now();
        let (mut rows, mut held, mut whole) = (vec![], 0, vec![]);
        for (id, path, size) in &batch {
            let keep = rule.whole(Path::new(path));
            let cap = if keep { crate::extract::EVERYTHING_TEXT } else { crate::extract::MAX_TEXT };
            // A file that ran past a program's time limit (a LibreOffice that never ends) is
            // not given another two minutes each time its program comes back, until it changes.
            let text = if left_out_file(path, cfg) {
                None
            } else if store.too_long(*id) {
                store.transcript_of(*id)
            } else {
                crate::tools::took_too_long();
                let text = crate::extract::text_up_to(Path::new(path), *size, cfg.text_max_size, cap);
                if text.is_none() && crate::tools::took_too_long() {
                    store.mark_too_long(*id)?;
                    store.note_error(Some(path), "took too long");
                }
                text.or_else(|| store.transcript_of(*id))
            };
            held += text.as_ref().map_or(0, String::len);
            whole.push((*id, keep, text.is_some()));
            rows.push((*id, text));
            // Text kept whole may be large: written before more is held.
            if held > HELD_TEXT {
                store.read(&std::mem::take(&mut rows))?;
                held = 0;
            }
        }
        store.read(&rows)?;
        store.mark_whole(&whole)?;
        // PGN files the text reader read (not left out, not online-only, not empty): the
        // positions of their games too, game by game as the file is read.
        let halt = || stop.load(Ordering::Relaxed) || store.cleared.load(Ordering::Relaxed);
        for ((id, path, _), (_, _, text)) in batch.iter().zip(&whole) {
            if *text && crate::chess::format(Path::new(path)) == Some(crate::chess::Format::Pgn) {
                store.put_positions(*id, Path::new(path), &halt)?;
            }
        }
        if done(n) {
            return Ok(true);
        }
        store.rest(start, stop);
    }
    let inside = store.unread_inside()?;
    let mut archives: HashMap<PathBuf, Vec<(i64, String)>> = HashMap::new();
    let mut lost = vec![];
    // Which archive a folder inside one is in: found once per folder, not per member.
    let mut holders: HashMap<PathBuf, Option<(PathBuf, String)>> = HashMap::new();
    for (id, path) in inside.into_iter().filter(|(_, path)| !away(path)) {
        let path = Path::new(&path);
        let holder = path.parent().map(|dir| {
            holders
                .entry(dir.to_path_buf())
                .or_insert_with(|| crate::archive::split(dir))
                .clone()
        });
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        match (holder.flatten(), name) {
            (Some((archive, inner)), Some(name))
                if !left_out_file(&path.to_string_lossy(), cfg) =>
            {
                let inner = if inner.is_empty() {
                    name
                } else {
                    format!("{inner}/{name}")
                };
                archives.entry(archive).or_default().push((id, inner));
            }
            _ => lost.push((id, None)),
        }
    }
    store.read(&lost)?;
    for (archive, files) in archives {
        // An archive that went since the scan: its files wait for the next one.
        if std::fs::symlink_metadata(&archive).is_err() {
            continue;
        }
        let start = Instant::now();
        let n = files.len();
        store.read(&read_inside(&archive, files, cfg.text_max_size))?;
        if done(n) {
            return Ok(true);
        }
        store.rest(start, stop);
    }
    // What waits for its disk, or went since the scan, is not being read.
    store.pending.store(0, Ordering::Relaxed);
    Ok(false)
}

/// The text of `files` (id, path inside) of `archive`, as `text_of` reads them from disk, each
/// no larger than `max`, and no more than `archive::SEARCH_READ` bytes from the archive. Each
/// is unpacked into a folder of the cache only the user can read, and is gone once read.
fn read_inside(archive: &Path, files: Vec<(i64, String)>, max: u64) -> Vec<(i64, Option<String>)> {
    let want: HashMap<String, i64> = files
        .iter()
        .map(|(id, inner)| (inner.clone(), *id))
        .collect();
    let mut texts: HashMap<i64, Option<String>> = files.iter().map(|(id, _)| (*id, None)).collect();
    let dir = crate::helper::folder().map(|d| d.join(format!("inside-{}", std::process::id())));
    if let Some(dir) = dir.as_ref().filter(|d| std::fs::create_dir_all(d).is_ok()) {
        #[cfg(unix)]
        let _ = std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
        let left = std::cell::Cell::new(crate::archive::SEARCH_READ);
        let size = std::fs::metadata(archive).map_or(0, |m| m.len());
        let wanted = |name: &str, size: u64| {
            size > 0 && size <= max.min(left.get()) && want.contains_key(name)
        };
        let _ = crate::archive::search_read(archive, size, &wanted, &mut |name, from| {
            let limit = max.min(left.get());
            let file = dir.join(name.rsplit('/').next().unwrap_or(name));
            let copied = std::fs::File::create(&file).and_then(|mut to| {
                std::io::copy(&mut std::io::Read::take(from, limit + 1), &mut to)
            });
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
fn history(
    store: &Store,
    repos: &[PathBuf],
    kept: &[(String, String)],
    stop: &AtomicBool,
) -> rusqlite::Result<bool> {
    let meta = |k: &str| -> Option<String> {
        store
            .db
            .lock()
            .unwrap()
            .query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get(0))
            .ok()
    };
    let set = |k: &str, v: Option<&str>| -> rusqlite::Result<()> {
        let db = store.db.lock().unwrap();
        match v {
            Some(v) => db.execute(
                "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
                [k, v],
            ),
            None => db.execute("DELETE FROM meta WHERE key = ?1", [k]),
        }
        .map(drop)
    };
    // Where each repository's commits are: below this key.
    let rows = |root: &Path| {
        crate::history::key(root, "")
            .trim_end_matches(std::path::MAIN_SEPARATOR)
            .to_string()
    };
    let had: Vec<String> = {
        let db = store.db.lock().unwrap();
        let mut q = db.prepare("SELECT substr(key, 9) FROM meta WHERE key LIKE 'history:%'")?;
        q.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let now: HashSet<String> = repos.iter().filter_map(|r| key(r)).collect();
    for root in had
        .iter()
        .filter(|r| !now.contains(*r) && !kept.iter().any(|(from, to)| *r > from && *r < to))
    {
        store.apply(&[rows(Path::new(root))], &[], &[], false, None)?;
        set(&format!("history:{root}"), None)?;
    }
    for repo in repos {
        if stop.load(Ordering::Relaxed) || store.cleared.load(Ordering::Relaxed) {
            return Ok(true);
        }
        let Some(root) = key(repo) else { continue };
        let old = meta(&format!("history:{root}"));
        let Some((head, grew)) = crate::history::head(repo, old.as_deref()) else {
            continue;
        };
        if old.as_deref() == Some(head.as_str()) {
            continue;
        }
        let start = Instant::now();
        // Rewritten (or new): read from the start.
        if !grew {
            store.apply(&[rows(repo)], &[], &[], false, None)?;
        }
        let Ok(commits) =
            crate::history::for_search(repo, &head, old.as_deref().filter(|_| grew), HISTORY_MAX)
        else {
            continue;
        };
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
pub fn refresh(
    store: &Store,
    cfg: &SearchConfig,
    paths: &mut HashSet<PathBuf>,
    later: &mut HashSet<PathBuf>,
    stop: &AtomicBool,
) -> rusqlite::Result<()> {
    let Some((roots, _)) = store.walked.lock().unwrap().clone() else {
        return Ok(());
    };
    let began = now();
    let (mut gone, mut changed, mut new, mut again) = (vec![], vec![], vec![], HashSet::new());
    for path in paths.iter() {
        let Some(text) = key(path) else { continue };
        let path = PathBuf::from(&text);
        let Some(root) = roots.iter().find(|r| path.starts_with(r) && path != **r) else {
            continue;
        };
        // The topmost folder left out on the way down, this one included if it is a folder.
        let mut down: Vec<&Path> = path.ancestors().take_while(|a| a != root).collect();
        down.reverse();
        if down.iter().any(|a| crate::fs::protected(a)) {
            continue;
        }
        let is_dir = path.is_dir();
        if let Some(out) = down
            .iter()
            .find(|a| (**a != path || is_dir) && left_out(a, cfg))
        {
            later.insert(out.to_path_buf());
            continue;
        }
        // A `.nosearch` or `CACHEDIR.TAG` that came or went changes what its folder is.
        if path
            .file_name()
            .is_some_and(|n| n == ".nosearch" || n == "CACHEDIR.TAG")
        {
            later.extend(path.parent().map(Path::to_path_buf));
        }
        match std::fs::symlink_metadata(&path) {
            Err(_) => gone.push(text),
            Ok(meta) if meta.is_file() => {
                let now = (meta.len(), secs(&meta), left_in_cloud(store, &meta, &path));
                if store.row(&text) != Some(now) {
                    let settling = cfg.archives
                        && crate::archive::is_archive(&path)
                        && meta
                            .modified()
                            .ok()
                            .and_then(|t| t.elapsed().ok())
                            .is_some_and(|age| age < SETTLE);
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
    let Some(found) = walk(store, new, cfg, &mut Known::default(), stop) else {
        return Ok(());
    };
    changed.extend(found.changed);
    store.apply(
        &gone,
        &changed,
        &found.skipped,
        false,
        cfg.archives.then_some(&roots[..]),
    )?;
    walked_now(store, began);
    read(store, cfg, stop).map(|_| ())
}

/// Measure the folders in `later` again, and take them from it: folders left out that
/// changed, and folders that stopped or started being left out.
pub fn measure(
    store: &Store,
    cfg: &SearchConfig,
    later: &mut HashSet<PathBuf>,
    stop: &AtomicBool,
) -> rusqlite::Result<()> {
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
    let Some(found) = walk(store, again, cfg, &mut Known::default(), stop) else {
        return Ok(());
    };
    skipped.extend(found.skipped);
    store.apply(
        &gone,
        &found.changed,
        &skipped,
        false,
        cfg.archives.then(|| roots(cfg)).as_deref(),
    )?;
    walked_now(store, began);
    read(store, cfg, stop).map(|_| ())
}

/// Keep the store current until `stop`: a scan, then the watcher's `changes` within seconds
/// (inside folders left out within a minute, and within a `SLICE` while hashes and vectors
/// are still being made), and a scan every ten minutes for what the watcher missed.
pub fn keep_current(
    store: &Store,
    cfg: &SearchConfig,
    stop: &AtomicBool,
    changes: Option<std::sync::mpsc::Receiver<Vec<PathBuf>>>,
) {
    let report = |r: rusqlite::Result<()>| {
        if let Err(e) = r {
            crate::helper::note(format_args!("search store: {e}"));
            store.note_error(None, &e.to_string());
            *store.error.lock().unwrap() = Some(e.to_string());
        }
    };
    while !stop.load(Ordering::Relaxed) {
        store.cleared.store(false, Ordering::SeqCst);
        let scanned = scan_for(store, cfg, stop, Some(SLICE));
        let mut more = matches!(scanned, Ok(true));
        // Index now is done with once the backlog is, whether the scan worked or not: a
        // failing one is not run again at once, over and over.
        if !more {
            store.hurry.store(false, Ordering::Relaxed);
        }
        if scanned.is_ok() {
            *store.error.lock().unwrap() = None;
        }
        store.warm_signs();
        report(scanned.map(|_| ()));
        let (mut rest, mut refreshed, mut measured) =
            (Instant::now(), Instant::now(), Instant::now());
        let (mut now, mut later) = (HashSet::new(), HashSet::new());
        // Again ten minutes after the hashes and vectors are done, or at Index now; not while
        // they go on (Index now only takes the rests from them then).
        let calm = |rest: Instant| {
            rest.elapsed() < Duration::from_secs(600) && !store.hurry.load(Ordering::Relaxed)
        };
        while (more || calm(rest))
            && !store.cleared.load(Ordering::Relaxed)
            && !stop.load(Ordering::Relaxed)
        {
            match changes
                .as_ref()
                .map(|c| c.recv_timeout(Duration::from_millis(200)))
            {
                Some(Ok(paths)) => now.extend(paths),
                Some(Err(std::sync::mpsc::RecvTimeoutError::Timeout)) => {}
                _ => std::thread::sleep(Duration::from_millis(200)),
            }
            // All that came during a slice, at once.
            if let Some(c) = &changes {
                now.extend(c.try_iter().flatten());
            }
            if !now.is_empty() && refreshed.elapsed() > Duration::from_secs(2) {
                report(refresh(store, cfg, &mut now, &mut later, stop));
                refreshed = Instant::now();
            }
            if !later.is_empty() && measured.elapsed() > Duration::from_secs(60) {
                report(measure(store, cfg, &mut later, stop));
                measured = Instant::now();
            }
            if more {
                let r = backlog(store, stop, Some(SLICE));
                more = matches!(r, Ok(true));
                if !more {
                    store.hurry.store(false, Ordering::Relaxed);
                }
                report(r.map(|_| ()));
                rest = Instant::now();
            }
        }
    }
}

/// How long hashes and vectors go on before what the watcher saw is followed.
const SLICE: Duration = Duration::from_secs(60);

/// A file waiting to be transcribed: where it is, how long it plays when known, and the
/// seconds of it done.
pub(crate) struct Waiting {
    pub path: String,
    pub length: Option<u64>,
    pub done: u64,
}

/// A file's size and modification time, as the store keeps them.
pub(crate) fn meta_of(meta: &std::fs::Metadata) -> (u64, i64) {
    (meta.len(), secs(meta))
}

/// Transcripts (`transcribe`): kept by path with the size and time of the file they were made
/// of; `finished` is 0 while waiting or under way, 1 when done, 2 when given up.
impl Store {
    /// Ask for `files` to be transcribed: those not done yet, or changed since. How many.
    pub fn transcribe_ask(&self, files: &[PathBuf]) -> rusqlite::Result<usize> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let mut n = 0;
        for f in files {
            let (Some(path), Ok(meta)) = (key(f), std::fs::metadata(f)) else { continue };
            let (size, modified) = meta_of(&meta);
            let kept: Option<(u64, i64, i64)> = tx
                .query_row("SELECT size, modified, finished FROM transcripts WHERE path = ?1", [&path], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?)))
                .ok();
            if kept.is_some_and(|(s, m, f)| s == size && m == modified && f != 2) {
                continue;
            }
            tx.execute(
                "INSERT OR REPLACE INTO transcripts(path, size, modified, asked) VALUES (?1, ?2, ?3, ?4)",
                params![path, size as i64, modified, now() as i64],
            )?;
            n += 1;
        }
        tx.commit()?;
        Ok(n)
    }

    /// The next file to work on: the one under way, else the one asked for first.
    pub(crate) fn transcript_next(&self) -> rusqlite::Result<Option<Waiting>> {
        let db = self.db.lock().unwrap();
        let r = db.query_row(
            "SELECT path, length, done FROM transcripts WHERE finished = 0 ORDER BY done = 0, asked, path LIMIT 1",
            [],
            |r| Ok(Waiting { path: r.get(0)?, length: r.get::<_, Option<i64>>(1)?.map(|l| l as u64), done: r.get::<_, i64>(2)? as u64 }),
        );
        match r {
            Ok(w) => Ok(Some(w)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// A part of `path` transcribed, up to `done` seconds; `meta` is the file as it is now. A
    /// file that changed since its first part is begun again. Done, its text is the file's
    /// text in the store when the store reads the file, at once.
    pub(crate) fn transcript_part(&self, path: &str, meta: (u64, i64), length: Option<u64>, done: u64, text: &str, finished: bool) -> rusqlite::Result<()> {
        let db = self.db.lock().unwrap();
        let kept: Option<(u64, i64, String)> = db
            .query_row("SELECT size, modified, text FROM transcripts WHERE path = ?1 AND finished = 0", [path], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?)))
            .ok();
        let Some((size, modified, before)) = kept else { return Ok(()) };
        if (size, modified) != meta {
            db.execute("UPDATE transcripts SET size = ?2, modified = ?3, length = NULL, done = 0, text = '' WHERE path = ?1", params![path, meta.0 as i64, meta.1])?;
            return Ok(());
        }
        let mut all = before;
        if !all.is_empty() && !text.is_empty() {
            all.push('\n');
        }
        crate::extract::push_within(&mut all, text, crate::extract::MAX_TEXT);
        db.execute(
            "UPDATE transcripts SET length = ?2, done = ?3, text = ?4, finished = ?5 WHERE path = ?1",
            params![path, length.map(|l| l as i64), done as i64, all, finished as i64],
        )?;
        let row: Option<i64> = db
            .query_row("SELECT id FROM files WHERE path = ?1 AND size = ?2 AND modified = ?3", params![path, meta.0 as i64, meta.1], |r| r.get(0))
            .ok();
        drop(db);
        if finished && let Some(id) = row {
            let text = crate::extract::tidy(&all);
            self.read(&[(id, (!text.trim().is_empty()).then_some(text))])?;
            self.db.lock().unwrap().execute("UPDATE files SET embedded = NULL WHERE id = ?1", [id])?;
        }
        Ok(())
    }

    /// `path` could not be transcribed: given up, with why, until it is asked for again.
    pub(crate) fn transcript_failed(&self, path: &str, why: &str) -> rusqlite::Result<()> {
        self.db.lock().unwrap().execute("UPDATE transcripts SET finished = 2, text = ?2 WHERE path = ?1", params![path, why])?;
        Ok(())
    }

    /// `path` is gone from the disk.
    pub(crate) fn transcript_gone(&self, path: &str) -> rusqlite::Result<()> {
        self.db.lock().unwrap().execute("DELETE FROM transcripts WHERE path = ?1", [path])?;
        Ok(())
    }

    /// Every file waiting is let go; the transcripts done stay.
    pub fn transcribe_stop(&self) -> rusqlite::Result<()> {
        self.db.lock().unwrap().execute("DELETE FROM transcripts WHERE finished = 0", [])?;
        Ok(())
    }

    /// Files waiting, done and given up, and the seconds of sound left of those waiting whose
    /// length is known.
    pub(crate) fn transcript_counts(&self) -> rusqlite::Result<(usize, usize, usize, u64)> {
        self.db.lock().unwrap().query_row(
            "SELECT count(*) FILTER (WHERE finished = 0), count(*) FILTER (WHERE finished = 1), count(*) FILTER (WHERE finished = 2), coalesce(sum(max(length - done, 0)) FILTER (WHERE finished = 0), 0) FROM transcripts",
            [],
            |r| Ok((r.get::<_, i64>(0)? as usize, r.get::<_, i64>(1)? as usize, r.get::<_, i64>(2)? as usize, r.get::<_, i64>(3)? as u64)),
        )
    }

    /// Seconds of work a second of sound took here, last measured.
    pub(crate) fn transcribe_rate(&self) -> Option<f64> {
        self.db.lock().unwrap().query_row("SELECT value FROM meta WHERE key = 'whisper_rate'", [], |r| r.get::<_, String>(0)).ok()?.parse().ok()
    }

    /// A part took `rate` seconds of work a second of sound: kept, evened with the last.
    pub(crate) fn transcribe_measured(&self, rate: f64) -> rusqlite::Result<()> {
        let rate = self.transcribe_rate().map_or(rate, |r| (r + rate) / 2.0);
        self.db.lock().unwrap().execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('whisper_rate', ?1)", [rate.to_string()])?;
        Ok(())
    }

    /// The transcript of `path`, done or under way, for the preview.
    pub fn transcript(&self, path: &Path) -> Option<crate::transcribe::Transcript> {
        let path = key(path)?;
        let db = self.db.lock().unwrap();
        let (text, finished, done, length, asked): (String, i64, i64, Option<i64>, i64) = db
            .query_row("SELECT text, finished, done, length, asked FROM transcripts WHERE path = ?1", [&path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .ok()?;
        let place = (finished == 0).then(|| {
            db.query_row("SELECT count(*) FROM transcripts WHERE finished = 0 AND done = 0 AND (asked < ?1 OR (asked = ?1 AND path < ?2))", params![asked, path], |r| r.get::<_, i64>(0))
                .unwrap_or(0) as usize
                + db.query_row("SELECT count(*) FROM transcripts WHERE finished = 0 AND done > 0 AND path != ?1", [&path], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
        });
        Some(crate::transcribe::Transcript { text, finished: finished == 1, failed: finished == 2, done: done as u64, length: length.map(|l| l as u64), place: place.map(|p| if done > 0 { 0 } else { p + 1 }) })
    }

    /// The transcript of the file of row `id`, when it was made of the file as it is; a file
    /// that changed since is transcribed again (it was asked for once).
    fn transcript_of(&self, id: i64) -> Option<String> {
        let db = self.db.lock().unwrap();
        let (path, size, modified): (String, i64, i64) = db.query_row("SELECT path, size, modified FROM files WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).ok()?;
        let (tsize, tmod, finished, text): (i64, i64, i64, String) = db
            .query_row("SELECT size, modified, finished, text FROM transcripts WHERE path = ?1", [&path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .ok()?;
        if (tsize, tmod) != (size, modified) {
            let _ = db.execute("UPDATE transcripts SET size = ?2, modified = ?3, length = NULL, done = 0, text = '', finished = 0 WHERE path = ?1", params![path, size, modified]);
            return None;
        }
        (finished == 1 && !text.trim().is_empty()).then_some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backlog_is_files_to_hash() {
        let d = std::env::temp_dir().join(format!("coxswain-backlog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let store = Store::open(&d.join("search.db")).unwrap();
        assert!(!has_backlog(&store).unwrap(), "nothing to do: not paused");
        let files = [d.join("a"), d.join("b")];
        for f in &files {
            store
                .db
                .lock()
                .unwrap()
                .execute(
                    "INSERT INTO files(path, size, modified, has_text) VALUES (?1, 5, 1, 1)",
                    [key(f).unwrap()],
                )
                .unwrap();
        }
        // Two of one size, with text: hashes to make; vectors only with search by meaning on.
        assert!(has_backlog(&store).unwrap());
        let hashed: Vec<_> = files
            .iter()
            .map(|f| (f.clone(), 5, 1, "h".into()))
            .collect();
        store.put_hashes(&hashed).unwrap();
        assert!(!has_backlog(&store).unwrap());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn writes_never_wait_for_signs_a_search_holds() {
        let d = std::env::temp_dir().join(format!("coxswain-signs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let store = Arc::new(Store::open(&d.join("search.db")).unwrap());
        let file = |path: &str| {
            let db = store.db.lock().unwrap();
            db.execute(
                "INSERT INTO files(path, size, modified, has_text) VALUES (?1, 1, 1, 1)",
                [path],
            )
            .unwrap();
            db.last_insert_rowid()
        };
        let (a, b) = (file("/a"), file("/b"));
        let v = crate::meaning::pack(&[0.5f32; 64]);
        store.put_vectors(a, std::slice::from_ref(&v)).unwrap();
        let count = |s: &Signs| s.files.len();
        assert_eq!(store.with_signs(count), 1);
        // A search goes through the signs while the passages of two files are written: one
        // anew, one for the first time. Neither waits for it.
        store.with_signs(|_| {
            let (done, wait) = std::sync::mpsc::channel();
            let writer = store.clone();
            let v = v.clone();
            std::thread::spawn(move || {
                writer.put_vectors(a, std::slice::from_ref(&v)).unwrap();
                writer.put_vectors(b, &[v.clone(), v]).unwrap();
                done.send(()).unwrap();
            });
            wait.recv_timeout(Duration::from_secs(10))
                .expect("a write waited for the signs");
        });
        // The next search has both, each file's passages once.
        assert_eq!(store.with_signs(count), 3);
        // One that wants them all read again (a model changed) has them read again.
        store.with_signs(|_| {
            let writer = store.clone();
            std::thread::spawn(move || writer.model_is("another", None).unwrap())
                .join()
                .unwrap();
        });
        assert_eq!(store.with_signs(count), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn store_finds_text_follows_changes_and_forgets() {
        let d = std::env::temp_dir().join(format!("coxswain-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for sub in [
            "docs",
            "node_modules/pkg",
            ".hidden",
            "private",
            "mail",
            "build-x",
            "not-a-cache",
        ] {
            std::fs::create_dir_all(d.join("home").join(sub)).unwrap();
        }
        let write =
            |name: &str, text: &[u8]| std::fs::write(d.join("home").join(name), text).unwrap();
        write(
            "docs/budget.md",
            "# Rocket budget\n\nFuel is the largest cost of flight seven.\n".as_bytes(),
        );
        write(
            "docs/notes.txt",
            "Book the ferry. Renew the domain.\n".as_bytes(),
        );
        write("docs/photo.bin", b"\x89PNG\x00\x00fuel");
        write("node_modules/pkg/index.js", b"const fuel = 1;\n");
        write(".hidden/secret.txt", b"fuel\n");
        write("private/diary.txt", b"fuel\n");
        write("private/.nosearch", b"");
        write(
            "build-x/CACHEDIR.TAG",
            b"Signature: 8a477f597d28d172789f06886806bc55\n# Cargo's\n",
        );
        write("build-x/notes.md", b"fuel for the cache");
        write("not-a-cache/CACHEDIR.TAG", b"anything else");
        write("not-a-cache/plan.md", b"harbour schedule");
        write("mail/inbox.txt", b"fuel\n");

        let cfg = SearchConfig {
            text_roots: vec![d.join("home")],
            names_only: vec![d.join("home/mail")],
            ..SearchConfig::default()
        };
        let (store, go) = (
            Store::open(&d.join("search.db")).unwrap(),
            AtomicBool::new(false),
        );
        scan(&store, &cfg, &go).unwrap();

        let names = |q: &str| {
            store
                .search(q, None, 10)
                .hits
                .iter()
                .map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names("fuel"),
            ["budget.md"],
            "not the binary, node_modules, the hidden folder, the .nosearch one, the names-only one or the cache-tagged one"
        );
        assert_eq!(
            names("harbour"),
            ["plan.md"],
            "a CACHEDIR.TAG without the signature leaves its folder in"
        );
        assert_eq!(
            names("rocket bud"),
            ["budget.md"],
            "every word, the last one begun"
        );
        // No file has both words: the files with either come instead.
        let mut either = names("ferry fuel");
        either.sort();
        assert_eq!(either, ["budget.md", "notes.txt"]);
        assert_eq!(
            names("ferry and fuel"),
            names("ferry fuel"),
            "short words are left out of either"
        );
        // A scope: the files below it alone.
        let docs = d.join("home/docs");
        assert_eq!(store.search("fuel", Some(&docs), 10).hits.len(), 1);
        assert_eq!(
            store.search("fuel", Some(&d.join("home/doc")), 10).total,
            0,
            "a folder whose name starts the same is not below"
        );
        assert_eq!(
            store
                .search("fuel", Some(&d.join("home/private")), 10)
                .total,
            0
        );
        assert_eq!(
            names("\"; DROP TABLE files"),
            [""; 0],
            "a query is words, never SQL"
        );
        let hit = &store.search("largest", None, 10).hits[0];
        assert_eq!(
            hit.snippet.as_deref(),
            Some(
                format!(
                    "# Rocket budget Fuel is the {}largest{} cost of flight seven.",
                    MARK.0, MARK.1
                )
                .as_str()
            )
        );
        assert_eq!(
            store.texts(),
            4,
            "budget.md, notes.txt and both files of not-a-cache"
        );

        // Sizes: the files' rows and the totals of the folders left out, as a walk counts them.
        let home = d.join("home");
        assert_eq!(
            store.size(&home).map(|s| s.0),
            Some(crate::fs::dir_size(&home))
        );
        assert_eq!(
            store.size(&home.join("node_modules")).map(|s| s.0),
            Some((16, 1)),
            "a folder left out has its total"
        );
        assert_eq!(
            store.size(&home.join("node_modules/pkg")),
            None,
            "inside one, nothing is known"
        );
        assert_eq!(store.size(&d), None, "outside the roots neither");

        // Hashes: files sharing a size get one; a file that goes loses it.
        let hash = |name: &str| {
            let meta = std::fs::metadata(home.join(name)).unwrap();
            store.hash(&home.join(name), meta.len(), secs(&meta) as u64)
        };
        assert!(hash("docs/notes.txt").is_none(), "its size is its own");
        write(
            "docs/copy.txt",
            "Book the ferry. Renew the domain.\n".as_bytes(),
        );
        aged(&home.join("docs/notes.txt"));
        aged(&home.join("docs/copy.txt"));
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(
            hash("docs/notes.txt").unwrap(),
            hash("docs/copy.txt").unwrap()
        );
        std::fs::remove_file(home.join("docs/copy.txt")).unwrap();

        // A file changes, one goes, one comes.
        std::thread::sleep(Duration::from_millis(1100));
        write("docs/notes.txt", "Book the train instead.\n".as_bytes());
        write("docs/new.rs", b"fn launch() {}\n");
        std::fs::remove_file(d.join("home/docs/budget.md")).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(
            (
                names("ferry"),
                names("train"),
                names("launch"),
                names("fuel")
            ),
            (
                vec![],
                vec!["notes.txt".to_string()],
                vec!["new.rs".to_string()],
                vec![]
            )
        );
        assert_eq!(store.pending.load(Ordering::Relaxed), 0);
        assert_eq!(
            store
                .db
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM hashes", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0,
            "the copy's hash went with it"
        );

        // Deleting the index forgets everything, sizes included, until the next scan.
        store.clear().unwrap();
        assert_eq!((store.texts(), store.size(&home)), (0, None));

        // The store of a newer version is left as it is: refused, not thrown away.
        drop(store);
        let old = Connection::open(d.join("search.db")).unwrap();
        old.execute_batch(
            "INSERT INTO errors VALUES (0, NULL, 'old'); INSERT INTO chunks(file, n, vector) VALUES (1, 0, x'00');",
        )
        .unwrap();
        old.pragma_update(None, "user_version", VERSION + 1)
            .unwrap();
        drop(old);
        let err = Store::open(&d.join("search.db")).err().expect("a newer store is refused");
        assert!(err.to_string().contains("newer Coxswain"), "{err}");
        let newer = Connection::open(d.join("search.db")).unwrap();
        assert_eq!(
            (
                newer.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0)).unwrap(),
                newer.query_row("SELECT count(*) FROM errors", [], |r| r.get::<_, i64>(0)).unwrap(),
                newer.query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0)).unwrap(),
            ),
            (VERSION + 1, 1, 1),
            "the newer store keeps its version and its rows"
        );

        // The store of an older version is filled afresh.
        newer.pragma_update(None, "user_version", VERSION - 1).unwrap();
        drop(newer);
        let store = Store::open(&d.join("search.db")).unwrap();
        assert_eq!(store.texts(), 0);
        let rows = |t: &str| {
            store
                .db
                .lock()
                .unwrap()
                .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap()
        };
        assert_eq!(
            (rows("errors"), rows("chunks")),
            (0, 0),
            "nothing of the old store stays"
        );
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Dated a minute back, as a file not just written.
    fn aged(p: &Path) {
        let f = std::fs::File::options().write(true).open(p).unwrap();
        f.set_modified(std::time::SystemTime::now() - Duration::from_secs(60))
            .unwrap();
    }

    #[test]
    fn store_leaves_files_only_in_the_cloud_there() {
        let d = std::env::temp_dir().join(format!("coxswain-store-cloud-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let home = d.join("home");
        std::fs::create_dir_all(home.join("OneDrive")).unwrap();
        let (plan, copy) = (
            home.join("OneDrive/plan.txt"),
            home.join("OneDrive/copy.txt"),
        );
        std::fs::write(&plan, "Orbit at dawn, land by noon.\n").unwrap();
        std::fs::write(&copy, "Orbit at dawn, land by noon.\n").unwrap();
        // Hashes are kept only for files whose date is a few seconds old (`dupes::settled`).
        aged(&plan);
        aged(&copy);
        let cfg = SearchConfig {
            text_roots: vec![home.clone()],
            ..SearchConfig::default()
        };
        let (store, go) = (
            Store::open(&d.join("search.db")).unwrap(),
            AtomicBool::new(false),
        );
        let found = |q: &str| store.search(q, None, 10).total;
        let hashes = || {
            store
                .db
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM hashes", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        let id = || {
            store
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT id FROM files WHERE path = ?1",
                    [key(&plan).unwrap()],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
        };
        let vectors = || {
            store
                .db
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };

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
        assert_eq!(
            store.size(&home).map(|s| s.0),
            Some(crate::fs::dir_size(&home))
        );
        assert_eq!(
            store.clouds(),
            [("OneDrive".to_string(), home.join("OneDrive"))]
        );
        assert!(
            crate::extract::text_of(&plan, 30, 1 << 20).is_none(),
            "never read"
        );
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
    /// A Mercurial repository's commits are found too, each opening as its history folder.
    fn store_keeps_the_history_of_mercurial() {
        if !crate::user_menu::installed("hg") {
            return;
        }
        let home = std::env::temp_dir().join(format!("coxswain-store-hg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let hg = |args: &[&str]| {
            let ok = std::process::Command::new("hg")
                .current_dir(&home)
                .args(args)
                .env("HGRCPATH", "")
                .env("HGUSER", "Ada <ada@example.org>")
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(ok.success(), "hg {args:?}");
        };
        hg(&["init"]);
        std::fs::write(home.join("engine.rs"), "fn main() {}\n").unwrap();
        hg(&["add"]);
        hg(&["commit", "-m", "Tune the rocket engine", "-d", "1700000000 0"]);
        let cfg = SearchConfig {
            text_roots: vec![home.clone()],
            ..SearchConfig::default()
        };
        let db = home.with_extension("db");
        let (store, go) = (Store::open(&db).unwrap(), AtomicBool::new(false));
        scan(&store, &cfg, &go).unwrap();
        let found = store.search("rocket", None, 10).hits;
        assert_eq!(found.len(), 1);
        let at = crate::history::split(&found[0].path).expect("a commit's history folder");
        assert_eq!(at.target, home);
        assert!(crate::history::show(&home, at.commit.as_deref().unwrap()).is_ok());
        assert_eq!(store.search("engine.rs", None, 10).hits.len(), 1, "by a path it changed");
        drop(store);
        let _ = std::fs::remove_file(&db);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn store_keeps_the_history_of_repositories() {
        use crate::history::tests::{add, commit_as, repo};
        let Some(home) = repo("store") else { return };
        std::fs::write(home.join("engine.rs"), "fn main() {}\n").unwrap();
        add(&home);
        commit_as(&home, "Ada", 1_700_000_000, "Tune the rocket engine");
        let cfg = SearchConfig {
            text_roots: vec![home.clone()],
            ..SearchConfig::default()
        };
        let db = home.with_extension("db");
        let (store, go) = (Store::open(&db).unwrap(), AtomicBool::new(false));
        scan(&store, &cfg, &go).unwrap();
        let hits = |q: &str| store.search(q, None, 10).hits;
        let found = hits("rocket");
        assert_eq!(found.len(), 1);
        let at = crate::history::split(&found[0].path).expect("a commit's history folder");
        assert_eq!(
            (at.target.as_path(), at.commit.as_ref().map(String::len)),
            (home.as_path(), Some(12))
        );
        assert!(
            found[0].snippet.as_deref().unwrap().starts_with("commit "),
            "{:?}",
            found[0].snippet
        );
        assert!(found[0].snippet.as_deref().unwrap().contains("· Ada ·"));
        assert_eq!(hits("Ada").len(), 1, "by its author");
        assert_eq!(hits("engine.rs").len(), 1, "by a path it changed");
        assert_eq!(
            store.size(&home).map(|s| s.0),
            Some(crate::fs::dir_size(&home)),
            "commits are no files in sizes"
        );

        // HEAD moves on: only the new commit is added, the old one stays.
        std::fs::write(home.join("engine.rs"), "fn main() { valve(); }\n").unwrap();
        commit_as(&home, "Bob", 1_700_100_000, "Fix the fuel valve");
        scan(&store, &cfg, &go).unwrap();
        assert_eq!((hits("fuel").len(), hits("rocket").len()), (1, 1));
        let rows = || {
            store
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM files WHERE path LIKE 'git:%'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
        };
        assert_eq!(rows(), 2);
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(rows(), 2, "nothing new: nothing read again");

        // Turned off, its commits go.
        scan(
            &store,
            &SearchConfig {
                history: false,
                ..cfg.clone()
            },
            &go,
        )
        .unwrap();
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
        let cfg = SearchConfig {
            text_roots: vec![home.clone()],
            ..SearchConfig::default()
        };
        let (store, go) = (
            Store::open(&d.join("search.db")).unwrap(),
            AtomicBool::new(false),
        );
        scan(&store, &cfg, &go).unwrap();
        let names = |q: &str| {
            store
                .search(q, None, 10)
                .hits
                .iter()
                .map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        let size = |p: &Path| store.size(p).map(|s| s.0);
        let mut later = HashSet::new();
        let saw = |paths: &[PathBuf], later: &mut HashSet<PathBuf>| {
            refresh(
                &store,
                &cfg,
                &mut paths.iter().cloned().collect(),
                later,
                &go,
            )
            .unwrap()
        };

        // A file comes, one goes, a folder is moved in.
        write("docs/budget.txt", b"fuel for march\n");
        std::fs::remove_file(home.join("docs/plan.txt")).unwrap();
        std::fs::create_dir_all(d.join("elsewhere/deep")).unwrap();
        std::fs::write(d.join("elsewhere/deep/orbit.txt"), b"orbit\n").unwrap();
        std::fs::rename(d.join("elsewhere"), home.join("moved")).unwrap();
        saw(
            &[
                home.join("docs/budget.txt"),
                home.join("docs/plan.txt"),
                home.join("moved"),
            ],
            &mut later,
        );
        assert_eq!(
            (names("march"), names("orbit")),
            (
                vec!["budget.txt".to_string()],
                vec!["orbit.txt".to_string()]
            )
        );
        assert_eq!(size(&home), Some(crate::fs::dir_size(&home)));
        assert!(later.is_empty());

        // Inside a folder left out, and a folder that becomes one: measured later.
        write("node_modules/b.js", b"yy");
        write("moved/.nosearch", b"");
        saw(
            &[home.join("node_modules/b.js"), home.join("moved/.nosearch")],
            &mut later,
        );
        assert_eq!(
            later,
            [home.join("node_modules"), home.join("moved")]
                .into_iter()
                .collect()
        );
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
        for (path, cached) in [
            ("/h/p/a.zip", false),
            ("/h/p/node_modules/x/a.zip", true),
            ("/h/.cargo/registry/a.tar.gz", true),
            ("/h/target.zip", false),
            ("/w/target/keep/a.zip", false),
            ("/elsewhere/target/a.zip", false),
        ] {
            assert_eq!(in_cache(Path::new(path), &roots), cached, "{path}");
        }
        let cache = crate::dirs::cache_dir().unwrap();
        assert!(
            in_cache(&cache.join("a.zip"), std::slice::from_ref(&cache)),
            "a folder read that is one"
        );
    }

    #[test]
    fn store_reads_inside_archives_and_follows_their_changes() {
        let d = std::env::temp_dir().join(format!("coxswain-store-arc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (home, src) = (d.join("home"), d.join("src"));
        std::fs::create_dir_all(&home).unwrap();
        for (f, text) in [
            ("docs/plan.txt", "launch window in march"),
            ("docs/notes.md", "# Fuel\n\nTanks refilled twice."),
            ("budget.txt", "the orbit budget"),
            ("big.txt", "colossal words that go on and on and on"),
        ] {
            std::fs::create_dir_all(src.join(f).parent().unwrap()).unwrap();
            std::fs::write(src.join(f), text).unwrap();
        }
        let (zip, tgz) = (home.join("website.zip"), home.join("site.tar.gz"));
        crate::archive::create(&zip, &[src.join("docs")]).unwrap();
        crate::archive::create(&tgz, &[src.join("big.txt")]).unwrap();
        // A zip whose file is locked: its name is seen, its text is never read.
        let locked = home.join("locked.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&locked).unwrap());
        w.start_file(
            "secret.txt",
            zip::write::SimpleFileOptions::default()
                .with_aes_encryption(zip::AesMode::Aes256, "hunter2"),
        )
        .unwrap();
        std::io::Write::write_all(&mut w, b"the secret rendezvous").unwrap();
        w.finish().unwrap();

        let mut cfg = SearchConfig {
            text_roots: vec![home.clone()],
            text_max_size: 30,
            ..SearchConfig::default()
        };
        let (store, go) = (
            Store::open(&d.join("search.db")).unwrap(),
            AtomicBool::new(false),
        );
        let paths = |q: &str| {
            store
                .search(q, None, 10)
                .hits
                .into_iter()
                .map(|h| h.path)
                .collect::<Vec<_>>()
        };
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(
            paths("colossal"),
            Vec::<PathBuf>::new(),
            "larger than text_max_size"
        );
        assert_eq!(
            store
                .db
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM files WHERE inside", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            4,
            "but known"
        );
        assert_eq!(paths("launch"), [zip.join("docs").join("plan.txt")]);
        let hit = &store.search("refilled", None, 10).hits[0];
        assert_eq!(
            (hit.path.clone(), hit.snippet.as_deref()),
            (
                zip.join("docs").join("notes.md"),
                Some(format!("# Fuel Tanks {}refilled{} twice.", MARK.0, MARK.1).as_str())
            )
        );
        assert_eq!(paths("rendezvous"), Vec::<PathBuf>::new(), "locked");
        assert_eq!(
            store.size(&home).map(|s| s.0),
            Some(crate::fs::dir_size(&home)),
            "files inside are not counted twice"
        );
        assert_eq!(store.pending.load(Ordering::Relaxed), 0);
        assert!(
            store
                .unhashed()
                .unwrap()
                .iter()
                .all(|(p, ..)| crate::archive::split(p).is_none())
        );

        // Changed by Coxswain, seen by the watcher: a file in, one out. Just written, it may
        // still be growing (a download): it waits for the next round, and nothing is read.
        std::thread::sleep(Duration::from_millis(1100));
        crate::archive::add(
            &zip,
            &[("docs/budget.txt".into(), src.join("budget.txt"))],
            None,
        )
        .unwrap();
        crate::archive::remove(&zip, &["docs/plan.txt".into()], None).unwrap();
        let mut seen: HashSet<PathBuf> = [zip.clone()].into_iter().collect();
        refresh(&store, &cfg, &mut seen, &mut HashSet::new(), &go).unwrap();
        assert!(
            seen.contains(&zip) && paths("launch") == [zip.join("docs").join("plan.txt")],
            "still settling"
        );
        std::thread::sleep(SETTLE);
        let id_of = |name: &str| {
            store
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT id FROM files WHERE path = ?1",
                    [zip.join("docs").join(name).to_str().unwrap()],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
        };
        let notes = id_of("notes.md");
        refresh(&store, &cfg, &mut seen, &mut HashSet::new(), &go).unwrap();
        assert!(seen.is_empty());
        assert_eq!(
            (paths("orbit"), paths("launch")),
            (vec![zip.join("docs").join("budget.txt")], vec![])
        );
        assert_eq!(
            id_of("notes.md"),
            notes,
            "a member that did not change keeps its row and text"
        );
        // Renamed inside, then written anew by something else, seen by the next scan.
        std::thread::sleep(Duration::from_millis(1100));
        crate::archive::rename_in(&zip, "docs/budget.txt", "docs/costs.txt", None).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(paths("orbit"), [zip.join("docs").join("costs.txt")]);
        std::fs::remove_file(&zip).unwrap();
        crate::archive::create(&zip, &[src.join("docs/plan.txt")]).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(
            (paths("orbit"), paths("launch")),
            (vec![], vec![zip.join("plan.txt")])
        );
        // An archive that goes takes its files with it.
        std::fs::remove_file(&tgz).unwrap();
        refresh(
            &store,
            &cfg,
            &mut [tgz.clone()].into_iter().collect(),
            &mut HashSet::new(),
            &go,
        )
        .unwrap();
        assert_eq!(
            store
                .db
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM files WHERE inside", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );

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
        let cfg = SearchConfig {
            text_roots: vec![d.join("disk")],
            ..SearchConfig::default()
        };
        let (store, go) = (
            Store::open(&d.join("search.db")).unwrap(),
            AtomicBool::new(false),
        );
        let found = || store.search("orbit", None, 10).hits.len();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(found(), 1);

        // Unplugged: the rows stay, out of sight, and Settings shows the disk as away.
        std::fs::rename(d.join("disk"), d.join("away")).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!((found(), store.texts()), (0, 1));
        let roots = store.root_sizes();
        assert_eq!((roots[0].1.clone(), roots[0].2.1), (None, 1));
        // A reader that learnt more asks for its files again while the disk is away: they keep
        // their text until it is back, and are read then.
        store.db.lock().unwrap().execute("UPDATE files SET has_text = NULL", []).unwrap();
        assert!(!read(&store, &cfg, &go).unwrap());
        let kept: i64 = store.db.lock().unwrap().query_row("SELECT count(*) FROM text", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, 1);
        let unread: Option<bool> = store.db.lock().unwrap().query_row("SELECT has_text FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(unread, None);

        // Plugged in again: found again, nothing read twice.
        std::fs::rename(d.join("away"), d.join("disk")).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(found(), 1);
        assert_eq!(
            store.root_sizes()[0].1.as_deref(),
            Some(d.join("disk").as_path())
        );
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A Find hit in a PGN file of many games opens the game it is about: through the store's own
    /// text and FTS5 snippet, whose window takes in the games around it.
    #[test]
    fn a_hit_in_a_pgn_file_names_its_game() {
        let d = std::env::temp_dir().join(format!("coxswain-store-pgn-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let players = [
            ("Morphy, Paul", "Duke Karl", "Paris", "1. e4 e5 2. Nf3 d6"),
            ("Anderssen, Adolf", "Kieseritzky, Lionel", "London", "1. e4 e5 2. f4 exf4"),
            ("Anderssen, Adolf", "Dufresne, Jean", "Berlin", "1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. b4"),
            ("Steinitz, Wilhelm", "Zukertort, Johannes", "New York", "1. d4 d5 2. c4"),
            ("Lasker, Emanuel", "Steinitz, Wilhelm", "Moscow", "1. e4 c5"),
            ("Capablanca, Jose", "Lasker, Emanuel", "Havana", "1. d4 Nf6 2. c4 e6"),
            ("Morphy, Paul", "Anderssen, Adolf", "Paris", "1. a3 e5"),
        ];
        let mut pgn = String::new();
        for (w, b, e, m) in players {
            pgn.push_str(&format!("[Event \"{e}\"]\n[White \"{w}\"]\n[Black \"{b}\"]\n[Result \"1-0\"]\n\n{m} 1-0\n\n"));
        }
        let file = d.join("games.pgn");
        std::fs::write(&file, &pgn).unwrap();
        let store = Store::open(&d.join("search.db")).unwrap();
        let path = file.to_string_lossy().into_owned();
        // An empty PGN file beside it: nothing read, so no positions either.
        let empty = d.join("empty.pgn");
        std::fs::write(&empty, "").unwrap();
        store
            .apply(&[], &[(path.clone(), pgn.len() as u64, 1, false), (empty.to_string_lossy().into_owned(), 0, 1, false)], &[], false, None)
            .unwrap();
        // The store's own reading: the text, then the positions of what it read.
        assert!(!read(&store, &SearchConfig::default(), &AtomicBool::new(false)).unwrap());
        for (query, game) in [("Kieseritzky", 1), ("Dufresne", 2), ("Zukertort", 3), ("Havana", 5), ("Philidor", 0), ("Evans", 2), ("Sicilian", 4)] {
            let hits = store.search(query, None, 10).hits;
            let snippet = hits[0].snippet.as_deref().unwrap_or_default();
            assert_eq!(crate::chess::games::game_of_hit(&file, snippet), Some(game), "{query}: {snippet}");
        }
        // Find this position: after 1.e4 e5 2.Nf3, games 0 and 2; after 1.e4 e5, 0, 1 and 2.
        let key = |fen: &str| crate::chess::polyglot::key(&crate::chess::pgn::board(fen).unwrap());
        let nf3 = key("rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2");
        assert_eq!(store.games_at(nf3, 10).unwrap(), vec![(file.clone(), 0), (file.clone(), 2)]);
        let e5 = key("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2");
        assert_eq!(store.games_at(e5, 2).unwrap().len(), 2);
        // Stopped at once: the file's rows go, none come, and the file is to be read again.
        let id: i64 = store.db.lock().unwrap().query_row("SELECT id FROM files WHERE path = ?1", [&path], |r| r.get(0)).unwrap();
        store.put_positions(id, &file, &|| true).unwrap();
        assert!(store.games_at(e5, 10).unwrap().is_empty());
        let unread: Option<bool> = store.db.lock().unwrap().query_row("SELECT has_text FROM files WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(unread, None);
        store.put_positions(id, &file, &|| false).unwrap();
        assert_eq!(store.games_at(e5, 10).unwrap().len(), 3);
        // Left out by `text_exclude`: Find this position no longer names it.
        store.exclude_changed(&["*.pgn".into()]).unwrap();
        assert!(store.games_at(e5, 10).unwrap().is_empty());
        store.put_positions(id, &file, &|| false).unwrap();
        // The file gone, its positions go with it.
        store.apply(&[path.clone()], &[], &[], false, None).unwrap();
        assert!(store.games_at(nf3, 10).unwrap().is_empty());
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn store_keeps_a_file_that_took_too_long_until_it_changes() {
        let d = std::env::temp_dir().join(format!("coxswain-store-slow-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let store = Store::open(&d.join("search.db")).unwrap();
        store.apply(&[], &[("/h/old.doc".into(), 10, 1, false)], &[], false, None).unwrap();
        let id = store.unread_disk(0, 10).unwrap()[0].0;
        assert!(!store.too_long(id));
        store.mark_too_long(id).unwrap();
        assert!(store.too_long(id), "not read again while it is as it was");
        store.apply(&[], &[("/h/old.doc".into(), 12, 2, false)], &[], false, None).unwrap();
        assert!(!store.too_long(id), "read again once it changed");
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn store_reads_again_what_a_new_program_can_read() {
        const KNOWN: &[&str] = &["png", "jpg", "ps"];
        let d = std::env::temp_dir().join(format!("coxswain-store-tools-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let store = Store::open(&d.join("search.db")).unwrap();
        store
            .apply(
                &[],
                &[
                    ("/h/scan.PNG".into(), 1, 1, false),
                    ("/h/a.txt".into(), 1, 1, false),
                ],
                &[],
                false,
                None,
            )
            .unwrap();
        store
            .read(
                &store
                    .unread_disk(0, 1000)
                    .unwrap()
                    .iter()
                    .map(|(id, ..)| (*id, None))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        assert!(store.unread_disk(0, 1000).unwrap().is_empty());
        store.tools_changed(&[], KNOWN).unwrap();
        assert!(
            store.unread_disk(0, 1000).unwrap().is_empty(),
            "nothing new"
        );
        store.tools_changed(&["png", "jpg"], KNOWN).unwrap();
        assert_eq!(
            store
                .unread_disk(0, 1000)
                .unwrap()
                .iter()
                .map(|r| r.1.as_str())
                .collect::<Vec<_>>(),
            ["/h/scan.PNG"],
            "tesseract came"
        );
        store.tools_changed(&["png", "jpg"], KNOWN).unwrap();
        // A reader that learnt more: its files are read again over the text they had. Before
        // 1.26.4 the old text made every scan fail there, and no file got its vectors after.
        let id = store.unread_disk(0, 1000).unwrap()[0].0;
        store.read(&[(id, Some("old words".into()))]).unwrap();
        store.readers_changed("test-2", &[".png"]).unwrap();
        assert_eq!(store.unread_disk(0, 1000).unwrap().len(), 1);
        store.read(&[(id, Some("new words".into()))]).unwrap();
        assert_eq!(store.search("new", None, 10).total, 1);
        assert_eq!(store.search("old", None, 10).total, 0);
        // A kind read as plain text before (PostScript) that Ghostscript now reads: read again,
        // though it had text; the other kinds stay.
        store.apply(&[], &[("/h/notes.ps".into(), 1, 1, false)], &[], false, None).unwrap();
        let ps = store.unread_disk(0, 1000).unwrap()[0].0;
        store.read(&[(ps, Some("%!PS moveto show".into()))]).unwrap();
        store.tools_changed(&["png", "jpg", "ps"], KNOWN).unwrap();
        assert_eq!(
            store.unread_disk(0, 1000).unwrap().iter().map(|r| r.1.as_str()).collect::<Vec<_>>(),
            ["/h/notes.ps"],
            "Ghostscript came"
        );
        store.read(&[(ps, Some("launch notes".into()))]).unwrap();
        // Ghostscript gone from PATH for a while: what it read stays.
        store.tools_changed(&["png", "jpg"], KNOWN).unwrap();
        assert!(store.unread_disk(0, 1000).unwrap().is_empty(), "a program that went keeps what it read");
        assert_eq!(store.search("launch", None, 10).total, 1);
        // Ghostscript back (a container runtime that started late, alike): what it read stays,
        // nothing is read or given vectors again. Then an app from before 2.32.2 (that knew no
        // PostScript reader) writes its shorter list: coming back to this one reads nothing again.
        store.tools_changed(&["png", "jpg", "ps"], KNOWN).unwrap();
        assert!(store.unread_disk(0, 1000).unwrap().is_empty(), "a program that came back reads nothing again");
        store.read(&[(ps, Some("launch notes".into()))]).unwrap();
        store.db.lock().unwrap().execute("UPDATE meta SET value = 'png,jpg' WHERE key = 'tools'", []).unwrap();
        store.tools_changed(&["png", "jpg", "ps"], KNOWN).unwrap();
        assert!(store.unread_disk(0, 1000).unwrap().is_empty(), "an older app's list costs nothing");
        // A newer app knew a kind this one does not: its list leaves this one's work alone,
        // and the way back reads nothing again either.
        store.tools_changed(&["png", "jpg", "ps", "xyz"], &["png", "jpg", "ps", "xyz"]).unwrap();
        store.tools_changed(&["png", "jpg", "ps"], KNOWN).unwrap();
        store.tools_changed(&["png", "jpg", "ps", "xyz"], &["png", "jpg", "ps", "xyz"]).unwrap();
        assert!(store.unread_disk(0, 1000).unwrap().is_empty(), "back and forth reads nothing again");
        // The PostScript reader's own version: its files once again, the others not.
        store.reader_changed("readers.postscript", "1", &[".ps", ".eps"]).unwrap();
        assert_eq!(store.unread_disk(0, 1000).unwrap().iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["/h/notes.ps"]);
        store.read(&[(ps, Some("launch notes".into()))]).unwrap();
        store.reader_changed("readers.postscript", "1", &[".ps", ".eps"]).unwrap();
        assert!(store.unread_disk(0, 1000).unwrap().is_empty(), "once");

        // A reader under its own key: its files once, then nothing; the `readers` tag an
        // older app reads stays as it was, so it reads nothing again either.
        store.reader_changed("readers.code", "2", &[".png"]).unwrap();
        assert_eq!(store.unread_disk(0, 1000).unwrap().len(), 1);
        store.read(&[(id, Some("new words".into()))]).unwrap();
        store.reader_changed("readers.code", "2", &[".png"]).unwrap();
        assert!(store.unread_disk(0, 1000).unwrap().is_empty(), "once");
        store.readers_changed("test-2", &[".png"]).unwrap();
        assert!(store.unread_disk(0, 1000).unwrap().is_empty(), "the old tag untouched");
        // A newer app wrote a higher version: this one reads nothing again and keeps its tag.
        store.reader_changed("readers.code", "3", &[".png"]).unwrap();
        store.read(&[(id, Some("new words".into()))]).unwrap();
        store.reader_changed("readers.code", "2", &[".png"]).unwrap();
        assert!(store.unread_disk(0, 1000).unwrap().is_empty(), "an older reader leaves a newer one's work");
        let tag: String = store.db.lock().unwrap()
            .query_row("SELECT value FROM meta WHERE key = 'readers.code'", [], |r| r.get(0)).unwrap();
        assert_eq!(tag, "3");
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A reader that now leaves out hidden text: its files lose their old text and the
    /// vectors of their passages at once, then are read and their passages made again, once,
    /// even when the new text is the same. Other files keep theirs, and a newer version's work
    /// stays.
    #[test]
    fn store_forgets_what_a_reader_no_longer_reads_and_makes_it_again() {
        let d = std::env::temp_dir().join(format!("coxswain-store-dropped-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let store = Store::open(&d.join("search.db")).unwrap();
        let paths = ["/h/report.PDF", "/h/letter.docx", "/h/notes.odt", "/h/plain.txt"];
        let files: Vec<_> = paths.iter().map(|p| (p.to_string(), 1, 1, false)).collect();
        store.apply(&[], &files, &[], false, None).unwrap();
        let ids: Vec<i64> = store.unread_disk(0, 1000).unwrap().iter().map(|r| r.0).collect();
        let said = "Seen words. Secret instructions";
        store.read(&ids.iter().map(|id| (*id, Some(said.to_string()))).collect::<Vec<_>>()).unwrap();
        let v = crate::meaning::pack(&[0.5f32; 64]);
        for id in &ids {
            store.put_vectors(*id, std::slice::from_ref(&v)).unwrap();
        }
        let passages = |s: &Signs| s.files.len();
        assert_eq!(store.with_signs(passages), 4);
        let chunks = |store: &Store| -> i64 {
            store.db.lock().unwrap().query_row("SELECT count(*) FROM chunks", [], |r| r.get(0)).unwrap()
        };
        store.reader_dropped("readers.pdf", "1", &[".pdf"]).unwrap();
        store.reader_dropped("readers.word", "1", &[".docx", ".docm", ".dotx", ".odt", ".ott"]).unwrap();
        // Nothing of the old text is found, by words or by meaning, but in the other file.
        assert_eq!(store.search("secret", None, 10).total, 1);
        assert_eq!(chunks(&store), 1);
        assert_eq!(store.with_signs(passages), 1, "the signs of the passages go too");
        let mut unread: Vec<String> = store.unread_disk(0, 1000).unwrap().into_iter().map(|r| r.1).collect();
        unread.sort();
        assert_eq!(unread, ["/h/letter.docx", "/h/notes.odt", "/h/report.PDF"]);
        // Read again with the same text: still their passages are made again.
        let again: Vec<_> = ids[..3].iter().map(|id| (*id, Some("Seen words.".to_string()))).collect();
        store.read(&again).unwrap();
        assert_eq!(store.search("secret", None, 10).total, 1);
        let mut waiting: Vec<i64> = store.unembedded(10).unwrap().into_iter().map(|r| r.0).collect();
        waiting.sort();
        assert_eq!(waiting, ids[..3]);
        // Once: the same version again, or an older one after a newer, drops nothing.
        for id in &ids[..3] {
            store.put_vectors(*id, std::slice::from_ref(&v)).unwrap();
        }
        store.reader_dropped("readers.pdf", "1", &[".pdf"]).unwrap();
        store.db.lock().unwrap().execute("UPDATE meta SET value = '2' WHERE key = 'readers.word'", []).unwrap();
        store.reader_dropped("readers.word", "1", &[".docx", ".docm", ".dotx", ".odt", ".ott"]).unwrap();
        assert!(store.unread_disk(0, 1000).unwrap().is_empty());
        assert_eq!(chunks(&store), 4);
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
        for (name, text) in [
            ("app.log", "fuel log"),
            ("notes.txt", "fuel notes"),
            ("drafts/a.txt", "fuel draft"),
            ("build", "fuel script"),
        ] {
            std::fs::write(d.join("home").join(name), text).unwrap();
        }
        let mut cfg = SearchConfig {
            text_roots: vec![d.join("home")],
            text_exclude: vec!["*.log".into(), "drafts".into(), "build".into()],
            ..SearchConfig::default()
        };
        let (store, go) = (
            Store::open(&d.join("search.db")).unwrap(),
            AtomicBool::new(false),
        );
        let found = |store: &Store| {
            let mut v: Vec<String> = store
                .search("fuel", None, 10)
                .hits
                .iter()
                .map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        };
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(
            found(&store),
            ["build", "notes.txt"],
            "a plain name leaves out folders only, a pattern files"
        );
        assert_eq!(
            store.size(&d.join("home")).map(|s| s.0.1),
            Some(4),
            "left out files still count in sizes"
        );
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
        assert!(
            !left_out(own.parent().unwrap(), &cfg)
                || own
                    .parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with('.')
        );
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
        let write =
            |name: &str, text: &str| std::fs::write(d.join("home").join(name), text).unwrap();
        write(
            "budget.txt",
            "The fuel budget for flight seven is the largest cost of the launch, and the tanks are refilled twice before lift-off.",
        );
        write(
            "budget-da.txt",
            "Brændstofbudgettet for flyvning syv er den største udgift ved opsendelsen, og tankene fyldes to gange før afgang.",
        );
        write(
            "cake.txt",
            "Opskrift på æblekage: smør, sukker, mel, æbler og kanel. Bag kagen i en time ved 180 grader og server med flødeskum.",
        );
        let cfg = SearchConfig {
            text_roots: vec![d.join("home")],
            ..SearchConfig::default()
        };
        let (store, go) = (
            Store::open(&d.join("search.db")).unwrap(),
            AtomicBool::new(false),
        );
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        store.hurry.store(true, Ordering::Relaxed);
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (0, 3));
        let names = |q: &str| {
            store
                .similar(q, None, 10)
                .iter()
                .map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        let found = names("how much does it cost to fuel the rocket");
        assert!(
            found.contains(&"budget.txt".to_string())
                && found.contains(&"budget-da.txt".to_string()),
            "{found:?}"
        );
        assert!(!found.contains(&"cake.txt".to_string()), "{found:?}");
        assert_eq!(
            names("apple cake recipe").first().map(String::as_str),
            Some("cake.txt")
        );
        assert!(
            store
                .similar("apple cake recipe", Some(&d.join("elsewhere")), 10)
                .is_empty(),
            "a scope with none of the files"
        );
        assert!(
            store
                .passages("apple cake recipe", Some(&d.join("elsewhere")), 10_000)
                .is_empty()
        );
        // What Ask answers from: whole passages, closest first.
        let passages = store.passages("what does the fuel cost", None, 10_000);
        assert!(
            passages
                .first()
                .is_some_and(|(p, text, _)| p.ends_with("budget.txt")
                    || p.ends_with("budget-da.txt") && text.contains("syv")),
            "{passages:?}"
        );

        // Changed: its vectors go with its old text, and come again for the new one.
        std::thread::sleep(Duration::from_millis(1100));
        write(
            "cake.txt",
            "Minutes of the board meeting: the budget was approved and the fuel supplier was changed.",
        );
        scan(&store, &cfg, &go).unwrap();
        // Its name still says cake, which the model is shown too: the passage is the new one.
        let hits = store.similar("apple cake recipe", None, 10);
        assert!(
            hits.iter()
                .filter(|h| h.path.ends_with("cake.txt"))
                .all(|h| h
                    .snippet
                    .as_ref()
                    .is_some_and(|s| s.contains("board meeting"))),
            "{hits:?}"
        );
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
                    let Some(h) = text.find("\r\n\r\n") else {
                        continue;
                    };
                    let len: usize = text[..h]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")?
                                .trim()
                                .parse()
                                .ok()
                        })
                        .unwrap_or(0);
                    if n == 0 || got.len() >= h + 4 + len {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&got);
                let v: serde_json::Value =
                    serde_json::from_str(text.split_once("\r\n\r\n").map_or("", |x| x.1))
                        .unwrap_or_default();
                let vectors: Vec<Vec<f32>> = v["input"].as_array().map_or(vec![], |a| {
                    a.iter()
                        .map(|t| {
                            let mut v = vec![0.0f32; 26];
                            for b in t
                                .as_str()
                                .unwrap_or("")
                                .bytes()
                                .filter(u8::is_ascii_alphabetic)
                            {
                                v[(b.to_ascii_lowercase() - b'a') as usize] += 1.0;
                            }
                            v
                        })
                        .collect()
                });
                let openai = text
                    .lines()
                    .next()
                    .is_some_and(|l| l.contains("/embeddings "));
                let reply = if openai { serde_json::json!({ "data": vectors.iter().map(|v| serde_json::json!({ "embedding": v })).collect::<Vec<_>>() }) } else { serde_json::json!({ "embeddings": vectors }) }.to_string();
                let _ = write!(
                    c,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
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
            std::fs::write(
                d.join("home").join(format!("doc-{i}.txt")),
                format!("zebra quartz {i} ").repeat(300),
            )
            .unwrap();
        }
        let cfg = SearchConfig {
            text_roots: vec![d.join("home")],
            meaning: true,
            meaning_engine: "ollama".into(),
            meaning_url: fake_embed_server(),
            meaning_model: "fake".into(),
            ..SearchConfig::default()
        };
        let (store, go) = (
            Store::open(&d.join("search.db")).unwrap(),
            AtomicBool::new(false),
        );
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        store.hurry.store(true, Ordering::Relaxed);
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (0, 40));
        store.bodies_read.store(0, Ordering::Relaxed);
        assert_eq!(store.similar("zebra quartz", None, 5).len(), 5);
        assert_eq!(
            store.bodies_read.load(Ordering::Relaxed),
            5,
            "five files shown, five read"
        );
        store.bodies_read.store(0, Ordering::Relaxed);
        let found = store.passages("zebra quartz", None, 6000);
        assert!(
            !found.is_empty()
                && found
                    .iter()
                    .map(|(p, t, _)| p.as_os_str().len() + 8 + t.len())
                    .sum::<usize>()
                    <= 6000,
            "{found:?}"
        );
        assert!(
            store.bodies_read.load(Ordering::Relaxed) <= ASK_FILES,
            "only the files that can give an excerpt are read"
        );
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Ask's excerpts: a hit with the passages before and after it, in the file's order, the
    /// strongest file first; the same text in a second file once; neighbours left out when
    /// they do not fit.
    #[test]
    fn ask_excerpts_read_as_a_whole() {
        let d =
            std::env::temp_dir().join(format!("coxswain-store-excerpts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home")).unwrap();
        // Letters the question does not have, but for the hit: the fake model counts letters.
        let text = format!(
            "# One\n{}\n# Two\n{}\n# Three\n{}\n# Four\n{}\n",
            "mill kiln ".repeat(30),
            "zebra quartz ".repeat(30),
            "milk hill ".repeat(30),
            "lily silk ".repeat(30)
        );
        std::fs::write(d.join("home/a.md"), &text).unwrap();
        std::fs::write(d.join("home/b.md"), &text).unwrap();
        let cfg = SearchConfig {
            text_roots: vec![d.join("home")],
            meaning: true,
            meaning_engine: "ollama".into(),
            meaning_url: fake_embed_server(),
            meaning_model: "fake".into(),
            ..SearchConfig::default()
        };
        let (store, go) = (
            Store::open(&d.join("search.db")).unwrap(),
            AtomicBool::new(false),
        );
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        store.hurry.store(true, Ordering::Relaxed);
        scan(&store, &cfg, &go).unwrap();
        let found = store.passages("zebra quartz", None, 10_000);
        assert_eq!(found.len(), 1, "the copy's text is given once: {found:?}");
        assert_eq!(found[0].2, 1, "and stands for the copy");
        let t = &found[0].1;
        let (before, hit, after) = (
            t.find("kiln").unwrap(),
            t.find("zebra").unwrap(),
            t.find("milk").unwrap(),
        );
        assert!(
            before < hit && hit < after && !t.contains("lily"),
            "the hit and its neighbours, in order: {t}"
        );
        let tight = store.passages("zebra quartz", None, 600);
        assert!(
            tight[0].1.contains("zebra") && !tight[0].1.contains("kiln"),
            "{tight:?}"
        );
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Fifty near copies of one text (a number apart, which the fake model does not see) and
    /// one other source: the copies fold into one source that says it stands for 49 more, and
    /// the other source gets a slot. Without folding the copies took all sixteen.
    #[test]
    fn ask_folds_near_copies_so_another_source_gets_a_slot() {
        let d = std::env::temp_dir().join(format!("coxswain-store-copies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home")).unwrap();
        for i in 0..50 {
            std::fs::write(
                d.join("home").join(format!("copy-{i}.txt")),
                format!("zebra quartz {i} ").repeat(30),
            )
            .unwrap();
        }
        std::fs::write(d.join("home/real.txt"), "zebra quartz milk ".repeat(30)).unwrap();
        let cfg = SearchConfig {
            text_roots: vec![d.join("home")],
            meaning: true,
            meaning_engine: "ollama".into(),
            meaning_url: fake_embed_server(),
            meaning_model: "fake".into(),
            ..SearchConfig::default()
        };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        store.hurry.store(true, Ordering::Relaxed);
        scan(&store, &cfg, &go).unwrap();
        let found = store.passages("zebra quartz", None, 20_000);
        let names: Vec<String> = found
            .iter()
            .map(|(p, _, n)| format!("{} {n}", p.file_name().unwrap().to_string_lossy()))
            .collect();
        assert_eq!(found.len(), 2, "{names:?}");
        assert!(names[0].starts_with("copy-") && names[0].ends_with(" 49"), "{names:?}");
        assert_eq!(names[1], "real.txt 0");
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
            std::fs::write(
                d.join("home").join(format!("doc-{i}.txt")),
                format!("zebra quartz {i} ").repeat(600),
            )
            .unwrap();
        }
        let cfg = SearchConfig {
            text_roots: vec![d.join("home")],
            meaning: true,
            meaning_engine: "ollama".into(),
            meaning_url: fake_embed_server(),
            meaning_model: "fake".into(),
            ..SearchConfig::default()
        };
        let go = AtomicBool::new(false);
        let open = || {
            let store = Store::open(&d.join("search.db")).unwrap();
            store.set_engine(crate::meaning::Engine::from_config(&cfg));
            store.hurry.store(true, Ordering::Relaxed);
            store
        };
        let store = open();
        assert_eq!(
            store.renewing.load(Ordering::Relaxed),
            0,
            "a new store has nothing to renew"
        );
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (0, 3));
        let chunks = |s: &Store| {
            s.db.lock()
                .unwrap()
                .query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        assert!(
            chunks(&store) > 3 * 8,
            "the whole of each file, not its first eight passages"
        );
        // Scheme 1, as a store from before had it (no `passages` in its meta).
        store.db.lock().unwrap().execute_batch("DELETE FROM meta WHERE key = 'passages'; UPDATE files SET modified = 5 WHERE path LIKE '%doc-1.txt'").unwrap();
        drop(store);

        let store = open();
        assert_eq!(store.renewing.load(Ordering::Relaxed), 3);
        drop(store);
        let store = open();
        assert_eq!(
            store.renewing.load(Ordering::Relaxed),
            3,
            "still renewing after a restart, or when another process opened the store first"
        );
        assert_eq!(
            (store.meaning_counts(), chunks(&store)),
            ((3, 0), 0),
            "the vectors went"
        );
        assert_eq!(store.search("zebra", None, 10).total, 3, "the text stayed");
        let first = store
            .unembedded(3)
            .unwrap()
            .into_iter()
            .map(|(_, path, _)| path)
            .collect::<Vec<_>>();
        assert!(
            first[2].ends_with("doc-1.txt"),
            "the last changed first: {first:?}"
        );
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
        assert_eq!(
            (
                store.renewing.load(Ordering::Relaxed),
                store.meaning_counts(),
                chunks(&store)
            ),
            (0, (0, 3), had)
        );
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A store whose passages a newer Coxswain cut keeps its vectors when an older one opens it.
    #[test]
    fn an_older_scheme_keeps_the_vectors_of_a_newer_one() {
        let d = std::env::temp_dir().join(format!("coxswain-store-newer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let store = Store::open(&d.join("search.db")).unwrap();
        {
            let db = store.db.lock().unwrap();
            db.execute("INSERT INTO files(id, path, size, modified, has_text, embedded) VALUES (1, '/a.txt', 1, 1, 1, 1)", []).unwrap();
            db.execute("INSERT INTO chunks(file, n, vector) VALUES (1, 0, x'00')", []).unwrap();
            db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES ('passages', '999')", []).unwrap();
        }
        drop(store);
        let store = Store::open(&d.join("search.db")).unwrap();
        assert_eq!(store.renewing.load(Ordering::Relaxed), 0);
        let db = store.db.lock().unwrap();
        let one = |q: &str| db.query_row(q, [], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!(one("SELECT CAST(count(*) AS TEXT) FROM chunks"), "1", "the vectors stayed");
        assert_eq!(one("SELECT value FROM meta WHERE key = 'passages'"), "999");
        drop(db);
        drop(store);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `bge-m3` picked again from Ollama's list as `bge-m3:latest`, or the same weights under
    /// another name, keep the vectors; another model does not.
    #[test]
    fn vectors_stay_when_only_the_way_the_model_is_written_changes() {
        assert!(crate::meaning::same_model(
            "ollama:bge-m3",
            "ollama:bge-m3:latest"
        ));
        assert!(!crate::meaning::same_model(
            "ollama:bge-m3",
            "ollama:nomic-embed-text"
        ));
        assert!(!crate::meaning::same_model(
            "openai:bge-m3",
            "ollama:bge-m3"
        ));
        let d = std::env::temp_dir().join(format!("coxswain-store-model-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let store = Store::open(&d.join("search.db")).unwrap();
        let vectors = || {
            let db = store.db.lock().unwrap();
            db.execute("INSERT OR IGNORE INTO files(id, path, size, modified, has_text) VALUES (1, '/a.txt', 1, 1, 1)", []).unwrap();
            db.execute(
                "INSERT OR REPLACE INTO chunks(file, n, vector) VALUES (1, 0, x'00')",
                [],
            )
            .unwrap();
            db.execute("UPDATE files SET embedded = 1", []).unwrap();
        };
        store.model_is("ollama:bge-m3", None).unwrap();
        vectors();
        store
            .model_is("ollama:bge-m3:latest", Some("sha256:1"))
            .unwrap();
        assert_eq!(
            store.meaning_counts(),
            (0, 1),
            "the same model with its tag"
        );
        store
            .model_is("ollama:m3-renamed:latest", Some("sha256:1"))
            .unwrap();
        assert_eq!(
            store.meaning_counts(),
            (0, 1),
            "the same weights under another name"
        );
        store
            .model_is("ollama:nomic-embed-text:latest", Some("sha256:2"))
            .unwrap();
        assert_eq!(
            store.meaning_counts(),
            (1, 0),
            "another model: the vectors go"
        );
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
        let dir =
            std::env::var_os("COXSWAIN_BENCH_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
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
            let words =
                "the launch plan and the fuel budget for the flight, written out in plain words "
                    .repeat(8);
            for f in 1..=FILES {
                tx.execute("INSERT INTO files(id, path, size, modified, has_text, embedded) VALUES (?1, ?2, 1, 1, 1, 1)", params![f, format!("/home/u/docs/doc-{f}.txt")]).unwrap();
                tx.execute(
                    "INSERT INTO text(rowid, body) VALUES (?1, ?2)",
                    params![
                        f,
                        (0..PER)
                            .map(|_| words.as_str())
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    ],
                )
                .unwrap();
                for n in 0..PER {
                    tx.execute(
                        "INSERT INTO chunks(file, n, vector) VALUES (?1, ?2, ?3)",
                        params![f, n, crate::meaning::pack(&unit())],
                    )
                    .unwrap();
                }
            }
            tx.commit().unwrap();
            println!("made in {:.0} s", t.elapsed().as_secs_f64());
        }
        println!("store: {} MB", store.bytes() >> 20);
        let q = unit();
        let t = Instant::now();
        let (count, bytes) = store.with_signs(|s| {
            (
                s.files.len(),
                s.bits.capacity() * 8 + s.files.capacity() * 8 + s.ns.capacity(),
            )
        });
        println!(
            "signs of {count} passages: {} MB, read in {:.0} ms",
            bytes >> 20,
            t.elapsed().as_secs_f64() * 1000.0
        );
        let wanted = crate::meaning::signs(&crate::meaning::pack(&q));
        let db = store.db.lock().unwrap();
        for _ in 0..3 {
            let t = Instant::now();
            let close = store.with_signs(|s| s.closest(&wanted, 1000));
            let sieve = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let mut vec = db
                .prepare_cached("SELECT vector FROM chunks WHERE file = ?1 AND n = ?2")
                .unwrap();
            let scored: Vec<f32> = close
                .iter()
                .filter_map(|(f, n)| {
                    vec.query_row(params![f, *n as i64], |r| {
                        Ok(crate::meaning::score(r.get_ref(0)?.as_blob()?, &q))
                    })
                    .ok()
                })
                .collect();
            println!(
                "sieve {sieve:.1} ms, {} vectors scored in {:.1} ms",
                scored.len(),
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
        let long =
            "the launch plan and the fuel budget for the flight, written out in plain words "
                .repeat(2000);
        for _ in 0..3 {
            let t = Instant::now();
            let n = crate::meaning::passages(&long, "long.txt").len();
            println!(
                "passages of a 30,000-word text: {n} in {:.1} ms",
                t.elapsed().as_secs_f64() * 1000.0
            );
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
        let d = std::env::temp_dir().join(format!(
            "coxswain-store-server-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home")).unwrap();
        std::fs::write(d.join("home/budget.txt"), "The fuel budget for flight seven is the largest cost of the launch, and the tanks are refilled twice before lift-off.").unwrap();
        std::fs::write(d.join("home/cake.txt"), "An apple cake: butter, sugar, flour, apples and cinnamon. Bake it for an hour and serve it with whipped cream.").unwrap();
        let mut cfg = SearchConfig {
            text_roots: vec![d.join("home")],
            meaning: true,
            meaning_engine: "ollama".into(),
            meaning_url: server.into(),
            meaning_model: model.into(),
            ..SearchConfig::default()
        };
        let (store, go) = (
            Store::open(&d.join("search.db")).unwrap(),
            AtomicBool::new(false),
        );
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        store.hurry.store(true, Ordering::Relaxed);
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.meaning_counts(), (0, 2));
        let first = |q: &str| {
            store
                .similar(q, None, 10)
                .first()
                .map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned())
        };
        assert_eq!(
            first("how much does fuelling the rocket cost").as_deref(),
            Some("budget.txt")
        );
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
        assert_eq!(
            store.meaning_counts(),
            (2, 0),
            "another model: the old vectors went"
        );
        assert!(store.meaning_error.lock().unwrap().is_some());
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A file an app keeps saving is found by its words at once and counted as changing, not
    /// as to go; it gets its vectors once unchanged for `QUIET`, and a new change, or a date
    /// in the future, makes it changing again.
    #[test]
    fn a_changing_file_gets_its_vectors_once_quiet() {
        let d = std::env::temp_dir().join(format!("coxswain-store-quiet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("home")).unwrap();
        let file = d.join("home").join("session.json");
        std::fs::write(&file, "zebra quartz ").unwrap();
        let cfg = SearchConfig {
            text_roots: vec![d.join("home")],
            meaning: true,
            meaning_engine: "ollama".into(),
            meaning_url: fake_embed_server(),
            meaning_model: "fake".into(),
            ..SearchConfig::default()
        };
        let go = AtomicBool::new(false);
        let store = Store::open(&d.join("search.db")).unwrap();
        store.set_engine(crate::meaning::Engine::from_config(&cfg));
        store.quiet.store(QUIET, Ordering::Relaxed);
        let state = |s: &Store| (s.meaning_counts(), s.meaning_changing());
        let date = |s: &Store, at: i64| {
            s.db.lock()
                .unwrap()
                .execute("UPDATE files SET modified = ?1", [at])
                .unwrap()
        };
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(store.search("zebra", None, 10).total, 1, "its words at once");
        assert_eq!(state(&store), ((0, 0), 1), "changing, not to go");
        assert!(store.unembedded(8).unwrap().is_empty());
        assert!(!has_backlog(&store).unwrap());

        // Five minutes on.
        date(&store, quiet_since(QUIET));
        assert_eq!(state(&store), ((1, 0), 0), "quiet: to go");
        assert_eq!(store.unembedded(8).unwrap().len(), 1);
        embed(&store, &go, None).unwrap();
        assert_eq!(state(&store), ((0, 1), 0), "it has its vectors");

        std::fs::write(&file, "zebra quartz orbit ").unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(state(&store), ((0, 0), 1), "saved again: changing again");

        date(&store, crate::fs::now() as i64 + 3600);
        assert_eq!(state(&store), ((0, 0), 1), "a date in the future is changing");
        // Dated in the future on disk too: unchanged at the next scan, it is quiet.
        let ahead = std::time::SystemTime::now() + Duration::from_secs(7200);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(ahead).unwrap();
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(state(&store), ((0, 0), 1), "first seen with that date: changing");
        scan(&store, &cfg, &go).unwrap();
        assert_eq!(state(&store), ((0, 1), 0), "unchanged since: quiet, and the scan gave it its vectors");
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A log with more text than is kept gets its passages from the whole file: its last
    /// words are in its last passage, and its passages are as many as the cap. One within the
    /// limit is cut from its text, as before.
    #[test]
    fn a_long_log_gets_its_passages_from_the_whole_file() {
        let d = std::env::temp_dir().join(format!("coxswain-long-passages-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let log = d.join("big.log");
        let mut text = String::new();
        let mut i = 0;
        while text.len() < crate::extract::MAX_TEXT * 3 / 2 {
            text.push_str(&format!("line {i} of the engine log, pressure nominal and valves closed\n"));
            i += 1;
        }
        text.push_str("finally the rocket landed safely in the harbour after its long flight\n");
        std::fs::write(&log, &text).unwrap();
        let path = log.to_string_lossy().into_owned();
        let body = crate::extract::text_of(&log, text.len() as u64, 20 << 20).unwrap();
        assert!(body.len() + LONG_MARGIN >= crate::extract::MAX_TEXT);
        let cut = crate::meaning::passages(&body, &path);
        let whole = whole_passages(&path, &body).expect("long: from the whole file");
        assert_eq!(whole.len(), crate::meaning::PASSAGES);
        assert!(whole.last().unwrap().text.contains("harbour"));
        assert!(!cut.iter().any(|p| p.text.contains("harbour")));
        assert_eq!(whole[0], cut[0], "the same start");
        let short = d.join("short.log");
        std::fs::write(&short, "a short log of a launch, all well, nothing more to say here\n").unwrap();
        assert!(whole_passages(&short.to_string_lossy(), "a short log").is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Long texts are found part by part: one past the first part is marked as well, and
    /// A Word file with more text than is kept gets its passages from the whole file, its end
    /// too; a short one gets those of its text.
    #[test]
    fn a_long_document_gets_its_passages_from_the_whole_file() {
        let d = std::env::temp_dir().join(format!("coxswain-long-docx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let docx = |name: &str, n: usize, last: &str| {
            let mut body: String = (0..n).map(|i| format!("<w:p><w:r><w:t>launch {i} of the rocket with its crew and fuel, all well</w:t></w:r></w:p>")).collect();
            body.push_str(&format!("<w:p><w:r><w:t>{last}</w:t></w:r></w:p>"));
            let p = d.join(name);
            crate::extract::tests::zip_file(&p, &[("word/document.xml", format!("<w:document xmlns:w=\"w\"><w:body>{body}</w:body></w:document>").as_bytes())]);
            p
        };
        let long = docx("long.docx", 80_000, "the ship reached the harbour at dawn and the crew went ashore");
        let path = long.to_string_lossy().into_owned();
        let body = crate::extract::text_of(&long, std::fs::metadata(&long).unwrap().len(), u64::MAX).unwrap();
        let whole = whole_passages(&path, &body).expect("long: from the whole file");
        assert_eq!(whole.len(), crate::meaning::PASSAGES);
        assert!(whole.last().unwrap().text.contains("harbour"));
        assert!(!crate::meaning::passages(&body, &path).iter().any(|p| p.text.contains("harbour")));
        let short = docx("short.docx", 30, "the harbour");
        let body = crate::extract::text_of(&short, std::fs::metadata(&short).unwrap().len(), u64::MAX).unwrap();
        assert!(whole_passages(&short.to_string_lossy(), &body).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    /// only a file read line by line.
    #[test]
    fn long_passages_are_found_past_the_first_part() {
        let d = std::env::temp_dir().join(format!("coxswain-long-parts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let store = Store::open(&d.join("search.db")).unwrap();
        let long = "x".repeat(crate::extract::MAX_TEXT);
        {
            let mut db = store.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            for id in 1..=2500i64 {
                let ext = if id == 10 { "pdf" } else { "log" };
                tx.execute("INSERT INTO files(id, path, size, modified, has_text, embedded) VALUES (?1, ?2, 1, 0, 1, 1)", params![id, format!("/r/{id}.{ext}")]).unwrap();
                let body = if id == 10 || id == 2400 { long.as_str() } else { "short" };
                tx.execute("INSERT INTO text(rowid, body) VALUES (?1, ?2)", params![id, body]).unwrap();
            }
            tx.commit().unwrap();
        }
        store.long_passages("passages.test", "1", LONG_MARGIN, crate::extract::streams).unwrap();
        let renewed: Vec<i64> = store.db.lock().unwrap().prepare("SELECT id FROM files WHERE embedded IS NULL").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert_eq!(renewed, [2400], "the long log, past the first part; not the PDF");
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A store stopped between adding `passage` and `heading` gets the second when opened.
    #[test]
    fn a_store_with_half_its_new_columns_gets_the_rest() {
        let d = std::env::temp_dir().join(format!("coxswain-store-columns-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("search.db");
        drop(Store::open(&path).unwrap());
        Connection::open(&path).unwrap().execute_batch("ALTER TABLE chunks DROP COLUMN heading").unwrap();
        let store = Store::open(&path).unwrap();
        assert!(store.db.lock().unwrap().prepare("SELECT passage, heading FROM chunks LIMIT 0").is_ok());
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// *Everything* for a folder keeps all of a long log's text and a passage for every part
    /// of it, as the warning said beforehand; back to *Sample*, nothing is read or embedded
    /// again, the vectors and the text are exactly those *Sample* made, and the store shrinks.
    #[test]
    fn long_files_everything_and_back_to_sample() {
        let d = std::env::temp_dir().join(format!("coxswain-long-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let home = d.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let mut log = String::new();
        let mut i = 0;
        while log.len() < crate::extract::MAX_TEXT + (1 << 20) {
            log.push_str(&format!("line {i} of the engine log, pressure nominal and valves closed\n"));
            i += 1;
        }
        log.push_str("finally the rocket landed safely in the harbour after its long flight\n");
        std::fs::write(home.join("big.log"), &log).unwrap();
        // More passages than Sample keeps, well within what is kept of its text.
        let notes: String = (0..400).map(|i| format!("Chapter {i}. The crew checked the fuel, the valves and the pressure of tank {i} before the launch.\n\n")).collect::<String>().repeat(6);
        std::fs::write(home.join("notes.txt"), &notes).unwrap();
        std::fs::write(home.join("short.txt"), "a short note about the launch window and nothing more").unwrap();
        // A Word file with more text than is kept: its passages come from all of it.
        let paras: String = (0..80_000).map(|i| format!("<w:p><w:r><w:t>Report {i} of the mission control room, all systems nominal</w:t></w:r></w:p>")).collect();
        let report = format!("<w:document xmlns:w=\"w\"><w:body>{paras}<w:p><w:r><w:t>the capsule splashed down near the lighthouse at dawn and the crew was well</w:t></w:r></w:p></w:body></w:document>");
        crate::extract::tests::zip_file(&home.join("report.docx"), &[("word/document.xml", report.as_bytes())]);
        let sample = SearchConfig {
            text_roots: vec![home.clone()],
            meaning: true,
            meaning_engine: "ollama".into(),
            meaning_url: fake_embed_server(),
            meaning_model: "fake".into(),
            ..SearchConfig::default()
        };
        let everything = SearchConfig { long_files_everything: vec![home.clone()], ..sample.clone() };
        let (store, go) = (Store::open(&d.join("search.db")).unwrap(), AtomicBool::new(false));
        let mode: i64 = store.db.lock().unwrap().pragma_query_value(None, "auto_vacuum", |r| r.get(0)).unwrap();
        assert_eq!(mode, 2, "a new store vacuums incrementally");
        store.set_engine(crate::meaning::Engine::from_config(&sample));
        store.hurry.store(true, Ordering::Relaxed);
        scan(&store, &sample, &go).unwrap();
        let id = |name: &str| -> i64 {
            store.db.lock().unwrap().query_row("SELECT id FROM files WHERE path = ?1", [home.join(name).to_string_lossy()], |r| r.get(0)).unwrap()
        };
        let (big, notes_id, short, docx) = (id("big.log"), id("notes.txt"), id("short.txt"), id("report.docx"));
        let vectors = |file: i64| -> Vec<Vec<u8>> {
            let db = store.db.lock().unwrap();
            let mut q = db.prepare("SELECT vector FROM chunks WHERE file = ?1 ORDER BY n").unwrap();
            q.query_map([file], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        let body = |file: i64| -> String { store.db.lock().unwrap().query_row("SELECT body FROM text WHERE rowid = ?1", [file], |r| r.get(0)).unwrap() };
        let size = || {
            store.db.lock().unwrap().execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
            std::fs::metadata(d.join("search.db")).unwrap().len()
        };
        let (big_sampled, notes_sampled, big_text) = (vectors(big), vectors(notes_id), body(big));
        let docx_sampled = vectors(docx);
        assert_eq!((big_sampled.len(), notes_sampled.len(), docx_sampled.len()), (crate::meaning::PASSAGES, crate::meaning::PASSAGES, crate::meaning::PASSAGES));
        let kept: Vec<String> = store.db.lock().unwrap().prepare("SELECT passage FROM chunks WHERE file = ?1 AND passage IS NOT NULL ORDER BY n").unwrap().query_map([docx], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert!(kept.last().is_some_and(|p| p.contains("lighthouse")), "Sample: the Word file's passages reach its end");
        assert!(big_text.len() <= crate::extract::MAX_TEXT);
        assert_eq!(store.search("harbour", None, 10).total, 0, "Sample keeps the first 4 MB of text");
        let short_vectors = vectors(short);

        // The warning: the two long files, more passages and room, the log read again.
        let cost = store.long_files_cost(&crate::long_files::Rule::of(&everything)).unwrap();
        assert_eq!((cost.files, cost.back), (3, 0), "{cost:?}");
        assert!(cost.passages > 5000 && cost.bytes > (1 << 20) && cost.read >= log.len() as u64, "{cost:?}");
        assert_eq!(store.long_files_cost(&crate::long_files::Rule::of(&sample)).unwrap(), crate::long_files::Cost::default(), "nothing changes");
        let small = size();

        scan(&store, &everything, &go).unwrap();
        let (big_all, notes_all) = (vectors(big), vectors(notes_id));
        assert!(big_all.len() > 5000 && notes_all.len() > crate::meaning::PASSAGES, "{} {}", big_all.len(), notes_all.len());
        // Its first 4 MB of text would give about 5,800.
        assert!(vectors(docx).len() >= 7900, "Everything: a passage for every part of the whole Word file: {}", vectors(docx).len());
        assert_eq!(store.search("harbour", None, 10).total, 1, "Everything keeps all the text");
        assert!(body(big).len() > crate::extract::MAX_TEXT);
        assert_eq!(vectors(short), short_vectors, "a short file stays as it was");
        assert!(store.meaning_counts().0 == 0, "every file has its vectors");
        let large = size();
        assert!(large > small, "{large} > {small}");
        // Found by what its last part is about.
        assert!(store.similar("rocket landed harbour flight", None, 3).iter().any(|h| h.path.ends_with("big.log")));
        // Its passages past the 256th are numbered as they are kept, and read one by one.
        let asked = store.passages("finally the rocket landed safely in the harbour", None, 6000);
        assert!(asked.iter().any(|(p, text, _)| p.ends_with("big.log") && text.contains("harbour")), "{asked:?}");

        // The cost of going back, then going back: nothing read or embedded again.
        let back = store.long_files_cost(&crate::long_files::Rule::of(&sample)).unwrap();
        assert_eq!((back.files, back.back), (0, 3), "{back:?}");
        assert!(back.freed > (1 << 20), "{back:?}");
        store.long_files_changed(&crate::long_files::Rule::of(&sample)).unwrap();
        assert_eq!(store.unread_count().unwrap(), 0, "nothing is read again");
        assert!(store.unembedded(8).unwrap().is_empty(), "no vectors are made again");
        assert_eq!(vectors(big), big_sampled, "the log's vectors are Sample's");
        assert_eq!(vectors(notes_id), notes_sampled, "the notes' vectors are Sample's");
        assert_eq!(vectors(docx), docx_sampled, "the Word file's vectors are Sample's");
        assert_eq!(body(big), big_text, "the text is cut back as reading would cut it");
        assert_eq!(store.search("harbour", None, 10).total, 0);
        let mode: i64 = store.db.lock().unwrap().pragma_query_value(None, "auto_vacuum", |r| r.get(0)).unwrap();
        assert_eq!(mode, 2);
        let after = size();
        assert!(after < large, "the store gave the room back: {after} < {large}");
        // A scan after it changes nothing more.
        scan(&store, &sample, &go).unwrap();
        assert_eq!(vectors(big), big_sampled);
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A store made before incremental vacuum gets it at its first compaction.
    #[test]
    fn long_files_compaction_turns_an_old_store_incremental() {
        let d = std::env::temp_dir().join(format!("coxswain-compact-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let old = Connection::open(d.join("search.db")).unwrap();
        old.execute_batch("CREATE TABLE old(x); DROP TABLE old;").unwrap();
        drop(old);
        let store = Store::open(&d.join("search.db")).unwrap();
        let mode = || -> i64 { store.db.lock().unwrap().pragma_query_value(None, "auto_vacuum", |r| r.get(0)).unwrap() };
        assert_eq!(mode(), 0);
        store.compact().unwrap();
        assert_eq!(mode(), 2);
        store.compact().unwrap();
        drop(store);
        std::fs::remove_dir_all(d).unwrap();
    }
}

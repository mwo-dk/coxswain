//! A games table on the disk (`games::Library`), so a database of any size opens in the same
//! memory: one row of `ROW` bytes a game in `rows`, read where it is, and the names (players,
//! events, openings) in `names.db` (SQLite), each with its sort key. Kept in the cache's
//! `chess/tables/`, one folder a database (`<path's hash>-<run>-<n>`): opened again, a database
//! that has not changed is not read again. A folder is whole once its `done` file is there,
//! written last, after the rest is on the disk: one a crash or a stop cut short has none and is
//! never read as a table, and goes. A table goes when its database changes or goes, when it was
//! not opened for `KEPT`, past `MOST` in all (the oldest first), and with Disk use's *Clear* of
//! the `chess` row.

use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};

/// A game's row on the disk.
pub(crate) const ROW: usize = 52;
/// What the folder holds changes form: an older form is never read.
const FORM: &str = "2";
/// A table not opened for this long goes.
pub const KEPT: Duration = Duration::from_secs(30 * 24 * 3600);
/// The most all tables take: past it the ones opened longest ago go, never the newest.
pub const MOST: u64 = 4 << 30;
/// A table being made whose rows did not grow for this long was left by a run that ended.
const STALE: Duration = Duration::from_secs(10 * 60);
/// Rows read at a time when the table is gone through.
const CHUNK: usize = 1 << 16;

/// One game in the table.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Rec {
    pub offset: u64,
    pub len: u32,
    pub white: u32,
    pub black: u32,
    pub event: u32,
    /// The opening's name; `u32::MAX` for none, as every name.
    pub opening: u32,
    /// yyyymmdd, unknown parts 0.
    pub date: u32,
    /// The result as written (draughts writes `2-0`); `u32::MAX` for chess.
    pub result_name: u32,
    pub white_elo: u16,
    pub black_elo: u16,
    /// 'A'..'E' as 1..5 times 100 plus the number, 0 for none.
    pub eco: u16,
    pub plies: u16,
    /// 1 White won, 2 Black won, 3 a draw, 0 anything else.
    pub result: u8,
    /// The site, for the search (`games::search_text`).
    pub site: u32,
}

impl Rec {
    fn put(&self, b: &mut [u8]) {
        b[..8].copy_from_slice(&self.offset.to_le_bytes());
        for (i, v) in [self.len, self.white, self.black, self.event, self.opening, self.date, self.result_name].into_iter().enumerate() {
            b[8 + 4 * i..12 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        for (i, v) in [self.white_elo, self.black_elo, self.eco, self.plies].into_iter().enumerate() {
            b[36 + 2 * i..38 + 2 * i].copy_from_slice(&v.to_le_bytes());
        }
        b[44] = self.result;
        b[45..48].fill(0);
        b[48..52].copy_from_slice(&self.site.to_le_bytes());
    }

    fn get(b: &[u8]) -> Rec {
        let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        Rec {
            offset: u64::from_le_bytes(b[..8].try_into().expect("8 bytes")),
            len: u32_at(8),
            white: u32_at(12),
            black: u32_at(16),
            event: u32_at(20),
            opening: u32_at(24),
            date: u32_at(28),
            result_name: u32_at(32),
            white_elo: u16_at(36),
            black_elo: u16_at(38),
            eco: u16_at(40),
            plies: u16_at(42),
            result: b[44],
            site: u32_at(48),
        }
    }
}

/// What `done` says: the table is whole, of the database as it was then.
#[derive(Serialize, Deserialize)]
pub(crate) struct Done {
    stamp: String,
    /// The limits it was read with.
    extra: String,
    /// Kept whole when it is not UTF-8, so its table is found again.
    #[serde(with = "crate::spill::path_json")]
    pub source: PathBuf,
    pub games: u32,
    pub capped: bool,
}

struct Inner {
    rows: Mutex<File>,
    /// While the table is made: where its rows go.
    writer: Mutex<Option<BufWriter<File>>>,
    names: Mutex<Connection>,
}

/// An open table: one kept whole from before, or one being made now.
pub(crate) struct Table {
    dir: PathBuf,
    inner: Option<Inner>,
    /// `done` is written: the folder stays when this goes.
    whole: AtomicBool,
    /// Not in the cache (there is none): removed when this goes.
    temp: bool,
    stamp: String,
    extra: String,
    source: PathBuf,
}

/// Where tables are kept: `chess/tables/` in the cache folder.
pub fn folder() -> Option<PathBuf> {
    Some(crate::helper::folder()?.join("chess").join("tables"))
}

/// The files a database is read from: the index's siblings too (SCID, ChessBase).
fn parts(path: &Path) -> Vec<PathBuf> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let siblings: &[&str] = match ext.as_str() {
        "si4" => &["sg4", "sn4"],
        "si5" => &["sg5", "sn5"],
        "cbh" => &["cbg", "cbp", "cbt", "cba", "cbj"],
        _ => &[],
    };
    std::iter::once(path.to_path_buf()).chain(siblings.iter().map(|e| path.with_extension(e))).collect()
}

/// The database's identity: its path, and the size and time of each of its files.
fn stamp(path: &Path) -> String {
    let mut h = blake3::Hasher::new();
    h.update(FORM.as_bytes());
    h.update(path.as_os_str().as_encoded_bytes());
    // SCID 5's reader changed (deleted games, names after the base's description): its tables
    // are read again once.
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("si5")) {
        h.update(b"si5 2");
    }
    // A file inside an archive (read as it is unpacked): the archive's size and date, and the entry's size.
    let outer = (!path.exists()).then(|| crate::archive::split(path)).flatten().map(|(a, _)| a);
    if outer.is_some() {
        h.update(&crate::archive::entry_size(path).unwrap_or(0).to_le_bytes());
    }
    for p in parts(path).into_iter().chain(outer) {
        if let Ok(m) = std::fs::metadata(&p) {
            let when = m.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
            h.update(&m.len().to_le_bytes());
            h.update(&when.to_le_bytes());
        }
    }
    h.finalize().to_hex()[..32].to_string()
}

/// A table folder's name starts with this, the hash of the database's path.
fn key(path: &Path) -> String {
    let mut h = blake3::Hasher::new();
    h.update(FORM.as_bytes());
    h.update(path.as_os_str().as_encoded_bytes());
    h.finalize().to_hex()[..16].to_string()
}

fn read_done(dir: &Path) -> Option<Done> {
    serde_json::from_slice(&std::fs::read(dir.join("done")).ok()?).ok()
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// A folder being made, by a run that ended without finishing it: its run (named in the
/// folder, `<key>-<run>-<n>`) has ended; or, when that cannot be told, its rows did not grow
/// for `STALE`.
fn stale(dir: &Path) -> bool {
    let name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let run = name.split_once('-').and_then(|(_, r)| r.rsplit_once('-')).map(|(r, _)| r.to_string());
    match run.as_deref().and_then(crate::spill::alive) {
        Some(alive) => !alive,
        None => {
            let when = modified(&dir.join("rows")).or_else(|| modified(dir));
            when.is_none_or(|t| t.elapsed().unwrap_or_default() > STALE)
        }
    }
}

fn connect(path: &Path, write: bool) -> io::Result<Connection> {
    let flags = if write { OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE } else { OpenFlags::SQLITE_OPEN_READ_ONLY };
    let db = Connection::open_with_flags(path, flags | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(io::Error::other)?;
    if write {
        // Nothing to keep whole on the way: a folder without `done` is never read.
        db.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; CREATE TABLE names (id INTEGER PRIMARY KEY, name TEXT NOT NULL, key BLOB NOT NULL);")
            .map_err(io::Error::other)?;
    }
    Ok(db)
}

impl Table {
    /// The table of the database at `path`: the one kept from before when it is whole and the
    /// database has not changed since and it was read with the same `extra` (the limits), with
    /// what `done` says; else a new one to fill.
    pub fn open(path: &Path, extra: &str) -> io::Result<(Table, Option<Done>)> {
        // One table a file, from whichever folder it is named.
        let path = &std::path::absolute(path)?;
        // Each table made in this run gets a folder of its own.
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let stamp = stamp(path);
        let source = path.to_path_buf();
        if let Some(root) = folder() {
            prune(&root);
            let key = key(path);
            for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !name.starts_with(&format!("{key}-")) {
                    continue;
                }
                let dir = e.path();
                match read_done(&dir) {
                    Some(d) if d.stamp == stamp && d.extra == extra && d.source == source => {
                        if let Ok(t) = Table::kept(&dir, &stamp, extra, &source) {
                            crate::spill::used(&dir);
                            // Two apps that read the file at once each made one: one is kept.
                            for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
                                let other = e.path();
                                if other != dir
                                    && e.file_name().to_string_lossy().starts_with(&format!("{key}-"))
                                    && read_done(&other).is_some_and(|o| o.source == source && o.extra == extra)
                                {
                                    crate::spill::gone(&other);
                                }
                            }
                            return Ok((t, Some(d)));
                        }
                        crate::spill::gone(&dir);
                    }
                    // The database changed since: its old table goes.
                    Some(d) if d.source == source && d.stamp != stamp => crate::spill::gone(&dir),
                    None if stale(&dir) => crate::spill::gone(&dir),
                    _ => {}
                }
            }
            // This run's list held, so another run tells this folder from one a crash left
            // (`stale`), also in a run that never wrote a passing name.
            crate::spill::register();
            let dir = root.join(format!("{key}-{}-{}", crate::spill::run(), N.fetch_add(1, Ordering::Relaxed)));
            if let Ok(t) = Table::new(&dir, false, &stamp, extra, &source) {
                return Ok((t, None));
            }
        }
        let dir = std::env::temp_dir().join(format!("coxswain-games-{}-{}", crate::spill::run(), N.fetch_add(1, Ordering::Relaxed)));
        Ok((Table::new(&dir, true, &stamp, extra, &source)?, None))
    }

    fn kept(dir: &Path, stamp: &str, extra: &str, source: &Path) -> io::Result<Table> {
        let inner = Inner { rows: Mutex::new(File::open(dir.join("rows"))?), writer: Mutex::new(None), names: Mutex::new(connect(&dir.join("names.db"), false)?) };
        Ok(Table { dir: dir.to_path_buf(), inner: Some(inner), whole: AtomicBool::new(true), temp: false, stamp: stamp.into(), extra: extra.into(), source: source.into() })
    }

    fn new(dir: &Path, temp: bool, stamp: &str, extra: &str, source: &Path) -> io::Result<Table> {
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        std::fs::create_dir_all(dir)?;
        let made = (|| {
            let w = File::create(dir.join("rows"))?;
            let inner = Inner { rows: Mutex::new(File::open(dir.join("rows"))?), writer: Mutex::new(Some(BufWriter::with_capacity(1 << 20, w))), names: Mutex::new(connect(&dir.join("names.db"), true)?) };
            Ok(Table { dir: dir.to_path_buf(), inner: Some(inner), whole: AtomicBool::new(false), temp, stamp: stamp.into(), extra: extra.into(), source: source.into() })
        })();
        if made.is_err() {
            let _ = std::fs::remove_dir_all(dir);
        }
        made
    }

    fn inner(&self) -> &Inner {
        self.inner.as_ref().expect("taken only when dropped")
    }

    /// The table's folder, where a database read out of an archive keeps its games' text.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Rows and the names they bring, added at the end; readable once this returns.
    pub fn append(&self, recs: &[Rec], names: &[(u32, Box<str>)]) -> io::Result<()> {
        let inner = self.inner();
        {
            let db = inner.names.lock().unwrap_or_else(|e| e.into_inner());
            let tx = db.unchecked_transaction().map_err(io::Error::other)?;
            {
                let mut put = tx.prepare_cached("INSERT INTO names (id, name, key) VALUES (?1, ?2, ?3)").map_err(io::Error::other)?;
                for (id, name) in names {
                    put.execute(params![id, &**name, crate::collate::key(name)]).map_err(io::Error::other)?;
                }
            }
            tx.commit().map_err(io::Error::other)?;
        }
        let mut w = inner.writer.lock().unwrap_or_else(|e| e.into_inner());
        let w = w.as_mut().ok_or_else(|| io::Error::other("the table is whole"))?;
        let mut b = [0u8; ROW];
        for r in recs {
            r.put(&mut b);
            w.write_all(&b)?;
        }
        w.flush()
    }

    /// All rows are in: the names' order is kept, everything is put on the disk, then `done`
    /// says the table is whole.
    pub fn finish(&self, games: u32, capped: bool) -> io::Result<()> {
        let inner = self.inner();
        if let Some(w) = inner.writer.lock().unwrap_or_else(|e| e.into_inner()).take() {
            w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        }
        // SQLite puts the names on the disk itself, through the file it holds open.
        inner.names.lock().unwrap_or_else(|e| e.into_inner()).execute_batch("PRAGMA synchronous=FULL; CREATE INDEX names_key ON names (key, id);").map_err(io::Error::other)?;
        if self.temp {
            return Ok(());
        }
        let done = Done { stamp: self.stamp.clone(), extra: self.extra.clone(), source: self.source.clone(), games, capped };
        let part = self.dir.join("done.part");
        let mut f = File::create(&part)?;
        f.write_all(&serde_json::to_vec(&done).map_err(io::Error::other)?)?;
        f.sync_all()?;
        std::fs::rename(&part, self.dir.join("done"))?;
        self.whole.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Row `i`, of the `count` there are.
    pub fn get(&self, i: u32, count: u32) -> Option<Rec> {
        if i >= count {
            return None;
        }
        let mut b = [0u8; ROW];
        let mut f = self.inner().rows.lock().unwrap_or_else(|e| e.into_inner());
        f.seek(SeekFrom::Start(i as u64 * ROW as u64)).ok()?;
        f.read_exact(&mut b).ok()?;
        Some(Rec::get(&b))
    }

    /// The first `count` rows, in order, `CHUNK` of them read at a time.
    pub fn each(&self, count: u32, mut each: impl FnMut(u32, &Rec)) -> io::Result<()> {
        let mut buf = vec![0u8; CHUNK * ROW];
        let mut i = 0u32;
        while i < count {
            let n = (count - i).min(CHUNK as u32) as usize;
            {
                let mut f = self.inner().rows.lock().unwrap_or_else(|e| e.into_inner());
                f.seek(SeekFrom::Start(i as u64 * ROW as u64))?;
                f.read_exact(&mut buf[..n * ROW])?;
            }
            for k in 0..n {
                each(i + k as u32, &Rec::get(&buf[k * ROW..(k + 1) * ROW]));
            }
            i += n as u32;
        }
        Ok(())
    }

    /// The name `id`; empty for none.
    pub fn name(&self, id: u32) -> String {
        if id == u32::MAX {
            return String::new();
        }
        let db = self.inner().names.lock().unwrap_or_else(|e| e.into_inner());
        db.prepare_cached("SELECT name FROM names WHERE id = ?1").and_then(|mut q| q.query_row([id], |r| r.get(0))).unwrap_or_default()
    }

    /// Which names have `needle` (lower case) in them, by id.
    pub fn matching(&self, needle: &str) -> Vec<bool> {
        let db = self.inner().names.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = vec![];
        let mut set = |id: u32| {
            if out.len() <= id as usize {
                out.resize(id as usize + 1, false);
            }
            out[id as usize] = true;
        };
        // SQLite's LIKE folds ASCII letters only: the same as a lower-case `contains` for an
        // ASCII needle; any other needle is looked for here.
        if needle.is_ascii() {
            let like = format!("%{}%", needle.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            if let Ok(mut q) = db.prepare("SELECT id FROM names WHERE name LIKE ?1 ESCAPE '\\'") {
                let _ = q.query_map([like], |r| r.get::<_, u32>(0)).map(|rows| rows.flatten().for_each(&mut set));
            }
        } else if let Ok(mut q) = db.prepare("SELECT id, name FROM names") {
            let _ = q.query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?))).map(|rows| {
                rows.flatten().filter(|(_, n)| n.to_lowercase().contains(needle)).for_each(|(id, _)| set(id))
            });
        }
        out
    }

    /// Each name's place among all, by id, in the app's collation (`collate`).
    pub fn ranks(&self) -> Vec<u32> {
        let db = self.inner().names.lock().unwrap_or_else(|e| e.into_inner());
        let mut rank = vec![];
        if let Ok(mut q) = db.prepare("SELECT id FROM names ORDER BY key, id") {
            let _ = q.query_map([], |r| r.get::<_, u32>(0)).map(|rows| {
                for (n, id) in (0u32..).zip(rows.flatten()) {
                    if rank.len() <= id as usize {
                        rank.resize(id as usize + 1, u32::MAX);
                    }
                    rank[id as usize] = n;
                }
            });
        }
        rank
    }
}

/// The whole tables kept, each with what its `done` says, of files that have not changed since
/// and that `wanted` takes (by their path): what Find looks through (`games::find`).
pub(crate) fn whole(wanted: &dyn Fn(&Path) -> bool) -> Vec<(Table, Done)> {
    let mut out: Vec<(Table, Done)> = vec![];
    for e in folder().and_then(|f| std::fs::read_dir(f).ok()).into_iter().flatten().flatten() {
        let dir = e.path();
        let Some(d) = read_done(&dir) else { continue };
        // One a file, when two apps each made one (the same file: the same rows).
        if !wanted(&d.source) || d.stamp != stamp(&d.source) || out.iter().any(|(_, o)| o.source == d.source) {
            continue;
        }
        if let Ok(t) = Table::kept(&dir, &d.stamp, &d.extra, &d.source) {
            // Looked through by Find: kept as one opened.
            crate::spill::used(&dir);
            out.push((t, d));
        }
    }
    out
}

impl Drop for Table {
    fn drop(&mut self) {
        // The files closed first: a folder with open files cannot go on Windows.
        drop(self.inner.take());
        if self.temp || !self.whole.load(Ordering::Relaxed) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// `prune_in` on the cache's tables, at most every few hours in a run.
fn prune(root: &Path) {
    static LAST: Mutex<Option<SystemTime>> = Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| t.elapsed().unwrap_or_default() < Duration::from_secs(6 * 3600)) {
            return;
        }
        *last = Some(SystemTime::now());
    }
    prune_in(root, KEPT, MOST);
}

/// The tables in `root` whose database is gone, or not opened for `age`, removed; then the ones
/// opened longest ago until all take no more than `most`, the newest always kept. Folders being
/// made are left, but for ones a run that ended left.
pub(crate) fn prune_in(root: &Path, age: Duration, most: u64) {
    let mut kept: Vec<(SystemTime, u64, PathBuf)> = vec![];
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let dir = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(".gone-") {
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        }
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Some(done) = read_done(&dir) else {
            if stale(&dir) {
                crate::spill::gone(&dir);
            }
            continue;
        };
        let when = modified(&dir).unwrap_or(SystemTime::UNIX_EPOCH);
        // Gone from a folder that is there: a disk not plugged in keeps its tables.
        // A PGN through an archive is there while its archive is.
        let src = if done.source.exists() { done.source.clone() } else { crate::archive::split(&done.source).map_or(done.source.clone(), |(a, _)| a) };
        let gone = !src.exists() && src.parent().is_some_and(|p| p.exists());
        if gone || when.elapsed().unwrap_or_default() > age {
            crate::spill::gone(&dir);
        } else {
            kept.push((when, crate::fs::dir_size(&dir).0, dir));
        }
    }
    let mut total: u64 = kept.iter().map(|k| k.1).sum();
    kept.sort();
    kept.pop();
    for (_, bytes, dir) in kept {
        if total <= most {
            break;
        }
        crate::spill::gone(&dir);
        total = total.saturating_sub(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_back_as_it_went() {
        let r = Rec { offset: 1 << 40, len: 7, white: 1, black: 2, event: u32::MAX, opening: 3, date: 18581102, result_name: u32::MAX, white_elo: 2690, black_elo: 0, eco: 341, plies: 9, result: 1, site: 5 };
        let mut b = [0u8; ROW];
        r.put(&mut b);
        assert_eq!(Rec::get(&b), r);
    }

    #[test]
    fn kept_until_the_database_changes_or_goes() {
        let src = std::env::temp_dir().join(format!("coxswain-table-test-{}", std::process::id()));
        std::fs::create_dir_all(&src).unwrap();
        let db = src.join("a.pgn");
        std::fs::write(&db, "x").unwrap();
        let (t, done) = Table::open(&db, "").unwrap();
        assert!(done.is_none());
        // Being made by a run that lives: never taken for one a crash left, however long it waits.
        assert!(!stale(&t.dir));
        let rec = Rec { white: 0, opening: u32::MAX, ..Rec::default() };
        t.append(&[rec, Rec { white: 1, ..rec }], &[(0, "Zoë".into()), (1, "anand".into())]).unwrap();
        t.finish(2, false).unwrap();
        assert_eq!((t.name(1), t.ranks(), t.matching("ZO".to_lowercase().as_str())), ("anand".to_string(), vec![1, 0], vec![true]));
        assert_eq!(t.matching("oë"), vec![true]);
        let dir = t.dir.clone();
        drop(t);
        // Opened again: the same table, read from the disk.
        let (t, done) = Table::open(&db, "").unwrap();
        assert_eq!((done.map(|d| d.games), t.get(1, 2).map(|r| r.white), t.get(2, 2)), (Some(2), Some(1), None));
        drop(t);
        // Another limit: another table; the kept one is not taken.
        assert!(Table::open(&db, "other").unwrap().1.is_none());
        assert!(dir.exists());
        // Changed: made anew, the old one gone.
        std::fs::write(&db, "xy").unwrap();
        let (t, done) = Table::open(&db, "").unwrap();
        assert!(done.is_none() && !dir.exists());
        // Never finished: its folder goes with it.
        let made = t.dir.clone();
        drop(t);
        assert!(!made.exists());
        // A whole table of a database that is gone goes at the next prune.
        let (t, _) = Table::open(&db, "").unwrap();
        t.finish(0, false).unwrap();
        let (dir, root) = (t.dir.clone(), folder().unwrap());
        drop(t);
        std::fs::remove_file(&db).unwrap();
        prune_in(&root, KEPT, MOST);
        assert!(!dir.exists());
        std::fs::remove_dir_all(&src).unwrap();
    }

    /// A database whose path is not UTF-8 is kept: its `done` is written and read back whole.
    #[cfg(unix)]
    #[test]
    fn a_path_not_utf8_is_kept() {
        use std::os::unix::ffi::OsStrExt;
        let source = PathBuf::from(std::ffi::OsStr::from_bytes(b"/games/caf\xe9.pgn"));
        let done = Done { stamp: "s".into(), extra: String::new(), source: source.clone(), games: 3, capped: false };
        let back: Done = serde_json::from_slice(&serde_json::to_vec(&done).unwrap()).unwrap();
        assert_eq!((back.source, back.games), (source, 3));
        // A `done` of before reads as it did.
        let old: Done = serde_json::from_str(r#"{"stamp":"s","extra":"","source":"/games/a.pgn","games":1,"capped":false}"#).unwrap();
        assert_eq!(old.source, Path::new("/games/a.pgn"));
    }

    /// A table being made holds its run's list, so another run never takes it for one a crash
    /// left, however slowly it grows, also in a run that wrote no passing name before.
    #[test]
    fn a_table_being_made_holds_its_runs_list() {
        let src = std::env::temp_dir().join(format!("coxswain-table-list-{}", std::process::id()));
        std::fs::create_dir_all(&src).unwrap();
        let db = src.join("b.pgn");
        std::fs::write(&db, "x").unwrap();
        let (t, _) = Table::open(&db, "").unwrap();
        assert!(!t.temp);
        assert_eq!(crate::spill::list_held(crate::spill::run()), Some(true));
        drop(t);
        std::fs::remove_dir_all(&src).unwrap();
    }
}

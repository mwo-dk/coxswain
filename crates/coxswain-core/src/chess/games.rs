//! A PGN file's games as a table, for files of millions of games: one pass reads only where
//! each game starts and its headers (on a thread of its own, the table filling as it goes);
//! a game's moves are read when it is opened. `.pgn.gz` and `.pgn.zst` are read as they unpack.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::openings;
use super::pgn::{self, Word};

/// The tags the table and the search read, by their place in `Head::tags`.
const WANTED: [&str; 11] =
    ["White", "Black", "WhiteElo", "BlackElo", "Result", "Date", "Event", "Site", "ECO", "Opening", "Variation"];
const WHITE: usize = 0;
const BLACK: usize = 1;
const WHITE_ELO: usize = 2;
const BLACK_ELO: usize = 3;
const RESULT: usize = 4;
const DATE: usize = 5;
const EVENT: usize = 6;
const SITE: usize = 7;
const ECO: usize = 8;
const OPENING: usize = 9;
const VARIATION: usize = 10;
/// The first moves kept to name the opening.
const FIRST_MOVES: usize = 24;

/// A game as the pass sees it: where it is, the headers wanted, its length in half-moves and
/// its first moves.
pub(crate) struct Head {
    pub offset: u64,
    pub len: u64,
    pub tags: [String; WANTED.len()],
    pub plies: u32,
    pub sans: Vec<String>,
}

impl Head {
    fn tag(&self, i: usize) -> &str {
        self.tags[i].trim()
    }

    /// ECO code and opening name: the file's own, else by the moves, else by the code.
    pub(crate) fn opening(&self) -> (String, String) {
        let eco = self.tag(ECO).to_string();
        let mut name = self.tag(OPENING).to_string();
        if !name.is_empty() && !self.tag(VARIATION).is_empty() {
            name = format!("{name}: {}", self.tag(VARIATION));
        }
        if name.is_empty() {
            match openings::by_moves(self.sans.iter().map(String::as_str)) {
                Some((code, n)) if eco.is_empty() || eco == code => return (code.to_string(), n.to_string()),
                _ => name = openings::by_eco(&eco).unwrap_or_default().to_string(),
            }
        }
        (eco, name)
    }

    /// The game in one line, as the search store keeps it: players, event, site, date, opening
    /// and result.
    pub(crate) fn line(&self) -> String {
        let (eco, opening) = self.opening();
        let parts = [
            format!("{} – {}", self.tag(WHITE), self.tag(BLACK)),
            self.tag(EVENT).to_string(),
            self.tag(SITE).to_string(),
            self.tag(DATE).to_string(),
            format!("{eco} {opening}").trim().to_string(),
            self.tag(RESULT).to_string(),
        ];
        let parts: Vec<&str> = parts.iter().map(|s| s.as_str()).filter(|s| !s.is_empty() && *s != "?" && *s != "????.??.??").collect();
        parts.join(" · ")
    }
}

/// Bytes as text: UTF-8, else Windows-1252 (old PGN files are Latin-1).
fn text(b: &[u8]) -> std::borrow::Cow<'_, str> {
    match std::str::from_utf8(b) {
        Ok(s) => s.into(),
        Err(_) => encoding_rs::WINDOWS_1252.decode_without_bom_handling(b).0,
    }
}

/// The longest line kept: the rest of a longer one is read past, not kept, so that a file of
/// one endless line cannot fill the memory.
pub const MAX_LINE: usize = 1 << 20;
/// The most of one game read to show it.
pub const MAX_GAME: u32 = 16 << 20;

/// A line of `r` into `line`, at most `MAX_LINE` bytes of it: how many bytes it had, 0 at the end.
fn read_line(r: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<usize> {
    line.clear();
    let mut n = 0;
    loop {
        let buf = match r.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if buf.is_empty() {
            return Ok(n);
        }
        let (take, done) = match memchr::memchr(b'\n', buf) {
            Some(i) => (i + 1, true),
            None => (buf.len(), false),
        };
        let room = MAX_LINE.saturating_sub(line.len());
        line.extend_from_slice(&buf[..take.min(room)]);
        r.consume(take);
        n += take;
        if done {
            return Ok(n);
        }
    }
}

/// The games of `r`, in order, each handed to `each` until it says to stop. Never fails on
/// what it reads: lines that are no PGN are taken for moves, and skipped there.
pub(crate) fn scan(mut r: impl BufRead, stop: &AtomicBool, mut each: impl FnMut(&Head) -> bool) -> io::Result<()> {
    let mut head = Head { offset: 0, len: 0, tags: Default::default(), plies: 0, sans: vec![] };
    let mut line = Vec::new();
    let mut at = 0u64;
    let (mut has_tags, mut has_moves, mut in_brace, mut depth) = (false, false, false, 0u32);
    loop {
        let n = read_line(&mut r, &mut line)?;
        // An unclosed `{` ends at the next game's first tag, at the start of a line.
        if in_brace && line.starts_with(b"[Event \"") {
            in_brace = false;
        }
        let starts = |head: &mut Head| {
            for t in &mut head.tags {
                t.clear();
            }
            head.offset = at;
            head.plies = 0;
            head.sans.clear();
        };
        let trimmed = line.trim_ascii_start();
        let is_tag = n > 0 && !in_brace && trimmed.first() == Some(&b'[');
        // A new game: tags after moves, an Event tag a second time, or the end.
        let new = n == 0 || (is_tag && (has_moves || (trimmed.starts_with(b"[Event ") && !head.tags[EVENT].is_empty())));
        if new && (has_tags || has_moves) {
            head.len = at - head.offset;
            if !each(&head) || stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            (has_tags, has_moves, in_brace, depth) = (false, false, false, 0);
        }
        if n == 0 {
            return Ok(());
        }
        if new || (!has_tags && !has_moves) {
            starts(&mut head);
        }
        if is_tag {
            if let Some((k, v)) = pgn::tag_pair(&text(&line))
                && let Some(i) = WANTED.iter().position(|w| w.eq_ignore_ascii_case(&k))
            {
                head.tags[i] = v;
            }
            has_tags = true;
        } else if !trimmed.is_empty() && (trimmed[0] != b'%' || in_brace) {
            has_moves = true;
            moves(&text(&line), &mut head, &mut in_brace, &mut depth);
        }
        at += n as u64;
    }
}

/// A line of movetext: counts the main line's moves and keeps the first few.
fn moves(line: &str, head: &mut Head, in_brace: &mut bool, depth: &mut u32) {
    let mut s = line;
    while !s.is_empty() {
        if *in_brace {
            match s.find('}') {
                Some(e) => {
                    s = &s[e + 1..];
                    *in_brace = false;
                }
                None => return,
            }
            continue;
        }
        let c = s.chars().next().unwrap_or(' ');
        match c {
            '{' => *in_brace = true,
            ';' => return,
            '(' => *depth += 1,
            ')' => *depth = depth.saturating_sub(1),
            c if c.is_whitespace() => {}
            _ => {
                let end = s.find(|c: char| c.is_whitespace() || "{}();".contains(c)).unwrap_or(s.len()).max(c.len_utf8());
                let w = &s[..end];
                s = &s[end..];
                if *depth == 0
                    && !w.starts_with('$')
                    && let Word::Move(san, _) = pgn::word(w)
                {
                    head.plies += 1;
                    if head.sans.len() < FIRST_MOVES {
                        head.sans.push(san.to_string());
                    }
                }
                continue;
            }
        }
        s = &s[c.len_utf8()..];
    }
}

/// Counts the bytes read through it, for the progress.
struct Counted<R> {
    inner: R,
    read: Arc<AtomicU64>,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// How the file is packed, by its name.
#[derive(Clone, Copy, PartialEq)]
enum Pack {
    None,
    Gz,
    Zst,
}

fn pack(path: &Path) -> Pack {
    let n = path.to_string_lossy().to_ascii_lowercase();
    if n.ends_with(".gz") {
        Pack::Gz
    } else if n.ends_with(".zst") {
        Pack::Zst
    } else {
        Pack::None
    }
}

/// What one file may cost: a file of endless tiny games, or a small packed file that unpacks
/// to terabytes, stops here, with the games read so far and a note saying so.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Games in the table (48 bytes each).
    pub games: usize,
    /// Bytes of the names kept (players, events, openings), with what keeping each costs.
    pub names: usize,
    /// Bytes a packed file is unpacked to, at most.
    pub unpacked: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { games: 10_000_000, names: 256 << 20, unpacked: 64 << 30 }
    }
}

/// The file's text, unpacked as it is read (up to `unpacked` bytes); `read` counts the file's
/// own bytes.
fn reader(path: &Path, read: Arc<AtomicU64>, unpacked: u64) -> io::Result<Box<dyn Read + Send>> {
    let f = BufReader::new(Counted { inner: File::open(path)?, read });
    Ok(match pack(path) {
        Pack::None => Box::new(f),
        Pack::Gz => Box::new(flate2::read::MultiGzDecoder::new(f).take(unpacked)),
        Pack::Zst => Box::new(ruzstd::decoding::StreamingDecoder::new(f).map_err(io::Error::other)?.take(unpacked)),
    })
}

/// The games of the PGN file at `path`, one by one (the search store, and Find's hits).
pub(crate) fn scan_file(path: &Path, each: impl FnMut(&Head) -> bool) -> io::Result<()> {
    let r = reader(path, Arc::default(), Limits::default().unpacked)?;
    scan(BufReader::with_capacity(1 << 16, r), &AtomicBool::new(false), each)
}

/// One game in the table: 48 bytes, so a million games take 48 MB.
#[derive(Clone, Copy)]
struct Rec {
    offset: u64,
    len: u32,
    white: u32,
    black: u32,
    event: u32,
    /// The opening's name, interned; `u32::MAX` for none.
    opening: u32,
    /// yyyymmdd, unknown parts 0.
    date: u32,
    white_elo: u16,
    black_elo: u16,
    /// 'A'..'E' as 1..5 times 100 plus the number, 0 for none.
    eco: u16,
    plies: u16,
    /// 1 White won, 2 Black won, 3 a draw, 0 anything else.
    result: u8,
}

#[derive(Default)]
struct Shared {
    recs: Vec<Rec>,
    strings: Vec<Box<str>>,
    done: bool,
    /// Stopped at `Limits`: the table has the games read until then.
    capped: bool,
    error: Option<String>,
}

fn result_code(r: &str) -> u8 {
    match r {
        "1-0" => 1,
        "0-1" => 2,
        "1/2-1/2" | "½-½" => 3,
        _ => 0,
    }
}

fn result_text(r: u8) -> &'static str {
    ["*", "1-0", "0-1", "½-½"][r as usize & 3]
}

fn date_code(d: &str) -> u32 {
    let mut parts = d.split(['.', '-', '/']).map(|p| p.trim().parse::<u32>().unwrap_or(0));
    let y = parts.next().unwrap_or(0).min(9999);
    let m = parts.next().unwrap_or(0).min(12);
    let day = parts.next().unwrap_or(0).min(31);
    y * 10000 + m * 100 + day
}

fn date_text(d: u32) -> String {
    let (y, m, day) = (d / 10000, d / 100 % 100, d % 100);
    match (y, m, day) {
        (0, _, _) => String::new(),
        (y, 0, _) => format!("{y}"),
        (y, m, 0) => format!("{y}.{m:02}"),
        (y, m, d) => format!("{y}.{m:02}.{d:02}"),
    }
}

fn eco_code(e: &str) -> u16 {
    let b = e.trim().as_bytes();
    if b.len() == 3 && (b'A'..=b'E').contains(&b[0].to_ascii_uppercase()) && b[1].is_ascii_digit() && b[2].is_ascii_digit() {
        (b[0].to_ascii_uppercase() - b'A' + 1) as u16 * 100 + ((b[1] - b'0') * 10 + b[2] - b'0') as u16
    } else {
        0
    }
}

fn eco_text(e: u16) -> String {
    if e == 0 { String::new() } else { format!("{}{:02}", (b'A' + (e / 100) as u8 - 1) as char, e % 100) }
}

/// Where the games come from when one is read: the file, or an unpacker kept where it stopped,
/// so that reading on through the file does not unpack it again from the start.
struct Source {
    path: PathBuf,
    open: Option<(Box<dyn Read + Send>, u64)>,
}

impl Source {
    fn read(&mut self, offset: u64, len: u32) -> io::Result<Vec<u8>> {
        let mut buf = vec![0; len.min(MAX_GAME) as usize];
        if pack(&self.path) == Pack::None {
            let mut f = File::open(&self.path)?;
            f.seek(SeekFrom::Start(offset))?;
            let n = read_full(&mut f, &mut buf)?;
            buf.truncate(n);
            return Ok(buf);
        }
        // ponytail: a packed file is unpacked from its start for a game before the last one
        // read; seek points every few MB would make going back cheap, if it is ever slow.
        if self.open.as_ref().is_none_or(|(_, at)| *at > offset) {
            self.open = Some((reader(&self.path, Arc::default(), Limits::default().unpacked)?, 0));
        }
        let (r, at) = self.open.as_mut().expect("opened above");
        io::copy(&mut r.take(offset - *at), &mut io::sink())?;
        let n = read_full(r, &mut buf)?;
        *at = offset + n as u64;
        buf.truncate(n);
        Ok(buf)
    }
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// How far the table has come.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Progress {
    pub games: usize,
    pub done: bool,
    /// Bytes of the file read, of its size.
    pub read: u64,
    pub size: u64,
    /// Stopped at a limit (`Limits`): only the first `games` games are in the table.
    pub capped: bool,
    pub error: Option<String>,
}

/// What the table shows: which games, in what order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Query {
    /// In White's or Black's name.
    pub player: String,
    /// In the opening's name, or the start of its ECO code.
    pub opening: String,
    /// `1-0`, `0-1`, `1/2-1/2`, or empty for any.
    pub result: String,
    pub from: Option<u16>,
    pub to: Option<u16>,
    /// Both players rated at least this.
    pub elo: Option<u16>,
    pub sort: Sort,
    pub desc: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Number,
    White,
    Black,
    WhiteElo,
    BlackElo,
    Result,
    Date,
    Event,
    Eco,
    Opening,
    Moves,
}

impl Sort {
    pub const ALL: [Sort; 11] = [
        Sort::Number,
        Sort::White,
        Sort::WhiteElo,
        Sort::Black,
        Sort::BlackElo,
        Sort::Result,
        Sort::Date,
        Sort::Event,
        Sort::Eco,
        Sort::Opening,
        Sort::Moves,
    ];
}

/// A game as the table shows it.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Row {
    /// Its place in the file, from 1.
    pub n: u32,
    pub white: String,
    pub black: String,
    pub white_elo: Option<u16>,
    pub black_elo: Option<u16>,
    pub result: String,
    pub date: String,
    pub event: String,
    pub eco: String,
    pub opening: String,
    /// Full moves.
    pub moves: u32,
}

/// The games of one PGN file.
pub struct Library {
    pub path: PathBuf,
    shared: Arc<Mutex<Shared>>,
    read: Arc<AtomicU64>,
    size: u64,
    stop: Arc<AtomicBool>,
    source: Mutex<Source>,
}

impl Drop for Library {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Library {
    /// Starts reading the file's games, on a thread of its own.
    pub fn open(path: &Path) -> io::Result<Library> {
        Library::open_with(path, Limits::default())
    }

    /// `open`, with limits of its own.
    pub fn open_with(path: &Path, limits: Limits) -> io::Result<Library> {
        let size = std::fs::metadata(path)?.len();
        let read = Arc::new(AtomicU64::new(0));
        let r = reader(path, read.clone(), limits.unpacked)?;
        // What the packed file unpacked to: at the limit, the rest of it was left.
        let unpacked = Arc::new(AtomicU64::new(0));
        let packed = pack(path) != Pack::None;
        let r: Box<dyn Read + Send> = Box::new(Counted { inner: r, read: unpacked.clone() });
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (sh, st) = (shared.clone(), stop.clone());
        std::thread::Builder::new().name("chess-games".into()).spawn(move || index(r, &sh, &st, limits, || packed && unpacked.load(Ordering::Relaxed) >= limits.unpacked))?;
        Ok(Library { path: path.to_path_buf(), shared, read, size, stop, source: Mutex::new(Source { path: path.to_path_buf(), open: None }) })
    }

    pub fn progress(&self) -> Progress {
        let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        Progress { games: s.recs.len(), done: s.done, capped: s.capped, read: self.read.load(Ordering::Relaxed).min(self.size), size: self.size, error: s.error.clone() }
    }

    /// The games that `q` lets through, in its order: their indices (from 0).
    pub fn query(&self, q: &Query) -> Vec<u32> {
        let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        let player = q.player.trim().to_lowercase();
        let opening = q.opening.trim().to_lowercase();
        // The interned names that match, looked at once each rather than per game.
        let names = |needle: &str| -> Vec<bool> {
            if needle.is_empty() {
                return vec![];
            }
            s.strings.iter().map(|n| n.to_lowercase().contains(needle)).collect()
        };
        let players = names(&player);
        let openings = names(&opening);
        let result = result_code(q.result.trim());
        let hit = |ids: &Vec<bool>, i: u32| ids.get(i as usize).copied().unwrap_or(false);
        let mut ids: Vec<u32> = (0..s.recs.len() as u32)
            .filter(|&i| {
                let r = &s.recs[i as usize];
                (player.is_empty() || hit(&players, r.white) || hit(&players, r.black))
                    && (opening.is_empty()
                        || hit(&openings, r.opening)
                        || eco_text(r.eco).to_lowercase().starts_with(&opening))
                    && (q.result.trim().is_empty() || r.result == result)
                    && q.from.is_none_or(|y| r.date / 10000 >= y as u32)
                    && q.to.is_none_or(|y| r.date / 10000 <= y as u32 && r.date > 0)
                    && q.elo.is_none_or(|e| r.white_elo.min(r.black_elo) >= e)
            })
            .collect();
        // Names sort by their rank among all names, worked out once.
        let rank = |s: &Shared| -> Vec<u32> {
            let mut order: Vec<u32> = (0..s.strings.len() as u32).collect();
            let lower: Vec<String> = s.strings.iter().map(|n| n.to_lowercase()).collect();
            order.sort_by(|&a, &b| lower[a as usize].cmp(&lower[b as usize]));
            let mut rank = vec![0; order.len()];
            for (r, &i) in order.iter().enumerate() {
                rank[i as usize] = r as u32;
            }
            rank
        };
        type Key = Box<dyn Fn(&Rec) -> u64>;
        let key: Option<Key> = match q.sort {
            Sort::Number => None,
            Sort::White | Sort::Black | Sort::Event | Sort::Opening => {
                let rank = rank(&s);
                let sort = q.sort;
                Some(Box::new(move |r: &Rec| {
                    let id = match sort {
                        Sort::White => r.white,
                        Sort::Black => r.black,
                        Sort::Event => r.event,
                        _ => r.opening,
                    };
                    // No name sorts last.
                    rank.get(id as usize).map_or(u64::MAX, |&k| k as u64)
                }))
            }
            Sort::WhiteElo => Some(Box::new(|r: &Rec| r.white_elo as u64)),
            Sort::BlackElo => Some(Box::new(|r: &Rec| r.black_elo as u64)),
            Sort::Result => Some(Box::new(|r: &Rec| r.result as u64)),
            Sort::Date => Some(Box::new(|r: &Rec| r.date as u64)),
            Sort::Eco => Some(Box::new(|r: &Rec| r.eco as u64)),
            Sort::Moves => Some(Box::new(|r: &Rec| r.plies as u64)),
        };
        if let Some(key) = key {
            ids.sort_by_key(|&i| key(&s.recs[i as usize]));
        }
        if q.desc {
            ids.reverse();
        }
        ids
    }

    /// The game at index `i`, as the table shows it.
    pub fn row(&self, i: u32) -> Option<Row> {
        let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        let r = s.recs.get(i as usize)?;
        let name = |id: u32| s.strings.get(id as usize).map_or(String::new(), |n| n.to_string());
        Some(Row {
            n: i + 1,
            white: name(r.white),
            black: name(r.black),
            white_elo: (r.white_elo > 0).then_some(r.white_elo),
            black_elo: (r.black_elo > 0).then_some(r.black_elo),
            result: result_text(r.result).into(),
            date: date_text(r.date),
            event: name(r.event),
            eco: eco_text(r.eco),
            opening: name(r.opening),
            moves: (r.plies as u32).div_ceil(2),
        })
    }

    /// The text of the game at index `i`, read now.
    pub fn text(&self, i: u32) -> io::Result<String> {
        let (offset, len) = {
            let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            let r = s.recs.get(i as usize).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such game"))?;
            (r.offset, r.len)
        };
        let bytes = self.source.lock().unwrap_or_else(|e| e.into_inner()).read(offset, len)?;
        Ok(text(&bytes).into_owned())
    }

    /// The game at index `i`, read and played out.
    pub fn game(&self, i: u32) -> io::Result<pgn::Game> {
        Ok(pgn::parse(&self.text(i)?))
    }
}

/// The pass over the games: their records, handed over in batches so the table fills.
/// `cut`: the packed file was unpacked as far as the limits allow.
fn index(r: Box<dyn Read + Send>, shared: &Mutex<Shared>, stop: &AtomicBool, limits: Limits, cut: impl Fn() -> bool) {
    // What the names cost so far: their bytes, twice (the list and the lookup), and the rest.
    let mut names = 0usize;
    let (mut games, mut capped) = (0usize, false);
    let mut ids: HashMap<Box<str>, u32> = HashMap::new();
    let mut fresh: Vec<Box<str>> = vec![];
    let mut batch: Vec<Rec> = vec![];
    let mut base = 0u32;
    let intern = |s: &str, ids: &mut HashMap<Box<str>, u32>, fresh: &mut Vec<Box<str>>, base: u32, names: &mut usize| -> u32 {
        let s = s.trim();
        if s.is_empty() || s == "?" {
            return u32::MAX;
        }
        if let Some(&id) = ids.get(s) {
            return id;
        }
        let id = base + fresh.len() as u32;
        *names += 2 * s.len() + 64;
        ids.insert(s.into(), id);
        fresh.push(s.into());
        id
    };
    let flush = |batch: &mut Vec<Rec>, fresh: &mut Vec<Box<str>>, base: &mut u32| {
        let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
        *base += fresh.len() as u32;
        s.strings.append(fresh);
        s.recs.append(batch);
    };
    let mut last = std::time::Instant::now();
    let res = scan(BufReader::with_capacity(1 << 16, r), stop, |h| {
        if games >= limits.games || names >= limits.names {
            capped = true;
            return false;
        }
        games += 1;
        let (eco, opening) = h.opening();
        let rec = Rec {
            offset: h.offset,
            len: h.len.min(MAX_GAME as u64) as u32,
            white: intern(h.tag(WHITE), &mut ids, &mut fresh, base, &mut names),
            black: intern(h.tag(BLACK), &mut ids, &mut fresh, base, &mut names),
            event: intern(h.tag(EVENT), &mut ids, &mut fresh, base, &mut names),
            opening: intern(&opening, &mut ids, &mut fresh, base, &mut names),
            date: date_code(h.tag(DATE)),
            white_elo: h.tag(WHITE_ELO).parse().unwrap_or(0),
            black_elo: h.tag(BLACK_ELO).parse().unwrap_or(0),
            eco: eco_code(&eco),
            plies: h.plies.min(u16::MAX as u32) as u16,
            result: result_code(h.tag(RESULT)),
        };
        batch.push(rec);
        if batch.len() >= 4096 && last.elapsed().as_millis() >= 200 {
            flush(&mut batch, &mut fresh, &mut base);
            last = std::time::Instant::now();
        }
        true
    });
    flush(&mut batch, &mut fresh, &mut base);
    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
    s.done = true;
    s.capped = capped || cut();
    if let Err(e) = res {
        s.error = Some(e.to_string());
    }
}

/// The search store's text of a PGN file: a line per game (`Head::line`), up to `max` bytes.
pub fn search_text(path: &Path, max: usize) -> Option<String> {
    let mut out = String::new();
    scan_file(path, |h| {
        let line = h.line();
        if out.len() + line.len() + 1 > max {
            return false;
        }
        out.push_str(&line);
        out.push('\n');
        true
    })
    .ok()?;
    (!out.is_empty()).then_some(out)
}

/// Words as the search store's index splits them: letters and digits, lower case, the
/// characters a reader drops left out. With each, whether it is between `store::MARK`s.
fn words(text: &str) -> Vec<(String, bool)> {
    let (open, close) = crate::store::MARK;
    let mut out = vec![];
    let (mut word, mut marked, mut inside) = (String::new(), false, false);
    let end = |word: &mut String, marked: &mut bool, out: &mut Vec<(String, bool)>| {
        if !word.is_empty() {
            out.push((std::mem::take(word), *marked));
        }
        *marked = false;
    };
    for c in text.chars() {
        match c {
            c if c == open => inside = true,
            c if c == close => inside = false,
            '\u{AD}' | '\u{200B}' | '\u{2060}' | '\u{FEFF}' => {}
            c if c.is_alphanumeric() => {
                word.extend(c.to_lowercase());
                marked |= inside;
            }
            _ => end(&mut word, &mut marked, &mut out),
        }
    }
    end(&mut word, &mut marked, &mut out);
    out
}

/// The game a Find hit in a PGN file is about, by its snippet: its index, from 0.
///
/// The store keeps a line per game (`search_text`) and its snippet is a run of words around
/// the ones found, often across the lines of neighbouring games, its line breaks gone. So the
/// snippet's words are looked for as a run in the file's games, one after the other; the game
/// of the first word found is the one. When no run matches (a reader that changed the text),
/// the game whose line has the found words and most of the others.
pub fn game_of_hit(path: &Path, snippet: &str) -> Option<u32> {
    let want = words(snippet);
    if want.is_empty() {
        return None;
    }
    let first = want.iter().position(|w| w.1).unwrap_or(0);
    let found: Vec<&str> = want.iter().filter(|w| w.1).map(|w| w.0.as_str()).collect();
    // The last words of the file, each with its game, as many as the snippet has.
    let mut window: std::collections::VecDeque<(String, u32)> = std::collections::VecDeque::new();
    let (mut n, mut hit, mut best) = (0u32, None, (0usize, None));
    scan_file(path, |h| {
        let line: Vec<String> = words(&h.line()).into_iter().map(|w| w.0).collect();
        // The fallback's score: the found words all here, then as many of the others.
        if found.iter().all(|f| line.iter().any(|w| w == f)) {
            let score = 1 + want.iter().filter(|w| line.contains(&w.0)).count();
            if score > best.0 {
                best = (score, Some(n));
            }
        }
        for w in line {
            window.push_back((w, n));
            if window.len() > want.len() {
                window.pop_front();
            }
            if window.len() == want.len() && window.iter().zip(&want).all(|(a, b)| a.0 == b.0) {
                hit = Some(window[first].1);
                return false;
            }
        }
        n += 1;
        true
    })
    .ok()?;
    hit.or(best.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const TWO: &str = "[Event \"Paris\"]\n[White \"Morphy, Paul\"]\n[Black \"Duke Karl\"]\n[Result \"1-0\"]\n[Date \"1858.11.02\"]\n[WhiteElo \"2690\"]\n\n1.e4 e5 2.Nf3 d6 {Philidor} 3.d4 Bg4 (3...exd4) 4.dxe5 1-0\n\n[Event \"London\"]\n[White \"Anderssen, Adolf\"]\n[Black \"Kieseritzky, Lionel\"]\n[Result \"1-0\"]\n[Date \"1851.06.21\"]\n[ECO \"C33\"]\n\n1.e4 e5 2.f4 exf4 {a\n[not a tag] } 3.Bc4 1-0\n";

    fn lib(text: &[u8], name: &str) -> (Library, tempdir::Dir) {
        let d = tempdir::Dir::new();
        let p = d.0.join(name);
        std::fs::File::create(&p).unwrap().write_all(text).unwrap();
        let l = Library::open(&p).unwrap();
        while !l.progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        (l, d)
    }

    mod tempdir {
        pub struct Dir(pub std::path::PathBuf);
        impl Dir {
            pub fn new() -> Dir {
                static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let p = std::env::temp_dir().join(format!("coxswain-chess-{}-{n}", std::process::id()));
                std::fs::create_dir_all(&p).unwrap();
                Dir(p)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn a_table_of_games() {
        let (l, _d) = lib(TWO.as_bytes(), "two.pgn");
        assert_eq!(l.progress().games, 2);
        let r = l.row(0).unwrap();
        assert_eq!((r.white.as_str(), r.white_elo, r.result.as_str(), r.date.as_str(), r.moves), ("Morphy, Paul", Some(2690), "1-0", "1858.11.02", 4));
        // The opening by the moves: Philidor (C41); the second by its own ECO code.
        assert_eq!((r.eco.as_str(), r.opening.as_str()), ("C41", "Philidor Defense"));
        let r = l.row(1).unwrap();
        assert_eq!((r.eco.as_str(), r.moves), ("C33", 3));
        assert_eq!(r.opening, "King's Gambit Accepted: Bishop's Gambit");
        // A tag-like line inside a comment is no new game.
        assert!(l.text(1).unwrap().contains("[not a tag]"));
        assert_eq!(l.game(0).unwrap().main_line().len(), 7);
        // Filters and sorting.
        let q = |q: Query| l.query(&q);
        assert_eq!(q(Query { player: "kiesERitz".into(), ..Query::default() }), vec![1]);
        assert_eq!(q(Query { opening: "c4".into(), ..Query::default() }), vec![0]);
        assert_eq!(q(Query { opening: "C".into(), ..Query::default() }), vec![0, 1]);
        assert_eq!(q(Query { opening: "philidor".into(), ..Query::default() }), vec![0]);
        assert_eq!(q(Query { from: Some(1855), ..Query::default() }), vec![0]);
        assert_eq!(q(Query { to: Some(1855), ..Query::default() }), vec![1]);
        assert_eq!(q(Query { elo: Some(2000), ..Query::default() }), Vec::<u32>::new());
        assert_eq!(q(Query { sort: Sort::White, ..Query::default() }), vec![1, 0]);
        assert_eq!(q(Query { sort: Sort::Date, desc: true, ..Query::default() }), vec![0, 1]);
        assert_eq!(q(Query { result: "0-1".into(), ..Query::default() }), Vec::<u32>::new());
    }

    #[test]
    fn packed_files_and_hits() {
        let mut gz = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
        gz.write_all(TWO.as_bytes()).unwrap();
        let (l, d) = lib(&gz.finish().unwrap(), "two.pgn.gz");
        assert_eq!(l.progress().games, 2);
        // Read back to front: the unpacker starts again for the earlier one.
        assert!(l.text(1).unwrap().contains("Anderssen"));
        assert!(l.text(0).unwrap().contains("Morphy"));
        let p = d.0.join("two.pgn.gz");
        let text = search_text(&p, 1 << 20).unwrap();
        assert!(text.contains("Anderssen, Adolf – Kieseritzky, Lionel · London · 1851.06.21 · C33 King's Gambit"), "{text}");
        let snippet = format!("…Kieseritzky, Lionel · {}London{} · 1851.06.21…", crate::store::MARK.0, crate::store::MARK.1);
        assert_eq!(game_of_hit(&p, &snippet), Some(1));
        // A PGN file bigger than [search] text_max_size is read too: its reader stops by itself.
        let size = std::fs::metadata(&p).unwrap().len();
        assert!(crate::extract::text_of(&p, size, 1).is_some_and(|t| t.contains("Kieseritzky")));
        // Across two games: the one with the word found.
        let (o, c) = crate::store::MARK;
        let snippet = format!("1-0 Anderssen, Adolf – Kieseritzky, Lionel · {o}London{c}");
        assert_eq!(game_of_hit(&p, &snippet), Some(1));
        let snippet = format!("…{o}Philidor{c} Defense · 1-0 Anderssen, Adolf");
        assert_eq!(game_of_hit(&p, &snippet), Some(0));
    }

    /// A packed file of a few MB can unpack to an endless line: what is kept stays small.
    #[test]
    fn endless_lines_stay_small() {
        let mut gz = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
        gz.write_all(b"[White \"").unwrap();
        let chunk = vec![b'x'; 1 << 20];
        for _ in 0..20 {
            gz.write_all(&chunk).unwrap();
        }
        gz.write_all(b"\"]\n\n1.e4 ").unwrap();
        for _ in 0..20 {
            gz.write_all(&chunk).unwrap();
        }
        gz.write_all(b" *\n\n[White \"B\"]\n\n1.d4 *\n").unwrap();
        let (l, _d) = lib(&gz.finish().unwrap(), "huge.pgn.gz");
        assert_eq!(l.progress().games, 2);
        assert!(l.row(0).unwrap().white.len() <= pgn::MAX_TAG);
        assert!(l.text(0).unwrap().len() <= MAX_GAME as usize);
        assert_eq!(l.row(1).unwrap().white, "B");
        let mut line = vec![];
        let mut r = io::Cursor::new(vec![b'y'; 3 * MAX_LINE]);
        assert_eq!(read_line(&mut r, &mut line).unwrap(), 3 * MAX_LINE);
        assert_eq!(line.len(), MAX_LINE);
    }

    /// A small packed file of endless tiny games stops at the limits, with what it read.
    #[test]
    fn limits_stop_endless_games() {
        let mut zst = vec![];
        let mut gz = flate2::write::GzEncoder::new(&mut zst, flate2::Compression::fast());
        for i in 0..50_000 {
            write!(gz, "[White \"p{i}\"]\n1.e4 *\n").unwrap();
        }
        gz.finish().unwrap();
        let d = tempdir::Dir::new();
        let p = d.0.join("tiny.pgn.gz");
        std::fs::write(&p, &zst).unwrap();
        let wait = |l: &Library| {
            while !l.progress().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            l.progress()
        };
        let full = Limits::default();
        let p1 = wait(&Library::open_with(&p, Limits { games: 1000, ..full }).unwrap());
        assert_eq!((p1.games, p1.capped), (1000, true));
        let p2 = wait(&Library::open_with(&p, Limits { names: 64 * 1024, ..full }).unwrap());
        assert!(p2.capped && p2.games < 1000, "{p2:?}");
        // Unpacked no further than allowed: about 20 bytes a game.
        let p3 = wait(&Library::open_with(&p, Limits { unpacked: 20_000, ..full }).unwrap());
        assert!(p3.games < 1100 && p3.capped, "{p3:?}");
        let p4 = wait(&Library::open(&p).unwrap());
        assert_eq!((p4.games, p4.capped), (50_000, false));
    }

    #[test]
    fn whatever_the_bytes() {
        // An unclosed comment ends where the next game's tags begin.
        let (l, _d) = lib(b"[Event \"x\"]\n\n1.e4 { never closed\n\n[Event \"y\"]\n\n1.d4 *\n", "brace.pgn");
        assert_eq!(l.progress().games, 2);
        drop(l);
        for text in [&b""[..], b"\n\n", b"[", b"[Event \"x\"]", b"1.e4", b"\xff\xfe[White \"\xe9\"]\n1.e4 *", b"{ never closed\n[Event \"y\"]\n1.d4"] {
            let (l, _d) = lib(text, "x.pgn");
            for i in 0..l.progress().games as u32 {
                let _ = l.game(i);
            }
        }
        // Latin-1 names come out as text.
        let (l, _d) = lib(b"[White \"Bj\xf6rn\"]\n\n1.e4 *\n", "latin.pgn");
        assert_eq!(l.row(0).unwrap().white, "Björn");
    }

    /// A big file, timed: `COXSWAIN_CHESS_BIG=/path/to/million.pgn cargo test --release -p
    /// coxswain-core big_file -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn big_file() {
        let Ok(p) = std::env::var("COXSWAIN_CHESS_BIG") else { return };
        let t = std::time::Instant::now();
        let l = Library::open(Path::new(&p)).unwrap();
        let mut first = None;
        while !l.progress().done {
            if first.is_none() && l.progress().games > 0 {
                first = Some(t.elapsed());
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let read = t.elapsed();
        let n = l.progress().games;
        let rss = std::fs::read_to_string("/proc/self/status").unwrap_or_default().lines().find(|l| l.starts_with("VmRSS")).unwrap_or("").to_string();
        let time = |q: Query| {
            let t = std::time::Instant::now();
            let ids = l.query(&q);
            (ids.len(), t.elapsed())
        };
        println!("{n} games: first rows after {first:?}, all read in {read:?}; {rss}");
        println!("all: {:?}", time(Query::default()));
        println!("player 'carlsen': {:?}", time(Query { player: "carlsen".into(), ..Query::default() }));
        println!("opening 'gambit' + 1-0 + 1900-1950 + elo 2400: {:?}", time(Query { opening: "gambit".into(), result: "1-0".into(), from: Some(1900), to: Some(1950), elo: Some(2400), ..Query::default() }));
        println!("sort by white: {:?}", time(Query { sort: Sort::White, ..Query::default() }));
        println!("sort by date desc: {:?}", time(Query { sort: Sort::Date, desc: true, ..Query::default() }));
        for i in [0, n as u32 / 2, n as u32 - 1, 10].into_iter().filter(|&i| (i as usize) < n) {
            let t = std::time::Instant::now();
            let g = l.game(i).unwrap();
            println!("game {i}: {} moves, {} nodes, error {:?}, in {:?}", g.main_line().len(), g.nodes.len(), g.error, t.elapsed());
        }
    }
}

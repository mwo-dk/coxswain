//! A games table's position index (docs/previews/chess-positions.md), opt-in: every position the
//! games reach in their first moves (15, 30, or the whole game), each with the games that reach
//! it and the move each played there. From it the opening tree: for the position on the board,
//! each next move with its games, White's wins, draws and Black's wins, the average Elo and the
//! best-rated player of the side making it, and the first and last year it was played; and the
//! games list cut to the games that reach the position.
//!
//! A position is its Zobrist key (`polyglot::key`: the pieces, the side to move, castling and an
//! en passant capture that can be made), so move orders that meet are one position, and the same
//! pieces with the other side to move are another. Xiangqi games (DhtmlXQ, a PGN that says so)
//! are keyed by their position the same way (the board and the side to move).
//!
//! Kept in the cache's `positions/`, one folder a database (`<path's hash>-<run>-<n>`), outside
//! the games tables' 4 GB (`table::MOST`) and never pruned: it goes when it is removed (the
//! games table's button, Disk use), and when its database is gone. A folder is whole once its
//! `done` is there, written last. Its files:
//!
//! - `postings`: every (position, game, move) once, sorted, in blocks of `BLOCK` entries, each
//!   entry a few bytes (`Writer`): a game that goes on reaching known positions costs little.
//! - `blocks`: each block's first position and its place in `postings`, read whole when the
//!   index is opened (16 bytes a block).
//! - `tree`: for each position that `HOT` entries or more reach, each move's sums (`Sums`), in
//!   fixed records sorted by position: the start position's tree is read, not counted again.
//!   A rarer position's tree is counted from its few games' rows in the games table.
//!
//! Built from the games table (`Library`), one code path for every format: one thread reads the
//! games' text in order (several, each a run of games, when any game can be read at its place:
//! SCID, ChessBase, a PGN kept in blocks), others play out their main lines; what they find is sorted in runs of
//! `RUN` entries on the disk and merged, so the memory stays the same for any database (the
//! runs, and 8 bytes a game for the sums). When the database changes, a new index is built in
//! the background while the old one is still used, marked out of date; a PGN file that only
//! grew keeps its old entries and reads only the games added.

use std::collections::{BinaryHeap, HashMap};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cozy_chess::{Board, Color, Move, Piece, Square};
use serde::{Deserialize, Serialize};

use super::games::Library;
use super::lex::{Movetext, Tok};
use super::pgn::{self, Rules, Word};
use super::table::Rec;

/// What the folder holds changes form: an index of another form is not read, and is built again.
const FORM: u32 = 1;
/// Entries a block of `postings`.
const BLOCK: usize = 4096;
/// A position this many entries reach has its tree kept in `tree`.
const HOT: usize = 32;
/// Entries a thread sorts at a time before they go to the disk: 16 bytes each.
const RUN: usize = 1 << 21;
/// Runs merged at once (files open together).
const FAN: usize = 96;
/// Games read into one batch for the threads that play them out, at most.
const BATCH: usize = 256;
/// A batch is closed once its games' text is this long.
const BATCH_BYTES: usize = 8 << 20;
/// Games one reader takes at a time, when several read (`Library::reads_anywhere`): a few
/// blocks of a PGN kept in blocks, so a block is unpacked about once.
const CHUNK: u64 = if cfg!(test) { 512 } else { 16 * BATCH as u64 };
/// The move code of a game that ends there (or where the index stops): it reaches the position.
const END: u16 = 0x7FFE;
/// The code of the sums of all games reaching a position, in `tree`.
const REACH: u16 = 0xFFFF;
/// The side bit of a move code: Black's (or in xiangqi Black's) move.
const BLACK: u16 = 0x8000;
/// A `tree` record's bytes.
const SUMS: usize = 48;
/// About this many bytes a position entry takes on the disk, with the trees: for the estimate
/// (8.9 on a database of 12 million games at 30 moves).
const BYTES_PER_ENTRY: u64 = 9;
/// Entries one thread plays out and sorts a second, about: for the estimate (the whole build,
/// merging too, on a 16-core machine).
const ENTRIES_PER_SECOND: u64 = 400_000;

/// The cache's `positions/`.
pub fn folder() -> Option<PathBuf> {
    Some(crate::helper::folder()?.join("positions"))
}

/// Half-moves the index takes of each game at `depth` moves; 0 is the whole game.
fn plies(depth: u32) -> usize {
    if depth == 0 { usize::MAX } else { depth as usize * 2 }
}

/// The depth `[chess] position_depth` names, in moves: `15`, `30` (the default), `all` (0, the
/// whole game).
pub fn depth_of(setting: &str) -> u32 {
    match setting.trim() {
        "all" => 0,
        s => s.parse::<u32>().ok().filter(|&d| d > 0).unwrap_or(30),
    }
}

// ---------------------------------------------------------------- the games' positions

/// How a kind of game is played out for the index.
trait Kind {
    type B: Clone;
    fn start(&self, fen: Option<&str>) -> Option<Self::B>;
    fn word<'a>(&self, w: &'a str) -> Word<'a>;
    /// The move `san` played on `b`: the board after it and the move's code (no side bit).
    fn step(&self, b: &Self::B, san: &str) -> Option<(Self::B, u16)>;
    fn key(&self, b: &Self::B) -> u64;
    fn black(&self, b: &Self::B) -> bool;
    /// The move `code` on `b`, as it is shown and played: SAN, UCI, the position after it.
    fn shown(&self, b: &Self::B, code: u16) -> Option<(String, String, String)>;
}

struct Chess;

fn chess_code(m: Move) -> u16 {
    let promo = match m.promotion {
        None => 0,
        Some(Piece::Knight) => 1,
        Some(Piece::Bishop) => 2,
        Some(Piece::Rook) => 3,
        Some(_) => 4,
    };
    m.from as u16 | (m.to as u16) << 6 | promo << 12
}

impl Kind for Chess {
    type B = Board;

    fn start(&self, fen: Option<&str>) -> Option<Board> {
        match fen {
            Some(f) => pgn::board(f),
            None => Some(Board::default()),
        }
    }

    fn word<'a>(&self, w: &'a str) -> Word<'a> {
        pgn::word(w)
    }

    fn step(&self, b: &Board, san: &str) -> Option<(Board, u16)> {
        let m = pgn::san_move(b, san)?;
        let mut nb = b.clone();
        nb.play_unchecked(m);
        Some((nb, chess_code(m)))
    }

    fn key(&self, b: &Board) -> u64 {
        super::polyglot::key(b)
    }

    fn black(&self, b: &Board) -> bool {
        b.side_to_move() == Color::Black
    }

    fn shown(&self, b: &Board, code: u16) -> Option<(String, String, String)> {
        let promotion = match (code >> 12) & 7 {
            0 => None,
            1 => Some(Piece::Knight),
            2 => Some(Piece::Bishop),
            3 => Some(Piece::Rook),
            4 => Some(Piece::Queen),
            _ => return None,
        };
        let m = Move { from: Square::index((code & 63) as usize), to: Square::index(((code >> 6) & 63) as usize), promotion };
        if !b.is_legal(m) {
            return None;
        }
        let mut after = b.clone();
        after.play_unchecked(m);
        Some((pgn::san(b, m), super::polyglot::uci(b, m), pgn::fen(&after)))
    }
}

struct Xiangqi;

/// A xiangqi cell (`x + y × 9`, rows from Black's side) in ICCS: `h2`.
fn iccs(cell: u16) -> String {
    format!("{}{}", (b'a' + (cell % 9) as u8) as char, 9 - cell / 9)
}

impl Kind for Xiangqi {
    type B = (<crate::xiangqi::Xiangqi as Rules>::Board, String);

    fn start(&self, fen: Option<&str>) -> Option<Self::B> {
        let tags: Vec<(String, String)> = fen.map(|f| ("FEN".to_string(), f.to_string())).into_iter().collect();
        Some(crate::xiangqi::Xiangqi.start(&tags, ""))
    }

    fn word<'a>(&self, w: &'a str) -> Word<'a> {
        crate::xiangqi::Xiangqi.word(w)
    }

    fn step(&self, b: &Self::B, san: &str) -> Option<(Self::B, u16)> {
        let (nb, fen, from, to) = crate::xiangqi::Xiangqi.play(&b.0, san)?;
        Some(((nb, fen), from? as u16 | (to? as u16) << 7))
    }

    fn key(&self, b: &Self::B) -> u64 {
        let h = blake3::hash(b.1.as_bytes());
        u64::from_le_bytes(h.as_bytes()[..8].try_into().expect("8 bytes"))
    }

    fn black(&self, b: &Self::B) -> bool {
        b.1.ends_with(" b")
    }

    fn shown(&self, b: &Self::B, code: u16) -> Option<(String, String, String)> {
        let (from, to) = (code & 127, (code >> 7) & 127);
        if from >= 90 || to >= 90 {
            return None;
        }
        let mv = format!("{}{}", iccs(from), iccs(to));
        let (_, fen, ..) = crate::xiangqi::Xiangqi.play(&b.0, &mv)?;
        Some((mv.clone(), mv, fen))
    }
}

/// The positions of the game `text`'s main line, up to `plies` half-moves: each with the move
/// played there (its side bit set for Black's), then the last one reached with `END`.
fn walk<K: Kind>(k: &K, text: &str, plies: usize, mut each: impl FnMut(u64, u16)) {
    let mut rest = text;
    let mut fen = None;
    loop {
        let t = rest.trim_start_matches('\u{feff}').trim_start();
        if !t.starts_with('[') {
            rest = t;
            break;
        }
        let end = t.find('\n').unwrap_or(t.len());
        if let Some((name, value)) = pgn::tag_pair(&t[..end])
            && name.eq_ignore_ascii_case("FEN")
        {
            fen = Some(value);
        }
        rest = &t[end..];
    }
    let Some(mut b) = k.start(fen.as_deref()) else { return };
    let mut depth = 0u32;
    let mut played = 0;
    for tok in Movetext::new(rest, false) {
        match tok {
            Tok::Open => depth += 1,
            Tok::Close => depth = depth.saturating_sub(1),
            Tok::Word(w) if depth == 0 => {
                let Word::Move(san, _) = k.word(w) else { continue };
                if played >= plies {
                    break;
                }
                let Some((nb, code)) = k.step(&b, san) else { break };
                each(k.key(&b), code | if k.black(&b) { BLACK } else { 0 });
                b = nb;
                played += 1;
            }
            _ => {}
        }
    }
    each(k.key(&b), END | if k.black(&b) { BLACK } else { 0 });
}

/// What the index needs of a game's row: the Elos, the year, the result.
#[derive(Clone, Copy, Default)]
struct Info {
    white_elo: u16,
    black_elo: u16,
    year: u16,
    /// 1 White won, 2 Black won, 3 a draw (`Rec::result`).
    result: u8,
}

impl Info {
    fn of(r: &Rec) -> Info {
        Info { white_elo: r.white_elo, black_elo: r.black_elo, year: (r.date / 10000).min(u16::MAX as u32) as u16, result: r.result }
    }
}

/// A move's sums at a position, or the position's own (`REACH`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Sums {
    code: u16,
    games: u32,
    white: u32,
    draws: u32,
    black: u32,
    elo_sum: u64,
    elo_n: u32,
    first: u16,
    last: u16,
    best_elo: u16,
    /// The game of the best-rated player making the move, `u32::MAX` for none.
    best_game: u32,
}

impl Sums {
    fn new(code: u16) -> Sums {
        Sums { code, best_game: u32::MAX, ..Sums::default() }
    }

    /// Game `g` played the move: its result and year, and the Elo of the side making the move.
    fn add(&mut self, g: u32, i: &Info) {
        self.games += 1;
        match i.result {
            1 => self.white += 1,
            2 => self.black += 1,
            3 => self.draws += 1,
            _ => {}
        }
        if i.year > 0 {
            self.first = if self.first == 0 { i.year } else { self.first.min(i.year) };
            self.last = self.last.max(i.year);
        }
        let elo = if self.code & BLACK != 0 { i.black_elo } else { i.white_elo };
        if elo > 0 {
            self.elo_sum += elo as u64;
            self.elo_n += 1;
            if elo > self.best_elo {
                (self.best_elo, self.best_game) = (elo, g);
            }
        }
    }

    fn put(&self, key: u64, b: &mut [u8; SUMS]) {
        b[..8].copy_from_slice(&key.to_le_bytes());
        b[8..10].copy_from_slice(&self.code.to_le_bytes());
        for (i, v) in [self.games, self.white, self.draws, self.black].into_iter().enumerate() {
            b[10 + 4 * i..14 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        b[26..34].copy_from_slice(&self.elo_sum.to_le_bytes());
        b[34..38].copy_from_slice(&self.elo_n.to_le_bytes());
        b[38..40].copy_from_slice(&self.first.to_le_bytes());
        b[40..42].copy_from_slice(&self.last.to_le_bytes());
        b[42..44].copy_from_slice(&self.best_elo.to_le_bytes());
        b[44..48].copy_from_slice(&self.best_game.to_le_bytes());
    }

    fn get(b: &[u8; SUMS]) -> (u64, Sums) {
        let u32_at = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().expect("4 bytes"));
        let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let key = u64::from_le_bytes(b[..8].try_into().expect("8 bytes"));
        let s = Sums {
            code: u16_at(8),
            games: u32_at(10),
            white: u32_at(14),
            draws: u32_at(18),
            black: u32_at(22),
            elo_sum: u64::from_le_bytes(b[26..34].try_into().expect("8 bytes")),
            elo_n: u32_at(34),
            first: u16_at(38),
            last: u16_at(40),
            best_elo: u16_at(42),
            best_game: u32_at(44),
        };
        (key, s)
    }
}

// ---------------------------------------------------------------- entries on the disk

/// An entry: position, game, move code, sorted in that order.
type Entry = u128;

fn entry(key: u64, game: u32, code: u16) -> Entry {
    (key as u128) << 64 | (game as u128) << 16 | code as u128
}

fn parts(e: Entry) -> (u64, u32, u16) {
    ((e >> 64) as u64, (e >> 16) as u32, e as u16)
}

fn put_var(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn get_var(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let c = *b.get(*at)?;
        *at += 1;
        v |= ((c & 0x7F) as u64) << shift;
        if c < 0x80 {
            return Some(v);
        }
    }
    None
}

/// Sorted entries written in blocks: a block is its byte length and its count (u32 each), then
/// its entries, each from the one before it (the first from zero): the position's distance as a
/// varint; the game's distance when the position is the same, else the game, as a varint; the
/// move's code (u16). The same entry twice is written once.
struct Writer<W: Write> {
    w: W,
    buf: Vec<u8>,
    n: usize,
    last: Option<Entry>,
    at: u64,
    /// Each block's first position and place.
    blocks: Vec<(u64, u64)>,
    entries: u64,
}

impl<W: Write> Writer<W> {
    fn new(w: W) -> Writer<W> {
        Writer { w, buf: Vec::with_capacity(64 << 10), n: 0, last: None, at: 0, blocks: vec![], entries: 0 }
    }

    fn push(&mut self, e: Entry) -> io::Result<()> {
        if self.last == Some(e) {
            return Ok(());
        }
        let (key, game, code) = parts(e);
        let (lk, lg) = match self.last {
            Some(l) if self.n > 0 => {
                let (k, g, _) = parts(l);
                (k, g)
            }
            _ => {
                self.blocks.push((key, self.at));
                (0, 0)
            }
        };
        debug_assert!(self.last.is_none_or(|l| l < e), "entries in order");
        put_var(&mut self.buf, key - lk);
        put_var(&mut self.buf, if key == lk && self.n > 0 { (game - lg) as u64 } else { game as u64 });
        self.buf.extend_from_slice(&code.to_le_bytes());
        self.last = Some(e);
        self.n += 1;
        self.entries += 1;
        if self.n == BLOCK {
            self.block()?;
        }
        Ok(())
    }

    fn block(&mut self) -> io::Result<()> {
        if self.n == 0 {
            return Ok(());
        }
        self.w.write_all(&(self.buf.len() as u32).to_le_bytes())?;
        self.w.write_all(&(self.n as u32).to_le_bytes())?;
        self.w.write_all(&self.buf)?;
        self.at += 8 + self.buf.len() as u64;
        self.buf.clear();
        self.n = 0;
        Ok(())
    }

    fn finish(mut self) -> io::Result<(W, Vec<(u64, u64)>, u64)> {
        self.block()?;
        self.w.flush()?;
        Ok((self.w, self.blocks, self.entries))
    }
}

/// A block's entries, in order, to `each` until it says stop: false then.
fn decode(b: &[u8], count: usize, mut each: impl FnMut(Entry) -> bool) -> io::Result<bool> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "a damaged position index");
    let (mut at, mut key, mut game) = (0usize, 0u64, 0u32);
    for i in 0..count {
        let dk = get_var(b, &mut at).ok_or_else(bad)?;
        let g = get_var(b, &mut at).ok_or_else(bad)?;
        key = key.checked_add(dk).ok_or_else(bad)?;
        game = if dk == 0 && i > 0 { game.checked_add(u32::try_from(g).map_err(|_| bad())?).ok_or_else(bad)? } else { u32::try_from(g).map_err(|_| bad())? };
        let code = u16::from_le_bytes(b.get(at..at + 2).ok_or_else(bad)?.try_into().expect("2 bytes"));
        at += 2;
        if !each(entry(key, game, code)) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Reads a block at the reader's place: its entries; none at the end.
fn read_block(r: &mut impl Read, buf: &mut Vec<u8>, out: &mut Vec<Entry>) -> io::Result<bool> {
    let mut head = [0u8; 8];
    match r.read_exact(&mut head) {
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
        r => r?,
    }
    let len = u32::from_le_bytes(head[..4].try_into().expect("4 bytes")) as usize;
    let count = u32::from_le_bytes(head[4..].try_into().expect("4 bytes")) as usize;
    if count == 0 || count > BLOCK || len > BLOCK * 22 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "a damaged position index"));
    }
    buf.resize(len, 0);
    r.read_exact(buf)?;
    out.clear();
    decode(buf, count, |e| {
        out.push(e);
        true
    })?;
    Ok(true)
}

/// A file of blocks read through in order.
struct Stream {
    r: BufReader<File>,
    buf: Vec<u8>,
    block: Vec<Entry>,
    at: usize,
}

impl Stream {
    fn open(p: &Path) -> io::Result<Stream> {
        Ok(Stream { r: BufReader::with_capacity(64 << 10, File::open(p)?), buf: vec![], block: vec![], at: 0 })
    }

    fn next(&mut self) -> io::Result<Option<Entry>> {
        if self.at == self.block.len() {
            if !read_block(&mut self.r, &mut self.buf, &mut self.block)? {
                return Ok(None);
            }
            self.at = 0;
        }
        self.at += 1;
        Ok(Some(self.block[self.at - 1]))
    }
}

/// The streams merged in order, to `each`; `stop` asked every block's worth.
fn merge(mut streams: Vec<Stream>, stop: &AtomicBool, mut each: impl FnMut(Entry) -> io::Result<()>) -> io::Result<()> {
    let mut heap = BinaryHeap::new();
    for (i, s) in streams.iter_mut().enumerate() {
        if let Some(e) = s.next()? {
            heap.push(std::cmp::Reverse((e, i)));
        }
    }
    let mut n = 0usize;
    while let Some(std::cmp::Reverse((e, i))) = heap.pop() {
        each(e)?;
        if let Some(next) = streams[i].next()? {
            heap.push(std::cmp::Reverse((next, i)));
        }
        n += 1;
        if n.is_multiple_of(BLOCK) && stop.load(Ordering::Relaxed) {
            return Err(stopped());
        }
    }
    Ok(())
}

fn stopped() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "stopped")
}

// ---------------------------------------------------------------- an index on the disk

/// What `done` says: the index is whole, of the database as it was then.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Done {
    form: u32,
    /// Kept whole when it is not UTF-8, so its index is found again.
    #[serde(with = "crate::spill::path_json")]
    source: PathBuf,
    /// The games table's stamp of the database (`table::stamp`).
    stamp: String,
    /// In moves; 0 the whole game.
    depth: u32,
    games: u32,
    entries: u64,
    /// The database's size when its table was read, and a hash of its last bytes before that: a
    /// PGN file that only grew has the same bytes there. Empty when the file had changed since
    /// its table was read: the next build reads every game.
    size: u64,
    tail: String,
    /// The games table's form it was built from: a table read anew in another form may cut the
    /// games differently, so a grown file is then read whole again.
    table_form: String,
    /// Nanoseconds since 1970 it was made: the newest of two is taken.
    made: u64,
}

/// An index open for reading.
pub struct Index {
    dir: PathBuf,
    done: Done,
    keys: Vec<u64>,
    places: Vec<u64>,
    postings: Mutex<File>,
    tree: Mutex<File>,
    sums: u64,
    /// Blocks of `postings` and records of `tree` read: the work of a look, counted.
    reads: AtomicU64,
}

impl Index {
    fn open(dir: &Path) -> io::Result<Index> {
        let done: Done = serde_json::from_slice(&std::fs::read(dir.join("done"))?).map_err(io::Error::other)?;
        if done.form != FORM {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "another form"));
        }
        let raw = std::fs::read(dir.join("blocks"))?;
        let (mut keys, mut places) = (vec![], vec![]);
        for c in raw.chunks_exact(16) {
            keys.push(u64::from_le_bytes(c[..8].try_into().expect("8 bytes")));
            places.push(u64::from_le_bytes(c[8..].try_into().expect("8 bytes")));
        }
        let tree = File::open(dir.join("tree"))?;
        let sums = tree.metadata()?.len() / SUMS as u64;
        Ok(Index { dir: dir.to_path_buf(), done, keys, places, postings: Mutex::new(File::open(dir.join("postings"))?), tree: Mutex::new(tree), sums, reads: AtomicU64::new(0) })
    }

    /// The bytes it takes.
    pub fn bytes(&self) -> u64 {
        crate::fs::dir_size(&self.dir).0
    }

    /// In moves; 0 the whole game.
    pub fn depth(&self) -> u32 {
        self.done.depth
    }

    pub fn games(&self) -> u32 {
        self.done.games
    }

    /// The entries of position `key` to `each`, in game order, until it says stop.
    fn entries(&self, key: u64, mut each: impl FnMut(u32, u16) -> bool) -> io::Result<()> {
        // The group may start in the block before the first one that starts with it.
        let mut b = self.keys.partition_point(|&k| k < key).saturating_sub(1);
        let mut f = self.postings.lock().unwrap_or_else(|e| e.into_inner());
        f.seek(SeekFrom::Start(self.places.get(b).copied().unwrap_or(0)))?;
        let mut r = BufReader::with_capacity(64 << 10, &mut *f);
        let (mut buf, mut block) = (vec![], vec![]);
        while b < self.keys.len() && self.keys[b] <= key {
            if !read_block(&mut r, &mut buf, &mut block)? {
                break;
            }
            self.reads.fetch_add(1, Ordering::Relaxed);
            for &e in &block {
                let (k, g, c) = parts(e);
                if k > key {
                    return Ok(());
                }
                if k == key && !each(g, c) {
                    return Ok(());
                }
            }
            b += 1;
        }
        Ok(())
    }

    fn sum_at(&self, f: &mut File, i: u64) -> io::Result<(u64, Sums)> {
        let mut b = [0u8; SUMS];
        self.reads.fetch_add(1, Ordering::Relaxed);
        f.seek(SeekFrom::Start(i * SUMS as u64))?;
        f.read_exact(&mut b)?;
        Ok(Sums::get(&b))
    }

    /// The kept sums of position `key`: none when it is a rarer one.
    fn kept(&self, key: u64) -> io::Result<Vec<Sums>> {
        let mut f = self.tree.lock().unwrap_or_else(|e| e.into_inner());
        let (mut lo, mut hi) = (0u64, self.sums);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.sum_at(&mut f, mid)?.0 < key { lo = mid + 1 } else { hi = mid }
        }
        let mut out = vec![];
        while lo < self.sums {
            let (k, s) = self.sum_at(&mut f, lo)?;
            if k != key {
                break;
            }
            out.push(s);
            lo += 1;
        }
        Ok(out)
    }

    /// The sums of position `key`, each move's and the position's own (`REACH`) last; counted
    /// from the games' rows (`info`) when they are not kept. The work: one look in `tree`, and
    /// for a rarer position its fewer than `HOT` entries and their rows.
    fn sums(&self, key: u64, info: &dyn Fn(u32) -> Option<Info>) -> io::Result<Vec<Sums>> {
        let kept = self.kept(key)?;
        if !kept.is_empty() {
            return Ok(kept);
        }
        let mut found: Vec<(u32, u16)> = vec![];
        self.entries(key, |g, c| {
            found.push((g, c));
            true
        })?;
        Ok(tally(&found, info))
    }

    /// The games that reach position `key`, in their order in the file, each once.
    fn reaching(&self, key: u64) -> io::Result<Vec<u32>> {
        let mut out: Vec<u32> = vec![];
        self.entries(key, |g, _| {
            if out.last() != Some(&g) {
                out.push(g);
            }
            true
        })?;
        Ok(out)
    }
}

/// The sums of one position's entries (in game order): each move's, sorted by code, then the
/// position's own (`REACH`).
fn tally(found: &[(u32, u16)], info: &dyn Fn(u32) -> Option<Info>) -> Vec<Sums> {
    let mut by: Vec<Sums> = vec![];
    let mut reach = Sums::new(REACH);
    let mut last = None;
    for &(g, c) in found {
        // A game without its row (a stale index of a file changed in its middle) is still
        // counted: its results, Elos and years are not.
        let i = info(g).unwrap_or_default();
        if last != Some(g) {
            // The position's own (REACH has the side bit: its best is Black's, never shown).
            reach.add(g, &i);
            last = Some(g);
        }
        if c & !BLACK == END {
            continue;
        }
        match by.iter_mut().find(|s| s.code == c) {
            Some(s) => s.add(g, &i),
            None => {
                let mut s = Sums::new(c);
                s.add(g, &i);
                by.push(s);
            }
        }
    }
    by.sort_by_key(|s| s.code);
    by.push(reach);
    by
}

/// A next move in the opening tree.
#[derive(Clone, Debug, Serialize)]
pub struct TreeMove {
    /// In English SAN (xiangqi: ICCS), in UCI, and the position after it.
    pub san: String,
    pub uci: String,
    pub fen: String,
    pub games: u32,
    /// Games White won, drew, Black won, in per cent of `games` (whole numbers).
    pub white: u32,
    pub draws: u32,
    pub black: u32,
    /// The average Elo of the side making the move, of the games that give one.
    pub elo: Option<u16>,
    /// The first and last year it was played.
    pub first: Option<u16>,
    pub last: Option<u16>,
    /// The best-rated player making it, and that Elo.
    pub best: Option<(String, u16)>,
}

/// The opening tree of a position.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Tree {
    /// The games that reach the position.
    pub games: u32,
    /// Its moves, the most played first.
    pub moves: Vec<TreeMove>,
    /// The index is of the database as it was before it changed: being built again.
    pub stale: bool,
    /// Its games can be listed and opened: not while it is stale, as its game numbers are of the
    /// database before it changed (`games_at` gives none then).
    pub opens: bool,
    /// The index's depth in moves (0: the whole game); a position past it has no moves.
    pub depth: u32,
}

fn per_cent(n: u32, of: u32) -> u32 {
    if of == 0 { 0 } else { ((n as u64 * 100 + of as u64 / 2) / of as u64) as u32 }
}

fn tree_of<K: Kind>(k: &K, idx: &Index, lib: &Library, fen: &str) -> io::Result<Tree> {
    let Some(b) = k.start(Some(fen)) else { return Ok(Tree::default()) };
    // The games' rows are read only when they are the index's games: of the same table, or of
    // a file that only grew. Else a stale index gives what it keeps, without the rows'.
    let rows = fresh(idx, lib) || grown(idx, &lib.path);
    let rec = |g: u32| if rows { lib.rec(g) } else { None };
    let sums = idx.sums(k.key(&b), &|g| rec(g).map(|r| Info::of(&r)))?;
    let games = sums.last().filter(|s| s.code == REACH).map_or(0, |s| s.games);
    let mut moves: Vec<TreeMove> = sums
        .iter()
        .filter(|s| s.code != REACH)
        .filter_map(|s| {
            let (san, uci, fen) = k.shown(&b, s.code & !BLACK)?;
            let best = rec(s.best_game).map(|r| {
                let name = lib.name(if s.code & BLACK != 0 { r.black } else { r.white });
                (name, s.best_elo)
            });
            Some(TreeMove {
                san,
                uci,
                fen,
                games: s.games,
                white: per_cent(s.white, s.games),
                draws: per_cent(s.draws, s.games),
                black: per_cent(s.black, s.games),
                elo: (s.elo_n > 0).then(|| (s.elo_sum / s.elo_n as u64) as u16),
                first: (s.first > 0).then_some(s.first),
                last: (s.last > 0).then_some(s.last),
                best: best.filter(|(n, _)| !n.is_empty()),
            })
        })
        .collect();
    moves.sort_by(|a, b| b.games.cmp(&a.games).then_with(|| a.san.cmp(&b.san)));
    Ok(Tree { games, moves, stale: false, opens: false, depth: idx.depth() })
}

fn key_of(lib: &Library, fen: &str) -> Option<u64> {
    match lib.format {
        super::Format::Xiangqi => Xiangqi.start(Some(fen)).map(|b| Xiangqi.key(&b)),
        _ => Chess.start(Some(fen)).map(|b| Chess.key(&b)),
    }
}

// ---------------------------------------------------------------- building

/// How far a build has come.
#[derive(Default)]
struct Job {
    /// 0 reading the games, 1 sorting, 2 done, 3 failed.
    phase: AtomicU8,
    done: AtomicU64,
    of: AtomicU64,
    stop: AtomicBool,
    error: Mutex<Option<String>>,
    /// The database's stamp it builds for.
    stamp: Mutex<String>,
    /// Games played out: the work, counted.
    walked: AtomicU64,
    /// Threads that read the games' text.
    readers: AtomicU64,
    /// The most text one batch of games held: the memory, counted.
    most: AtomicU64,
    /// Entries sorted into runs, for the merge's progress.
    entries: AtomicU64,
    /// The index was removed while this ran: its folders go once it has let them go.
    removed: AtomicBool,
}

/// The hash of `size`'s last 64 KB of the file at `path` (empty when it cannot be read).
fn tail(path: &Path, size: u64) -> String {
    let Ok(mut f) = File::open(path) else { return String::new() };
    let from = size.saturating_sub(64 << 10);
    let mut b = vec![0u8; (size - from) as usize];
    if f.seek(SeekFrom::Start(from)).and_then(|_| f.read_exact(&mut b)).is_err() {
        return String::new();
    }
    blake3::hash(&b).to_hex()[..32].to_string()
}

/// The size of the file at `path` and its tail (`tail`), while its stamp is `stamp` (the file
/// its table read); else an empty tail, so it is never taken for one that only grew.
fn sized(path: &Path, stamp: &str) -> (u64, String) {
    let same = || super::table::stamp(path) == stamp;
    if !grows(path) || !same() {
        return (0, String::new());
    }
    let size = std::fs::metadata(path).map_or(0, |m| m.len());
    let t = tail(path, size);
    // Changed while its tail was read: none.
    if same() { (size, t) } else { (0, String::new()) }
}

/// The file at `path` only grew since `idx` was made of it: its games before the last one are
/// the same, by the same numbers.
fn grown(idx: &Index, path: &Path) -> bool {
    let size = std::fs::metadata(path).map_or(0, |m| m.len());
    idx.done.table_form == super::table::FORM && grows(path) && size > idx.done.size && !idx.done.tail.is_empty() && tail(path, idx.done.size) == idx.done.tail
}

/// A plain PGN file, read where it is: one that grew can keep its old entries.
fn grows(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pgn")) && path.is_file()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

/// The games' entries, from game `from` on, played out and sorted in runs in `runs/`; `stop`
/// asked between games. The runs' files.
fn runs<K: Kind + Sync>(k: &K, lib: &Library, from: u32, count: u32, plies: usize, dir: &Path, job: &Job) -> io::Result<Vec<PathBuf>> {
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).div_ceil(2).clamp(1, 8);
    let made: Mutex<Vec<PathBuf>> = Mutex::new(vec![]);
    let failed: Mutex<Option<io::Error>> = Mutex::new(None);
    let n = AtomicU64::new(0);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<(u32, String)>>(threads * 2);
    let rx = Mutex::new(rx);
    // The next game a reader takes.
    let next = AtomicU64::new(from as u64);
    let write_run = |buf: &mut Vec<Entry>| -> io::Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        buf.sort_unstable();
        let p = dir.join(format!("run-{}", n.fetch_add(1, Ordering::Relaxed)));
        let mut w = Writer::new(BufWriter::with_capacity(1 << 20, File::create(&p)?));
        for &e in buf.iter() {
            w.push(e)?;
        }
        job.entries.fetch_add(w.finish()?.2, Ordering::Relaxed);
        made.lock().unwrap_or_else(|e| e.into_inner()).push(p);
        buf.clear();
        Ok(())
    };
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut buf: Vec<Entry> = Vec::with_capacity(RUN);
                let fail = |e: io::Error| {
                    failed.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(e);
                    job.stop.store(true, Ordering::Relaxed);
                };
                loop {
                    let batch = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                    let Ok(batch) = batch else { break };
                    let games = batch.len() as u64;
                    for (g, text) in batch {
                        walk(k, &text, plies, |key, code| buf.push(entry(key, g, code)));
                        job.walked.fetch_add(1, Ordering::Relaxed);
                        if buf.len() >= RUN - 1024
                            && let Err(e) = write_run(&mut buf)
                        {
                            fail(e);
                            // Taken and dropped, so no reader waits on a full channel for ever.
                            while rx.lock().unwrap_or_else(|e| e.into_inner()).recv().is_ok() {}
                            return;
                        }
                    }
                    job.done.fetch_add(games, Ordering::Relaxed);
                }
                if let Err(e) = write_run(&mut buf) {
                    fail(e);
                }
            });
        }
        // The games read in order, one thread: a packed file is unpacked once, front to back.
        // A database that reads any game at its place (`reads_anywhere`) is read by as many
        // threads as play them out, each taking `CHUNK` games after another. A batch holds
        // `BATCH` games or `BATCH_BYTES` of text, whichever comes first, so heavily annotated
        // games waiting in the channel hold little memory.
        let readers = if lib.reads_anywhere() { threads } else { 1 };
        for _ in 0..readers {
            job.readers.fetch_add(1, Ordering::Relaxed);
            let (tx, next) = (tx.clone(), &next);
            s.spawn(move || {
                let mut batch = Vec::with_capacity(BATCH);
                let mut bytes = 0;
                let send = |batch: Vec<(u32, String)>, bytes: usize| {
                    job.most.fetch_max(bytes as u64, Ordering::Relaxed);
                    tx.send(batch).is_ok()
                };
                'chunks: loop {
                    let first = next.fetch_add(CHUNK, Ordering::Relaxed);
                    if first >= count as u64 {
                        break;
                    }
                    for g in first as u32..(first + CHUNK).min(count as u64) as u32 {
                        if job.stop.load(Ordering::Relaxed) {
                            break 'chunks;
                        }
                        match lib.text(g) {
                            Ok(t) => {
                                bytes += t.len();
                                batch.push((g, t));
                            }
                            // A game that cannot be read has no positions; the others go on.
                            Err(_) => {}
                        }
                        if (batch.len() == BATCH || bytes >= BATCH_BYTES) && !send(std::mem::replace(&mut batch, Vec::with_capacity(BATCH)), std::mem::take(&mut bytes)) {
                            break 'chunks;
                        }
                    }
                }
                send(batch, bytes);
            });
        }
        drop(tx);
    });
    if let Some(e) = failed.into_inner().unwrap_or_else(|e| e.into_inner()) {
        return Err(e);
    }
    if job.stop.load(Ordering::Relaxed) {
        return Err(stopped());
    }
    Ok(made.into_inner().unwrap_or_else(|e| e.into_inner()))
}

/// Builds the index of `lib` (its table whole) into `dir`: from scratch, or keeping the entries
/// of `old` for its games before `keep`.
fn build_in(lib: &Library, depth: u32, dir: &Path, old: Option<(&Index, u32)>, job: &Job, stamp: &str) -> io::Result<Done> {
    // The file's size and tail now, while it is still the file the table read: taken at the
    // end, a file replaced meanwhile and then grown would keep this table's entries.
    let (size, tail) = sized(&lib.path, stamp);
    let count = lib.progress().games as u32;
    let from = old.map_or(0, |(_, keep)| keep).min(count);
    job.of.store((count - from) as u64, Ordering::Relaxed);
    let runs_dir = dir.join("runs");
    std::fs::create_dir_all(&runs_dir)?;
    let plies = plies(depth);
    let mut runs = match lib.format {
        super::Format::Xiangqi => runs(&Xiangqi, lib, from, count, plies, &runs_dir, job)?,
        _ => runs(&Chess, lib, from, count, plies, &runs_dir, job)?,
    };
    job.phase.store(1, Ordering::Relaxed);
    // The rows' Elos, years and results, 8 bytes a game, for the sums.
    let mut info = vec![Info::default(); count as usize];
    lib.each_rec(count, |i, r| info[i as usize] = Info::of(r))?;
    // Merged in rounds of `FAN` until few are left.
    let mut n = 0usize;
    while runs.len() > FAN {
        let rest = runs.split_off(FAN);
        let p = runs_dir.join(format!("merged-{n}"));
        n += 1;
        let mut w = Writer::new(BufWriter::with_capacity(1 << 20, File::create(&p)?));
        merge(runs.iter().map(|r| Stream::open(r)).collect::<io::Result<_>>()?, &job.stop, |e| w.push(e))?;
        w.finish()?;
        for r in &runs {
            let _ = std::fs::remove_file(r);
        }
        runs = rest;
        runs.push(p);
    }
    let mut streams: Vec<Stream> = runs.iter().map(|r| Stream::open(r)).collect::<io::Result<_>>()?;
    let mut total = 0u64;
    if let Some((old, keep)) = old {
        // The old entries of the games kept: a stream of its own, without the rest.
        let p = runs_dir.join("old");
        let mut w = Writer::new(BufWriter::with_capacity(1 << 20, File::create(&p)?));
        let mut s = Stream::open(&old.dir.join("postings"))?;
        while let Some(e) = s.next()? {
            if parts(e).1 < keep {
                w.push(e)?;
            }
        }
        w.finish()?;
        streams.push(Stream::open(&p)?);
        total += old.done.entries;
    }
    total += job.entries.load(Ordering::Relaxed);
    job.done.store(0, Ordering::Relaxed);
    job.of.store(total.max(1), Ordering::Relaxed);
    let mut post = Writer::new(BufWriter::with_capacity(1 << 20, File::create(dir.join("postings"))?));
    let mut tree = BufWriter::with_capacity(1 << 20, File::create(dir.join("tree"))?);
    // The position being merged: its entries, until it has `HOT`, then its sums.
    let mut group: Vec<(u32, u16)> = vec![];
    let mut sums: Option<Vec<Sums>> = None;
    let mut key = None;
    let mut last_game = None;
    let at = |g: u32| info.get(g as usize).copied();
    let close = |key: u64, sums: Option<Vec<Sums>>, tree: &mut BufWriter<File>| -> io::Result<()> {
        let Some(mut s) = sums else { return Ok(()) };
        s.sort_by_key(|s| s.code);
        let mut b = [0u8; SUMS];
        for x in &s {
            x.put(key, &mut b);
            tree.write_all(&b)?;
        }
        Ok(())
    };
    merge(streams, &job.stop, |e| {
        let (k, g, c) = parts(e);
        if key != Some(k) {
            if let Some(old) = key {
                close(old, sums.take(), &mut tree)?;
            }
            (key, last_game) = (Some(k), None);
            group.clear();
        }
        if post.last == Some(e) {
            return Ok(());
        }
        post.push(e)?;
        job.done.fetch_add(1, Ordering::Relaxed);
        match sums.as_mut() {
            Some(s) => {
                let Some(i) = at(g) else { return Ok(()) };
                let reach = s.iter_mut().find(|x| x.code == REACH).expect("the position's own");
                if last_game != Some(g) {
                    reach.add(g, &i);
                }
                if c & !BLACK != END {
                    match s.iter_mut().find(|x| x.code == c) {
                        Some(x) => x.add(g, &i),
                        None => {
                            let mut x = Sums::new(c);
                            x.add(g, &i);
                            s.push(x);
                        }
                    }
                }
            }
            None => {
                group.push((g, c));
                if group.len() >= HOT {
                    sums = Some(tally(&group, &at));
                    group.clear();
                }
            }
        }
        last_game = Some(g);
        Ok(())
    })?;
    if let Some(k) = key {
        close(k, sums.take(), &mut tree)?;
    }
    let (post, blocks, entries) = post.finish()?;
    post.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    tree.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    let mut b = Vec::with_capacity(blocks.len() * 16);
    for (k, at) in blocks {
        b.extend_from_slice(&k.to_le_bytes());
        b.extend_from_slice(&at.to_le_bytes());
    }
    let mut f = File::create(dir.join("blocks"))?;
    f.write_all(&b)?;
    f.sync_all()?;
    let _ = std::fs::remove_dir_all(&runs_dir);
    let done = Done { form: FORM, source: lib.path.clone(), stamp: stamp.to_string(), depth, games: count, entries, size, tail, table_form: super::table::FORM.to_string(), made: now() };
    let part = dir.join("done.part");
    let mut f = File::create(&part)?;
    f.write_all(&serde_json::to_vec(&done).map_err(io::Error::other)?)?;
    f.sync_all()?;
    std::fs::rename(&part, dir.join("done"))?;
    Ok(done)
}

// ---------------------------------------------------------------- the indexes of this run

/// The indexes open, by their database; the builds under way.
static OPEN: Mutex<Option<HashMap<PathBuf, Arc<Index>>>> = Mutex::new(None);
static JOBS: Mutex<Option<HashMap<PathBuf, Arc<Job>>>> = Mutex::new(None);

fn key_name(path: &Path) -> String {
    let mut h = blake3::Hasher::new();
    h.update(path.as_os_str().as_encoded_bytes());
    h.finalize().to_hex()[..16].to_string()
}

fn read_done(dir: &Path) -> Option<Done> {
    serde_json::from_slice(&std::fs::read(dir.join("done")).ok()?).ok()
}

/// The whole index folders of `path`, the newest first; each one a run that ended left half
/// made goes.
fn folders(path: &Path) -> Vec<(PathBuf, Done)> {
    let Some(root) = folder() else { return vec![] };
    let prefix = format!("{}-", key_name(path));
    let mut out = vec![];
    for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) {
            continue;
        }
        match read_done(&e.path()) {
            Some(d) if d.source == path && d.form == FORM => out.push((e.path(), d)),
            // An older version's goes; a newer one's is left to it.
            Some(d) if d.form < FORM => crate::spill::gone(&e.path()),
            Some(_) => {}
            None if super::table::stale(&e.path()) => crate::spill::gone(&e.path()),
            None => {}
        }
    }
    out.sort_by_key(|(_, d)| std::cmp::Reverse(d.made));
    out
}

/// The index of `path` this run uses: the newest whole one; older ones go.
fn opened(path: &Path) -> Option<Arc<Index>> {
    let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    let open = open.get_or_insert_with(HashMap::new);
    if let Some(i) = open.get(path) {
        return Some(i.clone());
    }
    let mut found = folders(path).into_iter();
    let (dir, _) = found.next()?;
    for (older, _) in found {
        crate::spill::gone(&older);
    }
    let i = Arc::new(Index::open(&dir).ok()?);
    open.insert(path.to_path_buf(), i.clone());
    Some(i)
}

/// The database's stamp (`table::stamp`). The apps ask where the index stands each second: a file
/// inside an archive, stamped by its archive, is asked again at most every few seconds.
fn stamp(path: &Path) -> String {
    // A file of its own: a few metadata calls, asked each time.
    if path.exists() {
        return super::table::stamp(path);
    }
    static KNOWN: Mutex<Option<HashMap<PathBuf, (std::time::Instant, String)>>> = Mutex::new(None);
    let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    let known = known.get_or_insert_with(HashMap::new);
    if let Some((at, s)) = known.get(path)
        && at.elapsed() < std::time::Duration::from_secs(3)
    {
        return s.clone();
    }
    let s = super::table::stamp(path);
    known.insert(path.to_path_buf(), (std::time::Instant::now(), s.clone()));
    s
}

/// Goes up each time an index is made or removed: a list cut to a position, kept by an app, is
/// asked again when it changes.
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Databases whose index was removed by hand, with their stamp then: Settings' every games
/// table does not build it again until the database changes.
static REMOVED: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

fn job(path: &Path) -> Option<Arc<Job>> {
    JOBS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).get(path).cloned()
}

/// Where a games table's index stands, for its button and its tree.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    /// Its games can have one: chess and xiangqi.
    pub can: bool,
    /// An index is there (`stale`: of the database before it changed).
    pub ready: bool,
    pub stale: bool,
    /// The index's depth in moves (0 the whole game), its bytes, its games.
    pub depth: u32,
    pub bytes: u64,
    pub games: u32,
    /// Being built: how far, 0 to 1.
    pub building: Option<f32>,
    /// The last build failed: why.
    pub error: Option<String>,
    /// What a build at the depth asked would take, about: bytes and seconds.
    pub estimate_bytes: u64,
    pub estimate_seconds: u64,
}

fn can(lib: &Library) -> bool {
    matches!(lib.format, super::Format::Pgn | super::Format::Xiangqi)
}

/// About what a build of `lib` at `depth` takes: bytes and seconds, from a sample of its rows.
pub fn estimate(lib: &Library, depth: u32) -> (u64, u64) {
    let count = lib.progress().games as u32;
    if count == 0 {
        return (0, 0);
    }
    let limit = plies(depth);
    let step = (count / 4096).max(1);
    let (mut seen, mut sum) = (0u64, 0u64);
    let mut g = 0;
    while g < count {
        if let Some(r) = lib.rec(g) {
            sum += (r.plies as usize).min(limit) as u64 + 1;
            seen += 1;
        }
        g += step;
    }
    let entries = sum * count as u64 / seen.max(1);
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).div_ceil(2).clamp(1, 8) as u64;
    (entries * BYTES_PER_ENTRY, (entries / (ENTRIES_PER_SECOND * threads)).max(1))
}

/// Where `lib`'s index stands; with `depth`, the estimate of a build at that depth.
pub fn status(lib: &Library, depth: u32) -> Status {
    if !can(lib) {
        return Status::default();
    }
    let path = &lib.path;
    let mut s = Status { can: true, ..Status::default() };
    if let Some(i) = opened(path) {
        (s.ready, s.depth, s.bytes, s.games) = (true, i.depth(), i.bytes(), i.games());
        s.stale = !fresh(&i, lib);
    }
    if let Some(j) = job(path) {
        match j.phase.load(Ordering::Relaxed) {
            0 | 1 => {
                let (d, of) = (j.done.load(Ordering::Relaxed), j.of.load(Ordering::Relaxed).max(1));
                let part = (d as f32 / of as f32).min(1.0);
                s.building = Some(if j.phase.load(Ordering::Relaxed) == 0 { part * 0.8 } else { 0.8 + part * 0.2 });
            }
            3 => s.error = j.error.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            _ => {}
        }
    }
    if lib.progress().done {
        // Worked out once for the table as it is: it reads a few thousand rows.
        static KNOWN: Mutex<Option<HashMap<(PathBuf, usize, u32), (u64, u64)>>> = Mutex::new(None);
        let k = (path.clone(), lib.progress().games, depth);
        let known = KNOWN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).get(&k).copied();
        (s.estimate_bytes, s.estimate_seconds) = known.unwrap_or_else(|| {
            let e = estimate(lib, depth);
            KNOWN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(k, e);
            e
        });
    }
    s
}

/// Builds `lib`'s index at `depth` in the background (its table read whole first), unless one
/// is being built: the old index, if any, is used until the new one is whole. A PGN file that
/// only grew since the old one keeps its entries and reads the games added.
pub fn build(lib: Arc<Library>, depth: u32) {
    if !can(&lib) {
        return;
    }
    // One there keeps its depth when built again; Settings' depth is for a new one.
    let depth = opened(&lib.path).map_or(depth, |i| i.depth());
    let path = lib.path.clone();
    let lib_path = path.clone();
    REMOVED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).remove(&path);
    let j = Arc::new(Job::default());
    {
        let mut jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
        let jobs = jobs.get_or_insert_with(HashMap::new);
        if jobs.get(&path).is_some_and(|j| j.phase.load(Ordering::Relaxed) < 2) {
            return;
        }
        jobs.insert(path.clone(), j.clone());
    }
    let job = j.clone();
    let run = move || {
        while !lib.progress().done {
            if j.stop.load(Ordering::Relaxed) {
                return Err(stopped());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if let Some(e) = lib.progress().error {
            return Err(io::Error::other(e));
        }
        // The stamp of the table the games are read from, not of the file now: a table read
        // before the file changed would give another file's game numbers.
        let stamp = lib.stamp().to_string();
        *j.stamp.lock().unwrap_or_else(|e| e.into_inner()) = stamp.clone();
        let root = folder().ok_or_else(|| io::Error::other("no cache folder"))?;
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = root.join(format!("{}-{}-{}", key_name(&path), crate::spill::run(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir)?;
        let old = opened(&path);
        // Kept: the old games but the last (a game cut short at the end may have grown).
        let keep = old.as_ref().filter(|o| o.done.depth == depth && grown(o, &path)).map(|o| o.done.games.saturating_sub(1));
        let made = build_in(&lib, depth, &dir, old.as_deref().zip(keep), &j, &stamp);
        if made.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        made?;
        let new = Arc::new(Index::open(&dir)?);
        // Put in place under the lock `remove` takes: removed meanwhile, it is not put in.
        let before = {
            let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
            if j.stop.load(Ordering::Relaxed) {
                drop(open);
                drop(new);
                let _ = std::fs::remove_dir_all(&dir);
                return Err(stopped());
            }
            open.get_or_insert_with(HashMap::new).insert(path.clone(), new)
        };
        GENERATION.fetch_add(1, Ordering::Relaxed);
        drop(old);
        // The old one goes once no one reads it; else the next open takes it out.
        if let Some(b) = before
            && let Some(b) = Arc::into_inner(b)
        {
            let dir = b.dir.clone();
            drop(b);
            crate::spill::gone(&dir);
        }
        Ok(())
    };
    let ended = job.clone();
    let source = lib_path.clone();
    let spawned = std::thread::Builder::new().name("chess-positions".into()).spawn(move || {
        // A panic is a failed build, not one that never ends.
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| Err(io::Error::other("the build failed")));
        // Removed while it ran: what it held open is let go now, so its folders can go.
        if job.removed.load(Ordering::Relaxed) {
            // An index built again since (Build after Remove) is in use: it stays.
            let live = OPEN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|o| o.get(&source)).map(|i| i.dir.clone());
            for (dir, _) in folders(&source).into_iter().filter(|(d, _)| Some(d) != live.as_ref()) {
                gone(&dir);
            }
        }
        match r {
            Ok(()) => job.phase.store(2, Ordering::Relaxed),
            Err(e) => {
                if e.kind() != io::ErrorKind::Interrupted {
                    *job.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
                }
                job.phase.store(3, Ordering::Relaxed);
            }
        }
    });
    if let Err(e) = spawned {
        *ended.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
        ended.phase.store(3, Ordering::Relaxed);
    }
}

/// What the games table does when it is shown: builds the index when Settings says every games
/// table gets one, and builds an out-of-date one again (both in the background, once a run).
pub fn keep_up(lib: &Arc<Library>, cfg: &crate::config::ChessConfig) {
    if !can(lib) || !lib.progress().done {
        return;
    }
    let path = &lib.path;
    // A table of the file before it changed: the app reads it again first (`outdated`).
    if outdated(lib) {
        return;
    }
    if let Some(j) = job(path) {
        // Under way, or tried for the database as it is now.
        let stamp = j.stamp.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if j.phase.load(Ordering::Relaxed) < 2 {
            // Of a table read before the file changed (the app has read it again): stopped, not
            // left to finish out of date; built again from this table once it has ended. An
            // empty stamp: still waiting for its table.
            if !stamp.is_empty() && stamp != lib.stamp() {
                stop(path);
            }
            return;
        }
        if stamp == self::stamp(path) {
            return;
        }
    }
    match opened(path) {
        Some(i) if !fresh(&i, lib) => build(lib.clone(), i.depth()),
        Some(_) => {}
        None if cfg.position_index => {
            let removed = REMOVED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).get(path).cloned();
            if removed.is_none_or(|s| s != stamp(path)) {
                build(lib.clone(), depth_of(&cfg.position_depth));
            }
        }
        None => {}
    }
}

/// Stops the build of `path`'s index, if one is under way.
pub fn stop(path: &Path) {
    if let Some(j) = job(path) {
        j.stop.store(true, Ordering::Relaxed);
    }
}

/// `path`'s index stopped and removed, every folder of it. The bytes freed.
pub fn remove(path: &Path) -> u64 {
    let running = job(path).filter(|j| j.phase.load(Ordering::Relaxed) < 2);
    if let Some(j) = &running {
        j.removed.store(true, Ordering::Relaxed);
    }
    let open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    stop(path);
    REMOVED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(path.to_path_buf(), super::table::stamp(path));
    GENERATION.fetch_add(1, Ordering::Relaxed);
    JOBS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).remove(path);
    let mut open = open;
    drop(open.get_or_insert_with(HashMap::new).remove(path));
    drop(open);
    let mut freed = 0;
    for (dir, _) in folders(path) {
        freed += crate::fs::dir_size(&dir).0;
        gone(&dir);
    }
    freed
}

/// An index folder removed. One still held open (Windows: a look in flight) cannot be moved
/// away: its `done` goes, so it is no index any more, and the folder goes once its run ended.
fn gone(dir: &Path) {
    crate::spill::gone(dir);
    if dir.exists() {
        let _ = std::fs::remove_file(dir.join("done"));
    }
}

/// The opening tree of the position `fen` in `lib`'s games; none without an index.
pub fn tree(lib: &Library, fen: &str) -> Option<Tree> {
    if !can(lib) {
        return None;
    }
    let idx = opened(&lib.path)?;
    let mut t = match lib.format {
        super::Format::Xiangqi => tree_of(&Xiangqi, &idx, lib, fen),
        _ => tree_of(&Chess, &idx, lib, fen),
    }
    .ok()?;
    t.stale = !fresh(&idx, lib);
    t.opens = !t.stale;
    Some(t)
}

/// The index is of the database as it is now. A stale one still gives its counts, but never its
/// game numbers: they are of the database before it changed, and would open other games.
fn fresh(idx: &Index, lib: &Library) -> bool {
    idx.done.stamp == lib.stamp() && idx.done.stamp == stamp(&lib.path)
}

/// `lib`'s table is of its file before it changed: an app reads it again (`Library::open`),
/// and the index is built again from that.
pub fn outdated(lib: &Library) -> bool {
    lib.stamp() != stamp(&lib.path)
}

/// The games of `lib`'s index may be listed and opened: it is there and not stale. A list cut
/// to a position, kept by an app, is made again when this changes.
pub fn opens(lib: &Library) -> bool {
    opened(&lib.path).is_some_and(|i| fresh(&i, lib))
}

/// The games of `lib` that reach the position `fen`, in the file's order; none without an
/// index, or while it is stale (`fresh`).
pub fn games_at(lib: &Library, fen: &str) -> Option<Vec<u32>> {
    let idx = opened(&lib.path).filter(|i| fresh(i, lib))?;
    idx.reaching(key_of(lib, fen)?).ok()
}

/// The index in `dir` removed (Disk use's *Remove*): every index of its database.
pub fn remove_folder(dir: &Path) {
    match read_done(dir) {
        Some(d) => {
            remove(&d.source);
        }
        None => crate::spill::gone(dir),
    }
}

/// One kept index, for Disk use: its folder, its database, its bytes.
pub struct Kept {
    pub dir: PathBuf,
    pub source: PathBuf,
    pub bytes: u64,
}

/// Every whole index kept; one whose database is gone (from a folder that is there) goes, and so
/// does one a build that ended (a quit, a crash) left half made.
pub fn kept() -> Vec<Kept> {
    let mut out = vec![];
    let Some(root) = folder() else { return out };
    for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        let dir = e.path();
        if e.file_name().to_string_lossy().starts_with(".gone-") {
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        }
        let Some(d) = read_done(&dir) else {
            // Its run (`<hash>-<run>-<n>`) is known to have ended: one that keeps no list may
            // still be building, however old the folder.
            let name = e.file_name().to_string_lossy().into_owned();
            let run = name.split_once('-').and_then(|(_, r)| r.rsplit_once('-')).map(|(r, _)| r.to_string());
            if run.as_deref().and_then(crate::spill::alive) == Some(false) {
                crate::spill::gone(&dir);
            }
            continue;
        };
        let src = if d.source.exists() { d.source.clone() } else { crate::archive::split(&d.source).map_or(d.source.clone(), |(a, _)| a) };
        if !src.exists() && src.parent().is_some_and(|p| p.exists()) {
            crate::spill::gone(&dir);
            continue;
        }
        out.push(Kept { bytes: crate::fs::dir_size(&dir).0, dir, source: d.source });
    }
    out.sort_by(|a, b| a.source.cmp(&b.source));
    out
}

#[cfg(test)]
mod tests;

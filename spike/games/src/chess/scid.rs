//! SCID databases (`.si4` index, `.sg4` games, `.sn4` names; SCID 5's `.si5`, `.sg5`, `.sn5`),
//! read in the chess table like a PGN file (docs/previews/chess.md#scid-databases). Written
//! from the layout of the files as SCID writes them, not from SCID's code. SCID 4: big-endian
//! numbers; an index of 182 bytes of header and 47 bytes a game; the names front-coded, each
//! with its id. SCID 5: an index of 56 bytes a game in little-endian words and no header; the
//! names one after another, each after its length and kind in one number of 7-bit groups. A
//! game is the same in both: its extra tags, its start position when not the usual one, and
//! its moves a byte each (the piece's number in its side's list and a value by its kind),
//! comments after the moves.
//!
//! Each game is turned into PGN text (`Base::pgn`), which `pgn::parse` reads as any other, so
//! the board, the moves, Find this position and the search store need nothing of their own.
//! Nothing is read whole but the names: the index streams by, a game is read where it is.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use cozy_chess::{Board, Color, Move, Piece, Square};

const MAGIC_INDEX: &[u8] = b"Scid.si\0";
const MAGIC_NAMES: &[u8] = b"Scid.sn\0";
const HEADER: u64 = 182;
const ENTRY: u64 = 47;
const NAMES_HEADER: usize = 36;
const ENTRY5: u64 = 56;
/// The most bytes of a names file read: SCID's largest bases keep a few tens of MB of names.
pub const MAX_NAMES: u64 = 256 << 20;
/// The most names, of all four kinds, a names file (SCID 4 or 5) may hold: far more than the
/// largest databases have, and a few hundred megabytes at most in memory.
const MAX_NAME_COUNT: u32 = 1 << 24;

/// Whether `path` is a SCID index file by its name.
pub fn is_scid(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("si4") || e.eq_ignore_ascii_case("si5"))
}

/// Whether `path` is a SCID 5 index (`.si5`).
fn is_scid5(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("si5"))
}

/// A game's index entry: where its moves are and what the table shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// Where the game starts in the games file: SCID 5's can be past 4 GiB.
    pub offset: u64,
    pub len: u32,
    pub white: u32,
    pub black: u32,
    pub event: u32,
    pub site: u32,
    pub round: u32,
    /// 0 none, 1 White won, 2 Black won, 3 a draw.
    pub result: u8,
    /// `C41`, or empty.
    pub eco: String,
    /// `1858.11.02`, unknown parts `??`.
    pub date: String,
    pub white_elo: u16,
    pub black_elo: u16,
    pub plies: u32,
    /// The event's date (SCID 5), `2026.10.01`; empty when not known.
    pub event_date: String,
    /// Deleted in SCID but still in the files (SCID 5): not shown.
    pub deleted: bool,
}

/// A packed date (year 11 bits, month 4, day 5) as PGN writes one, unknown parts `??`.
fn date(d: u32) -> String {
    let (y, m, day) = (d >> 9 & 0x7ff, (d >> 5) & 15, d & 31);
    let part = |v: u32, w: usize| if v == 0 { "?".repeat(w) } else { format!("{v:0w$}") };
    format!("{}.{}.{}", part(y, 4), part(m.min(12), 2), part(day, 2))
}

/// An ECO code from SCID's number: 131 values for each code (it and SCID's extensions).
fn eco(e: u32) -> String {
    match e {
        0 => String::new(),
        e => {
            let basic = (e - 1) / 131;
            if basic < 500 { format!("{}{:02}", (b'A' + (basic / 100) as u8) as char, basic % 100) } else { String::new() }
        }
    }
}

impl Entry {
    fn parse(b: &[u8; ENTRY as usize]) -> Entry {
        let u16_at = |i: usize| u16::from_be_bytes([b[i], b[i + 1]]) as u32;
        let u32_at = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let eco = eco(u16_at(23));
        Entry {
            offset: u32_at(0) as u64,
            len: u16_at(4) | ((b[6] as u32 & 0x80) << 9),
            white: ((b[9] as u32 >> 4) << 16) | u16_at(10),
            black: ((b[9] as u32 & 15) << 16) | u16_at(12),
            event: ((b[14] as u32 >> 5) << 16) | u16_at(15),
            site: (((b[14] as u32 >> 2) & 7) << 16) | u16_at(17),
            round: ((b[14] as u32 & 3) << 16) | u16_at(19),
            result: (u16_at(21) >> 12) as u8 & 3,
            eco,
            date: date(u32_at(25) & 0xF_FFFF),
            white_elo: (u16_at(29) & 0xFFF) as u16,
            black_elo: (u16_at(31) & 0xFFF) as u16,
            plies: b[37] as u32 | ((b[38] as u32 >> 6) << 8),
            event_date: String::new(),
            deleted: false,
        }
    }

    /// A SCID 5 entry: 14 little-endian words. The ids of White, Black, the event, the site
    /// and the round (its top bit: Chess960); White's Elo and the date; Black's Elo and the
    /// event's date; the plies and flags (bit 3: deleted); the game's length above the top 15
    /// bits of its place, then the low 32 bits of its place; the result and ECO code in word 11.
    fn parse5(b: &[u8; ENTRY5 as usize]) -> Entry {
        let w = |i: usize| u32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
        let event_date = match w(6) & 0xF_FFFF {
            0 => String::new(),
            d => date(d),
        };
        Entry {
            offset: ((w(8) & 0x7FFF) as u64) << 32 | w(9) as u64,
            len: w(8) >> 15,
            white: w(0) & 0x0FFF_FFFF,
            black: w(1) & 0x0FFF_FFFF,
            event: w(2) & 0x0FFF_FFFF,
            site: w(3),
            round: w(4) & 0x7FFF_FFFF,
            result: (w(11) >> 16) as u8 & 3,
            eco: eco(w(11) & 0xFFFF),
            date: date(w(5) & 0xF_FFFF),
            white_elo: (w(5) >> 20) as u16,
            black_elo: (w(6) >> 20) as u16,
            plies: w(7) >> 22,
            event_date,
            deleted: w(7) & 0x08 != 0,
        }
    }
}

/// An open SCID database: its names, read at once, and where its index and games are.
pub struct Base {
    index: PathBuf,
    games: PathBuf,
    /// How many games the index has (by its header, at most what its size holds).
    pub count: u32,
    /// Players, events, sites and rounds, by id.
    names: [Vec<Box<str>>; 4],
    /// A SCID 5 base.
    v5: bool,
}

const PLAYER: usize = 0;
const EVENT: usize = 1;
const SITE: usize = 2;
const ROUND: usize = 3;

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("not a SCID database: {what}"))
}

impl Base {
    /// The database whose index is at `path` (`x.si4`, beside `x.sg4` and `x.sn4`).
    pub fn open(path: &Path) -> io::Result<Base> {
        if is_scid5(path) {
            let len = std::fs::metadata(path)?.len();
            let names = read_names5(&path.with_extension("sn5"))?;
            let count = (len / ENTRY5).min(u32::MAX as u64) as u32;
            return Ok(Base { index: path.to_path_buf(), games: path.with_extension("sg5"), count, names, v5: true });
        }
        let mut f = File::open(path)?;
        let mut head = [0u8; HEADER as usize];
        f.read_exact(&mut head).map_err(|_| bad("index too short"))?;
        if &head[..8] != MAGIC_INDEX {
            return Err(bad("index"));
        }
        let version = u16::from_be_bytes([head[8], head[9]]);
        if !(300..500).contains(&version) {
            return Err(bad("index version"));
        }
        let stated = u32::from_be_bytes([0, head[14], head[15], head[16]]);
        let fits = (f.metadata()?.len().saturating_sub(HEADER) / ENTRY) as u32;
        let names = read_names(&path.with_extension("sn4"))?;
        Ok(Base { index: path.to_path_buf(), games: path.with_extension("sg4"), count: stated.min(fits), names, v5: false })
    }

    fn name(&self, kind: usize, id: u32) -> &str {
        self.names[kind].get(id as usize).map_or("", |s| s)
    }

    pub fn white(&self, e: &Entry) -> &str {
        self.name(PLAYER, e.white)
    }
    pub fn black(&self, e: &Entry) -> &str {
        self.name(PLAYER, e.black)
    }
    pub fn event(&self, e: &Entry) -> &str {
        self.name(EVENT, e.event)
    }
    pub fn site(&self, e: &Entry) -> &str {
        self.name(SITE, e.site)
    }

    /// The bytes of one game's entry in the index.
    pub fn entry_len(&self) -> u64 {
        if self.v5 { ENTRY5 } else { ENTRY }
    }

    /// The games' entries in order, each handed to `each` (with its number, from 0) until it
    /// says to stop. The index is read as it streams by.
    pub fn entries(&self, mut each: impl FnMut(u32, &Entry) -> bool) -> io::Result<()> {
        let mut r = BufReader::with_capacity(1 << 16, File::open(&self.index)?);
        r.seek(SeekFrom::Start(if self.v5 { 0 } else { HEADER }))?;
        for n in 0..self.count {
            let Ok(e) = self.read_entry(&mut r) else { break };
            if !e.deleted && !each(n, &e) {
                break;
            }
        }
        Ok(())
    }

    fn read_entry(&self, r: &mut impl Read) -> io::Result<Entry> {
        if self.v5 {
            let mut b = [0u8; ENTRY5 as usize];
            r.read_exact(&mut b)?;
            return Ok(Entry::parse5(&b));
        }
        let mut b = [0u8; ENTRY as usize];
        r.read_exact(&mut b)?;
        Ok(Entry::parse(&b))
    }

    /// The entry of game `n` (from 0).
    pub fn entry(&self, n: u32) -> io::Result<Entry> {
        if n >= self.count {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no such game"));
        }
        let mut f = File::open(&self.index)?;
        f.seek(SeekFrom::Start(if self.v5 { n as u64 * ENTRY5 } else { HEADER + n as u64 * ENTRY }))?;
        self.read_entry(&mut f)
    }

    /// Game `n` (from 0) as PGN text: its tags, its moves with variations, comments and
    /// annotation glyphs. Moves the reader cannot play end the game there.
    pub fn pgn(&self, n: u32) -> io::Result<String> {
        let e = self.entry(n)?;
        let mut f = File::open(&self.games)?;
        f.seek(SeekFrom::Start(e.offset))?;
        let mut bytes = Vec::with_capacity(e.len as usize);
        f.take(e.len as u64).read_to_end(&mut bytes)?;
        Ok(self.pgn_of(&e, &bytes))
    }

    fn pgn_of(&self, e: &Entry, bytes: &[u8]) -> String {
        let result = ["*", "1-0", "0-1", "1/2-1/2"][e.result as usize & 3];
        let game = decode(bytes);
        let mut out = String::new();
        let mut tag = |k: &str, v: &str| {
            let v: String = v.chars().filter(|c| !c.is_control()).map(|c| if c == '"' { '\'' } else { c }).collect();
            out.push_str(&format!("[{k} \"{}\"]\n", v.replace('\\', "/")));
        };
        let or_q = |s: &str| if s.is_empty() { "?".to_string() } else { s.to_string() };
        tag("Event", &or_q(self.event(e)));
        tag("Site", &or_q(self.site(e)));
        tag("Date", &e.date);
        tag("Round", &or_q(self.name(ROUND, e.round)));
        tag("White", &or_q(self.white(e)));
        tag("Black", &or_q(self.black(e)));
        tag("Result", result);
        if e.white_elo > 0 {
            tag("WhiteElo", &e.white_elo.to_string());
        }
        if e.black_elo > 0 {
            tag("BlackElo", &e.black_elo.to_string());
        }
        if !e.eco.is_empty() {
            tag("ECO", &e.eco);
        }
        if !e.event_date.is_empty() && !game.tags.iter().any(|(k, _)| k.eq_ignore_ascii_case("EventDate")) {
            tag("EventDate", &e.event_date);
        }
        for (k, v) in &game.tags {
            if !k.eq_ignore_ascii_case("SetUp") && !k.eq_ignore_ascii_case("FEN") && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !k.is_empty() {
                tag(k, v);
            }
        }
        if let Some(fen) = &game.fen {
            tag("SetUp", "1");
            tag("FEN", fen);
        }
        out.push('\n');
        out.push_str(&game.moves);
        out.push_str(result);
        out.push('\n');
        out
    }
}

/// The names file: four lists (players, events, sites, rounds), each sorted and front-coded:
/// a name's id, its count (1, 2 or 3 bytes by the largest), its length, the bytes it shares
/// with the name before (not for the first), and the rest of it.
fn read_names(path: &Path) -> io::Result<[Vec<Box<str>>; 4]> {
    let mut bytes = vec![];
    File::open(path)?.take(MAX_NAMES).read_to_end(&mut bytes)?;
    names_of(&bytes)
}

// spike: the same reader, from bytes.
pub(crate) fn names_of(bytes: &[u8]) -> io::Result<[Vec<Box<str>>; 4]> {
    if bytes.len() < NAMES_HEADER || &bytes[..8] != MAGIC_NAMES {
        return Err(bad("names"));
    }
    let u24 = |i: usize| u32::from_be_bytes([0, bytes[i], bytes[i + 1], bytes[i + 2]]);
    // Four counts of up to 16M each: refused past the most names in all, not gigabytes asked for.
    if (0..4).map(|k| u24(12 + 3 * k)).sum::<u32>() > MAX_NAME_COUNT {
        return Err(bad("names"));
    }
    let mut out: [Vec<Box<str>>; 4] = Default::default();
    let mut at = NAMES_HEADER;
    for (kind, list) in out.iter_mut().enumerate() {
        let count = u24(12 + 3 * kind);
        let most = u24(24 + 3 * kind);
        let id_len = if count >= 1 << 16 { 3 } else { 2 };
        let freq_len = if most >= 1 << 16 { 3 } else if most >= 1 << 8 { 2 } else { 1 };
        // At most one name a byte left: a made-up count cannot ask for gigabytes.
        let count = (count as usize).min(bytes.len().saturating_sub(at));
        list.resize(count, "".into());
        let mut last: Vec<u8> = vec![];
        for i in 0..count {
            let need = id_len + freq_len + 1 + usize::from(i > 0);
            let Some(h) = bytes.get(at..at + need) else { return Ok(out) };
            let id = h[..id_len].iter().fold(0usize, |a, &b| a << 8 | b as usize);
            let len = h[id_len + freq_len] as usize;
            let shared = if i > 0 { (h[need - 1] as usize).min(len).min(last.len()) } else { 0 };
            at += need;
            let Some(rest) = bytes.get(at..at + (len - shared)) else { return Ok(out) };
            at += len - shared;
            last.truncate(shared);
            last.extend_from_slice(rest);
            if let Some(slot) = list.get_mut(id) {
                *slot = super::games::text(&last).trim().into();
            }
        }
    }
    Ok(out)
}

/// SCID 5's names file: the names one after another, each after one number in 7-bit groups
/// (low first, the top bit set while more follow): its length times 8 plus its kind (0 player,
/// 1 event, 2 site, 3 round; 4 the base's own facts, such as its description, skipped). A
/// name's id is its place among those of its kind.
fn read_names5(path: &Path) -> io::Result<[Vec<Box<str>>; 4]> {
    let mut bytes = vec![];
    File::open(path)?.take(MAX_NAMES).read_to_end(&mut bytes)?;
    let mut out: [Vec<Box<str>>; 4] = Default::default();
    let (mut at, mut names) = (0, 0u32);
    while at < bytes.len() {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let Some(&b) = bytes.get(at) else { return Ok(out) };
            at += 1;
            v |= ((b & 0x7f) as u64) << shift;
            shift += 7;
            if b & 0x80 == 0 || shift > 35 {
                break;
            }
        }
        let (len, kind) = ((v >> 3) as usize, (v & 7) as usize);
        let Some(name) = bytes.get(at..at.saturating_add(len)) else { break };
        // A name may be a byte long: without a cap, a file of such names would ask for gigabytes.
        names += 1;
        if names > MAX_NAME_COUNT {
            return Err(bad("names"));
        }
        if let Some(list) = out.get_mut(kind) {
            list.push(super::games::text(name).trim().into());
        }
        at += len;
    }
    Ok(out)
}

/// What a game record holds, its moves already written as PGN movetext.
#[derive(Default)]
struct Decoded {
    tags: Vec<(String, String)>,
    fen: Option<String>,
    moves: String,
}

/// The names of the tags SCID writes as one byte, 241 and on.
const COMMON: [&str; 10] = ["WhiteCountry", "BlackCountry", "Annotator", "PlyCount", "EventDate", "Opening", "Variation", "Setup", "Source", "SetUp"];

/// The position, and each side's pieces as SCID numbers them: in the start position the king
/// 0, the other pieces from a to h, then the pawns; from a FEN, the pieces in the FEN's order
/// (from a8 on), the king put first and the piece it displaces put where the king came.
#[derive(Clone)]
struct Pos {
    board: Board,
    list: [Vec<Square>; 2],
}

impl Pos {
    fn new(board: Board, standard: bool) -> Option<Pos> {
        let mut list: [Vec<Square>; 2] = Default::default();
        for (c, color) in [Color::White, Color::Black].into_iter().enumerate() {
            let l = &mut list[c];
            if standard {
                let (back, pawns) = if c == 0 { (0, 1) } else { (7, 6) };
                l.push(Square::index(back * 8 + 4));
                l.extend([0, 1, 2, 3, 5, 6, 7].map(|f| Square::index(back * 8 + f)));
                l.extend((0..8).map(|f| Square::index(pawns * 8 + f)));
                continue;
            }
            for rank in (0..8).rev() {
                for file in 0..8 {
                    let sq = Square::index(rank * 8 + file);
                    if board.color_on(sq) == Some(color) {
                        l.push(sq);
                        if board.piece_on(sq) == Some(Piece::King) {
                            let k = l.len() - 1;
                            l.swap(0, k);
                        }
                    }
                }
            }
            if l.len() > 16 || l.is_empty() {
                return None;
            }
        }
        Some(Pos { board, list })
    }

    /// Plays the move byte `b` (and the next one, for a queen's diagonal move, taken from
    /// `next`): its SAN, or None when it is no move here.
    fn play(&mut self, b: u8, next: &mut impl Iterator<Item = u8>) -> Option<String> {
        let side = self.board.side_to_move();
        let c = side as usize;
        let from = *self.list[c].get((b >> 4) as usize)?;
        let v = (b & 15) as i32;
        let (f, r) = (from.file() as i32, from.rank() as i32);
        let sq = |f: i32, r: i32| ((0..8).contains(&f) && (0..8).contains(&r)).then(|| Square::index((r * 8 + f) as usize));
        let piece = self.board.piece_on(from)?;
        let mut promo = None;
        let to = match piece {
            Piece::King if v == 0 => {
                let nb = self.board.null_move()?;
                self.board = nb;
                return Some("--".into());
            }
            Piece::King if v <= 8 => {
                let (df, dr) = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)][v as usize - 1];
                sq(f + df, r + dr)?
            }
            // Castling: cozy-chess moves the king onto its rook.
            Piece::King => {
                if v > 10 {
                    return None;
                }
                let mut rook = None;
                self.board.generate_moves_for(from.bitboard(), |ms| {
                    for m in ms {
                        if self.board.color_on(m.to) == Some(side) && (m.to.file() > from.file()) == (v == 10) {
                            rook = Some(m.to);
                        }
                    }
                    false
                });
                rook?
            }
            Piece::Knight => {
                let (df, dr) = *[(-1, -2), (1, -2), (-2, -1), (2, -1), (-2, 1), (2, 1), (-1, 2), (1, 2)].get((v as usize).checked_sub(1)?)?;
                sq(f + df, r + dr)?
            }
            Piece::Rook | Piece::Queen => {
                let t = if v < 8 { sq(v, r)? } else { sq(f, v - 8)? };
                if t == from {
                    // A queen's diagonal move: the square in the next byte, plus 64.
                    if piece != Piece::Queen {
                        return None;
                    }
                    Square::try_index(next.next()?.checked_sub(64)? as usize)?
                } else {
                    t
                }
            }
            Piece::Bishop => {
                let d = (v & 7) - f;
                if v < 8 { sq(f + d, r + d)? } else { sq(f + d, r - d)? }
            }
            Piece::Pawn => {
                let fwd = if side == Color::White { 1 } else { -1 };
                if v == 15 {
                    sq(f, r + 2 * fwd)?
                } else {
                    promo = [None, Some(Piece::Queen), Some(Piece::Rook), Some(Piece::Bishop), Some(Piece::Knight)][(v / 3) as usize];
                    // 7, 8 or 9 squares on for White, back for Black.
                    sq(f + (v % 3 - 1) * fwd, r + fwd)?
                }
            }
        };
        let mut found = None;
        self.board.generate_moves_for(from.bitboard(), |ms| {
            for m in ms {
                if m.to == to && m.promotion == promo {
                    found = Some(m);
                }
            }
            found.is_some()
        });
        let mv: Move = found?;
        let san = super::pgn::san(&self.board, mv);
        // The pieces' squares after it: a taken piece leaves its side's list, the last one
        // taking its place.
        let other = 1 - c;
        let castle = self.board.color_on(mv.to) == Some(side);
        let taken = if self.board.color_on(mv.to) == Some(!side) {
            Some(mv.to)
        } else if piece == Piece::Pawn && mv.from.file() != mv.to.file() {
            sq(mv.to.file() as i32, r)
        } else {
            None
        };
        if let Some(t) = taken
            && let Some(i) = self.list[other].iter().position(|&s| s == t)
        {
            self.list[other].swap_remove(i);
        }
        let idx = (b >> 4) as usize;
        if castle {
            let king_side = mv.to.file() > mv.from.file();
            let (kf, rf) = if king_side { (6, 5) } else { (2, 3) };
            if let Some(i) = self.list[c].iter().position(|&s| s == mv.to) {
                self.list[c][i] = sq(rf, r)?;
            }
            self.list[c][idx] = sq(kf, r)?;
        } else {
            self.list[c][idx] = mv.to;
        }
        self.board.play_unchecked(mv);
        Some(san)
    }
}

/// A game record: its extra tags, start position and moves as PGN movetext.
fn decode(bytes: &[u8]) -> Decoded {
    let mut out = Decoded::default();
    let mut it = bytes.iter().copied();
    // Extra tags, up to a 0: a name (its length, or 241… for a common one) and a value.
    loop {
        let Some(l) = it.next() else { return out };
        if l == 0 {
            break;
        }
        if l == 255 {
            // EventDate in three bytes: the table has no use for it.
            for _ in 0..3 {
                it.next();
            }
            continue;
        }
        let name = if l <= 240 {
            let n: Vec<u8> = it.by_ref().take(l as usize).collect();
            super::games::text(&n).into_owned()
        } else {
            COMMON.get(l as usize - 241).map_or(String::new(), |s| s.to_string())
        };
        let Some(vl) = it.next() else { return out };
        let v: Vec<u8> = it.by_ref().take(vl as usize).collect();
        if !name.is_empty() && out.tags.len() < 64 {
            out.tags.push((name, super::games::text(&v).into_owned()));
        }
    }
    let Some(flags) = it.next() else { return out };
    let start = if flags & 1 != 0 {
        let fen: Vec<u8> = it.by_ref().take_while(|&b| b != 0).take(256).collect();
        let fen = String::from_utf8_lossy(&fen).into_owned();
        out.fen = Some(fen.clone());
        super::pgn::board(&fen)
    } else {
        Some(Board::default())
    };
    let Some(mut cur) = start.and_then(|b| Pos::new(b, flags & 1 == 0)) else { return out };
    // The words of the movetext; `None` where a comment goes, its text read after the moves.
    let mut words: Vec<Option<String>> = vec![];
    let mut prev: Option<Pos> = None;
    let mut stack: Vec<(Pos, Option<Pos>)> = vec![];
    let mut moves = 0usize;
    let mut ended = false;
    while let Some(b) = it.next() {
        match b {
            11 => {
                let Some(nag) = it.next() else { break };
                words.push(Some(format!("${nag}")));
            }
            12 => words.push(None),
            13 => {
                // A variation: the last move played otherwise.
                let Some(p) = prev.clone() else { break };
                if stack.len() >= super::pgn::MAX_DEPTH as usize {
                    break;
                }
                stack.push((std::mem::replace(&mut cur, p), prev.take()));
                words.push(Some("(".into()));
            }
            14 => {
                let Some((c, p)) = stack.pop() else { break };
                (cur, prev) = (c, p);
                words.push(Some(")".into()));
            }
            15 => {
                ended = true;
                break;
            }
            _ => {
                if moves >= super::pgn::MAX_MOVES {
                    break;
                }
                let before = cur.clone();
                let white = before.board.side_to_move() == Color::White;
                let number = before.board.fullmove_number();
                let Some(san) = cur.play(b, &mut it) else { break };
                moves += 1;
                words.push(Some(if white { format!("{number}.{san}") } else { format!("{number}...{san}") }));
                prev = Some(before);
            }
        }
    }
    for _ in 0..stack.len() {
        words.push(Some(")".into()));
    }
    // The comments, in order, each ending in a 0; only when the moves were all read.
    let mut text = String::new();
    for w in words {
        match w {
            Some(w) => text.push_str(&w),
            None if ended => {
                let c: Vec<u8> = it.by_ref().take_while(|&b| b != 0).collect();
                let c = super::games::text(&c).replace('}', ")");
                if c.trim().is_empty() {
                    continue;
                }
                text.push('{');
                text.push_str(c.trim());
                text.push('}');
            }
            None => continue,
        }
        text.push(' ');
    }
    out.moves = text;
    out
}


// spike: the games table and the full decode, from the three files' bytes.
pub fn count_of(si4: &[u8]) -> u32 {
    if si4.len() < HEADER as usize || &si4[..8] != MAGIC_INDEX {
        return 0;
    }
    let stated = u32::from_be_bytes([0, si4[14], si4[15], si4[16]]);
    stated.min(((si4.len() as u64 - HEADER) / ENTRY) as u32)
}

pub fn rows(si4: &[u8], sn4: &[u8], sg4: &[u8], from: u64, to: u64, full: bool) -> io::Result<Vec<crate::Row>> {
    let count = count_of(si4);
    let base = Base { index: PathBuf::new(), games: PathBuf::new(), count, names: names_of(sn4)?, v5: false };
    let mut out = Vec::with_capacity(to.saturating_sub(from) as usize);
    for n in from..to.min(count as u64) {
        let at = (HEADER + n * ENTRY) as usize;
        let e = Entry::parse(si4[at..at + ENTRY as usize].try_into().unwrap());
        let mut plies = e.plies;
        if full {
            let g = sg4.get(e.offset as usize..(e.offset + e.len as u64) as usize).unwrap_or(&[]);
            plies = crate::plies(&base.pgn_of(&e, g));
        }
        out.push(crate::Row {
            white: base.white(&e).into(),
            black: base.black(&e).into(),
            event: base.event(&e).into(),
            site: base.site(&e).into(),
            date: e.date.clone(),
            result: ["*", "1-0", "0-1", "1/2-1/2"][e.result as usize & 3].into(),
            eco: e.eco.clone(),
            plies,
        });
    }
    Ok(out)
}

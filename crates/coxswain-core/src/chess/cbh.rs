//! ChessBase databases (docs/previews/chess.md#chessbase-databases): a `.cbh` file and the files
//! beside it under the same name, `.cbg` (the moves), `.cbp` (players) and `.cbt` (tournaments).
//! Each game is read as the PGN it holds, so the table, the board and the search take it as they
//! take a PGN file's games (`games`). Read only; annotations (`.cba`) are not read.
//!
//! The layout follows the public descriptions of the format: the MIT-licensed `cbvault`
//! (github.com/itshak/cbvault, from `oschess-cb-bridge`) and `asdfjkl/cbh2pgn`. Facts only: no
//! code of theirs is used. The three byte tables below are data from them (MIT, "Copyright (c)
//! 2026 cbvault Contributors"; `asdfjkl/cbh2pgn` is credited there for the table data).
//!
//! Every file is untrusted: sizes and ids are checked before they are used, a game record is
//! read up to `games::MAX_GAME` bytes, and a game stops at `pgn::MAX_MOVES` moves and
//! `pgn::MAX_DEPTH` levels of variations. A game whose moves cannot be read to the end shows the
//! moves read and says so.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use cozy_chess::{Board, Color, Move, Piece, Rank, Square};

use super::pgn;

/// The size of a `.cbh` record; the file starts with one of its own.
const RECORD: u64 = 46;
/// The most bytes of an entity record (players' are 58, tournaments' 90).
const MAX_ENTITY: u64 = 64 << 10;
/// Names kept from the entity files while the games are listed.
const MAX_CACHE: usize = 200_000;

/// Whether `path` is a ChessBase database's header file, by its name.
pub fn is_cbh(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("cbh"))
}

/// The file beside `cbh` with the ending `ext`: the same case first, else any case.
fn sibling(cbh: &Path, ext: &str) -> PathBuf {
    let exact = cbh.with_extension(ext);
    if exact.exists() {
        return exact;
    }
    let upper = cbh.with_extension(ext.to_ascii_uppercase());
    if upper.exists() { upper } else { exact }
}

fn be(b: &[u8]) -> u32 {
    b.iter().fold(0, |n, &x| n << 8 | x as u32)
}

fn le_i32(b: &[u8], at: usize) -> i32 {
    b.get(at..at + 4).map_or(0, |x| i32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

/// Text up to its first NUL: UTF-8, else Windows-1252.
fn text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    super::games::text(&b[..end]).trim().to_string()
}

/// An entity file (`.cbp`, `.cbt`): a header, then records of the same size; an id is a
/// record's place.
struct Entities {
    file: Option<File>,
    header: u64,
    record: u64,
    count: u64,
    cache: HashMap<u32, Option<Vec<u8>>>,
}

impl Entities {
    fn open(path: &Path) -> Entities {
        let none = Entities { file: None, header: 0, record: 1, count: 0, cache: HashMap::new() };
        let Ok(mut f) = File::open(path) else { return none };
        let mut h = [0u8; 28];
        let len = f.metadata().map_or(0, |m| m.len());
        if f.read_exact(&mut h).is_err() {
            return none;
        }
        let data = le_i32(&h, 0x0c);
        let extra = le_i32(&h, 0x18);
        if data <= 0 || data as u64 > MAX_ENTITY || !(extra == 0 || extra == 4) {
            return none;
        }
        let (header, record) = (28 + extra as u64, 9 + data as u64);
        Entities { file: Some(f), header, record, count: len.saturating_sub(header) / record, cache: HashMap::new() }
    }

    /// The data of entity `id`; None past the file or for a deleted one.
    fn get(&mut self, id: u32) -> Option<Vec<u8>> {
        if let Some(d) = self.cache.get(&id) {
            return d.clone();
        }
        if self.cache.len() >= MAX_CACHE {
            self.cache.clear();
        }
        let d = self.read(id);
        self.cache.insert(id, d.clone());
        d
    }

    fn read(&mut self, id: u32) -> Option<Vec<u8>> {
        if id as u64 >= self.count {
            return None;
        }
        let f = self.file.as_mut()?;
        f.seek(SeekFrom::Start(self.header + id as u64 * self.record)).ok()?;
        let mut r = vec![0; self.record as usize];
        f.read_exact(&mut r).ok()?;
        // A deleted record's left link.
        if le_i32(&r, 0) == -999 {
            return None;
        }
        Some(r.split_off(9))
    }

    /// A player as PGN names one: `Last, First`.
    fn player(&mut self, id: u32) -> String {
        let Some(d) = self.get(id) else { return String::new() };
        let last = text(&d[..d.len().min(30)]);
        let first = text(d.get(30..50).unwrap_or_default());
        if first.is_empty() { last } else { format!("{last}, {first}") }
    }

    /// A tournament: its title and its place.
    fn tournament(&mut self, id: u32) -> (String, String) {
        let Some(d) = self.get(id) else { return Default::default() };
        (text(&d[..d.len().min(40)]), text(d.get(40..70).unwrap_or_default()))
    }
}

/// A game as the `.cbh` file lists it: its record, its tags, and its length in moves.
pub(crate) struct Listed {
    pub record: u32,
    pub tags: Vec<(&'static str, String)>,
    pub moves: u32,
}

/// The names a database's games refer to.
struct Names {
    players: Entities,
    tournaments: Entities,
}

impl Names {
    fn open(cbh: &Path) -> Names {
        Names { players: Entities::open(&sibling(cbh, "cbp")), tournaments: Entities::open(&sibling(cbh, "cbt")) }
    }
}

/// A packed date (day 5 bits, month 4, year 12) as PGN writes one.
fn date(v: u32) -> String {
    let (d, m, y) = (v & 31, v >> 5 & 15, v >> 9 & 0xfff);
    let part = |n: u32, w: usize| if n == 0 { "?".repeat(w) } else { format!("{n:0w$}") };
    format!("{}.{}.{}", part(y, 4), part(m, 2), part(d, 2))
}

fn result(v: u8) -> &'static str {
    match v {
        0 | 4 => "0-1",
        1 | 5 => "1/2-1/2",
        2 | 6 => "1-0",
        _ => "*",
    }
}

/// The ECO field as its code, `A00`..`E99`; empty for none.
fn eco(v: u32) -> String {
    match v {
        128..=64127 if v / 128 - 1 < 500 => {
            let c = v / 128 - 1;
            format!("{}{:02}", (b'A' + (c / 100) as u8) as char, c % 100)
        }
        _ => String::new(),
    }
}

/// Whether record `r` is a game: not deleted, not a text.
fn is_game(r: &[u8]) -> bool {
    r[0] & 0x80 == 0 && r[0] & 0x02 == 0 && r[0] & 0x01 == 1
}

fn tags(r: &[u8], names: &mut Names) -> Vec<(&'static str, String)> {
    let (event, site) = names.tournaments.tournament(be(&r[0x0f..0x12]));
    let round = match (r[0x1d], r[0x1e]) {
        (0, _) => String::new(),
        (n, 0) => n.to_string(),
        (n, s) => format!("{n}.{s}"),
    };
    let elo = |at: usize| match be(&r[at..at + 2]) {
        0 => String::new(),
        n => n.to_string(),
    };
    vec![
        ("Event", event),
        ("Site", site),
        ("Date", date(be(&r[0x18..0x1b]))),
        ("Round", round),
        ("White", names.players.player(be(&r[0x09..0x0c]))),
        ("Black", names.players.player(be(&r[0x0c..0x0f]))),
        ("Result", result(r[0x1b]).to_string()),
        ("WhiteElo", elo(0x1f)),
        ("BlackElo", elo(0x21)),
        ("ECO", eco(be(&r[0x23..0x25]))),
    ]
}

/// The `.cbh` file, its own record checked: its records are 46 bytes.
fn open_cbh(path: &Path) -> io::Result<(File, u64)> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut h = [0u8; RECORD as usize];
    f.read_exact(&mut h).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "no ChessBase header"))?;
    if be(&h[3..5]) != RECORD as u32 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a ChessBase database"));
    }
    Ok((f, len / RECORD))
}

/// The games of the database at `path`, in order, each handed to `each` until it says to stop;
/// `read` is told the bytes of the `.cbh` file read so far.
pub(crate) fn scan(path: &Path, stop: &dyn Fn() -> bool, read: &dyn Fn(u64), mut each: impl FnMut(&Listed) -> bool) -> io::Result<()> {
    let (f, records) = open_cbh(path)?;
    let mut names = Names::open(path);
    let mut r = io::BufReader::with_capacity(1 << 16, f);
    let mut rec = [0u8; RECORD as usize];
    for n in 1..records.min(u32::MAX as u64) {
        if stop() {
            break;
        }
        r.read_exact(&mut rec)?;
        if n % 4096 == 0 {
            read((n + 1) * RECORD);
        }
        if !is_game(&rec) {
            continue;
        }
        let listed = Listed { record: n as u32, tags: tags(&rec, &mut names), moves: rec[0x2d] as u32 };
        if !each(&listed) {
            break;
        }
    }
    read(records * RECORD);
    Ok(())
}

/// Game `record` of the database at `path` as PGN: its tags, then its moves and variations.
pub fn pgn(path: &Path, record: u32) -> io::Result<String> {
    let (mut f, records) = open_cbh(path)?;
    if record == 0 || record as u64 >= records {
        return Err(io::Error::new(io::ErrorKind::NotFound, "no such game"));
    }
    let mut rec = [0u8; RECORD as usize];
    f.seek(SeekFrom::Start(record as u64 * RECORD))?;
    f.read_exact(&mut rec)?;
    if !is_game(&rec) {
        return Err(io::Error::new(io::ErrorKind::NotFound, "no such game"));
    }
    let mut tags = tags(&rec, &mut Names::open(path));
    let result = tags.iter().find(|t| t.0 == "Result").map_or("*".to_string(), |t| t.1.clone());
    let (game, cut) = match read_game(&sibling(path, "cbg"), be(&rec[1..5]) as u64) {
        Ok(bytes) => decode(&bytes),
        Err(_) => (Tree::default(), true),
    };
    if let Some(fen) = &game.fen {
        tags.push(("SetUp", "1".into()));
        tags.push(("FEN", fen.clone()));
    }
    let mut out = String::new();
    for (k, v) in tags.iter().filter(|t| !t.1.is_empty()) {
        out.push_str(&format!("[{k} \"{}\"]\n", v.replace('\\', "\\\\").replace('"', "\\\"")));
    }
    out.push('\n');
    if cut {
        out.push_str(&format!("{{{}}} ", crate::t!("chess.cbh_cut").replace(['{', '}'], "")));
    }
    out.push_str(&game.movetext());
    out.push_str(&result);
    out.push('\n');
    Ok(out)
}

/// A game's record in the `.cbg` file: a flags byte, its size in 3 bytes (these 4 counted), the
/// rest.
fn read_game(cbg: &Path, at: u64) -> io::Result<Vec<u8>> {
    let mut f = File::open(cbg)?;
    let len = f.metadata()?.len();
    if at < 4 || at + 4 > len {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "game out of the file"));
    }
    f.seek(SeekFrom::Start(at))?;
    let mut head = [0u8; 4];
    f.read_exact(&mut head)?;
    let size = be(&head[1..4]) as u64;
    if size < 4 || at + size > len || size > super::games::MAX_GAME as u64 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "game size"));
    }
    let mut out = head.to_vec();
    out.resize(size as usize, 0);
    f.read_exact(&mut out[4..])?;
    Ok(out)
}

/// A move of the tree: its SAN, the move it follows, the moves that follow it (the first is
/// the main one), and its number.
struct Node {
    san: String,
    parent: Option<u32>,
    children: Vec<u32>,
    white: bool,
    number: u32,
}

/// A game's moves as stored: the start's FEN when it is set up, and the moves in stored order.
#[derive(Default)]
struct Tree {
    fen: Option<String>,
    nodes: Vec<Node>,
    /// The first moves of the game.
    roots: Vec<u32>,
}

impl Tree {
    /// The moves as PGN, variations in brackets after the move they replace.
    fn movetext(&self) -> String {
        enum Step {
            /// A line from this move; with its later siblings as variations or not.
            Line(u32, bool, bool),
            Open,
            Close,
        }
        let mut out = String::new();
        let mut todo = vec![];
        if let Some(&first) = self.roots.first() {
            todo.push(Step::Line(first, true, true));
        }
        let siblings = |n: u32| -> &[u32] {
            match self.nodes[n as usize].parent {
                Some(p) => &self.nodes[p as usize].children,
                None => &self.roots,
            }
        };
        while let Some(step) = todo.pop() {
            match step {
                Step::Open => out.push_str("( "),
                Step::Close => out.push_str(") "),
                Step::Line(n, alternatives, numbered) => {
                    let node = &self.nodes[n as usize];
                    let alts: &[u32] = if alternatives { &siblings(n)[1..] } else { &[] };
                    if let Some(&next) = node.children.first() {
                        todo.push(Step::Line(next, true, !alts.is_empty()));
                    }
                    for &a in alts.iter().rev() {
                        todo.push(Step::Close);
                        todo.push(Step::Line(a, false, true));
                        todo.push(Step::Open);
                    }
                    if node.white {
                        out.push_str(&format!("{}. ", node.number));
                    } else if numbered {
                        out.push_str(&format!("{}... ", node.number));
                    }
                    out.push_str(&node.san);
                    out.push(' ');
                }
            }
        }
        out
    }
}

/// The pieces the compact encoding numbers, by side: queens, rooks, bishops and knights in the
/// order a scan of the squares finds them (a1, a2 … h8, a file at a time), and the pawns by a
/// number they keep all game. A taken piece makes the ones after it move down.
#[derive(Clone, Default)]
struct Lists {
    kinds: [[Vec<u8>; 4]; 2],
    pawns: [[Option<u8>; 8]; 2],
}

/// ChessBase's square number: a file at a time, a1 = 0, a2 = 1 … h8 = 63.
fn cb(s: Square) -> u8 {
    s.file() as u8 * 8 + s.rank() as u8
}

fn square(n: u8) -> Square {
    Square::new(cozy_chess::File::index((n / 8 % 8) as usize), Rank::index((n % 8) as usize))
}

fn kind(p: Piece) -> Option<usize> {
    match p {
        Piece::Queen => Some(0),
        Piece::Rook => Some(1),
        Piece::Bishop => Some(2),
        Piece::Knight => Some(3),
        _ => None,
    }
}

impl Lists {
    fn scan(b: &Board) -> Result<Lists, ()> {
        let mut l = Lists::default();
        let mut pawns = [0usize; 2];
        for n in 0..64 {
            let s = square(n);
            let (Some(p), Some(c)) = (b.piece_on(s), b.color_on(s)) else { continue };
            if p == Piece::Pawn {
                *l.pawns[c as usize].get_mut(pawns[c as usize]).ok_or(())? = Some(n);
                pawns[c as usize] += 1;
            } else if let Some(k) = kind(p) {
                l.kinds[c as usize][k].push(n);
            }
        }
        Ok(l)
    }

    /// Follows `mv` (legal on `b`, not yet played) through the lists.
    fn update(&mut self, b: &Board, mv: Move) -> Result<(), ()> {
        let us = b.side_to_move();
        let (me, them) = (us as usize, !us as usize);
        let (from, to) = (cb(mv.from), cb(mv.to));
        let moving = b.piece_on(mv.from).ok_or(())?;
        let target = b.piece_on(mv.to).zip(b.color_on(mv.to));
        if moving == Piece::King
            && let Some((Piece::Rook, c)) = target
            && c == us
        {
            // Castling: the king onto its own rook; the rook ends beside the king.
            let file = if mv.to.file() > mv.from.file() { 5 } else { 3 };
            let rook = self.kinds[me][1].iter_mut().find(|s| **s == to).ok_or(())?;
            *rook = file * 8 + mv.to.rank() as u8;
            return Ok(());
        }
        let taken = match target {
            Some((p, c)) if c != us => Some((p, to)),
            _ if moving == Piece::Pawn && mv.from.file() != mv.to.file() => Some((Piece::Pawn, mv.to.file() as u8 * 8 + mv.from.rank() as u8)),
            _ => None,
        };
        if let Some((p, at)) = taken {
            match kind(p) {
                Some(k) => {
                    let list = &mut self.kinds[them][k];
                    let i = list.iter().position(|&s| s == at).ok_or(())?;
                    list.remove(i);
                }
                None => *self.pawns[them].iter_mut().find(|s| **s == Some(at)).ok_or(())? = None,
            }
        }
        match kind(moving) {
            Some(k) => *self.kinds[me][k].iter_mut().find(|s| **s == from).ok_or(())? = to,
            None if moving == Piece::Pawn => {
                let slot = self.pawns[me].iter_mut().find(|s| **s == Some(from)).ok_or(())?;
                match mv.promotion.and_then(kind) {
                    Some(k) => {
                        *slot = None;
                        self.kinds[me][k].push(to);
                    }
                    None => *slot = Some(to),
                }
            }
            None => {}
        }
        Ok(())
    }
}

/// The byte tables of the encodings read: mode 0 and 4 (one byte a move), 5 (two).
const MODE_0: [u8; 256] = [
    0xa2, 0x95, 0x43, 0xf5, 0xc1, 0x3d, 0x4a, 0x6c, 0x53, 0x83, 0xcc, 0x7c, 0xff, 0xae, 0x68, 0xad,
    0xd1, 0x92, 0x8b, 0x8d, 0x35, 0x81, 0x5e, 0x74, 0x26, 0x8e, 0xab, 0xca, 0xfd, 0x9a, 0xf3, 0xa0,
    0xa5, 0x15, 0xfc, 0xb1, 0x1e, 0xed, 0x30, 0xea, 0x22, 0xeb, 0xa7, 0xcd, 0x4e, 0x6f, 0x2e, 0x24,
    0x32, 0x94, 0x41, 0x8c, 0x6e, 0x58, 0x82, 0x50, 0xbb, 0x02, 0x8a, 0xd8, 0xfa, 0x60, 0xde, 0x52,
    0xba, 0x46, 0xac, 0x29, 0x9d, 0xd7, 0xdf, 0x08, 0x21, 0x01, 0x66, 0xa3, 0xf1, 0x19, 0x27, 0xb5,
    0x91, 0xd5, 0x42, 0x0e, 0xb4, 0x4c, 0xd9, 0x18, 0x5f, 0xbc, 0x25, 0xa6, 0x96, 0x04, 0x56, 0x6a,
    0xaa, 0x33, 0x1c, 0x2b, 0x73, 0xf0, 0xdd, 0xa4, 0x37, 0xd3, 0xc5, 0x10, 0xbf, 0x5a, 0x23, 0x34,
    0x75, 0x5b, 0xb8, 0x55, 0xd2, 0x6b, 0x09, 0x3a, 0x57, 0x12, 0xb3, 0x77, 0x48, 0x85, 0x9b, 0x0f,
    0x9e, 0xc7, 0xc8, 0xa1, 0x7f, 0x7a, 0xc0, 0xbd, 0x31, 0x6d, 0xf6, 0x3e, 0xc3, 0x11, 0x71, 0xce,
    0x7d, 0xda, 0xa8, 0x54, 0x90, 0x97, 0x1f, 0x44, 0x40, 0x16, 0xc9, 0xe3, 0x2c, 0xcb, 0x84, 0xec,
    0x9f, 0x3f, 0x5c, 0xe6, 0x76, 0x0b, 0x3c, 0x20, 0xb7, 0x36, 0x00, 0xdc, 0xe7, 0xf9, 0x4f, 0xf7,
    0xaf, 0x06, 0x07, 0xe0, 0x1a, 0x0a, 0xa9, 0x4b, 0x0c, 0xd6, 0x63, 0x87, 0x89, 0x1d, 0x13, 0x1b,
    0xe4, 0x70, 0x05, 0x47, 0x67, 0x7b, 0x2f, 0xee, 0xe2, 0xe8, 0x98, 0x0d, 0xef, 0xcf, 0xc4, 0xf4,
    0xfb, 0xb0, 0x17, 0x99, 0x64, 0xf2, 0xd4, 0x2a, 0x03, 0x4d, 0x78, 0xc6, 0xfe, 0x65, 0x86, 0x88,
    0x79, 0x45, 0x3b, 0xe5, 0x49, 0x8f, 0x2d, 0xb9, 0xbe, 0x62, 0x93, 0x14, 0xe9, 0xd0, 0x38, 0x9c,
    0xb2, 0xc2, 0x59, 0x5d, 0xb6, 0x72, 0x51, 0xf8, 0x28, 0x7e, 0x61, 0x39, 0xe1, 0xdb, 0x69, 0x80,
];
const MODE_4: [u8; 256] = [
    0x58, 0xa3, 0xee, 0x72, 0x47, 0xb9, 0x1f, 0xa0, 0x26, 0xfd, 0xdc, 0x6d, 0xf1, 0xe3, 0xcb, 0xc5,
    0xe2, 0xd0, 0x29, 0xe7, 0x89, 0xe1, 0xae, 0x7b, 0x34, 0x41, 0x35, 0x4b, 0xa5, 0x7c, 0xf6, 0xe5,
    0xdb, 0x8c, 0x5f, 0x37, 0x05, 0xbb, 0x32, 0x54, 0x4c, 0xef, 0x5d, 0x2a, 0x7f, 0xd3, 0x81, 0x86,
    0xdf, 0x18, 0xc3, 0x44, 0x16, 0xf7, 0x85, 0x0f, 0x4d, 0x30, 0x79, 0x14, 0x45, 0xaa, 0xa2, 0x6c,
    0x15, 0x92, 0xa6, 0x62, 0x48, 0x19, 0xe9, 0x3c, 0x38, 0x94, 0x28, 0xa8, 0x66, 0x09, 0x03, 0x63,
    0x20, 0x7d, 0x36, 0xd7, 0x2e, 0x98, 0xec, 0xda, 0x01, 0x13, 0x24, 0x73, 0xcd, 0x9e, 0xcf, 0xd9,
    0xd5, 0x68, 0x8b, 0xd2, 0xab, 0x5b, 0x69, 0xb0, 0x27, 0xfc, 0x31, 0x1b, 0x53, 0x7e, 0x65, 0x76,
    0x55, 0x75, 0xaf, 0x0a, 0xcc, 0xc7, 0xc4, 0xb2, 0xb5, 0x22, 0xe8, 0xa7, 0xbe, 0xf5, 0x00, 0x5a,
    0x21, 0xd1, 0x74, 0x8e, 0xde, 0x9c, 0xe4, 0x42, 0x2b, 0xb1, 0x9b, 0x93, 0xc1, 0x78, 0x70, 0xd6,
    0xb6, 0xad, 0x4e, 0x6a, 0x64, 0x7a, 0x57, 0x49, 0x11, 0x9d, 0x12, 0xc0, 0x3a, 0x2f, 0xe6, 0x84,
    0xc6, 0x07, 0xce, 0x08, 0x82, 0x04, 0x90, 0xb8, 0x87, 0xca, 0x6e, 0x1e, 0x40, 0x1a, 0x50, 0xb4,
    0x10, 0xbc, 0x43, 0x8a, 0x71, 0x0b, 0x8d, 0x5c, 0x3b, 0x3f, 0x88, 0xba, 0xf4, 0xf3, 0x61, 0x1d,
    0xf9, 0xfe, 0x25, 0x51, 0x0d, 0xeb, 0xc8, 0x59, 0x17, 0x46, 0xa4, 0xf2, 0x56, 0xb7, 0x02, 0x2d,
    0x33, 0x9f, 0xd8, 0x83, 0xfb, 0xbf, 0xea, 0xed, 0xac, 0x60, 0x67, 0x39, 0xdd, 0x96, 0x6b, 0xa9,
    0xc2, 0x77, 0x0c, 0x0e, 0x91, 0xfa, 0x99, 0x9a, 0xe0, 0xa1, 0xf0, 0x5e, 0xc9, 0x06, 0x4f, 0xbd,
    0x52, 0x80, 0xd4, 0xf8, 0x97, 0x23, 0x3e, 0x1c, 0x2c, 0x95, 0x4a, 0x8f, 0x3d, 0x6f, 0xff, 0xb3,
];

const MODE_5: [u8; 256] = [
    0xe0, 0x1b, 0x97, 0x86, 0xa2, 0x48, 0xf0, 0x23, 0x01, 0x37, 0xfa, 0xeb, 0x50, 0x76, 0x5d, 0xcd,
    0x80, 0x1a, 0x31, 0x5c, 0xff, 0xbc, 0x78, 0xda, 0xe5, 0xa3, 0x75, 0xb3, 0x71, 0xe1, 0xa1, 0x0e,
    0x56, 0x41, 0x3f, 0xe3, 0x0d, 0xc4, 0x25, 0xe6, 0x88, 0x89, 0x70, 0xdc, 0x99, 0xa6, 0x0b, 0x9a,
    0x14, 0x2c, 0x57, 0x9e, 0x69, 0x7a, 0xdf, 0x55, 0xad, 0x73, 0x7c, 0xc5, 0x03, 0x72, 0xf5, 0x45,
    0x62, 0x8e, 0xaf, 0x5f, 0xcc, 0xc0, 0x5e, 0xf3, 0x18, 0x13, 0xb5, 0x38, 0x09, 0x28, 0x3c, 0xbd,
    0x10, 0x8b, 0xe9, 0xdd, 0x8c, 0x94, 0x08, 0x81, 0xa7, 0xfe, 0x24, 0x79, 0x44, 0x2b, 0xc1, 0x36,
    0x32, 0x7f, 0x67, 0xe4, 0xfd, 0x49, 0x4b, 0x21, 0xa9, 0x5b, 0x77, 0x1f, 0x1c, 0xf6, 0x40, 0x65,
    0x3d, 0xf9, 0x5a, 0xf8, 0x47, 0x26, 0x53, 0xd4, 0x20, 0xd2, 0xd8, 0x4f, 0x27, 0xae, 0xfb, 0xb6,
    0x9b, 0x74, 0xb8, 0x42, 0xfc, 0x16, 0x6f, 0xd7, 0x9f, 0xb9, 0x7b, 0x60, 0xde, 0xc2, 0x46, 0x06,
    0x9c, 0xd3, 0x2a, 0xc7, 0x05, 0x85, 0x3e, 0xb1, 0x00, 0x15, 0xd1, 0x96, 0xce, 0x90, 0x95, 0x11,
    0x33, 0x8f, 0xc6, 0x82, 0x84, 0xb7, 0x39, 0xa0, 0xe8, 0x34, 0x1e, 0x7d, 0xa8, 0xd0, 0xea, 0xe2,
    0xef, 0x4c, 0xc3, 0x58, 0x4e, 0x02, 0x1d, 0xb2, 0x7e, 0x98, 0x2d, 0x07, 0xd5, 0xca, 0x68, 0xcb,
    0x30, 0xb4, 0xac, 0x83, 0x63, 0xec, 0x87, 0xee, 0x2e, 0xbe, 0xcf, 0x6d, 0x4d, 0x8a, 0xd6, 0x04,
    0xb0, 0x35, 0xc8, 0xc9, 0x12, 0x17, 0x61, 0x8d, 0x64, 0x0f, 0xa5, 0x54, 0x29, 0x3a, 0x4a, 0x6b,
    0xbb, 0x2f, 0xd9, 0x3b, 0x93, 0x6a, 0xaa, 0xa4, 0x22, 0xf4, 0x19, 0x43, 0x6e, 0xf1, 0xf7, 0xdb,
    0x92, 0xed, 0x66, 0x59, 0x0a, 0xbf, 0x6c, 0x51, 0x91, 0x52, 0xf2, 0x0c, 0x9d, 0xab, 0xba, 0xe7,
];

/// Reads a game's record (`read_game`) into its tree; true when it could not be read to the end.
fn decode(rec: &[u8]) -> (Tree, bool) {
    let mut d = Decoder { tree: Tree::default(), cur: None, stack: vec![], branch_next: false, plies: 0, ended: false };
    let ok = d.run(rec).is_ok();
    (d.tree, !ok)
}

struct Decoder {
    tree: Tree,
    /// The move the next one follows; None at the start.
    cur: Option<u32>,
    /// Where each open variation starts: the move before it, the board and the lists there.
    stack: Vec<(Option<u32>, Board, Lists)>,
    branch_next: bool,
    /// Moves read, which the byte tables are keyed by.
    plies: u32,
    ended: bool,
}

const KING: [(i8, i8); 8] = [(0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0), (-1, 1)];
const KNIGHT: [(i8, i8); 8] = [(2, 1), (1, 2), (-1, 2), (-2, 1), (-2, -1), (-1, -2), (1, -2), (2, -1)];
const QUEEN: [(i8, i8); 4] = [(0, 1), (1, 0), (1, 1), (1, -1)];
const ROOK: [(i8, i8); 2] = [(0, 1), (1, 0)];
const BISHOP: [(i8, i8); 2] = [(1, 1), (1, -1)];

impl Decoder {
    fn run(&mut self, rec: &[u8]) -> Result<(), ()> {
        let flags = *rec.first().ok_or(())?;
        let mut at = 4;
        let mut board = Board::default();
        if flags & 0x40 != 0 {
            let start = rec.get(4..32).ok_or(())?;
            let fen = setup_fen(start).ok_or(())?;
            board = pgn::board(&fen).ok_or(())?;
            self.tree.fen = Some(board.to_string());
            at = 32;
        }
        let (table, pre, simple) = match flags & 0x3f {
            0 => (&MODE_0, true, false),
            4 => (&MODE_4, false, false),
            5 => (&MODE_5, false, true),
            // Chess960 (10, 11) and the rest are not read.
            _ => return Err(()),
        };
        let mut lists = Lists::scan(&board)?;
        let s = &rec[at..];
        let tr = |b: u8, plies: u32| {
            let n = plies as u8;
            if pre { table[b.wrapping_sub(n) as usize] } else { table[b as usize].wrapping_sub(n) }
        };
        let mut i = 0;
        while i < s.len() && !self.ended {
            if simple {
                let pair = s.get(i..i + 2).ok_or(())?;
                i += 2;
                let w = (tr(pair[0], self.plies) as u16) << 8 | tr(pair[1], self.plies) as u16;
                if w & 0x8000 != 0 {
                    self.start_variation()?;
                }
                let mv = by_squares(&board, w & 0x3fff)?;
                self.play(&mut board, &mut lists, mv)?;
                if w & 0x4000 != 0 {
                    self.end_line(&mut board, &mut lists)?;
                }
                continue;
            }
            let v = tr(s[i], self.plies);
            i += 1;
            match v {
                236 => {}
                254 => self.start_variation()?,
                255 => self.end_line(&mut board, &mut lists)?,
                235 => {
                    let pair = s.get(i..i + 2).ok_or(())?;
                    i += 2;
                    let w = (tr(pair[0], self.plies) as u16) << 8 | tr(pair[1], self.plies) as u16;
                    let mv = by_squares(&board, w)?;
                    self.play(&mut board, &mut lists, mv)?;
                }
                237..=253 => return Err(()),
                code => {
                    let mv = by_code(&board, &lists, code)?;
                    self.play(&mut board, &mut lists, mv)?;
                }
            }
        }
        // A game without moves may have no bytes at all; else its last line must be ended.
        if self.ended || s.is_empty() { Ok(()) } else { Err(()) }
    }

    fn start_variation(&mut self) -> Result<(), ()> {
        if self.branch_next || self.stack.len() >= pgn::MAX_DEPTH as usize {
            return Err(());
        }
        self.branch_next = true;
        Ok(())
    }

    fn end_line(&mut self, board: &mut Board, lists: &mut Lists) -> Result<(), ()> {
        if self.branch_next {
            return Err(());
        }
        match self.stack.pop() {
            Some((cur, b, l)) => (self.cur, *board, *lists) = (cur, b, l),
            None => self.ended = true,
        }
        Ok(())
    }

    /// Plays `mv` (None: a null move) and adds it to the tree.
    fn play(&mut self, board: &mut Board, lists: &mut Lists, mv: Option<Move>) -> Result<(), ()> {
        if self.tree.nodes.len() >= pgn::MAX_MOVES {
            return Err(());
        }
        let saved = self.branch_next.then(|| (self.cur, board.clone(), lists.clone()));
        self.branch_next = false;
        let (white, number) = (board.side_to_move() == Color::White, board.fullmove_number() as u32);
        let san = match mv {
            Some(mv) => {
                if !board.is_legal(mv) {
                    return Err(());
                }
                let san = pgn::san(board, mv);
                lists.update(board, mv)?;
                board.play_unchecked(mv);
                san
            }
            None => {
                *board = board.null_move().ok_or(())?;
                "--".into()
            }
        };
        let id = self.tree.nodes.len() as u32;
        self.tree.nodes.push(Node { san, parent: self.cur, children: vec![], white, number });
        match self.cur {
            Some(p) => self.tree.nodes[p as usize].children.push(id),
            None => self.tree.roots.push(id),
        }
        self.cur = Some(id);
        self.plies += 1;
        if let Some(s) = saved {
            self.stack.push(s);
        }
        Ok(())
    }
}

/// The square `delta` (files, ranks) from ChessBase square `from`, each taken modulo 8.
fn step(from: u8, (dx, dy): (i8, i8)) -> Square {
    let x = ((from / 8) as i8 + dx) & 7;
    let y = ((from % 8) as i8 + dy) & 7;
    square(x as u8 * 8 + y as u8)
}

/// Castling for the side to move, as cozy-chess writes it: the king onto its rook.
fn castle(b: &Board, short: bool) -> Result<Option<Move>, ()> {
    let us = b.side_to_move();
    let rights = b.castle_rights(us);
    let file = if short { rights.short } else { rights.long }.ok_or(())?;
    let king = b.king(us);
    Ok(Some(Move { from: king, to: Square::new(file, king.rank()), promotion: None }))
}

/// A move given by its squares (the two-byte form): from, to, and the promotion piece.
fn by_squares(b: &Board, w: u16) -> Result<Option<Move>, ()> {
    let (from, to) = ((w & 63) as u8, (w >> 6 & 63) as u8);
    if from == to {
        return if w & 0x0fff == 0 { Ok(None) } else { Err(()) };
    }
    let (f, t) = (square(from), square(to));
    let us = b.side_to_move();
    let back = if us == Color::White { Rank::First } else { Rank::Eighth };
    if b.piece_on(f) == Some(Piece::King) && f == Square::new(cozy_chess::File::E, back) && t.rank() == back {
        match t.file() {
            cozy_chess::File::G => return castle(b, true),
            cozy_chess::File::C => return castle(b, false),
            _ => {}
        }
    }
    let promotion = (b.piece_on(f) == Some(Piece::Pawn) && matches!(t.rank(), Rank::First | Rank::Eighth))
        .then(|| [Piece::Queen, Piece::Rook, Piece::Bishop, Piece::Knight][(w >> 12 & 3) as usize]);
    Ok(Some(Move { from: f, to: t, promotion }))
}

/// A one-byte move: a piece by its kind and number, and how it moves.
fn by_code(b: &Board, l: &Lists, code: u8) -> Result<Option<Move>, ()> {
    let us = b.side_to_move();
    let line = |k: u8, dirs: &[(i8, i8)]| {
        let (d, s) = (dirs[(k / 7) as usize], (k % 7 + 1) as i8);
        (d.0 * s, d.1 * s)
    };
    let (kind, index, delta) = match code {
        0 => return Ok(None),
        1..=8 => {
            let from = cb(b.king(us));
            return Ok(Some(Move { from: square(from), to: step(from, KING[(code - 1) as usize]), promotion: None }));
        }
        9 => return castle(b, true),
        10 => return castle(b, false),
        11..=38 => (0, 0, line(code - 11, &QUEEN)),
        39..=52 => (1, 0, line(code - 39, &ROOK)),
        53..=66 => (1, 1, line(code - 53, &ROOK)),
        67..=80 => (2, 0, line(code - 67, &BISHOP)),
        81..=94 => (2, 1, line(code - 81, &BISHOP)),
        95..=102 => (3, 0, KNIGHT[(code - 95) as usize]),
        103..=110 => (3, 1, KNIGHT[(code - 103) as usize]),
        111..=142 => {
            let (pawn, how) = ((code - 111) / 4, (code - 111) % 4);
            let f: i8 = if us == Color::White { 1 } else { -1 };
            let delta = [(0, f), (0, 2 * f), (f, f), (-f, f)][how as usize];
            let from = l.pawns[us as usize][pawn as usize].ok_or(())?;
            let to = step(from, delta);
            if matches!(to.rank(), Rank::First | Rank::Eighth) {
                return Err(());
            }
            return Ok(Some(Move { from: square(from), to, promotion: None }));
        }
        143..=170 => (0, 1, line(code - 143, &QUEEN)),
        171..=198 => (0, 2, line(code - 171, &QUEEN)),
        199..=212 => (1, 2, line(code - 199, &ROOK)),
        213..=226 => (2, 2, line(code - 213, &BISHOP)),
        227..=234 => (3, 2, KNIGHT[(code - 227) as usize]),
        _ => return Err(()),
    };
    let from = *l.kinds[us as usize][kind].get(index).ok_or(())?;
    Ok(Some(Move { from: square(from), to: step(from, delta), promotion: None }))
}

/// The FEN of a set-up start: a marker byte, the side to move and en passant file, the castling
/// rights, the move number, then the board, a square at a time (a1, a2 … h8): a 0 bit for an
/// empty one, else a 1 bit, a colour bit (1 Black) and 3 bits of piece.
fn setup_fen(s: &[u8]) -> Option<String> {
    let mut cells = [None; 64];
    let mut pos = 0usize;
    let mut bit = || -> Option<u8> {
        let b = s.get(4 + pos / 8)? >> (7 - pos % 8) & 1;
        pos += 1;
        Some(b)
    };
    for n in 0..64u8 {
        if bit()? == 0 {
            continue;
        }
        let black = bit()? == 1;
        let code = bit()? << 2 | bit()? << 1 | bit()?;
        let c = b" kqnbrp".get(code as usize).copied().filter(|&c| c != b' ')?;
        let sq = square(n);
        cells[sq.rank() as usize * 8 + sq.file() as usize] = Some(if black { c } else { c.to_ascii_uppercase() } as char);
    }
    let mut fen = String::new();
    for rank in (0..8).rev() {
        let mut empty = 0;
        for file in 0..8 {
            match cells[rank * 8 + file] {
                Some(c) => {
                    if empty > 0 {
                        fen.push_str(&empty.to_string());
                        empty = 0;
                    }
                    fen.push(c);
                }
                None => empty += 1,
            }
        }
        if empty > 0 {
            fen.push_str(&empty.to_string());
        }
        if rank > 0 {
            fen.push('/');
        }
    }
    let black = s[1] & 0x10 != 0;
    let castling: String = [(2, 'K'), (1, 'Q'), (8, 'k'), (4, 'q')].iter().filter(|(b, _)| s[2] & b != 0).map(|x| x.1).collect();
    let ep = match s[1] & 0x0f {
        f @ 1..=8 => format!("{}{}", (b'a' + f - 1) as char, if black { 3 } else { 6 }),
        _ => "-".into(),
    };
    Some(format!(
        "{fen} {} {} {ep} 0 {}",
        if black { 'b' } else { 'w' },
        if castling.is_empty() { "-".to_string() } else { castling },
        s[3].max(1)
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A move's bytes in mode 0: each byte that `decode` translates back, `plies` the moves
    /// before it.
    fn enc(v: u8, plies: u32) -> u8 {
        let i = MODE_0.iter().position(|&x| x == v).unwrap() as u8;
        i.wrapping_add(plies as u8)
    }

    /// One entity file: records of `size` data bytes, the given texts at the given places.
    fn entities(size: usize, records: &[&[(usize, &str)]]) -> Vec<u8> {
        let mut out = vec![0u8; 28];
        out[0x0c..0x10].copy_from_slice(&(size as i32).to_le_bytes());
        for r in records {
            let mut rec = vec![0u8; 9 + size];
            for (at, s) in r.iter() {
                rec[9 + at..9 + at + s.len()].copy_from_slice(s.as_bytes());
            }
            out.extend(rec);
        }
        out
    }

    /// A database of one game, `moves` (codes, with 254 and 255) in mode 0, in `dir` as
    /// `name.cbh` and the files beside it.
    pub(crate) fn base(dir: &Path, name: &str, moves: &[u8]) -> PathBuf {
        let (mut bytes, mut plies) = (vec![], 0u32);
        for &m in moves {
            bytes.push(enc(m, plies));
            if m < 235 {
                plies += 1;
            }
        }
        let mut cbg = vec![0u8; 10];
        let size = (4 + bytes.len()) as u32;
        cbg.extend([0, (size >> 16) as u8, (size >> 8) as u8, size as u8]);
        cbg.extend(bytes);
        let mut cbh = vec![0u8; 46];
        cbh[3..5].copy_from_slice(&46u16.to_be_bytes());
        let mut r = [0u8; 46];
        r[0] = 1;
        r[1..5].copy_from_slice(&10u32.to_be_bytes());
        r[0x0c..0x0f].copy_from_slice(&[0, 0, 1]);
        let d: u32 = 1858 << 9 | 11 << 5 | 2;
        r[0x18..0x1b].copy_from_slice(&d.to_be_bytes()[1..]);
        r[0x1b] = 2;
        r[0x1d] = 3;
        r[0x1f..0x21].copy_from_slice(&2690u16.to_be_bytes());
        r[0x23..0x25].copy_from_slice(&((244u16 + 1) * 128).to_be_bytes());
        r[0x2d] = 3;
        cbh.extend(r);
        // A deleted game, which is no game.
        let mut deleted = r;
        deleted[0] = 0x81;
        cbh.extend(deleted);
        let p = dir.join(format!("{name}.cbh"));
        std::fs::write(&p, cbh).unwrap();
        std::fs::write(p.with_extension("cbg"), cbg).unwrap();
        std::fs::write(p.with_extension("cbp"), entities(58, &[&[(0, "Morphy"), (30, "Paul")], &[(0, "Duke Karl")]])).unwrap();
        std::fs::write(p.with_extension("cbt"), entities(90, &[&[(0, "Paris"), (40, "Paris Opera")]])).unwrap();
        p
    }

    /// 1.e4 e5 2.Nf3 (2.f4 exf4) 2...Nc6 3.Bb5, stored main line first.
    pub(crate) const RUY: [u8; 9] = [128, 128, 254, 105, 101, 84, 255, 132, 130];

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-cbh-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_game_reads_as_pgn() {
        let d = dir("pgn");
        let mut moves = RUY.to_vec();
        moves.push(255);
        let p = base(&d, "games", &moves);
        let mut listed = vec![];
        scan(&p, &|| false, &|_| {}, |g| {
            listed.push((g.record, g.tags.clone(), g.moves));
            true
        })
        .unwrap();
        assert_eq!(listed.len(), 1, "the deleted game is left out");
        let (record, tags, n) = &listed[0];
        assert_eq!((*record, *n), (1, 3));
        let tag = |k: &str| tags.iter().find(|t| t.0 == k).unwrap().1.as_str();
        assert_eq!((tag("White"), tag("Black"), tag("Event"), tag("Site")), ("Morphy, Paul", "Duke Karl", "Paris", "Paris Opera"));
        assert_eq!((tag("Date"), tag("Result"), tag("ECO"), tag("WhiteElo"), tag("Round")), ("1858.11.02", "1-0", "C44", "2690", "3"));
        let text = pgn(&p, 1).unwrap();
        assert!(text.ends_with("1. e4 e5 2. Nf3 ( 2. f4 exf4 ) 2... Nc6 3. Bb5 1-0\n"), "{text}");
        let g = pgn::parse(&text);
        assert_eq!(g.main_line().len(), 5);
        assert!(pgn(&p, 2).is_err(), "a deleted game");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_library_lists_and_opens_a_database() {
        let d = dir("lib");
        let mut moves = RUY.to_vec();
        moves.push(255);
        let p = base(&d, "Games", &moves);
        let lib = crate::chess::Library::open(&p).unwrap();
        while !lib.progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let row = lib.row(0).unwrap();
        assert_eq!((row.white.as_str(), row.moves, row.eco.as_str()), ("Morphy, Paul", 3, "C44"));
        assert_eq!(lib.game(0).unwrap().main_line().len(), 5);
        let text = crate::chess::games::search_text(&p, 1 << 20).unwrap();
        assert!(text.contains("Morphy, Paul – Duke Karl"), "{text}");
        let mut keys = vec![];
        crate::chess::games::positions(&p, &|| false, |n, k| keys.push((n, k.len()))).unwrap();
        assert_eq!(keys, vec![(0, 5)]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_game_that_breaks_off_keeps_its_moves_and_says_so() {
        let d = dir("cut");
        // The last line is never ended, and then a move no piece can make.
        let p = base(&d, "cut", &[128, 128, 30]);
        let text = pgn(&p, 1).unwrap();
        assert!(text.contains("1. e4 e5"), "{text}");
        assert!(text.contains(&crate::t!("chess.cbh_cut")), "{text}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn hostile_records_never_panic() {
        // Random bytes after every mode's head: whatever comes, a tree or a cut game.
        let mut x: u32 = 12345;
        for len in 0..400usize {
            for flags in [0u8, 4, 5, 0x40, 0x45, 10, 0x80] {
                let mut rec = vec![flags, 0, 0, 0];
                for _ in 0..len {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    rec.push(x as u8);
                }
                let (tree, _) = decode(&rec);
                let _ = tree.movetext();
            }
        }
        // A start position bit stream of nothing but pieces.
        assert!(setup_fen(&[0xff; 28]).is_none());
        let d = dir("hostile");
        let p = d.join("bad.cbh");
        // Not 46-byte records; no files beside it.
        std::fs::write(&p, [0u8; 100]).unwrap();
        assert!(scan(&p, &|| false, &|_| {}, |_| true).is_err());
        let mut cbh = vec![0u8; 46];
        cbh[3..5].copy_from_slice(&46u16.to_be_bytes());
        let mut r = [0xffu8; 46];
        r[0] = 1;
        cbh.extend(r);
        std::fs::write(&p, &cbh).unwrap();
        let mut n = 0;
        scan(&p, &|| false, &|_| {}, |_| {
            n += 1;
            true
        })
        .unwrap();
        assert_eq!(n, 1);
        assert!(pgn(&p, 1).unwrap().contains(&crate::t!("chess.cbh_cut")));
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// Every game of a real database (`COXSWAIN_CBH=/path/to/base.cbh`) read to its end: none
    /// cut, every move legal. No real database is in the repository.
    #[test]
    #[ignore]
    fn a_real_database_reads_whole() {
        let p = PathBuf::from(std::env::var("COXSWAIN_CBH").unwrap());
        if let Ok(n) = std::env::var("COXSWAIN_CBH_SHOW") {
            eprintln!("{}", pgn(&p, n.parse().unwrap()).unwrap());
            return;
        }
        let (mut games, mut cut, mut moves) = (0, vec![], 0);
        scan(&p, &|| false, &|_| {}, |g| {
            let text = pgn(&p, g.record).unwrap();
            games += 1;
            if text.contains(&crate::t!("chess.cbh_cut")) {
                cut.push(g.record);
            }
            moves += pgn::parse(&text).nodes.len();
            true
        })
        .unwrap();
        eprintln!("{games} games, {moves} moves, cut: {cut:?}");
        assert!(cut.is_empty());
    }

    #[test]
    fn a_set_up_start_reads_as_fen() {
        // White king e1, black king e8, White to move, no castling, move 40: a square at a time.
        let mut bits = vec![];
        for n in 0..64u8 {
            match n {
                32 => bits.extend([1, 0, 0, 0, 1]),
                39 => bits.extend([1, 1, 0, 0, 1]),
                _ => bits.push(0),
            }
        }
        let mut s = vec![1u8, 0, 0, 40];
        for c in bits.chunks(8) {
            let mut b = 0u8;
            for (i, &v) in c.iter().enumerate() {
                b |= v << (7 - i);
            }
            s.push(b);
        }
        s.resize(28, 0);
        assert_eq!(setup_fen(&s).unwrap(), "4k3/8/8/8/8/8/8/4K3 w - - 0 40");
    }
}

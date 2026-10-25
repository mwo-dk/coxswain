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
    /// The game's bytes, up to `MAX_GAME` of them, when `scan` is asked to keep them.
    pub text: Vec<u8>,
    /// A chess game (`Pgn`), else draughts (`Pdn`) or xiangqi, which have no openings by name.
    pub format: super::Format,
}

impl Head {
    fn tag(&self, i: usize) -> &str {
        self.tags[i].trim()
    }

    /// ECO code and opening name: the file's own, else by the moves, else by the code.
    pub(crate) fn opening(&self) -> (String, String) {
        if self.format == super::Format::Pbn {
            // A deal's contract stands where a game's opening does.
            return (String::new(), self.tag(OPENING).to_string());
        }
        if self.format != super::Format::Pgn {
            return Default::default();
        }
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
pub(crate) fn text(b: &[u8]) -> std::borrow::Cow<'_, str> {
    match std::str::from_utf8(b) {
        Ok(s) => s.into(),
        Err(_) => encoding_rs::WINDOWS_1252.decode_without_bom_handling(b).0,
    }
}

pub use super::lex::MAX_LINE;
use super::lex::{Lexer, Movetext, Tok};
/// The most of one game read to show it.
pub const MAX_GAME: u32 = 16 << 20;

/// The games of `r`, in order, each handed to `each` until it says to stop. Never fails on
/// what it reads: lines that are no PGN are taken for moves, and skipped there.
/// `format`: whose moves they are, chess's (`Pgn`), draughts' or xiangqi's. `full`: the moves
/// counted to the end of each game (`Head::plies`); else past its first moves only where its
/// comments start and end is looked at, to know where the next game starts. `keep`: each game's
/// bytes kept in `Head::text` (PGN, PDN and xiangqi), its moves not looked at.
pub(crate) fn scan(r: impl BufRead, stop: &AtomicBool, format: super::Format, full: bool, keep: bool, mut each: impl FnMut(&Head) -> bool) -> io::Result<()> {
    let mut r = Lexer::new(r);
    if matches!(format, super::Format::Kif | super::Format::Csa) {
        return scan_shogi(&mut r, stop, format, each);
    }
    if format == super::Format::Pbn {
        return scan_pbn(&mut r, stop, each);
    }
    let mut head = Head { offset: 0, len: 0, tags: Default::default(), plies: 0, sans: vec![], text: vec![], format };
    let mut line = Vec::new();
    let (mut has_tags, mut has_moves, mut in_brace, mut depth) = (false, false, false, 0u32);
    loop {
        let mut at = r.at;
        let n = r.line(&mut line)?;
        // A byte order mark before the first tag is no part of the game.
        if at == 0 && line.starts_with(b"\xEF\xBB\xBF") {
            line.drain(..3);
            at = 3;
        }
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
            head.text.clear();
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
        if keep && head.text.len() + line.len() <= MAX_GAME as usize {
            head.text.extend_from_slice(&line);
        }
        if is_tag {
            if let Some((k, v)) = pgn::tag_pair(&text(&line))
                // Xiangqi names its first player Red.
                && let Some(i) = WANTED.iter().position(|w| w.eq_ignore_ascii_case(&k)).or_else(|| k.eq_ignore_ascii_case("Red").then_some(WHITE))
            {
                head.tags[i] = v;
            }
            has_tags = true;
        } else if !trimmed.is_empty() && (trimmed[0] != b'%' || in_brace) {
            has_moves = true;
            moves(&text(&line), &mut head, &mut in_brace, &mut depth, full, keep);
        }
    }
}

/// The games of a KIF or CSA file (`crate::shogi`), read as they stream by: a CSA file's games
/// end at a line of `/`, a KIF file is one game. Each game is read in full for its tags and
/// moves, up to `MAX_GAME` bytes of it.
fn scan_shogi(r: &mut Lexer<impl BufRead>, stop: &AtomicBool, format: super::Format, mut each: impl FnMut(&Head) -> bool) -> io::Result<()> {
    let (mut game, mut line) = (Vec::new(), Vec::new());
    let mut start = 0u64;
    let mut emit = |game: &mut Vec<u8>, start: u64, end: u64| -> bool {
        let keep = game.iter().any(|c| !c.is_ascii_whitespace());
        let g = crate::shogi::parse(&crate::shogi::decode(game), format);
        game.clear();
        if !keep {
            return true;
        }
        let mut head = Head { offset: start, len: end - start, tags: Default::default(), plies: g.main_line().len() as u32, sans: vec![], text: vec![], format };
        for (k, v) in &g.tags {
            if let Some(i) = WANTED.iter().position(|w| w.eq_ignore_ascii_case(k)) {
                head.tags[i] = v.clone();
            }
        }
        head.tags[RESULT] = g.result.clone();
        each(&head)
    };
    loop {
        let at = r.at;
        if r.line(&mut line)? == 0 {
            emit(&mut game, start, at);
            return Ok(());
        }
        if format == super::Format::Csa && line.trim_ascii() == b"/" {
            if !emit(&mut game, start, at) || stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            start = r.at;
            continue;
        }
        if game.len() + line.len() <= MAX_GAME as usize {
            game.extend_from_slice(&line);
        }
    }
}

/// The deals of a PBN file (`crate::bridge`), read as they stream by: a deal ends at an empty
/// line before the next one's tags. Each deal is read in full, up to `MAX_GAME` bytes of it;
/// a tag whose value is `#` takes the deal before's, as PBN says.
fn scan_pbn(r: &mut Lexer<impl BufRead>, stop: &AtomicBool, mut each: impl FnMut(&Head) -> bool) -> io::Result<()> {
    let (mut game, mut line) = (Vec::new(), Vec::new());
    let (mut start, mut blank) = (0u64, true);
    let mut last: HashMap<String, String> = HashMap::new();
    let mut emit = |game: &mut Vec<u8>, start: u64, end: u64| -> bool {
        if !game.iter().any(|c| !c.is_ascii_whitespace()) {
            game.clear();
            return true;
        }
        let mut g = crate::bridge::parse(&text(game));
        game.clear();
        for (k, v) in &mut g.tags {
            if v == "#" {
                *v = last.get(k).cloned().unwrap_or_default();
            } else if last.len() < 64 || last.contains_key(k) {
                last.insert(k.clone(), v.clone());
            }
        }
        let (pairs, contract) = crate::bridge::summary(&g);
        let mut head = Head { offset: start, len: end - start, tags: Default::default(), plies: g.nodes.len() as u32, sans: vec![], text: vec![], format: super::Format::Pbn };
        let tag = |k: &str| g.tag(k).unwrap_or("").to_string();
        head.tags[WHITE] = pairs[0].clone();
        head.tags[BLACK] = pairs[1].clone();
        head.tags[EVENT] = tag("Event");
        head.tags[SITE] = tag("Site");
        head.tags[DATE] = tag("Date");
        head.tags[OPENING] = contract;
        head.tags[RESULT] = g.result.clone();
        each(&head)
    };
    loop {
        let at = r.at;
        if r.line(&mut line)? == 0 {
            emit(&mut game, start, at);
            return Ok(());
        }
        let t = line.trim_ascii();
        if t.starts_with(b"[") && blank && !game.is_empty() {
            if !emit(&mut game, start, at) || stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            start = at;
        }
        blank = t.is_empty();
        if game.is_empty() {
            start = at;
        }
        if game.len() + line.len() <= MAX_GAME as usize {
            game.extend_from_slice(&line);
        }
    }
}

/// A line of movetext: counts the main line's moves and keeps the first few. Not `full`, once
/// the first few are kept, only where comments start and end is looked at; `skim`, only that.
fn moves(line: &str, head: &mut Head, in_brace: &mut bool, depth: &mut u32, full: bool, skim: bool) {
    let mut toks = Movetext::new(line, *in_brace);
    while !skim && (full || head.sans.len() < FIRST_MOVES) {
        let Some(tok) = toks.next() else {
            *in_brace = toks.in_brace;
            return;
        };
        match tok {
            Tok::Open => *depth = depth.saturating_add(1),
            Tok::Close => *depth = depth.saturating_sub(1),
            Tok::Word(w) if *depth == 0 => {
                let mv = match head.format {
                    super::Format::Pdn if crate::draughts::is_move(w) => Word::Move(w, None),
                    super::Format::Xiangqi if crate::xiangqi::is_move(w) => Word::Move(w, None),
                    super::Format::Pdn | super::Format::Xiangqi => Word::Other,
                    _ => pgn::word(w),
                };
                if let Word::Move(san, _) = mv {
                    head.plies = head.plies.saturating_add(1);
                    if head.sans.len() < FIRST_MOVES {
                        head.sans.push(san.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    // Past the first moves, not `full`: only where comments start and end.
    let (mut s, mut open) = (toks.rest(), toks.in_brace);
    loop {
        let b = s.as_bytes();
        if open {
            let Some(e) = memchr::memchr(b'}', b) else { break };
            (s, open) = (&s[e + 1..], false);
        } else {
            match memchr::memchr2(b'{', b';', b) {
                Some(i) if b[i] == b'{' => (s, open) = (&s[i + 1..], true),
                _ => break,
            }
        }
    }
    *in_brace = open;
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

/// How the file is packed, by its name; a DhtmlXQ game by its first bytes, read as the PGN it
/// is turned into (`xiangqi::dhtml_to_pgn`).
#[derive(Clone, Copy, PartialEq)]
enum Pack {
    None,
    Gz,
    Zst,
    Dhtml,
}

fn pack(path: &Path) -> Pack {
    let n = path.to_string_lossy().to_ascii_lowercase();
    if n.ends_with(".gz") {
        Pack::Gz
    } else if n.ends_with(".zst") {
        Pack::Zst
    } else if is_dhtml(path) {
        Pack::Dhtml
    } else {
        Pack::None
    }
}

/// Whether the file at `path` is a DhtmlXQ game, by its first bytes: only a name such files
/// have (no ending, `.che`, `.txt`, `.dhtmlxq`), and never one only in the cloud.
pub(crate) fn is_dhtml(path: &Path) -> bool {
    let named = path.extension().is_none_or(|e| ["che", "txt", "dhtmlxq"].iter().any(|x| e.eq_ignore_ascii_case(x)));
    if !named || crate::cloud::unasked(path) {
        return false;
    }
    let mut head = [0u8; 64];
    crate::fs::open_regular(path).and_then(|mut f| read_full(&mut f, &mut head)).is_ok_and(|n| crate::xiangqi::is_dhtml(&head[..n]))
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
pub(crate) fn reader(path: &Path, read: Arc<AtomicU64>, unpacked: u64) -> io::Result<Box<dyn Read + Send>> {
    let f = BufReader::new(Counted { inner: File::open(path)?, read });
    Ok(match pack(path) {
        Pack::None => Box::new(f),
        Pack::Gz => Box::new(flate2::read::MultiGzDecoder::new(f).take(unpacked)),
        Pack::Zst => Box::new(ruzstd::decoding::StreamingDecoder::new(f).map_err(io::Error::other)?.take(unpacked)),
        Pack::Dhtml => {
            let mut text = vec![];
            f.take(crate::xiangqi::DHTML_MAX).read_to_end(&mut text)?;
            let pgn = crate::xiangqi::dhtml_to_pgn(&String::from_utf8_lossy(&text));
            Box::new(io::Cursor::new(pgn.into_bytes()).take(unpacked))
        }
    })
}

/// The games of the PGN file at `path`, one by one (the search store, and Find's hits).
pub(crate) fn scan_file(path: &Path, each: impl FnMut(&Head) -> bool) -> io::Result<()> {
    if super::cbh::is_cbh(path) {
        return scan_cbh(path, &AtomicBool::new(false), &AtomicU64::new(0), 0, each);
    }
    // The search store reads every PGN file: 1 GiB unpacked of each is plenty.
    if super::scid::is_scid(path) {
        return scan_scid(&super::scid::Base::open(path)?, &AtomicBool::new(false), &Arc::default(), each);
    }
    let format = kind(path);
    let r = reader(path, Arc::default(), 1 << 30)?;
    scan(BufReader::with_capacity(1 << 16, r), &AtomicBool::new(false), format, false, false, each)
}

/// The games of a SCID database (`scid`), from its index as it streams by: a game's place in
/// the index is its `offset`. `read` counts the index bytes read, for the progress.
fn scan_scid(base: &super::scid::Base, stop: &AtomicBool, read: &AtomicU64, mut each: impl FnMut(&Head) -> bool) -> io::Result<()> {
    base.entries(|n, e| {
        let mut head = Head { offset: n as u64, len: 0, tags: Default::default(), plies: e.plies, sans: vec![], text: vec![], format: super::Format::Pgn };
        head.tags[WHITE] = base.white(e).to_string();
        head.tags[BLACK] = base.black(e).to_string();
        head.tags[EVENT] = base.event(e).to_string();
        head.tags[SITE] = base.site(e).to_string();
        head.tags[DATE] = e.date.clone();
        head.tags[ECO] = e.eco.clone();
        head.tags[RESULT] = ["*", "1-0", "0-1", "1/2-1/2"][e.result as usize & 3].to_string();
        for (i, elo) in [(WHITE_ELO, e.white_elo), (BLACK_ELO, e.black_elo)] {
            if elo > 0 {
                head.tags[i] = elo.to_string();
            }
        }
        read.fetch_add(47, Ordering::Relaxed);
        each(&head) && !stop.load(Ordering::Relaxed)
    })
}

/// The games of a ChessBase database (`cbh`), as the pass over a PGN file sees them: a game's
/// `offset` is its record in the `.cbh` file. `read` is told how far of `size` it is: an
/// archive's (`.cbv`) `size` is twice its own, the first half its unpacking (stopped by `stop`
/// too), the second its `.cbh` file read.
fn scan_cbh(path: &Path, stop: &AtomicBool, read: &AtomicU64, size: u64, mut each: impl FnMut(&Head) -> bool) -> io::Result<()> {
    let mut head = Head { offset: 0, len: 0, tags: Default::default(), plies: 0, sans: vec![], text: vec![], format: super::Format::Pgn };
    let share = |n: u64, of: u64, base: u64| read.store(base + (n.min(of) as u128 * (size / 2) as u128 / of.max(1) as u128) as u64, Ordering::Relaxed);
    let (path, cbh_len) = if super::cbv::is_cbv(path) {
        let p = super::cbv::unpacked_with(path, &|| stop.load(Ordering::Relaxed), &|n, of| share(n, of, 0))?;
        let len = std::fs::metadata(&p)?.len();
        (p, Some(len))
    } else {
        (path.to_path_buf(), None)
    };
    let progress = |n: u64| match cbh_len {
        Some(len) => share(n, len, size / 2),
        None => read.store(n, Ordering::Relaxed),
    };
    super::cbh::scan(&path, &|| stop.load(Ordering::Relaxed), &progress, |g| {
        for t in &mut head.tags {
            t.clear();
        }
        for (k, v) in &g.tags {
            if let Some(i) = WANTED.iter().position(|w| w == k) {
                head.tags[i].clone_from(v);
            }
        }
        head.offset = g.record as u64;
        head.plies = g.moves * 2;
        each(&head)
    })
}

/// Whose games a PGN-family file holds: draughts for `.pdn`, xiangqi when the first tags of a
/// PGN file say so (`xiangqi::is_xiangqi`), else chess.
pub fn kind(path: &Path) -> super::Format {
    if super::scid::is_scid(path) {
        return super::Format::Pgn;
    }
    if let Some(f @ (super::Format::Pdn | super::Format::Kif | super::Format::Csa | super::Format::Pbn)) = super::format(path) {
        return f;
    }
    if super::cbh::is_cbh(path) {
        return super::Format::Pgn;
    }
    let mut head = vec![];
    let xiangqi = reader(path, Arc::default(), crate::xiangqi::SNIFF as u64)
        .and_then(|r| r.take(crate::xiangqi::SNIFF as u64).read_to_end(&mut head))
        .is_ok_and(|_| crate::xiangqi::is_xiangqi(&head));
    if xiangqi { super::Format::Xiangqi } else { super::Format::Pgn }
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
    /// The result as written, interned: draughts writes `2-0`.
    result_name: u32,
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
        "1-0" | "2-0" => 1,
        "0-1" | "0-2" => 2,
        "1/2-1/2" | "½-½" | "1-1" => 3,
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
    /// `Pgn`, or `Pdn` for draughts.
    pub format: super::Format,
    shared: Arc<Mutex<Shared>>,
    read: Arc<AtomicU64>,
    size: u64,
    stop: Arc<AtomicBool>,
    source: Mutex<Source>,
    /// A SCID database's names and files, when the file is one (`scid`).
    scid: Option<Arc<super::scid::Base>>,
    /// A ChessBase archive's unpacked files, kept while this reads them (`cbv::hold`).
    _held: Option<super::cbv::Held>,
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
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let source = Mutex::new(Source { path: path.to_path_buf(), open: None });
        if super::scid::is_scid(path) {
            let base = Arc::new(super::scid::Base::open(path)?);
            let (sh, st, b, rd) = (shared.clone(), stop.clone(), base.clone(), read.clone());
            std::thread::Builder::new()
                .name("chess-games".into())
                .spawn(move || index(|each| scan_scid(&b, &st, &rd, each), &sh, super::Format::Pgn, limits, || false))?;
            return Ok(Library { path: path.to_path_buf(), format: super::Format::Pgn, shared, read, size, stop, source, scid: Some(base), _held: None });
        }
        if super::cbh::is_cbh(path) {
            // An archive is unpacked first, then read: its bar has both (`scan_cbh`).
            let size = if super::cbv::is_cbv(path) { size.saturating_mul(2) } else { size };
            let (sh, st, rd, p) = (shared.clone(), stop.clone(), read.clone(), path.to_path_buf());
            std::thread::Builder::new()
                .name("chess-games".into())
                .spawn(move || index(|each| scan_cbh(&p, &st, &rd, size, each), &sh, super::Format::Pgn, limits, || false))?;
            let _held = super::cbv::is_cbv(path).then(|| super::cbv::hold(path));
            return Ok(Library { path: path.to_path_buf(), format: super::Format::Pgn, shared, read, size, stop, source, scid: None, _held });
        }
        let r = reader(path, read.clone(), limits.unpacked)?;
        // What the packed file unpacked to: at the limit, the rest of it was left.
        let unpacked = Arc::new(AtomicU64::new(0));
        let packed = pack(path) != Pack::None;
        let r: Box<dyn Read + Send> = Box::new(Counted { inner: r, read: unpacked.clone() });
        let (sh, st) = (shared.clone(), stop.clone());
        let format = kind(path);
        std::thread::Builder::new().name("chess-games".into()).spawn(move || {
            index(|each| scan(BufReader::with_capacity(1 << 16, r), &st, format, true, false, each), &sh, format, limits, || packed && unpacked.load(Ordering::Relaxed) >= limits.unpacked)
        })?;
        Ok(Library { path: path.to_path_buf(), format, shared, read, size, stop, source, scid: None, _held: None })
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
            let keys = crate::collate::keys(&s.strings);
            order.sort_by(|&a, &b| keys[a as usize].cmp(&keys[b as usize]));
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
            result: if self.format == super::Format::Pgn || r.result_name == u32::MAX { result_text(r.result).into() } else { name(r.result_name) },
            date: date_text(r.date),
            event: name(r.event),
            eco: eco_text(r.eco),
            opening: name(r.opening),
            // Shogi counts every move; chess, draughts and xiangqi a move of each side as one.
            // Bridge counts the tricks played.
            moves: if matches!(self.format, super::Format::Kif | super::Format::Csa | super::Format::Pbn) { r.plies as u32 } else { (r.plies as u32).div_ceil(2) },
        })
    }

    /// The text of the game at index `i`, read now.
    pub fn text(&self, i: u32) -> io::Result<String> {
        let (offset, len) = {
            let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            let r = s.recs.get(i as usize).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such game"))?;
            (r.offset, r.len)
        };
        if let Some(base) = &self.scid {
            return base.pgn(offset as u32);
        }
        if super::cbh::is_cbh(&self.path) {
            return super::cbh::pgn(&self.path, offset as u32);
        }
        let bytes = self.source.lock().unwrap_or_else(|e| e.into_inner()).read(offset, len)?;
        // KIF and CSA files are often in Shift_JIS.
        Ok(match self.format {
            super::Format::Kif | super::Format::Csa => crate::shogi::decode(&bytes),
            _ => text(&bytes).into_owned(),
        })
    }

    /// The game at index `i`, read and played out.
    pub fn game(&self, i: u32) -> io::Result<pgn::Game> {
        let text = self.text(i)?;
        Ok(match self.format {
            super::Format::Pdn => crate::draughts::parse(&text),
            super::Format::Xiangqi => crate::xiangqi::parse(&text),
            super::Format::Kif | super::Format::Csa => crate::shogi::parse(&text, self.format),
            super::Format::Pbn => crate::bridge::parse(&text),
            _ => pgn::parse(&text),
        })
    }
}

/// The pass over the games: their records, handed over in batches so the table fills.
/// `cut`: the packed file was unpacked as far as the limits allow.
fn index(feed: impl FnOnce(&mut dyn FnMut(&Head) -> bool) -> io::Result<()>, shared: &Mutex<Shared>, format: super::Format, limits: Limits, cut: impl Fn() -> bool) {
    let chess = format == super::Format::Pgn;
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
    let res = feed(&mut |h| {
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
            result_name: if chess { u32::MAX } else { intern(h.tag(RESULT), &mut ids, &mut fresh, base, &mut names) },
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

/// The game a Find hit is about, from the lines of a file's games (`search_text`) fed in
/// order, by its snippet: PGN files here, SGF files in `go`.
///
/// The store keeps a line per game and its snippet is a run of words around the ones found,
/// often across the lines of neighbouring games, its line breaks gone. So the snippet's words
/// are looked for as a run in the games' lines, one after the other; the game of the first
/// word found is the one. When no run matches (a reader that changed the text), the game whose
/// line has the found words and most of the others.
pub struct Hit {
    want: Vec<(String, bool)>,
    first: usize,
    found: Vec<String>,
    /// The last words fed, each with its game, as many as the snippet has.
    window: std::collections::VecDeque<(String, u32)>,
    n: u32,
    hit: Option<u32>,
    best: (usize, Option<u32>),
}

impl Hit {
    /// None for a snippet without words.
    pub fn new(snippet: &str) -> Option<Hit> {
        let want = words(snippet);
        if want.is_empty() {
            return None;
        }
        let first = want.iter().position(|w| w.1).unwrap_or(0);
        let found = want.iter().filter(|w| w.1).map(|w| w.0.clone()).collect();
        Some(Hit { want, first, found, window: Default::default(), n: 0, hit: None, best: (0, None) })
    }

    /// The next game's line; false once the game is found.
    pub fn feed(&mut self, line: &str) -> bool {
        let line: Vec<String> = words(line).into_iter().map(|w| w.0).collect();
        // The fallback's score: the found words all here, then as many of the others.
        if self.found.iter().all(|f| line.iter().any(|w| w == f)) {
            let score = 1 + self.want.iter().filter(|w| line.contains(&w.0)).count();
            if score > self.best.0 {
                self.best = (score, Some(self.n));
            }
        }
        for w in line {
            self.window.push_back((w, self.n));
            if self.window.len() > self.want.len() {
                self.window.pop_front();
            }
            if self.window.len() == self.want.len() && self.window.iter().zip(&self.want).all(|(a, b)| a.0 == b.0) {
                self.hit = Some(self.window[self.first].1);
                return false;
            }
        }
        self.n += 1;
        true
    }

    /// The game found, from 0.
    pub fn found(&self) -> Option<u32> {
        self.hit.or(self.best.1)
    }
}

/// The game a Find hit in a PGN file is about, by its snippet (`Hit`): its index, from 0.
pub fn game_of_hit(path: &Path, snippet: &str) -> Option<u32> {
    let mut hit = Hit::new(snippet)?;
    scan_file(path, |h| hit.feed(&h.line())).ok()?;
    hit.found()
}

/// The Polyglot keys of the positions `g` reaches on its main line, each once, sorted.
fn main_line_keys(g: &pgn::Game) -> Vec<u64> {
    let mut keys: Vec<u64> = g
        .main_line()
        .iter()
        .map_while(|&i| g.nodes[i as usize].fen.as_deref().and_then(pgn::board))
        .map(|b| super::polyglot::key(&b))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// The most games of one file whose positions the search store keeps, and the most positions.
pub const POSITION_GAMES: u32 = 100_000;
pub const POSITION_ROWS: usize = 2_000_000;

/// The positions each game of the PGN file at `path` reaches on its main line, as Polyglot keys
/// (`polyglot::key`), each once a game, the start position left out (every game has it):
/// handed to `each` game by game, with the game's index, as the file is read, so a big file
/// never has more than one game in memory. Up to `POSITION_GAMES` games and `POSITION_ROWS`
/// positions; `stop` is asked between games, and true ends it there.
pub fn positions(path: &Path, stop: &dyn Fn() -> bool, mut each: impl FnMut(u32, &[u64])) -> io::Result<()> {
    if super::scid::is_scid(path) {
        // The games as the table lists them (`scan_scid`: deleted ones left out), so a game's
        // index here is its row's, not its place in the SCID index.
        let base = super::scid::Base::open(path)?;
        let mut games = vec![];
        base.entries(|n, _| {
            games.push(n);
            games.len() < POSITION_GAMES as usize
        })?;
        let mut rows = 0usize;
        for (row, n) in (0u32..).zip(games) {
            if rows >= POSITION_ROWS || stop() {
                break;
            }
            let mut keys = main_line_keys(&super::pgn::parse(&base.pgn(n)?));
            keys.truncate(POSITION_ROWS - rows);
            rows += keys.len();
            each(row, &keys);
        }
        return Ok(());
    }
    let (mut n, mut rows) = (0u32, 0usize);
    let mut flush = |game: &str, n: &mut u32, rows: &mut usize| {
        let mut keys = main_line_keys(&super::pgn::parse(game));
        keys.truncate(POSITION_ROWS.saturating_sub(*rows));
        *rows += keys.len();
        each(*n, &keys);
        *n += 1;
    };
    if super::cbh::is_cbh(path) {
        // The records first, then each game read as PGN: the game's index is its place in the list.
        let mut records = vec![];
        super::cbh::scan(path, stop, &|_| {}, |g| {
            records.push(g.record);
            records.len() < POSITION_GAMES as usize
        })?;
        for r in records {
            if rows >= POSITION_ROWS || stop() {
                break;
            }
            flush(&super::cbh::pgn(path, r).unwrap_or_else(|_| "*\n".into()), &mut n, &mut rows);
        }
        return Ok(());
    }
    // The games as the table cuts them (`scan`), so a game's index here is its row's.
    let r = BufReader::with_capacity(1 << 16, reader(path, Arc::default(), 1 << 30)?);
    scan(r, &AtomicBool::new(false), super::Format::Pgn, false, true, |h| {
        if n >= POSITION_GAMES || rows >= POSITION_ROWS || stop() {
            return false;
        }
        flush(&text(&h.text), &mut n, &mut rows);
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const TWO: &str = "[Event \"Paris\"]\n[White \"Morphy, Paul\"]\n[Black \"Duke Karl\"]\n[Result \"1-0\"]\n[Date \"1858.11.02\"]\n[WhiteElo \"2690\"]\n\n1.e4 e5 2.Nf3 d6 {Philidor} 3.d4 Bg4 (3...exd4) 4.dxe5 1-0\n\n[Event \"London\"]\n[White \"Anderssen, Adolf\"]\n[Black \"Kieseritzky, Lionel\"]\n[Result \"1-0\"]\n[Date \"1851.06.21\"]\n[ECO \"C33\"]\n\n1.e4 e5 2.f4 exf4 {a\n[not a tag] } 3.Bc4 1-0\n";

    #[test]
    fn positions_game_by_game() {
        let d = tempdir::Dir::new();
        let p = d.0.join("two.pgn");
        std::fs::write(&p, TWO).unwrap();
        let mut seen = vec![];
        positions(&p, &|| false, |n, keys| seen.push((n, keys.to_vec()))).unwrap();
        // Stopped: no game at all.
        let mut none = 0;
        positions(&p, &|| true, |_, _| none += 1).unwrap();
        assert_eq!(none, 0);
        assert_eq!(seen.len(), 2);
        // Both start 1.e4 e5: the position after it is in both, the start position in neither.
        let after = |fen: &str| crate::chess::polyglot::key(&crate::chess::pgn::board(fen).unwrap());
        let e4e5 = after("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2");
        assert!(seen.iter().all(|(_, k)| k.contains(&e4e5)));
        assert!(seen.iter().all(|(_, k)| !k.contains(&after(crate::chess::pgn::START))));
        // The first game's 4.dxe5 is in it alone; the comment with a [ line does not split a game.
        assert_eq!((seen[0].1.len(), seen[1].1.len()), (7, 5));
    }

    /// Find this position names a game by its row: positions are read from the games as the
    /// table cuts them, an unclosed comment and a lone Event tag too.
    #[test]
    fn positions_by_the_tables_games() {
        let text = "[Event \"a\"]\n\n1.e4 {never closed\n\n[Event \"b\"]\n\n1.d4 d5 *\n[Event \"x\"]\n[Event \"c\"]\n\n1.c4 $1 e5 *\n";
        let (l, d) = lib(text.as_bytes(), "odd.pgn");
        let mut seen = vec![];
        positions(&d.0.join("odd.pgn"), &|| false, |n, keys| seen.push((n, keys.len()))).unwrap();
        assert_eq!(l.progress().games, 4);
        assert_eq!(seen, [(0, 1), (1, 2), (2, 0), (3, 2)]);
        assert_eq!((l.row(3).unwrap().moves, l.row(3).unwrap().opening.as_str()), (1, "English Opening: King's English Variation"));
    }

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
    fn a_byte_order_mark_is_no_game() {
        let text = format!("\u{feff}{TWO}");
        let mut events = vec![];
        scan(text.as_bytes(), &AtomicBool::new(false), super::super::Format::Pgn, true, false, |h| {
            events.push((h.offset, h.tags[EVENT].clone()));
            true
        })
        .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, 3, "the game starts after the mark");
        assert!(!events[0].1.is_empty(), "its Event kept");
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

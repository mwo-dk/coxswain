//! A PGN file's games as a table, for files of millions of games: one pass reads only where
//! each game starts and its headers (on a thread of its own, the table filling as it goes);
//! a game's moves are read when it is opened. `.pgn.gz` and `.pgn.zst` are read as they unpack.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
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
/// bytes kept in `Head::text` (PGN, PDN and xiangqi), its moves not looked at unless `full`.
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
            moves(&text(&line), &mut head, &mut in_brace, &mut depth, full, keep && !full);
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

/// Whether a file of this name may be a DhtmlXQ game: no ending, `.che`, `.txt`, `.dhtmlxq`, or
/// an "ending" that is no file type Coxswain knows (`001-1.古谱的启示`, `game.abc`): one that
/// `space::family` puts in no family.
pub fn dhtml_name(name: &str) -> bool {
    match name.rsplit_once('.') {
        None | Some(("", _)) => true,
        Some((_, e)) => {
            ["che", "txt", "dhtmlxq"].iter().any(|x| e.eq_ignore_ascii_case(x)) || crate::space::family(&e.to_ascii_lowercase()) == crate::space::Family::Other
        }
    }
}

/// Whether the file at `path` is a DhtmlXQ game, by its first bytes: only a name such files
/// may have (`dhtml_name`), and never one only in the cloud.
pub(crate) fn is_dhtml(path: &Path) -> bool {
    let named = path.file_name().is_some_and(|n| dhtml_name(&n.to_string_lossy()));
    if !named || crate::cloud::unasked(path) {
        return false;
    }
    let mut head = [0u8; 64];
    if in_archive(path) {
        return crate::archive::head(path, 64).is_ok_and(|(b, _)| crate::xiangqi::is_dhtml(&b));
    }
    crate::fs::open_regular(path).and_then(|mut f| read_full(&mut f, &mut head)).is_ok_and(|n| crate::xiangqi::is_dhtml(&head[..n]))
}

/// Whether `path` is a file inside an archive (not on the disk): its games are read as it is
/// unpacked (`entry_file`), nothing copied. ChessBase and SCID databases, which are read in
/// pieces from several files, and opening books are not: those are read from a copy.
pub fn in_archive(path: &Path) -> bool {
    !path.exists() && crate::archive::split(path).is_some()
}

/// Whether the games of `path` can be read from inside its archive without a copy: a PGN family
/// file by its name (also packed), and a DhtmlXQ game by its first bytes.
pub fn streams(path: &Path) -> bool {
    if !in_archive(path) || super::scid::is_scid(path) || super::cbh::is_cbh(path) || !crate::archive::streamable(path) {
        return false;
    }
    matches!(super::format(path), Some(super::Format::Pgn | super::Format::Pdn | super::Format::Pbn | super::Format::Kif | super::Format::Csa)) || is_dhtml(path)
}

/// The file at `path` to read from its start: on the disk, or inside an archive as it is
/// unpacked.
fn open_any(path: &Path) -> io::Result<Box<dyn Read + Send>> {
    if in_archive(path) { Ok(Box::new(crate::entry_file::EntryReader::open(path)?)) } else { Ok(Box::new(File::open(path)?)) }
}

/// What one file may cost: a file of endless tiny games, or a small packed file that unpacks
/// to terabytes, stops here, with the games read so far and a note saying so.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Games in the table (52 bytes each, on the disk).
    pub games: usize,
    /// Bytes of memory the names met (players, events, openings) take while the file is
    /// read: 32 a name.
    pub names: usize,
    /// Bytes a packed file is unpacked to, at most.
    pub unpacked: u64,
    /// A plain PGN file inside an archive over this size keeps its games' text in the table.
    pub keep: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { games: 1 << 30, names: 256 << 20, unpacked: 64 << 30, keep: crate::archive::PEEK_MAX }
    }
}

/// The file's text, unpacked as it is read (up to `unpacked` bytes); `read` counts the file's
/// own bytes.
pub(crate) fn reader(path: &Path, read: Arc<AtomicU64>, unpacked: u64) -> io::Result<Box<dyn Read + Send>> {
    let f = BufReader::new(Counted { inner: open_any(path)?, read });
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
        return scan_cbh(path, &AtomicBool::new(false), &AtomicU64::new(0), 0, false, each);
    }
    // The search store reads every PGN file: 1 GiB unpacked of each is plenty.
    if super::scid::is_scid(path) {
        return scan_scid(&super::scid::Base::open(path)?, &AtomicBool::new(false), &Arc::default(), each);
    }
    let format = kind(path);
    let r = reader(path, Arc::default(), 1 << 30)?;
    scan(BufReader::with_capacity(1 << 16, r), &AtomicBool::new(false), format, false, false, each)
}

fn is_pgn(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "pgn")
}

/// The PGN file a ChessBase archive (`.cbv`) of a PGN database unpacks to (`cbv::unpacked`,
/// once per archive as it is); None for any other file.
fn archived_pgn(path: &Path) -> Option<PathBuf> {
    if !super::cbv::is_cbv(path) {
        return None;
    }
    super::cbv::unpacked(path).ok()?.into_iter().next().filter(|p| is_pgn(p))
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
/// `full` as `scan` takes it, for an archive of a PGN database (`archived_pgn`).
fn scan_cbh(path: &Path, stop: &AtomicBool, read: &AtomicU64, size: u64, full: bool, mut each: impl FnMut(&Head) -> bool) -> io::Result<()> {
    let mut head = Head { offset: 0, len: 0, tags: Default::default(), plies: 0, sans: vec![], text: vec![], format: super::Format::Pgn };
    let share = |n: u64, of: u64, base: u64| read.store(base + (n.min(of) as u128 * (size / 2) as u128 / of.max(1) as u128) as u64, Ordering::Relaxed);
    let cbh_len = if super::cbv::is_cbv(path) {
        let mut len = 0;
        let unpacked = super::cbv::unpacked_with(path, &|| stop.load(Ordering::Relaxed), &|n, of| share(n, of, 0))?;
        if let Some(pgn) = unpacked.first().filter(|p| is_pgn(p)) {
            let of = std::fs::metadata(pgn)?.len();
            let counted = Arc::new(AtomicU64::new(0));
            let r = BufReader::with_capacity(1 << 16, Counted { inner: File::open(pgn)?, read: counted.clone() });
            return scan(r, stop, super::Format::Pgn, full, false, |h| {
                share(counted.load(Ordering::Relaxed), of, size / 2);
                each(h)
            });
        }
        for p in unpacked {
            len += std::fs::metadata(&p)?.len();
        }
        Some(len)
    } else {
        None
    };
    let progress = |n: u64| match cbh_len {
        Some(len) => share(n, len, size / 2),
        None => read.store(n, Ordering::Relaxed),
    };
    super::cbh::scan(path, &|| stop.load(Ordering::Relaxed), &progress, |g| {
        for t in &mut head.tags {
            t.clear();
        }
        for (k, v) in &g.tags {
            if let Some(i) = WANTED.iter().position(|w| w == k) {
                head.tags[i].clone_from(v);
            }
        }
        head.offset = g.record;
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

use super::table::{Rec, Table};

#[derive(Default)]
struct Shared {
    /// Rows in the table, readable.
    count: u32,
    /// Names in the table: the names' order is worked out again when more came.
    names: u32,
    done: bool,
    /// Stopped at `Limits`: the table has the games read until then.
    capped: bool,
    error: Option<String>,
    kept: bool,
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
    /// A packed file's restart points (`tar_index`), once read.
    points: Option<Vec<crate::tar_index::Point>>,
    /// A file inside an archive: read at a place as it is unpacked.
    entry: Option<crate::entry_file::EntryFile>,
    /// How the file is packed, told once (a DhtmlXQ game is told by reading its start).
    pack: Pack,
}

impl Source {
    fn read(&mut self, offset: u64, len: u32) -> io::Result<Vec<u8>> {
        let mut buf = vec![0; len.min(MAX_GAME) as usize];
        if self.pack == Pack::None {
            if let Some(e) = &self.entry {
                let n = e.read_at(&mut buf, offset)?;
                buf.truncate(n);
                return Ok(buf);
            }
            let mut f = File::open(&self.path)?;
            f.seek(SeekFrom::Start(offset))?;
            let n = read_full(&mut f, &mut buf)?;
            buf.truncate(n);
            return Ok(buf);
        }
        // The place nearest before the game the first pass kept (`tar_index`: a gzip every few
        // MB, a zstd at each frame), read once: unpacked from there, not from the start.
        // None yet while the first pass reads: looked for again until there are some.
        if self.points.as_ref().is_none_or(|p| p.is_empty()) {
            self.points = Some(crate::tar_index::points_of(&self.path).unwrap_or_default());
        }
        let points = self.points.as_deref().unwrap_or_default();
        if self.open.as_ref().is_none_or(|(_, at)| *at > offset || offset - *at > 4 << 20)
            && let Some(p) = points.iter().rev().find(|p| p.out <= offset && p.out > 0)
        {
            let mut r = crate::tar_index::from_point(&self.path, pack(&self.path) == Pack::Gz, p)?;
            let skip = offset - p.out;
            io::copy(&mut (&mut r).take(skip), &mut io::sink())?;
            let n = read_full(&mut r, &mut buf)?;
            buf.truncate(n);
            return Ok(buf);
        }
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
    /// The table kept from before was taken (`table`): the file was not read again.
    pub kept: bool,
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

/// The games of one PGN file, in a table on the disk (`table`): a file of any number of games
/// takes the same memory, and opened again unchanged it is not read again.
pub struct Library {
    pub path: PathBuf,
    /// `Pgn`, or `Pdn` for draughts.
    pub format: super::Format,
    shared: Arc<Mutex<Shared>>,
    table: Arc<Table>,
    read: Arc<AtomicU64>,
    size: u64,
    stop: Arc<AtomicBool>,
    source: Mutex<Source>,
    /// A SCID database's names and files, when the file is one (`scid`).
    scid: Option<Arc<super::scid::Base>>,
    /// A PGN read out of an archive: each game's text is kept in the table's folder (`GAMES`).
    archived: bool,
    /// A ChessBase archive's unpacked files, kept while this reads them (`cbv::hold`).
    _held: Option<super::cbv::Held>,
    /// The names' order (`Table::ranks`), with how many names there were then.
    ranks: Mutex<Option<(u32, Arc<Vec<u32>>)>>,
}

impl Drop for Library {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Library {
    /// Starts reading the file's games, on a thread of its own; a table kept from before, of
    /// the file as it is, is taken as it is.
    pub fn open(path: &Path) -> io::Result<Library> {
        Library::open_with(path, Limits::default())
    }

    /// `open`, with limits of its own.
    pub fn open_with(path: &Path, limits: Limits) -> io::Result<Library> {
        let inside = in_archive(path);
        let size = if inside { crate::archive::entry_size(path)? } else { std::fs::metadata(path)?.len() };
        if inside && !streams(path) {
            return Err(io::Error::new(io::ErrorKind::Unsupported, path.display().to_string()));
        }
        let read = Arc::new(AtomicU64::new(0));
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        // A plain file inside an archive is read at a place by `entry`; a packed one, or a DhtmlXQ
        // game (read as the PGN it is turned into), from an unpacker (`Source::read`).
        let packing = pack(path);
        // A plain PGN over `PEEK_MAX` inside an archive: its games' text is kept in the table, as a
        // solid archive cannot give a game from its middle (`from_archive`).
        let streamed = inside && packing == Pack::None && kind(path) == super::Format::Pgn && size > limits.keep;
        let entry = if inside && packing == Pack::None && !streamed { Some(crate::entry_file::EntryFile::open(path)?) } else { None };
        let source = Mutex::new(Source { path: path.to_path_buf(), open: None, points: None, entry, pack: packing });
        let scid = if super::scid::is_scid(path) { Some(Arc::new(super::scid::Base::open(path)?)) } else { None };
        let cbh = super::cbh::is_cbh(path);
        let format = if scid.is_some() || cbh { super::Format::Pgn } else { kind(path) };
        let extra = if streamed { format!("{limits:?} {GAMES_FORM}") } else { format!("{limits:?}") };
        let (mut table, mut kept) = Table::open(path, &extra)?;
        // A table of a PGN out of an archive whose games' text is gone is read anew.
        if streamed && kept.is_some() && !table.dir().join(GAMES).exists() {
            let dir = table.dir().to_path_buf();
            drop(table);
            crate::spill::gone(&dir);
            (table, kept) = Table::open(path, &extra)?;
        }
        let table = Arc::new(table);
        let _held = (cbh && super::cbv::is_cbv(path)).then(|| super::cbv::hold(path));
        let lib = |shared, size| Library { path: path.to_path_buf(), format, shared, table: table.clone(), read: read.clone(), size, stop: stop.clone(), source, scid: scid.clone(), archived: streamed, _held, ranks: Mutex::new(None) };
        if let Some(done) = kept {
            {
                let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                (s.count, s.done, s.capped, s.kept) = (done.games, true, done.capped, true);
            }
            read.store(size, Ordering::Relaxed);
            return Ok(lib(shared, size));
        }
        let (sh, st, rd, t) = (shared.clone(), stop.clone(), read.clone(), table.clone());
        let thread = std::thread::Builder::new().name("chess-games".into());
        if let Some(base) = &scid {
            let b = base.clone();
            thread.spawn(move || index(|each| scan_scid(&b, &st, &rd, each), &sh, &t, &st, super::Format::Pgn, limits, || false))?;
            return Ok(lib(shared, size));
        }
        if cbh {
            // An archive is unpacked first, then read: its bar has both (`scan_cbh`).
            let size = if super::cbv::is_cbv(path) { size.saturating_mul(2) } else { size };
            let p = path.to_path_buf();
            thread.spawn(move || index(|each| scan_cbh(&p, &st, &rd, size, true, each), &sh, &t, &st, super::Format::Pgn, limits, || false))?;
            return Ok(lib(shared, size));
        }
        if streamed {
            let (dir, cut, p) = (table.dir().to_path_buf(), rd.clone(), path.to_path_buf());
            thread.spawn(move || index(|each| from_archive(&p, &dir, &st, &rd, limits.unpacked, each), &sh, &t, &st, format, limits, || cut.load(Ordering::Relaxed) >= limits.unpacked))?;
            return Ok(lib(shared, size));
        }
        let r = if matches!(packing, Pack::Gz | Pack::Zst) && !inside { None } else { Some(reader(path, read.clone(), limits.unpacked)?) };
        if r.is_none() {
            File::open(path)?;
        }
        let p = path.to_path_buf();
        thread.spawn(move || {
            // A gzip or zstd is unpacked here, noting the places it can be unpacked from
            // again (`tar_index`), so a game far into it is read without unpacking all before.
            let points = crate::tar_index::Points::default();
            let r: Box<dyn Read> = match r {
                Some(r) => r,
                None => match File::open(&p) {
                    Ok(f) => {
                        let f = BufReader::new(Counted { inner: f, read: rd });
                        if packing == Pack::Gz {
                            Box::new(crate::tar_index::Gz::new(f, Some(points.clone())).take(limits.unpacked))
                        } else {
                            Box::new(crate::tar_index::Zst::new(f, Some(points.clone())).take(limits.unpacked))
                        }
                    }
                    Err(e) => {
                        let mut s = sh.lock().unwrap_or_else(|e| e.into_inner());
                        (s.done, s.error) = (true, Some(e.to_string()));
                        return;
                    }
                },
            };
            // What the packed file unpacked to: at the limit, the rest of it was left.
            let unpacked = Arc::new(AtomicU64::new(0));
            let r = Counted { inner: r, read: unpacked.clone() };
            let packed = packing != Pack::None;
            index(|each| scan(BufReader::with_capacity(1 << 16, r), &st, format, true, false, each), &sh, &t, &st, format, limits, || packed && unpacked.load(Ordering::Relaxed) >= limits.unpacked);
            if !st.load(Ordering::Relaxed) {
                crate::tar_index::save_points(&p, &points.borrow());
            }
        })?;
        Ok(lib(shared, size))
    }

    pub fn progress(&self) -> Progress {
        let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        Progress { games: s.count as usize, done: s.done, capped: s.capped, read: self.read.load(Ordering::Relaxed).min(self.size), size: self.size, error: s.error.clone(), kept: s.kept }
    }

    fn count(&self) -> (u32, u32) {
        let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        (s.count, s.names)
    }

    /// The names' order, worked out once for as many names as there are.
    fn ranks(&self, names: u32) -> Arc<Vec<u32>> {
        let mut r = self.ranks.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((n, ranks)) = r.as_ref()
            && *n == names
        {
            return ranks.clone();
        }
        let ranks = Arc::new(self.table.ranks());
        *r = Some((names, ranks.clone()));
        ranks
    }

    /// The games that `q` lets through, in its order: their indices (from 0). The table is read
    /// through once from the disk; what stays in memory is 4 bytes a game let through (8 while
    /// they are sorted) and a byte a name.
    pub fn query(&self, q: &Query) -> Vec<u32> {
        let (count, names) = self.count();
        let all = Query { sort: q.sort, desc: q.desc, ..Query::default() };
        if *q == all && q.sort == Sort::Number {
            // Every game in the file's order, as while the table fills: nothing to read.
            let ids = 0..count;
            return if q.desc { ids.rev().collect() } else { ids.collect() };
        }
        let player = q.player.trim().to_lowercase();
        let opening = q.opening.trim().to_lowercase();
        // The names that match, looked at once each rather than per game.
        let players = if player.is_empty() { vec![] } else { self.table.matching(&player) };
        let openings = if opening.is_empty() { vec![] } else { self.table.matching(&opening) };
        let result = result_code(q.result.trim());
        let hit = |ids: &Vec<bool>, i: u32| ids.get(i as usize).copied().unwrap_or(false);
        let pass = |r: &Rec| {
            (player.is_empty() || hit(&players, r.white) || hit(&players, r.black))
                && (opening.is_empty() || hit(&openings, r.opening) || eco_text(r.eco).to_lowercase().starts_with(&opening))
                && (q.result.trim().is_empty() || r.result == result)
                && q.from.is_none_or(|y| r.date / 10000 >= y as u32)
                && q.to.is_none_or(|y| r.date / 10000 <= y as u32 && r.date > 0)
                && q.elo.is_none_or(|e| r.white_elo.min(r.black_elo) >= e)
        };
        let rank = matches!(q.sort, Sort::White | Sort::Black | Sort::Event | Sort::Opening).then(|| self.ranks(names));
        // Every key fits in 32 bits: a name's rank (none last), an Elo, a date as yyyymmdd.
        let key = |r: &Rec| -> u32 {
            let name = |id: u32| rank.as_ref().and_then(|k| k.get(id as usize).copied()).unwrap_or(u32::MAX);
            match q.sort {
                Sort::Number => 0,
                Sort::White => name(r.white),
                Sort::Black => name(r.black),
                Sort::Event => name(r.event),
                Sort::Opening => name(r.opening),
                Sort::WhiteElo => r.white_elo as u32,
                Sort::BlackElo => r.black_elo as u32,
                Sort::Result => r.result as u32,
                Sort::Date => r.date,
                Sort::Eco => r.eco as u32,
                Sort::Moves => r.plies as u32,
            }
        };
        let mut ids: Vec<u32> = if q.sort == Sort::Number {
            let mut ids = vec![];
            let _ = self.table.each(count, |i, r| {
                if pass(r) {
                    ids.push(i);
                }
            });
            ids
        } else {
            // The key above the game's number: sorted, equal keys stay in the file's order.
            let mut keyed: Vec<u64> = vec![];
            let _ = self.table.each(count, |i, r| {
                if pass(r) {
                    keyed.push((key(r) as u64) << 32 | i as u64);
                }
            });
            keyed.sort_unstable();
            keyed.into_iter().map(|k| k as u32).collect()
        };
        if q.desc {
            ids.reverse();
        }
        ids
    }

    /// Waits until the table is whole, or its reading failed or stopped, or this thread's
    /// Stop (`stop::asked`: the search helper's scan stopping): false then.
    pub fn wait(&self) -> bool {
        while !self.progress().done {
            if crate::stop::asked() {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        true
    }

    /// Game `i` in one line, as the search store keeps it (`Head::line`), from the table.
    pub fn line(&self, i: u32) -> Option<String> {
        Some(line_of(&self.table, &self.rec(i)?))
    }

    fn rec(&self, i: u32) -> Option<Rec> {
        self.table.get(i, self.count().0)
    }

    /// The game at index `i`, as the table shows it.
    pub fn row(&self, i: u32) -> Option<Row> {
        let r = self.rec(i)?;
        let name = |id: u32| self.table.name(id);
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
        let r = self.rec(i).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such game"))?;
        let (offset, len) = (r.offset, r.len);
        if let Some(base) = &self.scid {
            return base.pgn(offset as u32);
        }
        if self.archived {
            // `offset`: the block, `len`: the game's place in it.
            let mut f = File::open(self.table.dir().join(GAMES))?;
            f.seek(SeekFrom::Start(offset))?;
            let mut n = [0u8; 4];
            f.read_exact(&mut n)?;
            let mut raw = vec![];
            lzma_rust2::Lzma2Reader::new(f.take(u32::from_le_bytes(n) as u64), DICT, None).take(BLOCK_MAX).read_to_end(&mut raw)?;
            return Ok(text(&game_in_block(&raw, len as usize).ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?).into_owned());
        }
        if let Some(pgn) = archived_pgn(&self.path) {
            return Ok(text(&Source { pack: pack(&pgn), path: pgn, open: None, points: None, entry: None }.read(offset, len)?).into_owned());
        }
        if super::cbh::is_cbh(&self.path) {
            return super::cbh::pgn(&self.path, offset);
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

/// The file in a table's folder with the text of the games read out of an archive, in blocks of
/// about `BLOCK` bytes of text, each packed (LZMA2) on its own: a u32 of the packed size, then
/// the packed block. A block unpacked: a u32 of its games, two u32 for each (the size of its
/// tags, the size of its moves, the top bit set when its move numbers are kept as they are), then
/// all the tags, then all the moves (`numbers_out`). Tags next to tags and moves next to moves
/// pack smaller. A row's `offset` is its block's place and `len` its place in the block.
const GAMES: &str = "games";
/// The form of `GAMES`, in the table's `extra`: a table kept in another form is not taken (nor
/// removed: an older version still reads its own), and the file is read again into a new one.
const GAMES_FORM: &str = "games 2";
/// A block is closed once its text is this long (a game is never split).
const BLOCK: usize = 256 << 10;
/// The dictionary of a block's LZMA2: twice the block, so a block is one run.
const DICT: u32 = 2 * BLOCK as u32;
/// The most a block unpacks to: a game of `MAX_GAME` after less than `BLOCK`, and the sizes.
const BLOCK_MAX: u64 = MAX_GAME as u64 + 2 * BLOCK as u64;
/// A game's moves size, with this set: its move numbers kept as they are (`numbers_out`).
const KEPT_NUMBERS: u32 = 1 << 31;

/// Where a game's tags end: after its leading lines that start with `[`, and the blank lines
/// after them.
fn tags_end(game: &[u8]) -> usize {
    let mut at = 0;
    let mut tags = true;
    while at < game.len() {
        let end = game[at..].iter().position(|&b| b == b'\n').map_or(game.len(), |p| at + p + 1);
        let line = game[at..end].trim_ascii();
        if line.is_empty() {
            tags = false;
        } else if !(tags && line[0] == b'[') {
            break;
        }
        at = end;
    }
    at
}

/// A move number (`12.`) at `at` in `b`, not after a digit, without a leading zero and not
/// followed by another `.`: its value and where it ends (after the `.`).
fn number_at(b: &[u8], at: usize) -> Option<(u32, usize)> {
    if (at > 0 && b[at - 1].is_ascii_digit()) || b.get(at) == Some(&b'0') {
        return None;
    }
    let digits = b[at..].iter().take_while(|c| c.is_ascii_digit()).count();
    let dot = at + digits;
    if !(1..=6).contains(&digits) || b.get(dot) != Some(&b'.') || b.get(dot + 1) == Some(&b'.') {
        return None;
    }
    Some((std::str::from_utf8(&b[at..dot]).ok()?.parse().ok()?, dot + 1))
}

/// The moves `b` with each move number that is one more than the one before it (any number met,
/// `number_at`) written as one byte: 1 for `N. `, 2 for `N.` with no space after. Undone by
/// `numbers_in`. False, and nothing written, when `b` has a byte 1 or 2 of its own.
fn numbers_out(b: &[u8], out: &mut Vec<u8>) -> bool {
    if b.iter().any(|&c| c == 1 || c == 2) {
        return false;
    }
    let (mut last, mut at) = (0u32, 0);
    while at < b.len() {
        if let Some((n, end)) = number_at(b, at) {
            let space = b.get(end) == Some(&b' ');
            if n == last.wrapping_add(1) {
                out.push(if space { 1 } else { 2 });
                at = end + space as usize;
            } else {
                out.extend_from_slice(&b[at..end]);
                at = end;
            }
            last = n;
        } else {
            out.push(b[at]);
            at += 1;
        }
    }
    true
}

/// The moves `numbers_out` wrote, as they were.
fn numbers_in(b: &[u8], out: &mut Vec<u8>) {
    let (mut last, mut at) = (0u32, 0);
    while at < b.len() {
        if b[at] == 1 || b[at] == 2 {
            last = last.wrapping_add(1);
            out.extend_from_slice(last.to_string().as_bytes());
            out.extend_from_slice(if b[at] == 1 { b". " } else { b"." });
            at += 1;
        } else if let Some((n, end)) = number_at(b, at) {
            out.extend_from_slice(&b[at..end]);
            last = n;
            at = end;
        } else {
            out.push(b[at]);
            at += 1;
        }
    }
}

/// The text of the game at place `k` in an unpacked block; none when the block is cut short.
fn game_in_block(raw: &[u8], k: usize) -> Option<Vec<u8>> {
    let u32_at = |at: usize| raw.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")));
    let count = u32_at(0)? as usize;
    if k >= count {
        return None;
    }
    let sizes = |i: usize| Some((u32_at(4 + 8 * i)? as usize, u32_at(8 + 8 * i)?));
    // Checked: a damaged block on a 32-bit system must not wrap around.
    let start = count.checked_mul(8)?.checked_add(4)?;
    let (mut tags, mut moves) = (start, start);
    for i in 0..count {
        moves = moves.checked_add(sizes(i)?.0)?;
    }
    for i in 0..k {
        let (t, m) = sizes(i)?;
        tags = tags.checked_add(t)?;
        moves = moves.checked_add((m & !KEPT_NUMBERS) as usize)?;
    }
    let (t, m) = sizes(k)?;
    let mut game = raw.get(tags..tags.checked_add(t)?)?.to_vec();
    let packed = raw.get(moves..moves.checked_add((m & !KEPT_NUMBERS) as usize)?)?;
    if m & KEPT_NUMBERS != 0 {
        game.extend_from_slice(packed);
    } else {
        numbers_in(packed, &mut game);
    }
    Some(game)
}

/// How a block is packed: LZMA2's normal mode, about 16% smaller than preset 3 (used before)
/// but six times slower to pack, so blocks are packed on several threads. The stream carries
/// its own settings, so a block packed either way unpacks the same: no table is read again.
fn packing() -> lzma_rust2::Lzma2Options {
    let mut options = lzma_rust2::Lzma2Options::with_preset(6);
    options.lzma_options.dict_size = DICT;
    // Text: no position bits, and four of the byte before (about 2% smaller than the defaults).
    (options.lzma_options.pb, options.lzma_options.lc) = (0, 4);
    options
}

/// The threads that pack blocks: one block each at a time, so memory stays a few blocks.
fn packers() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 8)
}

/// One block, unpacked, packed.
fn pack_block(raw: &[u8]) -> io::Result<Vec<u8>> {
    let mut z = lzma_rust2::Lzma2Writer::new(Vec::new(), packing());
    z.write_all(raw)?;
    z.finish().map_err(io::Error::other)
}

/// A block being packed, on its own thread.
type Packer = std::thread::JoinHandle<io::Result<Vec<u8>>>;

/// The games of a block being made: their sizes, tags and moves, and their rows, handed on once
/// the block is on the disk (a row is never shown before its text can be read). Closed blocks
/// are packed on threads of their own, at most `threads` at once, and written in turn, so the
/// file is the same whatever the number of threads.
struct Block {
    side: io::BufWriter<File>,
    at: u64,
    sizes: Vec<u8>,
    tags: Vec<u8>,
    moves: Vec<u8>,
    heads: Vec<Head>,
    packing: std::collections::VecDeque<(Packer, Vec<Head>)>,
    threads: usize,
}

impl Block {
    fn new(side: File, threads: usize) -> Block {
        let side = io::BufWriter::with_capacity(1 << 20, side);
        Block { side, at: 0, sizes: vec![], tags: vec![], moves: vec![], heads: vec![], packing: Default::default(), threads: threads.max(1) }
    }

    /// What the block holds so far, unpacked.
    fn len(&self) -> usize {
        self.sizes.len() + self.tags.len() + self.moves.len()
    }

    fn add(&mut self, h: &Head) {
        let (tags, moves) = h.text.split_at(tags_end(&h.text));
        self.tags.extend_from_slice(tags);
        let before = self.moves.len();
        let mut size = if numbers_out(moves, &mut self.moves) { 0 } else { KEPT_NUMBERS };
        if size != 0 {
            self.moves.extend_from_slice(moves);
        }
        size |= (self.moves.len() - before) as u32;
        self.sizes.extend_from_slice(&(tags.len() as u32).to_le_bytes());
        self.sizes.extend_from_slice(&size.to_le_bytes());
        let place = self.heads.len() as u64;
        // The block's place in the file is set once it is written.
        self.heads.push(Head { offset: 0, len: place, tags: h.tags.clone(), plies: h.plies, sans: h.sans.clone(), text: vec![], format: h.format });
    }

    /// The block goes to be packed; the oldest packed blocks are written while `threads` are
    /// packing. False when `each` said stop.
    fn close(&mut self, each: &mut dyn FnMut(&Head) -> bool) -> io::Result<bool> {
        if !self.heads.is_empty() {
            let mut raw = Vec::with_capacity(4 + self.len());
            raw.extend_from_slice(&(self.heads.len() as u32).to_le_bytes());
            raw.append(&mut self.sizes);
            raw.append(&mut self.tags);
            raw.append(&mut self.moves);
            let packer = std::thread::Builder::new().name("pack games".into()).spawn(move || pack_block(&raw))?;
            self.packing.push_back((packer, std::mem::take(&mut self.heads)));
        }
        while self.packing.len() >= self.threads {
            if !self.write_oldest(each)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The block, then every block still being packed, written. False when `each` said stop.
    fn finish(&mut self, each: &mut dyn FnMut(&Head) -> bool) -> io::Result<bool> {
        if !self.close(each)? {
            return Ok(false);
        }
        while !self.packing.is_empty() {
            if !self.write_oldest(each)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The oldest block being packed is written and flushed, then its games are handed on.
    fn write_oldest(&mut self, each: &mut dyn FnMut(&Head) -> bool) -> io::Result<bool> {
        let Some((packer, heads)) = self.packing.pop_front() else { return Ok(true) };
        let packed = packer.join().map_err(|_| io::Error::other("packing a block failed"))??;
        self.side.write_all(&(packed.len() as u32).to_le_bytes())?;
        self.side.write_all(&packed)?;
        self.side.flush()?;
        let at = self.at;
        self.at += 4 + packed.len() as u64;
        let mut more = true;
        for mut h in heads {
            h.offset = at;
            more = more && each(&h);
        }
        Ok(more)
    }
}

/// The games of the PGN file `path` inside an archive, read as it unpacks (`entry_file`): each
/// game's text goes into `GAMES` in `dir` (a solid archive cannot be unpacked from the middle, so
/// the table keeps what it needs to show a game), and the game is handed on with its place
/// there. `unpacked` bytes at most are read.
fn from_archive(path: &Path, dir: &Path, stop: &AtomicBool, read: &Arc<AtomicU64>, unpacked: u64, each: &mut dyn FnMut(&Head) -> bool) -> io::Result<()> {
    let mut block = Block::new(File::create(dir.join(GAMES))?, packers());
    let mut failed = None;
    let r = reader(path, read.clone(), unpacked)?.take(unpacked);
    scan(BufReader::with_capacity(1 << 16, r), stop, super::Format::Pgn, true, true, |h| {
        block.add(h);
        if block.len() < BLOCK {
            return true;
        }
        match block.close(each) {
            Ok(more) => more,
            Err(e) => {
                failed = Some(e);
                false
            }
        }
    })?;
    if let Some(e) = failed {
        return Err(e);
    }
    // The last blocks, unless the pass was stopped.
    if !stop.load(Ordering::Relaxed) {
        block.finish(each)?;
    }
    block.side.get_ref().sync_all()
}

/// What a name costs in memory while the file is read: its entry in the lookup of the names
/// met, by their hash (the names themselves go to the table on the disk).
const NAME_COST: usize = 32;

/// The pass over the games: their rows, handed to the table in batches so it fills as the file
/// is read. `cut`: the packed file was unpacked as far as the limits allow. Stopped (`stop`),
/// the table is left unfinished, and goes.
#[allow(clippy::too_many_arguments)]
fn index(feed: impl FnOnce(&mut dyn FnMut(&Head) -> bool) -> io::Result<()>, shared: &Mutex<Shared>, table: &Table, stop: &AtomicBool, format: super::Format, limits: Limits, cut: impl Fn() -> bool) {
    use std::hash::BuildHasher;
    let chess = format == super::Format::Pgn;
    let (mut games, mut capped) = (0usize, false);
    // Names by a 64-bit hash, seeded for this pass: two names alike in it (one in billions)
    // would show as one.
    let seed = std::collections::hash_map::RandomState::new();
    let mut ids: HashMap<u64, u32> = HashMap::new();
    let mut fresh: Vec<(u32, Box<str>)> = vec![];
    let mut batch: Vec<Rec> = vec![];
    let mut failed: Option<io::Error> = None;
    let intern = |s: &str, ids: &mut HashMap<u64, u32>, fresh: &mut Vec<(u32, Box<str>)>| -> u32 {
        let s = s.trim();
        if s.is_empty() || s == "?" {
            return u32::MAX;
        }
        let next = ids.len() as u32;
        *ids.entry(seed.hash_one(s)).or_insert_with(|| {
            fresh.push((next, s.into()));
            next
        })
    };
    let flush = |batch: &mut Vec<Rec>, fresh: &mut Vec<(u32, Box<str>)>, names: u32| -> io::Result<()> {
        table.append(batch, fresh)?;
        let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
        s.count += batch.len() as u32;
        s.names = names;
        batch.clear();
        fresh.clear();
        Ok(())
    };
    let mut last = std::time::Instant::now();
    let res = feed(&mut |h| {
        if games >= limits.games || ids.len() * NAME_COST >= limits.names {
            capped = true;
            return false;
        }
        games += 1;
        let (eco, opening) = h.opening();
        let rec = Rec {
            offset: h.offset,
            len: h.len.min(MAX_GAME as u64) as u32,
            white: intern(h.tag(WHITE), &mut ids, &mut fresh),
            black: intern(h.tag(BLACK), &mut ids, &mut fresh),
            event: intern(h.tag(EVENT), &mut ids, &mut fresh),
            site: intern(h.tag(SITE), &mut ids, &mut fresh),
            opening: intern(&opening, &mut ids, &mut fresh),
            date: date_code(h.tag(DATE)),
            white_elo: h.tag(WHITE_ELO).parse().unwrap_or(0),
            black_elo: h.tag(BLACK_ELO).parse().unwrap_or(0),
            eco: eco_code(&eco),
            plies: h.plies.min(u16::MAX as u32) as u16,
            result: result_code(h.tag(RESULT)),
            result_name: if chess { u32::MAX } else { intern(h.tag(RESULT), &mut ids, &mut fresh) },
        };
        batch.push(rec);
        if batch.len() >= 4096 && last.elapsed().as_millis() >= 200 {
            if let Err(e) = flush(&mut batch, &mut fresh, ids.len() as u32) {
                failed = Some(e);
                return false;
            }
            last = std::time::Instant::now();
        }
        true
    });
    let res = res.and_then(|()| match failed {
        Some(e) => Err(e),
        None => flush(&mut batch, &mut fresh, ids.len() as u32),
    });
    let capped = capped || cut();
    let res = res.and_then(|()| if stop.load(Ordering::Relaxed) { Ok(()) } else { table.finish(games as u32, capped) });
    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
    s.done = true;
    s.capped = capped;
    if let Err(e) = res {
        s.error = Some(e.to_string());
    }
}

/// A game in one line from its row: players, event, site, date, opening and result, as
/// `Head::line` makes it from the tags.
fn line_of(table: &Table, r: &Rec) -> String {
    let name = |id: u32| table.name(id);
    let opening = format!("{} {}", eco_text(r.eco), name(r.opening));
    let parts = [
        format!("{} – {}", name(r.white), name(r.black)),
        name(r.event),
        name(r.site),
        date_text(r.date),
        opening.trim().to_string(),
        if r.result_name == u32::MAX { result_text(r.result).to_string() } else { name(r.result_name) },
    ];
    let parts: Vec<&str> = parts.iter().map(|s| s.as_str()).filter(|s| !s.is_empty() && *s != "?" && *s != "*").collect();
    parts.join(" · ")
}

/// The words of `line` that hold one of `words` (lower case), between `store::MARK`s.
fn marked(line: &str, words: &[String]) -> String {
    let (open, close) = crate::store::MARK;
    let mut out = String::with_capacity(line.len() + 8);
    let mut word = String::new();
    let end = |word: &mut String, out: &mut String| {
        let lower = word.to_lowercase();
        if !word.is_empty() && words.iter().any(|w| lower.contains(w.as_str())) {
            out.push(open);
            out.push_str(word);
            out.push(close);
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in line.chars() {
        if c.is_alphanumeric() {
            word.push(c);
        } else {
            end(&mut word, &mut out);
            out.push(c);
        }
    }
    end(&mut word, &mut out);
    out
}

/// The games of the game files Find reads (`within`, by their path) that have every word of
/// `query` in a player's name, the event, the site or the opening, or as their ECO code (or
/// its start) or year: through each file's kept games table (`table::whole`), so a database of
/// millions of games is looked through whole. For each file with any: its first `max` games
/// as hits (each with its game), and how many it has in all.
pub fn find(query: &str, within: &dyn Fn(&Path) -> bool, max: usize) -> Vec<(PathBuf, Vec<crate::index::Hit>, usize)> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.replace('"', "").to_lowercase()).filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return vec![];
    }
    let mut out = vec![];
    for (table, done) in super::table::whole(within) {
        let names: Vec<Vec<bool>> = words.iter().map(|w| table.matching(w)).collect();
        let has = |ids: &Vec<bool>, id: u32| ids.get(id as usize).copied().unwrap_or(false);
        // A year as a number; an ECO code or its start (`b9`, `b90`) as the range of codes it
        // names, as `Rec::eco` keeps them.
        let year = |w: &str| (w.len() == 4).then(|| w.parse::<u32>().ok()).flatten();
        let eco = |w: &str| -> Option<(u16, u16)> {
            let b = w.as_bytes();
            if !(2..=3).contains(&b.len()) || !(b'a'..=b'e').contains(&b[0]) || !b[1..].iter().all(u8::is_ascii_digit) {
                return None;
            }
            let base = (b[0] - b'a' + 1) as u16 * 100;
            let tens = (b[1] - b'0') as u16 * 10;
            Some(if b.len() == 3 { (base + tens + (b[2] - b'0') as u16, base + tens + (b[2] - b'0') as u16) } else { (base + tens, base + tens + 9) })
        };
        type Wanted = (Option<u32>, Option<(u16, u16)>);
        let wanted: Vec<Wanted> = words.iter().map(|w| (year(w), eco(w))).collect();
        // A word no name has, and that is no year nor ECO code: no game here has it.
        if names.iter().zip(&wanted).any(|(ids, (y, e))| !ids.contains(&true) && y.is_none() && e.is_none()) {
            continue;
        }
        let (mut hits, mut n) = (vec![], 0usize);
        let _ = table.each(done.games, |i, r| {
            let every = names.iter().zip(&wanted).all(|(ids, (y, e))| {
                [r.white, r.black, r.event, r.site, r.opening].iter().any(|&id| has(ids, id))
                    || y.is_some_and(|y| r.date / 10000 == y)
                    || e.is_some_and(|(lo, hi)| (lo..=hi).contains(&r.eco))
            });
            if every {
                n += 1;
                if hits.len() < max {
                    hits.push(i);
                }
            }
        });
        if n == 0 {
            continue;
        }
        let hits = hits
            .into_iter()
            .filter_map(|i| {
                let line = line_of(&table, &table.get(i, done.games)?);
                Some(crate::index::Hit { path: done.source.clone(), is_dir: false, snippet: Some(marked(&line, &words)), similar: None, game: Some(i) })
            })
            .collect();
        out.push((done.source.clone(), hits, n));
    }
    out
}

/// What Ask is told about the players a question names, from the games tables of the game
/// files `within` takes (`table::whole`): for each file, the player whose name holds most of
/// the question's words (at least one, of three letters or more), with the games' count, their
/// years, the results as White and as Black, the openings and opponents met most, the wins
/// over the highest rated, and some games with their numbers. Each file's text up to `bytes`.
pub fn ask_about(question: &str, within: &dyn Fn(&Path) -> bool, bytes: usize) -> Vec<(PathBuf, String)> {
    use std::collections::HashMap;
    let mut words: Vec<String> = question
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 3 && !w.chars().all(|c| c.is_ascii_digit()))
        .collect();
    words.sort();
    words.dedup();
    if words.is_empty() || bytes == 0 {
        return vec![];
    }
    // A word of a name as a whole word of it: "levon" in "Aronian, Levon", not "the" in
    // "Matthews".
    let whole_word = |name: &str, w: &str| name.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|p| p == w);
    let (mut out, mut left) = (vec![], bytes);
    for (table, done) in super::table::whole(within) {
        // How many of the words each name holds as a whole word. A word inside very many names
        // ("the", "who") names no one: left out, and the table is passed over at once when no
        // word is left.
        let mut hold: HashMap<u32, usize> = HashMap::new();
        for w in &words {
            let ids: Vec<u32> = table.matching(w).iter().enumerate().filter(|(_, m)| **m).map(|(id, _)| id as u32).collect();
            if ids.len() > 500 {
                continue;
            }
            for id in ids {
                if whole_word(&table.name(id), w) {
                    *hold.entry(id).or_default() += 1;
                }
            }
        }
        if hold.is_empty() {
            continue;
        }
        // Players only: the names that stand as White or Black, with their games.
        let mut games: HashMap<u32, u32> = HashMap::new();
        let _ = table.each(done.games, |_, r| {
            for id in [r.white, r.black] {
                if hold.contains_key(&id) {
                    *games.entry(id).or_default() += 1;
                }
            }
        });
        // The most words, then the most games.
        let Some((&player, _)) = games.iter().max_by_key(|(id, n)| (hold.get(*id).copied().unwrap_or(0), **n, std::cmp::Reverse(**id))) else { continue };
        let name = table.name(player);
        #[derive(Default)]
        struct Tally {
            n: u32,
            white: [u32; 3],
            black: [u32; 3],
            years: (u32, u32),
            openings: HashMap<(u16, u32), u32>,
            opponents: HashMap<u32, [u32; 3]>,
            best: Vec<(u16, u32)>,
            some: Vec<u32>,
        }
        let mut t = Tally { years: (u32::MAX, 0), ..Tally::default() };
        let _ = table.each(done.games, |i, r| {
            let white = r.white == player;
            if !white && r.black != player {
                return;
            }
            t.n += 1;
            let y = r.date / 10000;
            if y > 0 {
                t.years = (t.years.0.min(y), t.years.1.max(y));
            }
            // 0 won, 1 drawn, 2 lost, from the player's side.
            let res = match (r.result, white) {
                (1, true) | (2, false) => Some(0),
                (3, _) => Some(1),
                (1, false) | (2, true) => Some(2),
                _ => None,
            };
            let (opp, opp_elo) = if white { (r.black, r.black_elo) } else { (r.white, r.white_elo) };
            if opp == player {
                return;
            }
            if let Some(k) = res {
                if white { t.white[k] += 1 } else { t.black[k] += 1 }
                t.opponents.entry(opp).or_default()[k] += 1;
                if k == 0 && opp_elo > 0 {
                    t.best.push((opp_elo, i));
                    if t.best.len() > 64 {
                        t.best.sort_unstable_by(|a, b| b.cmp(a));
                        t.best.truncate(8);
                    }
                }
            } else {
                t.opponents.entry(opp).or_default();
            }
            if r.eco > 0 || r.opening != u32::MAX {
                *t.openings.entry((r.eco, r.opening)).or_default() += 1;
            }
            if t.some.len() < 5 {
                t.some.push(i);
            }
        });
        let line = |i: u32| table.get(i, done.games).map(|r| format!("game {}: {}", i + 1, line_of(&table, &r))).unwrap_or_default();
        let score = |r: &[u32; 3]| format!("{} won, {} drawn, {} lost", r[0], r[1], r[2]);
        let mut text = format!("Games of {name} in this database: {} (of {}).\n", t.n, done.games);
        if t.years.1 > 0 {
            text.push_str(&format!("Years: {} to {}.\n", t.years.0, t.years.1));
        }
        text.push_str(&format!("As White: {}. As Black: {}.\n", score(&t.white), score(&t.black)));
        let mut openings: Vec<_> = t.openings.into_iter().collect();
        openings.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let openings: Vec<String> = openings.iter().take(6).map(|((eco, o), n)| format!("{} {} ({n})", eco_text(*eco), table.name(*o)).trim().to_string()).collect();
        if !openings.is_empty() {
            text.push_str(&format!("Openings played most: {}.\n", openings.join("; ")));
        }
        let mut opponents: Vec<_> = t.opponents.into_iter().filter(|(id, _)| *id != u32::MAX).collect();
        opponents.sort_by(|a, b| (b.1.iter().sum::<u32>()).cmp(&a.1.iter().sum::<u32>()).then(a.0.cmp(&b.0)));
        let opponents: Vec<String> = opponents.iter().take(6).map(|(id, r)| format!("{} ({})", table.name(*id), score(r))).collect();
        if !opponents.is_empty() {
            text.push_str(&format!("Opponents met most: {}.\n", opponents.join("; ")));
        }
        t.best.sort_unstable_by(|a, b| b.cmp(a));
        if !t.best.is_empty() {
            text.push_str("Wins over the highest rated:\n");
            for (_, i) in t.best.iter().take(5) {
                text.push_str(&format!("- {}\n", line(*i)));
            }
        }
        text.push_str("Some games:\n");
        for i in &t.some {
            text.push_str(&format!("- {}\n", line(*i)));
        }
        // Whole lines, all files' together within `bytes`.
        if text.len() > left {
            let mut end = left;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            end = text[..end].rfind('\n').map_or(end, |n| n + 1);
            text.truncate(end);
        }
        left -= text.len();
        if !text.is_empty() {
            out.push((done.source.clone(), text));
        }
        if left == 0 {
            break;
        }
    }
    out
}

/// The search store's text of a PGN file: a line per game (`Head::line`), up to `max` bytes.
/// A file larger than that is read through its games table, which is kept (`table`), so the
/// file is read once for both, and Find looks through every game in it (`find`).
pub fn search_text(path: &Path, max: usize) -> Option<String> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > max as u64)
        && let Ok(lib) = Library::open(path)
    {
        // Stopped meanwhile: no text; the caller sees its stop and keeps the file unread.
        if !lib.wait() {
            return None;
        }
        if lib.progress().error.is_none() {
            let mut out = String::new();
            for i in 0..lib.progress().games as u32 {
                let Some(line) = lib.line(i) else { break };
                if out.len() + line.len() + 1 > max {
                    break;
                }
                out.push_str(&line);
                out.push('\n');
            }
            return (!out.is_empty()).then_some(out);
        }
    }
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
    if let Some(pgn) = archived_pgn(path) {
        return positions(&pgn, stop, each);
    }
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

    /// The games of a PGN file and of a DhtmlXQ game inside a zip are read as they are unpacked:
    /// the table, a game at a place before the last one read, no copy on the disk.
    #[test]
    fn games_inside_an_archive_are_read_without_a_copy() {
        let d = tempdir::Dir::new();
        let src = d.0.join("src");
        std::fs::create_dir_all(&src).unwrap();
        // More than a block of the entry's stream: the games are read across blocks.
        let n = 4000;
        std::fs::write(src.join("many.pgn"), TWO.repeat(n)).unwrap();
        let dhtml = "[DhtmlXQ]\n[DhtmlXQ_ver]www_vietcotuong_com[/DhtmlXQ_ver]\n[DhtmlXQ_red]Red[/DhtmlXQ_red]\n[DhtmlXQ_black]Black[/DhtmlXQ_black]\n[DhtmlXQ_binit]0919293949596979891777062646668600102030405060708012720323436383[/DhtmlXQ_binit]\n[DhtmlXQ_movelist]262563641927[/DhtmlXQ_movelist]\n";
        std::fs::write(src.join("game.che"), dhtml).unwrap();
        std::fs::write(src.join("notes.txt"), "just words\n").unwrap();
        let zip = d.0.join("games.zip");
        crate::archive::create(&zip, &[src.join("many.pgn"), src.join("game.che"), src.join("notes.txt")]).unwrap();
        let copies = || crate::archive::peek_folder().ok().and_then(|f| std::fs::read_dir(f).ok()).map_or(0, |r| r.count());
        let before = copies();
        let (pgn, che) = (zip.join("many.pgn"), zip.join("game.che"));
        assert!(streams(&pgn) && streams(&che) && !streams(&zip.join("notes.txt")));
        let wait = |l: &Library| {
            let limit = std::time::Instant::now() + crate::test_limit(std::time::Duration::from_secs(60));
            while !l.progress().done {
                assert!(std::time::Instant::now() < limit, "games never read");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        let l = Library::open(&pgn).unwrap();
        wait(&l);
        let progress = l.progress();
        assert_eq!((progress.games, progress.error), (2 * n, None));
        // The last game first, then the first (a place before the last one read).
        for i in [2 * n as u32 - 1, 0] {
            assert_eq!(l.game(i).unwrap().tags.iter().find(|(k, _)| k == "Event").map(|(_, v)| v.as_str()), Some(if i % 2 == 0 { "Paris" } else { "London" }));
        }
        let x = Library::open(&che).unwrap();
        wait(&x);
        assert_eq!((x.format, x.progress().games), (crate::chess::Format::Xiangqi, 1));
        assert!(x.game(0).unwrap().tags.iter().any(|(_, v)| v == "Red"));
        // A file that is no game, and one in a disk image or locked, is not read so.
        assert!(Library::open(&zip.join("notes.txt")).is_err());
        assert_eq!(copies(), before, "no copy was made on the disk");
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

    /// Opened again, unchanged, the table kept on the disk is taken: no game is read again, and
    /// it shows, sorts and filters as before. Changed, the file is read anew.
    #[test]
    fn a_table_kept_on_the_disk() {
        let (l, d) = lib(TWO.as_bytes(), "kept.pgn");
        let p = d.0.join("kept.pgn");
        assert!(!l.progress().kept);
        let by_white = Query { sort: Sort::White, ..Query::default() };
        let (rows, order) = ((0..2).map(|i| l.row(i).unwrap().white).collect::<Vec<_>>(), l.query(&by_white));
        drop(l);
        let again = Library::open(&p).unwrap();
        let pr = again.progress();
        assert!(pr.kept && pr.done && pr.games == 2, "{pr:?}");
        assert_eq!(again.read.load(Ordering::Relaxed), pr.size, "the bar is full");
        assert_eq!(((0..2).map(|i| again.row(i).unwrap().white).collect::<Vec<_>>(), again.query(&by_white)), (rows, order));
        assert_eq!(again.query(&Query { player: "morphy".into(), ..Query::default() }), vec![0]);
        assert!(again.text(1).unwrap().contains("Anderssen"));
        drop(again);
        std::fs::write(&p, &TWO[..TWO.find("\n\n[Event \"London").unwrap() + 1]).unwrap();
        let changed = Library::open(&p).unwrap();
        while !changed.progress().done {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!((changed.progress().kept, changed.progress().games), (false, 1));
    }

    /// A game file larger than the search store keeps is read through its games table, kept;
    /// Find then looks through every game of it, each a hit with its game's number, its words
    /// marked; a file Find does not read, or one changed since, is not looked through.
    #[test]
    fn every_game_of_a_big_file_found() {
        let d = tempdir::Dir::new();
        let p = d.0.join("big.pgn");
        let mut text = String::new();
        for i in 0..300 {
            text.push_str(&format!("[White \"Player {i}\"]\n[Black \"Aronian, Levon\"]\n[Site \"Wijk{i}\"]\n[Date \"20{:02}.01.01\"]\n[Result \"0-1\"]\n\n1.d4 Nf6 0-1\n\n", i % 30));
        }
        std::fs::write(&p, &text).unwrap();
        // Only the first games' lines fit; the table has them all.
        let kept = search_text(&p, 200).unwrap();
        assert!(kept.lines().count() < 5 && kept.starts_with("Player 0 – Aronian, Levon · Wijk0 · 2000.01.01"), "{kept}");
        // Only this test's file: other tests' tables share the cache.
        let me = std::path::absolute(&p).unwrap();
        let all = |q: &Path| q == me;
        let found = find("aronian wijk299", &all, 10);
        assert_eq!(found.len(), 1);
        let (path, hits, n) = &found[0];
        assert_eq!((path, n, hits.len(), hits[0].game), (&std::path::absolute(&p).unwrap(), &1, 1, Some(299)));
        let (o, c) = crate::store::MARK;
        assert!(hits[0].snippet.as_deref().unwrap().contains(&format!("{o}Aronian{c}, Levon")), "{hits:?}");
        // Many: the first few as hits, all counted; by year and ECO-less words too.
        let found = find("Aronian 2005", &all, 3);
        assert_eq!((found[0].1.len(), found[0].2, found[0].1[0].game), (3, 10, Some(5)));
        assert!(find("aronian", &|_: &Path| false, 3).is_empty());
        std::fs::write(&p, format!("{text}\n")).unwrap();
        assert!(find("aronian", &all, 3).is_empty(), "changed since: not looked through");
    }

    /// The search helper stopping: a big file's text read through its table gives up at
    /// once, with no text (its file stays unread), counted by the games read, not a clock.
    #[test]
    fn a_stop_ends_the_read_of_a_big_file() {
        let d = tempdir::Dir::new();
        let p = d.0.join("stopped.pgn");
        let game = "[White \"A\"]\n[Black \"B\"]\n\n1.e4 e5 *\n\n";
        std::fs::write(&p, game.repeat(200_000)).unwrap();
        let stop = std::sync::Arc::new(AtomicBool::new(true));
        assert_eq!(crate::stop::during(&stop, || search_text(&p, 100)), None);
        // Not stopped: the same file gives its text.
        assert!(search_text(&p, 100).is_some());
    }

    /// Ask is told about the player the question names, from the games table: games, years,
    /// results by colour, openings, opponents, best wins, some games by number.
    #[test]
    fn ask_about_a_player() {
        let d = tempdir::Dir::new();
        let p = d.0.join("ask.pgn");
        let mut text = String::new();
        for i in 0..40 {
            let (w, b, res) = if i % 2 == 0 { ("Aronian, Levon", format!("Rival {}", i % 3), "1-0") } else { ("Other, One", "Aronian, Levon".to_string(), "1/2-1/2") };
            text.push_str(&format!("[White \"{w}\"]\n[Black \"{b}\"]\n[WhiteElo \"2700\"]\n[BlackElo \"{}\"]\n[Date \"{}.01.01\"]\n[Result \"{res}\"]\n\n1.e4 e5 2.Nf3 Nc6 3.Bb5 {res}\n\n", 2600 + i, 2000 + i % 10));
        }
        std::fs::write(&p, &text).unwrap();
        let _ = search_text(&p, 100);
        let me = std::path::absolute(&p).unwrap();
        let only = |q: &Path| q == me;
        let said = ask_about("who is Levon Aronian and what do we have about him?", &only, 4000);
        assert_eq!(said.len(), 1);
        let s = &said[0].1;
        assert!(s.starts_with("Games of Aronian, Levon in this database: 40 (of 40)."), "{s}");
        assert!(s.contains("Years: 2000 to 2009.") && s.contains("As White: 20 won, 0 drawn, 0 lost. As Black: 0 won, 20 drawn, 0 lost."), "{s}");
        assert!(s.contains("C60 Ruy Lopez") && s.contains("Other, One (0 won, 20 drawn, 0 lost)"), "{s}");
        assert!(s.contains("Wins over the highest rated:\n- game 39:"), "{s}");
        assert!(ask_about("what about Kasparov?", &only, 4000).is_empty());
        // No player named: no one is picked for a word inside a name ("the" in "Other").
        assert!(ask_about("what are the most played openings?", &only, 4000).is_empty());
        assert!(ask_about("Aronian", &only, 30)[0].1.len() <= 30);
    }

    /// A PGN inside a 7z is read as the archive unpacks, with no copy of it made: the table
    /// has the games, and a game opens from the text the table kept of it, also when the table
    /// is opened again (the archive then not read at all).
    #[test]
    fn a_pgn_read_out_of_an_archive() {
        let d = tempdir::Dir::new();
        let pgn = TWO.repeat(3000);
        std::fs::write(d.0.join("games.pgn"), &pgn).unwrap();
        let seven = d.0.join("big.7z");
        crate::archive::create(&seven, &[d.0.join("games.pgn")]).unwrap();
        std::fs::remove_file(d.0.join("games.pgn")).unwrap();
        let p = seven.join("games.pgn");
        let wait = |l: &Library| {
            while !l.progress().done {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        let keep = Limits { keep: 0, ..Limits::default() };
        let l = Library::open_with(&p, keep).unwrap();
        wait(&l);
        let pr = l.progress();
        assert_eq!((pr.games, pr.error, pr.capped), (6000, None, false));
        assert_eq!(pr.read, pgn.len() as u64, "the bar is full");
        let r = l.row(5998).unwrap();
        assert_eq!((r.white.as_str(), r.moves, r.eco.as_str()), ("Morphy, Paul", 4, "C41"));
        assert_eq!(l.text(5999).unwrap(), TWO[TWO.find("[Event \"London").unwrap()..]);
        assert!(l.text(0).unwrap().starts_with("[Event \"Paris\"]"));
        assert_eq!(l.text(2501).unwrap(), TWO[TWO.find("[Event \"London").unwrap()..].to_string(), "a game in a later block");
        assert_eq!(l.game(0).unwrap().main_line().len(), 7);
        drop(l);
        // A later run prunes the tables: the archive is there, so is the table.
        crate::chess::table::prune_in(&crate::chess::table::folder().unwrap(), crate::chess::table::KEPT, crate::chess::table::MOST);
        // Again, the archive unchanged: the table and the games' text are taken from the disk.
        let again = Library::open_with(&p, keep).unwrap();
        assert!(again.progress().kept && again.progress().games == 6000);
        assert!(again.text(1).unwrap().contains("[not a tag]"));
        drop(again);
        // A table kept in the form before (2.36.0's, without `GAMES_FORM` in its key) is not
        // taken, so the file is read again, nor removed, as that version still reads it.
        let root = crate::chess::table::folder().unwrap();
        let source = std::path::absolute(&p).unwrap();
        let ours = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|d| std::fs::read(d.join("done")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).is_some_and(|v| v["source"].as_str() == source.to_str()))
            .unwrap();
        let old = ours.with_file_name(format!("{}-old-0", ours.file_name().unwrap().to_string_lossy().split('-').next().unwrap()));
        std::fs::create_dir(&old).unwrap();
        for f in ["rows", "names.db", GAMES] {
            std::fs::copy(ours.join(f), old.join(f)).unwrap();
        }
        let done = std::fs::read_to_string(ours.join("done")).unwrap().replace(&format!(" {GAMES_FORM}"), "");
        std::fs::write(old.join("done"), done).unwrap();
        std::fs::remove_dir_all(&ours).unwrap();
        let anew = Library::open_with(&p, keep).unwrap();
        assert!(!anew.progress().kept, "read again");
        wait(&anew);
        assert_eq!(anew.text(5999).unwrap(), TWO[TWO.find("[Event \"London").unwrap()..]);
        assert!(old.join("done").exists(), "the older table stays");
    }

    /// A game's moves lose their move numbers in the table and get them back as they were, also
    /// numbers out of turn, black's `1...`, numbers without a space, in comments and variations,
    /// and moves with a byte the packing uses.
    #[test]
    fn move_numbers_come_back_as_they_were() {
        let cases: &[&[u8]] = &[
            b"1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1-0\n",
            b"1.e4 e5 2.Nf3 {12. a comment} 2... Nc6 (2... d6 3. d4) 3.Bb5 *\r\n",
            b"5. e4 e5 6. Nf3 07. Bc4 11. a3 12.\n13. h3 1234567. x 999999. 1000000. 0",
            b"",
            b"1.",
            b"1. e4 \x01 2. d4",
            b"[not a tag] 1. e4\n\n",
            b"1.2. 3.4.",
            b"1. 1. 2.",
            b"12.3.",
            b"999999. 1000000. 1000001.",
            b"1.\r\n2.\r\n",
            b"\xff1. e4\xff",
        ];
        for &moves in cases {
            let mut packed = vec![];
            if numbers_out(moves, &mut packed) {
                let mut back = vec![];
                numbers_in(&packed, &mut back);
                assert_eq!(back, moves, "{}", String::from_utf8_lossy(moves));
            } else {
                assert!(packed.is_empty() && moves.contains(&1));
            }
        }
        let mut packed = vec![];
        assert!(numbers_out(cases[0], &mut packed));
        assert_eq!(packed, b"\x01e4 e5 \x01Nf3 Nc6 \x01Bb5 a6 1-0\n");
        // Each game of a block back as it was, tags and moves.
        let d = tempdir::Dir::new();
        let mut block = Block::new(File::create(d.0.join(GAMES)).unwrap(), 1);
        let texts: Vec<Vec<u8>> = TWO.split_inclusive("1-0\n").map(|g| g.as_bytes().to_vec()).chain(cases.iter().map(|c| c.to_vec())).collect();
        for t in &texts {
            block.add(&Head { offset: 0, len: 0, tags: Default::default(), plies: 0, sans: vec![], text: t.clone(), format: super::super::Format::Pgn });
        }
        assert!(block.finish(&mut |_| true).unwrap());
        drop(block);
        let packed = std::fs::read(d.0.join(GAMES)).unwrap();
        let mut raw = vec![];
        lzma_rust2::Lzma2Reader::new(&packed[4..], DICT, None).read_to_end(&mut raw).unwrap();
        for (k, t) in texts.iter().enumerate() {
            assert_eq!(game_in_block(&raw, k).as_ref(), Some(t), "game {k}");
        }
        assert_eq!(game_in_block(&raw, texts.len()), None);
        assert_eq!(game_in_block(&raw[..raw.len() - 1], texts.len() - 1), None, "a block cut short");
        // A block packed as before (preset 3) unpacks the same way, so such tables stay.
        let mut before = lzma_rust2::Lzma2Options::with_preset(3);
        before.lzma_options.dict_size = DICT;
        let mut z = lzma_rust2::Lzma2Writer::new(Vec::new(), before);
        z.write_all(&raw).unwrap();
        let mut again = vec![];
        lzma_rust2::Lzma2Reader::new(&z.finish().unwrap()[..], DICT, None).read_to_end(&mut again).unwrap();
        assert_eq!(again, raw);
    }

    /// Blocks packed on several threads make the same file as on one, hand the games on in their
    /// order with their block's place, and never pack more than `threads` blocks at once.
    #[test]
    fn blocks_pack_the_same_on_any_number_of_threads() {
        let d = tempdir::Dir::new();
        let texts: Vec<Vec<u8>> = (0..3000).map(|i| format!("[Event \"E{i}\"]\n[White \"P{}\"]\n\n1. e4 e5 2. Nf3 {{{i}}} {} 1-0\n\n", i % 97, "a3 h6 ".repeat(i % 40)).into_bytes()).collect();
        let run = |threads: usize| {
            let path = d.0.join(format!("games-{threads}"));
            let mut block = Block::new(File::create(&path).unwrap(), threads);
            let mut seen = vec![];
            for t in &texts {
                block.add(&Head { offset: 0, len: 0, tags: Default::default(), plies: 0, sans: vec![], text: t.clone(), format: super::super::Format::Pgn });
                // Small blocks, so there are many of them.
                if block.len() >= 4096 {
                    assert!(block.close(&mut |h| { seen.push((h.offset, h.len)); true }).unwrap());
                    assert!(block.packing.len() < threads.max(1), "at most `threads` blocks packing");
                }
            }
            assert!(block.finish(&mut |h| { seen.push((h.offset, h.len)); true }).unwrap());
            drop(block);
            (std::fs::read(&path).unwrap(), seen)
        };
        let (one, rows) = run(1);
        assert!(rows.iter().filter(|r| r.1 == 0).count() > 100, "many blocks");
        for threads in [2, 3, 8] {
            assert_eq!(run(threads), (one.clone(), rows.clone()), "{threads} threads");
        }
        // Each game back from its block's place.
        for (k, &(offset, len)) in rows.iter().enumerate() {
            let at = offset as usize;
            let n = u32::from_le_bytes(one[at..at + 4].try_into().unwrap()) as usize;
            let mut raw = vec![];
            lzma_rust2::Lzma2Reader::new(&one[at + 4..at + 4 + n], DICT, None).read_to_end(&mut raw).unwrap();
            assert_eq!(game_in_block(&raw, len as usize).unwrap(), texts[k]);
        }
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

    /// A game deep in a gzip or zstd file is unpacked from the nearest place the first pass
    /// kept before it, not from the file's start: what is unpacked to no use is counted.
    #[test]
    fn a_game_deep_in_a_packed_file() {
        let mut text = String::new();
        for i in 0..40_000 {
            text.push_str(&format!("[White \"W{i}\"]\n[Black \"B{}\"]\n\n1.e4 e5 2.Nf3 {{{}}} *\n\n", i % 7, "z".repeat(i % 300)));
        }
        let mut gz = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
        gz.write_all(text.as_bytes()).unwrap();
        let mut zst = vec![];
        ruzstd::encoding::compress(text.as_bytes(), &mut zst, ruzstd::encoding::CompressionLevel::Fastest);
        for (name, bytes, kind) in [("deep.pgn.gz", gz.finish().unwrap(), crate::archive::Pack::Gz), ("deep.pgn.zst", zst, crate::archive::Pack::Zst)] {
            let (l, d) = lib(&bytes, name);
            let last = l.progress().games as u32 - 1;
            assert_eq!(last, 39_999);
            assert!(l.text(last).unwrap().contains("W39999"));
            assert!(l.text(3).unwrap().contains("W3\""));
            let at = l.rec(last).unwrap().offset;
            match crate::tar_index::from(&d.0.join(name), kind, at).unwrap() {
                // At most a few MB unpacked before the game, of the file's 8 MB.
                Some((_, skip)) => assert!(skip <= 4 << 20 && at > 6 << 20, "{skip} of {at}"),
                // A zstd of one frame has no place but its start.
                None => assert_eq!(kind, crate::archive::Pack::Zst),
            }
        }
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
        let p2 = wait(&Library::open_with(&p, Limits { names: 16 * 1024, ..full }).unwrap());
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

    /// What Ask is told of a player in a big file, and how long it takes:
    /// `COXSWAIN_CHESS_BIG=/path/to/big.pgn cargo test --release -p coxswain-core ask_big --
    /// --ignored --nocapture`.
    #[test]
    #[ignore]
    fn ask_big() {
        let Ok(p) = std::env::var("COXSWAIN_CHESS_BIG") else { return };
        let l = Library::open(Path::new(&p)).unwrap();
        l.wait();
        let t = std::time::Instant::now();
        let said = ask_about("who is Levon Aronian and what info do we have about him", &|_: &Path| true, 6000);
        println!("{:?}\n{}", t.elapsed(), said.first().map_or("", |s| s.1.as_str()));
        let t = std::time::Instant::now();
        let found = find("Aronian Carlsen 2014", &|_: &Path| true, 5);
        println!("find {:?}: {:?}", t.elapsed(), found.iter().map(|f| f.2).collect::<Vec<_>>());
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
        let mem = || {
            let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            s.lines().filter(|l| l.starts_with("VmRSS") || l.starts_with("VmHWM")).map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(", ")
        };
        let rss = mem();
        let time = |l: &Library, q: Query| {
            let t = std::time::Instant::now();
            let ids = l.query(&q);
            (ids.len(), t.elapsed())
        };
        println!("{n} games: first rows after {first:?}, all read in {read:?}; {rss}");
        println!("all: {:?}", time(&l, Query::default()));
        println!("player 'carlsen': {:?}", time(&l, Query { player: "carlsen".into(), ..Query::default() }));
        println!("opening 'gambit' + 1-0 + 1900-1950 + elo 2400: {:?}", time(&l, Query { opening: "gambit".into(), result: "1-0".into(), from: Some(1900), to: Some(1950), elo: Some(2400), ..Query::default() }));
        println!("sort by white: {:?}", time(&l, Query { sort: Sort::White, ..Query::default() }));
        println!("sort by date desc: {:?}", time(&l, Query { sort: Sort::Date, desc: true, ..Query::default() }));
        for i in [0, n as u32 / 2, n as u32 - 1, 10].into_iter().filter(|&i| (i as usize) < n) {
            let t = std::time::Instant::now();
            let g = l.game(i).unwrap();
            println!("game {i}: {} moves, {} nodes, error {:?}, in {:?}", g.main_line().len(), g.nodes.len(), g.error, t.elapsed());
        }
        println!("after the queries: {}", mem());
        drop(l);
        let t = std::time::Instant::now();
        let l = Library::open(Path::new(&p)).unwrap();
        let p = l.progress();
        println!("opened again: {} games, kept {}, done {}, in {:?}; first row {:?}", p.games, p.kept, p.done, t.elapsed(), l.row(0).map(|r| r.white));
        println!("sort by white again: {:?}; {}", time(&l, Query { sort: Sort::White, ..Query::default() }), mem());
    }
}

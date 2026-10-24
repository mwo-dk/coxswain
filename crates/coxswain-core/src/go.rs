//! Go games in SGF (docs/previews/go.md), for both apps: a file's games as a list (players,
//! ranks, komi, handicap, result, date, event), each game on a board stepped through move by
//! move, its variations and comments, captures taken off the board as the rules say. Reading
//! only: nothing is played or scored.
//!
//! Any bytes at all are read without a panic, and without recursion: the reader keeps its own
//! stack, so a file nested a million variations deep costs memory (up to `MAX_NODES`), never
//! the stack. A collection is read as it streams by (`chess::lex`), game by game, never whole:
//! the list fills as it goes, and a game is read from where it starts when it is opened.

use crate::chess::lex::{Lexer, Sgf};
use serde::Serialize;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The most of one game read to show it.
pub const MAX_GAME: u64 = 64 << 20;
/// The most games listed of one file.
pub const MAX_GAMES: usize = 100_000;
/// The most nodes (moves and set-up positions) of one game with its variations: a big joseki
/// dictionary has about 100,000.
pub const MAX_NODES: usize = 250_000;
/// The most set-up points (`AB`, `AW`, `AE`) of one game with its variations: a rectangle
/// such as `AB[aa:ss]` is many, and a made-up file can repeat it on every node.
pub const MAX_SETUP: usize = 250_000;
/// The longest text kept of one property (a comment).
pub const MAX_TEXT: usize = 64 << 10;
/// The largest board SGF allows.
pub const MAX_SIZE: u8 = 52;

/// The file endings of SGF files, for the search store.
pub const ENDINGS: [&str; 1] = [".sgf"];

/// Whether the file is an SGF file, by its name.
pub fn is_sgf(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("sgf"))
}

/// A value as text: escapes undone, a soft line break (`\` before it) taken out, UTF-8 or the
/// file's `CA` charset (else Windows-1252). `simple`: line breaks become spaces.
fn text(raw: &[u8], charset: &str, simple: bool) -> String {
    let mut b = Vec::with_capacity(raw.len().min(MAX_TEXT));
    let mut i = 0;
    while i < raw.len() && b.len() < MAX_TEXT {
        match raw[i] {
            b'\\' if i + 1 < raw.len() => {
                i += 1;
                // A soft line break: \ before \n, \r\n, \r or \n\r.
                if matches!(raw[i], b'\n' | b'\r') {
                    let pair = i + 1 < raw.len() && matches!(raw[i + 1], b'\n' | b'\r') && raw[i + 1] != raw[i];
                    i += 1 + pair as usize;
                    continue;
                }
                b.push(raw[i]);
            }
            c => b.push(c),
        }
        i += 1;
    }
    let s = match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => {
            let b = e.into_bytes();
            let enc = encoding_rs::Encoding::for_label(charset.trim().as_bytes()).unwrap_or(encoding_rs::WINDOWS_1252);
            enc.decode_without_bom_handling(&b).0.into_owned()
        }
    };
    if simple { s.split_whitespace().collect::<Vec<_>>().join(" ") } else { s.trim().to_string() }
}

/// A point of the board, from its two letters: `a`…`z` are 0…25, `A`…`Z` 26…51.
fn point(v: &[u8]) -> Option<(u8, u8)> {
    let c = |b: u8| match b {
        b'a'..=b'z' => Some(b - b'a'),
        b'A'..=b'Z' => Some(b - b'A' + 26),
        _ => None,
    };
    let v = v.trim_ascii();
    (v.len() == 2).then_some(())?;
    Some((c(v[0])?, c(v[1])?))
}

/// A game as the list shows it, read from its first node.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Info {
    /// Its place in the file, from 1.
    pub n: u32,
    pub black: String,
    pub white: String,
    pub black_rank: String,
    pub white_rank: String,
    /// `B+R`, `W+3.5`, `0` (a draw) …, as the file has it.
    pub result: String,
    pub date: String,
    pub event: String,
    pub round: String,
    pub place: String,
    /// The game's own name (`GN`).
    pub name: String,
    pub komi: String,
    pub handicap: String,
    pub rules: String,
    /// The board: width and height.
    pub width: u8,
    pub height: u8,
    /// The moves of the main line.
    pub moves: u32,
    #[serde(skip)]
    offset: u64,
    #[serde(skip)]
    len: u64,
    #[serde(skip)]
    charset: String,
}

impl Info {
    fn new() -> Info {
        Info { width: 19, height: 19, ..Info::default() }
    }

    /// A property of the first node.
    fn set(&mut self, name: &str, raw: &[u8]) {
        let v = || text(raw, &self.charset, true);
        match name {
            "PB" => self.black = v(),
            "PW" => self.white = v(),
            "BR" => self.black_rank = v(),
            "WR" => self.white_rank = v(),
            "RE" => self.result = v(),
            "DT" => self.date = v(),
            "EV" => self.event = v(),
            "RO" => self.round = v(),
            "PC" => self.place = v(),
            "GN" => self.name = v(),
            "KM" => self.komi = v(),
            "HA" => self.handicap = v(),
            "RU" => self.rules = v(),
            "SZ" => {
                let s = v();
                let (w, h) = s.split_once(':').unwrap_or((&s, &s));
                let n = |t: &str| t.trim().parse::<u8>().ok().filter(|n| (1..=MAX_SIZE).contains(n));
                if let (Some(w), Some(h)) = (n(w), n(h)) {
                    (self.width, self.height) = (w, h);
                }
            }
            _ => {}
        }
    }

    /// The board's size as written: `19`, or `19×13` for one that is not square.
    pub fn size(&self) -> String {
        if self.width == self.height { self.width.to_string() } else { format!("{}×{}", self.width, self.height) }
    }

    /// The players with their ranks: "Honinbo Shusaku 4d – Gennan Inseki 8d". Black first,
    /// who plays first.
    pub fn players(&self) -> String {
        let one = |name: &str, rank: &str| if rank.is_empty() { name.to_string() } else { format!("{name} {rank}") };
        format!("{} – {}", one(&self.black, &self.black_rank), one(&self.white, &self.white_rank))
    }

    /// The game in one line, as the search store keeps it: players, name, event, round, place,
    /// date and result.
    pub fn line(&self) -> String {
        let players = if self.black.is_empty() && self.white.is_empty() { String::new() } else { self.players() };
        let parts = [&players, &self.name, &self.event, &self.round, &self.place, &self.date, &self.result];
        parts.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
    }
}

/// The games of an SGF text as it streams by, in order: where each is and what its first node
/// says, each handed to `each` until it says to stop. One pass, no game read in full; past the
/// first node only the moves' names are read, not their values. True when it stopped at
/// `MAX_GAMES`.
fn scan_from(r: impl BufRead, stop: &AtomicBool, mut each: impl FnMut(Info) -> bool) -> io::Result<bool> {
    let mut lex = Lexer::new(r);
    let mut depth = 0usize;
    let mut info = Info::new();
    let mut listed = 0usize;
    // Nodes of the game so far; on its main line still (the first variation of each branch).
    let (mut nodes, mut main) = (0usize, true);
    let mut root: Vec<(String, Vec<u8>)> = vec![];
    let finish_root = |info: &mut Info, root: &mut Vec<(String, Vec<u8>)>| {
        // The charset first: the names are read with it.
        if let Some((_, v)) = root.iter().find(|(k, _)| k == "CA") {
            info.charset = text(v, "", true);
        }
        for (k, v) in root.drain(..) {
            info.set(&k, &v);
        }
    };
    while let Some((at, tok)) = lex.sgf(depth > 0 && nodes == 1)? {
        match tok {
            Sgf::Open if depth == 0 => {
                if stop.load(Ordering::Relaxed) {
                    return Ok(false);
                }
                depth = 1;
                info = Info::new();
                info.offset = at;
                (nodes, main) = (0, true);
            }
            Sgf::Open => depth += 1,
            Sgf::Close if depth == 0 => {}
            Sgf::Close => {
                depth -= 1;
                if nodes == 1 {
                    finish_root(&mut info, &mut root);
                }
                if depth == 0 {
                    info.len = at + 1 - info.offset;
                    if nodes > 0 {
                        listed += 1;
                        info.n = listed as u32;
                        if !each(std::mem::replace(&mut info, Info::new())) {
                            return Ok(false);
                        }
                        if listed >= MAX_GAMES {
                            return Ok(true);
                        }
                    }
                } else {
                    main = false;
                }
            }
            Sgf::Node if depth > 0 => {
                if nodes == 1 {
                    finish_root(&mut info, &mut root);
                }
                nodes += 1;
            }
            Sgf::Node => {}
            Sgf::Prop(name, mut values) if depth > 0 => {
                // The facts of the game: one value each, and at most so many.
                if nodes == 1 && root.len() < 256 {
                    if !values.is_empty() {
                        root.push((name.clone(), values.swap_remove(0)));
                    }
                } else if main && (name == "B" || name == "W") {
                    info.moves += 1;
                }
                if nodes == 1 && (name == "B" || name == "W") {
                    info.moves += 1;
                }
            }
            Sgf::Prop(..) => {}
        }
    }
    // A game never closed: to the end of the file.
    if depth > 0 && nodes > 0 {
        if nodes == 1 {
            finish_root(&mut info, &mut root);
        }
        info.len = lex.at - info.offset;
        info.n = listed as u32 + 1;
        each(info);
    }
    Ok(false)
}

/// The games of an SGF text in memory (`scan_from`), and whether there were more.
fn scan(b: &[u8]) -> (Vec<Info>, bool) {
    let mut out = vec![];
    let capped = scan_from(b, &AtomicBool::new(false), |i| {
        out.push(i);
        true
    })
    .unwrap_or(false);
    (out, capped)
}

/// A move: whose, and where (`None`: a pass).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Move {
    pub black: bool,
    pub at: Option<(u8, u8)>,
}

/// A node of the game: a move, or a position set up (`AB`, `AW`, `AE`), with its comment.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Node {
    pub parent: Option<u32>,
    /// The nodes after it: the first continues its line, the others are variations.
    pub children: Vec<u32>,
    pub mv: Option<Move>,
    /// Stones put on the board (1 black, 2 white) or taken off (0), before the move. Kept for
    /// `Game::position`, not sent to the desktop app.
    #[serde(skip)]
    pub setup: Vec<(u8, u8, u8)>,
    pub comment: String,
    /// The node's own name (`N`).
    pub name: String,
    /// The moves played up to and with this one.
    pub number: u32,
    /// 0 on the main line, 1 in a variation, 2 in a variation of one, …
    pub depth: u32,
    /// Whose turn it is said to be (`PL`): true for Black.
    #[serde(skip)]
    player: Option<bool>,
}

/// A game read in full.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Game {
    pub info: Info,
    /// The first node is the game's start: node 0.
    pub nodes: Vec<Node>,
    /// Stopped at `MAX_NODES` or `MAX_SETUP`: the rest of the game is not read.
    pub capped: bool,
}

/// The game in `b` (one game tree), read in full.
pub fn parse(b: &[u8]) -> Game {
    let (mut infos, _) = scan(b);
    let info = if infos.is_empty() { Info::new() } else { infos.swap_remove(0) };
    let mut g = Game { nodes: vec![], capped: false, info };
    let (w, h) = (g.info.width, g.info.height);
    let mut cur: Option<u32> = None;
    let mut stack: Vec<Option<u32>> = vec![];
    let mut started = false;
    let mut setup = 0usize;
    let mut lex = Lexer::new(b);
    while let Ok(Some((_, tok))) = lex.sgf(true) {
        match tok {
            Sgf::Open => {
                // A second game tree in the same bytes: not this game's.
                if started && stack.is_empty() {
                    break;
                }
                started = true;
                stack.push(cur);
            }
            Sgf::Close => {
                cur = stack.pop().flatten();
                if stack.is_empty() {
                    break;
                }
            }
            Sgf::Node => {
                if g.nodes.len() >= MAX_NODES {
                    g.capped = true;
                    break;
                }
                let id = g.nodes.len() as u32;
                // A node outside any tree, or a second start: under the first.
                let parent = cur.or(if g.nodes.is_empty() { None } else { Some(0) });
                let (number, depth) = parent.map_or((0, 0), |p| {
                    let p = &g.nodes[p as usize];
                    (p.number, p.depth + (!p.children.is_empty()) as u32)
                });
                g.nodes.push(Node { parent, number, depth, ..Node::default() });
                if let Some(p) = parent {
                    g.nodes[p as usize].children.push(id);
                }
                cur = Some(id);
            }
            Sgf::Prop(name, values) => {
                let Some(at) = cur else { continue };
                let n = &mut g.nodes[at as usize];
                match name.as_str() {
                    "B" | "W" if n.mv.is_none() => {
                        let p = point(&values[0]).filter(|&(x, y)| x < w && y < h);
                        n.mv = Some(Move { black: name == "B", at: p });
                        n.number += 1;
                    }
                    "AB" | "AW" | "AE" => {
                        let colour = match name.as_str() {
                            "AB" => 1,
                            "AW" => 2,
                            _ => 0,
                        };
                        for v in &values {
                            // One point, or a rectangle of them: `aa:cc`.
                            let (a, z) = match v.iter().position(|&c| c == b':') {
                                Some(i) => (point(&v[..i]), point(&v[i + 1..])),
                                None => (point(v), point(v)),
                            };
                            let (Some(a), Some(z)) = (a, z) else { continue };
                            for x in a.0.min(z.0)..=a.0.max(z.0).min(w.saturating_sub(1)) {
                                for y in a.1.min(z.1)..=a.1.max(z.1).min(h.saturating_sub(1)) {
                                    if setup >= MAX_SETUP {
                                        g.capped = true;
                                    } else if n.setup.len() < w as usize * h as usize {
                                        n.setup.push((colour, x, y));
                                        setup += 1;
                                    }
                                }
                            }
                        }
                    }
                    "C" => n.comment = text(&values[0], &g.info.charset, false),
                    "N" => n.name = text(&values[0], &g.info.charset, true),
                    "PL" => n.player = Some(matches!(values[0].trim_ascii().first(), Some(b'B' | b'b' | b'1'))),
                    "MN" => {
                        if let Ok(m) = text(&values[0], "", true).parse::<u32>() {
                            n.number = m;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if g.nodes.is_empty() {
        g.nodes.push(Node::default());
    }
    g
}

/// The board after a node.
#[derive(Clone, Debug, Serialize)]
pub struct Position {
    pub width: u8,
    pub height: u8,
    /// Row by row from the top: 0 empty, 1 black, 2 white.
    pub stones: Vec<u8>,
    /// Stones taken: by Black, by White.
    pub captures: [u32; 2],
    /// The point of the node's move, for its mark.
    pub last: Option<(u8, u8)>,
    /// Black plays next.
    pub black_next: bool,
    /// The moves played to here.
    pub number: u32,
}

impl Position {
    pub fn at(&self, x: u8, y: u8) -> u8 {
        self.stones[y as usize * self.width as usize + x as usize]
    }
}

impl Game {
    /// The board after node `at`: every node from the start to it played out.
    pub fn position(&self, at: u32) -> Position {
        let (w, h) = (self.info.width, self.info.height);
        let mut p = Position { width: w, height: h, stones: vec![0; w as usize * h as usize], captures: [0, 0], last: None, black_next: true, number: 0 };
        let mut path = vec![];
        let mut n = Some(at.min(self.nodes.len().saturating_sub(1) as u32));
        while let Some(i) = n {
            path.push(i);
            n = self.nodes[i as usize].parent;
        }
        let mut seen = vec![false; p.stones.len()];
        // Points looked at for captures: a crafted game of huge groups costs at most this
        // much (about 50 ms) per board; past it, stones are only put down.
        let mut budget: u64 = 20_000_000;
        let handicap = self.info.handicap.trim().parse::<u32>().unwrap_or(0);
        for &i in path.iter().rev() {
            let node = &self.nodes[i as usize];
            p.last = None;
            for &(c, x, y) in &node.setup {
                p.stones[y as usize * w as usize + x as usize] = c;
            }
            // Handicap stones set up: White plays next.
            if node.parent.is_none() && handicap > 1 && node.setup.iter().any(|s| s.0 == 1) {
                p.black_next = false;
            }
            if let Some(m) = node.mv {
                if let Some((x, y)) = m.at {
                    play(&mut p, &mut seen, &mut budget, m.black, x, y);
                    p.last = Some((x, y));
                }
                p.black_next = !m.black;
            }
            if let Some(b) = node.player {
                p.black_next = b;
            }
            p.number = node.number;
        }
        p
    }

    /// The moves that go on from node `at` when there is more than one: each with its letter's
    /// place (0 = A) and its node.
    pub fn branches(&self, at: u32) -> Vec<u32> {
        let c = &self.nodes[at as usize].children;
        if c.len() > 1 { c.clone() } else { vec![] }
    }

    /// The last node of the line from `at`, by first children.
    pub fn end_of_line(&self, at: u32) -> u32 {
        let mut n = at;
        while let Some(&c) = self.nodes[n as usize].children.first() {
            n = c;
        }
        n
    }

    /// The nodes that could be instead of `at`: its parent's children (itself among them).
    pub fn siblings(&self, at: u32) -> Vec<u32> {
        match self.nodes[at as usize].parent {
            Some(p) => self.nodes[p as usize].children.clone(),
            None => vec![at],
        }
    }

    /// Where a point is, as Go players write it: columns A to T without I, rows from the
    /// bottom (`Q16`). Boards wider than 25 columns: column and row numbers (`27-3`).
    pub fn point_name(&self, x: u8, y: u8) -> String {
        point_name(self.info.width, self.info.height, x, y)
    }

    /// Node `at`'s move as shown: "37 ● Q16", "38 ○ pass"; empty for a node without one.
    pub fn move_text(&self, at: u32) -> String {
        let n = &self.nodes[at as usize];
        let Some(m) = n.mv else { return String::new() };
        let stone = if m.black { "●" } else { "○" };
        let at = m.at.map_or_else(|| crate::t!("go.pass"), |(x, y)| self.point_name(x, y));
        format!("{} {stone} {at}", n.number)
    }
}

/// `Game::point_name` for a board of `w` by `h`.
pub fn point_name(w: u8, h: u8, x: u8, y: u8) -> String {
    const LETTERS: &[u8] = b"ABCDEFGHJKLMNOPQRSTUVWXYZ";
    let row = h as u32 - y as u32;
    if (w as usize) <= LETTERS.len() { format!("{}{row}", LETTERS[x as usize] as char) } else { format!("{}-{row}", x as u32 + 1) }
}

/// The star points of a square board (the dots drawn on it): none on a small or odd-shaped one.
pub fn stars(w: u8, h: u8) -> Vec<(u8, u8)> {
    if w != h || w < 7 {
        return vec![];
    }
    let edge = if w >= 13 { 3 } else { 2 };
    let mut lines = vec![edge, w - 1 - edge];
    if w % 2 == 1 && (w >= 13 || w == 9) {
        lines.insert(1, w / 2);
    }
    lines.iter().flat_map(|&x| lines.iter().map(move |&y| (x, y))).collect()
}

/// A stone of Black (else White) played at `x`, `y`: the other side's groups without a
/// liberty are taken off, then its own (a suicide, which some rules allow).
fn play(p: &mut Position, seen: &mut [bool], budget: &mut u64, black: bool, x: u8, y: u8) {
    let (w, h) = (p.width as usize, p.height as usize);
    let me = if black { 1 } else { 2 };
    let at = y as usize * w + x as usize;
    p.stones[at] = me;
    let neighbours = |i: usize| {
        let (x, y) = (i % w, i / w);
        [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)].into_iter().flatten()
    };
    // A group without liberties, taken off: how many stones.
    let take = |p: &mut Position, seen: &mut [bool], budget: &mut u64, from: usize| -> u32 {
        let colour = p.stones[from];
        let mut group = vec![from];
        let mut stack = vec![from];
        seen[from] = true;
        let mut free = false;
        // Done at its first liberty; out of budget counts as one.
        while !free && let Some(i) = stack.pop() {
            if *budget == 0 {
                free = true;
                break;
            }
            *budget -= 1;
            for n in neighbours(i) {
                match p.stones[n] {
                    0 => free = true,
                    c if c == colour && !seen[n] => {
                        seen[n] = true;
                        group.push(n);
                        stack.push(n);
                    }
                    _ => {}
                }
            }
        }
        for &i in &group {
            seen[i] = false;
        }
        if free {
            return 0;
        }
        for &i in &group {
            p.stones[i] = 0;
        }
        group.len() as u32
    };
    let mut taken = 0;
    for n in neighbours(at) {
        if p.stones[n] == 3 - me {
            taken += take(p, seen, budget, n);
        }
    }
    p.captures[(!black) as usize] += taken;
    if taken == 0 {
        let lost = take(p, seen, budget, at);
        p.captures[black as usize] += lost;
    }
}

/// An SGF file's games: their list, read on a thread of its own and filling as it goes, each
/// game read in full when it is opened.
pub struct Collection {
    pub path: PathBuf,
    listed: Arc<Mutex<Listed>>,
    read: Arc<AtomicU64>,
    size: u64,
    stop: Arc<AtomicBool>,
}

#[derive(Default)]
struct Listed {
    games: Vec<Info>,
    done: bool,
    /// Stopped at `MAX_GAMES`: only the first games are listed.
    capped: bool,
    error: Option<String>,
}

impl Drop for Collection {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Collection {
    /// Starts listing the file's games.
    pub fn open(path: &Path) -> io::Result<Collection> {
        let size = std::fs::metadata(path)?.len();
        let read = Arc::new(AtomicU64::new(0));
        let r = crate::chess::games::reader(path, read.clone(), u64::MAX)?;
        let listed = Arc::new(Mutex::new(Listed::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (l, st) = (listed.clone(), stop.clone());
        std::thread::Builder::new().name("go-games".into()).spawn(move || {
            let lock = || l.lock().unwrap_or_else(|e| e.into_inner());
            let (mut batch, mut last, mut n) = (vec![], std::time::Instant::now(), 0usize);
            let res = scan_from(BufReader::with_capacity(1 << 16, r), &st, |info| {
                batch.push(info);
                n += 1;
                // The first games at once, then a batch every so often.
                if n <= 2 || last.elapsed().as_millis() >= 50 {
                    lock().games.append(&mut batch);
                    last = std::time::Instant::now();
                }
                true
            });
            let mut l = lock();
            l.games.append(&mut batch);
            l.done = true;
            match res {
                Ok(capped) => l.capped = capped,
                Err(e) => l.error = Some(e.to_string()),
            }
        })?;
        Ok(Collection { path: path.to_path_buf(), listed, read, size, stop })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Listed> {
        self.listed.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How far the list has come.
    pub fn progress(&self) -> crate::chess::Progress {
        let l = self.lock();
        crate::chess::Progress { games: l.games.len(), done: l.done, read: self.read.load(Ordering::Relaxed).min(self.size), size: self.size, capped: l.capped, error: l.error.clone() }
    }

    /// Up to `n` of the games listed so far, from the `from`th (from 0).
    pub fn games(&self, from: usize, n: usize) -> Vec<Info> {
        self.lock().games.iter().skip(from).take(n).cloned().collect()
    }

    /// Game `i` of the list (from 0), once listed.
    pub fn info(&self, i: usize) -> Option<Info> {
        self.lock().games.get(i).cloned()
    }

    /// Waits until `n` games are listed or the list is done.
    pub fn wait(&self, n: usize) {
        loop {
            let l = self.lock();
            if l.games.len() >= n || l.done {
                return;
            }
            drop(l);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Game `i` (from 0), read now; waits for the list to reach it.
    pub fn game(&self, i: usize) -> io::Result<Game> {
        self.wait(i + 1);
        let info = self.info(i).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such game"))?;
        let mut f = std::fs::File::open(&self.path)?;
        f.seek(SeekFrom::Start(info.offset))?;
        let mut b = vec![];
        f.take(info.len.min(MAX_GAME)).read_to_end(&mut b)?;
        let mut g = parse(&b);
        g.info.n = info.n;
        Ok(g)
    }
}

/// The games of the SGF file at `path`, as it streams by (the search store, and Find's hits):
/// 1 GiB of it at most.
fn scan_file(path: &Path, each: impl FnMut(Info) -> bool) -> io::Result<()> {
    let r = crate::chess::games::reader(path, Arc::default(), 1 << 30)?;
    scan_from(BufReader::with_capacity(1 << 16, r.take(1 << 30)), &AtomicBool::new(false), each).map(|_| ())
}

/// The search store's text of an SGF file: a line per game (`Info::line`), up to `max` bytes.
pub fn search_text(path: &Path, max: usize) -> Option<String> {
    let mut out = String::new();
    scan_file(path, |g| {
        let line = g.line();
        if out.len() + line.len() + 1 > max {
            return false;
        }
        out.push_str(&line);
        out.push('\n');
        true
    })
    .ok()?;
    (!out.trim().is_empty()).then_some(out)
}

/// The game a Find hit in an SGF file is about, by its snippet: its index, from 0
/// (`chess::games::Hit`).
pub fn game_of_hit(path: &Path, snippet: &str) -> Option<u32> {
    let mut hit = crate::chess::games::Hit::new(snippet)?;
    scan_file(path, |g| hit.feed(&g.line())).ok()?;
    hit.found()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The FF[4] example's first game, in part, and a game with handicap stones.
    const TWO: &str = "(;FF[4]GM[1]SZ[19]CA[UTF-8]PB[Honinbo Shusaku]BR[4d]PW[Gennan Inseki]WR[8d]RE[B+2]DT[1846]KM[0]\n;B[qd];W[dc];B[pq]C[A \\] bracket\\\n joined]\n(;W[oc];B[cp])\n(;W[dp]N[Other]))\n(;SZ[9]HA[2]AB[cc][gg]PB[Kim]PW[Lee];W[ee];B[tt])";

    #[test]
    fn a_list_of_games() {
        let (games, capped) = scan(TWO.as_bytes());
        assert!(!capped);
        assert_eq!(games.len(), 2);
        let g = &games[0];
        assert_eq!((g.black.as_str(), g.black_rank.as_str(), g.white.as_str(), g.result.as_str(), g.moves), ("Honinbo Shusaku", "4d", "Gennan Inseki", "B+2", 5));
        assert_eq!(g.line(), "Honinbo Shusaku 4d – Gennan Inseki 8d · 1846 · B+2");
        assert_eq!((games[1].width, games[1].handicap.as_str(), games[1].moves), (9, "2", 2));
    }

    #[test]
    fn a_game_played_out() {
        let g = parse(TWO.as_bytes());
        // Root, three moves, then two lines of two and one.
        assert_eq!(g.nodes.len(), 7);
        assert_eq!(g.nodes[3].comment, "A ] bracket joined");
        assert_eq!(g.branches(3).len(), 2);
        assert_eq!(g.move_text(1), "1 ● R16");
        let end = g.end_of_line(0);
        assert_eq!(g.nodes[end as usize].number, 5);
        assert_eq!(g.nodes[6].depth, 1);
        assert_eq!(g.nodes[6].name, "Other");
        let p = g.position(end);
        assert_eq!(p.at(2, 15), 1, "B C4 (cp)");
        assert_eq!(p.last, Some((2, 15)));
        assert!(!p.black_next);
        // The second game: handicap stones, White first; a pass on a small board.
        let text = TWO.as_bytes();
        let start = TWO.rfind("(;SZ[9]").unwrap();
        let g = parse(&text[start..]);
        assert_eq!(g.info.width, 9);
        assert!(!g.position(0).black_next);
        assert_eq!(g.position(2).last, None);
        assert_eq!(g.nodes[2].mv, Some(Move { black: true, at: None }));
    }

    #[test]
    fn captures_and_suicide() {
        // White's stone at b1 surrounded: a1 and c1 black, Black takes it at b2.
        let g = parse(b"(;SZ[5]AB[ae][ce];W[be];B[bd])");
        let p = g.position(2);
        assert_eq!(p.at(1, 4), 0);
        assert_eq!(p.captures, [1, 0]);
        // A group of two taken in the corner.
        let g = parse(b"(;SZ[5]AW[aa][ba]AB[ca];B[ab];W[ee];B[bb])");
        let p = g.position(3);
        assert_eq!((p.at(0, 0), p.at(1, 0), p.captures), (0, 0, [2, 0]));
        // A suicide: the stone itself goes, to the other side's count.
        let g = parse(b"(;SZ[3]AB[ba][ab];W[aa])");
        let p = g.position(1);
        assert_eq!((p.at(0, 0), p.captures), (0, [1, 0]));
        // A stone without liberties that takes is no suicide: it stays, and the two go.
        let g = parse(b"(;SZ[3]AW[ba][ab]AB[ca][bb][ac];B[aa])");
        let p = g.position(1);
        assert_eq!((p.at(0, 0), p.at(1, 0), p.at(0, 1), p.captures), (1, 0, 0, [2, 0]));
    }

    #[test]
    fn sizes_coordinates_and_stars() {
        assert_eq!(point_name(19, 19, 0, 18), "A1");
        assert_eq!(point_name(19, 19, 8, 0), "J19");
        assert_eq!(point_name(13, 13, 12, 12), "N1");
        assert_eq!(point_name(30, 30, 26, 0), "27-30");
        assert_eq!(stars(19, 19).len(), 9);
        assert_eq!(stars(13, 13).len(), 9);
        assert_eq!(stars(9, 9), vec![(2, 2), (2, 4), (2, 6), (4, 2), (4, 4), (4, 6), (6, 2), (6, 4), (6, 6)]);
        assert!(stars(19, 13).is_empty());
        let (g, _) = scan(b"(;SZ[19:13])(;SZ[99])(;SZ[x])");
        assert_eq!((g[0].width, g[0].height, g[0].size()), (19, 13, "19×13".to_string()));
        assert_eq!((g[1].width, g[2].width), (19, 19));
        // Points off the board are no moves there.
        let g = parse(b"(;SZ[9];B[zz];W[jj])");
        assert_eq!((g.nodes[1].mv.unwrap().at, g.nodes[2].mv.unwrap().at), (None, None));
        let _ = g.position(2);
    }

    #[test]
    fn whatever_the_bytes() {
        for text in [
            &b""[..], b"(", b")", b"(;", b"(;B[", b"(;B[aa", b"(;AB[aa:zz]", b"((((((", b")))(;", b"(;C[\\", b"(;SZ[1];B[aa];W[aa]",
            b"[;]", b"(;GM[1](;B[aa])(;W[bb](;B[cc]", b"\xff\xfe(;PB[\xe9]", b"(;AB[ss:aa]AW[aa:ss]AE[jj:jj])", b"(;MN[x]PL[W]B[])",
        ] {
            let (games, _) = scan(text);
            for i in 0..games.len() {
                let g = parse(&text[games[i].offset as usize..]);
                for n in 0..g.nodes.len() as u32 {
                    let _ = g.position(n);
                    let _ = g.move_text(n);
                }
            }
            let g = parse(text);
            let _ = g.position(0);
        }
        // Latin-1 by its CA, GB2312 too.
        let (g, _) = scan(b"(;CA[ISO-8859-1]PB[Bj\xf6rn])");
        assert_eq!(g[0].black, "Björn");
        let (g, _) = scan(b"(;CA[GB2312]PB[\xb9\xc5\xc1\xa6])");
        assert_eq!(g[0].black, "古力");
    }

    /// A million variations in each other, and a line of a million moves: no stack used, and no
    /// more nodes than `MAX_NODES`.
    #[test]
    fn deep_and_long() {
        let deep = format!("(;SZ[19]{}{})", "(;B[aa]".repeat(1_000_000), ")".repeat(1_000_000));
        let long = format!("(;SZ[19]{})", ";B[aa];W[bb]".repeat(500_000));
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                for t in [deep, long] {
                    let (games, _) = scan(t.as_bytes());
                    assert_eq!(games.len(), 1);
                    let g = parse(t.as_bytes());
                    assert!(g.capped);
                    assert_eq!(g.nodes.len(), MAX_NODES);
                    let end = g.end_of_line(0);
                    let p = g.position(end);
                    assert_eq!(p.number as usize, MAX_NODES - 1);
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    /// Millions of values to one property, and a board of one group played on and on: a
    /// few thousand values kept, each board made in bounded time.
    #[test]
    fn endless_values_and_huge_groups_stay_cheap() {
        // Millions of values to one property: a few thousand kept.
        let t = format!("(;A{}B[ok])", "[]".repeat(1_000_000));
        let (games, _) = scan(t.as_bytes());
        assert_eq!(games.len(), 1);
        let mut lex = Lexer::new(t.as_bytes());
        let n = std::iter::from_fn(|| lex.sgf(true).unwrap()).map(|(_, tok)| match tok {
            Sgf::Prop(_, v) => v.len(),
            _ => 0,
        });
        assert!(n.max().unwrap() <= crate::chess::lex::MAX_VALUES);
        // A board filled with one group, played on again and again: capped in time.
        let t = format!("(;SZ[52]AB[aa:ZY]{})", ";B[aa];W[]".repeat(125_000));
        let g = parse(t.as_bytes());
        let start = std::time::Instant::now();
        let _ = g.position(g.end_of_line(0));
        assert!(start.elapsed() < crate::test_limit(std::time::Duration::from_secs(3)));
    }

    /// Big set-up rectangles on every node: no more than `MAX_SETUP` points kept, fast.
    #[test]
    fn set_up_rectangles_are_capped() {
        let t = format!("(;SZ[52]{})", ";AB[aa:ZZ]AW[aa:ZZ]".repeat(10_000));
        let start = std::time::Instant::now();
        let g = parse(t.as_bytes());
        assert!(g.capped);
        assert!(g.nodes.iter().map(|n| n.setup.len()).sum::<usize>() <= MAX_SETUP);
        let _ = g.position(g.end_of_line(0));
        assert!(start.elapsed() < crate::test_limit(std::time::Duration::from_secs(2)));
    }

    #[test]
    fn files_and_hits() {
        let d = std::env::temp_dir().join(format!("coxswain-go-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("two.sgf");
        std::fs::write(&p, TWO).unwrap();
        let c = Collection::open(&p).unwrap();
        c.wait(usize::MAX);
        assert_eq!(c.progress().games, 2);
        assert_eq!(c.game(1).unwrap().info.black, "Kim");
        assert_eq!(c.game(1).unwrap().info.n, 2);
        assert!(c.game(2).is_err());
        let text = search_text(&p, 1 << 20).unwrap();
        assert_eq!(text.lines().count(), 2);
        let (o, cl) = crate::store::MARK;
        assert_eq!(game_of_hit(&p, &format!("{o}Kim{cl} – Lee")), Some(1));
        assert_eq!(game_of_hit(&p, &format!("…{o}Gennan{cl} Inseki 8d")), Some(0));
        assert!(crate::extract::text_of(&p, TWO.len() as u64, 1 << 20).is_some_and(|t| t.contains("Shusaku")));
        std::fs::remove_dir_all(&d).unwrap();
    }
}

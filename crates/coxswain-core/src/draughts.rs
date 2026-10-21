//! Draughts in PDN (docs/previews/draughts.md), for both apps: a PDN file's games go through
//! the chess table (`chess::Library`), each game played out here on its board. International
//! draughts (10×10, `GameType 20`, the default) and English draughts or checkers (8×8,
//! `GameType 21`) by their rules: men capture backwards in international draughts, kings fly
//! there and move one square in English draughts, a capture of several pieces goes on from
//! square to square. Reading only: nothing is played or checked beyond what showing it needs.
//!
//! Any text at all is read without a panic and without recursion; a capture is looked for
//! along at most `MAX_SEARCH` paths.

use crate::chess::pgn::{self, Game, Rules, Word};

/// The file endings of PDN files, for the search store.
pub const ENDINGS: [&str; 1] = [".pdn"];
/// The most capture paths looked at for one move: a king among many pieces can have a great
/// many, a real move a handful.
pub const MAX_SEARCH: usize = 20_000;
/// The largest board read (Canadian draughts is 12×12).
pub const MAX_SIZE: u8 = 12;

/// Which draughts: the board and the rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variant {
    /// The PDN `GameType` number: 20 international, 21 English, …
    pub kind: u8,
    pub width: u8,
    pub height: u8,
    /// Black moves first (English draughts); else White.
    pub black_first: bool,
    /// Men capture backwards too.
    pub men_back: bool,
    /// Kings move and capture any distance along a diagonal.
    pub flying: bool,
}

const INTERNATIONAL: Variant = Variant { kind: 20, width: 10, height: 10, black_first: false, men_back: true, flying: true };
const ENGLISH: Variant = Variant { kind: 21, width: 8, height: 8, black_first: true, men_back: false, flying: false };

impl Variant {
    /// The variant of a `GameType` tag: `20`, or in full `20,W,10,10,N2,0` (type, who starts,
    /// width, height, notation, inverted).
    fn by_type(tag: &str) -> Option<Variant> {
        let f: Vec<&str> = tag.split(',').map(str::trim).collect();
        let kind: u8 = f.first()?.parse().ok()?;
        let mut v = match kind {
            20 => INTERNATIONAL,
            21 => ENGLISH,
            // Russian, Brazilian, Pool: flying kings, men capturing backwards, on 8×8.
            23 | 25 | 26 => Variant { kind, width: 8, height: 8, black_first: kind == 23, ..INTERNATIONAL },
            27 => Variant { kind, width: 12, height: 12, ..INTERNATIONAL },
            // Italian, Spanish, Portuguese, Czech and the rest: 8×8, men forward only.
            _ => Variant { kind, black_first: false, ..ENGLISH },
        };
        if let Some(c) = f.get(1) {
            match c.to_ascii_uppercase().as_str() {
                "W" => v.black_first = false,
                "B" => v.black_first = true,
                _ => {}
            }
        }
        let size = |i: usize| f.get(i).and_then(|s| s.parse::<u8>().ok()).filter(|n| (4..=MAX_SIZE).contains(n) && n % 2 == 0);
        if let (Some(w), Some(h)) = (size(2), size(3)) {
            (v.width, v.height) = (w, h);
        }
        Some(v)
    }

    /// The variant of a game: its `GameType` tag; without one, English draughts when the first
    /// move starts at a square up to 20 (Black's men in English draughts) or a FEN tag has no
    /// square past 32, else international draughts, PDN's default.
    pub fn of(tags: &[(String, String)], first_move: Option<&str>) -> Variant {
        let tag = |k: &str| tags.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str());
        if let Some(v) = tag("GameType").and_then(Variant::by_type) {
            return v;
        }
        if let Some(fen) = tag("FEN") {
            let big = fen.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse::<u32>().ok()).any(|n| n > 32);
            return if big { INTERNATIONAL } else { ENGLISH };
        }
        let from = first_move.and_then(|m| m.split(['-', 'x', 'X', ':']).next()).and_then(|s| s.parse::<u32>().ok());
        if from.is_some_and(|n| n <= 20) { ENGLISH } else { INTERNATIONAL }
    }

    /// The variant's name, as the board's heading says it.
    pub fn name(&self) -> String {
        let size = format!("{}×{}", self.width, self.height);
        match self.kind {
            20 if self.width == 10 => crate::t!("draughts.international", "size" => size),
            21 if self.width == 8 => crate::t!("draughts.english", "size" => size),
            n => crate::t!("draughts.other", "n" => n, "size" => size),
        }
    }

    /// The dark squares, numbered 1 … from the top left, row by row.
    fn squares(&self) -> u32 {
        self.width as u32 * self.height as u32 / 2
    }

    /// Square `n`'s cell (`x + y × width`, rows from the top); the top row's dark squares are
    /// its 2nd, 4th, …, the next row's its 1st, 3rd, …
    fn cell(&self, n: u32) -> Option<usize> {
        if n == 0 || n > self.squares() {
            return None;
        }
        let half = self.width as u32 / 2;
        let (y, k) = ((n - 1) / half, (n - 1) % half);
        let x = 2 * k + if y % 2 == 0 { 1 } else { 0 };
        Some((x + y * self.width as u32) as usize)
    }

    /// A square as written: its number (`32`), or its column and row (`c3`, row 1 at the
    /// bottom), on a dark square.
    fn parse_square(&self, s: &str) -> Option<usize> {
        let s = s.trim();
        if s.is_empty() || s.len() > 3 {
            return None;
        }
        if s.bytes().all(|b| b.is_ascii_digit()) {
            return self.cell(s.parse().ok()?);
        }
        let b = s.as_bytes();
        let x = b[0].to_ascii_lowercase().checked_sub(b'a')? as u32;
        let row: u32 = s[1..].parse().ok()?;
        let (w, h) = (self.width as u32, self.height as u32);
        if x >= w || row == 0 || row > h {
            return None;
        }
        let y = h - row;
        ((x + y) % 2 == 1).then_some((x + y * w) as usize)
    }
}

/// A piece on a cell: 0 none, then White's man and king, Black's man and king.
const WM: u8 = 1;
const WK: u8 = 2;
const BM: u8 = 3;
const BK: u8 = 4;

fn white(p: u8) -> bool {
    p == WM || p == WK
}

fn king(p: u8) -> bool {
    p == WK || p == BK
}

/// A position: the pieces, whose move, and the move number.
#[derive(Clone, Debug)]
pub(crate) struct Board {
    v: Variant,
    cells: Vec<u8>,
    white: bool,
    number: u32,
}

impl Board {
    /// The start: Black's men on the top rows, White's on the bottom ones, two rows between.
    fn start(v: Variant) -> Board {
        let mut b = Board { v, cells: vec![0; v.width as usize * v.height as usize], white: !v.black_first, number: 1 };
        let rows = v.height as u32 / 2 - 1;
        let half = v.width as u32 / 2;
        for n in 1..=v.squares() {
            let y = (n - 1) / half;
            if let Some(c) = v.cell(n) {
                if y < rows {
                    b.cells[c] = BM;
                } else if y >= v.height as u32 - rows {
                    b.cells[c] = WM;
                }
            }
        }
        b
    }

    /// A PDN FEN: `W:W31,32,K46:B1-5,K10`: who moves, then each side's squares, a king's with
    /// `K`, runs as `1-5`. None when it names no square.
    fn from_fen(v: Variant, fen: &str) -> Option<Board> {
        let mut b = Board { v, cells: vec![0; v.width as usize * v.height as usize], white: !v.black_first, number: 1 };
        let mut any = false;
        let fen = fen.trim().trim_end_matches('.');
        for (i, part) in fen.split(':').enumerate() {
            let part = part.trim();
            let side = part.chars().next().map(|c| c.to_ascii_uppercase());
            if i == 0 && part.len() == 1 {
                b.white = side != Some('B');
                continue;
            }
            let (man, kg) = match side {
                Some('W') => (WM, WK),
                Some('B') => (BM, BK),
                _ => continue,
            };
            for item in part[1..].split(',').take(b.cells.len()) {
                let item = item.trim();
                let (k, sq) = match item.strip_prefix(['K', 'k']) {
                    Some(s) => (true, s),
                    None => (false, item),
                };
                let (a, z) = sq.split_once('-').unwrap_or((sq, sq));
                let (Ok(a), Ok(z)) = (a.trim().parse::<u32>(), z.trim().parse::<u32>()) else {
                    if let Some(c) = v.parse_square(sq) {
                        b.cells[c] = if k { kg } else { man };
                        any = true;
                    }
                    continue;
                };
                for n in a..=z.min(a.saturating_add(v.squares())) {
                    if let Some(c) = v.cell(n) {
                        b.cells[c] = if k { kg } else { man };
                        any = true;
                    }
                }
            }
        }
        any.then_some(b)
    }

    /// The position as the apps draw it: rows from the top, `/` between them, a run of empty
    /// cells as its count, `M` and `K` White's man and king, `m` and `k` Black's; then `w` or
    /// `b` for whose move.
    pub(crate) fn fen(&self) -> String {
        let w = self.v.width as usize;
        let mut out = String::new();
        for (y, row) in self.cells.chunks(w).enumerate() {
            if y > 0 {
                out.push('/');
            }
            let mut empty = 0;
            for &p in row {
                if p == 0 {
                    empty += 1;
                    continue;
                }
                if empty > 0 {
                    out.push_str(&empty.to_string());
                    empty = 0;
                }
                out.push(match p {
                    WM => 'M',
                    WK => 'K',
                    BM => 'm',
                    _ => 'k',
                });
            }
            if empty > 0 {
                out.push_str(&empty.to_string());
            }
        }
        out.push_str(if self.white { " w" } else { " b" });
        out
    }

    fn xy(&self, c: usize) -> (i32, i32) {
        let w = self.v.width as usize;
        ((c % w) as i32, (c / w) as i32)
    }

    fn at(&self, x: i32, y: i32) -> Option<usize> {
        let (w, h) = (self.v.width as i32, self.v.height as i32);
        (x >= 0 && y >= 0 && x < w && y < h).then(|| (x + y * w) as usize)
    }

    /// The row a side's men become kings on.
    fn crowned(&self, c: usize, white_side: bool) -> bool {
        let y = self.xy(c).1;
        if white_side { y == 0 } else { y == self.v.height as i32 - 1 }
    }

    /// Every capture the piece on `from` can make, each as the squares it lands on (from
    /// first) and the pieces it takes. At most `MAX_SEARCH` paths are looked at.
    fn captures(&self, from: usize) -> Vec<(Vec<usize>, Vec<usize>)> {
        let piece = self.cells[from];
        let me = white(piece);
        let mut out = vec![];
        // (where it is, still a man, landed on, taken)
        let mut stack = vec![(from, !king(piece), vec![from], vec![])];
        let mut looked = 0;
        while let Some((at, man, path, taken)) = stack.pop() {
            looked += 1;
            if looked > MAX_SEARCH {
                break;
            }
            let free = |c: usize| self.cells[c] == 0 || c == from;
            let foe = |c: usize| self.cells[c] != 0 && c != from && white(self.cells[c]) != me && !taken.contains(&c);
            let (x, y) = self.xy(at);
            let forward = if me { -1 } else { 1 };
            let mut next = vec![];
            for (dx, dy) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
                if man && !self.v.men_back && dy != forward {
                    continue;
                }
                let long = !man && self.v.flying;
                // Along the diagonal to the first piece (one step only for a man or short king).
                let mut k = 1;
                let mut hit = None;
                while let Some(c) = self.at(x + dx * k, y + dy * k) {
                    if !free(c) {
                        hit = Some((c, k));
                        break;
                    }
                    if !long {
                        break;
                    }
                    k += 1;
                }
                let Some((c, k)) = hit.filter(|&(c, _)| foe(c)) else { continue };
                // The squares beyond it: one, or every free one for a flying king.
                let mut j = k + 1;
                while let Some(land) = self.at(x + dx * j, y + dy * j) {
                    if !free(land) {
                        break;
                    }
                    next.push((land, c));
                    if !long {
                        break;
                    }
                    j += 1;
                }
            }
            if next.is_empty() {
                if !taken.is_empty() {
                    out.push((path, taken));
                }
                continue;
            }
            for (land, c) in next {
                let mut path = path.clone();
                path.push(land);
                let mut taken = taken.clone();
                taken.push(c);
                let crowned = man && self.crowned(land, me);
                // English draughts: a man that reaches the far row is crowned and stops there.
                // Russian draughts: it is crowned and captures on as a king.
                if crowned && self.v.kind == 21 {
                    out.push((path, taken));
                    continue;
                }
                stack.push((land, man && !(crowned && self.v.kind == 25), path, taken));
            }
        }
        out
    }

    /// `mv` (`32-28`, `28x19x10`, `28x10`, `c3-d4`) played: None when it cannot be.
    fn play(&self, mv: &str) -> Option<(Board, usize, usize)> {
        let capture = mv.contains(['x', 'X', ':']);
        let squares: Vec<usize> = mv.split(['-', 'x', 'X', ':']).map(|s| self.v.parse_square(s)).collect::<Option<_>>()?;
        let (&from, &to) = (squares.first()?, squares.last()?);
        let piece = self.cells[from];
        if squares.len() < 2 || piece == 0 || white(piece) != self.white {
            return None;
        }
        let mut b = self.clone();
        let (fx, fy) = self.xy(from);
        let (tx, ty) = self.xy(to);
        let step = (fx - tx).abs() == 1 && (fy - ty).abs() == 1;
        // A capture by the squares it lands on, the one taking the most when several fit.
        let found = if capture || !step {
            self.captures(from)
                .into_iter()
                .filter(|(path, _)| {
                    path.last() == Some(&to) && {
                        let mut it = path.iter();
                        squares[1..squares.len() - 1].iter().all(|s| it.any(|p| p == s))
                    }
                })
                .max_by_key(|(_, taken)| taken.len())
        } else {
            None
        };
        let mut crowned = false;
        match found {
            Some((path, taken)) => {
                for c in taken {
                    b.cells[c] = 0;
                }
                crowned = self.v.kind == 25 && path.iter().any(|&c| self.crowned(c, self.white));
            }
            // International and English draughts are played by their rules; another variant's
            // capture the rules here do not know takes the pieces of the other side between
            // each square written and the next, when they lie on a diagonal.
            None if capture && !matches!(self.v.kind, 20 | 21) => {
                let mut any = false;
                for w in squares.windows(2) {
                    let ((ax, ay), (zx, zy)) = (self.xy(w[0]), self.xy(w[1]));
                    let (dx, dy) = (zx - ax, zy - ay);
                    if dx.abs() != dy.abs() || dx == 0 {
                        return None;
                    }
                    for k in 1..dx.abs() {
                        let c = self.at(ax + dx.signum() * k, ay + dy.signum() * k)?;
                        if b.cells[c] != 0 && white(b.cells[c]) != self.white {
                            b.cells[c] = 0;
                            any = true;
                        }
                    }
                }
                if !any {
                    return None;
                }
            }
            None if capture => return None,
            None => {}
        }
        if b.cells[to] != 0 && to != from {
            return None;
        }
        b.cells[from] = 0;
        b.cells[to] = if !king(piece) && (crowned || self.crowned(to, self.white)) {
            if self.white { WK } else { BK }
        } else {
            piece
        };
        // The second player's move ends the move number.
        if self.white == self.v.black_first {
            b.number += 1;
        }
        b.white = !self.white;
        Some((b, from, to))
    }
}

/// Is it a result? PDN's (`2-0`, `0-2`, `1-1` in international draughts) or PGN's.
fn is_result(w: &str) -> bool {
    pgn::is_result(w) || matches!(w, "2-0" | "0-2" | "1-1" | "0-0")
}

/// A word of PDN movetext: a move (`32-28`, `19x30`, `28x19x10`, `c3-d4`), a result, or
/// something else; a move number glued to the front (`1.32-28`) is taken off.
fn word(w: &str) -> Word<'_> {
    if is_result(w) {
        return Word::Result(w);
    }
    let mut w = w;
    let digits = w.find(|c: char| !c.is_ascii_digit()).unwrap_or(w.len());
    if digits > 0 && w[digits..].starts_with(['.', '…']) {
        w = w[digits..].trim_start_matches(['.', '…']);
    }
    let body = w.trim_end_matches(['!', '?', '*', '+']);
    let nag = pgn::suffix_nag(w[body.len()..].trim_end_matches(['*', '+']));
    let parts: Vec<&str> = body.split(['-', 'x', 'X', ':']).collect();
    let square = |s: &str| {
        let b = s.as_bytes();
        match b.first() {
            Some(c) if c.is_ascii_digit() => b.len() <= 3 && b.iter().all(u8::is_ascii_digit),
            Some(c) if c.is_ascii_alphabetic() => (2..=3).contains(&b.len()) && b[1..].iter().all(u8::is_ascii_digit),
            _ => false,
        }
    };
    if parts.len() >= 2 && parts.iter().all(|p| square(p)) { Word::Move(body, nag) } else { Word::Other }
}

/// The first move of a game's movetext, outside variations and comments.
fn first_move(moves: &str) -> Option<&str> {
    let mut depth = 0u32;
    for tok in pgn::lex(moves) {
        match tok {
            pgn::Lex::Open => depth += 1,
            pgn::Lex::Close => depth = depth.saturating_sub(1),
            pgn::Lex::Word(w) if depth == 0 => {
                if let Word::Move(m, _) = word(w) {
                    return Some(m);
                }
            }
            _ => {}
        }
    }
    None
}

/// Draughts, as `pgn::parse_with` plays it out.
pub(crate) struct Draughts;

impl Rules for Draughts {
    type Board = Board;

    fn start(&self, tags: &[(String, String)], moves: &str) -> (Board, String) {
        let v = Variant::of(tags, first_move(moves));
        let fen = tags.iter().find(|(k, _)| k.eq_ignore_ascii_case("FEN")).map(|(_, f)| f.as_str());
        let b = fen.and_then(|f| Board::from_fen(v, f)).unwrap_or_else(|| Board::start(v));
        let fen = b.fen();
        (b, fen)
    }

    fn word<'a>(&self, w: &'a str) -> Word<'a> {
        word(w)
    }

    fn turn(&self, b: &Board) -> (bool, u32) {
        (b.white != b.v.black_first, b.number)
    }

    fn play(&self, b: &Board, mv: &str) -> Option<(Board, String, Option<u8>, Option<u8>)> {
        let (nb, from, to) = b.play(mv)?;
        let fen = nb.fen();
        Some((nb, fen, Some(from as u8), Some(to as u8)))
    }
}

/// A PDN game's text, read and played out. `Node::white` is the first player's move (Black's
/// in English draughts); `fen` is `Board::fen`, `from` and `to` cells (`x + y × width`).
pub fn parse(text: &str) -> Game {
    pgn::parse_with(text, &Draughts)
}

/// The variant of a game read by `parse`.
pub fn variant(g: &Game) -> Variant {
    Variant::of(&g.tags, g.roots.first().map(|&r| g.nodes[r as usize].san.as_str()))
}

/// The word `w` of movetext as the table counts it: a move or not.
pub(crate) fn is_move(w: &str) -> bool {
    matches!(word(w), Word::Move(..))
}

/// A board drawn from `Board::fen`: rows from the top, each cell `None` or its piece letter.
pub fn cells(fen: &str) -> Vec<Vec<Option<char>>> {
    let placement = fen.split_whitespace().next().unwrap_or("");
    placement
        .split('/')
        .take(MAX_SIZE as usize)
        .map(|row| {
            let mut out = vec![];
            let mut n = 0usize;
            for c in row.chars() {
                if let Some(d) = c.to_digit(10) {
                    n = n * 10 + d as usize;
                    continue;
                }
                out.extend(std::iter::repeat_n(None, n.min(MAX_SIZE as usize)));
                n = 0;
                out.push(Some(c));
            }
            out.extend(std::iter::repeat_n(None, n.min(MAX_SIZE as usize)));
            out.truncate(MAX_SIZE as usize);
            out
        })
        .collect()
}

/// The number of the dark square at `x`, `y` of a board `width` wide (rows from the top).
pub fn square_number(width: usize, x: usize, y: usize) -> Option<usize> {
    ((x + y) % 2 == 1).then(|| y * (width / 2) + x / 2 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(fen_tag: Option<&str>, game_type: &str, moves: &str) -> Game {
        let mut text = format!("[GameType \"{game_type}\"]\n");
        if let Some(f) = fen_tag {
            text.push_str(&format!("[FEN \"{f}\"]\n"));
        }
        text.push('\n');
        text.push_str(moves);
        parse(&text)
    }

    fn last(g: &Game) -> String {
        let main = g.main_line();
        main.last().map_or(g.start.clone(), |&n| g.nodes[n as usize].fen.clone().unwrap_or_default())
    }

    #[test]
    fn squares_are_numbered() {
        let v = INTERNATIONAL;
        assert_eq!(v.cell(1), Some(1));
        assert_eq!(v.cell(5), Some(9));
        assert_eq!(v.cell(6), Some(10));
        assert_eq!(v.cell(46), Some(90));
        assert_eq!(v.cell(50), Some(98));
        assert_eq!((v.cell(0), v.cell(51)), (None, None));
        assert_eq!(ENGLISH.cell(29), Some(56));
        assert_eq!(ENGLISH.parse_square("a1"), Some(56));
        assert_eq!(ENGLISH.parse_square("b1"), None, "a light square");
        for n in 1..=50 {
            let c = v.cell(n).unwrap();
            assert_eq!(square_number(10, c % 10, c / 10), Some(n as usize));
        }
        let b = Board::start(INTERNATIONAL);
        assert_eq!(b.fen(), "1m1m1m1m1m/m1m1m1m1m1/1m1m1m1m1m/m1m1m1m1m1/10/10/1M1M1M1M1M/M1M1M1M1M1/1M1M1M1M1M/M1M1M1M1M1 w");
        assert_eq!(Board::start(ENGLISH).fen().split(' ').nth(1), Some("b"));
        assert_eq!(cells(&b.fen())[0], [None, Some('m'), None, Some('m'), None, Some('m'), None, Some('m'), None, Some('m')]);
    }

    #[test]
    fn variants_by_tag_and_moves() {
        let t = |k: &str, v: &str| vec![(k.to_string(), v.to_string())];
        assert_eq!(Variant::of(&t("GameType", "21"), None), ENGLISH);
        assert_eq!(Variant::of(&t("GameType", "20,W,10,10,N2,0"), None), INTERNATIONAL);
        let v = Variant::of(&t("GameType", "25,W,8,8,A0,0"), None);
        assert_eq!((v.width, v.flying, v.men_back), (8, true, true));
        // A size past the largest, or odd, is not taken.
        assert_eq!(Variant::of(&t("GameType", "20,W,99,99"), None).width, 10);
        assert_eq!(Variant::of(&[], Some("11-15")), ENGLISH);
        assert_eq!(Variant::of(&[], Some("32-28")), INTERNATIONAL);
        assert_eq!(Variant::of(&[], None), INTERNATIONAL);
        assert_eq!(Variant::of(&t("FEN", "W:W21,22:B1,2"), None), ENGLISH);
    }

    #[test]
    fn an_international_game() {
        let g = parse("[White \"A\"]\n[Black \"B\"]\n[Result \"2-0\"]\n\n1. 32-28 19-23 2. 28x19 14x23 {back} 3. 37-32 (3. 33-28 23x32 4. 38x27) 3... 10-14 2-0\n");
        assert_eq!(g.error, None, "{:?}", g.nodes);
        assert_eq!(g.result, "2-0");
        let main = g.main_line();
        assert_eq!(main.len(), 6);
        assert!(g.nodes[main[0] as usize].white);
        assert!(!g.nodes[main[1] as usize].white);
        assert_eq!(g.nodes[main[3] as usize].after, "back");
        // 28x19 took the man on 23; 14x23 took the man on 19.
        let b = cells(g.nodes[main[3] as usize].fen.as_deref().unwrap());
        let at = |n: u32| {
            let c = INTERNATIONAL.cell(n).unwrap();
            b[c / 10][c % 10]
        };
        assert_eq!((at(19), at(23), at(14), at(28)), (None, Some('m'), None, None));
        // The variation's capture backwards by a man: 38x27 takes the man on 32.
        let v = g.nodes.iter().find(|n| n.san == "38x27").unwrap();
        assert!(v.fen.is_some() && v.depth == 1);
        assert!(variant(&g) == INTERNATIONAL);
    }

    #[test]
    fn multiple_captures_and_kings() {
        // A white man on 37 takes 32 and 22 (written in full, and from and to only), then
        // a black man is crowned on 46.
        for mv in ["37x28x17", "37x17"] {
            let g = play(Some("W:W37:B32,22,41"), "20", &format!("1. {mv} 41-46"));
            assert_eq!(g.error, None, "{mv}");
            let b = cells(&last(&g));
            let c = INTERNATIONAL.cell(17).unwrap();
            assert_eq!(b[c / 10][c % 10], Some('M'), "{mv}");
            let c = INTERNATIONAL.cell(46).unwrap();
            assert_eq!(b[c / 10][c % 10], Some('k'));
            assert_eq!(b.iter().flatten().filter(|p| p.is_some()).count(), 2);
        }
        // A flying king takes from afar and lands beyond: 46x5 over 23.
        let g = play(Some("W:WK46:B23"), "20", "1. 46x5");
        assert_eq!(g.error, None);
        let b = cells(&last(&g));
        assert_eq!(b.iter().flatten().filter(|p| p.is_some()).count(), 1);
        // English draughts: a man takes forward only, and a king one square at a time.
        let g = play(Some("B:W18:B23"), "21", "1. 23x14");
        assert!(g.error.is_some(), "a man takes forward only: 23 cannot take 18 backwards");
        let g = play(Some("B:W18:B14"), "21", "1. 14x23");
        assert_eq!(g.error, None);
        let g = play(Some("B:W18:BK9"), "21", "1. 9x23");
        assert!(g.error.is_some(), "an English king does not fly");
        // A man crowned by a capture stops there in English draughts.
        let g = play(Some("W:W10:B6,7"), "21", "1. 10x1");
        assert_eq!(g.error, None, "{g:?}");
        assert!(last(&g).starts_with("1K"), "{}", last(&g));
    }

    #[test]
    fn checkers_numbers_moves_by_black_first() {
        let g = parse("[GameType \"21\"]\n\n1. 11-15 23-19 2. 8-11 22-17 *");
        assert_eq!(g.error, None);
        let w: Vec<String> = g.layout().iter().filter_map(|t| match t {
            pgn::Token::Move { node, number } => Some(format!("{}{}", number.clone().unwrap_or_default(), g.nodes[*node as usize].san)),
            _ => None,
        }).collect();
        assert_eq!(w, ["1.11-15", "23-19", "2.8-11", "22-17"]);
    }

    #[test]
    fn words_of_pdn() {
        assert!(matches!(word("32-28"), Word::Move("32-28", None)));
        assert!(matches!(word("12.32-28!"), Word::Move("32-28", Some(1))));
        assert!(matches!(word("28x19x10"), Word::Move(..)));
        assert!(matches!(word("c3-d4"), Word::Move(..)));
        assert!(matches!(word("2-0"), Word::Result(_)));
        assert!(matches!(word("12."), Word::Other));
        assert!(matches!(word("1234-5"), Word::Other));
        assert!(matches!(word("Nf3"), Word::Other));
    }

    /// Hostile text: any bytes, deep variations, many moves, a capture with countless paths.
    #[test]
    fn whatever_the_text() {
        for t in ["", "[", "[GameType \"20,W,0,0\"]", "[FEN \"W:W1-99999999:B\"]\n1. 1-6", "1. 99-1", "((((", "1. 32x1x2x3x4x5", "[GameType \"255,X,12,12\"]\n1. 1-2", "1. a1-zz9", "[FEN \"::::K:W,,K\"]"] {
            let g = parse(t);
            let _ = variant(&g);
            for n in &g.nodes {
                let _ = n.fen.as_deref().map(cells);
            }
        }
        // Deeper than MAX_DEPTH: skipped, no stack used.
        let deep = format!("1. 32-28 {}19-23{}", "( 33-28 ".repeat(100_000), ")".repeat(100_000));
        let g = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || parse(&deep)).unwrap().join().unwrap();
        assert!(g.nodes.len() <= pgn::MAX_DEPTH as usize + 3);
        // More than MAX_MOVES: the rest not read.
        let long = "1. 32-28 19-23 2. 28-32 23-19 ".repeat(pgn::MAX_MOVES);
        assert_eq!(parse(&long).nodes.len(), pgn::MAX_MOVES);
    }

    #[test]
    fn capture_search_is_capped() {
        // Kings everywhere: a white king among black men with gaps, many paths.
        let mut fen = String::from("W:WK46:B");
        let blacks: Vec<String> = (7..=44).filter(|n| n % 3 == 0).map(|n| n.to_string()).collect();
        fen.push_str(&blacks.join(","));
        let v = INTERNATIONAL;
        let b = Board::from_fen(v, &fen).unwrap();
        let start = std::time::Instant::now();
        let paths = b.captures(v.cell(46).unwrap());
        assert!(paths.len() <= MAX_SEARCH);
        assert!(start.elapsed() < crate::test_limit(std::time::Duration::from_secs(2)));
        // And a 12×12 board full of pieces: still bounded.
        let v = Variant::of(&[("GameType".into(), "27".into())], None);
        let mut b = Board::start(v);
        b.cells.iter_mut().filter(|c| **c == WM).take(1).for_each(|c| *c = WK);
        for c in 0..b.cells.len() {
            let _ = b.captures(c);
        }
    }

    #[test]
    fn a_file_in_the_table_and_found() {
        let d = std::env::temp_dir().join(format!("coxswain-pdn-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("games.pdn");
        std::fs::write(&p, "[Event \"Club\"]\n[White \"Anna\"]\n[Black \"Bram\"]\n[Result \"2-0\"]\n\n1. 32-28 19-23 2. 28x19 14x23 2-0\n\n[GameType \"21\"]\n[White \"Carl\"]\n[Black \"Dora\"]\n[Result \"0-1\"]\n\n1. 11-15 23-19 0-1\n").unwrap();
        let lib = crate::chess::Library::open(&p).unwrap();
        while !lib.progress().done {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(lib.progress().games, 2);
        let r = lib.row(0).unwrap();
        assert_eq!((r.white.as_str(), r.result.as_str(), r.moves, r.eco.as_str(), r.opening.as_str()), ("Anna", "2-0", 2, "", ""));
        let q = crate::chess::Query { result: "1-0".into(), ..Default::default() };
        assert_eq!(lib.query(&q), vec![0]);
        let g = lib.game(1).unwrap();
        assert_eq!((g.error.clone(), variant(&g)), (None, ENGLISH));
        let text = crate::extract::text_of(&p, 1000, 1 << 20).unwrap();
        assert!(text.contains("Carl – Dora") && !text.contains("11-15"), "{text}");
        let (o, c) = crate::store::MARK;
        assert_eq!(crate::chess::games::game_of_hit(&p, &format!("{o}Dora{c}")), Some(1));
        std::fs::remove_dir_all(&d).unwrap();
    }
}

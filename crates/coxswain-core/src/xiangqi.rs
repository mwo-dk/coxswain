//! Xiangqi, Chinese chess (docs/previews/xiangqi.md), for both apps: a PGN file whose tags say
//! `[Game "Chinese Chess"]` (or `[Variant "xiangqi"]`) goes through the chess table
//! (`chess::Library`), each game played out here on the board of 9 files and 10 ranks. Moves
//! are read in ICCS (`h2e2`, `H2-E2`), WXF (`C2.5`, `H8+7`, `+R.4`) and Chinese (`炮二平五`,
//! `马８进７`, `前炮退一`); a position in FEN. Reading only: the moves are played as written, not
//! checked against the rules beyond the piece and where it goes.
//!
//! Any text at all is read without a panic and without recursion: squares and numbers are
//! looked at as characters, never by byte offsets into the text.

use crate::chess::pgn::{self, Game, Rules, Word};

/// The start position, in FEN: Red (upper case) at the bottom.
pub const START: &str = "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 1";
const W: usize = 9;
const H: usize = 10;
/// The most of a file read to tell a xiangqi PGN file from a chess one.
pub const SNIFF: usize = 64 << 10;

/// Whether a PGN file's first tags say it holds xiangqi.
pub fn is_xiangqi(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&head[..head.len().min(SNIFF)]).to_ascii_lowercase();
    text.lines().filter(|l| l.trim_start().starts_with('[')).any(|l| {
        pgn::tag_pair(l).is_some_and(|(k, v)| {
            let (k, v) = (k.to_ascii_lowercase(), v.to_ascii_lowercase());
            (k == "game" && (v.contains("chinese chess") || v.contains("xiangqi"))) || (k == "variant" && v.contains("xiangqi"))
        })
    })
}

/// A position: 90 cells row by row from the top (Black's side), each a FEN letter or none;
/// Red to move or not; the move number.
#[derive(Clone, Debug)]
pub(crate) struct Board {
    cells: [Option<char>; W * H],
    red: bool,
    number: u32,
}

/// A FEN letter as this module keeps it: `k a b n r c p`, Red's upper case (`h` and `e` are
/// read as `n` and `b`).
fn letter(c: char) -> Option<char> {
    let l = match c.to_ascii_lowercase() {
        'k' | 'g' => 'k',
        'a' => 'a',
        'b' | 'e' => 'b',
        'n' | 'h' => 'n',
        'r' => 'r',
        'c' => 'c',
        'p' => 'p',
        _ => return None,
    };
    Some(if c.is_ascii_uppercase() { l.to_ascii_uppercase() } else { l })
}

impl Board {
    fn from_fen(fen: &str) -> Option<Board> {
        let mut f = fen.split_whitespace();
        let placement = f.next()?;
        let mut cells = [None; W * H];
        let rows: Vec<&str> = placement.split('/').collect();
        if rows.len() != H {
            return None;
        }
        for (y, row) in rows.iter().enumerate() {
            let mut x = 0usize;
            for c in row.chars() {
                if let Some(d) = c.to_digit(10) {
                    x += d as usize;
                } else {
                    let p = letter(c)?;
                    if x >= W {
                        return None;
                    }
                    cells[x + y * W] = Some(p);
                    x += 1;
                }
                if x > W {
                    return None;
                }
            }
        }
        let red = !matches!(f.next(), Some("b" | "B"));
        let number = f.nth(2).and_then(|n| n.parse::<u32>().ok()).filter(|&n| n > 0 && n < 100_000).unwrap_or(1);
        Some(Board { cells, red, number })
    }

    /// The position in FEN, as the apps draw it.
    pub(crate) fn fen(&self) -> String {
        let mut out = String::new();
        for y in 0..H {
            if y > 0 {
                out.push('/');
            }
            let mut empty = 0;
            for x in 0..W {
                match self.cells[x + y * W] {
                    None => empty += 1,
                    Some(p) => {
                        if empty > 0 {
                            out.push_str(&empty.to_string());
                            empty = 0;
                        }
                        out.push(p);
                    }
                }
            }
            if empty > 0 {
                out.push_str(&empty.to_string());
            }
        }
        out.push_str(if self.red { " w" } else { " b" });
        out
    }

    fn mine(&self, p: char) -> bool {
        p.is_ascii_uppercase() == self.red
    }

    /// The cell of the moving side's file `n` (1 to 9, from its own right).
    fn file(&self, n: u32) -> Option<usize> {
        (1..=9).contains(&n).then(|| if self.red { 9 - n as usize } else { n as usize - 1 })
    }

    /// A move in ICCS: `h2e2`, `H2-E2`.
    fn iccs(&self, mv: &str) -> Option<(usize, usize)> {
        let c: Vec<char> = mv.chars().filter(|&c| c != '-').collect();
        if c.len() != 4 {
            return None;
        }
        let sq = |f: char, r: char| -> Option<usize> {
            let x = (f.to_ascii_lowercase() as u32).checked_sub('a' as u32).filter(|&x| x < 9)?;
            let r = r.to_digit(10)?;
            Some(x as usize + (9 - r as usize) * W)
        };
        Some((sq(c[0], c[1])?, sq(c[2], c[3])?))
    }

    /// A move in WXF: piece, file (or `+`/`-` for the front or rear of two on one file), way
    /// (`+` forward, `-` back, `.` or `=` across), then a file or a number of ranks.
    fn wxf(&self, mv: &str) -> Option<(usize, usize)> {
        let c: Vec<char> = mv.chars().collect();
        if c.len() != 4 {
            return None;
        }
        // The piece first, or the front/rear mark first: `C+.5` and `+C.5` alike.
        let (piece, at) = match (letter(c[0]), letter(c[1])) {
            (Some(p), _) => (p, c[1]),
            (None, Some(p)) if matches!(c[0], '+' | '-') => (p, c[0]),
            _ => return None,
        };
        let kind = piece.to_ascii_lowercase();
        let (way, to) = (c[2], c[3].to_digit(10)?);
        let forward: i32 = if self.red { -1 } else { 1 };
        let ours = |x: usize| -> Vec<usize> {
            (0..H).map(|y| x + y * W).filter(|&i| self.cells[i].is_some_and(|p| p.to_ascii_lowercase() == kind && self.mine(p))).collect()
        };
        let from = match at {
            '+' | '-' => {
                // The file with two (or more) of them; front is the furthest forward.
                let mut on = (0..W).map(ours).find(|v| v.len() >= 2)?;
                if forward < 0 {
                    on.reverse();
                }
                // `on` now runs from the back to the front.
                if at == '+' { *on.last()? } else { on[0] }
            }
            d => {
                let x = self.file(d.to_digit(10)?)?;
                let on = ours(x);
                match on.len() {
                    1 => on[0],
                    0 => return None,
                    // Two on the file, not told apart: the one whose move fits.
                    _ => *on.iter().find(|&&f| self.target(f, kind, way, to, forward).is_some_and(|t| self.cells[t].is_none_or(|p| !self.mine(p))))?,
                }
            }
        };
        Some((from, self.target(from, kind, way, to, forward)?))
    }

    /// Where a WXF move of a `kind` piece on `from` goes.
    fn target(&self, from: usize, kind: char, way: char, to: u32, forward: i32) -> Option<usize> {
        let (x, y) = ((from % W) as i32, (from / W) as i32);
        let (tx, ty) = match (kind, way) {
            ('r' | 'c' | 'p' | 'k', '.' | '=') => (self.file(to)? as i32, y),
            ('r' | 'c' | 'p' | 'k', '+') => (x, y + forward * to as i32),
            ('r' | 'c' | 'p' | 'k', '-') => (x, y - forward * to as i32),
            ('a' | 'b' | 'n', '+' | '-') => {
                let tx = self.file(to)? as i32;
                let dx = (tx - x).abs();
                let dy = match (kind, dx) {
                    ('a', 1) => 1,
                    ('b', 2) => 2,
                    ('n', 1) => 2,
                    ('n', 2) => 1,
                    _ => return None,
                };
                (tx, y + if way == '+' { forward * dy } else { -forward * dy })
            }
            _ => return None,
        };
        ((0..W as i32).contains(&tx) && (0..H as i32).contains(&ty)).then(|| (tx + ty * W as i32) as usize)
    }

    fn play(&self, mv: &str) -> Option<(Board, usize, usize)> {
        let wxf = chinese(mv);
        let (from, to) = self.iccs(mv).or_else(|| self.wxf(wxf.as_deref().unwrap_or(mv)))?;
        let p = self.cells[from]?;
        if !self.mine(p) || from == to || self.cells[to].is_some_and(|q| self.mine(q)) {
            return None;
        }
        let mut b = self.clone();
        b.cells[to] = Some(p);
        b.cells[from] = None;
        if !self.red {
            b.number += 1;
        }
        b.red = !self.red;
        Some((b, from, to))
    }
}

/// A move in Chinese, as WXF: `炮二平五` is `C2.5`, `马８进７` `H8+7`, `前车进一` `+R+1`. None for
/// anything else.
fn chinese(mv: &str) -> Option<String> {
    let c: Vec<char> = mv.chars().collect();
    if c.len() != 4 || c.iter().all(char::is_ascii) {
        return None;
    }
    let piece = |c: char| {
        Some(match c {
            '车' | '車' | '俥' | '伡' => 'R',
            '马' | '馬' | '傌' | '㐷' => 'H',
            '相' | '象' => 'E',
            '仕' | '士' => 'A',
            '帅' | '帥' | '将' | '將' => 'K',
            '炮' | '砲' | '包' => 'C',
            '兵' | '卒' => 'P',
            _ => return None,
        })
    };
    let number = |c: char| -> Option<char> {
        let n = match c {
            '一' | '１' => 1,
            '二' | '２' => 2,
            '三' | '３' => 3,
            '四' | '４' => 4,
            '五' | '５' => 5,
            '六' | '６' => 6,
            '七' | '７' => 7,
            '八' | '８' => 8,
            '九' | '９' => 9,
            c => c.to_digit(10).filter(|n| (1..=9).contains(n))?,
        };
        char::from_digit(n, 10)
    };
    let way = |c: char| {
        Some(match c {
            '进' | '進' => '+',
            '退' => '-',
            '平' => '.',
            _ => return None,
        })
    };
    let (p, at) = match (piece(c[0]), c[0]) {
        (Some(p), _) => (p, number(c[1])?),
        (None, '前') => (piece(c[1])?, '+'),
        (None, '后' | '後') => (piece(c[1])?, '-'),
        _ => return None,
    };
    Some(format!("{p}{at}{}{}", way(c[2])?, number(c[3])?))
}

/// A word of xiangqi movetext: a move in ICCS, WXF or Chinese, a result, or anything else.
fn word(w: &str) -> Word<'_> {
    if pgn::is_result(w) {
        return Word::Result(w);
    }
    let mut w = w;
    let digits = w.find(|c: char| !c.is_ascii_digit()).unwrap_or(w.len());
    if digits > 0 && w[digits..].starts_with(['.', '…']) {
        w = w[digits..].trim_start_matches(['.', '…']);
    }
    let body = w.trim_end_matches(['!', '?']);
    let nag = pgn::suffix_nag(&w[body.len()..]);
    let n = body.chars().count();
    let ascii = body.is_ascii();
    let iccs = ascii && {
        let c: Vec<char> = body.chars().filter(|&c| c != '-').collect();
        c.len() == 4 && c[0].is_ascii_alphabetic() && c[1].is_ascii_digit() && c[2].is_ascii_alphabetic() && c[3].is_ascii_digit()
    };
    let wxf = ascii && n == 4 && {
        let c: Vec<char> = body.chars().collect();
        (letter(c[0]).is_some() || matches!(c[0], '+' | '-')) && matches!(c[2], '+' | '-' | '.' | '=') && c[3].is_ascii_digit()
    };
    if iccs || wxf || chinese(body).is_some() { Word::Move(body, nag) } else { Word::Other }
}

/// Xiangqi, as `pgn::parse_with` plays it out.
pub(crate) struct Xiangqi;

impl Rules for Xiangqi {
    type Board = Board;

    fn start(&self, tags: &[(String, String)], _: &str) -> (Board, String) {
        let b = tags
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("FEN"))
            .and_then(|(_, f)| Board::from_fen(f))
            .unwrap_or_else(|| Board::from_fen(START).expect("the start position"));
        let fen = b.fen();
        (b, fen)
    }

    fn word<'a>(&self, w: &'a str) -> Word<'a> {
        word(w)
    }

    fn turn(&self, b: &Board) -> (bool, u32) {
        (b.red, b.number)
    }

    fn play(&self, b: &Board, mv: &str) -> Option<(Board, String, Option<u8>, Option<u8>)> {
        let (nb, from, to) = b.play(mv)?;
        let fen = nb.fen();
        Some((nb, fen, Some(from as u8), Some(to as u8)))
    }
}

/// A xiangqi game's text, read and played out. `Node::white` is Red's move; `fen` the
/// position in FEN; `from` and `to` cells (`x + y × 9`, rows from Black's side).
pub fn parse(text: &str) -> Game {
    pgn::parse_with(text, &Xiangqi)
}

/// Whether the move text `w` is a move, for the table's count.
pub(crate) fn is_move(w: &str) -> bool {
    matches!(word(w), Word::Move(..))
}

/// A position's cells from its FEN, rows from the top: each a FEN letter or none. A FEN that
/// is no xiangqi position gives an empty board.
pub fn cells(fen: &str) -> Vec<Option<char>> {
    Board::from_fen(fen).map_or_else(|| vec![None; W * H], |b| b.cells.to_vec())
}

/// A piece's character: Red's 帥仕相傌俥炮兵, Black's 將士象馬車砲卒.
pub fn glyph(p: char) -> char {
    let red = p.is_ascii_uppercase();
    match (p.to_ascii_lowercase(), red) {
        ('k', true) => '帥',
        ('k', false) => '將',
        ('a', _) => if red { '仕' } else { '士' },
        ('b', true) => '相',
        ('b', false) => '象',
        ('n', true) => '傌',
        ('n', false) => '馬',
        ('r', true) => '俥',
        ('r', false) => '車',
        ('c', true) => '炮',
        ('c', false) => '砲',
        (_, true) => '兵',
        (_, false) => '卒',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn last(g: &Game) -> String {
        let m = g.main_line();
        m.last().map_or(g.start.clone(), |&n| g.nodes[n as usize].fen.clone().unwrap_or_default())
    }

    #[test]
    fn the_same_opening_in_three_notations() {
        let iccs = parse("[Game \"Chinese Chess\"]\n\n1. h2e2 h9g7 2. h0g2 i9h9 *");
        let wxf = parse("1. C2.5 H8+7 2. H2+3 R9.8 *");
        let zh = parse("1. 炮二平五 马８进７ 2. 马二进三 车９平８ *");
        for g in [&iccs, &wxf, &zh] {
            assert_eq!(g.error, None, "{:?}", g.nodes.iter().map(|n| &n.san).collect::<Vec<_>>());
            assert_eq!(g.main_line().len(), 4);
        }
        assert_eq!(last(&iccs), last(&wxf));
        assert_eq!(last(&wxf), last(&zh));
        assert_eq!(last(&iccs), "rnbakabr1/9/1c4nc1/p1p1p1p1p/9/9/P1P1P1P1P/1C2C1N2/9/RNBAKAB1R w");
        let first = &iccs.nodes[iccs.main_line()[0] as usize];
        assert!(first.white);
        // h2: file h, rank 2 from Red's side: x 7, y 7.
        assert_eq!((first.from, first.to), (Some(7 + 7 * 9), Some(4 + 7 * 9)));
    }

    #[test]
    fn captures_and_front_and_rear() {
        // A cannon takes the horse over the screen: C2+7, from h2 up 7 ranks to h9.
        let g = parse("1. C2+7 *");
        assert_eq!(g.error, None);
        assert!(last(&g).starts_with("rnbakabCr/"), "{}", last(&g));
        // Two red chariots on one file: front and rear.
        let fen = "4k4/9/9/9/9/9/4R4/9/4R4/4K4 w - - 0 1";
        let g = parse(&format!("[FEN \"{fen}\"]\n\n1. +R.1 *"));
        assert_eq!(g.error, None);
        assert_eq!(last(&g), "4k4/9/9/9/9/9/8R/9/4R4/4K4 b");
        let g = parse(&format!("[FEN \"{fen}\"]\n\n1. 后车平九 *"));
        assert_eq!(last(&g), "4k4/9/9/9/9/9/4R4/9/R8/4K4 b");
        // A move onto one's own piece, or from nowhere, is not played.
        assert!(parse("1. R1+3 *").error.is_some());
        assert!(parse("1. a5a6 *").error.is_some());
    }

    #[test]
    fn files_are_sniffed() {
        assert!(is_xiangqi(b"[Event \"x\"]\n[Game \"Chinese Chess\"]\n\n1. h2e2"));
        assert!(is_xiangqi(b"[Variant \"Xiangqi\"]\n"));
        assert!(!is_xiangqi(b"[Event \"Chinese Chess Club\"]\n[White \"A\"]\n"));
        assert_eq!(glyph('R'), '俥');
        assert_eq!(glyph('k'), '將');
        assert_eq!(cells("nonsense").len(), 90);
        assert!(is_move("C2.5") && is_move("h2-e2") && is_move("炮二平五") && !is_move("Nf3") && !is_move("12."));
    }

    #[test]
    fn whatever_the_text() {
        for t in ["", "[FEN \"\"]", "[FEN \"9/9/9/9/9/9/9/9/9/99999999999 w\"]\n1. h2e2", "[FEN \"rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 99999999999\"]",
            "1. 炮二平 1. 炮炮炮炮 1. 前前前前 1. é2e2 1. h9h9 1. +X.1 1. +R+9 1. R9-9 1. H1+9 1. E3+9 1. A4+9", "1. z9z9 1. h2éé", "((((", "1. ８８８８"] {
            let g = parse(t);
            for n in &g.nodes {
                let _ = n.fen.as_deref().map(cells);
            }
        }
        let deep = format!("1. h2e2 {}h9g7{}", "( C2.5 ".repeat(100_000), ")".repeat(100_000));
        let g = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || parse(&deep)).unwrap().join().unwrap();
        assert!(g.nodes.len() <= pgn::MAX_DEPTH as usize + 3);
        let long = "1. h2e2 h9g7 2. e2h2 g7h9 ".repeat(pgn::MAX_MOVES);
        assert_eq!(parse(&long).nodes.len(), pgn::MAX_MOVES);
    }

    #[test]
    fn a_file_in_the_table_and_found() {
        let d = std::env::temp_dir().join(format!("coxswain-xq-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("games.pgn");
        std::fs::write(&p, "[Game \"Chinese Chess\"]\n[Event \"Club\"]\n[Red \"Hu\"]\n[Black \"Lin\"]\n[Result \"1-0\"]\n\n1. h2e2 h9g7 2. h0g2 1-0\n\n[Game \"Chinese Chess\"]\n[Red \"Wang\"]\n[Black \"Zhao\"]\n\n1. 炮二平五 *\n").unwrap();
        assert_eq!(crate::chess::games::kind(&p), crate::chess::Format::Xiangqi);
        let lib = crate::chess::Library::open(&p).unwrap();
        while !lib.progress().done {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(lib.format, crate::chess::Format::Xiangqi);
        let r = lib.row(0).unwrap();
        assert_eq!((r.white.as_str(), r.black.as_str(), r.moves, r.opening.as_str()), ("Hu", "Lin", 2, ""));
        assert_eq!(lib.game(1).unwrap().error, None);
        let text = crate::extract::text_of(&p, 1000, 1 << 20).unwrap();
        assert!(text.contains("Wang – Zhao") && !text.contains("h2e2"), "{text}");
        let (o, c) = crate::store::MARK;
        assert_eq!(crate::chess::games::game_of_hit(&p, &format!("{o}Zhao{c}")), Some(1));
        std::fs::remove_dir_all(&d).unwrap();
    }
}

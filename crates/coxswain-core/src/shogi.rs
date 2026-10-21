//! Shogi (docs/previews/shogi.md), for both apps: KIF (`.kif`, `.kifu`) and CSA (`.csa`)
//! files go through the chess table (`chess::Library`), each game played out here on its 9×9
//! board with the pieces each side holds in hand: captures go to the hand, drops come from it,
//! promotions turn a piece over. KIF's variations (`変化：`) and comments (`*`) are kept; a
//! handicap (`手合割`) or a board diagram (BOD) or CSA's `P1`…`P9` start a game of its own.
//! Reading only: moves are played as written, not checked against the rules.
//!
//! Any text at all is read without a panic and without recursion: the moves are read line by
//! line, every line and every game capped (`MAX_LINE`, `pgn::MAX_MOVES`, `MAX_GAMES`).

use crate::chess::pgn::{Game, Node};
use crate::chess::Format;

/// The file endings of shogi files, for the search store.
pub const ENDINGS: [&str; 3] = [".kif", ".kifu", ".csa"];
/// The most of one line looked at: a line of a real file is short.
const MAX_LINE: usize = 4096;
/// The most games of one CSA file.
pub const MAX_GAMES: usize = 100_000;
/// The most variations (`変化：`) of one KIF game.
pub const MAX_BRANCHES: usize = 10_000;

/// A piece: its kind (`P L N S G B R K`), Sente's (else Gote's), promoted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub kind: char,
    pub sente: bool,
    pub promoted: bool,
}

const KINDS: [char; 8] = ['R', 'B', 'G', 'S', 'N', 'L', 'P', 'K'];

/// A position: 81 cells row by row from the top (Gote's side), file 9 on the left; the pieces
/// in hand, counted per kind (`KINDS` order, the king left out); whose move.
#[derive(Clone, Debug, PartialEq)]
pub struct Board {
    pub cells: [Option<Piece>; 81],
    pub hands: [[u8; 7]; 2],
    pub sente: bool,
}

/// The start position, in SFEN.
pub const START: &str = "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1";

/// A square from its file (1 to 9, right to left as Sente sees it) and rank (1 to 9 from the
/// top).
fn cell(file: u32, rank: u32) -> Option<usize> {
    ((1..=9).contains(&file) && (1..=9).contains(&rank)).then(|| (9 - file as usize) + (rank as usize - 1) * 9)
}

impl Board {
    pub fn from_sfen(sfen: &str) -> Option<Board> {
        let mut f = sfen.split_whitespace();
        let rows: Vec<&str> = f.next()?.split('/').collect();
        if rows.len() != 9 {
            return None;
        }
        let mut b = Board { cells: [None; 81], hands: [[0; 7]; 2], sente: true };
        for (y, row) in rows.iter().enumerate() {
            let (mut x, mut plus) = (0usize, false);
            for c in row.chars() {
                if c == '+' {
                    plus = true;
                    continue;
                }
                if let Some(d) = c.to_digit(10) {
                    x += d as usize;
                } else {
                    let kind = c.to_ascii_uppercase();
                    if !KINDS.contains(&kind) || x >= 9 {
                        return None;
                    }
                    b.cells[x + y * 9] = Some(Piece { kind, sente: c.is_ascii_uppercase(), promoted: plus });
                    x += 1;
                }
                plus = false;
                if x > 9 {
                    return None;
                }
            }
        }
        b.sente = f.next() != Some("w");
        let mut n = 0u32;
        for c in f.next().unwrap_or("-").chars() {
            if let Some(d) = c.to_digit(10) {
                n = (n * 10 + d).min(99);
            } else if let Some(k) = KINDS[..7].iter().position(|&k| k == c.to_ascii_uppercase()) {
                let h = &mut b.hands[(!c.is_ascii_uppercase()) as usize][k];
                *h = h.saturating_add(n.max(1) as u8).min(18);
                n = 0;
            }
        }
        Some(b)
    }

    /// The position in SFEN, as the apps draw it (`+` before a promoted piece).
    pub fn sfen(&self) -> String {
        let mut out = String::new();
        for y in 0..9 {
            if y > 0 {
                out.push('/');
            }
            let mut empty = 0;
            for x in 0..9 {
                match self.cells[x + y * 9] {
                    None => empty += 1,
                    Some(p) => {
                        if empty > 0 {
                            out.push_str(&empty.to_string());
                            empty = 0;
                        }
                        if p.promoted {
                            out.push('+');
                        }
                        out.push(if p.sente { p.kind } else { p.kind.to_ascii_lowercase() });
                    }
                }
            }
            if empty > 0 {
                out.push_str(&empty.to_string());
            }
        }
        out.push_str(if self.sente { " b " } else { " w " });
        let mut hands = String::new();
        for (side, h) in self.hands.iter().enumerate() {
            for (k, &n) in h.iter().enumerate() {
                if n > 1 {
                    hands.push_str(&n.to_string());
                }
                if n > 0 {
                    hands.push(if side == 0 { KINDS[k] } else { KINDS[k].to_ascii_lowercase() });
                }
            }
        }
        out.push_str(if hands.is_empty() { "-" } else { &hands });
        out
    }

    /// A move: from a square (None: a drop of `kind` from the hand) to one, promoting or not.
    fn play(&self, from: Option<usize>, to: usize, kind: char, promote: bool) -> Option<Board> {
        let mut b = self.clone();
        let side = (!self.sente) as usize;
        let piece = match from {
            Some(f) => {
                let p = b.cells[f].take()?;
                if p.sente != self.sente {
                    return None;
                }
                Piece { promoted: p.promoted || promote, ..p }
            }
            None => {
                let k = KINDS[..7].iter().position(|&k| k == kind)?;
                let h = &mut b.hands[side][k];
                *h = h.checked_sub(1)?;
                Piece { kind, sente: self.sente, promoted: false }
            }
        };
        if let Some(taken) = b.cells[to] {
            if taken.sente == self.sente || from.is_none() {
                return None;
            }
            if let Some(k) = KINDS[..7].iter().position(|&k| k == taken.kind) {
                b.hands[side][k] = b.hands[side][k].saturating_add(1);
            }
        }
        b.cells[to] = Some(piece);
        b.sente = !self.sente;
        Some(b)
    }
}

/// The kind of a piece's kanji, and whether the kanji is a promoted piece.
fn kanji(c: char) -> Option<(char, bool)> {
    Some(match c {
        '歩' => ('P', false),
        '香' => ('L', false),
        '桂' => ('N', false),
        '銀' => ('S', false),
        '金' => ('G', false),
        '角' => ('B', false),
        '飛' => ('R', false),
        '玉' | '王' => ('K', false),
        'と' => ('P', true),
        '杏' => ('L', true),
        '圭' => ('N', true),
        '全' => ('S', true),
        '馬' => ('B', true),
        '龍' | '竜' => ('R', true),
        _ => return None,
    })
}

/// A piece's kanji as the board shows it: 歩 香 桂 銀 金 角 飛 玉 (王 for Gote's king) and the
/// promoted と 杏 圭 全 馬 龍.
pub fn glyph(p: Piece) -> char {
    match (p.kind, p.promoted) {
        ('P', false) => '歩',
        ('L', false) => '香',
        ('N', false) => '桂',
        ('S', false) => '銀',
        ('G', _) => '金',
        ('B', false) => '角',
        ('R', false) => '飛',
        ('K', _) => if p.sente { '玉' } else { '王' },
        ('P', true) => 'と',
        ('L', true) => '杏',
        ('N', true) => '圭',
        ('S', true) => '全',
        ('B', true) => '馬',
        _ => '龍',
    }
}

/// A move written in Japanese: `▲７六歩`, `△同　銀`, `▲４五角打`, `▲２二角成`.
fn written(sente: bool, to: usize, same: bool, p: Piece, drop: bool, promote: bool) -> String {
    const FULL: [char; 9] = ['１', '２', '３', '４', '５', '６', '７', '８', '９'];
    const RANK: [char; 9] = ['一', '二', '三', '四', '五', '六', '七', '八', '九'];
    let mark = if sente { '▲' } else { '△' };
    let at = if same { "同\u{3000}".to_string() } else { format!("{}{}", FULL[8 - to % 9], RANK[to / 9]) };
    let piece = glyph(Piece { promoted: p.promoted && !promote, ..p });
    format!("{mark}{at}{piece}{}", if promote { "成" } else if drop { "打" } else { "" })
}

/// A number written in kanji (一 … 九, 十 …) or digits, full-width too.
fn number(s: &str) -> Option<u32> {
    let mut n = 0u32;
    let mut any = false;
    for c in s.chars() {
        let d = match c {
            '一' => 1,
            '二' => 2,
            '三' => 3,
            '四' => 4,
            '五' => 5,
            '六' => 6,
            '七' => 7,
            '八' => 8,
            '九' => 9,
            '十' => {
                n = n.max(1) * 10;
                any = true;
                continue;
            }
            c => match c.to_digit(10).or_else(|| ('０'..='９').contains(&c).then(|| c as u32 - '０' as u32)) {
                Some(d) => d,
                None => break,
            },
        };
        n = if n >= 10 && n % 10 == 0 { n + d } else { n.saturating_mul(10).saturating_add(d) }.min(100_000);
        any = true;
    }
    any.then_some(n)
}

/// Text as read from the file: UTF-8, else Shift_JIS (KIF's own).
pub fn decode(b: &[u8]) -> String {
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::SHIFT_JIS.decode_without_bom_handling(b).0.into_owned(),
    }
}

/// The lines of a text, each cut at `MAX_LINE` characters.
fn lines(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().map(|l| l.chars().take(MAX_LINE).collect::<String>())
}

/// The position a handicap (`手合割`) starts from; Gote (the stronger) moves first.
fn handicap(name: &str) -> Option<Board> {
    let mut b = Board::from_sfen(START)?;
    let off: &[(u32, u32)] = match name.trim() {
        "平手" | "" => return Some(b),
        "香落ち" => &[(1, 1)],
        "右香落ち" => &[(9, 1)],
        "角落ち" => &[(2, 2)],
        "飛車落ち" => &[(8, 2)],
        "飛香落ち" => &[(8, 2), (1, 1)],
        "二枚落ち" => &[(8, 2), (2, 2)],
        "四枚落ち" => &[(8, 2), (2, 2), (1, 1), (9, 1)],
        "六枚落ち" => &[(8, 2), (2, 2), (1, 1), (9, 1), (2, 1), (8, 1)],
        "八枚落ち" => &[(8, 2), (2, 2), (1, 1), (9, 1), (2, 1), (8, 1), (3, 1), (7, 1)],
        "十枚落ち" => &[(8, 2), (2, 2), (1, 1), (9, 1), (2, 1), (8, 1), (3, 1), (7, 1), (4, 1), (6, 1)],
        _ => return None,
    };
    for &(f, r) in off {
        b.cells[cell(f, r)?] = None;
    }
    b.sente = false;
    Some(b)
}

/// A game being read: its tags, its tree, and the boards of its nodes.
struct Reading {
    g: Game,
    boards: Vec<Board>,
    start: Board,
}

impl Reading {
    fn new() -> Reading {
        let start = Board::from_sfen(START).expect("the start position");
        Reading { g: Game { start: START.to_string(), ..Game::default() }, boards: vec![], start }
    }

    fn tag(&mut self, k: &str, v: &str) {
        let v = v.trim();
        if !v.is_empty() && self.g.tags.len() < 64 {
            self.g.tags.push((k.to_string(), v.chars().take(crate::chess::pgn::MAX_TAG).collect()));
        }
    }

    fn set_start(&mut self, b: Board) {
        if self.g.nodes.is_empty() {
            self.g.start = b.sfen();
            self.start = b;
        }
    }

    /// The board after node `at`, or the start.
    fn board(&self, at: Option<u32>) -> &Board {
        at.map_or(&self.start, |n| &self.boards[n as usize])
    }

    /// A move after node `at`: played, its node added (None when it cannot be played or the
    /// game is full).
    fn add(&mut self, at: Option<u32>, from: Option<usize>, to: usize, kind: Option<char>, promote: bool, same: bool) -> Option<u32> {
        if self.g.nodes.len() >= crate::chess::pgn::MAX_MOVES {
            return None;
        }
        let before = self.board(at).clone();
        let kind = kind.or_else(|| from.and_then(|f| before.cells[f]).map(|p| p.kind))?;
        let piece = match from {
            Some(f) => before.cells[f]?,
            None => Piece { kind, sente: before.sente, promoted: false },
        };
        let after = before.play(from, to, kind, promote && !piece.promoted)?;
        let number = at.map_or(1, |n| self.g.nodes[n as usize].number + 1);
        let id = self.g.nodes.len() as u32;
        let depth = match at {
            Some(n) => self.g.nodes[n as usize].depth + (!self.g.nodes[n as usize].children.is_empty()) as u32,
            None => (!self.g.roots.is_empty()) as u32,
        };
        self.g.nodes.push(Node {
            san: written(before.sente, to, same, piece, from.is_none(), promote && !piece.promoted),
            parent: at,
            fen: Some(after.sfen()),
            from: from.map(|f| f as u8),
            to: Some(to as u8),
            white: true,
            number,
            depth,
            ..Node::default()
        });
        self.boards.push(after);
        match at {
            Some(n) => self.g.nodes[n as usize].children.push(id),
            None => self.g.roots.push(id),
        }
        Some(id)
    }

    fn comment(&mut self, at: Option<u32>, c: &str) {
        let c = c.trim();
        if c.is_empty() {
            return;
        }
        let target = match at {
            Some(n) => &mut self.g.nodes[n as usize].after,
            None => &mut self.g.comment,
        };
        if target.len() < crate::chess::pgn::MAX_TAG * 16 {
            if !target.is_empty() {
                target.push(' ');
            }
            target.push_str(c);
        }
    }
}

/// A KIF game read and played out.
fn kif(text: &str) -> Game {
    let mut r = Reading::new();
    // The lines read so far, each as its nodes in order; the one being read.
    let mut lines_read: Vec<Vec<u32>> = vec![vec![]];
    let mut line = 0usize;
    let mut at: Option<u32> = None;
    let mut stopped = false;
    let mut bod: Vec<String> = vec![];
    let mut hand: [Option<String>; 2] = [None, None];
    let mut first_gote = false;
    for l in lines(text) {
        let t = l.trim_end();
        if t.starts_with('#') || t.is_empty() {
            continue;
        }
        if t.starts_with('*') {
            r.comment(at, t.trim_start_matches('*'));
            continue;
        }
        if t.starts_with('|') {
            bod.push(t.to_string());
            continue;
        }
        if let Some(rest) = t.strip_prefix("変化：").or_else(|| t.strip_prefix("変化:")) {
            // A variation from move N: the latest line that reached move N - 1 goes on there.
            let n = number(rest).unwrap_or(1) as usize;
            if lines_read.len() >= MAX_BRANCHES {
                break;
            }
            let base = lines_read.iter().rev().find(|v| v.len() + 1 >= n).cloned().unwrap_or_default();
            let keep: Vec<u32> = base.into_iter().take(n.saturating_sub(1)).collect();
            at = keep.last().copied();
            lines_read.push(keep);
            line = lines_read.len() - 1;
            stopped = false;
            continue;
        }
        if let Some((k, v)) = t.split_once('：').or_else(|| t.split_once(':')).filter(|(k, _)| !k.trim_start().starts_with(|c: char| c.is_ascii_digit())) {
            let k = k.trim();
            match k {
                "先手" | "下手" => r.tag("White", v),
                "後手" | "上手" => r.tag("Black", v),
                "棋戦" => r.tag("Event", v),
                "場所" => r.tag("Site", v),
                "開始日時" => r.tag("Date", v.split_whitespace().next().unwrap_or(v)),
                "戦型" => r.tag("Opening", v),
                "手合割" => {
                    r.tag("Handicap", v);
                    if let Some(b) = handicap(v) {
                        r.set_start(b);
                    }
                }
                "先手の持駒" | "下手の持駒" => hand[0] = Some(v.to_string()),
                "後手の持駒" | "上手の持駒" => hand[1] = Some(v.to_string()),
                _ => {}
            }
            continue;
        }
        if t.starts_with("後手番") || t.starts_with("上手番") {
            first_gote = true;
            continue;
        }
        if t.starts_with("まで") {
            let res = if t.contains("先手の勝") || t.contains("下手の勝") {
                "1-0"
            } else if t.contains("後手の勝") || t.contains("上手の勝") {
                "0-1"
            } else if t.contains("千日手") || t.contains("持将棋") {
                "1/2-1/2"
            } else {
                ""
            };
            if !res.is_empty() && r.g.result.is_empty() {
                r.g.result = res.to_string();
            }
            continue;
        }
        // A move line: its number, then the move.
        let mut w = t.split_whitespace();
        let Some(n) = w.next().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let Some(mv) = w.next() else { continue };
        // A board diagram before the first move: the start, with the hands read by now.
        if !bod.is_empty() && r.g.nodes.is_empty() {
            if let Some(b) = bod_board(&bod, &hand, first_gote) {
                r.set_start(b);
            }
            bod.clear();
        }
        if stopped {
            continue;
        }
        let mv = if mv == "同" { format!("同{}", w.next().unwrap_or("")) } else { mv.to_string() };
        let _ = n;
        match kif_move(&mv, r.board(at), at.map(|a| r.g.nodes[a as usize].to.unwrap_or(0) as usize)) {
            KifMove::Move { from, to, kind, promote, same } => match r.add(at, from, to, kind, promote, same) {
                Some(id) => {
                    lines_read[line].push(id);
                    at = Some(id);
                }
                None => {
                    if r.g.error.is_none() {
                        r.g.error = Some(format!("{} {mv}", r.g.nodes.len() + 1));
                    }
                    stopped = true;
                }
            },
            KifMove::End(res) => {
                // The first line's end says who won: the side to move resigned.
                if line == 0 && r.g.result.is_empty() {
                    let sente_to_move = r.board(at).sente;
                    r.g.result = match res {
                        Some(true) => if sente_to_move { "0-1" } else { "1-0" },
                        Some(false) => "1/2-1/2",
                        None => "*",
                    }
                    .to_string();
                }
                stopped = true;
            }
            KifMove::Other => {}
        }
    }
    if !bod.is_empty() && r.g.nodes.is_empty() {
        if let Some(b) = bod_board(&bod, &hand, first_gote) {
            r.set_start(b);
        }
    }
    if r.g.result.is_empty() {
        r.g.result = "*".into();
    }
    r.g
}

enum KifMove {
    Move { from: Option<usize>, to: usize, kind: Option<char>, promote: bool, same: bool },
    /// The game ends: resigned or lost (Some(true)), a draw (Some(false)), stopped (None).
    End(Option<bool>),
    Other,
}

/// A KIF move: `７六歩(77)`, `同　歩(23)`, `４五角打`, `２二角成(88)`, `３三桂不成(45)`, or an end.
fn kif_move(mv: &str, b: &Board, last: Option<usize>) -> KifMove {
    match mv {
        "投了" | "詰み" | "反則負け" | "切れ負け" | "反則勝ち" => return KifMove::End(Some(true)),
        "千日手" | "持将棋" => return KifMove::End(Some(false)),
        "中断" | "封じ手" => return KifMove::End(None),
        _ => {}
    }
    let c: Vec<char> = mv.chars().filter(|&c| c != '\u{3000}' && c != ' ').collect();
    let (to, same, rest) = if c.first() == Some(&'同') {
        match last {
            Some(t) => (t, true, &c[1..]),
            None => return KifMove::Other,
        }
    } else if c.len() >= 2 {
        let (Some(f), Some(r)) = (number(&c[0].to_string()), number(&c[1].to_string())) else { return KifMove::Other };
        match cell(f, r) {
            Some(t) => (t, false, &c[2..]),
            None => return KifMove::Other,
        }
    } else {
        return KifMove::Other;
    };
    // The piece: one kanji, or 成 and one (成香 成桂 成銀).
    let (kind, rest) = match rest {
        ['成', k, rest @ ..] => match kanji(*k) {
            Some((k, _)) => (k, rest),
            None => return KifMove::Other,
        },
        [k, rest @ ..] => match kanji(*k) {
            Some((k, _)) => (k, rest),
            None => return KifMove::Other,
        },
        [] => return KifMove::Other,
    };
    let text: String = rest.iter().collect();
    let promote = text.starts_with('成');
    let drop = text.starts_with('打');
    let from = text.find('(').and_then(|i| {
        let d: Vec<u32> = text[i + 1..].chars().take_while(|&c| c != ')').filter_map(|c| c.to_digit(10)).collect();
        (d.len() == 2).then(|| cell(d[0], d[1])).flatten()
    });
    match (from, drop) {
        (Some(f), false) => KifMove::Move { from: Some(f), to, kind: Some(kind), promote, same },
        (_, true) => KifMove::Move { from: None, to, kind: Some(kind), promote: false, same },
        // No square to come from: the one piece of its kind that can get there, if there is
        // only one on the board of the side to move — else a drop.
        (None, false) => {
            let mine: Vec<usize> = (0..81).filter(|&i| b.cells[i].is_some_and(|p| p.sente == b.sente && p.kind == kind)).collect();
            match mine.as_slice() {
                [f] => KifMove::Move { from: Some(*f), to, kind: Some(kind), promote, same },
                _ => KifMove::Move { from: None, to, kind: Some(kind), promote: false, same },
            }
        }
    }
}

/// A KIF board diagram (BOD): nine lines `|v香v桂 ・…|`, Gote's pieces after `v`, and the
/// pieces in hand (`歩二 香`).
fn bod_board(rows: &[String], hand: &[Option<String>; 2], first_gote: bool) -> Option<Board> {
    if rows.len() < 9 {
        return None;
    }
    let mut b = Board { cells: [None; 81], hands: [[0; 7]; 2], sente: !first_gote };
    for (y, row) in rows.iter().take(9).enumerate() {
        let c: Vec<char> = row.trim_matches('|').chars().take_while(|&c| c != '|').collect();
        let mut x = 0;
        let mut i = 0;
        while i + 1 < c.len() + 1 && x < 9 && i < c.len() {
            let (gote, k) = (c[i] == 'v', c.get(i + 1).copied());
            if let Some((kind, promoted)) = k.and_then(kanji) {
                b.cells[x + y * 9] = Some(Piece { kind, sente: !gote, promoted });
            }
            x += 1;
            i += 2;
        }
    }
    for (side, h) in hand.iter().enumerate() {
        for item in h.as_deref().unwrap_or("").split(['\u{3000}', ' ']) {
            let mut ch = item.chars();
            let Some((kind, false)) = ch.next().and_then(kanji) else { continue };
            let Some(k) = KINDS[..7].iter().position(|&x| x == kind) else { continue };
            let n = number(ch.as_str()).unwrap_or(1).min(18) as u8;
            b.hands[side][k] = n;
        }
    }
    Some(b)
}

/// A CSA piece code: `FU KY KE GI KI KA HI OU`, promoted `TO NY NK NG UM RY`.
fn csa_piece(s: &str) -> Option<(char, bool)> {
    Some(match s {
        "FU" => ('P', false),
        "KY" => ('L', false),
        "KE" => ('N', false),
        "GI" => ('S', false),
        "KI" => ('G', false),
        "KA" => ('B', false),
        "HI" => ('R', false),
        "OU" => ('K', false),
        "TO" => ('P', true),
        "NY" => ('L', true),
        "NK" => ('N', true),
        "NG" => ('S', true),
        "UM" => ('B', true),
        "RY" => ('R', true),
        _ => return None,
    })
}

/// A CSA game read and played out (one game; `split_csa` cuts a file at its `/` lines).
fn csa(text: &str) -> Game {
    let mut r = Reading::new();
    let mut at: Option<u32> = None;
    let mut setup: Option<Board> = None;
    let mut stopped = false;
    for l in lines(text) {
        // Several statements may share a line, split by commas (not in comments).
        let parts: Vec<String> = if l.starts_with('\'') { vec![l.clone()] } else { l.split(',').map(str::to_string).collect() };
        for s in parts {
            let s = s.trim_end();
            if let Some(c) = s.strip_prefix('\'') {
                r.comment(at, c.trim_start_matches('*'));
            } else if let Some(n) = s.strip_prefix("N+") {
                r.tag("White", n);
            } else if let Some(n) = s.strip_prefix("N-") {
                r.tag("Black", n);
            } else if let Some(v) = s.strip_prefix("$EVENT:") {
                r.tag("Event", v);
            } else if let Some(v) = s.strip_prefix("$SITE:") {
                r.tag("Site", v);
            } else if let Some(v) = s.strip_prefix("$START_TIME:") {
                r.tag("Date", v.split_whitespace().next().unwrap_or(v));
            } else if let Some(v) = s.strip_prefix("$OPENING:") {
                r.tag("Opening", v);
            } else if let Some(rest) = s.strip_prefix("PI") {
                // The start position, less the pieces named: `PI82HI22KA`.
                let mut b = Board::from_sfen(START).expect("the start position");
                let c: Vec<char> = rest.chars().collect();
                for p in c.chunks(4) {
                    let f: String = p.iter().take(2).collect();
                    if let (Some(fl), Some(rk)) = (f.chars().next().and_then(|c| c.to_digit(10)), f.chars().nth(1).and_then(|c| c.to_digit(10))) {
                        if let Some(i) = cell(fl, rk) {
                            b.cells[i] = None;
                        }
                    }
                }
                setup = Some(b);
            } else if s.len() >= 2 && s.starts_with('P') && s.as_bytes()[1].is_ascii_digit() {
                let y = (s.as_bytes()[1] - b'1') as usize;
                let b = setup.get_or_insert(Board { cells: [None; 81], hands: [[0; 7]; 2], sente: true });
                let c: Vec<char> = s.chars().skip(2).collect();
                for (x, p) in c.chunks(3).take(9).enumerate() {
                    let t: String = p.iter().collect();
                    if y < 9 && t.len() == 3 && (t.starts_with('+') || t.starts_with('-')) {
                        if let Some((kind, promoted)) = csa_piece(&t[1..]) {
                            b.cells[x + y * 9] = Some(Piece { kind, sente: t.starts_with('+'), promoted });
                        }
                    }
                }
            } else if let Some(rest) = s.strip_prefix("P+").or_else(|| s.strip_prefix("P-")) {
                // Pieces in hand (`P+00FU00KA`), or put on the board (`P-5141OU`).
                let sente = s.starts_with("P+");
                let b = setup.get_or_insert(Board { cells: [None; 81], hands: [[0; 7]; 2], sente: true });
                let c: Vec<char> = rest.chars().collect();
                for p in c.chunks(4) {
                    let t: String = p.iter().collect();
                    if t.len() != 4 || !t.is_ascii() {
                        continue;
                    }
                    let Some((kind, promoted)) = csa_piece(&t[2..]) else { continue };
                    if &t[..2] == "00" {
                        if let Some(k) = KINDS[..7].iter().position(|&x| x == kind) {
                            let h = &mut b.hands[(!sente) as usize][k];
                            *h = h.saturating_add(1).min(18);
                        }
                    } else if let (Some(f), Some(rk)) = (t[..1].parse().ok(), t[1..2].parse().ok()) {
                        if let Some(i) = cell(f, rk) {
                            b.cells[i] = Some(Piece { kind, sente, promoted });
                        }
                    }
                }
            } else if s == "+" || s == "-" {
                if r.g.nodes.is_empty() {
                    let mut b = setup.clone().unwrap_or_else(|| Board::from_sfen(START).expect("the start position"));
                    b.sente = s == "+";
                    r.set_start(b);
                }
            } else if s.len() == 7 && (s.starts_with('+') || s.starts_with('-')) && s.is_ascii() {
                if r.g.nodes.is_empty() && at.is_none() {
                    if let Some(b) = setup.take() {
                        let sente = r.start.sente;
                        r.set_start(Board { sente, ..b });
                    }
                }
                if stopped {
                    continue;
                }
                let d: Vec<u32> = s[1..5].chars().filter_map(|c| c.to_digit(10)).collect();
                let piece = csa_piece(&s[5..]);
                let ok = d.len() == 4 && piece.is_some();
                let to = if ok { cell(d[2], d[3]) } else { None };
                let from = if ok && d[0] == 0 && d[1] == 0 { None } else if ok { cell(d[0], d[1]) } else { None };
                let played = to.and_then(|to| {
                    let (kind, promoted) = piece?;
                    let before = r.board(at);
                    let was = from.and_then(|f| before.cells[f]);
                    let promote = promoted && was.is_some_and(|p| !p.promoted);
                    let same = at.is_some_and(|a| r.g.nodes[a as usize].to == Some(to as u8));
                    r.add(at, from, to, Some(kind), promote, same)
                });
                match played {
                    Some(id) => at = Some(id),
                    None => {
                        if r.g.error.is_none() {
                            r.g.error = Some(format!("{} {s}", r.g.nodes.len() + 1));
                        }
                        stopped = true;
                    }
                }
            } else if let Some(end) = s.strip_prefix('%') {
                if r.g.result.is_empty() {
                    let sente_to_move = r.board(at).sente;
                    r.g.result = match end {
                        "TORYO" | "TSUMI" | "ILLEGAL_MOVE" | "TIME_UP" => if sente_to_move { "0-1" } else { "1-0" },
                        "+ILLEGAL_ACTION" => "0-1",
                        "-ILLEGAL_ACTION" => "1-0",
                        "KACHI" => if sente_to_move { "1-0" } else { "0-1" },
                        "SENNICHITE" | "JISHOGI" | "HIKIWAKE" => "1/2-1/2",
                        _ => "*",
                    }
                    .to_string();
                }
                stopped = true;
            }
        }
    }
    if r.g.nodes.is_empty() {
        if let Some(b) = setup {
            let sente = r.start.sente;
            r.set_start(Board { sente, ..b });
        }
    }
    if r.g.result.is_empty() {
        r.g.result = "*".into();
    }
    r.g
}

/// A shogi game's text, read and played out. `Node::san` is the move in Japanese (`▲７六歩`),
/// `number` the move's count from the start, `fen` the position in SFEN with the hands, `from`
/// and `to` cells (`x + y × 9`, file 9 on the left, rank 1 at the top; no `from` for a drop).
pub fn parse(text: &str, format: Format) -> Game {
    if format == Format::Csa { csa(text) } else { kif(text) }
}

/// The games of a file's bytes, each as its byte range: CSA files can hold several, cut at a
/// line of `/` (a byte Shift_JIS never uses inside a character); a KIF file is one game.
pub fn split(b: &[u8], format: Format) -> Vec<(usize, usize)> {
    if format != Format::Csa {
        return vec![(0, b.len())];
    }
    let blank = |r: &[u8]| r.iter().all(|c| c.is_ascii_whitespace());
    let mut out = vec![];
    let (mut start, mut at) = (0, 0);
    for l in b.split_inclusive(|&c| c == b'\n') {
        if l.trim_ascii() == b"/" {
            if !blank(&b[start..at]) {
                out.push((start, at));
            }
            start = at + l.len();
            if out.len() >= MAX_GAMES {
                return out;
            }
        }
        at += l.len();
    }
    if !blank(&b[start..]) {
        out.push((start, b.len()));
    }
    out
}

/// The games of a shogi file's bytes, each as its range and its game read in full.
pub(crate) fn games(b: &[u8], format: Format) -> impl Iterator<Item = ((usize, usize), Game)> + '_ {
    split(b, format).into_iter().map(move |(a, z)| ((a, z), parse(&decode(&b[a..z]), format)))
}

/// The shogi format of a file name: `Kif` or `Csa`.
pub fn format(name: &str) -> Option<Format> {
    let n = name.to_ascii_lowercase();
    if n.ends_with(".kif") || n.ends_with(".kifu") {
        Some(Format::Kif)
    } else if n.ends_with(".csa") {
        Some(Format::Csa)
    } else {
        None
    }
}

/// The pieces on a board from its SFEN, rows from the top, and the hands (Sente's, Gote's)
/// as (kanji, count). A SFEN that is no position gives an empty board.
pub fn view(sfen: &str) -> (Vec<Option<Piece>>, [Vec<(char, u8)>; 2]) {
    let b = Board::from_sfen(sfen).unwrap_or(Board { cells: [None; 81], hands: [[0; 7]; 2], sente: true });
    let hands = [0, 1].map(|s| {
        (0..7).filter(|&k| b.hands[s][k] > 0).map(|k| (glyph(Piece { kind: KINDS[k], sente: true, promoted: false }), b.hands[s][k])).collect()
    });
    (b.cells.to_vec(), hands)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn last(g: &Game) -> String {
        let m = g.main_line();
        m.last().map_or(g.start.clone(), |&n| g.nodes[n as usize].fen.clone().unwrap_or_default())
    }

    const KIF: &str = "# ---- Kifu for Windows ----\n開始日時：2020/01/02 10:00\n棋戦：テスト\n手合割：平手\n先手：羽生\n後手：佐藤\n手数----指手---------消費時間--\n   1 ７六歩(77)   ( 0:01/00:00:01)\n   2 ３四歩(33)   ( 0:01/00:00:01)\n*角道を開ける\n   3 ２二角成(88)   ( 0:01/00:00:02)+\n   4 同　銀(31)   ( 0:01/00:00:02)\n   5 ４五角打   ( 0:01/00:00:03)\n   6 投了\nまで5手で先手の勝ち\n\n変化：3手\n   3 ２六歩(27)\n   4 ８四歩(83)\n";

    #[test]
    fn a_kif_game() {
        let g = parse(KIF, Format::Kif);
        assert_eq!(g.error, None, "{:?}", g.nodes.iter().map(|n| &n.san).collect::<Vec<_>>());
        assert_eq!(g.tag("White"), Some("羽生"));
        assert_eq!(g.tag("Date"), Some("2020/01/02"));
        assert_eq!(g.result, "1-0");
        let main = g.main_line();
        let sans: Vec<&str> = main.iter().map(|&n| g.nodes[n as usize].san.as_str()).collect();
        assert_eq!(sans, ["▲７六歩", "△３四歩", "▲２二角成", "△同\u{3000}銀", "▲４五角打"]);
        assert_eq!(g.nodes[main[1] as usize].after, "角道を開ける");
        // Sente took a bishop, Gote took it back: each has one in hand, Sente dropped it again.
        assert!(last(&g).ends_with(" w b"), "{}", last(&g));
        let (cells, hands) = view(&last(&g));
        assert_eq!(cells[cell(4, 5).unwrap()].map(glyph), Some('角'));
        assert_eq!(hands, [vec![], vec![('角', 1)]]);
        // The variation at move 3: two moves, from the position after move 2.
        let v = g.nodes.iter().find(|n| n.san == "▲２六歩").unwrap();
        assert_eq!((v.parent, v.depth, v.number), (Some(main[1]), 1, 3));
        assert_eq!(g.nodes[v.children[0] as usize].san, "△８四歩");
    }

    #[test]
    fn a_csa_file_of_two_games() {
        let text = "V2.2\nN+Habu\nN-Sato\n$EVENT:Test\nPI\n+\n+7776FU\nT1\n-3334FU\n+8822UM\n-3122GI\n+0045KA\n%TORYO\n/\nN+A\nN-B\nPI82HI22KA\n-\n-3334FU,T3\n+7776FU\n'** a comment\n%SENNICHITE\n";
        let games = split(text.as_bytes(), Format::Csa);
        assert_eq!(games.len(), 2);
        let g = parse(&text[games[0].0..games[0].1], Format::Csa);
        assert_eq!((g.error.clone(), g.result.as_str(), g.tag("White")), (None, "1-0", Some("Habu")));
        let k = parse(KIF, Format::Kif);
        assert_eq!(last(&g), k.nodes[k.main_line()[4] as usize].fen.clone().unwrap());
        let g = parse(&text[games[1].0..games[1].1], Format::Csa);
        assert_eq!((g.error.clone(), g.result.as_str()), (None, "1/2-1/2"));
        assert!(g.start.starts_with("lnsgkgsnl/7b1/") || g.start.starts_with("lnsgkgsnl/9/"), "{}", g.start);
        assert!(g.start.contains(" w "));
        assert_eq!(g.nodes[1].after, "a comment");
    }

    #[test]
    fn handicaps_and_diagrams() {
        let g = parse("手合割：二枚落ち\n手数----指手--\n   1 ６二銀(71)\n", Format::Kif);
        assert_eq!(g.error, None);
        assert!(g.start.starts_with("lnsgkgsnl/9/") && g.start.contains(" w "), "{}", g.start);
        let bod = "後手の持駒：なし\n  ９ ８ ７ ６ ５ ４ ３ ２ １\n+---------------------------+\n|v香v桂 ・ ・ ・ ・ ・v桂v香|一\n| ・ ・ ・ ・ ・ ・ ・ ・ ・|二\n| ・ ・ ・ ・v玉 ・ ・ ・ ・|三\n| ・ ・ ・ ・ ・ ・ ・ ・ ・|四\n| ・ ・ ・ ・ ・ ・ ・ ・ ・|五\n| ・ ・ ・ ・ ・ ・ ・ ・ ・|六\n| ・ ・ ・ ・ ・ ・ ・ ・ ・|七\n| ・ ・ ・ ・ ・ ・ ・ ・ ・|八\n| 香 桂 ・ ・ 玉 ・ ・ 桂 香|九\n+---------------------------+\n先手の持駒：金二　歩三\n手数----指手--\n   1 ５二金打\n";
        let g = parse(bod, Format::Kif);
        assert_eq!(g.error, None);
        let (cells, hands) = view(&g.start);
        assert_eq!(cells[cell(5, 3).unwrap()].map(|p| (glyph(p), p.sente)), Some(('王', false)));
        assert_eq!(hands[0], vec![('金', 2), ('歩', 3)]);
        assert_eq!(number("十二"), Some(12));
        assert_eq!(number("１８"), Some(18));
    }

    #[test]
    fn whatever_the_text() {
        for t in ["", "変化：", "変化：999999999999手\n 1 ７六歩", "   1 同　歩", "   1 ０〇歩(00)", "   1 ７六歩(7", "   1 ９九龍打", "|v|v|v\n|", "手合割：ｘ", "先手の持駒：歩九九九九九九九九九九九九九", "\u{3000}\u{3000}\u{3000}"] {
            let g = parse(t, Format::Kif);
            for n in &g.nodes {
                let _ = n.fen.as_deref().map(view);
            }
        }
        for t in ["", "+", "P9", "P1+FU+FU+FU+FU+FU+FU+FU+FU+FU+FU+FU", "P+00FU00FU00XX", "P-99", "+0000FU", "+9999OU", "-7776FU", "+7776FUé", "PI99999999", "P+é00FU", "%TORYO\n+7776FU", "/\n/\n/"] {
            for (a, b) in split(t.as_bytes(), Format::Csa) {
                let g = parse(&t[a..b], Format::Csa);
                for n in &g.nodes {
                    let _ = n.fen.as_deref().map(view);
                }
            }
        }
        // A file of a million variations, and one of more moves than are read: no stack, capped.
        let many = format!("   1 ７六歩(77)\n{}", "変化：1手\n   1 ７六歩(77)\n".repeat(200_000));
        let g = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || parse(&many, Format::Kif)).unwrap().join().unwrap();
        assert!(g.nodes.len() <= MAX_BRANCHES + 1);
        let long = format!("PI\n+\n{}", "+5958OU\n-5152OU\n+5859OU\n-5251OU\n".repeat(crate::chess::pgn::MAX_MOVES));
        assert_eq!(parse(&long, Format::Csa).nodes.len(), crate::chess::pgn::MAX_MOVES);
        // Thousands of games: listed up to MAX_GAMES.
        let games = "+7776FU\n/\n".repeat(MAX_GAMES + 10);
        assert_eq!(split(games.as_bytes(), Format::Csa).len(), MAX_GAMES);
    }
}

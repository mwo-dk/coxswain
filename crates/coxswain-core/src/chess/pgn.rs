//! One PGN game read in full: its tags, and its moves as a tree (the main line and every
//! variation), each move played on a board so that every position can be shown. Any bytes at
//! all are read without a panic: a move that cannot be played ends the positions of its line
//! (the moves after it are still listed), and what is not PGN is skipped.

use cozy_chess::{Board, Color, File, Move, Piece, Rank, Square};
use serde::Serialize;
use super::lex::{Movetext, Tok};

/// How deep variations may nest, and how many moves a game may have with its variations:
/// far more than any real game, and what a made-up file cannot turn into gigabytes.
pub const MAX_DEPTH: u32 = 64;
pub const MAX_MOVES: usize = 50_000;
/// The longest tag value kept.
pub const MAX_TAG: usize = 4096;

/// The start position, in FEN.
pub const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// A move of the game, or of a variation.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Node {
    /// The move as the file writes it, in English SAN, without its `!?` (they are `nags`).
    pub san: String,
    /// The move before it; `None` for a first move.
    pub parent: Option<u32>,
    /// The moves after it: the first continues its line, the others are variations.
    pub children: Vec<u32>,
    /// The position after it, in FEN; `None` once a move of its line could not be played.
    pub fen: Option<String>,
    /// The squares it moved from and to (a1 = 0, h8 = 63), for the highlight.
    pub from: Option<u8>,
    pub to: Option<u8>,
    /// Annotation glyphs: `$1` … `$255`, and `!`, `?`, `!!`, `??`, `!?`, `?!` as 1 … 6.
    pub nags: Vec<u8>,
    /// Comments before and after it.
    pub before: String,
    pub after: String,
    /// White's move (else Black's), and its move number.
    pub white: bool,
    pub number: u32,
    /// 0 on the main line, 1 in a variation, 2 in a variation of one, …
    pub depth: u32,
}

/// A game read in full.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Game {
    pub tags: Vec<(String, String)>,
    /// The position it starts from (the FEN tag, else the start position).
    pub start: String,
    /// A comment before the first move.
    pub comment: String,
    pub nodes: Vec<Node>,
    /// The first moves: the game's, then other first moves (variations of it).
    pub roots: Vec<u32>,
    /// `1-0`, `0-1`, `1/2-1/2` or `*`.
    pub result: String,
    /// The first move that could not be played: "12… Nxe4".
    pub error: Option<String>,
}

impl Game {
    pub fn tag(&self, name: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The main line, first move to last.
    pub fn main_line(&self) -> Vec<u32> {
        let mut v = vec![];
        let mut at = self.roots.first().copied();
        while let Some(n) = at {
            v.push(n);
            at = self.nodes[n as usize].children.first().copied();
        }
        v
    }

    /// The moves from the first to `node`, in order; empty for none (the start position).
    pub fn path_to(&self, node: Option<u32>) -> Vec<u32> {
        let mut v = vec![];
        let mut at = node.filter(|&n| (n as usize) < self.nodes.len());
        while let Some(n) = at {
            v.push(n);
            at = self.nodes[n as usize].parent;
        }
        v.reverse();
        v
    }

    /// The moves as they are written out: the main line with each variation in brackets after
    /// the move it replaces, comments where they stand, move numbers where needed.
    pub fn layout(&self) -> Vec<Token> {
        let mut out = vec![];
        if !self.comment.is_empty() {
            out.push(Token::Comment { text: self.comment.clone() });
        }
        self.line(&self.roots, 0, &mut out);
        if !self.result.is_empty() {
            out.push(Token::Result { text: self.result.clone() });
        }
        out
    }

    /// The line that starts with `first[0]`, its alternatives `first[1..]` after its first move.
    /// Without recursion: a file can nest variations as deep as it likes.
    fn line(&self, first: &[u32], depth: u32, out: &mut Vec<Token>) {
        struct Frame<'a> {
            /// The move to write next, and the variations that replace it.
            next: Option<u32>,
            alts: &'a [u32],
            /// The variations of the move just written, still to write.
            pending: &'a [u32],
            depth: u32,
            /// Black's move gets its number at the start of a line and after anything else.
            numbered: bool,
        }
        let Some((&start, alts)) = first.split_first() else { return };
        let mut stack = vec![Frame { next: Some(start), alts, pending: &[], depth, numbered: true }];
        while let Some(f) = stack.last_mut() {
            if let Some((&alt, rest)) = f.pending.split_first() {
                f.pending = rest;
                f.numbered = true;
                let d = f.depth + 1;
                out.push(Token::Open { depth: d });
                stack.push(Frame { next: Some(alt), alts: &[], pending: &[], depth: d, numbered: true });
                continue;
            }
            let Some(at) = f.next else {
                let d = f.depth;
                stack.pop();
                if !stack.is_empty() {
                    out.push(Token::Close { depth: d });
                }
                continue;
            };
            let n = &self.nodes[at as usize];
            if !n.before.is_empty() {
                out.push(Token::Comment { text: n.before.clone() });
                f.numbered = true;
            }
            let number = if n.white {
                Some(format!("{}.", n.number))
            } else if f.numbered {
                Some(format!("{}…", n.number))
            } else {
                None
            };
            out.push(Token::Move { node: at, number });
            f.numbered = false;
            if !n.after.is_empty() {
                out.push(Token::Comment { text: n.after.clone() });
                f.numbered = true;
            }
            f.pending = f.alts;
            match n.children.split_first() {
                Some((&next, rest)) => (f.next, f.alts) = (Some(next), rest),
                None => (f.next, f.alts) = (None, &[]),
            }
        }
    }
}

/// A piece of the written-out moves.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Token {
    /// A move, with its number when it is written: "12." or "12…".
    Move { node: u32, number: Option<String> },
    Comment { text: String },
    /// A variation begins or ends, at this depth.
    Open { depth: u32 },
    Close { depth: u32 },
    Result { text: String },
}

/// The annotation `!`, `?`, `!!`, `??`, `!?` or `?!` as its NAG number.
pub(crate) fn suffix_nag(s: &str) -> Option<u8> {
    Some(match s {
        "!" => 1,
        "?" => 2,
        "!!" => 3,
        "??" => 4,
        "!?" => 5,
        "?!" => 6,
        _ => return None,
    })
}

/// A tag line's name and value: `[White "Morphy, Paul"]`.
pub fn tag_pair(line: &str) -> Option<(String, String)> {
    let s = line.trim().strip_prefix('[')?;
    let s = s.trim_start();
    let name_end = s.find(|c: char| c.is_whitespace() || c == '"')?;
    let name = &s[..name_end];
    if name.is_empty() {
        return None;
    }
    let rest = s[name_end..].trim_start().strip_prefix('"')?;
    let mut value = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => value.extend(chars.next()),
            '"' => return Some((name.chars().take(64).collect(), value)),
            _ if value.len() >= MAX_TAG => {}
            c => value.push(c),
        }
    }
    // No closing quote: the value runs to the end, less a closing bracket.
    let name = name.chars().take(64).collect();
    Some((name, value.trim_end().trim_end_matches(']').trim_end_matches('"').to_string()))
}

/// A word of the movetext, sorted.
#[derive(Debug, PartialEq)]
pub(crate) enum Word<'a> {
    /// A move in SAN, and the annotation glued to it (`!?`).
    Move(&'a str, Option<u8>),
    Result(&'a str),
    /// A move number, or anything else that is no move.
    Other,
}

/// Is it a result?
pub(crate) fn is_result(w: &str) -> bool {
    matches!(w, "1-0" | "0-1" | "1/2-1/2" | "½-½" | "*")
}

/// A word, with a move number glued to its front taken off ("12.e4", "12...Nf6").
pub(crate) fn word(w: &str) -> Word<'_> {
    if is_result(w) {
        return Word::Result(w);
    }
    let mut w = w;
    if w.starts_with(|c: char| c.is_ascii_digit()) && !w.starts_with("0-0") {
        let digits = w.find(|c: char| !c.is_ascii_digit()).unwrap_or(w.len());
        let rest = &w[digits..];
        let dots = rest.trim_start_matches(['.', '…']);
        if dots.len() == rest.len() {
            return Word::Other;
        }
        w = dots;
    }
    if w.is_empty() {
        return Word::Other;
    }
    let body = w.trim_end_matches(['!', '?']);
    let nag = suffix_nag(&w[body.len()..]);
    let first = body.chars().next().unwrap_or(' ');
    if body.len() >= 2 && ("KQRBNPOo0abcdefgh".contains(first) || body == "--" || body == "Z0") {
        Word::Move(body, nag)
    } else {
        Word::Other
    }
}

/// The FEN with its move counters, which an EPD line and some FEN tags leave out.
pub fn full_fen(fen: &str) -> String {
    let fields: Vec<&str> = fen.split_whitespace().collect();
    match fields.len() {
        0 => String::new(),
        1 => format!("{} w - - 0 1", fields[0]),
        2 => format!("{} {} - - 0 1", fields[0], fields[1]),
        3 => format!("{} {} {} - 0 1", fields[0], fields[1], fields[2]),
        4 => format!("{} 0 1", fields[..4].join(" ")),
        5 => format!("{} 1", fields[..5].join(" ")),
        _ => fields[..6].join(" "),
    }
}

/// A position from FEN, as lenient as can be: the counters may be missing, castling rights the
/// pieces do not allow are dropped, and an en passant square that cannot be taken too.
pub fn board(fen: &str) -> Option<Board> {
    let full = full_fen(fen);
    if let Ok(b) = Board::from_fen(&full, false) {
        return Some(b);
    }
    let mut f: Vec<&str> = full.split(' ').collect();
    if f.len() == 6 {
        f[3] = "-";
        if let Ok(b) = Board::from_fen(&f.join(" "), false) {
            return Some(b);
        }
        f[2] = "-";
        if let Ok(b) = Board::from_fen(&f.join(" "), false) {
            return Some(b);
        }
        // Shredder-FEN's castling (HAha), for Chess960.
        let f: Vec<&str> = full.split(' ').collect();
        return Board::from_fen(&f.join(" "), true).ok();
    }
    None
}

fn piece_of(c: char) -> Option<Piece> {
    Some(match c {
        'K' => Piece::King,
        'Q' => Piece::Queen,
        'R' => Piece::Rook,
        'B' => Piece::Bishop,
        'N' => Piece::Knight,
        'P' => Piece::Pawn,
        _ => return None,
    })
}

/// The move `san` names on `b`: SAN as written by hand too (`0-0`, `e2-e4`, `Ng1f3`, `e8Q`, a
/// missing `x`). None when no legal move fits, or more than one does.
pub fn san_move(b: &Board, san: &str) -> Option<Move> {
    let s: String = san
        .chars()
        .filter(|c| !matches!(c, '+' | '#' | 'x' | ':' | '-' | '=' | '!' | '?'))
        .collect();
    let castle = san.trim_end_matches(['+', '#', '!', '?']);
    let short = matches!(castle, "O-O" | "0-0" | "o-o");
    let long = matches!(castle, "O-O-O" | "0-0-0" | "o-o-o");
    let mut found = None;
    let mut count = 0;
    let king = b.king(b.side_to_move());
    if short || long {
        b.generate_moves_for(b.pieces(Piece::King), |moves| {
            for mv in moves {
                // cozy-chess castles as the king taking its own rook.
                if b.color_on(mv.to) == Some(b.side_to_move()) && mv.from == king {
                    let kingside = mv.to.file() > mv.from.file();
                    if kingside == short {
                        found = Some(mv);
                        count += 1;
                    }
                }
            }
            false
        });
        return (count == 1).then_some(found).flatten();
    }
    let mut chars: Vec<char> = s.chars().collect();
    // The promotion piece at the end: e8Q (from e8=Q).
    let mut promotion = None;
    if chars.len() >= 3
        && let Some(p) = chars.last().and_then(|&c| piece_of(c.to_ascii_uppercase()))
        && chars[chars.len() - 2].is_ascii_digit()
        && !matches!(p, Piece::King | Piece::Pawn)
    {
        promotion = Some(p);
        chars.pop();
    }
    let role = match chars.first() {
        Some(&c) if c.is_ascii_uppercase() => {
            chars.remove(0);
            piece_of(c)?
        }
        _ => Piece::Pawn,
    };
    if chars.len() < 2 {
        return None;
    }
    let (f, r) = (chars[chars.len() - 2], chars[chars.len() - 1]);
    if !('a'..='h').contains(&f) || !('1'..='8').contains(&r) {
        return None;
    }
    let to_file = File::try_index(f as usize - 'a' as usize)?;
    let to_rank = Rank::try_index(r as usize - '1' as usize)?;
    let to = Square::new(to_file, to_rank);
    let mut from_file = None;
    let mut from_rank = None;
    for &c in &chars[..chars.len() - 2] {
        if c.is_ascii_lowercase() && ('a'..='h').contains(&c) {
            from_file = File::try_index(c as usize - 'a' as usize);
        } else if ('1'..='8').contains(&c) {
            from_rank = Rank::try_index(c as usize - '1' as usize);
        } else {
            return None;
        }
    }
    b.generate_moves(|moves| {
        if moves.piece != role {
            return false;
        }
        for mv in moves {
            // A king never takes its own rook here: that is castling, named above.
            if mv.to != to || b.color_on(mv.to) == Some(b.side_to_move()) {
                continue;
            }
            if from_file.is_some_and(|f| mv.from.file() != f) || from_rank.is_some_and(|r| mv.from.rank() != r) {
                continue;
            }
            let promo = mv.promotion;
            if role == Piece::Pawn && promo.is_some() && promo != promotion.or(Some(Piece::Queen)) {
                continue;
            }
            if promotion.is_some() && promo != promotion {
                continue;
            }
            if found != Some(mv) {
                count += 1;
            }
            found = Some(mv);
        }
        false
    });
    (count == 1).then_some(found).flatten()
}

/// A legal move on `b` in English SAN, as a book writes it: `Nbd2`, `exd5`, `O-O`, `e8=Q+`.
pub fn san(b: &Board, mv: Move) -> String {
    let piece = b.piece_on(mv.from).unwrap_or(Piece::Pawn);
    let mut out = String::new();
    if piece == Piece::King && b.color_on(mv.to) == b.color_on(mv.from) {
        out.push_str(if mv.to.file() > mv.from.file() { "O-O" } else { "O-O-O" });
    } else {
        let capture = b.color_on(mv.to).is_some() || (piece == Piece::Pawn && mv.from.file() != mv.to.file());
        if piece == Piece::Pawn {
            if capture {
                out.push(char::from(b'a' + mv.from.file() as u8));
            }
        } else {
            out.push(match piece {
                Piece::Knight => 'N',
                Piece::Bishop => 'B',
                Piece::Rook => 'R',
                Piece::Queen => 'Q',
                _ => 'K',
            });
            // Other pieces of its kind that reach the square: its file, else its rank, else both.
            let mut others = vec![];
            b.generate_moves_for(b.colored_pieces(b.side_to_move(), piece), |moves| {
                for m in moves {
                    if m.to == mv.to && m.from != mv.from {
                        others.push(m.from);
                    }
                }
                false
            });
            if !others.is_empty() {
                let file = !others.iter().any(|s| s.file() == mv.from.file());
                let rank = !others.iter().any(|s| s.rank() == mv.from.rank());
                if file || !rank {
                    out.push(char::from(b'a' + mv.from.file() as u8));
                }
                if !file {
                    out.push(char::from(b'1' + mv.from.rank() as u8));
                }
            }
        }
        if capture {
            out.push('x');
        }
        out.push_str(&mv.to.to_string());
        if let Some(p) = mv.promotion {
            out.push('=');
            out.push(match p {
                Piece::Knight => 'N',
                Piece::Bishop => 'B',
                Piece::Rook => 'R',
                _ => 'Q',
            });
        }
    }
    let mut after = b.clone();
    after.play_unchecked(mv);
    if !after.checkers().is_empty() {
        let mut any = false;
        after.generate_moves(|_| {
            any = true;
            true
        });
        out.push(if any { '+' } else { '#' });
    }
    out
}

/// The square a move ends on as the eye sees it: castling goes to the king's square, not its
/// rook's (cozy-chess's notation).
pub(crate) fn shown_squares(b: &Board, mv: Move) -> (u8, u8) {
    let to = if b.piece_on(mv.from) == Some(Piece::King) && b.color_on(mv.to) == b.color_on(mv.from) {
        let file = if mv.to.file() > mv.from.file() { File::G } else { File::C };
        Square::new(file, mv.from.rank())
    } else {
        mv.to
    };
    (mv.from as u8, to as u8)
}

/// What a game of the PGN family needs to be read and played out: chess here, draughts in
/// `crate::draughts`. The rest (tags, comments, variations, NAGs, results) is the same.
pub(crate) trait Rules {
    type Board: Clone;
    /// The position the game starts from, by its tags (a FEN tag) and its first moves, and its
    /// FEN; the start position when the tags say nothing.
    fn start(&self, tags: &[(String, String)], moves: &str) -> (Self::Board, String);
    /// A word of the movetext, sorted.
    fn word<'a>(&self, w: &'a str) -> Word<'a>;
    /// Whose move it is on `b`: the first player's (White in chess) or not, and its number.
    fn turn(&self, b: &Self::Board) -> (bool, u32);
    /// The move `mv` played on `b`: the board after it, its FEN, and the squares it went from and
    /// to (for the highlight). None when it cannot be played there.
    fn play(&self, b: &Self::Board, mv: &str) -> Option<(Self::Board, String, Option<u8>, Option<u8>)>;
}

/// Chess, by cozy-chess.
pub(crate) struct Chess;

impl Rules for Chess {
    type Board = Board;

    fn start(&self, tags: &[(String, String)], _: &str) -> (Board, String) {
        tags.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("FEN"))
            .and_then(|(_, f)| board(f).map(|b| (b, full_fen(f))))
            .unwrap_or_else(|| (Board::default(), START.to_string()))
    }

    fn word<'a>(&self, w: &'a str) -> Word<'a> {
        word(w)
    }

    fn turn(&self, b: &Board) -> (bool, u32) {
        (b.side_to_move() == Color::White, b.fullmove_number() as u32)
    }

    fn play(&self, b: &Board, san: &str) -> Option<(Board, String, Option<u8>, Option<u8>)> {
        // A null move (`--`, or `Z0`): the turn passes.
        if san == "--" || san == "Z0" {
            let nb = b.null_move()?;
            return Some((nb.clone(), nb.to_string(), None, None));
        }
        let mv = san_move(b, san)?;
        let (from, to) = shown_squares(b, mv);
        let mut nb = b.clone();
        nb.play_unchecked(mv);
        Some((nb.clone(), nb.to_string(), Some(from), Some(to)))
    }
}

/// The game `text` (its tags and moves), read and played out.
pub fn parse(text: &str) -> Game {
    parse_with(text, &Chess)
}

/// A game of the PGN family, read and played out by `rules`. Without recursion, and with no
/// more than `MAX_MOVES` moves and `MAX_DEPTH` levels of variations, whatever the text.
pub(crate) fn parse_with<R: Rules>(text: &str, rules: &R) -> Game {
    let mut g = Game::default();
    // The tags: every `[` line before the moves.
    let mut rest = text;
    loop {
        let t = rest.trim_start_matches(['\u{feff}']).trim_start();
        if !t.starts_with('[') {
            rest = t;
            break;
        }
        let end = t.find('\n').unwrap_or(t.len());
        if let Some(pair) = tag_pair(&t[..end]) {
            g.tags.push(pair);
        }
        rest = &t[end..];
        if rest.is_empty() {
            break;
        }
    }
    let (first, start) = rules.start(&g.tags, rest);
    g.start = start;
    // Per node: its board, when its line could be played so far.
    let mut boards: Vec<Option<R::Board>> = vec![];
    // The move the next one follows (None: the start), and the variations open around it.
    let mut cur: Option<u32> = None;
    let mut stack: Vec<Option<u32>> = vec![];
    let mut before = String::new();
    // The last token was a move: a comment now is about it.
    let mut after_move = false;
    let mut depth = 0u32;
    // Inside a variation nested too deep: how many brackets are open there.
    let mut skip = 0u32;
    // The moves of a line with a move that could not be played have no boards: this is the
    // board before the next move, or None.
    let board_at = |at: Option<u32>, boards: &[Option<R::Board>]| match at {
        None => Some(first.clone()),
        Some(n) => boards[n as usize].clone(),
    };
    for tok in Movetext::new(rest, false) {
        match tok {
            // Variations nested deeper than any game needs are skipped, whole.
            Tok::Open if skip > 0 || depth >= MAX_DEPTH => skip += 1,
            Tok::Close if skip > 0 => skip -= 1,
            _ if skip > 0 => {}
            Tok::Comment(c) => {
                let c = c.split_whitespace().collect::<Vec<_>>().join(" ");
                if c.is_empty() {
                    continue;
                }
                let target = match cur {
                    Some(n) if after_move => &mut g.nodes[n as usize].after,
                    _ if g.nodes.is_empty() && depth == 0 => &mut g.comment,
                    _ => &mut before,
                };
                if !target.is_empty() {
                    target.push(' ');
                }
                target.push_str(&c);
            }
            Tok::Nag(n) => {
                if let Some(at) = cur {
                    g.nodes[at as usize].nags.push(n);
                }
            }
            Tok::Open => {
                // A variation replaces the last move: it starts from the position before it.
                stack.push(cur);
                cur = cur.and_then(|n| g.nodes[n as usize].parent);
                depth += 1;
                after_move = false;
            }
            Tok::Close => {
                if let Some(back) = stack.pop() {
                    cur = back;
                    depth -= 1;
                }
                after_move = false;
            }
            Tok::Word(w) => match rules.word(w) {
                Word::Result(r) => {
                    if depth == 0 {
                        g.result = if r == "½-½" { "1/2-1/2".into() } else { r.to_string() };
                    }
                }
                Word::Other => {}
                Word::Move(san, nag) => {
                    if g.nodes.len() >= MAX_MOVES {
                        break;
                    }
                    let id = g.nodes.len() as u32;
                    let prev = board_at(cur, &boards);
                    let (white, number) = match prev.as_ref() {
                        Some(b) => rules.turn(b),
                        None => {
                            // Counted on from the move before.
                            let p = cur.map(|n| &g.nodes[n as usize]);
                            let white = p.is_none_or(|p| !p.white);
                            let number = p.map_or(1, |p| if p.white { p.number } else { p.number + 1 });
                            (white, number)
                        }
                    };
                    let mut node = Node {
                        // A null move is shown as `--`.
                        san: if san == "Z0" { "--".into() } else { san.to_string() },
                        parent: cur,
                        white,
                        number,
                        depth,
                        before: std::mem::take(&mut before),
                        nags: nag.into_iter().collect(),
                        ..Node::default()
                    };
                    let mut after = None;
                    if let Some(b) = prev.as_ref() {
                        match rules.play(b, san) {
                            Some((nb, fen, from, to)) => {
                                node.from = from;
                                node.to = to;
                                node.fen = Some(fen);
                                after = Some(nb);
                            }
                            None => {
                                if g.error.is_none() {
                                    let n = if white { format!("{number}.") } else { format!("{number}…") };
                                    g.error = Some(format!("{n}{san}"));
                                }
                            }
                        }
                    }
                    g.nodes.push(node);
                    boards.push(after);
                    match cur {
                        Some(p) => g.nodes[p as usize].children.push(id),
                        None => g.roots.push(id),
                    }
                    cur = Some(id);
                    after_move = true;
                }
            },
        }
    }
    if g.result.is_empty() {
        g.result = g.tag("Result").unwrap_or("*").to_string();
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPERA: &str = r#"[Event "Paris"]
[Site "Paris FRA"]
[Date "1858.??.??"]
[White "Paul Morphy"]
[Black "Duke Karl / Count Isouard"]
[Result "1-0"]
[ECO "C41"]

1.e4 e5 2.Nf3 d6 3.d4 Bg4 {This is a weak move already.} 4.dxe5 Bxf3 5.Qxf3 dxe5
6.Bc4 Nf6 7.Qb3 Qe7 8.Nc3 c6 9.Bg5 b5 $2 10.Nxb5! cxb5 11.Bxb5+ Nbd7 12.O-O-O Rd8
13.Rxd7 Rxd7 14.Rd1 Qe6 15.Bxd7+ Nxd7 (15...Qxd7 16.Qb8+ Ke7 17.Qxe5+ (17.Bxf6+ gxf6) 17...Kd8) 16.Qb8+ Nxb8 17.Rd8# 1-0
"#;

    #[test]
    fn the_opera_game_plays_out() {
        let g = parse(OPERA);
        assert_eq!(g.tag("white"), Some("Paul Morphy"));
        assert_eq!(g.error, None);
        let main = g.main_line();
        assert_eq!(main.len(), 33);
        let last = &g.nodes[*main.last().unwrap() as usize];
        assert_eq!(last.san, "Rd8#");
        assert!(last.fen.as_deref().unwrap().starts_with("1n1Rkb1r/p4ppp/4q3/4p1B1/4P3/8/PPP2PPP/2K5 b k"));
        // 9…b5 $2 and 10.Nxb5!
        assert_eq!(g.nodes[main[17] as usize].nags, vec![2]);
        assert_eq!(g.nodes[main[18] as usize].nags, vec![1]);
        assert_eq!(g.nodes[main[5] as usize].after, "This is a weak move already.");
        // Castling long: e1 to c1 as shown.
        let castle = &g.nodes[main[22] as usize];
        assert_eq!((castle.san.as_str(), castle.from, castle.to), ("O-O-O", Some(4), Some(2)));
        // 15…Nxd7 has the variation 15…Qxd7, with one of its own.
        let nxd7 = &g.nodes[main[29] as usize];
        let parent = &g.nodes[nxd7.parent.unwrap() as usize];
        assert_eq!(parent.children.len(), 2);
        let qxd7 = &g.nodes[parent.children[1] as usize];
        assert_eq!((qxd7.san.as_str(), qxd7.depth, qxd7.white), ("Qxd7", 1, false));
        assert!(g.nodes.iter().any(|n| n.san == "Bxf6+" && n.depth == 2 && n.fen.is_some()));
        assert_eq!(g.result, "1-0");
        // Written out: numbers where needed, the variations in brackets.
        let words: Vec<String> = g
            .layout()
            .iter()
            .map(|t| match t {
                Token::Move { node, number } => format!("{}{}", number.clone().unwrap_or_default(), g.nodes[*node as usize].san),
                Token::Open { .. } => "(".into(),
                Token::Close { .. } => ")".into(),
                Token::Comment { text } => format!("{{{text}}}"),
                Token::Result { text } => text.clone(),
            })
            .collect();
        let s = words.join(" ");
        assert!(s.starts_with("1.e4 e5 2.Nf3 d6 3.d4 Bg4 {This is a weak move already.} 4.dxe5"), "{s}");
        assert!(s.contains("15.Bxd7+ Nxd7 ( 15…Qxd7 16.Qb8+ Ke7 17.Qxe5+ ( 17.Bxf6+ gxf6 ) 17…Kd8 ) 16.Qb8+"), "{s}");
        assert!(s.ends_with("17.Rd8# 1-0"));
    }

    #[test]
    fn san_as_people_write_it() {
        let b = Board::default();
        for s in ["e4", "e2-e4", "e2e4", "Pe4"] {
            assert!(san_move(&b, s).is_some(), "{s}");
        }
        assert!(san_move(&b, "Nf3").is_some() && san_move(&b, "Ng1f3").is_some() && san_move(&b, "Ng1-f3").is_some());
        assert!(san_move(&b, "Nc3").is_some() && san_move(&b, "Nd2").is_none());
        assert!(san_move(&b, "e5").is_none() && san_move(&b, "Ke2").is_none() && san_move(&b, "O-O").is_none());
        // Two knights reach d2: SAN says which.
        let b = board("4k3/8/8/8/8/8/8/1N2KN2 w - - 0 1").unwrap();
        assert!(san_move(&b, "Nd2").is_none());
        assert!(san_move(&b, "Nbd2").is_some() && san_move(&b, "Nfd2").is_some());
        // Promotion, with and without `=`, and castling with zeros.
        let b = board("8/P5k1/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        assert_eq!(san_move(&b, "a8=N").and_then(|m| m.promotion), Some(Piece::Knight));
        assert_eq!(san_move(&b, "a8Q").and_then(|m| m.promotion), Some(Piece::Queen));
        assert!(san_move(&b, "0-0").is_some() && san_move(&b, "O-O-O+").is_some());
    }

    #[test]
    fn nothing_breaks_it() {
        // Whatever the bytes, a game comes back.
        for text in [
            "", "[", "[White", "[White \"", "{", "(", ")", "))((", "1.", "1-0", "$", "$999", "1.e4 (", "1.e4 ) e5",
            "1.Qh5 Qh4 2.e4", "1. e4 e5 2. Nxe5?? {unclosed", "%escape\n1.e4", "\u{feff}[A \"b\"]\n1.e4 *",
            "1.e4 ((e5) d5)", "( 1.e4 ) 1.d4", "1.♘f3", "1.e4 e5 2.Ke2 Ke7 3.Ke1 Ke8 4.O-O",
        ] {
            let g = parse(text);
            let _ = g.layout();
        }
        let g = parse("1.e4 e5 2.Qh5 Qh4 3.Qxf7 Kxf7 4.Qxh7");
        assert_eq!(g.error.as_deref(), Some("4.Qxh7"));
        // The move after one that could not be played is still listed, without a position.
        assert_eq!(g.main_line().len(), 7);
        assert!(g.nodes[6].fen.is_none());
        // A game from a set-up position.
        let g = parse("[SetUp \"1\"]\n[FEN \"4k3/8/8/8/8/8/4P3/4K3 b - - 0 40\"]\n40...Kd7 41.e4 *");
        assert_eq!(g.error, None);
        assert_eq!((g.nodes[0].white, g.nodes[0].number, g.nodes[1].number), (false, 40, 41));
    }

    #[test]
    fn a_null_move_passes_the_turn() {
        for null in ["--", "Z0"] {
            let g = parse(&format!("1.e4 {null} 2.d4 d5 *"));
            assert_eq!(g.error, None, "{null}");
            assert_eq!(g.main_line().len(), 4);
            assert_eq!(g.nodes[1].san, "--");
            assert!(g.nodes[3].fen.is_some(), "the replay goes on");
        }
        // In check a turn cannot pass.
        assert!(parse("1.f3 e5 2.g4 Qh4+ 3.-- *").error.is_some());
    }

    #[test]
    fn deep_variations_and_odd_moves() {
        // 30,000 variations nested in each other: read and written out on a small stack.
        let mut text = String::from("1.e4 e5 ");
        for _ in 0..30_000 {
            text.push_str("( 1...c5 ");
        }
        text.push_str(&")".repeat(30_000));
        text.push_str(" 2.Nf3 *");
        std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(move || {
                let g = parse(&text);
                assert_eq!(g.main_line().len(), 3);
                let depth = g.nodes.iter().map(|n| n.depth).max().unwrap();
                assert_eq!(depth, MAX_DEPTH);
                let l = g.layout();
                assert_eq!(l.iter().filter(|t| matches!(t, Token::Open { .. })).count(), MAX_DEPTH as usize);
                assert_eq!(l.iter().filter(|t| matches!(t, Token::Close { .. })).count(), MAX_DEPTH as usize);
            })
            .unwrap()
            .join()
            .unwrap();
        // Moves no board has: no panic, just no move.
        let b = Board::default();
        for s in ["KQ1", "aA1", "e9", "Nz3", "Q@4", "N~3", "A1", "bxc0", "é4", "N1"] {
            assert!(san_move(&b, s).is_none(), "{s}");
        }
        let g = parse("1.KQ1 2.aA1 *");
        assert_eq!(g.error.as_deref(), Some("1.KQ1"));
        // So many moves: the game stops at MAX_MOVES.
        let g = parse(&"Nf3 Nf6 Ng1 Ng8 ".repeat(MAX_MOVES / 2));
        assert_eq!(g.nodes.len(), MAX_MOVES);
        // A tag value no longer than MAX_TAG.
        let long = format!("[White \"{}\"]", "x".repeat(100_000));
        assert_eq!(tag_pair(&long).unwrap().1.len(), MAX_TAG);
    }

    #[test]
    fn moves_written_in_san() {
        let all = |fen: &str| -> Vec<String> {
            let b = board(fen).unwrap();
            let mut v = vec![];
            b.generate_moves(|ms| {
                for m in ms {
                    v.push(san(&b, m));
                }
                false
            });
            // Every move written reads back as itself.
            for s in &v {
                let m = san_move(&b, s).unwrap_or_else(|| panic!("{s} on {fen}"));
                assert_eq!(&san(&b, m), s);
            }
            v
        };
        assert!(all(START).contains(&"Nf3".to_string()));
        let v = all("4k3/8/8/8/8/8/8/1N2KN2 w - - 0 1");
        assert!(v.contains(&"Nbd2".into()) && v.contains(&"Nfd2".into()));
        let v = all("8/P5k1/8/8/8/8/8/R3K2R w KQ - 0 1");
        assert!(v.contains(&"a8=Q".into()) && v.contains(&"O-O".into()) && v.contains(&"O-O-O".into()));
        let v = all("r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 0 1");
        assert!(v.contains(&"Qxf7#".into()));
        let v = all("rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3");
        assert!(v.contains(&"exf6".into()));
        // Rooks on one file: the rank tells them apart.
        let v = all("4k3/8/R7/8/8/8/R7/4K3 w - - 0 1");
        assert!(v.contains(&"R6a4".into()) && v.contains(&"R2a4".into()));
    }

    #[test]
    fn tags_with_escapes() {
        assert_eq!(tag_pair(r#"[Event "The \"Big\" One"]"#), Some(("Event".into(), "The \"Big\" One".into())));
        assert_eq!(tag_pair("[Site \"Paris]"), Some(("Site".into(), "Paris".into())));
        assert_eq!(tag_pair("[ ]"), None);
    }
}

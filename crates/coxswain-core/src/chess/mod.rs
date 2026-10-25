//! Chess games and positions (docs/previews/chess.md), for both apps: PGN files (also
//! `.pgn.gz` and `.pgn.zst`) as a table of games, each game as a board with its moves and
//! variations; EPD and FEN files as their positions. A position can be played on (`play`), in
//! the view only, and an installed engine asked about it. Moves are shown in figurines (♘f3) or in the letters of the app's language; the
//! files keep English SAN.

pub mod cbh;
pub mod cbv;
pub mod games;
pub(crate) mod lex;
pub mod openings;
pub mod pgn;
pub mod play;
pub mod review;
pub mod scid;
pub mod polyglot;

pub use games::{Library, Progress, Query, Row, Sort};
pub use pgn::{Game, Node, Token};

use serde::Serialize;
use std::path::Path;

use crate::config::Theme;

/// What kind of chess file a name is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// Games: `.pgn`, `.pgn.gz`, `.pgn.zst`, a SCID database's `.si4` or `.si5` (`scid`), and a ChessBase
    /// database's `.cbh` (`cbh`), each read as the PGN it holds.
    Pgn,
    /// Positions, one a line: `.epd`, `.fen`.
    Epd,
    /// Go games: `.sgf` (`crate::go`), shown by the same action.
    Sgf,
    /// A Polyglot opening book (`polyglot`): a `.bin` file that is one, by its bytes.
    Book,
    /// Draughts games: `.pdn` (`crate::draughts`), in the same table as PGN games.
    Pdn,
    /// Xiangqi games: a PGN file whose tags say so (`games::kind`, `crate::xiangqi`).
    Xiangqi,
    /// Shogi games (`crate::shogi`): KIF (`.kif`, `.kifu`) and CSA (`.csa`).
    Kif,
    Csa,
    /// Bridge deals: `.pbn` (`crate::bridge`).
    Pbn,
}

/// The chess file `path` is, by its name.
pub fn format(path: &Path) -> Option<Format> {
    let n = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    // A SCID database is shown by its index, as a PGN file is (`scid`).
    if [".pgn", ".pgn.gz", ".pgn.zst", ".si4", ".si5", ".cbh", ".cbv"].iter().any(|e| n.ends_with(e)) {
        Some(Format::Pgn)
    } else if n.ends_with(".epd") || n.ends_with(".fen") {
        Some(Format::Epd)
    } else if n.ends_with(".sgf") {
        Some(Format::Sgf)
    } else if n.ends_with(".pdn") {
        Some(Format::Pdn)
    } else if n.ends_with(".pbn") {
        Some(Format::Pbn)
    } else if let Some(f) = crate::shogi::format(&n) {
        Some(f)
    } else {
        None
    }
}

/// The games file `path` is: `format`, and for a `.bin` file, whether its bytes are a
/// Polyglot book's. It reads the file, so it is for one file the user is on, not a listing.
pub fn sniff(path: &Path) -> Option<Format> {
    format(path).or_else(|| polyglot::is_book(path).then_some(Format::Book))
}

/// The file endings of PGN files, for the search store.
pub const PGN_ENDINGS: [&str; 3] = [".pgn", ".pgn.gz", ".pgn.zst"];

/// A position of an EPD or FEN file.
#[derive(Clone, Debug, Serialize)]
pub struct Position {
    /// Its line in the file, from 1.
    pub line: usize,
    /// The full FEN (`pgn::full_fen`); `None` when the line is no position.
    pub fen: Option<String>,
    /// The rest of an EPD line: its operations (`bm Nf3; id "WAC.001";`).
    pub about: String,
}

/// The most positions read from one file.
pub const MAX_POSITIONS: usize = 100_000;

/// The positions of the EPD or FEN file at `path`, from its first 16 MB.
pub fn read_positions(path: &Path) -> std::io::Result<Vec<Position>> {
    use std::io::Read;
    let mut bytes = vec![];
    std::fs::File::open(path)?.take(games::MAX_GAME as u64).read_to_end(&mut bytes)?;
    Ok(positions(&String::from_utf8_lossy(&bytes)))
}

/// The positions of an EPD or FEN file: each line that is not empty.
pub fn positions(text: &str) -> Vec<Position> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .take(MAX_POSITIONS)
        .map(|(i, l)| {
            let fields: Vec<&str> = l.split_whitespace().collect();
            // FEN has six fields (counters last), EPD four and then operations.
            let counters = fields.len() >= 6 && fields[4].parse::<u32>().is_ok() && fields[5].parse::<u32>().is_ok();
            let n = if counters { 6 } else { fields.len().min(4) };
            let fen = fields[..n].join(" ");
            let ok = pgn::board(&fen).map(|b| b.to_string());
            Position { line: i + 1, fen: ok, about: fields[n..].join(" ") }
        })
        .collect()
}

/// How moves are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notation {
    /// ♔ ♕ ♖ ♗ ♘ (the default).
    Figurines,
    /// The app language's letters (`chess.letters`): K D T L S in German.
    Letters,
}

impl Notation {
    pub fn from_config(s: &str) -> Notation {
        if s == "letters" { Notation::Letters } else { Notation::Figurines }
    }
}

/// The letters of King, Queen, Rook, Bishop and Knight in `notation`.
pub fn pieces(notation: Notation) -> [String; 5] {
    let mut out = ["♔", "♕", "♖", "♗", "♘"].map(String::from);
    if notation == Notation::Letters {
        let letters = crate::t!("chess.letters");
        let mut it = letters.split_whitespace();
        for (o, d) in out.iter_mut().zip(["K", "Q", "R", "B", "N"]) {
            *o = it.next().unwrap_or(d).to_string();
        }
    }
    out
}

/// A move as shown: its piece letters (and a promotion's) in `pieces` (`pieces()`), the rest
/// as in the file. `Nf3` is `♘f3`, `e8=Q+` is `e8=♕+`.
pub fn show_san(san: &str, pieces: &[String; 5]) -> String {
    let mut out = String::with_capacity(san.len() + 4);
    for (i, c) in san.chars().enumerate() {
        let k = match c {
            'K' => 0,
            'Q' => 1,
            'R' => 2,
            'B' => 3,
            'N' => 4,
            // A pawn's own letter, which some files write, is not shown.
            'P' if i == 0 => continue,
            c => {
                out.push(c);
                continue;
            }
        };
        out.push_str(&pieces[k]);
    }
    out
}

/// A NAG as its symbol: `$1` is `!`, `$14` is `⩲`. One without a symbol is shown as `$n`.
pub fn nag(n: u8) -> String {
    let s = match n {
        1 => "!",
        2 => "?",
        3 => "‼",
        4 => "⁇",
        5 => "⁉",
        6 => "⁈",
        7 => "□",
        10..=12 => "=",
        13 => "∞",
        14 => "⩲",
        15 => "⩱",
        16 => "±",
        17 => "∓",
        18 | 20 => "+−",
        19 | 21 => "−+",
        22 | 23 => "⨀",
        32 | 33 => "⟳",
        36 | 37 => "↑",
        40 | 41 => "→",
        44 | 45 => "=∞",
        132 | 133 => "⇆",
        138 | 139 => "⊕",
        140 => "∆",
        142 => "⌓",
        145 => "RR",
        146 => "N",
        _ => return format!("${n}"),
    };
    s.to_string()
}

/// The NAGs of a move as shown after it: move marks (`!?`) glued on, the rest after a space.
pub fn nags(list: &[u8]) -> (String, String) {
    let mut glued = String::new();
    let mut apart = vec![];
    for &n in list {
        if (1..=6).contains(&n) {
            // ‼ and friends as two plain characters, which every font has.
            glued.push_str(["!", "?", "!!", "??", "!?", "?!"][n as usize - 1]);
        } else {
            apart.push(nag(n));
        }
    }
    (glued, apart.join(" "))
}

/// The board's colours, light square then dark, as `#rrggbb`: the theme's (`board` = `theme`),
/// or a tournament board (`green`, `brown`).
pub fn board_colours(theme: &Theme, board: &str) -> [String; 2] {
    match board {
        "green" => return ["#eeeed2".into(), "#769656".into()],
        "brown" => return ["#f0d9b5".into(), "#b58863".into()],
        _ => {}
    }
    // The theme's own colour: its panel's background when that has a colour (Classic blue),
    // else its cursor's, else its accent.
    let rgb = |c: &str| hex(c);
    let tinted = [rgb(&theme.panel.bg), rgb(&theme.cursor.bg), rgb(&theme.accent.bg), rgb(&theme.accent.fg)]
        .into_iter()
        .flatten()
        .find(|&c| hsl(c).1 > 0.25);
    let (h, s) = tinted.map_or((30.0, 0.3), |c| {
        let (h, s, _) = hsl(c);
        (h, s.min(0.7))
    });
    [css(from_hsl(h, s * 0.6, 0.82)), css(from_hsl(h, s * 0.7, 0.45))]
}

fn hex(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

fn css((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn hsl((r, g, b): (u8, u8, u8)) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

/// The pieces on a board, from FEN: 64 squares from a1 to h8, each a piece letter (`K` White's
/// king, `k` Black's) or `None`.
pub fn squares(fen: &str) -> [Option<char>; 64] {
    let mut out = [None; 64];
    let placement = fen.split_whitespace().next().unwrap_or("");
    for (r, row) in placement.split('/').take(8).enumerate() {
        let rank = 7 - r;
        let mut file = 0usize;
        for c in row.chars() {
            if let Some(d) = c.to_digit(10) {
                file += d as usize;
            } else if file < 8 {
                out[rank * 8 + file] = Some(c);
                file += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_as_shown() {
        let fig = pieces(Notation::Figurines);
        assert_eq!(show_san("Nf3", &fig), "♘f3");
        assert_eq!(show_san("Bxb5+", &fig), "♗xb5+");
        assert_eq!(show_san("e8=Q#", &fig), "e8=♕#");
        assert_eq!(show_san("O-O", &fig), "O-O");
        assert_eq!(show_san("Pe4", &fig), "e4");
        let de = ["K", "D", "T", "L", "S"].map(String::from);
        assert_eq!(show_san("Qxd7+", &de), "Dxd7+");
        assert_eq!(nags(&[1, 14]), ("!".into(), "⩲".into()));
        assert_eq!(nag(200), "$200");
    }

    #[test]
    fn names_and_positions() {
        assert_eq!(format(Path::new("/a/Games.PGN.gz")), Some(Format::Pgn));
        assert_eq!(format(Path::new("/a/Club.si4")), Some(Format::Pgn));
        assert_eq!(format(Path::new("/a/Club.si5")), Some(Format::Pgn));
        assert_eq!(format(Path::new("/a/Mega.CBV")), Some(Format::Pgn));
        assert_eq!(format(Path::new("x.fen")), Some(Format::Epd));
        assert_eq!(format(Path::new("x.gz")), None);
        assert_eq!(format(Path::new("Mega.CBH")), Some(Format::Pgn));
        let p = positions("r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - bm Qxf7#; id \"mate\";\n\nnonsense\n8/8/8/8/8/8/8/K6k b - - 3 60\n");
        assert_eq!(p.len(), 3);
        assert!(p[0].fen.is_some() && p[0].about.starts_with("bm Qxf7#"));
        assert!(p[1].fen.is_none());
        assert_eq!(p[2].fen.as_deref(), Some("8/8/8/8/8/8/8/K6k b - - 3 60"));
        let sq = squares(pgn::START);
        assert_eq!((sq[0], sq[4], sq[60], sq[27]), (Some('R'), Some('K'), Some('k'), None));
    }

    #[test]
    fn boards_follow_the_theme() {
        let nc = board_colours(&Theme::nc(), "theme");
        // Classic blue: a blue board.
        let (h, _, _) = hsl(hex(&nc[1]).unwrap());
        assert!((220.0..=250.0).contains(&h), "{nc:?}");
        let (h, _, _) = hsl(hex(&board_colours(&Theme::cyber(), "theme")[1]).unwrap());
        assert!((100.0..=160.0).contains(&h));
        assert_eq!(board_colours(&Theme::nc(), "brown")[0], "#f0d9b5");
    }
}

//! Game pieces drawn from block characters in the terminal app: the desktop app's SVG pieces,
//! made into masks by `tools/mosaic.py` and drawn here at any square size with octants,
//! sextants, quadrants or half blocks, each character cell in two colours.
//!
//! `mosaic/chess.bin` is made from Uray M. János's chess pieces, after Colin M.L. Burnett's
//! Wikimedia set, under CC BY-SA 3.0 (`gui/public/vendor/chess-pieces/LICENSE`).

/// How the terminal app draws a game's pieces: `[chess] pieces` in `config.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pieces {
    /// The SVG pieces themselves, where the terminal shows pictures.
    Pictures,
    /// Pieces drawn from block characters.
    Mosaic(Glyphs),
    /// One character per piece (♚ ♛ …), on a small board.
    Figurines,
}

/// The block characters a mosaic is drawn with, finest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyphs {
    /// Unicode 16's octants, 2 × 4 dots a cell.
    Octants,
    /// Sextants (Unicode 13), 2 × 3.
    Sextants,
    /// Quadrants ▖▗▘▝, 2 × 2.
    Quadrants,
    /// Half blocks ▀▄, 1 × 2: every terminal and font has them.
    Halves,
}

/// The names `[chess] pieces` takes, in Settings' order.
pub const CHOICES: [&str; 7] = [
    "auto",
    "pictures",
    "octants",
    "sextants",
    "quadrants",
    "halves",
    "figurines",
];

impl Pieces {
    /// The setting's value; `auto` (and anything unknown) by the terminal, from `TERM` and
    /// `TERM_PROGRAM`.
    pub fn from_config(value: &str, term: &str, program: &str) -> Pieces {
        match value {
            "octants" => Pieces::Mosaic(Glyphs::Octants),
            "sextants" => Pieces::Mosaic(Glyphs::Sextants),
            "quadrants" => Pieces::Mosaic(Glyphs::Quadrants),
            "halves" => Pieces::Mosaic(Glyphs::Halves),
            "figurines" => Pieces::Figurines,
            // Pictures where the terminal shows them: the terminal app asks it (`pictures`);
            // the mosaic meanwhile.
            _ => Pieces::Mosaic(Glyphs::detect(term, program)),
        }
    }
}

impl Glyphs {
    /// The finest set the terminal is known to draw: terminals that draw block characters
    /// themselves get octants or sextants, the Linux console half blocks, the rest quadrants
    /// (in nearly every monospace font).
    pub fn detect(term: &str, program: &str) -> Glyphs {
        let program = program.to_ascii_lowercase();
        if term == "linux" || term.starts_with("vt") {
            Glyphs::Halves
        } else if term.contains("kitty")
            || term.contains("ghostty")
            || term.starts_with("foot")
            || program == "ghostty"
            || program == "kitty"
        {
            Glyphs::Octants
        } else if program == "wezterm"
            || term.contains("wezterm")
            || term.contains("alacritty")
            || program == "konsole"
        {
            Glyphs::Sextants
        } else {
            Glyphs::Quadrants
        }
    }

    /// Dots across and down one character cell.
    pub fn dots(self) -> (usize, usize) {
        match self {
            Glyphs::Octants => (2, 4),
            Glyphs::Sextants => (2, 3),
            Glyphs::Quadrants => (2, 2),
            Glyphs::Halves => (1, 2),
        }
    }

    /// The fewest rows a square needs for its piece to be told apart from the others (fewer:
    /// figurines read better).
    pub fn min_rows(self) -> u16 {
        match self {
            Glyphs::Halves => 4,
            _ => 3,
        }
    }

    /// The character whose set dots are `bits`, row by row from the top left.
    pub fn char(self, bits: u8) -> char {
        match self {
            Glyphs::Halves => [' ', '▀', '▄', '█'][bits as usize & 3],
            Glyphs::Quadrants => [
                ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
            ][bits as usize & 15],
            Glyphs::Sextants => match bits & 63 {
                0 => ' ',
                21 => '▌',
                42 => '▐',
                63 => '█',
                v => char::from_u32(0x1FB00 + v as u32 - 1 - (v > 21) as u32 - (v > 42) as u32)
                    .unwrap_or('█'),
            },
            Glyphs::Octants => octant(bits),
        }
    }
}

/// The octant patterns Unicode left out because an older character draws them.
const OCTANT_OLD: [(u8, char); 26] = [
    (0, ' '),
    (1, '\u{1CEA8}'),
    (2, '\u{1CEAB}'),
    (3, '\u{1FB82}'),
    (5, '▘'),
    (10, '▝'),
    (15, '▀'),
    (20, '\u{1FBE6}'),
    (40, '\u{1FBE7}'),
    (63, '\u{1FB85}'),
    (64, '\u{1CEA3}'),
    (80, '▖'),
    (85, '▌'),
    (90, '▞'),
    (95, '▛'),
    (128, '\u{1CEA0}'),
    (160, '▗'),
    (165, '▚'),
    (170, '▐'),
    (175, '▜'),
    (192, '▂'),
    (240, '▄'),
    (245, '▙'),
    (250, '▟'),
    (252, '▆'),
    (255, '█'),
];

/// BLOCK OCTANT-… from U+1CD00 in pattern order, skipping the ones above.
fn octant(bits: u8) -> char {
    match OCTANT_OLD.binary_search_by_key(&bits, |&(b, _)| b) {
        Ok(i) => OCTANT_OLD[i].1,
        Err(i) => char::from_u32(0x1CD00 + bits as u32 - i as u32).unwrap_or('█'),
    }
}

/// What a dot shows: the square, or the light or dark part of a piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Square,
    Light,
    Dark,
}

/// One character cell of a square: `ch` drawn in `fg` on `bg`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Tone,
    pub bg: Tone,
}

/// The masks' side, in source pixels (`SIZE` in `tools/mosaic.py`).
const SIZE: usize = 64;

/// A set of pieces: a game's pieces as masks, `names` the letter of each in the file's order,
/// `svgs` the files they are made from (`SETS` in `tools/mosaic.py`), `plain` when its pieces
/// are simple shapes that read on a square a row smaller.
pub struct Set {
    data: &'static [u8],
    names: &'static str,
    pub svgs: &'static [&'static str],
    pub plain: bool,
}

/// The chess pieces (Uray M. János's set, as the desktop app's), white then black.
pub static CHESS: Set = Set {
    data: include_bytes!("mosaic/chess.bin"),
    names: "KQRBNPkqrbnp",
    svgs: &[
        "chess-pieces/wK.svg",
        "chess-pieces/wQ.svg",
        "chess-pieces/wR.svg",
        "chess-pieces/wB.svg",
        "chess-pieces/wN.svg",
        "chess-pieces/wP.svg",
        "chess-pieces/bK.svg",
        "chess-pieces/bQ.svg",
        "chess-pieces/bR.svg",
        "chess-pieces/bB.svg",
        "chess-pieces/bN.svg",
        "chess-pieces/bP.svg",
    ],
    plain: false,
};

/// The draughts men and kings, white then black, as `coxswain_core::draughts::cells` names them.
pub static DRAUGHTS: Set = Set {
    data: include_bytes!("mosaic/draughts.bin"),
    names: "MKmk",
    svgs: &[
        "game-pieces/draughts-wm.svg",
        "game-pieces/draughts-wk.svg",
        "game-pieces/draughts-bm.svg",
        "game-pieces/draughts-bk.svg",
    ],
    plain: true,
};

impl Set {
    /// The FNV-1a hash of the SVG files the masks were made from.
    pub fn made_from(&self) -> u64 {
        u64::from_le_bytes(self.data[..8].try_into().expect("eight bytes"))
    }

    /// Piece `name` drawn on a square of `cols` × `rows` cells (each twice as high as wide),
    /// row by row. `halo` gives a dark piece a light outline, for a dark square.
    pub fn draw(
        &self,
        name: char,
        glyphs: Glyphs,
        cols: usize,
        rows: usize,
        halo: bool,
    ) -> Option<Vec<Cell>> {
        let k = self.names.find(name)?;
        let mask = &self.data[8 + k * SIZE * SIZE..][..SIZE * SIZE];
        let (dx, dy) = glyphs.dots();
        let (w, h) = (cols * dx, rows * dy);
        // The square in units of a cell's width; the piece a little larger than in its SVG,
        // whose box leaves a margin.
        let side = (cols as f32).min(rows as f32 * 2.0);
        let (ox, oy) = ((cols as f32 - side) / 2.0, (rows as f32 * 2.0 - side) / 2.0);
        let zoom = 1.15;
        let mut tones = vec![Tone::Square; w * h];
        const N: usize = 4;
        for j in 0..h {
            for i in 0..w {
                let (mut cover, mut dark) = (0.0, 0.0);
                for sy in 0..N {
                    for sx in 0..N {
                        let x = (i as f32 + (sx as f32 + 0.5) / N as f32) / dx as f32;
                        let y = (j as f32 + (sy as f32 + 0.5) / N as f32) * 2.0 / dy as f32;
                        let u = (((x - ox) / side - 0.5) / zoom + 0.5) * SIZE as f32;
                        let v = (((y - oy) / side - 0.5) / zoom + 0.5) * SIZE as f32;
                        if u < 0.0 || v < 0.0 || u >= SIZE as f32 || v >= SIZE as f32 {
                            continue;
                        }
                        let b = mask[v as usize * SIZE + u as usize];
                        cover += (b >> 4) as f32 / 15.0;
                        dark += (b & 15) as f32 / 15.0;
                    }
                }
                let n = (N * N) as f32;
                let (cover, dark) = (cover / n, dark / n);
                if cover >= 0.45 {
                    tones[j * w + i] = if dark / cover >= 0.5 {
                        Tone::Dark
                    } else {
                        Tone::Light
                    };
                }
            }
        }
        // The outline: dark, but a dark piece's light on a dark square (`halo`).
        let lights = tones.iter().filter(|&&t| t == Tone::Light).count();
        let darks = tones.iter().filter(|&&t| t == Tone::Dark).count();
        let edge = if lights < darks && halo {
            Tone::Light
        } else {
            Tone::Dark
        };
        {
            let out = |i: isize, j: isize| {
                i < 0
                    || j < 0
                    || i >= w as isize
                    || j >= h as isize
                    || tones[j as usize * w + i as usize] == Tone::Square
            };
            let rim: Vec<usize> = (0..w * h)
                .filter(|&p| tones[p] != Tone::Square)
                .filter(|&p| {
                    let (i, j) = ((p % w) as isize, (p / w) as isize);
                    out(i - 1, j) || out(i + 1, j) || out(i, j - 1) || out(i, j + 1)
                })
                .collect();
            for p in rim {
                tones[p] = edge;
            }
        }
        let mut cells = Vec::with_capacity(cols * rows);
        for r in 0..rows {
            for c in 0..cols {
                let dots: Vec<Tone> = (0..dy)
                    .flat_map(|y| (0..dx).map(move |x| (y, x)))
                    .map(|(y, x)| tones[(r * dy + y) * w + c * dx + x])
                    .collect();
                cells.push(cell(glyphs, &dots));
            }
        }
        Some(cells)
    }
}

/// One cell from its dots: the two commonest tones (the square's first on a tie), a dot of the
/// third drawn in the less common of them.
fn cell(glyphs: Glyphs, dots: &[Tone]) -> Cell {
    let count = |t: Tone| dots.iter().filter(|&&d| d == t).count();
    let mut by = [Tone::Square, Tone::Dark, Tone::Light];
    by.sort_by_key(|&t| std::cmp::Reverse(count(t)));
    let (bg, fg) = if count(by[1]) == 0 {
        (by[0], by[0])
    } else if by[1] == Tone::Square {
        (by[1], by[0])
    } else {
        (by[0], by[1])
    };
    let bits = dots
        .iter()
        .enumerate()
        .filter(|&(_, &d)| d != bg)
        .fold(0u8, |b, (k, _)| b | 1 << k);
    Cell {
        ch: glyphs.char(bits),
        fg,
        bg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnv(data: &[u8]) -> u64 {
        data.iter().fold(0xCBF2_9CE4_8422_2325, |h, &b| {
            (h ^ b as u64).wrapping_mul(0x0100_0000_01B3)
        })
    }

    #[test]
    fn the_masks_are_made_from_the_svg_pieces_as_they_are() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gui/public/vendor");
        if !dir.is_dir() {
            return; // the published crate has no desktop app beside it
        }
        for set in [&CHESS, &DRAUGHTS] {
            assert_eq!(set.svgs.len(), set.names.chars().count());
            // Line ends as git checks them out on Windows too.
            let all: Vec<u8> = set
                .svgs
                .iter()
                .flat_map(|f| std::fs::read(dir.join(f)).unwrap())
                .filter(|&b| b != b'\r')
                .collect();
            assert_eq!(
                set.made_from(),
                fnv(&all),
                "{} changed: run python3 tools/mosaic.py",
                set.svgs[0]
            );
        }
    }

    #[test]
    fn glyphs_by_their_dots() {
        assert_eq!(Glyphs::Halves.char(0b01), '▀');
        assert_eq!(Glyphs::Quadrants.char(0b1001), '▚');
        assert_eq!(Glyphs::Sextants.char(1), '\u{1FB00}');
        assert_eq!(Glyphs::Sextants.char(62), '\u{1FB3B}');
        assert_eq!(Glyphs::Octants.char(4), '\u{1CD00}');
        assert_eq!(Glyphs::Octants.char(254), '\u{1CDE5}');
        assert_eq!(Glyphs::Octants.char(0b0000_0011), '\u{1FB82}');
        // Every pattern a character of its own.
        let all: std::collections::HashSet<char> = (0..=255u8).map(octant).collect();
        assert_eq!(all.len(), 256);
    }

    #[test]
    fn a_white_king_has_a_dark_outline_and_a_light_body() {
        let cells = CHESS.draw('K', Glyphs::Halves, 8, 4, false).unwrap();
        assert_eq!(cells.len(), 32);
        let tones = |t: Tone| cells.iter().filter(|c| c.fg == t || c.bg == t).count();
        assert!(tones(Tone::Light) > 0 && tones(Tone::Dark) > 0);
        // The corners are the square's.
        assert_eq!(
            cells[0],
            Cell {
                ch: ' ',
                fg: Tone::Square,
                bg: Tone::Square
            }
        );
        assert!(CHESS.draw('x', Glyphs::Halves, 8, 4, false).is_none());
    }

    #[test]
    fn the_setting() {
        assert_eq!(Pieces::from_config("figurines", "", ""), Pieces::Figurines);
        assert_eq!(
            Pieces::from_config("auto", "xterm-kitty", ""),
            Pieces::Mosaic(Glyphs::Octants)
        );
        assert_eq!(
            Pieces::from_config("auto", "linux", ""),
            Pieces::Mosaic(Glyphs::Halves)
        );
        assert_eq!(
            Pieces::from_config("auto", "xterm-256color", ""),
            Pieces::Mosaic(Glyphs::Quadrants)
        );
    }
}

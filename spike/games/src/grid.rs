//! A game drawn into a character grid with named colours (#863 decision 8, the terminal app).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colour {
    Text,
    Dim,
    Accent,
    LightSquare,
    DarkSquare,
    WhitePiece,
    BlackPiece,
    Highlight,
    Background,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Colour,
    pub bg: Colour,
}

#[derive(Default)]
pub struct Grid {
    fens: Vec<String>,
    sans: Vec<String>,
    tags: Vec<(String, String)>,
    ply: usize,
    flipped: bool,
}

impl Grid {
    pub fn open(&mut self, pgn_text: &[u8]) -> u32 {
        let g = crate::chess::pgn::parse(&crate::chess::games::text(pgn_text));
        self.fens = vec![g.start.clone()];
        self.sans.clear();
        for &n in &g.main_line() {
            let Some(f) = g.nodes[n as usize].fen.clone() else { break };
            self.fens.push(f);
            self.sans.push(g.nodes[n as usize].san.clone());
        }
        self.tags = g.tags.clone();
        self.ply = 0;
        self.sans.len() as u32
    }

    pub fn key(&mut self, name: &str) -> bool {
        let last = self.fens.len().saturating_sub(1);
        let before = (self.ply, self.flipped);
        match name {
            "left" => self.ply = self.ply.saturating_sub(1),
            "right" => self.ply = (self.ply + 1).min(last),
            "home" => self.ply = 0,
            "end" => self.ply = last,
            "f" => self.flipped = !self.flipped,
            _ => {}
        }
        before != (self.ply, self.flipped)
    }

    pub fn draw(&self, w: u16, h: u16) -> Vec<Cell> {
        let (w, h) = (w as usize, h as usize);
        let blank = Cell { ch: ' ', fg: Colour::Text, bg: Colour::Background };
        let mut out = vec![blank; w * h];
        let mut put = |x: usize, y: usize, s: &str, fg: Colour, bg: Colour| {
            for (i, ch) in s.chars().enumerate() {
                if x + i < w && y < h {
                    out[y * w + x + i] = Cell { ch, fg, bg };
                }
            }
        };
        let board = board_of(self.fens.get(self.ply).map_or("", |s| s));
        for r in 0..8 {
            for f in 0..8 {
                let (rank, file) = if self.flipped { (r, 7 - f) } else { (7 - r, f) };
                let sq = board[rank * 8 + file];
                let bg = if (rank + file) % 2 == 1 { Colour::LightSquare } else { Colour::DarkSquare };
                let (ch, fg) = match sq {
                    Some(c) => (glyph(c), if c.is_ascii_uppercase() { Colour::WhitePiece } else { Colour::BlackPiece }),
                    None => (' ', Colour::Text),
                };
                put(2 + f * 3, 1 + r, &format!(" {ch} "), fg, bg);
            }
            put(0, 1 + r, &format!("{}", if self.flipped { r + 1 } else { 8 - r }), Colour::Dim, Colour::Background);
        }
        let files: String = (0..8).map(|f| format!(" {} ", (b'a' + if self.flipped { 7 - f } else { f }) as char)).collect();
        put(2, 9, &files, Colour::Dim, Colour::Background);
        let tag = |k: &str| self.tags.iter().find(|(n, _)| n == k).map_or("?", |(_, v)| v.as_str()).to_string();
        let x = 30;
        put(x, 1, &format!("{} - {}", tag("White"), tag("Black")), Colour::Accent, Colour::Background);
        put(x, 2, &tag("Event"), Colour::Dim, Colour::Background);
        // The moves around the current one, a move pair a line.
        let rows = h.saturating_sub(5);
        let pair = self.ply.saturating_sub(1) / 2;
        let first = pair.saturating_sub(rows / 2);
        for (line, p) in (first..(self.sans.len() + 1) / 2).take(rows).enumerate() {
            let y = 4 + line;
            put(x, y, &format!("{:>3}.", p + 1), Colour::Dim, Colour::Background);
            for side in 0..2 {
                let i = p * 2 + side;
                if let Some(s) = self.sans.get(i) {
                    let bg = if i + 1 == self.ply { Colour::Highlight } else { Colour::Background };
                    put(x + 5 + side * 8, y, s, Colour::Text, bg);
                }
            }
        }
        put(0, h.saturating_sub(1), &format!("ply {}/{}  \u{2190}/\u{2192} move  Home/End  f flip  q quit", self.ply, self.sans.len()), Colour::Dim, Colour::Background);
        out
    }
}

fn glyph(c: char) -> char {
    match c {
        'K' => '♔', 'Q' => '♕', 'R' => '♖', 'B' => '♗', 'N' => '♘', 'P' => '♙',
        'k' => '♚', 'q' => '♛', 'r' => '♜', 'b' => '♝', 'n' => '♞', 'p' => '♟',
        c => c,
    }
}

/// The 64 squares of a FEN's board, a1 first.
fn board_of(fen: &str) -> [Option<char>; 64] {
    let mut b = [None; 64];
    let placement = fen.split(' ').next().unwrap_or("");
    for (r, row) in placement.split('/').enumerate().take(8) {
        let mut f = 0;
        for c in row.chars() {
            if let Some(d) = c.to_digit(10) {
                f += d as usize;
            } else if f < 8 {
                b[(7 - r) * 8 + f] = Some(c);
                f += 1;
            }
        }
    }
    b
}

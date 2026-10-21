//! A game board's squares as the terminal draws them, for every game view: each piece drawn
//! from block characters (coxswain-core's `mosaic`) when its square is large enough, a
//! figurine on a small board otherwise. `[chess] pieces` chooses.

use coxswain_core::mosaic::{Pieces, Set, Tone};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

/// The light and dark parts of a piece.
const LIGHT: Color = Color::Rgb(0xf4, 0xf1, 0xea);
const DARK: Color = Color::Rgb(0x16, 0x16, 0x16);

/// How the pieces are drawn, from the setting and the terminal.
pub fn pieces(cfg: &coxswain_core::config::Config) -> Pieces {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    // `auto` and `pictures`: the SVG pieces themselves where the terminal shows pictures.
    if matches!(cfg.chess.pieces.as_str(), "auto" | "pictures") && crate::pictures::support().is_some() {
        return Pieces::Pictures;
    }
    Pieces::from_config(&cfg.chess.pieces, &env("TERM"), &env("TERM_PROGRAM"))
}

/// A board of `files` × `ranks` squares in `width` × `height` cells: how its pieces are
/// drawn and a square's size in cells. Pieces as pictures or mosaics get the largest square
/// that fits (twice as wide as high, as a cell is twice as high as wide) when it is large
/// enough; otherwise figurines on today's small squares (3 or 5 wide, 1 or 2 high).
pub fn layout(
    pieces: Pieces,
    set: &Set,
    files: u16,
    ranks: u16,
    width: u16,
    height: u16,
) -> (Pieces, u16, u16) {
    let least = match pieces {
        Pieces::Mosaic(g) => g.min_rows() - set.plain as u16,
        Pieces::Pictures => 2,
        Pieces::Figurines => u16::MAX,
    };
    let rows = (height / ranks).min(width / files / 2);
    if rows >= least {
        return (pieces, rows * 2, rows);
    }
    let rows = (height / ranks).clamp(1, 2);
    let mut cols = rows * 2 + 1;
    while cols > 3 && cols * files > width {
        cols -= 2;
    }
    (Pieces::Figurines, cols, rows)
}

/// A piece on a square: its letter in `set`, its figurine and the figurine's colour.
pub struct Piece<'a> {
    pub name: char,
    pub figurine: &'a str,
    pub colour: Color,
}

/// One square, `cols` × `rows` cells in colour `square`: each row's spans.
pub fn square(
    pieces: Pieces,
    set: &Set,
    piece: Option<Piece>,
    cols: u16,
    rows: u16,
    square: Color,
) -> Vec<Vec<Span<'static>>> {
    let blank = || {
        vec![Span::styled(
            " ".repeat(cols as usize),
            Style::default().bg(square),
        )]
    };
    let Some(p) = piece else {
        return (0..rows).map(|_| blank()).collect();
    };
    if let Pieces::Mosaic(g) = pieces
        && let Some(cells) = set.draw(p.name, g, cols as usize, rows as usize, luma(square) < 0.2)
    {
        let tone = |t: Tone| match t {
            Tone::Square => square,
            Tone::Light => LIGHT,
            Tone::Dark => DARK,
        };
        return cells
            .chunks(cols as usize)
            .map(|row| {
                // One span per run of the same colours.
                let mut spans: Vec<Span<'static>> = vec![];
                for c in row {
                    let style = Style::default().fg(tone(c.fg)).bg(tone(c.bg));
                    match spans.last_mut() {
                        Some(s) if s.style == style => s.content.to_mut().push(c.ch),
                        _ => spans.push(Span::styled(c.ch.to_string(), style)),
                    }
                }
                spans
            })
            .collect();
    }
    (0..rows)
        .map(|r| {
            if r == rows / 2 {
                // The rest of an even square's width on the right.
                let left = ((cols - 1) / 2) as usize;
                let (pad, right) = (" ".repeat(left), " ".repeat(cols as usize - 1 - left));
                vec![Span::styled(
                    format!("{pad}{}{right}", p.figurine),
                    Style::default()
                        .bg(square)
                        .fg(p.colour)
                        .add_modifier(Modifier::BOLD),
                )]
            } else {
                blank()
            }
        })
        .collect()
}

/// How light a colour is, 0 to 1 (a colour by name: mid-grey).
fn luma(c: Color) -> f32 {
    match c {
        Color::Rgb(r, g, b) => (r as f32 * 0.299 + g as f32 * 0.587 + b as f32 * 0.114) / 255.0,
        _ => 0.5,
    }
}

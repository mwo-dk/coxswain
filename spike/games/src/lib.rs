//! The plugin spike's workload (#1024): the same code built natively and as a WebAssembly
//! component, so the two can be compared. Coxswain's own PGN lexer and parser and its SCID
//! reader do the reading (`chess`).

pub mod chess;
pub mod grid;

use chess::games::text;
use chess::lex::{Lexer, Movetext, Tok};
use chess::pgn::{self, Word};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub white: String,
    pub black: String,
    pub event: String,
    pub site: String,
    pub date: String,
    pub result: String,
    pub eco: String,
    pub plies: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Job {
    /// The games table from a PGN file: tags and plies counted, as `games::scan` does.
    PgnScan,
    /// Every game of a PGN file read in full, each move played on a board (`pgn::parse`).
    PgnParse,
    /// The games table from a SCID base: its index and names.
    ScidTable,
    /// Every game of a SCID base decoded into PGN, each move played on a board.
    ScidDecode,
}

/// Plies of the main line of a movetext, counted as `games::scan` counts them.
pub fn plies(movetext: &str) -> u32 {
    let (mut n, mut depth) = (0u32, 0u32);
    for line in movetext.lines() {
        if line.starts_with('[') {
            continue;
        }
        for t in Movetext::new(line, false) {
            match t {
                Tok::Open => depth += 1,
                Tok::Close => depth = depth.saturating_sub(1),
                Tok::Word(w) if depth == 0 => {
                    if let Word::Move(..) = pgn::word(w) {
                        n += 1;
                    }
                }
                _ => {}
            }
        }
    }
    n
}

const WANTED: [&str; 7] = ["White", "Black", "Event", "Site", "Date", "Result", "ECO"];

/// The games of a PGN text: each one's byte range and table row, read line by line as
/// `games::scan` reads them (tags, then the moves counted to the end of the game).
pub fn scan(bytes: &[u8], mut each: impl FnMut(u64, u64, Row)) {
    let mut r = Lexer::new(bytes);
    let mut line = Vec::new();
    let mut tags: [String; 7] = Default::default();
    let (mut offset, mut plies) = (0u64, 0u32);
    let (mut has_tags, mut has_moves, mut in_brace, mut depth) = (false, false, false, 0u32);
    loop {
        let at = r.at;
        let n = r.line(&mut line).unwrap_or(0);
        if in_brace && line.starts_with(b"[Event \"") {
            in_brace = false;
        }
        let trimmed = line.trim_ascii_start();
        let is_tag = n > 0 && !in_brace && trimmed.first() == Some(&b'[');
        let new = n == 0 || (is_tag && (has_moves || (trimmed.starts_with(b"[Event ") && !tags[2].is_empty())));
        if new && (has_tags || has_moves) {
            let [white, black, event, site, date, result, eco] = std::mem::take(&mut tags);
            each(offset, at, Row { white, black, event, site, date, result, eco, plies });
            (has_tags, has_moves, in_brace, depth) = (false, false, false, 0);
        }
        if n == 0 {
            return;
        }
        if new || (!has_tags && !has_moves) {
            for t in &mut tags {
                t.clear();
            }
            (offset, plies) = (at, 0);
        }
        if is_tag {
            if let Some((k, v)) = pgn::tag_pair(&text(&line))
                && let Some(i) = WANTED.iter().position(|w| w.eq_ignore_ascii_case(&k))
            {
                tags[i] = v;
            }
            has_tags = true;
        } else if !trimmed.is_empty() && (trimmed[0] != b'%' || in_brace) {
            has_moves = true;
            let line = text(&line);
            let mut toks = Movetext::new(&line, in_brace);
            for tok in toks.by_ref() {
                match tok {
                    Tok::Open => depth = depth.saturating_add(1),
                    Tok::Close => depth = depth.saturating_sub(1),
                    Tok::Word(w) if depth == 0 => {
                        if let Word::Move(..) = pgn::word(w) {
                            plies = plies.saturating_add(1);
                        }
                    }
                    _ => {}
                }
            }
            in_brace = toks.in_brace;
        }
    }
}

/// Cut into about `parts` chunks: byte ranges starting at a game's `[Event` (PGN), or ranges
/// of games (SCID).
pub fn split(job: Job, files: &[&[u8]], parts: u32) -> Vec<(u64, u64)> {
    let parts = parts.max(1) as u64;
    match job {
        Job::PgnScan | Job::PgnParse => {
            let b = files[0];
            let len = b.len() as u64;
            let mut cuts = vec![0u64];
            for k in 1..parts {
                let from = (len * k / parts).max(*cuts.last().unwrap()) as usize;
                let Some(i) = memchr::memmem::find(&b[from..], b"\n[Event ") else { break };
                cuts.push((from + i + 1) as u64);
            }
            cuts.push(len);
            cuts.dedup();
            cuts.windows(2).map(|w| (w[0], w[1])).collect()
        }
        Job::ScidTable | Job::ScidDecode => {
            let n = chess::scid::count_of(files[0]) as u64;
            (0..parts).map(|k| (n * k / parts, n * (k + 1) / parts)).filter(|(a, b)| a < b).collect()
        }
    }
}

/// One chunk into rows. PGN: `files[0]` holds the chunk (or the whole file, `span` cut from it).
pub fn work(job: Job, files: &[&[u8]], span: (u64, u64)) -> Vec<Row> {
    let mut out = Vec::new();
    match job {
        Job::PgnScan => scan(&files[0][span.0 as usize..span.1 as usize], |_, _, r| out.push(r)),
        Job::PgnParse => {
            let b = &files[0][span.0 as usize..span.1 as usize];
            scan(b, |from, to, _| {
                let g = pgn::parse(&text(&b[from as usize..to as usize]));
                let tag = |k| g.tag(k).unwrap_or("").to_string();
                out.push(Row {
                    white: tag("White"),
                    black: tag("Black"),
                    event: tag("Event"),
                    site: tag("Site"),
                    date: tag("Date"),
                    result: g.result.clone(),
                    eco: tag("ECO"),
                    plies: g.main_line().iter().filter(|&&n| g.nodes[n as usize].fen.is_some()).count() as u32,
                });
            })
        }
        Job::ScidTable | Job::ScidDecode => {
            out = chess::scid::rows(files[0], files[1], files[2], span.0, span.1, job == Job::ScidDecode).unwrap_or_default()
        }
    }
    out
}

/// The whole input in one go.
pub fn all(job: Job, files: &[&[u8]]) -> Vec<Row> {
    let end = match job {
        Job::PgnScan | Job::PgnParse => files[0].len() as u64,
        _ => chess::scid::count_of(files[0]) as u64,
    };
    work(job, files, (0, end))
}

/// One game's positions after each move of its main line, the start first, as FEN.
pub fn positions(pgn_text: &[u8]) -> Vec<String> {
    let g = pgn::parse(&text(pgn_text));
    let mut v = vec![g.start.clone()];
    v.extend(g.main_line().iter().map_while(|&n| g.nodes[n as usize].fen.clone()));
    v
}

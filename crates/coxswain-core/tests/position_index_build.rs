//! The position index built of a large games database: the games table read, then the index
//! built at 30 moves, with the time each took, the index's size and the peak memory (`VmHWM`,
//! Linux). Ignored: it needs a database of gigabytes; run it with `--ignored --nocapture`.
//!
//! - `made_up_games`: writes `COXSWAIN_POSITIONS_GAMES` (default 1M) made-up games, legal and
//!   varied (the first moves drawn from few, so openings meet), to the PGN `COXSWAIN_POSITIONS_PGN`.
//! - `build_the_index`: reads the database `COXSWAIN_POSITIONS_DB` (any games table: a PGN,
//!   packed, inside an archive, SCID, ChessBase) and builds its index, in the cache
//!   (`XDG_CACHE_HOME`). Run it twice: the second run takes the table kept by the first, so its
//!   time and memory are the index's alone.

use std::io::{BufWriter, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use coxswain_core::chess::{Library, pgn, positions};
use cozy_chess::Board;

fn peak_mb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines().find(|l| l.starts_with("VmHWM")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()).unwrap_or(0) / 1024
}

/// A plain random number from `s` (xorshift).
fn next(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

/// Game `i`, the same each time.
fn game(i: u64) -> String {
    let mut s = i.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut b = Board::default();
    let plies = 20 + next(&mut s) % 140;
    let mut moves = String::new();
    for ply in 0..plies {
        let mut legal = vec![];
        b.generate_moves(|ms| {
            legal.extend(ms);
            false
        });
        if legal.is_empty() {
            break;
        }
        // The first moves from the first few of the list: games share their openings.
        let pick = if ply < 12 { (next(&mut s) % 3).min(next(&mut s) % 4) as usize % legal.len() } else { (next(&mut s) % legal.len() as u64) as usize };
        let m = legal[pick];
        if ply % 2 == 0 {
            moves.push_str(&format!("{}. ", ply / 2 + 1));
        }
        moves.push_str(&pgn::san(&b, m));
        moves.push(' ');
        b.play_unchecked(m);
    }
    let result = ["1-0", "0-1", "1/2-1/2"][(next(&mut s) % 3) as usize];
    format!(
        "[Event \"Event {}\"]\n[Site \"Site {}\"]\n[Date \"{}.01.01\"]\n[Round \"1\"]\n[White \"Player {}\"]\n[Black \"Player {}\"]\n[Result \"{result}\"]\n[WhiteElo \"{}\"]\n[BlackElo \"{}\"]\n\n{moves}{result}\n\n",
        next(&mut s) % 200,
        next(&mut s) % 50,
        1950 + next(&mut s) % 75,
        next(&mut s) % 1000,
        next(&mut s) % 1000,
        1800 + next(&mut s) % 1000,
        1800 + next(&mut s) % 1000,
    )
}

#[test]
#[ignore]
fn made_up_games() {
    let Some(path) = std::env::var_os("COXSWAIN_POSITIONS_PGN") else { return };
    let games: u64 = std::env::var("COXSWAIN_POSITIONS_GAMES").ok().and_then(|n| n.parse().ok()).unwrap_or(1_000_000);
    let mut out = BufWriter::with_capacity(1 << 20, std::fs::File::create(path).unwrap());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()) as u64;
    let chunk = 8192;
    let mut at = 0;
    while at < games {
        // `threads` chunks made at once, written in order.
        let parts: Vec<String> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..threads)
                .map(|t| {
                    let from = at + t * chunk;
                    s.spawn(move || (from..(from + chunk).min(games)).map(game).collect::<String>())
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for p in parts {
            out.write_all(p.as_bytes()).unwrap();
        }
        at += threads * chunk;
    }
    out.flush().unwrap();
}

#[test]
#[ignore]
fn build_the_index() {
    let Some(path) = std::env::var_os("COXSWAIN_POSITIONS_DB") else { return };
    let t = Instant::now();
    let lib = Arc::new(Library::open(std::path::Path::new(&path)).unwrap());
    while !lib.progress().done {
        std::thread::sleep(Duration::from_millis(50));
    }
    let p = lib.progress();
    println!("table: {} games, kept {}, error {:?}, in {:.1?}, peak {} MB", p.games, p.kept, p.error, t.elapsed(), peak_mb());
    let t = Instant::now();
    positions::build(lib.clone(), 30);
    let s = loop {
        let s = positions::status(&lib, 30);
        if s.ready || s.error.is_some() {
            break s;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    println!("index: {} games, {} MB, error {:?}, in {:.1?}, peak {} MB", s.games, s.bytes >> 20, s.error, t.elapsed(), peak_mb());
}

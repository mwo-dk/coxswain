//! A collection of a million games, made up here: its games table is read to the end in bounded
//! memory, game by game, never the file whole. A test binary of its own, so the peak memory
//! measured (`VmHWM`, Linux) is this test's alone.

use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const GAMES: usize = 1_000_000;

/// Peak resident memory of this process so far, in MB (Linux; None elsewhere).
fn peak_mb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: u64 = s.lines().find(|l| l.starts_with("VmHWM"))?.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

fn write(p: &Path, games: usize, game: impl Fn(usize) -> String) {
    let mut f = BufWriter::new(std::fs::File::create(p).unwrap());
    for i in 0..games {
        f.write_all(game(i).as_bytes()).unwrap();
    }
    f.flush().unwrap();
}

#[test]
fn a_million_games_in_bounded_memory() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("million-games-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pgn = dir.join("million.pgn");
    write(&pgn, GAMES, |i| {
        format!(
            "[White \"Player {}\"]\n[Black \"Player {}\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 {{a comment\n[not a tag]}} Nc6 3. Bb5 1-0\n\n",
            i % 997,
            i % 991
        )
    });
    let size = std::fs::metadata(&pgn).unwrap().len();
    let before = peak_mb();
    let t = Instant::now();
    let lib = coxswain_core::chess::Library::open(&pgn).unwrap();
    let mut first = None;
    while !lib.progress().done {
        if first.is_none() && lib.progress().games > 0 {
            first = Some(t.elapsed());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let p = lib.progress();
    assert_eq!((p.games, p.capped, p.error.as_deref()), (GAMES, false, None));
    assert_eq!(lib.row(GAMES as u32 - 1).unwrap().white, format!("Player {}", (GAMES - 1) % 997));
    assert_eq!(lib.row(0).unwrap().moves, 3);
    // The last game read again from where it starts.
    assert_eq!(lib.game(GAMES as u32 - 1).unwrap().main_line().len(), 5);
    let peak = peak_mb();
    println!(
        "{GAMES} games, {} MB of PGN: first rows after {first:?}, all in {:?}; peak memory {before:?} MB before, {peak:?} MB after",
        size >> 20,
        t.elapsed()
    );
    // The table keeps 48 bytes a game: 48 MB, at most twice that while it grows, plus the
    // names; never the file's size.
    if let Some(peak) = peak {
        assert!(peak < 120, "peak memory {peak} MB for a {} MB file", size >> 20);
    }
    drop(lib);
    std::fs::remove_file(&pgn).unwrap();

    // SGF, a collection past the most listed: read game by game, stopped there.
    let sgf = dir.join("many.sgf");
    let n = coxswain_core::go::MAX_GAMES + 50_000;
    write(&sgf, n, |i| format!("(;GM[1]SZ[19]PB[Black {i}]PW[White]C[{}];B[pd];W[dp](;B[qp])(;B[dd]))\n", "x".repeat(200)));
    let c = coxswain_core::go::Collection::open(&sgf).unwrap();
    c.wait(usize::MAX);
    let p = c.progress();
    assert_eq!((p.games, p.capped, p.error.as_deref()), (coxswain_core::go::MAX_GAMES, true, None));
    assert_eq!(c.info(99_999).unwrap().black, "Black 99999");
    assert_eq!(c.info(0).unwrap().moves, 3);
    assert_eq!(c.game(99_999).unwrap().nodes.len(), 5);
    if let (Some(a), Some(b)) = (peak, peak_mb()) {
        println!("sgf: {n} games, {} MB: peak {b} MB", std::fs::metadata(&sgf).unwrap().len() >> 20);
        assert!(b < a.max(120), "sgf peak {b} MB");
    }
    drop(c);
    std::fs::remove_dir_all(&dir).unwrap();
}

//! A large PGN in a 7z (set `COXSWAIN_PGN_7Z` to `archive.7z/file.pgn`) read straight into the
//! games table: the time, the peak memory (`VmHWM`, Linux) and what the cache holds. Ignored:
//! it needs a database of gigabytes; run it with `--ignored --nocapture`.

use std::time::{Duration, Instant};

fn peak_mb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines().find(|l| l.starts_with("VmHWM")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()).unwrap_or(0) / 1024
}

fn folder_mb(p: &std::path::Path) -> u64 {
    let mut n = 0;
    for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
        let m = e.metadata().unwrap();
        n += if m.is_dir() { folder_mb(&e.path()) << 20 } else { m.len() };
    }
    n >> 20
}

#[test]
#[ignore]
fn a_large_pgn_in_a_7z() {
    let Some(path) = std::env::var_os("COXSWAIN_PGN_7Z") else { return };
    let path = std::path::PathBuf::from(path);
    let t = Instant::now();
    let lib = coxswain_core::chess::Library::open(&path).unwrap();
    let mut first = None;
    while !lib.progress().done {
        if first.is_none() && lib.progress().games > 0 {
            first = Some(t.elapsed());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let p = lib.progress();
    println!("games {} capped {} error {:?} first rows {first:?} done {:?} peak {} MB", p.games, p.capped, p.error, t.elapsed(), peak_mb());
    let last = p.games as u32 - 1;
    for i in [0, last / 2, last] {
        let t = Instant::now();
        let g = lib.text(i).unwrap();
        println!("game {i}: {} bytes in {:?}", g.len(), t.elapsed());
    }
    let cache = std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).unwrap();
    println!("cache {} MB", folder_mb(&cache));
    drop(lib);
    let t = Instant::now();
    let again = coxswain_core::chess::Library::open(&path).unwrap();
    println!("opened again in {:?}, kept {}, games {}", t.elapsed(), again.progress().kept, again.progress().games);
}

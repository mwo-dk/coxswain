//! Benchmarks on synthetic data, ignored by default: `cargo test --release -p coxswain-core
//! --test perf -- --ignored --nocapture`. The numbers are in docs/reference/performance.md.
//!
//! `COXSWAIN_BENCH_DIR` names the folder the data is made in (the temp folder by default);
//! the data stays there, so later runs are quicker.

use coxswain_core::fs::{self, SortKey};
use coxswain_core::index::Index;
use std::path::{Path, PathBuf};
use std::time::Instant;

const EXTS: [&str; 10] = ["txt", "rs", "jpg", "pdf", "md", "toml", "png", "zip", "csv", ""];

/// `n` files and `n / 100` folders, named as real ones are (digits, dots, cases).
fn flat(n: usize) -> PathBuf {
    let d = bench_dir().join(format!("flat-{n}"));
    if d.join(".done").exists() {
        return d;
    }
    std::fs::create_dir_all(&d).unwrap();
    for i in 0..n {
        let ext = EXTS[i % EXTS.len()];
        let name = match i % 4 {
            0 => format!("file_{i:06}.{ext}"),
            1 => format!("IMG_{}.{ext}", 1000 + i),
            2 => format!("Report v{}-{i}.{ext}", i / 7),
            _ => format!("photo {} of {} {i}.{ext}", i % 97, i % 13),
        };
        std::fs::write(d.join(name.trim_end_matches('.')), b"x").unwrap();
    }
    for i in 0..n / 100 {
        std::fs::create_dir(d.join(format!("dir{i}"))).unwrap();
    }
    std::fs::write(d.join(".done"), b"").unwrap();
    d
}

/// A tree of `dirs` folders with `per` files each: `dirs * per` names for the index.
fn tree(dirs: usize, per: usize) -> PathBuf {
    let d = bench_dir().join(format!("tree-{dirs}x{per}"));
    if d.join(".done").exists() {
        return d;
    }
    for i in 0..dirs {
        let sub = d.join(format!("project{}", i / 100)).join(format!("src{i}"));
        std::fs::create_dir_all(&sub).unwrap();
        for j in 0..per {
            std::fs::write(sub.join(format!("mod_{i}_{j}.{}", EXTS[j % 9])), b"").unwrap();
        }
    }
    std::fs::write(d.join(".done"), b"").unwrap();
    d
}

fn bench_dir() -> PathBuf {
    std::env::var_os("COXSWAIN_BENCH_DIR").map_or_else(|| std::env::temp_dir().join("coxswain-bench"), PathBuf::from)
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/statm").ok().and_then(|s| s.split(' ').nth(1)?.parse::<f64>().ok()).map_or(0.0, |pages| pages * 4096.0 / 1e6)
}

#[test]
#[ignore]
fn perf_list_and_sort_100k() {
    let d = flat(100_000);
    let before = rss_mb();
    let t = Instant::now();
    let mut v = fs::list(&d, true).unwrap();
    println!("list 100k: {:.0} ms ({} entries, +{:.0} MB)", ms(t), v.len(), rss_mb() - before);
    let t = Instant::now();
    let v2 = fs::list(&d, true).unwrap();
    println!("list 100k again (cache warm): {:.0} ms", ms(t));
    drop(v2);
    for key in [SortKey::Name, SortKey::Ext, SortKey::Time, SortKey::Size] {
        let t = Instant::now();
        fs::sort(&mut v, key, false);
        println!("sort 100k by {key:?}: {:.0} ms", ms(t));
    }
    // As the desktop app sends it: every entry as JSON.
    let t = Instant::now();
    let json = serde_json::to_vec(&v).unwrap();
    println!("entries to JSON: {:.0} ms, {:.1} MB", ms(t), json.len() as f64 / 1e6);
    let t = Instant::now();
    let size = fs::dir_size(&d);
    println!("dir_size of 100k: {:.0} ms ({size:?})", ms(t));
}

#[test]
#[ignore]
fn perf_find_file_1m() {
    let d = tree(1000, 1000);
    let before = rss_mb();
    let t = Instant::now();
    let ix = Index::build(&[d.clone()], &[]);
    println!("index build 1M names: {:.0} ms ({} names, +{:.0} MB)", ms(t), ix.len(), rss_mb() - before);
    for q in ["mod_500_7", "*.rs", "mod 12 3", "src99/ toml", "!mod", "ext:csv", "zzz"] {
        let t = Instant::now();
        let r = ix.search(q, None, 500);
        println!("search {q:?}: {:.1} ms, {} hits of {}", ms(t), r.hits.len(), r.total);
        let r = ix.search(q, Some(&d.join("project3")), 500);
        println!("search {q:?} in project3: {:.1} ms, {} of {}", ms(t), r.hits.len(), r.total);
    }
    let cache = bench_dir().join("index.bin");
    let t = Instant::now();
    ix.save(&cache).unwrap();
    println!("index save: {:.0} ms", ms(t));
    let t = Instant::now();
    let back = Index::load(&cache).unwrap();
    println!("index load: {:.0} ms ({} names)", ms(t), back.len());
}

#[test]
#[ignore]
fn perf_startup_pieces() {
    let t = Instant::now();
    let cfg = coxswain_core::config::Config::default();
    let keymap = cfg.keymap().unwrap();
    println!("default config + keymap: {:.2} ms ({} keys)", ms(t), keymap.len());
    let t = Instant::now();
    let cat = coxswain_core::i18n::catalogue("en-GB");
    println!("catalogue: {:.2} ms ({} strings)", ms(t), cat.len());
    let t = Instant::now();
    let _ = coxswain_core::state::AppState::load();
    println!("state load: {:.2} ms", ms(t));
    let t = Instant::now();
    let _ = cfg.themes.values().map(|t| t.slots().len()).sum::<usize>();
    println!("theme slots: {:.2} ms", ms(t));
    let _ = Path::new(".");
}

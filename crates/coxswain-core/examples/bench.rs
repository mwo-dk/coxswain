//! `cargo run --release -p coxswain-core --example bench -- [--archives] / [query...]`
//! With `--archives` the entries of the archives in the home folder are indexed too (set
//! `HOME` to choose another), and a second build takes them from the first, as the hourly
//! rebuild does (`BENCH_ONCE=1` skips it, to measure the memory of one build).
use coxswain_core::{config::SearchConfig, index::Index};
use std::time::Instant;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let archives = args.first().is_some_and(|a| a == "--archives");
    if archives {
        args.remove(0);
    }
    let mut args = args.into_iter();
    let root = args.next().unwrap_or_else(|| "/".into());
    let mut queries: Vec<String> = args.collect();
    if queries.is_empty() {
        queries = ["a", "main.rs", "ext:rs", "*.toml", "zzzzqqq", "src/ lib", "!e ext:md", "case: README"].map(String::from).to_vec();
    }
    let exclude = SearchConfig::default().exclude;
    let t = Instant::now();
    let cfg = SearchConfig::default();
    let ix = Index::build(&[root.clone().into()], &exclude, archives.then_some(&cfg), None);
    println!("build: {} entries in {:?}", ix.len(), t.elapsed());
    if archives && std::env::var_os("BENCH_ONCE").is_none() {
        let t = Instant::now();
        let again = Index::build(&[root.into()], &exclude, archives.then_some(&cfg), Some(&ix));
        println!("again: {} entries in {:?} (unchanged archives from the first)", again.len(), t.elapsed());
    }
    let file = std::env::temp_dir().join("coxswain-bench.bin");
    let t = Instant::now();
    ix.save(&file).unwrap();
    let size = std::fs::metadata(&file).unwrap().len();
    println!("save:  {:.1} MB in {:?}", size as f64 / 1e6, t.elapsed());
    let t = Instant::now();
    let ix = Index::load(&file).unwrap();
    println!("load:  {:?}", t.elapsed());
    for q in &queries {
        let _ = ix.search(q, None, 1000);
        let best = (0..5).map(|_| ix.search(q, None, 1000).micros).min().unwrap();
        println!("{:>14}  {:>8} hits  {:>7.2} ms", q, ix.search(q, None, 1000).total, best as f64 / 1000.0);
    }
    let _ = std::fs::remove_file(file);
}

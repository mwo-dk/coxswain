//! `cargo run --release -p coxswain-core --example bench -- / [query...]`
use coxswain_core::{config::SearchConfig, index::Index};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().unwrap_or_else(|| "/".into());
    let mut queries: Vec<String> = args.collect();
    if queries.is_empty() {
        queries = ["a", "main.rs", "ext:rs", "*.toml", "zzzzqqq", "src/ lib", "!e ext:md", "case: README"].map(String::from).to_vec();
    }
    let t = Instant::now();
    let ix = Index::build(&[root.into()], &SearchConfig::default().exclude);
    println!("build: {} entries in {:?}", ix.len(), t.elapsed());
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

//! Find duplicates under the given folders and print a summary:
//! `cargo run --release -p coxswain-core --example dupes -- ~/Pictures /mnt/old-disk`
use coxswain_core::dupes::{scan, Options, Progress};
use std::time::Instant;

fn main() {
    let roots: Vec<_> = std::env::args().skip(1).map(Into::into).collect();
    let t = Instant::now();
    let r = scan(&Options { roots, ..Default::default() }, &Progress::default());
    let mb = |b: u64| b as f64 / 1e6;
    println!(
        "{} files ({:.0} MB) scanned, {:.0} MB read, {} file groups, {} folder groups, {:.1} MB wasted, {:.2?}",
        r.scanned_files, mb(r.scanned_bytes), mb(r.hashed_bytes), r.groups.len(), r.folders.len(), mb(r.wasted), t.elapsed()
    );
    for g in r.folders.iter().take(3) {
        println!("  folder x{} {:.1} MB: {}", g.paths.len(), mb(g.size), g.paths[0].display());
    }
    for g in r.groups.iter().take(5) {
        println!("  file x{} {:.1} MB: {}", g.files.len(), mb(g.size), g.files[0].path.display());
    }
}

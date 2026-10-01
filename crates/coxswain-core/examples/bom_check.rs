//! `cargo run -p coxswain-core --example bom_check -- <file.cdx.json>...`
//! Reads each BOM as Coxswain's own BOM viewer does (tools/bom/generate.sh runs it on what it
//! made). Fails on a file it cannot read, or on a problem in one, such as a dangling reference.
//! For a CBOM it also prints how each asset rates, so a release shows its own cryptography.
use coxswain_core::bom::view::{is_crypto, Loaded};
use coxswain_core::bom::Status;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: bom_check <file.cdx.json>...");
        return ExitCode::FAILURE;
    }
    let mut ok = true;
    for file in &files {
        let l = match Loaded::open(Path::new(file), None) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("{file}: {e}");
                ok = false;
                continue;
            }
        };
        let issues: Vec<_> = l.bom.issues.iter().chain(&l.issues).collect();
        let assets: Vec<u32> = (0..l.bom.nodes.len() as u32).filter(|&i| is_crypto(l.node(i).kind)).collect();
        println!(
            "{file}: CycloneDX {}, {} nodes, {} cryptographic assets, {} problems",
            l.bom.source.spec_version,
            l.bom.nodes.len(),
            assets.len(),
            issues.len()
        );
        for i in &issues {
            eprintln!("  problem: {}", i.message);
        }
        // Names nothing knows are expected in a CBOM (BLAKE3, ZipCrypto): reported, not failed.
        ok &= issues.iter().all(|i| i.code == coxswain_core::bom::IssueCode::UnresolvedAlgorithm);
        let mut by: BTreeMap<Status, Vec<&str>> = BTreeMap::new();
        for &i in &assets {
            by.entry(l.status(i)).or_default().push(&l.node(i).label);
        }
        for (status, names) in by {
            println!("  {:<11} {}", status.name(), names.join(", "));
        }
    }
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

//! How well search by meaning finds a file, ignored by default. The documentation's pages are
//! the files (93 pages, 123,000 words, many of them longer than 1,500 words), and each question
//! has its answer past the first 960 words of its page: what the passages of 1.32 did not see.
//! Some questions are in Danish, a few name the page more than its words.
//!
//! `XDG_CACHE_HOME=<a folder of its own> cargo test --release -p coxswain-core --test
//! meaning_eval -- --ignored --nocapture`, with `COXSWAIN_EVAL_ENGINE=builtin` (the model in
//! that cache folder) or `ollama` (bge-m3 on Ollama here, the default). It prints recall@1,
//! recall@5 and MRR, the store's size and the time the first pass took, per 1,000 files. The
//! numbers are in docs/reference/performance.md.

use coxswain_core::config::SearchConfig;
use coxswain_core::find;
use coxswain_core::store::{self, Store};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// (the page that answers, the question)
const QUESTIONS: &[(&str, &str)] = &[
    ("files/archive-passwords.md", "Which encryption does a locked zip use?"),
    ("files/archive-passwords.md", "Hvordan får jeg programmet til at glemme adgangskoden til et arkiv?"),
    ("files/archives.md", "What happens if the power goes off while an archive is being changed?"),
    ("files/duplicates.md", "Can I compare my home folder with a backup on a USB disk to find copies?"),
    ("files/duplicates.md", "Hvorfor bliver to ens billeder ikke fundet som dubletter?"),
    ("files/pack-and-extract.md", "How do I make a .tar.gz?"),
    ("files/pack-and-extract.md", "Why is 7z suggested now rather than zip?"),
    ("panels/git-history.md", "Does the history of a file follow it when it was renamed?"),
    ("panels/git-history.md", "Hvordan får jeg en gammel version af en fil tilbage fra git?"),
    ("panels/git.md", "Why are the numbers not the same as what git status prints?"),
    ("panels/git.md", "Why does the target folder have a crossed-out eye?"),
    ("panels/the-screen.md", "The terminal app shows 104857600 where the desktop app says 100 MB"),
    ("previews/bom.md", "Can the cryptography inventory be rated against CNSA 2.0?"),
    ("previews/latex.md", "minted does not work in the LaTeX preview"),
    ("previews/latex.md", "Hvorfor bygges LaTeX med XeLaTeX?"),
    ("previews/tools.md", "How much disk do the previews made by tools take, and how do I free it?"),
    ("reference/privacy.md", "What exactly does a remote embedding server get to see?"),
    ("reference/privacy.md", "Kan andre brugere på samme maskine se mit indeks?"),
    ("reference/terminal-app.md", "Does the terminal app work over SSH?"),
    ("reference/terminal-app.md", "Why can I not select text with the mouse in the terminal app?"),
    ("search/archives.md", "Why are the jars in ~/.m2 not looked into?"),
    ("search/archives.md", "Hvorfor siger en 7z, at dens liste over indhold er for stor?"),
    ("search/ask.md", "Which chat model is a good choice for Ask?"),
    ("search/cloud-files.md", "Are Google Drive's streamed files and Proton Drive covered as well?"),
    ("search/cloud-files.md", "En ældre version hentede hele mit OneDrive. Hvordan frigør jeg pladsen igen?"),
    ("search/find-file.md", "Why does it say showing the first 500?"),
    ("search/notices.md", "I dismissed a tip by mistake. Can I get it back?"),
    ("customise/keys.md", "Can I use the Cmd key on a Mac in my key bindings?"),
    ("customise/own-theme.md", "Hvorfor er gul brun i skrivebordsappen?"),
    ("customise/languages.md", "Why are the names of keys always in English?"),
    ("commands/command-line.md", "cd - does not work on the command line"),
    ("customise/glyphs-and-fonts.md", "Ikonerne er tomme firkanter. Hvad gør jeg?"),
    ("design/bom-viewer.md", "the open questions of the BOM viewer's design"),
    ("reference/command-line-flags.md", "the exit status of the command line flags"),
];

#[test]
#[ignore]
fn meaning_eval() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let work = std::env::var_os("COXSWAIN_BENCH_DIR").map_or_else(std::env::temp_dir, PathBuf::from).join(format!("meaning-eval-{}", std::process::id()));
    let corpus = work.join("docs");
    let mut files = 0;
    for entry in walk(&docs) {
        let rel = entry.strip_prefix(&docs).unwrap();
        // The index and the collected questions repeat the pages' answers.
        if entry.extension().is_some_and(|e| e == "md") && rel != Path::new("README.md") && rel != Path::new("faq.md") {
            std::fs::create_dir_all(corpus.join(rel).parent().unwrap()).unwrap();
            std::fs::copy(&entry, corpus.join(rel)).unwrap();
            files += 1;
        }
    }
    let engine = std::env::var("COXSWAIN_EVAL_ENGINE").unwrap_or_else(|_| "ollama".into());
    let cfg = SearchConfig { text_roots: vec![corpus.clone()], meaning: true, meaning_engine: engine.clone(), meaning_model: if engine == "ollama" { "bge-m3".into() } else { String::new() }, ..SearchConfig::default() };
    let store = Store::open(&work.join("search.db")).unwrap();
    store.set_engine(coxswain_core::meaning::Engine::from_config(&cfg));
    assert!(store.meaning.load(Ordering::Relaxed), "no engine: {engine}");
    store.hurry.store(true, Ordering::Relaxed);
    let t = Instant::now();
    store::scan(&store, &cfg, &AtomicBool::new(false)).unwrap();
    let secs = t.elapsed().as_secs_f64();
    assert_eq!(store.meaning_counts().0, 0, "{:?}", store.meaning_error.lock().unwrap());
    let per = 1000.0 / files as f64;
    println!("{engine}: {files} files, first pass {secs:.0} s ({:.0} s per 1,000 files), store {:.1} MB ({:.0} MB per 1,000 files)", secs * per, store.bytes() as f64 / 1e6, store.bytes() as f64 / 1e6 * per);

    // Meaning alone, words alone, fused, as Find shows it (In files, then About this), and the
    // fusion weights the sweep tries (`COXSWAIN_EVAL_SWEEP=1`).
    let mut weights = vec![("meaning".to_string(), None), ("words".into(), None), ("fused".into(), Some(find::WEIGHTS)), ("shown".into(), None)];
    if std::env::var_os("COXSWAIN_EVAL_SWEEP").is_some() {
        for k in [10.0, 60.0] {
            for meaning in [1.0, 2.0] {
                for some in [0.0, 0.25, 0.5, 1.0] {
                    weights.push((format!("k {k}, words 1, some {some}, meaning {meaning}"), Some(find::Weights { k, words: 1.0, some, meaning })));
                }
            }
        }
    }
    let mut scores = vec![(0, 0, 0.0); weights.len()];
    for (page, q) in QUESTIONS {
        let ((words, every), similar) = (store.search_words(q, None, 20), store.similar(q, None, 20));
        let words = words.hits;
        let ws = find::Words { hits: &words, every };
        let (in_files, about, _) = find::fuse(ws, &similar, find::WEIGHTS);
        for (i, (name, w)) in weights.iter().enumerate() {
            let hits = match (name.as_str(), w) {
                ("meaning", _) => similar.clone(),
                ("words", _) => words.clone(),
                ("shown", _) => in_files.iter().take(5).chain(about.iter().take(5)).cloned().collect(),
                (_, w) => find::fused(ws, &similar, w.unwrap()),
            };
            let rank = hits.iter().position(|h| h.path == corpus.join(page));
            scores[i].0 += (rank == Some(0)) as usize;
            scores[i].1 += rank.is_some_and(|r| r < 5) as usize;
            scores[i].2 += rank.map_or(0.0, |r| 1.0 / (r + 1) as f64);
            if i == 0 {
                let first = hits.first().map(|h| h.path.strip_prefix(&corpus).unwrap_or(&h.path).display().to_string()).unwrap_or_default();
                println!("{:>4} {q}  ({first})", rank.map_or("-".into(), |r| (r + 1).to_string()));
            }
        }
    }
    let n = QUESTIONS.len() as f64;
    for ((name, _), (at1, at5, rr)) in weights.iter().zip(scores) {
        println!("{engine} {name}: recall@1 {:.2}, recall@5 {:.2}, MRR {:.2} over {} questions", at1 as f64 / n, at5 as f64 / n, rr / n, QUESTIONS.len());
    }
    drop(store);
    let _ = std::fs::remove_dir_all(work);
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() { out.extend(walk(&p)) } else { out.push(p) }
    }
    out
}

//! How well search by meaning and Ask find the right file, measured on a small corpus of
//! realistic documents (`tests/search-quality/`) and questions written the way people ask them
//! (`questions.tsv`). Ignored by default, as it needs a model:
//!
//! ```sh
//! cargo test --release -p coxswain-core --test search_quality -- --ignored --nocapture
//! COXSWAIN_EVAL_ENGINE=ollama cargo test --release -p coxswain-core --test search_quality -- --ignored --nocapture
//! ```
//!
//! The built-in model must be installed (Settings → Search by meaning), or Ollama must have
//! `bge-m3` (`COXSWAIN_EVAL_MODEL` names another). The numbers are in
//! docs/reference/performance.md under "Search quality".

use coxswain_core::config::SearchConfig;
use coxswain_core::find;
use coxswain_core::store::{self, Store};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

struct Question {
    text: String,
    expected: Vec<String>,
    phrase: String,
    kind: String,
}

fn questions(dir: &Path) -> Vec<Question> {
    std::fs::read_to_string(dir.join("questions.tsv"))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Question { text: f[0].into(), expected: f[1].split(',').map(String::from).collect(), phrase: f[2].to_lowercase(), kind: f[3].into() }
        })
        .collect()
}

/// The corpus, copied to a folder of its own so that nothing else is read with it.
fn corpus(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.path().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            corpus(&e.path(), &target);
        } else if e.file_name() != "questions.tsv" {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

/// Rank (1-based) of the first expected file among `paths`.
fn rank(paths: &[PathBuf], root: &Path, q: &Question) -> Option<usize> {
    paths.iter().position(|p| p.strip_prefix(root).is_ok_and(|r| q.expected.iter().any(|e| Path::new(e) == r))).map(|i| i + 1)
}

#[derive(Default)]
struct Score {
    n: usize,
    at1: usize,
    at5: usize,
    rr: f64,
}

impl Score {
    fn add(&mut self, r: Option<usize>) {
        self.n += 1;
        self.at1 += usize::from(r == Some(1));
        self.at5 += usize::from(r.is_some_and(|r| r <= 5));
        self.rr += r.map_or(0.0, |r| 1.0 / r as f64);
    }
    fn row(&self, name: &str) -> String {
        let n = self.n.max(1) as f64;
        format!("| {name} | {:.2} | {:.2} | {:.2} |", self.at1 as f64 / n, self.at5 as f64 / n, self.rr / n)
    }
}

/// The fusion weights the sweep tries.
fn sweep() -> Vec<find::Weights> {
    let mut v = vec![];
    for k in [10.0, 60.0] {
        for meaning in [1.0, 2.0] {
            for some in [0.0, 0.25, 0.5, 1.0] {
                v.push(find::Weights { k, words: 1.0, some, meaning });
            }
        }
    }
    v
}

#[test]
#[ignore]
fn search_quality() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/search-quality");
    let engine = std::env::var("COXSWAIN_EVAL_ENGINE").unwrap_or_else(|_| "builtin".into());
    let model = std::env::var("COXSWAIN_EVAL_MODEL").unwrap_or_else(|_| "bge-m3".into());
    if engine == "builtin" && !coxswain_core::meaning::installed() {
        eprintln!("the built-in model is not installed: nothing measured");
        return;
    }
    let d = std::env::temp_dir().join(format!("coxswain-search-quality-{}", std::process::id()));
    let root = d.join("files");
    std::fs::create_dir_all(&root).unwrap();
    corpus(&src, &root);
    let root = root.canonicalize().unwrap();
    let cfg = SearchConfig { text_roots: vec![root.clone()], meaning: true, meaning_engine: engine.clone(), meaning_model: model.clone(), history: false, ..SearchConfig::default() };
    let store = Store::open(&d.join("search.db")).unwrap();
    store.set_engine(coxswain_core::meaning::Engine::from_config(&cfg));
    store.hurry.store(true, Ordering::Relaxed);
    let started = std::time::Instant::now();
    store::scan(&store, &cfg, &AtomicBool::new(false)).unwrap();
    let (waiting, done) = store.meaning_counts();
    assert_eq!(waiting, 0, "every file has its vectors: {:?}", store.meaning_error.lock().unwrap());
    eprintln!("engine {} · {done} files read and embedded in {:.1} s", store.engine_id().unwrap_or_default(), started.elapsed().as_secs_f64());

    let qs = questions(&src);
    let (mut words, mut meaning, mut both, mut shown, mut ask_file, mut ask_passage) = (Score::default(), Score::default(), Score::default(), Score::default(), 0, 0);
    let mut by_kind: std::collections::BTreeMap<String, (Score, Score, usize)> = Default::default();
    let mut lists = vec![];
    for q in &qs {
        let paths = |hits: &[coxswain_core::index::Hit]| hits.iter().map(|h| h.path.clone()).collect::<Vec<_>>();
        let (word_hits, every) = store.search_words(&q.text, None, 20);
        let (word_hits, words_of) = (word_hits.hits, every);
        let similar = store.similar(&q.text, None, 20);
        let w = rank(&paths(&word_hits), &root, q);
        let m = rank(&paths(&similar), &root, q);
        let b = rank(&paths(&find::fused(find::Words { hits: &word_hits, every: words_of }, &similar, find::WEIGHTS)), &root, q);
        // As Find shows it in All: In files, then About this, five each.
        let (in_files, about, _) = find::fuse(find::Words { hits: &word_hits, every: words_of }, &similar, find::WEIGHTS);
        let s = rank(&paths(&in_files.iter().take(5).chain(about.iter().take(5)).cloned().collect::<Vec<_>>()), &root, q);
        let sources = store.passages(&q.text, None, 10);
        let from_file: Vec<&String> = sources.iter().filter(|(p, _)| rank(std::slice::from_ref(p), &root, q).is_some()).map(|(_, t)| t).collect();
        let passage = from_file.iter().any(|t| t.to_lowercase().contains(&q.phrase));
        words.add(w);
        meaning.add(m);
        both.add(b);
        shown.add(s);
        ask_file += usize::from(!from_file.is_empty());
        ask_passage += usize::from(passage);
        let k = by_kind.entry(q.kind.clone()).or_default();
        k.0.add(m);
        k.1.add(b);
        k.2 += usize::from(passage);
        // Ranks in words, meaning, fused, as shown; P: the answering passage was sent to Ask,
        // f: only other passages of the file; then how many files meaning found at all.
        let r = |r: Option<usize>| r.map_or("-".into(), |r| r.to_string());
        eprintln!("{:>2} {:>2} {:>2} {:>2} {} {:>2} {:8} {}", r(w), r(m), r(b), r(s), if passage { "P" } else if from_file.is_empty() { "-" } else { "f" }, similar.len(), q.kind, q.text);
        lists.push((word_hits, words_of, similar, q));
    }
    let n = qs.len();
    println!("\n{n} questions, {done} files, engine {}\n", store.engine_id().unwrap_or_default());
    println!("| List | Recall@1 | Recall@5 | MRR |\n|---|---|---|---|");
    println!("{}", words.row("Words alone (every word, then any of them)"));
    println!("{}", meaning.row("Meaning alone"));
    println!("{}", both.row("Fused (words and meaning by rank)"));
    println!("{}", shown.row("Find as shown (In files, then About this)"));
    println!("\nAsk: the right file among the 10 passages sent for {ask_file} of {n} questions, the passage that answers for {ask_passage} of {n}.\n");
    println!("| Kind | Questions | Meaning recall@5 | Fused recall@5 | Ask passage |\n|---|---|---|---|---|");
    for (kind, (m, b, p)) in &by_kind {
        println!("| {kind} | {} | {:.2} | {:.2} | {p} of {} |", m.n, m.at5 as f64 / m.n as f64, b.at5 as f64 / b.n as f64, m.n);
    }
    // The weights tried: `COXSWAIN_EVAL_SWEEP=1`.
    if std::env::var_os("COXSWAIN_EVAL_SWEEP").is_some() {
        println!("\n| k | words | some words | meaning | Recall@1 | Recall@5 | MRR |\n|---|---|---|---|---|---|---|");
        for w in sweep() {
            let mut sc = Score::default();
            for (wh, every, sim, q) in &lists {
                sc.add(rank(&find::fused(find::Words { hits: wh, every: *every }, sim, w).iter().map(|h| h.path.clone()).collect::<Vec<_>>(), &root, q));
            }
            println!("{}", sc.row(&format!("{} | {} | {} | {}", w.k, w.words, w.some, w.meaning)));
        }
    }
    drop(store);
    let _ = std::fs::remove_dir_all(d);
}

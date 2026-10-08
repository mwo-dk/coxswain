//! How well Ask answers questions about code: questions about this repository (a copy of it at
//! `PIN`, made with `git archive`) and about small projects in Python, TypeScript, Java, Go and
//! C (`tests/code-eval/`), each with the files that hold the answer and the facts the answer
//! must name (`questions.tsv`). Ignored by default, as it needs a model and git:
//!
//! ```sh
//! COXSWAIN_EVAL_ENGINE=ollama COXSWAIN_EVAL_ASK=qwen3:8b cargo test --release -p coxswain-core --test code_eval -- --ignored --nocapture
//! COXSWAIN_EVAL_ENGINE=builtin COXSWAIN_EVAL_ASK=builtin:qwen3-4b COXSWAIN_EVAL_LIMIT=2 cargo test --release …
//! ```
//!
//! Run it with `HOME` and `XDG_CACHE_HOME` of its own: the built-in models are looked for
//! there, and downloaded there with `COXSWAIN_EVAL_DOWNLOAD=1`. `COXSWAIN_EVAL_MODEL` names
//! the server's embedding model (bge-m3), `COXSWAIN_EVAL_ASK` the chat model (none: only what
//! Ask reads is measured), `COXSWAIN_EVAL_ONLY` the corpora (`coxswain,go-shortlink`),
//! `COXSWAIN_EVAL_LIMIT` the questions per corpus. Per question it prints whether a file that
//! holds the answer was among the sources, the share of the facts the sources hold and the
//! share the answer names, and the wait for the first word. The
//! numbers are in docs/reference/performance.md under "Code questions".

use coxswain_core::config::SearchConfig;
use coxswain_core::store::{self, Store};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// The commit of this repository the questions are about.
const PIN: &str = "fcd2d4047e929da961009a11ccb4dbcf10fd10df";

struct Question {
    corpus: String,
    text: String,
    files: Vec<String>,
    facts: Vec<Vec<String>>,
}

fn questions(dir: &Path) -> Vec<Question> {
    std::fs::read_to_string(dir.join("questions.tsv"))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let facts = f[3].split(';').map(|g| g.split('|').map(|a| a.to_lowercase()).collect()).collect();
            Question { corpus: f[0].into(), text: f[1].into(), files: f[2].split(',').map(String::from).collect(), facts }
        })
        .collect()
}

/// The share of `facts` (each a group of alternatives) that `text` names.
fn named(facts: &[Vec<String>], text: &str) -> f64 {
    let text = text.to_lowercase();
    facts.iter().filter(|g| g.iter().any(|a| text.contains(a.as_str()))).count() as f64 / facts.len().max(1) as f64
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        if e.path().is_dir() {
            copy(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// This repository at `PIN`, without its pictures and vendored viewers: what a clone holds
/// that is read as text.
fn this_repository(to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tar = std::process::Command::new("git").arg("-C").arg(&repo).args(["archive", "--format=tar", PIN]).output().unwrap();
    assert!(tar.status.success(), "git archive: {}", String::from_utf8_lossy(&tar.stderr));
    let mut untar = std::process::Command::new("tar").arg("-x").arg("-C").arg(to).stdin(std::process::Stdio::piped()).spawn().unwrap();
    std::io::Write::write_all(untar.stdin.as_mut().unwrap(), &tar.stdout).unwrap();
    assert!(untar.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(to.join("gui/public/vendor"));
    let _ = std::fs::remove_dir_all(to.join("gui/vendor"));
    fn prune(dir: &Path) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                prune(&p);
            } else if p.extension().is_some_and(|x| ["png", "gif", "jpg", "jpeg", "ico", "icns", "tgz", "woff", "woff2", "ttf"].contains(&x.to_string_lossy().as_ref())) {
                std::fs::remove_file(p).unwrap();
            }
        }
    }
    prune(to);
}

#[derive(Default)]
struct Tally {
    n: usize,
    file: usize,
    in_sources: f64,
    in_answer: f64,
    first_word: f64,
}

#[test]
#[ignore]
fn code_eval() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/code-eval");
    let engine = std::env::var("COXSWAIN_EVAL_ENGINE").unwrap_or_else(|_| "ollama".into());
    let model = std::env::var("COXSWAIN_EVAL_MODEL").unwrap_or_else(|_| "bge-m3".into());
    let ask = std::env::var("COXSWAIN_EVAL_ASK").unwrap_or_default();
    let only: Vec<String> = std::env::var("COXSWAIN_EVAL_ONLY").map(|s| s.split(',').filter(|c| !c.is_empty()).map(String::from).collect()).unwrap_or_default();
    let limit: usize = std::env::var("COXSWAIN_EVAL_LIMIT").ok().and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    // `COXSWAIN_EVAL_DOWNLOAD=1`: the built-in models it needs are downloaded first, into the
    // cache folder of the HOME it runs with.
    if std::env::var_os("COXSWAIN_EVAL_DOWNLOAD").is_some() {
        let p = coxswain_core::meaning::Progress::default();
        if engine == "builtin" {
            coxswain_core::meaning::download(&p).unwrap();
        }
        if let Some(m) = coxswain_core::chat::of(&ask) {
            m.download(&p).unwrap();
        }
    }
    if engine == "builtin" && !coxswain_core::meaning::installed() {
        eprintln!("the built-in model is not installed: nothing measured");
        return;
    }
    let work = std::env::temp_dir().join(format!("coxswain-code-eval-{}", std::process::id()));
    let qs = questions(&src);
    let mut corpora: Vec<String> = qs.iter().map(|q| q.corpus.clone()).collect();
    corpora.dedup();
    let mut tallies: Vec<(String, Tally)> = vec![];
    for corpus in corpora.into_iter().filter(|c| only.is_empty() || only.contains(c)) {
        let root = work.join(&corpus);
        if corpus == "coxswain" { this_repository(&root) } else { copy(&src.join(&corpus), &root) }
        let root = root.canonicalize().unwrap();
        let cfg = SearchConfig { text_roots: vec![root.clone()], meaning: true, meaning_engine: engine.clone(), meaning_model: model.clone(), ask_model: ask.clone(), history: false, ..SearchConfig::default() };
        let store = Store::open(&work.join(format!("{corpus}.db"))).unwrap();
        store.set_engine(coxswain_core::meaning::Engine::from_config(&cfg));
        store.hurry.store(true, Ordering::Relaxed);
        let t = Instant::now();
        store::scan(&store, &cfg, &AtomicBool::new(false)).unwrap();
        assert_eq!(store.meaning_counts().0, 0, "every file has its vectors: {:?}", store.meaning_error.lock().unwrap());
        eprintln!("\n{corpus}: {} files, {} passages, embedded in {:.0} s", store.meaning_counts().1, store.passage_count(), t.elapsed().as_secs_f64());
        let mut tally = Tally::default();
        for q in qs.iter().filter(|q| q.corpus == corpus).take(limit) {
            let sources = coxswain_core::ask::sources(|q, s, b| store.passages(q, s, b), &cfg, &[], &q.text, None, &root).unwrap_or_default();
            let rel = |p: &PathBuf| p.strip_prefix(&root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            let found = sources.iter().any(|(p, _)| q.files.iter().any(|f| rel(p) == *f || rel(p).starts_with(&format!("{f}/"))));
            let all: String = sources.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
            let in_sources = named(&q.facts, &all);
            let (mut answer, mut first) = (String::new(), None);
            if !ask.is_empty() {
                let t = Instant::now();
                let r = coxswain_core::meaning::ask(&cfg, &[], &q.text, &sources, |piece| {
                    if first.is_none() && !piece.trim().is_empty() {
                        first = Some(t.elapsed().as_secs_f64());
                    }
                    answer.push_str(piece);
                    true
                });
                if let Err(e) = r {
                    answer = format!("(error: {e})");
                }
            }
            let in_answer = named(&q.facts, &answer);
            tally.n += 1;
            tally.file += usize::from(found);
            tally.in_sources += in_sources;
            tally.in_answer += in_answer;
            tally.first_word += first.unwrap_or(0.0);
            let files: Vec<String> = sources.iter().map(|(p, _)| rel(p)).take(6).collect();
            eprintln!("{} sources {:.2} answer {:.2} {:>5.1} s  {}  [{}]", if found { "F" } else { "-" }, in_sources, in_answer, first.unwrap_or(0.0), q.text, files.join(", "));
            if std::env::var_os("COXSWAIN_EVAL_SHOW").is_some() {
                eprintln!("    {}", answer.trim().replace('\n', "\n    "));
            }
        }
        tallies.push((corpus, tally));
        drop(store);
    }
    println!("\nengine {engine} {}, chat model {}\n", if engine == "builtin" { "" } else { &model }, if ask.is_empty() { "none" } else { &ask });
    println!("| Corpus | Questions | Right file read | Facts in what was read | Facts in the answer | First word |\n|---|---|---|---|---|---|");
    let mut sum = Tally::default();
    for (name, t) in &tallies {
        let n = t.n.max(1) as f64;
        println!("| {name} | {} | {} | {:.2} | {:.2} | {:.1} s |", t.n, t.file, t.in_sources / n, t.in_answer / n, t.first_word / n);
        sum.n += t.n;
        sum.file += t.file;
        sum.in_sources += t.in_sources;
        sum.in_answer += t.in_answer;
        sum.first_word += t.first_word;
    }
    let n = sum.n.max(1) as f64;
    println!("| all | {} | {} | {:.2} | {:.2} | {:.1} s |", sum.n, sum.file, sum.in_sources / n, sum.in_answer / n, sum.first_word / n);
    let _ = std::fs::remove_dir_all(work);
}

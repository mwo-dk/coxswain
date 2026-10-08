//! What Ask reads before the chat model answers: as much of the files as the model's context
//! holds. Excerpts of the files closest to the question, each hit with the passages around it,
//! and for a question about the folder as a whole (*what is this repository about?*) an
//! overview of it first: its README, its project files, the tree of its folders and the index
//! of its docs.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::config::SearchConfig;
use crate::meaning::Turn;
use crate::t;

/// Between two excerpts of one file.
pub const GAP: &str = "\n[…]\n";

/// Bytes a token is taken to be, on the low side: English is about four, code three, Chinese
/// or Japanese three a character, and a character a token.
const TOKEN: usize = 3;

/// Bytes of sources Ask always gives, even when earlier turns take the rest.
const LEAST: usize = 1500;

/// Tokens the chat model is given, prompt and answer: a built-in model's own, or `ask_context`.
pub fn context(cfg: &SearchConfig) -> usize {
    match crate::chat::of(&cfg.ask_model) {
        Some(m) => m.context(cfg.meaning_device == "cpu"),
        None => cfg.ask_context.clamp(2048, 131_072),
    }
}

/// Tokens `text` takes, about, on the high side.
pub(crate) fn tokens(text: &str) -> usize {
    text.len().div_ceil(TOKEN)
}

/// Bytes of sources that fit: the context, less the rules, the turns before, the question and
/// room for the answer (and the thinking before it).
pub fn budget(cfg: &SearchConfig, earlier: &[Turn], question: &str) -> usize {
    let used = crate::meaning::RULES.len() + question.len() + earlier.iter().map(|(q, a)| q.len() + a.len() + 40).sum::<usize>();
    (context(cfg).saturating_sub(crate::chat::answer_room(crate::chat::thinks(cfg))) * TOKEN).saturating_sub(used + 200).max(LEAST)
}

/// The sources of an answer to `question`, numbered in their order: excerpts closest to it
/// (looked up with the question before it, which a follow-up often leans on), below `scope`
/// when given; for a question about the folder as a whole, an overview of `scope` or, without
/// one, of `here` (the panel's folder) first. `passages` is `Client::passages`. The error says
/// nothing was close.
pub fn sources(passages: impl FnOnce(&str, Option<&Path>, usize) -> Vec<(PathBuf, String)>, cfg: &SearchConfig, earlier: &[Turn], question: &str, scope: Option<&Path>, here: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let lookup = earlier.last().map_or(question.to_string(), |(q, _)| format!("{q} {question}"));
    let budget = budget(cfg, earlier, question);
    let dir = scope.unwrap_or(here);
    let mut out = if broad(question) { overview(dir, budget * 3 / 5, &cfg.text_exclude) } else { vec![] };
    let used: usize = out.iter().map(|(p, t)| p.as_os_str().len() + t.len() + 8).sum();
    for (path, text) in passages(&lookup, scope, budget.saturating_sub(used)) {
        match out.iter_mut().find(|(p, _)| *p == path) {
            // A README the overview has: what the passages add that it does not.
            Some((_, had)) => {
                let new: Vec<&str> = text.split(GAP).filter(|e| !had.contains(e)).collect();
                if !new.is_empty() {
                    had.push_str(GAP);
                    had.push_str(&new.join(GAP));
                }
            }
            None => out.push((path, text)),
        }
    }
    // Nothing close, but the question names the folder: what it holds says the most.
    if out.is_empty() && has(&question.to_lowercase(), HERE) {
        out = overview(dir, budget, &cfg.text_exclude);
    }
    if out.is_empty() {
        return Err(match scope {
            Some(dir) => t!("find.ask_nothing_in", "folder" => dir.file_name().unwrap_or_default().to_string_lossy()),
            None => t!("search.ask_nothing"),
        });
    }
    Ok(out)
}

/// The line over the sources: how much was read, "12 excerpts from 7 files, about 6,000 words".
pub fn read(sources: &[(PathBuf, String)]) -> String {
    let excerpts: usize = sources.iter().map(|(_, t)| t.matches(GAP).count() + 1).sum();
    let mut files: Vec<&PathBuf> = sources.iter().map(|(p, _)| p).collect();
    files.sort();
    files.dedup();
    let words: usize = sources.iter().map(|(_, t)| t.split_whitespace().count()).sum();
    let round = if words >= 1000 { 100 } else { 10 };
    let words = (words + round / 2) / round * round;
    t!("ask.read", "excerpts" => crate::tn!("ask.excerpts", excerpts), "files" => crate::tn!("preview.files", files.len()), "words" => crate::tn!("ask.words", words.max(1)))
}

/// Words that ask what something is or does, in the app's languages.
const WHAT: &[&str] = &[
    "what", "explain", "describe", "summar", "overview", "worum", "was ist", "was macht", "erklär", "beschreib", "überblick", "übersicht", "zusammenfass", "hvad", "forklar", "beskriv", "overblik",
    "oversigt", "opsummer", "vad", "förklara", "översikt", "sammanfatta", "mistä", "mikä", "mitä", "kerro", "yleiskatsaus", "waar gaat", "wat is", "wat doet", "leg uit", "overzicht", "samenvat",
    "de quoi", "c'est quoi", "qu'est", "que fait", "explique", "décri", "aperçu", "vue d'ensemble", "qué", "de qué", "explica", "describ", "què", "de què", "resum", "di cosa", "di che", "cos'è",
    "cosa fa", "spiega", "panoramica", "riassum", "o czym", "co to", "czym jest", "wyjaśnij", "opisz", "przegląd", "podsumuj", "o čem", "co je", "vysvětli", "popiš", "přehled", "shrň", "про що",
    "що це", "що робить", "поясни", "опиши", "огляд", "τι ", "εξήγησε", "περίγραψε", "επισκόπηση", "περίληψ", "何", "概要", "説明", "무엇", "뭐", "설명", "개요", "מה ", "על מה", "הסבר", "תאר",
    "סקירה", "چیست", "چه ", "توضیح", "مرور", "mis ", "millest", "selgita", "kirjelda", "ülevaade", "kas ir", "par ko", "paskaidro", "apraksti", "pārskats", "kas tai", "apie ką", "paaiškink",
    "apibūdink", "apžvalga", "zer da", "zertaz", "azaldu", "deskribatu", "ikuspegi", "ինչ", "բացատրիր", "նկարագրիր", "ակնարկ", "რა ", "რის", "ახსენი", "აღწერე", "მიმოხილვა",
];

/// Words for the folder or the project as a whole.
const HERE: &[&str] = &[
    "repo", "repositor", "project", "codebase", "code base", "this code", "the code", "source code", "folder", "directory", "projekt", "ordner", "verzeichnis", "quellcode", "mappen", "denne mappe", "kildekode", "koden",
    "kod", "katalog", "kansio", "projekti", "koodi", "deze map", "dossier", "projet", "dépôt", "le code", "carpeta", "proyecto", "repositorio", "código", "projecte", "repositori", "codi",
    "cartella", "progetto", "codice", "repozytor", "složk", "repozitář", "kód", "папк", "проєкт", "репозитор", "код", "φάκελ", "έργο", "κώδικ", "αποθετήρι", "フォルダ", "プロジェクト",
    "リポジトリ", "コード", "폴더", "프로젝트", "저장소", "코드", "תיקי", "פרויקט", "מאגר", "קוד", "پوشه", "پروژه", "مخزن", "کد", "kaust", "hoidla", "kood", "mape", "repozitorij", "kods",
    "aplank", "saugykl", "kodas", "karpeta", "proiektu", "biltegi", "kode", "թղթապանակ", "նախագիծ", "կոդ", "საქაღალდე", "პროექტ", "კოდ",
];

/// Whether `q` (lower case) has one of `words` at the start of a word (a word of four letters
/// or fewer whole); in a script without spaces, anywhere.
fn has(q: &str, words: &[&str]) -> bool {
    words.iter().any(|w| {
        let spaced = !w.chars().next().is_some_and(|c| ('\u{3040}'..='\u{9fff}').contains(&c) || ('\u{ac00}'..='\u{d7af}').contains(&c));
        // A short word must end there too: "repo" is not "report".
        let whole = w.chars().count() <= 4 && !w.ends_with(' ');
        q.match_indices(w).any(|(i, _)| !spaced || q[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric()) && (!whole || q[i + w.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric())))
    })
}

/// Whether `question` asks about the folder or the project as a whole: what it is, what it is
/// about, an overview of it.
// ponytail: word lists, not a model; a question they miss gets the closest excerpts alone.
pub fn broad(question: &str) -> bool {
    let q = question.to_lowercase();
    has(&q, WHAT) && has(&q, HERE) || has(&q, &["overview", "überblick", "overblik", "översikt", "overzicht", "aperçu", "panoramica", "概要", "개요"]) || has(&q, BUILT_WITH)
}

/// Words that ask how a project is built and with what: its architecture, frameworks,
/// libraries and dependencies, which the map of a project answers.
const BUILT_WITH: &[&str] = &[
    "architect", "framework", "technolog", "dependenc", "librar", "crates", "tech stack", "written in", "programming language", "architektur", "abhängigkeit", "bibliothek", "technologie", "arkitektur",
    "afhængighed", "bibliotek", "teknologi", "beroende", "riippuvuu", "kirjasto", "arkkitehtuuri", "afhankelijk", "bibliothe", "dépendance", "librairie", "arquitectura", "dependencia", "biblioteca", "tecnolog",
    "architettura", "dipendenz", "zależnoś", "bibliotek", "architektura", "závislost", "knihovn", "архітектур", "залежн", "бібліотек", "αρχιτεκτονικ", "εξαρτήσ", "βιβλιοθήκ", "アーキテクチャ", "ライブラリ",
    "依存", "아키텍처", "라이브러리", "의존", "ארכיטקטור", "معماری", "کتابخانه", "arhitektuur", "teek", "arhitektūr", "bibliotēk", "architektūr", "bibliotek", "arkitektura", "liburutegi",
    "ճարտարապետ", "գրադարան", "არქიტექტურ", "ბიბლიოთეკ",
];

/// Files that say what a project is, by name.
const MANIFESTS: &[&str] = &[
    "Cargo.toml", "package.json", "pyproject.toml", "setup.py", "setup.cfg", "go.mod", "pom.xml", "build.gradle", "build.gradle.kts", "Gemfile", "composer.json", "CMakeLists.txt", "meson.build",
    "mix.exs", "Package.swift", "pubspec.yaml", "deno.json", "flake.nix", "DESCRIPTION", "stack.yaml",
];

fn is_manifest(name: &str) -> bool {
    MANIFESTS.contains(&name) || [".csproj", ".fsproj", ".vbproj", ".cabal", ".gemspec", ".nimble"].iter().any(|e| name.ends_with(e))
}

/// An overview of `dir`, about `bytes` long: its README, the tree of its folders two deep
/// (hidden ones, `exclude`d ones and the names its `.gitignore` gives left out) and, for a
/// project, its map (`code::map`: what each project file says it uses, each file of code
/// with what it says it is and its public items), its project files to three deep, and its
/// docs' index. Each is a source of its own; the tree's and the map's is `dir`.
pub fn overview(dir: &Path, bytes: usize, exclude: &[String]) -> Vec<(PathBuf, String)> {
    let mut skip: Vec<String> = exclude.to_vec();
    // ponytail: plain names from the top .gitignore only; patterns and nested ones are not read.
    if let Ok(ignore) = std::fs::read_to_string(dir.join(".gitignore")) {
        skip.extend(ignore.lines().map(|l| l.trim().trim_matches('/')).filter(|l| !l.is_empty() && !l.starts_with('#') && !l.contains(['*', '?', '[', '!', '/'])).map(String::from));
    }
    let mut tree = format!("{}/\n", dir.file_name().unwrap_or_default().to_string_lossy());
    let mut manifests = vec![];
    walk(dir, 0, &skip, &mut tree, &mut manifests);
    let readme = entries(dir).into_iter().filter(|(_, d)| !d).map(|(p, _)| p).find(|p| p.file_stem().is_some_and(|s| s.eq_ignore_ascii_case("readme")));
    let docs = [("docs", "README.md"), ("docs", "index.md"), ("doc", "README.md"), ("doc", "index.md")].iter().map(|(d, f)| dir.join(d).join(f)).find(|p| p.is_file());
    let mut out = vec![];
    let mut left = bytes;
    let mut put = |path: PathBuf, text: String, most: usize| {
        // A scrap of a file says nothing: one gets a few lines at least, or none.
        if most.min(left) < 200 {
            return;
        }
        let text = cut(&text, most.min(left));
        if text.trim().is_empty() {
            return;
        }
        left -= text.len().min(left);
        out.push((path, text));
    };
    // A project's map says more of it than its README's later parts and its project files
    // in full: it gets half, and they less.
    let map = crate::code::map(dir, bytes / 2, &skip);
    let share = if map.is_empty() { [2, 4, 8] } else { [4, 8, 16] };
    if let Some(p) = readme {
        put(p.clone(), head(&p, bytes), (bytes / share[0]).max(200));
    }
    let tree = if map.is_empty() { tree } else { format!("{}\n{map}", cut(&tree, bytes / share[1])) };
    put(dir.to_path_buf(), tree, (bytes / share[1] + map.len()).max(200));
    for p in manifests.into_iter().chain(docs) {
        put(p.clone(), head(&p, bytes), (bytes / share[2]).max(200));
    }
    out
}

/// The folders and files of `dir`, sorted, folders first.
fn entries(dir: &Path) -> Vec<(PathBuf, bool)> {
    // A folder of a million files is not listed whole.
    let mut v: Vec<(PathBuf, bool)> = std::fs::read_dir(dir).into_iter().flatten().flatten().take(2000).map(|e| (e.path(), e.file_type().is_ok_and(|t| t.is_dir()))).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

/// The tree below `dir` into `tree`, two deep, 40 entries a folder; project files three deep
/// into `manifests`, the top ones first.
fn walk(dir: &Path, depth: usize, skip: &[String], tree: &mut String, manifests: &mut Vec<PathBuf>) {
    let all: Vec<_> = entries(dir).into_iter().filter(|(p, _)| p.file_name().is_some_and(|n| !n.to_string_lossy().starts_with('.') && !skip.iter().any(|s| *s == n.to_string_lossy()))).collect();
    manifests.extend(all.iter().filter(|(p, d)| !d && p.file_name().is_some_and(|n| is_manifest(&n.to_string_lossy()))).map(|(p, _)| p.clone()));
    for (i, (p, is_dir)) in all.iter().enumerate() {
        let name = p.file_name().unwrap_or_default().to_string_lossy();
        if depth < 2 && i < 40 {
            tree.push_str(&format!("{}{name}{}\n", "  ".repeat(depth + 1), if *is_dir { "/" } else { "" }));
        } else if depth < 2 && i == 40 {
            tree.push_str(&format!("{}… {} more\n", "  ".repeat(depth + 1), all.len() - 40));
        }
        if *is_dir && depth < 2 {
            walk(p, depth + 1, skip, tree, manifests);
        }
    }
}

/// The start of a file, `bytes` at most, as text.
fn head(path: &Path, bytes: usize) -> String {
    let mut buf = vec![];
    let _ = std::fs::File::open(path).and_then(|f| f.take(bytes as u64 + 4).read_to_end(&mut buf));
    String::from_utf8_lossy(&buf).into_owned()
}

/// `text` cut to `bytes` at most, at the end of a line where there is one.
fn cut(text: &str, bytes: usize) -> String {
    if text.len() <= bytes {
        return text.to_string();
    }
    let mut end = bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let end = text[..end].rfind('\n').filter(|&i| i > end / 2).unwrap_or(end);
    format!("{}\n…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broad_questions_are_told_from_specific_ones() {
        for q in ["What is this repository's code about?", "Give me an overview", "Explain this project", "Hvad handler denne mappe om?", "Worum geht es in diesem Projekt?", "De quoi parle ce dépôt ?", "このリポジトリの概要"] {
            assert!(broad(q), "{q}");
        }
        for q in ["How does the search helper start?", "Which systems are supported and how is each installed?", "What does the fuel cost?", "what is the mapping of keys"] {
            assert!(!broad(q), "{q}");
        }
    }

    #[test]
    fn the_budget_fills_the_context_less_what_else_it_holds() {
        let cfg = SearchConfig { ask_model: "qwen3:8b".into(), ..SearchConfig::default() };
        let b = budget(&cfg, &[], "What?");
        assert!(b > 15_000 && b < (8192 - 1024) * TOKEN, "{b}");
        let big = SearchConfig { ask_context: 32_768, ..cfg.clone() };
        assert!(budget(&big, &[], "What?") > 4 * b);
        let long = vec![("q".repeat(10_000), "a".repeat(10_000))];
        assert_eq!(budget(&cfg, &long, "What?"), LEAST, "earlier turns take it all");
        let builtin = SearchConfig { ask_model: "builtin:qwen3-4b".into(), meaning_device: "cpu".into(), ..cfg };
        assert!(budget(&builtin, &[], "What?") < 3000, "a processor gets a small one");
    }

    #[test]
    fn the_overview_has_the_readme_the_tree_and_the_manifests() {
        let d = std::env::temp_dir().join(format!("coxswain-overview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for dir in ["crates/core/src", "target/release", "docs", ".git", "secret"] {
            std::fs::create_dir_all(d.join(dir)).unwrap();
        }
        std::fs::write(d.join("README.md"), "# Rocket\n\nA rocket planner.\n").unwrap();
        std::fs::write(d.join(".gitignore"), "/secret\n*.log\n").unwrap();
        std::fs::write(d.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(d.join("crates/core/Cargo.toml"), "[package]\nname = \"core\"\ndescription = \"The core\"\n").unwrap();
        std::fs::write(d.join("docs/README.md"), "# Docs\n").unwrap();
        let o = overview(&d, 20_000, &["target".into()]);
        let names: Vec<_> = o.iter().map(|(p, _)| p.strip_prefix(&d).unwrap().to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(names, ["README.md", "", "Cargo.toml", "crates/core/Cargo.toml", "docs/README.md"]);
        let tree = &o[1].1;
        assert!(tree.contains("  crates/\n    core/\n") && tree.contains("  docs/\n"), "{tree}");
        assert!(!tree.contains("target") && !tree.contains(".git") && !tree.contains("secret") && !tree.contains("src"), "{tree}");
        let small = overview(&d, 300, &[]);
        assert!(small.len() < o.len() && small.iter().map(|(_, t)| t.len()).sum::<usize>() <= 300, "a small room: what fits, no scraps: {small:?}");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_broad_question_gets_the_overview_and_the_excerpts() {
        let d = std::env::temp_dir().join(format!("coxswain-ask-sources-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("README.md"), "# Rocket\n\nA rocket planner.\n").unwrap();
        let cfg = SearchConfig { ask_model: "qwen3:8b".into(), ..SearchConfig::default() };
        let readme = d.join("README.md");
        let found = |_: &str, _: Option<&Path>, _: usize| vec![(readme.clone(), format!("A rocket planner.{GAP}## Fuel budget")), (d.join("src/fuel.rs"), "fn fuel()".to_string())];
        let s = sources(found, &cfg, &[], "What is this project about?", None, &d).unwrap();
        let names: Vec<_> = s.iter().map(|(p, _)| p.strip_prefix(&d).unwrap().to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(names, ["README.md", "", "src/fuel.rs"], "the README once, with what the excerpts add");
        assert!(s[0].1.ends_with(&format!("{GAP}## Fuel budget")), "{:?}", s[0].1);
        let s = sources(found, &cfg, &[], "How much fuel?", None, &d).unwrap();
        assert_eq!(s.len(), 2, "a specific question: the excerpts alone");
        // Nothing close, but the question is about the folder.
        let none = |_: &str, _: Option<&Path>, _: usize| vec![];
        assert_eq!(sources(none, &cfg, &[], "how is the code laid out", None, &d).unwrap().len(), 2);
        assert!(sources(none, &cfg, &[], "how much fuel", None, &d).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_read_line_counts_excerpts_files_and_words() {
        let sources = vec![(PathBuf::from("/a"), format!("{}{GAP}{}", "w ".repeat(3000), "w ".repeat(1000))), (PathBuf::from("/b"), "w ".repeat(2000))];
        let line = read(&sources);
        assert_eq!(line, "3 excerpts from 2 files, about 6000 words");
    }
}

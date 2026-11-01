//! Search by meaning with the built-in model on long documents kept whole (*Long files:
//! Everything*): long Word and OpenDocument texts, one longer than the 4 MB kept of most files
//! (so read line by line again), and PDFs of 100 pages and more made from them by LibreOffice
//! when it is installed. Every passage of each file must be in the store with its vector, the
//! last page's too, and each fact written into the files must find its file by meaning, with
//! the passage that holds it. Prints the time the first pass took and the memory it held.
//!
//! Opt-in, as it downloads the built-in model (about 470 MB) into a home of its own under
//! `target/tmp/long-meaning` (never the user's cache):
//! `COXSWAIN_LONG_MEANING=1 cargo test --release -p coxswain-core --test long_meaning -- --nocapture`
//! (about 45 minutes on two threads of a desktop CPU); `COXSWAIN_LONG_MEANING=part` runs only
//! the Word text whose reader runs out of time, read once, at the next scan and a day later.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use coxswain_core::config::SearchConfig;
use coxswain_core::store::{self, Store};

const WORDS: &[&str] = &[
    "river", "window", "garden", "paper", "morning", "engine", "bridge", "letter", "market", "winter", "harbour", "kitchen",
    "silver", "station", "orchard", "ladder", "lantern", "meadow", "pocket", "candle", "valley", "basket", "mirror", "thunder",
    "quietly", "slowly", "often", "never", "always", "perhaps", "later", "almost", "behind", "across", "beneath", "toward",
    "walks", "carries", "builds", "watches", "follows", "paints", "counts", "opens", "closes", "finds", "keeps", "sends",
    "old", "narrow", "bright", "heavy", "early", "distant", "green", "careful", "patient", "small", "broad", "steady",
];
const ANIMALS: &[&str] = &["otter", "heron", "badger", "falcon", "walrus", "lynx", "beaver", "puffin", "tortoise", "jackal", "moose", "gecko"];
const THINGS: &[&str] = &["copper kettles", "violin strings", "salt barrels", "glass marbles", "wool blankets", "tin whistles", "oak barrels", "paper kites", "brass keys", "clay pots"];
const CITIES: &[&str] = &["Reykjavik", "Lisbon", "Tromsø", "Valparaíso", "Hobart", "Kraków", "Nairobi", "Kyoto", "Halifax", "Tallinn", "Cusco", "Dunedin"];

/// A long text: paragraphs of filler, and every so often a fact of its own (returned).
fn text(seed: u64, words: usize) -> (Vec<String>, Vec<String>) {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut next = |n: usize| {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 33) as usize % n
    };
    let (mut paras, mut facts, mut n) = (vec![], vec![], 0);
    while n < words {
        let mut p: Vec<String> = (0..60).map(|_| WORDS[next(WORDS.len())].to_string()).collect();
        n += 60;
        // A fact about every 600 words.
        if paras.len() % 10 == 5 {
            let fact = format!(
                "The {} named {} carried {} {} to {} in the year {}.",
                ANIMALS[next(ANIMALS.len())],
                ["Ottilie", "Bertram", "Saskia", "Ingmar", "Leopold", "Marisol", "Henrike", "Casimir"][next(8)],
                10 + next(900),
                THINGS[next(THINGS.len())],
                CITIES[next(CITIES.len())],
                1500 + facts.len() + 7 * seed as usize
            );
            p.push(fact.clone());
            facts.push(fact);
        }
        paras.push(p.join(" "));
    }
    (paras, facts)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;")
}

fn zip_of(path: &Path, files: &[(&str, String)]) {
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, body) in files {
        let o = zip::write::SimpleFileOptions::default().compression_method(if *name == "mimetype" {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        });
        z.start_file(*name, o).unwrap();
        z.write_all(body.as_bytes()).unwrap();
    }
    z.finish().unwrap();
}

fn docx(path: &Path, paras: &[String]) {
    let body: String = paras.iter().map(|p| format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", esc(p))).collect();
    zip_of(path, &[
        ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.into()),
        ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.into()),
        ("word/document.xml", format!(r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#)),
    ]);
}

fn odt(path: &Path, paras: &[String]) {
    let body: String = paras.iter().map(|p| format!("<text:p>{}</text:p>", esc(p))).collect();
    zip_of(path, &[
        ("mimetype", "application/vnd.oasis.opendocument.text".into()),
        ("META-INF/manifest.xml", r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#.into()),
        ("content.xml", format!(r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2"><office:body><office:text>{body}</office:text></office:body></office:document-content>"#)),
    ]);
}

fn peak_mb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines().find(|l| l.starts_with("VmHWM")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()).unwrap_or(0) / 1024
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn long_documents_are_found_by_meaning_whole() {
    if std::env::var_os("COXSWAIN_LONG_MEANING").is_none() {
        eprintln!("SKIPPED: set COXSWAIN_LONG_MEANING=1 to download the built-in model and read long documents");
        return;
    }
    let work = Path::new(env!("CARGO_TARGET_TMPDIR")).join("long-meaning");
    let home = work.join("home");
    let files = work.join("files");
    let _ = std::fs::remove_dir_all(&files);
    let _ = std::fs::remove_file(work.join("search.db"));
    std::fs::create_dir_all(&files).unwrap();
    // SAFETY: the only test of this binary; no other thread reads the environment.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
        std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
        std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
    }

    let t = Instant::now();
    coxswain_core::meaning::download(&Default::default()).unwrap();
    eprintln!("model: {:.0} s to fetch, {} MB", t.elapsed().as_secs_f64(), coxswain_core::meaning::size() >> 20);
    // `COXSWAIN_LONG_MEANING=part`: only the reader that runs out of time.
    if std::env::var("COXSWAIN_LONG_MEANING").as_deref() == Ok("part") {
        let mut failed = vec![];
        out_of_time(&work, &mut failed);
        assert!(failed.is_empty(), "{}", failed.join("\n"));
        return;
    }

    // (file, its facts)
    let mut corpus: Vec<(PathBuf, Vec<String>)> = vec![];
    for (i, (name, words)) in [("essay.docx", 60_000), ("report.odt", 60_000), ("almanac.docx", 700_000), ("diary.odt", 400_000)].iter().enumerate() {
        let (paras, facts) = text(i as u64 + 1, *words);
        let p = files.join(name);
        if name.ends_with(".docx") { docx(&p, &paras) } else { odt(&p, &paras) }
        corpus.push((p, facts));
    }
    // PDFs of about 100, 160 and 260 pages, made by LibreOffice from texts of their own.
    let made = work.join("made");
    std::fs::create_dir_all(&made).unwrap();
    let soffice = ["soffice", "libreoffice"].into_iter().find(|p| std::process::Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()));
    match soffice {
        None => eprintln!("SKIPPED: the PDFs (LibreOffice is not installed)"),
        Some(soffice) => {
            for (i, (name, words)) in [("manual", 65_000), ("thesis", 100_000), ("minutes", 160_000)].iter().enumerate() {
                let (paras, facts) = text(10 + i as u64, *words);
                let src = made.join(format!("{name}.odt"));
                odt(&src, &paras);
                let ok = std::process::Command::new(soffice)
                    .arg(format!("-env:UserInstallation=file://{}", home.join("lo").display()))
                    .args(["--headless", "--convert-to", "pdf", "--outdir"])
                    .arg(&files)
                    .arg(&src)
                    .output()
                    .unwrap()
                    .status
                    .success();
                assert!(ok, "LibreOffice made no {name}.pdf");
                corpus.push((files.join(format!("{name}.pdf")), facts));
            }
        }
    }
    for (p, facts) in &corpus {
        eprintln!("{}: {} KB, {} facts", p.file_name().unwrap().to_string_lossy(), std::fs::metadata(p).unwrap().len() / 1024, facts.len());
    }

    let cfg = SearchConfig {
        text_roots: vec![files.clone()],
        text: true,
        meaning: true,
        meaning_engine: "builtin".into(),
        long_files: "everything".into(),
        ..SearchConfig::default()
    };
    let store = Store::open(&work.join("search.db")).unwrap();
    store.set_engine(coxswain_core::meaning::Engine::from_config(&cfg));
    store.quiet.store(0, Ordering::Relaxed);
    store.hurry.store(true, Ordering::Relaxed);
    assert!(store.meaning.load(Ordering::Relaxed), "no engine");
    let (t, before) = (Instant::now(), peak_mb());
    store::scan(&store, &cfg, &AtomicBool::new(false)).unwrap();
    let secs = t.elapsed().as_secs_f64();
    assert_eq!(store.meaning_counts().0, 0, "{:?}", store.meaning_error.lock().unwrap());
    eprintln!(
        "first pass: {secs:.0} s, {} passages, {:.0} passages a second; store {} MB; peak memory {} MB (before the pass {before} MB)",
        store.passage_count(),
        store.passage_count() as f64 / secs,
        store.bytes() >> 20,
        peak_mb()
    );

    let db = rusqlite::Connection::open(work.join("search.db")).unwrap();
    let mut failed = vec![];
    for (p, facts) in &corpus {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let path = p.to_string_lossy().to_string();
        let (id, embedded, partial, whole): (i64, Option<i64>, Option<i64>, Option<i64>) = db
            .query_row("SELECT id, embedded, partial, whole FROM files WHERE path = ?1", [&path], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap();
        if embedded != Some(1) || partial.is_some() || whole.is_none() {
            failed.push(format!("{name}: embedded {embedded:?}, partial {partial:?}, whole {whole:?}"));
        }
        // The passages the store holds, in order: kept with their vectors, or cut from its text.
        let mut q = db.prepare("SELECT n, passage FROM chunks WHERE file = ?1 ORDER BY n").unwrap();
        let rows: Vec<(i64, Option<String>)> = q.query_map([id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
        if rows.iter().enumerate().any(|(i, (n, _))| *n != i as i64) {
            failed.push(format!("{name}: passages not numbered 0..{}", rows.len()));
        }
        let passages: Vec<String> = if rows.iter().all(|(_, t)| t.is_some()) {
            rows.iter().map(|(_, t)| squash(t.as_deref().unwrap())).collect()
        } else {
            let body: String = db.query_row("SELECT body FROM text WHERE rowid = ?1", [id], |r| r.get(0)).unwrap();
            let all = coxswain_core::meaning::every_passage(&body, &path);
            if all.len() != rows.len() {
                failed.push(format!("{name}: {} vectors for {} passages of its text", rows.len(), all.len()));
            }
            all.into_iter().map(|(p, _)| squash(&p.text)).collect()
        };
        let missing: Vec<&String> = facts.iter().filter(|f| !passages.iter().any(|p| p.contains(f.as_str()))).collect();
        if !missing.is_empty() {
            failed.push(format!("{name}: {} of {} facts in no passage, the first: {}", missing.len(), facts.len(), missing[0]));
        }
        // Found by meaning: every fact of the short files, every tenth of the long ones, and the
        // last, at the end of the file.
        let step = if facts.len() > 200 { 10 } else { 1 };
        let asked: Vec<&String> = facts.iter().step_by(step).chain(facts.last()).collect();
        let (mut first, mut five) = (0, 0);
        for f in &asked {
            let hits = store.similar(f, None, 5);
            let at = hits.iter().position(|h| h.path == *p);
            first += (at == Some(0)) as usize;
            five += at.is_some() as usize;
        }
        let last = store.similar(facts.last().unwrap(), None, 5);
        if !last.iter().any(|h| h.path == *p) {
            failed.push(format!("{name}: its last fact does not find it in the first five"));
        }
        eprintln!(
            "{name}: whole {whole:?}, {} passages, {} facts all in them: {}; by meaning first {first}/{n}, in the first five {five}/{n}",
            rows.len(),
            facts.len(),
            missing.is_empty(),
            n = asked.len()
        );
        if five * 100 < asked.len() * 95 {
            failed.push(format!("{name}: only {five} of {} facts find it in the first five", asked.len()));
        }
    }
    drop(store);
    out_of_time(&work, &mut failed);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    let _ = std::fs::remove_dir_all(&files);
    let _ = std::fs::remove_dir_all(&made);
}

/// A Word text whose reader runs out of its 20 seconds: over 4 MB of text, so it is read again
/// line by line for its passages, then 3.5 GB of tags with no text (the most read is 4 GB), then
/// more text. It gets the vectors of the part read, marked partial, is not read again at the next
/// scan, and is read once more a day later, keeping the larger part.
fn out_of_time(work: &Path, failed: &mut Vec<String>) {
    let files = work.join("slow");
    let _ = std::fs::remove_dir_all(&files);
    let _ = std::fs::remove_file(work.join("slow.db"));
    std::fs::create_dir_all(&files).unwrap();
    let path = files.join("slow.docx");
    let (paras, _) = text(99, 660_000);
    let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let o = zip::write::SimpleFileOptions::default().large_file(true);
    z.start_file("[Content_Types].xml", o).unwrap();
    z.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#).unwrap();
    z.start_file("_rels/.rels", o).unwrap();
    z.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#).unwrap();
    z.start_file("word/document.xml", o).unwrap();
    z.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#).unwrap();
    let filler = "<w:r><w:rPr><w:b/></w:rPr></w:r>".repeat(1 << 15);
    let split = paras.len() * 9 / 10;
    for (i, p) in paras.iter().enumerate() {
        if i == split {
            for _ in 0..3_500_000_000 / filler.len() {
                z.write_all(filler.as_bytes()).unwrap();
            }
        }
        z.write_all(format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", esc(p)).as_bytes()).unwrap();
    }
    z.write_all(b"</w:body></w:document>").unwrap();
    z.finish().unwrap();
    eprintln!("slow.docx: {} KB, {} paragraphs", std::fs::metadata(&path).unwrap().len() / 1024, paras.len());

    let cfg = SearchConfig {
        text_roots: vec![files.clone()],
        text: true,
        meaning: true,
        meaning_engine: "builtin".into(),
        long_files: "everything".into(),
        // The file is larger than the 20 MB files are read up to by default.
        text_max_size: 64 << 20,
        ..SearchConfig::default()
    };
    let store = Store::open(&work.join("slow.db")).unwrap();
    store.set_engine(coxswain_core::meaning::Engine::from_config(&cfg));
    store.quiet.store(0, Ordering::Relaxed);
    store.hurry.store(true, Ordering::Relaxed);
    let db = rusqlite::Connection::open(work.join("slow.db")).unwrap();
    let state = || -> (Option<i64>, Option<i64>, i64) {
        db.query_row(
            "SELECT embedded, partial, (SELECT count(*) FROM chunks c WHERE c.file = f.id) FROM files f WHERE path = ?1",
            [path.to_string_lossy()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    let t = Instant::now();
    store::scan(&store, &cfg, &AtomicBool::new(false)).unwrap();
    let first = state();
    let noted: i64 = db.query_row("SELECT count(*) FROM errors WHERE what LIKE '%time ran out%'", [], |r| r.get(0)).unwrap();
    eprintln!("slow.docx, first scan ({:.0} s): embedded {:?}, partial {:?}, {} passages; noted {noted}", t.elapsed().as_secs_f64(), first.0, first.1, first.2);
    if first.0 == Some(1) && first.1.is_none() {
        eprintln!("SKIPPED: the reader read slow.docx to its end within its time here; slow the process down while it reads (README)");
        drop(store);
        let _ = std::fs::remove_dir_all(&files);
        return;
    }
    if first.0 != Some(3) || first.1.is_none() || first.2 == 0 || noted == 0 {
        failed.push(format!("slow.docx: not kept in part after its time ran out: {first:?}, noted {noted}"));
        return;
    }
    store::scan(&store, &cfg, &AtomicBool::new(false)).unwrap();
    if state() != first {
        failed.push(format!("slow.docx: read again at once: {:?}", state()));
    }
    // A day later.
    db.execute("UPDATE files SET partial = partial - 90000 WHERE path = ?1", [path.to_string_lossy()]).unwrap();
    let t = Instant::now();
    store::scan(&store, &cfg, &AtomicBool::new(false)).unwrap();
    let again = state();
    eprintln!("slow.docx, a day later ({:.0} s): embedded {:?}, partial {:?}, {} passages", t.elapsed().as_secs_f64(), again.0, again.1, again.2);
    // Out of time again: the larger part, waiting until the file changes; or read to its end.
    if !matches!(again.0, Some(2) | Some(1)) || again.2 < first.2 {
        failed.push(format!("slow.docx: a day later {again:?}, before {first:?}"));
    }
    drop(store);
    let _ = std::fs::remove_dir_all(&files);
}

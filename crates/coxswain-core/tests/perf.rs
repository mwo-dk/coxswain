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
    let ix = Index::build(&[d.clone()], &[], None, None);
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

/// A home folder holding a zip of `n` small text files, for the search store.
fn home_with_zip(n: usize) -> (PathBuf, PathBuf) {
    let d = bench_dir().join(format!("home-zip-{n}"));
    let zip = d.join("docs.zip");
    if d.join(".done").exists() {
        return (d, zip);
    }
    let _ = std::fs::remove_dir_all(&d);
    let src = d.join("src");
    std::fs::create_dir_all(&src).unwrap();
    for i in 0..n {
        std::fs::write(src.join(format!("note-{i}.txt")), format!("note {i}: the rocket's fuel budget, flight {}\n", i % 97)).unwrap();
    }
    coxswain_core::archive::create(&zip, &[src.clone()]).unwrap();
    std::fs::remove_dir_all(&src).unwrap();
    std::fs::write(d.join(".done"), b"").unwrap();
    (d, zip)
}

/// One member of a zip of 10,000 changes: how long the store takes to follow it.
#[test]
#[ignore]
fn perf_store_archive_member_change() {
    use coxswain_core::store::{self, Store};
    use std::sync::atomic::AtomicBool;
    let (home, zip) = home_with_zip(10_000);
    let db = bench_dir().join("archive-store.db");
    let _ = std::fs::remove_file(&db);
    let cfg = coxswain_core::config::SearchConfig { text_roots: vec![home.clone()], archives: true, ..Default::default() };
    let store = Store::open(&db).unwrap();
    store.hurry.store(true, std::sync::atomic::Ordering::Relaxed);
    let go = AtomicBool::new(false);
    let t = Instant::now();
    store::scan(&store, &cfg, &go).unwrap();
    println!("scan a zip of 10k members: {:.0} ms, {} texts", ms(t), store.texts());
    let extra = home.join("extra.txt");
    std::fs::write(&extra, "a new note about the launch\n").unwrap();
    coxswain_core::archive::add(&zip, &[("src/extra.txt".into(), extra.clone())], None).unwrap();
    std::fs::remove_file(&extra).unwrap();
    // The watcher sees the change a few seconds after the archive was written.
    std::thread::sleep(std::time::Duration::from_millis(3100));
    let t = Instant::now();
    store::refresh(&store, &cfg, &mut [zip.clone()].into_iter().collect(), &mut Default::default(), &go).unwrap();
    println!("refresh after one member added: {:.0} ms, {} texts", ms(t), store.texts());
    assert_eq!(store.search("launch", None, 5).hits.len(), 1);
}

/// An embedding server that answers like Ollama, with a vector of letter counts per text:
/// what the store does with the vectors is measured, not the model.
fn fake_embed_server() -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for mut c in listener.incoming().flatten() {
            let mut got = Vec::new();
            let mut buf = [0u8; 65536];
            loop {
                let n = c.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got);
                if let Some(h) = text.find("\r\n\r\n") {
                    let len: usize = text[..h].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:")?.trim().parse().ok()).unwrap_or(0);
                    if got.len() >= h + 4 + len {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&got);
            let body = text.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("");
            let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            let vectors: Vec<Vec<f32>> = v["input"].as_array().map(|a| a.iter().map(|t| {
                let mut v = vec![0.0f32; 26];
                for b in t.as_str().unwrap_or("").bytes().filter(u8::is_ascii_alphabetic) {
                    v[(b.to_ascii_lowercase() - b'a') as usize] += 1.0;
                }
                v
            }).collect()).unwrap_or_default();
            let reply = serde_json::json!({ "embeddings": vectors }).to_string();
            let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
        }
    });
    url
}

/// Search by meaning over 400 files of 100 KB: what `similar` and `passages` cost once the
/// vectors are in, which is reading the passages of the closest files.
#[test]
#[ignore]
fn perf_meaning_closest() {
    use coxswain_core::store::{self, Store};
    use std::sync::atomic::AtomicBool;
    let d = bench_dir().join("home-meaning-400");
    if !d.join(".done").exists() {
        std::fs::create_dir_all(&d).unwrap();
        for i in 0..400 {
            let word = ["rocket", "budget", "zebra", "quartz", "launch", "orbit"][i % 6];
            let text: String = (0..12_000).map(|j| format!("{word} {} ", j % 50)).collect();
            std::fs::write(d.join(format!("doc-{i}.txt")), text).unwrap();
        }
        std::fs::write(d.join(".done"), b"").unwrap();
    }
    let db = bench_dir().join("meaning-store.db");
    let _ = std::fs::remove_file(&db);
    let url = fake_embed_server();
    let cfg = coxswain_core::config::SearchConfig { text_roots: vec![d.clone()], meaning: true, meaning_engine: "ollama".into(), meaning_url: url, meaning_model: "fake".into(), ..Default::default() };
    let store = Store::open(&db).unwrap();
    store.set_engine(coxswain_core::meaning::Engine::from_config(&cfg));
    store.hurry.store(true, std::sync::atomic::Ordering::Relaxed);
    let go = AtomicBool::new(false);
    let t = Instant::now();
    store::scan(&store, &cfg, &go).unwrap();
    println!("scan + vectors of 400 files: {:.0} ms, {:?} {:?}", ms(t), store.meaning_counts(), store.meaning_error.lock().unwrap());
    for q in ["zebra quartz", "rocket budget"] {
        let t = Instant::now();
        let hits = store.similar(q, None, 10);
        println!("similar {q:?}: {:.1} ms, {} hits", ms(t), hits.len());
        let t = Instant::now();
        let p = store.passages(q, None, 10);
        println!("passages {q:?}: {:.1} ms, {} passages", ms(t), p.len());
    }
}

/// A home folder laid out as on macOS, where the caches are not hidden: 10,000 files and 20
/// zips of 500 entries in projects, and 400 zips of 500 entries in `Library/Caches` and
/// `Library/Application Support`.
fn home_with_caches() -> PathBuf {
    let d = bench_dir().join("home-caches");
    if d.join(".done").exists() {
        return d;
    }
    let _ = std::fs::remove_dir_all(&d);
    let src = d.join("member-src");
    std::fs::create_dir_all(&src).unwrap();
    for i in 0..500 {
        std::fs::write(src.join(format!("member-{i}.txt")), b"x").unwrap();
    }
    for i in 0..200 {
        let sub = d.join("home/projects").join(format!("p{i}"));
        std::fs::create_dir_all(&sub).unwrap();
        for j in 0..50 {
            std::fs::write(sub.join(format!("mod_{i}_{j}.{}", EXTS[j % 9])), b"").unwrap();
        }
    }
    let first = d.join("home/projects/a0.zip");
    coxswain_core::archive::create(&first, &[src.clone()]).unwrap();
    let places = (0..20).map(|i| format!("projects/a{i}.zip")).chain((0..300).map(|i| format!("Library/Caches/pkgs/c{i}.zip"))).chain((0..100).map(|i| format!("Library/Application Support/app/s{i}.zip")));
    for p in places {
        let to = d.join("home").join(p);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        if to != first {
            std::fs::copy(&first, &to).unwrap();
        }
    }
    std::fs::write(d.join(".done"), b"").unwrap();
    d
}

/// The name index of `home_with_caches`, built as `COXSWAIN_BENCH_ARCHIVES` says: `off`,
/// `default` or `everywhere`. One per process, for its memory.
#[test]
#[ignore]
fn perf_index_archive_scope() {
    let home = home_with_caches().join("home");
    let how = std::env::var("COXSWAIN_BENCH_ARCHIVES").unwrap_or_else(|_| "default".into());
    // macOS's places, under this home.
    unsafe {
        std::env::set_var("XDG_CACHE_HOME", home.join("Library/Caches"));
        std::env::set_var("XDG_DATA_HOME", home.join("Library/Application Support"));
    }
    let cfg = coxswain_core::config::SearchConfig { text_roots: vec![home.clone()], archives_everywhere: how == "everywhere", ..Default::default() };
    let archives = (how != "off").then_some(&cfg);
    let before = rss_mb();
    let mut times = vec![];
    let mut ix = Index::default();
    for _ in 0..3 {
        drop(ix);
        let t = Instant::now();
        ix = Index::build(std::slice::from_ref(&home), &[], archives, None);
        times.push(ms(t));
    }
    times.sort_by(f64::total_cmp);
    println!("archives {how}: build {:.0} ms (median of 3), {} names, memory +{:.0} MB", times[1], ix.len(), rss_mb() - before);
}

/// Passages per second of the built-in model, on the CPU and where `auto` puts it (the GPU
/// through Metal on a Mac that has one). Needs the model downloaded (`coxswain --meaning on`).
#[test]
#[ignore]
fn perf_meaning_cpu_and_gpu() {
    use coxswain_core::meaning::Embedder;
    let texts: Vec<String> = (0..256).map(|i| format!("Passage {i}: {}", "the fuel budget of flight seven is the largest cost of the launch, and ".repeat(8))).collect();
    for cpu_only in [true, false] {
        let Some(e) = Embedder::load(cpu_only) else { return println!("the model is not downloaded") };
        let runs = e.runs();
        e.passages(&texts[..8]);
        let t = Instant::now();
        e.passages(&texts);
        println!("{}: {:.0} passages/s ({:?})", if cpu_only { "cpu " } else { "auto" }, texts.len() as f64 / t.elapsed().as_secs_f64(), runs);
    }
}

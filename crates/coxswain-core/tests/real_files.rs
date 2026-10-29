//! Real files, made by the programs that make them (Excel's `.xlsb`, ChessBase's `.cbv`), not by
//! us: each is downloaded from where it is published, pinned by its SHA-256, kept in
//! `target/tmp/real-files` (never the user's cache) and read as the preview, the search store and
//! the chess board read it. Opt-in: `COXSWAIN_REAL_FILES=1 cargo test -p coxswain-core --test
//! real_files -- --nocapture`; the nightly run sets it. A file that cannot be downloaded (offline,
//! the site down) is skipped and said so; a file whose bytes changed fails, so a moved file is
//! noticed. Sources and licences: `tests/README.md`. No file is committed.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use coxswain_core::chess::games::Library;
use coxswain_core::config::SearchConfig;
use coxswain_core::store::{self, Store};

struct Real {
    /// The name it is kept under.
    name: &'static str,
    url: &'static str,
    sha256: &'static str,
    /// Texts its preview and search must have; the longest word of the first is searched for in
    /// the store.
    has: &'static [&'static str],
    /// A database's games, and whether some of them are annotated (texts, symbols, variations).
    games: usize,
    notes: bool,
}

const SHEETS: &[Real] = &[
    // Saved by Excel 2010, 2013 and 2016 (MIT, DBHeise/fileid.testfiles).
    Real { name: "excel-2010.xlsb", url: concat!("https://media.githubusercontent.com/media/DBHeise/fileid.testfiles/c79fc54b8f22fccdb8db6f29913745480c9e6b30/xlsb/Microsoft%20Office", "/O14/14.0.4756.1000/Excel_good.xlsb"), sha256: "c7fa152c1a7388836287e62e1e73000fdef12245299f46c686486feb01010da6", has: &[], games: 0, notes: false },
    Real { name: "excel-2013.xlsb", url: concat!("https://media.githubusercontent.com/media/DBHeise/fileid.testfiles/c79fc54b8f22fccdb8db6f29913745480c9e6b30/xlsb/Microsoft%20Office", "/O15/15.0.5153.1000/Excel_good.xlsb"), sha256: "aae830a2aee21cb22bde4b979e7c60549b76ee443c9723cd73445086addff57e", has: &[], games: 0, notes: false },
    Real { name: "excel-2016.xlsb", url: concat!("https://media.githubusercontent.com/media/DBHeise/fileid.testfiles/c79fc54b8f22fccdb8db6f29913745480c9e6b30/xlsb/Microsoft%20Office", "/O16/16.0.11727.20244/Excel_good.xlsb"), sha256: "6ce02cce8775a81da0c950be8ea9777c3ce776f21a2c8396708e4c34b6245158", has: &[], games: 0, notes: false },
    // Greek: Attica's vehicle inspections of 2018 (CC BY 4.0, data.gov.gr).
    Real { name: "attica-kteo-2018.xlsb", url: "https://data.gov.gr/dataset/734690a3-cc75-4ec6-a9f4-1c9cdf487e99/resource/ae967b10-fbad-459e-b35e-f717f8216a18/download/2018-.xlsb", sha256: "a62be1c931632ab1194dd603aa8f2b5a785e7f958595bf1a557493e08e94f839", has: &["ΑΠΟΛΟΓΙΣΜΟΣ ΕΤΟΥΣ 2018"], games: 0, notes: false },
    // Barnet's absence dashboard, September 2019 (Open Government Licence, open.barnet.gov.uk).
    Real { name: "barnet-absence-2019-09.xlsb", url: "https://open.barnet.gov.uk/download/e7zn2/xxv/2019-10-01%20Absence%20Management%20Dashboard%20Sept%2019.xlsb", sha256: "ba7254c8ce892d52da419fa193f474e33b3f5b9fcaf11ad61f45a3566a04cf1a", has: &["Absence by Management Unit", "2018-10-01"], games: 0, notes: false },
    // Large: 5.7 MB, 17 sheets, a million cells (Environment Agency, Open Government Licence).
    Real { name: "wrmp24-supply-demand.xlsb", url: "https://environment.data.gov.uk/api/file/download?fileDataSetId=78599739-b9b0-4e6a-8b4e-ce311f238219&fileName=WRMP24%20Supply-Demand%20Data%2004102024.xlsb", sha256: "943530b02229638ebed40c0ab46bddfbeaae1de64c9a1afe4557df296615d0be", has: &["WRZ short code"], games: 0, notes: false },
    // Large: 9.3 MB, 65 sheets of formulas; 5.8 MB, 154 sheets (Yorkshire Water, CC BY 4.0,
    // Data Mill North).
    Real { name: "yorkshire-water-pr19-financial-model.xlsb", url: "https://datamillnorth.org/download/2ylqq/99f694a7-ca9d-43d5-9e60-c17e4534f60b/PR19%20financial%20model.xlsb", sha256: "aa89bcddab8973f5d25920f2c7486e92fb6571581cae59bd1399df2cb074efb4", has: &["PR19-14h-financial model - final"], games: 0, notes: false },
    Real { name: "yorkshire-water-pr19-tables.xlsb", url: "https://datamillnorth.org/download/2ylqq/88a0fc45-b580-41a9-b9ef-9a875f3c4c9e/YKY%20PR19%20Business%20plan%20tables%20June%2018.xlsb", sha256: "413f5440700ae851d12b4543025985a4d42a5edf80561024ad1649b6d432211a", has: &["Yorkshire Water Services Ltd"], games: 0, notes: false },
];
const CHESS: &[Real] = &[
    // The Week in Chess 1134 as TWIC published it in ChessBase's format (kept in uncbv's tests,
    // GPL-3.0; the games are TWIC's, free for personal use).
    Real { name: "twic1134.cbv", url: concat!("https://raw.githubusercontent.com/antoyo/uncbv/bf93b9d2d5b70db300d572231e95abf199010503/tests", "/twic1134.cbv"), sha256: "b87ec2039d2f34941f73e17e36cc01e090286f0054437a9a5e0e279acbf4ed3f", has: &["Vachier Lagrave, M"], games: 6117, notes: false },
    // Northwest Chess: Washington tournaments, and two months of the magazine's games, annotated
    // (nwchess.com, published for download, no licence stated).
    Real { name: "WA_Champ_2008.cbv", url: "https://www.nwchess.com/articles/games/WA_Champ_2008.cbv", sha256: "407247e6b5190e39dc9b02318da425c7f2a0c195bb08156a60be58a248f21f28", has: &["Greninger, Harley"], games: 135, notes: true },
    Real { name: "WA_ChCup_2007.cbv", url: "https://www.nwchess.com/articles/games/WA_ChCup_2007.cbv", sha256: "f89162dda59332297aecab77814170573f27f7c30ac3bda29a719898c84ccb2d", has: &["Challengers Cup"], games: 48, notes: true },
    Real { name: "WA_Class_2009.cbv", url: "https://www.nwchess.com/articles/games/WA_Class_2009.cbv", sha256: "a464ff8e9e35f3f4b1c4d8f86df875f7a413c76c35f44ecdd5ff450a14daad18", has: &["Washington Class Ch"], games: 194, notes: true },
    Real { name: "WA_Open_2008.cbv", url: "https://www.nwchess.com/articles/games/WA_Open_2008.cbv", sha256: "c43e815d20e58f81f849e63e8b734e9d5450bf7f90a14a0d106e67eba46f2314", has: &["Washington Open"], games: 217, notes: true },
    Real { name: "WA_Open_2010.cbv", url: "https://www.nwchess.com/articles/games/WA_Open_2010.cbv", sha256: "a1488ed7c2b5a929ffaff37686284b833789d012a929c1e09abf61b33bb67fdc", has: &["Cozianu, Costin"], games: 230, notes: true },
    Real { name: "NWC-2012-12-annotated.cbv", url: "https://www.nwchess.com/articles/games/published/NWC%202012-12%20Published%20Games.cbv", sha256: "d3e682f2c1138260df295eeabca6de182cb1bf2697b096448034133884cd3c79", has: &["Botvinnik Memorial"], games: 22, notes: true },
    Real { name: "NWC-2013-01-annotated.cbv", url: "https://www.nwchess.com/articles/games/published/NWC%202013-01%20Published%20Games.cbv", sha256: "7cb2805d2debadd6daacb7e32519023a5680fccf6d55f1755f802223ce49f664", has: &["Chiburdanidze, Maia"], games: 47, notes: false },
    // A PGN database backed up by ChessBase: the archive holds a .pgn, no .cbh.
    Real { name: "NWC-2026-02-pgn.cbv", url: "https://www.nwchess.com/articles/games/published/NWC%202026-02%20Published%20Raw.cbv", sha256: "b93dfa2d52d92bd98351a67e6662d439101a3e59babd464a4493a9c8df38f307", has: &["Portland, OR"], games: 24, notes: true },
    Real { name: "NWC-2026-10.cbv", url: "https://www.nwchess.com/articles/games/published/NWC%202026-10%20Published%20Raw.cbv", sha256: "43c53ae51606434c3f6a02ea10d9af2b96cfae061c90c15501dd7dbc5508948b", has: &["Vancouver Open"], games: 7, notes: true },
];

fn cache() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("real-files")
}

fn sha256(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// The file, from the cache or downloaded; None (and said) when it cannot be downloaded.
fn fetch(f: &Real) -> Option<PathBuf> {
    let path = cache().join(f.name);
    if std::fs::read(&path).is_ok_and(|b| sha256(&b) == f.sha256) {
        return Some(path);
    }
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(300))).build().into();
    let body = agent.get(f.url).call().and_then(|r| r.into_body().into_with_config().limit(64 << 20).read_to_vec());
    let body = match body {
        Ok(b) => b,
        Err(e) => {
            // A warning on the nightly run's page (a workflow command, on stdout).
            println!("::warning title=Real file skipped::{}: cannot download {}: {e}", f.name, f.url);
            return None;
        }
    };
    assert_eq!(sha256(&body), f.sha256, "{} changed at {}: check it and pin it again", f.name, f.url);
    std::fs::create_dir_all(cache()).unwrap();
    std::fs::write(&path, body).unwrap();
    Some(path)
}

fn check_sheet(path: &Path, f: &Real, failed: &mut Vec<String>) {
    let started = Instant::now();
    // The preview (facts::as_text) and the search store's text (extract::text_of).
    let preview = coxswain_core::extract::sheet::text(path, u64::MAX).unwrap_or_default();
    let size = std::fs::metadata(path).unwrap().len();
    let text = coxswain_core::extract::text_of(path, size, u64::MAX).unwrap_or_default();
    let mut lines = 0usize;
    let whole = coxswain_core::extract::whole_lines(path, &mut |_| lines += 1);
    eprintln!(
        "{}: preview {} B, text {} B, {lines} lines whole ({whole:?}) in {:.1} s",
        f.name,
        preview.len(),
        text.len(),
        started.elapsed().as_secs_f64()
    );
    for want in f.has {
        if !preview.contains(want) {
            failed.push(format!("{}: preview lacks {want:?}", f.name));
        }
        if !text.contains(want) {
            failed.push(format!("{}: search text lacks {want:?}", f.name));
        }
    }
    if whole != Some(true) {
        failed.push(format!("{}: whole_lines gave {whole:?}", f.name));
    }
}

fn check_chess(path: &Path, f: &Real, failed: &mut Vec<String>) {
    let started = Instant::now();
    let lib = match Library::open(path) {
        Ok(l) => l,
        Err(e) => return failed.push(format!("{}: does not open: {e}", f.name)),
    };
    while !lib.progress().done && started.elapsed() < coxswain_core::test_limit(Duration::from_secs(60)) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let p = lib.progress();
    if !p.done || p.error.is_some() || p.games == 0 {
        return failed.push(format!("{}: listed {} games, done {}, error {:?}", f.name, p.games, p.done, p.error));
    }
    let (mut moves, mut notes, mut bad) = (0usize, 0usize, vec![]);
    let cut = coxswain_core::t!("chess.cbh_cut");
    for i in 0..p.games as u32 {
        match lib.game(i) {
            Ok(g) => {
                moves += g.nodes.len();
                notes += g.nodes.iter().filter(|n| !n.after.is_empty() || !n.before.is_empty() || !n.nags.is_empty()).count();
                if let Some(e) = g.error {
                    bad.push(format!("game {}: {e}", i + 1));
                }
                if lib.text(i).is_ok_and(|t| t.contains(&cut)) {
                    bad.push(format!("game {}: cut short", i + 1));
                }
            }
            Err(e) => bad.push(format!("game {}: {e}", i + 1)),
        }
    }
    let first = lib.row(0).unwrap_or_default();
    eprintln!(
        "{}: {} games, {moves} moves, {notes} annotated, first {} - {} ({}), in {:.1} s",
        f.name,
        p.games,
        first.white,
        first.black,
        first.event,
        started.elapsed().as_secs_f64()
    );
    if f.games != 0 && p.games != f.games {
        failed.push(format!("{}: {} games, not {}", f.name, p.games, f.games));
    }
    if f.notes && notes == 0 {
        failed.push(format!("{}: no annotation read", f.name));
    }
    if !bad.is_empty() {
        failed.push(format!("{}: {} of {} games not read whole: {}", f.name, bad.len(), p.games, bad[..bad.len().min(5)].join("; ")));
    }
    let text = coxswain_core::chess::games::search_text(path, 4 << 20).unwrap_or_default();
    for want in f.has {
        if !text.contains(want) {
            failed.push(format!("{}: search text lacks {want:?}", f.name));
        }
    }
}

/// The store reads the folder of them all: each file is found by a word of its own.
fn check_store(files: &[(PathBuf, &Real)], failed: &mut Vec<String>) {
    let d = cache().join(format!("store-{}", std::process::id()));
    let root = d.join("files");
    std::fs::create_dir_all(&root).unwrap();
    for (p, _) in files {
        std::fs::copy(p, root.join(p.file_name().unwrap())).unwrap();
    }
    let root = root.canonicalize().unwrap();
    let cfg = SearchConfig { text_roots: vec![root.clone()], meaning: false, history: false, ..SearchConfig::default() };
    let store = Store::open(&d.join("search.db")).unwrap();
    store.quiet.store(0, std::sync::atomic::Ordering::Relaxed);
    store.hurry.store(true, std::sync::atomic::Ordering::Relaxed);
    store::scan(&store, &cfg, &AtomicBool::new(false)).unwrap();
    for (p, f) in files {
        let Some(word) = f.has.first().and_then(|h| h.split_whitespace().max_by_key(|w| w.chars().count())) else { continue };
        let (hits, _) = store.search_words(word, None, 50);
        if !hits.hits.iter().any(|h| h.path.file_name() == p.file_name()) {
            failed.push(format!("{}: the store does not find {word:?}", f.name));
        }
    }
    drop(store);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn real_files() {
    if std::env::var_os("COXSWAIN_REAL_FILES").is_none() {
        eprintln!("SKIPPED: set COXSWAIN_REAL_FILES=1 to download and read the real files (tests/README.md)");
        return;
    }
    // A home of its own: an archive is unpacked into the cache, the store keeps its things there.
    let home = cache().join("home");
    std::fs::create_dir_all(&home).unwrap();
    // SAFETY: the only test of this binary, before it starts a thread.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
        std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
        std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
    }
    let mut failed = vec![];
    let mut got = vec![];
    for f in SHEETS {
        if let Some(p) = fetch(f) {
            check_sheet(&p, f, &mut failed);
            got.push((p, f));
        }
    }
    for f in CHESS {
        if let Some(p) = fetch(f) {
            check_chess(&p, f, &mut failed);
            got.push((p, f));
        }
    }
    check_store(&got, &mut failed);
    eprintln!("{} of {} real files read", got.len(), SHEETS.len() + CHESS.len());
    assert!(failed.is_empty(), "real files read wrong:\n{}", failed.join("\n"));
}

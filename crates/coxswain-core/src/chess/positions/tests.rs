use super::*;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!("coxswain-positions-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn opened_lib(p: &Path) -> Arc<Library> {
    let l = Arc::new(Library::open(p).unwrap());
    let until = std::time::Instant::now() + crate::test_limit(std::time::Duration::from_secs(10));
    while !l.progress().done {
        assert!(std::time::Instant::now() < until, "the table never filled");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    l
}

/// Builds the index and waits for it: the job, for its counts.
fn built(lib: &Arc<Library>, depth: u32) -> Arc<Job> {
    build(lib.clone(), depth);
    let j = job(&lib.path).expect("a job");
    let until = std::time::Instant::now() + crate::test_limit(std::time::Duration::from_secs(30));
    while j.phase.load(Ordering::Relaxed) < 2 {
        assert!(std::time::Instant::now() < until, "the build never ended");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(j.phase.load(Ordering::Relaxed), 2, "{:?}", j.error.lock().unwrap());
    j
}

fn game(white: &str, black: &str, welo: u16, belo: u16, year: u32, result: &str, fen: Option<&str>, moves: &str) -> String {
    let mut s = format!("[White \"{white}\"]\n[Black \"{black}\"]\n[Result \"{result}\"]\n[Date \"{year}.01.01\"]\n");
    if welo > 0 {
        s += &format!("[WhiteElo \"{welo}\"]\n");
    }
    if belo > 0 {
        s += &format!("[BlackElo \"{belo}\"]\n");
    }
    if let Some(f) = fen {
        s += &format!("[SetUp \"1\"]\n[FEN \"{f}\"]\n");
    }
    format!("{s}\n{moves} {result}\n\n")
}

fn file(d: &Dir, name: &str, games: &[String]) -> PathBuf {
    let p = d.0.join(name);
    std::fs::File::create(&p).unwrap().write_all(games.concat().as_bytes()).unwrap();
    p
}

fn fen_after(moves: &[&str]) -> String {
    let mut b = Board::default();
    for m in moves {
        let mv = pgn::san_move(&b, m).unwrap();
        b.play_unchecked(mv);
    }
    pgn::fen(&b)
}

#[test]
fn entries_back_as_they_went_each_once() {
    let mut seed = 7u64;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut all: Vec<Entry> = (0..20_000).map(|i| entry(if i % 3 == 0 { 42 } else { rnd() }, (rnd() >> 40) as u32, rnd() as u16)).collect();
    all.extend_from_slice(&all[..500].to_vec());
    all.sort_unstable();
    let mut w = Writer::new(vec![]);
    for &e in &all {
        w.push(e).unwrap();
    }
    let (bytes, blocks, n) = w.finish().unwrap();
    all.dedup();
    assert_eq!(n as usize, all.len());
    assert_eq!(blocks.len(), all.len().div_ceil(BLOCK));
    let (mut r, mut buf, mut block, mut back) = (&bytes[..], vec![], vec![], vec![]);
    while read_block(&mut r, &mut buf, &mut block).unwrap() {
        back.extend_from_slice(&block);
    }
    assert_eq!(back, all);
    // Smaller than the 14 bytes an entry has in full.
    assert!(bytes.len() < all.len() * 14, "{} bytes", bytes.len());
}

/// A position is the same however the moves came to it, but only with the same side to move,
/// castling rights and en passant capture.
#[test]
fn transpositions_meet_and_side_castling_en_passant_part() {
    let k = |fen: &str| Chess.key(&Chess.start(Some(fen)).unwrap());
    assert_eq!(k(&fen_after(&["Nf3", "d5", "d4"])), k(&fen_after(&["d4", "d5", "Nf3"])));
    let w = "r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1";
    assert_ne!(k(w), k(&w.replace(" w ", " b ")));
    assert_ne!(k(w), k(&w.replace("KQkq", "Kkq")));
    // e2-e4 with a black pawn on d4: the capture en passant can be made, a position of its own;
    // without a pawn beside it, the same position as with no en passant square.
    let ep = "4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1";
    assert_ne!(k(ep), k(&ep.replace(" e3 ", " - ")));
    let none = "4k3/8/8/8/p3P3/8/8/4K3 b - e3 0 1";
    assert_eq!(k(none), k(&none.replace(" e3 ", " - ")));
}

#[test]
fn the_tree_counts_each_move_by_the_side_making_it() {
    let d = Dir::new();
    let games = [
        // Two move orders that meet after 1.Nf3 d5 2.d4 / 1.d4 d5 2.Nf3.
        game("Anand", "Carlsen", 2800, 2850, 2001, "1-0", None, "1.Nf3 d5 2.d4 Nf6 3.c4"),
        game("Kasparov", "Karpov", 2850, 2700, 1985, "0-1", None, "1.d4 d5 2.Nf3 Bf5"),
        game("Tal", "Petrosian", 0, 2600, 1960, "1/2-1/2", None, "1.d4 d5 2.Nf3 Nf6"),
        // The same pieces with White to move: not the position above.
        game("X", "Y", 1500, 1500, 2020, "1-0", Some(&fen_after(&["Nf3", "d5", "d4"]).replace(" b ", " w ")), "1.e3"),
    ];
    let p = file(&d, "meet.pgn", &games);
    let lib = opened_lib(&p);
    assert!(tree(&lib, pgn::START).is_none(), "no index yet");
    let st = status(&lib, 30);
    assert!(st.can && !st.ready && st.estimate_bytes > 0);
    built(&lib, 30);
    let t = tree(&lib, &fen_after(&["Nf3", "d5", "d4"])).unwrap();
    assert_eq!(t.games, 3);
    let nf6 = t.moves.iter().find(|m| m.san == "Nf6").unwrap();
    let bf5 = t.moves.iter().find(|m| m.san == "Bf5").unwrap();
    assert_eq!((t.moves[0].san.as_str(), nf6.games, bf5.games), ("Nf6", 2, 1));
    // Black's move: Black's Elo and Black's best player, whatever the move order.
    assert_eq!((nf6.white, nf6.draws, nf6.black), (50, 50, 0));
    assert_eq!((nf6.elo, nf6.best.clone()), (Some(2725), Some(("Carlsen".to_string(), 2850))));
    assert_eq!((nf6.first, nf6.last), (Some(1960), Some(2001)));
    assert_eq!((bf5.elo, bf5.best.clone(), bf5.black), (Some(2700), Some(("Karpov".to_string(), 2700)), 100));
    // A White move at the start: White's Elos (Tal has none).
    let start = tree(&lib, pgn::START).unwrap();
    let d4 = start.moves.iter().find(|m| m.san == "d4").unwrap();
    assert_eq!((start.games, d4.games, d4.elo, d4.best.clone()), (3, 2, Some(2850), Some(("Kasparov".to_string(), 2850))));
    assert_eq!(d4.fen, fen_after(&["d4"]));
    // The games reaching the position, and the table cut to them.
    assert_eq!(games_at(&lib, &fen_after(&["d4", "d5", "Nf3"])).unwrap(), vec![0, 1, 2]);
    let q = |position: &str| lib.query(&crate::chess::Query { position: position.into(), ..Default::default() });
    assert_eq!(q(&fen_after(&["d4", "d5", "Nf3", "Bf5"])), vec![1]);
    assert_eq!(q(&fen_after(&["Nf3", "d5", "d4"]).replace(" b ", " w ")), vec![3]);
    assert_eq!(q(""), vec![0, 1, 2, 3]);
    // Past the depth: reached, no moves.
    remove(&p);
    assert!(tree(&lib, pgn::START).is_none());
    built(&lib, 1);
    let t = tree(&lib, &fen_after(&["d4", "d5"])).unwrap();
    assert_eq!((t.games, t.moves.len(), t.depth), (2, 0, 1));
    remove(&p);
}

/// Random games, their tree checked against one counted from each game played out in full.
#[test]
fn the_tree_is_the_games_counted_kept_or_not() {
    let d = Dir::new();
    let mut seed = 11u64;
    let mut rnd = move |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n.max(1)
    };
    let mut texts = vec![];
    let mut lines = vec![];
    for g in 0..300 {
        let mut b = Board::default();
        let mut sans = vec![];
        for ply in 0..16 {
            let mut ms = vec![];
            b.generate_moves(|m| {
                ms.extend(m);
                false
            });
            if ms.is_empty() {
                break;
            }
            // Few choices early, so positions are shared by many games.
            let m = ms[if ply < 4 { rnd(2) } else { rnd(ms.len()) }];
            sans.push(pgn::san(&b, m));
            b.play_unchecked(m);
        }
        let moves = sans.join(" ");
        let result = ["1-0", "0-1", "1/2-1/2", "*"][rnd(4)];
        texts.push(game(&format!("W{g}"), &format!("B{g}"), 1000 + rnd(1800) as u16, if g % 5 == 0 { 0 } else { 1000 + rnd(1800) as u16 }, 1900 + rnd(120) as u32, result, None, &moves));
        lines.push(sans);
    }
    let p = file(&d, "random.pgn", &texts);
    let lib = opened_lib(&p);
    let j = built(&lib, 0);
    assert_eq!(j.walked.load(Ordering::Relaxed), 300);
    // Counted by hand: position (by key: move orders that meet are one) -> move -> games.
    let mut want: BTreeMap<u64, (String, BTreeMap<String, BTreeSet<u32>>)> = BTreeMap::new();
    for (g, sans) in lines.iter().enumerate() {
        let mut b = Board::default();
        for s in sans {
            let e = want.entry(crate::chess::polyglot::key(&b)).or_insert_with(|| (pgn::fen(&b), BTreeMap::new()));
            e.1.entry(s.clone()).or_default().insert(g as u32);
            b.play_unchecked(pgn::san_move(&b, s).unwrap());
        }
    }
    let idx = opened(&p).unwrap();
    let (mut hot, mut cold) = (0, 0);
    for (&key, (fen, moves)) in &want {
        let t = tree(&lib, fen).unwrap();
        let got: BTreeMap<String, u32> = t.moves.iter().map(|m| (m.san.clone(), m.games)).collect();
        let expected: BTreeMap<String, u32> = moves.iter().map(|(s, gs)| (s.clone(), gs.len() as u32)).collect();
        assert_eq!(got, expected, "{fen}");
        let kept = idx.kept(key).unwrap();
        if kept.is_empty() {
            cold += 1;
        } else {
            hot += 1;
            // What was kept is what the rows count.
            let mut found = vec![];
            idx.entries(key, |g, c| {
                found.push((g, c));
                true
            })
            .unwrap();
            assert_eq!(kept, tally(&found, &|g| lib.rec(g).map(|r| Info::of(&r))), "{fen}");
        }
    }
    assert!(hot > 3 && cold > 100, "{hot} kept, {cold} counted");
    remove(&p);
}

/// Random games of 16 half-moves each from game `seed`'s own sequence: their move texts.
fn random_lines(n: usize, seed: u64) -> Vec<Vec<String>> {
    let mut seed = seed;
    let mut rnd = move |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n.max(1)
    };
    (0..n)
        .map(|_| {
            let mut b = Board::default();
            let mut sans = vec![];
            for _ in 0..16 {
                let mut ms = vec![];
                b.generate_moves(|m| {
                    ms.extend(m);
                    false
                });
                if ms.is_empty() {
                    break;
                }
                let m = ms[rnd(ms.len())];
                sans.push(pgn::san(&b, m));
                b.play_unchecked(m);
            }
            sans
        })
        .collect()
}

/// A rare position costs a block or two to look up however many games the index has, and a
/// common one a binary search in `tree`: counted, not timed.
#[test]
fn a_look_costs_the_same_in_a_bigger_index() {
    let d = Dir::new();
    let reads = |n: usize| {
        let lines = random_lines(n, 5);
        let texts: Vec<String> = lines.iter().map(|l| game("a", "b", 2000, 2000, 2000, "1-0", None, &l.join(" "))).collect();
        let p = file(&d, &format!("n{n}.pgn"), &texts);
        let lib = opened_lib(&p);
        built(&lib, 0);
        let idx = opened(&p).unwrap();
        let rare = fen_after(&lines[0].iter().map(|s| s.as_str()).collect::<Vec<_>>()[..12]);
        let before = idx.reads.load(Ordering::Relaxed);
        assert_eq!(tree(&lib, &rare).unwrap().games, 1);
        let rare = idx.reads.load(Ordering::Relaxed) - before;
        let before = idx.reads.load(Ordering::Relaxed);
        assert_eq!(tree(&lib, pgn::START).unwrap().games as usize, n);
        let common = idx.reads.load(Ordering::Relaxed) - before;
        let blocks = idx.keys.len();
        remove(&p);
        (rare, common, blocks)
    };
    let (small, big) = (reads(600), reads(4800));
    assert!(big.2 >= small.2 * 6, "the index grew: {small:?} {big:?}");
    // The rare one: a look in `tree` (a binary search) that finds nothing, then a block or two.
    assert!(big.0 <= small.0 + 4 && big.1 <= small.1 + 4, "{small:?} {big:?}");
}

/// A PGN file that grew keeps the old entries and plays out only the games added (and the last
/// old one); one that changed otherwise is read again whole. The old index is used, marked out
/// of date, until the new one is there.
#[test]
fn a_grown_file_reads_only_its_new_games() {
    let d = Dir::new();
    let one = |w: &str, moves: &str| game(w, "b", 2000, 2000, 2000, "1-0", None, moves);
    let first: Vec<String> = (0..50).map(|i| one(&format!("w{i}"), "1.e4 e5")).collect();
    let p = file(&d, "grow.pgn", &first);
    let lib = opened_lib(&p);
    let j = built(&lib, 30);
    assert_eq!(j.walked.load(Ordering::Relaxed), 50);
    drop(lib);
    std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all([one("new1", "1.d4 d5"), one("new2", "1.d4 Nf6")].concat().as_bytes()).unwrap();
    let lib = opened_lib(&p);
    let st = status(&lib, 30);
    assert!(st.ready && st.stale);
    let t = tree(&lib, pgn::START).unwrap();
    assert!(t.stale && t.games == 50);
    // Its counts are shown, but its game numbers are of the file before it grew: none given.
    // Its games' rows are still the same games (the file only grew): the best player is there.
    assert!(!t.opens && t.moves.iter().all(|m| m.best.is_some()));
    assert_eq!(games_at(&lib, pgn::START), None);
    keep_up(&lib, &crate::config::ChessConfig::default());
    let j = job(&p).unwrap();
    let until = std::time::Instant::now() + crate::test_limit(std::time::Duration::from_secs(30));
    while j.phase.load(Ordering::Relaxed) < 2 {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(j.walked.load(Ordering::Relaxed), 3, "the last old game and the two new ones");
    let t = tree(&lib, pgn::START).unwrap();
    assert!(!t.stale && t.opens);
    assert_eq!(games_at(&lib, pgn::START).map(|g| g.len()), Some(52));
    assert_eq!((t.games, t.moves.iter().map(|m| (m.san.as_str(), m.games)).collect::<Vec<_>>()), (52, vec![("e4", 50), ("d4", 2)]));
    assert_eq!(folders(&p).len(), 1, "the old index went");
    // Changed in its middle: read again whole.
    drop(lib);
    let text = std::fs::read_to_string(&p).unwrap().replacen("1.e4 e5", "1.c4 e5", 1);
    std::fs::write(&p, text).unwrap();
    let lib = opened_lib(&p);
    // Stale, of a file changed in its middle: counts, but no game numbers and no rows read by
    // them (the best player would be another game's).
    let t = tree(&lib, pgn::START).unwrap();
    assert!(t.stale && !t.opens && t.games == 52 && t.moves.iter().all(|m| m.best.is_none()));
    assert_eq!(games_at(&lib, pgn::START), None);
    assert!(!opens(&lib));
    assert!(lib.query(&super::super::games::Query { position: pgn::START.into(), ..Default::default() }).is_empty());
    keep_up(&lib, &crate::config::ChessConfig::default());
    let j = job(&p).unwrap();
    while j.phase.load(Ordering::Relaxed) < 2 {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(j.walked.load(Ordering::Relaxed), 52);
    // Kept for Disk use, then removed: nothing left.
    assert!(kept().iter().any(|k| k.source == p && k.bytes > 0));
    assert!(remove(&p) > 0);
    assert!(folders(&p).is_empty() && !kept().iter().any(|k| k.source == p));
}

/// With Settings' every games table, a table shown builds its index; without, it does not.
#[test]
fn settings_build_for_every_table() {
    let d = Dir::new();
    let p = file(&d, "auto.pgn", &[game("a", "b", 0, 0, 2000, "1-0", None, "1.e4")]);
    let lib = opened_lib(&p);
    keep_up(&lib, &crate::config::ChessConfig::default());
    assert!(job(&p).is_none());
    keep_up(&lib, &crate::config::ChessConfig { position_index: true, position_depth: "15".into(), ..Default::default() });
    let j = job(&p).unwrap();
    let until = std::time::Instant::now() + crate::test_limit(std::time::Duration::from_secs(30));
    while j.phase.load(Ordering::Relaxed) < 2 {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(status(&lib, 30).depth, 15);
    // Removed by hand: not built again by itself until the database changes.
    remove(&p);
    let on = crate::config::ChessConfig { position_index: true, ..Default::default() };
    keep_up(&lib, &on);
    assert!(job(&p).is_none() && !status(&lib, 30).ready);
    let before = generation();
    built(&lib, 30);
    assert!(generation() > before, "a list cut to a position is asked again");
    remove(&p);
}

/// A database that is gone takes its index with it.
#[test]
fn an_index_goes_with_its_database() {
    let d = Dir::new();
    let p = file(&d, "gone.pgn", &[game("a", "b", 0, 0, 2000, "1-0", None, "1.e4")]);
    let lib = opened_lib(&p);
    built(&lib, 30);
    let dir = opened(&p).unwrap().dir.clone();
    drop(lib);
    drop(OPEN.lock().unwrap().as_mut().unwrap().remove(&p));
    std::fs::remove_file(&p).unwrap();
    assert!(!kept().iter().any(|k| k.source == p));
    assert!(!dir.exists());
}

#[test]
fn xiangqi_games_have_a_tree_too() {
    let d = Dir::new();
    let g = |moves: &str| format!("[Game \"Chinese Chess\"]\n[Red \"r\"]\n[Black \"b\"]\n[Result \"1-0\"]\n\n{moves} 1-0\n\n");
    let p = file(&d, "x.pgn", &[g("1. h2e2 h9g7"), g("1. h2e2 b9c7"), g("1. b2e2 h9g7")]);
    let lib = opened_lib(&p);
    assert_eq!(lib.format, crate::chess::Format::Xiangqi);
    built(&lib, 30);
    let start = crate::xiangqi::Xiangqi.start(&[], "").1;
    let t = tree(&lib, &start).unwrap();
    assert_eq!(t.moves.iter().map(|m| (m.san.as_str(), m.games)).collect::<Vec<_>>(), vec![("h2e2", 2), ("b2e2", 1)]);
    let after = &t.moves[0].fen;
    assert_eq!(tree(&lib, after).unwrap().games, 2);

    // A tree move played: the board's next position, Red's first move, and the tree follows.
    let x = crate::chess::Format::Xiangqi;
    let red = play_tree_move(x, &start, &t.moves[0].uci, None).expect("h2e2 plays");
    assert_eq!((red.fen.as_str(), red.white, red.number, red.san.as_str()), (after.as_str(), true, 1, "h2e2"));
    // h2 and e2: cells x + y × 9, rows from Black's side.
    assert_eq!((red.from, red.to), (7 + 7 * 9, 4 + 7 * 9));
    let next = tree(&lib, &red.fen).unwrap();
    assert_eq!(next.moves.iter().map(|m| (m.san.as_str(), m.games)).collect::<Vec<_>>(), vec![("b9c7", 1), ("h9g7", 1)]);
    let black = play_tree_move(x, &red.fen, &next.moves[1].uci, Some((red.white, red.number))).expect("h9g7 plays");
    assert_eq!((black.fen.as_str(), black.white, black.number), (next.moves[1].fen.as_str(), false, 1));
    assert_eq!(play_tree_move(x, &black.fen, "h9g7", Some((false, 1))).map(|p| p.number), None, "no piece of Red's there");
    let again = play_tree_move(x, &black.fen, "b0c2", Some((false, 1))).expect("Red again");
    assert_eq!((again.white, again.number), (true, 2));
    remove(&p);
}

#[test]
fn a_chess_tree_move_plays_by_the_rules() {
    let start = pgn::fen(&Board::default());
    let m = play_tree_move(crate::chess::Format::Pgn, &start, "e2e4", None).unwrap();
    assert_eq!((m.fen, m.san.as_str(), m.white, m.number), (fen_after(&["e4"]), "e4", true, 1));
    assert!(play_tree_move(crate::chess::Format::Pgn, &start, "e2e5", None).is_none());
}

/// A database whose name is not UTF-8 gets its index, found again by its path.
#[test]
#[cfg(all(unix, not(target_os = "macos")))]
fn a_name_that_is_not_utf8_gets_an_index() {
    use std::os::unix::ffi::OsStrExt;
    let d = Dir::new();
    let p = d.0.join(std::ffi::OsStr::from_bytes(b"caf\xe9.pgn"));
    std::fs::write(&p, game("a", "b", 0, 0, 2000, "1-0", None, "1.e4 e5")).unwrap();
    let lib = opened_lib(&p);
    built(&lib, 30);
    drop(OPEN.lock().unwrap().as_mut().unwrap().remove(&p));
    assert_eq!(tree(&lib, pgn::START).unwrap().games, 1);
    assert!(kept().iter().any(|k| k.source == p));
    remove(&p);
}

/// A file replaced by a smaller one after its table was read, then grown: the index built from
/// the old table is never taken for the new file's, so the next build reads every game.
#[test]
fn a_file_replaced_before_its_index_is_made_is_read_whole_next() {
    let d = Dir::new();
    let one = |w: &str, moves: &str| game(w, "b", 2000, 2000, 2000, "1-0", None, moves);
    let p = file(&d, "replaced.pgn", &(0..40).map(|i| one(&format!("w{i}"), "1.e4 e5")).collect::<Vec<_>>());
    let lib = opened_lib(&p);
    // Replaced between the table's read and the index's end: the build reads the old table.
    std::fs::write(&p, [one("x0", "1.d4 d5"), one("x1", "1.d4 d5")].concat()).unwrap();
    built(&lib, 30);
    drop(lib);
    std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(one("x2", "1.c4 c5").as_bytes()).unwrap();
    let lib = opened_lib(&p);
    keep_up(&lib, &crate::config::ChessConfig::default());
    let j = job(&p).unwrap();
    let until = std::time::Instant::now() + crate::test_limit(std::time::Duration::from_secs(30));
    while j.phase.load(Ordering::Relaxed) < 2 {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(j.phase.load(Ordering::Relaxed), 2, "{:?}", j.error.lock().unwrap());
    assert_eq!(j.walked.load(Ordering::Relaxed), 3, "every game of the file now");
    let t = tree(&lib, pgn::START).unwrap();
    assert_eq!((t.stale, t.games, t.moves.iter().map(|m| (m.san.as_str(), m.games)).collect::<Vec<_>>()), (false, 3, vec![("d4", 2), ("c4", 1)]));
    remove(&p);
}

/// A stale index of a file changed in its middle: a position fewer than `HOT` games reach still
/// shows its counts, only not what the rows give (results, Elos, years, the best player).
#[test]
fn a_rare_position_of_a_stale_index_shows_its_counts() {
    let d = Dir::new();
    let one = |w: &str| game(w, "b", 2000, 2000, 2000, "1-0", None, "1.e4 e5 2.Nf3");
    let p = file(&d, "cold.pgn", &[one("w0"), one("w1"), one("w2")]);
    let lib = opened_lib(&p);
    built(&lib, 30);
    drop(lib);
    let text = std::fs::read_to_string(&p).unwrap().replacen("1.e4 e5", "1.Nf3 e5", 1);
    std::fs::write(&p, text).unwrap();
    let lib = opened_lib(&p);
    assert!(opened(&p).unwrap().kept(crate::chess::polyglot::key(&Board::default())).unwrap().is_empty(), "a rare position");
    let t = tree(&lib, &fen_after(&["e4"])).unwrap();
    assert!(t.stale && !t.opens);
    assert_eq!((t.games, t.moves.iter().map(|m| (m.san.as_str(), m.games)).collect::<Vec<_>>()), (3, vec![("e5", 3)]));
    assert!(t.moves.iter().all(|m| m.best.is_none() && m.elo.is_none()));
    remove(&p);
}

/// A half-made index a build that ended left (a quit, a crash) goes when Disk use lists them;
/// one being built by a live run stays.
#[test]
#[cfg(unix)]
fn an_ended_builds_half_made_index_goes() {
    let root = folder().unwrap();
    let ended = root.join("00000000000000aa-4000021-0");
    let live = root.join(format!("00000000000000aa-{}-0", crate::spill::run()));
    for dir in [&ended, &live] {
        std::fs::create_dir_all(dir.join("runs")).unwrap();
        std::fs::write(dir.join("runs").join("run-0"), "x").unwrap();
    }
    // An id no real process of this test has, with its list held by none: the run ended. Written
    // last, so another test's `kept()` leaves the folder alone until it is whole.
    let lists = crate::helper::folder().unwrap().join("staging");
    crate::fs::private_dir(&lists).unwrap();
    std::fs::write(lists.join("4000021"), "").unwrap();
    kept();
    assert!(!ended.exists(), "ended: gone");
    assert!(live.join("runs").join("run-0").exists(), "live: kept");
    let _ = std::fs::remove_dir_all(&live);
    let _ = std::fs::remove_file(lists.join("4000021"));
}

/// The file changed and its table was read again (both apps do): a build still under way from
/// the table before is stopped, as it would be out of date when done. Both apps ask `keep_up`.
#[test]
fn a_build_of_an_older_table_is_stopped() {
    let d = Dir::new();
    let p = file(&d, "changed.pgn", &[game("a", "b", 0, 0, 2000, "1-0", None, "1.e4")]);
    let old = opened_lib(&p);
    let j = Arc::new(Job::default());
    *j.stamp.lock().unwrap() = old.stamp().to_string();
    JOBS.lock().unwrap().get_or_insert_with(HashMap::new).insert(p.clone(), j.clone());
    // Its own table: left to run.
    keep_up(&old, &crate::config::ChessConfig::default());
    assert!(!j.stop.load(Ordering::Relaxed));
    std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(game("c", "d", 0, 0, 2000, "1-0", None, "1.d4").as_bytes()).unwrap();
    let lib = opened_lib(&p);
    assert!(!outdated(&lib));
    keep_up(&lib, &crate::config::ChessConfig::default());
    assert!(j.stop.load(Ordering::Relaxed));
    JOBS.lock().unwrap().as_mut().unwrap().remove(&p);
}

/// Heavily annotated games: a batch waiting for the threads holds at most about `BATCH_BYTES`
/// of text, not `BATCH` games' whatever their size.
#[test]
fn a_batch_of_long_games_is_cut_by_its_text() {
    let d = Dir::new();
    let long = 400 << 10;
    let note = format!("{{{}}}", "x".repeat(long));
    let games: Vec<String> = (0..40).map(|i| game(&format!("w{i}"), "b", 0, 0, 2000, "1-0", None, &format!("1.e4 {note} e5"))).collect();
    let p = file(&d, "long.pgn", &games);
    let lib = opened_lib(&p);
    let j = built(&lib, 30);
    assert_eq!(j.walked.load(Ordering::Relaxed), 40);
    let most = j.most.load(Ordering::Relaxed) as usize;
    assert!(most > long && most < BATCH_BYTES + long + 4096, "{most} bytes in one batch");
    assert_eq!(tree(&lib, pgn::START).unwrap().games, 40);
    remove(&p);
}

/// A PGN kept in blocks (a large one in a 7z) is read by as many threads as play the games out,
/// each block still unpacked about once; a ChessBase database too, and a packed file by one
/// thread, front to back. The index is the same as the plain file's.
#[test]
fn games_read_at_their_place_are_read_by_several_threads() {
    let d = Dir::new();
    let note = format!("{{{}}}", "x".repeat(1000));
    let lines = random_lines(3000, 9);
    let texts: Vec<String> = lines.iter().map(|l| game("a", "b", 2000, 2000, 2000, "1-0", None, &format!("{note} {}", l.join(" ")))).collect();
    let plain = file(&d, "games.pgn", &texts);
    let seven = d.0.join("games.7z");
    crate::archive::create(&seven, std::slice::from_ref(&plain)).unwrap();
    let inside = seven.join("games.pgn");
    let lib = Arc::new(Library::open_with(&inside, super::super::games::Limits { keep: 0, ..Default::default() }).unwrap());
    let until = std::time::Instant::now() + crate::test_limit(std::time::Duration::from_secs(30));
    while !lib.progress().done {
        assert!(std::time::Instant::now() < until, "the table never filled");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(lib.progress().games, 3000);
    let blocks = (0..3000).map(|g| lib.rec(g).unwrap().offset).collect::<BTreeSet<_>>().len() as u64;
    assert!(blocks >= 8, "{blocks} blocks");
    let before = lib.unpacked();
    let j = built(&lib, 30);
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).div_ceil(2).clamp(1, 8) as u64;
    assert_eq!(j.readers.load(Ordering::Relaxed), threads);
    let unpacked = lib.unpacked() - before;
    // A block two readers' runs share may be unpacked by both.
    assert!(unpacked <= blocks + 3000u64.div_ceil(CHUNK), "{unpacked} blocks unpacked for {blocks}");
    let plain_lib = opened_lib(&plain);
    let pj = built(&plain_lib, 30);
    assert_eq!(pj.readers.load(Ordering::Relaxed), 1, "a plain file: one handle");
    let shown = |l: &Library, fen: &str| serde_json::to_string(&tree(l, fen).unwrap()).unwrap();
    for fen in [pgn::START.to_string(), fen_after(&lines[0].iter().map(|s| s.as_str()).collect::<Vec<_>>()[..6])] {
        assert_eq!(shown(&lib, &fen), shown(&plain_lib, &fen));
    }
    remove(&inside);
    remove(&plain);

    // A ChessBase database: several threads too.
    let cbh = d.0.join("Games.cbh");
    for ext in ["cba", "cbg", "cbh", "cbp", "cbt", "cbv"] {
        std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/chess/cbh-test/Games").with_extension(ext), cbh.with_extension(ext)).unwrap();
    }
    let lib = opened_lib(&cbh);
    assert_eq!(built(&lib, 30).readers.load(Ordering::Relaxed), threads);
    remove(&cbh);

    // A gzip: one thread, the file unpacked once.
    let gz = d.0.join("games.pgn.gz");
    let mut w = flate2::write::GzEncoder::new(std::fs::File::create(&gz).unwrap(), flate2::Compression::fast());
    w.write_all(texts[..50].concat().as_bytes()).unwrap();
    w.finish().unwrap();
    let lib = opened_lib(&gz);
    assert_eq!(built(&lib, 30).readers.load(Ordering::Relaxed), 1);
    remove(&gz);
}

//! A whole game analysed by the engine (docs/previews/chess-play.md#analysing-a-whole-game),
//! for both apps: each position of the main line evaluated in the background, the moves that
//! lose marked `?!`, `?` or `??` by how much they lose (and `!?` for the only move that holds),
//! kept in the cache per game, and written out as an annotated PGN file of its own.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::pgn::{self, Game, Token};
use super::play::{Engine, Program};

/// What a lost advantage is worth, in centipawns from the mover's side: at least this much is
/// `?!` (an inaccuracy), `?` (a mistake), `??` (a blunder). Evaluations count up to
/// `CAP` either way, so a move in a won position is not a blunder for winning a little less.
pub const INACCURACY: i32 = 50;
pub const MISTAKE: i32 = 100;
pub const BLUNDER: i32 = 300;
pub const CAP: i32 = 1000;
/// `!?`: the engine's best move, played, where its second choice is this much worse, in a
/// position not already won.
pub const ONLY_MOVE: i32 = 300;

/// A position's evaluation from White's side, in centipawns; a mate is `CAP` and more (minus
/// for Black), nearer mates further out, so a mate in 2 beats a mate in 5.
fn score(cp: Option<i32>, mate: Option<i32>) -> Option<i32> {
    match (mate, cp) {
        (Some(m), _) if m > 0 => Some(CAP + 100 - m.min(99)),
        (Some(m), _) if m < 0 => Some(-(CAP + 100 + m.max(-99))),
        (Some(_), _) => None,
        (None, c) => c,
    }
}

/// One move of the main line, evaluated.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ply {
    /// The game's node of the move.
    pub node: u32,
    /// The evaluation after it, from White's side (`score`), and as shown (`+0.35`, `#3`).
    pub cp: i32,
    pub eval: String,
    /// Its mark as a NAG: 6 `?!`, 2 `?`, 4 `??`, 5 `!?`.
    pub mark: Option<u8>,
    /// What the engine would have played instead, from the position before (English SAN).
    pub best: Vec<String>,
}

/// A game's analysis, as far as it has come.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub engine: String,
    /// The start position's evaluation, then each move's.
    pub start: Option<i32>,
    pub plies: Vec<Ply>,
    /// Of how many moves; done when all are in, or it stopped.
    pub total: usize,
    pub done: bool,
    pub error: Option<String>,
}

/// The mark of a move that took the evaluation from `before` to `after` (White's side), by
/// `white` or Black; `only`: it is the engine's best move, and by how much its second is worse.
pub fn mark(before: i32, after: i32, white: bool, only: Option<i32>) -> Option<u8> {
    let side = |v: i32| if white { v } else { -v };
    let lost = side(before).clamp(-CAP, CAP) - side(after).clamp(-CAP, CAP);
    if lost >= BLUNDER {
        Some(4)
    } else if lost >= MISTAKE {
        Some(2)
    } else if lost >= INACCURACY {
        Some(6)
    } else if only.is_some_and(|gap| gap >= ONLY_MOVE) && side(before) < MISTAKE * 3 {
        Some(5)
    } else {
        None
    }
}

/// The evaluation as shown: `+0.35`, `−1.20`, `#3`, `#−2`.
pub fn shown(v: i32) -> String {
    if v.abs() > CAP {
        let m = CAP + 100 - v.abs();
        return if v > 0 { format!("#{m}") } else { format!("#−{m}") };
    }
    let p = v.abs() as f64 / 100.0;
    if v < 0 { format!("−{p:.2}") } else { format!("+{p:.2}") }
}

/// The evaluation as PGN's `[%eval]` writes it: `0.35`, `-1.20`, `#3`, `#-2`.
fn pgn_eval(v: i32) -> String {
    shown(v).replace('−', "-").replace('+', "")
}

/// White's share of the game, 0 to 1, for the bar beside the board and the graph.
pub fn share(v: i32) -> f64 {
    1.0 / (1.0 + (-(v.clamp(-CAP, CAP)) as f64 / 250.0).exp())
}

/// The main line's moves that have a position, in order.
fn line(game: &Game) -> Vec<u32> {
    game.main_line().into_iter().take_while(|&n| game.nodes[n as usize].fen.is_some()).collect()
}

/// Where the analysis of `game` by `engine` at `ms` a position is kept: the cache, by a hash of
/// all three, so another engine or time is another analysis.
fn cached(engine: &str, ms: u32, game: &Game) -> Option<PathBuf> {
    // FNV-1a: stable across builds, unlike the standard hasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |s: &str| {
        for b in s.bytes().chain([0]) {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
    };
    feed(engine);
    feed(&ms.to_string());
    feed(&game.start);
    for n in game.main_line() {
        feed(&game.nodes[n as usize].san);
    }
    Some(crate::dirs::cache_dir()?.join("coxswain").join("chess").join(format!("{h:016x}.json")))
}

/// A game being analysed, on a thread of its own; dropped or stopped, the engine stops too.
pub struct Reviewer {
    shared: Arc<Mutex<Review>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Reviewer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Reviewer {
    /// Analyses the main line of `game` with the engine at `program`, `ms` a position. A game
    /// analysed before (same engine, time and moves) comes from the cache at once.
    pub fn start(program: &Program, ms: u32, game: &Game) -> Reviewer {
        let plies = line(game);
        let key = program.key();
        let file = cached(&key, ms, game);
        let shared = Arc::new(Mutex::new(Review { total: plies.len(), ..Default::default() }));
        let stop = Arc::new(AtomicBool::new(false));
        if let Some(r) = file.as_ref().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice::<Review>(&b).ok())
            && r.done
            && r.error.is_none()
        {
            *shared.lock().unwrap_or_else(|e| e.into_inner()) = r;
            return Reviewer { shared, stop };
        }
        let (s, st, g) = (shared.clone(), stop.clone(), game.clone());
        let program = program.clone();
        std::thread::spawn(move || {
            let r = run(&program, ms, &g, &plies, &s, &st);
            let mut out = s.lock().unwrap_or_else(|e| e.into_inner());
            out.done = true;
            if let Err(e) = r {
                if e.kind() != std::io::ErrorKind::Interrupted {
                    out.error = Some(e.to_string());
                }
            } else if let Some(f) = file
                && std::fs::create_dir_all(f.parent().unwrap_or(Path::new("."))).is_ok()
                && let Ok(json) = serde_json::to_vec(&*out)
            {
                let _ = std::fs::write(f, json);
            }
        });
        Reviewer { shared, stop }
    }

    /// How far it has come.
    pub fn now(&self) -> Review {
        self.shared.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run(program: &Program, ms: u32, game: &Game, plies: &[u32], shared: &Mutex<Review>, stop: &AtomicBool) -> std::io::Result<()> {
    let mut engine = Engine::start(program)?;
    shared.lock().unwrap_or_else(|e| e.into_inner()).engine = engine.name.clone();
    let halt = || stop.load(Ordering::Relaxed);
    // The position before each move: its score, the engine's line, and how much worse its
    // second choice is (from the mover's side).
    let mut before: Option<(i32, Vec<String>, Option<i32>)> = None;
    let fens = std::iter::once(game.start.clone()).chain(plies.iter().map(|&n| game.nodes[n as usize].fen.clone().unwrap_or_default()));
    for (i, fen) in fens.enumerate() {
        let a = engine.think(&fen, ms, 2, &halt)?;
        let b = pgn::board(&fen).ok_or_else(|| std::io::Error::other("no position"))?;
        let white = b.side_to_move() == cozy_chess::Color::White;
        let v = match a.lines.first().and_then(|l| score(l.cp, l.mate)) {
            Some(v) => v,
            // No move: mated (lost for the side to move) or stalemate.
            None if !b.checkers().is_empty() => if white { -(CAP + 100) } else { CAP + 100 },
            None => 0,
        };
        let gap = match (a.lines.first().and_then(|l| score(l.cp, l.mate)), a.lines.get(1).and_then(|l| score(l.cp, l.mate))) {
            (Some(x), Some(y)) => Some(if white { x - y } else { y - x }),
            _ => None,
        };
        let best_line = a.lines.first().map(|l| l.moves.iter().take(6).cloned().collect()).unwrap_or_default();
        let mut r = shared.lock().unwrap_or_else(|e| e.into_inner());
        if r.engine.is_empty() {
            r.engine = a.engine.clone();
        }
        match (i, before.take()) {
            (0, _) => r.start = Some(v),
            (_, Some((prev, best, only))) => {
                let n = plies[i - 1];
                let node = &game.nodes[n as usize];
                let played_best = best.first() == Some(&node.san);
                r.plies.push(Ply {
                    node: n,
                    cp: v,
                    eval: shown(v),
                    mark: mark(prev, v, node.white, only.filter(|_| played_best)),
                    best: if played_best { vec![] } else { best },
                });
            }
            _ => {}
        }
        before = Some((v, best_line, gap));
    }
    Ok(())
}

/// The game written out as PGN with the analysis in it: its tags (`Annotator` the engine), its
/// moves, variations and comments as they are, and on each analysed move its mark (a NAG), its
/// evaluation as `[%eval 0.35]`, and for a marked move the engine's line as a variation.
pub fn annotated_pgn(game: &Game, r: &Review) -> String {
    let mut out = String::new();
    for (k, v) in game.tags.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("Annotator")) {
        out.push_str(&format!("[{k} \"{}\"]\n", v.replace('\\', "\\\\").replace('"', "\\\"")));
    }
    out.push_str(&format!("[Annotator \"{}\"]\n\n", r.engine.replace('\\', "\\\\").replace('"', "\\\"")));
    let by_node: std::collections::HashMap<u32, &Ply> = r.plies.iter().map(|p| (p.node, p)).collect();
    let comment = |t: &str| format!("{{{}}}", t.replace('}', ")"));
    let mut words: Vec<String> = vec![];
    for tok in game.layout() {
        match tok {
            Token::Move { node, number } => {
                let n = &game.nodes[node as usize];
                if let Some(num) = number {
                    words.push(num.replace('…', "..."));
                }
                words.push(n.san.clone());
                let ply = by_node.get(&node);
                let mut nags = n.nags.clone();
                if let Some(m) = ply.and_then(|p| p.mark)
                    && !nags.iter().any(|x| (1..=6).contains(x))
                {
                    nags.push(m);
                }
                words.extend(nags.iter().map(|x| format!("${x}")));
                if let Some(p) = ply {
                    words.push(comment(&format!("[%eval {}]", pgn_eval(p.cp))));
                    if p.mark.is_some_and(|m| m != 5) && !p.best.is_empty() {
                        // The engine's line, from the position before the move.
                        let before = n.parent.and_then(|q| game.nodes[q as usize].fen.clone()).unwrap_or_else(|| game.start.clone());
                        let text = super::play::line_pgn(&[], &before, &p.best);
                        let moves = text.split("\n\n").nth(1).unwrap_or("").trim().trim_end_matches('*').trim();
                        words.push(format!("({moves})"));
                    }
                }
            }
            Token::Comment { text } => words.push(comment(&text)),
            Token::Open { .. } => words.push("(".into()),
            Token::Close { .. } => words.push(")".into()),
            Token::Result { text } => words.push(text),
        }
    }
    let mut line = String::new();
    for w in words {
        if !line.is_empty() && line.len() + 1 + w.len() > 79 {
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        if !line.is_empty() && !line.ends_with('(') && w != ")" {
            line.push(' ');
        }
        line.push_str(&w);
    }
    out.push_str(&line);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_by_what_is_lost() {
        // White from +0.30 to −0.10: 40 lost, nothing; to −0.30: 60, ?!; to −1.00: ?; to −3.00: ??.
        assert_eq!(mark(30, -10, true, None), None);
        assert_eq!(mark(30, -30, true, None), Some(6));
        assert_eq!(mark(30, -100, true, None), Some(2));
        assert_eq!(mark(30, -300, true, None), Some(4));
        // Black's side: White's score going up is Black losing.
        assert_eq!(mark(-30, 300, false, None), Some(4));
        // Winning a little less in a won position is no blunder.
        assert_eq!(mark(2000, 1000, true, None), None);
        // The only move that holds.
        assert_eq!(mark(0, 0, true, Some(400)), Some(5));
        assert_eq!(mark(0, 0, true, Some(200)), None);
        // Not in a position already won.
        assert_eq!(mark(900, 900, true, Some(400)), None);
        assert_eq!((shown(35), shown(-120), shown(score(None, Some(3)).unwrap()), shown(score(None, Some(-2)).unwrap())), ("+0.35".into(), "−1.20".into(), "#3".into(), "#−2".into()));
        assert!(share(300) > 0.7 && share(-300) < 0.3 && (share(0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn an_annotated_game() {
        let g = pgn::parse("[White \"A\"]\n[Annotator \"Old\"]\n\n1.e4 e5 {sure} (1...c5) 2.Qh5 Nc6 3.Bc4 Nf6 4.Qxf7# 1-0\n");
        let main = g.main_line();
        let r = Review {
            engine: "Fake 1".into(),
            start: Some(30),
            plies: vec![
                Ply { node: main[0], cp: 30, eval: "+0.30".into(), mark: None, best: vec![] },
                Ply { node: main[5], cp: score(None, Some(5)).unwrap(), eval: "#5".into(), mark: Some(4), best: vec!["g6".into(), "Qf3".into()] },
            ],
            total: 7,
            done: true,
            error: None,
        };
        let text = annotated_pgn(&g, &r);
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(text.contains("[Annotator \"Fake 1\"]") && !text.contains("Old"), "{text}");
        assert!(flat.contains("1. e4 {[%eval 0.30]} e5 {sure} (1... c5)"), "{text}");
        assert!(flat.contains("Nf6 $4 {[%eval #5]} (3... g6 4. Qf3)"), "{text}");
        // It reads back: the same main line, the mark on its move.
        let back = pgn::parse(&text);
        assert_eq!(back.main_line().len(), main.len());
        assert_eq!(back.nodes[back.main_line()[5] as usize].nags, vec![4]);
    }

    /// The whole game by a fake engine that says +0.20 for White to move and −0.20 for Black:
    /// every position the same for White, so no marks; then from the cache.
    #[cfg(unix)]
    #[test]
    fn a_game_reviewed_and_cached() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-review-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("engine");
        std::fs::write(
            &p,
            "#!/bin/sh\nwhile read l; do case \"$l\" in uci) echo 'id name Fake 2'; echo uciok;; isready) echo readyok;; go*) echo 'info depth 3 multipv 1 score cp 20 pv a2a3'; echo 'bestmove a2a3';; quit) exit 0;; esac; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        let g = pgn::parse("1.e4 e5 2.Nf3 *");
        let wait = |r: &Reviewer| {
            let t = std::time::Instant::now();
            while !r.now().done && t.elapsed().as_secs() < 20 {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            r.now()
        };
        let r = wait(&Reviewer::start(&Program::Installed(p.clone()), 50, &g));
        assert_eq!((r.engine.as_str(), r.plies.len(), r.error.as_deref()), ("Fake 2", 3, None));
        assert_eq!(r.plies[0].cp, -20);
        // From the cache, even with the engine gone.
        std::fs::remove_file(&p).unwrap();
        let again = Reviewer::start(&Program::Installed(p.clone()), 50, &g).now();
        assert!(again.done && again.plies.len() == 3, "{again:?}");
        std::fs::remove_dir_all(&d).unwrap();
    }
}

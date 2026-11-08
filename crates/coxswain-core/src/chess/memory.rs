//! The positions the engine has analysed, kept in the cache for both apps
//! (docs/previews/chess-play.md#positions-remembered): the live analysis and the game review
//! each keep what they found for a position, by engine, settings and position (its FEN without
//! the move counters), so a position seen once is not analysed again, and the live analysis
//! shows the review's result at once while it looks deeper.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::pgn;
use super::play::{Analysis, Limit};
use crate::config::ChessConfig;

/// How many positions are kept: the oldest go past it. One is 2 to 11 KB (by the lines asked for),
/// so the folder stays under 40 MB.
pub const KEPT: usize = 3000;

/// The folder they are kept in: `chess/positions` in Coxswain's cache.
pub fn dir() -> Option<PathBuf> {
    Some(crate::dirs::cache_dir()?.join("coxswain").join("chess").join("positions"))
}

/// What tells a live analysis from another beside the engine: its limit, lines and options.
pub(super) fn live(chess: &ChessConfig) -> String {
    format!("live {:?} {} {:?}", Limit::of(chess, false), chess.engine_lines.clamp(1, super::play::MAX_LINES), chess.engines)
}

/// The same for a position of the game review, which asks for two lines.
pub(super) fn review(chess: &ChessConfig) -> String {
    format!("review {:?} 2 {:?}", Limit::of(chess, true), chess.engines)
}

/// The position without its move counters: the same position reached at another move is the same.
fn position(fen: &str) -> String {
    pgn::full_fen(fen).split(' ').take(4).collect::<Vec<_>>().join(" ")
}

fn file(dir: &Path, engine: &str, settings: &str, fen: &str) -> PathBuf {
    // FNV-1a: stable across builds, unlike the standard hasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in [engine, settings, &position(fen)] {
        for b in s.bytes().chain([0]) {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
    dir.join(format!("{h:016x}.json"))
}

/// What the engine `engine` with `settings` found for `fen` before, if it is kept.
pub(super) fn recall(engine: &str, settings: &str, fen: &str) -> Option<Analysis> {
    recall_in(&dir()?, engine, settings, fen)
}

fn recall_in(dir: &Path, engine: &str, settings: &str, fen: &str) -> Option<Analysis> {
    let mut a: Analysis = serde_json::from_slice(&std::fs::read(file(dir, engine, settings, fen)).ok()?).ok()?;
    a.note = None;
    Some(a)
}

/// `a` kept for the next time `fen` is asked about the same way.
pub(super) fn keep(engine: &str, settings: &str, fen: &str, a: &Analysis) {
    if let Some(d) = dir() {
        keep_in(&d, engine, settings, fen, a);
    }
}

fn keep_in(dir: &Path, engine: &str, settings: &str, fen: &str, a: &Analysis) {
    static WRITTEN: AtomicUsize = AtomicUsize::new(0);
    static N: AtomicUsize = AtomicUsize::new(0);
    let Ok(json) = serde_json::to_vec(&Analysis { note: None, review: false, ..a.clone() }) else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    // Written whole, then renamed: the other app never reads half of it.
    let f = file(dir, engine, settings, fen);
    let tmp = f.with_extension(format!("{}-{}.tmp.json", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    if std::fs::write(&tmp, json).is_err() || std::fs::rename(&tmp, &f).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    // The bound checked on the first write and every hundredth after: past it by at most 100.
    if WRITTEN.fetch_add(1, Ordering::Relaxed) % 100 == 0 {
        super::review::keep_newest(dir, KEPT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_by_position_without_move_counters() {
        let d = std::env::temp_dir().join(format!("coxswain-positions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let a = Analysis { engine: "Fake".into(), depth: 7, note: Some("said".into()), ..Default::default() };
        let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
        assert!(recall_in(&d, "e", "s", fen).is_none());
        keep_in(&d, "e", "s", fen, &a);
        // The same position at another move number: the same; the note is not said again.
        let back = recall_in(&d, "e", "s", "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 4 9").unwrap();
        assert_eq!((back.depth, back.note), (7, None));
        // Another engine or settings: not the same analysis.
        assert!(recall_in(&d, "f", "s", fen).is_none() && recall_in(&d, "e", "t", fen).is_none());
        // No half-written file left beside it.
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }
}

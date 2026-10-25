//! Playing on from a position (docs/previews/chess-play.md), for both apps: the legal moves of
//! a position, a move played, the line played written out as a new PGN file, and an installed
//! UCI engine (Stockfish) asked for its best move and evaluation. The user's files are never
//! changed: a line is kept in the view until it is saved as a file of its own.

use cozy_chess::{Board, Color, Piece};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use super::pgn;

/// A move played from a position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Played {
    /// In English SAN (`Nf3`), and in UCI with castling as the king's own move (`e1g1`).
    pub san: String,
    pub uci: String,
    /// The position after it, in FEN.
    pub fen: String,
    /// The squares it moves from and to as the eye sees them (a1 = 0, h8 = 63).
    pub from: u8,
    pub to: u8,
    /// White's move (else Black's), and its move number.
    pub white: bool,
    pub number: u32,
    /// The piece a pawn becomes: `q`, `r`, `b` or `n`.
    pub promotion: Option<char>,
}

fn played(b: &Board, m: cozy_chess::Move) -> Played {
    let (from, to) = pgn::shown_squares(b, m);
    let mut after = b.clone();
    after.play_unchecked(m);
    let promotion = m.promotion.map(|p| match p {
        Piece::Knight => 'n',
        Piece::Bishop => 'b',
        Piece::Rook => 'r',
        _ => 'q',
    });
    Played {
        san: pgn::san(b, m),
        uci: super::polyglot::uci(b, m),
        fen: after.to_string(),
        from,
        to,
        white: b.side_to_move() == Color::White,
        number: b.fullmove_number() as u32,
        promotion,
    }
}

/// Every legal move from the position `fen`, by the square they leave, then the one they
/// reach. None: `fen` is no position (or the game is over: an empty list).
pub fn moves(fen: &str) -> Vec<Played> {
    let Some(b) = pgn::board(fen) else { return vec![] };
    let mut out = vec![];
    b.generate_moves(|ms| {
        out.extend(ms.into_iter().map(|m| played(&b, m)));
        false
    });
    out.sort_by_key(|p| (p.from, p.to, p.promotion.map(|c| "qrbn".find(c))));
    out
}

/// The move `uci` (`e2e4`, `e7e8q`, castling as `e1g1` or as the king onto its rook) played on
/// the position `fen`; None when it is no legal move there.
pub fn play(fen: &str, uci: &str) -> Option<Played> {
    let b = pgn::board(fen)?;
    let sq = |s: &str| -> Option<u8> {
        let s = s.as_bytes();
        let (f, r) = (s.first()?.wrapping_sub(b'a'), s.get(1)?.wrapping_sub(b'1'));
        (f < 8 && r < 8).then_some(r * 8 + f)
    };
    let (from, to) = (sq(uci.get(..2)?)?, sq(uci.get(2..4)?)?);
    let promo = uci.chars().nth(4).map(|c| c.to_ascii_lowercase());
    let mut found = None;
    b.generate_moves(|ms| {
        for m in ms {
            let p = played(&b, m);
            let rook = m.from as u8 == from && m.to as u8 == to;
            if (p.from == from && p.to == to || rook) && p.promotion == promo {
                found = Some(p);
                return true;
            }
        }
        false
    });
    found
}

/// A line as a PGN game: `tags` (the game's, its result made `*`; `SetUp` and `FEN` when it
/// does not start from the start position), then `sans` played from `start`.
pub fn line_pgn(tags: &[(String, String)], start: &str, sans: &[String]) -> String {
    let start = pgn::full_fen(start);
    let mut out = String::new();
    let mut tags: Vec<(String, String)> = tags
        .iter()
        .filter(|(k, _)| !["Result", "SetUp", "FEN", "PlyCount"].iter().any(|t| t.eq_ignore_ascii_case(k)))
        .cloned()
        .collect();
    for (k, v) in [("Event", "?"), ("Site", "?"), ("Date", "????.??.??"), ("Round", "?"), ("White", "?"), ("Black", "?")] {
        if !tags.iter().any(|(n, _)| n.eq_ignore_ascii_case(k)) {
            tags.push((k.into(), v.into()));
        }
    }
    tags.push(("Result".into(), "*".into()));
    if start != pgn::START {
        tags.push(("SetUp".into(), "1".into()));
        tags.push(("FEN".into(), start.clone()));
    }
    for (k, v) in &tags {
        let v = v.replace('\\', "\\\\").replace('"', "\\\"");
        out.push_str(&format!("[{k} \"{v}\"]\n"));
    }
    out.push('\n');
    let mut words = vec![];
    let mut b = pgn::board(&start);
    for (i, san) in sans.iter().enumerate() {
        if let Some(board) = &b {
            let n = board.fullmove_number();
            if board.side_to_move() == Color::White {
                words.push(format!("{n}."));
            } else if i == 0 {
                words.push(format!("{n}..."));
            }
            b = pgn::san_move(board, san).map(|m| {
                let mut nb = board.clone();
                nb.play_unchecked(m);
                nb
            });
        }
        words.push(san.clone());
    }
    words.push("*".into());
    let mut line = String::new();
    for w in words {
        if !line.is_empty() && line.len() + 1 + w.len() > 79 {
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&w);
    }
    out.push_str(&line);
    out.push('\n');
    out
}

/// Writes `text` as a new PGN file next to the game file `source`: `<name> <word>.pgn` (`games
/// line.pgn`), or `<name> <word> 2.pgn` and on when that is taken. Never over a file.
pub fn save_beside(source: &Path, word: &str, text: &str) -> std::io::Result<PathBuf> {
    let dir = source.parent().unwrap_or(Path::new("."));
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    let cut = [".pgn.gz", ".pgn.zst", ".pgn", ".si4", ".si5", ".cbh", ".epd", ".fen", ".bin"]
        .iter()
        .find(|e| lower.ends_with(*e))
        .map_or(name.len(), |e| name.len() - e.len());
    let stem = &name[..cut];
    for n in 1..10_000 {
        let p = dir.join(if n == 1 { format!("{stem} {word}.pgn") } else { format!("{stem} {word} {n}.pgn") });
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(mut f) => {
                f.write_all(text.as_bytes())?;
                return Ok(p);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("no free name"))
}

/// What the engine says about a position: its best lines, the best first.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Analysis {
    /// The engine's name, as it gives it (`Stockfish 17.1`).
    pub engine: String,
    pub depth: u32,
    pub lines: Vec<EngineLine>,
}

/// One of the engine's lines (UCI's MultiPV).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct EngineLine {
    /// The evaluation from White's side: in centipawns, or mate in so many moves (minus: Black
    /// mates).
    pub cp: Option<i32>,
    pub mate: Option<i32>,
    /// The evaluation as shown: `+0.35`, `−1.20`, `#3`, `#−2`.
    pub eval: String,
    /// Its first move, and the line in English SAN, played from the position.
    pub best: Option<Played>,
    pub moves: Vec<String>,
}

/// The most lines asked for (`[chess] engine_lines`).
pub const MAX_LINES: u32 = 5;

/// The evaluation as shown.
fn eval_text(cp: Option<i32>, mate: Option<i32>) -> String {
    match (mate, cp) {
        (Some(m), _) if m < 0 => format!("#−{}", -m),
        (Some(m), _) => format!("#{m}"),
        (None, Some(c)) if c < 0 => format!("−{:.2}", -c as f64 / 100.0),
        (None, Some(c)) => format!("+{:.2}", c as f64 / 100.0),
        _ => String::new(),
    }
}

/// The engine to ask: the one `[chess] engine` names, else `stockfish` on PATH. None: none is
/// installed. A bare name is looked for on PATH only, never in the working folder (often the
/// folder being browsed, where a downloaded file could be named `stockfish`); a path is one
/// with a separator, `~` the home folder, and a relative one starts from the home folder.
pub fn engine(configured: &str) -> Option<PathBuf> {
    let c = configured.trim();
    if c.is_empty() {
        return crate::tools::which("stockfish");
    }
    if !c.starts_with('~') && !c.contains(['/', '\\']) {
        return crate::tools::which(c);
    }
    let p = crate::fs::resolve(&crate::dirs::home_dir().unwrap_or_default(), c);
    p.is_file().then_some(p)
}

/// The line that installs Stockfish here, if Coxswain knows one.
pub fn install() -> Option<String> {
    crate::tools::install("stockfish")
}

/// The engine's tool and image under `[preview.images]` (`preview_tools`, kind `chess_engine`).
pub const TOOL: &str = "stockfish";

/// Where the engine runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Program {
    /// A program installed here.
    Installed(PathBuf),
    /// Coxswain's Stockfish image (`[preview.images] stockfish`), pulled, run by podman or
    /// docker at `rt`: no network, a read-only root, no capabilities, bounded memory.
    Container { rt: &'static str, path: PathBuf, image: String },
}

impl Program {
    /// What tells one engine from another, for the cache of analysed games.
    pub fn key(&self) -> String {
        match self {
            Program::Installed(p) => p.to_string_lossy().into_owned(),
            Program::Container { image, .. } => image.clone(),
        }
    }

    /// The command that starts it, and for a container its runtime and its name, to stop it by.
    fn command(&self) -> (std::process::Command, Option<(PathBuf, String)>) {
        match self {
            Program::Installed(p) => (crate::tools::command(p), None),
            Program::Container { rt, path, image } => {
                static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let name = format!("coxswain-stockfish-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
                let mut c = crate::images::sealed_bare(rt, path, &name);
                c.arg(image);
                (c, Some((path.clone(), name)))
            }
        }
    }
}

/// The engine for these settings: `[chess] engine` when it names one; else Stockfish installed
/// here, else Coxswain's Stockfish image when it is pulled (`[preview.tools] chess_engine`
/// turns the order round, or keeps to one). Err: why there is none, and what to do.
pub fn engine_for(cfg: &crate::config::Config) -> Result<Program, String> {
    let missing = || match install() {
        Some(line) => crate::t!("chess.engine_missing_install", "command" => line),
        None => crate::t!("chess.engine_missing"),
    };
    if !cfg.chess.engine.trim().is_empty() {
        return engine(&cfg.chess.engine).map(Program::Installed).ok_or_else(missing);
    }
    let p = &cfg.preview;
    let pref = crate::preview_tools::choice(p, TOOL);
    let local = || crate::tools::which("stockfish").map(Program::Installed);
    let image = p.images.get(TOOL).filter(|i| !i.is_empty());
    let container = || {
        let (rt, path) = crate::tools::container_runtime(p)?;
        let image = image?;
        crate::images::id(&path, image).map(|_| Program::Container { rt, path, image: image.clone() })
    };
    let found = match pref {
        "container" => container().or_else(local),
        "local" => local(),
        "off" => None,
        _ => local().or_else(container),
    };
    found.ok_or_else(|| {
        let mut why = missing();
        if pref != "local" && image.is_some() && crate::tools::container_runtime(p).is_some() {
            why = format!("{why} {}", crate::t!("chess.engine_or_image"));
        }
        why
    })
}

/// Asks the engine `program` about the position `fen` for `ms` milliseconds, for its best
/// `lines` (1 to 5), over UCI on its standard input and output; it runs here and nothing
/// leaves the machine. `stop` is asked at each line the engine writes: true ends it at once
/// (the position on screen changed).
pub fn analyse(program: &Program, fen: &str, ms: u32, lines: u32, stop: &dyn Fn() -> bool) -> std::io::Result<Analysis> {
    pgn::board(&pgn::full_fen(fen)).ok_or_else(|| std::io::Error::other("no position"))?;
    Engine::start(program, stop)?.think(fen, ms, lines, stop)
}

fn failed(kind: std::io::ErrorKind) -> std::io::Error {
    std::io::Error::new(kind, crate::t!("chess.engine_failed"))
}

/// A UCI engine running, asked about one position after another. It is killed and waited for
/// when dropped: no engine is left running.
pub struct Engine {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    /// Its lines, read on a thread of their own, so a silent engine never blocks the asker:
    /// it is waited for a tenth of a second at a time, `stop` and the deadline asked between.
    out: std::sync::mpsc::Receiver<std::io::Result<String>>,
    /// Its name, as it gives it (`Stockfish 17.1`).
    pub name: String,
    /// A container's runtime and name: stopped by it too, as killing `podman run` leaves the
    /// container running.
    container: Option<(PathBuf, String)>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.stdin.write_all(b"quit\n");
        let _ = self.stdin.flush();
        // A moment to quit by itself, then killed.
        for _ in 0..20 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some((rt, name)) = &self.container {
            let _ = crate::tools::command(rt).args(["kill", name]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
    }
}

impl Engine {
    /// The next line it writes; Err when it ended, `stop` turned true or `deadline` passed.
    fn line(&self, deadline: std::time::Instant, stop: &dyn Fn() -> bool) -> std::io::Result<String> {
        use std::sync::mpsc::RecvTimeoutError;
        loop {
            if stop() {
                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "stopped"));
            }
            let wait = deadline.saturating_duration_since(std::time::Instant::now());
            if wait.is_zero() {
                return Err(failed(std::io::ErrorKind::TimedOut));
            }
            match self.out.recv_timeout(wait.min(std::time::Duration::from_millis(100))) {
                Ok(l) => return l,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err(failed(std::io::ErrorKind::UnexpectedEof)),
            }
        }
    }

    /// Starts the engine `program` and waits for it to say it speaks UCI; given up on as soon
    /// as `stop` is true.
    pub fn start(program: &Program, stop: &dyn Fn() -> bool) -> std::io::Result<Engine> {
        let (mut cmd, container) = program.command();
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| failed(std::io::ErrorKind::BrokenPipe))?;
        let stdout = child.stdout.take().ok_or_else(|| failed(std::io::ErrorKind::BrokenPipe))?;
        let (tx, out) = std::sync::mpsc::channel();
        // Ends when the engine's output closes, at the latest when Drop kills it.
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines() {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        let mut e = Engine { child, stdin, out, name: String::new(), container };
        writeln!(e.stdin, "uci")?;
        e.stdin.flush()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let line = e.line(deadline, stop)?;
            let w: Vec<&str> = line.split_whitespace().collect();
            match w.first() {
                Some(&"id") if w.get(1) == Some(&"name") => e.name = w[2..].join(" "),
                Some(&"uciok") => return Ok(e),
                _ => {}
            }
        }
    }

    /// Its best `lines` (1 to 5) for the position `fen`, after `ms` milliseconds. `stop` is
    /// asked at each line the engine writes: true ends it, and the engine is of no more use.
    pub fn think(&mut self, fen: &str, ms: u32, lines: u32, stop: &dyn Fn() -> bool) -> std::io::Result<Analysis> {
        let fen = pgn::full_fen(fen);
        let b = pgn::board(&fen).ok_or_else(|| std::io::Error::other("no position"))?;
        let wanted = lines.clamp(1, MAX_LINES) as usize;
        write!(
            self.stdin,
            "setoption name MultiPV value {wanted}\nisready\nposition fen {fen}\ngo movetime {}\n",
            ms.clamp(50, 600_000)
        )?;
        self.stdin.flush()?;
        let black = b.side_to_move() == Color::Black;
        let mut a = Analysis { engine: self.name.clone(), ..Default::default() };
        // Per line: its score (cp, mate) and its moves in UCI.
        let mut pvs: Vec<(Option<i32>, Option<i32>, Vec<String>)> = vec![Default::default(); wanted];
        // An engine that says nothing for far longer than it was given: given up on.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms.clamp(50, 600_000) as u64 + 30_000);
        loop {
            let line = self.line(deadline, stop)?;
            let w: Vec<&str> = line.split_whitespace().collect();
            match w.first() {
                Some(&"info") if !w.contains(&"lowerbound") && !w.contains(&"upperbound") && w.contains(&"pv") => {
                    let at = |k: &str| w.iter().position(|x| *x == k).and_then(|i| w.get(i + 1));
                    let k = at("multipv").and_then(|k| k.parse::<usize>().ok()).unwrap_or(1);
                    let Some(slot) = pvs.get_mut(k.wrapping_sub(1)) else { continue };
                    if k == 1
                        && let Some(d) = at("depth").and_then(|d| d.parse().ok())
                    {
                        a.depth = d;
                    }
                    let flip = |v: i32| if black { -v } else { v };
                    if let Some(i) = w.iter().position(|x| *x == "score") {
                        let v = w.get(i + 2).and_then(|v| v.parse::<i32>().ok()).map(flip);
                        (slot.0, slot.1) = match w.get(i + 1) {
                            Some(&"mate") => (None, v),
                            _ => (v, None),
                        };
                    }
                    let from = w.iter().position(|x| *x == "pv").map_or(w.len(), |i| i + 1);
                    slot.2 = w[from..].iter().map(|s| s.to_string()).collect();
                }
                Some(&"bestmove") => {
                    // An engine that gave no line still names its move.
                    if pvs[0].2.is_empty()
                        && let Some(m) = w.get(1)
                    {
                        pvs[0].2 = vec![m.to_string()];
                    }
                    for (cp, mate, pv) in pvs {
                        let mut line = EngineLine { cp, mate, eval: eval_text(cp, mate), ..Default::default() };
                        let mut f = fen.clone();
                        for m in pv.iter().take(12) {
                            let Some(p) = play(&f, m) else { break };
                            f = p.fen.clone();
                            line.moves.push(p.san.clone());
                            line.best.get_or_insert(p);
                        }
                        // A mated position has no move, but its score still counts.
                        if line.best.is_some() || line.mate.is_some() || line.cp.is_some() {
                            a.lines.push(line);
                        }
                    }
                    return Ok(a);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_and_promotions() {
        let all = moves(pgn::START);
        assert_eq!(all.len(), 20);
        let e4 = play(pgn::START, "e2e4").unwrap();
        assert_eq!((e4.san.as_str(), e4.from, e4.to, e4.white, e4.number), ("e4", 12, 28, true, 1));
        assert!(play(pgn::START, "e2e5").is_none());
        // A pawn on the seventh: four moves to one square, one for each piece.
        let fen = "8/P6k/8/8/8/8/8/K7 w - - 0 1";
        let to_a8: Vec<_> = moves(fen).into_iter().filter(|m| m.to == 56).collect();
        assert_eq!(to_a8.iter().map(|m| m.promotion.unwrap()).collect::<String>(), "qrbn");
        assert_eq!(play(fen, "a7a8n").unwrap().san, "a8=N");
        // Castling as the king's move, and as the king onto its rook.
        let c = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        assert_eq!(play(c, "e1g1").unwrap().san, "O-O");
        assert_eq!(play(c, "e1a1").unwrap().san, "O-O-O");
        assert_eq!(play(c, "e1g1").unwrap().to, 6);
        assert!(moves("nonsense").is_empty());
    }

    #[test]
    fn a_line_as_pgn() {
        let tags = vec![("White".to_string(), "Morphy \"Paul\"".to_string()), ("Result".to_string(), "1-0".to_string())];
        let sans: Vec<String> = ["e4", "e5", "Nf3"].map(String::from).to_vec();
        let text = line_pgn(&tags, pgn::START, &sans);
        assert!(text.contains("[White \"Morphy \\\"Paul\\\"\"]") && text.contains("[Result \"*\"]") && !text.contains("FEN"));
        assert!(text.ends_with("1. e4 e5 2. Nf3 *\n") || text.ends_with("1. e4 e5 2. Nf3 *\n"), "{text}");
        let g = pgn::parse(&text);
        assert_eq!(g.main_line().len(), 3);
        assert_eq!(g.tag("White"), Some("Morphy \"Paul\""));
        // From Black's move in a set-up position.
        let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
        let text = line_pgn(&[], fen, &["c5".to_string()]);
        assert!(text.contains("[FEN \"") && text.contains("1... c5 *"), "{text}");
        assert_eq!(pgn::parse(&text).nodes[0].fen.is_some(), true);
    }

    #[test]
    fn a_line_saved_next_to_its_file() {
        let d = std::env::temp_dir().join(format!("coxswain-line-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let src = d.join("games.pgn.gz");
        assert_eq!(save_beside(&src, "line", "x").unwrap(), d.join("games line.pgn"));
        assert_eq!(save_beside(&src, "line", "y").unwrap(), d.join("games line 2.pgn"));
        assert_eq!(std::fs::read_to_string(d.join("games line.pgn")).unwrap(), "x");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn evaluations_as_shown() {
        assert_eq!(eval_text(Some(35), None), "+0.35");
        assert_eq!(eval_text(Some(-120), None), "−1.20");
        assert_eq!(eval_text(None, Some(3)), "#3");
        assert_eq!(eval_text(None, Some(-2)), "#−2");
    }

    /// A UCI engine of a few lines (a shell script): what it says is read as Stockfish's is,
    /// Black's score turned to White's side.
    #[cfg(unix)]
    #[test]
    fn an_engine_asked() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-uci-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("engine");
        std::fs::write(
            &p,
            "#!/bin/sh\nwhile read l; do case \"$l\" in uci) echo 'id name Fake 1'; echo uciok;; isready) echo readyok;; go*) echo 'info depth 5 score cp 40 lowerbound pv a7a6'; echo 'info depth 9 seldepth 12 multipv 1 score cp 30 nodes 99 pv e7e5 g1f3 b8c6'; echo 'info depth 9 multipv 2 score mate -4 pv c7c5'; echo 'bestmove e7e5 ponder g1f3';; quit) exit 0;; esac; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
        let a = analyse(&Program::Installed(p.clone()), fen, 100, 3, &|| false).unwrap();
        assert_eq!((a.engine.as_str(), a.depth, a.lines.len()), ("Fake 1", 9, 2));
        let (one, two) = (&a.lines[0], &a.lines[1]);
        assert_eq!((one.cp, one.eval.as_str()), (Some(-30), "−0.30"));
        assert_eq!(one.best.as_ref().unwrap().san, "e5");
        assert_eq!(one.moves, ["e5", "Nf3", "Nc6"]);
        // Black to move and mating: plus for Black is minus from White's side.
        assert_eq!((two.mate, two.eval.as_str(), two.moves[0].as_str()), (Some(4), "#4", "c5"));
        assert!(analyse(&Program::Installed(p.clone()), fen, 100, 1, &|| true).is_err());
        assert_eq!(engine(&p.to_string_lossy()), Some(p.clone()));
        // A bare name is looked for on PATH only, never as a file in the working folder.
        let bare = format!("coxswain-test-engine-{}", std::process::id());
        std::fs::write(&bare, "").ok();
        let found = engine(&bare);
        std::fs::remove_file(&bare).ok();
        assert_eq!(found, None);
        // One that never answers is given up on when asked to stop, and killed.
        let silent = d.join("silent");
        std::fs::write(&silent, "#!/bin/sh\nexec sleep 1000\n").unwrap();
        std::fs::set_permissions(&silent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let t = std::time::Instant::now();
        let soon = || t.elapsed() > std::time::Duration::from_millis(300);
        assert!(analyse(&Program::Installed(silent), fen, 100, 1, &soon).is_err());
        assert!(t.elapsed() < crate::test_limit(std::time::Duration::from_secs(5)));
        std::fs::remove_dir_all(&d).unwrap();
    }
}

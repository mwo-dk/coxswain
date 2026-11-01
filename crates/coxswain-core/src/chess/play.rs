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
        fen: pgn::fen(&after),
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

/// The file name `<name> <word>.pgn` (`games line.pgn`), or `<name> <word> 2.pgn` and on, for a
/// new PGN made from the game file `source`.
fn save_name(source: &Path, word: &str, n: u32) -> String {
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    let cut = [".pgn.gz", ".pgn.zst", ".pgn", ".si4", ".si5", ".cbh", ".cbv", ".epd", ".fen", ".bin"]
        .iter()
        .find(|e| lower.ends_with(*e))
        .map_or(name.len(), |e| name.len() - e.len());
    let stem = &name[..cut];
    if n <= 1 { format!("{stem} {word}.pgn") } else { format!("{stem} {word} {n}.pgn") }
}

/// Writes `text` as a new PGN file in `dir`, named for the game file `source`. Never over a file.
fn save_in(dir: &Path, source: &Path, word: &str, text: &str) -> std::io::Result<PathBuf> {
    for n in 1..10_000 {
        let p = dir.join(save_name(source, word, n));
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

/// Writes `text` as a new PGN file next to the game file `source`: `<name> <word>.pgn` (`games
/// line.pgn`), or `<name> <word> 2.pgn` and on when that is taken. Never over a file.
pub fn save_beside(source: &Path, word: &str, text: &str) -> std::io::Result<PathBuf> {
    save_in(source.parent().unwrap_or(Path::new(".")), source, word, text)
}

/// Where a new PGN made from a game inside an archive is saved.
#[derive(Clone, Debug, PartialEq)]
pub enum Where {
    /// Beside the archive (the default).
    Beside,
    /// In another folder.
    Folder(PathBuf),
    /// Into the archive itself, next to the game file.
    Inside,
}

/// A game file that was opened from inside an archive: the archive, the game file's path in it,
/// and whether the archive can be written (not a disk image, mailbox, split archive or one inside
/// another).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Origin {
    pub archive: PathBuf,
    pub inner: String,
    pub writable: bool,
}

/// The archive that the game file `source` (a copy `archive::peek` made) came from; `None` for a
/// file that is not such a copy.
pub fn origin(source: &Path) -> Option<Origin> {
    let (archive, inner) = crate::archive::split(&crate::archive::peeked_from(source)?)?;
    let writable = !crate::archive::read_only(&archive) && !crate::archive::nested(&archive, &inner, false);
    Some(Origin { archive, inner, writable })
}

/// Saves `text` as a new PGN made from the game file `source` where `at` says (`Beside` for a
/// file that is not from an archive too), and returns the status line that names where it went.
pub fn save_where(source: &Path, at: &Where, word: &str, text: &str) -> Result<String, String> {
    let failed = |e: std::io::Error| crate::t!("chess.line_not_saved", "error" => e);
    let Some(o) = origin(source) else {
        let p = save_beside(source, word, text).map_err(failed)?;
        return Ok(crate::t!("chess.line_saved", "name" => file_name(&p)));
    };
    let archive = o.archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match at {
        Where::Beside => {
            let p = save_in(o.archive.parent().unwrap_or(Path::new(".")), source, word, text).map_err(failed)?;
            Ok(crate::t!("chess.saved_beside", "name" => file_name(&p), "archive" => archive))
        }
        Where::Folder(dir) => {
            let p = save_in(dir, source, word, text).map_err(failed)?;
            Ok(crate::t!("chess.saved_folder", "name" => file_name(&p), "folder" => dir.display()))
        }
        Where::Inside => {
            if !o.writable {
                return Err(crate::t!("chess.save_inside_no", "archive" => archive));
            }
            let folder = o.inner.rsplit_once('/').map_or("", |(f, _)| f);
            let name = crate::archive::add_text(&o.archive, folder, &|n| save_name(source, word, n), text).map_err(failed)?;
            Ok(crate::t!("chess.saved_inside", "name" => name, "archive" => archive))
        }
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// What the engine says about a position: its best lines, the best first.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Analysis {
    /// The engine's name, as it gives it (`Stockfish 17.1`).
    pub engine: String,
    pub depth: u32,
    pub lines: Vec<EngineLine>,
    /// Options set for this engine that were not sent (not offered, or a value that does not
    /// hold), said once a session: shown on the status line.
    pub note: Option<String>,
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
    /// Its moves as played, each with the position after it: what stepping through the line
    /// shows on the board without playing it into the game.
    pub steps: Vec<Played>,
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

/// A program called `name` on PATH or in its usual folder, and on Debian and its kin in
/// `/usr/games`, where their chess engines go; on Windows also a download that kept its long
/// name (`stockfish-windows-x86-64-avx2.exe`) in a PATH folder or WinGet's package folders.
/// Only looked for, never run.
fn find(name: &str) -> Option<PathBuf> {
    crate::tools::which(name)
        .or_else(|| {
            let p = Path::new("/usr/games").join(name);
            (cfg!(unix) && p.is_file()).then_some(p)
        })
        .or_else(|| if cfg!(windows) { long_named(name, ".exe", windows_dirs()) } else { None })
}

/// PATH's folders, then WinGet's package folders and the one level inside each (its zips
/// unpack into a folder of their own). Never the Downloads folder: whatever lands there
/// would run as the engine.
fn windows_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).filter(|d| d.is_absolute()).collect()).unwrap_or_default();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let packages = Path::new(&local).join(r"Microsoft\WinGet\Packages");
        for pkg in std::fs::read_dir(packages).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
            dirs.extend(std::fs::read_dir(&pkg).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()));
            dirs.push(pkg);
        }
    }
    dirs
}

/// The first file (by name) in the first of `dirs` that has one named `<name>-…` or `<name>_…`
/// and ending in `ext`, letter case aside: an engine downloaded under its long name.
fn long_named(name: &str, ext: &str, dirs: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let name = name.to_ascii_lowercase();
    dirs.into_iter().filter_map(|d| std::fs::read_dir(d).ok()).find_map(|rd| {
        let mut hits: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let f = p.file_name().and_then(|f| f.to_str()).unwrap_or_default().to_ascii_lowercase();
                f.strip_prefix(&name).is_some_and(|r| r.starts_with(['-', '_'])) && f.ends_with(ext) && p.is_file()
            })
            .collect();
        hits.sort();
        hits.into_iter().next()
    })
}

/// A UCI engine Settings offers to choose: its name, its program's names (the first is the
/// one its packages install and `[chess] engine` takes), its official site, and what to know
/// about it (a `chess.note_*` text).
pub struct Known {
    pub name: &'static str,
    pub programs: &'static [&'static str],
    pub site: Option<&'static str>,
    pub note: Option<&'static str>,
}

/// The engines Settings lists; any other is chosen by its path. Only UCI engines: Crafty
/// speaks the xboard protocol alone, and Komodo Dragon's site has closed.
pub const KNOWN: &[Known] = &[
    Known { name: "Stockfish", programs: &["stockfish"], site: Some("https://stockfishchess.org/download/"), note: None },
    Known { name: "Leela Chess Zero", programs: &["lc0"], site: Some("https://lczero.org/play/download/"), note: Some("chess.note_lc0") },
    Known { name: "Berserk", programs: &["berserk"], site: Some("https://github.com/jhonnold/berserk/releases"), note: None },
    Known { name: "Ethereal", programs: &["ethereal-chess", "ethereal", "Ethereal"], site: Some("https://github.com/AndyGrant/Ethereal"), note: None },
    Known { name: "RubiChess", programs: &["RubiChess", "rubichess"], site: Some("https://github.com/Matthies/RubiChess/releases"), note: None },
    Known { name: "Koivisto", programs: &["Koivisto", "koivisto"], site: Some("https://github.com/Luecx/Koivisto/releases"), note: None },
    Known { name: "Igel", programs: &["igel"], site: Some("https://github.com/vshcherbyna/igel/releases"), note: None },
    Known { name: "Arasan", programs: &["arasanx-64", "arasanx"], site: Some("https://www.arasanchess.org/"), note: None },
    Known { name: "Fruit", programs: &["fruit"], site: None, note: Some("chess.note_fruit") },
    Known { name: "Fairy-Stockfish", programs: &["fairy-stockfish"], site: Some("https://github.com/fairy-stockfish/Fairy-Stockfish/releases"), note: Some("chess.note_fairy") },
];

/// A known engine as Settings shows it: where it is (only looked for, never run), else the
/// line that installs it here; whether `[chess] engine` chooses it.
#[derive(Clone, Debug, Serialize)]
pub struct Listed {
    pub name: &'static str,
    /// The name `[chess] engine` takes: the one found, else the one its packages install.
    pub program: String,
    pub site: Option<&'static str>,
    pub note: Option<String>,
    pub path: Option<PathBuf>,
    pub install: Option<String>,
    pub chosen: bool,
}

/// The known engines here. Stockfish is chosen by an empty `[chess] engine` too (then
/// Coxswain's Stockfish image when none is installed).
/// A path in `[chess] engine` to a known engine's program (`~/engines/stockfish`) counts as
/// that engine, found there and chosen.
pub fn listed(cfg: &crate::config::Config) -> Vec<Listed> {
    let chosen = cfg.chess.engine.trim();
    let own = (chosen.starts_with('~') || chosen.contains(['/', '\\'])).then(|| engine(chosen)).flatten();
    let named = |p: &Path, k: &Known| {
        let n = p.file_stem().and_then(|n| n.to_str()).unwrap_or_default();
        k.programs.contains(&n)
    };
    KNOWN
        .iter()
        .map(|k| {
            let by_path = own.as_ref().filter(|p| named(p, k)).map(|p| (chosen, p.clone()));
            let found = by_path.clone().or_else(|| k.programs.iter().find_map(|p| find(p).map(|path| (*p, path))));
            let program = found.as_ref().map_or(k.programs[0], |f| f.0).to_string();
            Listed {
                name: k.name,
                site: k.site,
                note: k.note.map(|n| crate::t!(n)),
                install: found.is_none().then(|| crate::tools::install(k.programs[0])).flatten(),
                chosen: by_path.is_some() || k.programs.contains(&chosen) || (chosen.is_empty() && k.programs[0] == "stockfish"),
                path: found.map(|f| f.1),
                program,
            }
        })
        .collect()
}

/// What `[chess] engine` is set to for the known engine `program`: Stockfish is the default.
pub fn choose(program: &str) -> &str {
    if program == "stockfish" { "" } else { program }
}

/// The engine to ask: the one `[chess] engine` names, else `stockfish` on PATH. None: none is
/// installed. A bare name is looked for on PATH only, never in the working folder (often the
/// folder being browsed, where a downloaded file could be named `stockfish`); a path is one
/// with a separator, `~` the home folder, and a relative one starts from the home folder.
pub fn engine(configured: &str) -> Option<PathBuf> {
    let c = configured.trim();
    if c.is_empty() {
        return find("stockfish");
    }
    if !c.starts_with('~') && !c.contains(['/', '\\']) {
        return find(c);
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
    let local = || find("stockfish").map(Program::Installed);
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

/// What ends the engine's thinking about a position (`[chess] engine_limit`, `review_limit`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Limit {
    /// So many milliseconds (`go movetime`).
    Time(u32),
    /// So many half-moves deep (`go depth`).
    Depth(u32),
    /// So many positions searched (`go nodes`).
    Nodes(u64),
}

/// How long a depth or a node count may take before the engine is told to stop: a slow engine
/// or a large number never keeps the view waiting longer.
pub const CAP: std::time::Duration = std::time::Duration::from_secs(60);

impl Limit {
    /// The limit these settings give a position; `review`: one of a game analysed as a whole.
    pub fn of(c: &crate::config::ChessConfig, review: bool) -> Limit {
        let (kind, ms, depth, nodes) = if review { (&c.review_limit, c.review_time, c.review_depth, c.review_nodes) } else { (&c.engine_limit, c.engine_time, c.engine_depth, c.engine_nodes) };
        match kind.as_str() {
            "depth" => Limit::Depth(depth.clamp(1, 99)),
            "nodes" => Limit::Nodes(nodes.clamp(1_000, 10_000_000_000)),
            _ => Limit::Time(ms.clamp(50, 600_000)),
        }
    }

    fn go(self) -> String {
        match self {
            Limit::Time(ms) => format!("go movetime {ms}"),
            Limit::Depth(d) => format!("go depth {d}"),
            Limit::Nodes(n) => format!("go nodes {n}"),
        }
    }

    /// When the engine is told to stop if it has not answered by itself.
    fn cap(self) -> std::time::Duration {
        match self {
            Limit::Time(ms) => std::time::Duration::from_millis(ms as u64 + 5_000),
            _ => CAP,
        }
    }
}

/// One of the options an engine offers (its `option` lines after `uci`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct UciOption {
    pub name: String,
    /// `spin` (a whole number from `min` to `max`), `check` (true or false), `combo` (one of
    /// `vars`), `string` or `button`.
    pub kind: String,
    pub default: String,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub vars: Vec<String>,
}

/// Options Coxswain sets itself for each position: not the user's to set.
const MANAGED: [&str; 3] = ["MultiPV", "UCI_Chess960", "Ponder"];

impl UciOption {
    /// One the user can set: not one Coxswain sets, and no button (each analysis starts the
    /// engine afresh, so a button pressed would do nothing).
    pub fn settable(&self) -> bool {
        self.kind != "button" && !MANAGED.iter().any(|m| m.eq_ignore_ascii_case(&self.name))
    }
}

/// An `option name … type … [default …] [min …] [max …] [var …]*` line.
fn parse_option(line: &str) -> Option<UciOption> {
    let mut o = UciOption::default();
    let mut field = "";
    for w in line.split_whitespace().skip(1) {
        // A name may hold any word but `type`.
        if matches!(w, "name" | "type" | "default" | "min" | "max" | "var") && (field != "name" || w == "type") {
            field = w;
            if w == "var" {
                o.vars.push(String::new());
            }
            continue;
        }
        let join = |s: &mut String| {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(w);
        };
        match field {
            "name" => join(&mut o.name),
            "type" => o.kind = w.to_string(),
            "default" => join(&mut o.default),
            "min" => o.min = w.parse().ok(),
            "max" => o.max = w.parse().ok(),
            "var" => join(o.vars.last_mut().expect("pushed at var")),
            _ => {}
        }
    }
    if o.default == "<empty>" {
        o.default.clear();
    }
    (!o.name.is_empty() && !o.kind.is_empty()).then_some(o)
}

/// The engine's name without its version (`Stockfish 17.1` → `Stockfish`, `Lc0 v0.31.2` →
/// `Lc0`): its table under `[chess.engines]`, kept when it is upgraded.
pub fn engine_key(name: &str) -> String {
    let mut w: Vec<&str> = name.split_whitespace().collect();
    let version = |s: &str| {
        let s = s.trim_start_matches(['v', 'V']);
        s.starts_with(|c: char| c.is_ascii_digit()) || s.starts_with("dev")
    };
    while w.len() > 1 && w.last().is_some_and(|l| version(l)) {
        w.pop();
    }
    if w.is_empty() { "engine".into() } else { w.join(" ") }
}

/// The most a spin option may be set to here: the engine's own most, and for `Threads` no more
/// than this machine's logical processors, for `Hash` (in MB) no more than half its memory (and
/// 1 GB in Coxswain's container, which is bounded to 3 GB).
fn most(o: &UciOption, container: bool) -> i64 {
    let own = o.max.unwrap_or(i64::MAX);
    let here = match o.name.to_ascii_lowercase().as_str() {
        "threads" => std::thread::available_parallelism().map_or(1, |n| n.get()) as i64,
        "hash" => {
            let half = match crate::setup::ram_gb() {
                0 => i64::MAX,
                gb => gb as i64 * 512,
            };
            if container { half.min(1024) } else { half }
        }
        _ => i64::MAX,
    };
    own.min(here)
}

/// The value `v` for the option `o` as sent in `setoption … value`, or why it cannot be: a
/// spin's whole number within its bounds, a check's true or false, one of a combo's values, a
/// string of at most 1,024 characters with no line break or other control character.
pub fn check(o: &UciOption, v: &toml::Value, container: bool) -> Result<String, String> {
    let bad = || crate::t!("chess.option_bad", "option" => &o.name);
    match o.kind.as_str() {
        "spin" => {
            let n = match v {
                toml::Value::Integer(n) => *n,
                toml::Value::String(s) => s.trim().parse().map_err(|_| bad())?,
                _ => return Err(bad()),
            };
            let (lo, hi) = (o.min.unwrap_or(i64::MIN), most(o, container));
            if n < lo || n > hi {
                return Err(crate::t!("chess.option_range", "option" => &o.name, "min" => lo, "max" => hi));
            }
            Ok(n.to_string())
        }
        "check" => match v {
            toml::Value::Boolean(b) => Ok(b.to_string()),
            toml::Value::String(s) if s == "true" || s == "false" => Ok(s.clone()),
            _ => Err(bad()),
        },
        "combo" => {
            let s = v.as_str().ok_or_else(bad)?;
            o.vars.iter().find(|x| x.eq_ignore_ascii_case(s)).cloned().ok_or_else(bad)
        }
        "string" => {
            let s = v.as_str().ok_or_else(bad)?;
            if s.chars().count() > 1024 || s.chars().any(char::is_control) {
                return Err(bad());
            }
            Ok(if s.is_empty() { "<empty>".into() } else { s.to_string() })
        }
        _ => Err(bad()),
    }
}

/// The notice for options set but not sent: once a session for the same engine and options.
fn ignored_note(engine: &str, ignored: &[(String, String)]) -> Option<String> {
    static SAID: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    if ignored.is_empty() {
        return None;
    }
    let key = format!("{engine}\u{1}{ignored:?}");
    let mut said = SAID.lock().unwrap_or_else(|e| e.into_inner());
    if said.contains(&key) {
        return None;
    }
    said.push(key);
    let list: Vec<String> = ignored.iter().map(|(n, why)| format!("{n}: {why}")).collect();
    Some(crate::t!("chess.options_ignored", "engine" => engine_key(engine), "options" => list.join("; ")))
}

/// Asks the engine `program` about the position `fen` as `chess` says (its limit, lines and
/// options), over UCI on its standard input and output; it runs here and nothing leaves the
/// machine. `stop` is asked at each line the engine writes: true ends it at once (the position
/// on screen changed).
pub fn analyse(program: &Program, chess: &crate::config::ChessConfig, fen: &str, stop: &dyn Fn() -> bool) -> std::io::Result<Analysis> {
    pgn::board(&pgn::full_fen(fen)).ok_or_else(|| std::io::Error::other("no position"))?;
    let mut e = Engine::start(program, stop)?;
    let ignored = e.configure(chess)?;
    let mut a = e.think(fen, Limit::of(chess, false), chess.engine_lines, stop)?;
    a.note = ignored_note(&e.name, &ignored);
    Ok(a)
}

/// The engine of these settings, asked what it is and what options it offers (Settings' *Test
/// engine*): with the values `[chess.engines]` gives them, and the ones set there that it does
/// not take, with why.
#[derive(Clone, Debug, Default, Serialize)]
pub struct EngineInfo {
    pub name: String,
    /// Its table under `[chess.engines]` (`engine_key`).
    pub key: String,
    pub container: bool,
    pub options: Vec<Shown>,
    pub ignored: Vec<(String, String)>,
}

/// An option as Settings shows it: its `max` the most allowed here, its value when set.
#[derive(Clone, Debug, Serialize)]
pub struct Shown {
    #[serde(flatten)]
    pub option: UciOption,
    pub value: Option<toml::Value>,
}

/// Starts the engine of `cfg`, reads its name and options, and stops it.
pub fn info(cfg: &crate::config::Config) -> Result<EngineInfo, String> {
    let program = engine_for(cfg)?;
    let container = matches!(program, Program::Container { .. });
    let e = Engine::start(&program, &|| false).map_err(|e| e.to_string())?;
    let options = e
        .options
        .iter()
        .filter(|o| o.settable())
        .map(|o| {
            let mut option = o.clone();
            if o.kind == "spin" {
                option.max = Some(most(o, container));
            }
            Shown { option, value: None }
        })
        .collect();
    let mut info = EngineInfo { name: e.name.clone(), key: engine_key(&e.name), container, options, ignored: vec![] };
    info.refresh(&cfg.chess);
    Ok(info)
}

impl EngineInfo {
    /// The values `[chess.engines]` gives its options now, and the ones set there it does not
    /// take, with why: after a change, without running the engine again.
    pub fn refresh(&mut self, chess: &crate::config::ChessConfig) {
        let set = chess.engines.get(&self.key);
        let value = |name: &str| set.and_then(|s| s.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone()));
        for s in &mut self.options {
            s.value = value(&s.option.name);
        }
        let container = self.container;
        self.ignored = set
            .into_iter()
            .flatten()
            .filter_map(|(n, v)| match self.options.iter().find(|s| s.option.name.eq_ignore_ascii_case(n)) {
                None => Some((n.clone(), crate::t!("chess.option_unknown"))),
                Some(s) => check(&s.option, v, container).err().map(|why| (n.clone(), why)),
            })
            .collect();
    }
}

/// Sets the option `name` of the engine `info` describes in the config.toml at `path`, after
/// checking `value` (a number, true or false, or text; a spin or check may come as text too);
/// None takes it out, so the engine's default applies.
pub fn set_option(path: &Path, info: &EngineInfo, name: &str, value: Option<&serde_json::Value>) -> Result<crate::config::Config, String> {
    use serde_json::Value as J;
    let s = info.options.iter().find(|s| s.option.name == name).ok_or_else(|| crate::t!("chess.option_unknown"))?;
    let o = &s.option;
    let bad = || crate::t!("chess.option_bad", "option" => &o.name);
    let new = match value {
        None => None,
        Some(v) => {
            let tv = match (o.kind.as_str(), v) {
                ("spin", J::Number(n)) => toml::Value::Integer(n.as_i64().ok_or_else(bad)?),
                ("spin", J::String(t)) => toml::Value::Integer(t.trim().parse().map_err(|_| bad())?),
                ("check", J::Bool(b)) => toml::Value::Boolean(*b),
                ("check", J::String(t)) => toml::Value::Boolean(t.parse().map_err(|_| bad())?),
                (_, J::String(t)) => toml::Value::String(t.clone()),
                _ => return Err(bad()),
            };
            check(o, &tv, info.container)?;
            Some(match tv {
                toml::Value::Integer(i) => toml_edit::Value::from(i),
                toml::Value::Boolean(b) => b.into(),
                toml::Value::String(t) => t.into(),
                _ => return Err(bad()),
            })
        }
    };
    let keys = ["chess", "engines", info.key.as_str(), o.name.as_str()];
    crate::config::update_config(path, |text| {
        let edited = match &new {
            Some(v) => crate::config::Config::edit(text, &keys, v.clone())?,
            None => crate::config::Config::unset(text, &keys)?,
        };
        let cfg = crate::config::Config::parse_saved(text, &edited)?;
        Ok((Some(edited), cfg))
    })
}

/// Takes every option of the engine `key` out of the config.toml at `path`: the engine's own
/// defaults apply again.
pub fn reset_options(path: &Path, key: &str) -> Result<crate::config::Config, String> {
    crate::config::update_config(path, |text| {
        let edited = crate::config::Config::unset(text, &["chess", "engines", key])?;
        let cfg = crate::config::Config::parse_saved(text, &edited)?;
        Ok((Some(edited), cfg))
    })
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
    /// The options it offers.
    pub options: Vec<UciOption>,
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

    /// Starts the engine `program` and waits for it to say it speaks UCI, reading its name and
    /// options; given up on as soon as `stop` is true.
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
        let mut e = Engine { child, stdin, out, name: String::new(), options: vec![], container };
        writeln!(e.stdin, "uci")?;
        e.stdin.flush()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let line = e.line(deadline, stop)?;
            let w: Vec<&str> = line.split_whitespace().collect();
            match w.first() {
                Some(&"id") if w.get(1) == Some(&"name") => e.name = w[2..].join(" "),
                // No engine offers thousands: a runaway one is not kept in memory.
                Some(&"option") if e.options.len() < 1_000 => e.options.extend(parse_option(&line)),
                Some(&"uciok") => return Ok(e),
                _ => {}
            }
        }
    }

    /// Sends the options `[chess.engines]` sets for this engine (by its name without the
    /// version): only ones it offers, with values that hold (`check`); the rest are not sent
    /// and come back with why. A UCI_Elo set turns UCI_LimitStrength on, unless that is set too.
    pub fn configure(&mut self, chess: &crate::config::ChessConfig) -> std::io::Result<Vec<(String, String)>> {
        let mut ignored = vec![];
        let Some(set) = chess.engines.get(&engine_key(&self.name)) else { return Ok(ignored) };
        let container = self.container.is_some();
        let offered = |name: &str| self.options.iter().find(|o| o.settable() && o.name.eq_ignore_ascii_case(name)).cloned();
        let mut lines = String::new();
        for (name, v) in set {
            let Some(o) = offered(name) else {
                ignored.push((name.clone(), crate::t!("chess.option_unknown")));
                continue;
            };
            match check(&o, v, container) {
                Ok(text) => lines.push_str(&format!("setoption name {} value {text}\n", o.name)),
                Err(why) => ignored.push((name.clone(), why)),
            }
        }
        let has = |n: &str| set.keys().any(|k| k.eq_ignore_ascii_case(n));
        if has("UCI_Elo")
            && !has("UCI_LimitStrength")
            && let Some(o) = offered("UCI_LimitStrength")
        {
            lines.push_str(&format!("setoption name {} value true\n", o.name));
        }
        self.stdin.write_all(lines.as_bytes())?;
        self.stdin.flush()?;
        Ok(ignored)
    }

    /// Its best `lines` (1 to 5) for the position `fen`, thinking until `limit`. One that has
    /// not answered by `Limit::cap` is told to stop, and given up on 30 seconds after that.
    /// `stop` is asked at each line the engine writes: true ends it, and the engine is of no
    /// more use.
    pub fn think(&mut self, fen: &str, limit: Limit, lines: u32, stop: &dyn Fn() -> bool) -> std::io::Result<Analysis> {
        let fen = pgn::full_fen(fen);
        let b = pgn::board(&fen).ok_or_else(|| std::io::Error::other("no position"))?;
        let wanted = lines.clamp(1, MAX_LINES) as usize;
        // Castling in Shredder-FEN (`pgn::fen`): a Chess960 position, which the engine must be told.
        let chess960 = fen.split(' ').nth(2).is_some_and(|c| c.chars().any(|c| !"KQkq-".contains(c)));
        write!(
            self.stdin,
            "setoption name UCI_Chess960 value {chess960}\nsetoption name MultiPV value {wanted}\nisready\nposition fen {fen}\n{}\n",
            limit.go()
        )?;
        self.stdin.flush()?;
        let black = b.side_to_move() == Color::Black;
        let mut a = Analysis { engine: self.name.clone(), ..Default::default() };
        // Per line: its score (cp, mate) and its moves in UCI.
        let mut pvs: Vec<(Option<i32>, Option<i32>, Vec<String>)> = vec![Default::default(); wanted];
        let mut deadline = std::time::Instant::now() + limit.cap();
        let mut told = false;
        loop {
            let line = match self.line(deadline, stop) {
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut && !told => {
                    // Past its cap: told to stop, which makes a UCI engine name its move.
                    told = true;
                    self.stdin.write_all(b"stop\n")?;
                    self.stdin.flush()?;
                    deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                    continue;
                }
                r => r?,
            };
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
                            line.steps.push(p.clone());
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
    #[test]
    fn long_named_downloads_are_found() {
        let root = std::env::temp_dir().join(format!("coxswain-engine-names-{}", std::process::id()));
        let (d, e) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::create_dir_all(&e).unwrap();
        for f in ["stockfishbook.exe", "fairy-stockfish-largeboard.exe", "stockfish-notes.txt"] {
            std::fs::write(d.join(f), b"").unwrap();
        }
        std::fs::write(e.join("Stockfish-windows-x86-64-avx2.exe"), b"").unwrap();
        std::fs::write(e.join("stockfish_17_x64.exe"), b"").unwrap();
        let dirs = || vec![d.clone(), e.clone()];
        assert_eq!(super::long_named("stockfish", ".exe", dirs()), Some(e.join("Stockfish-windows-x86-64-avx2.exe")));
        assert_eq!(super::long_named("lc0", ".exe", dirs()), None);
        assert_eq!(super::long_named("fairy-stockfish", ".exe", dirs()), Some(d.join("fairy-stockfish-largeboard.exe")));
        std::fs::remove_dir_all(&root).unwrap();
    }

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
    fn a_game_from_an_archive_is_saved_where_asked() {
        use crate::archive;
        let d = std::env::temp_dir().join(format!("coxswain-where-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("other")).unwrap();
        let games = d.join("games.pgn");
        std::fs::write(&games, "1. e4 *\n").unwrap();
        let zip = d.join("games.zip");
        archive::create(&zip, &[games.clone()]).unwrap();
        std::fs::remove_file(&games).unwrap();
        let copy = archive::peek(&zip.join("games.pgn")).unwrap();
        assert!(origin(&copy).unwrap().writable);

        // Beside the archive, never over a file: the next free name.
        let said = save_where(&copy, &Where::Beside, "line", "a").unwrap();
        assert!(said.contains("games line.pgn") && said.contains("games.zip"), "{said}");
        save_where(&copy, &Where::Beside, "line", "b").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("games line.pgn")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(d.join("games line 2.pgn")).unwrap(), "b");
        // Not beside the cache copy.
        assert!(!copy.with_file_name("games line.pgn").exists());

        // Another folder.
        let said = save_where(&copy, &Where::Folder(d.join("other")), "line", "c").unwrap();
        assert!(said.contains("games line.pgn"), "{said}");
        assert_eq!(std::fs::read_to_string(d.join("other/games line.pgn")).unwrap(), "c");

        // Into the archive: a new entry beside the game, a second time under the next name.
        let said = save_where(&copy, &Where::Inside, "line", "d").unwrap();
        assert!(said.contains("games line.pgn") && said.contains("inside") || said.contains("games.zip"), "{said}");
        save_where(&copy, &Where::Inside, "line", "e").unwrap();
        let names: Vec<String> = archive::list(&zip, 100).unwrap().0.into_iter().map(|e| e.name).collect();
        assert!(names.contains(&"games line.pgn".to_string()) && names.contains(&"games line 2.pgn".to_string()), "{names:?}");
        let inside = archive::peek(&zip.join("games line 2.pgn")).unwrap();
        assert_eq!(std::fs::read_to_string(inside).unwrap(), "e");

        // A file that is not from an archive: next to itself, as before.
        let plain = d.join("other/x.pgn");
        std::fs::write(&plain, "").unwrap();
        assert!(origin(&plain).is_none());
        save_where(&plain, &Where::Inside, "line", "f").unwrap();
        assert!(d.join("other/x line.pgn").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn an_archive_that_cannot_be_written_offers_no_inside() {
        let d = std::env::temp_dir().join(format!("coxswain-where-ro-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mbox = d.join("Games.mbox");
        std::fs::write(&mbox, "From a@example.com Mon Sep 28 10:00:00 2026\nFrom: a@example.com\nSubject: x\n\nbody\n\n").unwrap();
        let (list, _) = crate::archive::list(&mbox, 10).unwrap();
        let copy = crate::archive::peek(&mbox.join(&list[0].name)).unwrap();
        assert!(!origin(&copy).unwrap().writable);
        assert!(save_where(&copy, &Where::Inside, "line", "x").is_err());
        assert!(save_where(&copy, &Where::Beside, "line", "x").is_ok());
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
        let three = crate::config::ChessConfig { engine_time: 100, engine_lines: 3, ..Default::default() };
        let single = crate::config::ChessConfig { engine_lines: 1, ..three.clone() };
        let a = analyse(&Program::Installed(p.clone()), &three, fen, &|| false).unwrap();
        assert_eq!((a.engine.as_str(), a.depth, a.lines.len()), ("Fake 1", 9, 2));
        let (one, two) = (&a.lines[0], &a.lines[1]);
        assert_eq!((one.cp, one.eval.as_str()), (Some(-30), "−0.30"));
        assert_eq!(one.best.as_ref().unwrap().san, "e5");
        assert_eq!(one.moves, ["e5", "Nf3", "Nc6"]);
        // Each move with the position after it, to step through without playing it.
        assert_eq!(one.steps.iter().map(|p| p.san.as_str()).collect::<Vec<_>>(), one.moves);
        assert_eq!(one.steps[2].fen, "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3");
        // Black to move and mating: plus for Black is minus from White's side.
        assert_eq!((two.mate, two.eval.as_str(), two.moves[0].as_str()), (Some(4), "#4", "c5"));
        assert!(analyse(&Program::Installed(p.clone()), &single, fen, &|| true).is_err());
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
        assert!(analyse(&Program::Installed(silent), &single, fen, &soon).is_err());
        assert!(t.elapsed() < crate::test_limit(std::time::Duration::from_secs(5)));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn options_read_and_checked() {
        let o = parse_option("option name Skill Level type spin default 20 min 0 max 20").unwrap();
        assert_eq!((o.name.as_str(), o.kind.as_str(), o.default.as_str(), o.min, o.max), ("Skill Level", "spin", "20", Some(0), Some(20)));
        let c = parse_option("option name Backend type combo default cuda var cuda var opencl blas var eigen").unwrap();
        assert_eq!(c.vars, ["cuda", "opencl blas", "eigen"]);
        let w = parse_option("option name WeightsFile type string default <empty>").unwrap();
        assert_eq!(w.default, "");
        assert!(parse_option("option name Clear Hash type button").is_some_and(|b| !b.settable()));
        assert!(!parse_option("option name MultiPV type spin default 1 min 1 max 500").unwrap().settable());
        use toml::Value as V;
        assert_eq!(check(&o, &V::Integer(5), false), Ok("5".into()));
        assert!(check(&o, &V::Integer(21), false).is_err());
        assert!(check(&o, &V::String("x".into()), false).is_err());
        assert_eq!(check(&c, &V::String("EIGEN".into()), false), Ok("eigen".into()));
        assert!(check(&c, &V::String("metal".into()), false).is_err());
        assert_eq!(check(&w, &V::String(String::new()), false), Ok("<empty>".into()));
        assert!(check(&w, &V::String("a\nquit".into()), false).is_err());
        assert!(check(&w, &V::String("x".repeat(1025)), false).is_err());
        let t = parse_option("option name Threads type spin default 1 min 1 max 1024").unwrap();
        let cpus = std::thread::available_parallelism().unwrap().get() as i64;
        assert!(check(&t, &V::Integer(cpus), false).is_ok() && check(&t, &V::Integer(cpus + 1), false).is_err());
        let h = parse_option("option name Hash type spin default 16 min 1 max 33554432").unwrap();
        assert!(check(&h, &V::Integer(1024), true).is_ok() && check(&h, &V::Integer(1025), true).is_err());
        assert_eq!(engine_key("Stockfish 17.1"), "Stockfish");
        assert_eq!(engine_key("Lc0 v0.31.2"), "Lc0");
        assert_eq!(engine_key("Stockfish dev-20250101-abc"), "Stockfish");
        assert_eq!(engine_key("Komodo Dragon 3.3"), "Komodo Dragon");
        // A path to a known engine's program is that engine, chosen; the rest are not.
        #[cfg(unix)]
        {
            let d = std::env::temp_dir().join(format!("coxswain-listed-{}", std::process::id()));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("lc0"), "").unwrap();
            let mut cfg = crate::config::Config::default();
            cfg.chess.engine = d.join("lc0").display().to_string();
            let l = listed(&cfg);
            let lc0 = l.iter().find(|e| e.name == "Leela Chess Zero").unwrap();
            assert!(lc0.chosen && lc0.path.is_some() && lc0.program == cfg.chess.engine);
            assert_eq!(l.iter().filter(|e| e.chosen).count(), 1);
            std::fs::remove_dir_all(&d).unwrap();
        }
        let mut cfg = crate::config::ChessConfig::default();
        assert_eq!(Limit::of(&cfg, false), Limit::Time(1000));
        cfg.engine_limit = "depth".into();
        cfg.review_limit = "nodes".into();
        assert_eq!((Limit::of(&cfg, false), Limit::of(&cfg, true)), (Limit::Depth(20), Limit::Nodes(300_000)));
        assert_eq!((Limit::Depth(7).go(), Limit::Nodes(5000).go()), ("go depth 7".to_string(), "go nodes 5000".to_string()));
    }

    /// A fake engine that writes down what it is sent: the limit as `go depth`, the options it
    /// offers set (Elo turning the limit of strength on), the rest left out and said once; an
    /// engine that ignores its depth is told to stop at its cap.
    #[cfg(unix)]
    #[test]
    fn an_engine_set_up_and_limited() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-uci-opts-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let log = d.join("sent");
        let p = d.join("engine");
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\nwhile read l; do echo \"$l\" >> '{}'; case \"$l\" in uci) echo 'id name Fake 3.1'; echo 'option name Threads type spin default 1 min 1 max 512'; echo 'option name UCI_LimitStrength type check default false'; echo 'option name UCI_Elo type spin default 1320 min 1320 max 3190'; echo 'option name Style type combo default Normal var Solid var Normal'; echo uciok;; go*) echo 'info depth 4 multipv 1 score cp 10 pv e2e4'; echo 'bestmove e2e4';; quit) exit 0;; esac; done\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut chess = crate::config::ChessConfig { engine_limit: "depth".into(), engine_depth: 12, engine_lines: 1, ..Default::default() };
        let set: std::collections::BTreeMap<String, toml::Value> = toml::from_str("Threads = 1\nUCI_Elo = 1500\nStyle = \"solid\"\nContempt = 5\n").unwrap();
        chess.engines.insert("Fake".into(), set);
        let a = analyse(&Program::Installed(p.clone()), &chess, pgn::START, &|| false).unwrap();
        assert_eq!(a.lines[0].moves, ["e4"]);
        let note = a.note.clone().unwrap_or_default();
        assert!(note.contains("Contempt") && !note.contains("Threads"), "{note}");
        let sent = std::fs::read_to_string(&log).unwrap();
        for l in ["setoption name Threads value 1", "setoption name UCI_Elo value 1500", "setoption name UCI_LimitStrength value true", "setoption name Style value Solid", "go depth 12"] {
            assert!(sent.lines().any(|s| s == l), "{l} not in {sent}");
        }
        assert!(!sent.contains("Contempt") && !sent.contains("movetime"), "{sent}");
        // Said once a session.
        assert_eq!(analyse(&Program::Installed(p.clone()), &chess, pgn::START, &|| false).unwrap().note, None);
        // An engine that answers `go` only when told to stop: stopped at its cap (time + 5 s).
        let slow = d.join("slow");
        std::fs::write(&slow, "#!/bin/sh\nwhile read l; do case \"$l\" in uci) echo uciok;; stop) echo 'bestmove e2e4';; quit) exit 0;; esac; done\n").unwrap();
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut e = Engine::start(&Program::Installed(slow), &|| false).unwrap();
        let t = std::time::Instant::now();
        let a = e.think(pgn::START, Limit::Time(100), 1, &|| false).unwrap();
        assert_eq!(a.lines[0].moves, ["e4"]);
        assert!(t.elapsed() >= std::time::Duration::from_secs(5));
        std::fs::remove_dir_all(&d).unwrap();
    }
}

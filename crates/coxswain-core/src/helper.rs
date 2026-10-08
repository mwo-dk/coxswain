//! The index helper: one process holds the file name index, and every window and the terminal
//! app ask it. Two windows no longer scan the disk twice or keep two copies in memory.
//!
//! The helper is the app itself, started with `--index-helper`, so packages ship nothing
//! extra. The first app that finds no helper starts one; it lingers a while after the last
//! app has gone, then exits. Registered with the system (`service`), it starts with the
//! session instead. What goes wrong when it starts is in `helper.log` in the cache folder.
//!
//! They talk over a loopback TCP socket, one JSON line per request and per reply. The port
//! and a token are in a file only the user can read; a connection that does not start with
//! the token is dropped. If no helper can be reached, the app indexes by itself as before.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::{Config, SearchConfig};
use crate::find::{Found, Kind};
use crate::index::{Results, Service, State};
use crate::sizes::Size;
use crate::store::{self, Store};

/// The argument that makes an app the helper.
pub const ARG: &str = "--index-helper";
/// How long the helper stays after the last app has gone.
const LINGER: Duration = Duration::from_secs(600);
/// A helper of an older version steps down for ours; one of a newer version serves us while
/// it speaks our `PROTOCOL`.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The wire format. One more when a request or a reply changes so that another version cannot
/// read it; added fields with defaults do not count.
const PROTOCOL: u32 = 2;
/// A helper's or an app's version and protocol: other ones in tests.
type Ours = (&'static str, u32);
const OURS: Ours = (VERSION, PROTOCOL);

/// Apps before the protocol was named spoke the first one.
fn first_protocol() -> u32 {
    1
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Hello {
        token: String,
        version: String,
        #[serde(default = "first_protocol")]
        protocol: u32,
    },
    /// File names alone.
    Search { query: String, scope: Option<PathBuf>, max: usize },
    /// Find: names, words in files and meaning, `rows` hits per group.
    Find { query: String, scope: Option<PathBuf>, kind: Kind, rows: usize },
    Status,
    /// Ask: excerpts of the files closest to a question, `bytes` in all, below `scope` when given.
    Passages { query: String, scope: Option<PathBuf>, bytes: usize },
    /// Bytes and files below a folder, from the store.
    Size { path: PathBuf },
    /// Read the backlog without rests.
    IndexNow,
    /// Empty the store; it fills again.
    Forget,
    /// Go, so that a helper with the new settings comes.
    Restart,
    /// Ask the built-in chat model, `cpu` alone or on the GPU, `think` first if it can: the
    /// answer comes in pieces.
    Ask { model: String, cpu: bool, think: bool, earlier: Vec<crate::meaning::Turn>, question: String, sources: Vec<(PathBuf, String)> },
    /// Let the built-in chat model go now, freeing its memory.
    Unload,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "re", rename_all = "snake_case")]
enum Reply {
    /// `same`: the app may use this helper. Otherwise the helper is an older one leaving for the
    /// app's, or, with `newer` (its version), a newer one the app cannot talk to: it stays, and
    /// the app indexes by itself.
    Hello {
        same: bool,
        #[serde(default)]
        newer: Option<String>,
    },
    Results(Results),
    Found(Found),
    Passages { found: Vec<(PathBuf, String)> },
    Status(Status),
    /// With the time the walk they come from began.
    Size { size: Option<(Size, u64)> },
    Done,
    /// A piece of an answer; empty while the model reads.
    Piece { text: String },
    /// The answer is complete, or why there is none.
    Answered { error: Option<String> },
}

/// How the index is doing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub state: State,
    /// File names in the index.
    pub len: usize,
    /// Files whose text can be searched, and files still to be read.
    #[serde(default)]
    pub texts: usize,
    #[serde(default)]
    pub pending: usize,
    /// Bytes the store takes on disk.
    #[serde(default)]
    pub bytes: u64,
    /// Search by meaning: on, files still to get their vectors, files that have them.
    #[serde(default)]
    pub meaning: bool,
    #[serde(default)]
    pub meaning_pending: usize,
    #[serde(default)]
    pub meaning_done: usize,
    /// Passages that have their vectors: what a change of model makes again.
    #[serde(default)]
    pub meaning_passages: usize,
    /// Files whose vectors are being made again for a new way of cutting passages (0 when
    /// none), and how long one takes here, in milliseconds (0 until measured).
    #[serde(default)]
    pub meaning_renewing: usize,
    #[serde(default)]
    pub meaning_ms_per_file: usize,
    /// Which model makes the vectors (`builtin:…`, `ollama:bge-m3`), and why it could not.
    #[serde(default)]
    pub meaning_engine: String,
    #[serde(default)]
    pub meaning_error: Option<String>,
    /// Where the built-in model runs: the GPU or the CPU, and why the CPU on a Mac.
    #[serde(default)]
    pub meaning_runs: Option<crate::meaning::Runs>,
    /// The built-in chat model loaded in the helper, and whether on a Mac's GPU.
    #[serde(default)]
    pub chat_loaded: Option<(String, bool)>,
    /// Why reading the files' text failed last time; nothing further on is read until it works.
    #[serde(default)]
    pub error: Option<String>,
    /// Reading waits until the machine is off its battery.
    #[serde(default)]
    pub paused: bool,
    /// The folders read, each with where it is now (none while its disk is not plugged in)
    /// and its bytes and files.
    #[serde(default)]
    pub roots: Vec<(PathBuf, Option<PathBuf>, Size)>,
    /// The installed programs that read more (OCR, LibreOffice), and whether each is there.
    #[serde(default)]
    pub tools: Vec<(String, bool)>,
    /// The clouds whose files the walk found only in the cloud: (name, folder).
    #[serde(default)]
    pub clouds: Vec<(String, PathBuf)>,
    /// The helper is this newer version, which this app cannot talk to: it indexes by itself
    /// until it is restarted. Never sent by a helper.
    #[serde(default)]
    pub outdated: Option<String>,
}

/// Where the helper's address and lock live: the cache folder, which is the user's own.
pub fn folder() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("coxswain"))
}

/// Whether a helper runs now: it holds `index.lock` for as long as it does.
pub fn runs() -> bool {
    let Some(lock) = folder().and_then(|d| std::fs::File::open(d.join("index.lock")).ok()) else { return false };
    matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

/// Where a helper's errors go: `helper.log` beside its address.
pub fn log() -> Option<PathBuf> {
    Some(folder()?.join("helper.log"))
}

/// A line in helper.log (the helper's stderr), with the time.
fn note(what: std::fmt::Arguments) {
    eprintln!("{} coxswain {VERSION}: {what}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
}

/// The system was to start the registered helper and did not, so this app started one: a
/// notice says where to allow it.
pub static UNSTARTED: AtomicBool = AtomicBool::new(false);

/// A request longer than this is nobody's: the connection is dropped, so a stray process on the
/// machine cannot make the helper swallow memory.
const LINE_MAX: u64 = 1 << 20;
/// How long a connection has to say hello; an app does so at once.
const HELLO_WAIT: Duration = Duration::from_secs(5);

/// 128 bits from the system's random source, as hex.
fn token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system's random source");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `theirs` is the token, taking the same time whatever the first wrong byte.
fn is_token(theirs: &str, token: &str) -> bool {
    theirs.len() == token.len() && theirs.bytes().zip(token.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// `text` into `path`, readable by this user alone. On Windows the cache folder is in the
/// user's profile, which is theirs alone already; the file inherits that.
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(&tmp)?.write_all(text.as_bytes())?;
    std::fs::rename(tmp, path)
}

// ---------------------------------------------------------------- the helper

/// Be the helper: serve until no app has asked for `LINGER`. Returns at once when another
/// helper already runs.
pub fn serve() -> io::Result<()> {
    // Not one that grows without end, when a helper fails again and again.
    if let Some(log) = log().and_then(|l| std::fs::OpenOptions::new().write(true).open(l).ok())
        && log.metadata().is_ok_and(|m| m.len() > 1 << 20)
    {
        let _ = log.set_len(0);
    }
    note(format_args!("helper starts, process {}", std::process::id()));
    let served = serve_here();
    if let Err(e) = &served {
        note(format_args!("helper stops: {e}"));
    }
    served
}

fn serve_here() -> io::Result<()> {
    crate::fs::lock_down();
    // Started by launchd, the helper has the bare system PATH, without the programs that read
    // more (Homebrew's tesseract, pdftotext).
    if cfg!(target_os = "macos") {
        crate::tools::login_path();
    }
    let dir = folder().ok_or_else(|| io::Error::other("no cache folder"))?;
    let search = Config::load().map(|c| c.search).unwrap_or_default();
    // First, or the store cannot be made on a machine that has no cache folder yet.
    std::fs::create_dir_all(&dir)?;
    let store = if search.text { Store::open(&dir.join("search.db")).inspect_err(|e| note(format_args!("search inside files is off: the search store: {e}"))).ok().map(Arc::new) } else { None };
    if let Some(s) = &store {
        s.set_engine(if search.meaning { crate::meaning::Engine::from_config(&search) } else { None });
    }
    let cfg = search.clone();
    // Registered to start with the session: it stays when the apps have gone.
    let linger = if std::env::args().any(|a| a == crate::service::STAY) { Duration::MAX } else { LINGER };
    serve_in(OURS, &dir, linger, move || Service::start(&search), store.map(|s| (s, cfg)))
}

/// `serve` with its version, folder, patience and index given, for tests.
fn serve_in(ours: Ours, dir: &Path, linger: Duration, index: impl FnOnce() -> Arc<Service>, texts: Option<(Arc<Store>, SearchConfig)>) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    // One helper at a time: the lock is released when this process ends, however it ends.
    let lock = std::fs::File::create(dir.join("index.lock"))?;
    // A helper that was asked to go may take a moment to let go.
    let wait = Instant::now();
    while lock.try_lock().is_err() {
        if wait.elapsed() > Duration::from_secs(3) {
            note(format_args!("another helper runs: this one leaves"));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let (token, addr) = (token(), dir.join("index.addr"));
    write_private(&addr, &format!("{}\n{token}\n", listener.local_addr()?.port()))?;

    let index = index();
    let stop = Arc::new(AtomicBool::new(false));
    let text_roots: Arc<[PathBuf]> = texts.as_ref().map(|(_, cfg)| store::roots(cfg)).unwrap_or_default().into();
    let store = texts.map(|(store, cfg)| {
        let (s, stop) = (store.clone(), stop.clone());
        let changes = index.changes();
        std::thread::spawn(move || store::keep_current(&s, &cfg, &stop, Some(changes)));
        store
    });
    let (clients, quit, last) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(Instant::now())));
    {
        let (clients, quit, last, text_roots) = (clients.clone(), quit.clone(), last.clone(), text_roots.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (index, store, roots, token, clients, quit, last) = (index.clone(), store.clone(), text_roots.clone(), token.clone(), clients.clone(), quit.clone(), last.clone());
                std::thread::spawn(move || {
                    clients.fetch_add(1, Ordering::SeqCst);
                    let _ = answer(ours, stream, &index, store.as_deref(), &roots, &token, &quit);
                    clients.fetch_sub(1, Ordering::SeqCst);
                    *last.lock().unwrap() = Instant::now();
                });
            }
        });
    }
    while !quit.load(Ordering::SeqCst) && (clients.load(Ordering::SeqCst) > 0 || last.lock().unwrap().elapsed() < linger) {
        std::thread::sleep(linger.min(Duration::from_millis(200)));
    }
    stop.store(true, Ordering::SeqCst);
    let _ = std::fs::remove_file(addr);
    note(format_args!("helper leaves: {}", if quit.load(Ordering::SeqCst) { "an app asked it to, for new settings or its own version" } else { "no app asked for a while" }));
    Ok(())
}

/// What a helper of `ours` does when an app of `version`, speaking `protocol`, says hello.
#[derive(Debug, PartialEq)]
enum Greet {
    /// A newer app: the helper leaves, and the app starts its own.
    Leave,
    /// The same or an older app that speaks its protocol.
    Serve,
    /// An older app that does not: the helper stays, and the app indexes by itself. An old app
    /// does not hand over to an older helper: that one would leave again for the next newer app.
    Refuse,
}

fn greet(ours: Ours, version: &str, protocol: u32) -> Greet {
    if newer(version, ours.0) {
        Greet::Leave
    } else if protocol == ours.1 {
        Greet::Serve
    } else {
        Greet::Refuse
    }
}

/// Whether version `a` comes after `b`: 2.10.0 after 2.9.1.
fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| v.split('.').map(|p| p.split(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse().ok()).unwrap_or(0u64)).collect::<Vec<_>>();
    parts(a) > parts(b)
}

/// `what` in helper.log the first time only: an app that cannot be served asks again and again.
fn note_once(what: String) {
    static TOLD: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut told = TOLD.lock().unwrap();
    if !told.contains(&what) {
        note(format_args!("{what}"));
        told.push(what);
    }
}

fn answer(ours: Ours, stream: TcpStream, index: &Service, store: Option<&Store>, text_roots: &[PathBuf], token: &str, quit: &AtomicBool) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(HELLO_WAIT))?;
    let mut out = stream.try_clone()?;
    let mut from = BufReader::new(stream);
    let mut said_hello = false;
    let mut line = String::new();
    loop {
        line.clear();
        if (&mut from).take(LINE_MAX).read_line(&mut line)? == 0 || !line.ends_with('\n') {
            return Ok(());
        }
        let reply = match serde_json::from_str(&line).map_err(io::Error::other)? {
            Request::Hello { token: theirs, version, protocol } => {
                if !is_token(&theirs, token) {
                    return Ok(());
                }
                said_hello = true;
                // An app keeps its line open for as long as it runs.
                out.set_read_timeout(None)?;
                match greet(ours, &version, protocol) {
                    // A helper on its way out sends newcomers to the next one.
                    _ if quit.load(Ordering::SeqCst) => Reply::Hello { same: false, newer: None },
                    Greet::Leave => {
                        if !quit.fetch_or(true, Ordering::SeqCst) {
                            note(format_args!("an app of the newer version {version} came: leaving for its helper"));
                        }
                        Reply::Hello { same: false, newer: None }
                    }
                    Greet::Serve => {
                        if version != ours.0 {
                            note_once(format!("an app of the older version {version} came: it uses this helper, which stays"));
                        }
                        Reply::Hello { same: true, newer: None }
                    }
                    Greet::Refuse => {
                        note_once(format!("an app of the older version {version} came, which speaks protocol {protocol} and this helper {}: it stays, and the app indexes by itself until it is restarted", ours.1));
                        Reply::Hello { same: false, newer: Some(ours.0.into()) }
                    }
                }
            }
            _ if !said_hello => return Ok(()),
            Request::Search { query, scope, max } => Reply::Results(index.search(&query, scope.as_deref(), max)),
            Request::Find { query, scope, kind, rows } => {
                let found = crate::find::run(|q, max| index.search(q, scope.as_deref(), max), store.ok_or(crate::find::Off::TextOff), text_roots, &query, scope.as_deref(), kind, rows);
                Reply::Found(found)
            }
            Request::Passages { query, scope, bytes } => Reply::Passages { found: store.map(|s| s.passages(&query, scope.as_deref(), bytes)).unwrap_or_default() },
            Request::Size { path } => Reply::Size { size: store.and_then(|s| s.size(&path)) },
            Request::IndexNow => {
                store.inspect(|s| s.hurry.store(true, Ordering::Relaxed));
                Reply::Done
            }
            Request::Forget => {
                store.map(Store::clear).transpose().map_err(io::Error::other)?;
                Reply::Done
            }
            Request::Restart => {
                quit.store(true, Ordering::SeqCst);
                Reply::Done
            }
            Request::Unload => {
                crate::chat::unload();
                Reply::Done
            }
            // An app that stops the answer closes the line: the next piece cannot be sent.
            Request::Ask { model, cpu, think, earlier, question, sources } => {
                let mut gone = false;
                let done = match crate::chat::of(&model) {
                    Some(m) => crate::chat::ask(m, cpu, think, &earlier, &question, &sources, |text| {
                        gone = send(&mut out, &Reply::Piece { text: text.into() }).is_err();
                        !gone
                    }),
                    None => Err(format!("no built-in model {model}")),
                };
                if gone {
                    return Ok(());
                }
                Reply::Answered { error: done.err() }
            }
            Request::Status => Reply::Status({
                let counts = store.map(Store::meaning_counts).unwrap_or_default();
                Status {
                state: index.state(),
                len: index.len(),
                texts: store.map_or(0, Store::texts),
                pending: store.map_or(0, |s| s.pending.load(Ordering::Relaxed)),
                bytes: store.map_or(0, Store::bytes),
                meaning: store.is_some_and(|s| s.meaning.load(Ordering::Relaxed)),
                meaning_pending: counts.0,
                meaning_done: counts.1,
                meaning_passages: store.map_or(0, Store::passage_count),
                meaning_renewing: store.map_or(0, |s| s.renewing.load(Ordering::Relaxed)),
                meaning_ms_per_file: store.map_or(0, |s| s.ms_per_file.load(Ordering::Relaxed)),
                meaning_engine: store.and_then(Store::engine_id).unwrap_or_default(),
                meaning_error: store.and_then(|s| s.meaning_error.lock().unwrap().clone()),
                meaning_runs: store.and_then(Store::engine_runs),
                chat_loaded: crate::chat::loaded().map(|(id, metal)| (id.to_string(), metal)),
                error: store.and_then(|s| s.error.lock().unwrap().clone()),
                paused: store.is_some_and(|s| s.paused.load(Ordering::Relaxed)),
                roots: store.map(Store::root_sizes).unwrap_or_default(),
                tools: crate::extract::installed::found().iter().map(|(n, p)| (n.to_string(), p.is_some())).collect(),
                clouds: store.map(Store::clouds).unwrap_or_default(),
                outdated: None,
                }
            }),
        };
        send(&mut out, &reply)?;
    }
}

fn send(out: &mut TcpStream, reply: &Reply) -> io::Result<()> {
    let mut text = serde_json::to_string(reply).map_err(io::Error::other)?;
    text.push('\n');
    out.write_all(text.as_bytes())
}

// ---------------------------------------------------------------- the apps' side

type Line = (BufReader<TcpStream>, TcpStream);

/// The index as an app sees it: the helper's, or its own when no helper can be had.
pub struct Client {
    dir: Option<PathBuf>,
    /// Starts a helper, the `n`th time no helper answers (`STEPS`). The apps start themselves
    /// with `ARG`.
    spawn: Box<dyn Fn(usize) + Send + Sync>,
    search: SearchConfig,
    ours: Ours,
    /// A newer helper this app cannot talk to answered: its version.
    outdated: OnceLock<String>,
    line: Mutex<Option<Line>>,
    /// The last status and when it was asked: the terminal app asks with every frame.
    status: Mutex<Option<(Instant, Status)>>,
    own: OnceLock<Arc<Service>>,
}

impl Client {
    /// Connects in the background, so the app starts without waiting for the helper.
    pub fn start(search: &SearchConfig) -> Arc<Client> {
        let client = Client::with(folder(), search, |n| {
            let registered = crate::service::installed();
            let Ok(exe) = crate::tools::this_app() else { return };
            // A registration that starts another program (an older version, or one an upgrade
            // has removed) is taken over by this app, unless an upgrade removed this one.
            if n == 0 && registered && exe.exists() && !crate::service::starts(&exe) && crate::service::install(&exe).is_ok() {
                return;
            }
            match step(n, registered, crate::service::supervised()) {
                Step::Ask => crate::service::kick(),
                Step::Own { stay } => {
                    let _ = launch(&exe, stay);
                }
                Step::Rescue => {
                    UNSTARTED.store(true, Ordering::Relaxed);
                    let _ = launch(&exe, true);
                }
                Step::Wait => {}
            }
        });
        let c = client.clone();
        std::thread::spawn(move || {
            crate::service::refresh();
            c.status()
        });
        client
    }

    pub fn with(dir: Option<PathBuf>, search: &SearchConfig, spawn: impl Fn(usize) + Send + Sync + 'static) -> Arc<Client> {
        Client::of(OURS, dir, search, spawn)
    }

    /// `with` for an app of another version, for tests.
    fn of(ours: Ours, dir: Option<PathBuf>, search: &SearchConfig, spawn: impl Fn(usize) + Send + Sync + 'static) -> Arc<Client> {
        Arc::new(Client { dir, spawn: Box::new(spawn), search: search.clone(), ours, outdated: OnceLock::new(), line: Mutex::default(), status: Mutex::default(), own: OnceLock::new() })
    }

    pub fn search(&self, query: &str, scope: Option<&Path>, max: usize) -> Results {
        match self.ask(&Request::Search { query: query.into(), scope: scope.map(Path::to_path_buf), max }) {
            Some(Reply::Results(r)) => r,
            _ => self.own().search(query, scope, max),
        }
    }

    /// Find: names, words in files and meaning, below `scope` when given, `rows` hits per
    /// group. Without the helper, names alone from the app's own index: the store is the
    /// helper's alone.
    pub fn find(&self, query: &str, scope: Option<&Path>, kind: Kind, rows: usize) -> Found {
        match self.ask(&Request::Find { query: query.into(), scope: scope.map(Path::to_path_buf), kind, rows }) {
            Some(Reply::Found(f)) => f,
            _ => crate::find::run(|q, max| self.own().search(q, scope, max), Err(if self.search.text { crate::find::Off::NoHelper } else { crate::find::Off::TextOff }), &[], query, scope, kind, rows),
        }
    }

    /// Ask: excerpts of the files closest to `question`, `bytes` in all, below `scope` when
    /// given (`Store::passages`). Nothing without the helper, or while search by meaning is off.
    pub fn passages(&self, question: &str, scope: Option<&Path>, bytes: usize) -> Vec<(PathBuf, String)> {
        match self.ask(&Request::Passages { query: question.into(), scope: scope.map(Path::to_path_buf), bytes }) {
            Some(Reply::Passages { found }) => found,
            _ => vec![],
        }
    }

    /// Ask's answer, piece by piece to `piece` (false stops it). The built-in model answers in
    /// the helper, on a line of its own, so that every app shares one copy of it in memory and
    /// the app's other questions go on meanwhile; here when there is no helper. A server's
    /// model is asked from here.
    pub fn answer(&self, cfg: &SearchConfig, earlier: &[crate::meaning::Turn], question: &str, sources: &[(PathBuf, String)], mut piece: impl FnMut(&str) -> bool) -> Result<(), String> {
        let line = (crate::chat::of(&cfg.ask_model).is_some() && self.own.get().is_none()).then(|| self.connect()).flatten();
        let Some((mut from, mut to)) = line else { return crate::meaning::ask(cfg, earlier, question, sources, piece) };
        let ask = Request::Ask { model: cfg.ask_model.clone(), cpu: cfg.meaning_device == "cpu", think: cfg.ask_think, earlier: earlier.to_vec(), question: question.into(), sources: sources.to_vec() };
        let mut text = serde_json::to_string(&ask).map_err(|e| e.to_string())?;
        text.push('\n');
        // Pieces come at least between parts of the prompt, seconds apart on a slow CPU.
        to.set_read_timeout(Some(Duration::from_secs(600))).map_err(|e| e.to_string())?;
        to.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        loop {
            let mut line = String::new();
            if from.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("the search helper stopped".into());
            }
            match serde_json::from_str(&line).map_err(|e| e.to_string())? {
                Reply::Piece { text } if piece(&text) => {}
                Reply::Piece { .. } => return Ok(()),
                Reply::Answered { error } => return error.map_or(Ok(()), Err),
                _ => return Err("the search helper answered something else".into()),
            }
        }
    }

    /// Bytes and files below `dir` as the store knew them, with when its walk began.
    pub fn size(&self, dir: &Path) -> Option<(Size, u64)> {
        match self.ask(&Request::Size { path: dir.to_path_buf() }) {
            Some(Reply::Size { size }) => size,
            _ => None,
        }
    }

    /// Read the backlog of the files' text at full speed.
    pub fn index_now(&self) {
        self.ask(&Request::IndexNow);
    }

    /// Empty the search store; the helper fills it again.
    pub fn forget(&self) {
        self.ask(&Request::Forget);
    }

    /// The built-in chat model is let go, in the helper and in this app.
    pub fn unload(&self) {
        crate::chat::unload();
        self.ask(&Request::Unload);
    }

    /// What is loaded of the built-in models: in the helper, else in this app.
    pub fn loaded(&self) -> crate::models::Loaded {
        let st = self.status();
        let mine = crate::chat::loaded().map(|(id, metal)| (id.to_string(), metal));
        crate::models::Loaded { chat: st.chat_loaded.or(mine), meaning: st.meaning_runs }
    }

    /// The helper goes and a new one comes, with the settings as they are now.
    pub fn restart(&self) {
        self.ask(&Request::Restart);
        *self.line.lock().unwrap() = None;
        *self.status.lock().unwrap() = None;
    }

    pub fn state(&self) -> State {
        self.status().state
    }

    pub fn len(&self) -> usize {
        self.status().len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the index is the helper's (and not our own, the fallback).
    pub fn shared(&self) -> bool {
        self.own.get().is_none()
    }

    pub fn status(&self) -> Status {
        let mut status = self.status.lock().unwrap();
        if let Some((at, known)) = &*status {
            if at.elapsed() < Duration::from_millis(500) {
                return known.clone();
            }
        }
        // A search under way holds the line: the status from before will do, so drawing the
        // screen never waits for a search.
        if self.line.try_lock().is_err() {
            if let Some((_, known)) = &*status {
                return known.clone();
            }
        }
        let now = match self.ask(&Request::Status) {
            Some(Reply::Status(s)) => s,
            _ => Status { state: self.own().state(), len: self.own().len(), texts: 0, pending: 0, bytes: 0, paused: false, roots: vec![], tools: vec![], meaning: false, meaning_pending: 0, meaning_done: 0, meaning_passages: 0, meaning_renewing: 0, meaning_ms_per_file: 0, meaning_engine: String::new(), meaning_error: None, meaning_runs: None, chat_loaded: None, error: None, clouds: vec![], outdated: self.outdated.get().cloned() },
        };
        *status = Some((Instant::now(), now.clone()));
        now
    }

    fn own(&self) -> &Arc<Service> {
        self.own.get_or_init(|| Service::start(&self.search))
    }

    /// One request, one reply. A broken line is opened again once, which starts a helper when
    /// none runs. `None` when that fails too: then the app's own index answers, from now on.
    fn ask(&self, request: &Request) -> Option<Reply> {
        if self.own.get().is_some() {
            return None;
        }
        let mut line = self.line.lock().unwrap();
        for _ in 0..2 {
            if line.is_none() {
                *line = self.connect();
            }
            let Some(l) = line.as_mut() else { break };
            match exchange(l, request) {
                Ok(reply) => return Some(reply),
                Err(_) => *line = None,
            }
        }
        None
    }

    fn connect(&self) -> Option<Line> {
        let dir = self.dir.as_deref()?;
        let mut tries = 0;
        let wait = Instant::now();
        // A registered helper that was asked to go takes the system a moment to start again.
        let patience = Duration::from_secs(if crate::service::installed() { 15 } else { 5 });
        loop {
            match dial(dir) {
                Some((mut line, token)) => match exchange(&mut line, &Request::Hello { token, version: self.ours.0.into(), protocol: self.ours.1 }) {
                    Ok(Reply::Hello { same: true, .. }) => return Some(line),
                    // A newer helper that cannot serve us stays: neither asking it to go nor
                    // starting ours, which would be that newer version again.
                    Ok(Reply::Hello { newer: Some(v), .. }) => {
                        if self.outdated.set(v.clone()).is_ok() {
                            app_note(dir, format_args!("the helper is the newer version {v}, which this app of {} cannot talk to: it indexes by itself until it is restarted", self.ours.0));
                        }
                        return None;
                    }
                    // An older helper leaving for ours, or a stale address: ours comes next.
                    _ => {}
                },
                None if STEPS.get(tries).is_some_and(|&at| wait.elapsed() >= at) => {
                    (self.spawn)(tries);
                    tries += 1;
                }
                None => {}
            }
            if wait.elapsed() > patience {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// When, after the first try, an app that finds no helper starts one again (see `step`).
const STEPS: [Duration; 3] = [Duration::ZERO, Duration::from_secs(3), Duration::from_secs(8)];

/// What an app does the `n`th time it finds no helper.
#[derive(Debug, PartialEq)]
enum Step {
    /// Ask systemd or launchd to start the registered one.
    Ask,
    /// Start one, which stays when it is registered.
    Own { stay: bool },
    /// The service manager did not start it: start one that stays, and say so.
    Rescue,
    Wait,
}

/// A registered helper is the service manager's to start (twice: a helper that was just asked
/// to go may still be leaving), then the app's own; the lock lets only one of them serve. On
/// Windows and the BSDs nothing starts it again, so the app does at once.
fn step(n: usize, registered: bool, supervised: bool) -> Step {
    match (n, registered && supervised) {
        (0 | 1, true) => Step::Ask,
        (_, true) => Step::Rescue,
        (0, false) => Step::Own { stay: registered },
        _ => Step::Wait,
    }
}

/// The helper's address file, read and dialled.
fn dial(dir: &Path) -> Option<(Line, String)> {
    let text = std::fs::read_to_string(dir.join("index.addr")).ok()?;
    let mut lines = text.lines();
    let (port, token) = (lines.next()?.parse().ok()?, lines.next()?.to_string());
    let stream = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), Duration::from_millis(300)).ok()?;
    stream.set_nodelay(true).ok()?;
    // A search over a million names answers in milliseconds; a helper that is silent this
    // long is stuck.
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    Some(((BufReader::new(stream.try_clone().ok()?), stream), token))
}

fn exchange((from, to): &mut Line, request: &Request) -> io::Result<Reply> {
    let mut text = serde_json::to_string(request).map_err(io::Error::other)?;
    text.push('\n');
    to.write_all(text.as_bytes())?;
    let mut line = String::new();
    if from.read_line(&mut line)? == 0 {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    serde_json::from_str(&line).map_err(io::Error::other)
}

/// A line in helper.log from an app, which has its own stderr.
fn app_note(dir: &Path, what: std::fmt::Arguments) {
    let mut o = std::fs::OpenOptions::new();
    o.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    if let Ok(mut f) = o.open(dir.join("helper.log")) {
        let _ = writeln!(f, "{} coxswain {VERSION} (app, process {}): {what}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), std::process::id());
    }
}

/// Start `exe` as a helper that outlives the app and its terminal, staying when `stay`. Waited
/// for in a thread, so that one which leaves before the app does not stay behind as a zombie.
pub(crate) fn launch(exe: &Path, stay: bool) -> io::Result<()> {
    let mut c = detached(exe);
    if stay {
        c.arg(crate::service::STAY);
    }
    let mut child = c.spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

fn detached(exe: &Path) -> std::process::Command {
    use std::process::Stdio;
    let mut c = crate::tools::command(exe);
    // Its errors to helper.log, which only this user can read.
    let log = log().and_then(|l| {
        crate::fs::private(l.parent()?, None).ok()?;
        let mut o = std::fs::OpenOptions::new();
        o.create(true).append(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        o.open(l).ok()
    });
    c.arg(ARG).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log.map_or_else(Stdio::null, Stdio::from));
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    #[cfg(windows)]
    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP: no console, and Ctrl+C in the app's is not ours.
    std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0000_0008 | 0x0000_0200);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-helper-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("files/src")).unwrap();
        std::fs::create_dir_all(d.join("cache")).unwrap();
        for (f, text) in [("files/src/main.rs", "fn main() { launch(); }"), ("files/src/lib.rs", "pub fn orbit() {}"), ("files/README.md", "# Rocket")] {
            std::fs::write(d.join(f), text).unwrap();
        }
        d
    }

    fn config(d: &Path) -> SearchConfig {
        SearchConfig { roots: vec![d.join("files")], text_roots: vec![d.join("files")], watch: false, ..SearchConfig::default() }
    }

    /// A helper in a thread of this test, as an app would start one.
    fn helper(d: &Path, linger: Duration) -> impl Fn() + Send + Sync + Clone + 'static {
        helper_of(OURS, d, linger)
    }

    /// The same, of another version.
    fn helper_of(ours: Ours, d: &Path, linger: Duration) -> impl Fn() + Send + Sync + Clone + 'static {
        let (dir, search) = (d.join("cache"), config(d));
        move || {
            let (dir, search) = (dir.clone(), search.clone());
            std::thread::spawn(move || {
                let store = Arc::new(Store::open(&dir.join("search.db")).unwrap());
                serve_in(ours, &dir, linger, { let s = search.clone(); move || Service::start(&s) }, Some((store, search))).unwrap()
            });
        }
    }

    fn ready(c: &Client) {
        let wait = Instant::now();
        while c.state() != State::Ready && wait.elapsed() < crate::test_limit(Duration::from_secs(10)) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(c.state(), State::Ready);
    }

    /// Ask end to end with the built-in models (downloads 1.6 GB once): the helper reads the
    /// files for meaning, finds the passages closest to a question, and its chat model answers
    /// from them, piece by piece over the line.
    #[test]
    #[ignore]
    fn asks_the_builtin_model_through_the_helper() {
        let d = tree("ask");
        std::fs::write(d.join("files/plan.md"), "# Launch plan\n\nThe rocket is called Kestrel. It launches from Andøya on 14 March, weather permitting. The crew of three trains in Tromsø all winter.").unwrap();
        std::fs::write(d.join("files/budget.md"), "# Budget\n\nFuel costs 40,000 euros per flight, more than the crew and the launch pad together. The board meets in April.").unwrap();
        let m = &crate::chat::MODELS[0];
        crate::meaning::download(&Default::default()).unwrap();
        m.download(&Default::default()).unwrap();
        let search = SearchConfig { text: true, meaning: true, ask_model: m.key(), ..config(&d) };
        let (dir, s) = (d.join("cache"), search.clone());
        std::thread::spawn(move || {
            let store = Arc::new(Store::open(&dir.join("search.db")).unwrap());
            store.set_engine(crate::meaning::Engine::from_config(&s));
            serve_in(OURS, &dir, Duration::from_secs(5), { let s = s.clone(); move || Service::start(&s) }, Some((store, s))).unwrap()
        });
        let c = Client::with(Some(d.join("cache")), &search, |_| {});
        let (question, wait) = ("What does the fuel cost per flight?", Instant::now());
        let mut sources = vec![];
        while sources.len() < 2 && wait.elapsed() < Duration::from_secs(120) {
            std::thread::sleep(Duration::from_millis(500));
            sources = c.passages(question, None, 10);
        }
        assert!(!sources.is_empty(), "no passages");
        let (mut answer, mut pieces, start) = (String::new(), 0, Instant::now());
        c.answer(&search, &[], question, &sources, |t| {
            pieces += !t.is_empty() as usize;
            answer.push_str(t);
            true
        })
        .unwrap();
        eprintln!("{answer}\n-- {pieces} pieces in {:.1?}", start.elapsed());
        assert!(answer.contains("40") && pieces > 3, "{answer}");
    }

    #[test]
    fn helper_is_started_shared_and_leaves() {
        let d = tree("shared");
        let starts = Arc::new(AtomicUsize::new(0));
        let start = {
            let (starts, helper) = (starts.clone(), helper(&d, Duration::from_millis(400)));
            move |_| {
                starts.fetch_add(1, Ordering::SeqCst);
                helper()
            }
        };
        let start = Arc::new(start);
        let start = move |n| start(n);
        let one = Client::with(Some(d.join("cache")), &config(&d), start.clone());
        let two = Client::with(Some(d.join("cache")), &config(&d), start);
        ready(&one);
        ready(&two);
        assert!(one.shared() && two.shared());
        assert_eq!(starts.load(Ordering::SeqCst), 1, "the second app found the first one's helper");
        let names = |c: &Client| c.search("*.rs", None, 10).hits.iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names(&one).len(), 2);
        assert_eq!(names(&one), names(&two));
        assert_eq!(one.len(), two.len());

        // The text of the files, from the same helper.
        let wait = Instant::now();
        while one.status().texts < 3 && wait.elapsed() < crate::test_limit(Duration::from_secs(10)) {
            std::thread::sleep(Duration::from_millis(100));
        }
        let found = two.find("launch", None, Kind::All, 10);
        assert_eq!(found.in_files.hits.iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(), ["main.rs"]);
        assert!(found.in_files.hits[0].snippet.as_deref().unwrap().contains("launch"));
        assert!(found.off.contains(&crate::find::Off::MeaningOff));
        // Limited to a folder: its files alone, names and text; a folder whose text is not read says so.
        let src = two.find("main", Some(&d.join("files/src")), Kind::All, 10);
        assert_eq!(src.names.hits.len(), 1);
        let away = two.find("launch", Some(&d.join("cache")), Kind::InFiles, 10);
        assert_eq!(away.off, [crate::find::Off::NotRead(d.join("cache"))]);
        // And folder sizes from its store.
        assert_eq!(two.size(&d.join("files/src")).map(|s| s.0), Some(crate::fs::dir_size(&d.join("files/src"))));

        // A restart brings a new helper.
        one.restart();
        ready(&one);
        assert_eq!(starts.load(Ordering::SeqCst), 2, "a new helper came");

        // Both apps go: the helper waits its while, then takes its address with it.
        drop((one, two));
        let wait = Instant::now();
        while d.join("cache/index.addr").exists() && wait.elapsed() < crate::test_limit(Duration::from_secs(5)) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!d.join("cache/index.addr").exists());
        // The helper may still hold the store: on Windows an open file cannot be removed.
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn helpers_hand_over_to_newer_apps_alone() {
        assert!(newer("2.10.0", "2.9.1") && newer("2.3.0", "2.2.0") && !newer("2.2.0", "2.3.0") && !newer("2.3.0", "2.3.0"));
        let ours = ("2.3.0", 2);
        assert_eq!(greet(ours, "2.4.0", 1), Greet::Leave, "a newer app, whatever it speaks");
        assert_eq!([greet(ours, "2.3.0", 2), greet(ours, "2.2.0", 2)], [Greet::Serve, Greet::Serve]);
        assert_eq!(greet(ours, "2.2.0", 1), Greet::Refuse);
    }

    /// Wait until a helper answers at `d`'s cache folder.
    fn up(d: &Path) {
        let wait = Instant::now();
        while dial(&d.join("cache")).is_none() && wait.elapsed() < crate::test_limit(Duration::from_secs(5)) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// An app that counts the helpers it starts, and starts `start` with each.
    fn counting(start: impl Fn() + Send + Sync + 'static) -> (Arc<AtomicUsize>, impl Fn(usize) + Send + Sync + 'static) {
        let n = Arc::new(AtomicUsize::new(0));
        let m = n.clone();
        (n, move |_| {
            m.fetch_add(1, Ordering::SeqCst);
            start()
        })
    }

    #[test]
    fn an_old_app_uses_a_newer_helper_that_speaks_its_protocol() {
        let d = tree("older");
        helper_of(("9.0.0", PROTOCOL), &d, Duration::from_secs(5))();
        up(&d);
        // The old app's own start would be the new version again, through the link on PATH.
        let (starts, spawn) = counting(helper_of(("9.0.0", PROTOCOL), &d, Duration::from_secs(5)));
        let old = Client::of(("1.0.0", PROTOCOL), Some(d.join("cache")), &config(&d), spawn);
        ready(&old);
        assert!(old.shared(), "served by the newer helper");
        assert_eq!(old.search("README", None, 10).total, 1);
        // An app from before the protocol was named says no protocol: it speaks the first, which
        // this one no longer does.
        let (mut line, token) = dial(&d.join("cache")).unwrap();
        line.1.write_all(format!("{{\"op\":\"hello\",\"token\":\"{token}\",\"version\":\"1.0.0\"}}\n").as_bytes()).unwrap();
        let mut reply = String::new();
        line.0.read_line(&mut reply).unwrap();
        assert_eq!(reply.contains("\"same\":true"), PROTOCOL == first_protocol(), "{reply}");
        // The helper stayed: the new app finds it, and nobody started another.
        let new = Client::of(("9.0.0", PROTOCOL), Some(d.join("cache")), &config(&d), |_| panic!("the helper left"));
        ready(&new);
        assert!(new.shared() && old.shared());
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        drop((old, new));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn an_old_app_leaves_a_newer_helper_it_cannot_talk_to_alone() {
        let d = tree("refused");
        helper_of(("9.0.0", PROTOCOL + 1), &d, Duration::from_secs(5))();
        up(&d);
        let (starts, spawn) = counting(helper_of(("9.0.0", PROTOCOL + 1), &d, Duration::from_secs(5)));
        let old = Client::of(("1.0.0", PROTOCOL), Some(d.join("cache")), &config(&d), spawn);
        ready(&old);
        // Its own index, at once, and a notice to restart; the helper neither left nor came again.
        assert!(!old.shared());
        assert_eq!(old.search("README", None, 10).total, 1);
        assert_eq!(old.status().outdated.as_deref(), Some("9.0.0"));
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        let log = std::fs::read_to_string(d.join("cache/helper.log")).unwrap();
        assert!(log.contains("the helper is the newer version 9.0.0"), "{log}");
        let new = Client::of(("9.0.0", PROTOCOL + 1), Some(d.join("cache")), &config(&d), |_| panic!("the helper left"));
        ready(&new);
        assert!(new.shared());
        drop((old, new));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_new_app_takes_over_from_an_old_helper_and_the_old_app_follows() {
        let d = tree("newer");
        helper_of(("1.0.0", PROTOCOL), &d, Duration::from_secs(5))();
        up(&d);
        let new_helper = helper_of(("9.0.0", PROTOCOL), &d, Duration::from_secs(5));
        let (new_starts, spawn) = counting(new_helper.clone());
        let new = Client::of(("9.0.0", PROTOCOL), Some(d.join("cache")), &config(&d), spawn);
        ready(&new);
        assert!(new.shared());
        assert_eq!(new_starts.load(Ordering::SeqCst), 1, "the old helper left, and the new app started its own");
        // The old app comes back to the new helper, which stays: no ping-pong.
        let (old_starts, spawn) = counting(new_helper);
        let old = Client::of(("1.0.0", PROTOCOL), Some(d.join("cache")), &config(&d), spawn);
        ready(&old);
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(new.search("README", None, 10).total, 1);
        assert!(old.shared() && new.shared());
        assert_eq!((new_starts.load(Ordering::SeqCst), old_starts.load(Ordering::SeqCst)), (1, 0));
        drop((old, new));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_helper_the_system_does_not_start_is_started_by_the_app() {
        // Registered with systemd or launchd: asked for twice, then the app's own, and a notice.
        assert_eq!([0, 1, 2, 3].map(|n| step(n, true, true)), [Step::Ask, Step::Ask, Step::Rescue, Step::Rescue]);
        // Windows and the BSDs: the app starts one that stays, at once.
        assert_eq!([0, 1].map(|n| step(n, true, false)), [Step::Own { stay: true }, Step::Wait]);
        // Not registered: one that leaves after the last app.
        assert_eq!([0, 1, 2].map(|n| step(n, false, true)), [Step::Own { stay: false }, Step::Wait, Step::Wait]);

        // The first try brings nothing (the system was asked, and did not start it); the second does.
        let d = tree("rescue");
        let tries = Arc::new(Mutex::new(vec![]));
        let start = {
            let (tries, helper) = (tries.clone(), helper(&d, Duration::from_millis(400)));
            move |n| {
                tries.lock().unwrap().push(n);
                if n == 1 {
                    helper()
                }
            }
        };
        let app = Client::with(Some(d.join("cache")), &config(&d), start);
        ready(&app);
        assert!(app.shared(), "the helper started on the second try answers");
        assert_eq!(*tries.lock().unwrap(), [0, 1]);
        drop(app);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn helper_wants_the_token_and_the_app_manages_without_one() {
        let d = tree("token");
        helper(&d, Duration::from_secs(5))();
        let wait = Instant::now();
        while dial(&d.join("cache")).is_none() && wait.elapsed() < crate::test_limit(Duration::from_secs(5)) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let (mut line, token) = dial(&d.join("cache")).unwrap();
        assert!(exchange(&mut line, &Request::Status).is_err(), "no hello, no answer");
        let (mut line, _) = dial(&d.join("cache")).unwrap();
        assert!(exchange(&mut line, &Request::Hello { token: "guess".into(), version: VERSION.into(), protocol: PROTOCOL }).is_err());
        let (mut line, _) = dial(&d.join("cache")).unwrap();
        assert!(matches!(exchange(&mut line, &Request::Hello { token: token.clone(), version: VERSION.into(), protocol: PROTOCOL }), Ok(Reply::Hello { same: true, .. })));
        // Ask with a built-in model there is not: the answer is why, and the line goes on.
        let ask = Request::Ask { model: "builtin:none".into(), cpu: true, think: false, earlier: vec![], question: "?".into(), sources: vec![] };
        assert!(matches!(exchange(&mut line, &ask), Ok(Reply::Answered { error: Some(_) })));
        assert!(matches!(exchange(&mut line, &Request::Status), Ok(Reply::Status(_))));
        assert_eq!(token.len(), 32);
        assert!(token.bytes().all(|b| b.is_ascii_hexdigit()) && token != super::token());
        assert!(is_token(&token, &token) && !is_token(&token[1..], &token) && !is_token(&format!("{token}0"), &token));
        // A line without end that goes on and on is dropped before it fills the memory.
        let (mut long, _) = dial(&d.join("cache")).unwrap();
        let _ = long.1.write_all(&vec![b'a'; 2 << 20]);
        let _ = long.1.write_all(b"\n");
        assert!(exchange(&mut long, &Request::Hello { token, version: VERSION.into(), protocol: PROTOCOL }).is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(d.join("cache/index.addr")).unwrap().permissions().mode() & 0o777, 0o600);
        }

        // No helper to be had: the app indexes by itself.
        let alone = Client::with(None, &config(&d), |_| {});
        ready(&alone);
        assert_eq!(alone.search("README", None, 10).total, 1);
        assert!(!alone.shared());
        // The helper may still hold the store: on Windows an open file cannot be removed.
        let _ = std::fs::remove_dir_all(d);
    }
}

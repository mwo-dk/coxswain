//! The index helper: one process holds the file name index, and every window and the terminal
//! app ask it. Two windows no longer scan the disk twice or keep two copies in memory.
//!
//! The helper is the app itself, started with `--index-helper`, so packages ship nothing
//! extra. The first app that finds no helper starts one; it lingers a while after the last
//! app has gone, then exits. Nothing is registered with the system.
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
use crate::index::{Results, Service, State};
use crate::sizes::Size;
use crate::store::{self, Store};

/// The argument that makes an app the helper.
pub const ARG: &str = "--index-helper";
/// How long the helper stays after the last app has gone.
const LINGER: Duration = Duration::from_secs(600);
/// A helper of another version steps down for ours.
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Hello { token: String, version: String },
    /// `text`: in the files' text, not in their names.
    Search {
        query: String,
        scope: Option<PathBuf>,
        max: usize,
        #[serde(default)]
        text: bool,
    },
    Status,
    /// Ask: the passages closest to a question, whole.
    Passages { query: String, max: usize },
    /// Bytes and files below a folder, from the store.
    Size { path: PathBuf },
    /// Read the backlog without rests.
    IndexNow,
    /// Empty the store; it fills again.
    Forget,
    /// Go, so that a helper with the new settings comes.
    Restart,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "re", rename_all = "snake_case")]
enum Reply {
    /// `same`: the helper is our version. Otherwise it exits, and we start ours.
    Hello { same: bool },
    Results(Results),
    Passages { found: Vec<(PathBuf, String)> },
    Status(Status),
    /// With the time the walk they come from began.
    Size { size: Option<(Size, u64)> },
    Done,
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
    /// Which model makes the vectors (`builtin:…`, `ollama:bge-m3`), and why it could not.
    #[serde(default)]
    pub meaning_engine: String,
    #[serde(default)]
    pub meaning_error: Option<String>,
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
}

/// Where the helper's address and lock live: the cache folder, which is the user's own.
pub fn folder() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("coxswain"))
}

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
    crate::fs::lock_down();
    let dir = folder().ok_or_else(|| io::Error::other("no cache folder"))?;
    let search = Config::load().map(|c| c.search).unwrap_or_default();
    // First, or the store cannot be made on a machine that has no cache folder yet.
    std::fs::create_dir_all(&dir)?;
    let store = if search.text { Store::open(&dir.join("search.db")).inspect_err(|e| eprintln!("coxswain: search store: {e}")).ok().map(Arc::new) } else { None };
    if let Some(s) = &store {
        s.set_engine(if search.meaning { crate::meaning::Engine::from_config(&search) } else { None });
    }
    let cfg = search.clone();
    // Registered to start with the session: it stays when the apps have gone.
    let linger = if std::env::args().any(|a| a == crate::service::STAY) { Duration::MAX } else { LINGER };
    serve_in(&dir, linger, move || Service::start(&search), store.map(|s| (s, cfg)))
}

/// `serve` with its folder, its patience and its index given, for tests.
pub fn serve_in(dir: &Path, linger: Duration, index: impl FnOnce() -> Arc<Service>, texts: Option<(Arc<Store>, SearchConfig)>) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    // One helper at a time: the lock is released when this process ends, however it ends.
    let lock = std::fs::File::create(dir.join("index.lock"))?;
    // A helper that was asked to go may take a moment to let go.
    let wait = Instant::now();
    while lock.try_lock().is_err() {
        if wait.elapsed() > Duration::from_secs(3) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let (token, addr) = (token(), dir.join("index.addr"));
    write_private(&addr, &format!("{}\n{token}\n", listener.local_addr()?.port()))?;

    let index = index();
    let stop = Arc::new(AtomicBool::new(false));
    let store = texts.map(|(store, cfg)| {
        let (s, stop) = (store.clone(), stop.clone());
        let changes = index.changes();
        std::thread::spawn(move || store::keep_current(&s, &cfg, &stop, Some(changes)));
        store
    });
    let (clients, quit, last) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(Instant::now())));
    {
        let (clients, quit, last) = (clients.clone(), quit.clone(), last.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (index, store, token, clients, quit, last) = (index.clone(), store.clone(), token.clone(), clients.clone(), quit.clone(), last.clone());
                std::thread::spawn(move || {
                    clients.fetch_add(1, Ordering::SeqCst);
                    let _ = answer(stream, &index, store.as_deref(), &token, &quit);
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
    Ok(())
}

/// The files with the words, then the files about the same thing that the words missed.
fn with_meaning(store: &Store, query: &str, max: usize) -> Results {
    let mut found = store.search(query, max);
    let start = std::time::Instant::now();
    let similar: Vec<_> = store.similar(query, max).into_iter().filter(|h| !found.hits.iter().any(|w| w.path == h.path)).collect();
    found.total += similar.len();
    found.hits.extend(similar.into_iter().take(max.saturating_sub(found.hits.len())));
    found.micros += start.elapsed().as_micros() as u64;
    found
}

fn answer(stream: TcpStream, index: &Service, store: Option<&Store>, token: &str, quit: &AtomicBool) -> io::Result<()> {
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
            Request::Hello { token: theirs, version } => {
                if !is_token(&theirs, token) {
                    return Ok(());
                }
                said_hello = true;
                // An app keeps its line open for as long as it runs.
                out.set_read_timeout(None)?;
                // A helper on its way out sends newcomers to the next one.
                let same = version == VERSION && !quit.load(Ordering::SeqCst);
                // Another version of the app: it starts its own helper once this one is gone.
                quit.fetch_or(!same, Ordering::SeqCst);
                Reply::Hello { same }
            }
            _ if !said_hello => return Ok(()),
            Request::Search { query, max, text: true, .. } => Reply::Results(store.map(|s| with_meaning(s, &query, max)).unwrap_or_default()),
            Request::Search { query, scope, max, .. } => Reply::Results(index.search(&query, scope.as_deref(), max)),
            Request::Passages { query, max } => Reply::Passages { found: store.map(|s| s.passages(&query, max)).unwrap_or_default() },
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
                meaning_engine: store.and_then(Store::engine_id).unwrap_or_default(),
                meaning_error: store.and_then(|s| s.meaning_error.lock().unwrap().clone()),
                error: store.and_then(|s| s.error.lock().unwrap().clone()),
                paused: store.is_some_and(|s| s.paused.load(Ordering::Relaxed)),
                roots: store.map(Store::root_sizes).unwrap_or_default(),
                tools: crate::extract::installed::found().iter().map(|(n, p)| (n.to_string(), p.is_some())).collect(),
                clouds: store.map(Store::clouds).unwrap_or_default(),
                }
            }),
        };
        let mut text = serde_json::to_string(&reply).map_err(io::Error::other)?;
        text.push('\n');
        out.write_all(text.as_bytes())?;
    }
}

// ---------------------------------------------------------------- the apps' side

type Line = (BufReader<TcpStream>, TcpStream);

/// The index as an app sees it: the helper's, or its own when no helper can be had.
pub struct Client {
    dir: Option<PathBuf>,
    /// Starts a helper. The apps start themselves with `ARG`.
    spawn: Box<dyn Fn() + Send + Sync>,
    search: SearchConfig,
    line: Mutex<Option<Line>>,
    /// The last status and when it was asked: the terminal app asks with every frame.
    status: Mutex<Option<(Instant, Status)>>,
    own: OnceLock<Arc<Service>>,
}

impl Client {
    /// Connects in the background, so the app starts without waiting for the helper.
    pub fn start(search: &SearchConfig) -> Arc<Client> {
        let client = Client::with(folder(), search, || {
            // A registration that starts another program (an older version, or one an upgrade
            // has removed) is taken over by this app.
            let registered = crate::service::installed();
            let Ok(exe) = crate::tools::this_app() else { return };
            if registered && crate::service::registered().as_deref() != Some(exe.as_path()) && crate::service::install(&exe).is_ok() {
                return;
            }
            // A registered helper is started again by systemd or launchd; on Windows nothing
            // does, so the app starts one that stays.
            if registered && !cfg!(windows) {
                return;
            }
            let mut c = detached(&exe);
            if registered {
                c.arg(crate::service::STAY);
            }
            let _ = c.spawn();
        });
        let c = client.clone();
        std::thread::spawn(move || c.status());
        client
    }

    pub fn with(dir: Option<PathBuf>, search: &SearchConfig, spawn: impl Fn() + Send + Sync + 'static) -> Arc<Client> {
        Arc::new(Client { dir, spawn: Box::new(spawn), search: search.clone(), line: Mutex::default(), status: Mutex::default(), own: OnceLock::new() })
    }

    pub fn search(&self, query: &str, scope: Option<&Path>, max: usize) -> Results {
        match self.ask(&Request::Search { query: query.into(), scope: scope.map(Path::to_path_buf), max, text: false }) {
            Some(Reply::Results(r)) => r,
            _ => self.own().search(query, scope, max),
        }
    }

    /// Search in the files' text. Nothing without the helper: the store is its alone.
    pub fn search_text(&self, query: &str, max: usize) -> Results {
        match self.ask(&Request::Search { query: query.into(), scope: None, max, text: true }) {
            Some(Reply::Results(r)) => r,
            _ => Results::default(),
        }
    }

    /// Ask: the `max` passages closest to `question`, with their files. Nothing without the
    /// helper, or while search by meaning is off.
    pub fn passages(&self, question: &str, max: usize) -> Vec<(PathBuf, String)> {
        match self.ask(&Request::Passages { query: question.into(), max }) {
            Some(Reply::Passages { found }) => found,
            _ => vec![],
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
            _ => Status { state: self.own().state(), len: self.own().len(), texts: 0, pending: 0, bytes: 0, paused: false, roots: vec![], tools: vec![], meaning: false, meaning_pending: 0, meaning_done: 0, meaning_engine: String::new(), meaning_error: None, error: None, clouds: vec![] },
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
        let mut started = false;
        let wait = Instant::now();
        // A registered helper that was asked to go takes the system a moment to start again.
        let patience = Duration::from_secs(if crate::service::installed() { 15 } else { 5 });
        loop {
            match dial(dir) {
                Some((mut line, token)) => match exchange(&mut line, &Request::Hello { token, version: VERSION.into() }) {
                    Ok(Reply::Hello { same: true }) => return Some(line),
                    // Another version, or a stale address: it goes away, then ours comes.
                    _ => {}
                },
                None if !started => {
                    started = true;
                    (self.spawn)();
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

/// `exe` as a helper that outlives the app and its terminal.
pub(crate) fn detached(exe: &Path) -> std::process::Command {
    use std::process::Stdio;
    let mut c = crate::tools::command(exe);
    c.arg(ARG).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
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
        let (dir, search) = (d.join("cache"), config(d));
        move || {
            let (dir, search) = (dir.clone(), search.clone());
            std::thread::spawn(move || {
                let store = Arc::new(Store::open(&dir.join("search.db")).unwrap());
                serve_in(&dir, linger, { let s = search.clone(); move || Service::start(&s) }, Some((store, search))).unwrap()
            });
        }
    }

    fn ready(c: &Client) {
        let wait = Instant::now();
        while c.state() != State::Ready && wait.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(c.state(), State::Ready);
    }

    #[test]
    fn helper_is_started_shared_and_leaves() {
        let d = tree("shared");
        let starts = Arc::new(AtomicUsize::new(0));
        let start = {
            let (starts, helper) = (starts.clone(), helper(&d, Duration::from_millis(400)));
            move || {
                starts.fetch_add(1, Ordering::SeqCst);
                helper()
            }
        };
        let start = Arc::new(start);
        let start = move || start();
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
        while one.status().texts < 3 && wait.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(100));
        }
        let found = two.search_text("launch", 10);
        assert_eq!(found.hits.iter().map(|h| h.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(), ["main.rs"]);
        assert!(found.hits[0].snippet.as_deref().unwrap().contains("launch"));
        // And folder sizes from its store.
        assert_eq!(two.size(&d.join("files/src")).map(|s| s.0), Some(crate::fs::dir_size(&d.join("files/src"))));

        // A restart brings a new helper.
        one.restart();
        ready(&one);
        assert_eq!(starts.load(Ordering::SeqCst), 2, "a new helper came");

        // Both apps go: the helper waits its while, then takes its address with it.
        drop((one, two));
        let wait = Instant::now();
        while d.join("cache/index.addr").exists() && wait.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!d.join("cache/index.addr").exists());
        // The helper may still hold the store: on Windows an open file cannot be removed.
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn helper_wants_the_token_and_the_app_manages_without_one() {
        let d = tree("token");
        helper(&d, Duration::from_secs(5))();
        let wait = Instant::now();
        while dial(&d.join("cache")).is_none() && wait.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let (mut line, token) = dial(&d.join("cache")).unwrap();
        assert!(exchange(&mut line, &Request::Status).is_err(), "no hello, no answer");
        let (mut line, _) = dial(&d.join("cache")).unwrap();
        assert!(exchange(&mut line, &Request::Hello { token: "guess".into(), version: VERSION.into() }).is_err());
        let (mut line, _) = dial(&d.join("cache")).unwrap();
        assert!(matches!(exchange(&mut line, &Request::Hello { token: token.clone(), version: VERSION.into() }), Ok(Reply::Hello { same: true })));
        assert_eq!(token.len(), 32);
        assert!(token.bytes().all(|b| b.is_ascii_hexdigit()) && token != super::token());
        assert!(is_token(&token, &token) && !is_token(&token[1..], &token) && !is_token(&format!("{token}0"), &token));
        // A line without end that goes on and on is dropped before it fills the memory.
        let (mut long, _) = dial(&d.join("cache")).unwrap();
        let _ = long.1.write_all(&vec![b'a'; 2 << 20]);
        let _ = long.1.write_all(b"\n");
        assert!(exchange(&mut long, &Request::Hello { token, version: VERSION.into() }).is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(d.join("cache/index.addr")).unwrap().permissions().mode() & 0o777, 0o600);
        }

        // No helper to be had: the app indexes by itself.
        let alone = Client::with(None, &config(&d), || {});
        ready(&alone);
        assert_eq!(alone.search("README", None, 10).total, 1);
        assert!(!alone.shared());
        // The helper may still hold the store: on Windows an open file cannot be removed.
        let _ = std::fs::remove_dir_all(d);
    }
}

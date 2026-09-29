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

use std::hash::{BuildHasher, Hasher};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::{Config, SearchConfig};
use crate::index::{Results, Service, State};

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
    Search { query: String, scope: Option<PathBuf>, max: usize },
    Status,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "re", rename_all = "snake_case")]
enum Reply {
    /// `same`: the helper is our version. Otherwise it exits, and we start ours.
    Hello { same: bool },
    Results(Results),
    Status { state: State, len: usize },
}

/// Where the helper's address and lock live: the cache folder, which is the user's own.
pub fn folder() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("coxswain"))
}

fn token() -> String {
    // std's hash keys come from the system's random source: enough for a token that only has
    // to be unguessable to other users of this machine.
    (0..2).map(|_| format!("{:016x}", std::hash::RandomState::new().build_hasher().finish())).collect()
}

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
    let dir = folder().ok_or_else(|| io::Error::other("no cache folder"))?;
    let search = Config::load().map(|c| c.search).unwrap_or_default();
    serve_in(&dir, LINGER, move || Service::start(&search))
}

/// `serve` with its folder, its patience and its index given, for tests.
pub fn serve_in(dir: &Path, linger: Duration, index: impl FnOnce() -> Arc<Service>) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    // One helper at a time: the lock is released when this process ends, however it ends.
    let lock = std::fs::File::create(dir.join("index.lock"))?;
    if lock.try_lock().is_err() {
        return Ok(());
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let (token, addr) = (token(), dir.join("index.addr"));
    write_private(&addr, &format!("{}\n{token}\n", listener.local_addr()?.port()))?;

    let index = index();
    let (clients, quit, last) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(Instant::now())));
    {
        let (clients, quit, last) = (clients.clone(), quit.clone(), last.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (index, token, clients, quit, last) = (index.clone(), token.clone(), clients.clone(), quit.clone(), last.clone());
                std::thread::spawn(move || {
                    clients.fetch_add(1, Ordering::SeqCst);
                    let _ = answer(stream, &index, &token, &quit);
                    clients.fetch_sub(1, Ordering::SeqCst);
                    *last.lock().unwrap() = Instant::now();
                });
            }
        });
    }
    while !quit.load(Ordering::SeqCst) && (clients.load(Ordering::SeqCst) > 0 || last.lock().unwrap().elapsed() < linger) {
        std::thread::sleep(linger.min(Duration::from_millis(200)));
    }
    let _ = std::fs::remove_file(addr);
    Ok(())
}

fn answer(stream: TcpStream, index: &Service, token: &str, quit: &AtomicBool) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let mut out = stream.try_clone()?;
    let mut said_hello = false;
    for line in BufReader::new(stream).lines() {
        let reply = match serde_json::from_str(&line?).map_err(io::Error::other)? {
            Request::Hello { token: theirs, version } => {
                if theirs != token {
                    return Ok(());
                }
                said_hello = true;
                let same = version == VERSION;
                // Another version of the app: it starts its own helper once this one is gone.
                quit.fetch_or(!same, Ordering::SeqCst);
                Reply::Hello { same }
            }
            _ if !said_hello => return Ok(()),
            Request::Search { query, scope, max } => Reply::Results(index.search(&query, scope.as_deref(), max)),
            Request::Status => Reply::Status { state: index.state(), len: index.len() },
        };
        let mut text = serde_json::to_string(&reply).map_err(io::Error::other)?;
        text.push('\n');
        out.write_all(text.as_bytes())?;
    }
    Ok(())
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
    status: Mutex<Option<(Instant, State, usize)>>,
    own: OnceLock<Arc<Service>>,
}

impl Client {
    /// Connects in the background, so the app starts without waiting for the helper.
    pub fn start(search: &SearchConfig) -> Arc<Client> {
        let client = Client::with(folder(), search, || {
            if let Ok(exe) = std::env::current_exe() {
                let _ = detached(&exe).spawn();
            }
        });
        let c = client.clone();
        std::thread::spawn(move || c.status());
        client
    }

    pub fn with(dir: Option<PathBuf>, search: &SearchConfig, spawn: impl Fn() + Send + Sync + 'static) -> Arc<Client> {
        Arc::new(Client { dir, spawn: Box::new(spawn), search: search.clone(), line: Mutex::default(), status: Mutex::default(), own: OnceLock::new() })
    }

    pub fn search(&self, query: &str, scope: Option<&Path>, max: usize) -> Results {
        match self.ask(&Request::Search { query: query.into(), scope: scope.map(Path::to_path_buf), max }) {
            Some(Reply::Results(r)) => r,
            _ => self.own().search(query, scope, max),
        }
    }

    pub fn state(&self) -> State {
        self.status().0
    }

    pub fn len(&self) -> usize {
        self.status().1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the index is the helper's (and not our own, the fallback).
    pub fn shared(&self) -> bool {
        self.own.get().is_none()
    }

    fn status(&self) -> (State, usize) {
        let mut status = self.status.lock().unwrap();
        if let Some((at, state, len)) = *status {
            if at.elapsed() < Duration::from_millis(500) {
                return (state, len);
            }
        }
        let (state, len) = match self.ask(&Request::Status) {
            Some(Reply::Status { state, len }) => (state, len),
            _ => (self.own().state(), self.own().len()),
        };
        *status = Some((Instant::now(), state, len));
        (state, len)
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
            if wait.elapsed() > Duration::from_secs(5) {
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
fn detached(exe: &Path) -> std::process::Command {
    use std::process::{Command, Stdio};
    let mut c = Command::new(exe);
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
        for f in ["files/src/main.rs", "files/src/lib.rs", "files/README.md"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        d
    }

    fn config(d: &Path) -> SearchConfig {
        SearchConfig { roots: vec![d.join("files")], watch: false, ..SearchConfig::default() }
    }

    /// A helper in a thread of this test, as an app would start one.
    fn helper(d: &Path, linger: Duration) -> impl Fn() + Send + Sync + Clone + 'static {
        let (dir, search) = (d.join("cache"), config(d));
        move || {
            let (dir, search) = (dir.clone(), search.clone());
            std::thread::spawn(move || serve_in(&dir, linger, move || Service::start(&search)).unwrap());
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

        // Both apps go: the helper waits its while, then takes its address with it.
        drop((one, two));
        let wait = Instant::now();
        while d.join("cache/index.addr").exists() && wait.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!d.join("cache/index.addr").exists());
        std::fs::remove_dir_all(d).unwrap();
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
        assert!(matches!(exchange(&mut line, &Request::Hello { token, version: VERSION.into() }), Ok(Reply::Hello { same: true })));

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
        std::fs::remove_dir_all(d).unwrap();
    }
}

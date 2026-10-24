//! Apache Tika in a container, for the text of formats no reader here knows (OneNote, XPS, CHM,
//! Outlook data files …), and of older Office files when LibreOffice is not installed. Off
//! unless `[search] tika` is on; the image is `[preview.images] tika`.
//!
//! Tika is a server, so it runs as one: started on demand, in a container with no network
//! (`--network=none`: only its own loopback), no capabilities and no mounts. A file reaches it
//! as the body of a request written to `podman exec -i`, where bash passes it on to the server
//! on the container's own 127.0.0.1; the answer comes back the same way. No port is opened on
//! this machine, no file name is ever an argument, and no folder of the user's is mounted.
//! Every request notes the time in the container, and the container stops itself after
//! `IDLE` seconds without one, also when the helper that started it is gone.
//!
//! What Tika says is someone else's file's text: kept to `MAX_TEXT`, tidied like every
//! reader's, and passed to Ask only through the store's passages, as any other text is.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::Config;

/// Formats only Tika reads here: OneNote, XPS and OpenXPS, compiled HTML help, Outlook data
/// files, MHTML web archives, Hangul word processor files, Access databases.
pub const FORMATS: &[&str] = &[
    "one", "xps", "oxps", "chm", "pst", "ost", "mht", "mhtml", "hwp", "mdb", "accdb",
];
/// Of the formats LibreOffice reads (`installed::OFFICE`), what Tika reads when LibreOffice is
/// not installed: Word and PowerPoint 97–2003, Publisher, Visio, WordPerfect, OpenDocument
/// drawings and StarOffice files.
pub const OFFICE: &[&str] = &[
    "doc", "dot", "ppt", "pps", "pot", "pub", "vsd", "vsdx", "wpd", "odg", "sxw", "sxc", "sxi",
    "sxd",
];
/// The version of what this reader gets of a file: one more reads its files again, once.
pub const READER: &str = "1";
/// Seconds without a file before the server stops itself.
const IDLE: u64 = 300;
/// How long one file may take.
const TIMEOUT: Duration = Duration::from_secs(120);
/// How long the server may take to start: Java and Tika's parsers load.
const START: Duration = Duration::from_secs(if cfg!(test) { 2 } else { 90 });
/// After a server that would not start, how long no file is given to Tika before it is tried
/// again (the helper starts afresh, and so tries at once, when a search option changes).
const BACK_OFF: Duration = Duration::from_secs(600);
/// Files one server reads before a fresh one takes over: a parser a hostile file got hold of
/// does not stay to see many more. With its root read-only and /tmp in memory, nothing of one
/// server is left for the next.
const FRESH_AFTER: u32 = 100;
/// A file the server answered it could not parse counts as this many files: a folder of broken
/// files gets a fresh server every tenth of them, not one for each.
const REFUSED_WEIGHS: u32 = 10;
/// Most starts within a minute: a server that crashes on file after file (or is held up by
/// each) is not started again for every one; the files of that minute are left unread.
const MOST_STARTS: usize = 3;

/// What Settings and the status lines say of Tika.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// The image is downloading: the runtime's last progress line.
    Pulling(String),
    /// The image is there: files are read.
    Ready,
    /// Neither podman nor docker is installed (or `[preview] container` is off).
    NoRuntime,
    /// The image could not be pulled, or the server would not start: why.
    Failed(String),
}

/// Tika as the settings set it.
struct Tika {
    rt: PathBuf,
    image: String,
    /// The container's name: one per user (rootful docker shares the names).
    name: String,
}

/// Tika when `[search] tika` is on; `Err` with the reason when it is on but cannot run. Read
/// from config.toml once: the helper starts again when a search option changes.
fn tika() -> Option<&'static Result<Tika, State>> {
    static T: OnceLock<Option<Result<Tika, State>>> = OnceLock::new();
    T.get_or_init(|| configured(&Config::load().ok()?)).as_ref()
}

fn configured(cfg: &Config) -> Option<Result<Tika, State>> {
    if !cfg.search.tika || !cfg.search.text {
        return None;
    }
    let image = cfg.preview.images.get("tika").cloned().unwrap_or_default();
    Some(match crate::tools::container_runtime(&cfg.preview) {
        Some((_, rt)) if !image.is_empty() => Ok(Tika {
            rt,
            image,
            name: name(),
        }),
        _ => Err(State::NoRuntime),
    })
}

/// `coxswain-tika-<a hash of the cache folder>`: the same for every helper of this user.
fn name() -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    crate::dirs::cache_dir().hash(&mut h);
    format!("coxswain-tika-{:08x}", h.finish() as u32)
}

/// The image is pulled: set once it is known, so the status costs no runtime call.
static PULLED: AtomicBool = AtomicBool::new(false);
static FAILED: Mutex<Option<String>> = Mutex::new(None);
/// When the server last failed to start: no start is tried for `BACK_OFF` after it.
static FAILED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// The server would not start: why, for `state`, and no new start for `BACK_OFF`.
fn failed(why: String) -> String {
    crate::helper::note(format_args!("Apache Tika: {why}"));
    *FAILED.lock().unwrap() = Some(why.clone());
    *FAILED_AT.lock().unwrap() = Some(Instant::now());
    why
}

/// Within `BACK_OFF` of a start that failed.
fn backing_off() -> bool {
    FAILED_AT.lock().unwrap().is_some_and(|at| at.elapsed() < BACK_OFF)
}

/// How Tika stands, for the status lines; None when it is off.
pub fn state() -> Option<State> {
    let t = match tika()? {
        Ok(t) => t,
        Err(s) => return Some(s.clone()),
    };
    if let Some(line) = crate::images::pulling().get(&t.image) {
        return Some(State::Pulling(line.clone()));
    }
    if let Some(why) = FAILED.lock().unwrap().clone() {
        return Some(State::Failed(why));
    }
    Some(if PULLED.load(Ordering::Relaxed) {
        State::Ready
    } else {
        State::Pulling(String::new())
    })
}

/// When the helper starts: with Tika on and its image not pulled yet, pull it in the
/// background (turning the option on, under its size and cost, is the click that asks for
/// it), then `pulled` to read the files it can now read. Nothing when Tika is off.
pub fn prepare(pulled: impl FnOnce() + Send + 'static) {
    let Some(Ok(t)) = tika() else { return };
    if crate::tools::image_size(&t.rt, &t.image).is_some() {
        PULLED.store(true, Ordering::Relaxed);
        return;
    }
    std::thread::spawn(move || match crate::images::pull(&t.rt, &t.image) {
        Ok(()) => {
            PULLED.store(true, Ordering::Relaxed);
            used(&t.image);
            pulled();
        }
        Err(e) => {
            crate::helper::note(format_args!("Apache Tika: {e}"));
            *FAILED.lock().unwrap() = Some(e);
        }
    });
}

/// The extensions Tika reads now: none until its image is there, and the older Office ones
/// only while LibreOffice is not installed (which reads them first).
pub fn extensions() -> Vec<&'static str> {
    if !matches!(tika(), Some(Ok(_))) || !PULLED.load(Ordering::Relaxed) {
        return vec![];
    }
    let mut v = FORMATS.to_vec();
    if crate::tools::which("soffice").is_none() {
        v.extend(OFFICE);
    }
    v
}

/// The text of the file at `path`, no larger than `max`, as Tika reads it; None when Tika is
/// off or not ready, or the file has none.
pub fn text(path: &Path, max: u64) -> Option<String> {
    let Some(Ok(t)) = tika() else { return None };
    if !PULLED.load(Ordering::Relaxed) {
        return None;
    }
    let bytes = read_most(path, max)?;
    let text = read(t, &bytes, path)?;
    used_daily(&t.image);
    Some(text)
}

/// The file, if it is no larger than `max`.
fn read_most(path: &Path, max: u64) -> Option<Vec<u8>> {
    let mut bytes = vec![];
    std::fs::File::open(path)
        .ok()?
        .take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= max).then_some(bytes)
}

/// When the server last answered, or started: while it is this fresh it is taken to run.
static LAST: Mutex<Option<Instant>> = Mutex::new(None);

/// `bytes` read by the server, started first when it does not run. A request that cannot
/// reach it (it stopped itself just now) starts it and is tried once more.
/// The server is stopped after `FRESH_AFTER` files (a file it could not parse counts as
/// `REFUSED_WEIGHS`), and at once after one that held it up, or a crash: the next file starts
/// a fresh one, at most `MOST_STARTS` a minute.
fn read(t: &Tika, bytes: &[u8], path: &Path) -> Option<String> {
    static SERVED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if backing_off() {
        return None;
    }
    for _ in 0..2 {
        if !running(t) {
            SERVED.store(0, Ordering::Relaxed);
            start(t).ok()?;
        }
        let asked = ask(t, &request(bytes, path));
        let served = SERVED.fetch_add(1, Ordering::Relaxed) + 1;
        match asked {
            Asked::Text(text) => {
                *LAST.lock().unwrap() = Some(Instant::now());
                if served >= FRESH_AFTER {
                    kill(t);
                }
                return text;
            }
            Asked::Unreached => kill(t),
            // It answered: it runs as it should, and the file is broken.
            Asked::Refused => {
                *LAST.lock().unwrap() = Some(Instant::now());
                if SERVED.fetch_add(REFUSED_WEIGHS - 1, Ordering::Relaxed) + REFUSED_WEIGHS - 1 >= FRESH_AFTER {
                    kill(t);
                }
                return None;
            }
            Asked::TooLong => {
                kill(t);
                return None;
            }
        }
    }
    None
}

/// Whether the server runs: when it answered lately, without asking the runtime.
fn running(t: &Tika) -> bool {
    let fresh = LAST
        .lock()
        .unwrap()
        .is_some_and(|l| l.elapsed() < Duration::from_secs(IDLE - 60));
    fresh
        || crate::tools::command(&t.rt)
            .args(["container", "inspect", "--format", "{{.State.Running}}", &t.name])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
}

/// The server in the background, and its watch: it stops itself after `IDLE` seconds without
/// a request (each touches /tmp/beat), or when Java ends. At the lowest priority, Java given
/// a gigabyte at most, listening only on the container's own loopback.
const SERVE: &str = r#"touch /tmp/beat
nice -n 19 java -Xmx1g -cp "/opt/tika-server/*:/opt/tika-server/lib/*:/tika-extras/*:/*" org.apache.tika.server.core.TikaServerCli -h 127.0.0.1 -p 9998 &
while sleep 15; do
  kill -0 $! 2>/dev/null || exit 0
  [ $(( $(date +%s) - $(stat -c %Y /tmp/beat) )) -lt $0 ] || { kill $!; exit 0; }
done"#;

/// A request, from standard input to the server, its answer to standard output. Passed as a
/// script, never with a file name in it.
const ASK: &str = "touch /tmp/beat; exec 3<>/dev/tcp/127.0.0.1/9998 || exit 3; cat <&3 & cat >&3; wait";

/// The arguments of `run` for the server: no network, no capabilities, no new privileges,
/// nothing mounted, its root read-only with /tmp in memory, never a pull (the image removed in
/// Settings is not fetched again behind the user's back), and with `limits` its memory and
/// processes bounded.
fn run_args(t: &Tika, limits: bool) -> Vec<String> {
    let mut v: Vec<String> = [
        "run", "-d", "--rm", "--pull=never", "--network=none",
    ]
    .map(String::from)
    .to_vec();
    // The bounds every sealed container has (`images::TIGHT`), the memory and process ones only
    // where the runtime can keep them.
    v.extend(crate::images::bounded(limits).into_iter().map(String::from));
    v.extend(["--name".into(), t.name.clone()]);
    v.extend(["--entrypoint".into(), "sh".into(), t.image.clone(), "-c".into(), SERVE.into(), IDLE.to_string()]);
    v
}

/// Start the server and wait until it answers. One start at a time: the others wait for it.
fn start(t: &Tika) -> Result<(), String> {
    static STARTING: Mutex<()> = Mutex::new(());
    let _one = STARTING.lock().unwrap();
    if LAST.lock().unwrap().is_some() && running(t) {
        return Ok(());
    }
    // Every file would wait for a start that fails again: none is tried for a while.
    if backing_off() {
        return Err(FAILED.lock().unwrap().clone().unwrap_or_default());
    }
    static STARTS: Mutex<Vec<Instant>> = Mutex::new(Vec::new());
    if !may_start(&mut STARTS.lock().unwrap(), Instant::now()) {
        return Err(format!("started {MOST_STARTS} times within a minute"));
    }
    // One left from a helper that ended, stopping or broken: gone first.
    let _ = crate::tools::command(&t.rt)
        .args(["rm", "-f", &t.name])
        .stdin(Stdio::null())
        .output();
    // A rootless podman with no memory or process controller delegated cannot bound them: the
    // server then runs without those two, Java's own -Xmx still holding. Asked once, apart.
    let out = crate::tools::command(&t.rt)
        .args(run_args(t, crate::images::bounds_work(&t.rt, &t.image)))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(failed(format!("would not start: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    let began = Instant::now();
    while began.elapsed() < START {
        if let Asked::Text(_) = ask(t, b"GET /tika HTTP/1.0\r\n\r\n") {
            *LAST.lock().unwrap() = Some(Instant::now());
            *FAILED.lock().unwrap() = None;
            *FAILED_AT.lock().unwrap() = None;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    kill(t);
    Err(failed(format!("did not start within {} seconds", START.as_secs())))
}

/// Whether a start may be made now, by the starts made (kept to the last minute's): it is
/// counted when it may.
fn may_start(starts: &mut Vec<Instant>, now: Instant) -> bool {
    starts.retain(|at| now.duration_since(*at) < Duration::from_secs(60));
    let may = starts.len() < MOST_STARTS;
    if may {
        starts.push(now);
    }
    may
}

fn kill(t: &Tika) {
    *LAST.lock().unwrap() = None;
    let _ = crate::tools::command(&t.rt)
        .args(["rm", "-f", &t.name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// The HTTP request for `bytes`: the text of the file, as plain text. Its name is not sent,
/// only its extension, which helps Tika tell the format, and only when it is letters and
/// digits.
fn request(bytes: &[u8], path: &Path) -> Vec<u8> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| e.len() <= 8 && e.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map(|e| format!("Content-Disposition: attachment; filename=\"file.{e}\"\r\n"))
        .unwrap_or_default();
    let mut r = format!(
        "PUT /tika HTTP/1.0\r\nAccept: text/plain\r\nContent-Length: {}\r\n{ext}\r\n",
        bytes.len()
    )
    .into_bytes();
    r.extend_from_slice(bytes);
    r
}

enum Asked {
    /// The server answered: the text, when the answer was a 200 with some.
    Text(Option<String>),
    /// The server answered with an error: it could not parse the file.
    Refused,
    /// No server to answer.
    Unreached,
    TooLong,
}

/// Send `request` to the server in the container, and read its answer, no more than
/// `MAX_TEXT` of it.
fn ask(t: &Tika, request: &[u8]) -> Asked {
    let mut c: Command = crate::tools::command(&t.rt);
    crate::tools::own_group(&mut c).args(["exec", "-i", &t.name, "bash", "-c", ASK])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let Ok(mut child) = c.spawn() else { return Asked::Unreached };
    let mut stdin = child.stdin.take().unwrap();
    let body = request.to_vec();
    // Written while the answer is read: neither pipe fills and holds the other up.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&body);
    });
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut v = vec![];
        let _ = (&mut stdout)
            .take(super::MAX_TEXT as u64 + 64 * 1024)
            .read_to_end(&mut v);
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        v
    });
    let status = crate::tools::wait(&mut child, TIMEOUT);
    let _ = writer.join();
    let answer = reader.join().unwrap_or_default();
    match status {
        Err(_) => Asked::TooLong,
        Ok(_) => match parse(&answer) {
            Some((true, text)) => Asked::Text((!text.trim().is_empty()).then_some(text)),
            Some((false, _)) => Asked::Refused,
            None => Asked::Unreached,
        },
    }
}

/// An HTTP answer: whether it is a 200, and its body; None when there is none.
fn parse(answer: &[u8]) -> Option<(bool, String)> {
    let head_end = answer.windows(4).position(|w| w == b"\r\n\r\n")?;
    let status = std::str::from_utf8(&answer[..head_end]).ok()?.lines().next()?;
    if !status.starts_with("HTTP/") {
        return None;
    }
    let ok = status.split_whitespace().nth(1) == Some("200");
    let body = &answer[head_end + 4..];
    let text = String::from_utf8_lossy(&body[..body.len().min(super::MAX_TEXT)]).into_owned();
    Some((ok, text))
}

/// The image was used: what *Remove images not used for 90 days* goes by.
fn used(image: &str) {
    let mut st = crate::state::AppState::load();
    crate::images::used(&mut st, image, crate::fs::now());
    let _ = st.save();
}

/// `used`, once a day at most.
fn used_daily(image: &str) {
    static NOTED: AtomicU64 = AtomicU64::new(0);
    let now = crate::fs::now();
    if now.saturating_sub(NOTED.load(Ordering::Relaxed)) >= 24 * 3600 {
        NOTED.store(now, Ordering::Relaxed);
        used(image);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tika_is_off_unless_turned_on_and_needs_a_runtime() {
        let mut cfg = Config::default();
        assert!(configured(&cfg).is_none(), "off by default");
        cfg.search.tika = true;
        cfg.preview.container = "off".into();
        assert!(matches!(configured(&cfg), Some(Err(State::NoRuntime))));
        cfg.search.text = false;
        assert!(configured(&cfg).is_none(), "nothing is read without text search");
    }

    #[test]
    fn tika_never_names_the_file_and_runs_sealed() {
        let r = request(b"abc", Path::new("/home/x/-rf $(id).one"));
        let r = String::from_utf8(r).unwrap();
        assert!(r.starts_with("PUT /tika HTTP/1.0\r\n"), "{r}");
        assert!(r.contains("Content-Length: 3\r\n") && r.contains("filename=\"file.one\"") && r.ends_with("\r\n\r\nabc"), "{r}");
        assert!(!r.contains("rf") && !r.contains("id"), "{r}");
        let odd = String::from_utf8(request(b"", Path::new("a.b\r\nX-Evil: 1"))).unwrap();
        assert!(!odd.contains("Evil") && !odd.contains("filename"), "{odd}");
        let t = Tika { rt: "podman".into(), image: "docker.io/apache/tika:4.1.0@sha256:06bcdbd09aca073293e5a171ad161418cc93c91d58d9e4e205064eade84889d4".into(), name: "n".into() };
        let args = run_args(&t, true);
        for want in ["--network=none", "--cap-drop=ALL", "no-new-privileges", "--rm", "--pull=never", "--read-only", "/tmp:rw,size=1g", "--memory=3g", "--pids-limit=256"] {
            assert!(args.iter().any(|a| a == want), "{want}: {args:?}");
        }
        assert!(!args.iter().any(|a| a.contains("label=disable")), "SELinux stays on: {args:?}");
        assert!(!run_args(&t, false).iter().any(|a| a.starts_with("--memory") || a.starts_with("--pids")));
        assert!(!args.iter().any(|a| a.starts_with("-v") || a.starts_with("--mount") || a.starts_with("-p")), "{args:?}");
        assert!(SERVE.contains("-h 127.0.0.1"));
    }

    #[test]
    fn tika_answers_are_read_within_bounds() {
        assert_eq!(parse(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nRocket fuel\n"), Some((true, "Rocket fuel\n".into())));
        assert_eq!(parse(b"HTTP/1.1 422 Unprocessable\r\n\r\nerror"), Some((false, "error".into())));
        assert_eq!(parse(b""), None, "no server");
        assert_eq!(parse(b"bash: connect: refused\r\n\r\n"), None);
        let mut big = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
        big.extend(std::iter::repeat_n(b'a', super::super::MAX_TEXT + 10));
        assert_eq!(parse(&big).unwrap().1.len(), super::super::MAX_TEXT);
    }

    /// A server that does not answer within `START` is a failure `state` names, and no file
    /// waits for another start for `BACK_OFF`; a runtime that will not run one, the same.
    #[test]
    fn tika_starts_at_most_three_times_a_minute() {
        let (mut starts, now) = (vec![], Instant::now());
        for _ in 0..MOST_STARTS {
            assert!(may_start(&mut starts, now));
        }
        assert!(!may_start(&mut starts, now + Duration::from_secs(30)), "a fourth within the minute");
        assert!(may_start(&mut starts, now + Duration::from_secs(61)), "the minute gone by");
    }

    #[test]
    #[cfg(unix)]
    fn tika_that_will_not_start_fails_once_and_backs_off() {
        let rt = |p: &str| crate::tools::which(p).unwrap();
        // `true` "runs" every command and answers nothing: a server that never comes up.
        let silent = Tika { rt: rt("true"), image: "img".into(), name: "n".into() };
        let began = Instant::now();
        let e = start(&silent).unwrap_err();
        assert!(e.contains("did not start within") && began.elapsed() >= START, "{e}");
        assert_eq!(*FAILED.lock().unwrap(), Some(e.clone()));
        assert!(backing_off());
        let began = Instant::now();
        assert_eq!(read(&silent, b"x", Path::new("a.one")), None);
        assert_eq!(start(&silent).unwrap_err(), e);
        assert!(began.elapsed() < Duration::from_secs(1), "no new start: {:?}", began.elapsed());
        // Past the back-off, a runtime that refuses to run it: the reason it gave.
        *FAILED_AT.lock().unwrap() = Some(Instant::now() - BACK_OFF);
        let refusing = Tika { rt: rt("false"), ..silent };
        assert!(start(&refusing).unwrap_err().starts_with("would not start"));
        assert!(backing_off());
        *FAILED.lock().unwrap() = None;
        *FAILED_AT.lock().unwrap() = None;
    }

    /// Needs podman and the image pulled (`podman pull` the image of `[preview.images] tika`):
    /// `cargo test -p coxswain-core -- --ignored real_tika`. A Word 97 file LibreOffice made.
    #[test]
    #[ignore]
    fn real_tika_reads_a_word_97_file() {
        let rt = crate::tools::which("podman").unwrap();
        let t = Tika { rt, image: "docker.io/apache/tika:4.1.0@sha256:06bcdbd09aca073293e5a171ad161418cc93c91d58d9e4e205064eade84889d4".into(), name: format!("coxswain-tika-test-{}", std::process::id()) };
        let doc = std::env::var("COXSWAIN_TEST_DOC").expect("COXSWAIN_TEST_DOC: a .doc file");
        let bytes = std::fs::read(&doc).unwrap();
        let text = read(&t, &bytes, Path::new(&doc)).unwrap_or_default();
        assert!(text.contains("rocket"), "{text:?}");
        let started = Instant::now();
        let again = read(&t, &bytes, Path::new(&doc)).unwrap_or_default();
        assert_eq!(text, again);
        assert!(started.elapsed() < Duration::from_secs(5), "a running server: {:?}", started.elapsed());
        kill(&t);
    }
}

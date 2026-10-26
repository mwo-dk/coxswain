//! Text files of any size, logs above all, as both apps' viewers show them: the terminal app's
//! built-in viewer (F3) and the desktop app's log view in the preview pane.
//!
//! A file is never read whole. A thread finds where its lines start (`Lines`, 8 bytes a line),
//! and only the lines on screen are read. Following a log (`Lines::grow`) goes on from where the
//! thread stopped, and a log that was rotated or cut short is read again from its start.
//!
//! A log's format is told from its first lines (`detect`): JSON Lines, logfmt, an access log
//! (Apache, nginx: common and combined), syslog (RFC 3164 and 5424) or W3C extended (IIS, with its
//! `#Fields:` line), else plain text. Each line is split into its fields only when it is shown
//! (`record`), and its level comes from its fields or from a word in it (ERROR, WARN, `[error]`).
//! The lines a stack trace adds under a line (indented, `at …`, `Caused by:`, a Python
//! `Traceback`) belong to it (`continues`). `Rows` picks the lines a filter keeps (a level and
//! up, a text, a field's value) and folds stack traces under their line, on a thread of its
//! own, as the lines are found: nothing waits for the whole file.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use regex::Regex;
use serde::Serialize;

/// Bytes of one line read at most (a minified file, one without a newline): the rest is `…`.
pub const LONGEST_READ: u64 = 1 << 20;
/// Lines found at most: their starts take 8 bytes each (160 MB for these).
pub const MOST_LINES: usize = 20_000_000;
/// Bytes read in one go when lines are read together; past it, one line at a time.
pub const READ_AT_ONCE: u64 = 4 << 20;
/// Lines read at most in one batch of a search or of `Rows`.
pub const BATCH_LINES: usize = 4096;
/// Lines looked at above the screen for the line a stack trace on it belongs to.
const LOOK_BACK: usize = 64;

// ---------------------------------------------------------------- reading files

/// Open `path` only when it is a regular file, asked of the opened handle: a pipe or a device
/// would block the read (or never end), and a file swapped for one between a look at the path
/// and the open is caught too. On Unix the open itself does not wait (`O_NONBLOCK`, which a FIFO
/// with no writer would); the flag is taken off again for the reads.
pub fn open_file(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other(crate::t!("viewer.not_a_file")));
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let fd = file.as_raw_fd();
        // SAFETY: fcntl on a descriptor this function owns, reading and setting its flags.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(file)
}

/// Read into `buf` from byte `at`, as far as the file goes: the bytes read. Each read says
/// where, so a viewer and its threads never move each other's place in the file.
pub fn read_at(f: &File, buf: &mut [u8], at: u64) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        #[cfg(unix)]
        let k = std::os::unix::fs::FileExt::read_at(f, &mut buf[n..], at + n as u64)?;
        #[cfg(windows)]
        let k = std::os::windows::fs::FileExt::seek_read(f, &mut buf[n..], at + n as u64)?;
        if k == 0 {
            break;
        }
        n += k;
    }
    Ok(n)
}

/// Where a file's lines start, found by a thread.
#[derive(Default)]
struct Index {
    /// Byte 0 and each byte after a newline: the last entry starts the line still being written
    /// (or nothing, at the end).
    starts: Mutex<Vec<u64>>,
    /// Bytes read so far; written under `starts`' lock.
    end: AtomicU64,
    /// The thread has read to the end, or to `MOST_LINES`.
    done: AtomicBool,
    /// It stopped at `MOST_LINES`.
    capped: AtomicBool,
    /// The file is closed: the thread stops.
    stop: AtomicBool,
}

impl Index {
    /// A thread finding the lines of `file` from byte `from` on, into a new index, or (`into`)
    /// on into one that has read to `from`.
    fn start(file: File, from: u64, into: Option<Arc<Index>>) -> Arc<Index> {
        let index = into.unwrap_or_else(|| Arc::new(Index { starts: Mutex::new(vec![0]), ..Index::default() }));
        index.done.store(false, Ordering::Release);
        let i = index.clone();
        std::thread::spawn(move || find_lines(&file, &i, from));
        index
    }

    /// Lines known so far, and whether that is all of them: a last line without its newline
    /// counts once the end is reached.
    fn count(&self) -> (usize, bool) {
        let done = self.done.load(Ordering::Acquire);
        let s = self.starts.lock().unwrap();
        let last = *s.last().unwrap_or(&0);
        let partial = done && self.end.load(Ordering::Acquire) > last;
        (s.len().saturating_sub(1) + usize::from(partial), done)
    }
}

/// Find where each line of the file starts, a megabyte at a time from byte `at`, into `index`;
/// `done` at the end, at `MOST_LINES`, or when the file cannot be read further; nothing more once
/// `stop`.
fn find_lines(f: &File, index: &Index, mut at: u64) {
    let mut buf = vec![0u8; 1 << 20];
    let mut found = vec![];
    while !index.stop.load(Ordering::Relaxed) {
        let n = match read_at(f, &mut buf, at) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        found.extend(memchr::memchr_iter(b'\n', &buf[..n]).map(|i| at + i as u64 + 1));
        at += n as u64;
        let mut s = index.starts.lock().unwrap();
        s.append(&mut found);
        index.end.store(at, Ordering::Release);
        if s.len() > MOST_LINES {
            s.truncate(MOST_LINES + 1);
            index.end.store(s[MOST_LINES], Ordering::Release);
            index.capped.store(true, Ordering::Relaxed);
            break;
        }
    }
    index.done.store(true, Ordering::Release);
}

/// A file's lines, read as they are asked for; cheap to clone for another thread.
pub struct Reader {
    file: File,
    index: Arc<Index>,
    /// UTF-8 (stray bytes shown as �), else Windows-1252: the file's start says it for all.
    pub utf8: bool,
}

impl Reader {
    /// Another handle on the same file and its lines, for a thread.
    pub fn try_clone(&self) -> std::io::Result<Reader> {
        Ok(Reader { file: self.file.try_clone()?, index: self.index.clone(), utf8: self.utf8 })
    }

    /// Lines known so far, and whether that is all of them.
    pub fn count(&self) -> (usize, bool) {
        self.index.count()
    }

    /// The thread stopped at `MOST_LINES`.
    pub fn capped(&self) -> bool {
        self.index.capped.load(Ordering::Relaxed)
    }

    /// Where lines `a..b` (as many of them as there are) start, and where the last one ends.
    fn cuts(&self, a: usize, b: usize) -> Vec<u64> {
        let (n, _) = self.count();
        let s = self.index.starts.lock().unwrap();
        let (a, b) = (a.min(n), b.min(n));
        if a >= b {
            return vec![];
        }
        let mut cuts = s[a..b.min(s.len())].to_vec();
        cuts.push(s.get(b).copied().unwrap_or_else(|| self.index.end.load(Ordering::Acquire)));
        cuts
    }

    /// Tests only: the lines as a moment after opening, only the first `n` found and the
    /// thread still at work; what was taken goes back with `give_back`.
    #[cfg(any(test, feature = "test-home"))]
    pub fn hold_back(&self, n: usize) -> Vec<u64> {
        let mut s = self.index.starts.lock().unwrap();
        self.index.done.store(false, Ordering::Release);
        s.split_off(n + 1)
    }

    #[cfg(any(test, feature = "test-home"))]
    pub fn give_back(&self, rest: Vec<u64>) {
        self.index.starts.lock().unwrap().extend(rest);
        self.index.done.store(true, Ordering::Release);
    }

    /// Bytes of line `i`, at most `LONGEST_READ`; 0 past the end.
    pub fn span(&self, i: usize) -> u64 {
        let c = self.cuts(i, i + 1);
        if c.len() < 2 { 0 } else { (c[1] - c[0]).min(LONGEST_READ) }
    }

    /// The far end of a batch of lines starting at `from` and going towards `limit`
    /// (`forward`, or backwards): at most `BATCH_LINES` lines, and at most `READ_AT_ONCE` bytes
    /// counting each line to `LONGEST_READ`, but always one line.
    pub fn batch(&self, from: usize, forward: bool, limit: usize) -> usize {
        let (lo, hi) = if forward { (from, limit.min(from + BATCH_LINES)) } else { (limit.max(from.saturating_sub(BATCH_LINES)), from) };
        let cuts = self.cuts(lo, hi);
        let len = |i: usize| match (cuts.get(i - lo), cuts.get(i - lo + 1)) {
            (Some(a), Some(b)) => (b - a).min(LONGEST_READ),
            _ => 0,
        };
        let (mut at, mut bytes) = (from, 0u64);
        loop {
            let next = if forward { at + 1 } else { at.wrapping_sub(1) };
            let line = if forward { at } else { next };
            if (forward && at >= limit) || (!forward && at <= limit) || from.abs_diff(at) >= BATCH_LINES {
                return at;
            }
            bytes += len(line);
            if bytes > READ_AT_ONCE && at != from {
                return at;
            }
            at = next;
        }
    }

    /// Lines `a..b` (as many of them as there are), each at most `most` bytes, a longer one cut
    /// with `…`; without their line ends.
    pub fn read(&self, a: usize, b: usize, most: u64) -> Vec<String> {
        let cuts = self.cuts(a, b);
        let (Some(&from), Some(&to)) = (cuts.first(), cuts.last()) else { return vec![] };
        // Short lines in one read; a span of long ones a line at a time, each cut.
        let whole = (to - from <= READ_AT_ONCE).then(|| {
            let mut buf = vec![0u8; (to - from) as usize];
            let n = read_at(&self.file, &mut buf, from).unwrap_or(0);
            buf.truncate(n);
            buf
        });
        cuts.windows(2)
            .map(|w| {
                let (len, cut) = ((w[1] - w[0]).min(most), w[1] - w[0] > most);
                let mut own = vec![];
                let line = match &whole {
                    Some(buf) => {
                        let s = ((w[0] - from) as usize).min(buf.len());
                        &buf[s..(s + len as usize).min(buf.len())]
                    }
                    None => {
                        own.resize(len as usize, 0);
                        let n = read_at(&self.file, &mut own, w[0]).unwrap_or(0);
                        own.truncate(n);
                        &own[..]
                    }
                };
                let line = line.strip_suffix(b"\n").unwrap_or(line);
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                let mut text = crate::view::decode_line(line, self.utf8);
                if cut {
                    text.push('…');
                }
                text
            })
            .collect()
    }
}

/// What `Lines::grow` found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grew {
    /// Nothing new (or the lines are still being found).
    No,
    /// Lines were written to the end: they are being found after the ones known.
    More,
    /// The file was rotated (another file has its name now) or cut short: it is read again
    /// from its start, and what was known of it is gone.
    Anew,
}

/// A text file's lines, found by a thread as the file is opened.
pub struct Lines {
    path: PathBuf,
    reader: Reader,
}

impl Lines {
    /// The lines of `file`, opened from `path` (`open_file`), read as UTF-8 or (not `utf8`)
    /// Windows-1252.
    pub fn new(path: &Path, file: File, utf8: bool) -> std::io::Result<Lines> {
        let index = Index::start(file.try_clone()?, 0, None);
        Ok(Lines { path: path.to_path_buf(), reader: Reader { file, index, utf8 } })
    }

    /// Open `path` and find its lines; its first 8 KB say how it is read. `None` for a file that
    /// is no text (`view::decode`) or UTF-16, which is read whole.
    pub fn open(path: &Path) -> std::io::Result<Option<Lines>> {
        let file = open_file(path)?;
        let mut head = vec![0u8; 8192];
        let n = read_at(&file, &mut head, 0)?;
        head.truncate(n);
        if head.starts_with(&[0xff, 0xfe]) || head.starts_with(&[0xfe, 0xff]) || head.contains(&0) {
            return Ok(None);
        }
        let utf8 = crate::view::is_utf8(&head);
        Lines::new(path, file, utf8).map(Some)
    }

    pub fn reader(&self) -> &Reader {
        &self.reader
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Look for lines written since: on from the end, or from the start again when the file
    /// was rotated or cut short. Nothing while the lines are still being found.
    pub fn grow(&mut self) -> Grew {
        let index = &self.reader.index;
        if !index.done.load(Ordering::Acquire) || self.reader.capped() {
            return Grew::No;
        }
        let end = index.end.load(Ordering::Acquire);
        if replaced(&self.path, &self.reader.file) {
            return match open_file(&self.path) {
                Ok(f) => self.anew(Some(f)),
                Err(_) => Grew::No,
            };
        }
        match self.reader.file.metadata().map(|m| m.len()) {
            Ok(len) if len < end => self.anew(None),
            Ok(len) if len > end => match self.reader.file.try_clone() {
                Ok(f) => {
                    Index::start(f, end, Some(index.clone()));
                    Grew::More
                }
                Err(_) => Grew::No,
            },
            _ => Grew::No,
        }
    }

    /// Find the lines again from the start, of `file` when it is another one.
    fn anew(&mut self, file: Option<File>) -> Grew {
        self.reader.index.stop.store(true, Ordering::Relaxed);
        if let Some(f) = file {
            self.reader.file = f;
        }
        match self.reader.file.try_clone() {
            Ok(f) => {
                self.reader.index = Index::start(f, 0, None);
                Grew::Anew
            }
            Err(_) => Grew::No,
        }
    }
}

impl Drop for Lines {
    fn drop(&mut self) {
        self.reader.index.stop.store(true, Ordering::Relaxed);
    }
}

/// Another file has `path`'s name now than the one open (a log rotated by renaming it): on
/// Unix by its inode, elsewhere by its creation time, or by being shorter than the one open
/// (the open one's length is asked first, so one written to meanwhile is never taken for
/// another).
fn replaced(path: &Path, open: &File) -> bool {
    let Ok(was) = open.metadata() else { return false };
    let Ok(now) = std::fs::metadata(path) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        now.dev() != was.dev() || now.ino() != was.ino()
    }
    #[cfg(not(unix))]
    {
        now.len() < was.len() || matches!((now.created(), was.created()), (Ok(a), Ok(b)) if a != b)
    }
}

// ---------------------------------------------------------------- levels

/// How much a log line matters, least first, so a filter keeps a level and the ones above it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Debug, Level::Info, Level::Warn, Level::Error];

    /// The theme slot a line at this level is coloured with, in both apps.
    pub fn slot(self) -> &'static str {
        match self {
            Level::Error => "git_deleted",
            Level::Warn => "git_modified",
            Level::Info => "git_added",
            Level::Debug => "hidden",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }

    pub fn from_name(s: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.name() == s)
    }

    /// The level a filter keeps after `self` (`None`: all of them): all → warnings and errors
    /// → errors → all, as one key steps through them.
    pub fn next_filter(now: Option<Level>) -> Option<Level> {
        match now {
            None => Some(Level::Warn),
            Some(Level::Warn) => Some(Level::Error),
            _ => None,
        }
    }

    /// A level's word as logs write it in a field (`level=warn`, `"severity":"ERROR"`), case
    /// aside; or bunyan's and pino's numbers (10 trace … 60 fatal).
    pub fn of_value(v: &str) -> Option<Level> {
        let v = v.trim();
        if let Ok(n) = v.parse::<u32>() {
            return match n {
                50.. => Some(Level::Error),
                40..=49 => Some(Level::Warn),
                30..=39 => Some(Level::Info),
                10..=29 => Some(Level::Debug),
                _ => None,
            };
        }
        match v.to_ascii_lowercase().as_str() {
            "error" | "err" | "e" | "fatal" | "crit" | "critical" | "panic" | "alert" | "emerg" | "emergency" | "severe" | "dpanic" => Some(Level::Error),
            "warn" | "warning" | "w" => Some(Level::Warn),
            "info" | "information" | "informational" | "notice" | "i" => Some(Level::Info),
            "debug" | "trace" | "verbose" | "fine" | "finer" | "finest" | "d" | "v" => Some(Level::Debug),
            _ => None,
        }
    }

    /// The level a plain line says by a word in capitals (`ERROR`, `WARN`), or in brackets as
    /// nginx and Apache write it (`[error]`): the highest it names.
    pub fn of_words(line: &str) -> Option<Level> {
        static WORDS: OnceLock<[(Regex, Level); 4]> = OnceLock::new();
        let words = WORDS.get_or_init(|| {
            let r = |s: &str| Regex::new(s).unwrap();
            [
                (r(r"\b(FATAL|CRIT(ICAL)?|ERROR|ERR|PANIC|SEVERE)\b|\[(error|crit|alert|emerg|fatal)\]"), Level::Error),
                (r(r"\b(WARN(ING)?)\b|\[warn(ing)?\]"), Level::Warn),
                (r(r"\b(INFO|NOTICE)\b|\[(info|notice)\]"), Level::Info),
                (r(r"\b(DEBUG|TRACE|VERBOSE)\b|\[(debug|trace)\]"), Level::Debug),
            ]
        });
        words.iter().find(|(re, _)| re.is_match(line)).map(|(_, l)| *l)
    }
}

// ---------------------------------------------------------------- formats

/// How a log's lines are written.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(tag = "kind", content = "fields", rename_all = "snake_case")]
pub enum Format {
    Plain,
    /// One JSON object a line (`.jsonl`, `.ndjson`, and many a `.log`).
    JsonLines,
    /// `key=value` pairs, as Go's and Rust's loggers write them.
    Logfmt,
    /// Apache's and nginx's common or combined log format.
    Access,
    /// RFC 3164 (BSD) or RFC 5424 syslog, and rsyslog's lines with an ISO time.
    Syslog,
    /// W3C extended (IIS): its fields named by its `#Fields:` line.
    W3c(Vec<String>),
}

impl Format {
    /// Its name in the texts (`logs.format.<id>`).
    pub fn id(&self) -> &'static str {
        match self {
            Format::Plain => "plain",
            Format::JsonLines => "json_lines",
            Format::Logfmt => "logfmt",
            Format::Access => "access",
            Format::Syslog => "syslog",
            Format::W3c(_) => "w3c",
        }
    }

    /// Its lines have fields, which can be shown as columns and filtered by.
    pub fn has_fields(&self) -> bool {
        *self != Format::Plain
    }

    /// Its lines are records of their own: none is the next line of a stack trace.
    fn one_line_each(&self) -> bool {
        matches!(self, Format::Access | Format::W3c(_))
    }
}

/// Lines `detect` looks at.
pub const SAMPLE: usize = 50;

fn access_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"^(\S+) (\S+) (\S+) \[([^\]]+)\] "((?:[^"\\]|\\.)*)" (\d{3}|-) (\S+)(?: "((?:[^"\\]|\\.)*)" "((?:[^"\\]|\\.)*)")?"#).unwrap())
}

/// RFC 5424 (`<34>1 2026-10-11T22:14:15.003Z host app 812 ID47 [sd] message`), RFC 3164
/// (`<34>Oct 11 22:14:15 host app[812]: message`, its priority optional, as /var/log has it)
/// and rsyslog's ISO time in place of 3164's.
fn syslog_res() -> &'static [Regex; 2] {
    static RE: OnceLock<[Regex; 2]> = OnceLock::new();
    RE.get_or_init(|| {
        [
            Regex::new(r"^<(\d{1,3})>1 (\S+) (\S+) (\S+) (\S+) (\S+) (-|(?:\[(?:[^\]\\]|\\.)*\])+) ?(.*)$").unwrap(),
            Regex::new(r"^(?:<(\d{1,3})>)?([A-Z][a-z]{2} [ \d]\d \d\d:\d\d:\d\d|\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?(?:Z|[+-]\d\d:?\d\d)?) (\S+) ([^\s:\[]+)(?:\[(\d+)\])?: ?(.*)$").unwrap(),
        ]
    })
}

/// The format of a log, from its first lines (up to `SAMPLE`, empty ones aside): the one most
/// of them are written in, else plain text.
pub fn detect(lines: &[String]) -> Format {
    let lines: Vec<&str> = lines.iter().map(|l| l.trim_end()).filter(|l| !l.is_empty()).take(SAMPLE).collect();
    if let Some(fields) = lines.iter().find_map(|l| l.strip_prefix("#Fields:")) {
        return Format::W3c(fields.split_whitespace().map(str::to_owned).collect());
    }
    if lines.is_empty() {
        return Format::Plain;
    }
    let share = |f: &dyn Fn(&str) -> bool| lines.iter().filter(|l| f(l)).count() * 100 / lines.len();
    if share(&|l| l.starts_with('{') && matches!(serde_json::from_str::<serde_json::Value>(l), Ok(serde_json::Value::Object(_)))) >= 60 {
        return Format::JsonLines;
    }
    if share(&|l| access_re().is_match(l)) >= 80 {
        return Format::Access;
    }
    // Syslog's lines carry stack traces and messages of more lines: a share of them is enough.
    if share(&|l| syslog_res().iter().any(|r| r.is_match(l))) >= 50 {
        return Format::Syslog;
    }
    if share(&|l| logfmt(l).len() >= 3 && !l.starts_with(char::is_whitespace)) >= 60 {
        return Format::Logfmt;
    }
    Format::Plain
}

/// Whether a file's name says it is a log, so a viewer looks at its lines as one: `.log`, a
/// rotated one (`app.log.1`, `syslog.2`), and /var/log's usual names without an extension.
pub fn is_log_name(name: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)(\.log|\.jsonl|\.ndjson)(\.\d+)?$|^(syslog|messages|kern\.log|auth\.log|daemon\.log|secure|maillog|access_log|error_log|dmesg)(\.\d+)?$").unwrap()
    });
    re.is_match(name)
}

/// A line's fields and level.
#[derive(Clone, Default, PartialEq, Debug, Serialize)]
pub struct Record {
    pub level: Option<Level>,
    /// In the line's order; JSON objects inside one flattened (`http.status`).
    pub fields: Vec<(String, String)>,
}

impl Record {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// Fields whose value is a line's level, the most usual first.
const LEVEL_KEYS: &[&str] = &["level", "lvl", "severity", "loglevel", "log.level", "levelname", "level_name", "@l", "status_text"];
/// Fields that hold when, and what was said: these come first among a log's columns.
const TIME_KEYS: &[&str] = &["time", "ts", "timestamp", "@timestamp", "t", "@t", "datetime", "date"];
const MESSAGE_KEYS: &[&str] = &["msg", "message", "@m", "@mt", "text", "event"];

/// A line's fields, in `format`, and its level: from its fields where they say it, else a word.
pub fn record(format: &Format, line: &str) -> Record {
    let fields = match format {
        Format::Plain => vec![],
        Format::JsonLines => json_fields(line),
        Format::Logfmt => logfmt(line),
        Format::Access => access_re().captures(line).map_or_else(Vec::new, |c| {
            ACCESS.iter().enumerate().filter_map(|(i, k)| c.get(i + 1).map(|m| (k.to_string(), m.as_str().to_owned()))).collect()
        }),
        Format::Syslog => syslog(line),
        Format::W3c(names) if !line.starts_with('#') => names.iter().cloned().zip(line.split_whitespace().map(str::to_owned)).collect(),
        Format::W3c(_) => vec![],
    };
    let status = |k: &str| fields.iter().find(|(f, _)| f == k).and_then(|(_, v)| v.parse::<u16>().ok());
    let level = match format {
        Format::Access | Format::W3c(_) => status(if *format == Format::Access { "status" } else { "sc-status" }).and_then(|s| match s {
            500.. => Some(Level::Error),
            400..=499 => Some(Level::Warn),
            _ => None,
        }),
        Format::Syslog => match fields.iter().find(|(k, _)| k == "priority").and_then(|(_, v)| v.parse::<u8>().ok()) {
            Some(p) => Some(match p & 7 {
                0..=3 => Level::Error,
                4 => Level::Warn,
                5 | 6 => Level::Info,
                _ => Level::Debug,
            }),
            None => Level::of_words(line),
        },
        _ => LEVEL_KEYS
            .iter()
            .find_map(|k| fields.iter().find(|(f, _)| f.eq_ignore_ascii_case(k)))
            .and_then(|(_, v)| Level::of_value(v))
            .or_else(|| Level::of_words(line)),
    };
    Record { level, fields }
}

/// The fields of an access log's line.
const ACCESS: [&str; 9] = ["client", "ident", "user", "time", "request", "status", "size", "referrer", "agent"];

fn json_fields(line: &str) -> Vec<(String, String)> {
    fn flat(prefix: &str, v: &serde_json::Value, out: &mut Vec<(String, String)>) {
        match v {
            serde_json::Value::Object(m) if out.len() < 200 => {
                for (k, v) in m {
                    let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                    flat(&key, v, out);
                }
            }
            serde_json::Value::String(s) => out.push((prefix.to_owned(), s.clone())),
            serde_json::Value::Null => out.push((prefix.to_owned(), String::new())),
            v => out.push((prefix.to_owned(), v.to_string())),
        }
    }
    let mut out = vec![];
    if let Ok(v @ serde_json::Value::Object(_)) = serde_json::from_str::<serde_json::Value>(line) {
        flat("", &v, &mut out);
    }
    out
}

/// `key=value` pairs, a value in double quotes with `\"` in it; words between them left out.
fn logfmt(line: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        let k0 = i;
        while i < b.len() && b[i] != b'=' && b[i] != b' ' {
            i += 1;
        }
        let key = &line[k0..i];
        if i >= b.len() || b[i] != b'=' {
            continue;
        }
        i += 1;
        let value = if b.get(i) == Some(&b'"') {
            i += 1;
            let mut v = String::new();
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' && i + 1 < b.len() {
                    i += 1;
                }
                let c = line[i..].chars().next().unwrap_or(' ');
                v.push(c);
                i += c.len_utf8();
            }
            i += 1;
            v
        } else {
            let v0 = i;
            while i < b.len() && b[i] != b' ' {
                i += 1;
            }
            line[v0..i].to_owned()
        };
        let good = !key.is_empty() && key.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-' | b'@'));
        if good {
            out.push((key.to_owned(), value));
        }
    }
    out
}

fn syslog(line: &str) -> Vec<(String, String)> {
    let [rfc5424, bsd] = syslog_res();
    let f = |pairs: &[(&str, Option<regex::Match>)]| {
        pairs.iter().filter_map(|(k, m)| m.map(|m| (k.to_string(), m.as_str().to_owned()))).filter(|(_, v)| v != "-").collect()
    };
    if let Some(c) = rfc5424.captures(line) {
        return f(&[("priority", c.get(1)), ("time", c.get(2)), ("host", c.get(3)), ("app", c.get(4)), ("pid", c.get(5)), ("msgid", c.get(6)), ("data", c.get(7)), ("message", c.get(8))]);
    }
    bsd.captures(line).map_or_else(Vec::new, |c| f(&[("priority", c.get(1)), ("time", c.get(2)), ("host", c.get(3)), ("app", c.get(4)), ("pid", c.get(5)), ("message", c.get(6))]))
}

/// The columns a log's lines are shown in: its format's fields, or (JSON Lines, logfmt) the
/// fields its first lines have, when, level and message first; at most 12.
pub fn columns(format: &Format, sample: &[String]) -> Vec<String> {
    let own = |names: &[&str]| names.iter().map(|s| s.to_string()).collect();
    match format {
        Format::Plain => vec![],
        Format::Access => own(&ACCESS),
        Format::Syslog => own(&["time", "host", "app", "pid", "message"]),
        Format::W3c(names) => names.clone(),
        Format::JsonLines | Format::Logfmt => {
            let mut seen: Vec<String> = vec![];
            for l in sample.iter().take(SAMPLE * 4) {
                for (k, _) in record(format, l).fields {
                    if !seen.contains(&k) {
                        seen.push(k);
                    }
                }
            }
            let rank = |k: &str| {
                let k = k.to_ascii_lowercase();
                if TIME_KEYS.contains(&k.as_str()) {
                    0
                } else if LEVEL_KEYS.contains(&k.as_str()) {
                    1
                } else if MESSAGE_KEYS.contains(&k.as_str()) {
                    2
                } else {
                    3
                }
            };
            // A stable sort keeps the rest in the order the lines have them.
            seen.sort_by_key(|k| rank(k));
            seen.truncate(12);
            seen
        }
    }
}

// ---------------------------------------------------------------- stack traces

/// Where a line stands in a stack trace, as the lines go by.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Trace {
    #[default]
    Out,
    /// In a Python traceback: its last line, not indented, ends it.
    Python,
    /// Just after a Python traceback, where `During handling of the above exception` may
    /// chain another.
    After,
}

/// Whether `line` goes on the entry above it (the next line of a stack trace, or of a message
/// of several lines) rather than start one; `trace` keeps where a traceback stands.
pub fn continues(format: &Format, line: &str, trace: &mut Trace) -> bool {
    if format.one_line_each() {
        return false;
    }
    let t = line.trim_start();
    let chained = t.starts_with("During handling of the above exception") || t.starts_with("The above exception was the direct cause");
    let traceback = t.starts_with("Traceback (most recent call last)");
    match *trace {
        Trace::Python if line.starts_with([' ', '\t']) || line.is_empty() || traceback || chained => return true,
        Trace::Python => {
            // Its last line: the exception and its message.
            *trace = Trace::After;
            return true;
        }
        Trace::After if line.trim().is_empty() || chained => return true,
        Trace::After if traceback => {
            *trace = Trace::Python;
            return true;
        }
        _ => *trace = Trace::Out,
    }
    if traceback {
        *trace = Trace::Python;
        return true;
    }
    if *format == Format::JsonLines {
        // A line that is no JSON object between them: what a program printed under one.
        return !t.starts_with('{') && !t.is_empty();
    }
    if line.is_empty() {
        return false;
    }
    if line.starts_with([' ', '\t']) {
        return true;
    }
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // Java's `Caused by:`, `... 12 more`, a frame without its indent, and the exception's own
        // line under the message (`java.lang.IllegalStateException: …`), Go's goroutine dump.
        Regex::new(r"^(Caused by: |Suppressed: |at [\w$.<>/]+\(|\.\.\. \d+ more$|([a-z][\w$]*\.)+[A-Z][\w$]*(Exception|Error|Throwable)(: |$)|goroutine \d+ \[)").unwrap()
    });
    re.is_match(line)
}

/// The level each of lines `a..b` is shown in: its entry's, so the stack trace under an error
/// is coloured as the error is. Reads up to `LOOK_BACK` lines above `a` for the entry's start.
pub fn levels(reader: &Reader, format: &Format, a: usize, b: usize) -> Vec<Option<Level>> {
    let from = a.saturating_sub(LOOK_BACK);
    let lines = reader.read(from, b, LONGEST_READ);
    let mut trace = Trace::Out;
    let mut level = None;
    let mut out = vec![];
    for (k, line) in lines.iter().enumerate() {
        if k == 0 || !continues(format, line, &mut trace) {
            level = record(format, line).level;
        }
        if from + k >= a {
            out.push(level);
        }
    }
    out
}

// ---------------------------------------------------------------- filtering and folding

/// What a log's view keeps and folds.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Shape {
    /// Only entries at this level and above.
    pub level: Option<Level>,
    /// Only entries with this text in them, case aside.
    pub text: String,
    /// Only entries whose field has this value, case aside.
    pub field: Option<(String, String)>,
    /// A stack trace folded under the line it belongs to.
    pub fold: bool,
}

impl Shape {
    /// Every line, as it is: no `Rows` needed.
    pub fn is_plain(&self) -> bool {
        self.level.is_none() && self.text.is_empty() && self.field.is_none() && !self.fold
    }

    /// A filter as typed: `name=value` (a field's value, in a log with fields), else a text.
    pub fn typed(&mut self, format: &Format, s: &str) {
        self.text.clear();
        self.field = None;
        let s = s.trim();
        if format.has_fields()
            && let Some((k, v)) = s.split_once('=')
            && !k.is_empty()
            && k.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-' | b'@'))
        {
            self.field = Some((k.to_owned(), v.trim().to_owned()));
        } else {
            self.text = s.to_owned();
        }
    }

    /// The filter as it would be typed again; empty without one.
    pub fn typed_text(&self) -> String {
        match &self.field {
            Some((k, v)) => format!("{k}={v}"),
            None => self.text.clone(),
        }
    }

    /// Whether a line starting an entry passes the level and the field.
    fn passes(&self, rec: &Record) -> bool {
        self.level.is_none_or(|l| rec.level.is_some_and(|r| r >= l))
            && self.field.as_ref().is_none_or(|(k, v)| rec.get(k).is_some_and(|x| x.trim().eq_ignore_ascii_case(v)))
    }
}

/// A row of a log's view: a line, the level of its entry, and how many lines of a stack trace
/// are folded under it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Row {
    pub line: u32,
    /// Lines folded under it, in the low 29 bits; its level (0 none, 1 + `Level`) in the top 3.
    packed: u32,
}

impl Row {
    fn new(line: usize, level: Option<Level>) -> Row {
        Row { line: line as u32, packed: (level.map_or(0, |l| l as u32 + 1)) << 29 }
    }

    pub fn folded(self) -> u32 {
        self.packed & ((1 << 29) - 1)
    }

    pub fn level(self) -> Option<Level> {
        match self.packed >> 29 {
            0 => None,
            n => Level::ALL.get(n as usize - 1).copied(),
        }
    }

    fn fold_one(&mut self) {
        if self.folded() < (1 << 29) - 1 {
            self.packed += 1;
        }
    }
}

/// The rows a `Shape` leaves of a log, found by a thread as its lines are; it goes on with the
/// lines a followed log gets, and stops once this is dropped.
#[derive(Default)]
pub struct Rows {
    rows: Mutex<Vec<Row>>,
    /// Lines gone through so far.
    read: AtomicUsize,
    /// Every line found so far is gone through, and the file's lines are all found.
    caught_up: AtomicBool,
}

/// An entry being gone through: its first line and its lines under it.
struct Entry {
    level: Option<Level>,
    /// It passes the level and the field.
    passes: bool,
    /// The filter's text is in it, or there is none.
    has_text: bool,
    /// Its first row's place in the rows, once it is shown.
    shown: Option<usize>,
    /// Its lines not shown yet: the first, then the ones under it.
    lines: Vec<usize>,
}

impl Rows {
    /// A thread going through `reader`'s lines for the rows `shape` leaves.
    pub fn start(reader: Reader, format: Format, shape: Shape) -> Arc<Rows> {
        let rows = Arc::new(Rows::default());
        let r = Arc::downgrade(&rows);
        std::thread::spawn(move || build(&reader, &format, &shape, &r));
        rows
    }

    /// Rows found so far, and whether every line is gone through.
    pub fn count(&self) -> (usize, bool) {
        (self.rows.lock().unwrap().len(), self.caught_up.load(Ordering::Acquire))
    }

    /// Rows `a..b`, as many as there are.
    pub fn get(&self, a: usize, b: usize) -> Vec<Row> {
        let r = self.rows.lock().unwrap();
        r[a.min(r.len())..b.min(r.len())].to_vec()
    }

    /// Lines gone through so far.
    pub fn read(&self) -> usize {
        self.read.load(Ordering::Relaxed)
    }

    /// The row of line `line`, or of the last line above it that has one.
    pub fn row_of(&self, line: usize) -> usize {
        let r = self.rows.lock().unwrap();
        r.partition_point(|row| row.line as usize <= line).saturating_sub(1)
    }

    /// Show an entry once it passes all three: its lines not shown yet become rows, or folds
    /// under its first.
    fn show(&self, e: &mut Entry, fold: bool) {
        if e.shown.is_some() || !(e.passes && e.has_text) || e.lines.is_empty() {
            return;
        }
        let mut r = self.rows.lock().unwrap();
        e.shown = Some(r.len());
        let mut head = Row::new(e.lines[0], e.level);
        if fold {
            for _ in 1..e.lines.len() {
                head.fold_one();
            }
            r.push(head);
        } else {
            r.push(head);
            r.extend(e.lines[1..].iter().map(|&l| Row::new(l, e.level)));
        }
        e.lines.clear();
    }
}

/// `Rows`' thread: through the lines as they are found, until its `Rows` is dropped.
fn build(reader: &Reader, format: &Format, shape: &Shape, rows: &std::sync::Weak<Rows>) {
    let text = shape.text.to_lowercase();
    let has = |line: &str| text.is_empty() || line.to_lowercase().contains(&text);
    let mut next = 0;
    let mut trace = Trace::Out;
    let mut entry: Option<Entry> = None;
    while let Some(rows) = rows.upgrade() {
        let (n, done) = reader.count();
        if next >= n {
            if done {
                rows.caught_up.store(true, Ordering::Release);
            }
            drop(rows);
            std::thread::sleep(std::time::Duration::from_millis(if done { 50 } else { 5 }));
            continue;
        }
        rows.caught_up.store(false, Ordering::Release);
        let hi = reader.batch(next, true, n);
        let lines = reader.read(next, hi, LONGEST_READ);
        if lines.is_empty() {
            drop(rows);
            std::thread::sleep(std::time::Duration::from_millis(5));
            continue;
        }
        for (k, line) in lines.iter().enumerate() {
            let i = next + k;
            match entry.as_mut() {
                Some(e) if continues(format, line, &mut trace) => {
                    if !e.has_text && has(line) {
                        e.has_text = true;
                    }
                    match e.shown {
                        Some(at) if shape.fold => rows.rows.lock().unwrap()[at].fold_one(),
                        Some(_) => rows.rows.lock().unwrap().push(Row::new(i, e.level)),
                        None if e.passes => e.lines.push(i),
                        None => {}
                    }
                    rows.show(e, shape.fold);
                }
                _ => {
                    let rec = record(format, line);
                    let mut e = Entry { level: rec.level, passes: shape.passes(&rec), has_text: has(line), shown: None, lines: vec![i] };
                    rows.show(&mut e, shape.fold);
                    entry = Some(e);
                }
            }
        }
        next += lines.len();
        rows.read.store(next, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------- a file's view

/// A column of a log is at most this many characters wide; its text is cut with `…`.
pub const WIDEST_COLUMN: usize = 40;
/// A log up to this size opens with its stack traces folded: its rows are all found in a moment.
/// A bigger one opens unfolded, so the end is there at once (folding it is a key away).
pub const FOLD_AT_OPEN: u64 = 64 << 20;
/// New rows a followed log's pause on an error looks at, at most, each time.
const ERRORS_LOOKED_AT: usize = 10_000;

/// A log line in columns: each column's value, and the fields with no column.
pub type Cells = (Vec<String>, Vec<(String, String)>);

/// A text file as both apps' viewers show it: its lines, as a log when it is one (its format,
/// its columns, its levels), and the rows a `Shape` leaves of it.
pub struct View {
    lines: Lines,
    /// `Plain` for a text that is no log.
    pub format: Format,
    /// A log, by its name or the format its first lines are in: it gets the log keys.
    pub log: bool,
    /// A log's columns and their widths in characters (`WIDEST_COLUMN` at most).
    pub columns: Vec<(String, usize)>,
    shape: Shape,
    rows: Option<Arc<Rows>>,
    /// Bumped when the rows change: rows a screen holds from before are stale.
    era: u64,
}

impl View {
    /// The view of `lines`, a file called `name` of `size` bytes whose text starts with `head`.
    pub fn new(lines: Lines, name: &str, head: &str, size: u64) -> View {
        // The last line of the head may be cut off: it is left out.
        let mut sample: Vec<String> = head.lines().map(str::to_owned).collect();
        if sample.len() > 1 {
            sample.pop();
        }
        let format = detect(&sample);
        let log = format != Format::Plain || is_log_name(name);
        let width = |s: &str| s.chars().count();
        let columns = columns(&format, &sample)
            .into_iter()
            .map(|c| {
                let widest = sample.iter().map(|l| record(&format, l).get(&c).map_or(0, width)).max().unwrap_or(0);
                let w = widest.max(width(&c)).min(WIDEST_COLUMN);
                (c, w)
            })
            .collect();
        let mut v = View { lines, format, log, columns, shape: Shape { fold: log && size <= FOLD_AT_OPEN, ..Shape::default() }, rows: None, era: 0 };
        v.reshape();
        v
    }

    /// `path` opened and viewed; `None` for a file that is no text, or UTF-16.
    pub fn open(path: &Path) -> std::io::Result<Option<View>> {
        let file = open_file(path)?;
        let size = file.metadata()?.len();
        let mut head = vec![0u8; 8192];
        let n = read_at(&file, &mut head, 0)?;
        head.truncate(n);
        if head.starts_with(&[0xff, 0xfe]) || head.starts_with(&[0xfe, 0xff]) || head.contains(&0) {
            return Ok(None);
        }
        let utf8 = crate::view::is_utf8(&head);
        let text = crate::view::decode(&head).unwrap_or_default();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Some(View::new(Lines::new(path, file, utf8)?, &name, &text, size)))
    }

    pub fn reader(&self) -> &Reader {
        self.lines.reader()
    }

    pub fn shape(&self) -> &Shape {
        &self.shape
    }

    /// Bumped each time the rows change.
    pub fn era(&self) -> u64 {
        self.era
    }

    /// Keep and fold by `shape`: the rows are found again from the start, on a thread, unless
    /// every line is kept.
    pub fn set_shape(&mut self, shape: Shape) {
        self.shape = shape;
        self.reshape();
    }

    fn reshape(&mut self) {
        let format = if self.log { self.format.clone() } else { Format::Plain };
        self.rows = (!self.shape.is_plain()).then(|| self.reader().try_clone().ok()).flatten().map(|r| Rows::start(r, format, self.shape.clone()));
        self.era += 1;
    }

    /// Whether some lines are left out or folded, so rows are not lines.
    pub fn filtered(&self) -> bool {
        self.rows.is_some()
    }

    /// Rows known so far, and whether that is all of them.
    pub fn count(&self) -> (usize, bool) {
        let (known, done) = self.reader().count();
        match &self.rows {
            Some(rows) => {
                let (n, caught_up) = rows.count();
                (n, caught_up && done && rows.read() >= known)
            }
            None => (known, done),
        }
    }

    /// The lines of rows `a..b` (as many as there are), each one's level in a log, and the lines
    /// folded under each.
    pub fn rows(&self, a: usize, b: usize) -> Vec<(usize, Option<Level>, u32)> {
        match &self.rows {
            Some(rows) => rows.get(a, b).into_iter().map(|r| (r.line as usize, r.level(), r.folded())).collect(),
            None => {
                let b = b.min(self.reader().count().0);
                let levels = if self.log && a < b { levels(self.reader(), &self.format, a, b) } else { vec![] };
                (a..b).map(|i| (i, levels.get(i - a).copied().flatten(), 0)).collect()
            }
        }
    }

    /// The text of rows `a..b`, each at most `most` bytes, a longer one cut with `…`.
    pub fn texts(&self, a: usize, b: usize, most: u64) -> Vec<String> {
        if self.rows.is_none() {
            return self.reader().read(a, b, most);
        }
        let wanted: Vec<usize> = self.rows(a, b).into_iter().map(|(l, _, _)| l).collect();
        let mut out = Vec::with_capacity(wanted.len());
        let mut i = 0;
        // A run of following lines in one read.
        while i < wanted.len() {
            let mut j = i + 1;
            while j < wanted.len() && wanted[j] == wanted[j - 1] + 1 {
                j += 1;
            }
            let got = self.reader().read(wanted[i], wanted[j - 1] + 1, most);
            let short = (j - i).saturating_sub(got.len());
            out.extend(got);
            out.extend(std::iter::repeat_n(String::new(), short));
            i = j;
        }
        out
    }

    /// The far end of a batch of rows for a search, from `from` towards `limit` (`forward`, or
    /// backwards): at most a few thousand lines and a few megabytes, but always one row.
    pub fn batch(&self, from: usize, forward: bool, limit: usize) -> usize {
        const ROWS: usize = 512;
        match &self.rows {
            None => self.reader().batch(from, forward, limit),
            Some(_) if forward => (from + ROWS).min(limit).max(from.min(limit)),
            Some(_) => from.saturating_sub(ROWS).max(limit),
        }
    }

    /// The row of line `line` (from 0), or of the last line above it the rows keep; `None` while
    /// the rows have not reached it.
    pub fn row_of(&self, line: usize) -> Option<usize> {
        match &self.rows {
            None => Some(line.min(self.count().0.saturating_sub(1))),
            Some(rows) if rows.read() > line || self.count().1 => Some(rows.row_of(line)),
            Some(_) => None,
        }
    }

    /// Look for lines written since it was opened (`Lines::grow`); a rotated file's rows are
    /// found again from its start.
    pub fn grow(&mut self) -> Grew {
        let g = self.lines.grow();
        match g {
            Grew::Anew => self.reshape(),
            Grew::More => self.era += 1,
            Grew::No => {}
        }
        g
    }

    /// The first row of a line from `line` on (among the last `ERRORS_LOOKED_AT` rows) at the
    /// level of an error: where following stops when it pauses on errors. By line, not row, so
    /// rows found again (another filter) do not bring back the errors before it.
    pub fn first_error(&self, line: usize) -> Option<usize> {
        let (n, _) = self.count();
        let from = n.saturating_sub(ERRORS_LOOKED_AT);
        self.rows(from, n).into_iter().position(|(l, lv, _)| l >= line && lv == Some(Level::Error)).map(|k| from + k)
    }

    /// A log's line in its columns: each column's value, and the fields with no column;
    /// `None` for a line without fields (the next line of a stack trace).
    pub fn cells(&self, text: &str) -> Option<Cells> {
        if !self.log || !self.format.has_fields() {
            return None;
        }
        let rec = record(&self.format, text);
        if rec.fields.is_empty() {
            return None;
        }
        let cols = self.columns.iter().map(|(c, _)| rec.get(c).unwrap_or("").to_owned()).collect();
        let rest = rec.fields.iter().filter(|(k, _)| !self.columns.iter().any(|(c, _)| c.eq_ignore_ascii_case(k))).cloned().collect();
        Some((cols, rest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-logs-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn found(l: &Lines) {
        let limit = std::time::Instant::now() + crate::test_limit(std::time::Duration::from_secs(10));
        while !l.reader().count().1 {
            assert!(std::time::Instant::now() < limit, "lines never found");
            std::thread::yield_now();
        }
    }

    fn caught_up(r: &Rows) {
        let limit = std::time::Instant::now() + crate::test_limit(std::time::Duration::from_secs(10));
        while !r.count().1 {
            assert!(std::time::Instant::now() < limit, "rows never caught up");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn lines_are_found_read_and_followed_as_the_log_grows_or_is_rotated() {
        let d = dir("grow");
        let p = d.join("app.log");
        // A last line still being written counts once the end is reached.
        std::fs::write(&p, "one\ntwo\nthr").unwrap();
        let mut l = Lines::open(&p).unwrap().unwrap();
        found(&l);
        assert_eq!(l.reader().count(), (3, true));
        assert_eq!(l.reader().read(0, 9, LONGEST_READ), ["one", "two", "thr"]);
        assert_eq!(l.grow(), Grew::No);
        // Written to: the half line is finished and more follow, found after the ones known.
        std::fs::write(&p, "one\ntwo\nthree\nfour\r\n").unwrap();
        assert_eq!(l.grow(), Grew::More);
        found(&l);
        assert_eq!(l.reader().read(0, 9, LONGEST_READ), ["one", "two", "three", "four"]);
        // Cut short (copytruncate): from the start again.
        std::fs::write(&p, "new\n").unwrap();
        assert_eq!(l.grow(), Grew::Anew);
        found(&l);
        assert_eq!(l.reader().read(0, 9, LONGEST_READ), ["new"]);
        // Rotated: renamed away and a new file under its name, longer than the old one.
        std::fs::rename(&p, d.join("app.log.1")).unwrap();
        std::fs::write(&p, "fresh one\nfresh two\nfresh three\n").unwrap();
        assert_eq!(l.grow(), Grew::Anew);
        found(&l);
        assert_eq!(l.reader().count().0, 3);
        assert_eq!(l.reader().read(2, 3, LONGEST_READ), ["fresh three"]);
        // A line over the most read is cut with …; binary and UTF-16 are not read by lines.
        std::fs::write(d.join("min.js"), "x".repeat(3 << 20)).unwrap();
        let l = Lines::open(&d.join("min.js")).unwrap().unwrap();
        found(&l);
        let line = l.reader().read(0, 1, LONGEST_READ).remove(0);
        assert!(line.ends_with('…') && line.len() == (1 << 20) + '…'.len_utf8());
        std::fs::write(d.join("a.bin"), b"MZ\0\x01").unwrap();
        assert!(Lines::open(&d.join("a.bin")).unwrap().is_none());
        drop((l,));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn formats_are_told_from_the_first_lines() {
        let json = s(&[r#"{"time":"2026-10-26T10:00:00Z","level":"info","msg":"started"}"#, r#"{"time":"2026-10-26T10:00:01Z","level":"error","msg":"lost","err":{"code":5}}"#]);
        assert_eq!(detect(&json), Format::JsonLines);
        let lf = s(&[r#"time=2026-10-26T10:00:00Z level=info msg="engine started" stage=1"#, "time=2026-10-26T10:00:02Z level=warn msg=hot temp=91"]);
        assert_eq!(detect(&lf), Format::Logfmt);
        let access = s(&[
            r#"10.0.0.7 - - [26/Oct/2026:10:00:00 +0000] "GET /launch HTTP/1.1" 200 512 "-" "curl/8.0""#,
            r#"10.0.0.8 - ada [26/Oct/2026:10:00:01 +0000] "POST /abort HTTP/1.1" 503 0"#,
        ]);
        assert_eq!(detect(&access), Format::Access);
        let sys = s(&[
            "Oct 26 10:00:00 rocket sshd[812]: Accepted publickey for ada",
            "Oct 26 10:00:01 rocket kernel: usb 1-1: new device",
            "<34>1 2026-10-26T10:00:02.003Z rocket app 99 ID47 - fuel low",
        ]);
        assert_eq!(detect(&sys), Format::Syslog);
        let w3c = s(&["#Software: Microsoft Internet Information Services 10.0", "#Fields: date time cs-method cs-uri-stem sc-status", "2026-10-26 10:00:00 GET /index.htm 404"]);
        assert_eq!(detect(&w3c), Format::W3c(s(&["date", "time", "cs-method", "cs-uri-stem", "sc-status"])));
        assert_eq!(detect(&s(&["2026-10-26 10:00:00 ERROR engine stalled", "  at fuel.rs:12"])), Format::Plain);
        assert_eq!(detect(&[]), Format::Plain);
        for name in ["app.log", "app.log.3", "syslog", "syslog.1", "events.jsonl", "ACCESS_LOG"] {
            assert!(is_log_name(name), "{name}");
        }
        assert!(!is_log_name("catalog.txt") && !is_log_name("logbook.md"));
    }

    #[test]
    fn fields_and_levels_come_from_each_format() {
        let r = record(&Format::JsonLines, r#"{"ts":1,"level":"ERROR","msg":"lost","http":{"status":503,"path":"/x"},"tags":["a"]}"#);
        assert_eq!(r.level, Some(Level::Error));
        assert_eq!(r.get("http.status"), Some("503"));
        assert_eq!(r.get("tags"), Some(r#"["a"]"#));
        assert_eq!(record(&Format::JsonLines, r#"{"level":40,"msg":"x"}"#).level, Some(Level::Warn), "pino's numbers");
        let r = record(&Format::Logfmt, r#"lvl=warn msg="fuel \"low\" now" a=1 b= stray"#);
        assert_eq!(r.fields, vec![("lvl".into(), "warn".into()), ("msg".into(), r#"fuel "low" now"#.into()), ("a".into(), "1".into()), ("b".into(), "".into())]);
        assert_eq!(r.level, Some(Level::Warn));
        let r = record(&Format::Access, r#"10.0.0.8 - ada [26/Oct/2026:10:00:01 +0000] "POST /abort HTTP/1.1" 503 0 "-" "curl""#);
        assert_eq!((r.get("user"), r.get("request"), r.get("agent"), r.level), (Some("ada"), Some("POST /abort HTTP/1.1"), Some("curl"), Some(Level::Error)));
        assert_eq!(record(&Format::Access, r#"1.2.3.4 - - [x] "GET / HTTP/1.1" 404 9"#).level, Some(Level::Warn));
        let r = record(&Format::Syslog, "<11>1 2026-10-26T10:00:02Z rocket app 99 - [x@1 a=\"b\"] valve stuck");
        assert_eq!((r.get("app"), r.get("message"), r.get("data"), r.level), (Some("app"), Some("valve stuck"), Some("[x@1 a=\"b\"]"), Some(Level::Error)));
        let r = record(&Format::Syslog, "Oct 26 10:00:00 rocket sshd[812]: WARNING: low entropy");
        assert_eq!((r.get("pid"), r.get("message"), r.level), (Some("812"), Some("WARNING: low entropy"), Some(Level::Warn)));
        let w3c = Format::W3c(s(&["date", "time", "sc-status"]));
        assert_eq!(record(&w3c, "2026-10-26 10:00:00 500").level, Some(Level::Error));
        assert!(record(&w3c, "#Fields: date").fields.is_empty());
        assert_eq!(Level::of_words("2026/10/26 10:00:00 [error] 12#0: upstream timed out"), Some(Level::Error));
        assert_eq!(Level::of_words("INFO then ERROR later"), Some(Level::Error), "the highest it names");
        assert_eq!(Level::of_words("an error in lower case"), None, "only as logs write it");
        assert_eq!(columns(&Format::JsonLines, &s(&[r#"{"msg":"a","user":"ada","level":"info","time":"t"}"#])), ["time", "level", "msg", "user"]);
        assert_eq!(Level::next_filter(None), Some(Level::Warn));
        assert_eq!(Level::next_filter(Some(Level::Error)), None);
    }

    #[test]
    fn stack_traces_go_on_the_line_above() {
        let java = s(&[
            "2026-10-26 10:00:00 ERROR launch failed",
            "java.lang.IllegalStateException: no fuel",
            "\tat rocket.Engine.burn(Engine.java:12)",
            "Caused by: java.io.IOException: valve",
            "\t... 3 more",
            "2026-10-26 10:00:01 INFO retrying",
        ]);
        let python = s(&[
            "ERROR:root:burn failed",
            "Traceback (most recent call last):",
            "  File \"engine.py\", line 3, in <module>",
            "ValueError: no fuel",
            "",
            "During handling of the above exception, another exception occurred:",
            "",
            "Traceback (most recent call last):",
            "  File \"engine.py\", line 5, in <module>",
            "RuntimeError: abort",
            "INFO:root:next",
        ]);
        for (lines, want) in [(&java, vec![false, true, true, true, true, false]), (&python, vec![false, true, true, true, true, true, true, true, true, true, false])] {
            let mut t = Trace::Out;
            let got: Vec<bool> = lines.iter().enumerate().map(|(i, l)| i > 0 && continues(&Format::Plain, l, &mut t)).collect();
            assert_eq!(got, want, "{lines:?}");
        }
        let mut t = Trace::Out;
        assert!(!continues(&Format::Access, "  indented", &mut t), "access logs have no traces");
        assert!(continues(&Format::JsonLines, "panic: boom", &mut t));
        assert!(!continues(&Format::JsonLines, r#"{"a":1}"#, &mut t));
    }

    #[test]
    fn rows_keep_a_level_a_text_or_a_field_and_fold_traces_as_the_log_grows() {
        let d = dir("rows");
        let p = d.join("app.log");
        let log = "\
2026-10-26 10:00:00 INFO start
2026-10-26 10:00:01 ERROR launch failed
java.lang.IllegalStateException: no fuel
\tat rocket.Engine.burn(Engine.java:12)
2026-10-26 10:00:02 WARN hot
2026-10-26 10:00:03 DEBUG temp=91
";
        std::fs::write(&p, log).unwrap();
        let mut l = Lines::open(&p).unwrap().unwrap();
        found(&l);
        let rows = |shape: Shape| {
            let r = Rows::start(l.reader().try_clone().unwrap(), Format::Plain, shape);
            caught_up(&r);
            r
        };
        let lines = |r: &Rows| r.get(0, 99).iter().map(|r| (r.line, r.folded(), r.level())).collect::<Vec<_>>();
        // Warnings and up: the error with its trace under it, coloured as the error.
        let warn = rows(Shape { level: Some(Level::Warn), ..Shape::default() });
        assert_eq!(lines(&warn), [(1, 0, Some(Level::Error)), (2, 0, Some(Level::Error)), (3, 0, Some(Level::Error)), (4, 0, Some(Level::Warn))]);
        // Folded: the trace's two lines under the error.
        let folded = rows(Shape { fold: true, ..Shape::default() });
        assert_eq!(lines(&folded), [(0, 0, Some(Level::Info)), (1, 2, Some(Level::Error)), (4, 0, Some(Level::Warn)), (5, 0, Some(Level::Debug))]);
        assert_eq!(folded.row_of(3), 1, "a line folded away goes to its entry's row");
        // A text only in the trace keeps the whole entry.
        let text = rows(Shape { text: "ENGINE.java".into(), fold: true, ..Shape::default() });
        assert_eq!(lines(&text), [(1, 2, Some(Level::Error))]);
        // Following: lines written later are gone through as they are found.
        std::fs::write(&p, format!("{log}2026-10-26 10:00:04 ERROR abort\n\tat x\n")).unwrap();
        assert_eq!(l.grow(), Grew::More);
        let limit = std::time::Instant::now() + crate::test_limit(std::time::Duration::from_secs(10));
        while lines(&folded).len() < 5 || lines(&folded)[4].1 < 1 {
            assert!(std::time::Instant::now() < limit, "{:?}", lines(&folded));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(lines(&folded)[4], (6, 1, Some(Level::Error)));
        // A field's value, in a log with fields; typed as name=value.
        std::fs::write(&p, "level=info user=ada msg=a\nlevel=error user=bob msg=b\nlevel=info user=Ada msg=c\n").unwrap();
        let l = Lines::open(&p).unwrap().unwrap();
        found(&l);
        let mut shape = Shape::default();
        shape.typed(&Format::Logfmt, "user=ada");
        assert_eq!((shape.field.clone(), shape.typed_text()), (Some(("user".into(), "ada".into())), "user=ada".into()));
        let r = Rows::start(l.reader().try_clone().unwrap(), Format::Logfmt, shape);
        caught_up(&r);
        assert_eq!(r.get(0, 9).iter().map(|r| r.line).collect::<Vec<_>>(), [0, 2]);
        let mut plain = Shape::default();
        plain.typed(&Format::Plain, "user=ada");
        assert_eq!((plain.field, plain.text.as_str()), (None, "user=ada"), "a plain log has no fields");
        assert_eq!(levels(l.reader(), &Format::Logfmt, 1, 3), [Some(Level::Error), Some(Level::Info)]);
        drop((l, r));
        std::fs::remove_dir_all(&d).ok();
    }
}

//! Checksums (docs/files/checksums.md), in both apps: a SHA256SUMS, SHA512SUMS or B3SUMS for
//! the marked files and folders, in the standard `hash  relative/path` lines that `sha256sum -c`
//! and `b3sum -c` read; and a list of sums checked against the files, each OK, changed, missing
//! or unreadable. Files are read in pieces of a megabyte, so memory stays small whatever their
//! size, and a run says how far it is and stops when asked.

use serde::Serialize;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Algo {
    Sha256,
    Sha512,
    Blake3,
}

impl Algo {
    pub const ALL: [Algo; 3] = [Algo::Sha256, Algo::Sha512, Algo::Blake3];

    pub fn name(self) -> &'static str {
        match self {
            Algo::Sha256 => "SHA-256",
            Algo::Sha512 => "SHA-512",
            Algo::Blake3 => "BLAKE3",
        }
    }

    /// The list's usual file name.
    pub fn file_name(self) -> &'static str {
        match self {
            Algo::Sha256 => "SHA256SUMS",
            Algo::Sha512 => "SHA512SUMS",
            Algo::Blake3 => "B3SUMS",
        }
    }

    pub fn from_id(id: &str) -> Option<Algo> {
        Algo::ALL.into_iter().find(|a| a.id() == id)
    }

    /// Its name for the desktop app: `sha256`, `sha512`, `blake3`.
    pub fn id(self) -> &'static str {
        match self {
            Algo::Sha256 => "sha256",
            Algo::Sha512 => "sha512",
            Algo::Blake3 => "blake3",
        }
    }
}

enum Hasher {
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
    Blake3(Box<blake3::Hasher>),
}

impl Hasher {
    fn new(a: Algo) -> Hasher {
        use sha2::Digest;
        match a {
            Algo::Sha256 => Hasher::Sha256(sha2::Sha256::new()),
            Algo::Sha512 => Hasher::Sha512(sha2::Sha512::new()),
            Algo::Blake3 => Hasher::Blake3(Box::new(blake3::Hasher::new())),
        }
    }
    fn update(&mut self, b: &[u8]) {
        use sha2::Digest;
        match self {
            Hasher::Sha256(h) => h.update(b),
            Hasher::Sha512(h) => h.update(b),
            Hasher::Blake3(h) => drop(h.update(b)),
        }
    }
    fn hex(self) -> String {
        use sha2::Digest;
        let bytes: Vec<u8> = match self {
            Hasher::Sha256(h) => h.finalize().to_vec(),
            Hasher::Sha512(h) => h.finalize().to_vec(),
            Hasher::Blake3(h) => h.finalize().as_bytes().to_vec(),
        };
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// How far a run is: on file `at` (from 0) of `all`, `done` of `total` bytes read.
#[derive(Clone, Default, Debug, Serialize)]
pub struct Progress {
    pub at: usize,
    pub all: usize,
    pub name: String,
    pub done: u64,
    pub total: u64,
}

impl Progress {
    /// The status line: *Checksums: report.pdf (3 of 12), 41 %*.
    pub fn line(&self, what: &str) -> String {
        let pct = (self.done.saturating_mul(100)).checked_div(self.total).unwrap_or(0);
        if self.all > 1 {
            crate::t!("sums.busy_of", "what" => what, "name" => self.name, "n" => self.at + 1, "all" => self.all, "pct" => pct)
        } else {
            crate::t!("sums.busy", "what" => what, "name" => self.name, "pct" => pct)
        }
    }
}

fn stopped() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, crate::t!("sums.stopped"))
}

/// The hash of `r` in hex, read a megabyte at a time; `read(n)` hears each piece's size. Stops
/// with `Interrupted` when `stop` is set.
pub fn hash_reader(mut r: impl Read, algo: Algo, stop: &AtomicBool, mut read: impl FnMut(u64)) -> io::Result<String> {
    let mut h = Hasher::new(algo);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(stopped());
        }
        let n = match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        h.update(&buf[..n]);
        read(n as u64);
    }
    Ok(h.hex())
}

pub fn hash_file(path: &Path, algo: Algo, stop: &AtomicBool, read: impl FnMut(u64)) -> io::Result<String> {
    hash_reader(std::fs::File::open(path)?, algo, stop, read)
}

/// The files under `items` (files, and folders walked), with their names relative to `base` in
/// `/`-separated form, sorted. Links are left out, never followed.
pub fn files(base: &Path, items: &[PathBuf]) -> Vec<(PathBuf, String, u64)> {
    fn walk(p: &Path, base: &Path, out: &mut Vec<(PathBuf, String, u64)>) {
        let Ok(m) = std::fs::symlink_metadata(p) else { return };
        if m.is_dir() {
            if let Ok(rd) = std::fs::read_dir(p) {
                rd.flatten().for_each(|e| walk(&e.path(), base, out));
            }
        } else if m.is_file() {
            let rel = p.strip_prefix(base).unwrap_or(p);
            let name = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            out.push((p.to_path_buf(), name, m.len()));
        }
    }
    let mut out = vec![];
    items.iter().for_each(|p| walk(p, base, &mut out));
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// A name in a list, written as the GNU tools do: one with a backslash or a line break in it
/// starts its line with `\` and has them escaped.
fn line(hash: &str, name: &str) -> String {
    if name.contains(['\\', '\n', '\r']) {
        let n = name.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r");
        format!("\\{hash}  {n}\n")
    } else {
        format!("{hash}  {name}\n")
    }
}

/// What a finished make wrote.
#[derive(Clone, Debug, Serialize)]
pub struct Made {
    pub path: PathBuf,
    pub files: usize,
}

/// The list's file in `dir`: its usual name, or `SHA256SUMS-2` and on when that is taken.
pub fn target(dir: &Path, algo: Algo) -> PathBuf {
    let base = algo.file_name();
    (1..)
        .map(|i| dir.join(if i == 1 { base.to_string() } else { format!("{base}-{i}") }))
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .expect("a free name")
}

/// Write the sums of everything under `items` into a new list in `dir`, never over a file
/// that is there. A stopped run writes nothing.
pub fn make(dir: &Path, items: &[PathBuf], algo: Algo, stop: &AtomicBool, mut at: impl FnMut(&Progress)) -> io::Result<Made> {
    let list = files(dir, items);
    let mut p = Progress { all: list.len(), total: list.iter().map(|f| f.2).sum(), ..Default::default() };
    let mut out = String::new();
    for (i, (path, name, _)) in list.iter().enumerate() {
        (p.at, p.name) = (i, name.clone());
        at(&p);
        let h = hash_file(path, algo, stop, |n| {
            p.done += n;
            at(&p);
        })
        .map_err(|e| if e.kind() == io::ErrorKind::Interrupted { e } else { io::Error::new(e.kind(), format!("{name}: {e}")) })?;
        out.push_str(&line(&h, name));
    }
    let path = target(dir, algo);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
    f.write_all(out.as_bytes())?;
    f.sync_all()?;
    Ok(Made { path, files: list.len() })
}

/// The file names a list of sums goes by: `SHA256SUMS`, `B3SUMS-2`, `x.iso.sha256`.
pub fn is_list(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_ascii_uppercase();
    ["SHA256SUMS", "SHA512SUMS", "B3SUMS"].iter().any(|b| name.starts_with(b))
        || [".SHA256", ".SHA512", ".B3"].iter().any(|e| name.ends_with(e))
}

/// One line of a list: the hash, the name, and its algorithm when the line says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub hash: String,
    pub name: String,
    pub algo: Algo,
}

fn hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The lines of a list: GNU's `hash  name` and `hash *name` (escaped names after a `\`), BSD's
/// `SHA256 (name) = hash`, and a lone hash in `x.iso.sha256`, meant for `x.iso`. Lines that are
/// none of these are left out. `list` is the list's own path: its name tells the algorithm
/// where the hash's length does not (BLAKE3 and SHA-256 are both 64 digits).
pub fn parse(text: &str, list: &Path) -> Vec<Entry> {
    let lname = list.file_name().unwrap_or_default().to_string_lossy().to_string();
    let upper = lname.to_ascii_uppercase();
    let b3 = upper.starts_with("B3SUMS") || upper.ends_with(".B3");
    let guess = |h: &str| match h.len() {
        128 => Some(Algo::Sha512),
        64 if b3 => Some(Algo::Blake3),
        64 => Some(Algo::Sha256),
        _ => None,
    };
    let mut out = vec![];
    for raw in text.lines() {
        let l = raw.trim_end_matches('\r');
        // BSD: `SHA256 (name) = hash`.
        if let Some((tag, rest)) = l.split_once(" (")
            && let Some((name, hash)) = rest.rsplit_once(") = ")
        {
            let algo = match tag {
                "SHA256" => Some(Algo::Sha256),
                "SHA512" => Some(Algo::Sha512),
                "BLAKE3" => Some(Algo::Blake3),
                _ => None,
            };
            if let Some(algo) = algo.filter(|_| hex(hash)) {
                out.push(Entry { hash: hash.to_ascii_lowercase(), name: name.to_string(), algo });
            }
            continue;
        }
        let (escaped, l) = match l.strip_prefix('\\') {
            Some(r) => (true, r),
            None => (false, l),
        };
        let (hash, name) = match l.split_once(' ') {
            Some((h, n)) => (h, n.strip_prefix([' ', '*']).unwrap_or(n).to_string()),
            None if hex(l) => {
                // A lone hash: the list is named after its file.
                let Some(stem) = Path::new(&lname).file_stem() else { continue };
                (l, stem.to_string_lossy().into_owned())
            }
            None => continue,
        };
        let name = if escaped { name.replace("\\\\", "\0").replace("\\n", "\n").replace("\\r", "\r").replace('\0', "\\") } else { name };
        if let (true, Some(algo)) = (hex(hash) && !name.is_empty(), guess(hash)) {
            out.push(Entry { hash: hash.to_ascii_lowercase(), name, algo });
        }
    }
    out
}

/// How a file stands against its sum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "why", rename_all = "snake_case")]
pub enum State {
    Ok,
    Changed,
    Missing,
    /// Not read: why (a link, a name outside the list's folder, an error).
    Unreadable(String),
}

#[derive(Clone, Debug, Serialize)]
pub struct Checked {
    pub name: String,
    pub path: PathBuf,
    #[serde(flatten)]
    pub state: State,
}

/// Where a name of a list is: under the list's folder, or nowhere (an absolute name, or one
/// that climbs out with `..`).
fn inside(dir: &Path, name: &str) -> Option<PathBuf> {
    let rel = Path::new(name);
    rel.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)).then(|| dir.join(rel))
}

/// Check every file a list names against its sum. A stopped run gives what it checked so far,
/// with `true`.
pub fn verify(list: &Path, stop: &AtomicBool, mut at: impl FnMut(&Progress)) -> io::Result<(Vec<Checked>, bool)> {
    let text = std::fs::read(list)?;
    let entries = parse(&String::from_utf8_lossy(&text), list);
    let dir = list.parent().unwrap_or(Path::new("."));
    let places: Vec<Option<PathBuf>> = entries.iter().map(|e| inside(dir, &e.name)).collect();
    let size = |p: &Option<PathBuf>| p.as_ref().and_then(|p| std::fs::symlink_metadata(p).ok()).filter(|m| m.is_file()).map_or(0, |m| m.len());
    let mut p = Progress { all: entries.len(), total: places.iter().map(size).sum(), ..Default::default() };
    let mut out = vec![];
    for (i, (e, place)) in entries.iter().zip(places).enumerate() {
        (p.at, p.name) = (i, e.name.clone());
        at(&p);
        let Some(path) = place else {
            out.push(Checked { name: e.name.clone(), path: PathBuf::new(), state: State::Unreadable(crate::t!("sums.outside")) });
            continue;
        };
        let state = match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => State::Missing,
            Err(e) => State::Unreadable(e.to_string()),
            Ok(m) if m.file_type().is_symlink() => State::Unreadable(crate::t!("sums.link")),
            Ok(m) if !m.is_file() => State::Unreadable(crate::t!("sums.not_file")),
            Ok(_) => match hash_file(&path, e.algo, stop, |n| {
                p.done += n;
                at(&p);
            }) {
                Ok(h) if h == e.hash => State::Ok,
                Ok(_) => State::Changed,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => return Ok((out, true)),
                Err(err) => State::Unreadable(err.to_string()),
            },
        };
        out.push(Checked { name: e.name.clone(), path, state });
    }
    Ok((out, false))
}

/// The counts, for the dialog's first line: *12 OK, 1 changed, 1 missing*.
pub fn summary(checked: &[Checked], stopped: bool) -> String {
    let count = |f: fn(&State) -> bool| checked.iter().filter(|c| f(&c.state)).count();
    let mut parts = vec![crate::tn!("sums.ok", count(|s| *s == State::Ok))];
    for (n, key) in [(count(|s| *s == State::Changed), "sums.changed"), (count(|s| *s == State::Missing), "sums.missing"), (count(|s| matches!(s, State::Unreadable(_))), "sums.unreadable")] {
        if n > 0 {
            parts.push(crate::tn!(key, n));
        }
    }
    let s = parts.join(", ");
    if stopped { format!("{s} · {}", crate::t!("sums.stopped")) } else { s }
}

/// The word for a state, as the list shows it.
pub fn word(s: &State) -> String {
    match s {
        State::Ok => crate::t!("sums.word_ok"),
        State::Changed => crate::t!("sums.word_changed"),
        State::Missing => crate::t!("sums.word_missing"),
        State::Unreadable(why) => format!("{}: {why}", crate::t!("sums.word_unreadable")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_are_made_and_checked_and_a_change_is_found() {
        let d = crate::fs::Scratch::named("checksum");
        let dir: &Path = &d;
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
        std::fs::write(dir.join("sub/b.txt"), "world\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/passwd", dir.join("link")).unwrap();
        let no = AtomicBool::new(false);
        let items = vec![dir.join("a.txt"), dir.join("sub"), dir.join("link")];
        let made = make(dir, &items, Algo::Sha256, &no, |_| {}).unwrap();
        assert_eq!(made.path, dir.join("SHA256SUMS"));
        let text = std::fs::read_to_string(&made.path).unwrap();
        // sha256sum's own output for "hello\n".
        assert_eq!(text.lines().next().unwrap(), "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03  a.txt");
        assert!(text.contains("  sub/b.txt\n") && !text.contains("link"), "{text}");
        // A second list does not write over the first.
        assert_eq!(make(dir, &items, Algo::Blake3, &no, |_| {}).unwrap().path, dir.join("B3SUMS"));
        assert_eq!(target(dir, Algo::Sha256), dir.join("SHA256SUMS-2"));

        let (all, stopped) = verify(&made.path, &no, |_| {}).unwrap();
        assert!(!stopped && all.iter().all(|c| c.state == State::Ok), "{all:?}");
        std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
        std::fs::remove_file(dir.join("sub/b.txt")).unwrap();
        let (all, _) = verify(&made.path, &no, |_| {}).unwrap();
        assert_eq!(all[0].state, State::Changed);
        assert_eq!(all[1].state, State::Missing);
        assert!(verify(&dir.join("B3SUMS"), &no, |_| {}).unwrap().0[0].state == State::Changed);
        // Stopped: nothing written, and verify says so.
        let yes = AtomicBool::new(true);
        assert!(make(dir, &items, Algo::Sha512, &yes, |_| {}).is_err());
        assert!(!dir.join("SHA512SUMS").exists());
        assert!(verify(&made.path, &yes, |_| {}).unwrap().1);
    }

    #[test]
    fn lists_in_every_form_are_read() {
        let h = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
        let text = format!("{h}  a b.txt\n{h} *bin.iso\r\nSHA256 (bsd.txt) = {h}\n\\{h}  back\\\\slash\nnot a line\n");
        let e = parse(&text, Path::new("SHA256SUMS"));
        let names: Vec<&str> = e.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a b.txt", "bin.iso", "bsd.txt", "back\\slash"]);
        assert!(e.iter().all(|e| e.algo == Algo::Sha256));
        assert_eq!(parse(h, Path::new("x.iso.b3"))[0], Entry { hash: h.into(), name: "x.iso".into(), algo: Algo::Blake3 });
        assert_eq!(parse(&format!("{h}{h}  f"), Path::new("f.sha512"))[0].algo, Algo::Sha512);
        assert!(is_list(Path::new("SHA256SUMS")) && is_list(Path::new("x.ISO.sha512")) && is_list(Path::new("B3SUMS-2")));
        assert!(!is_list(Path::new("notes.txt")));
        assert!(inside(Path::new("/d"), "../etc/passwd").is_none() && inside(Path::new("/d"), "/etc/passwd").is_none());
        assert_eq!(line(h, "a\nb"), format!("\\{h}  a\\nb\n"));
    }
}

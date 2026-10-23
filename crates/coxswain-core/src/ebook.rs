//! E-books calibre reads, in both apps: MOBI and Kindle books (.mobi, .azw, .azw3), FictionBook
//! (.fb2), Microsoft Reader (.lit) and Palm e-books (.pdb). Their previews (the cover, title,
//! author and first pages), their text for search, and *Convert e-book* to EPUB, AZW3 or PDF,
//! next to the original. Comics (.cbz, .cbr) and EPUB have readers of their own and are not
//! sent to calibre.
//!
//! Off unless `[preview] ebooks` is on. calibre runs as installed (`ebook-convert`), else from
//! its container image (`[preview.images] calibre`), chosen by `[preview] prefer` and
//! `[preview.tools] ebooks` as every preview tool is. The container is held tight: no network, no
//! capabilities, no new privileges, a read-only root with /tmp in memory, bounded memory and
//! processes, and nothing of the user's mounted. The book goes in on standard input as
//! `/tmp/in.<ending>`, so its name is never an argument, and what calibre made comes back on
//! standard output as a tar of the few files asked for, each bounded.
//!
//! A book locked with DRM is left alone: never unlocked, never converted. A MOBI or Kindle book
//! says so in its header, which is read first; calibre refuses the others, and that is said.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::config::PreviewConfig;
use crate::t;

/// Its name under `[preview.images]`.
pub const TOOL: &str = "calibre";
/// LinuxServer.io's calibre image, pinned to the calibre release tested here (9.16.0, the
/// index of its amd64 and arm64 images).
pub const IMAGE: &str = "docker.io/linuxserver/calibre:9.16.0@sha256:48c8a5ba2a20c303012b1d4b2815a2b99b362a634a571fbec101b85bfa203ba3";
/// The endings calibre reads here.
pub const FORMATS: &[&str] = &["mobi", "azw", "azw3", "fb2", "lit", "pdb"];
/// What *Convert e-book* makes.
pub const TO: [&str; 3] = ["epub", "azw3", "pdf"];
/// The version of what the search gets of these books: one more reads only them again.
pub const READER: &str = "1";
/// The largest book sent to calibre.
const MAX_IN: u64 = 256 << 20;
/// The most calibre may send back, all files together.
const MAX_OUT: u64 = 768 << 20;

/// The book's ending, lower case, when calibre reads it here. A `.pdb` is also a debugging
/// database or a protein structure: only a Palm e-book's header makes it one.
pub fn ending(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let e = FORMATS.iter().find(|f| **f == ext)?;
    (*e != "pdb" || palm_book(path)).then_some(*e)
}

/// Is it a book calibre reads here?
pub fn is_ebook(path: &Path) -> bool {
    ending(path).is_some()
}

/// The type and creator a Palm database gives an e-book: PalmDOC, eReader, Plucker, MOBI.
fn palm_book(path: &Path) -> bool {
    let mut h = [0u8; 68];
    File::open(path).and_then(|mut f| f.read_exact(&mut h)).is_ok()
        && [&b"TEXtREAd"[..], b"PNRdPPrs", b"DataPlkr", b"BOOKMOBI", b"TEXtTlDc"].contains(&&h[60..68])
}

/// A MOBI or Kindle book whose header says it is locked with DRM (its encryption type is not
/// 0). Others with DRM are found when calibre refuses them.
pub fn locked(path: &Path) -> bool {
    let read = || -> std::io::Result<bool> {
        let mut f = File::open(path)?;
        let mut h = [0u8; 82];
        f.read_exact(&mut h)?;
        if &h[60..68] != b"BOOKMOBI" {
            return Ok(false);
        }
        let first = u32::from_be_bytes([h[78], h[79], h[80], h[81]]);
        f.seek(SeekFrom::Start(first as u64 + 12))?;
        let mut e = [0u8; 2];
        f.read_exact(&mut e)?;
        Ok(e != [0, 0])
    };
    read().unwrap_or(false)
}

/// How calibre runs here.
#[derive(Clone, Debug, PartialEq)]
pub enum How {
    /// calibre's `ebook-convert`, installed, and its `ebook-meta` when there is one.
    Local { convert: PathBuf, meta: Option<PathBuf> },
    Container { rt: &'static str, path: PathBuf, image: String },
}

/// The way calibre runs here, or why it cannot: it is turned off, or what to install, or which
/// setting to change.
pub fn how(cfg: &PreviewConfig) -> Result<How, String> {
    if !cfg.ebooks {
        return Err(t!("ebook.off"));
    }
    let pref = crate::preview_tools::asked(cfg, TOOL);
    let pref = pref.as_str();
    let local = crate::tools::which("ebook-convert").map(|convert| How::Local { convert, meta: crate::tools::which("ebook-meta") });
    let image = cfg.images.get(TOOL).filter(|i| !i.is_empty()).cloned();
    let container = match (crate::tools::container_runtime(cfg), &image) {
        (Some((rt, path)), Some(image)) => Some(How::Container { rt, path, image: image.clone() }),
        _ => None,
    };
    let found = match pref {
        "container" => container.or(local),
        "local" => local,
        _ => local.or(container),
    };
    found.ok_or_else(|| {
        let install = match crate::tools::install("calibre") {
            Some(line) => t!("ebook.install_line", "line" => line),
            None => t!("ebook.install_site"),
        };
        let container = if pref == "local" {
            t!("searchable.prefer_local")
        } else if cfg.container == "off" {
            t!("searchable.containers_off")
        } else if image.is_none() {
            t!("convert.no_image", "tool" => TOOL)
        } else {
            crate::tools::no_runtime()
        };
        format!("{} {install} {container}", t!("ebook.needs"))
    })
}

/// Is what `how` needs there: an installed calibre, or the pulled image?
pub fn ready(how: &How) -> bool {
    match how {
        How::Local { .. } => true,
        How::Container { path, image, .. } => crate::tools::image_size(path, image).is_some(),
    }
}

/// What a run does, for the question before it: which calibre, and the first pull with its
/// size when the image is not there yet. The bool: a pull it makes.
pub fn about(how: &How) -> (String, bool) {
    match how {
        How::Local { .. } => (t!("ebook.runs_local"), false),
        How::Container { rt, image, .. } => {
            // Named without its digest, which says nothing to a reader.
            let named = image.split('@').next().unwrap_or(image);
            let runs = t!("ebook.runs_container", "runtime" => *rt, "image" => named);
            if ready(how) {
                return (runs, false);
            }
            let pull = match crate::images::download(image) {
                Some(size) => t!("convert.first_run_pulls_large", "image" => named, "size" => size),
                None => t!("convert.first_run_pulls", "image" => named),
            };
            (format!("{runs} {pull}."), true)
        }
    }
}

/// What one run of calibre makes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Want {
    /// An EPUB, the cover and what `ebook-meta` says (title, authors): a preview.
    Preview,
    /// An EPUB, at the lowest priority: the search reads its text.
    Text,
    /// The book as this format, one of `TO`.
    To(&'static str),
}

impl Want {
    fn format(self) -> &'static str {
        match self {
            Want::To(f) => f,
            _ => "epub",
        }
    }
}

/// The files a run may send back, and how large each may be.
const MADE: &[(&str, u64)] = &[
    ("out.epub", MAX_OUT),
    ("out.azw3", MAX_OUT),
    ("out.pdf", MAX_OUT),
    ("cover.jpg", 32 << 20),
    ("meta.txt", 64 << 10),
];

/// The shell line the container runs: every word in it is ours, the book's ending one of
/// `FORMATS` and the format one of `TO`.
fn script(ending: &str, want: Want) -> String {
    let to = want.format();
    let nice = if want == Want::Text { "nice -n 19 " } else { "" };
    let meta = if want == Want::Preview { "ebook-meta in.EXT --get-cover=cover.jpg > meta.txt 2>/dev/null; " } else { "" };
    let extra = if want == Want::Preview { " meta.txt $(ls cover.jpg 2>/dev/null)" } else { "" };
    format!("cd /tmp && cat > in.EXT && {nice}ebook-convert in.EXT out.TO >&2 && {meta}tar -cf - out.TO{extra}")
        .replace("EXT", ending)
        .replace("TO", to)
}

/// The arguments of `run` for calibre's container: no network, no capabilities, no new
/// privileges, nothing mounted, its root read-only with /tmp in memory, never a pull (an image
/// removed in Settings is not fetched again behind the user's back), as nobody, with `bounds`
/// its memory and processes bounded (`images::bounds_work`: off only where the runtime lacks
/// the controllers).
fn run_args(name: &str, image: &str, script: &str, bounds: bool) -> Vec<String> {
    let mut v: Vec<String> = ["run", "--rm", "-i", "--pull=never", "--network=none"].map(String::from).to_vec();
    // ffmpeg's and ImageMagick's: a read-only root, a small /tmp in memory, bounded memory and
    // processes, no capabilities, no new privileges.
    v.extend(crate::images::bounded(bounds).into_iter().map(String::from));
    v.extend(["--user", "65534:65534", "-e", "HOME=/tmp", "--name", name, "--entrypoint", "sh", image, "-c", script].map(String::from));
    v
}

static N: AtomicUsize = AtomicUsize::new(0);

/// What a run says while it goes.
pub enum Said<'a> {
    /// The image is being pulled: its progress line.
    Pulling(&'a str),
    /// A line calibre wrote.
    Line(&'a str),
}

/// calibre makes `want` of the book at `path` into the empty folder `out`. The image is pulled
/// first when it is not there (only ever after the user said yes to a run that says so).
/// `stop` and `timeout` end it, and its container with it.
fn make(how: &How, path: &Path, want: Want, out: &Path, stop: &AtomicBool, timeout: Duration, said: &mut dyn FnMut(Said)) -> Result<(), String> {
    let ending = ending(path).ok_or_else(|| t!("ebook.not_a_book"))?;
    if locked(path) {
        return Err(t!("ebook.drm"));
    }
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_IN {
        return Err(t!("ebook.too_large"));
    }
    match how {
        How::Local { convert, meta } => {
            // An absolute path: a name never reads as an option. calibre goes by the ending.
            let path = std::path::absolute(path).map_err(|e| e.to_string())?;
            let to = out.join(format!("out.{}", want.format()));
            let mut c = crate::tools::low(convert);
            c.arg(&path).arg(&to);
            go(c, None, stop, timeout, None, None, said)?;
            if want == Want::Preview
                && let Some(meta) = meta
            {
                let mut c = crate::tools::low(meta);
                c.arg(&path).arg(format!("--get-cover={}", out.join("cover.jpg").display()));
                if let Ok(o) = crate::tools::capture(c, Duration::from_secs(60), 64 << 10) {
                    let _ = std::fs::write(out.join("meta.txt"), o.stdout);
                }
            }
            Ok(())
        }
        How::Container { path: rt, image, .. } => {
            if crate::tools::image_size(rt, image).is_none() {
                // Stop ends the pull too: it runs on a scoped thread that watches `stop`.
                std::thread::scope(|scope| {
                    let puller = scope.spawn(|| crate::images::pull_until(rt, image, stop));
                    while !puller.is_finished() {
                        if let Some(line) = crate::images::pulling().get(image.as_str()) {
                            said(Said::Pulling(line));
                        }
                        std::thread::sleep(Duration::from_millis(300));
                    }
                    puller.join().map_err(|_| t!("convert.failed", "status" => "pull"))?
                })?;
            }
            let name = format!("coxswain-calibre-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
            let mut c = crate::tools::command(rt);
            c.args(run_args(&name, image, &script(ending, want), crate::images::bounds_work(rt, image)));
            let book = File::open(path).map_err(|e| e.to_string())?;
            go(c, Some(book), stop, timeout, Some((rt, &name)), Some(out), said)
        }
    }
}

/// Run `c`, `input` on its standard input, its standard error said line by line; with `untar`,
/// its standard output is the tar of what it made, unpacked there (`unpack`). `stop` or
/// `timeout` kill it, and `kill`'s container.
fn go(
    c: Command,
    input: Option<File>,
    stop: &AtomicBool,
    timeout: Duration,
    kill: Option<(&Path, &str)>,
    untar: Option<&Path>,
    said: &mut dyn FnMut(Said),
) -> Result<(), String> {
    let mut c = crate::tools::hosted(c);
    c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(if untar.is_some() { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| e.to_string())?;
    let writer = child.stdin.take().zip(input).map(|(mut stdin, mut book)| {
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut book, &mut stdin);
        })
    });
    let reader = child.stdout.take().zip(untar.map(Path::to_path_buf)).map(|(stdout, dir)| std::thread::spawn(move || unpack(stdout, &dir)));
    let lines = crate::searchable::follow(&mut child);
    let (began, mut last) = (Instant::now(), Vec::<String>::new());
    let end = |child: &mut std::process::Child| {
        let _ = child.kill();
        if let Some((rt, name)) = kill {
            let _ = crate::tools::command(rt).args(["kill", name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
        let _ = child.wait();
    };
    let status = loop {
        while let Ok(l) = lines.try_recv() {
            said(Said::Line(&l));
            last.push(l);
            if last.len() > 40 {
                last.remove(0);
            }
        }
        if stop.load(Ordering::Relaxed) {
            end(&mut child);
            return Err(t!("ebook.stopped"));
        }
        if began.elapsed() > timeout {
            end(&mut child);
            return Err(t!("ebook.took_too_long"));
        }
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if let Some(w) = writer {
        let _ = w.join();
    }
    let unpacked = reader.map(|r| r.join().unwrap_or_else(|_| Err("tar".into())));
    while let Ok(l) = lines.try_recv() {
        last.push(l);
    }
    if !status.success() {
        // calibre's own word for a locked book; whatever else the book makes it say is only shown.
        if last.iter().any(|l| l.contains("DRMError")) {
            return Err(t!("ebook.drm"));
        }
        let tail: Vec<&str> = last.iter().rev().take(6).rev().map(String::as_str).collect();
        return Err(if tail.is_empty() { t!("convert.failed", "status" => status) } else { tail.join("\n") });
    }
    unpacked.unwrap_or(Ok(()))
}

/// Unpack the tar on `r` into `dir`: only the names of `MADE`, plain files, each within its
/// bound and all within `MAX_OUT`; never a link, a folder, or a name twice.
fn unpack(r: impl Read, dir: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(r.take(MAX_OUT + (1 << 20)));
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let name = entry.path().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
        let Some((_, most)) = MADE.iter().find(|(n, _)| *n == name) else { continue };
        if entry.header().entry_type() != tar::EntryType::Regular {
            continue;
        }
        if entry.header().size().map_err(|e| e.to_string())? > *most {
            return Err(t!("ebook.too_large"));
        }
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(&name)).map_err(|e| e.to_string())?;
        std::io::copy(&mut (&mut entry).take(*most), &mut f).map_err(|e| e.to_string())?;
    }
    // What tar pads its end with, read too: a pipe closed early would stop it with an error.
    let _ = std::io::copy(&mut archive.into_inner(), &mut std::io::sink());
    Ok(())
}

// ---------------------------------------------------------------- preview

/// What the preview shows of a book: its title and authors, its cover, and the EPUB calibre
/// made of it, whose first chapter (`extract::book::first_chapter`) or text is shown.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Preview {
    pub title: String,
    pub authors: String,
    pub cover: Option<PathBuf>,
    pub epub: PathBuf,
}

/// The preview of the book at `path` in the folder `dir`, made by an earlier run; None when
/// there is none.
pub fn shown(dir: &Path) -> Option<Preview> {
    let epub = dir.join("out.epub");
    if !epub.is_file() {
        return None;
    }
    let meta = std::fs::read(dir.join("meta.txt")).unwrap_or_default();
    let (mut title, authors) = meta_of(&String::from_utf8_lossy(&meta));
    if title.is_empty() {
        title = crate::extract::book::first_chapter(&epub).map(|(t, _)| t).unwrap_or_default();
    }
    let cover = Some(dir.join("cover.jpg")).filter(|c| std::fs::metadata(c).is_ok_and(|m| m.len() > 0));
    Some(Preview { title, authors, cover, epub })
}

/// Make the preview of the book at `path` into `dir`, a folder of its own (emptied first);
/// `said` hears how a first pull goes.
pub fn preview(how: &How, path: &Path, dir: &Path, timeout: Duration, said: &mut dyn FnMut(Said)) -> Result<Preview, String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let made = make(how, path, Want::Preview, dir, &AtomicBool::new(false), timeout, said);
    if let Err(e) = made {
        let _ = std::fs::remove_dir_all(dir);
        return Err(e);
    }
    shown(dir).ok_or_else(|| t!("convert.no_result"))
}

/// The folder in the cache for the preview of the book at `path` as it is now.
pub fn cache_dir(path: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, meta.len(), meta.modified().ok(), TOOL).hash(&mut h);
    Some(crate::dirs::cache_dir()?.join("coxswain").join("previews").join(format!("{:016x}", h.finish())))
}

/// The preview as text, for the terminal app's viewer (F3): the book's title and authors,
/// then its chapters, written next to the preview as `book.txt`.
pub fn text_copy(p: &Preview) -> Result<PathBuf, String> {
    let to = p.epub.with_file_name("book.txt");
    if !to.is_file() {
        let len = std::fs::metadata(&p.epub).map_err(|e| e.to_string())?.len();
        let text = crate::extract::book::text(&p.epub, len).unwrap_or_default();
        std::fs::write(&to, crate::extract::tidy(&text)).map_err(|e| e.to_string())?;
    }
    Ok(to)
}

/// Title and authors as `ebook-meta` prints them (`Title : …`, `Author(s) : Ada [Lovelace,
/// Ada]`), without the name calibre sorts by.
fn meta_of(text: &str) -> (String, String) {
    let (mut title, mut authors) = (String::new(), String::new());
    for line in text.lines().take(200) {
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        match k.trim() {
            "Title" if title.is_empty() => title = v.chars().take(500).collect(),
            "Author(s)" if authors.is_empty() => {
                let v = match v.rsplit_once(" [") {
                    Some((names, _)) if v.ends_with(']') => names,
                    _ => v,
                };
                authors = v.chars().take(500).collect();
            }
            _ => {}
        }
    }
    (title, authors)
}

// ---------------------------------------------------------------- Convert e-book

/// Where `path` made into `to` goes: `name.<to>`, else `name 2.<to>` and on, the first name
/// that is free.
pub fn target(path: &Path, to: &str) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    (1..)
        .map(|n| dir.join(if n == 1 { format!("{stem}.{to}") } else { format!("{stem} {n}.{to}") }))
        .find(|p| !p.exists())
        .expect("a free name")
}

/// The formats a book can be made into: those of `TO` it is not already.
pub fn formats_for(path: &Path) -> Vec<&'static str> {
    let own = ending(path);
    TO.iter().copied().filter(|t| Some(*t) != own).collect()
}

/// Convert each of `books` into `to`, one after the other: `at(i)` as each starts, `said` with
/// what calibre says. `stop` ends the run under way, and the rest are not started. One result
/// per book started: the file written next to it, or why not.
pub fn run_all(
    cfg: &PreviewConfig,
    to: &str,
    books: &[PathBuf],
    stop: &AtomicBool,
    mut at: impl FnMut(usize),
    mut said: impl FnMut(Said),
) -> Vec<Result<PathBuf, String>> {
    let how = match how(cfg) {
        Ok(h) => h,
        Err(e) => return vec![Err(e)],
    };
    let Some(to) = TO.iter().copied().find(|t| *t == to) else {
        return vec![Err(t!("convert.no_result"))];
    };
    let timeout = Duration::from_secs(cfg.timeout.max(600));
    let mut done = vec![];
    for (i, book) in books.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        at(i);
        done.push(convert(&how, book, to, stop, timeout, &mut said));
    }
    done
}

/// One book. A folder that cannot be written to says so before calibre starts (a probe file,
/// removed at once); the name is taken only when the result is written, with `create_new`, so
/// nothing is overwritten and an app that ends mid-run leaves no empty file.
fn convert(how: &How, book: &Path, to: &'static str, stop: &AtomicBool, timeout: Duration, said: &mut dyn FnMut(Said)) -> Result<PathBuf, String> {
    if ending(book) == Some(to) {
        return Err(t!("ebook.same_format"));
    }
    let probe = target(book, to);
    std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(|e| format!("{}: {e}", probe.display()))?;
    let _ = std::fs::remove_file(&probe);
    let scratch = crate::fs::Scratch::new().map_err(|e| e.to_string())?;
    make(how, book, Want::To(to), &scratch, stop, timeout, said)?;
    let made = scratch.join(format!("out.{to}"));
    if !made.is_file() {
        return Err(t!("convert.no_result"));
    }
    let (to_path, mut file) = (0..100)
        .map(|_| target(book, to))
        .find_map(|p| std::fs::OpenOptions::new().write(true).create_new(true).open(&p).ok().map(|f| (p, f)))
        .ok_or_else(|| format!("{}: {}", probe.display(), t!("err.no_file_name")))?;
    let copied = std::io::copy(&mut File::open(&made).map_err(|e| e.to_string())?, &mut file).and_then(|_| file.flush());
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&to_path);
        return Err(format!("{}: {e}", to_path.display()));
    }
    Ok(to_path)
}

// ---------------------------------------------------------------- search

/// calibre as the search helper uses it, from config.toml as it was when the helper started
/// (it starts again when `[preview] ebooks` changes): None unless e-books and text search are
/// both on.
fn searching() -> Option<&'static How> {
    static H: OnceLock<Option<How>> = OnceLock::new();
    H.get_or_init(|| {
        let cfg = crate::config::Config::load().ok()?;
        (cfg.search.text && cfg.preview.ebooks).then(|| how(&cfg.preview).ok()).flatten()
    })
    .as_ref()
}

/// The image is pulled (or calibre installed): set once known, so a read costs no runtime call.
static READY: AtomicBool = AtomicBool::new(false);

/// When the helper starts: with e-books on and the image not pulled yet, it is pulled in the
/// background (turning the option on, under its size and cost, is the click that asks for it),
/// then `pulled` to read the books it can now read. Nothing when it is off.
pub fn prepare(pulled: impl FnOnce() + Send + 'static) {
    let Some(how) = searching() else { return };
    if ready(how) {
        READY.store(true, Ordering::Relaxed);
        return;
    }
    let How::Container { path, image, .. } = how.clone() else { return };
    std::thread::spawn(move || match crate::images::pull(&path, &image) {
        Ok(()) => {
            READY.store(true, Ordering::Relaxed);
            used(&image);
            pulled();
        }
        Err(e) => crate::helper::note(format_args!("calibre: {e}")),
    });
}

/// The endings the search reads with calibre now: none until it can run.
pub fn extensions() -> Vec<&'static str> {
    if searching().is_some() && READY.load(Ordering::Relaxed) { FORMATS.to_vec() } else { vec![] }
}

/// The text of the book at `path` for the search: its title, authors and chapters, from the
/// EPUB calibre makes of it at the lowest priority. None when calibre is off or not ready, for
/// a book locked with DRM, and for one larger than `max`.
pub fn text(path: &Path, max: u64) -> Option<String> {
    let how = searching()?;
    if !READY.load(Ordering::Relaxed) || std::fs::metadata(path).ok()?.len() > max.min(MAX_IN) {
        return None;
    }
    let scratch = crate::fs::Scratch::new().ok()?;
    make(how, path, Want::Text, &scratch, &AtomicBool::new(false), Duration::from_secs(300), &mut |_| {}).ok()?;
    let epub = scratch.join("out.epub");
    let text = crate::extract::book::text(&epub, std::fs::metadata(&epub).ok()?.len())?;
    if let How::Container { image, .. } = how {
        used_daily(image);
    }
    Some(text)
}

/// The image was used: what *Remove images not used for 90 days* goes by.
fn used(image: &str) {
    let mut st = crate::state::AppState::load();
    crate::images::used(&mut st, image, crate::fs::now());
    let _ = st.save();
}

/// `used`, once a day at most.
fn used_daily(image: &str) {
    static NOTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = crate::fs::now();
    if now.saturating_sub(NOTED.load(Ordering::Relaxed)) >= 24 * 3600 {
        NOTED.store(now, Ordering::Relaxed);
        used(image);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Palm database header of `kind` (type and creator), its first record at 86 with a
    /// PalmDOC header whose encryption field is `lock`.
    fn palm(kind: &[u8; 8], lock: u16) -> Vec<u8> {
        let mut b = vec![0u8; 86];
        b[60..68].copy_from_slice(kind);
        b[76..78].copy_from_slice(&1u16.to_be_bytes());
        b[78..82].copy_from_slice(&86u32.to_be_bytes());
        let mut rec = vec![0u8; 16];
        rec[12..14].copy_from_slice(&lock.to_be_bytes());
        b.extend(rec);
        b
    }

    #[test]
    fn ebooks_locked_with_drm_are_known_by_their_header() {
        let d = crate::fs::Scratch::named("ebook-drm");
        std::fs::write(d.join("open.mobi"), palm(b"BOOKMOBI", 0)).unwrap();
        std::fs::write(d.join("locked.azw3"), palm(b"BOOKMOBI", 2)).unwrap();
        std::fs::write(d.join("short.mobi"), b"BOOKMOBI").unwrap();
        std::fs::write(d.join("doc.pdb"), palm(b"TEXtREAd", 0)).unwrap();
        std::fs::write(d.join("symbols.pdb"), b"Microsoft C/C++ MSF 7.00\r\n\x1aDS\0\0\0").unwrap();
        assert!(!locked(&d.join("open.mobi")));
        assert!(locked(&d.join("locked.azw3")));
        assert!(!locked(&d.join("short.mobi")), "too short to say");
        assert!(!locked(&d.join("doc.pdb")), "PalmDOC has no encryption field");
        assert_eq!(ending(&d.join("doc.pdb")), Some("pdb"));
        assert_eq!(ending(&d.join("symbols.pdb")), None, "a debugging database is no book");
        assert_eq!(ending(Path::new("/x/Book.AZW3")), Some("azw3"));
        assert!(!is_ebook(Path::new("/x/comic.cbz")) && !is_ebook(Path::new("/x/b.epub")), "they have their own readers");
        let mut cfg = PreviewConfig { ebooks: true, ..Default::default() };
        cfg.tools.insert("ebooks".into(), "local".into());
        let how = How::Local { convert: "/nonexistent/ebook-convert".into(), meta: None };
        let r = make(&how, &d.join("locked.azw3"), Want::Text, &d, &AtomicBool::new(false), Duration::from_secs(1), &mut |_| {});
        crate::i18n::set_language("en-GB");
        assert_eq!(r, Err(t!("ebook.drm")), "never sent to calibre");
    }

    #[test]
    fn ebooks_are_off_by_default_and_follow_the_preference() {
        crate::i18n::set_language("en-GB");
        let mut cfg = PreviewConfig::default();
        assert!(!cfg.ebooks);
        assert_eq!(how(&cfg), Err(t!("ebook.off")));
        assert_eq!(cfg.images.get(TOOL).map(String::as_str), Some(IMAGE));
        cfg.ebooks = true;
        cfg.container = "off".into();
        cfg.tools.insert("ebooks".into(), "container".into());
        match how(&cfg) {
            Ok(How::Local { .. }) => {}
            Ok(other) => panic!("containers are off: {other:?}"),
            Err(e) => assert!(e.contains(&t!("searchable.containers_off")), "{e}"),
        }
    }

    #[test]
    fn ebooks_run_sealed_and_never_name_the_book() {
        let s = script("mobi", Want::Preview);
        assert_eq!(s, "cd /tmp && cat > in.mobi && ebook-convert in.mobi out.epub >&2 && ebook-meta in.mobi --get-cover=cover.jpg > meta.txt 2>/dev/null; tar -cf - out.epub meta.txt $(ls cover.jpg 2>/dev/null)");
        assert!(script("fb2", Want::Text).contains("nice -n 19 ebook-convert in.fb2 out.epub"));
        assert!(script("lit", Want::To("pdf")).ends_with("tar -cf - out.pdf"));
        assert!(!run_args("n", IMAGE, "x", false).iter().any(|a| a.starts_with("--memory") || a.starts_with("--pids")));
        let args = run_args("n", IMAGE, "x", true);
        for want in ["--rm", "--pull=never", "--network=none", "--cap-drop=ALL", "no-new-privileges", "--read-only", "/tmp:rw,size=1g", "--memory=3g", "--pids-limit=256", "65534:65534"] {
            assert!(args.iter().any(|a| a == want), "{want}: {args:?}");
        }
        assert!(!args.iter().any(|a| a.starts_with("-v") || a.starts_with("--mount") || a.starts_with("-p") || a.contains("label=disable")), "{args:?}");
    }

    #[test]
    fn ebooks_unpack_only_what_was_asked_for_within_bounds() {
        let tar_of = |files: &[(&str, &[u8], tar::EntryType)]| {
            let mut b = tar::Builder::new(vec![]);
            for (name, data, kind) in files {
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(*kind);
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                b.append_data(&mut h, name, *data).unwrap();
            }
            b.into_inner().unwrap()
        };
        let d = crate::fs::Scratch::named("ebook-unpack");
        let t = tar_of(&[("out.epub", b"book", tar::EntryType::Regular), ("x/out.pdf", b"x", tar::EntryType::Regular), ("meta.txt", b"Title : T", tar::EntryType::Regular), ("cover.jpg", b"", tar::EntryType::Symlink)]);
        unpack(&t[..], &d).unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&*d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["meta.txt", "out.epub"]);
        assert!(unpack(&tar_of(&[("out.epub", b"again", tar::EntryType::Regular)])[..], &d).is_err(), "never written over");
        let big = vec![b'a'; (64 << 10) + 1];
        let e = crate::fs::Scratch::named("ebook-unpack-big");
        assert!(unpack(&tar_of(&[("meta.txt", &big, tar::EntryType::Regular)])[..], &e).is_err());
    }

    #[test]
    fn ebooks_meta_and_targets() {
        assert_eq!(
            meta_of("Title               : The Rocket Garden\nAuthor(s)           : Ada Lovelace & Grace Hopper [Lovelace, Ada & Hopper, Grace]\nLanguages : eng\n"),
            ("The Rocket Garden".into(), "Ada Lovelace & Grace Hopper".into())
        );
        assert_eq!(meta_of("No cover found\n"), (String::new(), String::new()));
        let d = crate::fs::Scratch::named("ebook-target");
        let book = d.join("story.azw3");
        assert_eq!(target(&book, "epub"), d.join("story.epub"));
        std::fs::write(d.join("story.epub"), "").unwrap();
        assert_eq!(target(&book, "epub"), d.join("story 2.epub"));
        std::fs::write(&book, palm(b"BOOKMOBI", 0)).unwrap();
        assert_eq!(formats_for(&book), ["epub", "pdf"]);
        assert_eq!(formats_for(&d.join("a.fb2")), TO);
    }

    /// Needs podman and calibre's image pulled (`podman pull` of `IMAGE`), and a book:
    /// `COXSWAIN_TEST_BOOK=x.mobi cargo test -p coxswain-core -- --ignored real_calibre`.
    #[test]
    #[ignore]
    fn real_calibre_previews_converts_and_reads() {
        let book = PathBuf::from(std::env::var("COXSWAIN_TEST_BOOK").expect("COXSWAIN_TEST_BOOK: an e-book"));
        let d = crate::fs::Scratch::named("ebook-real");
        let copy = d.join(format!("-x $(id).{}", ending(&book).unwrap()));
        std::fs::copy(&book, &copy).unwrap();
        let how = How::Container { rt: "podman", path: crate::tools::which("podman").unwrap(), image: IMAGE.into() };
        let started = Instant::now();
        let p = preview(&how, &copy, &d.join("preview"), Duration::from_secs(120), &mut |_| {}).unwrap();
        eprintln!("{p:?} in {:?}", started.elapsed());
        assert!(!p.title.is_empty() && p.epub.is_file());
        let txt = std::fs::read_to_string(text_copy(&p).unwrap()).unwrap();
        assert!(txt.starts_with(&p.title) && txt.contains("Chapter"), "{txt}");
        let mut cfg = PreviewConfig { ebooks: true, ..Default::default() };
        cfg.tools.insert("ebooks".into(), "container".into());
        for to in formats_for(&copy) {
            let r = run_all(&cfg, to, std::slice::from_ref(&copy), &AtomicBool::new(false), |_| {}, |_| {});
            let made = r.into_iter().next().unwrap().unwrap();
            assert_eq!(made, copy.with_extension(to));
            assert!(std::fs::metadata(&made).unwrap().len() > 100);
        }
        // Made again: next to the first, never over it.
        let r = run_all(&cfg, "pdf", std::slice::from_ref(&copy), &AtomicBool::new(false), |_| {}, |_| {});
        assert!(r[0].as_ref().unwrap().to_string_lossy().ends_with(" 2.pdf"));
    }
}

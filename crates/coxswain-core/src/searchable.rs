//! *Make PDF searchable*, in both apps: OCRmyPDF finds the words on the pages of a scanned PDF
//! and writes a copy with them in it, `report.searchable.pdf` next to `report.pdf`; the
//! original is never changed and nothing is ever overwritten. The copy is a PDF with text, so
//! the search store reads it as it reads any other: nothing is read by OCR in the background.
//!
//! OCRmyPDF runs as installed, else from its container image (`[preview.images] ocrmypdf`), as
//! a preview tool does: `[preview] prefer` and `[preview.tools] ocr` choose, a container
//! gets no network and sees the PDF's folder read-only. Its image is pulled only once the user
//! has said yes to a first run that says it pulls.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::config::PreviewConfig;
use crate::t;

/// Its name under `[preview.images]`.
pub const TOOL: &str = "ocrmypdf";

/// The languages OCRmyPDF's default image reads (`/usr/share/tessdata`, 2026-10).
const IMAGE_LANGUAGES: [&str; 6] = ["eng", "deu", "fra", "por", "spa", "chi_sim"];

/// How OCRmyPDF runs here.
#[derive(Clone, Debug, PartialEq)]
pub enum How {
    Local(PathBuf),
    Container { rt: &'static str, path: PathBuf, image: String },
}

/// The way it runs here, or why it cannot: what to install, or which setting to change.
pub fn how(cfg: &PreviewConfig) -> Result<How, String> {
    let pref = crate::preview_tools::asked(cfg, TOOL);
    let pref = pref.as_str();
    let local = crate::tools::which("ocrmypdf").map(How::Local);
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
    found.ok_or_else(|| missing(cfg, pref, image.is_some()))
}

/// Why OCRmyPDF cannot run: the line that installs it, and what keeps the container from it.
fn missing(cfg: &PreviewConfig, pref: &str, image: bool) -> String {
    let install = match crate::tools::install("ocrmypdf") {
        Some(line) => t!("searchable.install_line", "line" => line),
        None => t!("searchable.install_site"),
    };
    let container = if pref == "local" {
        t!("searchable.prefer_local")
    } else if cfg.container == "off" {
        t!("searchable.containers_off")
    } else if !image {
        t!("convert.no_image", "tool" => TOOL)
    } else {
        crate::tools::no_runtime()
    };
    format!("{} {install} {container}", t!("searchable.needs"))
}

/// What a run does, for the question before it: which program, and the first pull with its
/// size when the image is not there yet. The bool: a pull it makes.
pub fn about(how: &How) -> (String, bool) {
    match how {
        How::Local(_) => (t!("searchable.runs_local"), false),
        How::Container { rt, path, image } => {
            let runs = t!("searchable.runs_container", "runtime" => *rt, "image" => crate::images::short(image));
            if crate::tools::image_size(path, image).is_some() {
                return (runs, false);
            }
            let pull = match crate::images::download(image) {
                Some(size) => t!("convert.first_run_pulls_large", "image" => crate::images::short(image), "size" => size),
                None => t!("convert.first_run_pulls", "image" => crate::images::short(image)),
            };
            (format!("{runs} {pull}."), true)
        }
    }
}

/// Is it a PDF, by its name?
pub fn is_pdf(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// A PDF without text on its pages, a scan: the preview offers to make it searchable. Read
/// with the search's own reader, three seconds at most; a file over 15 MB is not looked at.
pub fn scanned(path: &Path) -> bool {
    const MAX: u64 = 15_000_000;
    is_pdf(path)
        && std::fs::metadata(path).is_ok_and(|m| m.len() <= MAX)
        && crate::extract::pdf::text(path, MAX)
            .is_none_or(|t| t.chars().filter(|c| c.is_alphanumeric()).count() < 20)
}

/// Where the copy of `pdf` goes: `name.searchable.pdf`, else `name.searchable 2.pdf` and on,
/// the first name that is free.
pub fn target(pdf: &Path) -> PathBuf {
    let dir = pdf.parent().unwrap_or(Path::new("."));
    let stem = pdf.file_stem().unwrap_or_default().to_string_lossy();
    (1..)
        .map(|n| dir.join(if n == 1 { format!("{stem}.searchable.pdf") } else { format!("{stem}.searchable {n}.pdf") }))
        .find(|p| !p.exists())
        .expect("a free name")
}

/// The languages to read: English and the one Coxswain is set to, of those there.
fn languages(how: &How) -> String {
    match how {
        How::Local(_) => crate::tools::which("tesseract")
            .map(|t| crate::extract::installed::languages(&t).to_string())
            .unwrap_or_else(|| "eng".into()),
        How::Container { .. } => {
            let own = crate::extract::installed::own_language();
            ["eng", own].into_iter().filter(|l| IMAGE_LANGUAGES.contains(l)).collect::<Vec<_>>().join("+")
        }
    }
}

/// What a run says while it goes.
pub enum Said<'a> {
    /// The image is being pulled: its progress line.
    Pulling(&'a str),
    /// A line OCRmyPDF wrote.
    Line(&'a str),
}

/// Make each of `pdfs` searchable, one after the other: `at(i)` as each starts, `said` with
/// what it says. `stop` ends the run under way, and the rest are not started; what a stopped
/// run made is removed. One result per PDF started: the copy written, or why not.
pub fn run_all(
    cfg: &PreviewConfig,
    pdfs: &[PathBuf],
    stop: &AtomicBool,
    mut at: impl FnMut(usize),
    mut said: impl FnMut(Said),
) -> Vec<Result<PathBuf, String>> {
    let how = match how(cfg) {
        Ok(h) => h,
        Err(e) => return vec![Err(e)],
    };
    let mut done = vec![];
    for (i, pdf) in pdfs.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        at(i);
        done.push(run(&how, pdf, stop, &mut said));
    }
    done
}

/// One PDF. A folder that cannot be written to says so before OCRmyPDF starts (a probe file,
/// removed at once); the copy's name is taken only when the copy is written, with
/// `create_new`, so nothing is overwritten and an app that ends mid-run leaves no empty file.
fn run(how: &How, pdf: &Path, stop: &AtomicBool, said: &mut impl FnMut(Said)) -> Result<PathBuf, String> {
    let dir = pdf.parent().ok_or_else(|| t!("err.no_folder"))?;
    let name = pdf.file_name().ok_or_else(|| t!("err.no_file_name"))?.to_string_lossy().into_owned();
    let probe = target(pdf);
    std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(|e| format!("{}: {e}", probe.display()))?;
    let _ = std::fs::remove_file(&probe);
    let mut made: Option<PathBuf> = None;
    let result = (|| {
        let scratch = Scratch::new()?;
        let out = scratch.0.join("out.pdf");
        let langs = languages(how);
        let (c, kill) = match how {
            How::Local(program) => {
                let mut c = crate::tools::low(program);
                c.args(["--skip-text", "-l", &langs]).arg(pdf).arg(&out);
                (c, None)
            }
            How::Container { rt, path, image } => {
                if crate::tools::image_size(path, image).is_none() {
                    // Stop ends the pull too: it runs on a scoped thread that watches `stop`.
                    std::thread::scope(|scope| {
                        let puller = scope.spawn(|| crate::images::pull_until(path, image, stop));
                        while !puller.is_finished() {
                            if let Some(line) = crate::images::pulling().get(image.as_str()) {
                                said(Said::Pulling(line));
                            }
                            std::thread::sleep(Duration::from_millis(300));
                        }
                        puller.join().map_err(|_| t!("convert.failed", "status" => "pull"))?
                    })?;
                }
                let container = format!("coxswain-ocr-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
                let mut c = crate::images::sealed(rt, path, &container, dir, Some(&scratch.0));
                // The image's entry point is ocrmypdf.
                c.args(["-w", "/src", image.as_str(), "--skip-text", "-l", &langs, &format!("/src/{name}"), "/out/out.pdf"]);
                (c, Some((path.clone(), container)))
            }
        };
        let mut c = crate::tools::hosted(c);
        let mut child = crate::tools::own_group(&mut c).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
        let lines = follow(&mut child);
        let mut last = vec![];
        let status = loop {
            while let Ok(l) = lines.try_recv() {
                said(Said::Line(&l));
                last.push(l);
                if last.len() > 12 {
                    last.remove(0);
                }
            }
            if stop.load(Ordering::Relaxed) {
                crate::tools::kill_tree(&mut child);
                if let Some((rt, name)) = &kill {
                    let _ = crate::tools::command(rt).args(["kill", name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
                }
                return Err(t!("searchable.stopped"));
            }
            if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                break s;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        while let Ok(l) = lines.try_recv() {
            last.push(l);
        }
        if !status.success() || !out.is_file() {
            let tail = last.iter().rev().take(6).rev().cloned().collect::<Vec<_>>().join("\n");
            return Err(if tail.is_empty() { t!("convert.failed", "status" => status) } else { tail });
        }
        // The first free name now, taken with create_new: a file made meanwhile is never
        // written over.
        let to = (0..100)
            .map(|_| target(pdf))
            .find_map(|to| std::fs::OpenOptions::new().write(true).create_new(true).open(&to).ok().map(|f| (to, f)));
        let (to, mut file) = to.ok_or_else(|| format!("{}: {}", probe.display(), t!("err.no_file_name")))?;
        made = Some(to.clone());
        let (m, len) = crate::images::take_result(&out)?;
        std::io::copy(&mut std::io::Read::take(m, len), &mut file).map_err(|e| format!("{}: {e}", to.display()))?;
        Ok(to)
    })();
    if let (Err(_), Some(half)) = (&result, made) {
        let _ = std::fs::remove_file(half);
    }
    result
}

static N: AtomicUsize = AtomicUsize::new(0);

/// The lines `child` writes to its standard error, as they come; progress bars that redraw
/// with `\r` count as lines.
pub(crate) fn follow(child: &mut Child) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    if let Some(mut err) = child.stderr.take() {
        std::thread::spawn(move || {
            let (mut buf, mut acc) = ([0u8; 4096], String::new());
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                acc.push_str(&String::from_utf8_lossy(&buf[..n]));
                while let Some(i) = acc.find(['\n', '\r']) {
                    let line = acc[..i].trim().to_string();
                    acc.drain(..=i);
                    if !line.is_empty() && tx.send(line).is_err() {
                        return;
                    }
                }
            }
            if !acc.trim().is_empty() {
                let _ = tx.send(acc.trim().to_string());
            }
        });
    }
    rx
}

/// A folder of its own in the cache for one run, removed when dropped.
pub(crate) struct Scratch(pub(crate) PathBuf);

impl Scratch {
    fn new() -> Result<Scratch, String> {
        Scratch::named("ocr")
    }

    /// A folder of its own under the cache's `coxswain/<kind>`.
    pub(crate) fn named(kind: &str) -> Result<Scratch, String> {
        let base = crate::dirs::cache_dir().ok_or_else(|| t!("err.no_cache_folder"))?;
        let ocr = base.join("coxswain").join(kind);
        // What a run of an app that was killed left behind, a day on (a run may take hours).
        for e in std::fs::read_dir(&ocr).into_iter().flatten().flatten() {
            if e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|a| a > Duration::from_secs(24 * 3600))) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
        let dir = ocr.join(format!("{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searchable_copies_never_take_a_name_in_use() {
        let d = std::env::temp_dir().join(format!("coxswain-searchable-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let pdf = d.join("scan.PDF");
        assert!(is_pdf(&pdf));
        assert_eq!(target(&pdf), d.join("scan.searchable.pdf"));
        std::fs::write(d.join("scan.searchable.pdf"), "").unwrap();
        assert_eq!(target(&pdf), d.join("scan.searchable 2.pdf"));
        std::fs::write(&pdf, "not a pdf").unwrap();
        assert!(scanned(&pdf), "no text found: offered");
        assert!(!scanned(&d.join("scan.searchable.txt")));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn searchable_follows_the_preference_and_says_what_is_missing() {
        crate::i18n::set_language("en-GB");
        let mut cfg = PreviewConfig { container: "off".into(), ..Default::default() };
        cfg.tools.insert("ocr".into(), "container".into());
        match how(&cfg) {
            Ok(How::Local(_)) => {}
            Ok(other) => panic!("containers are off: {other:?}"),
            Err(e) => assert!(e.contains(&t!("searchable.containers_off")), "{e}"),
        }
        cfg.tools.insert("ocr".into(), "local".into());
        if let Err(e) = how(&cfg) {
            assert!(e.contains(&t!("searchable.prefer_local")), "{e}");
        }
    }

    /// Needs podman (or a `podman` on PATH that adds its own store) and the network:
    /// `cargo test -p coxswain-core -- --ignored real_searchable`. A page drawn as a picture.
    #[test]
    #[ignore]
    fn real_searchable_in_a_container() {
        let src = std::env::var("COXSWAIN_SCAN").expect("COXSWAIN_SCAN=a scanned PDF");
        let d = std::env::temp_dir().join(format!("coxswain-real-searchable-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let pdf = d.join("scan.pdf");
        std::fs::copy(src, &pdf).unwrap();
        assert!(scanned(&pdf));
        let mut cfg = PreviewConfig::default();
        cfg.tools.insert("ocr".into(), "container".into());
        let start = std::time::Instant::now();
        let r = run_all(&cfg, std::slice::from_ref(&pdf), &AtomicBool::new(false), |_| {}, |s| {
            if let Said::Line(l) = s {
                eprintln!("ocrmypdf: {l}");
            }
        });
        let out = r.into_iter().next().unwrap().unwrap();
        eprintln!("made {} ({} bytes) in {:?}", out.display(), std::fs::metadata(&out).unwrap().len(), start.elapsed());
        assert_eq!(out, d.join("scan.searchable.pdf"));
        assert!(!scanned(&out), "its words are in it now");
        // Stopped while it reads a long one: no copy, and its container is gone.
        if let Ok(long) = std::env::var("COXSWAIN_LONG_SCAN") {
            let long_pdf = d.join("long.pdf");
            std::fs::copy(long, &long_pdf).unwrap();
            let stop = std::sync::Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(800));
                flag.store(true, Ordering::Relaxed);
            });
            let r = run_all(&cfg, std::slice::from_ref(&long_pdf), &stop, |_| {}, |_| {});
            assert_eq!(r[0].as_ref().unwrap_err(), &t!("searchable.stopped"));
            assert!(!d.join("long.searchable.pdf").exists(), "no copy");
        }
        // A stop before the start makes nothing.
        let r = run_all(&cfg, std::slice::from_ref(&pdf), &AtomicBool::new(true), |_| {}, |_| {});
        assert!(r.is_empty());
        assert!(!d.join("scan.searchable 2.pdf").exists());
        std::fs::remove_dir_all(d).unwrap();
    }
}

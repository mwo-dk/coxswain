//! *Build PDF of AsciiDoc* and *Export draw.io diagram*, in both apps: Asciidoctor PDF makes a
//! PDF of an AsciiDoc document, draw.io a PNG, SVG or PDF of a diagram, each written next to the
//! original under the first free name; nothing is ever overwritten.
//!
//! Asciidoctor PDF runs as installed (`asciidoctor-pdf`), else from its container image
//! (`[preview.images] asciidoctor`), as a preview tool does: `[preview.tools] asciidoc` and
//! `[preview] prefer` choose. draw.io runs only from its image (`[preview.images] drawio`). A container gets no network, a read-only root and bounded memory;
//! an AsciiDoc document sees its own folder read-only (its includes and pictures), a diagram
//! only a copy of itself. draw.io is opt-in (`[preview] drawio`): its image is large.
//!
//! Nothing comes from the web: Asciidoctor runs in its `server` safe mode without
//! `allow-uri-read` (no remote includes or pictures, and the document cannot turn that on), and
//! draw.io runs only in its container, never installed, its requests to a proxy that answers
//! nothing. Diagrams in an AsciiDoc document are
//! drawn by the same programs as the diagram previews (PlantUML, Graphviz, Kroki's), here, and
//! put into the PDF as pictures. Here too: what the terminal app's F3 shows of an AsciiDoc
//! document (`adoc_text`), and the diagram inside a `.drawio.svg` or `.drawio.png` (`drawio_xml`).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::config::PreviewConfig;
use crate::t;

/// Asciidoctor's name under `[preview.images]` (its kind in `[preview.tools]` is `asciidoc`).
pub const ADOC: &str = "asciidoctor";
/// draw.io's.
pub const DRAWIO: &str = "drawio";

/// The largest result taken: a PDF with many pictures can be big, but not bigger.
const MOST: u64 = 512 << 20;
/// The largest document or diagram read.
const MOST_IN: u64 = 32 << 20;

/// What to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    AdocPdf,
    DrawioPng,
    DrawioSvg,
    DrawioPdf,
}

impl Op {
    /// What *Export draw.io diagram* offers, in its order.
    pub const DRAWIO: [Op; 3] = [Op::DrawioPng, Op::DrawioSvg, Op::DrawioPdf];

    pub fn id(self) -> &'static str {
        match self {
            Op::AdocPdf => "adoc_pdf",
            Op::DrawioPng => "drawio_png",
            Op::DrawioSvg => "drawio_svg",
            Op::DrawioPdf => "drawio_pdf",
        }
    }

    pub fn from_id(id: &str) -> Option<Op> {
        [Op::AdocPdf, Op::DrawioPng, Op::DrawioSvg, Op::DrawioPdf].into_iter().find(|o| o.id() == id)
    }

    /// Its tool: `ADOC` or `DRAWIO`.
    pub fn tool(self) -> &'static str {
        if self == Op::AdocPdf { ADOC } else { DRAWIO }
    }

    /// The ending of what it makes.
    pub fn ext(self) -> &'static str {
        match self {
            Op::DrawioPng => "png",
            Op::DrawioSvg => "svg",
            _ => "pdf",
        }
    }

    /// Its name in a menu: "PNG picture of each page".
    pub fn label(self) -> String {
        crate::i18n::tr(&format!("export.op.{}", self.id()), &[])
    }

    /// Does it take `path`?
    pub fn takes(self, path: &Path) -> bool {
        if self == Op::AdocPdf { is_adoc(path) } else { is_drawio(path) }
    }
}

/// An AsciiDoc document, by its name.
pub fn is_adoc(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("adoc") || e.eq_ignore_ascii_case("asciidoc"))
}

/// A draw.io diagram, by its name: `.drawio`, `.dio`, or a picture with the diagram in it,
/// `.drawio.svg` and `.drawio.png`.
pub fn is_drawio(path: &Path) -> bool {
    let n = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    [".drawio", ".dio", ".drawio.svg", ".drawio.png"].iter().any(|e| n.ends_with(e))
}

/// The name without its diagram ending: `flow` of `flow.drawio.svg`, `guide` of `guide.adoc`.
fn stem(path: &Path) -> String {
    let n = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let lower = n.to_lowercase();
    for e in [".drawio.svg", ".drawio.png", ".drawio", ".dio", ".asciidoc", ".adoc"] {
        if lower.ends_with(e) && n.len() > e.len() {
            return n[..n.len() - e.len()].to_string();
        }
    }
    path.file_stem().unwrap_or_default().to_string_lossy().into_owned()
}

/// Where a result of `file` goes: `flow.png`, or for one page of several `flow - Detail.png`;
/// else `flow 2.png` and on, the first name that is free.
pub fn target(file: &Path, op: Op, page: Option<&str>) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("."));
    let base = match page {
        Some(p) => format!("{} - {}", stem(file), safe_name(p)),
        None => stem(file),
    };
    let ext = op.ext();
    (1..)
        .map(|n| dir.join(if n == 1 { format!("{base}.{ext}") } else { format!("{base} {n}.{ext}") }))
        .find(|p| !p.exists() && p != file)
        .expect("a free name")
}

/// A page's name as part of a file name: no separators, no characters Windows refuses, no
/// control characters, 60 characters at most.
fn safe_name(page: &str) -> String {
    let s: String = page.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).take(60).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "_".into() } else { s }
}

/// How a tool runs here.
#[derive(Clone, Debug, PartialEq)]
pub enum How {
    Local(PathBuf),
    Container { rt: &'static str, path: PathBuf, image: String },
}

/// The way `tool` runs here, or why it cannot: what to install, which setting to change.
pub fn how(cfg: &PreviewConfig, tool: &str) -> Result<How, String> {
    if tool == DRAWIO && !cfg.drawio {
        return Err(t!("export.drawio_off"));
    }
    let pref = crate::preview_tools::asked(cfg, tool);
    let pref = pref.as_str();
    // draw.io only in its container: an installed draw.io, unsealed, could read any file of
    // yours a diagram points at (a picture, a stencil, a link) into what it makes.
    let local = if tool == ADOC { crate::tools::which("asciidoctor-pdf").map(How::Local) } else { None };
    let image = cfg.images.get(tool).filter(|i| !i.is_empty()).cloned();
    let container = match (crate::tools::container_runtime(cfg), &image) {
        (Some((rt, path)), Some(image)) => Some(How::Container { rt, path, image: image.clone() }),
        _ => None,
    };
    let found = match pref {
        _ if tool == DRAWIO => container,
        "container" => container.or(local),
        "local" => local,
        _ => local.or(container),
    };
    found.ok_or_else(|| {
        let install = match (tool == ADOC).then(|| crate::tools::install("asciidoctor-pdf")) {
            Some(Some(line)) => t!("searchable.install_line", "line" => line),
            Some(None) => t!("export.install_adoc"),
            None => String::new(),
        };
        let container = if pref == "local" && tool == ADOC {
            t!("searchable.prefer_local")
        } else if cfg.container == "off" {
            t!("searchable.containers_off")
        } else if image.is_none() {
            t!("convert.no_image", "tool" => tool)
        } else {
            crate::tools::no_runtime()
        };
        let needs = t!(if tool == ADOC { "export.needs_adoc" } else { "export.needs_drawio" });
        [needs, install, container].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
    })
}

/// What a run does, for the question before it: which program, and the first pull with its
/// size when the image is not there yet. The bool: a pull it makes.
pub fn about(how: &How) -> (String, bool) {
    match how {
        How::Local(p) => (t!("export.runs_local", "program" => p.file_name().unwrap_or_default().to_string_lossy()), false),
        How::Container { rt, path, image } => {
            let runs = t!("export.runs_container", "runtime" => *rt, "image" => image.as_str());
            if crate::tools::image_size(path, image).is_some() {
                return (runs, false);
            }
            let pull = match crate::images::download(image) {
                Some(size) => t!("convert.first_run_pulls_large", "image" => image.as_str(), "size" => size),
                None => t!("convert.first_run_pulls", "image" => image.as_str()),
            };
            (format!("{runs} {pull}."), true)
        }
    }
}

/// The question before a run: what it writes, and where.
pub fn confirm(op: Op, files: &[PathBuf]) -> String {
    let name = |p: &Path| p.file_name().unwrap_or_default().to_string_lossy().into_owned();
    match (op, files) {
        (Op::AdocPdf, [one]) => t!("export.confirm_adoc_one", "name" => name(one), "target" => name(&target(one, op, None))),
        (Op::AdocPdf, many) => t!("export.confirm_adoc_many", "n" => many.len()),
        (_, [one]) => t!("export.confirm_drawio_one", "name" => name(one), "format" => op.label()),
        (_, many) => t!("export.confirm_drawio_many", "n" => many.len(), "format" => op.label()),
    }
}

/// What a run says while it goes.
pub enum Said<'a> {
    /// The image is being pulled: its progress line.
    Pulling(&'a str),
    /// A line the program wrote.
    Line(&'a str),
}

/// Make `op` of each of `files`, one after the other: `at(i)` as each starts, `said` with what
/// it says. `stop` ends the run under way, and the rest are not started; what a stopped run
/// made is removed. One result per file started: what was written, or why not.
pub fn run_all(
    cfg: &PreviewConfig,
    op: Op,
    files: &[PathBuf],
    stop: &AtomicBool,
    mut at: impl FnMut(usize),
    mut said: impl FnMut(Said),
) -> Vec<Result<Vec<PathBuf>, String>> {
    let how = match how(cfg, op.tool()) {
        Ok(h) => h,
        Err(e) => return vec![Err(e)],
    };
    let mut done = vec![];
    for (i, f) in files.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        at(i);
        done.push(run(cfg, &how, op, f, stop, &mut said));
    }
    done
}

static N: AtomicUsize = AtomicUsize::new(0);

/// One file. A folder that cannot be written to says so before the program starts (a probe
/// file, removed at once); each result's name is taken only when it is written, with
/// `create_new`, so nothing is overwritten and a stopped run leaves nothing.
fn run(cfg: &PreviewConfig, how: &How, op: Op, file: &Path, stop: &AtomicBool, said: &mut impl FnMut(Said)) -> Result<Vec<PathBuf>, String> {
    let probe = target(file, op, None);
    std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(|e| format!("{}: {e}", probe.display()))?;
    let _ = std::fs::remove_file(&probe);
    if let How::Container { path, image, .. } = how {
        pull(path, image, stop, said)?;
    }
    let scratch = Scratch::new()?;
    // What was made, in the scratch folder, and the page it is of (None: the whole file).
    let made: Vec<(PathBuf, Option<String>)> = if op == Op::AdocPdf {
        let doc = adoc_for_pdf(cfg, &read_most(file)?);
        let out = scratch.0.join("out.pdf");
        go(adoc_command(how, file, &scratch.0)?, how, Some(doc.into_bytes()), Some(&out), stop, said)?;
        vec![(out, None)]
    } else {
        // draw.io gets the diagram as read here, a plain `.drawio`, in a folder of its own.
        let xml = drawio_xml(file);
        let input = scratch.0.join("in").join(if xml.is_some() { "in.drawio".to_string() } else { format!("in{}", drawio_ending(file)) });
        std::fs::create_dir_all(input.parent().unwrap()).map_err(|e| e.to_string())?;
        match &xml {
            Some(x) => std::fs::write(&input, x).map_err(|e| e.to_string())?,
            None => std::fs::copy(file, &input).map(drop).map_err(|e| format!("{}: {e}", file.display()))?,
        }
        let out = scratch.0.join("out");
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        let pages = xml.as_deref().map(pages).unwrap_or_default();
        let ext = op.ext();
        if op == Op::DrawioPdf || pages.len() <= 1 {
            let to = out.join(format!("all.{ext}"));
            let mut args = vec!["-x".to_string(), "-f".into(), ext.into(), "-o".into()];
            args.push(at_out(how, &to, &out));
            if op == Op::DrawioPdf {
                args.push("-a".into());
            }
            go(drawio_command(how, &input, &out, &args)?, how, None, None, stop, said)?;
            vec![(to, None)]
        } else {
            let mut v = vec![];
            for (i, name) in pages.iter().enumerate() {
                let to = out.join(format!("p{}.{ext}", i + 1));
                let args = vec!["-x".to_string(), "-f".into(), ext.into(), "--page-index".into(), (i + 1).to_string(), "-o".into(), at_out(how, &to, &out)];
                go(drawio_command(how, &input, &out, &args)?, how, None, None, stop, said)?;
                v.push((to, Some(name.clone())));
            }
            v
        }
    };
    // Every result there and within bounds before any is written next to the file.
    for (m, _) in &made {
        let len = std::fs::metadata(m).map(|m| m.len()).map_err(|_| t!("convert.no_result"))?;
        if len == 0 || len > MOST {
            return Err(t!("convert.no_result"));
        }
    }
    let mut written: Vec<PathBuf> = vec![];
    for (m, page) in made {
        match place(&m, file, op, page.as_deref()) {
            Ok(to) => written.push(to),
            Err(e) => {
                for w in written {
                    let _ = std::fs::remove_file(w);
                }
                return Err(e);
            }
        }
    }
    Ok(written)
}

/// `made` copied next to `file` under the first free name, taken with create_new: a file made
/// meanwhile is never written over; a copy that fails halfway is removed.
fn place(made: &Path, file: &Path, op: Op, page: Option<&str>) -> Result<PathBuf, String> {
    let (to, mut f) = (0..100)
        .map(|_| target(file, op, page))
        .find_map(|to| std::fs::OpenOptions::new().write(true).create_new(true).open(&to).ok().map(|f| (to, f)))
        .ok_or_else(|| format!("{}: {}", file.display(), t!("err.no_file_name")))?;
    let copied = std::fs::File::open(made).and_then(|mut m| std::io::copy(&mut m, &mut f));
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&to);
        return Err(format!("{}: {e}", to.display()));
    }
    Ok(to)
}

/// The file's text, up to `MOST_IN`.
fn read_most(file: &Path) -> Result<String, String> {
    let mut b = vec![];
    std::fs::File::open(file).and_then(|f| f.take(MOST_IN).read_to_end(&mut b)).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

/// The image pulled, when it is not there yet; `stop` ends the pull.
fn pull(rt: &Path, image: &str, stop: &AtomicBool, said: &mut impl FnMut(Said)) -> Result<(), String> {
    if crate::tools::image_size(rt, image).is_some() {
        return Ok(());
    }
    std::thread::scope(|scope| {
        let puller = scope.spawn(|| crate::images::pull_until(rt, image, stop));
        while !puller.is_finished() {
            if let Some(line) = crate::images::pulling().get(image) {
                said(Said::Pulling(line));
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        puller.join().map_err(|_| t!("convert.failed", "status" => "pull"))?
    })
}


fn container_name() -> String {
    format!("coxswain-export-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

/// Asciidoctor PDF on the document, read from standard input, writing the PDF to standard
/// output: in its `server` safe mode its includes and pictures come only from the document's
/// folder (`-B`), and `allow-uri-read` stays off, so nothing comes from the web.
fn adoc_command(how: &How, file: &Path, scratch: &Path) -> Result<(Command, Option<String>), String> {
    let dir = file.parent().ok_or_else(|| t!("err.no_folder"))?;
    // Installed, it reads a copy of the folder's own files: a link in it could name a file
    // anywhere, which Asciidoctor's jail (by path, not by what a link points at) would let in.
    // The container sees only the folder, where such a link leads nowhere.
    let base = scratch.join("doc");
    if matches!(how, How::Local(_)) {
        copy_plain(dir, &base)?;
    }
    let dir = if matches!(how, How::Local(_)) { base.as_path() } else { dir };
    // `allow-uri-read` stays unset (no remote includes or pictures): in `server` safe mode the
    // document cannot set it.
    let args = ["--safe-mode", "server", "-B"];
    Ok(match how {
        How::Local(program) => {
            let mut c = crate::tools::low(program);
            c.current_dir(dir).args(args).arg(dir).args(["-o", "-", "-"]);
            (c, None)
        }
        How::Container { rt, path, image } => {
            let name = container_name();
            let mut c = crate::images::sealed(rt, path, &name, dir, None);
            c.args(["-w", "/src", image.as_str(), "asciidoctor-pdf"]).args(args).args(["/src", "-o", "-", "-"]);
            (c, Some(name))
        }
    })
}

/// The regular files of `from` and its folders, copied to `to`: no links, no hidden files or
/// folders, four levels deep, 2000 files and 256 MB at most (past that the rest is left out:
/// an include of it is not found).
fn copy_plain(from: &Path, to: &Path) -> Result<(), String> {
    let (mut files, mut bytes) = (0usize, 0u64);
    let mut stack = vec![(from.to_path_buf(), to.to_path_buf(), 0)];
    while let Some((src, dst, depth)) = stack.pop() {
        std::fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
        for e in std::fs::read_dir(&src).into_iter().flatten().flatten() {
            let name = e.file_name();
            // The entry itself, not what a link points at.
            let Ok(kind) = e.file_type() else { continue };
            if name.to_string_lossy().starts_with('.') || kind.is_symlink() {
                continue;
            }
            if kind.is_dir() && depth < 4 {
                stack.push((e.path(), dst.join(&name), depth + 1));
            } else if kind.is_file() {
                let len = e.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
                if files >= 2000 || bytes.saturating_add(len) > 256 << 20 {
                    continue;
                }
                if std::fs::copy(e.path(), dst.join(&name)).is_ok() {
                    files += 1;
                    bytes += len;
                }
            }
        }
    }
    Ok(())
}

/// The ending a diagram keeps in its copy: draw.io reads the kind of file by it.
fn drawio_ending(file: &Path) -> &'static str {
    let n = file.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    if n.ends_with(".drawio.svg") {
        ".drawio.svg"
    } else if n.ends_with(".drawio.png") {
        ".drawio.png"
    } else {
        ".drawio"
    }
}

/// `to`, in `out`, as the program sees it: in a container `out` is /out.
fn at_out(how: &How, to: &Path, out: &Path) -> String {
    match how {
        How::Local(_) => to.display().to_string(),
        How::Container { .. } => format!("/out/{}", to.strip_prefix(out).unwrap_or(to).to_string_lossy()),
    }
}

/// draw.io exporting `input` with `args`. Its own web requests go to a proxy that answers
/// nothing: a diagram's pictures from the web stay out. In the container it gets a display of
/// its own (Xvfb) and sees only the copy of the diagram, at /src.
fn drawio_command(how: &How, input: &Path, out: &Path, args: &[String]) -> Result<(Command, Option<String>), String> {
    const OFFLINE: [&str; 3] = ["--proxy-server=http://127.0.0.1:9", "--disable-gpu", "--disable-update"];
    Ok(match how {
        // Never installed (`how`).
        How::Local(_) => return Err(t!("export.needs_drawio")),
        How::Container { rt, path, image } => {
            let src = input.parent().ok_or_else(|| t!("err.no_folder"))?;
            let name = container_name();
            let mut c = crate::images::sealed(rt, path, &name, src, Some(out));
            // The arguments reach draw.io as the script's own ("$@"), never as part of its text.
            let script = "Xvfb :42 -nolisten unix >/dev/null 2>&1 & export DISPLAY=:42; exec /opt/drawio/drawio --no-sandbox \"$@\"";
            // Its settings go to a home in the container's /tmp: the root is read-only.
            c.args(["--entrypoint", "sh", image.as_str(), "-c", script, "sh"]).args(OFFLINE).args(args);
            c.arg(format!("/src/{}", input.file_name().unwrap_or_default().to_string_lossy()));
            (c, Some(name))
        }
    })
}

/// Run `c` to its end: `stdin` written to it, its standard output into `out` (bounded), its
/// lines said as they come; `stop` kills it (and its container).
fn go((c, name): (Command, Option<String>), how: &How, stdin: Option<Vec<u8>>, out: Option<&Path>, stop: &AtomicBool, said: &mut impl FnMut(Said)) -> Result<(), String> {
    let mut c = crate::tools::hosted(c);
    c.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(if out.is_some() { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| e.to_string())?;
    if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = pipe.write_all(&data);
        });
    }
    let writer = match (out, child.stdout.take()) {
        (Some(out), Some(pipe)) => {
            let out = out.to_path_buf();
            Some(std::thread::spawn(move || -> std::io::Result<bool> {
                let mut f = std::fs::File::create(&out)?;
                let n = std::io::copy(&mut pipe.take(MOST + 1), &mut f)?;
                Ok(n <= MOST)
            }))
        }
        _ => None,
    };
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
            kill(&mut child, how, name.as_deref());
            return Err(t!("export.stopped"));
        }
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    while let Ok(l) = lines.try_recv() {
        last.push(l);
    }
    let within = writer.map(|w| w.join().ok().and_then(Result::ok)).unwrap_or(Some(true));
    if !status.success() {
        let tail = last.iter().rev().take(6).rev().cloned().collect::<Vec<_>>().join("\n");
        return Err(if tail.is_empty() { t!("convert.failed", "status" => status) } else { tail });
    }
    match within {
        Some(true) => Ok(()),
        _ => Err(t!("convert.no_result")),
    }
}

/// `child` killed, and its container, by name: killing podman's client leaves it running.
fn kill(child: &mut Child, how: &How, name: Option<&str>) {
    let _ = child.kill();
    if let (How::Container { path, .. }, Some(name)) = (how, name) {
        let _ = crate::tools::command(path).args(["kill", name]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = child.wait();
}

/// The lines `child` writes to its standard error, as they come.
fn follow(child: &mut Child) -> mpsc::Receiver<String> {
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
                    let line = plain(&acc[..i]);
                    acc.drain(..=i);
                    if !line.is_empty() && tx.send(line).is_err() {
                        return;
                    }
                }
            }
            if !plain(&acc).is_empty() {
                let _ = tx.send(plain(&acc));
            }
        });
    }
    rx
}

/// A line a program wrote, without its terminal escapes (colours, cursor moves) and other
/// control characters: it is shown on the status line and in dialogs.
fn plain(line: &str) -> String {
    use std::sync::OnceLock;
    static ESC: OnceLock<regex::Regex> = OnceLock::new();
    let esc = ESC.get_or_init(|| regex::Regex::new(r"\x1b(\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(\x07|\x1b\\)?|.)").expect("a pattern"));
    esc.replace_all(line, "").chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

/// A folder of its own in the cache for one run, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Scratch, String> {
        let base = crate::dirs::cache_dir().ok_or_else(|| t!("err.no_cache_folder"))?.join("coxswain").join("export");
        // What a run of an app that was killed left behind, a day on.
        for e in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            if e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|a| a > Duration::from_secs(24 * 3600))) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
        let dir = base.join(format!("{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ------------------------------------------------------------ draw.io

/// The diagram of a draw.io file as draw.io's own XML (`<mxfile>`): the file itself, or the
/// diagram a `.drawio.svg` keeps in its `content` attribute, or a `.drawio.png` in a text chunk.
/// `None` for a file without one.
pub fn drawio_xml(path: &Path) -> Option<String> {
    let mut b = vec![];
    std::fs::File::open(path).ok()?.take(MOST_IN).read_to_end(&mut b).ok()?;
    let n = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    if n.ends_with(".png") {
        return png_diagram(&b);
    }
    let text = String::from_utf8_lossy(&b);
    if n.ends_with(".svg") {
        return svg_diagram(&text);
    }
    (text.contains("<mxfile") || text.contains("<mxGraphModel")).then(|| text.into_owned())
}

/// The diagram in an SVG draw.io wrote: its root's `content`, XML as it is, or (older files)
/// base64 of deflated, percent-encoded XML.
fn svg_diagram(svg: &str) -> Option<String> {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(svg);
    let root = loop {
        match r.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) => break e,
            Event::Eof | Event::DocType(_) => return None,
            _ => {}
        }
    };
    if root.local_name().as_ref() != "svg" {
        return None;
    }
    // One `content` only: a second (or a duplicate attribute) is refused, not chosen between.
    let mut found = None;
    for a in root.attributes().with_checks(true) {
        let a = a.ok()?;
        if a.key.local_name().as_ref() == "content" {
            if found.is_some() {
                return None;
            }
            found = Some(a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.into_owned());
        }
    }
    let xml = found?;
    if xml.trim_start().starts_with('<') {
        return Some(xml);
    }
    let packed = base64::engine::general_purpose::STANDARD.decode(xml.trim()).ok()?;
    let mut out = String::new();
    flate2::read::DeflateDecoder::new(&packed[..]).take(MOST_IN).read_to_string(&mut out).ok()?;
    Some(percent_decode(&out)).filter(|x| x.trim_start().starts_with('<'))
}

/// The diagram in a PNG draw.io wrote: a `tEXt`, `zTXt` or `iTXt` chunk named `mxfile` (or
/// `mxGraphModel`), percent-encoded XML.
fn png_diagram(png: &[u8]) -> Option<String> {
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let (mut i, mut found) = (8, None);
    while i + 12 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().ok()?) as usize;
        let kind = &png[i + 4..i + 8];
        let data = png.get(i + 8..i + 8 + len)?;
        i += 12 + len;
        let Some(nul) = data.iter().position(|&b| b == 0) else { continue };
        if !matches!(&data[..nul], b"mxfile" | b"mxGraphModel") {
            continue;
        }
        let rest = &data[nul + 1..];
        let inflate = |z: &[u8]| {
            let mut v = vec![];
            flate2::read::ZlibDecoder::new(z).take(MOST_IN).read_to_end(&mut v).ok().map(|_| v)
        };
        let raw = match kind {
            b"tEXt" => Some(rest.to_vec()),
            b"zTXt" => rest.get(1..).and_then(inflate),
            b"iTXt" => {
                let (flag, rest) = (*rest.first()?, rest.get(2..)?);
                // The language and the translated keyword, each ended by a zero.
                let mut parts = rest.splitn(3, |&b| b == 0);
                let (_, _, text) = (parts.next()?, parts.next()?, parts.next()?);
                if flag == 1 { inflate(text) } else { Some(text.to_vec()) }
            }
            _ => None,
        }?;
        let xml = percent_decode(&String::from_utf8_lossy(&raw));
        // A second diagram chunk is refused, not chosen between.
        if found.is_some() || !xml.trim_start().starts_with('<') {
            return None;
        }
        found = Some(xml);
    }
    found
}

/// `%3C` as `<`, as JavaScript's decodeURIComponent; anything not encoded stays.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The names of a diagram's pages, in order ("Page 2" for one without); one for a bare
/// `<mxGraphModel>`.
pub fn pages(xml: &str) -> Vec<String> {
    let re = regex::Regex::new(r"<diagram\b([^>]*)>").expect("a pattern");
    let name = regex::Regex::new(r#"\bname\s*=\s*"([^"]*)""#).expect("a pattern");
    let v: Vec<String> = re
        .captures_iter(xml)
        .enumerate()
        .map(|(i, c)| {
            name.captures(&c[1])
                .and_then(|n| quick_xml::escape::unescape(&n[1]).ok().map(|s| s.into_owned()))
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| t!("export.page", "n" => i + 1))
        })
        .collect();
    if v.is_empty() && xml.contains("<mxGraphModel") { vec![t!("export.page", "n" => 1)] } else { v }
}

/// One at a time: each draw.io run starts a browser.
static DRAWING: Mutex<()> = Mutex::new(());

/// The first page of `path` as a PNG `width` pixels wide, at `to`, for a thumbnail: only when
/// draw.io exports are on and draw.io runs without a pull. `timeout` bounds it.
pub fn drawio_thumbnail(cfg: &PreviewConfig, path: &Path, to: &Path, width: u32, timeout: Duration) -> Result<(), String> {
    // Only in the container: a thumbnail is made without being asked for, and the installed
    // draw.io could read files a diagram names (`file:` pictures) into it.
    let image = cfg.images.get(DRAWIO).filter(|i| !i.is_empty()).ok_or_else(|| t!("convert.no_container_image", "tool" => DRAWIO))?;
    let (rt, rt_path) = crate::tools::container_runtime(cfg).ok_or_else(crate::tools::no_runtime)?;
    if !cfg.drawio || crate::tools::image_size(&rt_path, image).is_none() {
        return Err(t!("convert.first_run_pulls", "image" => image.as_str()));
    }
    let how = How::Container { rt, path: rt_path, image: image.clone() };
    let _one = DRAWING.lock().map_err(|e| e.to_string())?;
    let scratch = Scratch::new()?;
    let input = scratch.0.join("in").join(format!("in{}", drawio_ending(path)));
    std::fs::create_dir_all(input.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::copy(path, &input).map_err(|e| e.to_string())?;
    let out = scratch.0.join("out");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let png = out.join("thumb.png");
    let args = ["-x", "-f", "png", "--page-index", "1", "--width", &width.to_string(), "-o", &at_out(&how, &png, &out)].map(String::from);
    let (c, name) = drawio_command(&how, &input, &out, &args)?;
    let o = crate::tools::capture(crate::tools::hosted(c), timeout, 64 << 10);
    if let (How::Container { path, .. }, Some(name), Err(_)) = (&how, &name, &o) {
        // Past its time: the container goes too.
        let _ = crate::tools::command(path).args(["kill", name]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let o = o.map_err(|e| e.to_string())?;
    let len = std::fs::metadata(&png).map(|m| m.len()).unwrap_or(0);
    if !o.status.success() || len == 0 || len > 16 << 20 {
        return Err(t!("convert.no_result"));
    }
    std::fs::copy(&png, to).map(|_| ()).map_err(|e| e.to_string())
}

// ------------------------------------------------------------ AsciiDoc

/// A diagram in an AsciiDoc document: a listing or literal block whose style names a kind of
/// diagram (`[plantuml]` over `----`), by its lines.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Block {
    /// The diagram kind: `plantuml`, `graphviz`, `mermaid`, `d2`, … (`DIAGRAMS`).
    pub kind: String,
    /// Its source, between the delimiters.
    pub text: String,
    /// Its lines: the style line to the closing delimiter, counted from 0.
    first: usize,
    last: usize,
}

/// The diagram kinds a block's style may name: Mermaid and Graphviz, which the desktop app
/// draws itself (Graphviz's `dot` here for a PDF), PlantUML, and Kroki's kinds.
pub const DIAGRAMS: &[&str] = &[
    "mermaid", "graphviz", "dot", "plantuml", "d2", "ditaa", "svgbob", "pikchr", "nomnoml", "wavedrom", "vega", "vegalite", "dbml", "erd", "bytefield",
];

/// The diagram blocks of an AsciiDoc document, in order.
pub fn blocks(src: &str) -> Vec<Block> {
    let lines: Vec<&str> = src.lines().collect();
    let mut v = vec![];
    let mut i = 0;
    while i < lines.len() {
        let Some(kind) = style(lines[i]) else {
            i += 1;
            continue;
        };
        // Block titles (`.Fuel flow`) may stand between the style and the block.
        let mut j = i + 1;
        while j < lines.len() && is_title(lines[j]) {
            j += 1;
        }
        let Some(delim) = lines.get(j).map(|l| l.trim_end()).filter(|l| is_delimiter(l)) else {
            i += 1;
            continue;
        };
        let Some(end) = (j + 1..lines.len()).find(|&k| lines[k].trim_end() == delim) else {
            i += 1;
            continue;
        };
        v.push(Block { kind, text: lines[j + 1..end].join("\n"), first: i, last: end });
        i = end + 1;
    }
    v
}

/// The diagram kind a block attribute line names as its style: `[plantuml, target=x]`,
/// `[d2#launch.wide]`.
fn style(line: &str) -> Option<String> {
    let inner = line.trim_end().strip_prefix('[')?.strip_suffix(']')?;
    if inner.starts_with('[') {
        return None;
    }
    let first = inner.split(',').next()?.trim();
    let kind = first.split(['#', '.', '%']).next()?.trim().to_lowercase();
    DIAGRAMS.contains(&kind.as_str()).then_some(kind)
}

fn is_title(line: &str) -> bool {
    line.starts_with('.') && line.chars().nth(1).is_some_and(|c| !c.is_whitespace() && c != '.')
}

/// `----` or `....`, four or more.
fn is_delimiter(line: &str) -> bool {
    line.len() >= 4 && (line.chars().all(|c| c == '-') || line.chars().all(|c| c == '.'))
}

/// The document with a placeholder for each diagram block, for the desktop app's preview,
/// which draws into it (`<div class="cx-diagram" data-n="0">`, a passthrough block), and the
/// blocks.
pub fn with_placeholders(src: &str) -> (String, Vec<Block>) {
    let found = blocks(src);
    let lines: Vec<&str> = src.lines().collect();
    let mut out: Vec<String> = vec![];
    let mut next = 0;
    for (i, b) in found.iter().enumerate() {
        out.extend(lines[next..b.first].iter().map(|l| l.to_string()));
        out.extend(["++++".to_string(), format!("<div class=\"cx-diagram\" data-n=\"{i}\"></div>"), "++++".to_string()]);
        next = b.last + 1;
    }
    out.extend(lines[next..].iter().map(|l| l.to_string()));
    (if found.is_empty() { src.to_string() } else { out.join("\n") }, found)
}

/// The document with each diagram it can draw here as a picture in it (an image with an SVG
/// data URI), for Asciidoctor PDF; a diagram it cannot draw stays as its source.
fn adoc_for_pdf(cfg: &PreviewConfig, src: &str) -> String {
    let found = blocks(src);
    if found.is_empty() {
        return src.to_string();
    }
    let lines: Vec<&str> = src.lines().collect();
    let mut out: Vec<String> = vec![];
    let mut next = 0;
    for b in found {
        out.extend(lines[next..b.first].iter().map(|l| l.to_string()));
        match diagram_svg(cfg, &b.kind, &b.text) {
            Ok(svg) => {
                // Its titles stay, over the picture.
                out.extend(lines[b.first + 1..b.last].iter().take_while(|l| is_title(l)).map(|l| l.to_string()));
                out.push(format!("image::data:image/svg+xml;base64,{}[{}]", base64::engine::general_purpose::STANDARD.encode(svg), b.kind));
            }
            Err(_) => out.extend(lines[b.first..=b.last].iter().map(|l| l.to_string())),
        }
        next = b.last + 1;
    }
    out.extend(lines[next..].iter().map(|l| l.to_string()));
    out.join("\n")
}

/// How Kroki's image draws each of its kinds (`images::KROKI`): the program and its arguments,
/// reading the diagram on standard input and writing SVG. `None` for any other kind.
pub fn kroki(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        "d2" => &["d2", "-", "-"],
        "ditaa" => &["ditaa", "--svg", "-", "-"],
        "svgbob" => &["svgbob"],
        "pikchr" => &["pikchr", "--svg-only", "-"],
        "nomnoml" => &["nomnoml"],
        "wavedrom" => &["wavedrom"],
        "vega" => &["vega", "--spec-format=default"],
        "vegalite" => &["vega", "--spec-format=lite"],
        "dbml" => &["dbml"],
        "erd" => &["erd", "-f", "svg"],
        "bytefield" => &["bytefield"],
        _ => return None,
    })
}

/// A diagram as SVG, drawn here in its container image (PlantUML's, else Kroki's) when that is
/// pulled already (never pulled for this), without network; never by an installed program,
/// which could read what the diagram names. Mermaid has no program here: the desktop app draws
/// it itself.
pub fn diagram_svg(cfg: &PreviewConfig, kind: &str, text: &str) -> Result<String, String> {
    let kind = if kind == "dot" { "graphviz" } else { kind };
    let image_key = if kind == "plantuml" { "plantuml" } else { "kroki" };
    // Kroki's image has Graphviz's `dot` too.
    let argv: &[&str] = match kind {
        "plantuml" => &["plantuml", "-tsvg", "-pipe"],
        "graphviz" => &["dot", "-Tsvg"],
        k => kroki(k).ok_or_else(|| t!("convert.unknown_engine"))?,
    };
    let timeout = Duration::from_secs(cfg.timeout.max(10));
    // Only in the container, without network: a diagram in a document someone sent may name
    // files or addresses (a D2 import, a Vega data URL), which an installed program would read.
    let image = cfg.images.get(image_key).filter(|i| !i.is_empty()).ok_or_else(|| t!("convert.no_container_image", "tool" => image_key))?;
    let (rt, path) = crate::tools::container_runtime(cfg).ok_or_else(crate::tools::no_runtime)?;
    if crate::tools::image_size(&path, image).is_none() {
        return Err(t!("convert.first_run_pulls", "image" => image.as_str()));
    }
    let empty = Scratch::new()?;
    let mut c = crate::images::sealed(rt, &path, &container_name(), &empty.0, None);
    if kind == "plantuml" {
        c.args(["-e", "PLANTUML_SECURITY_PROFILE=SANDBOX", image.as_str()]).args(&argv[1..]);
    } else {
        c.args(["--entrypoint", argv[0], image.as_str()]).args(&argv[1..]);
    }
    let svg = feed(c, text.as_bytes(), timeout)?;
    let svg = String::from_utf8_lossy(&svg).into_owned();
    if !svg.contains("<svg") {
        return Err(t!("convert.no_result"));
    }
    Ok(svg)
}

/// `c` given `input` on its standard input; its output (8 MB at most) when it ends well
/// within `timeout`.
fn feed(c: Command, input: &[u8], timeout: Duration) -> Result<Vec<u8>, String> {
    let mut c = crate::tools::hosted(c);
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let data = input.to_vec();
    if let Some(mut pipe) = child.stdin.take() {
        std::thread::spawn(move || {
            let _ = pipe.write_all(&data);
        });
    }
    let out = child.stdout.take().map(|o| std::thread::spawn(move || {
        let mut v = vec![];
        let _ = o.take(8 << 20).read_to_end(&mut v);
        v
    }));
    let err = child.stderr.take().map(|e| std::thread::spawn(move || {
        let mut v = vec![];
        let _ = e.take(64 << 10).read_to_end(&mut v);
        v
    }));
    let status = crate::tools::wait(&mut child, timeout).map_err(|e| e.to_string())?;
    let out = out.and_then(|t| t.join().ok()).unwrap_or_default();
    let err = err.and_then(|t| t.join().ok()).unwrap_or_default();
    if status.success() {
        Ok(out)
    } else {
        let e = String::from_utf8_lossy(&err);
        Err(e.lines().rev().map(plain).find(|l| !l.is_empty()).unwrap_or_else(|| t!("convert.failed", "status" => status)))
    }
}

/// An AsciiDoc document as readable text, for the terminal app's F3: headings underlined,
/// lists with bullets, tables as rows of cells, inline marks and attribute lines gone, links
/// as their text. Block contents (code, diagrams) are kept as they are, indented.
pub fn adoc_text(src: &str) -> String {
    let mut attrs: std::collections::HashMap<String, String> = Default::default();
    let mut out = String::new();
    let mut verbatim: Option<String> = None;
    let mut comment = false;
    let mut counters = [0usize; 6];
    for raw in src.lines() {
        let line = raw.trim_end();
        if let Some(delim) = &verbatim {
            if line == delim {
                verbatim = None;
            } else {
                out += &format!("    {line}\n");
            }
            continue;
        }
        if line.len() >= 4 && line.chars().all(|c| c == '/') {
            comment = !comment;
            continue;
        }
        if comment || (line.starts_with("//") && !line.starts_with("///")) {
            continue;
        }
        if is_delimiter(line) || (line.len() >= 4 && line.chars().all(|c| c == '+')) {
            verbatim = Some(line.to_string());
            continue;
        }
        // Other delimiters (examples, sidebars, quotes, open blocks) and the table's.
        if line == "--" || (line.len() >= 4 && (line.chars().all(|c| c == '=') || line.chars().all(|c| c == '*') || line.chars().all(|c| c == '_'))) || line.starts_with("|===") {
            continue;
        }
        if let Some(rest) = line.strip_prefix(':')
            && let Some((name, value)) = rest.split_once(": ").or_else(|| rest.strip_suffix(':').map(|n| (n, "")))
            && !name.contains(' ')
        {
            attrs.insert(name.trim_end_matches('!').to_string(), value.to_string());
            continue;
        }
        if (line.starts_with('[') && line.ends_with(']')) || line.starts_with("[[") {
            continue;
        }
        if line.is_empty() {
            counters = [0; 6];
            if !out.ends_with("\n\n") && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        let heading = line.find(|c| c != '=' && c != '#').filter(|&n| n > 0 && n <= 6 && line[n..].starts_with(' '));
        if let Some(level) = heading {
            let title = inline(line[level..].trim(), &attrs);
            let w = title.chars().count();
            out += &format!("{title}\n");
            match level {
                1 => out += &format!("{}\n", "=".repeat(w)),
                2 => out += &format!("{}\n", "-".repeat(w)),
                _ => {}
            }
            continue;
        }
        if is_title(line) {
            out += &format!("{}\n", inline(&line[1..], &attrs));
            continue;
        }
        if let Some(img) = line.strip_prefix("image::") {
            let (target, alt) = img.split_once('[').unwrap_or((img, ""));
            let alt = alt.trim_end_matches(']').split(',').next().unwrap_or("").trim();
            out += &format!("[{}]\n", if alt.is_empty() { target } else { alt });
            continue;
        }
        let marks = line.chars().take_while(|&c| c == '*' || c == '-' || c == '.').count();
        if marks > 0 && line[marks..].starts_with(' ') && !line.starts_with("...") {
            let depth = (if line.starts_with('-') { 1 } else { marks }).min(6);
            let text = inline(line[marks..].trim(), &attrs);
            let pad = "  ".repeat(depth - 1);
            if line.starts_with('.') {
                counters[depth - 1] += 1;
                out += &format!("{pad}{}. {text}\n", counters[depth - 1]);
            } else {
                out += &format!("{pad}• {text}\n");
            }
            continue;
        }
        if let Some(cells) = line.strip_prefix('|') {
            let cells: Vec<String> = cells.split('|').map(|c| inline(c.trim(), &attrs)).collect();
            out += &format!("{}\n", cells.join(" │ "));
            continue;
        }
        let text = inline(line.strip_suffix(" +").unwrap_or(line), &attrs);
        out += &format!("{text}\n");
    }
    out
}

/// A line's inline marks taken out: `*bold*`, `_italic_`, `` `code` ``, `#marked#`, links and
/// cross references as their text, `{attribute}` as its value.
fn inline(s: &str, attrs: &std::collections::HashMap<String, String>) -> String {
    use std::sync::OnceLock;
    static RES: OnceLock<Vec<(regex::Regex, &'static str)>> = OnceLock::new();
    let res = RES.get_or_init(|| {
        [
            (r"<<[^,>]*,\s*([^>]+)>>", "$1"),
            (r"<<([^>]+)>>", "$1"),
            (r"xref:[^\[\s]*\[([^\]]*)\]", "$1"),
            (r"link:([^\[\s]+)\[\]", "$1"),
            (r"link:([^\[\s]+)\[([^\]]+)\]", "$2 ($1)"),
            (r"((?:https?|ftp|mailto)://?[^\[\s]+)\[\]", "$1"),
            (r"((?:https?|ftp|mailto)://?[^\[\s]+)\[([^\]]+)\]", "$2 ($1)"),
            (r"footnote:[^\[]*\[([^\]]*)\]", " ($1)"),
            (r"(?:kbd|btn|pass):\[([^\]]*)\]", "$1"),
            (r"menu:([^\[]+)\[([^\]]*)\]", "$1 > $2"),
            (r"image:[^\[\s]+\[([^\],]*)[^\]]*\]", "$1"),
            (r"\*\*([^*]+)\*\*", "$1"),
            (r"__([^_]+)__", "$1"),
            (r"``([^`]+)``", "$1"),
            (r"##([^#]+)##", "$1"),
            (r"(^|[^\w*])\*([^*\s][^*]*?)\*($|[^\w*])", "$1$2$3"),
            (r"(^|[^\w_])_([^_\s][^_]*?)_($|[^\w_])", "$1$2$3"),
            (r"`\+?([^`]+?)\+?`", "$1"),
            (r"(^|[^\w#])#([^#\s][^#]*?)#($|[^\w#])", "$1$2$3"),
            (r"\[[.#][^\]]*\]#([^#]+)#", "$1"),
        ]
        .into_iter()
        .map(|(p, r)| (regex::Regex::new(p).expect("a pattern"), r))
        .collect()
    });
    let mut s = s.to_string();
    for (re, with) in res {
        s = re.replace_all(&s, *with).into_owned();
    }
    static ATTR: OnceLock<regex::Regex> = OnceLock::new();
    let attr = ATTR.get_or_init(|| regex::Regex::new(r"\{([\w-]+)\}").expect("a pattern"));
    attr.replace_all(&s, |c: &regex::Captures| attrs.get(&c[1]).cloned().unwrap_or_else(|| c[0].to_string())).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-export-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn export_names_never_take_one_in_use_nor_the_original() {
        let d = folder("names");
        let f = d.join("flow.drawio.png");
        assert!(is_drawio(&f) && is_drawio(&d.join("a.DIO")) && !is_drawio(&d.join("a.png")));
        assert!(is_adoc(&d.join("Guide.AsciiDoc")) && !is_adoc(&d.join("a.md")));
        assert_eq!(target(&f, Op::DrawioSvg, None), d.join("flow.svg"));
        std::fs::write(d.join("flow.png"), "").unwrap();
        assert_eq!(target(&f, Op::DrawioPng, None), d.join("flow 2.png"));
        assert_eq!(target(&f, Op::DrawioPng, Some("Fuel/Engine: v2")), d.join("flow - Fuel_Engine_ v2.png"));
        assert_eq!(target(&d.join("guide.adoc"), Op::AdocPdf, None), d.join("guide.pdf"));
        // `x.png` exported as PNG would be itself: the next name.
        let png = d.join("x.png");
        assert_eq!(target(&png, Op::DrawioPng, None), d.join("x 2.png"));
        assert_eq!(Op::from_id("drawio_pdf"), Some(Op::DrawioPdf));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn export_reads_the_diagram_inside_svg_and_png() {
        let xml = r#"<mxfile><diagram id="a" name="Overview &amp; more"><mxGraphModel><root/></mxGraphModel></diagram><diagram id="b"><mxGraphModel/></diagram></mxfile>"#;
        let esc = quick_xml::escape::escape(xml);
        let svg = format!(r#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" content="{esc}" width="10"><rect/></svg>"#);
        assert_eq!(svg_diagram(&svg).as_deref(), Some(xml));
        assert_eq!(pages(xml), ["Overview & more".to_string(), t!("export.page", "n" => 2)]);
        // A PNG with the diagram, percent-encoded, deflated in a zTXt chunk.
        let encoded: String = xml.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
        let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
        z.write_all(encoded.as_bytes()).unwrap();
        let chunk = |kind: &[u8], data: &[u8]| [&(data.len() as u32).to_be_bytes()[..], kind, data, &[0, 0, 0, 0]].concat();
        let ztxt = [&b"mxGraphModel\0\0"[..], &z.finish().unwrap()].concat();
        let png = [&b"\x89PNG\r\n\x1a\n"[..], &chunk(b"IHDR", &[0; 13]), &chunk(b"zTXt", &ztxt), &chunk(b"IEND", &[])].concat();
        assert_eq!(png_diagram(&png).as_deref(), Some(xml));
        let text = [&b"\x89PNG\r\n\x1a\n"[..], &chunk(b"tEXt", format!("mxfile\0{encoded}").as_bytes())].concat();
        assert_eq!(png_diagram(&text).as_deref(), Some(xml));
        assert_eq!(png_diagram(b"\x89PNG\r\n\x1a\n\0\0\0\x05tEXtab"), None, "cut short");
        assert_eq!(svg_diagram("<svg width='1'/>"), None);
    }

    #[test]
    fn export_finds_diagram_blocks_and_leaves_code_alone() {
        let src = "= Guide\n\n[plantuml, flow, svg]\n.Fuel flow\n----\nA -> B\n----\n\n[source,d2]\n----\nx -> y\n----\n\n[d2#launch]\n....\na -> b\n....\n\n[mermaid]\n----\nnever closed\n";
        let b = blocks(src);
        assert_eq!(b.iter().map(|b| (b.kind.as_str(), b.text.as_str())).collect::<Vec<_>>(), [("plantuml", "A -> B"), ("d2", "a -> b")]);
        // Nothing to draw with (no program, containers off): the source stays as it is.
        let (shown, again) = with_placeholders(src);
        assert_eq!(again, b);
        assert!(shown.contains("++++\n<div class=\"cx-diagram\" data-n=\"1\"></div>\n++++") && shown.contains("[source,d2]") && !shown.contains("A -> B"), "{shown}");
        let cfg = PreviewConfig { container: "off".into(), prefer: "container".into(), ..Default::default() };
        assert_eq!(adoc_for_pdf(&cfg, src), src.trim_end_matches('\n'));
    }

    #[test]
    fn export_asciidoc_reads_as_text() {
        let src = ":product: Rocket\n= Launch guide\n// a comment\n\n== Before {product}\n\nCheck the *valve* and _fuel_, see <<s2,the list>> and https://example.com[the site].\n\n* one\n** two\n. first\n. second\n\n[source,rust]\n----\nfn main() {}\n----\n\n|===\n|Part |Mass\n|Tank |40 kg\n|===\n\nNOTE: Keep `x` dry.\nimage::rocket.png[The rocket]\n";
        let t = adoc_text(src);
        for want in ["Launch guide\n============", "Before Rocket\n-------------", "Check the valve and fuel, see the list and the site (https://example.com).", "• one", "  • two", "1. first", "2. second", "    fn main() {}", "Part │ Mass", "NOTE: Keep x dry.", "[The rocket]"] {
            assert!(t.contains(want), "{want:?} in\n{t}");
        }
        assert!(!t.contains("comment") && !t.contains(":product:") && !t.contains("[source"), "{t}");
    }

    #[test]
    fn export_refuses_svg_and_png_with_two_diagrams() {
        let one = "&lt;mxfile&gt;&lt;diagram name=&quot;A&quot;/&gt;&lt;/mxfile&gt;";
        assert!(svg_diagram(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" content="{one}"/>"#)).is_some());
        assert!(svg_diagram(&format!(r#"<svg content="{one}" content="{one}"/>"#)).is_none());
        assert!(svg_diagram(&format!(r#"<svg content="{one}" x:content="{one}"/>"#)).is_none());
        // A `content="` inside another attribute is not the diagram.
        assert!(svg_diagram(&format!(r#"<svg data-x=' content="{one}"'/>"#)).is_none());
        let chunk = |kind: &[u8], data: &[u8]| {
            let mut c = (data.len() as u32).to_be_bytes().to_vec();
            c.extend_from_slice(kind);
            c.extend_from_slice(data);
            c.extend_from_slice(&[0; 4]);
            c
        };
        let png = |n: usize| {
            let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
            for _ in 0..n {
                p.extend(chunk(b"tEXt", b"mxfile\0%3Cmxfile%3E%3C%2Fmxfile%3E"));
            }
            p
        };
        assert!(png_diagram(&png(1)).is_some());
        assert!(png_diagram(&png(2)).is_none());
    }

    #[test]
    fn export_copies_only_plain_files_of_the_folder() {
        let d = folder("plain");
        std::fs::create_dir_all(d.join("doc/chapters")).unwrap();
        std::fs::create_dir_all(d.join("doc/.git")).unwrap();
        std::fs::write(d.join("doc/guide.adoc"), "= G\n").unwrap();
        std::fs::write(d.join("doc/chapters/one.adoc"), "One\n").unwrap();
        std::fs::write(d.join("doc/.git/config"), "x").unwrap();
        std::fs::write(d.join("secret.txt"), "secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(d.join("secret.txt"), d.join("doc/chapters/secret.adoc")).unwrap();
        copy_plain(&d.join("doc"), &d.join("copy")).unwrap();
        assert!(d.join("copy/guide.adoc").is_file() && d.join("copy/chapters/one.adoc").is_file());
        assert!(!d.join("copy/chapters/secret.adoc").exists(), "no link");
        assert!(!d.join("copy/.git").exists(), "no hidden folder");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn export_draws_document_diagrams_only_in_containers() {
        // An installed program is never used: with containers off, nothing is drawn.
        let cfg = PreviewConfig { container: "off".into(), prefer: "local".into(), ..Default::default() };
        assert!(diagram_svg(&cfg, "graphviz", "digraph { a -> b }").is_err());
        let off = PreviewConfig { drawio: true, container: "off".into(), ..Default::default() };
        let d = folder("thumb");
        let f = d.join("a.drawio");
        std::fs::write(&f, "<mxfile/>").unwrap();
        assert!(drawio_thumbnail(&off, &f, &d.join("t.png"), 64, Duration::from_secs(5)).is_err(), "no container: no thumbnail, never the installed draw.io");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn export_lines_lose_their_escapes() {
        assert_eq!(plain("\x1b[1m\x1b[47m\x1b[31mA JavaScript error\x1b[0m\r"), "A JavaScript error");
        assert_eq!(plain("\x1b]0;title\x07said \x1b[2Kthis\x07"), "said this");
    }

    #[test]
    fn export_says_why_drawio_cannot_run() {
        crate::i18n::set_language("en-GB");
        let mut cfg = PreviewConfig::default();
        assert!(!cfg.drawio, "off by default");
        assert_eq!(how(&cfg, DRAWIO), Err(t!("export.drawio_off")));
        cfg.drawio = true;
        cfg.container = "off".into();
        // Never the installed draw.io, whatever is preferred: only its container.
        cfg.tools.insert("drawio".into(), "local".into());
        let e = how(&cfg, DRAWIO).unwrap_err();
        assert!(e.contains(&t!("export.needs_drawio")) && e.contains(&t!("searchable.containers_off")), "{e}");
        assert!(!confirm(Op::DrawioPng, &[PathBuf::from("/x/a.drawio")]).contains("export."));
    }

    /// Needs podman (or a `podman` on PATH that adds its own store) and the network:
    /// `cargo test -p coxswain-core -- --ignored real_export`. Pulls both images when missing.
    #[test]
    #[ignore]
    fn real_export_in_containers() {
        let d = folder("real");
        let mut cfg = PreviewConfig { drawio: true, ..Default::default() };
        cfg.tools.insert("asciidoc".into(), "container".into());
                let flow = d.join("flow.drawio");
        std::fs::write(&flow, r#"<mxfile><diagram name="Overview"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="Tank" vertex="1" parent="1"><mxGeometry x="10" y="10" width="80" height="40" as="geometry"/></mxCell></root></mxGraphModel></diagram><diagram name="Detail"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="Valve" vertex="1" parent="1"><mxGeometry x="10" y="10" width="80" height="40" as="geometry"/></mxCell></root></mxGraphModel></diagram></mxfile>"#).unwrap();
        let said = |s: Said| if let Said::Line(l) = s { eprintln!("{l}") };
        let r = run_all(&cfg, Op::DrawioPng, std::slice::from_ref(&flow), &AtomicBool::new(false), |_| {}, said);
        assert_eq!(r[0].as_ref().unwrap(), &[d.join("flow - Overview.png"), d.join("flow - Detail.png")]);
        let r = run_all(&cfg, Op::DrawioPdf, std::slice::from_ref(&flow), &AtomicBool::new(false), |_| {}, said);
        assert_eq!(r[0].as_ref().unwrap(), &[d.join("flow.pdf")]);
        let thumb = d.join("thumb.png");
        drawio_thumbnail(&cfg, &flow, &thumb, 256, Duration::from_secs(120)).unwrap();
        assert!(std::fs::read(&thumb).unwrap().starts_with(b"\x89PNG"));
        let guide = d.join("guide.adoc");
        std::fs::write(&guide, "= Guide\n\ninclude::part.adoc[]\n\nimage::https://example.com/x.png[Remote]\n\ninclude::/etc/hostname[]\n\n[plantuml]\n.Fuel flow\n----\nTank -> Engine\n----\n").unwrap();
        // PlantUML's image (pulled already, as a preview would have it) draws the diagram.
        cfg.tools.insert("diagrams".into(), "container".into());
        assert!(adoc_for_pdf(&cfg, &std::fs::read_to_string(&guide).unwrap()).contains(".Fuel flow\nimage::data:image/svg+xml;base64,"));
        std::fs::write(d.join("part.adoc"), "Included words.\n").unwrap();
        let r = run_all(&cfg, Op::AdocPdf, std::slice::from_ref(&guide), &AtomicBool::new(false), |_| {}, said);
        let pdf = &r[0].as_ref().unwrap()[0];
        assert_eq!(pdf, &d.join("guide.pdf"));
        assert!(std::fs::read(pdf).unwrap().starts_with(b"%PDF"));
        std::fs::remove_dir_all(d).unwrap();
    }
}

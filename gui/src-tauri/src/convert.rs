//! Previews made by external tools: LaTeX, Typst, LibreOffice, Ghostscript, PlantUML, the
//! diagram programs of Kroki's image (D2, ditaa, svgbob …), pandoc, DuckDB, and ffmpeg and
//! ImageMagick (`coxswain_core::media`: a video's frame, a song's waveform, HEIC, GIMP and RAW
//! photos, and their thumbnails).
//!
//! Each tool runs either from a local install or from a container image (podman or docker),
//! chosen per kind of file (`coxswain_core::preview_tools`, `[preview.tools]`) and by the
//! buttons in the preview pane. Results go to
//! the cache folder, keyed by the file's path, size, modification time and the engine, so a
//! file renders once and shows immediately afterwards.
//!
//! Containers get no network, the file's folder read-only at /src and an empty /out.

use coxswain_core::config::PreviewConfig;
use coxswain_core::t;
use serde::Serialize;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

type Res<T> = Result<T, String>;

/// The media previews, and what each makes.
fn media_op(tool: &str) -> Option<coxswain_core::media::Op> {
    use coxswain_core::media::Op;
    Some(match tool {
        "ffmpeg" => Op::Frame,
        "waveform" => Op::Waveform,
        "heif" | "imagemagick" | "raw" => Op::Picture,
        // GDAL's map of geodata (`coxswain_core::geo`).
        "gdal" => Op::Map,
        "openscad" => Op::Model,
        "kicad" => Op::Board,
        "kicad_thumb" => Op::BoardThumb,
        _ => return None,
    })
}

/// How each diagram program of Kroki's image draws its kind (`export::kroki`).
use coxswain_core::export::kroki;

use coxswain_core::tools::{container_runtime as runtime, image_size, which};

#[derive(Serialize, Clone)]
pub struct Engine {
    /// `local:<program>` or `container:<runtime>`.
    id: String,
    label: String,
    available: bool,
    /// Why it is unavailable, or what a first run will do (pull an image).
    note: String,
    /// A container whose image is not pulled yet: never run it without a click.
    needs_pull: bool,
    /// The line that installs this program here, when it is not installed and one is known.
    install: Option<String>,
    /// A container's image and how much it downloads ("126 MB"), when known.
    image: Option<String>,
    size: Option<String>,
}

/// The ways `tool` can run here, in the order of preference; the first available one is the
/// default.
#[tauri::command]
pub async fn preview_engines(tool: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<Vec<Engine>> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || engines(&cfg, &tool)).await.map_err(|e| e.to_string())
}

fn engines(cfg: &PreviewConfig, tool: &str) -> Vec<Engine> {
    // E-books are off until Settings turns them on: nothing runs, the pane says where.
    if tool == coxswain_core::ebook::TOOL && !cfg.ebooks {
        return vec![Engine { id: "local:ebook-convert".into(), label: "calibre".into(), available: false, note: t!("ebook.off"), needs_pull: false, install: None, image: None, size: None }];
    }
    use coxswain_core::preview_tools;
    let pref = preview_tools::choice(cfg, tool);
    // Off for this kind (Settings → Previews → Preview tools): nothing runs.
    if pref == "off" {
        return vec![Engine { id: "off:".into(), label: t!("ptools.choice.off"), available: false, note: t!("ptools.off_note"), needs_pull: false, install: None, image: None, size: None }];
    }
    let install = preview_tools::find(tool).map(|t| t.install).filter(|i| !i.is_empty()).and_then(coxswain_core::tools::install);
    let mut local: Vec<Engine> = preview_tools::programs(tool)
        .iter()
        .map(|p| {
            let found = which(p);
            Engine { id: format!("local:{p}"), label: p.to_string(), available: found.is_some(), note: if found.is_some() { String::new() } else { t!("convert.not_installed", "program" => p) }, needs_pull: false, install: if found.is_some() { None } else { install.clone() }, image: None, size: None }
        })
        .collect();
    // Only show missing local programs when none is installed, to keep the buttons few, and
    // one button per actual program (`libreoffice` is usually a link to `soffice`).
    if local.iter().any(|e| e.available) {
        let mut seen = std::collections::HashSet::new();
        local.retain(|e| {
            e.available && e.id.strip_prefix("local:").and_then(which).and_then(|p| std::fs::canonicalize(p).ok()).is_some_and(|p| seen.insert(p))
        });
    } else {
        local.truncate(1);
    }
    let key = coxswain_core::images::key(tool);
    let image = cfg.images.get(key).cloned().unwrap_or_default();
    let container = match (runtime(cfg), image.is_empty()) {
        (_, true) => Engine { id: "container:".into(), label: t!("convert.container"), available: false, note: t!("convert.no_image", "tool" => key), needs_pull: false, install: None, image: None, size: None },
        (None, false) => Engine { id: "container:".into(), label: t!("convert.container"), available: false, note: coxswain_core::tools::no_runtime(), needs_pull: false, install: None, size: coxswain_core::images::download(&image), image: Some(image.clone()) },
        (Some((rt, path)), false) => {
            let pulled = image_size(&path, &image).is_some();
            let short = coxswain_core::images::short(&image).rsplit('/').next().unwrap_or(&image).to_string();
            Engine {
                id: format!("container:{rt}"),
                label: format!("{rt} {short}"),
                available: true,
                note: if pulled {
                    t!("convert.runs_offline", "image" => coxswain_core::images::short(&image))
                } else if let Some(size) = coxswain_core::images::download(&image) {
                    t!("convert.first_run_pulls_large", "image" => coxswain_core::images::short(&image), "size" => size)
                } else {
                    t!("convert.first_run_pulls", "image" => coxswain_core::images::short(&image))
                },
                needs_pull: !pulled,
                install: None,
                size: coxswain_core::images::download(&image),
                image: Some(image.clone()),
            }
        }
    };
    // ImageMagick runs in its container first, once its image is there (`media::how`).
    let pref = if pref == "auto" && key == coxswain_core::media::MAGICK && container.available && !container.needs_pull { "container" } else { pref };
    let mut out = if pref == "container" { vec![container.clone()] } else { vec![] };
    out.extend(local);
    if pref != "container" {
        out.push(container);
    }
    if pref == "local" {
        // Still listed, but a container is never the default when local is preferred.
        out.sort_by_key(|e| e.id.starts_with("container:"));
    }
    out
}

#[derive(Serialize)]
pub struct Converted {
    /// "pdf", "svg", "html" or "json".
    kind: &'static str,
    /// The result file (pdf, svg) in the cache.
    file: Option<PathBuf>,
    /// The result text (html, json).
    text: Option<String>,
    /// What went wrong, when a result came all the same (LaTeX builds past its errors).
    note: Option<String>,
    /// Which engine made it, when the one picked could not.
    instead: Option<String>,
    /// An e-book's title, authors and first chapter (`file` is its cover).
    #[serde(skip_serializing_if = "Option::is_none")]
    book: Option<Book>,
}

#[derive(Serialize)]
struct Book {
    title: String,
    authors: String,
    /// The first chapter with real text, as XHTML body content. The UI sanitizes it.
    html: Option<String>,
}

/// Where the result for `path` goes; `main` is the document it builds (`latex::main_file`).
pub(crate) fn cache_dir(path: &Path, main: &Path, tool: &str, engine: &str) -> Res<PathBuf> {
    Ok(previews_dir()?.join(cache_name(path, main, tool, engine)?))
}

/// The name of `cache_dir`'s folder: a hash of the file as it is now and how it is made.
pub(crate) fn cache_name(path: &Path, main: &Path, tool: &str, engine: &str) -> Res<String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, meta.len(), meta.modified().ok(), tool, engine).hash(&mut h);
    // A chapter, a picture or the bibliography changed: that is a new document too.
    match tool {
        "latex" => latex::newest(main, latex::READ).hash(&mut h),
        "typst" => latex::newest(main, TYPST_READ).hash(&mut h),
        _ => {}
    }
    Ok(format!("{:016x}", h.finish()))
}

/// What each tool produces, and where.
fn output(tool: &str, path: &Path, out: &Path) -> (&'static str, PathBuf) {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    match tool {
        "latex" => ("pdf", out.join(path.with_extension("pdf").file_name().unwrap_or_default())),
        "libreoffice" | "typst" | "ghostscript" | "gxps" => ("pdf", out.join(format!("{stem}.pdf"))),
        "plantuml" => ("svg", out.join("diagram.svg")),
        t if kroki(t).is_some() => ("svg", out.join("diagram.svg")),
        "pandoc" => ("html", out.join("doc.html")),
        "gdal" => ("svg", out.join("map.svg")),
        "openscad" => ("svg", out.join("model.svg")),
        "kicad" => ("pdf", out.join("kicad.pdf")),
        "kicad_thumb" => ("svg", out.join("thumb.svg")),
        t if media_op(t).is_some() => ("png", out.join("out.png")),
        "calibre" => ("ebook", out.join("out.epub")),
        _ => ("json", out.join("tables.json")),
    }
}

/// Render `path` with `tool` using `engine` (an id from `preview_engines`). With `cached_only`,
/// returns the earlier result if there is one and does nothing otherwise.
#[tauri::command]
pub async fn convert(path: PathBuf, tool: String, engine: String, cached_only: bool, ctx: tauri::State<'_, crate::Ctx>) -> Res<Option<Converted>> {
    crate::here(&path)?;
    let cfg = ctx.cfg().preview.clone();
    // Its kind *Off* (an engine picked before it was turned off): nothing runs.
    if coxswain_core::preview_tools::off(&cfg, &tool) {
        return Err(t!("ptools.off_note"));
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        // A chapter builds its document, not itself.
        let main = if tool == "latex" { latex::main_file(&path) } else { path.clone() };
        let out = cache_dir(&path, &main, &tool, &engine)?;
        let (kind, file) = output(&tool, &main, &out);
        // What the build said is kept next to its result, so the next look says it too.
        let said = |what: &str| std::fs::read_to_string(out.join(what)).ok().filter(|s| !s.is_empty());
        let done = |file: PathBuf| -> Res<Option<Converted>> {
            let (note, instead) = (said("note.txt"), said("instead.txt"));
            Ok(Some(match kind {
                "pdf" | "svg" | "png" => Converted { kind, file: Some(file), text: None, note, instead, book: None },
                "ebook" => {
                    let p = coxswain_core::ebook::shown(&out).ok_or_else(|| t!("convert.no_result"))?;
                    let html = coxswain_core::extract::book::first_chapter(&p.epub).and_then(|(_, html)| html);
                    Converted { kind, file: p.cover, text: None, note, instead, book: Some(Book { title: p.title, authors: p.authors, html }) }
                }
                _ => Converted { kind, file: None, text: Some(std::fs::read_to_string(&file).map_err(|e| e.to_string())?), note, instead, book: None },
            }))
        };
        if file.is_file() {
            return done(file);
        }
        if cached_only {
            return Ok(None);
        }
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        match run(&cfg, &tool, &engine, &main, &out, &file) {
            Ok(()) if file.is_file() => done(file),
            // A document with an error still makes a PDF: shown, with the error above it.
            Err(e) if file.is_file() => {
                let _ = std::fs::write(out.join("note.txt"), failure(&tool, &main, &out, &e));
                done(file)
            }
            Ok(()) => Err(failure(&tool, &main, &out, &t!("convert.no_result"))),
            Err(e) => {
                let why = failure(&tool, &main, &out, &e);
                // A LaTeX build that one engine cannot make, another often can (tectonic is
                // XeTeX only, a TeX Live container has everything): the others are tried, and
                // what they make is kept as this engine's result, so the next look finds it.
                if tool == "latex" {
                    let first = engines(&cfg, &tool).into_iter().find(|x| x.id == engine).map_or(engine.clone(), |x| x.label);
                    for other in engines(&cfg, &tool).into_iter().filter(|x| x.available && !x.needs_pull && x.id != engine) {
                        let _ = std::fs::remove_dir_all(&out);
                        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                        if run(&cfg, &tool, &other.id, &main, &out, &file).is_ok() || file.is_file() {
                            let said = why.lines().next().unwrap_or_default().to_string();
                            let _ = std::fs::write(out.join("instead.txt"), t!("convert.built_instead", "engine" => other.label, "failed" => first, "why" => said));
                            return done(file);
                        }
                    }
                }
                Err(why)
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    // The images it ran in were used now: what *Remove images not used for 90 days* goes by.
    let used = std::mem::take(&mut *USED.lock().unwrap());
    if !used.is_empty() {
        let now = coxswain_core::fs::now();
        let _ = ctx.edit(|st| used.iter().for_each(|i| coxswain_core::images::used(st, i, now)));
    }
    result
}

/// The images previews ran in since `convert` last wrote them into the state.
pub(crate) static USED: Mutex<std::collections::BTreeSet<String>> = Mutex::new(std::collections::BTreeSet::new());

/// While the app runs: once an hour, the pulled images are updated when
/// `[preview] image_updates` says a round is due, and those no preview used for 90 days are
/// removed when `image_prune` is on. Never on battery, never with containers off; the state
/// is not locked over a pull.
pub fn keep(app: tauri::AppHandle) {
    use coxswain_core::images;
    use tauri::Manager;
    std::thread::spawn(move || {
        // Not in the first minutes: the app is starting, and a short run needs none of it.
        let mut wait = 600;
        loop {
            std::thread::sleep(Duration::from_secs(wait));
            wait = 3600;
            let ctx = app.state::<crate::Ctx>();
            let cfg = ctx.cfg().preview.clone();
            // No runtime, or containers off.
            let Some((_, rt)) = runtime(&cfg) else { continue };
            if coxswain_core::machine::on_battery() {
                continue;
            }
            let now = coxswain_core::fs::now();
            if ctx.state.lock().is_ok_and(|st| images::update_due(&cfg, &st, now)) {
                // A new version's pins first: the old copies they replace go.
                let moved = ctx.state.lock().map(|st| images::superseded(&cfg, &st)).unwrap_or_default();
                let gone = images::move_pins(&rt, &moved, |i| pull(&rt, i));
                if !gone.is_empty() {
                    let _ = ctx.edit(|st| st.images_used.retain(|i, _| !gone.contains(i)));
                }
                // Every image is tried; when all fail (offline) the round stays due for the next hour.
                if images::update_round(&rt, &images::pulled(&cfg, &rt), |i| pull(&rt, i)) {
                    let _ = ctx.edit(|st| st.images_checked = now);
                }
            }
            if cfg.image_prune {
                let pulled = images::pulled(&cfg, &rt);
                let stale = ctx.edit_if(|st| {
                    let before = st.images_used.len();
                    let stale = images::stale(st, &pulled, now);
                    (stale, st.images_used.len() != before)
                });
                for image in stale.unwrap_or_default() {
                    if images::remove(&rt, &image).is_ok() {
                        let _ = ctx.edit(|st| st.images_used.remove(&image));
                    }
                }
            }
        }
    });
}

pub(crate) fn previews_dir() -> Res<PathBuf> {
    Ok(coxswain_core::dirs::cache_dir().ok_or_else(|| t!("err.no_cache_folder"))?.join("coxswain").join("previews"))
}

/// Bytes the previews made so far take in the cache.
#[tauri::command]
pub async fn preview_cache() -> Res<u64> {
    tauri::async_runtime::spawn_blocking(|| Ok(coxswain_core::fs::dir_size(&previews_dir()?).0)).await.map_err(|e| e.to_string())?
}

/// Forget every preview made so far: each is made again when next shown.
#[tauri::command]
pub async fn clear_preview_cache() -> Res<()> {
    tauri::async_runtime::spawn_blocking(|| match std::fs::remove_dir_all(previews_dir()?) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The error, plus the telling part of a LaTeX log.
fn failure(tool: &str, path: &Path, out: &Path, err: &str) -> String {
    if tool != "latex" {
        return err.to_string();
    }
    let log = out.join(path.with_extension("log").file_name().unwrap_or_default());
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    match lines.iter().position(|l| l.starts_with('!')) {
        Some(i) => lines[i..(i + 6).min(lines.len())].join("\n"),
        // No log (tectonic keeps none): what the program said, without its warnings, which
        // come first and are seldom why it stopped.
        None => {
            let said: Vec<&str> = err.lines().filter(|l| !l.starts_with("warning:") && !l.starts_with("note:") && !l.trim().is_empty()).collect();
            if said.is_empty() { err.to_string() } else { said.join("\n") }
        }
    }
}

/// LibreOffice allows one conversion per profile at a time.
static LIBREOFFICE: Mutex<()> = Mutex::new(());

fn run(cfg: &PreviewConfig, tool: &str, engine: &str, path: &Path, out: &Path, file: &Path) -> Res<()> {
    let first = build(cfg, tool, engine, path, out, file, None);
    // pdfLaTeX was picked, and a package the document loads needs XeTeX: once more with that.
    if tool == "latex" && first.is_err() && !file.is_file() && latex::engine_flag(path) == "-pdf" {
        let log = out.join(path.with_extension("log").file_name().unwrap_or_default());
        if std::fs::read_to_string(log).is_ok_and(|l| l.contains("requires either XeTeX or") || l.contains("requires either XeTeX or LuaTeX")) {
            let _ = std::fs::remove_dir_all(out);
            std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
            return build(cfg, tool, engine, path, out, file, Some("-pdfxe"));
        }
    }
    first
}

/// One run of `tool` on `path`, the document (for LaTeX its main file, not a chapter).
/// `flag`: latexmk's engine, when not the document's own.
fn build(cfg: &PreviewConfig, tool: &str, engine: &str, path: &Path, out: &Path, file: &Path, flag: Option<&'static str>) -> Res<()> {
    let dir = path.parent().ok_or_else(|| t!("err.no_folder"))?;
    let name = path.file_name().ok_or_else(|| t!("err.no_file_name"))?.to_string_lossy().into_owned();
    // The document as a path, never a bare name: TeX tools read a name starting with `-` as an
    // option, and latexmk runs `-pdflatex=COMMAND` through the shell.
    let doc = format!("./{name}");
    // latexmk hands the name to the shell in double quotes: a name with `$`, a backquote and the
    // like is built by the engine itself, which uses no shell.
    let plain = latex::shell_safe(&name);
    let timeout = Duration::from_secs(cfg.timeout.max(10));
    let (kind, which_one) = engine.split_once(':').ok_or_else(|| t!("convert.unknown_engine"))?;
    if let Some(op) = media_op(tool) {
        let how = media_how(cfg, tool, kind, which_one)?;
        if let coxswain_core::media::How::Container { image, .. } = &how {
            USED.lock().unwrap().insert(image.clone());
        }
        return coxswain_core::media::preview(&how, &op, path, file, timeout);
    }

    if kind == "container" {
        let (rt, rt_path) = runtime(cfg).ok_or_else(coxswain_core::tools::no_runtime)?;
        if rt != which_one {
            return Err(t!("convert.not_available", "program" => which_one));
        }
        let key = coxswain_core::images::key(tool);
        let image = cfg.images.get(key).filter(|i| !i.is_empty()).ok_or_else(|| t!("convert.no_container_image", "tool" => key))?;
        if image_size(&rt_path, image).is_none() {
            pull(&rt_path, image)?;
        }
        USED.lock().unwrap().insert(image.clone());
        if tool == coxswain_core::ebook::TOOL {
            let how = coxswain_core::ebook::How::Container { rt, path: rt_path, image: image.clone() };
            return coxswain_core::ebook::preview(&how, path, out, timeout, &mut |_| {}).map(drop);
        }
        let container_name = format!("coxswain-preview-{}", out.file_name().unwrap_or_default().to_string_lossy());
        // LaTeX and Typst see their whole project, so `../figures/plot.pdf` is found; the rest only their folder.
        let mount = match tool {
            "latex" => latex::project(path),
            "typst" => typst_root(path),
            _ => dir.to_path_buf(),
        };
        let workdir = Path::new("/src").join(dir.strip_prefix(&mount).unwrap_or(Path::new("")));
        let at = |p: &Path| Path::new("/src").join(p.strip_prefix(&mount).unwrap_or(p)).to_string_lossy().replace('\\', "/");
        let mut c = coxswain_core::images::sealed(rt, &rt_path, &container_name, &mount, Some(out));
        c.arg("-w").arg(workdir.to_string_lossy().replace('\\', "/"));
        let stdin = match tool {
            "latex" if plain => {
                c.args([image.as_str(), "latexmk", flag.unwrap_or_else(|| latex::engine_flag(path)), "-interaction=nonstopmode", "-norc", "-outdir=/out", &doc]);
                None
            }
            "latex" => {
                c.args([image.as_str(), latex::engine(flag.unwrap_or_else(|| latex::engine_flag(path))), "-interaction=nonstopmode", "-halt-on-error", "-output-directory=/out", &doc]);
                None
            }
            "typst" => {
                let pdf = format!("/out/{}", file.file_name().unwrap_or_default().to_string_lossy());
                c.args([image.as_str(), "compile", "--root", "/src", &at(path), &pdf]);
                None
            }
            "libreoffice" => {
                // A profile in the container's /tmp, the one place it can write besides /out.
                c.args([image.as_str(), "soffice", "-env:UserInstallation=file:///tmp/lo", "--headless", "--convert-to", "pdf", "--outdir", "/out", &format!("/src/{name}")]);
                None
            }
            "ghostscript" => {
                let pdf = format!("-sOutputFile=/out/{}", file.file_name().unwrap_or_default().to_string_lossy());
                c.args(["--entrypoint", "gs", image.as_str()]).args(ghostscript(path)).args([&pdf, &format!("/src/{name}")]);
                None
            }
            "gxps" => {
                let pdf = format!("-sOutputFile=/out/{}", file.file_name().unwrap_or_default().to_string_lossy());
                c.args(["--entrypoint", "gxps", image.as_str()]).args(ghostscript(Path::new("x.xps"))).args([&pdf, &format!("/src/{name}")]);
                None
            }
            // The official images' entry points are the tools themselves.
            "plantuml" => {
                c.args(["-e", "PLANTUML_SECURITY_PROFILE=ALLOWLIST", "-e", "JAVA_TOOL_OPTIONS=-Dplantuml.allowlist.path=/src", image, "-tsvg", "-pipe"]);
                Some(path)
            }
            "pandoc" => {
                c.args([image, "--sandbox", "-f", "rst", "-t", "html5"]);
                Some(path)
            }
            t if kroki(t).is_some() => {
                let argv = kroki(t).unwrap_or_default();
                c.args(["--entrypoint", argv[0], image.as_str()]).args(&argv[1..]);
                Some(path)
            }
            _ => {
                c.args([image.as_str(), "-readonly", "-json", &format!("/src/{name}"), "-c", DUCKDB_SQL]);
                return capture(c, None, timeout, file, Some((&rt_path, &container_name)));
            }
        };
        return if stdin.is_some() { capture(c, stdin, timeout, file, Some((&rt_path, &container_name))) } else { wait(c, timeout, Some((&rt_path, &container_name))) };
    }

    // Only a program of this tool's own list: the page names it, and a name outside the list
    // would run whatever is on the PATH under that name.
    if !coxswain_core::preview_tools::programs(tool).contains(&which_one) {
        return Err(t!("convert.unknown_engine"));
    }
    let program = which(which_one).ok_or_else(|| t!("convert.not_installed", "program" => which_one))?;
    let mut c = coxswain_core::tools::command(&program);
    c.current_dir(dir);
    match (tool, which_one) {
        ("latex", "latexmk") if plain => {
            // No -halt-on-error: past a first error LaTeX usually still makes the PDF.
            c.args([flag.unwrap_or_else(|| latex::engine_flag(path)), "-interaction=nonstopmode"]).arg(format!("-outdir={}", out.display()));
            latex::own_rc(&mut c);
            c.arg(&doc);
        }
        // Its cache, when this run makes it, is Coxswain's doing: Disk use lists it.
        ("latex", "tectonic") => {
            c.arg("--outdir").arg(out).arg(&doc);
            let absent = coxswain_core::disk::tectonic_absent();
            let done = wait(c, timeout, None);
            coxswain_core::disk::note_made(&absent);
            return done;
        }
        ("latex", _) => {
            // A document that asks for XeLaTeX or LuaLaTeX gets it when TeX Live has it; latexmk
            // with a name it would hand to the shell gets the engine itself.
            let engine = latex::engine(flag.unwrap_or_else(|| latex::engine_flag(path)));
            let wanted = which(engine).or_else(|| if which_one == "latexmk" { which("pdflatex") } else { None });
            if let Some(other) = wanted {
                c = coxswain_core::tools::command(&other);
                c.current_dir(dir);
            } else if which_one == "latexmk" {
                return Err(t!("convert.not_installed", "program" => engine));
            }
            c.args(["-interaction=nonstopmode", "-halt-on-error"]).arg(format!("-output-directory={}", out.display())).arg(&doc);
        }
        ("libreoffice", _) => {
            // Its own profile, so a running LibreOffice does not swallow the conversion, with
            // macros and link updates off.
            let url = coxswain_core::extract::installed::libreoffice_profile("libreoffice-profile").ok_or_else(|| t!("err.no_cache_folder"))?;
            c.arg(format!("-env:UserInstallation={url}")).args(["--headless", "--convert-to", "pdf", "--outdir"]).arg(out).arg(path);
            let _one = LIBREOFFICE.lock().map_err(|e| e.to_string())?;
            return wait(c, timeout, None);
        }
        ("plantuml", _) => {
            // A diagram may include files from its own folder, and nothing from the web.
            let java = std::env::var("JAVA_TOOL_OPTIONS").map(|o| format!("{o} ")).unwrap_or_default();
            c.env("PLANTUML_SECURITY_PROFILE", "ALLOWLIST").env("JAVA_TOOL_OPTIONS", format!("{java}-Dplantuml.allowlist.path=\"{}\"", dir.display()));
            c.args(["-tsvg", "-pipe"]);
            return capture(c, Some(path), timeout, file, None);
        }
        ("calibre", _) => {
            let how = coxswain_core::ebook::How::Local { convert: program, meta: which("ebook-meta") };
            return coxswain_core::ebook::preview(&how, path, out, timeout, &mut |_| {}).map(drop);
        }
        ("typst", _) => {
            c.arg("compile").arg("--root").arg(typst_root(path)).arg(path).arg(file);
        }
        ("ghostscript" | "gxps", _) => {
            c.args(ghostscript(path)).arg(format!("-sOutputFile={}", file.display())).arg(path);
        }
        (t, _) if kroki(t).is_some() => {
            c.args(&kroki(t).unwrap_or_default()[1..]);
            return capture(c, Some(path), timeout, file, None);
        }
        ("pandoc", _) => {
            // --sandbox: no includes of files and nothing from the web.
            c.args(["--sandbox", "-f", "rst", "-t", "html5"]);
            return capture(c, Some(path), timeout, file, None);
        }
        _ => {
            c.args(["-readonly", "-json"]).arg(path).args(["-c", DUCKDB_SQL]);
            return capture(c, None, timeout, file, None);
        }
    }
    wait(c, timeout, None)
}

/// How a media preview runs with the engine `kind:which_one` (`local:magick`,
/// `container:podman`): only a program of the tool's own list, or the runtime set.
fn media_how(cfg: &PreviewConfig, tool: &str, kind: &str, which_one: &str) -> Res<coxswain_core::media::How> {
    use coxswain_core::media::{How, Program};
    let key = coxswain_core::images::key(tool);
    if kind == "container" {
        let (rt, path) = runtime(cfg).ok_or_else(coxswain_core::tools::no_runtime)?;
        if rt != which_one {
            return Err(t!("convert.not_available", "program" => which_one));
        }
        let image = cfg.images.get(key).filter(|i| !i.is_empty()).ok_or_else(|| t!("convert.no_container_image", "tool" => key))?;
        let program = Program::ALL.into_iter().find(|p| p.tool() == key).unwrap_or(Program::Magick);
        return Ok(How::Container { program, rt, path, image: image.clone() });
    }
    if !coxswain_core::preview_tools::programs(tool).contains(&which_one) {
        return Err(t!("convert.unknown_engine"));
    }
    let path = which(which_one).ok_or_else(|| t!("convert.not_installed", "program" => which_one))?;
    let program = match which_one {
        "ffmpeg" => Program::Ffmpeg,
        "gdalinfo" => Program::Gdal,
        "openscad" => Program::Openscad,
        "kicad-cli" => Program::Kicad,
        _ => Program::Magick,
    };
    Ok(How::Local { program, path })
}

/// Ghostscript's options for a PostScript file into a PDF. `-dSAFER` above all: PostScript is a
/// programming language, and without it a file could read and write files of yours.
fn ghostscript(path: &Path) -> Vec<&'static str> {
    let mut v = vec!["-dSAFER", "-dBATCH", "-dNOPAUSE", "-dQUIET", "-sDEVICE=pdfwrite"];
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("eps")) {
        // A picture: the page is its bounding box, not a sheet of A4 around it.
        v.push("-dEPSCrop");
    }
    v
}

/// What a Typst build reads, for when its preview is made again: sources, data and pictures.
// ponytail: by extension, under the document's folder (as for LaTeX); `../` files and other
// kinds are not watched, Build again picks them up.
const TYPST_READ: &[&str] = &["typ", "bib", "yml", "yaml", "toml", "json", "csv", "xml", "txt", "png", "jpg", "jpeg", "gif", "svg", "webp", "pdf", "otf", "ttf"];

/// What a Typst document may read (`--root`): the git repository it is in, else its folder.
fn typst_root(path: &Path) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new("."));
    dir.ancestors().find(|d| d.join(".git").exists()).unwrap_or(dir).to_path_buf()
}

const DUCKDB_SQL: &str = "SELECT schema_name, table_name, estimated_size AS rows, column_count AS columns FROM duckdb_tables() ORDER BY 1, 2";

/// What a LaTeX editor works out before building: which file is the document, which folder
/// is the project, and which engine it asks for.
mod latex {
    use std::path::{Path, PathBuf};

    fn head(path: &Path) -> String {
        start(path, 64 * 1024)
    }

    /// The first `max` bytes of a file, as text: a file in Latin-1, or one cut in the middle of
    /// a character, keeps the rest of its text.
    fn start(path: &Path, max: u64) -> String {
        use std::io::Read;
        let mut b = vec![];
        if let Ok(f) = std::fs::File::open(path) {
            let _ = f.take(max).read_to_end(&mut b);
        }
        String::from_utf8_lossy(&b).into_owned()
    }

    /// `% !TEX key = value` in the first lines, as TeXShop, TeXstudio and LaTeX Workshop read it.
    fn magic(text: &str, key: &str) -> Option<String> {
        text.lines().take(20).find_map(|l| {
            let l = l.trim_start().strip_prefix('%')?.trim_start();
            let l = l.strip_prefix("!TEX").or_else(|| l.strip_prefix("!TeX"))?.trim_start();
            let (k, v) = l.split_once('=')?;
            (k.trim().eq_ignore_ascii_case(key) || k.trim().eq_ignore_ascii_case(&format!("TS-{key}"))).then(|| v.trim().to_string())
        })
    }

    /// Whether latexmk can be given this file name: it puts it in double quotes on a shell line.
    pub fn shell_safe(name: &str) -> bool {
        !name.chars().any(|c| c.is_control() || "$`\"\\'%;&|<>*?!#~{}".contains(c))
    }

    /// The engine latexmk's flag stands for.
    pub fn engine(flag: &str) -> &'static str {
        match flag {
            "-pdfxe" => "xelatex",
            "-pdflua" => "lualatex",
            _ => "pdflatex",
        }
    }

    /// The document `path` belongs to: itself, the file its `% !TEX root` names, or for a
    /// file without `\documentclass` the document nearby that includes it.
    pub fn main_file(path: &Path) -> PathBuf {
        let text = head(path);
        let dir = path.parent().unwrap_or(Path::new("."));
        // Only a root inside the project: `% !TEX root = /etc/x` or `../../..` builds nothing there.
        if let Some(root) = magic(&text, "root") {
            let within = std::fs::canonicalize(project(path));
            if let (Ok(p), Ok(within)) = (std::fs::canonicalize(dir.join(root)), within) {
                if p.is_file() && p.starts_with(&within) && p.extension().is_some_and(|x| x == "tex" || x == "ltx") {
                    return p;
                }
            }
        }
        if text.contains("\\documentclass") {
            return path.to_path_buf();
        }
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        // ponytail: its folder and two above, not the whole tree; `% !TEX root` covers the rest.
        dir.ancestors()
            .take(3)
            .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "tex" || x == "ltx") && p != path)
            .find(|p| {
                let t = start(p, 4 << 20);
                t.contains("\\documentclass") && t.contains(&stem)
            })
            .unwrap_or_else(|| path.to_path_buf())
    }

    /// The folder a build has to see: the git repository `main` is in, else as far up as the
    /// document's `../` paths reach (three folders at most, never the home folder or above).
    pub fn project(main: &Path) -> PathBuf {
        let dir = main.parent().unwrap_or(Path::new("."));
        if let Some(repo) = dir.ancestors().find(|d| d.join(".git").exists()) {
            return repo.to_path_buf();
        }
        let text = start(main, 4 << 20);
        let depth = text.split(|c: char| c == '{' || c == ',' || c.is_whitespace()).map(|arg| arg.matches("../").count()).max().unwrap_or(0).min(3);
        let home = coxswain_core::dirs::home_dir();
        dir.ancestors().take(depth + 1).take_while(|d| d.parent().is_some() && Some(*d) != home.as_deref()).last().unwrap_or(dir).to_path_buf()
    }

    /// What a build reads: sources, styles, bibliographies, pictures and data for plots.
    // ponytail: by extension; a document that inputs another kind is rebuilt with Build.
    pub const READ: &[&str] = &["tex", "ltx", "sty", "cls", "clo", "def", "cfg", "dtx", "bib", "bst", "bbx", "cbx", "png", "jpg", "jpeg", "pdf", "eps", "svg", "tikz", "pgf", "csv", "dat"];

    /// When anything a build reads in the document's folder last changed. Other files there
    /// (a database a preview opened, an editor's backup) are not part of it: with them, the
    /// preview was made again every time.
    pub fn newest(main: &Path, read: &[&str]) -> Option<std::time::SystemTime> {
        // ponytail: the document's folder and below, 5000 files at most; pictures in `../figures`
        // are not watched, press Build again after changing the opened file for those.
        let mut stack = vec![main.parent()?.to_path_buf()];
        let (mut newest, mut seen) = (None, 0);
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let (p, Ok(m)) = (e.path(), e.metadata()) else { continue };
                if e.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                seen += 1;
                if seen > 5000 {
                    return newest;
                }
                if m.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| read.iter().any(|r| x.eq_ignore_ascii_case(r))) {
                    newest = newest.max(m.modified().ok());
                }
            }
        }
        newest
    }

    /// latexmk reads a `latexmkrc` in the document's folder, which is Perl: a downloaded
    /// project could run anything while it is looked at. Only the user's own rc files count.
    pub fn own_rc(c: &mut std::process::Command) {
        c.arg("-norc");
        let home = coxswain_core::dirs::home_dir().unwrap_or_default();
        for rc in [home.join(".latexmkrc"), home.join("latexmkrc"), home.join(".config/latexmk/latexmkrc")] {
            if rc.is_file() {
                c.arg("-r").arg(rc);
            }
        }
    }

    /// latexmk's flag for `% !TEX program = xelatex` and friends; without one, the engine the
    /// packages ask for (fontspec: XeLaTeX, luacode: LuaLaTeX), in the document or in the style
    /// and class files next to it; pdfLaTeX otherwise.
    pub fn engine_flag(main: &Path) -> &'static str {
        let program = magic(&head(main), "program").unwrap_or_else(|| guess(main)).to_lowercase();
        match program.as_str() {
            "xelatex" => "-pdfxe",
            "lualatex" => "-pdflua",
            _ => "-pdf",
        }
    }

    /// "lualatex", "xelatex" or "" by the packages loaded.
    fn guess(main: &Path) -> String {
        let dir = main.parent().unwrap_or(Path::new("."));
        let own: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "sty" || e == "cls")).take(20).collect();
        // Without comments: `% \usepackage{fontspec}` asks for nothing. A file that asks which
        // engine runs (`\ifxetex`, `iftex`) works with each and tells nothing either.
        let uncommented = |t: String| t.lines().map(|l| l.split('%').next().unwrap_or("")).collect::<Vec<_>>().join("\n");
        let branches = |t: &str| ["\\ifxetex", "\\ifluatex", "\\ifpdftex", "\\iftutex", "{iftex}", "{ifxetex}", "{ifluatex}"].iter().any(|w| t.to_lowercase().contains(w));
        let text: String = [main.to_path_buf()].iter().chain(&own).map(|p| uncommented(head(p))).filter(|t| !branches(t)).collect::<Vec<_>>().join("\n");
        let any = |words: &[&str]| words.iter().any(|w| text.contains(w));
        if any(&["luacode", "luatexja", "luaotfload", "\\directlua", "luatexbase"]) {
            "lualatex".into()
        } else if any(&["fontspec", "unicode-math", "polyglossia", "xeCJK", "xltxtra", "mathspec", "\\setmainfont"]) {
            "xelatex".into()
        } else {
            String::new()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn latex_errors_say_why_it_stopped() {
            let d = std::env::temp_dir().join(format!("coxswain-latex-why-{}", std::process::id()));
            std::fs::create_dir_all(&d).unwrap();
            let main = d.join("main.tex");
            std::fs::write(&main, "\\documentclass{book}\n").unwrap();
            let said = "warning: accessing absolute path `/usr/share/fonts/a.ttf`; build may not be reproducible\nnote: downloading x\ncalled `Result::unwrap()` on an `Err` value: NulError";
            assert_eq!(super::super::failure("latex", &main, &d, said), "called `Result::unwrap()` on an `Err` value: NulError", "tectonic keeps no log: its warnings are not why");
            std::fs::write(d.join("main.log"), "This is XeTeX\n! Undefined control sequence.\nl.3 \\foo\n").unwrap();
            assert!(super::super::failure("latex", &main, &d, said).starts_with("! Undefined control sequence."));
            std::fs::remove_dir_all(d).unwrap();
        }

        #[test]
        fn latex_finds_the_document_the_project_and_the_engine() {
            let d = std::env::temp_dir().join(format!("coxswain-latex-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(d.join("paper/chapters")).unwrap();
            std::fs::create_dir_all(d.join("figures")).unwrap();
            let main = d.join("paper/main.tex");
            std::fs::write(&main, "% !TEX program = xelatex\n\\documentclass{article}\n\\begin{document}\n\\includegraphics{../figures/plot}\n\\input{chapters/intro}\n\\end{document}\n").unwrap();
            std::fs::write(d.join("paper/chapters/intro.tex"), "\\section{Intro}\n").unwrap();
            std::fs::write(d.join("paper/chapters/method.tex"), "%!TEX root = ../main.tex\n\\section{Method}\n").unwrap();

            assert_eq!(main_file(&main), main);
            // A document of its own in Latin-1 (its name in main.tex too): not taken for a part.
            let latin = d.join("paper/plot.tex");
            std::fs::write(&latin, [&b"% K\xf8benhavn\n"[..], b"\\documentclass{article}\n"].concat()).unwrap();
            assert_eq!(main_file(&latin), latin);
            assert_eq!(main_file(&d.join("paper/chapters/intro.tex")), main, "found by who includes it");
            assert_eq!(main_file(&d.join("paper/chapters/method.tex")), std::fs::canonicalize(&main).unwrap(), "found by its magic comment");
            let outside = std::env::temp_dir().join(format!("coxswain-latex-outside-{}.tex", std::process::id()));
            std::fs::write(&outside, "\\documentclass{article}\n").unwrap();
            std::fs::write(d.join("paper/chapters/evil.tex"), format!("%!TEX root = {}\n", outside.display())).unwrap();
            assert_eq!(main_file(&d.join("paper/chapters/evil.tex")), d.join("paper/chapters/evil.tex"), "a root outside the project is not taken");
            std::fs::remove_file(&outside).unwrap();
            assert_eq!(project(&main), d, "one `../` up");
            assert!(shell_safe("Kapitel 2 (udkast).tex"));
            for bad in ["$(id).tex", "`id`.tex", "a\"b.tex", "a\\b.tex", "50%.tex"] {
                assert!(!shell_safe(bad), "{bad}");
            }
            assert_eq!(engine_flag(&main), "-pdfxe");
            assert_eq!(engine_flag(&d.join("paper/chapters/intro.tex")), "-pdf");
            // No magic comment: the packages tell, also from the document's own style file.
            std::fs::create_dir_all(d.join("book")).unwrap();
            std::fs::write(d.join("book/main.tex"), "\\documentclass{book}\n% \\usepackage{luacode}\n\\usepackage{mybook}\n").unwrap();
            std::fs::write(d.join("book/mybook.sty"), "\\RequirePackage{fontspec}\n\\setmainfont{Libertinus Serif}\n").unwrap();
            assert_eq!(engine_flag(&d.join("book/main.tex")), "-pdfxe", "fontspec in mybook.sty; luacode only in a comment");
            std::fs::write(d.join("book/mybook.cls"), "\\RequirePackage{iftex}\n\\ifXeTeX\\RequirePackage{fontspec}\\fi\n").unwrap();
            std::fs::write(d.join("book/mybook.sty"), "").unwrap();
            assert_eq!(engine_flag(&d.join("book/main.tex")), "-pdf", "a class that works with each engine asks for none");
            std::fs::write(d.join("book/mybook.sty"), "\\usepackage{luacode}\n").unwrap();
            assert_eq!(engine_flag(&d.join("book/main.tex")), "-pdflua");
            std::fs::remove_file(d.join("book/mybook.sty")).unwrap();
            assert_eq!(engine_flag(&d.join("book/main.tex")), "-pdf");
            let before = newest(&main, READ).unwrap();
            let later = before + std::time::Duration::from_secs(60);
            std::fs::File::options().write(true).open(d.join("paper/chapters/intro.tex")).unwrap().set_modified(later).unwrap();
            assert_eq!(newest(&main, READ), Some(later), "a chapter changed");
            std::fs::write(d.join("paper/data.db-shm"), "").unwrap();
            std::fs::File::options().write(true).open(d.join("paper/data.db-shm")).unwrap().set_modified(later + std::time::Duration::from_secs(60)).unwrap();
            assert_eq!(newest(&main, READ), Some(later), "a file the build does not read changes nothing");

            std::fs::create_dir_all(d.join(".git")).unwrap();
            assert_eq!(project(&d.join("paper/chapters/intro.tex")), d, "the repository");
            let _ = std::fs::remove_dir_all(&d);
        }
    }
}


use coxswain_core::images::pull;

/// Settings → Previews → Preview tools: every kind of file, what is installed, its images and
/// whether the runtime has them (with the progress of a pull under way), the choice, and what
/// is used.
#[derive(Serialize)]
pub struct Tools {
    rows: Vec<coxswain_core::preview_tools::Row>,
    pulling: BTreeMap<String, String>,
    /// Why no container runs, when none can.
    no_runtime: Option<String>,
}

#[tauri::command]
pub async fn preview_tools(ctx: tauri::State<'_, crate::Ctx>) -> Res<Tools> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || Tools {
        rows: coxswain_core::preview_tools::rows(&cfg),
        pulling: coxswain_core::images::pulling(),
        no_runtime: runtime(&cfg).is_none().then(|| if cfg.container == "off" { t!("searchable.containers_off") } else { coxswain_core::tools::no_runtime() }),
    })
    .await
    .map_err(|e| e.to_string())
}

/// *Preview with…*: the row of the kind `tool` belongs to.
#[tauri::command]
pub async fn preview_with(tool: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<coxswain_core::preview_tools::Row> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || coxswain_core::preview_tools::row_of(&cfg, &tool).ok_or_else(|| t!("ptools.none_here"))).await.map_err(|e| e.to_string())?
}

/// *Try*: the sample of `kind`, written into the cache, for the preview to show.
#[tauri::command]
pub fn preview_sample(kind: String) -> Res<PathBuf> {
    coxswain_core::preview_tools::sample(&kind)
}

/// Pull `image`, or update it to the newest. Poll `pull_progress` meanwhile.
#[tauri::command]
pub async fn pull_image(image: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<()> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || pull(&runtime(&cfg).ok_or_else(coxswain_core::tools::no_runtime)?.1, &image)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_image(image: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<()> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (_, rt) = runtime(&cfg).ok_or_else(coxswain_core::tools::no_runtime)?;
        let out = coxswain_core::tools::command(rt).args(["image", "rm", &image]).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
        if out.status.success() { Ok(()) } else { Err(last_lines(&String::from_utf8_lossy(&out.stderr), 3).unwrap_or_default()) }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The latest progress line of every pull under way, by image.
#[tauri::command]
pub fn pull_progress() -> BTreeMap<String, String> {
    coxswain_core::images::pulling()
}

/// *Make PDF searchable* (`coxswain_core::searchable`): what it would do with `paths`, for the
/// question before it. `paths` are the PDFs among what was picked.
#[derive(Serialize)]
pub struct SearchablePlan {
    pdfs: Vec<PathBuf>,
    /// The name of the copy each one gets.
    targets: Vec<String>,
    /// Which program runs, and the first pull with its size.
    about: String,
    pull: bool,
    /// Why it cannot run here: what to install or which setting to change.
    missing: Option<String>,
}

#[tauri::command]
pub async fn searchable_plan(paths: Vec<PathBuf>, ctx: tauri::State<'_, crate::Ctx>) -> Res<SearchablePlan> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // A PDF only in the cloud is not fetched to be read: it is left out.
        let pdfs: Vec<PathBuf> = paths.into_iter().filter(|p| coxswain_core::searchable::is_pdf(p) && crate::here(p).is_ok() && p.is_file()).collect();
        let targets = pdfs.iter().map(|p| coxswain_core::searchable::target(p).file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
        let (about, pull, missing) = match coxswain_core::searchable::how(&cfg) {
            Ok(how) => {
                let (about, pull) = coxswain_core::searchable::about(&how);
                (about, pull, None)
            }
            Err(e) => (String::new(), false, Some(e)),
        };
        SearchablePlan { pdfs, targets, about, pull, missing }
    })
    .await
    .map_err(|e| e.to_string())
}

/// How far a *Make PDF searchable* run is, for the status line.
#[derive(Serialize, Clone, Default)]
pub struct SearchableProgress {
    /// The PDF being worked on, counted from 0, of `all`.
    at: usize,
    all: usize,
    name: String,
    /// What OCRmyPDF (or the pull) said last.
    line: String,
}

static SEARCHABLE: Mutex<Option<SearchableProgress>> = Mutex::new(None);
static SEARCHABLE_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Make each of `paths` searchable, one after the other; one at a time in the app. The copies
/// made, and what failed (`path: why`, one per line).
#[tauri::command]
pub async fn searchable_run(paths: Vec<PathBuf>, ctx: tauri::State<'_, crate::Ctx>) -> Res<Vec<PathBuf>> {
    use std::sync::atomic::Ordering;
    for p in &paths {
        crate::here(p)?;
    }
    let cfg = ctx.cfg().preview.clone();
    {
        let mut s = SEARCHABLE.lock().unwrap();
        if s.is_some() {
            return Err(t!("searchable.busy_already"));
        }
        *s = Some(SearchableProgress { all: paths.len(), ..Default::default() });
    }
    SEARCHABLE_STOP.store(false, Ordering::Relaxed);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let set = |f: &dyn Fn(&mut SearchableProgress)| {
            if let Some(p) = SEARCHABLE.lock().unwrap().as_mut() {
                f(p)
            }
        };
        let names: Vec<String> = paths.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
        let done = coxswain_core::searchable::run_all(&cfg, &paths, &SEARCHABLE_STOP, |i| set(&|p| (p.at, p.name, p.line) = (i, names[i].clone(), String::new())), |said| {
            let line = match said {
                coxswain_core::searchable::Said::Pulling(l) => format!("{} {l}", t!("convert.pulling", "image" => coxswain_core::images::short(&cfg.images.get(coxswain_core::searchable::TOOL).cloned().unwrap_or_default()))),
                coxswain_core::searchable::Said::Line(l) => l.to_string(),
            };
            set(&|p| p.line = line.clone());
        });
        // The image was used only when the container ran, not the installed program.
        if let Ok(coxswain_core::searchable::How::Container { image, .. }) = coxswain_core::searchable::how(&cfg) {
            USED.lock().unwrap().insert(image);
        }
        // Done here, on this thread, so an app that is ending sees it (`searchable_end`).
        *SEARCHABLE.lock().unwrap() = None;
        let (mut made, mut failed) = (vec![], vec![]);
        for (i, r) in done.into_iter().enumerate() {
            match r {
                Ok(p) => made.push(p),
                Err(e) => failed.push(format!("{}: {e}", names.get(i).cloned().unwrap_or_default())),
            }
        }
        if failed.is_empty() { Ok(made) } else { Err(failed.join("\n")) }
    })
    .await
    .map_err(|e| e.to_string());
    *SEARCHABLE.lock().unwrap() = None;
    let used = std::mem::take(&mut *USED.lock().unwrap());
    if !used.is_empty() {
        let now = coxswain_core::fs::now();
        let _ = ctx.edit(|st| used.iter().for_each(|i| coxswain_core::images::used(st, i, now)));
    }
    result?
}

#[tauri::command]
pub fn searchable_progress() -> Option<SearchableProgress> {
    SEARCHABLE.lock().unwrap().clone()
}

/// Stop the run under way: the PDF being worked on gets no copy, the rest are not started.
#[tauri::command]
pub fn searchable_stop() {
    SEARCHABLE_STOP.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Stop the run under way and wait for it, ten seconds at most: the app is ending.
pub fn searchable_end() {
    searchable_stop();
    for _ in 0..100 {
        if SEARCHABLE.lock().unwrap().is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Has the PDF no text on its pages (a scan)? The preview then offers *Make PDF searchable*.
#[tauri::command]
pub async fn pdf_scanned(path: PathBuf) -> bool {
    // A PDF only in the cloud is not downloaded to be looked at.
    crate::here(&path).is_ok() && tauri::async_runtime::spawn_blocking(move || coxswain_core::searchable::scanned(&path)).await.unwrap_or(false)
}

/// Run to completion within `timeout`; on timeout kill it (and its container).
fn wait(c: Command, timeout: Duration, container: Option<(&Path, &str)>) -> Res<()> {
    let mut c = coxswain_core::tools::hosted(c);
    let mut child = coxswain_core::tools::own_group(&mut c).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let err = child.stderr.take();
    let reader = std::thread::spawn(move || err.map(tail).unwrap_or_default());
    let status = wait_child(&mut child, timeout, container)?;
    let stderr = reader.join().unwrap_or_default();
    if status.success() { Ok(()) } else { Err(last_lines(&stderr, 12).unwrap_or_else(|| t!("convert.failed", "status" => status))) }
}

/// Run with `input` on stdin and its stdout saved as `file`.
fn capture(c: Command, input: Option<&Path>, timeout: Duration, file: &Path, container: Option<(&Path, &str)>) -> Res<()> {
    let mut c = coxswain_core::tools::hosted(c);
    let mut child = coxswain_core::tools::own_group(&mut c).stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    if let (Some(p), Some(mut stdin)) = (input, child.stdin.take()) {
        let data = std::fs::read(p).map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }
    let (mut so, se) = (child.stdout.take(), child.stderr.take());
    let out_reader = std::thread::spawn(move || {
        let mut v = vec![];
        if let Some(o) = so.as_mut() {
            let _ = o.take(50 << 20).read_to_end(&mut v);
        }
        v
    });
    let err_reader = std::thread::spawn(move || se.map(tail).unwrap_or_default());
    let status = wait_child(&mut child, timeout, container)?;
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    if !status.success() || stdout.is_empty() {
        return Err(last_lines(&stderr, 12).unwrap_or_else(|| t!("convert.failed", "status" => status)));
    }
    std::fs::write(file, stdout).map_err(|e| e.to_string())
}

/// Wait within `timeout`; on timeout kill it (`tools::wait`) and its container.
fn wait_child(child: &mut std::process::Child, timeout: Duration, container: Option<(&Path, &str)>) -> Res<std::process::ExitStatus> {
    match coxswain_core::tools::wait(child, timeout) {
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            if let Some((rt, name)) = container {
                let _ = coxswain_core::tools::command(rt).args(["kill", name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            }
            Err(t!("convert.timed_out", "seconds" => timeout.as_secs()))
        }
        r => r.map_err(|e| e.to_string()),
    }
}

/// What a program wrote, to its end, of which the last 64 KiB are kept: the pipe stays open
/// until it is done, so a tool that says much is not stopped by a closed pipe.
fn tail(mut r: impl Read) -> String {
    const CAP: usize = 64 * 1024;
    let (mut keep, mut buf) = (Vec::new(), [0u8; 8192]);
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        keep.extend_from_slice(&buf[..n]);
        if keep.len() > 2 * CAP {
            keep.drain(..keep.len() - CAP);
        }
    }
    String::from_utf8_lossy(&keep[keep.len().saturating_sub(CAP)..]).into_owned()
}

fn last_lines(s: &str, n: usize) -> Option<String> {
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    (!lines.is_empty()).then(|| lines[lines.len().saturating_sub(n)..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engines_follow_preferences() {
        let mut cfg = PreviewConfig { container: "off".into(), ..Default::default() };
        let e = engines(&cfg, "latex");
        assert!(e.iter().all(|e| !e.id.starts_with("container:") || !e.available), "containers off");
        cfg.prefer = "container".into();
        assert!(engines(&cfg, "latex")[0].id.starts_with("container:"), "container listed first when preferred");
        cfg.prefer = "auto".into();
        cfg.tools.insert("latex".into(), "container".into());
        assert!(engines(&cfg, "latex")[0].id.starts_with("container:"), "the kind's choice wins");
        cfg.tools.insert("diagrams".into(), "off".into());
        assert_eq!(engines(&cfg, "d2").iter().map(|e| (e.id.as_str(), e.available)).collect::<Vec<_>>(), [("off:", false)], "off: nothing runs");
        assert!(engines(&cfg, "nonsense").iter().all(|e| !e.available));
    }

    #[test]
    fn typst_previews_are_made_again_when_an_included_file_changes() {
        let d = std::env::temp_dir().join(format!("coxswain-typst-cache-{}", std::process::id()));
        std::fs::create_dir_all(d.join("chapters")).unwrap();
        let main = d.join("main.typ");
        std::fs::write(&main, "#include \"chapters/one.typ\"\n").unwrap();
        std::fs::write(d.join("chapters/one.typ"), "= One\n").unwrap();
        let before = cache_dir(&main, &main, "typst", "local:typst").unwrap();
        let later = std::time::SystemTime::now() + Duration::from_secs(60);
        std::fs::File::options().write(true).open(d.join("chapters/one.typ")).unwrap().set_modified(later).unwrap();
        assert_ne!(cache_dir(&main, &main, "typst", "local:typst").unwrap(), before, "a chapter changed");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn tail_reads_to_the_end_and_keeps_the_last_part() {
        let said = "x".repeat(300 * 1024) + "\nthe error\n";
        let kept = tail(said.as_bytes());
        assert_eq!(kept.len(), 64 * 1024);
        assert!(kept.ends_with("the error\n"));
    }

    #[test]
    fn outputs_and_errors() {
        let out = Path::new("/tmp/x");
        assert_eq!(output("latex", Path::new("/a/paper.tex"), out), ("pdf", out.join("paper.pdf")));
        assert_eq!(output("plantuml", Path::new("/a/seq.puml"), out).0, "svg");
        assert_eq!(last_lines("a\n\nb\nc\n", 2).unwrap(), "b\nc");
        assert!(last_lines("\n \n", 3).is_none());
    }


    /// Needs podman and the network: `cargo test -- --ignored real_pull`.
    #[test]
    #[ignore]
    fn real_pull_streams_progress() {
        let rt = which("podman").unwrap();
        let image = "docker.io/library/alpine:latest";
        pull(&rt, image).unwrap();
        assert!(image_size(&rt, image).is_some());
        assert!(coxswain_core::images::pulling().is_empty());
        assert!(pull(&rt, "docker.io/library/no-such-image-coxswain:1").is_err());
    }

    /// Needs podman with the texlive image: `cargo test -- --ignored real_latex_project`.
    /// A chapter of a document whose picture is in a folder next to the document's own.
    #[test]
    #[ignore]
    fn real_latex_project_in_a_container() {
        let d = std::env::temp_dir().join(format!("coxswain-test-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for sub in ["paper/chapters", "figures", "out-project"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
        // A one-pixel PNG.
        let png: [u8; 67] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0d, 0x0a, 0x2d, 0xb4, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82];
        std::fs::write(d.join("figures/dot.png"), png).unwrap();
        std::fs::write(d.join("paper/main.tex"), "\\documentclass{article}\\usepackage{graphicx}\\begin{document}\\includegraphics{../figures/dot.png}\\input{chapters/method}\\end{document}\n").unwrap();
        let chapter = d.join("paper/chapters/method.tex");
        std::fs::write(&chapter, "% !TEX root = ../main.tex\n\\section{Method}\n").unwrap();
        let chapter = latex::main_file(&chapter);
        let (_, pdf) = output("latex", &chapter, &d.join("out-project"));
        assert!(pdf.ends_with("out-project/main.pdf"), "{pdf:?}");
        if let Err(e) = run(&PreviewConfig::default(), "latex", "container:podman", &chapter, &d.join("out-project"), &pdf) {
            panic!("{}", failure("latex", &chapter, &d.join("out-project"), &e));
        }
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Needs podman with the texlive image and LibreOffice: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_latex_container_and_libreoffice() {
        let d = std::env::temp_dir().join(format!("coxswain-test-convert-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("out")).unwrap();
        std::fs::write(d.join("paper.tex"), "\\documentclass{article}\\begin{document}Hello $E=mc^2$\\end{document}\n").unwrap();
        let cfg = PreviewConfig::default();
        let (_, pdf) = output("latex", &d.join("paper.tex"), &d.join("out"));
        run(&cfg, "latex", "container:podman", &d.join("paper.tex"), &d.join("out"), &pdf).unwrap();
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));

        std::fs::write(d.join("broken.tex"), "\\documentclass{article}\\begin{document}\\undefinedmacro\\end{document}\n").unwrap();
        std::fs::create_dir_all(d.join("out2")).unwrap();
        let (_, pdf2) = output("latex", &d.join("broken.tex"), &d.join("out2"));
        let err = run(&cfg, "latex", "container:podman", &d.join("broken.tex"), &d.join("out2"), &pdf2).unwrap_err();
        let msg = failure("latex", &d.join("broken.tex"), &d.join("out2"), &err);
        assert!(msg.contains("Undefined control sequence"), "{msg}");

        std::fs::write(d.join("note.rtf"), "{\\rtf1\\ansi Hello from RTF}").unwrap();
        std::fs::create_dir_all(d.join("out3")).unwrap();
        let (_, pdf3) = output("libreoffice", &d.join("note.rtf"), &d.join("out3"));
        run(&cfg, "libreoffice", "local:soffice", &d.join("note.rtf"), &d.join("out3"), &pdf3).unwrap();
        assert!(std::fs::read(&pdf3).unwrap().starts_with(b"%PDF"));
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Needs podman (a `podman` first on PATH may add a store of its own) and the network:
    /// `COXSWAIN_FILES=<folder> cargo test -p coxswain-gui -- --ignored real_new_images --nocapture`.
    /// The folder holds a.d2, a.ditaa, a.bob, a.pikchr, a.nomnoml, a.wavedrom, a.vl, a.dbml,
    /// a.vg, a.er, a.bytefield, report.doc, notes.ps, patch.eps, plan.typ, clip.wmf, clip.emf,
    /// report.fodt, a.puml and a.rst; each is made in its
    /// container as the preview would, and the time and size of each result printed.
    #[test]
    #[ignore]
    fn real_new_images() {
        let src = PathBuf::from(std::env::var("COXSWAIN_FILES").expect("COXSWAIN_FILES"));
        let mut cfg = PreviewConfig { prefer: "container".into(), ..Default::default() };
        cfg.container = "podman".into();
        let files = [
            ("a.d2", "d2"), ("a.ditaa", "ditaa"), ("a.bob", "svgbob"), ("a.pikchr", "pikchr"), ("a.nomnoml", "nomnoml"),
            ("a.wavedrom", "wavedrom"), ("a.vl", "vegalite"), ("a.vg", "vega"), ("a.dbml", "dbml"), ("a.er", "erd"), ("a.bytefield", "bytefield"),
            ("report.doc", "libreoffice"), ("notes.ps", "ghostscript"), ("patch.eps", "ghostscript"), ("plan.typ", "typst"),
            ("clip.wmf", "libreoffice"), ("clip.emf", "libreoffice"), ("report.fodt", "libreoffice"), ("a.puml", "plantuml"), ("a.rst", "pandoc"),
        ];
        for (name, tool) in files {
            let path = src.join(name);
            let out = std::env::temp_dir().join(format!("coxswain-real-new-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&out);
            std::fs::create_dir_all(&out).unwrap();
            let (kind, file) = output(tool, &path, &out);
            let start = std::time::Instant::now();
            run(&cfg, tool, "container:podman", &path, &out, &file).unwrap_or_else(|e| panic!("{name}: {e}"));
            let bytes = std::fs::read(&file).unwrap();
            assert!(match kind { "pdf" => bytes.starts_with(b"%PDF"), "svg" => String::from_utf8_lossy(&bytes).contains("<svg"), _ => !bytes.is_empty() }, "{name}");
            eprintln!("{name:12} {tool:12} {kind} {:>7} bytes {:>6} ms", bytes.len(), start.elapsed().as_millis());
            std::fs::remove_dir_all(out).unwrap();
        }
    }

    /// Fills the preview cache for screenshots, as a click on Build would:
    /// `COXSWAIN_WARM="path|tool|engine" test-binary warm_cache --ignored`.
    #[test]
    #[ignore]
    fn warm_cache() {
        let spec = std::env::var("COXSWAIN_WARM").expect("COXSWAIN_WARM=path|tool|engine");
        let [path, tool, engine]: [&str; 3] = spec.split('|').collect::<Vec<_>>().try_into().unwrap();
        let path = PathBuf::from(path);
        let main = if tool == "latex" { latex::main_file(&path) } else { path.clone() };
        let out = cache_dir(&path, &main, tool, engine).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let (_, file) = output(tool, &main, &out);
        let path = main;
        // COXSWAIN_WARM_COPY: a result made outside (where the container runtime is), copied in.
        match std::env::var("COXSWAIN_WARM_COPY") {
            Ok(src) => drop(std::fs::copy(src, &file).unwrap()),
            Err(_) => run(&PreviewConfig::default(), tool, engine, &path, &out, &file).unwrap(),
        }
        assert!(file.is_file());
    }
}

// ---------------------------------------------------------------- ffmpeg and ImageMagick

/// A media preview made without asking (`tool` as in `preview_tools::KINDS`): with the first engine that runs
/// without a pull, else none; the preview's own picture when that was made with it. For the
/// thumbnails and a song's waveform under its player. One at a time.
#[tauri::command]
pub async fn media_thumbnail(path: PathBuf, tool: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<Option<PathBuf>> {
    static ONE: Mutex<()> = Mutex::new(());
    crate::here(&path)?;
    if media_op(&tool).is_none() || coxswain_core::cloud::unasked(&path) {
        return Ok(None);
    }
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(eng) = engines(&cfg, &tool).into_iter().find(|e| e.available && !e.needs_pull) else { return Ok(None) };
        let out = cache_dir(&path, &path, &tool, &eng.id)?;
        let (_, file) = output(&tool, &path, &out);
        if file.is_file() {
            return Ok(Some(file));
        }
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        if file.is_file() {
            return Ok(Some(file));
        }
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        build(&cfg, &tool, &eng.id, &path, &out, &file, None).map(|_| file.is_file().then_some(file))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// *Convert video or audio* or *Convert or edit pictures*: what `op` would do with `paths`, for
/// the question before it. `files` are those of `paths` it takes.
#[derive(Serialize)]
pub struct MediaPlan {
    files: Vec<PathBuf>,
    /// What it writes, which program runs, and the first pull with its size.
    about: String,
    pull: bool,
    /// Why it cannot run here: what to install or which setting to change.
    missing: Option<String>,
}

#[tauri::command]
pub async fn media_plan(paths: Vec<PathBuf>, op: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<MediaPlan> {
    use coxswain_core::media;
    let op = media::Op::parse(&op).ok_or_else(|| t!("convert.unknown_engine"))?;
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // A file only in the cloud is not fetched to be read: it is left out.
        let files: Vec<PathBuf> = paths.into_iter().filter(|p| op.takes(p) && crate::here(p).is_ok() && p.is_file()).collect();
        let (about, pull, missing) = match media::how(&cfg, op.program()) {
            Ok(how) => {
                let (about, pull) = media::about(&how);
                (format!("{} {about}", media::confirm(&op, &files)), pull, None)
            }
            Err(e) => (String::new(), false, Some(e)),
        };
        MediaPlan { files, about, pull, missing }
    })
    .await
    .map_err(|e| e.to_string())
}

static MEDIA: Mutex<Option<SearchableProgress>> = Mutex::new(None);
static MEDIA_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Do `op` on each of `paths`, one after the other, one run at a time in the app: the files
/// made, or what failed (`name: why`, one per line).
#[tauri::command]
pub async fn media_run(paths: Vec<PathBuf>, op: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<Vec<PathBuf>> {
    use coxswain_core::media::{self, Said};
    use std::sync::atomic::Ordering;
    let op = media::Op::parse(&op).ok_or_else(|| t!("convert.unknown_engine"))?;
    for p in &paths {
        crate::here(p)?;
    }
    let cfg = ctx.cfg().preview.clone();
    {
        let mut s = MEDIA.lock().unwrap();
        if s.is_some() {
            return Err(t!("media.busy_already"));
        }
        *s = Some(SearchableProgress { all: paths.len(), ..Default::default() });
    }
    MEDIA_STOP.store(false, Ordering::Relaxed);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let set = |f: &dyn Fn(&mut SearchableProgress)| {
            if let Some(p) = MEDIA.lock().unwrap().as_mut() {
                f(p)
            }
        };
        let tool = op.program().tool();
        let names: Vec<String> = paths.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
        let done = media::run_all(&cfg, &op, &paths, &MEDIA_STOP, |i| set(&|p| (p.at, p.name, p.line) = (i, names[i].clone(), String::new())), |said| {
            let line = match said {
                Said::Pulling(l) => format!("{} {l}", t!("convert.pulling", "image" => coxswain_core::images::short(&cfg.images.get(tool).cloned().unwrap_or_default()))),
                Said::Percent(n) => format!("{n} %"),
                Said::Line(l) => l.to_string(),
            };
            set(&|p| p.line = line.clone());
        });
        if let Ok(media::How::Container { image, .. }) = media::how(&cfg, op.program()) {
            USED.lock().unwrap().insert(image);
        }
        *MEDIA.lock().unwrap() = None;
        let (mut made, mut failed) = (vec![], vec![]);
        for (i, r) in done.into_iter().enumerate() {
            match r {
                Ok(p) => made.push(p),
                Err(e) => failed.push(format!("{}: {e}", names.get(i).cloned().unwrap_or_default())),
            }
        }
        if failed.is_empty() { Ok(made) } else { Err(failed.join("\n")) }
    })
    .await
    .map_err(|e| e.to_string());
    *MEDIA.lock().unwrap() = None;
    let used = std::mem::take(&mut *USED.lock().unwrap());
    if !used.is_empty() {
        let now = coxswain_core::fs::now();
        let _ = ctx.edit(|st| used.iter().for_each(|i| coxswain_core::images::used(st, i, now)));
    }
    result?
}

#[tauri::command]
pub fn media_progress() -> Option<SearchableProgress> {
    MEDIA.lock().unwrap().clone()
}

/// Stop the run under way: the file being worked on gets no result, the rest are not started.
#[tauri::command]
pub fn media_stop() {
    MEDIA_STOP.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Stop the run under way and wait for it, ten seconds at most: the app is ending.
pub fn media_end() {
    media_stop();
    for _ in 0..100 {
        if MEDIA.lock().unwrap().is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A GPX file's map, drawn without GDAL (`coxswain_core::geo::gpx_map`), kept in the preview
/// cache like the others; for its preview and its thumbnail.
#[tauri::command]
pub async fn gpx_map(path: PathBuf) -> Res<Option<PathBuf>> {
    crate::here(&path)?;
    if coxswain_core::cloud::unasked(&path) {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let out = cache_dir(&path, &path, "gpx", "coxswain")?;
        let file = out.join("map.svg");
        if !file.is_file() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            std::fs::write(&file, coxswain_core::geo::gpx_map(&path)?).map_err(|e| e.to_string())?;
        }
        Ok(Some(file))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// *Build PDF of AsciiDoc* and *Export draw.io diagram* (`coxswain_core::export`): what it
/// would do with `paths`, for the question before it.
#[derive(Serialize)]
pub struct ExportPlan {
    /// The files among those picked it takes.
    files: Vec<PathBuf>,
    /// What it writes, which program runs, and the first pull with its size.
    about: String,
    /// Why it cannot run here: what to install, or which setting to change.
    missing: Option<String>,
    /// draw.io exports are off: Settings opens at their switch.
    off: bool,
}

#[tauri::command]
pub async fn export_plan(paths: Vec<PathBuf>, op: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<ExportPlan> {
    use coxswain_core::export;
    let op = export::Op::from_id(&op).ok_or_else(|| t!("convert.unknown_engine"))?;
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // A file only in the cloud is not fetched to be read: it is left out.
        let files: Vec<PathBuf> = paths.into_iter().filter(|p| op.takes(p) && crate::here(p).is_ok() && p.is_file()).collect();
        let (about, missing) = match export::how(&cfg, op.tool()) {
            Ok(how) => (format!("{} {}", export::confirm(op, &files), export::about(&how).0), None),
            Err(e) => (String::new(), Some(e)),
        };
        ExportPlan { files, about, missing, off: op.tool() == export::DRAWIO && !cfg.drawio }
    })
    .await
    .map_err(|e| e.to_string())
}

static EXPORT: Mutex<Option<SearchableProgress>> = Mutex::new(None);
static EXPORT_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Make `op` of each of `paths`, one after the other; one export at a time in the app. What was
/// written, and what failed (`name: why`, one per line).
#[tauri::command]
pub async fn export_run(paths: Vec<PathBuf>, op: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<Vec<PathBuf>> {
    use coxswain_core::export::{self, Said};
    use std::sync::atomic::Ordering;
    let op = export::Op::from_id(&op).ok_or_else(|| t!("convert.unknown_engine"))?;
    for p in &paths {
        crate::here(p)?;
    }
    let cfg = ctx.cfg().preview.clone();
    {
        let mut s = EXPORT.lock().unwrap();
        if s.is_some() {
            return Err(t!("export.busy_already"));
        }
        *s = Some(SearchableProgress { all: paths.len(), ..Default::default() });
    }
    EXPORT_STOP.store(false, Ordering::Relaxed);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let set = |f: &dyn Fn(&mut SearchableProgress)| {
            if let Some(p) = EXPORT.lock().unwrap().as_mut() {
                f(p)
            }
        };
        let names: Vec<String> = paths.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
        let done = export::run_all(&cfg, op, &paths, &EXPORT_STOP, |i| set(&|p| (p.at, p.name, p.line) = (i, names[i].clone(), String::new())), |said| {
            let line = match said {
                Said::Pulling(l) => format!("{} {l}", t!("convert.pulling", "image" => coxswain_core::images::short(&cfg.images.get(op.tool()).cloned().unwrap_or_default()))),
                Said::Line(l) => l.to_string(),
            };
            set(&|p| p.line = line.clone());
        });
        // The image was used only when the container ran, not the installed program.
        if let Ok(export::How::Container { image, .. }) = export::how(&cfg, op.tool()) {
            USED.lock().unwrap().insert(image);
        }
        // Done here, on this thread, so an app that is ending sees it (`export_end`).
        *EXPORT.lock().unwrap() = None;
        let (mut made, mut failed) = (vec![], vec![]);
        for (i, r) in done.into_iter().enumerate() {
            match r {
                Ok(p) => made.extend(p),
                Err(e) => failed.push(format!("{}: {e}", names.get(i).cloned().unwrap_or_default())),
            }
        }
        if failed.is_empty() { Ok(made) } else { Err(failed.join("\n")) }
    })
    .await
    .map_err(|e| e.to_string());
    *EXPORT.lock().unwrap() = None;
    let used = std::mem::take(&mut *USED.lock().unwrap());
    if !used.is_empty() {
        let now = coxswain_core::fs::now();
        let _ = ctx.edit(|st| used.iter().for_each(|i| coxswain_core::images::used(st, i, now)));
    }
    result?
}

#[tauri::command]
pub fn export_progress() -> Option<SearchableProgress> {
    EXPORT.lock().unwrap().clone()
}

/// Stop the export under way: the file being worked on gets nothing, the rest are not started.
#[tauri::command]
pub fn export_stop() {
    EXPORT_STOP.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Stop the export under way and wait for it, ten seconds at most: the app is ending.
pub fn export_end() {
    export_stop();
    for _ in 0..100 {
        if EXPORT.lock().unwrap().is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The diagram of a draw.io file, a `.drawio.svg` or a `.drawio.png`, as draw.io's XML, for its
/// viewer (`export::drawio_xml`). None for a file without one, or only in the cloud.
#[tauri::command]
pub async fn drawio_xml(path: PathBuf) -> Option<String> {
    if crate::here(&path).is_err() {
        return None;
    }
    tauri::async_runtime::spawn_blocking(move || coxswain_core::export::drawio_xml(&path)).await.ok().flatten()
}

/// The first page of a diagram as a 256-pixel PNG, for the grid view: made once by draw.io and
/// kept in the previews cache. None while draw.io exports are off, before its image is pulled,
/// for a file only in the cloud, or when it cannot be made.
#[tauri::command]
pub async fn drawio_thumbnail(path: PathBuf, ctx: tauri::State<'_, crate::Ctx>) -> Res<Option<PathBuf>> {
    let cfg = ctx.cfg().preview.clone();
    if !cfg.drawio || crate::here(&path).is_err() {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let out = cache_dir(&path, &path, "drawio-thumb", "")?;
        let png = out.join("thumb.png");
        if png.is_file() {
            return Ok(Some(png));
        }
        // A thumbnail that could not be made is not tried again until the file changes.
        if out.join("none").exists() {
            return Ok(None);
        }
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        match coxswain_core::export::drawio_thumbnail(&cfg, &path, &png, 256, Duration::from_secs(cfg.timeout.max(10))) {
            Ok(()) => Ok(Some(png)),
            Err(_) => {
                let _ = std::fs::write(out.join("none"), "");
                Ok(None)
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// An AsciiDoc document for the preview: its diagram blocks (`export::blocks`) each made a
/// placeholder the preview draws into, and the blocks themselves.
#[derive(Serialize)]
pub struct AdocPrepared {
    source: String,
    blocks: Vec<coxswain_core::export::Block>,
}

#[tauri::command]
pub fn adoc_blocks(text: String) -> AdocPrepared {
    let (source, blocks) = coxswain_core::export::with_placeholders(&text);
    AdocPrepared { source, blocks }
}

/// A diagram of an AsciiDoc document as SVG, drawn here by its program, installed or in an
/// image already pulled (`export::diagram_svg`), and kept in the previews cache; what was
/// missing when it could not be drawn. The preview shows it as a picture (a data URI).
#[tauri::command]
pub async fn adoc_diagram(kind: String, text: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<String> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        ("adoc-diagram", &kind, &text).hash(&mut h);
        let dir = previews_dir()?.join(format!("{:016x}", h.finish()));
        let svg = dir.join("diagram.svg");
        if let Ok(kept) = std::fs::read_to_string(&svg) {
            return Ok(kept);
        }
        let drawn = coxswain_core::export::diagram_svg(&cfg, &kind, &text)?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(&svg, &drawn).map_err(|e| e.to_string())?;
        Ok(drawn)
    })
    .await
    .map_err(|e| e.to_string())?
}

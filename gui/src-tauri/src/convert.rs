//! Previews made by external tools: LaTeX, LibreOffice, PlantUML, pandoc and DuckDB.
//!
//! Each tool runs either from a local install or from a container image (podman or docker),
//! chosen per `[preview]` in the config and by the buttons in the preview pane. Results go to
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
use std::time::{Duration, Instant};

type Res<T> = Result<T, String>;

/// The tools, with the local programs that can do the job, best first.
const TOOLS: &[(&str, &[&str])] = &[
    ("latex", &["latexmk", "tectonic", "pdflatex"]),
    ("libreoffice", &["soffice", "libreoffice"]),
    ("plantuml", &["plantuml"]),
    ("pandoc", &["pandoc"]),
    ("duckdb", &["duckdb"]),
];

use coxswain_core::tools::which;

/// podman or docker, as the config allows.
fn runtime(cfg: &PreviewConfig) -> Option<(&'static str, PathBuf)> {
    let order: &[&'static str] = match cfg.container.as_str() {
        "off" => &[],
        "podman" => &["podman"],
        "docker" => &["docker"],
        _ => &["podman", "docker"],
    };
    order.iter().find_map(|&r| which(r).map(|p| (r, p)))
}

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
}

/// The ways `tool` can run here, in the order of preference; the first available one is the
/// default.
#[tauri::command]
pub async fn preview_engines(tool: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<Vec<Engine>> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || engines(&cfg, &tool)).await.map_err(|e| e.to_string())
}

fn engines(cfg: &PreviewConfig, tool: &str) -> Vec<Engine> {
    let programs = TOOLS.iter().find(|(t, _)| *t == tool).map_or(&[][..], |(_, p)| *p);
    let mut local: Vec<Engine> = programs
        .iter()
        .map(|p| {
            let found = which(p);
            Engine { id: format!("local:{p}"), label: p.to_string(), available: found.is_some(), note: if found.is_some() { String::new() } else { t!("convert.not_installed", "program" => p) }, needs_pull: false }
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
    let image = cfg.images.get(tool).cloned().unwrap_or_default();
    let container = match (runtime(cfg), image.is_empty()) {
        (_, true) => Engine { id: "container:".into(), label: t!("convert.container"), available: false, note: t!("convert.no_image", "tool" => tool), needs_pull: false },
        (None, false) => Engine { id: "container:".into(), label: t!("convert.container"), available: false, note: t!("convert.no_runtime"), needs_pull: false },
        (Some((rt, path)), false) => {
            let pulled = image_size(&path, &image).is_some();
            let short = image.rsplit('/').next().unwrap_or(&image).to_string();
            Engine {
                id: format!("container:{rt}"),
                label: format!("{rt} {short}"),
                available: true,
                note: if pulled {
                    t!("convert.runs_offline", "image" => image)
                } else if tool == "latex" {
                    t!("convert.first_run_pulls_large", "image" => image, "size" => "5 GB")
                } else {
                    t!("convert.first_run_pulls", "image" => image)
                },
                needs_pull: !pulled,
            }
        }
    };
    let pref = cfg.prefer_tool.get(tool).unwrap_or(&cfg.prefer).as_str();
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
}

fn cache_dir(path: &Path, tool: &str, engine: &str) -> Res<PathBuf> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, meta.len(), meta.modified().ok(), tool, engine).hash(&mut h);
    if tool == "latex" {
        // A chapter, a picture or the bibliography changed: that is a new document too.
        latex::newest(&latex::main_file(path)).hash(&mut h);
    }
    let dir = previews_dir()?.join(format!("{:016x}", h.finish()));
    Ok(dir)
}

/// What each tool produces, and where.
fn output(tool: &str, path: &Path, out: &Path) -> (&'static str, PathBuf) {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    match tool {
        "latex" => ("pdf", out.join(latex::main_file(path).with_extension("pdf").file_name().unwrap_or_default())),
        "libreoffice" => ("pdf", out.join(format!("{stem}.pdf"))),
        "plantuml" => ("svg", out.join("diagram.svg")),
        "pandoc" => ("html", out.join("doc.html")),
        _ => ("json", out.join("tables.json")),
    }
}

/// Render `path` with `tool` using `engine` (an id from `preview_engines`). With `cached_only`,
/// returns the earlier result if there is one and does nothing otherwise.
#[tauri::command]
pub async fn convert(path: PathBuf, tool: String, engine: String, cached_only: bool, ctx: tauri::State<'_, crate::Ctx>) -> Res<Option<Converted>> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let out = cache_dir(&path, &tool, &engine)?;
        let (kind, file) = output(&tool, &path, &out);
        // What the build said is kept next to its result, so the next look says it too.
        let said = |what: &str| std::fs::read_to_string(out.join(what)).ok().filter(|s| !s.is_empty());
        let done = |file: PathBuf| -> Res<Option<Converted>> {
            let (note, instead) = (said("note.txt"), said("instead.txt"));
            Ok(Some(match kind {
                "pdf" | "svg" => Converted { kind, file: Some(file), text: None, note, instead },
                _ => Converted { kind, file: None, text: Some(std::fs::read_to_string(&file).map_err(|e| e.to_string())?), note, instead },
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
        match run(&cfg, &tool, &engine, &path, &out, &file) {
            Ok(()) if file.is_file() => done(file),
            // A document with an error still makes a PDF: shown, with the error above it.
            Err(e) if file.is_file() => {
                let _ = std::fs::write(out.join("note.txt"), failure(&tool, &path, &out, &e));
                done(file)
            }
            Ok(()) => Err(failure(&tool, &path, &out, &t!("convert.no_result"))),
            Err(e) => {
                let why = failure(&tool, &path, &out, &e);
                // A LaTeX build that one engine cannot make, another often can (tectonic is
                // XeTeX only, a TeX Live container has everything): the others are tried, and
                // what they make is kept as this engine's result, so the next look finds it.
                if tool == "latex" {
                    let first = engines(&cfg, &tool).into_iter().find(|x| x.id == engine).map_or(engine.clone(), |x| x.label);
                    for other in engines(&cfg, &tool).into_iter().filter(|x| x.available && !x.needs_pull && x.id != engine) {
                        let _ = std::fs::remove_dir_all(&out);
                        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                        if run(&cfg, &tool, &other.id, &path, &out, &file).is_ok() || file.is_file() {
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
    .map_err(|e| e.to_string())?
}

fn previews_dir() -> Res<PathBuf> {
    Ok(dirs::cache_dir().ok_or_else(|| t!("err.no_cache_folder"))?.join("coxswain").join("previews"))
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
    let log = out.join(latex::main_file(path).with_extension("log").file_name().unwrap_or_default());
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
    if tool == "latex" && first.is_err() && !file.is_file() && latex::engine_flag(&latex::main_file(path)) == "-pdf" {
        let log = out.join(latex::main_file(path).with_extension("log").file_name().unwrap_or_default());
        if std::fs::read_to_string(log).is_ok_and(|l| l.contains("requires either XeTeX or") || l.contains("requires either XeTeX or LuaTeX")) {
            let _ = std::fs::remove_dir_all(out);
            std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
            return build(cfg, tool, engine, path, out, file, Some("-pdfxe"));
        }
    }
    first
}

/// One run of `tool`. `flag`: latexmk's engine, when not the document's own.
fn build(cfg: &PreviewConfig, tool: &str, engine: &str, path: &Path, out: &Path, file: &Path, flag: Option<&'static str>) -> Res<()> {
    // A chapter builds its document, not itself.
    let main = if tool == "latex" { latex::main_file(path) } else { path.to_path_buf() };
    let path = main.as_path();
    let dir = path.parent().ok_or_else(|| t!("err.no_folder"))?;
    let name = path.file_name().ok_or_else(|| t!("err.no_file_name"))?.to_string_lossy().into_owned();
    let timeout = Duration::from_secs(cfg.timeout.max(10));
    let (kind, which_one) = engine.split_once(':').ok_or_else(|| t!("convert.unknown_engine"))?;

    if kind == "container" {
        let (rt, rt_path) = runtime(cfg).ok_or_else(|| t!("convert.no_runtime"))?;
        if rt != which_one {
            return Err(t!("convert.not_available", "program" => which_one));
        }
        let image = cfg.images.get(tool).filter(|i| !i.is_empty()).ok_or_else(|| t!("convert.no_container_image", "tool" => tool))?;
        if image_size(&rt_path, image).is_none() {
            pull(&rt_path, image)?;
        }
        let container_name = format!("coxswain-preview-{}", out.file_name().unwrap_or_default().to_string_lossy());
        let mut c = coxswain_core::tools::command(&rt_path);
        c.args(["run", "--rm", "--network=none", "--security-opt", "label=disable", "--name", &container_name]);
        // LaTeX sees its whole project, so `../figures/plot.pdf` is found; the rest only their folder.
        let mount = if tool == "latex" { latex::project(path) } else { dir.to_path_buf() };
        let workdir = Path::new("/src").join(dir.strip_prefix(&mount).unwrap_or(Path::new("")));
        c.arg("-v").arg(format!("{}:/src:ro", mount.display())).arg("-v").arg(format!("{}:/out", out.display()));
        #[cfg(unix)]
        if rt == "docker" {
            // Rootful docker would leave root-owned files in the cache.
            use std::os::unix::fs::MetadataExt;
            let m = std::fs::metadata(out).map_err(|e| e.to_string())?;
            c.arg("--user").arg(format!("{}:{}", m.uid(), m.gid()));
        }
        c.arg("-w").arg(workdir.to_string_lossy().replace('\\', "/"));
        let stdin = match tool {
            "latex" => {
                c.args([image.as_str(), "latexmk", flag.unwrap_or_else(|| latex::engine_flag(path)), "-interaction=nonstopmode", "-norc", "-outdir=/out", &name]);
                None
            }
            "libreoffice" => {
                c.args([image.as_str(), "soffice", "--headless", "--convert-to", "pdf", "--outdir", "/out", &format!("/src/{name}")]);
                None
            }
            // The official images' entry points are the tools themselves.
            "plantuml" => {
                c.args(["-e", "PLANTUML_SECURITY_PROFILE=ALLOWLIST", "-e", "JAVA_TOOL_OPTIONS=-Dplantuml.allowlist.path=/src", "-i", image, "-tsvg", "-pipe"]);
                Some(path)
            }
            "pandoc" => {
                c.args(["-i", image, "--sandbox", "-f", "rst", "-t", "html5"]);
                Some(path)
            }
            _ => {
                c.args([image.as_str(), "-readonly", "-json", &format!("/src/{name}"), "-c", DUCKDB_SQL]);
                return capture(c, None, timeout, file, Some((&rt_path, &container_name)));
            }
        };
        return if stdin.is_some() { capture(c, stdin, timeout, file, Some((&rt_path, &container_name))) } else { wait(c, timeout, Some((&rt_path, &container_name))) };
    }

    let program = which(which_one).ok_or_else(|| t!("convert.not_installed", "program" => which_one))?;
    let mut c = coxswain_core::tools::command(&program);
    c.current_dir(dir);
    match (tool, which_one) {
        ("latex", "latexmk") => {
            // No -halt-on-error: past a first error LaTeX usually still makes the PDF.
            c.args([flag.unwrap_or_else(|| latex::engine_flag(path)), "-interaction=nonstopmode"]).arg(format!("-outdir={}", out.display()));
            latex::own_rc(&mut c);
            c.arg(&name);
        }
        ("latex", "tectonic") => {
            c.arg("--outdir").arg(out).arg(&name);
        }
        ("latex", _) => {
            // A document that asks for XeLaTeX or LuaLaTeX gets it when TeX Live has it.
            let wanted = match flag.unwrap_or_else(|| latex::engine_flag(path)) {
                "-pdfxe" => which("xelatex"),
                "-pdflua" => which("lualatex"),
                _ => None,
            };
            if let Some(other) = wanted {
                c = coxswain_core::tools::command(&other);
                c.current_dir(dir);
            }
            c.args(["-interaction=nonstopmode", "-halt-on-error"]).arg(format!("-output-directory={}", out.display())).arg(&name);
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

const DUCKDB_SQL: &str = "SELECT schema_name, table_name, estimated_size AS rows, column_count AS columns FROM duckdb_tables() ORDER BY 1, 2";

/// What a LaTeX editor works out before building: which file is the document, which folder
/// is the project, and which engine it asks for.
mod latex {
    use std::path::{Path, PathBuf};

    fn head(path: &Path) -> String {
        use std::io::Read;
        let mut s = String::new();
        if let Ok(f) = std::fs::File::open(path) {
            let _ = f.take(64 * 1024).read_to_string(&mut s);
        }
        s
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

    /// The document `path` belongs to: itself, the file its `% !TEX root` names, or for a
    /// file without `\documentclass` the document nearby that includes it.
    pub fn main_file(path: &Path) -> PathBuf {
        let text = head(path);
        let dir = path.parent().unwrap_or(Path::new("."));
        if let Some(root) = magic(&text, "root") {
            let p = dir.join(root);
            if p.is_file() {
                return std::fs::canonicalize(&p).unwrap_or(p);
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
                let t = std::fs::read_to_string(p).unwrap_or_default();
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
        let text = std::fs::read_to_string(main).unwrap_or_default();
        let depth = text.split(|c: char| c == '{' || c == ',' || c.is_whitespace()).map(|arg| arg.matches("../").count()).max().unwrap_or(0).min(3);
        let home = std::env::home_dir();
        dir.ancestors().take(depth + 1).take_while(|d| d.parent().is_some() && Some(*d) != home.as_deref()).last().unwrap_or(dir).to_path_buf()
    }

    /// What a build reads: sources, styles, bibliographies, pictures and data for plots.
    // ponytail: by extension; a document that inputs another kind is rebuilt with Build.
    const READ: &[&str] = &["tex", "ltx", "sty", "cls", "clo", "def", "cfg", "dtx", "bib", "bst", "bbx", "cbx", "png", "jpg", "jpeg", "pdf", "eps", "svg", "tikz", "pgf", "csv", "dat"];

    /// When anything a build reads in the document's folder last changed. Other files there
    /// (a database a preview opened, an editor's backup) are not part of it: with them, the
    /// preview was made again every time.
    pub fn newest(main: &Path) -> Option<std::time::SystemTime> {
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
                } else if p.extension().is_some_and(|x| READ.iter().any(|r| x.eq_ignore_ascii_case(r))) {
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
        let home = std::env::home_dir().unwrap_or_default();
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
            assert_eq!(main_file(&d.join("paper/chapters/intro.tex")), main, "found by who includes it");
            assert_eq!(main_file(&d.join("paper/chapters/method.tex")), std::fs::canonicalize(&main).unwrap(), "found by its magic comment");
            assert_eq!(project(&main), d, "one `../` up");
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
            let before = newest(&main).unwrap();
            let later = before + std::time::Duration::from_secs(60);
            std::fs::File::options().write(true).open(d.join("paper/chapters/intro.tex")).unwrap().set_modified(later).unwrap();
            assert_eq!(newest(&main), Some(later), "a chapter changed");
            std::fs::write(d.join("paper/data.db-shm"), "").unwrap();
            std::fs::File::options().write(true).open(d.join("paper/data.db-shm")).unwrap().set_modified(later + std::time::Duration::from_secs(60)).unwrap();
            assert_eq!(newest(&main), Some(later), "a file the build does not read changes nothing");

            std::fs::create_dir_all(d.join(".git")).unwrap();
            assert_eq!(project(&d.join("paper/chapters/intro.tex")), d, "the repository");
            let _ = std::fs::remove_dir_all(&d);
        }
    }
}

/// The size of `image` in bytes, if the runtime has it.
fn image_size(rt: &Path, image: &str) -> Option<u64> {
    let out = coxswain_core::tools::command(rt).args(["image", "inspect", "--format", "{{.Size}}", image]).stdin(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().parse().ok()).flatten()
}

/// The last progress line of every pull under way, by image, for the GUI to show.
static PULLING: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

/// Pull (or update) `image`, streaming its progress into `PULLING`. Not covered by the
/// timeout: images can be gigabytes. Two windows pulling the same image at once are fine, the
/// runtime shares one download.
fn pull(rt: &Path, image: &str) -> Res<()> {
    PULLING.lock().unwrap().insert(image.to_string(), String::new());
    let result = (|| {
        let mut child = coxswain_core::tools::command(rt).args(["pull", image]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
        // docker reports on stdout, podman on stderr; errors come on stderr.
        let (out, err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
        let name = image.to_string();
        let t = std::thread::spawn(move || follow(out, &name));
        let last_err = follow(err, image);
        let _ = t.join();
        let status = child.wait().map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err(t!("convert.pull_failed", "image" => image, "error" => last_err)) }
    })();
    PULLING.lock().unwrap().remove(image);
    result
}

/// Keep the latest line (progress bars redraw with `\r`) of `r` in `PULLING` for `image`;
/// returns the last one.
fn follow(mut r: impl Read, image: &str) -> String {
    let (mut buf, mut acc, mut last) = ([0u8; 4096], String::new(), String::new());
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        acc.push_str(&String::from_utf8_lossy(&buf[..n]));
        let Some(i) = acc.rfind(['\n', '\r']) else { continue };
        if let Some(line) = acc[..i].rsplit(['\n', '\r']).map(str::trim).find(|l| !l.is_empty()) {
            last = line.to_string();
            PULLING.lock().unwrap().insert(image.to_string(), last.clone());
        }
        acc.drain(..=i);
    }
    last
}

#[derive(Serialize)]
pub struct Image {
    tool: String,
    image: String,
    /// Bytes, when the runtime has it.
    size: Option<u64>,
    /// The latest progress line while it is being pulled.
    pulling: Option<String>,
}

/// Every configured image and whether the runtime has it, for Settings.
#[tauri::command]
pub async fn images(ctx: tauri::State<'_, crate::Ctx>) -> Res<Vec<Image>> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (_, rt) = runtime(&cfg).ok_or_else(|| t!("convert.no_runtime"))?;
        let pulling = PULLING.lock().unwrap().clone();
        Ok(cfg
            .images
            .iter()
            .filter(|(_, image)| !image.is_empty())
            .map(|(tool, image)| Image { tool: tool.clone(), image: image.clone(), size: image_size(&rt, image), pulling: pulling.get(image).cloned() })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Pull `image`, or update it to the newest. Poll `pull_progress` meanwhile.
#[tauri::command]
pub async fn pull_image(image: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<()> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || pull(&runtime(&cfg).ok_or_else(|| t!("convert.no_runtime"))?.1, &image)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_image(image: String, ctx: tauri::State<'_, crate::Ctx>) -> Res<()> {
    let cfg = ctx.cfg().preview.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (_, rt) = runtime(&cfg).ok_or_else(|| t!("convert.no_runtime"))?;
        let out = coxswain_core::tools::command(rt).args(["image", "rm", &image]).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
        if out.status.success() { Ok(()) } else { Err(last_lines(&String::from_utf8_lossy(&out.stderr), 3).unwrap_or_default()) }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The latest progress line of every pull under way, by image.
#[tauri::command]
pub fn pull_progress() -> BTreeMap<String, String> {
    PULLING.lock().unwrap().clone()
}

/// Run to completion within `timeout`; on timeout kill it (and its container).
fn wait(mut c: Command, timeout: Duration, container: Option<(&Path, &str)>) -> Res<()> {
    let mut child = c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let mut err = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err.as_mut() {
            let _ = e.take(64 * 1024).read_to_string(&mut s);
        }
        s
    });
    let status = wait_child(&mut child, timeout, container)?;
    let stderr = reader.join().unwrap_or_default();
    if status.success() { Ok(()) } else { Err(last_lines(&stderr, 12).unwrap_or_else(|| t!("convert.failed", "status" => status))) }
}

/// Run with `input` on stdin and its stdout saved as `file`.
fn capture(mut c: Command, input: Option<&Path>, timeout: Duration, file: &Path, container: Option<(&Path, &str)>) -> Res<()> {
    let mut child = c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    if let (Some(p), Some(mut stdin)) = (input, child.stdin.take()) {
        let data = std::fs::read(p).map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }
    let (mut so, mut se) = (child.stdout.take(), child.stderr.take());
    let out_reader = std::thread::spawn(move || {
        let mut v = vec![];
        if let Some(o) = so.as_mut() {
            let _ = o.take(50 << 20).read_to_end(&mut v);
        }
        v
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = se.as_mut() {
            let _ = e.take(64 * 1024).read_to_string(&mut s);
        }
        s
    });
    let status = wait_child(&mut child, timeout, container)?;
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    if !status.success() || stdout.is_empty() {
        return Err(last_lines(&stderr, 12).unwrap_or_else(|| t!("convert.failed", "status" => status)));
    }
    std::fs::write(file, stdout).map_err(|e| e.to_string())
}

fn wait_child(child: &mut std::process::Child, timeout: Duration, container: Option<(&Path, &str)>) -> Res<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(s);
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            if let Some((rt, name)) = container {
                let _ = coxswain_core::tools::command(rt).args(["kill", name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            }
            let _ = child.wait();
            return Err(t!("convert.timed_out", "seconds" => timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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
        cfg.prefer_tool.insert("latex".into(), "container".into());
        assert!(engines(&cfg, "latex")[0].id.starts_with("container:"), "per-tool preference wins");
        assert!(engines(&cfg, "nonsense").iter().all(|e| !e.available));
    }

    #[test]
    fn outputs_and_errors() {
        let out = Path::new("/tmp/x");
        assert_eq!(output("latex", Path::new("/a/paper.tex"), out), ("pdf", out.join("paper.pdf")));
        assert_eq!(output("plantuml", Path::new("/a/seq.puml"), out).0, "svg");
        assert_eq!(last_lines("a\n\nb\nc\n", 2).unwrap(), "b\nc");
        assert!(last_lines("\n \n", 3).is_none());
    }

    #[test]
    fn follow_keeps_last_complete_line() {
        assert_eq!(follow("one\ntwo\rthree".as_bytes(), "t"), "two");
        assert_eq!(PULLING.lock().unwrap().get("t").map(String::as_str), Some("two"));
    }

    /// Needs podman and the network: `cargo test -- --ignored real_pull`.
    #[test]
    #[ignore]
    fn real_pull_streams_progress() {
        let rt = which("podman").unwrap();
        let image = "docker.io/library/alpine:latest";
        pull(&rt, image).unwrap();
        assert!(image_size(&rt, image).is_some());
        assert!(PULLING.lock().unwrap().is_empty());
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

    /// Fills the preview cache for screenshots, as a click on Build would:
    /// `COXSWAIN_WARM="path|tool|engine" test-binary warm_cache --ignored`.
    #[test]
    #[ignore]
    fn warm_cache() {
        let spec = std::env::var("COXSWAIN_WARM").expect("COXSWAIN_WARM=path|tool|engine");
        let [path, tool, engine]: [&str; 3] = spec.split('|').collect::<Vec<_>>().try_into().unwrap();
        let path = PathBuf::from(path);
        let out = cache_dir(&path, tool, engine).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let (_, file) = output(tool, &path, &out);
        // COXSWAIN_WARM_COPY: a result made outside (where the container runtime is), copied in.
        match std::env::var("COXSWAIN_WARM_COPY") {
            Ok(src) => drop(std::fs::copy(src, &file).unwrap()),
            Err(_) => run(&PreviewConfig::default(), tool, engine, &path, &out, &file).unwrap(),
        }
        assert!(file.is_file());
    }
}

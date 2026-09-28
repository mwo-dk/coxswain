//! Previews made by external tools: LaTeX, LibreOffice, PlantUML, pandoc, draw.io and DuckDB.
//!
//! Each tool runs either from a local install or from a container image (podman or docker),
//! chosen per `[preview]` in the config and by the buttons in the preview pane. Results go to
//! the cache folder, keyed by the file's path, size, modification time and the engine, so a
//! file renders once and shows immediately afterwards.
//!
//! Containers get no network, the file's folder read-only at /src and an empty /out.

use bosum_core::config::PreviewConfig;
use bosum_core::t;
use serde::Serialize;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

type Res<T> = Result<T, String>;

/// The tools, with the local programs that can do the job, best first.
const TOOLS: &[(&str, &[&str])] = &[
    ("latex", &["latexmk", "tectonic", "pdflatex"]),
    ("libreoffice", &["soffice", "libreoffice"]),
    ("plantuml", &["plantuml"]),
    ("pandoc", &["pandoc"]),
    ("drawio", &["drawio", "draw.io"]),
    ("duckdb", &["duckdb"]),
];

/// Where installers put these outside PATH.
fn extra_paths(program: &str) -> Vec<PathBuf> {
    let v: &[&str] = match program {
        "soffice" if cfg!(target_os = "macos") => &["/Applications/LibreOffice.app/Contents/MacOS/soffice"],
        "soffice" if cfg!(windows) => &[r"C:\Program Files\LibreOffice\program\soffice.exe"],
        "drawio" if cfg!(target_os = "macos") => &["/Applications/draw.io.app/Contents/MacOS/draw.io"],
        "drawio" if cfg!(windows) => &[r"C:\Program Files\draw.io\draw.io.exe"],
        _ => &[],
    };
    v.iter().map(PathBuf::from).collect()
}

/// A program on PATH (or in its usual install folder), like `which`.
fn which(program: &str) -> Option<PathBuf> {
    let names = if cfg!(windows) { vec![format!("{program}.exe"), format!("{program}.cmd"), format!("{program}.bat")] } else { vec![program.to_string()] };
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .chain(extra_paths(program))
        .find(|p| p.is_file())
}

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
            let pulled = Command::new(&path).args(if rt == "podman" { vec!["image", "exists", &image] } else { vec!["image", "inspect", &image] }).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
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
}

fn cache_dir(path: &Path, tool: &str, engine: &str) -> Res<PathBuf> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, meta.len(), meta.modified().ok(), tool, engine).hash(&mut h);
    let dir = dirs::cache_dir().ok_or_else(|| t!("err.no_cache_folder"))?.join("bosum").join("previews").join(format!("{:016x}", h.finish()));
    Ok(dir)
}

/// What each tool produces, and where.
fn output(tool: &str, path: &Path, out: &Path) -> (&'static str, PathBuf) {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    match tool {
        "latex" | "libreoffice" => ("pdf", out.join(format!("{stem}.pdf"))),
        "plantuml" | "drawio" => ("svg", out.join("diagram.svg")),
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
        let done = |file: PathBuf| -> Res<Option<Converted>> {
            Ok(Some(match kind {
                "pdf" | "svg" => Converted { kind, file: Some(file), text: None },
                _ => Converted { kind, file: None, text: Some(std::fs::read_to_string(&file).map_err(|e| e.to_string())?) },
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
            Ok(()) => Err(failure(&tool, &path, &out, &t!("convert.no_result"))),
            Err(e) => Err(failure(&tool, &path, &out, &e)),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The error, plus the telling part of a LaTeX log.
fn failure(tool: &str, path: &Path, out: &Path, err: &str) -> String {
    if tool != "latex" {
        return err.to_string();
    }
    let log = out.join(format!("{}.log", path.file_stem().unwrap_or_default().to_string_lossy()));
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    match lines.iter().position(|l| l.starts_with('!')) {
        Some(i) => lines[i..(i + 6).min(lines.len())].join("\n"),
        None => err.to_string(),
    }
}

/// LibreOffice allows one conversion per profile at a time.
static LIBREOFFICE: Mutex<()> = Mutex::new(());

fn run(cfg: &PreviewConfig, tool: &str, engine: &str, path: &Path, out: &Path, file: &Path) -> Res<()> {
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
        pull(&rt_path, image)?;
        let container_name = format!("bosum-preview-{}", out.file_name().unwrap_or_default().to_string_lossy());
        let mut c = Command::new(&rt_path);
        c.args(["run", "--rm", "--network=none", "--security-opt", "label=disable", "--name", &container_name]);
        c.arg("-v").arg(format!("{}:/src:ro", dir.display())).arg("-v").arg(format!("{}:/out", out.display()));
        #[cfg(unix)]
        if rt == "docker" {
            // Rootful docker would leave root-owned files in the cache.
            use std::os::unix::fs::MetadataExt;
            let m = std::fs::metadata(out).map_err(|e| e.to_string())?;
            c.arg("--user").arg(format!("{}:{}", m.uid(), m.gid()));
        }
        c.args(["-w", "/src"]);
        let stdin = match tool {
            "latex" => {
                c.args([image.as_str(), "latexmk", "-pdf", "-interaction=nonstopmode", "-halt-on-error", "-outdir=/out", &name]);
                None
            }
            "libreoffice" => {
                c.args([image.as_str(), "soffice", "--headless", "--convert-to", "pdf", "--outdir", "/out", &format!("/src/{name}")]);
                None
            }
            // The official images' entry points are the tools themselves.
            "plantuml" => {
                c.args(["-i", image, "-tsvg", "-pipe"]);
                Some(path)
            }
            "pandoc" => {
                c.args(["-i", image, "-f", "rst", "-t", "html5"]);
                Some(path)
            }
            "drawio" => {
                c.args([image.as_str(), "-x", "-f", "svg", "-o", "/out/diagram.svg", &format!("/src/{name}")]);
                None
            }
            _ => {
                c.args([image.as_str(), "-readonly", "-json", &format!("/src/{name}"), "-c", DUCKDB_SQL]);
                return capture(c, None, timeout, file, Some((&rt_path, &container_name)));
            }
        };
        return if stdin.is_some() { capture(c, stdin, timeout, file, Some((&rt_path, &container_name))) } else { wait(c, timeout, Some((&rt_path, &container_name))) };
    }

    let program = which(which_one).ok_or_else(|| t!("convert.not_installed", "program" => which_one))?;
    let mut c = Command::new(&program);
    c.current_dir(dir);
    match (tool, which_one) {
        ("latex", "latexmk") => {
            c.args(["-pdf", "-interaction=nonstopmode", "-halt-on-error"]).arg(format!("-outdir={}", out.display())).arg(&name);
        }
        ("latex", "tectonic") => {
            c.arg("--outdir").arg(out).arg(&name);
        }
        ("latex", _) => {
            c.args(["-interaction=nonstopmode", "-halt-on-error"]).arg(format!("-output-directory={}", out.display())).arg(&name);
        }
        ("libreoffice", _) => {
            // Its own profile, so a running LibreOffice does not swallow the conversion.
            let profile = dirs::cache_dir().ok_or_else(|| t!("err.no_cache_folder"))?.join("bosum").join("libreoffice-profile");
            let url = format!("file:///{}", profile.to_string_lossy().trim_start_matches('/').replace('\\', "/"));
            c.arg(format!("-env:UserInstallation={url}")).args(["--headless", "--convert-to", "pdf", "--outdir"]).arg(out).arg(path);
            let _one = LIBREOFFICE.lock().map_err(|e| e.to_string())?;
            return wait(c, timeout, None);
        }
        ("plantuml", _) => {
            c.args(["-tsvg", "-pipe"]);
            return capture(c, Some(path), timeout, file, None);
        }
        ("pandoc", _) => {
            c.args(["-f", "rst", "-t", "html5"]);
            return capture(c, Some(path), timeout, file, None);
        }
        ("drawio", _) => {
            c.args(["-x", "-f", "svg", "-o"]).arg(file).arg(path);
        }
        _ => {
            c.args(["-readonly", "-json"]).arg(path).args(["-c", DUCKDB_SQL]);
            return capture(c, None, timeout, file, None);
        }
    }
    wait(c, timeout, None)
}

const DUCKDB_SQL: &str = "SELECT schema_name, table_name, estimated_size AS rows, column_count AS columns FROM duckdb_tables() ORDER BY 1, 2";

/// Pull `image` if it is not there yet. Not covered by the timeout: images can be gigabytes.
fn pull(rt: &Path, image: &str) -> Res<()> {
    let have = Command::new(rt).args(["image", "inspect", image]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if have {
        return Ok(());
    }
    let out = Command::new(rt).args(["pull", "-q", image]).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(t!("convert.pull_failed", "image" => image, "error" => String::from_utf8_lossy(&out.stderr).trim())) }
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
                let _ = Command::new(rt).args(["kill", name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
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

    /// Needs podman with the texlive image and LibreOffice: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_latex_container_and_libreoffice() {
        let d = std::env::temp_dir().join(format!("bosum-test-convert-{}", std::process::id()));
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
    /// `BOSUM_WARM="path|tool|engine" test-binary warm_cache --ignored`.
    #[test]
    #[ignore]
    fn warm_cache() {
        let spec = std::env::var("BOSUM_WARM").expect("BOSUM_WARM=path|tool|engine");
        let [path, tool, engine]: [&str; 3] = spec.split('|').collect::<Vec<_>>().try_into().unwrap();
        let path = PathBuf::from(path);
        let out = cache_dir(&path, tool, engine).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let (_, file) = output(tool, &path, &out);
        // BOSUM_WARM_COPY: a result made outside (where the container runtime is), copied in.
        match std::env::var("BOSUM_WARM_COPY") {
            Ok(src) => drop(std::fs::copy(src, &file).unwrap()),
            Err(_) => run(&PreviewConfig::default(), tool, engine, &path, &out, &file).unwrap(),
        }
        assert!(file.is_file());
    }
}

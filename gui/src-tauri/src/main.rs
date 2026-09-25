//! Coxswain GUI: Tauri commands over coxswain-core. The Svelte side owns all UI state.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use coxswain_core::config::{color_to_rgb, Action, Config};
use coxswain_core::fs::{self as bfs, Entry, SortKey};
use coxswain_core::git;
use coxswain_core::index::{Results, Service, State};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct AppState {
    cfg: Config,
    index: Arc<Service>,
    start: [PathBuf; 2],
}

#[derive(Serialize)]
struct UiStyle {
    fg: Option<String>,
    bg: Option<String>,
    bold: bool,
}

#[derive(Serialize)]
struct UiConfig {
    /// Canonical key string (as `Key` displays it) -> action name.
    keymap: BTreeMap<String, &'static str>,
    /// Action name -> (label, first key).
    actions: BTreeMap<&'static str, (&'static str, String)>,
    theme: BTreeMap<&'static str, UiStyle>,
    glyphs: coxswain_core::config::Glyphs,
    show_hidden: bool,
    confirm_delete: bool,
    user_menu: Vec<coxswain_core::config::UserCommand>,
    start: [PathBuf; 2],
    config_path: Option<PathBuf>,
    gui: coxswain_core::config::GuiConfig,
}

type Res<T> = Result<T, String>;

fn css(c: &str) -> Option<String> {
    color_to_rgb(c).map(|(r, g, b)| format!("#{r:02x}{g:02x}{b:02x}"))
}

#[tauri::command]
fn get_config(st: tauri::State<AppState>) -> Res<UiConfig> {
    let cfg = &st.cfg;
    let keymap = cfg.keymap()?.into_iter().map(|(k, a)| (k.to_string(), a.name())).collect();
    let actions = Action::ALL.iter().map(|&a| (a.name(), (a.label(), cfg.key_for(a).unwrap_or("").to_string()))).collect();
    let theme = cfg
        .theme()
        .slots()
        .into_iter()
        .map(|(n, s)| (n, UiStyle { fg: css(&s.fg), bg: css(&s.bg), bold: s.bold }))
        .collect();
    Ok(UiConfig {
        keymap,
        actions,
        theme,
        glyphs: cfg.glyphs(),
        show_hidden: cfg.show_hidden,
        confirm_delete: cfg.confirm_delete,
        user_menu: cfg.user_menu.clone(),
        start: st.start.clone(),
        config_path: Config::path(),
        gui: cfg.gui.clone(),
    })
}

#[derive(Serialize)]
struct Listing {
    dir: PathBuf,
    entries: Vec<Entry>,
}

#[tauri::command]
fn list_dir(dir: PathBuf, show_hidden: bool, sort: SortKey, reverse: bool) -> Res<Listing> {
    let mut entries = bfs::list(&dir, show_hidden).map_err(|e| format!("{}: {e}", dir.display()))?;
    bfs::sort(&mut entries, sort, reverse);
    Ok(Listing { dir, entries })
}

#[derive(Serialize)]
struct GitInfo {
    prompt: String,
    /// File name (direct children of the directory) -> status.
    files: BTreeMap<String, git::FileStatus>,
}

#[tauri::command]
async fn git_status(dir: PathBuf, st: tauri::State<'_, AppState>) -> Res<Option<GitInfo>> {
    let glyphs = st.cfg.glyphs();
    Ok(git::Status::read(&dir).map(|s| {
        let files = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|de| Some((de.file_name().to_string_lossy().into_owned(), s.get(&de.path())?)))
            .collect();
        GitInfo { prompt: s.prompt(&glyphs), files }
    }))
}

#[derive(Serialize)]
struct SearchOut {
    #[serde(flatten)]
    results: Results,
    state: State,
    indexed: usize,
}

#[tauri::command]
async fn search(query: String, scope: Option<PathBuf>, st: tauri::State<'_, AppState>) -> Res<SearchOut> {
    let results = st.index.search(&query, scope.as_deref(), st.cfg.search.max_results);
    Ok(SearchOut { results, state: st.index.state(), indexed: st.index.len() })
}

fn resolve(base: &Path, s: &str) -> PathBuf {
    let s = s.trim();
    let p = match s.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            std::env::home_dir().unwrap_or_default().join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(s),
    };
    let p = if p.is_absolute() { p } else { base.join(p) };
    std::fs::canonicalize(&p).unwrap_or(p)
}

#[tauri::command]
fn resolve_path(base: PathBuf, input: String) -> PathBuf {
    resolve(&base, &input)
}

/// Run `op` on every source; collect failures into one message.
fn each(paths: &[PathBuf], op: impl Fn(&Path) -> std::io::Result<()>) -> Res<()> {
    let errors: Vec<String> = paths.iter().filter_map(|p| op(p).err().map(|e| format!("{}: {e}", p.display()))).collect();
    if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
}

#[tauri::command]
async fn copy(paths: Vec<PathBuf>, base: PathBuf, dest: String) -> Res<()> {
    let dst = resolve(&base, &dest);
    each(&paths, |p| bfs::copy(p, &dst).map(drop))
}

#[tauri::command]
async fn rename(paths: Vec<PathBuf>, base: PathBuf, dest: String) -> Res<()> {
    let dst = resolve(&base, &dest);
    each(&paths, |p| bfs::rename(p, &dst).map(drop))
}

#[tauri::command]
async fn delete(paths: Vec<PathBuf>) -> Res<()> {
    each(&paths, bfs::delete)
}

#[tauri::command]
fn mkdir(base: PathBuf, name: String) -> Res<PathBuf> {
    let d = resolve(&base, &name);
    bfs::mkdir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    Ok(d)
}

#[tauri::command]
fn open_path(path: PathBuf) -> Res<()> {
    bfs::open_default(&path).map_err(|e| e.to_string())
}

/// `editor` from the config, else the desktop default.
#[tauri::command]
fn edit_path(path: PathBuf, st: tauri::State<AppState>) -> Res<()> {
    match &st.cfg.editor {
        Some(ed) => {
            let (sh, flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
            let cmd = format!("{ed} {}", coxswain_core::config::quote(&path.to_string_lossy()));
            std::process::Command::new(sh).arg(flag).arg(cmd).spawn().map(drop).map_err(|e| e.to_string())
        }
        None => open_path(path),
    }
}

/// Up to `max` bytes as text for the F3 viewer; binary files come back hex-dumped.
#[tauri::command]
async fn read_text(path: PathBuf, max: usize) -> Res<(String, bool)> {
    let mut buf = vec![];
    let f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    f.take(max as u64).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let truncated = len > buf.len() as u64;
    if buf.iter().take(8192).any(|&b| b == 0) {
        let hex = buf
            .chunks(16)
            .enumerate()
            .map(|(i, c)| {
                let h: String = c.iter().map(|b| format!("{b:02x} ")).collect();
                let a: String = c.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }).collect();
                format!("{:08x}  {h:<48} {a}", i * 16)
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Ok((hex, truncated));
    }
    Ok((String::from_utf8_lossy(&buf).into_owned(), truncated))
}

/// Run a shell command in `dir` and return its output (stdout and stderr, interleaved
/// per stream). `cd` is handled by the UI.
#[tauri::command]
async fn run_command(cmd: String, dir: PathBuf) -> Res<String> {
    let (sh, flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
    let out = std::process::Command::new(sh)
        .arg(flag)
        .arg(&cmd)
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text += &String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        text += &format!("\n[{}]", out.status);
    }
    Ok(text)
}

#[tauri::command]
async fn run_user(index: usize, dir: PathBuf, file: Option<PathBuf>, selected: Vec<PathBuf>, st: tauri::State<'_, AppState>) -> Res<String> {
    let u = st.cfg.user_menu.get(index).ok_or("no such command")?;
    run_command(u.expand(&dir, file.as_deref(), &selected), dir).await
}

fn main() {
    let cfg = Config::load().unwrap_or_else(|e| {
        eprintln!("coxswain: {e}; using defaults");
        Config::default()
    });
    // Validate early: a bad key in the config should not surface as a blank window.
    if let Err(e) = cfg.keymap() {
        eprintln!("coxswain: {e}");
    }
    // Launched from a desktop menu the cwd is usually `/`; home is a better start.
    let home = std::env::home_dir().unwrap_or_default();
    let cwd = std::env::current_dir().ok().filter(|d| d.parent().is_some()).unwrap_or(home);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = |i: usize| args.get(i).map(|a| resolve(&cwd, a)).unwrap_or_else(|| cwd.clone());
    let state = AppState { index: Service::start(&cfg.search), start: [dir(0), dir(1)], cfg };
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_config, list_dir, git_status, search, resolve_path, copy, rename, delete, mkdir, open_path, edit_path, read_text, run_command, run_user
        ])
        .run(tauri::generate_context!())
        .expect("error while running Coxswain");
}

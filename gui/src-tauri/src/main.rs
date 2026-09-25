//! Coxswain GUI: Tauri commands over coxswain-core. The Svelte side owns all UI state except what
//! persists (session, favorites, tags, notes), which lives in `coxswain_core::state`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use coxswain_core::config::{color_to_rgb, Action, Config, Glyphs, GuiConfig, UserCommand};
use coxswain_core::fs::{self as bfs, Entry, SortKey};
use coxswain_core::icons::{icon, Icon};
use coxswain_core::index::{Results, Service, State};
use coxswain_core::rename::{self, Flags, Planned};
use coxswain_core::state::{AppState, FavoriteGroup};
use coxswain_core::git;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

struct Ctx {
    cfg: Config,
    index: Arc<Service>,
    start: [PathBuf; 2],
    state: Mutex<AppState>,
}

impl Ctx {
    /// Change the persisted state and save it right away (the file is small).
    fn edit<T>(&self, f: impl FnOnce(&mut AppState) -> T) -> Res<T> {
        let mut st = self.state.lock().map_err(|e| e.to_string())?;
        let out = f(&mut st);
        st.save().map_err(|e| format!("saving state: {e}"))?;
        Ok(out)
    }
}

type Res<T> = Result<T, String>;

#[derive(Serialize)]
struct UiStyle {
    fg: Option<String>,
    bg: Option<String>,
    bold: bool,
}

type UiTheme = BTreeMap<&'static str, UiStyle>;

#[derive(Serialize)]
struct UiConfig {
    /// Canonical key string (as `Key` displays it) -> action name.
    keymap: BTreeMap<String, &'static str>,
    /// Action name -> (label, first key).
    actions: BTreeMap<&'static str, (&'static str, String)>,
    /// Every theme, by name, so the GUI can switch live.
    themes: BTreeMap<String, UiTheme>,
    glyphs: Glyphs,
    show_hidden: bool,
    confirm_delete: bool,
    user_menu: Vec<UserCommand>,
    start: [PathBuf; 2],
    config_path: Option<PathBuf>,
    gui: GuiConfig,
    home: PathBuf,
    sep: char,
}

fn css(c: &str) -> Option<String> {
    color_to_rgb(c).map(|(r, g, b)| format!("#{r:02x}{g:02x}{b:02x}"))
}

#[tauri::command]
fn get_config(ctx: tauri::State<Ctx>) -> Res<UiConfig> {
    let cfg = &ctx.cfg;
    let themes = cfg
        .themes
        .iter()
        .map(|(name, t)| {
            let slots = t.slots().into_iter().map(|(n, s)| (n, UiStyle { fg: css(&s.fg), bg: css(&s.bg), bold: s.bold })).collect();
            (name.clone(), slots)
        })
        .collect();
    Ok(UiConfig {
        keymap: cfg.keymap()?.into_iter().map(|(k, a)| (k.to_string(), a.name())).collect(),
        actions: Action::ALL.iter().map(|&a| (a.name(), (a.label(), cfg.key_for(a).unwrap_or("").to_string()))).collect(),
        themes,
        glyphs: cfg.glyphs(),
        show_hidden: cfg.show_hidden,
        confirm_delete: cfg.confirm_delete,
        user_menu: cfg.user_menu.clone(),
        start: ctx.start.clone(),
        config_path: Config::path(),
        gui: cfg.gui.clone(),
        home: std::env::home_dir().unwrap_or_default(),
        sep: std::path::MAIN_SEPARATOR,
    })
}

// ---------------------------------------------------------------- listing

#[derive(Serialize)]
struct Item {
    #[serde(flatten)]
    entry: Entry,
    icon: Icon,
    tag: Option<String>,
}

#[derive(Serialize)]
struct Listing {
    dir: PathBuf,
    items: Vec<Item>,
    has_notes: bool,
}

#[tauri::command]
fn list_dir(dir: PathBuf, show_hidden: bool, sort: SortKey, reverse: bool, ctx: tauri::State<Ctx>) -> Res<Listing> {
    let mut entries = bfs::list(&dir, show_hidden).map_err(|e| format!("{}: {e}", dir.display()))?;
    bfs::sort(&mut entries, sort, reverse);
    let st = ctx.state.lock().map_err(|e| e.to_string())?;
    let items = entries
        .into_iter()
        .map(|e| Item { icon: icon(&e.name, e.is_dir), tag: st.tags.get(&e.path).cloned(), entry: e })
        .collect();
    Ok(Listing { has_notes: st.notes.contains_key(&dir), dir, items })
}

#[derive(Serialize)]
struct GitInfo {
    root: PathBuf,
    prompt: String,
    branch: String,
    /// File name (direct children of the directory) -> status.
    files: BTreeMap<String, git::FileStatus>,
}

#[tauri::command]
async fn git_status(dir: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<Option<GitInfo>> {
    let Some(s) = git::Status::read(&dir) else { return Ok(None) };
    let files = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|de| Some((de.file_name().to_string_lossy().into_owned(), s.get(&de.path())?)))
        .collect();
    ctx.edit(|st| st.touch_repo(&s.root))?;
    Ok(Some(GitInfo { prompt: s.prompt(&ctx.cfg.glyphs()), branch: s.summary.branch.clone(), root: s.root, files }))
}

// ---------------------------------------------------------------- sidebar

#[derive(Serialize)]
struct Place {
    name: String,
    path: PathBuf,
    icon: &'static str,
}

#[tauri::command]
fn places() -> Vec<Place> {
    let p = |name: &str, dir: Option<PathBuf>, icon: &'static str| dir.filter(|d| d.is_dir()).map(|path| Place { name: name.into(), path, icon });
    [
        p("Home", dirs::home_dir(), "\u{f015}"),
        p("Desktop", dirs::desktop_dir(), "\u{f108}"),
        p("Documents", dirs::document_dir(), "\u{f0219}"),
        p("Downloads", dirs::download_dir(), "\u{f019}"),
        p("Pictures", dirs::picture_dir(), "\u{f03e}"),
        p("Music", dirs::audio_dir(), "\u{f001}"),
        p("Videos", dirs::video_dir(), "\u{f03d}"),
    ]
    .into_iter()
    .flatten()
    // Some desktops point Desktop etc. at $HOME; list each folder once.
    .fold(Vec::<Place>::new(), |mut v, x| {
        if !v.iter().any(|y| y.path == x.path) {
            v.push(x);
        }
        v
    })
}

#[derive(Serialize)]
struct Disk {
    label: String,
    device: String,
    mount: PathBuf,
    total: u64,
    free: u64,
    removable: bool,
}

#[tauri::command]
async fn disks() -> Vec<Disk> {
    let list = sysinfo::Disks::new_with_refreshed_list();
    let mut out: Vec<Disk> = vec![];
    // Subvolumes and bind mounts repeat the same device; keep its shortest mount point.
    let mut sorted: Vec<_> = list.list().iter().filter(|d| d.total_space() > 0).collect();
    sorted.sort_by_key(|d| d.mount_point().as_os_str().len());
    for d in sorted {
        let name = d.name().to_string_lossy().into_owned();
        let mount = d.mount_point().to_path_buf();
        let skip = ["/boot", "/efi", "/snap", "/var/lib", "/run", "/proc", "/sys"].iter().any(|p| mount.starts_with(p));
        if skip || out.iter().any(|o| o.device == name) {
            continue;
        }
        let label = if mount.parent().is_none() { "System".into() } else { mount.file_name().map_or(name.clone(), |n| n.to_string_lossy().into_owned()) };
        out.push(Disk { label, device: name, mount, total: d.total_space(), free: d.available_space(), removable: d.is_removable() });
    }
    out
}

#[tauri::command]
fn get_state(ctx: tauri::State<Ctx>) -> Res<AppState> {
    ctx.state.lock().map(|s| s.clone()).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_session(session: serde_json::Value, ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| st.session = session)
}

#[tauri::command]
fn save_favorites(favorites: Vec<FavoriteGroup>, ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| st.favorites = favorites)
}

#[tauri::command]
fn set_tags(paths: Vec<PathBuf>, color: String, ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| paths.iter().for_each(|p| st.set_tag(p, &color)))
}

#[tauri::command]
fn set_note(dir: PathBuf, text: String, ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| st.set_note(&dir, &text))
}

#[tauri::command]
fn get_note(dir: PathBuf, ctx: tauri::State<Ctx>) -> Res<String> {
    Ok(ctx.state.lock().map_err(|e| e.to_string())?.notes.get(&dir).cloned().unwrap_or_default())
}

// ---------------------------------------------------------------- search & ops

#[derive(Serialize)]
struct SearchOut {
    #[serde(flatten)]
    results: Results,
    state: State,
    indexed: usize,
}

#[tauri::command]
async fn search(query: String, scope: Option<PathBuf>, ctx: tauri::State<'_, Ctx>) -> Res<SearchOut> {
    let results = ctx.index.search(&query, scope.as_deref(), ctx.cfg.search.max_results);
    Ok(SearchOut { results, state: ctx.index.state(), indexed: ctx.index.len() })
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
async fn dir_sizes(paths: Vec<PathBuf>) -> BTreeMap<PathBuf, (u64, u64)> {
    paths.into_iter().map(|p| (bfs::dir_size(&p), p)).map(|(s, p)| (p, s)).collect()
}

#[tauri::command]
fn rename_plan(dir: PathBuf, selected: Vec<String>, pattern: String, replacement: String, flags: Flags) -> Res<Vec<Planned>> {
    let existing: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .collect();
    rename::plan(&selected, &existing, &pattern, &replacement, flags)
}

#[tauri::command]
fn rename_apply(dir: PathBuf, plan: Vec<Planned>) -> Res<()> {
    rename::apply(&dir, &plan)
}

#[tauri::command]
fn open_path(path: PathBuf) -> Res<()> {
    bfs::open_default(&path).map_err(|e| e.to_string())
}

/// `editor` from the config, else the desktop default.
#[tauri::command]
fn edit_path(path: PathBuf, ctx: tauri::State<Ctx>) -> Res<()> {
    match &ctx.cfg.editor {
        Some(ed) => {
            let cmd = format!("{ed} {}", coxswain_core::config::quote(&path.to_string_lossy()));
            shell(&cmd).spawn().map(drop).map_err(|e| e.to_string())
        }
        None => open_path(path),
    }
}

/// Up to `max` bytes as text for the preview; binary files come back hex-dumped.
#[tauri::command]
async fn read_text(path: PathBuf, max: usize) -> Res<(String, bool, bool)> {
    let mut buf = vec![];
    let f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    f.take(max as u64).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let truncated = len > buf.len() as u64;
    if buf.iter().take(8192).any(|&b| b == 0) {
        let hex = buf
            .chunks(16)
            .take(4096)
            .enumerate()
            .map(|(i, c)| {
                let h: String = c.iter().map(|b| format!("{b:02x} ")).collect();
                let a: String = c.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }).collect();
                format!("{:08x}  {h:<48} {a}", i * 16)
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Ok((hex, truncated, true));
    }
    Ok((String::from_utf8_lossy(&buf).into_owned(), truncated, false))
}

// ---------------------------------------------------------------- commands & scripts

fn shell(cmd: &str) -> std::process::Command {
    let (sh, flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
    let mut c = std::process::Command::new(sh);
    c.arg(flag).arg(cmd);
    c
}

fn output(mut c: std::process::Command, dir: &Path) -> Res<String> {
    let out = c.current_dir(dir).stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text += &String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        text += &format!("\n[{}]", out.status);
    }
    Ok(text)
}

/// Run a shell command in `dir` and return what it printed. `cd` is handled by the UI.
#[tauri::command]
async fn run_command(cmd: String, dir: PathBuf) -> Res<String> {
    output(shell(&cmd), &dir)
}

#[derive(Serialize)]
struct Script {
    key: String,
    label: String,
    /// Index into `user_menu`, or a script file.
    user: Option<usize>,
    path: Option<PathBuf>,
}

fn scripts_dir() -> Option<PathBuf> {
    Config::path().and_then(|p| Some(p.parent()?.join("scripts")))
}

/// F2: `[[user_menu]]` entries, then executables in `<config>/coxswain/scripts/`.
#[tauri::command]
fn scripts(ctx: tauri::State<Ctx>) -> Vec<Script> {
    let mut out: Vec<Script> =
        ctx.cfg.user_menu.iter().enumerate().map(|(i, u)| Script { key: u.key.clone(), label: u.label.clone(), user: Some(i), path: None }).collect();
    if let Some(Ok(rd)) = scripts_dir().map(std::fs::read_dir) {
        let mut files: Vec<PathBuf> = rd.flatten().map(|d| d.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for p in files {
            let label = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            out.push(Script { key: String::new(), label, user: None, path: Some(p) });
        }
    }
    out
}

/// Run a user-menu entry (expanded) or a script file with the selection as arguments.
#[tauri::command]
async fn run_script(user: Option<usize>, path: Option<PathBuf>, dir: PathBuf, file: Option<PathBuf>, selected: Vec<PathBuf>, ctx: tauri::State<'_, Ctx>) -> Res<String> {
    if let Some(i) = user {
        let u = ctx.cfg.user_menu.get(i).ok_or("no such command")?;
        return output(shell(&u.expand(&dir, file.as_deref(), &selected)), &dir);
    }
    let script = path.ok_or("nothing to run")?;
    // Only files from the scripts directory may run this way.
    let allowed = scripts_dir().and_then(|d| std::fs::canonicalize(d).ok());
    let real = std::fs::canonicalize(&script).map_err(|e| e.to_string())?;
    if !allowed.is_some_and(|d| real.starts_with(d)) {
        return Err("not a Coxswain script".into());
    }
    let args = if selected.is_empty() { file.into_iter().collect() } else { selected };
    let mut c = std::process::Command::new(&real);
    c.args(args);
    output(c, &dir)
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
    let ctx = Ctx { index: Service::start(&cfg.search), start: [dir(0), dir(1)], state: Mutex::new(AppState::load()), cfg };
    tauri::Builder::default()
        .manage(ctx)
        .invoke_handler(tauri::generate_handler![
            get_config, list_dir, git_status, places, disks, get_state, save_session, save_favorites, set_tags, set_note, get_note,
            search, resolve_path, copy, rename, delete, mkdir, dir_sizes, rename_plan, rename_apply, open_path, edit_path,
            read_text, run_command, scripts, run_script
        ])
        .run(tauri::generate_context!())
        .expect("error while running Coxswain");
}

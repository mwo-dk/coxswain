//! Coxswain GUI: Tauri commands over coxswain-core. The Svelte side owns all UI state except what
//! persists (session, favorites, tags, notes), which lives in `coxswain_core::state`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use coxswain_core::config::{color_to_rgb, Action, Config, Glyphs, GuiConfig, UserCommand};
use coxswain_core::fs::{self as bfs, Entry, SortKey};
use coxswain_core::icons::{icon, Icon};
use coxswain_core::helper::{self, Client};
use coxswain_core::index::{Results, State};
use coxswain_core::rename::{self, Flags, Planned};
use coxswain_core::state::{AppState, FavoriteGroup};
use coxswain_core::git;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

mod bom;
mod convert;
mod preview;

pub struct Ctx {
    /// Replaced when Settings are saved.
    cfg: std::sync::RwLock<Config>,
    index: Arc<Client>,
    start: [PathBuf; 2],
    /// `--duplicates <folders>`: open the duplicate finder on these folders at start.
    duplicates: Option<Vec<PathBuf>>,
    /// `--settings[=section]`: open the Settings window at start, at that section ("" for the top).
    open_settings: Option<String>,
    state: Mutex<AppState>,
    /// Folders shown in the panes, watched so they reread themselves.
    watched: Mutex<Vec<PathBuf>>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    /// What Coxswain last put on the clipboard, and whether it was a cut.
    clip: Mutex<(Vec<PathBuf>, bool)>,
    /// The running duplicate scan's progress, if any.
    dupes: Mutex<Option<Arc<coxswain_core::dupes::Progress>>>,
    sizer: Arc<coxswain_core::sizes::Sizer>,
    /// The model download for search by meaning, while one runs or has failed.
    meaning: Mutex<Option<(Arc<coxswain_core::meaning::Progress>, Option<String>)>>,
    /// The stop flag of each tab's background measuring.
    measuring: Mutex<std::collections::HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
    /// Which Ask may still write its answer; a new question or Stop moves it on.
    asking: Arc<std::sync::atomic::AtomicU64>,
}

impl Ctx {
    pub fn cfg(&self) -> std::sync::RwLockReadGuard<'_, Config> {
        self.cfg.read().unwrap_or_else(|e| e.into_inner())
    }

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
    actions: BTreeMap<&'static str, (String, String)>,
    /// Every theme, by name, so the GUI can switch live.
    themes: BTreeMap<String, UiTheme>,
    /// Each theme's look (shapes and chrome, see `Theme::look`), by name.
    looks: BTreeMap<String, String>,
    /// The built-in themes' names, in the order Settings shows them.
    builtin_themes: Vec<&'static str>,
    glyphs: Glyphs,
    show_hidden: bool,
    folder_sizes: bool,
    confirm_delete: bool,
    user_menu: Vec<UserCommand>,
    start: [PathBuf; 2],
    duplicates: Option<Vec<PathBuf>>,
    /// Start with Settings open, at this section ("" for the top).
    open_settings: Option<String>,
    /// The language in use (resolved from `language`), its texts, and whether it is written
    /// right to left.
    language: &'static str,
    strings: std::collections::HashMap<String, serde_json::Value>,
    rtl: bool,
    /// Every language Coxswain has: (code, own name, flag).
    languages: &'static [(&'static str, &'static str, &'static str)],
    /// The config file's raw values, for the Settings window.
    settings: Settings,
    config_path: Option<PathBuf>,
    gui: GuiConfig,
    home: PathBuf,
    sep: char,
    version: &'static str,
}

fn css(c: &str) -> Option<String> {
    color_to_rgb(c).map(|(r, g, b)| format!("#{r:02x}{g:02x}{b:02x}"))
}

#[tauri::command]
fn get_config(ctx: tauri::State<Ctx>) -> Res<UiConfig> {
    let cfg = ctx.cfg();
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
        looks: cfg.themes.iter().map(|(name, t)| (name.clone(), t.look.clone())).collect(),
        builtin_themes: coxswain_core::config::Theme::builtin().into_iter().map(|(name, _)| name).collect(),
        glyphs: cfg.glyphs(),
        show_hidden: cfg.show_hidden,
        folder_sizes: cfg.folder_sizes,
        confirm_delete: cfg.confirm_delete,
        user_menu: cfg.user_menu.clone(),
        start: ctx.start.clone(),
        duplicates: ctx.duplicates.clone(),
        open_settings: ctx.open_settings.clone(),
        language: coxswain_core::i18n::language(),
        strings: coxswain_core::i18n::catalogue(coxswain_core::i18n::language()),
        rtl: coxswain_core::i18n::is_rtl(coxswain_core::i18n::language()),
        languages: coxswain_core::i18n::LANGUAGES,
        settings: Settings::from(&*cfg),
        config_path: Config::path(),
        gui: cfg.gui.clone(),
        home: std::env::home_dir().unwrap_or_default(),
        sep: std::path::MAIN_SEPARATOR,
        version: coxswain_core::update::VERSION,
    })
}

// ---------------------------------------------------------------- settings

/// What the Settings window edits, as stored in config.toml.
#[derive(Serialize)]
struct Settings {
    language: String,
    theme: String,
    glyphs: String,
    show_hidden: bool,
    confirm_delete: bool,
    check_updates: bool,
    font: String,
    mono_font: String,
    icon_font: String,
    font_size: u32,
    preview_prefer: String,
    preview_container: String,
    preview_timeout: u64,
    latex_image: String,
    search_text: bool,
    /// The folders whose text is read; the home folder when none are set.
    text_roots: Vec<PathBuf>,
    names_only: Vec<PathBuf>,
    search_meaning: bool,
    latex_auto: bool,
    meaning_engine: String,
    meaning_url: String,
    meaning_model: String,
    meaning_key_env: String,
    ask_model: String,
}

impl From<&Config> for Settings {
    fn from(c: &Config) -> Self {
        Settings {
            language: c.language.clone(),
            theme: c.gui.theme.clone(),
            glyphs: c.glyphs.clone(),
            show_hidden: c.show_hidden,
            confirm_delete: c.confirm_delete,
            check_updates: c.check_updates,
            font: c.gui.font.clone(),
            mono_font: c.gui.mono_font.clone(),
            icon_font: c.gui.icon_font.clone(),
            font_size: c.gui.font_size as u32,
            preview_prefer: c.preview.prefer.clone(),
            preview_container: c.preview.container.clone(),
            preview_timeout: c.preview.timeout,
            latex_image: c.preview.images.get("latex").cloned().unwrap_or_default(),
            search_text: c.search.text,
            text_roots: c.search.text_roots.clone(),
            names_only: c.search.names_only.clone(),
            search_meaning: c.search.meaning,
            latex_auto: c.preview.latex_auto,
            meaning_engine: c.search.meaning_engine.clone(),
            meaning_url: c.search.meaning_url.clone(),
            meaning_model: c.search.meaning_model.clone(),
            meaning_key_env: c.search.meaning_key_env.clone(),
            ask_model: c.search.ask_model.clone(),
        }
    }
}

/// Where each setting lives in config.toml.
const SETTING_PATHS: &[(&str, &[&str])] = &[
    ("language", &["language"]),
    ("theme", &["gui", "theme"]),
    ("glyphs", &["glyphs"]),
    ("show_hidden", &["show_hidden"]),
    ("confirm_delete", &["confirm_delete"]),
    ("check_updates", &["check_updates"]),
    ("font", &["gui", "font"]),
    ("mono_font", &["gui", "mono_font"]),
    ("icon_font", &["gui", "icon_font"]),
    ("font_size", &["gui", "font_size"]),
    ("preview_prefer", &["preview", "prefer"]),
    ("preview_container", &["preview", "container"]),
    ("preview_timeout", &["preview", "timeout"]),
    ("latex_image", &["preview", "images", "latex"]),
    ("search_text", &["search", "text"]),
    ("text_roots", &["search", "text_roots"]),
    ("names_only", &["search", "names_only"]),
    ("search_meaning", &["search", "meaning"]),
    ("latex_auto", &["preview", "latex_auto"]),
    ("meaning_engine", &["search", "meaning_engine"]),
    ("meaning_url", &["search", "meaning_url"]),
    ("meaning_model", &["search", "meaning_model"]),
    ("meaning_key_env", &["search", "meaning_key_env"]),
    ("ask_model", &["search", "ask_model"]),
];

/// `text` (a config.toml) with the settings in `changes` set, comments and layout kept.
fn apply_settings(text: &str, changes: &serde_json::Map<String, serde_json::Value>) -> Res<String> {
    let mut text = text.to_string();
    for (name, v) in changes {
        let keys = SETTING_PATHS.iter().find(|(n, _)| n == name).map(|(_, k)| *k).ok_or_else(|| coxswain_core::t!("err.unknown_setting", "name" => name))?;
        let value: toml_edit::Value = match v {
            serde_json::Value::Bool(b) => (*b).into(),
            serde_json::Value::Number(n) => n.as_i64().ok_or_else(|| coxswain_core::t!("err.not_whole_number"))?.into(),
            serde_json::Value::String(s) => s.as_str().into(),
            serde_json::Value::Array(a) => {
                let texts: Option<Vec<&str>> = a.iter().map(|v| v.as_str()).collect();
                texts.ok_or_else(|| coxswain_core::t!("err.unsupported_value", "name" => name))?.into_iter().collect::<toml_edit::Array>().into()
            }
            _ => return Err(coxswain_core::t!("err.unsupported_value", "name" => name)),
        };
        text = Config::edit(&text, keys, value)?;
    }
    Ok(text)
}

/// Write the changed settings into config.toml, keeping its comments and layout, then use
/// the new config at once. Returns the new UI config (texts in the new language and so on).
#[tauri::command]
fn save_settings(changes: serde_json::Map<String, serde_json::Value>, ctx: tauri::State<Ctx>) -> Res<UiConfig> {
    let path = Config::path().ok_or_else(|| coxswain_core::t!("err.no_config_folder"))?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let new_text = apply_settings(&text, &changes)?;
    // Only write what parses: a broken config must never replace a working one.
    let cfg = Config::parse(&new_text)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, new_text).map_err(|e| format!("{}: {e}", path.display()))?;
    coxswain_core::i18n::set_language(coxswain_core::i18n::resolve(&cfg.language));
    *ctx.cfg.write().map_err(|e| e.to_string())? = cfg;
    get_config(ctx)
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
    /// The archive the folder is inside, and whether something in it is locked.
    archive: Option<PathBuf>,
    locked: bool,
}

/// Async, as a compressed tar is read in full to list a folder in it.
#[tauri::command]
async fn list_dir(dir: PathBuf, show_hidden: bool, sort: SortKey, reverse: bool, ctx: tauri::State<'_, Ctx>) -> Res<Listing> {
    let (mut entries, inside) = bfs::list_with_archive(&dir, show_hidden).map_err(|e| format!("{}: {e}", dir.display()))?;
    bfs::sort(&mut entries, sort, reverse);
    let st = ctx.state.lock().map_err(|e| e.to_string())?;
    let items = entries
        .into_iter()
        .map(|e| Item { icon: icon(&e.name, e.is_dir), tag: st.tags.get(&e.path).cloned(), entry: e })
        .collect();
    let (archive, locked) = inside.map_or((None, false), |(a, locked)| (Some(a), locked));
    Ok(Listing { has_notes: st.notes.contains_key(&dir), dir, items, archive, locked })
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
    Ok(Some(GitInfo { prompt: s.prompt(&ctx.cfg().glyphs()), branch: s.summary.branch.clone(), root: s.root, files }))
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
    let p = |name: &str, dir: Option<PathBuf>, icon: &'static str| dir.filter(|d| d.is_dir()).map(|path| Place { name: coxswain_core::t!(name), path, icon });
    [
        p("place.home", dirs::home_dir(), "\u{f015}"),
        p("place.desktop", dirs::desktop_dir(), "\u{f108}"),
        p("place.documents", dirs::document_dir(), "\u{f0219}"),
        p("place.downloads", dirs::download_dir(), "\u{f019}"),
        p("place.pictures", dirs::picture_dir(), "\u{f03e}"),
        p("place.music", dirs::audio_dir(), "\u{f001}"),
        p("place.videos", dirs::video_dir(), "\u{f03d}"),
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
        let label = if mount.parent().is_none() { coxswain_core::t!("place.system") } else { mount.file_name().map_or(name.clone(), |n| n.to_string_lossy().into_owned()) };
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

/// How the search helper is doing, for Settings.
#[derive(Serialize)]
struct IndexStatus {
    #[serde(flatten)]
    status: coxswain_core::helper::Status,
    /// Where the store is kept.
    path: Option<PathBuf>,
    /// Whether the helper answers: without it there is no text search.
    shared: bool,
    /// Whether the helper starts with the session.
    service: bool,
}

#[tauri::command]
async fn index_status(ctx: tauri::State<'_, Ctx>) -> Res<IndexStatus> {
    let index = ctx.index.clone();
    tauri::async_runtime::spawn_blocking(move || IndexStatus { status: index.status(), path: coxswain_core::store::Store::path(), shared: index.shared(), service: coxswain_core::service::installed() }).await.map_err(|e| e.to_string())
}

/// The notice to show in the status line, if any, and the window's title: the version and
/// the kinds of search on.
#[derive(Serialize)]
struct Notices {
    notice: Option<coxswain_core::notices::Notice>,
    title: String,
}

#[tauri::command]
async fn notices(ctx: tauri::State<'_, Ctx>) -> Res<Notices> {
    let (index, cfg) = (ctx.index.clone(), ctx.cfg().clone());
    let status = tauri::async_runtime::spawn_blocking(move || index.status()).await.map_err(|e| e.to_string())?;
    let st = ctx.state.lock().map_err(|e| e.to_string())?;
    Ok(Notices {
        notice: coxswain_core::notices::next(&cfg, &status, &st, false),
        title: coxswain_core::t!("title.window", "version" => coxswain_core::update::VERSION, "search" => coxswain_core::notices::search_level(&cfg, &status)),
    })
}

/// The window's title. On Linux the title bar that GTK draws keeps the title it was made
/// with, so it is told too.
#[tauri::command]
fn set_title(title: String, window: tauri::WebviewWindow) -> Res<()> {
    window.set_title(&title).map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    {
        let w = window.clone();
        window
            .run_on_main_thread(move || {
                use gtk::prelude::{BinExt, Cast, GtkWindowExt, HeaderBarExt};
                // On Wayland tao puts its header bar inside an event box.
                let Some(top) = w.gtk_window().ok().and_then(|g| g.titlebar()) else { return };
                let bar = match top.clone().downcast::<gtk::EventBox>() {
                    Ok(event_box) => event_box.child().and_then(|c| c.downcast::<gtk::HeaderBar>().ok()),
                    Err(_) => top.downcast::<gtk::HeaderBar>().ok(),
                };
                if let Some(bar) = bar {
                    bar.set_title(Some(&title));
                }
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn dismiss_notice(id: String, ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| coxswain_core::notices::dismiss(st, &id))
}

/// Search by meaning, for Settings: the model there or not, its size, a download under way
/// (bytes done, bytes in all) or the error that stopped it.
#[derive(Serialize)]
struct MeaningStatus {
    installed: bool,
    size: u64,
    /// Where the built-in model is, or would be.
    folder: Option<PathBuf>,
    downloading: Option<(u64, u64)>,
    error: Option<String>,
}

#[tauri::command]
fn meaning_status(ctx: tauri::State<Ctx>) -> Res<MeaningStatus> {
    use std::sync::atomic::Ordering;
    let m = ctx.meaning.lock().map_err(|e| e.to_string())?;
    Ok(MeaningStatus {
        installed: coxswain_core::meaning::installed(),
        size: coxswain_core::meaning::size(),
        folder: coxswain_core::meaning::folder(),
        downloading: m.as_ref().filter(|(_, err)| err.is_none()).map(|(p, _)| (p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed))),
        error: m.as_ref().and_then(|(_, e)| e.clone()),
    })
}

/// The embedding models a server offers, for Settings: Ollama's pulled ones, or an OpenAI
/// server's list. An error when it does not answer.
#[tauri::command]
async fn meaning_models(engine: String, url: String) -> Res<Vec<String>> {
    tauri::async_runtime::spawn_blocking(move || coxswain_core::meaning::server_models(engine == "openai", &url)).await.map_err(|e| e.to_string())?
}

/// Ask Ollama to pull `model`; the progress is the download's, in `meaning_status`.
#[tauri::command]
fn meaning_pull(model: String, url: String, app: tauri::AppHandle, ctx: tauri::State<Ctx>) -> Res<()> {
    let p = Arc::new(coxswain_core::meaning::Progress::default());
    *ctx.meaning.lock().map_err(|e| e.to_string())? = Some((p.clone(), None));
    let index = ctx.index.clone();
    std::thread::spawn(move || {
        let done = coxswain_core::meaning::ollama_pull(&url, &model, &p);
        if let Ok(mut m) = app.state::<Ctx>().meaning.lock() {
            *m = match done {
                Err(e) if !p.cancel.load(std::sync::atomic::Ordering::Relaxed) => Some((p, Some(e.to_string()))),
                _ => None,
            };
        }
        index.restart();
    });
    Ok(())
}

/// "download": fetch the model, then turn search by meaning on; "cancel" the download;
/// "off": turn it off; "remove": turn it off and delete the model. The helper starts again
/// with the new setting.
#[tauri::command]
async fn meaning_action(what: String, app: tauri::AppHandle, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    use std::sync::atomic::Ordering;
    let set = |on: bool| -> Res<()> {
        let cfg = Config::save_value(&["search", "meaning"], on.into())?;
        *app.state::<Ctx>().cfg.write().map_err(|e| e.to_string())? = cfg;
        Ok(())
    };
    match what.as_str() {
        "download" => {
            let p = Arc::new(coxswain_core::meaning::Progress::default());
            *ctx.meaning.lock().map_err(|e| e.to_string())? = Some((p.clone(), None));
            let (app, index) = (app.clone(), ctx.index.clone());
            std::thread::spawn(move || {
                let ctx = app.state::<Ctx>();
                let done = coxswain_core::meaning::download(&p).map_err(|e| e.to_string()).and_then(|()| {
                    let cfg = Config::save_value(&["search", "meaning"], true.into())?;
                    *ctx.cfg.write().map_err(|e| e.to_string())? = cfg;
                    Ok(())
                });
                if let Ok(mut m) = ctx.meaning.lock() {
                    *m = match done {
                        Ok(()) => None,
                        Err(_) if p.cancel.load(Ordering::Relaxed) => None,
                        Err(e) => Some((p, Some(e))),
                    };
                }
                index.restart();
            });
            return Ok(());
        }
        "cancel" => {
            if let Some((p, _)) = ctx.meaning.lock().map_err(|e| e.to_string())?.as_ref() {
                p.cancel.store(true, Ordering::Relaxed);
            }
            return Ok(());
        }
        "off" => set(false)?,
        _ => {
            set(false)?;
            coxswain_core::meaning::remove().map_err(|e| e.to_string())?;
        }
    }
    let index = ctx.index.clone();
    tauri::async_runtime::spawn_blocking(move || index.restart()).await.map_err(|e| e.to_string())
}

/// Start the search helper with the session, or stop doing so; the helper running now makes
/// way for the right one.
#[tauri::command]
async fn index_service(on: bool, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let index = ctx.index.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let done = if on { coxswain_core::tools::this_app().and_then(|exe| coxswain_core::service::install(&exe)) } else { coxswain_core::service::uninstall() };
        index.restart();
        done.map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// "now": read the backlog at full speed; "forget": empty the store; "restart": a helper
/// with the settings as they are now.
#[tauri::command]
async fn index_action(what: String, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let index = ctx.index.clone();
    tauri::async_runtime::spawn_blocking(move || match what.as_str() {
        "now" => index.index_now(),
        "forget" => index.forget(),
        _ => index.restart(),
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct SearchOut {
    #[serde(flatten)]
    results: Results,
    state: State,
    indexed: usize,
    /// Files whose text can be searched, and files still to be read.
    texts: usize,
    pending: usize,
    /// Search by meaning is on: without it, text mode points to where it is turned on.
    meaning: bool,
}

/// The GUI lists every hit it gets as a row, so it takes fewer than the TUI, which only draws
/// what fits on screen. `total` still counts them all.
const GUI_MAX_HITS: usize = 500;

#[tauri::command]
async fn search(query: String, scope: Option<PathBuf>, text: Option<bool>, ctx: tauri::State<'_, Ctx>) -> Res<SearchOut> {
    // A CPU-bound scan (rayon, all cores), so off the async runtime's worker threads.
    let index = ctx.index.clone();
    let max = ctx.cfg().search.max_results.min(GUI_MAX_HITS);
    tauri::async_runtime::spawn_blocking(move || {
        let results = if text == Some(true) { index.search_text(&query, max) } else { index.search(&query, scope.as_deref(), max) };
        let now = index.status();
        SearchOut { results, state: now.state, indexed: now.len, texts: now.texts, pending: now.pending, meaning: now.meaning }
    })
    .await
    .map_err(|e| e.to_string())
}

/// What an Ask sends while it runs: first the numbered sources, then the answer piece by piece.
#[derive(Clone, Serialize)]
#[serde(tag = "k", rename_all = "snake_case")]
enum AskEvent {
    Sources { paths: Vec<PathBuf> },
    Piece { text: String },
}

/// How many passages an answer is made from.
const ASK_PASSAGES: usize = 10;

/// Ask: `question` answered by the user's chat model from the closest passages, with the
/// questions and answers `earlier` in this Find file. Nothing is kept.
#[tauri::command]
async fn ask(question: String, earlier: Vec<(String, String)>, on_event: tauri::ipc::Channel<AskEvent>, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    use std::sync::atomic::Ordering;
    let (index, cfg, asking) = (ctx.index.clone(), ctx.cfg().search.clone(), ctx.asking.clone());
    let me = asking.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        // A follow-up is looked up with the question before it, which it often leans on.
        let lookup = earlier.last().map_or(question.clone(), |(q, _)| format!("{q} {question}"));
        let sources = index.passages(&lookup, ASK_PASSAGES);
        if sources.is_empty() {
            return Err(coxswain_core::t!("search.ask_nothing"));
        }
        let _ = on_event.send(AskEvent::Sources { paths: sources.iter().map(|(p, _)| p.clone()).collect() });
        coxswain_core::meaning::ask(&cfg, &earlier, &question, &sources, |text| asking.load(Ordering::SeqCst) == me && on_event.send(AskEvent::Piece { text: text.to_string() }).is_ok())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stop the answer being written.
#[tauri::command]
fn ask_stop(ctx: tauri::State<Ctx>) {
    ctx.asking.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
async fn copy(paths: Vec<PathBuf>, base: PathBuf, dest: String, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let dst = resolve(&base, &dest);
    ctx.sizer.forget(&dst.join("new"));
    // The password of a locked archive, held for this copy only.
    each(&paths, |p| bfs::copy_locked(p, &dst, password.as_deref()).map(drop))
}

#[tauri::command]
async fn rename(paths: Vec<PathBuf>, base: PathBuf, dest: String, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let dst = resolve(&base, &dest);
    paths.iter().chain([&dst.join("new")]).for_each(|p| ctx.sizer.forget(p));
    each(&paths, |p| bfs::rename_locked(p, &dst, password.as_deref()).map(drop))
}

/// To the trash, or gone for good with `forever`. Inside a locked 7z, `password` opens it.
#[tauri::command]
async fn delete(paths: Vec<PathBuf>, forever: bool, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    paths.iter().for_each(|p| ctx.sizer.forget(p));
    let pw = password.as_deref();
    each(&paths, |p| if forever { bfs::delete_locked(p, pw) } else { bfs::trash_locked(p, pw) })
}

/// Async, as inside an archive the archive is written anew.
#[tauri::command]
async fn mkdir(base: PathBuf, name: String, password: Option<String>) -> Res<PathBuf> {
    let d = resolve(&base, &name);
    bfs::mkdir_locked(&d, password.as_deref()).map_err(|e| format!("{}: {e}", d.display()))?;
    Ok(d)
}

/// Bytes and file count of each folder. With `tab` it is that tab's background measuring: the
/// run before it stops, and a size measured a short while ago is taken as it is. Without, the
/// user asked, and gets a fresh measure. A folder that was stopped is left out.
#[tauri::command]
async fn dir_sizes(paths: Vec<PathBuf>, tab: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<BTreeMap<PathBuf, (u64, u64)>> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (sizer, stop) = (ctx.sizer.clone(), Arc::new(AtomicBool::new(false)));
    match tab {
        Some(tab) => {
            if let Some(before) = ctx.measuring.lock().map_err(|e| e.to_string())?.insert(tab, stop.clone()) {
                before.store(true, Ordering::Relaxed);
            }
        }
        None => paths.iter().for_each(|p| sizer.forget(p)),
    }
    tauri::async_runtime::spawn_blocking(move || paths.into_iter().filter_map(|p| sizer.measure(&p, &stop).map(|s| (p, s))).collect())
        .await
        .map_err(|e| e.to_string())
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
    let editor = ctx.cfg().editor.clone();
    match &editor {
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

// ---------------------------------------------------------------- archives

#[derive(Serialize)]
struct ArchiveListing {
    entries: Vec<coxswain_core::archive::ArchiveEntry>,
    more: bool,
}

#[tauri::command]
async fn archive_list(path: PathBuf) -> Res<ArchiveListing> {
    let (entries, more) = coxswain_core::archive::list(&path, 2000).map_err(|e| e.to_string())?;
    Ok(ArchiveListing { entries, more })
}

#[tauri::command]
async fn extract(paths: Vec<PathBuf>, base: PathBuf, dest: String, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let dst = resolve(&base, &dest);
    ctx.sizer.forget(&dst.join("new"));
    each(&paths, |p| coxswain_core::archive::extract_locked(p, &dst, password.as_deref()).map(drop))
}

/// The password of the locked archive `path` is in (or is), kept in memory for this run.
#[tauri::command]
fn archive_password(path: PathBuf, password: String) {
    if let Some((archive, _)) = coxswain_core::archive::split(&path).or_else(|| coxswain_core::archive::is_archive(&path).then(|| (path.clone(), String::new()))) {
        coxswain_core::archive::remember(&archive, &password);
    }
}

/// A new archive at `dest` (zip, tar or tar.gz, by its name) with `paths` in it.
#[tauri::command]
async fn pack(paths: Vec<PathBuf>, base: PathBuf, dest: String, ctx: tauri::State<'_, Ctx>) -> Res<PathBuf> {
    let to = resolve(&base, &dest);
    ctx.sizer.forget(&to);
    coxswain_core::archive::create(&to, &paths).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(to)
}

// ---------------------------------------------------------------- properties

#[derive(Serialize)]
struct Props {
    path: PathBuf,
    /// "file", "folder" or "symlink"; the UI shows it in its language.
    kind: &'static str,
    link_target: Option<PathBuf>,
    size: u64,
    files: u64,
    created: Option<u64>,
    modified: Option<u64>,
    accessed: Option<u64>,
    readonly: bool,
    /// Unix permission bits, e.g. 0o644, and owner ids.
    mode: Option<u32>,
    uid: Option<u32>,
    gid: Option<u32>,
}

fn secs(t: std::io::Result<std::time::SystemTime>) -> Option<u64> {
    Some(t.ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs())
}

#[tauri::command]
async fn properties(path: PathBuf) -> Res<Props> {
    let lmeta = std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let meta = std::fs::metadata(&path).unwrap_or_else(|_| lmeta.clone());
    let (size, files) = bfs::dir_size(&path);
    #[cfg(unix)]
    let (mode, uid, gid) = {
        use std::os::unix::fs::MetadataExt;
        (Some(meta.mode() & 0o7777), Some(meta.uid()), Some(meta.gid()))
    };
    #[cfg(not(unix))]
    let (mode, uid, gid) = (None, None, None);
    Ok(Props {
        kind: if lmeta.is_symlink() { "symlink" } else if meta.is_dir() { "folder" } else { "file" },
        link_target: std::fs::read_link(&path).ok(),
        size,
        files,
        created: secs(meta.created()),
        modified: secs(meta.modified()),
        accessed: secs(meta.accessed()),
        readonly: meta.permissions().readonly(),
        mode,
        uid,
        gid,
        path,
    })
}

/// Unix permission bits when `mode` is given, else the read-only flag.
#[tauri::command]
fn set_permissions(path: PathBuf, mode: Option<u32>, readonly: bool) -> Res<()> {
    let mut perms = std::fs::metadata(&path).map_err(|e| e.to_string())?.permissions();
    match mode {
        #[cfg(unix)]
        Some(m) => std::os::unix::fs::PermissionsExt::set_mode(&mut perms, m & 0o7777),
        _ => perms.set_readonly(readonly),
    }
    std::fs::set_permissions(&path, perms).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- clipboard

// Linux file managers exchange percent-encoded `file://` URIs; macOS and Windows take paths.
fn to_clip(p: &Path) -> String {
    if !cfg!(target_os = "linux") {
        return p.to_string_lossy().into_owned();
    }
    let mut s = String::from("file://");
    for &b in p.as_os_str().as_encoded_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            s.push(b as char);
        } else {
            s += &format!("%{b:02X}");
        }
    }
    s
}

fn from_clip(s: &str) -> PathBuf {
    let Some(rest) = s.strip_prefix("file://") else { return PathBuf::from(s) };
    let (b, mut out, mut i) = (rest.as_bytes(), vec![], 0);
    while i < b.len() {
        let hex = (b[i] == b'%').then(|| std::str::from_utf8(b.get(i + 1..i + 3)?).ok()).flatten();
        match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            Some(v) => {
                out.push(v);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    PathBuf::from(String::from_utf8_lossy(&out).into_owned())
}

fn os_clipboard() -> Option<clipboard_rs::ClipboardContext> {
    clipboard_rs::ClipboardContext::new().ok()
}

/// Put files on the system clipboard (other file managers can paste them); a cut is
/// remembered here, since there is no portable way to mark one.
#[tauri::command]
fn clip_set(paths: Vec<PathBuf>, cut: bool, ctx: tauri::State<Ctx>) -> Res<()> {
    use clipboard_rs::Clipboard;
    if let Some(c) = os_clipboard() {
        // Best effort: without a system clipboard, paste inside Coxswain still works.
        let _ = c.set_files(paths.iter().map(|p| to_clip(p)).collect());
    }
    *ctx.clip.lock().map_err(|e| e.to_string())? = (paths, cut);
    Ok(())
}

/// Paste into `dir`: files from the system clipboard, else what Coxswain copied. Moves when
/// Coxswain cut exactly those files. A name that is taken becomes `name (2)`.
#[tauri::command]
async fn paste(dir: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<(usize, bool)> {
    use clipboard_rs::Clipboard;
    let os: Vec<PathBuf> = os_clipboard().and_then(|c| c.get_files().ok()).unwrap_or_default().iter().map(|s| from_clip(s)).collect();
    let mut clip = ctx.clip.lock().map_err(|e| e.to_string())?;
    let cut = clip.1 && (os.is_empty() || os == clip.0);
    let paths = if os.is_empty() { clip.0.clone() } else { os };
    if paths.is_empty() {
        return Err(coxswain_core::t!("err.clipboard_no_files"));
    }
    if cut {
        // A cut pastes once.
        *clip = (vec![], false);
    }
    drop(clip);
    // ponytail: text copied elsewhere after a Coxswain copy still pastes Coxswain's files on
    // clipboards that report "no files" as empty; track the clipboard owner if that confuses.
    each(&paths, |p| {
        if cut && p.parent() == Some(dir.as_path()) {
            return Ok(()); // cut and pasted in place
        }
        let to = bfs::free_name(&dir, &p.file_name().unwrap_or_default().to_string_lossy());
        ctx.sizer.forget(&to);
        if cut {
            ctx.sizer.forget(p);
        }
        if cut { bfs::rename(p, &to).map(drop) } else { bfs::copy(p, &to).map(drop) }
    })?;
    Ok((paths.len(), cut))
}

// ---------------------------------------------------------------- drag out, watching

/// A native drag, so files can be dropped on other applications (and back on Coxswain).
#[tauri::command]
fn start_drag(paths: Vec<PathBuf>, window: tauri::Window) -> Res<()> {
    let app = window.app_handle().clone();
    app.run_on_main_thread(move || {
        let icon = drag::Image::Raw(include_bytes!("../icons/32x32.png").to_vec());
        #[cfg(target_os = "linux")]
        let target = window.gtk_window();
        #[cfg(not(target_os = "linux"))]
        let target = Ok::<_, tauri::Error>(window.clone());
        if let Ok(w) = target {
            let _ = drag::start_drag(&w, drag::DragItem::Files(paths), icon, |_, _| {}, drag::Options::default());
        }
    })
    .map_err(|e| e.to_string())
}

/// Watch exactly these folders (not their subfolders).
#[tauri::command]
fn watch_dirs(dirs: Vec<PathBuf>, ctx: tauri::State<Ctx>) -> Res<()> {
    use notify::{RecursiveMode, Watcher};
    let mut watched = ctx.watched.lock().map_err(|e| e.to_string())?;
    let mut w = ctx.watcher.lock().map_err(|e| e.to_string())?;
    let Some(w) = w.as_mut() else { return Ok(()) };
    for d in watched.iter().filter(|d| !dirs.contains(d)) {
        let _ = w.unwatch(d);
    }
    for d in dirs.iter().filter(|d| !watched.contains(d)) {
        let _ = w.watch(d, RecursiveMode::NonRecursive);
    }
    *watched = dirs;
    Ok(())
}

/// Emit `dir-changed` with the watched folders that changed, at most every 250 ms.
fn start_watcher(app: &tauri::AppHandle) -> Option<notify::RecommendedWatcher> {
    use notify::Watcher;
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let watcher = notify::RecommendedWatcher::new(tx, notify::Config::default()).ok()?;
    let app = app.clone();
    std::thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            std::thread::sleep(std::time::Duration::from_millis(250));
            let events = std::iter::once(first).chain(rx.try_iter());
            let paths: Vec<PathBuf> = events.flatten().filter(|e| !e.kind.is_access()).flat_map(|e| e.paths).collect();
            paths.iter().for_each(|p| app.state::<Ctx>().sizer.forget(p));
            let watched = app.state::<Ctx>().watched.lock().map(|w| w.clone()).unwrap_or_default();
            let changed: Vec<&PathBuf> = watched.iter().filter(|d| paths.iter().any(|p| p == *d || p.parent() == Some(d.as_path()))).collect();
            if !changed.is_empty() {
                let _ = app.emit("dir-changed", changed);
            }
        }
    });
    Some(watcher)
}

// ---------------------------------------------------------------- duplicates

/// Scan for duplicate files and folders; one scan at a time. Poll `dupes_progress` meanwhile.
#[tauri::command]
async fn dupes_scan(options: coxswain_core::dupes::Options, ctx: tauri::State<'_, Ctx>) -> Res<coxswain_core::dupes::Report> {
    let p = Arc::new(coxswain_core::dupes::Progress::default());
    *ctx.dupes.lock().map_err(|e| e.to_string())? = Some(p.clone());
    let report = tauri::async_runtime::spawn_blocking(move || coxswain_core::dupes::scan(&options, &p)).await.map_err(|e| e.to_string());
    let cancelled = ctx.dupes.lock().map_err(|e| e.to_string())?.take().is_some_and(|p| p.cancel.load(std::sync::atomic::Ordering::Relaxed));
    if cancelled { Err(coxswain_core::t!("err.cancelled")) } else { report }
}

#[derive(Serialize)]
struct DupesProgress {
    phase: u64,
    files: u64,
    done: u64,
    total: u64,
    bytes: u64,
}

#[tauri::command]
fn dupes_progress(ctx: tauri::State<Ctx>) -> Option<DupesProgress> {
    use std::sync::atomic::Ordering::Relaxed;
    let p = ctx.dupes.lock().ok()?.clone()?;
    Some(DupesProgress { phase: p.phase.load(Relaxed), files: p.files.load(Relaxed), done: p.done.load(Relaxed), total: p.total.load(Relaxed), bytes: p.bytes.load(Relaxed) })
}

#[tauri::command]
fn dupes_cancel(ctx: tauri::State<Ctx>) {
    if let Some(p) = ctx.dupes.lock().ok().and_then(|g| g.clone()) {
        p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------- commands & scripts

fn shell(cmd: &str) -> std::process::Command {
    let (sh, flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
    let mut c = coxswain_core::tools::command(sh);
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
        ctx.cfg().user_menu.iter().enumerate().map(|(i, u)| Script { key: u.key.clone(), label: u.label.clone(), user: Some(i), path: None }).collect();
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
        let cmd = ctx.cfg().user_menu.get(i).ok_or_else(|| coxswain_core::t!("err.no_such_command"))?.expand(&dir, file.as_deref(), &selected);
        return output(shell(&cmd), &dir);
    }
    let script = path.ok_or_else(|| coxswain_core::t!("err.nothing_to_run"))?;
    // Only files from the scripts directory may run this way.
    let allowed = scripts_dir().and_then(|d| std::fs::canonicalize(d).ok());
    let real = std::fs::canonicalize(&script).map_err(|e| e.to_string())?;
    if !allowed.is_some_and(|d| real.starts_with(d)) {
        return Err(coxswain_core::t!("err.not_coxswain_script"));
    }
    let args = if selected.is_empty() { file.into_iter().collect() } else { selected };
    let mut c = coxswain_core::tools::command(&real);
    c.args(args);
    output(c, &dir)
}

/// A newer release, if the (daily, cached) check found one, and the command that upgrades this
/// copy. The network call runs outside the state lock so other commands are not held up.
#[tauri::command]
async fn check_update(ctx: tauri::State<'_, Ctx>) -> Res<Option<(String, Option<&'static str>)>> {
    use coxswain_core::update;
    if !ctx.cfg().check_updates {
        return Ok(None);
    }
    let due = update::due(&*ctx.state.lock().map_err(|e| e.to_string())?);
    if due {
        if let Some(latest) = update::fetch_latest() {
            ctx.edit(|st| update::record(st, latest))?;
        }
    }
    let v = update::available(&*ctx.state.lock().map_err(|e| e.to_string())?);
    Ok(v.map(|v| (v, update::upgrade_hint())))
}

fn main() {
    // Started by an app, not by hand: hold the file name index for all of them. No window.
    if std::env::args().nth(1).as_deref() == Some(helper::ARG) {
        return drop(helper::serve());
    }
    let cfg = Config::load().unwrap_or_else(|e| {
        eprintln!("coxswain: {e}; using defaults");
        Config::default()
    });
    coxswain_core::i18n::set_language(coxswain_core::i18n::resolve(&cfg.language));
    // Validate early: a bad key in the config should not surface as a blank window.
    if let Err(e) = cfg.keymap() {
        eprintln!("coxswain: {e}");
    }
    // Launched from the Dock, a macOS app gets the bare system PATH, so docker, podman,
    // pandoc and the rest of Homebrew are invisible. Ask the login shell for the real one.
    if cfg!(target_os = "macos") {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        if let Ok(out) = coxswain_core::tools::command(shell).args(["-lc", "echo $PATH"]).output() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() && !path.is_empty() {
                // Safety: nothing else runs yet, so no other thread reads the environment.
                unsafe { std::env::set_var("PATH", path) };
            }
        }
    }
    // Launched from a desktop menu the cwd is usually `/`; home is a better start.
    let home = std::env::home_dir().unwrap_or_default();
    let cwd = std::env::current_dir().ok().filter(|d| d.parent().is_some()).unwrap_or(home);
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `coxswain-gui --settings[=section] [folders…]`: start with the Settings window open, at
    // that section (`search`).
    let open_settings = args.first().and_then(|a| a.strip_prefix("--settings")).filter(|r| r.is_empty() || r.starts_with('=')).map(|r| r.trim_start_matches('=').to_string());
    if open_settings.is_some() {
        args.remove(0);
    }
    // `coxswain-gui --duplicates [folders…]`: the folders (default: the current one) are scanned for
    // duplicates at start instead of being opened in the panes.
    let duplicates = (args.first().map(String::as_str) == Some("--duplicates")).then(|| {
        let roots: Vec<PathBuf> = args.drain(..).skip(1).map(|a| resolve(&cwd, &a)).collect();
        if roots.is_empty() { vec![cwd.clone()] } else { roots }
    });
    let dir = |i: usize| args.get(i).map(|a| resolve(&cwd, a)).unwrap_or_else(|| cwd.clone());
    let index = Client::start(&cfg.search);
    let ctx = Ctx {
        sizer: Arc::new(coxswain_core::sizes::Sizer::new(Some(index.clone()))),
        index,
        start: [dir(0), dir(1)],
        duplicates,
        open_settings,
        state: Mutex::new({
            // A first start of this version is remembered, so the next update is told of.
            let mut st = AppState::load();
            if coxswain_core::notices::started(&mut st) {
                let _ = st.save();
            }
            st
        }),
        cfg: std::sync::RwLock::new(cfg),
        watched: Mutex::default(),
        watcher: Mutex::default(),
        clip: Mutex::default(),
        dupes: Mutex::default(),
        meaning: Mutex::default(),
        measuring: Mutex::default(),
        asking: Arc::default(),
    };
    tauri::Builder::default()
        .manage(ctx)
        .setup(|app| {
            *app.state::<Ctx>().watcher.lock().expect("fresh lock") = start_watcher(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config, notices, dismiss_notice, set_title, index_status, index_action, index_service, meaning_status, meaning_action, meaning_models, meaning_pull, list_dir, git_status, places, disks, get_state, save_session, save_favorites, set_tags, set_note, get_note,
            search, ask, ask_stop, resolve_path, copy, rename, delete, mkdir, dir_sizes, rename_plan, rename_apply, open_path, edit_path,
            read_text, run_command, scripts, run_script, check_update, archive_list, extract, pack, archive_password, properties, set_permissions,
            clip_set, paste, start_drag, watch_dirs, preview::git_diff, preview::sqlite_info, preview::epub_preview,
            preview::file_facts, preview::cert_info, bom::bom_info, bom::bom_node, bom::bom_diff, preview::mail_preview, preview::plist_xml, convert::preview_engines, convert::preview_cache, convert::clear_preview_cache,
            convert::convert, convert::images, convert::pull_image, convert::remove_image, convert::pull_progress, dupes_scan,
            dupes_progress, dupes_cancel, save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running Coxswain");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_keep_comments_and_other_keys() {
        let text = "# my config\ntheme = \"nc\"  # terminal\n\n[gui]\n# big text\nfont_size = 15\n\n[keys]\nquit = [\"F10\"]\n";
        let mut ch = serde_json::Map::new();
        ch.insert("font_size".into(), 17.into());
        ch.insert("language".into(), "da".into());
        ch.insert("latex_image".into(), "texlive:medium".into());
        ch.insert("show_hidden".into(), false.into());
        ch.insert("names_only".into(), serde_json::json!(["/home/me/Mail"]));
        let out = apply_settings(text, &ch).unwrap();
        for kept in ["# my config", "theme = \"nc\"  # terminal", "# big text", "quit = [\"F10\"]"] {
            assert!(out.contains(kept), "{kept} lost:\n{out}");
        }
        let cfg = Config::parse(&out).unwrap();
        assert_eq!((cfg.gui.font_size, cfg.language.as_str(), cfg.show_hidden), (17.0, "da", false));
        assert_eq!(cfg.preview.images["latex"], "texlive:medium");
        assert_eq!(cfg.search.names_only, [PathBuf::from("/home/me/Mail")]);
        assert_eq!(cfg.preview.images["plantuml"], "docker.io/plantuml/plantuml:latest", "other defaults stay");
        assert!(apply_settings(text, &serde_json::Map::from_iter([("nope".into(), 1.into())])).is_err());
    }

    #[test]
    fn clipboard_uris_roundtrip() {
        let p = PathBuf::from("/tmp/a b/ø%.txt");
        if cfg!(target_os = "linux") {
            assert_eq!(to_clip(&p), "file:///tmp/a%20b/%C3%B8%25.txt");
        }
        assert_eq!(from_clip(&to_clip(&p)), p);
        assert_eq!(from_clip("file:///x/%zz"), PathBuf::from("/x/%zz"));
    }
}

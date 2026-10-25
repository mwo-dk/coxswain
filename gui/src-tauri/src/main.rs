//! Coxswain GUI: Tauri commands over coxswain-core. The Svelte side owns all UI state except what
//! persists (session, favorites, tags, notes), which lives in `coxswain_core::state`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use coxswain_core::config::{color_to_rgb, Action, Config, Glyphs, GuiConfig};
use coxswain_core::fs::{self as bfs, Entry, SortKey};
use coxswain_core::icons::{self, Icon};
use coxswain_core::helper::{self, Client};
use coxswain_core::index::State;
use coxswain_core::rename::{self, Flags, Planned};
use coxswain_core::undo::{History, Kind as UndoKind, Op, Record};
use coxswain_core::state::{AppState, FavoriteGroup};
use coxswain_core::git;
use coxswain_core::history;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

mod asset;
mod attrs;
mod folders_sync;
mod bom;
mod chess;
mod compare;
mod go;
mod provenance;
mod convert;
mod ebook;
mod preview;
mod recent;
mod replace;
mod work;
mod tabs;
mod viruses;

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
    /// The first start: the first-run guide opens.
    guide: bool,
    /// Folders shown in the panes, watched so they reread themselves.
    watched: Mutex<Vec<PathBuf>>,
    watcher: Mutex<Option<coxswain_core::DirWatcher>>,
    /// What Coxswain last put on the clipboard, and whether it was a cut.
    clip: Mutex<(Vec<PathBuf>, bool)>,
    /// The running duplicate scan's progress, if any.
    dupes: Mutex<Option<Arc<coxswain_core::dupes::Progress>>>,
    /// Space: the walk running, if any, and the tree it made last.
    space_walk: Mutex<Option<Arc<coxswain_core::space::Progress>>>,
    space: Arc<Mutex<SpaceTree>>,
    sizer: Arc<coxswain_core::sizes::Sizer>,
    /// The model download for search by meaning, while one runs or has failed.
    meaning: Mutex<Option<(Arc<coxswain_core::meaning::Progress>, Option<String>)>>,
    /// The download of a built-in chat model (its `ask_model`), while one runs or has failed.
    chat: Mutex<Option<ChatDownload>>,
    /// The stop flag of each tab's background measuring.
    measuring: Mutex<std::collections::HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
    /// Which Ask may still write its answer; a new question or Stop moves it on.
    asking: Arc<std::sync::atomic::AtomicU64>,
    /// What Ctrl+Z undoes, the newest last; for this run.
    undo: Mutex<History>,
    /// The Stop of the file operations started since the last Stop: the one running and those
    /// waiting their turn, which a Stop keeps from starting. Replaced by each Stop.
    job_stop: Mutex<Arc<std::sync::atomic::AtomicBool>>,
    /// File operations run one at a time, in the order they were asked for.
    job_turn: tauri::async_runtime::Mutex<()>,
    /// The app, set at start: what tells the window that sizes changed.
    app: std::sync::OnceLock<tauri::AppHandle>,
}

impl Ctx {
    /// Keep what can be undone of an operation, done now: the folders it changed are measured again.
    pub(crate) fn remember(&self, rec: Record) {
        self.changed(&rec.touched());
        if let Ok(mut h) = self.undo.lock() {
            h.push(rec);
        }
    }

    /// These were added, removed or written by an operation that has ended: every folder they
    /// are in loses its remembered size, here and in the window (`sizes-changed`), which
    /// measures those again. A measure taken while it ran would show a size halfway.
    fn changed(&self, paths: &[PathBuf]) {
        paths.iter().for_each(|p| self.sizer.forget(p));
        if let Some(app) = self.app.get() {
            let _ = app.emit("sizes-changed", paths);
        }
    }

    pub fn cfg(&self) -> std::sync::RwLockReadGuard<'_, Config> {
        self.cfg.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Change the persisted state and save it right away (the file is small).
    fn edit<T>(&self, f: impl FnOnce(&mut AppState) -> T) -> Res<T> {
        self.edit_if(|st| (f(st), true))
    }

    /// `edit`, saved only when `f` says it changed something. What others wrote since (the
    /// terminal app, a reset of the hints) is read first, so it is not saved over.
    fn edit_if<T>(&self, f: impl FnOnce(&mut AppState) -> (T, bool)) -> Res<T> {
        let mut st = self.state.lock().map_err(|e| e.to_string())?;
        st.reload();
        let (out, changed) = f(&mut st);
        if changed {
            st.save().map_err(|e| format!("saving state: {e}"))?;
        }
        Ok(out)
    }
}

type Res<T> = Result<T, String>;

// A command without `async` runs on the main thread, the webview's: everything that reads or
// writes the disk, the clipboard or the config is `async` and runs on the async runtime
// instead, so a slow disk never freezes the window. Its slow part (a disk read, a process to
// wait for) goes through `blocking`, so it does not hold one of the runtime's few workers.
// What needs the main thread (`set_title`, `start_drag`) and what only reads memory stays plain.

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
    /// The key Find names for its scope (`Config::handy_key`).
    find_key: String,
    /// F2's scripts folder (`<config>/scripts`), there or not.
    scripts_dir: Option<PathBuf>,
    /// Group headings with their actions, in the order F1 and F9 list them.
    groups: Vec<(String, Vec<&'static str>)>,
    /// Every theme, by name, so the GUI can switch live.
    themes: BTreeMap<String, UiTheme>,
    /// Each theme's look (shapes and chrome, see `Theme::look`), by name.
    looks: BTreeMap<String, String>,
    /// Which themes draw the tube (`Theme::crt`), by name.
    crt: BTreeMap<String, bool>,
    /// The built-in themes, in the order Settings shows them: (name, `theme.group.*` key).
    builtin_themes: Vec<(&'static str, &'static str)>,
    glyphs: Glyphs,
    show_hidden: bool,
    folder_sizes: bool,
    confirm_delete: bool,
    start: [PathBuf; 2],
    duplicates: Option<Vec<PathBuf>>,
    /// Start with Settings open, at this section ("" for the top).
    open_settings: Option<String>,
    /// The language in use (resolved from `language`), its texts, and whether it is written
    /// right to left.
    language: &'static str,
    strings: std::collections::HashMap<String, serde_json::Value>,
    rtl: bool,
    /// Every language Coxswain has, by region; where to help improve a new translation.
    languages: &'static [coxswain_core::i18n::Language],
    improve_url: &'static str,
    /// The config file's raw values, for the Settings window, by option name.
    settings: serde_json::Map<String, serde_json::Value>,
    /// What Settings shows: every option with its area and costs, every name it can be opened
    /// at, what can leave the machine, and where things are kept.
    options: &'static [coxswain_core::settings::Opt],
    sections: Vec<(&'static str, coxswain_core::settings::Area, Option<&'static str>)>,
    outbound: Vec<coxswain_core::settings::Out>,
    /// The server search by meaning and Ask use (`settings::server_url`), its host and whether
    /// it is this machine.
    server_url: String,
    server_host: String,
    server_local: bool,
    /// The server the update check asks, whether or not it is on.
    update_host: &'static str,
    paths: Vec<(&'static str, Option<PathBuf>)>,
    /// The search level the settings make (none for a mix), and each level's costs.
    level: Option<coxswain_core::settings::Level>,
    levels: Vec<(coxswain_core::settings::Level, &'static [coxswain_core::settings::Cost])>,
    config_path: Option<PathBuf>,
    gui: GuiConfig,
    home: PathBuf,
    sep: char,
    version: &'static str,
    /// Why video and sound cannot play in the preview here, if they cannot.
    media_missing: Option<String>,
    /// The formats Pack offers.
    pack_formats: &'static [coxswain_core::archive::PackFormat],
    /// Every archive ending, and those of archives that take a password (`archive::endings`).
    archive_endings: (Vec<&'static str>, Vec<&'static str>),
    /// Every ending a disk image may have (`disk_image::endings`): `.vhd` and `.img` are checked
    /// with `disk_image_facts` before they are taken for one.
    disk_endings: Vec<&'static str>,
    /// The endings of the old pictures the core reads (`pictures::endings`), without the dot.
    picture_endings: Vec<&'static str>,
    /// What ffmpeg and ImageMagick take (`media`), by kind: `video`, `video_other`, `audio`,
    /// `audio_other`, `heif`, `magick`, `raw`, and `pictures` (what *Convert or edit pictures* takes).
    media_endings: BTreeMap<&'static str, Vec<&'static str>>,
    /// What *Convert video or audio* and *Convert or edit pictures* offer, in their order, as
    /// `media::Op::id`s; `clip:…` and `resize:…` ask for their values.
    media_ops: [Vec<String>; 5],
    /// Geodata's endings (`geo::endings`): `vector`, and `raster` (TIFF, geodata when GDAL says so).
    geo_endings: BTreeMap<&'static str, Vec<&'static str>>,
    /// Open the first-run guide at start; its keys and themes.
    guide: bool,
    guide_keys: Vec<(String, String)>,
    /// Step 1's line on the action menu and right-clicks.
    guide_menu: String,
    guide_features: String,
    guide_themes: [&'static str; 4],
    /// The line that installs each search tool and the icon font here, where Coxswain knows one
    /// (a preview tool's comes with its engines, `preview_engines`).
    installs: BTreeMap<&'static str, Option<String>>,
    /// Ask's chat model as the user sees it: a built-in one by its name.
    ask_name: String,
    /// The trash gives back what went into it, so Ctrl+Z undoes a move to the trash.
    trash_restores: bool,
    /// The endings of recordings whose speech can be transcribed (`transcribe::FORMATS`).
    speech_endings: &'static [&'static str],
}

fn css(c: &str) -> Option<String> {
    color_to_rgb(c).map(|(r, g, b)| format!("#{r:02x}{g:02x}{b:02x}"))
}

#[tauri::command(async)]
fn get_config(ctx: tauri::State<Ctx>) -> Res<UiConfig> {
    let cfg = ctx.cfg();
    let server_url = coxswain_core::settings::server_url(&cfg.search);
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
        scripts_dir: coxswain_core::user_menu::scripts_dir(),
        find_key: cfg.handy_key(Action::Search).unwrap_or("").to_string(),
        actions: Action::ALL.iter().map(|&a| (a.name(), (a.label(), cfg.key_for(a).unwrap_or("").to_string()))).collect(),
        groups: coxswain_core::config::Group::ALL.iter().map(|g| (g.label(), g.actions().map(Action::name).collect())).collect(),
        themes,
        looks: cfg.themes.iter().map(|(name, t)| (name.clone(), t.look.clone())).collect(),
        crt: cfg.themes.iter().map(|(name, t)| (name.clone(), t.crt)).collect(),
        builtin_themes: coxswain_core::config::Theme::BUILTIN.iter().map(|(name, group, _)| (*name, *group)).collect(),
        glyphs: cfg.glyphs(),
        show_hidden: cfg.show_hidden,
        folder_sizes: cfg.folder_sizes,
        confirm_delete: cfg.confirm_delete,
        start: ctx.start.clone(),
        duplicates: ctx.duplicates.clone(),
        open_settings: ctx.open_settings.clone(),
        language: coxswain_core::i18n::language(),
        strings: coxswain_core::i18n::catalogue(coxswain_core::i18n::language()),
        rtl: coxswain_core::i18n::is_rtl(coxswain_core::i18n::language()),
        languages: coxswain_core::i18n::LANGUAGES,
        improve_url: coxswain_core::i18n::IMPROVE_URL,
        settings: coxswain_core::settings::values(&cfg),
        options: coxswain_core::settings::OPTIONS,
        sections: coxswain_core::settings::sections(),
        outbound: coxswain_core::settings::outbound(&cfg),
        server_host: coxswain_core::settings::host(&server_url).to_string(),
        server_local: coxswain_core::settings::is_local(&server_url),
        server_url,
        update_host: coxswain_core::update::host(),
        paths: Config::paths(),
        level: coxswain_core::settings::level(&cfg.search),
        levels: coxswain_core::settings::Level::ALL.iter().map(|l| (*l, l.costs())).collect(),
        config_path: Config::path(),
        gui: cfg.gui.clone(),
        home: coxswain_core::dirs::home_dir().unwrap_or_default(),
        sep: std::path::MAIN_SEPARATOR,
        version: coxswain_core::update::VERSION,
        media_missing: coxswain_core::tools::media_missing(),
        pack_formats: coxswain_core::archive::PACK_FORMATS,
        archive_endings: coxswain_core::archive::endings(),
        disk_endings: coxswain_core::disk_image::endings(),
        picture_endings: coxswain_core::pictures::endings(),
        media_endings: coxswain_core::media::endings(),
        media_ops: coxswain_core::media::Program::ALL.map(|p| coxswain_core::media::Op::menu(p).iter().map(|o| o.id()).collect()),
        geo_endings: coxswain_core::geo::endings(),
        guide: ctx.guide,
        guide_keys: coxswain_core::guide::keys(&cfg),
        guide_menu: coxswain_core::guide::menu_line(&cfg),
        guide_features: coxswain_core::guide::features_line(&cfg),
        guide_themes: coxswain_core::guide::THEMES,
        ask_name: coxswain_core::chat::shown(&cfg.search.ask_model),
        trash_restores: coxswain_core::undo::trash_restores(),
        speech_endings: coxswain_core::transcribe::FORMATS,
        installs: ["tesseract", "pdftoppm", "soffice", "gs", "nerd-font"].into_iter().map(|p| (p, coxswain_core::tools::install(p))).collect(),
    })
}

// ---------------------------------------------------------------- settings

/// Write the changed settings into config.toml, keeping its comments and layout, then use
/// the new config at once. Returns the new UI config (texts in the new language and so on).
#[tauri::command(async)]
fn save_settings(changes: serde_json::Map<String, serde_json::Value>, ctx: tauri::State<Ctx>) -> Res<UiConfig> {
    let cfg = coxswain_core::settings::save(&changes)?;
    *ctx.cfg.write().map_err(|e| e.to_string())? = cfg;
    // The helper reads its options when it starts: a helper with the new ones takes over.
    if changes.keys().any(|k| coxswain_core::settings::find(k).is_some_and(|o| o.restarts_helper())) {
        ctx.index.restart();
    }
    get_config(ctx)
}

/// Settings → Previews → Preview tools: how `kind` is made ("local", "container", "off"; ""
/// follows How previews are made), written into config.toml and used at once.
#[tauri::command(async)]
fn set_preview_tool(kind: String, choice: String, ctx: tauri::State<Ctx>) -> Res<UiConfig> {
    let path = coxswain_core::config::Config::path().ok_or_else(|| coxswain_core::t!("err.no_config_folder"))?;
    let cfg = coxswain_core::preview_tools::set_choice(&path, &kind, &choice)?;
    *ctx.cfg.write().map_err(|e| e.to_string())? = cfg;
    get_config(ctx)
}

/// Settings → Finding files: one line for each part of search, with its next step.
#[tauri::command]
async fn search_status(ctx: tauri::State<'_, Ctx>) -> Res<Vec<coxswain_core::settings::Line>> {
    let (index, cfg) = (ctx.index.clone(), ctx.cfg().search.clone());
    blocking(move || Ok(coxswain_core::settings::status(&cfg, &index.status(), index.shared(), &coxswain_core::setup::poor(&cfg, false)))).await
}

/// What choosing a search level changes; none when the setup guide has to choose a model first.
#[tauri::command]
fn search_level(level: coxswain_core::settings::Level, ctx: tauri::State<Ctx>) -> Option<serde_json::Map<String, serde_json::Value>> {
    coxswain_core::settings::level_changes(level, &ctx.cfg().search, coxswain_core::meaning::installed())
}

// ---------------------------------------------------------------- action menu and hints

/// The action menu for what is under the cursor: each heading with its actions' names.
#[tauri::command]
fn action_menu(subject: coxswain_core::menu::Subject) -> Vec<(String, Vec<&'static str>)> {
    coxswain_core::menu::actions(&subject, true).into_iter().map(|(g, v)| (g.label(), v.into_iter().map(Action::name).collect())).collect()
}

/// The hint for what is under the cursor: its id and text. `last`: the subject the hint on show
/// was picked for, and its id.
#[tauri::command(async)]
fn hint(subject: coxswain_core::menu::Subject, last: Option<(coxswain_core::menu::Subject, String)>, ctx: tauri::State<Ctx>) -> Option<(&'static str, String)> {
    // The counts in memory, shared with `hint_once`: state.json is read and written only when
    // one changes (or another app wrote it), not each time the cursor moves.
    coxswain_core::menu::hint(&subject, &ctx.cfg(), last.as_ref().map(|(s, id)| (s, id.as_str())))
}

/// Whether the hint `id` that shows in place (Find's scope) is still to be shown; counted.
#[tauri::command(async)]
fn hint_once(id: String, ctx: tauri::State<Ctx>) -> bool {
    coxswain_core::menu::once(&id, &ctx.cfg())
}

/// F1 → Features: every feature with its keys and questions, filtered by `query`.
#[tauri::command(async)]
fn features(query: String, ctx: tauri::State<Ctx>) -> Vec<coxswain_core::features::Entry> {
    coxswain_core::features::list(&ctx.cfg(), &query, true)
}

/// "Show me": the feature's docs page on GitHub, in the browser. Only on that click: nothing
/// else in Features reaches the network.
#[tauri::command]
async fn open_docs(id: String) -> Res<String> {
    let url = coxswain_core::features::find(&id).ok_or_else(|| format!("no feature {id}"))?.url();
    let at = url.clone();
    tauri::async_runtime::spawn_blocking(move || bfs::open_default(Path::new(&at)).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())??;
    Ok(url)
}

#[tauri::command(async)]
fn hints_reset(ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| st.hints_shown.clear())
}

// ---------------------------------------------------------------- listing

#[derive(Serialize)]
struct Item {
    #[serde(flatten)]
    entry: Entry,
    icon: Icon,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
}

/// What a listing says of its folder, besides the entries.
#[derive(Serialize)]
struct Meta {
    dir: PathBuf,
    has_notes: bool,
    /// The archive the folder is inside, and whether something in it is locked.
    archive: Option<PathBuf>,
    locked: bool,
    /// That archive is a disk image: read-only.
    disk_image: bool,
    /// Inside an archive inside another: read-only, looked into from a copy in the cache.
    nested: bool,
    /// That archive is a container image: its first tag (`container_image::title`).
    image: Option<String>,
    /// The history the folder is in.
    history: Option<HistoryInfo>,
    /// The ZFS snapshot the folder is in, or the list of snapshots it is.
    snapshot: Option<coxswain_core::zfs::At>,
    /// The package whose files the folder lists.
    package: Option<String>,
    /// Files in it may belong to an installed package (`bsd::packages_here`).
    packages: bool,
    /// In a model store's folder: the model each entry is (`local_models::names`).
    #[serde(skip_serializing_if = "Option::is_none")]
    models: Option<std::collections::HashMap<String, String>>,
}

/// A history being looked into: the file or folder it is of, and the commit looked into.
#[derive(Serialize)]
struct HistoryInfo {
    target: PathBuf,
    /// The folder on disk it leads back to: the target, or the folder holding it.
    base: PathBuf,
    commit: Option<history::Commit>,
    /// A history's commits, the branches or the worktrees (what the list above a commit is).
    view: history::View,
    /// HEAD's commits, every branch's, or those against the upstream.
    scope: history::Scope,
    /// Against the upstream: which, how far ahead and behind, and the last fetch.
    upstream: Option<String>,
}

/// A folder's entries, sorted, and what the listing says of it.
async fn read_listing(dir: PathBuf, show_hidden: bool, sort: SortKey, reverse: bool, ctx: &Ctx) -> Res<(Vec<Entry>, Meta)> {
    // Reading the folder (an archive's, a history's: git) blocks: not on the runtime's workers.
    let d = dir.clone();
    let (entries, inside, in_history, snapshot, package) = tauri::async_runtime::spawn_blocking(move || {
        let dir = d;
        let (mut entries, inside) = bfs::list_with_archive(&dir, show_hidden).map_err(|e| format!("{}: {e}", dir.display()))?;
        // Gone into an archive: the user asked for it, so its files may be previewed.
        if let Some((archive, _)) = &inside {
            coxswain_core::cloud::ask_for(archive);
        }
        // By size, folders as they are measured are sorted again (the page's `resortSoon`).
        bfs::sort_in(&dir, &mut entries, sort, reverse, &Default::default());
        // By last commit: as the walk git did is known; else by name now, and the page sorts
        // again when `git_last` answers.
        if sort == SortKey::Commit
            && let Some(lasts) = history::last_changes_cached(&dir)
        {
            history::sort_by_last(&mut entries, &lasts, reverse);
        }
        let in_history = history::split(&dir).filter(|_| !dir.is_dir()).map(|at| HistoryInfo { commit: at.commit.as_deref().and_then(|c| history::show(&at.base, c).ok()), upstream: (at.scope == history::Scope::Upstream && at.commit.is_none()).then(|| history::upstream_line(&at.base)).flatten(), target: at.target, base: at.base, view: at.view, scope: at.scope });
        let snapshot = coxswain_core::zfs::at(&dir);
        let package = coxswain_core::bsd::split(&dir).and_then(|f| coxswain_core::bsd::package_of(&f));
        Ok::<_, String>((entries, inside, in_history, snapshot, package))
    })
    .await
    .map_err(|e| e.to_string())??;
    let has_notes = ctx.state.lock().map_err(|e| e.to_string())?.notes.contains_key(&dir);
    let (archive, locked) = inside.map_or((None, false), |(a, locked)| (Some(a), locked));
    let disk_image = archive.as_deref().is_some_and(coxswain_core::archive::read_only);
    let nested = archive.is_some() && coxswain_core::archive::nested_dir(&dir);
    let image = archive.as_deref().filter(|a| coxswain_core::archive::is_image(a)).and_then(coxswain_core::container_image::title);
    Ok((entries, Meta { nested, image, has_notes, packages: coxswain_core::bsd::packages_here(&dir), models: coxswain_core::local_models::names(&dir).map(|n| (*n).clone()), dir, archive, locked, disk_image, history: in_history, snapshot, package }))
}

/// What a folder holds, for its preview: only the counts, not the panel's listing.
#[tauri::command]
async fn dir_summary(dir: PathBuf, show_hidden: bool) -> Res<bfs::Summary> {
    tauri::async_runtime::spawn_blocking(move || bfs::summary(&dir, show_hidden).map_err(|e| format!("{}: {e}", dir.display()))).await.map_err(|e| e.to_string())?
}

/// The entries as the page gets them: with their icon and tag, and without the path when it
/// is `prefix` and the name (see `tabs::Rows::prefix`).
fn to_page(dir: &Path, entries: Vec<Entry>, tag: impl Fn(&Path) -> Option<String>, plain: Option<&Glyphs>) -> (String, Vec<Item>) {
    let joined = dir.join("x").to_string_lossy().into_owned();
    let prefix = joined.strip_suffix('x').unwrap_or_default().to_string();
    let items = entries
        .into_iter()
        .map(|mut e| {
            let tag = tag(&e.path);
            if e.path.to_str().and_then(|p| p.strip_prefix(prefix.as_str())) == Some(e.name.as_str()) {
                e.path = PathBuf::new();
            }
            Item { icon: icons::entry(&e.name, e.is_dir, e.is_symlink, plain), tag, entry: e }
        })
        .collect();
    (prefix, items)
}

#[derive(Serialize)]
struct GitLast {
    /// The commit the map is for: HEAD, or the commit looked into.
    head: String,
    /// `None` when `head` is what the page said it had: the map it has holds.
    lasts: Option<std::sync::Arc<history::Lasts>>,
}

/// The graph column's rows for the commits `from..to` of the history listed at `dir` (`..` is
/// none): worked out as far as the window in view, from what the listing read.
#[tauri::command]
fn history_graph(dir: PathBuf, from: usize, to: usize) -> Vec<coxswain_core::graph::Row> {
    history::graph(&dir, from, to)
}

/// The last commit of each entry of `dir` (a work tree's folder or a history's), as git's one
/// walk over the folder finds it. `None` outside a repository or when switched off.
#[tauri::command]
async fn git_last(dir: PathBuf, have: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<Option<GitLast>> {
    if !ctx.cfg().git.last_commit {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(move || history::last_changes_since(&dir, have.as_deref(), false).map(|(head, lasts)| GitLast { head, lasts })).await.map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct GitInfo {
    root: PathBuf,
    prompt: String,
    branch: String,
    /// File name (direct children of the directory) -> status.
    files: BTreeMap<String, git::FileStatus>,
    /// The status of every entry not in `files`: the folder's own, when it is untracked or
    /// ignored (then so is everything in it).
    all: Option<git::FileStatus>,
    /// The recent repositories, this one first (`AppState::touch_repo`).
    recent: Vec<PathBuf>,
}

#[tauri::command]
async fn git_status(dir: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<Option<GitInfo>> {
    // `git status` on a big repository takes a while: not on the async runtime's workers.
    let d = dir.clone();
    let Some(s) = tauri::async_runtime::spawn_blocking(move || git::Status::read(&d)).await.map_err(|e| e.to_string())? else { return Ok(None) };
    let (files, all) = children(&s, &dir);
    // Every listing comes through here: the state file is written only when the repo moves up.
    let moved = ctx.state.lock().map_err(|e| e.to_string())?.recent_repos.first() != Some(&s.root);
    let recent = if moved {
        ctx.edit_if(|st| {
            let changed = st.touch_repo(&s.root);
            (st.recent_repos.clone(), changed)
        })?
    } else {
        ctx.state.lock().map_err(|e| e.to_string())?.recent_repos.clone()
    };
    Ok(Some(GitInfo { prompt: s.prompt(&ctx.cfg().glyphs()), branch: s.summary.branch.clone(), root: s.root, files, all, recent }))
}

/// What the git line's tooltip and a repository folder's tooltip show, one fact a line: read
/// when hovered, never per listing. `dir` is a repository's own folder, or it has none.
#[tauri::command]
async fn git_facts(dir: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<Option<Vec<String>>> {
    let g = ctx.cfg().glyphs();
    blocking(move || Ok(git::facts(&dir, &g).map(|f| f.lines()))).await
}

/// Switch to the branch `entry` of the list of branches `dir`: git's message, or why it refused.
#[tauri::command]
async fn git_switch(dir: PathBuf, entry: String) -> Res<String> {
    blocking(move || coxswain_core::branches::switch(&dir, &entry).map_err(|e| e.to_string())).await
}

/// The branch an entry of the list of branches names.
#[tauri::command]
fn branch_name(entry: String) -> String {
    coxswain_core::branches::name_of(&entry)
}

/// A new branch `name`, switched to: from the branch `from` of the list of branches `dir`, or
/// from the current commit of the repository `dir` is in.
#[tauri::command]
async fn git_new_branch(dir: PathBuf, name: String, from: Option<String>) -> Res<String> {
    blocking(move || coxswain_core::branches::create(&dir, &name, from.as_deref()).map_err(|e| e.to_string())).await
}

/// Commit, push, pull and the rest: the repository `dir` is in, in the user's own git tool. A
/// terminal one opens in a terminal window. What the status line says, or why it could not.
#[tauri::command]
async fn git_tool(dir: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<String> {
    let tool = ctx.cfg().git.tool.clone();
    blocking(move || {
        let h = coxswain_core::handoff::handoff(&tool, &dir, false)?;
        coxswain_core::handoff::spawn(&h).map_err(|e| e.to_string())?;
        Ok(coxswain_core::handoff::opened(&h))
    })
    .await
}

/// The git config as git uses it in `dir`, read now: the repository's name (none outside one)
/// and every key with its value, scope and file, secrets hidden (`git_config`). Read-only.
#[tauri::command]
async fn git_config(dir: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<(Option<String>, Vec<coxswain_core::git_config::Entry>, bool)> {
    let cfg = ctx.cfg().clone();
    blocking(move || {
        let entries = coxswain_core::git_config::read(&dir)?;
        // How a hidden secret is shown: said the first few times one is there.
        let hint = entries.iter().any(|e| e.hidden.is_some()) && coxswain_core::menu::once("git_config_show", &cfg);
        Ok((coxswain_core::git_config::repo_name(&dir), entries, hint))
    })
    .await
}

/// Whether `path` is a model Netron draws: for a `.h5`, whether its bytes hold a Keras model.
#[tauri::command(async)]
fn is_model(path: PathBuf) -> bool {
    coxswain_core::neural::read(&path).is_some()
}

/// A status line's error in plain words (`fs::plain`).
#[tauri::command]
fn plain_error(raw: String) -> String {
    coxswain_core::fs::plain(&raw)
}

/// The git tools installed here, in the order they are tried: for Settings.
#[tauri::command(async)]
fn git_tools() -> Vec<&'static str> {
    coxswain_core::handoff::installed()
}

/// The statuses of `dir`'s entries, from git's alone: the folder is not read again.
fn children(s: &git::Status, dir: &Path) -> (BTreeMap<String, git::FileStatus>, Option<git::FileStatus>) {
    let files = s.files.iter().filter(|(p, _)| p.parent() == Some(dir)).filter_map(|(p, st)| Some((p.file_name()?.to_string_lossy().into_owned(), *st))).collect();
    let all = s.get(dir).filter(|st| matches!(st.kind, git::Kind::Untracked | git::Kind::Ignored));
    (files, all)
}

// ---------------------------------------------------------------- ZFS and FreeBSD

/// The ZFS dataset `dir` is on, for the footer: none off ZFS.
#[tauri::command]
async fn zfs_facts(dir: PathBuf) -> Res<Option<coxswain_core::zfs::Facts>> {
    blocking(move || Ok(coxswain_core::zfs::facts(&dir))).await
}

/// The boot environments and the jails, for the sidebar (FreeBSD).
#[tauri::command]
async fn bsd_places() -> Res<Vec<coxswain_core::bsd::Place>> {
    blocking(|| Ok(coxswain_core::bsd::places())).await
}

/// The cloud accounts on this machine, for the sidebar. Read again with the disks: a cloud
/// mounted meanwhile shows up. None while a hung cloud mount keeps the last look from coming
/// back, so the 30 s refresh does not pile up threads stuck on it.
#[tauri::command]
async fn clouds() -> Res<Vec<coxswain_core::bsd::Place>> {
    blocking(|| Ok(coxswain_core::clouds::places_within(std::time::Duration::from_secs(5)).unwrap_or_default())).await
}

// ---------------------------------------------------------------- sidebar

#[derive(Serialize)]
struct Place {
    name: String,
    path: PathBuf,
    icon: &'static str,
}

#[tauri::command(async)]
fn places() -> Vec<Place> {
    let p = |name: &str, dir: Option<PathBuf>, icon: &'static str| dir.filter(|d| d.is_dir()).map(|path| Place { name: coxswain_core::t!(name), path, icon });
    [
        p("place.home", coxswain_core::dirs::home_dir(), "\u{f015}"),
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
    #[serde(skip)]
    fstype: String,
}

/// The mounted disks and their free space. The window asks again every so often, so the
/// work runs on a blocking thread: a slow network mount must not hold up other commands.
#[tauri::command]
async fn disks() -> Res<Vec<Disk>> {
    tauri::async_runtime::spawn_blocking(read_disks).await.map_err(|e| e.to_string())
}

/// The mounted file systems as unlabelled disks: from sysinfo on Linux, macOS and Windows, which
/// also says which are removable.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn mounted_disks() -> Vec<Disk> {
    let list = sysinfo::Disks::new_with_refreshed_list();
    list.list().iter().map(|d| Disk { label: String::new(), device: d.name().to_string_lossy().into_owned(), mount: d.mount_point().to_path_buf(), total: d.total_space(), free: d.available_space(), removable: d.is_removable(), fstype: d.file_system().to_string_lossy().into_owned() }).collect()
}

/// The mounted file systems as unlabelled disks: from the core on the BSDs and illumos, which
/// sysinfo does not know or, on FreeBSD, counts free inodes as free space.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn mounted_disks() -> Vec<Disk> {
    coxswain_core::machine::mounted().into_iter().map(|m| Disk { label: String::new(), device: m.device, mount: m.mount, total: m.total, free: m.free, removable: false, fstype: m.fstype }).collect()
}

fn read_disks() -> Vec<Disk> {
    let mut out: Vec<Disk> = vec![];
    // Subvolumes and bind mounts repeat the same device; keep its shortest mount point.
    let mut sorted: Vec<Disk> = mounted_disks().into_iter().filter(|d| d.total > 0).collect();
    sorted.sort_by_key(|d| d.mount.as_os_str().len());
    for mut d in sorted {
        if coxswain_core::machine::hidden_mount(&d.mount, &d.fstype) || out.iter().any(|o| o.device == d.device) {
            continue;
        }
        d.label = if d.mount.parent().is_none() { coxswain_core::t!("place.system") } else { d.mount.file_name().map_or(d.device.clone(), |n| n.to_string_lossy().into_owned()) };
        out.push(d);
    }
    out
}

/// Before a history opens: the Subversion or CVS server it would be read from, when the user
/// has not said yes to it yet (the question is asked), else none.
#[tauri::command(async)]
async fn history_server(path: PathBuf) -> Res<Option<coxswain_core::wc::Ask>> {
    tauri::async_runtime::spawn_blocking(move || coxswain_core::wc::ask_first(&path)).await.map_err(|e| e.to_string())
}

/// Yes to a server: its history may be read from now on (`AppState::history_servers`).
#[tauri::command(async)]
fn history_allow(server: String, ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| {
        if !st.history_servers.contains(&server) {
            st.history_servers.push(server);
        }
        coxswain_core::wc::allow(&st.history_servers);
    })
}

/// Settings → Privacy: a server forgotten, asked about again next time.
#[tauri::command(async)]
fn history_forget(server: String, ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| {
        st.history_servers.retain(|s| *s != server);
        coxswain_core::wc::allow(&st.history_servers);
    })
}

#[tauri::command(async)]
fn get_state(ctx: tauri::State<Ctx>) -> Res<AppState> {
    ctx.state.lock().map(|s| s.clone()).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn save_session(session: serde_json::Value, ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| st.session = session)
}

#[tauri::command(async)]
fn save_favorites(favorites: Vec<FavoriteGroup>, ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| st.favorites = favorites)
}

#[tauri::command(async)]
fn set_tags(paths: Vec<PathBuf>, color: String, ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| paths.iter().for_each(|p| st.set_tag(p, &color)))
}

#[tauri::command(async)]
fn set_note(dir: PathBuf, text: String, ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(|st| st.set_note(&dir, &text))
}

#[tauri::command(async)]
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
    /// "Built-in model · on the GPU (Metal)", or on the CPU and why.
    meaning_runs_text: Option<String>,
}

#[tauri::command]
async fn index_status(ctx: tauri::State<'_, Ctx>) -> Res<IndexStatus> {
    let index = ctx.index.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let status = index.status();
        let meaning_runs_text = status.meaning_runs.as_ref().map(|r| coxswain_core::t!("settings.meaning_runs", "where" => r.text()));
        IndexStatus { status, path: coxswain_core::store::Store::path(), shared: index.shared(), service: coxswain_core::service::installed(), meaning_runs_text }
    })
    .await
    .map_err(|e| e.to_string())
}

/// What Settings → What's new lists: the notices not dismissed and the versions not read (their
/// count is on the Settings button), and the window's title: the version and the kinds of
/// search on.
#[derive(Serialize)]
struct Notices {
    notices: Vec<coxswain_core::notices::Notice>,
    unread: Vec<String>,
    title: String,
}

#[tauri::command]
async fn notices(ctx: tauri::State<'_, Ctx>) -> Res<Notices> {
    let (index, cfg) = (ctx.index.clone(), ctx.cfg().clone());
    let status = tauri::async_runtime::spawn_blocking(move || index.status()).await.map_err(|e| e.to_string())?;
    let st = ctx.state.lock().map_err(|e| e.to_string())?;
    Ok(Notices {
        notices: coxswain_core::notices::all(&cfg, &status, &st, false),
        unread: coxswain_core::notices::unread(&st).into_iter().map(|c| c.version).collect(),
        title: coxswain_core::t!("title.window", "version" => coxswain_core::update::VERSION),
    })
}

/// Every version's changes, newest first, for Settings → What's new.
#[tauri::command]
fn changes() -> Vec<coxswain_core::notices::Change> {
    coxswain_core::notices::changes()
}

/// What's new has been looked at: the versions up to this one are read.
#[tauri::command(async)]
fn read_changes(ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(coxswain_core::notices::read)
}

/// The window's title. On Linux the title bar that GTK draws keeps the title it was made
/// with, so it is told too.
#[tauri::command]
fn set_title(title: String, window: tauri::WebviewWindow) -> Res<()> {
    window.set_title(&title).map_err(|e| e.to_string())?;
    #[cfg(all(unix, not(target_os = "macos")))]
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

/// The first-run guide was closed: it does not open by itself again.
#[tauri::command(async)]
fn guide_seen(ctx: tauri::State<Ctx>) -> Res<()> {
    ctx.edit(coxswain_core::guide::seen)
}

/// Whether a Nerd Font is installed (None: it cannot be told), for the first-run guide.
#[tauri::command(async)]
fn nerd_font() -> Option<bool> {
    coxswain_core::tools::nerd_font()
}

/// A line of text on the system clipboard: an install command.
#[tauri::command(async)]
fn copy_text(text: String) -> Res<()> {
    use clipboard_rs::Clipboard;
    os_clipboard().ok_or_else(|| coxswain_core::t!("err.no_clipboard"))?.set_text(text).map_err(|e| e.to_string())
}

#[tauri::command(async)]
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

/// The model's files are looked at on a blocking thread, the download's lock not held meanwhile.
#[tauri::command]
async fn meaning_status(ctx: tauri::State<'_, Ctx>) -> Res<MeaningStatus> {
    use std::sync::atomic::Ordering;
    let (downloading, error) = {
        let m = ctx.meaning.lock().map_err(|e| e.to_string())?;
        (m.as_ref().filter(|(_, err)| err.is_none()).map(|(p, _)| (p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed))), m.as_ref().and_then(|(_, e)| e.clone()))
    };
    blocking(move || {
        Ok(MeaningStatus { installed: coxswain_core::meaning::installed(), size: coxswain_core::meaning::size(), folder: coxswain_core::meaning::folder(), downloading, error })
    })
    .await
}

/// A chat model's download: its `ask_model`, how far it is, and the error that stopped it.
type ChatDownload = (String, Arc<coxswain_core::meaning::Progress>, Option<String>);

/// A built-in chat model, for Settings and the setup.
#[derive(Serialize)]
struct ChatModel {
    /// Its `ask_model`.
    key: String,
    name: &'static str,
    size: u64,
    /// GB of memory it needs.
    ram: u64,
    installed: bool,
    /// The one for this machine.
    suggested: bool,
    /// How quickly it answers on this processor; None on a Mac's GPU.
    estimate: Option<String>,
}

/// The built-in chat models, a download under way (which, bytes done, bytes in all) or the
/// error that stopped it, and where they would run.
#[derive(Serialize)]
struct ChatStatus {
    models: Vec<ChatModel>,
    downloading: Option<(String, u64, u64)>,
    error: Option<String>,
    runs: String,
    /// The model to preselect when Ask has none: a server's on the graphics card, else built in.
    preselected: String,
    /// Every chat model Ask can take, by place: the server's and the built-in ones.
    groups: Vec<coxswain_core::setup::AskGroup>,
    /// The same for the model for code questions, and why one is recommended here or none.
    code_groups: Vec<coxswain_core::setup::AskGroup>,
    code_why: String,
    /// Whether Ask's context fits the server's model: the line and the value **Use …** sets.
    context: coxswain_core::setup::ContextAdvice,
}

/// The probes (memory, the model servers, the files) run on a blocking thread.
#[tauri::command]
async fn chat_status(ctx: tauri::State<'_, Ctx>) -> Res<ChatStatus> {
    use coxswain_core::chat;
    use std::sync::atomic::Ordering;
    static RAM: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let (downloading, error) = {
        let d = ctx.chat.lock().map_err(|e| e.to_string())?;
        (d.as_ref().filter(|(_, _, err)| err.is_none()).map(|(k, p, _)| (k.clone(), p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed))), d.as_ref().and_then(|(_, _, e)| e.clone()))
    };
    let cfg = ctx.cfg().search.clone();
    blocking(move || {
        // The first time on a processor: the probe, under a second, then kept.
        let cpu_only = cfg.meaning_device == "cpu";
        let ram = *RAM.get_or_init(coxswain_core::setup::ram_gb);
        let suggested = chat::suggest(ram, cpu_only);
        let look = coxswain_core::setup::look(false);
        let models = chat::fitting(ram)
            .map(|m| ChatModel { key: m.key(), name: m.name, size: m.size(), ram: m.ram, installed: m.installed(), suggested: suggested.is_some_and(|s| std::ptr::eq(m, s)), estimate: chat::estimate(m, cpu_only, true).map(|e| e.text()) })
            .collect();
        Ok(ChatStatus {
            models,
            downloading,
            error,
            runs: coxswain_core::setup::builtin_runs(&cfg, None),
            preselected: coxswain_core::setup::recommend_ask(&cfg, look.as_deref()),
            groups: coxswain_core::setup::ask_choices(&cfg, look.as_deref()),
            code_groups: coxswain_core::setup::code_choices(&cfg, look.as_deref()),
            code_why: coxswain_core::setup::code_advice(&cfg, look.as_deref()).why,
            context: coxswain_core::setup::context_advice(&cfg, look.as_deref()),
        })
    })
    .await
}

/// The built-in models on the disk, for Settings → Finding files → Built-in models.
#[tauri::command]
async fn models_list(ctx: tauri::State<'_, Ctx>) -> Res<Vec<coxswain_core::models::Entry>> {
    let (index, cfg) = (ctx.index.clone(), ctx.cfg().search.clone());
    blocking(move || Ok(coxswain_core::models::list(&cfg, &index.loaded()))).await
}

/// Settings → Privacy and updates → Disk use: every place Coxswain keeps things; with `images`
/// also the container images (which asks podman or docker).
#[tauri::command]
async fn disk_list(images: bool, ctx: tauri::State<'_, Ctx>) -> Res<Vec<coxswain_core::disk::Item>> {
    let (index, cfg) = (ctx.index.clone(), ctx.cfg().clone());
    blocking(move || Ok(coxswain_core::disk::items(&cfg, &index.loaded(), images))).await
}

/// Clear the row `id` of Disk use (a model goes through `models_action`), or with `id` empty
/// everything that can be built again, the models not in use and what is outside as ticked.
/// What was done, in words.
#[tauri::command]
async fn disk_clear(id: String, models: bool, outside: bool, ctx: tauri::State<'_, Ctx>) -> Res<String> {
    use coxswain_core::disk;
    let (index, cfg) = (ctx.index.clone(), ctx.cfg().clone());
    blocking(move || {
        let items = disk::items(&cfg, &index.loaded(), outside || id.starts_with("image:"));
        if id.is_empty() {
            let (freed, failed) = disk::clear_all(&items, models, outside, &cfg, &index);
            return if failed.is_empty() { Ok(coxswain_core::t!("disk.cleared_all", "size" => coxswain_core::settings::human(freed))) } else { Err(failed.join(" · ")) };
        }
        let i = items.iter().find(|i| i.id == id).ok_or_else(|| coxswain_core::t!("models.no_such", "name" => id.as_str()))?;
        disk::clear(i, &cfg, &index).map(|n| disk::cleared(&i.name, n))
    })
    .await
}

/// The preview of Coxswain's own `index.bin` or `search.db`, here or a copy; `None` for any
/// other file.
#[tauri::command]
async fn own_store(path: PathBuf) -> Res<Option<coxswain_core::disk::Report>> {
    blocking(move || Ok(coxswain_core::disk::describe(&path))).await
}

/// "unload" the built-in chat model; "delete" the model `id` (the one in use gives way to the
/// recommended choice, saved: the page hears `config-changed`); "unused": delete all not in
/// use. What was done, in words.
#[tauri::command]
async fn models_action(what: String, id: String, app: tauri::AppHandle, ctx: tauri::State<'_, Ctx>) -> Res<String> {
    use coxswain_core::models;
    let (index, cfg) = (ctx.index.clone(), ctx.cfg().search.clone());
    let all = blocking({
        let (index, cfg) = (index.clone(), cfg.clone());
        move || Ok(models::list(&cfg, &index.loaded()))
    })
    .await?;
    let human = coxswain_core::settings::human;
    match what.as_str() {
        "unload" => {
            index.unload();
            Ok(coxswain_core::t!("models.unloaded", "model" => all.iter().find(|e| e.id == id).map_or(id.as_str(), |e| e.name.as_str())))
        }
        "unused" => {
            let gone: Vec<models::Entry> = all.into_iter().filter(|e| !e.in_use).collect();
            blocking(move || {
                for e in &gone {
                    models::delete(e, &index).map_err(|err| format!("{}: {err}", e.folder.display()))?;
                }
                Ok(coxswain_core::t!("models.deleted_all", "n" => gone.len(), "size" => human(gone.iter().map(|e| e.bytes).sum())))
            })
            .await
        }
        _ => {
            let e = all.into_iter().find(|e| e.id == id).ok_or_else(|| coxswain_core::t!("models.no_such", "name" => id))?;
            let instead = blocking({
                let (cfg, e) = (cfg.clone(), e.clone());
                move || Ok(models::instead(&cfg, &e, coxswain_core::setup::look(true).as_deref()))
            })
            .await?;
            if let Some((changes, _)) = &instead {
                save_settings(changes.clone(), ctx.clone())?;
                let _ = app.emit("config-changed", ());
            }
            let said = blocking({
                let e = e.clone();
                move || models::delete(&e, &index).map_err(|err| format!("{}: {err}", e.folder.display()))
            })
            .await
            .map(|()| coxswain_core::t!("models.deleted", "model" => e.name.as_str(), "size" => human(e.bytes)))?;
            Ok(instead.map_or(said.clone(), |(_, then)| format!("{said} {then}")))
        }
    }
}

/// "download" the built-in chat model `model`, then make it Ask's (the page hears
/// `config-changed`); "cancel" the download; "remove" it, and turn Ask off if it was Ask's.
#[tauri::command]
async fn chat_action(what: String, model: String, app: tauri::AppHandle, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    use std::sync::atomic::Ordering;
    let m = coxswain_core::chat::of(&model).ok_or_else(|| format!("no built-in model {model}"))?;
    let use_it = move |app: &tauri::AppHandle, value: &str| -> Res<()> {
        let cfg = coxswain_core::settings::save(serde_json::json!({ "ask_model": value }).as_object().expect("an object"))?;
        *app.state::<Ctx>().cfg.write().map_err(|e| e.to_string())? = cfg;
        let _ = app.emit("config-changed", ());
        Ok(())
    };
    match what.as_str() {
        "download" => {
            let p = Arc::new(coxswain_core::meaning::Progress::default());
            *ctx.chat.lock().map_err(|e| e.to_string())? = Some((model.clone(), p.clone(), None));
            std::thread::spawn(move || {
                let done = m.download(&p).map_err(|e| e.to_string()).and_then(|()| use_it(&app, &model));
                if let Ok(mut d) = app.state::<Ctx>().chat.lock() {
                    *d = match done {
                        Err(e) if !p.cancel.load(Ordering::Relaxed) => Some((model, p, Some(e))),
                        _ => None,
                    };
                }
            });
        }
        "cancel" => {
            if let Some((_, p, _)) = ctx.chat.lock().map_err(|e| e.to_string())?.as_ref() {
                p.cancel.store(true, Ordering::Relaxed);
            }
        }
        _ => {
            if ctx.cfg().search.ask_model == model {
                use_it(&app, "")?;
            }
            let index = ctx.index.clone();
            blocking(move || m.remove(&index).map_err(|e| e.to_string())).await?;
        }
    }
    Ok(())
}

/// The models a server offers, for Settings: Ollama's pulled ones, or an OpenAI server's list;
/// with `chat`, only those that can answer (for Ask). An error when it does not answer.
#[tauri::command]
async fn meaning_models(engine: String, url: String, chat: Option<bool>, ctx: tauri::State<'_, Ctx>) -> Res<Vec<String>> {
    use coxswain_core::meaning;
    // The key goes only to the server it is saved for.
    let search = ctx.cfg().search.clone();
    let key = meaning::key_of(&search).filter(|_| url.trim_end_matches('/') == search.meaning_url.trim_end_matches('/'));
    let list = if chat == Some(true) { meaning::chat_models } else { meaning::server_models };
    tauri::async_runtime::spawn_blocking(move || list(engine == "openai", &url, key.as_deref())).await.map_err(|e| e.to_string())?
}

/// What changing the vectors' engine or model to `value` costs, said before it is saved: the
/// files whose meaning is read again and about how long that takes, or None when the vectors stay.
#[tauri::command]
async fn meaning_change(name: String, value: String, engine: Option<String>, url: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<Option<String>> {
    let (old, index) = (ctx.cfg().search.clone(), ctx.index.clone());
    let mut new = old.clone();
    // The setup changes the server along with the model.
    if let (Some(engine), Some(url)) = (engine, url) {
        (new.meaning_engine, new.meaning_url) = (engine, url);
    }
    match name.as_str() {
        "meaning_engine" => new.meaning_engine = value,
        "meaning_model" => new.meaning_model = value,
        _ => return Ok(None),
    }
    tauri::async_runtime::spawn_blocking(move || { let st = index.status(); coxswain_core::meaning::change_notice(&old, &new, st.meaning_done, st.meaning_passages) }).await.map_err(|e| e.to_string())
}

/// What changing what is kept of long files (`long_files`, `long_files_everything`,
/// `long_files_sample`) as `changes` say costs, said before it is saved; None when it stays.
#[tauri::command]
async fn long_files_change(changes: serde_json::Map<String, serde_json::Value>, ctx: tauri::State<'_, Ctx>) -> Res<Option<String>> {
    let (cfg, index) = (ctx.cfg().clone(), ctx.index.clone());
    let text = coxswain_core::config::Config::path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let new = coxswain_core::settings::apply(&text, &changes).and_then(|t| coxswain_core::config::Config::parse_saved(&text, &t))?;
    tauri::async_runtime::spawn_blocking(move || {
        let measured = index.status().meaning_ms_per_passage;
        coxswain_core::long_files::change_notice(&cfg.search, &new.search, measured, |r| index.long_files_cost(r))
    })
    .await
    .map_err(|e| e.to_string())
}

/// *Long files: sample or everything…* on the folder `dir`: its choices, each with what it
/// would cost and the settings it writes (`save_settings`).
#[tauri::command]
async fn long_files_choices(dir: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<Vec<coxswain_core::long_files::Choice>> {
    let (cfg, index) = (ctx.cfg().search.clone(), ctx.index.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let measured = index.status().meaning_ms_per_passage;
        coxswain_core::long_files::choices(&cfg, &dir, measured, |r| index.long_files_cost(r))
    })
    .await
    .map_err(|e| e.to_string())
}

/// Why Ask's chat model cannot answer (one that only makes vectors, one the server does not
/// have), or None. `try_it` sends an OpenAI server a one-word question, when it is saved.
#[tauri::command]
async fn ask_check(try_it: bool, ctx: tauri::State<'_, Ctx>) -> Res<Option<String>> {
    let cfg = ctx.cfg().search.clone();
    tauri::async_runtime::spawn_blocking(move || coxswain_core::meaning::chat_problem(&cfg, try_it)).await.map_err(|e| e.to_string())
}

/// Ask Ollama to pull `model`; the progress is the download's, in `meaning_status`.
#[tauri::command]
fn meaning_pull(model: String, url: String, kind: Option<coxswain_core::setup::Kind>, app: tauri::AppHandle, ctx: tauri::State<Ctx>) -> Res<()> {
    use coxswain_core::setup;
    let p = Arc::new(coxswain_core::meaning::Progress::default());
    *ctx.meaning.lock().map_err(|e| e.to_string())? = Some((p.clone(), None));
    let index = ctx.index.clone();
    // Ollama unless the setup names another server (Lemonade pulls too).
    let server = setup::Found { kind: kind.unwrap_or(setup::Kind::Ollama), url, engine: String::new(), models: vec![] };
    std::thread::spawn(move || {
        let done = setup::pull(&server, &model, &p);
        if let Ok(mut m) = app.state::<Ctx>().meaning.lock() {
            *m = match done {
                Err(e) if !p.cancel.load(std::sync::atomic::Ordering::Relaxed) => Some((p, Some(e))),
                _ => None,
            };
        }
        index.restart();
    });
    Ok(())
}

/// What the guided setup of search by meaning shows first: the model servers on this machine
/// and what their models can do, the machine, and what suits it.
#[derive(Serialize)]
struct SetupLook {
    found: Vec<FoundView>,
    machine: coxswain_core::setup::Machine,
    advice: coxswain_core::setup::Advice,
    /// The index in `found` of the server recommended.
    best: Option<usize>,
    /// Where the built-in model runs: "on the GPU (Metal)", "on the CPU".
    builtin_runs: String,
}

#[tauri::command]
async fn setup_probe(ctx: tauri::State<'_, Ctx>) -> Res<SetupLook> {
    use coxswain_core::setup;
    let (index, cfg) = (ctx.index.clone(), ctx.cfg().search.clone());
    blocking(move || {
        let found = setup::probe();
        let machine = setup::machine(&found);
        let advice = setup::advise(&machine, &found);
        let best = setup::best(&found, &advice).and_then(|b| found.iter().position(|f| f.url == b.url));
        let runs = index.status().meaning_runs.filter(|_| cfg.meaning && cfg.meaning_engine == "builtin");
        let found = found.into_iter().map(FoundView::from).collect();
        Ok(SetupLook { found, machine, advice, best, builtin_runs: setup::builtin_runs(&cfg, runs.as_ref()) })
    })
    .await
}

/// A server on another machine, named in the setup: what answers there.
#[tauri::command]
async fn setup_probe_url(url: String, ctx: tauri::State<'_, Ctx>) -> Res<Option<FoundView>> {
    let key = coxswain_core::meaning::key_of(&ctx.cfg().search);
    blocking(move || Ok(coxswain_core::setup::probe_url(&url, key.as_deref()).map(FoundView::from))).await
}

/// A server that answered, with its host and whether it is this machine, as Settings says them.
#[derive(Serialize)]
struct FoundView {
    #[serde(flatten)]
    found: coxswain_core::setup::Found,
    host: String,
    local: bool,
}

impl From<coxswain_core::setup::Found> for FoundView {
    fn from(found: coxswain_core::setup::Found) -> Self {
        let (host, local) = (coxswain_core::settings::host(&found.url).to_string(), coxswain_core::settings::is_local(&found.url));
        FoundView { found, host, local }
    }
}

/// The test question to Ask's chat model: milliseconds to its first word.
#[tauri::command]
async fn setup_try(ctx: tauri::State<'_, Ctx>) -> Res<u64> {
    let cfg = ctx.cfg().search.clone();
    blocking(move || coxswain_core::setup::try_ask(&cfg).map(|d| d.as_millis() as u64)).await
}

/// The speed test of search by meaning: its report.
#[tauri::command]
async fn meaning_bench(ctx: tauri::State<'_, Ctx>) -> Res<String> {
    let (index, cfg) = (ctx.index.clone(), ctx.cfg().search.clone());
    blocking(move || coxswain_core::meaning::bench_report(&cfg, Some(&index.status()))).await
}

/// Whether `server` runs the chat model on the processor while a graphics card is there.
#[tauri::command]
async fn setup_speed(server: coxswain_core::setup::Found, ctx: tauri::State<'_, Ctx>) -> Res<Option<String>> {
    let model = ctx.cfg().search.ask_model.clone();
    blocking(move || {
        let machine = coxswain_core::setup::machine(std::slice::from_ref(&server));
        Ok(coxswain_core::setup::speed_problem(&server, &machine, &model))
    })
    .await
}

/// "download": fetch the model, then turn search by meaning on with it, and search inside files
/// (the page hears `config-changed`); "cancel" the download;
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
                    let on = serde_json::json!({ "meaning_engine": "builtin", "search_meaning": true, "search_text": true });
                    let cfg = coxswain_core::settings::save(on.as_object().expect("an object"))?;
                    *ctx.cfg.write().map_err(|e| e.to_string())? = cfg;
                    let _ = app.emit("config-changed", ());
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
        "pause" => index.transcribe_pause(true, false),
        "resume" => index.transcribe_pause(false, false),
        "stop_transcribing" => index.transcribe_pause(false, true),
        _ => index.restart(),
    })
    .await
    .map_err(|e| e.to_string())
}

/// *Transcribe speech*'s question: the recordings among `paths` (folders gone through), and
/// what to ask; or, with transcribing off, why.
#[derive(Serialize)]
struct TranscribePlan {
    files: Vec<PathBuf>,
    text: String,
    off: bool,
}

#[tauri::command]
async fn transcribe_plan(paths: Vec<PathBuf>, ctx: tauri::State<'_, Ctx>) -> Res<TranscribePlan> {
    let (cfg, index) = (ctx.cfg().clone(), ctx.index.clone());
    if !cfg.search.whisper || !cfg.search.text {
        return Ok(TranscribePlan { files: vec![], text: coxswain_core::t!("whisper.off"), off: true });
    }
    blocking(move || {
        let survey = coxswain_core::transcribe::survey(&paths);
        let rate = index.status().transcripts.and_then(|t| t.rate);
        let text = if survey.files.is_empty() { coxswain_core::t!("whisper.none") } else { coxswain_core::transcribe::plan_text(&cfg, &survey, rate) };
        Ok(TranscribePlan { files: survey.files, text, off: false })
    })
    .await
}

/// Hand `files` to the helper to transcribe: how many it took.
#[tauri::command]
async fn transcribe(files: Vec<PathBuf>, ctx: tauri::State<'_, Ctx>) -> Res<usize> {
    let index = ctx.index.clone();
    blocking(move || index.transcribe(&files).ok_or_else(|| coxswain_core::t!("whisper.no_helper"))).await
}

/// The transcript of a recording, done or under way, for the preview.
#[tauri::command]
async fn transcript(path: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<Option<coxswain_core::transcribe::Transcript>> {
    let index = ctx.index.clone();
    blocking(move || Ok(index.transcript(&path))).await
}

/// The Whisper models to choose from, each with its size and whether it suits this machine.
#[tauri::command]
async fn whisper_models(ctx: tauri::State<'_, Ctx>) -> Res<Vec<(String, String)>> {
    let cfg = ctx.cfg().clone();
    blocking(move || Ok(coxswain_core::transcribe::choices(&cfg))).await
}

/// What Find shows: its rows, where the cursor starts, what can be searched (the footer), and
/// why Ask cannot be asked yet.
#[derive(Serialize)]
struct FindOut {
    /// `find::Row`s; a row that says what is missing has its `text`, its `step` and the
    /// `dismiss` id that sends it away for good.
    rows: Vec<serde_json::Value>,
    start: usize,
    footer: String,
    ask: Option<coxswain_core::find::Off>,
    /// The names are still being counted: Find asks again.
    building: bool,
}

/// The GUI lists every hit it gets as a row, so it takes fewer than the TUI, which only draws
/// what fits on screen. A group's count still counts them all.
const GUI_MAX_HITS: usize = 500;

/// Find: `query` under the kind `chip`, below `scope` when given. `ask_problem`: why the chat
/// model cannot answer, as `ask_check` said.
#[tauri::command]
async fn find(query: String, scope: Option<PathBuf>, chip: coxswain_core::find::Kind, ask_problem: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<FindOut> {
    use coxswain_core::find::{self, Kind, Off};
    // A CPU-bound scan (rayon, all cores), so off the async runtime's worker threads.
    let (index, cfg) = (ctx.index.clone(), ctx.cfg().search.clone());
    let dismissed = ctx.state.lock().map_err(|e| e.to_string())?.notices_dismissed.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let kind = find::parse(&query).0.unwrap_or(chip);
        let rows = if kind == Kind::All { find::ALL_ROWS } else { cfg.max_results.min(GUI_MAX_HITS) };
        let found = index.find(&query, scope.as_deref(), chip, rows);
        let ask = find::ask_ready(&cfg).and_then(|_| ask_problem.map_or(Ok(()), |p| Err(Off::AskModel(p))));
        let slow = coxswain_core::setup::poor(&cfg, false).into_iter().find(|p| p.ask);
        let rows = find::rows(&query, chip, &found, ask.clone(), slow.as_ref(), |id| dismissed.iter().any(|d| d == id));
        let status = index.status();
        let start = find::start(&query, &rows);
        let rows = rows
            .iter()
            .map(|r| {
                let mut v = serde_json::to_value(r).unwrap_or_default();
                if let find::Row::Ask { off: Some(off) } | find::Row::Off { off, .. } = r {
                    v["text"] = off.text().into();
                    v["step"] = off.step().into();
                    v["dismiss"] = off.dismiss_id().into();
                }
                v
            })
            .collect();
        FindOut { start, rows, footer: find::footer(&status), ask: ask.err(), building: status.state == State::Building }
    })
    .await
    .map_err(|e| e.to_string())
}

/// "Read this folder too": `dir` added to the folders whose text is read; the helper starts again.
#[tauri::command]
async fn find_read_too(dir: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<UiConfig> {
    let roots = coxswain_core::find::read_too(&ctx.cfg().search, &dir);
    let mut changes = serde_json::Map::new();
    changes.insert("text_roots".into(), serde_json::to_value(roots).map_err(|e| e.to_string())?);
    let cfg = save_settings(changes, ctx.clone())?;
    let index = ctx.index.clone();
    tauri::async_runtime::spawn_blocking(move || index.restart()).await.map_err(|e| e.to_string())?;
    Ok(cfg)
}

/// What an Ask sends while it runs: first the numbered sources, then the answer piece by piece.
#[derive(Clone, Serialize)]
#[serde(tag = "k", rename_all = "snake_case")]
enum AskEvent {
    Sources { paths: Vec<PathBuf>, similar: Vec<usize>, read: String },
    Piece { text: String },
}

/// Ask: `question` answered by the user's chat model from the closest excerpts (an overview of
/// the folder too, `here` without a scope, for a question about it as a whole), with the
/// questions and answers `earlier` in this Find file. Nothing is kept.
#[tauri::command]
async fn ask(question: String, earlier: Vec<(String, String)>, scope: Option<PathBuf>, here: PathBuf, on_event: tauri::ipc::Channel<AskEvent>, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    use std::sync::atomic::Ordering;
    let (index, cfg, asking) = (ctx.index.clone(), ctx.cfg().search.clone(), ctx.asking.clone());
    let me = asking.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        // The chat model loads while the sources are looked up.
        coxswain_core::meaning::warm(&cfg);
        let (sources, similar) = coxswain_core::ask::sources(|q, s, b| index.passages(q, s, b), &cfg, &earlier, &question, scope.as_deref(), &here)?;
        let _ = on_event.send(AskEvent::Sources { paths: sources.iter().map(|(p, _)| p.clone()).collect(), similar, read: coxswain_core::ask::read(&sources) });
        // Stopped while searching: the model is not asked (a request it would work on alone).
        if asking.load(Ordering::SeqCst) != me {
            return Ok(());
        }
        index.answer(&cfg, &earlier, &question, &sources, |text| asking.load(Ordering::SeqCst) == me && (text.is_empty() || on_event.send(AskEvent::Piece { text: text.to_string() }).is_ok()))
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
    let p = bfs::resolve(base, s);
    std::fs::canonicalize(&p).unwrap_or(p)
}

#[tauri::command(async)]
fn resolve_path(base: PathBuf, input: String) -> PathBuf {
    resolve(&base, &input)
}

/// How far the file operation under way is, for its progress bar: the item it is on, of how
/// many, its name, how many wait their turn behind it, and whether Stop was pressed.
#[derive(Clone, Default, serde::Serialize)]
struct JobProgress {
    at: usize,
    all: usize,
    name: String,
    waiting: usize,
    stopping: bool,
    #[serde(skip)]
    stop: Arc<std::sync::atomic::AtomicBool>,
}

static JOB: Mutex<Option<JobProgress>> = Mutex::new(None);
static WAITING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// `op` on `paths` (to `dst`), as both apps run it, on a blocking thread: a big copy must not
/// hold one of the async runtime's few workers. One at a time: one asked for while another runs
/// waits its turn. The folders it changes are forgotten while it runs and measured again after,
/// what can be undone goes into the history, and failures come back as one message. Stopped
/// (`job_stop`), the line that says how far it got; one that waited is not started.
async fn job(ctx: &Ctx, op: Op, paths: Vec<PathBuf>, dst: PathBuf, password: Option<String>) -> Res<Option<String>> {
    use std::sync::atomic::Ordering;
    let stop = ctx.job_stop.lock().map_err(|e| e.to_string())?.clone();
    WAITING.fetch_add(1, Ordering::SeqCst);
    let turn = ctx.job_turn.lock().await;
    WAITING.fetch_sub(1, Ordering::SeqCst);
    if stop.load(Ordering::SeqCst) {
        return Ok(Some(coxswain_core::undo::stopped_line(op, 0, paths.len())));
    }
    let touched = op.touches(&paths, &dst);
    touched.iter().for_each(|p| ctx.sizer.forget(p));
    let names: Vec<String> = paths.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
    *JOB.lock().map_err(|e| e.to_string())? = Some(JobProgress { all: paths.len(), name: names.first().cloned().unwrap_or_default(), stop: stop.clone(), ..Default::default() });
    let ran = tauri::async_runtime::spawn_blocking(move || {
        coxswain_core::undo::run(op, &paths, &dst, password.as_deref(), &stop, |i| {
            if let Some(p) = JOB.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                (p.at, p.name) = (i, names.get(i).cloned().unwrap_or_default());
            }
        })
    })
    .await;
    *JOB.lock().unwrap_or_else(|e| e.into_inner()) = None;
    drop(turn);
    let ran = ran.map_err(|e| e.to_string())?;
    ctx.changed(&touched);
    ctx.remember(ran.rec);
    // One line for each item that failed: the page counts them for the title. Stopped as well,
    // the failures are said; stopped alone, how far it got.
    let errors: Vec<String> = ran.failed.iter().map(|(p, e)| format!("{}: {}", p.display(), e.to_string().replace('\n', " "))).collect();
    if errors.is_empty() { Ok(ran.stopped) } else { Err(errors.join("\n")) }
}

/// The file operation under way, for its progress bar; `None` when none runs.
#[tauri::command]
fn job_progress() -> Option<JobProgress> {
    let mut p = JOB.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    p.waiting = WAITING.load(std::sync::atomic::Ordering::SeqCst);
    p.stopping = p.stop.load(std::sync::atomic::Ordering::SeqCst);
    Some(p)
}

/// Stop: the file operation under way stops (the file it is on removed, what is done kept),
/// and those waiting their turn are not started. Operations asked for after it run.
#[tauri::command]
fn job_stop(ctx: tauri::State<Ctx>) -> Res<()> {
    let mut stop = ctx.job_stop.lock().map_err(|e| e.to_string())?;
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    *stop = Arc::default();
    Ok(())
}

/// Run `op` on every source, as an operation that can be undone (on a blocking thread, like
/// `job`): what can be of it goes into the history; failures come back as one message.
async fn each_undo(ctx: &Ctx, kind: UndoKind, paths: Vec<PathBuf>, op: impl Fn(&mut Record, &Path) -> std::io::Result<()> + Send + 'static) -> Res<()> {
    let (rec, r) = tauri::async_runtime::spawn_blocking(move || {
        let mut rec = Record::new(kind);
        let errors: Vec<String> = paths.iter().filter_map(|p| op(&mut rec, p).err().map(|e| format!("{}: {e}", p.display()))).collect();
        (rec, if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) })
    })
    .await
    .map_err(|e| e.to_string())?;
    ctx.remember(rec);
    r
}

/// `f` on a blocking thread, its answer back here.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn copy(paths: Vec<PathBuf>, base: PathBuf, dest: String, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<Option<String>> {
    // The password of a locked archive, held for this copy only.
    job(&ctx, Op::Copy, paths, resolve(&base, &dest), password).await
}

#[tauri::command]
async fn rename(paths: Vec<PathBuf>, base: PathBuf, dest: String, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<Option<String>> {
    job(&ctx, Op::Move, paths, resolve(&base, &dest), password).await
}

/// How deleting `paths` goes (as in the terminal app): for good inside an archive or where the
/// trash cannot take them, and then asked even when deleting is not confirmed.
#[tauri::command(async)]
fn delete_plan(paths: Vec<PathBuf>, forever: bool, inside: bool) -> bfs::DeletePlan {
    bfs::delete_plan(&paths, forever, inside)
}

/// To the trash, or gone for good with `forever` (as `delete_plan` says). Inside a locked 7z,
/// `password` opens it.
#[tauri::command]
async fn delete(paths: Vec<PathBuf>, forever: bool, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<Option<String>> {
    job(&ctx, if forever { Op::Forever } else { Op::Trash }, paths, PathBuf::new(), password).await
}

/// Async, as inside an archive the archive is written anew.
#[tauri::command]
async fn mkdir(base: PathBuf, name: String, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<PathBuf> {
    let d = resolve(&base, &name);
    job(&ctx, Op::Mkdir, vec![d.clone()], PathBuf::new(), password).await?;
    Ok(d)
}

/// Bytes and file count of a folder when it is cheap (measured lately, or in the store), never
/// walked: for "..". The store's answer is a round trip to the helper, so off the main thread.
#[tauri::command]
async fn dir_size_known(path: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<Option<(u64, u64)>> {
    let sizer = ctx.sizer.clone();
    tauri::async_runtime::spawn_blocking(move || sizer.known_only(&path)).await.map_err(|e| e.to_string())
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

/// A planned rename, with both names as the list shows them (a newline as `␊`).
#[derive(Serialize)]
struct PlannedShown {
    #[serde(flatten)]
    plan: Planned,
    from_shown: String,
    to_shown: String,
}

#[tauri::command(async)]
fn rename_plan(dir: PathBuf, selected: Vec<String>, pattern: String, replacement: String, flags: Flags) -> Res<Vec<PlannedShown>> {
    let existing: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .collect();
    let plan = rename::plan(&selected, &existing, &pattern, &replacement, flags)?;
    Ok(plan.into_iter().map(|p| PlannedShown { from_shown: rename::shown(&p.from), to_shown: rename::shown(&p.to), plan: p }).collect())
}

/// A name as the Rename field shows it: a control character as its picture.
#[tauri::command]
fn rename_shown(name: String) -> String {
    rename::shown(&name)
}

/// The new name typed over `name` in the Rename field, or `None` when the text is as shown.
#[tauri::command]
fn rename_typed(name: String, typed: String) -> Option<String> {
    rename::typed(&name, &typed)
}

#[tauri::command(async)]
fn rename_apply(dir: PathBuf, plan: Vec<Planned>, ctx: tauri::State<Ctx>) -> Res<()> {
    rename::apply(&dir, &plan)?;
    let mut rec = Record::new(UndoKind::Rename);
    plan.iter().filter(|p| p.from != p.to).for_each(|p| rec.done(&dir.join(&p.from), &dir.join(&p.to)));
    ctx.remember(rec);
    Ok(())
}

/// Ctrl+Z: undo the newest operation. What it was, a line for each item left as it is, and
/// what is back where it was (the cursor goes there).
#[tauri::command]
async fn undo(ctx: tauri::State<'_, Ctx>) -> Res<coxswain_core::undo::Undone> {
    let rec = ctx.undo.lock().map_err(|e| e.to_string())?.pop().ok_or_else(|| coxswain_core::t!("undo.nothing"))?;
    // In turn with the file operations: never under a copy or a move that runs.
    let _turn = ctx.job_turn.lock().await;
    let u = blocking(move || Ok(rec.undo(|_| {}))).await?;
    ctx.changed(&u.touched);
    Ok(u)
}

/// What Ctrl+Z would undo, and how many operations were kept so far (a new one: the page says
/// how to undo it).
#[tauri::command]
fn undo_next(ctx: tauri::State<Ctx>) -> (u64, Option<String>) {
    ctx.undo.lock().map(|h| (h.pushed, h.next().map(Record::label))).unwrap_or_default()
}

/// The opener is watched for a moment, so "no application" comes back as an error.
#[tauri::command]
async fn open_path(path: PathBuf) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || bfs::open_default(&path).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

/// `editor` from the config, else the desktop default. An editor that is not there fails at
/// once, and the shell's word for it is the error.
#[tauri::command]
async fn edit_path(path: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let Some(ed) = ctx.cfg().editor.clone() else { return open_path(path).await };
    let mut line = coxswain_core::tools::Line::default();
    line.cmd = format!("{ed} {}", line.word(&path.to_string_lossy()));
    tauri::async_runtime::spawn_blocking(move || coxswain_core::tools::spawn_watched(line.command(), std::time::Duration::from_secs(1)).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

/// The built-in editor (Shift+F4): `path` read for editing (`edit::open`).
#[tauri::command]
async fn edit_open(path: PathBuf) -> Res<coxswain_core::edit::Doc> {
    crate::here(&path)?;
    blocking(move || coxswain_core::edit::open(&path)).await
}

/// The built-in editor's text saved as the file was read; `"changed"` when the file changed on
/// the disk since `stamp` (none: saved over it whatever). The new stamp.
#[tauri::command]
async fn edit_save(path: PathBuf, text: String, encoding: coxswain_core::edit::Encoding, crlf: bool, stamp: Option<coxswain_core::edit::Stamp>) -> Res<coxswain_core::edit::Stamp> {
    use coxswain_core::edit::SaveError;
    blocking(move || {
        coxswain_core::edit::save(&path, &text, encoding, crlf, stamp.as_ref()).map_err(|e| match e {
            SaveError::Changed => "changed".to_string(),
            e => e.to_string(),
        })
    })
    .await
}

/// Lines of a hex dump, 16 bytes each: 64 KB. The UI says as much.
const HEX_LINES: usize = 4096;

/// Up to `max` bytes as text for the preview; binary files come back hex-dumped. On a
/// blocking thread: a slow share must not hold one of the runtime's workers.
#[tauri::command]
async fn read_text(path: PathBuf, max: usize) -> Res<(String, bool, bool)> {
    crate::here(&path)?;
    blocking(move || read_text_of(&path, max)).await
}

fn read_text_of(path: &Path, max: usize) -> Res<(String, bool, bool)> {
    let mut buf = vec![];
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    f.take(max as u64).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let truncated = len > buf.len() as u64;
    // UTF-8, UTF-16 by its mark, else Latin-1; no text: a hex dump of the first 64 KB, four
    // thousand lines are plenty to see what a file is.
    match coxswain_core::view::decode(&buf) {
        Some(text) => Ok((text, truncated, false)),
        None => Ok((coxswain_core::view::hex(&buf, HEX_LINES).join("\n"), len > (HEX_LINES * 16) as u64, true)),
    }
}

/// The grammar a file's text is highlighted with (`code::language`): by its name, then what
/// `head`, the start of its text, says.
#[tauri::command]
fn code_language(path: String, head: String) -> Option<&'static str> {
    coxswain_core::code::language(&path, &head)
}

// ---------------------------------------------------------------- archives

#[derive(Serialize)]
struct ArchiveListing {
    entries: Vec<coxswain_core::archive::ArchiveEntry>,
    more: bool,
    /// It is a container image, read-only (`archive::is_image`).
    image: bool,
}

/// A preview reads only what is on the disk, or a file the user asked to download: never a file
/// only in the cloud by itself, whatever the page asks.
pub(crate) fn here(path: &Path) -> Res<()> {
    if coxswain_core::cloud::unasked(path) { Err(format!("{}: {}", path.display(), coxswain_core::t!("cloud.online_only"))) } else { Ok(()) }
}

/// The user pressed *Download and preview*: the previews may read (and download) this file.
#[tauri::command]
fn cloud_fetch(path: PathBuf) {
    coxswain_core::cloud::ask_for(&path);
}

#[tauri::command]
async fn archive_list(path: PathBuf) -> Res<ArchiveListing> {
    crate::here(&path)?;
    let (entries, more, image) = blocking(move || {
        let (entries, more) = coxswain_core::archive::list(&path, 2000).map_err(|e| e.to_string())?;
        Ok::<_, String>((entries, more, coxswain_core::archive::is_image(&path)))
    })
    .await?;
    Ok(ArchiveListing { entries, more, image })
}

#[tauri::command]
async fn extract(paths: Vec<PathBuf>, base: PathBuf, dest: String, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<Option<String>> {
    job(&ctx, Op::Extract, paths, resolve(&base, &dest), password).await
}

/// A disk image's facts for the preview, as lines, what can be done with it next, and whether
/// it opens like a folder now. `None`: no disk image (VHDL source named `.vhd`, any `.img`).
#[derive(Serialize)]
struct DiskFacts {
    lines: Vec<(String, String)>,
    next: coxswain_core::disk_image::Next,
    opens: bool,
    /// Its header was read as a format known here (an `.iso` may be one 7-Zip still reads).
    known: bool,
}

#[tauri::command]
async fn disk_image_facts(path: PathBuf) -> Res<Option<DiskFacts>> {
    crate::here(&path)?;
    blocking(move || {
        let facts = coxswain_core::disk_image::facts(&path);
        // A .vhd or .img that is none is no disk image (VHDL source); an .iso, .chm … still is.
        if facts.is_none() && !coxswain_core::disk_image::sure(&path) {
            return Ok(None);
        }
        Ok(Some(DiskFacts {
            known: facts.is_some(),
            lines: facts.as_ref().map(coxswain_core::disk_image::lines).unwrap_or_default(),
            next: coxswain_core::disk_image::next(),
            opens: coxswain_core::archive::opens(&path),
        }))
    })
    .await
}

/// Whether the file at `path` opens like a folder: an archive, or a disk image while that is on.
#[tauri::command(async)]
fn opens_as_folder(path: PathBuf) -> bool {
    coxswain_core::archive::opens(&path)
}

/// The password of the locked archive `path` is in (or is), kept in memory for this run.
#[tauri::command(async)]
fn archive_password(path: PathBuf, password: String) {
    if let Some((archive, _)) = coxswain_core::archive::split(&path).or_else(|| coxswain_core::archive::is_archive(&path).then(|| (path.clone(), String::new()))) {
        coxswain_core::archive::remember(&archive, &password);
    }
}

/// A copy of a file inside an archive, to preview: see `archive::peek`.
#[tauri::command]
async fn archive_peek(path: PathBuf) -> Res<PathBuf> {
    crate::here(&path)?;
    // A file in a history: as it was at that commit.
    let peek = if history::is_history(&path) { history::peek } else { coxswain_core::archive::peek };
    tauri::async_runtime::spawn_blocking(move || peek(&path)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}

/// A new archive at `dest` (.zip, .7z, .tar, .tar.gz, ..., by its name) with `paths` in it,
/// locked with `password` if one is given (a 7z's names too with `hide_names`). The password
/// is kept in memory for this run only.
#[tauri::command]
async fn pack(paths: Vec<PathBuf>, base: PathBuf, dest: String, password: Option<String>, hide_names: bool, ctx: tauri::State<'_, Ctx>) -> Res<Option<String>> {
    let to = resolve(&base, &dest);
    let stopped = job(&ctx, Op::Pack { hide_names }, paths, to, password).await?;
    // The format is suggested next time, in both apps.
    ctx.edit_if(|st| ((), coxswain_core::archive::remember_format(st, &dest)))?;
    Ok(stopped)
}

/// What the pack dialog, or a copy into an archive, says when the disk may be short of room:
/// the archive (`dest`, named relative to `base`) is written beside itself first.
#[tauri::command]
async fn pack_room(paths: Vec<PathBuf>, base: PathBuf, dest: String) -> Option<String> {
    let to = resolve(&base, &dest);
    let archive = coxswain_core::archive::split(&to).map_or(to, |(a, _)| a);
    tauri::async_runtime::spawn_blocking(move || coxswain_core::archive::short_of_room(&archive, &paths)).await.ok().flatten()
}

// ---------------------------------------------------------------- properties

/// On a blocking thread: a folder's size is a walk of it, and the facts run `zfs` and `pkg`.
#[tauri::command]
async fn properties(path: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<bfs::Props> {
    let cfg = ctx.cfg().preview.clone();
    blocking(move || bfs::properties(&path).map(|p| coxswain_core::media::with_facts(p, &cfg, &path)).map_err(|e| format!("{}: {e}", path.display()))).await
}

/// The user flags named in `on` set on `path`, the others cleared (FreeBSD, macOS).
#[tauri::command(async)]
fn set_flags(path: PathBuf, on: Vec<String>) -> Res<()> {
    coxswain_core::flags::set_user(&path, &on.iter().map(String::as_str).collect::<Vec<_>>()).map_err(|e| e.to_string())
}

/// Unix permission bits when `mode` is given, else the read-only flag.
#[tauri::command(async)]
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

// Linux and BSD file managers exchange percent-encoded `file://` URIs; macOS and Windows take paths.
fn to_clip(p: &Path) -> String {
    if cfg!(any(windows, target_os = "macos")) {
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
    PathBuf::from(String::from_utf8_lossy(&coxswain_core::fs::percent_decode(rest.as_bytes())).into_owned())
}

fn os_clipboard() -> Option<clipboard_rs::ClipboardContext> {
    clipboard_rs::ClipboardContext::new().ok()
}

/// Put files on the system clipboard (other file managers can paste them); a cut is
/// remembered here, since there is no portable way to mark one.
#[tauri::command(async)]
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
    let (paths, cut) = {
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
        (paths, cut)
    };
    // ponytail: text copied elsewhere after a Coxswain copy still pastes Coxswain's files on
    // clipboards that report "no files" as empty; track the clipboard owner if that confuses.
    let (n, sizer) = (paths.len(), ctx.sizer.clone());
    each_undo(&ctx, if cut { UndoKind::Move } else { UndoKind::Copy }, paths, move |rec, p| {
        if cut && p.parent() == Some(dir.as_path()) {
            return Ok(()); // cut and pasted in place
        }
        let to = bfs::free_name(&dir, &p.file_name().unwrap_or_default().to_string_lossy());
        sizer.forget(&to);
        if cut {
            sizer.forget(p);
        }
        rec.run(p, &to, None)
    })
    .await?;
    Ok((n, cut))
}

// ---------------------------------------------------------------- drag out, watching

/// A native drag of `paths`, so they can be dropped on other applications (and back on
/// Coxswain): what is only inside an archive or a git history is copied out first
/// (`drag::out`). `image`: the drag image, a PNG the page drew at `scale`. Returns the paths
/// dragged, which a drop back on a pane is known by.
#[tauri::command]
async fn start_drag(paths: Vec<PathBuf>, image: Vec<u8>, scale: i32, window: tauri::Window) -> Res<Vec<PathBuf>> {
    let real = blocking(move || coxswain_core::drag::out(&paths).map_err(|e| e.to_string())).await?;
    let dragged = real.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    window
        .app_handle()
        .clone()
        .run_on_main_thread(move || {
            let _ = tx.send(native_drag(&window, dragged, image, scale));
        })
        .map_err(|e| e.to_string())?;
    let started = tauri::async_runtime::spawn_blocking(move || rx.recv().unwrap_or(false)).await.map_err(|e| e.to_string())?;
    // The button let go while the files were being copied out: they are ready for the next drag.
    if !started {
        return Err(coxswain_core::t!("drag.again"));
    }
    Ok(real)
}

/// GTK's own drag (the `drag` crate's GTK backend ended it before the other application had
/// asked for the files, so drops on other applications got nothing). Copy only: another file
/// manager must not move our files away by its own rules; a drop on a pane asks or goes by the
/// keys held (`drop_keys`).
#[cfg(all(unix, not(target_os = "macos")))]
fn native_drag(window: &tauri::Window, paths: Vec<PathBuf>, image: Vec<u8>, scale: i32) -> bool {
    use gtk::{gdk, gdk_pixbuf, glib, prelude::*};
    use std::{cell::RefCell, rc::Rc};
    let Ok(w) = window.gtk_window() else { return false };
    // Started when the button is still down, or the drag would hang on the pointer.
    let held = w.window().zip(w.display().default_seat().and_then(|s| s.pointer())).is_some_and(|(gw, p)| gw.device_position(&p).3.contains(gdk::ModifierType::BUTTON1_MASK));
    if !held {
        return false;
    }
    let uris: Vec<String> = paths.iter().filter_map(|p| glib::filename_to_uri(p, None).ok()).map(String::from).collect();
    let targets = gtk::TargetList::new(&[]);
    targets.add_uri_targets(0);
    let get = w.connect_drag_data_get(move |_, _, data, _, _| {
        data.set_uris(&uris.iter().map(String::as_str).collect::<Vec<_>>());
    });
    let Some(ctx) = w.drag_begin_with_coordinates(&targets, gdk::DragAction::COPY, 1, None, -1, -1) else {
        w.disconnect(get);
        return false;
    };
    let loader = gdk_pixbuf::PixbufLoader::new();
    if loader.write(&image).and_then(|()| loader.close()).is_ok()
        && let Some(surface) = loader.pixbuf().and_then(|pb| pb.create_surface(scale.max(1), w.window().as_ref()))
    {
        ctx.drag_set_icon_surface(&surface);
    }
    // The files are handed over after the drop: the handlers go when the drag has ended.
    let ids = Rc::new(RefCell::new(vec![get]));
    let end = w.connect_drag_end({
        let ids = ids.clone();
        move |w, _| ids.take().into_iter().for_each(|id| w.disconnect(id))
    });
    ids.borrow_mut().push(end);
    true
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn native_drag(window: &tauri::Window, paths: Vec<PathBuf>, image: Vec<u8>, _scale: i32) -> bool {
    drag::start_drag(window, drag::DragItem::Files(paths), drag::Image::Raw(image), |_, _| {}, drag::Options::default()).is_ok()
}

/// The keys held at a drop on a pane: copy (Ctrl; Option on macOS) or move (Shift; Command on
/// macOS), as `drag::by_keys` reads them.
#[tauri::command]
async fn drop_keys(window: tauri::Window) -> Res<(bool, bool)> {
    let (tx, rx) = std::sync::mpsc::channel();
    window
        .app_handle()
        .clone()
        .run_on_main_thread(move || {
            let _ = tx.send(held_keys(&window));
        })
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv().unwrap_or_default()).await.map_err(|e| e.to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn held_keys(window: &tauri::Window) -> (bool, bool) {
    use gtk::{gdk, prelude::*};
    let Ok(w) = window.gtk_window() else { return (false, false) };
    let m = gdk::Keymap::for_display(&w.display()).map_or(0, |k| k.modifier_state());
    let m = gdk::ModifierType::from_bits_truncate(m);
    (m.contains(gdk::ModifierType::CONTROL_MASK), m.contains(gdk::ModifierType::SHIFT_MASK))
}

#[cfg(target_os = "macos")]
fn held_keys(_: &tauri::Window) -> (bool, bool) {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state: i32) -> u64;
    }
    // The combined session state; Option and Command.
    let f = unsafe { CGEventSourceFlagsState(0) };
    (f & 0x0008_0000 != 0, f & 0x0010_0000 != 0)
}

#[cfg(windows)]
fn held_keys(_: &tauri::Window) -> (bool, bool) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetAsyncKeyState(key: i32) -> i16;
    }
    // VK_CONTROL, VK_SHIFT; the high bit is set while held.
    let down = |k| unsafe { GetAsyncKeyState(k) } < 0;
    (down(0x11), down(0x10))
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

/// Emit `dir-changed` with the watched folders that changed, once 250 ms pass without a change,
/// and at most every 2 s while changes keep coming (`fs::Settle`, as the terminal app does).
fn start_watcher(app: &tauri::AppHandle) -> Option<coxswain_core::DirWatcher> {
    use notify::Watcher;
    use std::sync::mpsc::RecvTimeoutError;
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let watcher = coxswain_core::DirWatcher::new(tx, notify::Config::default()).ok()?;
    let app = app.clone();
    std::thread::spawn(move || {
        let mut settle = coxswain_core::fs::Settle::default();
        loop {
            let first = match rx.recv_timeout(std::time::Duration::from_millis(250)) {
                Ok(e) => Some(e),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            let events = first.into_iter().chain(rx.try_iter());
            let now = std::time::Instant::now();
            settle.add(events.flatten().filter(|e| !e.kind.is_access()).flat_map(|e| e.paths).collect(), now);
            let paths = settle.take(now);
            if paths.is_empty() {
                continue;
            }
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

// ---------------------------------------------------------------- space

/// Space's tree, and the lists of the folder last shown (its id, bytes by type, largest files):
/// worked out once per folder, not again for each resize.
#[derive(Default)]
pub struct SpaceTree {
    tree: Option<coxswain_core::space::Tree>,
    lists: Option<(u32, Arc<Vec<coxswain_core::space::TypeRow>>, Arc<Largest>)>,
}

/// The largest files: id, bytes, path, and the model a store's blob is (`space::Tree::label`).
type Largest = Vec<(u32, u64, PathBuf, Option<String>)>;

/// Walk `dir` for Space, keeping every file's size; one walk at a time, `space_progress`
/// meanwhile. The tree stays here for `space_view` until `space_close`. What it holds in all,
/// the files only in the cloud, and how long it took.
#[tauri::command]
async fn space_scan(dir: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<(u64, u64, u64, f64)> {
    use coxswain_core::space::{Progress, Tree, ROOT};
    let p = Arc::new(Progress::default());
    if let Some(old) = ctx.space_walk.lock().map_err(|e| e.to_string())?.replace(p.clone()) {
        old.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let started = std::time::Instant::now();
    let walk = p.clone();
    let tree = tauri::async_runtime::spawn_blocking(move || Tree::scan(&dir, &walk)).await.map_err(|e| e.to_string())?;
    // Only the newest walk keeps its tree: an older one stopped late must not replace it.
    {
        let mut running = ctx.space_walk.lock().map_err(|e| e.to_string())?;
        if !running.as_ref().is_some_and(|r| Arc::ptr_eq(r, &p)) {
            return Err(coxswain_core::t!("err.cancelled"));
        }
        *running = None;
    }
    let tree = tree.ok_or_else(|| coxswain_core::t!("err.cancelled"))?;
    let (n, online) = (tree.node(ROOT), tree.online);
    let out = (n.size, n.files as u64, online, started.elapsed().as_secs_f64());
    *ctx.space.lock().map_err(|e| e.to_string())? = SpaceTree { tree: Some(tree), lists: None };
    Ok(out)
}

/// Files and bytes the walk has measured so far.
#[tauri::command]
fn space_progress(ctx: tauri::State<Ctx>) -> Option<(u64, u64)> {
    Some(ctx.space_walk.lock().ok()?.as_ref()?.now())
}

#[tauri::command]
fn space_cancel(ctx: tauri::State<Ctx>) {
    if let Some(p) = ctx.space_walk.lock().ok().and_then(|g| g.clone()) {
        p.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Space closed: its tree is let go (a whole disk's can be large).
#[tauri::command]
async fn space_close(ctx: tauri::State<'_, Ctx>) -> Res<()> {
    space_cancel(ctx.clone());
    let space = ctx.space.clone();
    blocking(move || {
        let old = std::mem::take(&mut *space.lock().map_err(|e| e.to_string())?);
        drop(old);
        Ok(())
    })
    .await
}

/// A rectangle of the map as the window draws it: its type's row in `types` (-1 for folders
/// and small blocks), and the name and size of one big enough to carry a label.
#[derive(Serialize)]
struct SpaceTile {
    #[serde(flatten)]
    tile: coxswain_core::space::Tile,
    ty: i32,
    name: Option<String>,
    size: u64,
}

#[derive(Serialize)]
struct SpaceView {
    path: PathBuf,
    size: u64,
    files: u32,
    /// From the top down to the folder shown: (id, name).
    crumbs: Vec<(u32, String)>,
    tiles: Vec<SpaceTile>,
    types: Arc<Vec<coxswain_core::space::TypeRow>>,
    /// The largest files: id, size, path.
    largest: Arc<Largest>,
    /// Each family's colour, by its id.
    colours: BTreeMap<&'static str, &'static str>,
}

/// The treemap of the folder `zoom` (an id of the tree; 0 the top) in `w` × `h` pixels, with
/// its bytes by type and its largest files.
#[tauri::command]
async fn space_view(zoom: u32, w: f32, h: f32, ctx: tauri::State<'_, Ctx>) -> Res<SpaceView> {
    use coxswain_core::space::{Kind, Opts, Rect, ROOT};
    let space = ctx.space.clone();
    blocking(move || {
        let mut guard = space.lock().map_err(|e| e.to_string())?;
        let SpaceTree { tree, lists } = &mut *guard;
        let t = tree.as_ref().ok_or("no space tree")?;
        let zoom = if t.alive(zoom) { zoom } else { ROOT };
        let (types, largest) = match lists {
            Some((z, ty, l)) if *z == zoom => (ty.clone(), l.clone()),
            _ => {
                let ty = Arc::new(t.types(zoom));
                let l: Arc<Largest> = Arc::new(t.largest(zoom, 100).into_iter().map(|i| (i, t.node(i).size, t.path(i), Some(t.label(i)).filter(|l| *l != t.node(i).label()).map(|l| l.into_owned()))).collect());
                *lists = Some((zoom, ty.clone(), l.clone()));
                (ty, l)
            }
        };
        let row: std::collections::HashMap<&str, i32> = types.iter().enumerate().map(|(i, r)| (r.ext.as_str(), i as i32)).collect();
        // Rectangles under about 10 × 10 pixels go into their folder's block of small files,
        // which also bounds what a flat folder of a million files sends.
        let opts = Opts { pad: 2.0, head: 18.0, min_area: 100.0 };
        let tiles = t
            .layout(zoom, Rect { x: 0.0, y: 0.0, w, h }, &opts)
            .into_iter()
            .map(|tile| {
                let n = t.node(tile.id);
                let r = tile.rect;
                let labelled = match tile.kind {
                    // The window shows it only where it fits whole (or nearly).
                    Kind::File => r.w >= 40.0 && r.h >= 16.0,
                    Kind::Folder => r.w >= opts.head * 4.0 && r.h >= opts.head * 3.0,
                    Kind::Mount => r.w >= 48.0 && r.h >= 16.0,
                    Kind::Small => false,
                };
                SpaceTile {
                    ty: if tile.kind == Kind::File { row.get(t.ext(tile.id).as_str()).copied().unwrap_or(-1) } else { -1 },
                    name: labelled.then(|| t.label(tile.id).into_owned()),
                    size: n.size,
                    tile,
                }
            })
            .collect();
        let mut crumbs: Vec<(u32, String)> = vec![];
        let mut at = zoom;
        loop {
            let name = if at == ROOT { t.root.display().to_string() } else { t.label(at).into_owned() };
            crumbs.push((at, name));
            if at == ROOT {
                break;
            }
            at = t.node(at).parent;
        }
        crumbs.reverse();
        let n = t.node(zoom);
        Ok(SpaceView {
            path: t.path(zoom),
            size: n.size,
            files: n.files,
            crumbs,
            tiles,
            largest,
            types,
            colours: coxswain_core::space::Family::ALL.iter().map(|f| (f.id(), f.colour())).collect(),
        })
    })
    .await
}

/// What the window shows of one rectangle: its path, and whether it is a folder.
#[tauri::command]
async fn space_info(id: u32, ctx: tauri::State<'_, Ctx>) -> Res<Option<(PathBuf, bool)>> {
    let space = ctx.space.clone();
    blocking(move || {
        let g = space.lock().map_err(|e| e.to_string())?;
        Ok(g.tree.as_ref().filter(|t| t.alive(id)).map(|t| (t.path(id), t.node(id).dir)))
    })
    .await
}

/// The folder one level below `zoom` on the way to `id`: what a zoom in shows.
#[tauri::command]
async fn space_toward(zoom: u32, id: u32, ctx: tauri::State<'_, Ctx>) -> Res<Option<u32>> {
    let space = ctx.space.clone();
    blocking(move || {
        let g = space.lock().map_err(|e| e.to_string())?;
        Ok(g.tree.as_ref().filter(|t| t.alive(id) && t.alive(zoom)).and_then(|t| t.toward(zoom, id).filter(|&n| t.node(n).dir).or((id != zoom && t.node(id).dir).then_some(id))))
    })
    .await
}

/// These went (a delete from Space): out of the tree, which shows the rest. The bytes and
/// files left in all.
#[tauri::command]
async fn space_gone(paths: Vec<PathBuf>, ctx: tauri::State<'_, Ctx>) -> Res<Option<(u64, u32)>> {
    let space = ctx.space.clone();
    blocking(move || {
        let mut g = space.lock().map_err(|e| e.to_string())?;
        g.lists = None;
        let Some(t) = g.tree.as_mut() else { return Ok(None) };
        for p in paths.iter().filter(|p| std::fs::symlink_metadata(p).is_err()) {
            t.remove(p);
        }
        let n = t.node(coxswain_core::space::ROOT);
        Ok(Some((n.size, n.files)))
    })
    .await
}

// ---------------------------------------------------------------- commands & scripts

fn output(mut c: std::process::Command, dir: &Path) -> Res<String> {
    let out = c.current_dir(dir).stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text += &String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        text += &format!("\n[{}]", out.status);
    }
    Ok(text)
}

/// Run the command line the user typed in `dir`, by `sh` (the names the page puts in are quoted
/// for it), and return what it printed. `cd` is handled by the UI.
#[tauri::command]
async fn run_command(cmd: String, dir: PathBuf) -> Res<String> {
    blocking(move || output(coxswain_core::tools::Line::from(cmd).command(), &dir)).await
}

#[derive(Serialize)]
struct Script {
    key: String,
    label: String,
    /// Index into `user_menu::entries`, or a script file, or neither: "Add your own command".
    user: Option<usize>,
    path: Option<PathBuf>,
    /// Show the output even when there is none.
    wait: bool,
}

/// F2 in `dir` with `file` under the cursor: the user menu's entries that fit there, then the
/// executables in `<config>/coxswain/scripts/`, then "Add your own command".
#[tauri::command(async)]
fn scripts(dir: PathBuf, file: Option<PathBuf>, ctx: tauri::State<Ctx>) -> Vec<Script> {
    // Read again: an entry just added in the editor shows at once.
    if let (Ok(new), Ok(mut cfg)) = (Config::load(), ctx.cfg.write()) {
        cfg.user_menu = new.user_menu;
    }
    let mut out: Vec<Script> = coxswain_core::user_menu::entries(&ctx.cfg(), &dir, file.as_deref())
        .into_iter()
        .enumerate()
        .map(|(i, u)| Script { key: u.key, label: u.label, user: Some(i), path: None, wait: u.wait })
        .collect();
    for p in coxswain_core::user_menu::scripts() {
        let label = coxswain_core::user_menu::script_label(&p);
        out.push(Script { key: String::new(), label, user: None, path: Some(p), wait: true });
    }
    let key = if out.iter().any(|s| s.key == "+") { "" } else { "+" };
    out.push(Script { key: key.into(), label: coxswain_core::t!("usermenu.add"), user: None, path: None, wait: false });
    out
}

/// Run a user-menu entry (expanded) or a script file with the selection as arguments.
#[tauri::command]
async fn run_script(user: Option<usize>, path: Option<PathBuf>, dir: PathBuf, file: Option<PathBuf>, selected: Vec<PathBuf>, ctx: tauri::State<'_, Ctx>) -> Res<String> {
    if let Some(i) = user {
        let list = coxswain_core::user_menu::entries(&ctx.cfg(), &dir, file.as_deref());
        let line = list.get(i).ok_or_else(|| coxswain_core::t!("err.no_such_command"))?.expand(&dir, file.as_deref(), &selected);
        return blocking(move || output(line.command(), &dir)).await;
    }
    let script = path.ok_or_else(|| coxswain_core::t!("err.nothing_to_run"))?;
    blocking(move || output(coxswain_core::user_menu::script_line(&script, file.as_deref(), &selected)?.command(), &dir)).await
}

/// Settings → Keys: config.toml at `[keys]` (an example written there first when it has none),
/// in `editor` at that line, or with the desktop's default when no editor is set.
#[tauri::command]
async fn open_keys(ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let (path, line) = blocking(Config::prepare_keys).await?;
    let editor = ctx.cfg().editor.clone();
    edit_at(path, line, editor).await
}

/// A notice of a mistake in config.toml: the file in `editor` at its `line`, or with the
/// desktop's default when no editor is set.
#[tauri::command]
async fn open_config_at(line: usize, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let path = Config::path().ok_or_else(|| coxswain_core::t!("err.no_config_folder"))?;
    let editor = ctx.cfg().editor.clone();
    edit_at(path, line, editor).await
}

/// `path` in `editor` at `line`, or with the desktop's default when no editor is set.
async fn edit_at(path: PathBuf, line: usize, editor: Option<String>) -> Res<()> {
    let Some(ed) = editor else { return open_path(path).await };
    let line = coxswain_core::config::edit_command(&ed, &path, line);
    tauri::async_runtime::spawn_blocking(move || coxswain_core::tools::spawn_watched(line.command(), std::time::Duration::from_secs(1)).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

/// F2's scripts folder, made when it is not there yet, for Settings → Keys to open.
#[tauri::command(async)]
fn scripts_folder() -> Res<PathBuf> {
    coxswain_core::user_menu::scripts_folder()
}

/// F2's "Add your own command": config.toml in `editor` at `[[user_menu]]` (an example
/// written there first), or with the desktop's default when no editor is set.
#[tauri::command]
async fn add_user_command(ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let (path, line) = blocking(coxswain_core::user_menu::prepare).await?;
    let editor = ctx.cfg().editor.clone();
    edit_at(path, line, editor).await
}

/// A newer release, if the (daily, cached) check found one, and the command that upgrades this
/// copy. The network call runs outside the state lock so other commands are not held up.
#[tauri::command]
async fn check_update(ctx: tauri::State<'_, Ctx>) -> Res<Option<(String, Option<coxswain_core::update::Upgrade>)>> {
    use coxswain_core::update;
    if !ctx.cfg().check_updates {
        return Ok(None);
    }
    let due = update::due(&*ctx.state.lock().map_err(|e| e.to_string())?);
    if due {
        // Up to 5 s on the network: not on the async runtime's workers.
        if let Some(latest) = tauri::async_runtime::spawn_blocking(update::fetch_latest).await.map_err(|e| e.to_string())? {
            ctx.edit(|st| update::record(st, latest))?;
        }
    }
    let v = update::available(&*ctx.state.lock().map_err(|e| e.to_string())?);
    Ok(v.map(|v| (v, update::upgrade_hint())))
}

/// Whether the webview may go to `url`: the app's own pages and files, never the web. A link
/// in a preview is opened outside instead (`open_path`); a page in the sandboxed frame that
/// tries to load another stays where it is. WebKit asks this for every frame, so a PDF's frame
/// needs `asset://localhost`; WebView2 asks it only for the window itself, which never shows a
/// file, so `asset.localhost` is not let in. A file the window did reach runs no script: the
/// protocol serves no page and sandboxes all but a real PDF (`asset::respond`).
fn local(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" | "about" | "blob" | "data" => true,
        "asset" => url.host_str() == Some("localhost"),
        "http" | "https" => matches!(url.host_str(), Some("tauri.localhost" | "ipc.localhost")) || (cfg!(debug_assertions) && url.host_str() == Some("localhost")),
        _ => false,
    }
}

const USAGE: &str = "coxswain-gui [LEFT] [RIGHT]   a folder, or a file to open its folder with the cursor on it
  --settings[=AREA|OPTION]  start with Settings open: overview, search, previews, looks,
                            behaviour, keys, privacy, or an option such as show_hidden
  --duplicates [FOLDER …]   start with Find duplicates scanning these folders (default: this one)
  --version
  --help
The terminal app, coxswain, has the rest: coxswain --help";

fn main() {
    coxswain_core::sizes::deep_stacks();
    coxswain_core::tools::flatpak_tmp();
    // Started by an app, not by hand: hold the file name index for all of them. No window.
    if std::env::args_os().nth(1).is_some_and(|a| a == helper::ARG) {
        return drop(helper::serve());
    }
    // As the terminal app has them: they print, and need no window.
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => return println!("coxswain-gui {}", coxswain_core::update::VERSION),
        Some("--help" | "-h") => return println!("{USAGE}"),
        _ => {}
    }
    coxswain_core::fs::lock_down();
    // What a write of an archive cut short by a crash left beside it.
    std::thread::spawn(coxswain_core::archive::clean_stale);
    // A config.toml of 1.x gets the names of 2.0, once.
    if let Err(e) = coxswain_core::migrate::on_start() {
        eprintln!("coxswain: {e}");
    }
    // A mistake in config.toml costs only its setting, and a notice names its line; only a
    // file that cannot be read at all leaves every default.
    let cfg = Config::load().unwrap_or_else(|e| {
        eprintln!("coxswain: {e}; using defaults");
        Config::default()
    });
    coxswain_core::i18n::set_language(coxswain_core::i18n::resolve(&cfg.language));
    coxswain_core::disk_image::set_open(cfg.archive.disk_images);
    // Over ssh, or on a console: GTK cannot start, and would say so with a panic.
    #[cfg(all(unix, not(target_os = "macos")))]
    if ["DISPLAY", "WAYLAND_DISPLAY", "GDK_BACKEND"].iter().all(|v| std::env::var_os(v).is_none_or(|x| x.is_empty())) {
        eprintln!("coxswain-gui: {}", coxswain_core::t!("cli.no_display"));
        std::process::exit(1);
    }
    // Launched from the Dock, a macOS app gets the bare system PATH, so docker, podman,
    // pandoc and the rest of Homebrew are invisible.
    if cfg!(target_os = "macos") {
        coxswain_core::tools::login_path();
    }
    // Launched from a desktop menu the cwd is usually `/`; home is a better start.
    let home = coxswain_core::dirs::home_dir().unwrap_or_default();
    let cwd = std::env::current_dir().ok().filter(|d| d.parent().is_some()).unwrap_or(home);
    // A folder's name need not be Unicode; the options are.
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let folder = |a: &std::ffi::OsStr| match a.to_str() {
        Some(s) => resolve(&cwd, s),
        None => cwd.join(a),
    };
    // `coxswain-gui --settings[=section] [folders…]`: start with the Settings window open, at
    // that section (`search`).
    let open_settings = args.first().and_then(|a| a.to_str()?.strip_prefix("--settings")).filter(|r| r.is_empty() || r.starts_with('=')).map(|r| r.trim_start_matches('=').to_string());
    if open_settings.is_some() {
        args.remove(0);
    }
    // `coxswain-gui --duplicates [folders…]`: the folders (default: the current one) are scanned for
    // duplicates at start instead of being opened in the panes.
    let duplicates = args.first().is_some_and(|a| a == "--duplicates").then(|| {
        let roots: Vec<PathBuf> = args.drain(..).skip(1).map(|a| folder(&a)).collect();
        if roots.is_empty() { vec![cwd.clone()] } else { roots }
    });
    let dir = |i: usize| args.get(i).map(|a| folder(a)).unwrap_or_else(|| cwd.clone());
    let index = Client::start(&cfg.search);
    let ctx = Ctx {
        sizer: Arc::new(coxswain_core::sizes::Sizer::new(Some(index.clone()))),
        index,
        start: [dir(0), dir(1)],
        duplicates,
        open_settings,
        guide: AppState::load().guide_due(),
        state: Mutex::new({
            // A first start of this version is remembered, so the next update is told of.
            let mut st = AppState::load();
            if coxswain_core::notices::started(&mut st) {
                let _ = st.save();
            }
            // The servers a Subversion or CVS history may be read from.
            coxswain_core::wc::allow(&st.history_servers);
            st
        }),
        cfg: std::sync::RwLock::new(cfg),
        watched: Mutex::default(),
        watcher: Mutex::default(),
        clip: Mutex::default(),
        dupes: Mutex::default(),
        space_walk: Mutex::default(),
        space: Arc::default(),
        meaning: Mutex::default(),
        chat: Mutex::default(),
        measuring: Mutex::default(),
        asking: Arc::default(),
        undo: Mutex::default(),
        job_stop: Mutex::default(),
        job_turn: tauri::async_runtime::Mutex::new(()),
        app: std::sync::OnceLock::new(),
    };
    tauri::Builder::default()
        .manage(ctx)
        .manage(tabs::Tabs::default())
        // Files for the page come through our own `asset:`, which keeps cloud files in the cloud.
        .register_asynchronous_uri_scheme_protocol("asset", |_, req, responder| {
            tauri::async_runtime::spawn_blocking(move || responder.respond(asset::respond(&req, coxswain_core::cloud::unasked)));
        })
        .setup(|app| {
            *app.state::<Ctx>().watcher.lock().expect("fresh lock") = start_watcher(app.handle());
            let _ = app.state::<Ctx>().app.set(app.handle().clone());
            convert::keep(app.handle().clone());
            // The window from the config, made here so that it gets its navigation guard.
            let window = app.config().app.windows.first().cloned().ok_or("no window in the config")?;
            tauri::WebviewWindowBuilder::from_config(app.handle(), &window)?.on_navigation(local).build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config, action_menu, hint, hint_once, hints_reset, notices, dismiss_notice, changes, read_changes, set_title, index_status, index_action, index_service, meaning_status, meaning_action, chat_status, chat_action, meaning_models, meaning_pull, tabs::tab_list, tabs::tab_rows, tabs::tab_sort, tabs::tab_find, tabs::tab_quick, tabs::tab_mark, tabs::tab_pick, tabs::tab_drop, tabs::tab_reset, dir_summary, zfs_facts, bsd_places, clouds, git_status, git_facts, git_last, history_graph, history_server, history_allow, history_forget, git_switch, branch_name, git_new_branch, git_tool, git_tools, git_config, places, disks, get_state, save_session, save_favorites, set_tags, set_note, get_note,
            find, find_read_too, ask, ask_stop, ask_check, meaning_change, setup_probe, setup_probe_url, setup_try, setup_speed, meaning_bench, resolve_path, copy, rename, delete_plan, delete, mkdir, dir_sizes, dir_size_known, rename_plan, rename_shown, rename_typed, rename_apply, replace::replace_plan, replace::replace_apply, replace::replace_progress, replace::replace_stop, attrs::attrs_plan, attrs::attrs_apply, attrs::attrs_progress, attrs::attrs_stop, folders_sync::sync_check, folders_sync::sync_plan, folders_sync::sync_apply, folders_sync::sync_progress, folders_sync::sync_stop, undo, undo_next, open_path, edit_path, edit_open, edit_save, compare::compare_open, compare::compare_close, compare::compare_state, compare::compare_rows, compare::compare_hex, compare::compare_next, compare::compare_take, compare::compare_undo, compare::compare_save, compare::folders_open, compare::folders_close, compare::folders_state, compare::folders_states, compare::folders_diff_only, compare::folders_ahead, open_keys, open_config_at, scripts_folder,
            plain_error, is_model,
            read_text, code_language, run_command, scripts, run_script, add_user_command, check_update, archive_list, extract, pack, archive_password, archive_peek, disk_image_facts, opens_as_folder, cloud_fetch, properties, set_flags, set_permissions,
            clip_set, paste, start_drag, drop_keys, watch_dirs, chess::chess_looks, chess::chess_rows, chess::chess_game, chess::chess_positions, chess::chess_hit, chess::chess_close, chess::chess_book, chess::chess_is_book, chess::chess_sniffed, chess::chess_moves, chess::chess_analyse, chess::chess_save_line, chess::chess_review, chess::chess_review_save, chess::chess_position_games, go::go_games, go::go_game, go::go_position, go::go_hit, go::go_close, preview::git_diff, preview::sqlite_info, preview::epub_preview,
            preview::file_facts, preview::old_picture, preview::comic_pages, preview::module_names, preview::cert_info, bom::bom_info, bom::bom_node, bom::bom_diff, provenance::provenance_info, provenance::provenance_statements, provenance::provenance_subject, provenance::provenance_cancel, provenance::provenance_sources, provenance::provenance_diff, provenance::provenance_bom, preview::mail_preview, preview::plist_xml, convert::preview_engines, convert::preview_cache, convert::clear_preview_cache,
            job_progress, job_stop, pack_room, convert::convert, convert::preview_tools, convert::preview_with, long_files_change, long_files_choices, convert::preview_sample, set_preview_tool, ebook::ebook_plan, ebook::ebook_run, ebook::ebook_progress, ebook::ebook_stop, convert::pull_image, convert::remove_image, convert::pull_progress, convert::searchable_plan, convert::searchable_run, convert::searchable_progress, convert::searchable_stop, convert::pdf_scanned, viruses::viruses_plan, viruses::viruses_run, viruses::viruses_progress, viruses::viruses_stop, viruses::viruses_act, viruses::viruses_facts, convert::media_thumbnail, convert::media_plan, convert::media_run, convert::media_progress, convert::media_stop, convert::export_plan, convert::export_run, convert::export_progress, convert::export_stop, convert::drawio_xml, convert::drawio_thumbnail, convert::adoc_blocks, convert::adoc_diagram, convert::gpx_map, dupes_scan,
            dupes_progress, dupes_cancel, space_scan, space_progress, space_cancel, space_view, space_info, space_toward, space_gone, space_close, save_settings, search_status, search_level, guide_seen, nerd_font, copy_text, features, open_docs, models_list, models_action, disk_list, disk_clear, own_store, work::sums_algos, work::sums_make, work::sums_verify, work::work_progress, work::work_stop, transcribe_plan, transcribe, transcript, whisper_models, work::split_sizes, work::split_size, work::split_file, work::join_plan, work::join_parts
        ])
        .build(tauri::generate_context!())
        .expect("error while running Coxswain")
        // The last window closed: a Make PDF searchable run, a conversion or Convert e-book is stopped
        // and waited for, so its container is killed and its scratch folder removed; a file
        // operation too, at the file it is on, so none is left half copied.
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(ctx) = app.try_state::<Ctx>() {
                    let _ = job_stop(ctx);
                }
                convert::searchable_end();
                convert::media_end();
                ebook::end();
                for _ in 0..100 {
                    if JOB.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                coxswain_core::archive::clean_up();
                coxswain_core::drag::done();
                work::work_end();
                convert::export_end();
                viruses::viruses_end();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's tests keep their config, state and caches in a home of their own in the
    /// temporary folder (`coxswain-core`'s `test-home`), never the developer's own.
    #[test]
    fn tests_never_reach_the_real_home() {
        let mut all = coxswain_core::config::Config::paths();
        all.push(("home", coxswain_core::dirs::home_dir()));
        for (what, path) in all {
            let path = path.unwrap_or_else(|| panic!("{what}: none"));
            assert!(path.starts_with(std::env::temp_dir()), "{what}: {}", path.display());
        }
    }

    #[test]
    fn a_listing_sends_a_path_only_where_the_page_cannot_join_it() {
        let e = |name: &str, path: PathBuf| Entry { name: name.into(), path, is_dir: false, is_symlink: false, is_exec: false, hidden: false, size: 0, modified: 0, created: 0, online: false, referenced: 0 };
        let dir = Path::new("/srv/box");
        let (prefix, items) = to_page(dir, vec![e("..", "/srv".into()), e("a.txt", dir.join("a.txt")), e("x", dir.join("x")), e("b", Path::new("/elsewhere").join("b"))], |_| None, None);
        assert_eq!(prefix, format!("/srv/box{}", std::path::MAIN_SEPARATOR));
        let left: Vec<_> = items.iter().map(|i| i.entry.path.as_os_str().is_empty()).collect();
        assert_eq!(left, [false, true, true, false], "`..` and one elsewhere keep their paths");
        // A folder whose name ends in the letter the prefix is found with.
        assert_eq!(to_page(Path::new("/tmp/xx"), vec![], |_| None, None).0, format!("/tmp/xx{}", std::path::MAIN_SEPARATOR));
        let json = serde_json::to_string(&items[1]).unwrap();
        assert!(!json.contains("\"path\""), "{json}");
    }

    #[test]
    fn git_statuses_of_a_folder_come_from_git_alone() {
        let root = Path::new("/r");
        let out = "1 .M N... 100644 100644 100644 0 0 src/a.rs\0? new.txt\0! target/\0";
        let s = git::Status::parse(root, out);
        let (files, all) = children(&s, root);
        let mut names: Vec<_> = files.keys().cloned().collect();
        names.sort();
        assert_eq!(names, ["new.txt", "src", "target"], "files and folders right in it, a changed one by its folder");
        assert!(all.is_none());
        let (files, all) = children(&s, &root.join("target/debug"));
        assert!(files.is_empty());
        assert_eq!(all.map(|s| s.kind), Some(git::Kind::Ignored), "in an ignored folder, everything is");
    }

    #[test]
    fn the_webview_stays_on_the_app() {
        let ok = |u: &str| local(&tauri::Url::parse(u).unwrap());
        assert!(ok("tauri://localhost/index.html") && ok("http://tauri.localhost/index.html") && ok("asset://localhost/%2Fhome%2Fme%2Fa.pdf") && ok("about:srcdoc"));
        assert!(!ok("http://asset.localhost/C%3A/a.pdf") && !ok("asset://evil.example/%2Fa.pdf"));
        assert!(!ok("https://example.com/") && !ok("http://example.com/") && !ok("file:///etc/passwd") && !ok("javascript:alert(1)"));
    }

    #[test]
    fn clipboard_uris_roundtrip() {
        let p = PathBuf::from("/tmp/a b/ø%.txt");
        if !cfg!(any(windows, target_os = "macos")) {
            assert_eq!(to_clip(&p), "file:///tmp/a%20b/%C3%B8%25.txt");
        }
        assert_eq!(from_clip(&to_clip(&p)), p);
        assert_eq!(from_clip("file:///x/%zz"), PathBuf::from("/x/%zz"));
    }
}

/// What the desktop app sends for a big folder, ignored by default: `cargo test --release -p
/// coxswain-gui -- --ignored --nocapture perf_`. `COXSWAIN_BENCH_DIR` is the folder with the
/// data `coxswain-core`'s `tests/perf.rs` makes; the JSON goes next to it for the DOM benchmark.
#[cfg(test)]
mod perf {
    use super::*;

    #[test]
    #[ignore]
    fn perf_list_dir_json_100k() {
        let Some(bench) = std::env::var_os("COXSWAIN_BENCH_DIR").map(PathBuf::from).filter(|d| d.join("flat-100000").is_dir()) else {
            return println!("no COXSWAIN_BENCH_DIR/flat-100000: run coxswain-core's perf_list_and_sort_100k first");
        };
        let dir = bench.join("flat-100000");
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let t = std::time::Instant::now();
        let (mut entries, _) = bfs::list_with_archive(&dir, true).unwrap();
        bfs::sort(&mut entries, SortKey::Name, false);
        let (_, items) = to_page(&dir, entries, |_| None, None);
        println!("listing (list, sort, icons): {:.0} ms", ms(t));
        let t = std::time::Instant::now();
        let json = serde_json::to_vec(&items).unwrap();
        println!("listing to JSON: {:.0} ms, {:.1} MB for {} items", ms(t), json.len() as f64 / 1e6, items.len());
        std::fs::write(bench.join("listing.json"), json).unwrap();
    }
}

//! Coxswain GUI: Tauri commands over coxswain-core. The Svelte side owns all UI state except what
//! persists (session, favorites, tags, notes), which lives in `coxswain_core::state`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use coxswain_core::config::{color_to_rgb, Action, Config, Glyphs, GuiConfig};
use coxswain_core::fs::{self as bfs, Entry, SortKey};
use coxswain_core::icons::{self, Icon};
use coxswain_core::helper::{self, Client};
use coxswain_core::index::State;
use coxswain_core::rename::{self, Flags, Planned};
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
mod bom;
mod provenance;
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
    /// The first start: the first-run guide opens.
    guide: bool,
    /// Folders shown in the panes, watched so they reread themselves.
    watched: Mutex<Vec<PathBuf>>,
    watcher: Mutex<Option<coxswain_core::DirWatcher>>,
    /// What Coxswain last put on the clipboard, and whether it was a cut.
    clip: Mutex<(Vec<PathBuf>, bool)>,
    /// The running duplicate scan's progress, if any.
    dupes: Mutex<Option<Arc<coxswain_core::dupes::Progress>>>,
    sizer: Arc<coxswain_core::sizes::Sizer>,
    /// The model download for search by meaning, while one runs or has failed.
    meaning: Mutex<Option<(Arc<coxswain_core::meaning::Progress>, Option<String>)>>,
    /// The download of a built-in chat model (its `ask_model`), while one runs or has failed.
    chat: Mutex<Option<ChatDownload>>,
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

// A command without `async` runs on the main thread, the webview's: everything that reads or
// writes the disk, the clipboard or the config is `command(async)` and runs on the async
// runtime instead, so a slow disk never freezes the window. What needs the main thread
// (`set_title`, `start_drag`) and what only reads memory stays plain.

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
    /// Group headings with their actions, in the order F1 and F9 list them.
    groups: Vec<(String, Vec<&'static str>)>,
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
    /// Open the first-run guide at start; its keys and themes.
    guide: bool,
    guide_keys: Vec<(String, String)>,
    /// Step 1's line on the action menu and right-clicks.
    guide_menu: String,
    guide_themes: [&'static str; 4],
    /// The line that installs each program Coxswain can use, on this system, where it knows one.
    installs: BTreeMap<&'static str, Option<String>>,
    /// Ask's chat model as the user sees it: a built-in one by its name.
    ask_name: String,
}

fn css(c: &str) -> Option<String> {
    color_to_rgb(c).map(|(r, g, b)| format!("#{r:02x}{g:02x}{b:02x}"))
}

#[tauri::command(async)]
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
        groups: coxswain_core::config::Group::ALL.iter().map(|g| (g.label(), g.actions().map(Action::name).collect())).collect(),
        themes,
        looks: cfg.themes.iter().map(|(name, t)| (name.clone(), t.look.clone())).collect(),
        builtin_themes: coxswain_core::config::Theme::builtin().into_iter().map(|(name, _)| name).collect(),
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
        paths: Config::paths(),
        level: coxswain_core::settings::level(&cfg.search),
        levels: coxswain_core::settings::Level::ALL.iter().map(|l| (*l, l.costs())).collect(),
        config_path: Config::path(),
        gui: cfg.gui.clone(),
        home: std::env::home_dir().unwrap_or_default(),
        sep: std::path::MAIN_SEPARATOR,
        version: coxswain_core::update::VERSION,
        media_missing: coxswain_core::tools::media_missing(),
        pack_formats: coxswain_core::archive::PACK_FORMATS,
        guide: ctx.guide,
        guide_keys: coxswain_core::guide::keys(&cfg),
        guide_menu: coxswain_core::guide::menu_line(&cfg),
        guide_themes: coxswain_core::guide::THEMES,
        ask_name: coxswain_core::chat::shown(&cfg.search.ask_model),
        installs: ["tesseract", "pdftoppm", "soffice", "latex", "plantuml", "pandoc", "nerd-font"].into_iter().map(|p| (p, coxswain_core::tools::install(p))).collect(),
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

/// Settings → Finding files: one line for each part of search, with its next step.
#[tauri::command]
async fn search_status(ctx: tauri::State<'_, Ctx>) -> Res<Vec<coxswain_core::settings::Line>> {
    let (index, cfg) = (ctx.index.clone(), ctx.cfg().search.clone());
    blocking(move || Ok(coxswain_core::settings::status(&cfg, &index.status(), index.shared()))).await
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
    coxswain_core::menu::hint(&subject, &ctx.cfg(), last.as_ref().map(|(s, id)| (s, id.as_str())))
}

#[tauri::command(async)]
fn hints_reset() -> Res<()> {
    coxswain_core::menu::reset().map_err(|e| e.to_string())
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

#[derive(Serialize)]
struct Listing {
    dir: PathBuf,
    /// What an entry's path starts with when it is the folder's path and its name: those
    /// entries come without a path (the page joins it), a third less JSON for a big folder.
    prefix: String,
    items: Vec<Item>,
    has_notes: bool,
    /// The archive the folder is inside, and whether something in it is locked.
    archive: Option<PathBuf>,
    locked: bool,
    /// The history the folder is in.
    history: Option<HistoryInfo>,
    /// The ZFS snapshot the folder is in, or the list of snapshots it is.
    snapshot: Option<coxswain_core::zfs::At>,
    /// The package whose files the folder lists.
    package: Option<String>,
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
}

/// Async, as a compressed tar is read in full to list a folder in it.
#[tauri::command]
async fn list_dir(dir: PathBuf, show_hidden: bool, sort: SortKey, reverse: bool, ctx: tauri::State<'_, Ctx>) -> Res<Listing> {
    // Reading the folder (an archive's, a history's: git) blocks: not on the runtime's workers.
    let d = dir.clone();
    let (entries, inside, in_history, snapshot, package) = tauri::async_runtime::spawn_blocking(move || {
        let dir = d;
        let (mut entries, inside) = bfs::list_with_archive(&dir, show_hidden).map_err(|e| format!("{}: {e}", dir.display()))?;
        // Gone into an archive: the user asked for it, so its files may be previewed.
        if let Some((archive, _)) = &inside {
            coxswain_core::cloud::ask_for(archive);
        }
        bfs::sort_in(&dir, &mut entries, sort, reverse);
        // By last commit: as the walk git did is known; else by name now, and the page sorts
        // again when `git_last` answers.
        if sort == SortKey::Commit
            && let Some(lasts) = history::last_changes_cached(&dir)
        {
            history::sort_by_last(&mut entries, &lasts, reverse);
        }
        let in_history = history::split(&dir).filter(|_| !dir.is_dir()).map(|at| HistoryInfo { commit: at.commit.as_deref().and_then(|c| history::show(&at.base, c).ok()), target: at.target, base: at.base, view: at.view });
        let snapshot = coxswain_core::zfs::at(&dir);
        let package = coxswain_core::bsd::split(&dir).and_then(|f| coxswain_core::bsd::package_of(&f));
        Ok::<_, String>((entries, inside, in_history, snapshot, package))
    })
    .await
    .map_err(|e| e.to_string())??;
    let st = ctx.state.lock().map_err(|e| e.to_string())?;
    let cfg = ctx.cfg();
    let plain = cfg.plain_glyphs().then(|| cfg.glyphs());
    let (prefix, items) = to_page(&dir, entries, |p| st.tags.get(p).cloned(), plain.as_ref());
    let (archive, locked) = inside.map_or((None, false), |(a, locked)| (Some(a), locked));
    Ok(Listing { has_notes: st.notes.contains_key(&dir), dir, prefix, items, archive, locked, history: in_history, snapshot, package })
}

/// The entries as the page gets them: with their icon and tag, and without the path when it
/// is `prefix` and the name (see `Listing::prefix`).
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
}

#[tauri::command]
async fn git_status(dir: PathBuf, ctx: tauri::State<'_, Ctx>) -> Res<Option<GitInfo>> {
    // `git status` on a big repository takes a while: not on the async runtime's workers.
    let d = dir.clone();
    let Some(s) = tauri::async_runtime::spawn_blocking(move || git::Status::read(&d)).await.map_err(|e| e.to_string())? else { return Ok(None) };
    let (files, all) = children(&s, &dir);
    // Every listing comes through here: the state file is written only when the repo moves up.
    let mut st = ctx.state.lock().map_err(|e| e.to_string())?;
    if st.touch_repo(&s.root) {
        st.save().map_err(|e| format!("saving state: {e}"))?;
    }
    drop(st);
    Ok(Some(GitInfo { prompt: s.prompt(&ctx.cfg().glyphs()), branch: s.summary.branch.clone(), root: s.root, files, all }))
}

/// Switch to the branch `entry` of the list of branches `dir`: git's message, or why it refused.
#[tauri::command]
async fn git_switch(dir: PathBuf, entry: String) -> Res<String> {
    blocking(move || coxswain_core::branches::switch(&dir, &entry).map_err(|e| e.to_string())).await
}

/// A new branch `name`, switched to: from the branch `from` of the list of branches `dir`, or
/// from the current commit of the repository `dir` is in.
#[tauri::command]
async fn git_new_branch(dir: PathBuf, name: String, from: Option<String>) -> Res<String> {
    blocking(move || coxswain_core::branches::create(&dir, &name, from.as_deref()).map_err(|e| e.to_string())).await
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

/// System mounts that are not places to go. `/run` is one, except `/run/media`, where
/// removable disks are mounted. So is a ZFS snapshot ZFS mounted when it was looked into.
fn hidden_mount(mount: &Path) -> bool {
    (["/boot", "/efi", "/snap", "/var/lib", "/run", "/proc", "/sys"].iter().any(|p| mount.starts_with(p)) && !mount.starts_with("/run/media")) || coxswain_core::zfs::in_snapshot(mount).is_some()
}

/// The mounted disks and their free space. The window asks again every so often, so the
/// work runs on a blocking thread: a slow network mount must not hold up other commands.
#[tauri::command]
async fn disks() -> Res<Vec<Disk>> {
    tauri::async_runtime::spawn_blocking(read_disks).await.map_err(|e| e.to_string())
}

/// The mounted file systems as unlabelled disks: from sysinfo where it knows the system.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd", windows))]
fn mounted_disks() -> Vec<Disk> {
    let list = sysinfo::Disks::new_with_refreshed_list();
    list.list().iter().map(|d| Disk { label: String::new(), device: d.name().to_string_lossy().into_owned(), mount: d.mount_point().to_path_buf(), total: d.total_space(), free: d.available_space(), removable: d.is_removable() }).collect()
}

/// The mounted file systems as unlabelled disks: from the core on NetBSD, OpenBSD and
/// illumos, which sysinfo does not know.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd", windows)))]
fn mounted_disks() -> Vec<Disk> {
    coxswain_core::machine::mounted().into_iter().map(|m| Disk { label: String::new(), device: m.device, mount: m.mount, total: m.total, free: m.free, removable: false }).collect()
}

fn read_disks() -> Vec<Disk> {
    let mut out: Vec<Disk> = vec![];
    // Subvolumes and bind mounts repeat the same device; keep its shortest mount point.
    let mut sorted: Vec<Disk> = mounted_disks().into_iter().filter(|d| d.total > 0).collect();
    sorted.sort_by_key(|d| d.mount.as_os_str().len());
    for mut d in sorted {
        if hidden_mount(&d.mount) || out.iter().any(|o| o.device == d.device) {
            continue;
        }
        d.label = if d.mount.parent().is_none() { coxswain_core::t!("place.system") } else { d.mount.file_name().map_or(d.device.clone(), |n| n.to_string_lossy().into_owned()) };
        out.push(d);
    }
    out
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

#[tauri::command(async)]
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

/// A chat model's download: its `ask_model`, how far it is, and the error that stopped it.
type ChatDownload = (String, Arc<coxswain_core::meaning::Progress>, Option<String>);

/// A built-in chat model, for Settings and the setup.
#[derive(Serialize)]
struct ChatModel {
    /// Its `ask_model`.
    key: String,
    name: &'static str,
    size: u64,
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
    /// The built-in model to preselect when Ask has none and no server offers one.
    preselected: String,
}

#[tauri::command(async)]
fn chat_status(ctx: tauri::State<Ctx>) -> Res<ChatStatus> {
    use coxswain_core::chat;
    use std::sync::atomic::Ordering;
    static RAM: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    // The first time on a processor: the probe, under a second, then kept.
    let cpu_only = ctx.cfg().search.meaning_device == "cpu";
    let ram = *RAM.get_or_init(coxswain_core::setup::ram_gb);
    let suggested = chat::suggest(ram, cpu_only);
    let models = chat::MODELS
        .iter()
        .map(|m| ChatModel { key: m.key(), name: m.name, size: m.size(), installed: m.installed(), suggested: suggested.is_some_and(|s| std::ptr::eq(m, s)), estimate: chat::estimate(m, cpu_only, true).map(|e| e.text()) })
        .collect();
    let d = ctx.chat.lock().map_err(|e| e.to_string())?;
    Ok(ChatStatus {
        models,
        downloading: d.as_ref().filter(|(_, _, err)| err.is_none()).map(|(k, p, _)| (k.clone(), p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed))),
        error: d.as_ref().and_then(|(_, _, e)| e.clone()),
        runs: coxswain_core::setup::builtin_runs(&ctx.cfg().search, None),
        preselected: chat::preselect(ram, cpu_only).key(),
    })
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
            blocking(move || m.remove().map_err(|e| e.to_string())).await?;
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
    found: Vec<coxswain_core::setup::Found>,
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
        Ok(SetupLook { found, machine, advice, best, builtin_runs: setup::builtin_runs(&cfg, runs.as_ref()) })
    })
    .await
}

/// A server on another machine, named in the setup: what answers there.
#[tauri::command]
async fn setup_probe_url(url: String, ctx: tauri::State<'_, Ctx>) -> Res<Option<coxswain_core::setup::Found>> {
    let key = coxswain_core::meaning::key_of(&ctx.cfg().search);
    blocking(move || Ok(coxswain_core::setup::probe_url(&url, key.as_deref()))).await
}

/// The test question to Ask's chat model: milliseconds to its first word.
#[tauri::command]
async fn setup_try(ctx: tauri::State<'_, Ctx>) -> Res<u64> {
    let cfg = ctx.cfg().search.clone();
    blocking(move || coxswain_core::setup::try_ask(&cfg).map(|d| d.as_millis() as u64)).await
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
        _ => index.restart(),
    })
    .await
    .map_err(|e| e.to_string())
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
        let rows = find::rows(&query, chip, &found, ask.clone(), |id| dismissed.iter().any(|d| d == id));
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
    Sources { paths: Vec<PathBuf> },
    Piece { text: String },
}

/// How many passages an answer is made from.
const ASK_PASSAGES: usize = 10;

/// Ask: `question` answered by the user's chat model from the closest passages, with the
/// questions and answers `earlier` in this Find file. Nothing is kept.
#[tauri::command]
async fn ask(question: String, earlier: Vec<(String, String)>, scope: Option<PathBuf>, on_event: tauri::ipc::Channel<AskEvent>, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    use std::sync::atomic::Ordering;
    let (index, cfg, asking) = (ctx.index.clone(), ctx.cfg().search.clone(), ctx.asking.clone());
    let me = asking.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        // The chat model loads while the sources are looked up.
        coxswain_core::meaning::warm(&cfg);
        // A follow-up is looked up with the question before it, which it often leans on.
        let lookup = earlier.last().map_or(question.clone(), |(q, _)| format!("{q} {question}"));
        let sources = index.passages(&lookup, scope.as_deref(), ASK_PASSAGES);
        if sources.is_empty() {
            return Err(match &scope {
                Some(dir) => coxswain_core::t!("find.ask_nothing_in", "folder" => dir.file_name().unwrap_or_default().to_string_lossy()),
                None => coxswain_core::t!("search.ask_nothing"),
            });
        }
        let _ = on_event.send(AskEvent::Sources { paths: sources.iter().map(|(p, _)| p.clone()).collect() });
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

/// Run `op` on every source, on a blocking thread (a big copy must not hold one of the async
/// runtime's few workers); collect failures into one message.
async fn each(paths: Vec<PathBuf>, op: impl Fn(&Path) -> std::io::Result<()> + Send + 'static) -> Res<()> {
    blocking(move || {
        let errors: Vec<String> = paths.iter().filter_map(|p| op(p).err().map(|e| format!("{}: {e}", p.display()))).collect();
        if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
    })
    .await
}

/// `f` on a blocking thread, its answer back here.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn copy(paths: Vec<PathBuf>, base: PathBuf, dest: String, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let dst = resolve(&base, &dest);
    ctx.sizer.forget(&dst.join("new"));
    // The password of a locked archive, held for this copy only.
    each(paths, move |p| bfs::copy_locked(p, &dst, password.as_deref()).map(drop)).await
}

#[tauri::command]
async fn rename(paths: Vec<PathBuf>, base: PathBuf, dest: String, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let dst = resolve(&base, &dest);
    paths.iter().chain([&dst.join("new")]).for_each(|p| ctx.sizer.forget(p));
    each(paths, move |p| bfs::rename_locked(p, &dst, password.as_deref()).map(drop)).await
}

/// To the trash, or gone for good with `forever`. Inside a locked 7z, `password` opens it.
#[tauri::command]
async fn delete(paths: Vec<PathBuf>, forever: bool, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    paths.iter().for_each(|p| ctx.sizer.forget(p));
    each(paths, move |p| {
        let pw = password.as_deref();
        if forever { bfs::delete_locked(p, pw) } else { bfs::trash_locked(p, pw) }
    })
    .await
}

/// Async, as inside an archive the archive is written anew.
#[tauri::command]
async fn mkdir(base: PathBuf, name: String, password: Option<String>) -> Res<PathBuf> {
    let d = resolve(&base, &name);
    blocking(move || bfs::mkdir_locked(&d, password.as_deref()).map(|_| d.clone()).map_err(|e| format!("{}: {e}", d.display()))).await
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

#[tauri::command(async)]
fn rename_plan(dir: PathBuf, selected: Vec<String>, pattern: String, replacement: String, flags: Flags) -> Res<Vec<Planned>> {
    let existing: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .collect();
    rename::plan(&selected, &existing, &pattern, &replacement, flags)
}

#[tauri::command(async)]
fn rename_apply(dir: PathBuf, plan: Vec<Planned>) -> Res<()> {
    rename::apply(&dir, &plan)
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
    let cmd = format!("{ed} {}", coxswain_core::config::quote(&path.to_string_lossy()));
    tauri::async_runtime::spawn_blocking(move || coxswain_core::tools::spawn_watched(shell(&cmd), std::time::Duration::from_secs(1)).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
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
    if buf.iter().take(8192).any(|&b| b == 0) {
        // A hex dump of the first 64 KB: four thousand lines are plenty to see what a file is.
        let hex = buf
            .chunks(16)
            .take(HEX_LINES)
            .enumerate()
            .map(|(i, c)| {
                let h: String = c.iter().map(|b| format!("{b:02x} ")).collect();
                let a: String = c.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }).collect();
                format!("{:08x}  {h:<48} {a}", i * 16)
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Ok((hex, len > (HEX_LINES * 16) as u64, true));
    }
    Ok((String::from_utf8_lossy(&buf).into_owned(), truncated, false))
}

// ---------------------------------------------------------------- archives

#[derive(Serialize)]
struct ArchiveListing {
    entries: Vec<coxswain_core::archive::ArchiveEntry>,
    more: bool,
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
    let (entries, more) = blocking(move || coxswain_core::archive::list(&path, 2000).map_err(|e| e.to_string())).await?;
    Ok(ArchiveListing { entries, more })
}

#[tauri::command]
async fn extract(paths: Vec<PathBuf>, base: PathBuf, dest: String, password: Option<String>, ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let dst = resolve(&base, &dest);
    ctx.sizer.forget(&dst.join("new"));
    each(paths, move |p| coxswain_core::archive::extract_locked(p, &dst, password.as_deref()).map(drop)).await
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
async fn pack(paths: Vec<PathBuf>, base: PathBuf, dest: String, password: Option<String>, hide_names: bool, ctx: tauri::State<'_, Ctx>) -> Res<PathBuf> {
    let to = resolve(&base, &dest);
    ctx.sizer.forget(&to);
    let at = to.clone();
    tauri::async_runtime::spawn_blocking(move || coxswain_core::archive::create_locked(&at, &paths, password.as_deref(), hide_names))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{}: {e}", to.display()))?;
    // The format is suggested next time, in both apps.
    if let Some(f) = coxswain_core::archive::pack_format(&dest) {
        ctx.edit(|st| st.pack_ending = f.endings[0].into())?;
    }
    Ok(to)
}

// ---------------------------------------------------------------- properties

/// On a blocking thread: a folder's size is a walk of it, and the facts run `zfs` and `pkg`.
#[tauri::command]
async fn properties(path: PathBuf) -> Res<bfs::Props> {
    blocking(move || bfs::properties(&path).map_err(|e| format!("{}: {e}", path.display()))).await
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
    each(paths, move |p| {
        if cut && p.parent() == Some(dir.as_path()) {
            return Ok(()); // cut and pasted in place
        }
        let to = bfs::free_name(&dir, &p.file_name().unwrap_or_default().to_string_lossy());
        sizer.forget(&to);
        if cut {
            sizer.forget(p);
        }
        if cut { bfs::rename(p, &to).map(drop) } else { bfs::copy(p, &to).map(drop) }
    })
    .await?;
    Ok((n, cut))
}

// ---------------------------------------------------------------- drag out, watching

/// A native drag, so files can be dropped on other applications (and back on Coxswain).
#[tauri::command]
fn start_drag(paths: Vec<PathBuf>, window: tauri::Window) -> Res<()> {
    let app = window.app_handle().clone();
    app.run_on_main_thread(move || {
        let icon = drag::Image::Raw(include_bytes!("../icons/32x32.png").to_vec());
        #[cfg(all(unix, not(target_os = "macos")))]
        let target = window.gtk_window();
        #[cfg(not(all(unix, not(target_os = "macos"))))]
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
fn start_watcher(app: &tauri::AppHandle) -> Option<coxswain_core::DirWatcher> {
    use notify::Watcher;
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let watcher = coxswain_core::DirWatcher::new(tx, notify::Config::default()).ok()?;
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
    // In a Flatpak, on the host: the user's commands and editor are there.
    let mut c = coxswain_core::tools::user_command(sh);
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
    /// Index into `user_menu::entries`, or a script file, or neither: "Add your own command".
    user: Option<usize>,
    path: Option<PathBuf>,
    /// Show the output even when there is none.
    wait: bool,
}

fn scripts_dir() -> Option<PathBuf> {
    Config::path().and_then(|p| Some(p.parent()?.join("scripts")))
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
    for p in scripts_dir().map(|d| script_files(&d)).unwrap_or_default() {
        let label = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        out.push(Script { key: String::new(), label, user: None, path: Some(p), wait: true });
    }
    let key = if out.iter().any(|s| s.key == "+") { "" } else { "+" };
    out.push(Script { key: key.into(), label: coxswain_core::t!("usermenu.add"), user: None, path: None, wait: false });
    out
}

/// The scripts in `dir`, by name: the files that can run (executable on Unix; any file on
/// Windows, which decides by the extension), and not hidden ones (`.DS_Store`).
fn script_files(dir: &Path) -> Vec<PathBuf> {
    let runs = |p: &Path| -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
        }
        #[cfg(not(unix))]
        true
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|d| d.path())
        .filter(|p| p.is_file() && !p.file_name().unwrap_or_default().to_string_lossy().starts_with('.') && runs(p))
        .collect();
    files.sort();
    files
}

/// Run a user-menu entry (expanded) or a script file with the selection as arguments.
#[tauri::command]
async fn run_script(user: Option<usize>, path: Option<PathBuf>, dir: PathBuf, file: Option<PathBuf>, selected: Vec<PathBuf>, ctx: tauri::State<'_, Ctx>) -> Res<String> {
    if let Some(i) = user {
        let list = coxswain_core::user_menu::entries(&ctx.cfg(), &dir, file.as_deref());
        let cmd = list.get(i).ok_or_else(|| coxswain_core::t!("err.no_such_command"))?.expand(&dir, file.as_deref(), &selected);
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
    let mut c = coxswain_core::tools::user_command(&real);
    c.args(args);
    output(c, &dir)
}

/// F2's "Add your own command": config.toml in `editor` at `[[user_menu]]` (an example
/// written there first), or with the desktop's default when no editor is set.
#[tauri::command]
async fn add_user_command(ctx: tauri::State<'_, Ctx>) -> Res<()> {
    let (path, line) = blocking(coxswain_core::user_menu::prepare).await?;
    let Some(ed) = ctx.cfg().editor.clone() else { return open_path(path).await };
    let cmd = coxswain_core::user_menu::edit_command(&ed, &path, line);
    tauri::async_runtime::spawn_blocking(move || coxswain_core::tools::spawn_watched(shell(&cmd), std::time::Duration::from_secs(1)).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
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
/// tries to load another stays where it is.
fn local(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" | "asset" | "about" | "blob" | "data" => true,
        "http" | "https" => matches!(url.host_str(), Some("tauri.localhost" | "asset.localhost" | "ipc.localhost")) || (cfg!(debug_assertions) && url.host_str() == Some("localhost")),
        _ => false,
    }
}

fn main() {
    coxswain_core::tools::flatpak_tmp();
    // Started by an app, not by hand: hold the file name index for all of them. No window.
    if std::env::args().nth(1).as_deref() == Some(helper::ARG) {
        return drop(helper::serve());
    }
    coxswain_core::fs::lock_down();
    // A config.toml of 1.x gets the names of 2.0, once.
    if let Err(e) = coxswain_core::migrate::on_start() {
        eprintln!("coxswain: {e}");
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
    // pandoc and the rest of Homebrew are invisible.
    if cfg!(target_os = "macos") {
        coxswain_core::tools::login_path();
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
        guide: AppState::load().guide_due(),
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
        chat: Mutex::default(),
        measuring: Mutex::default(),
        asking: Arc::default(),
    };
    tauri::Builder::default()
        .manage(ctx)
        // Files for the page come through our own `asset:`, which keeps cloud files in the cloud.
        .register_asynchronous_uri_scheme_protocol("asset", |_, req, responder| {
            tauri::async_runtime::spawn_blocking(move || responder.respond(asset::respond(&req, coxswain_core::cloud::unasked)));
        })
        .setup(|app| {
            *app.state::<Ctx>().watcher.lock().expect("fresh lock") = start_watcher(app.handle());
            // The window from the config, made here so that it gets its navigation guard.
            let window = app.config().app.windows.first().cloned().ok_or("no window in the config")?;
            tauri::WebviewWindowBuilder::from_config(app.handle(), &window)?.on_navigation(local).build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config, action_menu, hint, hints_reset, notices, dismiss_notice, changes, read_changes, set_title, index_status, index_action, index_service, meaning_status, meaning_action, chat_status, chat_action, meaning_models, meaning_pull, list_dir, zfs_facts, bsd_places, git_status, git_last, git_switch, git_new_branch, places, disks, get_state, save_session, save_favorites, set_tags, set_note, get_note,
            find, find_read_too, ask, ask_stop, ask_check, meaning_change, setup_probe, setup_probe_url, setup_try, setup_speed, resolve_path, copy, rename, delete, mkdir, dir_sizes, rename_plan, rename_apply, open_path, edit_path,
            read_text, run_command, scripts, run_script, add_user_command, check_update, archive_list, extract, pack, archive_password, archive_peek, cloud_fetch, properties, set_flags, set_permissions,
            clip_set, paste, start_drag, watch_dirs, preview::git_diff, preview::sqlite_info, preview::epub_preview,
            preview::file_facts, preview::cert_info, bom::bom_info, bom::bom_node, bom::bom_diff, provenance::provenance_info, provenance::provenance_statements, provenance::provenance_subject, provenance::provenance_cancel, provenance::provenance_sources, provenance::provenance_diff, provenance::provenance_bom, preview::mail_preview, preview::plist_xml, convert::preview_engines, convert::preview_cache, convert::clear_preview_cache,
            convert::convert, convert::images, convert::pull_image, convert::remove_image, convert::pull_progress, dupes_scan,
            dupes_progress, dupes_cancel, save_settings, search_status, search_level, guide_seen, nerd_font, copy_text
        ])
        .run(tauri::generate_context!())
        .expect("error while running Coxswain");
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(ok("tauri://localhost/index.html") && ok("http://tauri.localhost/index.html") && ok("asset://localhost/%2Fhome%2Fme%2Fa.pdf") && ok("http://asset.localhost/C%3A/a.pdf") && ok("about:srcdoc"));
        assert!(!ok("https://example.com/") && !ok("http://example.com/") && !ok("file:///etc/passwd") && !ok("javascript:alert(1)"));
    }

    #[test]
    fn drives_show_removable_disks_under_run_media() {
        assert!(!hidden_mount(Path::new("/run/media/me/USB")));
        assert!(hidden_mount(Path::new("/run/user/1000")));
        assert!(hidden_mount(Path::new("/boot/efi")));
        assert!(!hidden_mount(Path::new("/mnt/data")));
        assert!(hidden_mount(Path::new("/tank/home/.zfs/snapshot/daily")));
    }

    #[test]
    #[cfg(unix)]
    fn scripts_are_the_executable_files_only() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-scripts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("README.md"), "").unwrap();
        std::fs::write(d.join(".DS_Store"), "").unwrap();
        std::fs::write(d.join("resize"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(d.join("resize"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(d.join(".hidden"), "").unwrap();
        std::fs::set_permissions(d.join(".hidden"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(script_files(&d), vec![d.join("resize")]);
        std::fs::remove_dir_all(d).unwrap();
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
        let (prefix, items) = to_page(&dir, entries, |_| None, None);
        let listing = Listing { has_notes: false, dir, prefix, items, archive: None, locked: false, history: None, snapshot: None, package: None };
        println!("list_dir body (list, sort, icons): {:.0} ms", ms(t));
        let t = std::time::Instant::now();
        let json = serde_json::to_vec(&listing).unwrap();
        println!("list_dir to JSON: {:.0} ms, {:.1} MB for {} items", ms(t), json.len() as f64 / 1e6, listing.items.len());
        std::fs::write(bench.join("listing.json"), json).unwrap();
    }
}

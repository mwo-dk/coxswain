//! Disk use, in both apps (Settings → Privacy and updates → Disk use) and `coxswain --disk`:
//! every place Coxswain keeps things in its cache folder, the built-in models, and what it
//! caused outside its folder (Tectonic's, Typst's and kicad-cli's caches, container images
//! pulled for previews), each with
//! its size, where it is, what clearing it costs, and a way to clear it. Clearing the search
//! store or the name index while the search helper runs goes through the helper, so a store in
//! use is never cut from under it.
//!
//! Here too: what F3 and the preview show of Coxswain's own `index.bin` and `search.db`, read
//! without writing anything.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{Config, PreviewConfig};
use crate::helper::Client;
use crate::settings::{count, human};
use crate::t;

/// Whose a row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// In Coxswain's cache folder.
    Own,
    /// A built-in model (Settings → Finding files → Built-in models has the same).
    Model,
    /// Outside Coxswain's folder, caused by it: never cleared without being asked by name.
    Outside,
    /// The user's own files Coxswain keeps aside (quarantine, undo copies, what a sync set
    /// aside): never in *Clear everything*, each removed only on its own row.
    Yours,
}

/// One place on the disk.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Item {
    /// What `--disk clear` takes: `store`, `index`, …, a model's folder name, `tectonic`,
    /// `typst`, `kicad`, `image:<name>`.
    pub id: String,
    /// As the user sees it: `search.db`, `previews/`, `Qwen3 4B`, the image's name.
    pub name: String,
    pub what: String,
    /// What clearing costs.
    pub cost: String,
    pub bytes: u64,
    /// The files and folders it is; none for a container image.
    pub paths: Vec<PathBuf>,
    pub kind: Kind,
    /// It comes back by itself: what *Clear everything that can be built again* clears.
    pub rebuildable: bool,
    /// A built-in model the settings use: deleting it changes them.
    pub in_use: bool,
    /// It has a Clear (or Remove) button; the quarantine has none, it is shown only.
    pub clear: bool,
}

/// The rows of Coxswain's own folder: id, what they are in the cache folder, built again by
/// itself. `inside` is every `inside-<n>` folder.
const OWN: &[(&str, &[&str], bool)] = &[
    (
        "store",
        &["search.db", "search.db-wal", "search.db-shm"],
        true,
    ),
    ("index", &["index.bin", "index.tmp"], true),
    ("previews", &["previews"], true),
    ("peek", &["peek", "nested", "archive-index"], true),
    ("inside", &[], true),
    (
        "libreoffice",
        &["libreoffice-profile", "libreoffice-index-profile"],
        true,
    ),
    ("log", &["helper.log"], false),
    // ClamAV's virus database (`viruses`), fetched again before the next scan.
    ("clamav", &["clamav"], true),
    // The work of conversions, scans and drags, one folder per run (`spill::RUN_FOLDERS`).
    (
        "work",
        &["scratch", "media", "geo", "ocr", "export", "clamscan", "drag", "provenance"],
        true,
    ),
    // Chess analyses kept, and ChessBase archives unpacked to be read.
    ("chess", &["chess", "cbv"], true),
];

/// What `--disk clear` takes besides the rows' ids.
pub const ALL_IMAGES: &str = "images";
pub const REBUILDABLE: &str = "rebuildable";

fn size(paths: &[PathBuf]) -> u64 {
    paths.iter().map(|p| crate::fs::dir_size(p).0).sum()
}

/// The `inside-<n>` folders of the helpers that read archives' text.
fn inside_folders(cache: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(cache)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("inside-") && e.path().is_dir())
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

/// The rows of the cache folder, the built-in models and Tectonic's cache; with `images` also
/// the container images (which asks podman or docker, a moment each).
pub fn items(cfg: &Config, loaded: &crate::models::Loaded, images: bool) -> Vec<Item> {
    let mut v = vec![];
    if let Some(cache) = crate::helper::folder() {
        for (id, names, rebuildable) in OWN {
            let paths: Vec<PathBuf> = if *id == "inside" {
                inside_folders(&cache)
            } else {
                names
                    .iter()
                    .map(|n| cache.join(n))
                    .filter(|p| p.exists() || p.ends_with(names[0]))
                    .collect()
            };
            if *id == "inside" && paths.is_empty() {
                continue;
            }
            let name = match *id {
                "inside" => "inside-…/".to_string(),
                _ => format!(
                    "{}{}",
                    names[0],
                    if names[0].contains('.') { "" } else { "/" }
                ),
            };
            v.push(Item {
                id: id.to_string(),
                name,
                what: t!(&format!("disk.{id}.what")),
                cost: t!(&format!("disk.{id}.cost")),
                bytes: size(&paths),
                paths,
                kind: Kind::Own,
                rebuildable: *rebuildable,
                clear: true,
                in_use: false,
            });
        }
    }
    // The text of commits' changes inside search.db, when kept: turned off, the room comes back.
    if let Some(bytes) = crate::store::Store::path().map(|p| crate::store::changes_bytes(&p)).filter(|b| *b > 0) {
        v.push(Item {
            id: "changes".into(),
            name: t!("disk.changes.name"),
            what: t!("disk.changes.what"),
            cost: t!("disk.changes.cost"),
            bytes,
            paths: vec![],
            kind: Kind::Own,
            rebuildable: false,
            in_use: false,
        });
    }
    for e in crate::models::list(&cfg.search, loaded) {
        v.push(Item {
            id: e.id.clone(),
            name: e.name.clone(),
            what: e.summary.clone(),
            cost: t!("disk.model.cost"),
            bytes: e.bytes,
            paths: vec![e.folder.clone()],
            kind: Kind::Model,
            rebuildable: false,
            clear: true,
            in_use: e.in_use,
        });
    }
    for (tool, p) in made_outside().into_iter().filter(|(t, _)| t.id != KICAD_SETTINGS.id) {
        v.push(Item {
            id: tool.id.into(),
            name: tool.name.into(),
            what: t!(tool.what),
            cost: t!(tool.cost),
            bytes: size(std::slice::from_ref(&p)),
            paths: vec![p],
            kind: Kind::Outside,
            rebuildable: false,
            clear: true,
            in_use: false,
        });
    }
    if images {
        v.extend(image_items(&cfg.preview));
    }
    v.extend(yours());
    v
}

/// The user's own files Coxswain keeps aside: the quarantine (shown, never cleared here),
/// find-and-replace's undo copies (only those of runs that ended go), and each folder a sync
/// that ended set aside where the bin could not take it.
fn yours() -> Vec<Item> {
    let mut v = vec![];
    let row = |id: String, name: String, what: String, cost: String, paths: Vec<PathBuf>, clear: bool| Item {
        id,
        name,
        what,
        cost,
        bytes: size(&paths),
        paths,
        kind: Kind::Yours,
        rebuildable: false,
        in_use: false,
        clear,
    };
    if let Some(q) = crate::viruses::quarantine_dir().filter(|q| std::fs::read_dir(q).is_ok_and(|mut r| r.next().is_some())) {
        v.push(row("quarantine".into(), "quarantine/".into(), t!("disk.quarantine.what"), t!("disk.quarantine.cost"), vec![q], false));
    }
    // kicad-cli's settings, which an installed KiCad shares: shown, never cleared.
    for (_, p) in made_outside().into_iter().filter(|(t, _)| t.id == KICAD_SETTINGS.id) {
        v.push(row(KICAD_SETTINGS.id.into(), "kicad/".into(), t!(KICAD_SETTINGS.what), t!(KICAD_SETTINGS.cost), vec![p], false));
    }
    let undo = crate::replace::kept_dirs();
    if !undo.is_empty() {
        v.push(row("replace".into(), "coxswain-replace-…/".into(), t!("disk.replace.what"), t!("disk.replace.cost"), undo, true));
    }
    for l in crate::compare::aside::left().into_iter().filter(|l| !l.binned) {
        let p = l.path();
        if !std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let name = format!("{}/", p.file_name().unwrap_or_default().to_string_lossy());
        let what = t!("disk.sync.what", "n" => count(l.files as usize), "folder" => l.folder().display());
        v.push(row(format!("sync:{}", name.trim_end_matches('/')), name, what, t!("disk.sync.cost"), vec![p], true));
    }
    v
}

/// The container images of `[preview] images` that podman or docker has, by their exact names;
/// and a former default no tool is set to any more (`images::FORMER`), said as such, to clear.
fn image_items(cfg: &PreviewConfig) -> Vec<Item> {
    crate::tools::container_runtime(cfg).map_or_else(Vec::new, |(name, rt)| images_in(cfg, name, &rt))
}

fn images_in(cfg: &PreviewConfig, name: &str, rt: &std::path::Path) -> Vec<Item> {
    let mut seen = std::collections::HashSet::new();
    let former = crate::images::FORMER.iter().map(|(image, tool)| (tool.to_string(), image.to_string(), true));
    cfg.images
        .iter()
        .map(|(tool, image)| (tool.clone(), image.clone(), false))
        .chain(former)
        .filter(|(_, image, _)| !image.is_empty() && seen.insert(image.to_string()))
        .filter_map(|(tool, image, former)| {
            let bytes = crate::tools::image_size(rt, &image)?;
            Some(Item {
                id: format!("image:{image}"),
                name: image.clone(),
                what: if former {
                    t!("disk.image.former", "tool" => tool)
                } else {
                    crate::images::about(&tool, &image)
                        .unwrap_or_else(|| t!("disk.image.what", "tool" => tool, "runtime" => name))
                },
                cost: t!(if former { "disk.image.former_cost" } else { "disk.image.cost" }),
                bytes,
                paths: vec![],
                kind: Kind::Outside,
                rebuildable: false,
                clear: true,
                in_use: false,
            })
        })
        .collect()
}

/// The rows `name` names on the command line: an id, a model as `--models` names it, `images`.
pub fn find<'a>(items: &'a [Item], name: &str) -> Vec<&'a Item> {
    if name == ALL_IMAGES {
        return items
            .iter()
            .filter(|i| i.id.starts_with("image:"))
            .collect();
    }
    if let Some(i) = items
        .iter()
        .find(|i| i.id == name || i.id == format!("image:{name}"))
    {
        return vec![i];
    }
    items
        .iter()
        .filter(|i| i.kind == Kind::Model && i.name.eq_ignore_ascii_case(name))
        .take(1)
        .collect()
}

/// Bytes *Clear everything that can be built again* frees: Coxswain's own rows, and with
/// `models` the built-in models not in use, with `outside` what it caused outside its folder.
pub fn clearable_bytes(items: &[Item], models: bool, outside: bool) -> u64 {
    items
        .iter()
        .filter(|i| goes(i, models, outside))
        .map(|i| i.bytes)
        .sum()
}

fn goes(i: &Item, models: bool, outside: bool) -> bool {
    match i.kind {
        Kind::Own => i.rebuildable,
        Kind::Model => models && !i.in_use,
        Kind::Outside => outside,
        Kind::Yours => false,
    }
}

fn remove(p: &Path) -> std::io::Result<()> {
    let r = if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    };
    match r {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Clear `item`: the bytes it freed. The search store is emptied by the helper while one runs;
/// the name index is deleted and a new helper builds it again. A built-in model in use is not
/// deleted here: Settings → Built-in models (or `--models delete`) says what comes instead.
pub fn clear(item: &Item, cfg: &Config, helper: &Client) -> Result<u64, String> {
    let fail = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let gone = |paths: &[PathBuf]| -> Result<(), String> {
        paths
            .iter()
            .try_for_each(|p| remove(p).map_err(|e| fail(p, e)))
    };
    match (item.kind, item.id.as_str()) {
        (Kind::Own, "store") if crate::helper::runs() && cfg.search.text => helper.forget(),
        // Turned off: the helper takes them out when it starts, and gives the room back.
        (Kind::Own, "changes") => {
            let off = serde_json::json!({ "search_commit_changes": false, "search_commit_changes_in": [] });
            crate::settings::save(off.as_object().ok_or("changes")?)?;
            if crate::helper::runs() {
                helper.restart();
            }
            return Ok(item.bytes);
        }
        (Kind::Own, "index") => {
            gone(&item.paths)?;
            if crate::helper::runs() {
                helper.restart();
            }
        }
        // A helper reading an archive now has its folder; one left by a helper that stopped is
        // a day old at least, or no helper runs.
        (Kind::Own, "inside") => {
            let running = crate::helper::runs();
            let old = |p: &Path| {
                std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t.elapsed().unwrap_or_default().as_secs() > 24 * 3600)
            };
            let left: Vec<PathBuf> = item
                .paths
                .iter()
                .filter(|p| !running || old(p))
                .cloned()
                .collect();
            gone(&left)?;
        }
        // The helper writes to it: emptied, not deleted.
        (Kind::Own, "log") => {
            for p in &item.paths {
                if let Ok(f) = std::fs::OpenOptions::new().write(true).open(p) {
                    f.set_len(0).map_err(|e| fail(p, e))?;
                }
            }
        }
        // A folder of runs' folders: only those of runs that ended, so what a running app or
        // helper is working on now stays (`spill::in_use`).
        (Kind::Own, _) => {
            for p in &item.paths {
                if p.file_name().is_some_and(|n| crate::spill::by_run(&n.to_string_lossy())) {
                    crate::spill::clear_ended_in(p).map_err(|e| fail(p, e))?;
                } else {
                    remove(p).map_err(|e| fail(p, e))?;
                }
            }
        }
        (Kind::Model, _) => {
            let all = crate::models::list(&cfg.search, &helper.loaded());
            let e = all
                .iter()
                .find(|e| e.id == item.id)
                .ok_or_else(|| t!("models.no_such", "name" => item.id.as_str()))?;
            if e.in_use {
                return Err(t!("disk.model_in_use", "model" => e.name.as_str()));
            }
            crate::models::delete(e, helper).map_err(|err| fail(&e.folder, err))?;
        }
        (Kind::Yours, "replace") => crate::replace::sweep(),
        (Kind::Yours, id) if id.starts_with("sync:") => {
            for p in &item.paths {
                crate::compare::aside::remove(p).map_err(|e| fail(p, e))?;
            }
        }
        // Your own files: removed in a panel, never here.
        (Kind::Yours, _) => return Err(item.cost.clone()),
        (Kind::Outside, id) if !id.starts_with("image:") => {
            gone(&item.paths)?;
            forget_made(&item.paths);
        }
        (Kind::Outside, _) => {
            let image = item.id.trim_start_matches("image:");
            let (_, rt) = crate::tools::container_runtime(&cfg.preview)
                .ok_or_else(crate::tools::no_runtime)?;
            // Only what no other image shares is freed.
            return Ok(crate::images::remove_freeing(&rt, image)?.unwrap_or(item.bytes));
        }
    }
    Ok(item.bytes.saturating_sub(size(&item.paths)))
}

/// *Clear everything that can be built again*: the bytes freed, and what could not be cleared.
pub fn clear_all(
    items: &[Item],
    models: bool,
    outside: bool,
    cfg: &Config,
    helper: &Client,
) -> (u64, Vec<String>) {
    let (mut freed, mut failed) = (0, vec![]);
    for i in items.iter().filter(|i| goes(i, models, outside)) {
        match clear(i, cfg, helper) {
            Ok(n) => freed += n,
            Err(e) => failed.push(format!("{}: {e}", i.name)),
        }
    }
    (freed, failed)
}

/// What clearing said: "search.db cleared: 284 KB freed." An image goes by its name without
/// its digest, so the size freed still fits on a terminal's line.
pub fn cleared(name: &str, freed: u64) -> String {
    t!("disk.cleared", "name" => crate::images::short(name), "size" => human(freed))
}

// ---------------------------------------------------------------- outside the cache folder

/// Where Coxswain notes what it made outside its folder, so it clears nothing it did not make.
fn made_file() -> Option<PathBuf> {
    Some(crate::helper::folder()?.join("made-outside.txt"))
}

/// A tool whose cache Coxswain may cause outside its folder, by running it for a preview.
pub struct Tool {
    /// The row's id for `--disk clear`.
    pub id: &'static str,
    name: &'static str,
    /// The keys of its texts.
    what: &'static str,
    cost: &'static str,
    caches: fn() -> Vec<PathBuf>,
}

/// Tectonic's TeX packages, Typst's packages, kicad-cli's cache; and kicad-cli's settings
/// (`~/.config/kicad`), listed with the user's own files and never cleared: an installed KiCad
/// keeps the user's settings there.
pub const TECTONIC: Tool = Tool {
    id: "tectonic",
    name: "Tectonic",
    what: "disk.tectonic.what",
    cost: "disk.tectonic.cost",
    caches: tectonic_caches,
};
pub const TYPST: Tool = Tool {
    id: "typst",
    name: "Typst",
    what: "disk.typst.what",
    cost: "disk.typst.cost",
    caches: typst_caches,
};
pub const KICAD: Tool = Tool {
    id: "kicad",
    name: "KiCad",
    what: "disk.kicad.what",
    cost: "disk.kicad.cost",
    caches: kicad_caches,
};
pub const KICAD_SETTINGS: Tool = Tool {
    id: "kicad-settings",
    name: "kicad/",
    what: "disk.kicad_settings.what",
    cost: "disk.kicad_settings.cost",
    caches: kicad_settings,
};
const TOOLS: [&Tool; 4] = [&TECTONIC, &TYPST, &KICAD, &KICAD_SETTINGS];

/// The folders noted, those still there, with the tool whose they are.
fn made_outside() -> Vec<(&'static Tool, PathBuf)> {
    let text = made_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .unwrap_or_default();
    text.lines()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let tool = TOOLS.into_iter().find(|t| (t.caches)().contains(&p))?;
            Some((tool, p))
        })
        .collect()
}

fn forget_made(paths: &[PathBuf]) {
    let Some(f) = made_file() else { return };
    let text = std::fs::read_to_string(&f).unwrap_or_default();
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| !paths.iter().any(|p| Path::new(l) == p))
        .collect();
    let _ = if kept.is_empty() {
        std::fs::remove_file(&f)
    } else {
        std::fs::write(&f, kept.join("\n") + "\n")
    };
}

/// Where Tectonic may keep its cache: `TECTONIC_CACHE_DIR`, else its folder in the system's
/// cache folder (`~/.cache/tectonic`, `~/Library/Caches/Tectonic`,
/// `%LOCALAPPDATA%\TectonicProject\Tectonic`).
fn tectonic_caches() -> Vec<PathBuf> {
    if let Some(d) = std::env::var_os("TECTONIC_CACHE_DIR").filter(|d| !d.is_empty()) {
        return vec![d.into()];
    }
    let Some(base) = crate::dirs::cache_dir() else {
        return vec![];
    };
    // Newer tectonic names it in lower case; Windows has the project's folder around it.
    vec![
        base.join("tectonic"),
        base.join("Tectonic"),
        base.join("TectonicProject").join("Tectonic"),
    ]
}

/// Typst's package cache: `TYPST_PACKAGE_CACHE_PATH`, else `typst/` in the system's cache
/// folder.
fn typst_caches() -> Vec<PathBuf> {
    if let Some(d) = std::env::var_os("TYPST_PACKAGE_CACHE_PATH").filter(|d| !d.is_empty()) {
        return vec![d.into()];
    }
    crate::dirs::cache_dir().map(|b| b.join("typst")).into_iter().collect()
}

/// kicad-cli's cache: `kicad/` in the system's cache folder.
fn kicad_caches() -> Vec<PathBuf> {
    crate::dirs::cache_dir().map(|b| b.join("kicad")).into_iter().collect()
}

/// kicad-cli's settings: `kicad/` in the system's preferences folder (`~/.config/kicad`,
/// `~/Library/Preferences/kicad`, `%APPDATA%\kicad`).
fn kicad_settings() -> Vec<PathBuf> {
    crate::dirs::preference_dir().map(|b| b.join("kicad")).into_iter().collect()
}

/// The folders of `tool` that are there now: for one listed when a preview used it, made by it
/// or not (`note_made`).
pub fn present(tool: &Tool) -> Vec<PathBuf> {
    (tool.caches)().into_iter().filter(|p| p.is_dir()).collect()
}

/// Before Coxswain runs `tool`: the places of its cache that are not there yet. Give them to
/// `note_made` afterwards, which notes those that came.
pub fn absent(tool: &Tool) -> Vec<PathBuf> {
    (tool.caches)().into_iter().filter(|p| !p.exists()).collect()
}

/// Note the folders of `absent` that are there now: Coxswain made them.
pub fn note_made(absent: &[PathBuf]) {
    let made: Vec<&PathBuf> = absent.iter().filter(|p| p.is_dir()).collect();
    let Some(f) = made_file().filter(|_| !made.is_empty()) else {
        return;
    };
    let mut text = std::fs::read_to_string(&f).unwrap_or_default();
    let had = text.clone();
    for p in made {
        // Once each, however often a preview runs.
        if !had.lines().any(|l| Path::new(l) == p.as_path()) {
            text += &format!("{}\n", p.display());
        }
    }
    if text == had {
        return;
    }
    let _ = std::fs::write(f, text);
}

// ---------------------------------------------------------------- the stores, previewed

/// What the preview and F3 show of Coxswain's own `index.bin` or `search.db`.
#[derive(Debug, Serialize)]
pub struct Report {
    /// The row of Disk use it is: `index` or `store`.
    pub item: &'static str,
    pub title: String,
    /// The one Coxswain uses (in its cache folder), not a copy elsewhere: it can be cleared.
    pub live: bool,
    pub sections: Vec<Section>,
}

#[derive(Debug, Serialize)]
pub struct Section {
    pub title: String,
    /// Label and value; a value may be empty (a folder in a list).
    pub rows: Vec<(String, String)>,
}

fn section(title: String, rows: Vec<(String, String)>) -> Section {
    Section { title, rows }
}

fn same_file(a: &Path, b: Option<PathBuf>) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    b.is_some_and(|b| canon(a) == canon(&b))
}

/// `path` as Coxswain's name index or search store, wherever it is; `None` for any other file.
pub fn describe(path: &Path) -> Option<Report> {
    let mut head = [0u8; 16];
    std::io::Read::read_exact(&mut std::fs::File::open(path).ok()?, &mut head).ok()?;
    // A store's write-ahead log is part of it.
    let bytes = ["", "-wal", "-shm"]
        .iter()
        .filter_map(|x| std::fs::metadata(format!("{}{x}", path.display())).ok())
        .map(|m| m.len())
        .sum();
    if crate::index::is_index(&head) {
        return Some(index_report(
            path,
            bytes,
            crate::index::Index::facts(path).ok()?,
        ));
    }
    if &head == b"SQLite format 3\0" {
        return Some(store_report(path, bytes, crate::store::facts(path).ok()?));
    }
    None
}

fn here_row(path: &Path, live: bool, ours: Option<PathBuf>) -> (String, String) {
    let value = if live {
        t!("disk.report.live")
    } else {
        t!("disk.report.copy", "path" => ours.map(|p| p.display().to_string()).unwrap_or_default())
    };
    (
        t!("disk.report.here"),
        format!("{} · {value}", path.display()),
    )
}

fn index_report(path: &Path, bytes: u64, f: crate::index::Facts) -> Report {
    let ours = crate::index::Index::cache_path();
    let live = same_file(path, ours.clone());
    let mut first = vec![
        (
            t!("disk.report.entries"),
            t!("disk.report.entries_value", "all" => count(f.files + f.folders), "files" => count(f.files), "folders" => count(f.folders)),
        ),
        (
            t!("disk.report.built"),
            match f.built {
                Some((at, ms)) => {
                    t!("disk.report.built_value", "when" => crate::i18n::date_time(at), "seconds" => crate::i18n::digits(if ms < 1000 { format!("{:.2}", ms as f64 / 1000.0) } else { format!("{:.1}", ms as f64 / 1000.0) }))
                }
                None => t!("disk.report.built_unknown"),
            },
        ),
        (t!("disk.report.size"), human(bytes)),
        here_row(path, live, ours),
    ];
    if f.archives > 0 {
        first.insert(1, (t!("disk.report.archives"), t!("disk.report.archives_value", "archives" => count(f.archives), "entries" => count(f.inside))));
    }
    let mut left = vec![(
        t!("disk.report.name_exclude"),
        if f.exclude.is_empty() {
            t!("disk.report.none")
        } else {
            f.exclude.join(", ")
        },
    )];
    left.extend(
        f.nosearch
            .iter()
            .map(|p| (p.display().to_string(), t!("disk.report.nosearch"))),
    );
    // Files only in the cloud are named here; the store knows which, and leaves their text.
    if live
        && let Some(s) = crate::store::Store::path()
            .and_then(|p| crate::store::facts(&p).ok())
            .filter(|s| s.cloud > 0)
    {
        left.push((
            t!("disk.report.cloud"),
            t!("disk.report.cloud_value", "n" => count(s.cloud)),
        ));
    }
    Report {
        item: "index",
        title: t!("disk.report.index_title"),
        live,
        sections: vec![
            section(t!("disk.report.index_title"), first),
            section(
                t!("disk.report.roots"),
                f.roots
                    .iter()
                    .map(|r| (r.display().to_string(), String::new()))
                    .collect(),
            ),
            section(
                t!("disk.report.largest"),
                f.largest
                    .iter()
                    .map(|(p, n)| (p.display().to_string(), count(*n)))
                    .collect(),
            ),
            section(t!("disk.report.left_out"), left),
        ],
    }
}

fn store_report(path: &Path, bytes: u64, f: crate::store::Facts) -> Report {
    let ours = crate::store::Store::path();
    let live = same_file(path, ours.clone());
    let first = vec![
        (t!("disk.report.files_known"), count(f.files)),
        (t!("disk.report.files_read"), count(f.read)),
        (t!("disk.report.waiting"), count(f.waiting)),
        (t!("disk.report.no_text"), count(f.no_text)),
        (t!("disk.report.cloud"), count(f.cloud)),
        (
            t!("disk.report.last_read"),
            f.read_at
                .map(crate::i18n::date_time)
                .unwrap_or_else(|| t!("disk.report.not_yet")),
        ),
        (t!("disk.report.size"), human(bytes)),
        here_row(path, live, ours),
    ];
    let mut kinds: Vec<(String, String)> = f
        .kinds
        .iter()
        .take(12)
        .map(|(ext, n)| {
            (
                if ext.is_empty() {
                    t!("disk.report.no_ext")
                } else {
                    format!(".{ext}")
                },
                count(*n),
            )
        })
        .collect();
    if f.kinds.len() > 12 {
        let rest: usize = f.kinds[12..].iter().map(|k| k.1).sum();
        kinds.push((
            t!("disk.report.other_kinds", "n" => f.kinds.len() - 12),
            count(rest),
        ));
    }
    if f.commits > 0 {
        kinds.push((t!("disk.report.commits"), count(f.commits)));
    }
    if f.changes > 0 {
        kinds.push((t!("disk.report.changes"), format!("{} · {}", count(f.changes), human(f.changes_bytes))));
    }
    if f.inside > 0 {
        kinds.push((t!("disk.report.inside"), count(f.inside)));
    }
    let meaning = vec![
        (
            t!("disk.report.passages"),
            t!("disk.report.passages_value", "passages" => count(f.passages), "files" => count(f.with_vectors)),
        ),
        (
            t!("disk.report.model"),
            if f.model.is_empty() {
                t!("disk.report.none")
            } else {
                f.model.clone()
            },
        ),
        (t!("disk.report.vectors_waiting"), count(f.vectors_waiting)),
        (t!("disk.report.vectors_changing"), count(f.vectors_changing)),
    ];
    let parts = f
        .parts
        .iter()
        .map(|(p, b)| (t!(&format!("disk.report.part.{p}")), human(*b)))
        .collect();
    let errors = if f.errors.is_empty() {
        vec![(t!("disk.report.no_errors"), String::new())]
    } else {
        f.errors
            .iter()
            .map(|(at, p, what)| {
                (
                    format!(
                        "{} {}",
                        crate::i18n::date_time(*at),
                        p.as_deref().unwrap_or("")
                    ),
                    crate::store::shown_error(what),
                )
            })
            .collect()
    };
    Report {
        item: "store",
        title: t!("disk.report.store_title"),
        live,
        sections: vec![
            section(t!("disk.report.store_title"), first),
            section(t!("disk.report.kinds"), kinds),
            section(t!("disk.report.meaning"), meaning),
            section(t!("disk.report.parts"), parts),
            section(t!("disk.report.errors"), errors),
        ],
    }
}

impl Report {
    /// As text, for the terminal app's pager: the sections, then where Settings has it.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for s in &self.sections {
            out += &format!("{}\n", s.title);
            let w = s
                .rows
                .iter()
                .map(|(l, _)| l.chars().count())
                .max()
                .unwrap_or(0)
                .min(48);
            for (l, v) in &s.rows {
                let pad = w.saturating_sub(l.chars().count());
                out += &format!("  {l}{}  {v}\n", " ".repeat(pad));
            }
            out.push('\n');
        }
        out += &t!("disk.report.settings_tui");
        out.push('\n');
        if self.live {
            out += &t!("disk.report.clear_tui", "item" => self.item);
            out.push('\n');
        }
        out
    }
}

/// `report` of `path` as text in a file of the cache, for the pager: F3.
pub fn text_copy(path: &Path, report: &Report) -> Result<PathBuf, String> {
    let to = crate::archive::peek_folder()
        .map_err(|e| e.to_string())?
        .join(format!(
            "{}.txt",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
    std::fs::write(&to, report.text()).map_err(|e| e.to_string())?;
    Ok(to)
}

/// The rows as `coxswain --disk` prints them.
pub fn table(items: &[Item]) -> String {
    let mut out = String::new();
    for (kind, head) in [
        (Kind::Own, "disk.head.own"),
        (Kind::Model, "disk.head.models"),
        (Kind::Outside, "disk.head.outside"),
        (Kind::Yours, "disk.head.yours"),
    ] {
        let rows: Vec<&Item> = items.iter().filter(|i| i.kind == kind).collect();
        if rows.is_empty() {
            continue;
        }
        out += &format!("{}\n", t!(head));
        for i in rows {
            let (pad, id) = (" ".repeat(29), i.id.trim_start_matches("image:"));
            out += &format!(
                "  {id:<16} {:>9}  {}\n",
                human(i.bytes),
                if i.name == id { "" } else { &i.name }
            );
            if let Some(p) = i.paths.first() {
                out += &format!("{pad} {}\n", p.display());
            }
            out += &format!(
                "{pad} {}\n{pad} {}: {}\n",
                i.what,
                t!("disk.cost_label"),
                i.cost
            );
        }
        out.push('\n');
    }
    out += &t!("disk.rebuildable_cli", "size" => human(clearable_bytes(items, false, false)));
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder noted again at each preview is listed once; KiCad's settings are shown under
    /// your own files and never cleared.
    #[test]
    fn kicad_settings_are_noted_once_and_never_cleared() {
        let d = kicad_settings().remove(0);
        std::fs::create_dir_all(&d).unwrap();
        note_made(&present(&KICAD_SETTINGS));
        note_made(&present(&KICAD_SETTINGS));
        let text = std::fs::read_to_string(made_file().unwrap()).unwrap();
        assert_eq!(text.lines().filter(|l| Path::new(l) == d).count(), 1, "{text}");
        let row = yours().into_iter().find(|i| i.id == KICAD_SETTINGS.id).expect("listed");
        assert!(!row.clear && row.kind == Kind::Yours && !goes(&row, true, true));
        assert!(made_outside().iter().any(|(t, p)| t.id == KICAD_SETTINGS.id && *p == d));
        forget_made(std::slice::from_ref(&d));
        let _ = std::fs::remove_dir(&d);
    }

    /// A name index and a search store, in folders of the test's own: each is told apart from
    /// any other file by what it holds, and its preview says what it holds.
    /// A cleared image is named without its digest: the size freed is what must show.
    #[test]
    fn a_cleared_image_is_named_short() {
        let said = cleared("docker.io/texlive/texlive:latest@sha256:e8148b126545105c4aeeaa212f8a7bd02d05b500351372881055e1e59ae67d33", 1 << 30);
        assert!(said.starts_with("docker.io/texlive/texlive:latest ") && !said.contains("sha256"), "{said}");
        assert!(cleared("search.db", 1).starts_with("search.db "));
    }

    /// A runtime that has today's ImageMagick and the former default: the former is listed,
    /// said as no longer used, to clear.
    #[cfg(unix)]
    #[test]
    fn a_former_default_image_is_listed_to_clear() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("coxswain-disk-former-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let cfg = PreviewConfig::default();
        let now = cfg.images["imagemagick"].clone();
        let rt = d.join("podman");
        std::fs::write(&rt, format!("#!/bin/sh\n[ \"$1 $2\" = 'image inspect' ] && grep -qxF \"$5\" {}/has && echo 1000\n", d.display())).unwrap();
        std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(d.join("has"), format!("{now}\ndocker.io/dstmodders/imagemagick:latest\n")).unwrap();
        let items = images_in(&cfg, "podman", &rt);
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, [now.as_str(), "docker.io/dstmodders/imagemagick:latest"]);
        assert_eq!((items[1].id.as_str(), &items[1].what, &items[1].cost), ("image:docker.io/dstmodders/imagemagick:latest", &t!("disk.image.former", "tool" => "imagemagick"), &t!("disk.image.former_cost")));
        // Not there: no row.
        std::fs::write(d.join("has"), format!("{now}\n")).unwrap();
        assert_eq!(images_in(&cfg, "podman", &rt).len(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_stores_are_known_by_what_they_hold() {
        let d = std::env::temp_dir().join(format!("coxswain-disk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let home = d.join("home");
        for f in [
            "docs/a.md",
            "docs/b.md",
            "docs/c.txt",
            "private/.nosearch",
            "node_modules/x.js",
        ] {
            let p = home.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "fuel for flight seven").unwrap();
        }
        let ix = crate::index::Index::build(
            std::slice::from_ref(&home),
            &["node_modules".into()],
            None,
            None,
        );
        let copy = d.join("index-copy.bin");
        ix.save(&copy).unwrap();
        let r = describe(&copy).expect("an index is known by its magic, wherever it is");
        assert!(!r.live && r.item == "index");
        let text = r.text();
        assert!(
            text.contains("node_modules") && text.contains("private"),
            "{text}"
        );
        let f = crate::index::Index::facts(&copy).unwrap();
        assert_eq!((f.files, f.folders), (4, 2), "{f:?}");
        assert!(f.built.is_some_and(|b| b.0 > 0) && f.nosearch == [home.join("private")]);
        assert_eq!(
            f.largest.first().map(|l| l.0.clone()),
            Some(home.join("docs")),
            "{:?}",
            f.largest
        );

        let db = d.join("search.db");
        let store = crate::store::Store::open(&db).unwrap();
        store.note_error(Some("/x/y.pdf"), "the server said no");
        drop(store);
        let r = describe(&db).expect("a search store is known by its tables");
        assert_eq!(r.item, "store");
        assert!(r.text().contains("/x/y.pdf"), "{}", r.text());
        // Another database, and another file, are not.
        let other = d.join("other.db");
        rusqlite::Connection::open(&other)
            .unwrap()
            .execute_batch("CREATE TABLE files(x)")
            .unwrap();
        assert!(describe(&other).is_none() && describe(&home.join("docs/a.md")).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn clearing_everything_leaves_the_models_and_outside_unless_ticked() {
        let item = |id: &str, kind: Kind, rebuildable: bool, in_use: bool, bytes: u64| Item {
            id: id.into(),
            name: id.into(),
            what: String::new(),
            cost: String::new(),
            bytes,
            paths: vec![],
            kind,
            rebuildable,
            in_use,
            clear: true,
        };
        let items = [
            item("store", Kind::Own, true, false, 10),
            item("log", Kind::Own, false, false, 1),
            item("m", Kind::Model, false, false, 100),
            item("used", Kind::Model, false, true, 1000),
            item("image:x", Kind::Outside, false, false, 5000),
        ];
        assert_eq!(clearable_bytes(&items, false, false), 10);
        assert_eq!(
            clearable_bytes(&items, true, false),
            110,
            "never the model in use"
        );
        assert_eq!(clearable_bytes(&items, true, true), 5110);
        assert_eq!(find(&items, "images").len(), 1);
        assert_eq!(
            find(&items, "x").first().map(|i| i.id.as_str()),
            Some("image:x"),
            "an image by its exact name"
        );
        assert!(find(&items, "nothing").is_empty());
    }
}

//! Disk use, in both apps (Settings → Privacy and updates → Disk use) and `coxswain --disk`:
//! every place Coxswain keeps things in its cache folder, the built-in models, and what it
//! caused outside its folder (Tectonic's cache, container images pulled for previews), each with
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
}

/// One place on the disk.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Item {
    /// What `--disk clear` takes: `store`, `index`, …, a model's folder name, `tectonic`,
    /// `image:<name>`.
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
}

/// The rows of Coxswain's own folder: id, what they are in the cache folder, built again by
/// itself. `inside` is every `inside-<n>` folder.
const OWN: &[(&str, &[&str], bool)] = &[
    ("store", &["search.db", "search.db-wal", "search.db-shm"], true),
    ("index", &["index.bin", "index.tmp"], true),
    ("previews", &["previews"], true),
    ("peek", &["peek"], true),
    ("inside", &[], true),
    ("libreoffice", &["libreoffice-profile", "libreoffice-index-profile"], true),
    ("log", &["helper.log"], false),
];

/// What `--disk clear` takes besides the rows' ids.
pub const ALL_IMAGES: &str = "images";
pub const REBUILDABLE: &str = "rebuildable";

fn size(paths: &[PathBuf]) -> u64 {
    paths.iter().map(|p| crate::fs::dir_size(p).0).sum()
}

/// The `inside-<n>` folders of the helpers that read archives' text.
fn inside_folders(cache: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(cache).into_iter().flatten().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("inside-") && e.path().is_dir()).map(|e| e.path()).collect();
    v.sort();
    v
}

/// The rows of the cache folder, the built-in models and Tectonic's cache; with `images` also
/// the container images (which asks podman or docker, a moment each).
pub fn items(cfg: &Config, loaded: &crate::models::Loaded, images: bool) -> Vec<Item> {
    let mut v = vec![];
    if let Some(cache) = crate::helper::folder() {
        for (id, names, rebuildable) in OWN {
            let paths: Vec<PathBuf> = if *id == "inside" { inside_folders(&cache) } else { names.iter().map(|n| cache.join(n)).filter(|p| p.exists() || p.ends_with(names[0])).collect() };
            if *id == "inside" && paths.is_empty() {
                continue;
            }
            let name = match *id {
                "inside" => "inside-…/".to_string(),
                _ => format!("{}{}", names[0], if names[0].contains('.') { "" } else { "/" }),
            };
            v.push(Item { id: id.to_string(), name, what: t!(&format!("disk.{id}.what")), cost: t!(&format!("disk.{id}.cost")), bytes: size(&paths), paths, kind: Kind::Own, rebuildable: *rebuildable, in_use: false });
        }
    }
    for e in crate::models::list(&cfg.search, loaded) {
        v.push(Item { id: e.id.clone(), name: e.name.clone(), what: e.summary.clone(), cost: t!("disk.model.cost"), bytes: e.bytes, paths: vec![e.folder.clone()], kind: Kind::Model, rebuildable: false, in_use: e.in_use });
    }
    for p in made_outside() {
        v.push(Item { id: "tectonic".into(), name: "Tectonic".into(), what: t!("disk.tectonic.what"), cost: t!("disk.tectonic.cost"), bytes: size(std::slice::from_ref(&p)), paths: vec![p], kind: Kind::Outside, rebuildable: false, in_use: false });
    }
    if images {
        v.extend(image_items(&cfg.preview));
    }
    v
}

/// The container images of `[preview] images` that podman or docker has, by their exact names.
fn image_items(cfg: &PreviewConfig) -> Vec<Item> {
    let Some((name, rt)) = crate::tools::container_runtime(cfg) else { return vec![] };
    let mut seen = std::collections::HashSet::new();
    cfg.images
        .iter()
        .filter(|(_, image)| !image.is_empty() && seen.insert(image.to_string()))
        .filter_map(|(tool, image)| {
            let bytes = crate::tools::image_size(&rt, image)?;
            Some(Item { id: format!("image:{image}"), name: image.clone(), what: t!("disk.image.what", "tool" => tool, "runtime" => name), cost: t!("disk.image.cost"), bytes, paths: vec![], kind: Kind::Outside, rebuildable: false, in_use: false })
        })
        .collect()
}

/// The rows `name` names on the command line: an id, a model as `--models` names it, `images`.
pub fn find<'a>(items: &'a [Item], name: &str) -> Vec<&'a Item> {
    if name == ALL_IMAGES {
        return items.iter().filter(|i| i.id.starts_with("image:")).collect();
    }
    if let Some(i) = items.iter().find(|i| i.id == name || i.id == format!("image:{name}")) {
        return vec![i];
    }
    items.iter().filter(|i| i.kind == Kind::Model && i.name.eq_ignore_ascii_case(name)).take(1).collect()
}

/// Bytes *Clear everything that can be built again* frees: Coxswain's own rows, and with
/// `models` the built-in models not in use, with `outside` what it caused outside its folder.
pub fn clearable_bytes(items: &[Item], models: bool, outside: bool) -> u64 {
    items.iter().filter(|i| goes(i, models, outside)).map(|i| i.bytes).sum()
}

fn goes(i: &Item, models: bool, outside: bool) -> bool {
    match i.kind {
        Kind::Own => i.rebuildable,
        Kind::Model => models && !i.in_use,
        Kind::Outside => outside,
    }
}

fn remove(p: &Path) -> std::io::Result<()> {
    let r = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
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
    let gone = |paths: &[PathBuf]| -> Result<(), String> { paths.iter().try_for_each(|p| remove(p).map_err(|e| fail(p, e))) };
    match (item.kind, item.id.as_str()) {
        (Kind::Own, "store") if crate::helper::runs() && cfg.search.text => helper.forget(),
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
            let old = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().unwrap_or_default().as_secs() > 24 * 3600);
            let left: Vec<PathBuf> = item.paths.iter().filter(|p| !running || old(p)).cloned().collect();
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
        (Kind::Own, _) => gone(&item.paths)?,
        (Kind::Model, _) => {
            let all = crate::models::list(&cfg.search, &helper.loaded());
            let e = all.iter().find(|e| e.id == item.id).ok_or_else(|| t!("models.no_such", "name" => item.id.as_str()))?;
            if e.in_use {
                return Err(t!("disk.model_in_use", "model" => e.name.as_str()));
            }
            crate::models::delete(e, helper).map_err(|err| fail(&e.folder, err))?;
        }
        (Kind::Outside, "tectonic") => {
            gone(&item.paths)?;
            forget_made(&item.paths);
        }
        (Kind::Outside, _) => {
            let image = item.id.trim_start_matches("image:");
            let (_, rt) = crate::tools::container_runtime(&cfg.preview).ok_or_else(|| t!("convert.no_runtime"))?;
            let out = crate::tools::command(&rt).args(["image", "rm", image]).stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default().to_string());
            }
            return Ok(item.bytes);
        }
    }
    Ok(item.bytes.saturating_sub(size(&item.paths)))
}

/// *Clear everything that can be built again*: the bytes freed, and what could not be cleared.
pub fn clear_all(items: &[Item], models: bool, outside: bool, cfg: &Config, helper: &Client) -> (u64, Vec<String>) {
    let (mut freed, mut failed) = (0, vec![]);
    for i in items.iter().filter(|i| goes(i, models, outside)) {
        match clear(i, cfg, helper) {
            Ok(n) => freed += n,
            Err(e) => failed.push(format!("{}: {e}", i.name)),
        }
    }
    (freed, failed)
}

/// What clearing said: "search.db cleared: 284 KB freed."
pub fn cleared(name: &str, freed: u64) -> String {
    t!("disk.cleared", "name" => name, "size" => human(freed))
}

// ---------------------------------------------------------------- outside the cache folder

/// Where Coxswain notes what it made outside its folder, so it clears nothing it did not make.
fn made_file() -> Option<PathBuf> {
    Some(crate::helper::folder()?.join("made-outside.txt"))
}

/// The folders noted, those still there.
fn made_outside() -> Vec<PathBuf> {
    let text = made_file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    text.lines().map(PathBuf::from).filter(|p| p.is_dir()).collect()
}

fn forget_made(paths: &[PathBuf]) {
    let Some(f) = made_file() else { return };
    let text = std::fs::read_to_string(&f).unwrap_or_default();
    let kept: Vec<&str> = text.lines().filter(|l| !paths.iter().any(|p| Path::new(l) == p)).collect();
    let _ = if kept.is_empty() { std::fs::remove_file(&f) } else { std::fs::write(&f, kept.join("\n") + "\n") };
}

/// Where Tectonic may keep its cache: `TECTONIC_CACHE_DIR`, else its folder in the system's
/// cache folder (`~/.cache/tectonic`, `~/Library/Caches/Tectonic`,
/// `%LOCALAPPDATA%\TectonicProject\Tectonic`).
fn tectonic_caches() -> Vec<PathBuf> {
    if let Some(d) = std::env::var_os("TECTONIC_CACHE_DIR").filter(|d| !d.is_empty()) {
        return vec![d.into()];
    }
    let Some(base) = dirs::cache_dir() else { return vec![] };
    // Newer tectonic names it in lower case; Windows has the project's folder around it.
    vec![base.join("tectonic"), base.join("Tectonic"), base.join("TectonicProject").join("Tectonic")]
}

/// Before Coxswain runs Tectonic: the places of its cache that are not there yet. Give them to
/// `note_made` afterwards, which notes those that came.
pub fn tectonic_absent() -> Vec<PathBuf> {
    tectonic_caches().into_iter().filter(|p| !p.exists()).collect()
}

/// Note the folders of `absent` that are there now: Coxswain made them.
pub fn note_made(absent: &[PathBuf]) {
    let made: Vec<&PathBuf> = absent.iter().filter(|p| p.is_dir()).collect();
    let Some(f) = made_file().filter(|_| !made.is_empty()) else { return };
    let mut text = std::fs::read_to_string(&f).unwrap_or_default();
    for p in made {
        text += &format!("{}\n", p.display());
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

fn when(secs: u64) -> String {
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string()
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
    let bytes = ["", "-wal", "-shm"].iter().filter_map(|x| std::fs::metadata(format!("{}{x}", path.display())).ok()).map(|m| m.len()).sum();
    if crate::index::is_index(&head) {
        return Some(index_report(path, bytes, crate::index::Index::facts(path).ok()?));
    }
    if &head == b"SQLite format 3\0" {
        return Some(store_report(path, bytes, crate::store::facts(path).ok()?));
    }
    None
}

fn here_row(path: &Path, live: bool, ours: Option<PathBuf>) -> (String, String) {
    let value = if live { t!("disk.report.live") } else { t!("disk.report.copy", "path" => ours.map(|p| p.display().to_string()).unwrap_or_default()) };
    (t!("disk.report.here"), format!("{} · {value}", path.display()))
}

fn index_report(path: &Path, bytes: u64, f: crate::index::Facts) -> Report {
    let ours = crate::index::Index::cache_path();
    let live = same_file(path, ours.clone());
    let mut first = vec![
        (t!("disk.report.entries"), t!("disk.report.entries_value", "all" => count(f.files + f.folders), "files" => count(f.files), "folders" => count(f.folders))),
        (
            t!("disk.report.built"),
            match f.built {
                Some((at, ms)) => t!("disk.report.built_value", "when" => when(at), "seconds" => if ms < 1000 { format!("{:.2}", ms as f64 / 1000.0) } else { format!("{:.1}", ms as f64 / 1000.0) }),
                None => t!("disk.report.built_unknown"),
            },
        ),
        (t!("disk.report.size"), human(bytes)),
        here_row(path, live, ours),
    ];
    if f.archives > 0 {
        first.insert(1, (t!("disk.report.archives"), t!("disk.report.archives_value", "archives" => count(f.archives), "entries" => count(f.inside))));
    }
    let mut left = vec![(t!("disk.report.name_exclude"), if f.exclude.is_empty() { t!("disk.report.none") } else { f.exclude.join(", ") })];
    left.extend(f.nosearch.iter().map(|p| (p.display().to_string(), t!("disk.report.nosearch"))));
    // Files only in the cloud are named here; the store knows which, and leaves their text.
    if live && let Some(s) = crate::store::Store::path().and_then(|p| crate::store::facts(&p).ok()).filter(|s| s.cloud > 0) {
        left.push((t!("disk.report.cloud"), t!("disk.report.cloud_value", "n" => count(s.cloud))));
    }
    Report {
        item: "index",
        title: t!("disk.report.index_title"),
        live,
        sections: vec![
            section(t!("disk.report.index_title"), first),
            section(t!("disk.report.roots"), f.roots.iter().map(|r| (r.display().to_string(), String::new())).collect()),
            section(t!("disk.report.largest"), f.largest.iter().map(|(p, n)| (p.display().to_string(), count(*n))).collect()),
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
        (t!("disk.report.last_read"), f.read_at.map(when).unwrap_or_else(|| t!("disk.report.not_yet"))),
        (t!("disk.report.size"), human(bytes)),
        here_row(path, live, ours),
    ];
    let mut kinds: Vec<(String, String)> = f.kinds.iter().take(12).map(|(ext, n)| (if ext.is_empty() { t!("disk.report.no_ext") } else { format!(".{ext}") }, count(*n))).collect();
    if f.kinds.len() > 12 {
        let rest: usize = f.kinds[12..].iter().map(|k| k.1).sum();
        kinds.push((t!("disk.report.other_kinds", "n" => f.kinds.len() - 12), count(rest)));
    }
    if f.commits > 0 {
        kinds.push((t!("disk.report.commits"), count(f.commits)));
    }
    if f.inside > 0 {
        kinds.push((t!("disk.report.inside"), count(f.inside)));
    }
    let meaning = vec![
        (t!("disk.report.passages"), t!("disk.report.passages_value", "passages" => count(f.passages), "files" => count(f.with_vectors))),
        (t!("disk.report.model"), if f.model.is_empty() { t!("disk.report.none") } else { f.model.clone() }),
        (t!("disk.report.vectors_waiting"), count(f.vectors_waiting)),
    ];
    let parts = f.parts.iter().map(|(p, b)| (t!(&format!("disk.report.part.{p}")), human(*b))).collect();
    let errors = if f.errors.is_empty() { vec![(t!("disk.report.no_errors"), String::new())] } else { f.errors.iter().map(|(at, p, what)| (format!("{} {}", when(*at), p.as_deref().unwrap_or("")), what.clone())).collect() };
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
            let w = s.rows.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0).min(48);
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
    let to = crate::archive::peek_folder().map_err(|e| e.to_string())?.join(format!("{}.txt", path.file_name().unwrap_or_default().to_string_lossy()));
    std::fs::write(&to, report.text()).map_err(|e| e.to_string())?;
    Ok(to)
}

/// The rows as `coxswain --disk` prints them.
pub fn table(items: &[Item]) -> String {
    let mut out = String::new();
    for (kind, head) in [(Kind::Own, "disk.head.own"), (Kind::Model, "disk.head.models"), (Kind::Outside, "disk.head.outside")] {
        let rows: Vec<&Item> = items.iter().filter(|i| i.kind == kind).collect();
        if rows.is_empty() {
            continue;
        }
        out += &format!("{}\n", t!(head));
        for i in rows {
            let (pad, id) = (" ".repeat(29), i.id.trim_start_matches("image:"));
            out += &format!("  {id:<16} {:>9}  {}\n", human(i.bytes), if i.name == id { "" } else { &i.name });
            if let Some(p) = i.paths.first() {
                out += &format!("{pad} {}\n", p.display());
            }
            out += &format!("{pad} {}\n{pad} {}: {}\n", i.what, t!("disk.cost_label"), i.cost);
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

    /// A name index and a search store, in folders of the test's own: each is told apart from
    /// any other file by what it holds, and its preview says what it holds.
    #[test]
    fn the_stores_are_known_by_what_they_hold() {
        let d = std::env::temp_dir().join(format!("coxswain-disk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let home = d.join("home");
        for f in ["docs/a.md", "docs/b.md", "docs/c.txt", "private/.nosearch", "node_modules/x.js"] {
            let p = home.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "fuel for flight seven").unwrap();
        }
        let ix = crate::index::Index::build(std::slice::from_ref(&home), &["node_modules".into()], None, None);
        let copy = d.join("index-copy.bin");
        ix.save(&copy).unwrap();
        let r = describe(&copy).expect("an index is known by its magic, wherever it is");
        assert!(!r.live && r.item == "index");
        let text = r.text();
        assert!(text.contains("node_modules") && text.contains("private"), "{text}");
        let f = crate::index::Index::facts(&copy).unwrap();
        assert_eq!((f.files, f.folders), (4, 2), "{f:?}");
        assert!(f.built.is_some_and(|b| b.0 > 0) && f.nosearch == [home.join("private")]);
        assert_eq!(f.largest.first().map(|l| l.0.clone()), Some(home.join("docs")), "{:?}", f.largest);

        let db = d.join("search.db");
        let store = crate::store::Store::open(&db).unwrap();
        store.note_error(Some("/x/y.pdf"), "the server said no");
        drop(store);
        let r = describe(&db).expect("a search store is known by its tables");
        assert_eq!(r.item, "store");
        assert!(r.text().contains("/x/y.pdf"), "{}", r.text());
        // Another database, and another file, are not.
        let other = d.join("other.db");
        rusqlite::Connection::open(&other).unwrap().execute_batch("CREATE TABLE files(x)").unwrap();
        assert!(describe(&other).is_none() && describe(&home.join("docs/a.md")).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn clearing_everything_leaves_the_models_and_outside_unless_ticked() {
        let item = |id: &str, kind: Kind, rebuildable: bool, in_use: bool, bytes: u64| Item { id: id.into(), name: id.into(), what: String::new(), cost: String::new(), bytes, paths: vec![], kind, rebuildable, in_use };
        let items = [item("store", Kind::Own, true, false, 10), item("log", Kind::Own, false, false, 1), item("m", Kind::Model, false, false, 100), item("used", Kind::Model, false, true, 1000), item("image:x", Kind::Outside, false, false, 5000)];
        assert_eq!(clearable_bytes(&items, false, false), 10);
        assert_eq!(clearable_bytes(&items, true, false), 110, "never the model in use");
        assert_eq!(clearable_bytes(&items, true, true), 5110);
        assert_eq!(find(&items, "images").len(), 1);
        assert_eq!(find(&items, "x").first().map(|i| i.id.as_str()), Some("image:x"), "an image by its exact name");
        assert!(find(&items, "nothing").is_empty());
    }
}

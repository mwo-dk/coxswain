//! Each tab's listing, held here rather than in the page: the page gets the rows in view (and
//! a margin) and the count, and asks for more as it scrolls. Sorting, marking, quick search
//! and the totals of the marked entries work on the whole listing here. A folder of a million
//! files costs the page what a screenful does.

use super::{read_listing, to_page, Ctx, Item, Meta, Res};
use coxswain_core::fs::{Entry, SortKey};
use coxswain_core::history;
use coxswain_core::menu::Subject;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

/// Up to this many entries a listing comes whole, so a small folder needs no more asking.
const WHOLE: usize = 5000;

/// Said instead of a listing when a later one of the tab was asked for: the page drops it.
const STALE: &str = "stale listing";

struct List {
    /// The page's number for the listing: a later one replaces it, an earlier one never does.
    generation: u64,
    dir: PathBuf,
    entries: Vec<Entry>,
    marked: HashSet<PathBuf>,
}

/// The tabs' listings, by the page's tab id.
#[derive(Default)]
pub struct Tabs(Mutex<HashMap<u64, List>>);

impl Tabs {
    fn with<T>(&self, tab: u64, f: impl FnOnce(&mut List) -> T) -> Res<T> {
        let mut all = self.0.lock().map_err(|e| e.to_string())?;
        all.get_mut(&tab).map(f).ok_or_else(|| format!("no listing for tab {tab}"))
    }
}

#[derive(Serialize)]
struct Row {
    #[serde(flatten)]
    item: Item,
    /// Marked.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    m: bool,
}

/// Rows `from..` of a listing (paths joined from `prefix` by the page).
#[derive(Serialize)]
pub struct Rows {
    from: usize,
    /// What an entry's path starts with when it is the folder's path and its name: those
    /// entries come without a path (the page joins it), a third less JSON.
    prefix: String,
    items: Vec<Row>,
}

/// The marked entries: how many, their files' bytes, the folders among them (whose measured
/// sizes the page adds) and what they are, for the action menu and the hints.
#[derive(Serialize)]
pub struct Marks {
    count: usize,
    bytes: u64,
    dirs: Vec<PathBuf>,
    /// The folders' own sizes as listed: inside an archive, their contents'.
    dir_bytes: u64,
    subject: Subject,
}

/// A listing as the page gets it: what it says of the folder, its length, where the cursor
/// goes, the rows around it and the marks.
#[derive(Serialize)]
pub struct TabListing {
    #[serde(flatten)]
    meta: Option<Meta>,
    total: usize,
    /// Where `..` leads, when the listing has it.
    up: Option<PathBuf>,
    /// The folder is an OCI image layout (it holds a file `oci-layout`): its `index.json`
    /// opens as the image.
    oci: bool,
    cursor: usize,
    rows: Rows,
    marks: Marks,
}

impl List {
    fn rows(&self, from: usize, to: usize, ctx: &Ctx) -> Rows {
        let (from, to) = (from.min(self.entries.len()), to.min(self.entries.len()));
        let part = &self.entries[from..to];
        let marked: Vec<bool> = part.iter().map(|e| self.marked.contains(&e.path)).collect();
        let st = ctx.state.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = ctx.cfg();
        let plain = cfg.plain_glyphs().then(|| cfg.glyphs());
        let (prefix, items) = to_page(&self.dir, part.to_vec(), |p| st.tags.get(p).cloned(), plain.as_ref());
        Rows { from, prefix, items: items.into_iter().zip(marked).map(|(item, m)| Row { item, m }).collect() }
    }

    /// The rows around `cursor`, `span` on each side; all of a small listing.
    fn around(&self, cursor: usize, span: usize, ctx: &Ctx) -> Rows {
        if self.entries.len() <= WHOLE {
            return self.rows(0, WHOLE, ctx);
        }
        self.rows(cursor.saturating_sub(span), cursor + span, ctx)
    }

    fn marked(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| !self.marked.is_empty() && self.marked.contains(&e.path))
    }

    fn marks(&self) -> Marks {
        // ponytail: one pass over the listing per change of marks; kept counts if a million
        // marked entries make Insert slow.
        let list: Vec<&Entry> = self.marked().collect();
        Marks {
            count: list.len(),
            bytes: list.iter().filter(|e| !e.is_dir).map(|e| e.size).sum(),
            dirs: list.iter().filter(|e| e.is_dir).map(|e| e.path.clone()).collect(),
            dir_bytes: list.iter().filter(|e| e.is_dir).map(|e| e.size).sum(),
            // By name only: no file is read for a change of marks (the action menu reads one).
            subject: Subject::of(&list, false),
        }
    }

    fn up(&self) -> Option<PathBuf> {
        self.entries.first().filter(|e| e.is_parent()).map(|e| e.path.clone())
    }

    fn oci(&self) -> bool {
        self.entries.iter().any(|e| e.name == "oci-layout" && !e.is_dir)
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.name == name || e.path.as_os_str() == name)
    }

    fn sort(&mut self, sort: SortKey, reverse: bool, measured: &HashMap<PathBuf, u64>) {
        coxswain_core::fs::sort_in(&self.dir, &mut self.entries, sort, reverse, measured);
        if sort == SortKey::Commit
            && let Some(lasts) = history::last_changes_cached(&self.dir)
        {
            history::sort_by_last(&mut self.entries, &lasts, reverse);
        }
    }

    fn mark(&mut self, op: MarkOp) {
        let toggle = |marked: &mut HashSet<PathBuf>, e: &Entry| {
            if !marked.remove(&e.path) {
                marked.insert(e.path.clone());
            }
        };
        let real = |e: &&Entry| !e.is_parent();
        match op {
            MarkOp::Toggle(i) => {
                if let Some(e) = self.entries.get(i).filter(real) {
                    toggle(&mut self.marked, e);
                }
            }
            MarkOp::Range(a, b) => self.marked.extend(self.entries.iter().take(b.saturating_add(1)).skip(a).filter(real).map(|e| e.path.clone())),
            // As in a file explorer: everything in the folder, files and folders, but `..`.
            MarkOp::All => self.marked.extend(self.entries.iter().filter(real).map(|e| e.path.clone())),
            MarkOp::None => self.marked.clear(),
            MarkOp::Paths(paths) => {
                let here: HashSet<&PathBuf> = self.entries.iter().filter(real).map(|e| &e.path).collect();
                let add: Vec<PathBuf> = paths.into_iter().filter(|p| here.contains(p)).collect();
                self.marked.extend(add);
            }
            MarkOp::Invert => self.entries.iter().filter(|e| !e.is_dir).for_each(|e| toggle(&mut self.marked, e)),
            MarkOp::Glob { patterns, on } => {
                // By letters, not bytes: `?` is one "æ" or "日", as in the terminal app.
                let pats: Vec<Vec<char>> = patterns.split([' ', ';', ',']).filter(|s| !s.is_empty()).map(|s| s.to_lowercase().chars().collect()).collect();
                for e in self.entries.iter().filter(|e| !e.is_dir) {
                    let name: Vec<char> = e.name.to_lowercase().chars().collect();
                    if pats.iter().any(|g| coxswain_core::index::glob(g, &name)) {
                        if on {
                            self.marked.insert(e.path.clone());
                        } else {
                            self.marked.remove(&e.path);
                        }
                    }
                }
            }
        }
    }
}

/// A change of marks.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkOp {
    /// The entry at this row.
    Toggle(usize),
    /// Rows from the first to the last, both included, marked.
    Range(usize, usize),
    All,
    None,
    /// These entries marked (those in the listing): what a check of checksums found changed.
    Paths(Vec<PathBuf>),
    /// The files' marks turned over; folders stay as they are.
    Invert,
    /// Files whose names match one of the patterns (`*.jpg *.png`), marked or unmarked.
    Glob { patterns: String, on: bool },
}

/// Read `dir` into tab `tab`: its marks stay where the folder is the same (those still there),
/// the cursor goes on `focus` (a name or a path), else stays at `cursor` as far as it can.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn tab_list(
    tab: u64,
    generation: u64,
    dir: PathBuf,
    show_hidden: bool,
    sort: SortKey,
    reverse: bool,
    focus: Option<String>,
    cursor: usize,
    span: usize,
    ctx: tauri::State<'_, Ctx>,
    tabs: tauri::State<'_, Tabs>,
) -> Res<TabListing> {
    let (entries, meta) = read_listing(dir, show_hidden, sort, reverse, &ctx).await?;
    let mut all = tabs.0.lock().map_err(|e| e.to_string())?;
    let mut marked = HashSet::new();
    if let Some(old) = all.get_mut(&tab) {
        if old.generation > generation {
            return Err(STALE.into());
        }
        if old.dir == meta.dir {
            marked = std::mem::take(&mut old.marked);
        }
    }
    let alive: HashSet<&PathBuf> = if marked.is_empty() { HashSet::new() } else { entries.iter().map(|e| &e.path).collect() };
    marked.retain(|p| alive.contains(p));
    let list = List { generation, dir: meta.dir.clone(), entries, marked };
    let total = list.entries.len();
    let cursor = focus.and_then(|f| list.find(&f)).unwrap_or(cursor.min(total.saturating_sub(1)));
    let answer = TabListing { total, up: list.up(), oci: list.oci(), cursor, rows: list.around(cursor, span, &ctx), marks: list.marks(), meta: Some(meta) };
    all.insert(tab, list);
    Ok(answer)
}

/// Rows `from..to` of the tab's listing number `generation`; `None` when it has another now.
#[tauri::command(async)]
pub fn tab_rows(tab: u64, generation: u64, from: usize, to: usize, ctx: tauri::State<Ctx>, tabs: tauri::State<Tabs>) -> Res<Option<Rows>> {
    tabs.with(tab, |l| (l.generation == generation).then(|| l.rows(from, to, &ctx)))
}

/// Sort the tab's listing again, as number `generation`, the cursor kept on the entry at
/// `cursor`; by size, folders by their `measured` sizes (`fs::sort_in`).
#[tauri::command(async)]
pub fn tab_sort(tab: u64, generation: u64, sort: SortKey, reverse: bool, measured: HashMap<PathBuf, u64>, cursor: usize, span: usize, ctx: tauri::State<Ctx>, tabs: tauri::State<Tabs>) -> Res<TabListing> {
    tabs.with(tab, |l| {
        let at = l.entries.get(cursor).map(|e| e.path.clone());
        l.sort(sort, reverse, &measured);
        l.generation = l.generation.max(generation);
        let cursor = at.and_then(|p| l.entries.iter().position(|e| e.path == p)).unwrap_or(0);
        TabListing { meta: None, total: l.entries.len(), up: l.up(), oci: l.oci(), cursor, rows: l.around(cursor, span, &ctx), marks: l.marks() }
    })
}

/// The row of the entry called `name` (or with that path).
#[tauri::command(async)]
pub fn tab_find(tab: u64, name: String, tabs: tauri::State<Tabs>) -> Res<Option<usize>> {
    tabs.with(tab, |l| l.find(&name))
}

/// Quick search: the first row whose name starts with `query` (`fs::quick_match`).
#[tauri::command(async)]
pub fn tab_quick(tab: u64, query: String, tabs: tauri::State<Tabs>) -> Res<Option<usize>> {
    tabs.with(tab, |l| coxswain_core::fs::quick_match(&l.entries, &query))
}

/// Change the marks of the tab's listing number `generation` (rows are by number: another
/// listing would have other entries there); what is marked now.
#[tauri::command(async)]
pub fn tab_mark(tab: u64, generation: u64, op: MarkOp, tabs: tauri::State<Tabs>) -> Res<Marks> {
    tabs.with(tab, |l| {
        if l.generation != generation {
            return Err(STALE.to_string());
        }
        l.mark(op);
        Ok(l.marks())
    })?
}

#[derive(Serialize)]
pub struct Pick {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

/// Entries of the tab's listing, in its order, `..` never: the marked ones, the folders, or all.
#[tauri::command(async)]
pub fn tab_pick(tab: u64, which: String, tabs: tauri::State<Tabs>) -> Res<Vec<Pick>> {
    tabs.with(tab, |l| {
        let pick = |e: &Entry| Pick { name: e.name.clone(), path: e.path.clone(), is_dir: e.is_dir };
        match which.as_str() {
            "marked" => l.marked().map(pick).collect(),
            "dirs" => l.entries.iter().filter(|e| e.is_dir && !e.is_parent()).map(pick).collect(),
            _ => l.entries.iter().filter(|e| !e.is_parent()).map(pick).collect(),
        }
    })
}

/// The page started (again): no listing of an earlier page is kept.
#[tauri::command(async)]
pub fn tab_reset(tabs: tauri::State<Tabs>) {
    if let Ok(mut all) = tabs.0.lock() {
        all.clear();
    }
}

/// A tab closed: its listing goes.
#[tauri::command(async)]
pub fn tab_drop(tab: u64, tabs: tauri::State<Tabs>) {
    if let Ok(mut all) = tabs.0.lock() {
        all.remove(&tab);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(names: &[(&str, bool)]) -> List {
        let dir = PathBuf::from("/d");
        let entries = names
            .iter()
            .map(|&(name, is_dir)| Entry { name: name.into(), path: if name == ".." { "/".into() } else { dir.join(name) }, is_dir, is_symlink: false, is_exec: false, hidden: false, size: 10, modified: 0, created: 0, online: false, referenced: 0 })
            .collect();
        List { generation: 1, dir, entries, marked: HashSet::new() }
    }

    #[test]
    fn marking_works_on_the_whole_listing_never_on_the_way_up() {
        let mut l = list(&[("..", true), ("src", true), ("a.rs", false), ("b.txt", false), ("C.RS", false)]);
        l.mark(MarkOp::All);
        assert_eq!(l.marks().count, 4, "all but ..");
        assert_eq!(l.marks().bytes, 30, "the files' bytes; a folder's size the page adds");
        assert_eq!(l.marks().dirs, [PathBuf::from("/d/src")]);
        l.mark(MarkOp::Invert);
        assert_eq!(l.marks().count, 1, "files turned over, folders kept");
        l.mark(MarkOp::None);
        l.mark(MarkOp::Glob { patterns: "*.rs;x".into(), on: true });
        let names: Vec<_> = l.marked().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.rs", "C.RS"], "in any case, in the listing's order");
        l.mark(MarkOp::Toggle(0));
        l.mark(MarkOp::Toggle(2));
        assert_eq!(l.marks().count, 1, "`..` is never marked; a marked one unmarked");
        l.mark(MarkOp::Range(0, 3));
        assert_eq!(l.marks().count, 4);
        assert_eq!(l.marks().subject.files, 3);
        l.mark(MarkOp::None);
        l.mark(MarkOp::Paths(vec![PathBuf::from("/d").join("b.txt"), PathBuf::from("/elsewhere")]));
        assert_eq!(l.marks().count, 1, "only what the listing has");
    }

    #[test]
    fn a_listing_is_found_by_name_or_path_and_sorted_again_in_place() {
        let mut l = list(&[("..", true), ("b", false), ("a", false)]);
        assert_eq!(l.find("b"), Some(1));
        assert_eq!(l.find("/d/a"), Some(2));
        l.sort(SortKey::Name, false, &HashMap::new());
        assert_eq!(l.find("a"), Some(1), "`..` stays first");
    }
}

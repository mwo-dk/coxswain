//! F1 → Features, in both apps: every feature, by area as `docs/README.md` has them, each with
//! a line on what it does, its keys as configured, two or three questions answered, and its docs
//! page ("Show me"). The texts are `feature.<id>.title`, `.what`, `.q1`/`.a1` … in the locales;
//! `{action}` in them (`{copy}`) is that action's key in the user's keymap.

use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use crate::config::{Action, Config};
use Action::*;

/// A feature: its id (its texts' key), its docs page under `docs/` (whose folder is its area),
/// and the actions that are its keys, the most used first.
pub struct Feature {
    pub id: &'static str,
    pub docs: &'static str,
    pub actions: &'static [Action],
}

const fn f(id: &'static str, docs: &'static str, actions: &'static [Action]) -> Feature {
    Feature { id, docs, actions }
}

/// The areas, in the order of `docs/README.md`: `features.area.<area>` names each.
pub const AREAS: [&str; 8] = ["panels", "organise", "commands", "search", "previews", "files", "customise", "reference"];

/// Where "Show me" opens a feature's page.
pub const DOCS: &str = "https://github.com/mwo-dk/coxswain/blob/master/docs/";

/// Every feature, in the order of `docs/README.md`. A docs page of a feature has its entry here
/// (the test below checks), with its texts in every language.
pub const FEATURES: &[Feature] = &[
    // panels
    f("first_run", "panels/first-run.md", &[Help]),
    f("the_screen", "panels/the-screen.md", &[SwitchPanel, TogglePanels]),
    f("moving", "panels/moving.md", &[Open, Parent, GotoLeft, GotoRight, EditPath, Refresh]),
    f("quick_search", "panels/quick-search.md", &[]),
    f("marking", "panels/marking.md", &[Mark, MarkGroup, UnmarkGroup, InvertMarks, MarkAll]),
    f("sorting", "panels/sorting.md", &[SortName, SortExt, SortTime, SortSize, ToggleHidden]),
    f("tabs_and_panes", "panels/tabs-and-panes.md", &[NewTab, CloseTab, NextTab, Back, Forward, TogglePanels]),
    f("views", "panels/views.md", &[ToggleView, Columns]),
    f("folder_sizes", "panels/folder-sizes.md", &[FolderSizes, Columns]),
    f("git", "panels/git.md", &[History, Branches, Refresh]),
    f("git_history", "panels/git-history.md", &[History]),
    f("git_branches", "panels/git-branches.md", &[Branches, SwitchBranch, Worktrees, NewBranch]),
    f("mouse", "panels/mouse.md", &[]),
    f("command_list", "panels/command-list.md", &[Menu, Help]),
    f("features", "panels/features.md", &[Help, Features]),
    f("action_menu", "panels/action-menu.md", &[ActionMenu]),
    f("session", "panels/session.md", &[]),
    f("default_keys", "panels/keys.md", &[Help, Menu]),
    // organise
    f("tags", "organise/tags.md", &[Tag]),
    f("notes", "organise/notes.md", &[Notes]),
    f("favourites", "organise/favourites.md", &[ToggleSidebar]),
    f("sidebar", "organise/sidebar.md", &[ToggleSidebar]),
    // commands
    f("command_line", "commands/command-line.md", &[CopyPath, TogglePanels]),
    f("user_menu", "commands/user-menu.md", &[UserMenu]),
    f("scripts", "commands/scripts.md", &[UserMenu]),
    f("view_and_edit", "commands/view-and-edit.md", &[View, Edit]),
    f("opening_files", "commands/opening-files.md", &[Open]),
    // search
    f("search_setup", "search/setup.md", &[Settings]),
    f("find_file", "search/find-file.md", &[Search, SearchText, Ask]),
    f("names", "search/names.md", &[Search]),
    f("name_syntax", "search/name-syntax.md", &[Search]),
    f("text", "search/text.md", &[SearchText, Search]),
    f("search_documents", "search/documents.md", &[SearchText]),
    f("scans", "search/scans.md", &[]),
    f("search_diagrams", "search/diagrams.md", &[SearchText, Search]),
    f("search_history", "search/history.md", &[Search, SearchText]),
    f("search_archives", "search/archives.md", &[Search]),
    f("search_folders", "search/folders.md", &[Settings]),
    f("cloud_files", "search/cloud-files.md", &[]),
    f("removable_disks", "search/removable-disks.md", &[]),
    f("meaning", "search/meaning.md", &[Search]),
    f("servers", "search/servers.md", &[]),
    f("ask", "search/ask.md", &[Ask, Search]),
    f("ask_builtin", "search/ask-builtin.md", &[Ask]),
    f("models", "search/models.md", &[Settings]),
    f("helper", "search/helper.md", &[]),
    f("battery", "search/battery.md", &[]),
    f("notices", "search/notices.md", &[Settings]),
    f("search_settings", "search/settings.md", &[Settings]),
    // previews
    f("text_and_code", "previews/text-and-code.md", &[TogglePreview, View, Edit, Open]),
    f("preview_documents", "previews/documents.md", &[TogglePreview, View, Open]),
    f("html", "previews/html.md", &[TogglePreview, View, Open]),
    f("office", "previews/office.md", &[TogglePreview, Open, Settings]),
    f("preview_data", "previews/data.md", &[TogglePreview, View, Open]),
    f("cbom", "previews/bom.md", &[View]),
    f("provenance", "previews/provenance.md", &[View]),
    f("preview_media", "previews/media.md", &[TogglePreview, View, Open, Extract]),
    f("preview_diagrams", "previews/diagrams.md", &[TogglePreview, View, Open, Settings]),
    f("latex", "previews/latex.md", &[TogglePreview, View, Edit, Open, Settings]),
    f("preview_tools", "previews/tools.md", &[TogglePreview, Settings]),
    f("containers", "previews/containers.md", &[Settings, TogglePreview]),
    f("preview_safety", "previews/safety.md", &[Open, Edit, TogglePreview]),
    // files
    f("copy", "files/copy.md", &[Copy]),
    f("move_and_rename", "files/move-and-rename.md", &[Move, Rename]),
    f("new_folder", "files/new-folder.md", &[NewFolder]),
    f("delete", "files/delete.md", &[Delete, DeleteForever]),
    f("undo", "files/undo.md", &[Undo]),
    f("clipboard", "files/clipboard.md", &[ClipCopy, ClipCut, Paste]),
    f("drag_and_drop", "files/drag-and-drop.md", &[]),
    f("batch_rename", "files/batch-rename.md", &[BatchRename]),
    f("archives", "files/archives.md", &[Open, Copy, Move, NewFolder, Delete]),
    f("pack_and_extract", "files/pack-and-extract.md", &[Pack, Extract]),
    f("archive_passwords", "files/archive-passwords.md", &[Pack, Extract]),
    f("properties", "files/properties.md", &[Properties, Flags, Package]),
    f("duplicates", "files/duplicates.md", &[Duplicates]),
    f("zfs_snapshots", "files/zfs-snapshots.md", &[Snapshots]),
    // customise
    f("settings_window", "customise/settings.md", &[Settings]),
    f("themes", "customise/themes.md", &[Settings]),
    f("looks", "customise/looks.md", &[]),
    f("own_theme", "customise/own-theme.md", &[]),
    f("languages", "customise/languages.md", &[Settings]),
    f("change_keys", "customise/keys.md", &[Help, Settings]),
    f("glyphs_and_fonts", "customise/glyphs-and-fonts.md", &[Settings]),
    // reference
    f("terminal_app", "reference/terminal-app.md", &[Menu, Help, View, Edit, Refresh, Quit]),
    f("command_line_flags", "reference/command-line-flags.md", &[]),
    f("configuration", "reference/configuration.md", &[Settings]),
    f("privacy", "reference/privacy.md", &[Settings]),
    f("updates", "reference/updates.md", &[Settings]),
    f("where_things_are_kept", "reference/where-things-are-kept.md", &[Settings]),
    f("bills_of_materials", "reference/bills-of-materials.md", &[]),
    f("linux_arm", "reference/linux-arm.md", &[]),
    f("chromeos", "reference/chromeos.md", &[]),
    f("macos", "reference/macos.md", &[]),
    f("windows_arm", "reference/windows-arm.md", &[]),
];

impl Feature {
    /// Its area: the folder of its page.
    pub fn area(&self) -> &'static str {
        self.docs.split('/').next().unwrap_or_default()
    }

    /// Its page on GitHub.
    pub fn url(&self) -> String {
        format!("{DOCS}{}", self.docs)
    }
}

/// The feature `id`.
pub fn find(id: &str) -> Option<&'static Feature> {
    FEATURES.iter().find(|f| f.id == id)
}

/// A feature as F1 shows it, in the current language.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Entry {
    pub id: &'static str,
    /// The area's heading.
    pub area: String,
    pub title: String,
    pub what: String,
    /// Each action with its keys: "Copy", "F5".
    pub keys: Vec<(String, String)>,
    /// The questions with their answers (only those that match, when filtered by them).
    pub qa: Vec<(String, String)>,
    pub url: String,
}

/// A key as written on the keyboard: `Ctrl+G`, not `Ctrl+g`.
fn written(k: &str) -> String {
    match k.rsplit_once('+') {
        Some((mods, c)) if c.len() == 1 => format!("{mods}+{}", c.to_uppercase()),
        _ => k.to_string(),
    }
}

/// The keys of `a`, "F8 / Delete", or "—" with none.
fn keys(cfg: &Config, a: Action) -> String {
    match cfg.keys.get(&a).filter(|k| !k.is_empty()) {
        Some(k) => k.iter().map(|k| written(k)).collect::<Vec<_>>().join(" / "),
        None => "—".into(),
    }
}

/// The text `key` with each `{action}` filled with that action's handiest key (its name when it
/// has none). None when the language has no such text.
fn text(cfg: &Config, key: &str) -> Option<String> {
    let s = crate::i18n::tr(key, &[]);
    if s == key {
        return None;
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let name = tail[1..].find('}').map(|j| &tail[1..1 + j]);
        match name.and_then(|n| Action::ALL.iter().find(|a| a.name() == n)) {
            Some(&a) => {
                out.push_str(&cfg.handy_key(a).map(written).unwrap_or_else(|| a.label()));
                rest = &tail[a.name().len() + 2..];
            }
            None => {
                out.push('{');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    Some(out)
}

/// `s` lower-cased without its diacritics, for the filter: "Größe" finds "grosse", "é" finds "e".
pub fn fold(s: &str) -> String {
    s.nfkd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .flat_map(|c| match c {
            'ß' => "ss".chars().collect::<Vec<_>>(),
            'ø' => vec!['o'],
            'æ' => "ae".chars().collect(),
            'œ' => "oe".chars().collect(),
            'ł' => vec!['l'],
            'đ' => vec!['d'],
            c => vec![c],
        })
        .collect()
}

/// Every word of `words` is in `hay` (both folded).
fn all_in(words: &[String], hay: &str) -> bool {
    words.iter().all(|w| hay.contains(w.as_str()))
}

/// The entry of `f` in the current language; `gui`: for the desktop app, which has actions the
/// terminal app lacks.
pub fn entry(cfg: &Config, f: &Feature, gui: bool) -> Entry {
    let t = |k: &str| text(cfg, &format!("feature.{}.{k}", f.id)).unwrap_or_default();
    let qa = (1..=4).map_while(|i| Some((text(cfg, &format!("feature.{}.q{i}", f.id))?, t(&format!("a{i}"))))).collect();
    Entry {
        id: f.id,
        area: crate::i18n::tr(&format!("features.area.{}", f.area()), &[]),
        title: t("title"),
        what: t("what"),
        keys: f.actions.iter().filter(|a| gui || !a.gui_only()).map(|&a| (a.label(), keys(cfg, a))).collect(),
        qa,
        url: f.url(),
    }
}

/// The features as F1 → Features lists them, filtered by `query` in the current language: a
/// feature whose title or line holds every word of it, with all its questions; else only its
/// questions that do. The title it names comes first, then the other title matches, then the
/// line's, then the questions'.
pub fn list(cfg: &Config, query: &str, gui: bool) -> Vec<Entry> {
    let words: Vec<String> = fold(query).split_whitespace().map(String::from).collect();
    let mut out: Vec<(u8, Entry)> = FEATURES
        .iter()
        .filter_map(|f| {
            let mut e = entry(cfg, f, gui);
            if words.is_empty() {
                return Some((0, e));
            }
            let (title, what) = (fold(&e.title), fold(&e.what));
            let rank = if title == fold(query.trim()) {
                0
            } else if all_in(&words, &title) {
                1
            } else if all_in(&words, &format!("{title} {what}")) {
                2
            } else {
                e.qa.retain(|(q, a)| all_in(&words, &fold(&format!("{} {} {q} {a}", e.title, e.what))));
                if e.qa.is_empty() {
                    return None;
                }
                3
            };
            Some((rank, e))
        })
        .collect();
    out.sort_by_key(|(r, _)| *r);
    out.into_iter().map(|(_, e)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference pages that are not one feature: a system's install and its differences.
    const NOT_FEATURES: &[&str] = &[
        "reference/security.md",
        "reference/performance.md",
        "reference/freebsd.md",
        "reference/netbsd.md",
        "reference/openbsd.md",
        "reference/illumos.md",
        "reference/truenas.md",
        "reference/flatpak.md",
        "reference/nix.md",
        "reference/termux.md",
    ];

    #[test]
    fn every_docs_feature_page_has_an_entry() {
        let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let mut missing = vec![];
        for area in AREAS {
            for e in std::fs::read_dir(docs.join(area)).unwrap_or_else(|e| panic!("docs/{area}: {e}")) {
                let name = e.unwrap().file_name().to_string_lossy().into_owned();
                let page = format!("{area}/{name}");
                if name.ends_with(".md") && name != "README.md" && !NOT_FEATURES.contains(&page.as_str()) && !FEATURES.iter().any(|f| f.docs == page) {
                    missing.push(page);
                }
            }
        }
        assert!(missing.is_empty(), "docs pages without a Features entry (crates/coxswain-core/src/features.rs): {missing:?}");
        for f in FEATURES {
            assert!(docs.join(f.docs).is_file(), "{}: no docs/{}", f.id, f.docs);
            assert!(AREAS.contains(&f.area()), "{}: area {}", f.id, f.area());
            assert_eq!(FEATURES.iter().filter(|g| g.id == f.id).count(), 1, "{} twice", f.id);
        }
    }

    #[test]
    fn every_feature_has_its_texts_and_two_or_three_questions() {
        let cfg = Config::default();
        for f in FEATURES {
            let e = entry(&cfg, f, true);
            assert!(!e.title.is_empty() && !e.what.is_empty(), "{}: title and what", f.id);
            assert!((2..=3).contains(&e.qa.len()), "{}: {} questions", f.id, e.qa.len());
            for (q, a) in &e.qa {
                assert!(!q.is_empty() && !a.is_empty(), "{}: {q}", f.id);
            }
            for s in [&e.title, &e.what].into_iter().chain(e.qa.iter().flat_map(|(q, a)| [q, a])) {
                assert!(!s.contains('{'), "{}: a placeholder that is no action: {s}", f.id);
            }
        }
    }

    #[test]
    fn keys_come_from_the_keymap_and_the_filter_folds() {
        let mut cfg = Config::default();
        assert!(text(&cfg, "feature.copy.what").is_some());
        let e = entry(&cfg, find("copy").unwrap(), false);
        assert!(e.keys.iter().any(|(_, k)| k == "F5"), "{:?}", e.keys);
        cfg.keys.insert(Action::Copy, vec!["Ctrl+k".into()]);
        let e = entry(&cfg, find("copy").unwrap(), false);
        assert!(e.keys.iter().any(|(_, k)| k == "Ctrl+K"), "{:?}", e.keys);
        assert_eq!(fold("Größe Éclair Ångström"), "grosse eclair angstrom");
        let all = list(&cfg, "", true);
        assert_eq!(all.len(), FEATURES.len());
        let undo = list(&cfg, "UNDO", true);
        assert_eq!(undo[0].id, "undo", "a title match first");
        assert_eq!(list(&cfg, "Find", true)[0].id, "find_file", "the title it names before Find duplicates");
        let areas: Vec<&str> = FEATURES.iter().map(Feature::area).collect();
        let mut sorted = areas.clone();
        sorted.sort_by_key(|a| AREAS.iter().position(|x| x == a));
        assert_eq!(areas, sorted, "in the order of the areas");
        assert!(list(&cfg, "zzzzqqq", true).is_empty());
        // Words anywhere in a question and its answer: only those questions.
        let some = list(&cfg, "password zip", true);
        assert!(!some.is_empty() && some.iter().all(|e| !e.qa.is_empty()));
        assert!(find("features").is_some() && all[0].url.starts_with(DOCS));
    }
}

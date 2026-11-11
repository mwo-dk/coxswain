//! Version 2.0 gave a few things in config.toml the word the apps use for them: marking, not
//! selecting; folders, not directories; the name index's folders under the names Settings has.
//! 2.33 chose the preview tools per kind of file (`[preview] prefer_tool` → `[preview.tools]`).
//! 2.37 made the viewers modules (`[modules] off`): three older switches went into them.
//! On start, both apps rewrite an older config once, its comments and order kept, and a notice
//! says what changed.

use toml_edit::{DocumentMut, Item, Key, TableLike};

/// (table, old key, new key).
pub const RENAMED: &[(&str, &str, &str)] = &[
    ("keys", "select_group", "mark_group"),
    ("keys", "unselect_group", "unmark_group"),
    ("keys", "invert_selection", "invert_marks"),
    ("keys", "mkdir", "new_folder"),
    ("keys", "dir_sizes", "folder_sizes"),
    ("search", "roots", "name_roots"),
    ("search", "exclude", "name_exclude"),
];

/// `text` with the keys of 1.x renamed, and what changed (`[keys] mkdir → new_folder`); `None`
/// when nothing had to.
pub fn rewrite(text: &str) -> Result<Option<(String, Vec<String>)>, String> {
    let mut doc: DocumentMut = text.parse().map_err(|e| format!("config: {e}"))?;
    let mut said = vec![];
    for table in ["keys", "search"] {
        let renames: Vec<(&str, &str)> = RENAMED
            .iter()
            .filter(|r| r.0 == table)
            .map(|r| (r.1, r.2))
            .collect();
        // `[keys]` or `keys = { … }` on one line.
        let Some(t) = doc.get_mut(table).and_then(Item::as_table_like_mut) else {
            continue;
        };
        if !renames.iter().any(|(old, _)| t.contains_key(old)) {
            continue;
        }
        rename(t, &renames, |old, new, kept| {
            said.push(if kept {
                format!("[{table}] {old} → {new}")
            } else {
                format!("[{table}] {old} ({new} kept)")
            });
        });
    }
    prefer_tool(&mut doc, &mut said);
    switches(&mut doc, &mut said);
    Ok((!said.is_empty()).then(|| (doc.to_string(), said)))
}

/// 2.33 chose per kind of file, not per tool: `[preview] prefer_tool` (`latex`, `d2`,
/// `ffmpeg` …) becomes `[preview.tools]` by kind (`latex`, `diagrams`, `video`). A kind
/// chosen already keeps its choice; `auto` was the default, so it goes.
fn prefer_tool(doc: &mut DocumentMut, said: &mut Vec<String>) {
    let Some(preview) = doc.get_mut("preview").and_then(Item::as_table_like_mut) else { return };
    let Some(old) = preview.remove("prefer_tool") else { return };
    let moved: Vec<(String, String)> = old
        .as_table_like()
        .map(|t| {
            t.iter()
                .filter_map(|(tool, v)| Some((crate::preview_tools::kind_of(tool)?.id.to_string(), v.as_str()?.to_string())))
                .filter(|(_, v)| crate::preview_tools::CHOICES.contains(&v.as_str()))
                .collect()
        })
        .unwrap_or_default();
    if !moved.is_empty() {
        if !preview.contains_key("tools") {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            preview.insert("tools", Item::Table(t));
        }
        if let Some(tools) = preview.get_mut("tools").and_then(Item::as_table_like_mut) {
            for (kind, v) in moved {
                if !tools.contains_key(&kind) {
                    tools.insert(&kind, toml_edit::value(v));
                }
            }
        }
    }
    said.push("[preview] prefer_tool → tools".into());
}

/// (table, old switch, its module): the switches 2.37 folded into modules.
pub const SWITCHES: &[(Option<&str>, &str, &str)] = &[(None, "bom_viewer", "bom"), (None, "provenance_viewer", "provenance"), (Some("preview"), "ebooks", "ebooks")];

/// 2.37 turned the older switches into modules: each goes, and one that was `false` puts its
/// module in `[modules] off` (kept there if it is already). `true`, the default, just goes:
/// every module is on unless turned off.
fn switches(doc: &mut DocumentMut, said: &mut Vec<String>) {
    // Put above the first table once all are moved, `[modules]` too when it is the only one.
    let mut notes = String::new();
    for &(table, key, module) in SWITCHES {
        let t = match table {
            None => Some(doc.as_table_mut() as &mut dyn TableLike),
            Some(t) => doc.get_mut(t).and_then(Item::as_table_like_mut),
        };
        let Some(old) = t.and_then(|t| take(t, key, table.is_none())) else { continue };
        notes.push_str(old.1.as_deref().unwrap_or(""));
        // A `[preview]` with nothing left in it is not written as an empty header.
        if let Some(name) = table.filter(|n| doc.get(n).and_then(Item::as_table).is_some_and(|t| t.iter().all(|(_, i)| !i.is_value()))) {
            // Its comment goes on to the table after it before its header is hidden.
            let note = doc.get(name).and_then(Item::as_table).and_then(|t| t.decor().prefix()).and_then(|p| p.as_str()).filter(|p| p.contains('#')).map(str::to_string);
            if let Some(note) = note {
                keep_note_after(doc, name, note);
            }
            if let Some(t) = doc.get_mut(name).and_then(Item::as_table_mut) {
                t.set_implicit(true);
            }
        }
        let name = table.map_or(key.to_string(), |t| format!("[{t}] {key}"));
        if old.0.as_bool() != Some(false) {
            said.push(format!("{name} (removed)"));
        } else if turn_off(doc, module) {
            said.push(format!("{name} = false → [modules] off = [\"{module}\"]"));
        } else {
            // `[modules] off` is there but no list: said, for the user to do.
            said.push(format!("{name} = false (add \"{module}\" to [modules] off yourself)"));
        }
    }
    if !notes.is_empty() {
        keep_note(doc, notes);
    }
}

/// `module` in `[modules] off` (a new table at the end when there is none); false when that
/// is there but no list it can be added to.
fn turn_off(doc: &mut DocumentMut, module: &str) -> bool {
    if !doc.contains_key("modules") {
        doc.insert("modules", Item::Table(toml_edit::Table::new()));
    }
    let Some(modules) = doc.get_mut("modules").and_then(Item::as_table_like_mut) else { return false };
    let Some(list) = modules.entry("off").or_insert(toml_edit::value(toml_edit::Array::new())).as_array_mut() else { return false };
    if !list.iter().any(|v| v.as_str() == Some(module)) {
        list.push(module);
    }
    true
}

/// `key` taken out of `t`, with the comment above it when it has one and `carry`: that goes to
/// the value after it, or, with none, back to the caller (`keep_note`). Without `carry` (in a
/// table, where it was about that key alone) it goes with the key.
fn take(t: &mut dyn TableLike, key: &str, carry: bool) -> Option<(Item, Option<String>)> {
    let names: Vec<String> = t.iter().filter(|(k, i)| *k == key || i.is_value()).map(|(k, _)| k.to_string()).collect();
    let at = names.iter().position(|n| n == key)?;
    let note = t.key(key).and_then(|k| k.leaf_decor().prefix()).and_then(|p| p.as_str()).filter(|p| carry && p.contains('#')).map(str::to_string);
    let item = t.remove(key)?;
    let Some(note) = note else { return Some((item, None)) };
    match names.get(at + 1).and_then(|n| t.key_mut(n)) {
        Some(mut next) => {
            let was = next.leaf_decor().prefix().and_then(|p| p.as_str()).unwrap_or("").to_string();
            next.leaf_decor_mut().set_prefix(format!("{note}{was}"));
            Some((item, None))
        }
        None => Some((item, Some(note))),
    }
}

/// A comment above the last top-level value (a file's own heading): above the first table, or,
/// with no table, at the end of the file.
fn keep_note(doc: &mut DocumentMut, note: String) {
    let first = doc.iter().filter(|(_, i)| i.is_table()).map(|(k, _)| k.to_string()).min_by_key(|k| doc.get(k).and_then(Item::as_table).and_then(|t| t.position()).unwrap_or(isize::MAX));
    put_above(doc, first, note);
}

/// `note` (the comment of the table `name`, about to lose its header) above the table that
/// prints next: its own first sub-table, else the next top-level one, else the file's end.
fn keep_note_after(doc: &mut DocumentMut, name: &str, note: String) {
    let pos = |i: &Item| i.as_table().and_then(|t| t.position()).unwrap_or(isize::MAX);
    let child = doc.get(name).and_then(Item::as_table).and_then(|t| t.iter().filter(|(_, i)| i.is_table()).min_by_key(|(_, i)| pos(i)).map(|(k, _)| k.to_string()));
    if let Some(child) = child {
        if let Some(t) = doc.get_mut(name).and_then(Item::as_table_mut).and_then(|t| t.get_mut(&child)).and_then(Item::as_table_mut) {
            let was = t.decor().prefix().and_then(|p| p.as_str()).unwrap_or("").to_string();
            t.decor_mut().set_prefix(format!("{note}{was}"));
            return;
        }
    }
    let here = doc.get(name).map_or(isize::MAX, pos);
    let next = doc.iter().filter(|(k, i)| *k != name && i.is_table() && pos(i) > here).min_by_key(|(_, i)| pos(i)).map(|(k, _)| k.to_string());
    put_above(doc, next, note);
}

fn put_above(doc: &mut DocumentMut, table: Option<String>, note: String) {
    match table.and_then(|k| doc.get_mut(&k)).and_then(Item::as_table_mut) {
        Some(t) => {
            let was = t.decor().prefix().and_then(|p| p.as_str()).unwrap_or("").to_string();
            t.decor_mut().set_prefix(format!("{note}{was}"));
        }
        None => {
            let was = doc.trailing().as_str().unwrap_or("").to_string();
            doc.set_trailing(format!("{was}{note}"));
        }
    }
}

/// Every key of `t` in its place, the old ones under their new names with their comments. Where
/// the new name is there already, the old key goes (`kept` false).
fn rename(t: &mut dyn TableLike, renames: &[(&str, &str)], mut said: impl FnMut(&str, &str, bool)) {
    let names: Vec<String> = t.iter().map(|(k, _)| k.to_string()).collect();
    for name in &names {
        let (Some(key), Some(item)) = (t.key(name).cloned(), t.remove(name)) else {
            continue;
        };
        match renames.iter().find(|(old, _)| old == name) {
            Some((old, new)) if names.iter().any(|n| n == new) => said(old, new, false),
            Some((old, new)) => {
                let key = Key::new(*new)
                    .with_leaf_decor(key.leaf_decor().clone())
                    .with_dotted_decor(key.dotted_decor().clone());
                t.entry_format(&key).or_insert(item);
                said(old, new, true);
            }
            None => {
                t.entry_format(&key).or_insert(item);
            }
        }
    }
}

/// On start, before the config is read: a config.toml of 1.x rewritten (through a file beside
/// it, so a half-written one never replaces it), and what changed kept in the state for the
/// notice. An error says what could not be done; the app goes on.
pub fn on_start() -> Result<(), String> {
    let Some(path) = crate::config::Config::path() else {
        return Ok(());
    };
    // Through a link (a config kept with dotfiles) to the file itself.
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let Some((new, said)) = rewrite(&text)? else {
        return Ok(());
    };
    crate::fs::replace(&path, new.as_bytes(), false).map_err(|e| {
        crate::t!("notice.migrate_failed", "path" => path.display(), "why" => e, "changes" => said.join(", "))
    })?;
    let mut st = crate::state::AppState::load();
    // The last rewrite only: an earlier one's notice was shown then.
    st.migrated = said;
    st.save().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config.toml as 1.x wrote and documented it.
    const OLD: &str = r#"# my config
theme = "nc"

[keys]
# NC's keys for marking
select_group = ["+", "Alt+="]
unselect_group = ["-"]
quit = ["F10"]   # as always
mkdir = ["F7", "Shift+F7"]

[search]
text = true
roots = ["/home/me", "/mnt/data"]  # two disks
exclude = ["/proc", "node_modules"]
max_results = 500
"#;

    #[test]
    fn migrate_prefer_tool_to_a_choice_per_kind() {
        let old = "[preview]\nprefer = \"local\"\n\n[preview.prefer_tool]\nd2 = \"container\"\nlatex = \"auto\"\nwaveform = \"local\"\n";
        let (new, said) = rewrite(old).unwrap().unwrap();
        assert_eq!(said, ["[preview] prefer_tool → tools"]);
        let cfg = crate::config::Config::parse(&new).unwrap();
        assert_eq!(cfg.preview.tools.get("diagrams").map(String::as_str), Some("container"));
        assert_eq!(cfg.preview.tools.get("video").map(String::as_str), Some("local"));
        assert!(!cfg.preview.tools.contains_key("latex"), "auto was the default");
        assert!(!new.contains("prefer_tool"), "{new}");
        assert!(rewrite(&new).unwrap().is_none(), "once");
        let inline = "[preview]\nprefer_tool = { ocrmypdf = \"container\" }\n";
        let cfg = crate::config::Config::parse(&rewrite(inline).unwrap().unwrap().0).unwrap();
        assert_eq!(cfg.preview.tools["ocr"], "container");
    }

    #[test]
    fn migrate_the_old_switches_to_modules_once() {
        // false: its module off; true or absent: nothing but the key gone.
        let old = "# mine\nbom_viewer = false\nprovenance_viewer = true\n\n[preview]\nebooks = false  # no calibre\ntimeout = 60\n";
        let (new, said) = rewrite(old).unwrap().unwrap();
        assert_eq!(said, ["bom_viewer = false → [modules] off = [\"bom\"]", "provenance_viewer (removed)", "[preview] ebooks = false → [modules] off = [\"ebooks\"]"]);
        let cfg = crate::config::Config::parse(&new).unwrap();
        assert_eq!(cfg.modules.off, ["bom", "ebooks"]);
        assert_eq!(cfg.preview.timeout, 60);
        for gone in ["bom_viewer", "provenance_viewer", "ebooks ="] {
            assert!(!new.contains(gone), "{gone} left:\n{new}");
        }
        assert!(new.starts_with("# mine\n"), "{new}");
        assert_eq!(rewrite(&new).unwrap(), None, "once");

        // true and absent turn nothing off; a module turned off already stays once.
        let (new, _) = rewrite("bom_viewer = true\n[preview]\nebooks = true\n").unwrap().unwrap();
        assert!(crate::config::Config::parse(&new).unwrap().modules.off.is_empty(), "{new}");
        assert!(!new.contains("modules"), "{new}");
        assert_eq!(rewrite("[preview]\ntimeout = 60\n").unwrap(), None);
        let (new, _) = rewrite("provenance_viewer = false\n[modules]\noff = [\"games\", \"provenance\"]\n").unwrap().unwrap();
        assert_eq!(crate::config::Config::parse(&new).unwrap().modules.off, ["games", "provenance"]);
        // A heading over the only key of a file with no table: kept above the new [modules].
        let (new, _) = rewrite("# mine\nbom_viewer = false\n").unwrap().unwrap();
        assert!(new.starts_with("# mine\n"), "{new}");
        // [modules] off of another type: nothing lost without a word.
        let (new, said) = rewrite("bom_viewer = false\n[modules]\noff = \"games\"\n").unwrap().unwrap();
        assert_eq!(said, ["bom_viewer = false (add \"bom\" to [modules] off yourself)"], "{new}");
        // [preview] left empty is no empty header; a comment on ebooks goes with it.
        let (new, _) = rewrite("[preview.tools]\noffice = \"local\"\n[preview]\n# cal\nebooks = false\n").unwrap().unwrap();
        assert!(!new.contains("[preview]\n") && !new.contains("# cal"), "{new}");
        assert_eq!(crate::config::Config::parse(&new).unwrap().preview.tools["office"], "local");
        // Comments survive: no table at all, and a [preview] whose only value was ebooks.
        for nl in ["\n", "\r\n"] {
            let fix = |t: &str| t.replace('\n', nl);
            let (new, _) = rewrite(&fix("# heading\nbom_viewer = true\n")).unwrap().unwrap();
            assert_eq!(new, fix("# heading\n"), "{new:?}");
            assert_eq!(rewrite(&new).unwrap(), None, "once");
            let (new, _) = rewrite(&fix("# top\nbom_viewer = true\n\n# about preview\n[preview]\nebooks = true\n\n# about modules\n[modules]\noff = []\n")).unwrap().unwrap();
            assert!(new.contains(&fix("# about preview\n")) && new.contains("# about modules") && new.contains("# top") && !new.contains("[preview]"), "{new:?}");
            assert_eq!(new.contains('\r'), nl == "\r\n");
            assert_eq!(new.matches('\n').count(), new.matches(nl).count());
            assert_eq!(rewrite(&new).unwrap(), None, "once");
            let (new, _) = rewrite(&fix("# about preview\n[preview]\nebooks = true\n")).unwrap().unwrap();
            assert_eq!(new, fix("# about preview\n"), "{new:?}");
            let (new, _) = rewrite(&fix("# about preview\n[preview]\nebooks = true\n[preview.tools]\noffice = \"local\"\n")).unwrap().unwrap();
            assert_eq!(new, fix("# about preview\n[preview.tools]\noffice = \"local\"\n"), "{new:?}");
        }
        // On one line too.
        let (new, _) = rewrite("preview = { ebooks = false }\n").unwrap().unwrap();
        assert_eq!(crate::config::Config::parse(&new).unwrap().modules.off, ["ebooks"], "{new}");
    }

    #[test]
    fn migrate_a_config_of_1x_once_with_its_comments() {
        assert_eq!(
            crate::config::Config::parse(OLD).unwrap().keys[&crate::config::Action::MarkGroup],
            ["+"],
            "2.0 does not know the old action names"
        );
        let (new, said) = rewrite(OLD).unwrap().unwrap();
        assert_eq!(
            said,
            [
                "[keys] select_group → mark_group",
                "[keys] unselect_group → unmark_group",
                "[keys] mkdir → new_folder",
                "[search] roots → name_roots",
                "[search] exclude → name_exclude"
            ]
        );
        for kept in [
            "# my config",
            "# NC's keys for marking\nmark_group = [\"+\", \"Alt+=\"]",
            "quit = [\"F10\"]   # as always",
            "name_roots = [\"/home/me\", \"/mnt/data\"]  # two disks",
        ] {
            assert!(new.contains(kept), "{kept} lost:\n{new}");
        }
        // In their places: mark_group first in [keys], max_results last in [search].
        assert!(
            new.find("mark_group").unwrap() < new.find("quit").unwrap()
                && new.find("name_exclude").unwrap() < new.find("max_results").unwrap(),
            "{new}"
        );
        let cfg = crate::config::Config::parse(&new).unwrap();
        use crate::config::Action;
        assert_eq!(cfg.keys[&Action::MarkGroup], ["+", "Alt+="]);
        assert_eq!(cfg.keys[&Action::NewFolder], ["F7", "Shift+F7"]);
        assert_eq!(
            cfg.search.roots,
            [std::path::PathBuf::from("/home/me"), "/mnt/data".into()]
        );
        assert_eq!((cfg.search.exclude.len(), cfg.search.max_results), (2, 500));
        assert_eq!(rewrite(&new).unwrap(), None, "once");

        // The new name there already: it wins, the old one goes.
        let (both, said) = rewrite("[keys]\nmkdir = [\"F7\"]\nnew_folder = [\"Ctrl+N\"]\n")
            .unwrap()
            .unwrap();
        assert_eq!(
            (both.as_str(), said[0].as_str()),
            (
                "[keys]\nnew_folder = [\"Ctrl+N\"]\n",
                "[keys] mkdir (new_folder kept)"
            )
        );
        assert_eq!(rewrite("").unwrap(), None);
        assert!(rewrite("[keys\n").is_err());

        // On one line (`keys = { … }`): renamed in place too.
        let (inline, said) = rewrite("keys = { mkdir = [\"F7\"], quit = [\"F10\"] }\nsearch = { roots = [\"/a\"] }\n")
            .unwrap()
            .unwrap();
        assert_eq!(said, ["[keys] mkdir → new_folder", "[search] roots → name_roots"]);
        let cfg = crate::config::Config::parse(&inline).unwrap();
        assert_eq!(cfg.keys[&Action::NewFolder], ["F7"], "{inline}");
        assert_eq!(cfg.search.roots, [std::path::PathBuf::from("/a")]);
    }
}

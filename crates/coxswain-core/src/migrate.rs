//! Version 2.0 gave a few things in config.toml the word the apps use for them: marking, not
//! selecting; folders, not directories; the name index's folders under the names Settings has.
//! On start, both apps rewrite a config from 1.x once, its comments and order kept, and a notice
//! says what changed.

use toml_edit::{DocumentMut, Item, Key, Table};

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
        let renames: Vec<(&str, &str)> = RENAMED.iter().filter(|r| r.0 == table).map(|r| (r.1, r.2)).collect();
        let Some(t) = doc.get_mut(table).and_then(Item::as_table_mut) else { continue };
        if !renames.iter().any(|(old, _)| t.contains_key(old)) {
            continue;
        }
        rename(t, &renames, |old, new, kept| {
            said.push(if kept { format!("[{table}] {old} → {new}") } else { format!("[{table}] {old} ({new} kept)") });
        });
    }
    Ok((!said.is_empty()).then(|| (doc.to_string(), said)))
}

/// Every key of `t` in its place, the old ones under their new names with their comments. Where
/// the new name is there already, the old key goes (`kept` false).
fn rename(t: &mut Table, renames: &[(&str, &str)], mut said: impl FnMut(&str, &str, bool)) {
    let names: Vec<String> = t.iter().map(|(k, _)| k.to_string()).collect();
    for name in &names {
        let Some((key, item)) = t.remove_entry(name) else { continue };
        match renames.iter().find(|(old, _)| old == name) {
            Some((old, new)) if names.iter().any(|n| n == new) => said(old, new, false),
            Some((old, new)) => {
                let key = Key::new(*new).with_leaf_decor(key.leaf_decor().clone()).with_dotted_decor(key.dotted_decor().clone());
                t.insert_formatted(&key, item);
                said(old, new, true);
            }
            None => {
                t.insert_formatted(&key, item);
            }
        }
    }
}

/// On start, before the config is read: a config.toml of 1.x rewritten (through a file beside
/// it, so a half-written one never replaces it), and what changed kept in the state for the
/// notice. An error says what could not be done; the app goes on.
pub fn on_start() -> Result<(), String> {
    let Some(path) = crate::config::Config::path() else { return Ok(()) };
    // Through a link (a config kept with dotfiles) to the file itself.
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(()) };
    let Some((new, said)) = rewrite(&text)? else { return Ok(()) };
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, new).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        crate::t!("notice.migrate_failed", "path" => path.display(), "why" => e, "changes" => said.join(", "))
    })?;
    let mut st = crate::state::AppState::load();
    st.migrated.extend(said);
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
    fn migrate_a_config_of_1x_once_with_its_comments() {
        assert!(crate::config::Config::parse(OLD).is_err(), "2.0 does not read the old action names");
        let (new, said) = rewrite(OLD).unwrap().unwrap();
        assert_eq!(said, ["[keys] select_group → mark_group", "[keys] unselect_group → unmark_group", "[keys] mkdir → new_folder", "[search] roots → name_roots", "[search] exclude → name_exclude"]);
        for kept in ["# my config", "# NC's keys for marking\nmark_group = [\"+\", \"Alt+=\"]", "quit = [\"F10\"]   # as always", "name_roots = [\"/home/me\", \"/mnt/data\"]  # two disks"] {
            assert!(new.contains(kept), "{kept} lost:\n{new}");
        }
        // In their places: mark_group first in [keys], max_results last in [search].
        assert!(new.find("mark_group").unwrap() < new.find("quit").unwrap() && new.find("name_exclude").unwrap() < new.find("max_results").unwrap(), "{new}");
        let cfg = crate::config::Config::parse(&new).unwrap();
        use crate::config::Action;
        assert_eq!(cfg.keys[&Action::MarkGroup], ["+", "Alt+="]);
        assert_eq!(cfg.keys[&Action::NewFolder], ["F7", "Shift+F7"]);
        assert_eq!(cfg.search.roots, [std::path::PathBuf::from("/home/me"), "/mnt/data".into()]);
        assert_eq!((cfg.search.exclude.len(), cfg.search.max_results), (2, 500));
        assert_eq!(rewrite(&new).unwrap(), None, "once");

        // The new name there already: it wins, the old one goes.
        let (both, said) = rewrite("[keys]\nmkdir = [\"F7\"]\nnew_folder = [\"Ctrl+N\"]\n").unwrap().unwrap();
        assert_eq!((both.as_str(), said[0].as_str()), ("[keys]\nnew_folder = [\"Ctrl+N\"]\n", "[keys] mkdir (new_folder kept)"));
        assert_eq!(rewrite("").unwrap(), None);
        assert!(rewrite("[keys\n").is_err());
    }
}

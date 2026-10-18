//! The docs check, the part that needs the code's own lists: every built-in theme, every action,
//! every `config.toml` key and every command-line flag is named in `docs/` (in code, as
//! `` `page_up` `` or in a fenced block), so nothing a user can set or press goes undocumented.
//! F1's texts name an action's key only through its `{action}` placeholder, so they follow the
//! keymap. Links, the changelog and screenshot placeholders are `tools/docs-check.py`'s; the
//! features (docs page ↔ `features::FEATURES`) are checked in `features.rs`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use coxswain_core::config::{Action, Config, Glyphs, Theme};

/// A file of the repository as text, with Windows line ends made plain: git may check out CRLF.
fn read(path: impl AsRef<std::path::Path>) -> String {
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            markdown(&p, out);
        } else if p.extension().is_some_and(|x| x == "md") {
            out.push(p);
        }
    }
}

/// The code of README.md and every docs page: inline code spans and fenced blocks, one per line.
/// Read once per test run.
fn code() -> &'static str {
    static CODE: OnceLock<String> = OnceLock::new();
    CODE.get_or_init(|| {
        let mut pages = vec![root().join("README.md")];
        markdown(&root().join("docs"), &mut pages);
        let mut code = String::new();
        for page in pages {
            let text = read(&page);
            let mut fenced = false;
            for line in text.lines() {
                let t = line.trim_start();
                if t.starts_with("```") || t.starts_with("~~~") {
                    fenced = !fenced;
                } else if fenced {
                    code.push_str(line);
                    code.push('\n');
                } else {
                    // `a` `b`: the odd pieces between backticks.
                    for span in line.split('`').skip(1).step_by(2) {
                        code.push_str(span);
                        code.push('\n');
                    }
                }
            }
        }
        code
    })
}

/// `word` stands on its own somewhere in `code`: not inside a longer name (`win11` is not found
/// in `win11-dark`, nor `text` in `text_roots`).
fn named(code: &str, word: &str) -> bool {
    let part = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    code.match_indices(word).any(|(i, _)| {
        !code[..i].chars().next_back().is_some_and(part) && !code[i + word.len()..].chars().next().is_some_and(part)
    })
}

fn missing<'a>(what: &str, names: impl IntoIterator<Item = &'a str>, except: &[&str]) {
    let code = code();
    let gone: Vec<&str> = names.into_iter().filter(|n| !except.contains(n) && !named(code, n)).collect();
    assert!(gone.is_empty(), "{what} not named in docs/ (in `code`): {gone:?}");
}

#[test]
fn every_builtin_theme_is_in_the_docs() {
    let names: Vec<&str> = Theme::BUILTIN.iter().map(|(n, ..)| *n).collect();
    missing("built-in themes", names, &[]);
}

#[test]
fn every_action_is_in_the_docs() {
    missing("actions", Action::ALL.iter().map(|a| a.name()), &[]);
}

/// Every key `config.toml` takes, as `section.key` (`search.name_roots`, `themes.*.crt`,
/// `themes.*.panel.fg`): from the config's own structs, serialized with every optional part set.
fn config_keys() -> Vec<String> {
    let mut cfg = Config::parse(
        "editor = \"e\"\nviewer = \"v\"\n[[user_menu]]\nkey = \"k\"\nlabel = \"l\"\ncommand = \"c\"\nwait = true\nwhen = \"file\"\n",
    )
    .unwrap();
    cfg.glyph_set = Some(Glyphs::default());
    cfg.keys.clear();
    cfg.themes.clear();
    let mut keys = vec![];
    let toml::Value::Table(top) = toml::Value::try_from(&cfg).unwrap() else { unreachable!() };
    fn walk(prefix: &str, value: &toml::Value, keys: &mut Vec<String>) {
        let table = match value {
            toml::Value::Table(t) => t,
            toml::Value::Array(a) => match a.first() {
                Some(toml::Value::Table(t)) => t,
                _ => return,
            },
            _ => return,
        };
        for (k, v) in table {
            let key = format!("{prefix}{k}");
            walk(&format!("{key}."), v, keys);
            keys.push(key);
        }
    }
    walk("", &toml::Value::Table(top), &mut keys);
    // A theme of one's own: the look, the tube, and fg, bg and bold on every slot.
    keys.extend(["themes", "keys", "themes.*.look", "themes.*.crt", "themes.*.*.fg", "themes.*.*.bg", "themes.*.*.bold"].map(String::from));
    keys.extend(Theme::default().slots().into_iter().map(|(s, _)| format!("themes.*.{s}")));
    // preview.images and prefer_tool are maps of tool names, not keys.
    keys.retain(|k| !k.starts_with("preview.images.") && !k.starts_with("preview.prefer_tool."));
    keys
}

#[test]
fn every_config_key_is_in_the_docs() {
    let keys = config_keys();
    assert!(keys.iter().any(|k| k == "search.name_roots") && keys.iter().any(|k| k == "user_menu.when"), "{keys:?}");
    // The last part names it in the docs (`name_roots` under `[search]`, `crt = true` in a theme).
    missing("config.toml keys", keys.iter().map(|k| k.rsplit('.').next().unwrap()), &[]);
}

/// The flags of both apps: the terminal app's `OPTIONS` with the values each takes, and the
/// desktop app's, from their sources (the binaries are not this crate's).
fn flags() -> Vec<String> {
    let file = |p: &str| read(root().join(p));
    let mut flags = vec![];
    let tui = file("crates/coxswain/src/lib.rs");
    let start = tui.find("const OPTIONS: &[(&str, &[&str])] = &[").expect("OPTIONS in crates/coxswain/src/lib.rs");
    let table = &tui[start..start + tui[start..].find("];").unwrap()];
    for line in table.lines().filter(|l| l.trim_start().starts_with("(\"")) {
        let words: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
        let (flag, values) = words.split_first().unwrap();
        flags.extend(values.iter().map(|v| format!("{flag} {v}")));
        flags.push(flag.to_string());
    }
    let gui = file("gui/src-tauri/src/main.rs");
    for piece in gui.split('"').skip(1).step_by(2) {
        if piece.starts_with("--") && piece[2..].chars().all(|c| c.is_ascii_lowercase() || c == '-') {
            flags.push(piece.to_string());
        }
    }
    flags.push(coxswain_core::helper::ARG.into());
    flags.push(coxswain_core::service::STAY.into());
    flags.sort();
    flags.dedup();
    flags
}

#[test]
fn every_flag_is_in_the_docs() {
    let flags = flags();
    assert!(flags.iter().any(|f| f == "--meaning bench") && flags.iter().any(|f| f == "--duplicates"), "{flags:?}");
    missing("command-line flags", flags.iter().map(String::as_str), &[]);
}

#[test]
fn a_name_counts_only_on_its_own() {
    let code = code();
    assert!(named(code, "win11-dark") && named(code, "name_roots") && !named(code, "no_such_key_anywhere"));
    assert!(!named("win11-dark", "win11") && !named("text_roots", "text") && named("a = text\n", "text"));
    let (keys, flags) = (config_keys().len(), flags().len());
    assert!(keys > 80 && flags > 20, "{keys} keys, {flags} flags");
}

/// `--settings=<name>`: every area and group Settings opens at is in the flags page, and every
/// option it opens at is named somewhere in the docs.
#[test]
fn every_settings_section_is_in_the_docs() {
    use coxswain_core::settings::{OPTIONS, sections};
    let options: Vec<&str> = OPTIONS.iter().map(|o| o.name).collect();
    let page = read(root().join("docs/reference/command-line-flags.md"));
    let listed = |n: &str| page.contains(&format!("`{n}`")) || page.contains(&format!("--settings={n}`"));
    let gone: Vec<&str> = sections().into_iter().map(|s| s.0).filter(|n| !options.contains(n) && !listed(n)).collect();
    assert!(gone.is_empty(), "--settings= names not in docs/reference/command-line-flags.md: {gone:?}");
    missing("Settings options", options, &[]);
}

/// The flags page shows `coxswain --help` as the terminal app prints it, word for word.
#[test]
fn the_flags_page_shows_the_help_as_it_is() {
    let src = read(root().join("crates/coxswain/src/lib.rs"));
    let start = src.find("const USAGE: &str = \"").expect("USAGE in crates/coxswain/src/lib.rs") + 21;
    let usage = &src[start..start + src[start..].find("\";").unwrap()];
    let page = read(root().join("docs/reference/command-line-flags.md"));
    assert!(
        page.contains(&format!("```\n{usage}\n```")),
        "docs/reference/command-line-flags.md must show coxswain --help as crates/coxswain/src/lib.rs's USAGE has it:\n{usage}"
    );
}

/// F1 → Features (`feature.*`) and Help (`help.*`) in every language name an action's key as
/// `{action}` (`{settings}`), never as its default written out (`F12`, `Ctrl+O`, `Alt+G`): a
/// written key goes stale when the default changes, and is wrong for whoever changed it.
#[test]
fn f1_texts_name_keys_by_their_action() {
    use coxswain_core::config::written;
    // Keys in these texts that are not the action's: a key inside Find or a prompt, the
    // terminal's own Ctrl+C, a `[keys]` example, the key a question is about.
    const NOT_THE_ACTION: &[(&str, &str)] = &[
        ("feature.change_keys.a2", "*"),
        ("feature.clipboard.a3", "Ctrl+C"),
        ("feature.find_file.a3", "*"),
        ("feature.settings_window.q3", "Ctrl+,"),
        ("feature.settings_window.a3", "Ctrl+,"),
        ("help.also4", "Ctrl+U"),
    ];
    let cfg = Config::default();
    // An F-key or a key with a modifier: `Enter`, `+` or `Space` are words too.
    let keys: Vec<String> = Action::ALL
        .iter()
        .flat_map(|a| cfg.keys.get(a).into_iter().flatten())
        .map(|k| written(k))
        .filter(|k| (k.len() > 1 && k.contains('+')) || (k.starts_with('F') && k[1..].parse::<u8>().is_ok()))
        .collect();
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '+');
    let mut found = vec![];
    for e in std::fs::read_dir(root().join("crates/coxswain-core/locales")).unwrap() {
        let path = e.unwrap().path();
        let lang = path.file_stem().unwrap().to_string_lossy().into_owned();
        let texts: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&read(&path)).unwrap();
        for (id, text) in &texts {
            let Some(text) = text.as_str().filter(|_| id.starts_with("feature.") || id.starts_with("help.")) else { continue };
            for k in &keys {
                if NOT_THE_ACTION.iter().any(|&(i, key)| i == id && (key == "*" || key == k)) {
                    continue;
                }
                if text.match_indices(k.as_str()).any(|(i, _)| !edge(text[..i].chars().next_back()) && !edge(text[i + k.len()..].chars().next())) {
                    found.push(format!("{lang} {id}: {k}"));
                }
            }
        }
    }
    found.sort();
    assert!(found.is_empty(), "a default key written out in F1's texts; use its {{action}} placeholder:\n{}", found.join("\n"));
}

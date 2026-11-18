//! Pick mode writes neither config.toml nor state.json, whatever command asks for it. A process of
//! its own: `pick::keep_own_files` holds for the whole run, and the other tests must write.

use std::path::{Path, PathBuf};

use coxswain_core::{pick, preview_tools, settings, state::AppState};

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("coxswain-pick-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_picker_writes_neither_config_nor_state() {
    pick::keep_own_files();
    let d = scratch("writes");
    let (config, state) = (d.join("config.toml"), d.join("state.json"));
    std::fs::write(&config, "theme = \"nc\"\n").unwrap();

    // Every change of config.toml (Settings, Read this folder too, Ask's model, a preview tool,
    // an engine option, the examples written before opening it) goes through one function.
    let mut ch = serde_json::Map::new();
    ch.insert("text_roots".into(), serde_json::json!(["/somewhere"]));
    assert!(settings::save_to(&config, &ch).is_err(), "a settings change is refused");
    assert!(preview_tools::set_choice(&config, "pdf", "off").is_err(), "a preview tool is refused");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "theme = \"nc\"\n", "config.toml as it was");

    // And every change of state.json (hints, notices, servers allowed, the guide, repositories,
    // sessions): refused without an error, so no page or status line shows one.
    let mut st = AppState::default();
    st.notices_dismissed.push("x".into());
    st.save_to(&state).unwrap();
    assert!(!state.exists(), "state.json not written");
    assert_eq!(AppState::load_from(&state).notices_dismissed, ["x"], "but kept until the picker closes");
    let mut later = AppState::default();
    later.reload_from(&state);
    assert_eq!(later.notices_dismissed, ["x"], "a copy read again before a change keeps it too");
    std::fs::remove_dir_all(&d).unwrap();
}

/// Non-test source of every crate of the workspace and the desktop app.
fn sources() -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    let mut todo = vec![root.join("crates"), root.join("gui/src-tauri/src")];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if p.is_dir() {
                if !matches!(name.as_str(), "tests" | "target" | "benches") {
                    todo.push(p);
                }
            } else if name.ends_with(".rs") {
                let text = std::fs::read_to_string(&p).unwrap();
                // A file's tests write files of their own.
                let text = text.split("#[cfg(test)]\nmod tests").next().unwrap().to_string();
                out.push((p, text));
            }
        }
    }
    out
}

/// A new way to write either file would slip past `keep_own_files`: config.toml and state.json
/// are replaced only in the functions that ask it, and by the 1.x migration, which asks it too.
/// shortcut: a path taken on one line and written on another is not seen; a review must catch that.
#[test]
fn config_and_state_are_written_only_where_pick_mode_is_asked() {
    let allowed = ["config.rs", "state.rs", "migrate.rs", "fs.rs"].map(|f| Path::new("crates/coxswain-core/src").join(f));
    let all = sources();
    assert!(all.iter().any(|(p, t)| p.ends_with("state.rs") && t.contains("fs::replace(")), "the sources were found");
    for (p, text) in all {
        for (i, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap();
            let at = format!("{}:{}", p.display(), i + 1);
            if code.contains("fs::replace(") {
                assert!(allowed.iter().any(|a| p.ends_with(a)), "{at}: config.toml and state.json are written through config::update_config or AppState::save_to");
            }
            let own = code.contains("Config::path()") || code.contains("AppState::path()");
            let writes = ["fs::write(", "File::create(", "OpenOptions::new(", "fs::remove_file(", "fs::rename("].iter().any(|w| code.contains(w));
            assert!(!(own && writes), "{at}: written outside config::update_config and AppState::save_to, so a picker would write it");
        }
    }
}

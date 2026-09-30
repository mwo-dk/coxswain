//! What Coxswain remembers between runs: the GUI session, favorites, tags, notes and recent
//! repositories. One JSON file in the data dir, written atomically.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FavoriteGroup {
    pub name: String,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    /// Tabs, splits and view modes. Owned by the GUI; opaque here.
    pub session: serde_json::Value,
    pub favorites: Vec<FavoriteGroup>,
    /// Path -> tag color name.
    pub tags: BTreeMap<PathBuf, String>,
    /// Folder -> note text.
    pub notes: BTreeMap<PathBuf, String>,
    /// Most recent first.
    pub recent_repos: Vec<PathBuf>,
    /// Unix time of the last update check, and the latest version it found.
    pub update_checked: u64,
    pub latest_version: String,
    /// Notices (`notices`) dismissed, and the version last started, to tell of the next one.
    #[serde(default)]
    pub notices_dismissed: Vec<String>,
    #[serde(default)]
    pub seen_version: String,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            session: serde_json::Value::Null,
            // Named in the language in use when the state is first created.
            favorites: vec![FavoriteGroup { name: crate::t!("state.favourites"), paths: vec![] }],
            tags: BTreeMap::new(),
            notes: BTreeMap::new(),
            recent_repos: vec![],
            update_checked: 0,
            latest_version: String::new(),
            notices_dismissed: vec![],
            seen_version: String::new(),
        }
    }
}

const RECENT_MAX: usize = 12;

impl AppState {
    pub fn path() -> Option<PathBuf> {
        Some(dirs::data_dir()?.join("coxswain").join("state.json"))
    }

    /// Missing file = defaults. A corrupt file is moved aside to `state.json.bad` so saving
    /// the defaults never destroys it.
    pub fn load_from(path: &Path) -> AppState {
        let Ok(text) = std::fs::read_to_string(path) else { return AppState::default() };
        serde_json::from_str(&text).unwrap_or_else(|_| {
            let _ = std::fs::rename(path, path.with_extension("json.bad"));
            AppState::default()
        })
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }

    pub fn load() -> AppState {
        AppState::path().map(|p| AppState::load_from(&p)).unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        match AppState::path() {
            Some(p) => self.save_to(&p),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "no data directory")),
        }
    }

    /// Moves `root` to the top of the recent repositories; false when it was there already.
    pub fn touch_repo(&mut self, root: &Path) -> bool {
        if self.recent_repos.first().is_some_and(|r| r == root) {
            return false;
        }
        self.recent_repos.retain(|r| r != root);
        self.recent_repos.insert(0, root.to_path_buf());
        self.recent_repos.truncate(RECENT_MAX);
        true
    }

    /// Empty color or text removes the entry.
    pub fn set_tag(&mut self, path: &Path, color: &str) {
        if color.is_empty() {
            self.tags.remove(path);
        } else {
            self.tags.insert(path.to_path_buf(), color.into());
        }
    }

    pub fn set_note(&mut self, dir: &Path, text: &str) {
        if text.trim().is_empty() {
            self.notes.remove(dir);
        } else {
            self.notes.insert(dir.to_path_buf(), text.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip_and_corrupt_file() {
        let d = std::env::temp_dir().join(format!("coxswain-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let f = d.join("state.json");
        assert_eq!(AppState::load_from(&f), AppState::default());

        let mut s = AppState::default();
        s.set_tag(Path::new("/a"), "red");
        s.set_note(Path::new("/p"), "todo: ship it");
        for i in 0..20 {
            s.touch_repo(Path::new(&format!("/r{i}")));
        }
        assert!(s.touch_repo(Path::new("/r5")));
        assert!(!s.touch_repo(Path::new("/r5")));
        s.session = serde_json::json!({"tabs": [1, 2]});
        s.save_to(&f).unwrap();
        let back = AppState::load_from(&f);
        assert_eq!(back, s);
        assert_eq!(back.recent_repos.len(), RECENT_MAX);
        assert_eq!(back.recent_repos[0], Path::new("/r5"));

        s.set_tag(Path::new("/a"), "");
        s.set_note(Path::new("/p"), "  ");
        assert!(s.tags.is_empty() && s.notes.is_empty());

        std::fs::write(&f, "{not json").unwrap();
        assert_eq!(AppState::load_from(&f), AppState::default());
        assert!(d.join("state.json.bad").exists());
        std::fs::remove_dir_all(d).unwrap();
    }
}

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
    /// The ending of the last archive packed (".7z"), suggested next time; empty: ".zip".
    pub pack_ending: String,
    /// The first-run guide was gone through or skipped.
    pub guide_seen: bool,
    /// What the start of 2.0 renamed in config.toml (`migrate`), for its notice.
    pub migrated: Vec<String>,
    /// How often each hint on the status line was shown (`menu::hint`).
    pub hints_shown: BTreeMap<String, u32>,
    /// Unix time of the last round of container image updates (`images`).
    pub images_checked: u64,
    /// Container image -> Unix time a preview last ran in it (`images::stale`).
    pub images_used: BTreeMap<String, u64>,
    /// The file is there but could not be read (no permission, say): it is never saved over.
    #[serde(skip)]
    pub unreadable: bool,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            session: serde_json::Value::Null,
            // Named in the language in use when the state is first created.
            favorites: vec![FavoriteGroup {
                name: crate::t!("state.favourites"),
                paths: vec![],
            }],
            tags: BTreeMap::new(),
            notes: BTreeMap::new(),
            recent_repos: vec![],
            update_checked: 0,
            latest_version: String::new(),
            notices_dismissed: vec![],
            seen_version: String::new(),
            pack_ending: String::new(),
            guide_seen: false,
            migrated: vec![],
            hints_shown: BTreeMap::new(),
            images_checked: 0,
            images_used: BTreeMap::new(),
            unreadable: false,
        }
    }
}

const RECENT_MAX: usize = 12;

impl AppState {
    pub fn path() -> Option<PathBuf> {
        Some(dirs::data_dir()?.join("coxswain").join("state.json"))
    }

    /// Missing file = defaults. A corrupt file (not JSON, not UTF-8) is moved aside to
    /// `state.json.bad` (`state.json.<time>.bad` when that is taken) so saving the defaults
    /// never destroys it, nor an earlier one.
    /// One that cannot be read gives defaults that are never saved over it.
    pub fn load_from(path: &Path) -> AppState {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return AppState::default(),
            Err(_) => {
                return AppState {
                    unreadable: true,
                    ..AppState::default()
                };
            }
        };
        serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            let mut bad = path.with_extension("json.bad");
            if bad.exists() {
                bad = path.with_extension(format!("json.{}.bad", crate::fs::now()));
            }
            let _ = std::fs::rename(path, bad);
            AppState::default()
        })
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if self.unreadable {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "{}: could not be read, so it is not saved over",
                    path.display()
                ),
            ));
        }
        // Notes, tags, favourites and recent folders: readable by the user alone.
        crate::fs::replace(path, &serde_json::to_vec_pretty(self)?, true)
    }

    /// A copy kept in memory (the desktop app's) takes what was written since it was read (hint
    /// counts, dismissed notices, the terminal app's changes), before it is changed and saved.
    /// A missing or corrupt file leaves the copy as it is; one that cannot be read is never
    /// saved over.
    pub fn reload_from(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) => {
                if let Ok(st) = serde_json::from_slice(&bytes) {
                    *self = st;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => self.unreadable = true,
        }
    }

    pub fn reload(&mut self) {
        if let Some(p) = AppState::path() {
            self.reload_from(&p);
        }
    }

    pub fn load() -> AppState {
        AppState::path()
            .map(|p| AppState::load_from(&p))
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        match AppState::path() {
            Some(p) => self.save_to(&p),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "no data directory")),
        }
    }

    /// Whether the first-run guide is due: on the very first start (no version was started
    /// before; one updated from 1.x has, and is told what is new instead), until it is closed.
    pub fn guide_due(&self) -> bool {
        !self.guide_seen && self.seen_version.is_empty()
    }

    /// The ending Pack suggests: the last one used, else ".zip".
    pub fn pack_ending(&self) -> &str {
        if crate::archive::pack_format(&self.pack_ending).is_some() {
            &self.pack_ending
        } else {
            ".zip"
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
        let bad = |d: &Path| {
            std::fs::read_dir(d)
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".bad"))
                .count()
        };
        assert_eq!(bad(&d), 1);
        // Not UTF-8 is corrupt too.
        std::fs::write(&f, b"\xff\xfe{}").unwrap();
        assert_eq!(AppState::load_from(&f), AppState::default());
        assert!(!f.exists() && bad(&d) == 2, "the first one is kept");
        // A file that cannot be read is never saved over.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            s.save_to(&f).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
            let locked = AppState::load_from(&f);
            // Root reads it anyway.
            if locked.unreadable {
                assert!(locked.save_to(&f).is_err());
            }
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(AppState::load_from(&f), s);
        }
        // A copy kept in memory takes what another app saved since.
        let mut kept = s.clone();
        let mut other = AppState::load_from(&f);
        other.hints_shown.insert("find_scope".into(), 3);
        other.save_to(&f).unwrap();
        kept.reload_from(&f);
        assert_eq!(kept.hints_shown.get("find_scope"), Some(&3));
        std::fs::write(&f, "{not json").unwrap();
        kept.reload_from(&f);
        assert_eq!(
            kept.hints_shown.get("find_scope"),
            Some(&3),
            "corrupt: kept as it was"
        );
        std::fs::remove_dir_all(d).unwrap();
    }
}

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
    /// Most recent first: the last `RECENT_MAX` visited, and every pinned one.
    pub recent_repos: Vec<PathBuf>,
    /// Repositories pinned in the sidebar: they never drop out of `recent_repos`.
    pub pinned_repos: Vec<PathBuf>,
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
    /// The servers a Subversion or CVS history may be read from (`wc::server`): each asked once.
    pub history_servers: Vec<String>,
    /// What a newer Coxswain keeps here that this one does not know: saved as it was, so going
    /// back to an older version and forward again loses none of it.
    #[serde(flatten)]
    pub newer: serde_json::Map<String, serde_json::Value>,
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
            pinned_repos: vec![],
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
            history_servers: vec![],
            newer: serde_json::Map::new(),
            unreadable: false,
        }
    }
}

/// How many repositories that are not pinned the sidebar keeps.
const RECENT_MAX: usize = 8;

/// A repository in the sidebar's list.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Repo {
    pub path: PathBuf,
    pub pinned: bool,
}

impl AppState {
    pub fn path() -> Option<PathBuf> {
        Some(crate::dirs::data_dir()?.join("coxswain").join("state.json"))
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
        self.trim_repos();
        true
    }

    /// Drops the oldest repositories that are not pinned beyond `RECENT_MAX`.
    fn trim_repos(&mut self) {
        let mut kept = 0;
        let pinned = &self.pinned_repos;
        self.recent_repos.retain(|r| {
            pinned.contains(r) || {
                kept += 1;
                kept <= RECENT_MAX
            }
        });
    }

    /// Pins `root` (it never drops out) or unpins it (it keeps its place by its last visit, and
    /// goes when that is older than the last `RECENT_MAX` others); false when nothing changed.
    pub fn pin_repo(&mut self, root: &Path, pin: bool) -> bool {
        if self.pinned_repos.iter().any(|r| r == root) == pin {
            return false;
        }
        if pin {
            self.pinned_repos.push(root.to_path_buf());
            if !self.recent_repos.iter().any(|r| r == root) {
                self.recent_repos.insert(0, root.to_path_buf());
            }
        } else {
            self.pinned_repos.retain(|r| r != root);
            self.trim_repos();
        }
        true
    }

    /// Takes `root` out of the list, pinned or not; false when it was not there.
    pub fn remove_repo(&mut self, root: &Path) -> bool {
        let n = self.recent_repos.len() + self.pinned_repos.len();
        self.recent_repos.retain(|r| r != root);
        self.pinned_repos.retain(|r| r != root);
        n != self.recent_repos.len() + self.pinned_repos.len()
    }

    /// The sidebar's list: the pinned ones first, then the rest, each part by folder name
    /// (case and accents aside, the whole path when two names are the same) or, with `by_name`
    /// false, most recently visited first.
    pub fn repos(&self, by_name: bool) -> Vec<Repo> {
        let mut v: Vec<Repo> = self
            .recent_repos
            .iter()
            .map(|p| Repo {
                path: p.clone(),
                pinned: self.pinned_repos.contains(p),
            })
            .collect();
        let name = |p: &Path| {
            let n = p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            (crate::features::fold(&n), crate::features::fold(&p.to_string_lossy()))
        };
        if by_name {
            v.sort_by_cached_key(|r| (!r.pinned, name(&r.path)));
        } else {
            v.sort_by_key(|r| !r.pinned);
        }
        v
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
    fn repos_pin_remove_and_order() {
        let mut s = AppState::default();
        let p = |n: &str| PathBuf::from(format!("/src/{n}"));
        for n in ["Zeta", "alpha", "Émile", "beta"] {
            s.touch_repo(&p(n));
        }
        let names = |s: &AppState, by_name| s.repos(by_name).iter().map(|r| r.path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names(&s, true), ["alpha", "beta", "Émile", "Zeta"], "case and accents aside");
        assert_eq!(names(&s, false), ["beta", "Émile", "alpha", "Zeta"], "newest first");
        assert!(s.pin_repo(&p("Zeta"), true));
        assert!(!s.pin_repo(&p("Zeta"), true));
        assert_eq!(names(&s, true)[0], "Zeta", "pinned first");
        assert!(s.repos(true)[0].pinned);
        // Twenty more visits: the pinned one stays, eight unpinned are kept.
        for i in 0..20 {
            s.touch_repo(&p(&format!("r{i}")));
        }
        assert_eq!(s.recent_repos.len(), RECENT_MAX + 1);
        assert!(s.recent_repos.contains(&p("Zeta")));
        assert!(s.pin_repo(&p("Zeta"), false));
        assert_eq!(s.recent_repos.len(), RECENT_MAX, "unpinned: the oldest goes");
        assert!(s.remove_repo(&p("r19")));
        assert!(!s.remove_repo(&p("r19")));
        assert!(!s.recent_repos.contains(&p("r19")));
    }

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
        // What a newer version wrote is saved again as it was.
        std::fs::write(&f, r#"{"tags": {"/a": "red"}, "later_thing": {"x": [1, 2]}}"#).unwrap();
        let older = AppState::load_from(&f);
        older.save_to(&f).unwrap();
        let back: serde_json::Value = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
        assert_eq!(back["later_thing"], serde_json::json!({"x": [1, 2]}));
        assert_eq!(back["tags"]["/a"], "red");
        s.save_to(&f).unwrap();
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

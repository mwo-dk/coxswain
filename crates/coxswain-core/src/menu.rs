//! What can be done with what is under the cursor, in both apps: the action menu (Shift+F10,
//! the Menu key) lists the actions that fit, under the headings of F1 and F9, and the status
//! line gives one short hint that fits, a few times each. Both are worked out from a
//! `Subject`, which each app makes from its panel.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::config::{Action, Config, Group};

/// What kind of folder the panel shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// A folder on disk.
    #[default]
    Disk,
    /// Inside an archive: changed by writing the archive anew.
    Archive,
    /// Inside a disk image: read-only, files are copied out.
    DiskImage,
    /// A git history or one of its commits: read-only.
    History,
    /// The list of a repository's branches.
    Branches,
    /// A ZFS snapshot or the list of them: read-only.
    Snapshot,
    /// A FreeBSD package's list of files: read-only.
    Package,
}

/// What the menu and the hints are about: the marked entries, or the one under the cursor
/// when none is marked (none on `..`), and where they are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Subject {
    pub files: usize,
    pub folders: usize,
    /// Of the files, those that are archives Coxswain opens.
    pub archives: usize,
    /// Of the files, the disk images that would open like folders if that option were on (0
    /// while it is on).
    pub disk_images: usize,
    /// Of the files, the PDFs (*Make PDF searchable*).
    pub pdfs: usize,
    /// Of the files, chess games or positions (`chess::format`).
    pub chess: usize,
    /// Of the files, videos and songs (`media::is_media`), and pictures ImageMagick takes
    /// (`media::is_picture`).
    pub media: usize,
    pub pictures: usize,
    /// Of the files, geodata by its name (`geo::is_vector`, and TIFF, which GDAL is asked about).
    pub geo: usize,
    /// The one entry is the one under the cursor (actions on a single entry take that one).
    pub one: bool,
    /// How many are marked.
    pub marked: usize,
    pub place: Place,
    /// The folder is in a git repository, on a ZFS dataset.
    pub git: bool,
    pub zfs: bool,
    /// The folder may hold files of installed packages (`bsd::packages_here`).
    pub packages: bool,
    /// The panel shows a drive's top folder, or the cursor is on a folder measured at 1 GB or
    /// more: where Space is worth a hint.
    pub big: bool,
}

impl Subject {
    /// What kind of entries these are, for when a hint comes up anew.
    fn kind(&self) -> (Place, bool, bool, bool, bool) {
        (
            self.place,
            self.files > 0,
            self.folders > 0,
            self.archives > 0,
            self.marked > 1,
        )
    }
    fn count(&self) -> usize {
        self.files + self.folders
    }
    fn single(&self) -> bool {
        self.count() == 1 && self.one
    }
    /// Things can be changed here: on disk, or inside an archive.
    fn writable(&self) -> bool {
        matches!(self.place, Place::Disk | Place::Archive)
    }
}

impl Action {
    /// The action fits `s`, and belongs in its action menu.
    pub fn applies(self, s: &Subject) -> bool {
        use Action::*;
        let (n, disk) = (s.count(), s.place == Place::Disk);
        match self {
            // A file in a history is not opened: Enter says so.
            Open => s.single() && !(s.place == Place::History && s.files == 1),
            View => s.single() && s.files == 1,
            Edit | EditHere => s.single() && s.files == 1 && disk,
            CopyPath => s.single() && disk,
            Copy => n > 0 && s.place != Place::Branches,
            Move | Delete => n > 0 && s.writable(),
            Rename => s.single() && s.writable(),
            BatchRename => n > 1 && disk,
            NewFolder => s.writable(),
            ClipCopy => n > 0 && disk,
            Properties => s.single() && disk,
            Pack => n > 0 && disk,
            Extract => s.archives > 0 && disk,
            DiskImages => s.disk_images > 0 && disk,
            SearchablePdf => s.pdfs > 0 && disk,
            ConvertMedia => s.media > 0 && disk,
            EditPictures => s.pictures > 0 && disk,
            ConvertGeodata => s.geo > 0 && disk,
            History => s.git && disk && (n == 0 || s.single()),
            Branches => s.git && disk,
            SwitchBranch => s.place == Place::Branches && s.single(),
            NewBranch => s.place == Place::Branches,
            Snapshots => {
                s.zfs && (disk && (n == 0 || s.folders == 1 && s.one) || s.place == Place::Snapshot)
            }
            Package => s.packages && disk && s.single() && s.files == 1,
            Tag => n > 0 && disk,
            Notes => disk,
            Chess => s.single() && s.chess == 1 && matches!(s.place, Place::Disk | Place::Archive),
            Worktrees | GitTool | GitConfig => s.git && disk,
            // Find, anywhere; duplicates among the folders, or in this one.
            Search | SearchText | Ask => true,
            Duplicates => disk && (s.folders > 0 || n == 0),
            // The folder under the cursor, else the panel's.
            SpaceView => disk,
            // On `..` with nothing marked: the panels themselves.
            SwapPanels | SameDir | TogglePanels => n == 0,
            // What Coxswain can do, and Settings, from any menu.
            Features | Settings => true,
            _ => false,
        }
    }
}

/// The headings the action menu shows, in its order: opening first, then the app (Settings,
/// what Coxswain can do), so Settings is found near the top, then looking and the rest.
const GROUPS: [Group; 8] = [
    Group::Moving,
    Group::App,
    Group::Viewing,
    Group::Files,
    Group::Archives,
    Group::Search,
    Group::Git,
    Group::Panels,
];

/// The action menu for `s`: under each heading the actions that fit, the most used first.
/// `gui`: the desktop app, which has actions the terminal app lacks.
pub fn actions(s: &Subject, gui: bool) -> Vec<(Group, Vec<Action>)> {
    GROUPS
        .iter()
        .map(|&g| {
            (
                g,
                g.actions()
                    .filter(|a| a.applies(s) && (gui || !a.gui_only()))
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

// ---------------------------------------------------------------- hints

/// How often each hint shows before it is not shown again.
pub const TIMES: u32 = 3;

/// The hints, the first that fits and is not used up showing: its id (its text is `hint.<id>`)
/// and the actions its text names, by placeholder.
const HINTS: &[(&str, &[(&str, Action)])] = &[
    // Before the general ones: a chess file is rare, and what to press on it is not guessed.
    ("chess", &[("key", Action::Chess)]),
    ("menu", &[("key", Action::ActionMenu)]),
    ("space", &[("key", Action::SpaceView)]),
    ("find", &[("key", Action::Search)]),
    ("features", &[("key", Action::Help)]),
    ("settings", &[("key", Action::Settings)]),
    ("quick", &[]),
    (
        "panels",
        &[
            ("swap", Action::SwapPanels),
            ("same", Action::SameDir),
            ("off", Action::TogglePanels),
        ],
    ),
    ("marked", &[("copy", Action::Copy), ("move", Action::Move)]),
    ("in_archive", &[("copy", Action::Copy)]),
    ("in_disk_image", &[("copy", Action::Copy)]),
    ("disk_image", &[("menu", Action::ActionMenu)]),
    ("archive", &[("extract", Action::Extract)]),
    ("pdf", &[("key", Action::SearchablePdf)]),
    ("media", &[("menu", Action::ActionMenu)]),
    ("geo", &[("menu", Action::ActionMenu)]),
    (
        "folder",
        &[("pack", Action::Pack), ("rename", Action::Rename)],
    ),
    (
        "file",
        &[
            ("view", Action::View),
            ("edit", Action::Edit),
            ("rename", Action::Rename),
        ],
    ),
    ("branches", &[("switch", Action::SwitchBranch)]),
    (
        "git",
        &[("branches", Action::Branches), ("history", Action::History)],
    ),
    ("git_tool", &[("key", Action::GitTool)]),
    ("zfs", &[("snapshots", Action::Snapshots)]),
];

fn fits(id: &str, s: &Subject) -> bool {
    let disk = s.place == Place::Disk;
    match id {
        "menu" => s.count() > 0,
        "find" | "features" | "settings" => true,
        "quick" => disk && s.count() > 0,
        "panels" => disk && s.count() == 0,
        "marked" => s.marked > 1 && s.writable(),
        "in_archive" => s.place == Place::Archive && s.files > 0,
        "in_disk_image" => s.place == Place::DiskImage && s.files > 0,
        "disk_image" => disk && s.single() && s.disk_images == 1,
        "pdf" => disk && s.pdfs > 0,
        "media" => disk && s.media + s.pictures > 0,
        "geo" => disk && s.geo > 0,
        "archive" => disk && s.single() && s.archives == 1,
        "chess" => disk && s.single() && s.chess == 1,
        "folder" => disk && s.single() && s.folders == 1,
        "file" => disk && s.single() && s.files == 1 && s.archives == 0,
        "branches" => s.place == Place::Branches && s.single(),
        "git" | "git_tool" => disk && s.git,
        "zfs" => disk && s.zfs,
        "space" => disk && s.big,
        _ => false,
    }
}

/// The keys of `a` as the hint names them: "Shift+F10 / Menu".
fn keys(cfg: &Config, a: Action) -> Option<String> {
    cfg.keys.get(&a).filter(|k| !k.is_empty())?;
    Some(cfg.keys_shown(a))
}

/// The hint for `s`, given how often each was shown: its id and text. None when hints are off,
/// or every one that fits has been shown `TIMES` times, or one of its actions has no key.
pub fn pick(
    s: &Subject,
    cfg: &Config,
    shown: &BTreeMap<String, u32>,
) -> Option<(&'static str, String)> {
    if !cfg.hints {
        return None;
    }
    HINTS
        .iter()
        .filter(|(id, _)| fits(id, s) && shown.get(*id).copied().unwrap_or(0) < TIMES)
        .find_map(|(id, acts)| {
            let named: Option<Vec<(&str, String)>> = acts
                .iter()
                .map(|&(p, a)| keys(cfg, a).map(|k| (p, k)))
                .collect();
            let named = named?;
            let args: Vec<(&str, &str)> = named.iter().map(|(p, k)| (*p, k.as_str())).collect();
            Some((*id, crate::i18n::tr(&format!("hint.{id}"), &args)))
        })
}

/// `pick`, counted in `shown`, and whether that changed (then the caller saves it). `last` is
/// the subject the hint on show (its id) was picked for. A hint counts as shown once more when
/// it comes up anew: another hint was on show, or the cursor went to another kind of entry (a
/// file after a folder, several marked, into an archive). Moving from file to file keeps it,
/// and counts it once.
pub fn hint_in(
    s: &Subject,
    cfg: &Config,
    last: Option<(&Subject, &str)>,
    shown: &mut BTreeMap<String, u32>,
) -> (Option<(&'static str, String)>, bool) {
    let Some(h) = pick(s, cfg, shown) else {
        return (None, false);
    };
    let anew = last.is_none_or(|(was, id)| id != h.0 || was.kind() != s.kind());
    if anew {
        *shown.entry(h.0.to_string()).or_default() += 1;
    }
    (Some(h), anew)
}

/// The counts as `counted` last read or wrote them, with state.json's time then and when.
type Shown = (
    Option<std::time::SystemTime>,
    std::time::Instant,
    BTreeMap<String, u32>,
);
static SHOWN: std::sync::Mutex<Option<Shown>> = std::sync::Mutex::new(None);

/// When state.json was last written.
fn stamp() -> Option<std::time::SystemTime> {
    std::fs::metadata(crate::state::AppState::path()?)
        .ok()?
        .modified()
        .ok()
}

/// `f` on the hint counts in state.json, tried on the counts in memory first: the file is read
/// and written only when `f` says a count changed, not each time the cursor moves. Read again
/// when another app wrote it (a reset of the hints), or half a minute went by.
fn counted<T>(f: impl Fn(&mut BTreeMap<String, u32>) -> (T, bool)) -> T {
    let mut known = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    let now = stamp();
    let fresh = |k: &Shown| k.0 == now && k.1.elapsed() < std::time::Duration::from_secs(30);
    if !known.as_ref().is_some_and(fresh) {
        let shown = crate::state::AppState::load().hints_shown;
        *known = Some((now, std::time::Instant::now(), shown));
    }
    let mut probe = known.as_ref().unwrap().2.clone();
    let (out, changed) = f(&mut probe);
    if !changed {
        return out;
    }
    // What another app wrote since (a reset, its own counts) is read first.
    let mut st = crate::state::AppState::load();
    let (out, changed) = f(&mut st.hints_shown);
    if changed {
        let _ = st.save();
    }
    *known = Some((stamp(), std::time::Instant::now(), st.hints_shown));
    out
}

/// `hint_in`, with the counts kept in state.json, written only when one changed.
pub fn hint(
    s: &Subject,
    cfg: &Config,
    last: Option<(&Subject, &str)>,
) -> Option<(&'static str, String)> {
    if !cfg.hints {
        return None;
    }
    counted(|shown| hint_in(s, cfg, last, shown))
}

/// Whether the hint `id`, shown where it applies rather than on the status line (Find's
/// scope), is still to be shown; counted in `shown` as shown once more when it is (so `shown`
/// changed exactly when this is true).
pub fn once_in(id: &str, cfg: &Config, shown: &mut BTreeMap<String, u32>) -> bool {
    if !cfg.hints {
        return false;
    }
    let n = shown.entry(id.to_string()).or_default();
    if *n >= TIMES {
        return false;
    }
    *n += 1;
    true
}

/// `once_in`, with the counts kept in state.json.
pub fn once(id: &str, cfg: &Config) -> bool {
    if !cfg.hints {
        return false;
    }
    counted(|shown| {
        let due = once_in(id, cfg, shown);
        (due, due)
    })
}

/// Every hint shows again (`coxswain --hints reset`, Settings → Behaviour). An app that keeps
/// the state in memory clears its `hints_shown` instead and saves that.
pub fn reset() -> std::io::Result<()> {
    let mut st = crate::state::AppState::load();
    st.hints_shown.clear();
    *SHOWN.lock().unwrap_or_else(|e| e.into_inner()) = None;
    st.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(s: &Subject, gui: bool) -> Vec<&'static str> {
        actions(s, gui)
            .into_iter()
            .flat_map(|(_, v)| v)
            .map(Action::name)
            .collect()
    }

    #[test]
    fn menu_fits_what_is_under_the_cursor() {
        let file = Subject {
            files: 1,
            one: true,
            ..Default::default()
        };
        let n = names(&file, true);
        assert_eq!(
            &n[..5],
            ["open", "features", "settings", "view", "edit"],
            "opening first, Settings near the top, then looking: {n:?}"
        );
        for a in ["copy", "move", "rename", "delete", "pack", "properties"] {
            assert!(n.contains(&a), "{a} in {n:?}");
        }
        assert!(!n.contains(&"extract") && !n.contains(&"history") && !n.contains(&"action_menu"));

        let folder = Subject {
            folders: 1,
            one: true,
            git: true,
            ..Default::default()
        };
        let n = names(&folder, false);
        assert!(
            !n.contains(&"view")
                && !n.contains(&"edit")
                && n.contains(&"pack")
                && n.contains(&"history")
                && n.contains(&"branches")
        );
        assert!(
            !n.contains(&"clip_copy"),
            "the terminal app has no clipboard: {n:?}"
        );

        let archive = Subject {
            files: 1,
            archives: 1,
            one: true,
            ..Default::default()
        };
        assert!(names(&archive, true).contains(&"extract"));

        // Several marked: what works on all of them, nothing for one.
        let marked = Subject {
            files: 2,
            folders: 1,
            marked: 3,
            ..Default::default()
        };
        let n = names(&marked, true);
        assert!(
            n.contains(&"copy")
                && n.contains(&"batch_rename")
                && !n.contains(&"rename")
                && !n.contains(&"open"),
            "{n:?}"
        );

        // Read-only places: nothing that changes.
        let snap = Subject {
            files: 1,
            one: true,
            place: Place::Snapshot,
            zfs: true,
            ..Default::default()
        };
        let n = names(&snap, true);
        assert!(
            n.contains(&"copy")
                && n.contains(&"snapshots")
                && !n.contains(&"delete")
                && !n.contains(&"rename"),
            "{n:?}"
        );

        // On `..` with nothing marked: what is done to the folder itself.
        let here = Subject {
            git: true,
            ..Default::default()
        };
        let n = names(&here, true);
        for a in [
            "new_folder",
            "notes",
            "search",
            "ask",
            "duplicates",
            "history",
            "worktrees",
            "swap_panels",
            "same_dir",
            "toggle_panels",
        ] {
            assert!(n.contains(&a), "{a} in {n:?}");
        }
        assert!(!n.contains(&"tag") && !n.contains(&"copy"), "{n:?}");
        // Find on a file too; duplicates and the panels not.
        let n = names(&file, true);
        assert!(
            n.contains(&"search")
                && n.contains(&"tag")
                && !n.contains(&"duplicates")
                && !n.contains(&"swap_panels"),
            "{n:?}"
        );
        assert!(
            !names(&file, false).contains(&"tag"),
            "the terminal app has no colour tags"
        );
    }

    #[test]
    fn hints_count_when_they_come_up_anew() {
        let cfg = Config::default();
        let folder = Subject {
            folders: 1,
            one: true,
            ..Default::default()
        };
        let other = folder.clone();
        let mut shown = BTreeMap::new();
        let (h, changed) = hint_in(&folder, &cfg, None, &mut shown);
        let id = h.unwrap().0;
        assert!(changed && shown[id] == 1);
        // The same kind again (folder to folder): no count, nothing to save.
        assert!(!hint_in(&other, &cfg, Some((&folder, id)), &mut shown).1);
        assert_eq!(shown[id], 1);
        assert!(once_in("find_scope", &cfg, &mut shown));
        shown.insert("find_scope".into(), TIMES);
        assert!(!once_in("find_scope", &cfg, &mut shown));
    }

    #[test]
    fn hints_show_a_few_times_then_never() {
        let cfg = Config::default();
        assert_eq!(
            cfg.handy_key(Action::Search),
            Some("Ctrl+F"),
            "Find's scope names Ctrl+F, not Alt+F7"
        );
        assert_eq!(cfg.handy_key(Action::Copy), Some("F5"));
        let folder = Subject {
            folders: 1,
            one: true,
            ..Default::default()
        };
        let mut shown = BTreeMap::new();
        assert_eq!(pick(&folder, &cfg, &shown).map(|h| h.0), Some("menu"));
        assert!(pick(&folder, &cfg, &shown).unwrap().1.contains("Shift+F10"));
        shown.insert("menu".to_string(), TIMES);
        assert_eq!(pick(&folder, &cfg, &shown).map(|h| h.0), Some("find"));
        assert!(pick(&folder, &cfg, &shown).unwrap().1.contains("Ctrl+F"));
        shown.insert("find".to_string(), TIMES);
        assert_eq!(pick(&folder, &cfg, &shown).map(|h| h.0), Some("features"));
        assert!(pick(&folder, &cfg, &shown).unwrap().1.contains("F1"));
        shown.insert("features".to_string(), TIMES);
        assert_eq!(pick(&folder, &cfg, &shown).map(|h| h.0), Some("settings"));
        assert!(pick(&folder, &cfg, &shown).unwrap().1.contains("F12"));
        shown.insert("settings".to_string(), TIMES);
        assert_eq!(pick(&folder, &cfg, &shown).map(|h| h.0), Some("quick"));
        shown.insert("quick".to_string(), TIMES);
        let parent = Subject::default();
        let text = pick(&parent, &cfg, &shown).unwrap().1;
        assert!(
            text.contains("Ctrl+U") && text.contains("Alt+O") && text.contains("Ctrl+O"),
            "on ..: the panels: {text}"
        );
        let (id, text) = pick(&folder, &cfg, &shown).unwrap();
        assert_eq!(id, "folder");
        assert!(
            text.contains("Alt+F5") && text.contains("Shift+F6"),
            "{text}"
        );
        shown.insert("folder".to_string(), TIMES);
        assert_eq!(pick(&folder, &cfg, &shown), None);
        // A big folder, or a drive: where the space goes.
        let big = Subject { big: true, ..folder.clone() };
        let (id, text) = pick(&big, &cfg, &shown).unwrap();
        assert_eq!(id, "space");
        assert!(text.contains("Alt+U"), "{text}");
        assert!(names(&big, false).contains(&"space_view"));
        let marked = Subject {
            files: 3,
            marked: 3,
            ..Default::default()
        };
        assert!(pick(&marked, &cfg, &shown).unwrap().1.contains("F5"));
        // Off in Settings, or with a key unbound: none.
        assert_eq!(
            pick(
                &marked,
                &Config {
                    hints: false,
                    ..Config::default()
                },
                &shown
            ),
            None
        );
        let mut unbound = Config::default();
        unbound.keys.insert(Action::Copy, vec![]);
        assert_eq!(pick(&marked, &unbound, &shown), None);
    }
}

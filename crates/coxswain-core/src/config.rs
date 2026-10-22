//! Shared configuration: key bindings, color schemes, glyphs, user menu.
//! One TOML file, read by both the TUI and the GUI.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

// ---------------------------------------------------------------- keys

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyCode {
    F(u8),
    Char(char),
    Enter,
    Esc,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    /// The context-menu key, beside the right Ctrl or Alt.
    Menu,
}

/// A key chord. Letters are stored lowercase; Shift is explicit.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Key {
    pub fn new(code: KeyCode, ctrl: bool, alt: bool, shift: bool) -> Self {
        // Normalise so "Shift+a" and "A" are the same chord.
        let (code, shift) = match code {
            KeyCode::Char(c) if c.is_ascii_uppercase() => (KeyCode::Char(c.to_ascii_lowercase()), true),
            KeyCode::Char(' ') => (KeyCode::Char(' '), shift),
            KeyCode::Char(c) if !c.is_ascii_alphabetic() => (KeyCode::Char(c), false),
            c => (c, shift),
        };
        Key { code, ctrl, alt, shift }
    }
}

impl FromStr for Key {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        let mut rest = s.trim();
        loop {
            let lower = rest.to_ascii_lowercase();
            let strip = |p: &str| lower.starts_with(p) && rest.len() > p.len();
            if strip("ctrl+") {
                ctrl = true;
                rest = &rest[5..];
            } else if strip("alt+") {
                alt = true;
                rest = &rest[4..];
            } else if strip("shift+") {
                shift = true;
                rest = &rest[6..];
            } else {
                break;
            }
        }
        let code = match rest.to_ascii_lowercase().as_str() {
            "enter" | "return" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "backspace" => KeyCode::Backspace,
            "delete" | "del" => KeyCode::Delete,
            "insert" | "ins" => KeyCode::Insert,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" | "pgup" => KeyCode::PageUp,
            "pagedown" | "pgdn" => KeyCode::PageDown,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "space" => KeyCode::Char(' '),
            "menu" => KeyCode::Menu,
            f if f.len() >= 2 && f.starts_with('f') && f[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => {
                KeyCode::F(f[1..].parse().unwrap())
            }
            _ => {
                let mut chars = rest.chars();
                match (chars.next(), chars.next()) {
                    // "Ctrl+F" means Ctrl+f; only a bare "F" implies Shift.
                    (Some(c), None) if ctrl || alt => KeyCode::Char(c.to_ascii_lowercase()),
                    (Some(c), None) => KeyCode::Char(c),
                    _ => return Err(format!("unknown key '{s}'")),
                }
            }
        };
        Ok(Key::new(code, ctrl, alt, shift))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        match self.code {
            KeyCode::F(n) => write!(f, "F{n}"),
            KeyCode::Char(' ') => f.write_str("Space"),
            KeyCode::Char(c) => write!(f, "{}", if self.shift { c.to_ascii_uppercase() } else { c }),
            c => write!(f, "{c:?}"),
        }
    }
}

/// A key as written on the keyboard, however config.toml spells it: `Ctrl+G` for `ctrl+g`,
/// `Delete` for `del`.
pub fn written(k: &str) -> String {
    let k = k.parse::<Key>().map_or_else(|_| k.to_string(), |k| k.to_string());
    match k.rsplit_once('+') {
        Some((mods, c)) if c.chars().count() == 1 => format!("{mods}+{}", c.to_uppercase()),
        _ => k,
    }
}

// ---------------------------------------------------------------- actions

/// `[keys]` without the actions this version does not know: a config.toml a newer Coxswain
/// wrote (or one shared with it) still starts this one, and the file keeps them for that one.
fn known_actions<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<Action, Vec<String>>, D::Error> {
    let keys = BTreeMap::<String, Vec<String>>::deserialize(d)?;
    Ok(keys
        .into_iter()
        .filter_map(|(name, keys)| Some((*Action::ALL.iter().find(|a| a.name() == name)?, keys)))
        .collect())
}

macro_rules! actions {
    ($($variant:ident = $name:literal, $label:literal, $group:ident, [$($key:literal),*];)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Action { $($variant),* }

        impl Action {
            pub const ALL: &[Action] = &[$(Action::$variant),*];
            /// snake_case name, as used in config files and by the GUI.
            pub fn name(self) -> &'static str { match self { $(Action::$variant => $name),* } }
            /// Human label for menus and the key bar.
            /// The name shown for the action, in the current language.
            pub fn label(self) -> String {
                let key = format!("action.{}", self.name());
                let t = $crate::i18n::tr(&key, &[]);
                if t == key { match self { $(Action::$variant => $label.to_string()),* } } else { t }
            }
            fn default_keys(self) -> &'static [&'static str] { match self { $(Action::$variant => &[$($key),*]),* } }
            /// Where F1 and F9 list the action.
            pub fn group(self) -> Group { match self { $(Action::$variant => Group::$group),* } }
        }
    };
}

/// The headings F1 and F9 list actions under, in the order a new user needs them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group { Moving, Panels, Marking, Files, Archives, Search, Git, Viewing, App }

impl Group {
    pub const ALL: &[Group] = &[Group::Moving, Group::Panels, Group::Marking, Group::Files, Group::Archives, Group::Search, Group::Git, Group::Viewing, Group::App];
    /// The heading, in the current language.
    pub fn label(self) -> String {
        crate::i18n::tr(&format!("group.{}", format!("{self:?}").to_lowercase()), &[])
    }
    /// Its actions, the most used first (the order of the actions table).
    pub fn actions(self) -> impl Iterator<Item = Action> {
        Action::ALL.iter().copied().filter(move |a| a.group() == self)
    }
}

// Grouped as F1 lists them, the most used first in each group.
// Norton Commander defaults. Plain printable keys (+ - *) only fire while the command line
// is empty, as in NC.
actions! {
    // Moving, the most used first.
    Open = "open", "Open", Moving, ["Enter"];
    Up = "up", "Up", Moving, ["Up"];
    Down = "down", "Down", Moving, ["Down"];
    Parent = "parent", "Parent folder", Moving, ["Ctrl+PageUp", "Backspace"];
    PageUp = "page_up", "Page up", Moving, ["PageUp", "Left"];
    PageDown = "page_down", "Page down", Moving, ["PageDown", "Right"];
    Home = "home", "First", Moving, ["Home"];
    End = "end", "Last", Moving, ["End"];
    Back = "back", "Back", Moving, ["Alt+Left"];
    Forward = "forward", "Forward", Moving, ["Alt+Right"];
    GotoLeft = "goto_left", "Left: go to", Moving, ["Alt+F1"];
    GotoRight = "goto_right", "Right: go to", Moving, ["Alt+F2"];
    EditPath = "edit_path", "Edit path", Moving, ["Ctrl+L"];
    // Panels and tabs, the most used first.
    SwitchPanel = "switch_panel", "Other panel", Panels, ["Tab"];
    ToggleHidden = "toggle_hidden", "Hidden files", Panels, ["Alt+."];
    Refresh = "refresh", "Refresh", Panels, ["Ctrl+R"];
    SameDir = "same_dir", "Other panel here", Panels, ["Alt+O"];
    SwapPanels = "swap_panels", "Swap panels", Panels, ["Ctrl+U"];
    TogglePanels = "toggle_panels", "Panels on/off", Panels, ["Ctrl+O"];
    NewTab = "new_tab", "New tab", Panels, ["Ctrl+T"];
    CloseTab = "close_tab", "Close tab", Panels, ["Ctrl+W"];
    NextTab = "next_tab", "Next tab", Panels, ["Ctrl+Tab"];
    PrevTab = "prev_tab", "Previous tab", Panels, ["Ctrl+Shift+Tab"];
    ToggleView = "toggle_view", "Details/columns/thumbnails", Panels, ["Alt+V"];
    ToggleSidebar = "toggle_sidebar", "Sidebar", Panels, ["Ctrl+B"];
    SortName = "sort_name", "Sort by name", Panels, ["Ctrl+F3"];
    SortExt = "sort_ext", "Sort by extension", Panels, ["Ctrl+F4"];
    SortTime = "sort_time", "Sort by time", Panels, ["Ctrl+F5"];
    SortSize = "sort_size", "Sort by size", Panels, ["Ctrl+F6"];
    // No key of its own: sizes appear by themselves (`folder_sizes`), and Ctrl+Space belongs
    // to the system on a Mac.
    FolderSizes = "folder_sizes", "Folder sizes", Panels, [];
    Columns = "columns", "Columns and folder sizes", Panels, [];
    // Where the space goes, as a treemap (`space`). Alt+U, for usage: free, and few names
    // start with u, so quick search hardly misses it.
    SpaceView = "space_view", "Space view", Panels, ["Alt+U"];
    // Marking, the most used first.
    Mark = "mark", "Mark", Marking, ["Insert", "Shift+Down"];
    MarkAll = "mark_all", "Mark all", Marking, ["Ctrl+A"];
    MarkGroup = "mark_group", "Mark group", Marking, ["+"];
    UnmarkGroup = "unmark_group", "Unmark group", Marking, ["-"];
    InvertMarks = "invert_marks", "Invert marks", Marking, ["*"];
    // Files, the most used first. The action menu first: it lists the rest.
    ActionMenu = "action_menu", "What can I do with this?", Files, ["Shift+F10", "Menu"];
    Copy = "copy", "Copy", Files, ["F5"];
    Move = "move", "Move or rename", Files, ["F6"];
    Rename = "rename", "Rename", Files, ["Shift+F6"];
    Delete = "delete", "Delete", Files, ["F8", "Delete"];
    Undo = "undo", "Undo", Files, ["Ctrl+Z"];
    NewFolder = "new_folder", "New folder", Files, ["F7"];
    ClipCopy = "clip_copy", "Copy to clipboard", Files, ["Ctrl+C"];
    ClipCut = "clip_cut", "Cut to clipboard", Files, ["Ctrl+X"];
    Paste = "paste", "Paste", Files, ["Ctrl+V"];
    DeleteForever = "delete_forever", "Delete permanently", Files, ["Shift+F8", "Shift+Delete"];
    Properties = "properties", "Properties", Files, ["Alt+Enter"];
    // Two files side by side (`compare`): free, and an Alt+letter would cost quick search.
    CompareFiles = "compare_files", "Compare files", Files, ["Shift+F2"];
    BatchRename = "batch_rename", "Batch rename", Files, ["Ctrl+M"];
    Tag = "tag", "Colour tag", Files, ["Alt+T"];
    // ZFS and FreeBSD: a folder's snapshots, a file's flags and the files of its package.
    Snapshots = "snapshots", "ZFS snapshots", Files, ["Alt+Z"];
    Flags = "flags", "File flags", Files, [];
    Package = "package", "Files of this package", Files, [];
    // Archives, the most used first.
    Extract = "extract", "Extract archive", Archives, ["Ctrl+E"];
    Pack = "pack", "Pack into an archive", Archives, ["Alt+F5"];
    // Turns on `[archive] disk_images`: from F9 and the action menu on a disk image.
    DiskImages = "disk_images", "Open disk images like folders", Archives, [];
    // OCRmyPDF: a copy of a scanned PDF with its words in it (`searchable`).
    SearchablePdf = "searchable_pdf", "Make PDF searchable", Files, ["Alt+P"];
    // ffmpeg: MP4, MP3, FLAC, the sound, a clip, next to the original (`media`). No key of its
    // own: the action menu and the preview offer it.
    ConvertMedia = "convert_media", "Convert video or audio", Files, [];
    // ImageMagick: JPEG, PNG, WebP, smaller, without the location, turned (`media`).
    EditPictures = "edit_pictures", "Convert or edit pictures", Files, [];
    // GDAL: GeoPackage, GeoJSON, a shapefile, KML, GPX; a GeoTIFF as COG or PNG (`geo`).
    ConvertGeodata = "convert_geodata", "Convert geodata", Files, [];
    // OpenSCAD: an STL or 3MF of a .scad model (`scad`).
    ExportModel = "export_model", "Export 3D model", Files, [];
    // calibre: an e-book as EPUB, AZW3 or PDF, next to it (`ebook`). No key of its own: the
    // action menu and the preview offer it.
    ConvertEbook = "convert_ebook", "Convert e-book", Files, [];
    // kicad-cli: PDF, SVG, a bill of materials, Gerbers of a schematic or board (`kicad`).
    ExportKicad = "export_kicad", "Export from KiCad", Files, [];
    // Checksums (`checksum`): a SHA256SUMS, SHA512SUMS or B3SUMS of the marked files; H for
    // hash. A list is checked by Enter on it, and from the action menu.
    MakeChecksums = "make_checksums", "Make checksums", Files, ["Alt+H"];
    VerifyChecksums = "verify_checksums", "Verify checksums", Files, [];
    // Search, the most used first.
    Search = "search", "Find", Search, ["Alt+F7", "Ctrl+F"];
    SearchText = "search_text", "Search inside files", Search, ["Shift+F7", "Ctrl+Shift+F"];
    Ask = "ask", "Ask your files", Search, ["Ctrl+F7"];
    Duplicates = "duplicates", "Find duplicates", Search, ["Ctrl+D"];
    // Whisper: the speech of a recording, or of every one in a folder, into the search store
    // (`transcribe`). No key of its own: the action menu, F9 and the preview offer it.
    Transcribe = "transcribe", "Transcribe speech", Search, [];
    // Git, the most used first.
    History = "history", "Git history", Git, ["Ctrl+G"];
    // In a history: every branch's commits in the graph, or this branch's again. A for all.
    HistoryAll = "history_all", "History of all branches", Git, ["Alt+A"];
    // In a history: the commits not pushed and not merged, as of the last fetch. Y: a free key.
    HistoryUpstream = "history_upstream", "Against upstream", Git, ["Alt+Y"];
    Branches = "branches", "Git branches", Git, ["Alt+B"];
    SwitchBranch = "switch_branch", "Switch to branch", Git, ["Alt+S"];
    Worktrees = "worktrees", "Git worktrees", Git, ["Alt+W"];
    NewBranch = "new_branch", "New branch here", Git, [];
    // Commit, push, pull and the rest: in the user's own tool (`handoff`).
    GitTool = "git_tool", "Open in your git tool", Git, ["Alt+G"];
    // The config as git uses it, read-only (`git_config`): changed in the user's own tool.
    GitConfig = "git_config", "Git config", Git, ["Alt+C"];
    // Viewing and editing, the most used first.
    View = "view", "View", Viewing, ["F3"];
    Edit = "edit", "Edit", Viewing, ["F4"];
    // The built-in editor (`edit`), whatever F4 starts: mc's Shift+F4, free here.
    EditHere = "edit_here", "Edit here", Viewing, ["Shift+F4"];
    TogglePreview = "toggle_preview", "Preview", Viewing, ["Space"];
    CopyPath = "copy_path", "Path to command line", Viewing, ["Ctrl+Enter", "Ctrl+J"];
    UserMenu = "user_menu", "Menu", Viewing, ["F2"];
    Notes = "notes", "Folder notes", Viewing, ["Alt+N"];
    // A PGN file's games, an EPD or FEN file's positions (`chess`): K for the king.
    Chess = "chess", "Open games", Viewing, ["Alt+K"];
    // App, the most used first.
    Help = "help", "Help", App, ["F1"];
    // F1's second tab, and F1 inside a dialog: no key of its own.
    Features = "features", "Features and questions", App, [];
    Menu = "menu", "Commands", App, ["F9"];
    // F12 too: most terminals never pass Ctrl+, on, and every one passes F12.
    Settings = "settings", "Settings", App, ["Ctrl+,", "F12"];
    // Settings at Disk use: from F9 and the action menu, no key of its own.
    DiskUse = "disk_use", "Disk use", App, [];
    Quit = "quit", "Quit", App, ["F10"];
}

impl Action {
    /// Actions only the GUI implements.
    pub fn gui_only(self) -> bool {
        use Action::*;
        matches!(
            self,
            NewTab | CloseTab | NextTab | PrevTab | TogglePreview | ToggleView | ToggleSidebar | EditPath | BatchRename | Tag | Notes | Back | Forward
                | ClipCopy | ClipCut | Paste | Columns | Duplicates
        )
    }
}

// ---------------------------------------------------------------- theme

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    /// A color name (`blue`, `lightcyan`, `reset`) or `#rrggbb`. Empty = inherit.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fg: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub bg: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
}

fn st(fg: &str, bg: &str) -> Style {
    Style { fg: fg.into(), bg: bg.into(), bold: false }
}
fn bold(mut s: Style) -> Style {
    s.bold = true;
    s
}

macro_rules! theme {
    ($($slot:ident),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Theme {
            /// The desktop app's shapes and chrome: `modern`, `crt`, `dos`, `workbench`, `gem`,
            /// `win31`, `win95`, `winxp`, `win7`, `win10`, `win11`, `system7`, `platinum`, `aqua`,
            /// `macos`, `next` or `beos`.
            /// Colors come from the slots; the terminal app ignores it.
            pub look: String,
            /// The desktop app's tube: scanlines, a glow on the text and rings instead of
            /// shadows, over any look. The built-in CRT themes have it; the terminal app
            /// ignores it.
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            pub crt: bool,
            $(pub $slot: Style),*
        }

        impl Theme {
            /// (slot name, style) pairs, used by the GUI to emit CSS variables.
            pub fn slots(&self) -> Vec<(&'static str, &Style)> { vec![$((stringify!($slot), &self.$slot)),*] }
        }
    };
}

theme!(
    panel, border, header, directory, executable, hidden, symlink, cursor, marked, marked_cursor,
    status, keybar_num, keybar_label, cmdline, dialog, dialog_border, dialog_input, git_branch,
    git_modified, git_added, git_untracked, git_deleted, git_renamed, git_conflict, git_ignored,
    search_hit, accent, sidebar, tab, tab_active, preview,
);

impl Default for Theme {
    fn default() -> Self {
        Theme::nc()
    }
}

/// A built-in theme: its name, its group (a `theme.group.*` key) and how it is made.
pub type BuiltinTheme = (&'static str, &'static str, fn() -> Theme);

impl Theme {
    /// Norton Commander 5: cyan on blue, black-on-cyan cursor, yellow marks. Exact CGA RGB, so
    /// the terminal's own palette (often a pale "blue") does not wash it out.
    pub fn nc() -> Self {
        Theme {
            look: "dos".into(),
            crt: false,
            panel: st("#55ffff", "#0000aa"),
            border: st("#55ffff", "#0000aa"),
            header: bold(st("#ffff55", "#0000aa")),
            directory: bold(st("#ffffff", "#0000aa")),
            executable: st("#55ff55", "#0000aa"),
            hidden: st("#00aaaa", "#0000aa"),
            symlink: st("#ff55ff", "#0000aa"),
            cursor: st("#000000", "#00aaaa"),
            marked: bold(st("#ffff55", "#0000aa")),
            marked_cursor: bold(st("#ffff55", "#00aaaa")),
            status: st("#55ffff", "#0000aa"),
            keybar_num: st("#ffffff", "#000000"),
            keybar_label: st("#000000", "#00aaaa"),
            cmdline: st("#aaaaaa", "#000000"),
            dialog: st("#000000", "#aaaaaa"),
            dialog_border: st("#ffffff", "#aaaaaa"),
            dialog_input: st("#000000", "#00aaaa"),
            git_branch: bold(st("#ff55ff", "#0000aa")),
            git_modified: st("#ffff55", "#0000aa"),
            git_added: st("#55ff55", "#0000aa"),
            git_untracked: st("#ff5555", "#0000aa"),
            git_deleted: st("#aa0000", "#0000aa"),
            git_renamed: st("#5555ff", "#0000aa"),
            git_conflict: bold(st("#ff5555", "#0000aa")),
            git_ignored: st("#555555", "#0000aa"),
            search_hit: bold(st("#ffff55", "")),
            accent: st("#ffff55", "#0000aa"),
            sidebar: st("#55ffff", "#0000aa"),
            tab: st("#00aaaa", "#0000aa"),
            tab_active: st("#000000", "#00aaaa"),
            preview: st("#55ffff", "#0000aa"),
        }
    }

    /// The default in the desktop app: a green phosphor terminal, as in WarGames. Black, green
    /// text with cyan folders, amber instead of red.
    pub fn cyber() -> Self {
        Theme::from_palette(&Palette {
            look: "crt", bg: "#020805", alt: "#010503", raised: "#06140b", fg: "#33ff66", sel_fg: "#d8ffe2",
            dim: "#1c9c48", border: "#0d3f1f", accent: "#33ff66", sel_bg: "#0f5528", dir: "#5cf2ff", red: "#ffa31a",
            green: "#33ff66", yellow: "#d4ff3a", blue: "#3fd0ff", magenta: "#3fffd2", orange: "#ffb000",
        })
    }

    /// A modern dark scheme (Tokyo Night).
    pub fn midnight() -> Self {
        Theme::from_palette(&Palette {
            look: "modern", bg: "#1a1b26", alt: "#16161e", raised: "#24283b", fg: "#c0caf5", sel_fg: "#c0caf5", dim: "#565f89", border: "#292e42",
            accent: "#7aa2f7", sel_bg: "#283457", dir: "#7aa2f7", red: "#f7768e", green: "#9ece6a", yellow: "#e0af68",
            blue: "#2ac3de", magenta: "#bb9af7", orange: "#ff9e64",
        })
    }

    /// Neutral dark, blue accent.
    pub fn dark() -> Self {
        Theme::from_palette(&Palette {
            look: "modern", bg: "#1e1f22", alt: "#18191b", raised: "#2b2d31", fg: "#dcdde1", sel_fg: "#dcdde1", dim: "#80848e", border: "#313338",
            accent: "#4c8dff", sel_bg: "#2e436e", dir: "#8ab4f8", red: "#f28b82", green: "#81c995", yellow: "#fdd663",
            blue: "#78d9ec", magenta: "#c58af9", orange: "#fcad70",
        })
    }

    pub fn light() -> Self {
        Theme::from_palette(&Palette {
            look: "modern", bg: "#ffffff", alt: "#f3f4f6", raised: "#ffffff", fg: "#1f2328", sel_fg: "#1f2328", dim: "#6e7781", border: "#d8dee4",
            accent: "#0969da", sel_bg: "#cfe3ff", dir: "#0550ae", red: "#cf222e", green: "#1a7f37", yellow: "#9a6700",
            blue: "#0598bc", magenta: "#8250df", orange: "#bc4c00",
        })
    }

    pub fn nord() -> Self {
        Theme::from_palette(&Palette {
            look: "modern", bg: "#2e3440", alt: "#272c36", raised: "#3b4252", fg: "#e5e9f0", sel_fg: "#e5e9f0", dim: "#7b88a1", border: "#3b4252",
            accent: "#88c0d0", sel_bg: "#434c5e", dir: "#88c0d0", red: "#bf616a", green: "#a3be8c", yellow: "#ebcb8b",
            blue: "#81a1c1", magenta: "#b48ead", orange: "#d08770",
        })
    }

    /// Windows 3.11: white lists, grey chrome, navy selection.
    pub fn win31() -> Self {
        Theme::from_palette(&Palette {
            look: "win31", bg: "#ffffff", alt: "#ffffff", raised: "#c0c0c0", fg: "#000000", sel_fg: "#ffffff",
            dim: "#808080", border: "#000000", accent: "#000080", sel_bg: "#000080", dir: "#000000", red: "#800000",
            green: "#008000", yellow: "#808000", blue: "#0000ff", magenta: "#800080", orange: "#808000",
        })
    }

    /// Windows 95 and 98: grey 3D chrome, navy selection.
    pub fn win95() -> Self {
        Theme::from_palette(&Palette {
            look: "win95", bg: "#ffffff", alt: "#c0c0c0", raised: "#c0c0c0", fg: "#000000", sel_fg: "#ffffff",
            dim: "#808080", border: "#808080", accent: "#000080", sel_bg: "#000080", dir: "#000000", red: "#800000",
            green: "#008000", yellow: "#808000", blue: "#0000ff", magenta: "#800080", orange: "#808000",
        })
    }

    /// Windows XP (Luna): beige chrome, blue task pane, blue selection.
    pub fn winxp() -> Self {
        Theme::from_palette(&Palette {
            look: "winxp", bg: "#ffffff", alt: "#d6dff7", raised: "#ece9d8", fg: "#000000", sel_fg: "#ffffff",
            dim: "#7f7f7f", border: "#7f9db9", accent: "#316ac5", sel_bg: "#316ac5", dir: "#000000", red: "#c00000",
            green: "#008000", yellow: "#a07000", blue: "#0000ff", magenta: "#800080", orange: "#c05000",
        })
    }

    /// Windows 7 (Aero): pale blue navigation, glassy light-blue selection.
    pub fn win7() -> Self {
        Theme::from_palette(&Palette {
            look: "win7", bg: "#ffffff", alt: "#f1f5fb", raised: "#f0f0f0", fg: "#1e1e1e", sel_fg: "#000000",
            dim: "#6d6d6d", border: "#d5dfe5", accent: "#3399ff", sel_bg: "#cce8ff", dir: "#1e1e1e", red: "#c42b1c",
            green: "#107c10", yellow: "#9d5d00", blue: "#0066cc", magenta: "#881798", orange: "#ca5010",
        })
    }

    /// Windows 10: flat and white, square corners.
    pub fn win10() -> Self {
        Theme::from_palette(&Palette {
            look: "win10", bg: "#ffffff", alt: "#ffffff", raised: "#f0f0f0", fg: "#000000", sel_fg: "#000000",
            dim: "#6d6d6d", border: "#e5e5e5", accent: "#0078d7", sel_bg: "#cce8ff", dir: "#000000", red: "#c42b1c",
            green: "#107c10", yellow: "#9d5d00", blue: "#0063b1", magenta: "#881798", orange: "#ca5010",
        })
    }

    /// Windows 11 (Mica), light.
    pub fn win11() -> Self {
        Theme::from_palette(&Palette {
            look: "win11", bg: "#ffffff", alt: "#f3f3f3", raised: "#fbfbfb", fg: "#1a1a1a", sel_fg: "#1a1a1a",
            dim: "#5f5f5f", border: "#e5e5e5", accent: "#005fb8", sel_bg: "#d3e5f7", dir: "#1a1a1a", red: "#c42b1c",
            green: "#0f7b0f", yellow: "#9d5d00", blue: "#005fb8", magenta: "#881798", orange: "#ca5010",
        })
    }

    /// Windows 11 (Mica), dark.
    pub fn win11_dark() -> Self {
        Theme::from_palette(&Palette {
            look: "win11", bg: "#1c1c1c", alt: "#202020", raised: "#2b2b2b", fg: "#ffffff", sel_fg: "#ffffff",
            dim: "#9d9d9d", border: "#333333", accent: "#4cc2ff", sel_bg: "#2e4a66", dir: "#ffffff", red: "#ff99a4",
            green: "#6ccb5f", yellow: "#fce100", blue: "#60cdff", magenta: "#d59dff", orange: "#fcb57c",
        })
    }

    /// Macintosh System 7: black on white, one-pixel lines, inverted selection.
    pub fn system7() -> Self {
        Theme::from_palette(&Palette {
            look: "system7", bg: "#ffffff", alt: "#ffffff", raised: "#ffffff", fg: "#000000", sel_fg: "#ffffff",
            dim: "#555555", border: "#000000", accent: "#000000", sel_bg: "#000000", dir: "#000000", red: "#dd0806",
            green: "#006411", yellow: "#90713a", blue: "#0000d4", magenta: "#f20884", orange: "#ff6403",
        })
    }

    /// Mac OS 8 and 9 (Platinum): grey bevels, lavender selection.
    pub fn platinum() -> Self {
        Theme::from_palette(&Palette {
            look: "platinum", bg: "#ffffff", alt: "#dddddd", raised: "#dddddd", fg: "#000000", sel_fg: "#000000",
            dim: "#777777", border: "#999999", accent: "#6666cc", sel_bg: "#ccccff", dir: "#000000", red: "#dd0806",
            green: "#006411", yellow: "#90713a", blue: "#0000d4", magenta: "#b000a0", orange: "#d05000",
        })
    }

    /// Mac OS X Aqua (10.0 to 10.4): pinstripes, gel buttons, blue selection.
    pub fn aqua() -> Self {
        Theme::from_palette(&Palette {
            look: "aqua", bg: "#ffffff", alt: "#e8edf5", raised: "#ececec", fg: "#000000", sel_fg: "#ffffff",
            dim: "#808080", border: "#a5a5a5", accent: "#3875d7", sel_bg: "#3875d7", dir: "#000000", red: "#c4161c",
            green: "#1d8b2c", yellow: "#a07000", blue: "#1a5fd0", magenta: "#9b30b0", orange: "#d06000",
        })
    }

    /// Current macOS, light.
    pub fn macos() -> Self {
        Theme::from_palette(&Palette {
            look: "macos", bg: "#ffffff", alt: "#ececec", raised: "#f6f6f6", fg: "#1d1d1f", sel_fg: "#ffffff",
            dim: "#86868b", border: "#e0e0e0", accent: "#007aff", sel_bg: "#0064e1", dir: "#1d1d1f", red: "#d70015",
            green: "#248a3d", yellow: "#a05a00", blue: "#0071e3", magenta: "#8944ab", orange: "#c93400",
        })
    }

    /// Current macOS, dark.
    pub fn macos_dark() -> Self {
        Theme::from_palette(&Palette {
            look: "macos", bg: "#1e1e1e", alt: "#2a2a2a", raised: "#323232", fg: "#e5e5e5", sel_fg: "#ffffff",
            dim: "#98989d", border: "#3a3a3a", accent: "#0a84ff", sel_bg: "#0a5ad6", dir: "#e5e5e5", red: "#ff6961",
            green: "#30d158", yellow: "#ffd60a", blue: "#64d2ff", magenta: "#bf5af2", orange: "#ff9f0a",
        })
    }

    /// An amber monochrome phosphor terminal (P3): near-black, amber text, one phosphor in
    /// every slot, paler amber folders. The amber is #ffb000, the usual sRGB for a P3 monitor's
    /// yellow-orange glow (peak about 602 nm); the rest are tints and shades of that one hue.
    pub fn amber() -> Self {
        Theme::from_palette(&Palette {
            look: "crt",
            bg: "#0a0600",
            alt: "#070400",
            raised: "#1a1000",
            fg: "#ffb000",
            sel_fg: "#ffd27a",
            dim: "#b37a00",
            border: "#4d3300",
            accent: "#ffb000",
            sel_bg: "#4d3300",
            dir: "#ffcc55",
            red: "#ff8c00",
            green: "#e0a020",
            yellow: "#ffe39c",
            blue: "#d9a84e",
            magenta: "#f0c070",
            orange: "#ffd27a",
        })
    }

    /// Paper white: a P4 white-phosphor terminal (the Macintosh and VT420 white), one phosphor
    /// in greys with a cool cast. Folders and marks bold white, hidden files dim, a lit grey
    /// cursor row.
    pub fn paper() -> Self {
        Theme {
            look: "crt".into(),
            crt: true,
            panel: st("#e3e6ea", "#0b0b0e"),
            border: st("#44464c", "#0b0b0e"),
            header: st("#888b90", "#0b0b0e"),
            directory: bold(st("#ffffff", "#0b0b0e")),
            executable: st("#ffffff", "#0b0b0e"),
            hidden: st("#888b90", "#0b0b0e"),
            symlink: st("#b4b7bc", "#0b0b0e"),
            cursor: st("#ffffff", "#3a3a40"),
            marked: bold(st("#ffffff", "#1e1e23")),
            marked_cursor: bold(st("#ffffff", "#3a3a40")),
            status: st("#888b90", "#08080a"),
            keybar_num: st("#888b90", "#08080a"),
            keybar_label: st("#e3e6ea", "#141418"),
            cmdline: st("#e3e6ea", "#08080a"),
            dialog: st("#e3e6ea", "#141418"),
            dialog_border: st("#e3e6ea", "#141418"),
            dialog_input: st("#e3e6ea", "#0b0b0e"),
            git_branch: bold(st("#b4b7bc", "#0b0b0e")),
            git_modified: st("#b4b7bc", "#0b0b0e"),
            git_added: st("#e3e6ea", "#0b0b0e"),
            git_untracked: st("#ffffff", "#0b0b0e"),
            git_deleted: st("#ffffff", "#0b0b0e"),
            git_renamed: st("#b4b7bc", "#0b0b0e"),
            git_conflict: bold(st("#ffffff", "#0b0b0e")),
            git_ignored: st("#888b90", "#0b0b0e"),
            search_hit: bold(st("#ffffff", "")),
            accent: bold(st("#0b0b0e", "#e3e6ea")),
            sidebar: st("#e3e6ea", "#08080a"),
            tab: st("#888b90", "#08080a"),
            tab_active: st("#e3e6ea", "#0b0b0e"),
            preview: st("#e3e6ea", "#08080a"),
        }
    }

    /// XTree Gold 2/3 for DOS: light-grey and bright-white text on the dark blue screen, a
    /// yellow bar on the current item, cyan and green for sizes and stats, black on grey for
    /// the command area, white on blue for dialogs with grey fields, white on red for errors.
    /// Exact CGA RGB only.
    pub fn xtree() -> Self {
        Theme {
            look: "dos".into(),
            crt: false,
            panel: st("#aaaaaa", "#0000aa"),
            border: st("#aaaaaa", "#0000aa"),
            header: bold(st("#55ffff", "#0000aa")),
            directory: bold(st("#ffffff", "#0000aa")),
            executable: st("#55ff55", "#0000aa"),
            hidden: st("#00aaaa", "#0000aa"),
            symlink: st("#ff55ff", "#0000aa"),
            cursor: st("#000000", "#ffff55"),
            marked: bold(st("#ffff55", "#0000aa")),
            marked_cursor: bold(st("#0000aa", "#ffff55")),
            status: st("#000000", "#aaaaaa"),
            keybar_num: bold(st("#0000aa", "#aaaaaa")),
            keybar_label: st("#000000", "#aaaaaa"),
            cmdline: st("#000000", "#aaaaaa"),
            dialog: st("#ffffff", "#0000aa"),
            dialog_border: st("#aaaaaa", "#0000aa"),
            dialog_input: st("#000000", "#aaaaaa"),
            git_branch: bold(st("#ff55ff", "#0000aa")),
            git_modified: st("#ffff55", "#0000aa"),
            git_added: st("#55ff55", "#0000aa"),
            git_untracked: st("#55ffff", "#0000aa"),
            git_deleted: st("#ffffff", "#aa0000"),
            git_renamed: st("#ff55ff", "#0000aa"),
            git_conflict: bold(st("#ffffff", "#aa0000")),
            git_ignored: st("#00aaaa", "#0000aa"),
            search_hit: bold(st("#ffff55", "")),
            accent: bold(st("#000000", "#ffff55")),
            sidebar: st("#aaaaaa", "#0000aa"),
            tab: st("#00aaaa", "#0000aa"),
            tab_active: st("#000000", "#aaaaaa"),
            preview: st("#aaaaaa", "#0000aa"),
        }
    }

    /// MS-DOS Shell (DOS 5 and 6), its default "Basic Blue" scheme: black on light grey lists,
    /// white-on-blue /// title bars, a black-on-cyan cursor, blue folders, in exact CGA RGB;
    /// git states, symlinks and search hits /// in deeper shades, since no CGA colour but black
    /// and blue reads on light grey.
    pub fn dosshell() -> Self {
        Theme {
            look: "dos".into(),
            crt: false,
            panel: st("#000000", "#aaaaaa"),
            border: st("#000000", "#aaaaaa"),
            header: bold(st("#ffffff", "#0000aa")),
            directory: st("#0000aa", "#aaaaaa"),
            executable: bold(st("#000000", "#aaaaaa")),
            hidden: st("#555555", "#aaaaaa"),
            symlink: st("#550055", "#aaaaaa"),
            cursor: st("#000000", "#00aaaa"),
            marked: bold(st("#000000", "#ffffff")),
            marked_cursor: bold(st("#000000", "#00aaaa")),
            status: st("#000000", "#ffffff"),
            keybar_num: st("#ffffff", "#0000aa"),
            keybar_label: st("#000000", "#aaaaaa"),
            cmdline: st("#000000", "#aaaaaa"),
            dialog: st("#000000", "#aaaaaa"),
            dialog_border: st("#000000", "#aaaaaa"),
            dialog_input: st("#000000", "#00aaaa"),
            git_branch: bold(st("#550055", "#aaaaaa")),
            git_modified: st("#442200", "#aaaaaa"),
            git_added: st("#003300", "#aaaaaa"),
            git_untracked: st("#660000", "#aaaaaa"),
            git_deleted: st("#660000", "#aaaaaa"),
            git_renamed: st("#003333", "#aaaaaa"),
            git_conflict: bold(st("#660000", "#aaaaaa")),
            git_ignored: st("#555555", "#aaaaaa"),
            search_hit: bold(st("#660000", "")),
            accent: st("#ffffff", "#0000aa"),
            sidebar: st("#000000", "#aaaaaa"),
            tab: st("#555555", "#aaaaaa"),
            tab_active: st("#ffffff", "#0000aa"),
            preview: st("#000000", "#aaaaaa"),
        }
    }

    /// GNU Midnight Commander's default skin: light grey on blue (not NC's cyan), white
    /// folders, yellow marks, black-on-cyan cursor, grey dialogs, black-on-cyan tabs and
    /// buttons; in the Linux console's CGA RGB.
    pub fn mc() -> Self {
        Theme {
            look: "dos".into(),
            crt: false,
            panel: st("#aaaaaa", "#0000aa"),
            border: st("#aaaaaa", "#0000aa"),
            header: bold(st("#ffff55", "#0000aa")),
            directory: bold(st("#ffffff", "#0000aa")),
            executable: st("#55ff55", "#0000aa"),
            hidden: st("#00aaaa", "#0000aa"),
            symlink: st("#aaaaaa", "#0000aa"),
            cursor: st("#000000", "#00aaaa"),
            marked: bold(st("#ffff55", "#0000aa")),
            marked_cursor: bold(st("#0000aa", "#00aaaa")),
            status: st("#aaaaaa", "#000000"),
            keybar_num: st("#ffffff", "#000000"),
            keybar_label: st("#000000", "#00aaaa"),
            cmdline: st("#aaaaaa", "#0000aa"),
            dialog: st("#000000", "#aaaaaa"),
            dialog_border: st("#000000", "#aaaaaa"),
            dialog_input: st("#000000", "#00aaaa"),
            git_branch: bold(st("#ff55ff", "#0000aa")),
            git_modified: st("#ffff55", "#0000aa"),
            git_added: st("#55ff55", "#0000aa"),
            git_untracked: st("#ff6666", "#0000aa"),
            git_deleted: st("#ff6666", "#0000aa"),
            git_renamed: st("#55ffff", "#0000aa"),
            git_conflict: bold(st("#ff6666", "#0000aa")),
            git_ignored: st("#00aaaa", "#0000aa"),
            search_hit: bold(st("#ffff55", "")),
            accent: bold(st("#000000", "#00aaaa")),
            sidebar: st("#aaaaaa", "#0000aa"),
            tab: st("#000000", "#00aaaa"),
            tab_active: st("#ffffff", "#000000"),
            preview: st("#aaaaaa", "#0000aa"),
        }
    }

    /// Commodore 64 power-on screen: light blue on blue, white folders, an inverse cursor row.
    /// VIC-II colours in the Colodore palette (pepto.de: a PAL C64 as measured on its monitor);
    /// the text's light blue, light red and purple are lightened to read, the border keeps the
    /// real light blue.
    pub fn c64() -> Self {
        Theme {
            look: "crt".into(),
            crt: true,
            panel: st("#b3b0f3", "#2e2c9b"),
            border: st("#706deb", "#2e2c9b"),
            header: st("#ffffff", "#2e2c9b"),
            directory: bold(st("#ffffff", "#2e2c9b")),
            executable: st("#a9ff9f", "#2e2c9b"),
            hidden: st("#b2b2b2", "#2e2c9b"),
            symlink: st("#c6a1cc", "#2e2c9b"),
            cursor: st("#2e2c9b", "#b3b0f3"),
            marked: bold(st("#edf171", "#2e2c9b")),
            marked_cursor: bold(st("#2e2c9b", "#edf171")),
            status: st("#b2b2b2", "#2e2c9b"),
            keybar_num: st("#b2b2b2", "#2e2c9b"),
            keybar_label: st("#2e2c9b", "#b3b0f3"),
            cmdline: st("#b3b0f3", "#2e2c9b"),
            dialog: st("#b3b0f3", "#2e2c9b"),
            dialog_border: st("#ffffff", "#2e2c9b"),
            dialog_input: st("#b3b0f3", "#2e2c9b"),
            git_branch: bold(st("#c6a1cc", "#2e2c9b")),
            git_modified: st("#edf171", "#2e2c9b"),
            git_added: st("#a9ff9f", "#2e2c9b"),
            git_untracked: st("#d2a09c", "#2e2c9b"),
            git_deleted: st("#d2a09c", "#2e2c9b"),
            git_renamed: st("#75cec8", "#2e2c9b"),
            git_conflict: bold(st("#d2a09c", "#2e2c9b")),
            git_ignored: st("#b2b2b2", "#2e2c9b"),
            search_hit: bold(st("#edf171", "")),
            accent: bold(st("#2e2c9b", "#b3b0f3")),
            sidebar: st("#b3b0f3", "#2e2c9b"),
            tab: st("#b2b2b2", "#2e2c9b"),
            tab_active: st("#ffffff", "#2e2c9b"),
            preview: st("#b3b0f3", "#2e2c9b"),
        }
    }

    /// Sinclair ZX Spectrum: bright white on black, the ULA's bright palette for colours and
    /// its non-bright shades for dim. Colour values are the Spectrum's canonical RGB (bright at
    /// 0xff, non-bright at 0xd7).
    pub fn spectrum() -> Self {
        Theme::from_palette(&Palette {
            look: "crt",
            bg: "#000000",
            alt: "#000000",
            raised: "#000000",
            fg: "#ffffff",
            sel_fg: "#ffffff",
            dim: "#d7d7d7",
            border: "#d7d7d7",
            accent: "#00ffff",
            sel_bg: "#0000d7",
            dir: "#ffff00",
            red: "#ff0000",
            green: "#00ff00",
            yellow: "#ffff00",
            blue: "#00d7d7",
            magenta: "#ff00ff",
            orange: "#00ffff",
        })
    }

    /// Amstrad CPC 464 at power-on: bright yellow on blue, the cursor inverted (blue on
    /// yellow). Every colour is one of the CPC's 27 hardware colours (0, 128 or 255 a channel).
    pub fn cpc() -> Self {
        Theme {
            look: "crt".into(),
            crt: true,
            panel: st("#ffff00", "#000080"),
            border: st("#8080ff", "#000080"),
            header: st("#8080ff", "#000080"),
            directory: st("#00ffff", "#000080"),
            executable: st("#00ff00", "#000080"),
            hidden: st("#8080ff", "#000080"),
            symlink: st("#ff00ff", "#000080"),
            cursor: st("#000080", "#ffff00"),
            marked: bold(st("#ffffff", "#000080")),
            marked_cursor: bold(st("#000080", "#ffffff")),
            status: st("#8080ff", "#000080"),
            keybar_num: st("#8080ff", "#000080"),
            keybar_label: st("#ffff00", "#0000ff"),
            cmdline: st("#ffff00", "#000080"),
            dialog: st("#ffff00", "#0000ff"),
            dialog_border: st("#ffffff", "#0000ff"),
            dialog_input: st("#ffff00", "#000080"),
            git_branch: bold(st("#ff00ff", "#000080")),
            git_modified: st("#ff8000", "#000080"),
            git_added: st("#00ff00", "#000080"),
            git_untracked: st("#ff8080", "#000080"),
            git_deleted: st("#ff8080", "#000080"),
            git_renamed: st("#80ffff", "#000080"),
            git_conflict: bold(st("#ff8080", "#000080")),
            git_ignored: st("#8080ff", "#000080"),
            search_hit: bold(st("#ff8000", "")),
            accent: bold(st("#000080", "#ffff00")),
            sidebar: st("#ffff00", "#000080"),
            tab: st("#8080ff", "#000080"),
            tab_active: st("#ffff00", "#000080"),
            preview: st("#ffff00", "#000080"),
        }
    }

    /// MSX: white text on the dark blue of MSX-BASIC's boot screen, light blue lines, cyan
    /// folders, a black cursor bar, the function keys white on blue as KEY ON draws them. The
    /// V9938's default palette (the MSX2's 3-bit RGB rendering of the TMS9918 colours), with
    /// red, blue and magenta lightened within its 512 colours so they read on the blue, and an
    /// orange from the same steps for search hits.
    pub fn msx() -> Self {
        Theme {
            look: "crt".into(),
            crt: true,
            panel: st("#ffffff", "#2424ff"),
            border: st("#496dff", "#2424ff"),
            header: st("#b6b6b6", "#2424ff"),
            directory: st("#49dbff", "#2424ff"),
            executable: st("#6dff6d", "#2424ff"),
            hidden: st("#b6b6b6", "#2424ff"),
            symlink: st("#ffb6ff", "#2424ff"),
            cursor: st("#ffffff", "#000000"),
            marked: bold(st("#dbdb24", "#2424ff")),
            marked_cursor: bold(st("#dbdb24", "#000000")),
            status: st("#b6b6b6", "#2424ff"),
            keybar_num: st("#ffffff", "#2424ff"),
            keybar_label: st("#ffffff", "#000000"),
            cmdline: st("#ffffff", "#2424ff"),
            dialog: st("#ffffff", "#000000"),
            dialog_border: st("#49dbff", "#000000"),
            dialog_input: st("#ffffff", "#2424ff"),
            git_branch: bold(st("#ffb6ff", "#2424ff")),
            git_modified: st("#dbdb24", "#2424ff"),
            git_added: st("#6dff6d", "#2424ff"),
            git_untracked: st("#ffb6b6", "#2424ff"),
            git_deleted: st("#ffb6b6", "#2424ff"),
            git_renamed: st("#92dbff", "#2424ff"),
            git_conflict: bold(st("#ffb6b6", "#2424ff")),
            git_ignored: st("#b6b6b6", "#2424ff"),
            search_hit: bold(st("#ffdb49", "")),
            accent: bold(st("#2424ff", "#49dbff")),
            sidebar: st("#ffffff", "#2424ff"),
            tab: st("#b6b6b6", "#2424ff"),
            tab_active: st("#ffffff", "#2424ff"),
            preview: st("#ffffff", "#2424ff"),
        }
    }

    /// BBC Micro Mode 7 (Teletext): white on black, yellow headers on a blue bar, cyan folders,
    /// green marks, a Ceefax page. The eight saturated Teletext colours of the SAA5050, nothing
    /// else.
    pub fn bbc() -> Self {
        Theme {
            look: "crt".into(),
            crt: true,
            panel: st("#ffffff", "#000000"),
            border: st("#0000ff", "#000000"),
            header: st("#ffff00", "#0000ff"),
            directory: st("#00ffff", "#000000"),
            executable: st("#ffff00", "#000000"),
            hidden: st("#ff00ff", "#000000"),
            symlink: st("#ff00ff", "#000000"),
            cursor: st("#ffffff", "#0000ff"),
            marked: st("#00ff00", "#000000"),
            marked_cursor: st("#00ff00", "#0000ff"),
            status: st("#ffffff", "#0000ff"),
            keybar_num: st("#ffffff", "#000000"),
            keybar_label: st("#000000", "#00ffff"),
            cmdline: st("#ffffff", "#000000"),
            dialog: st("#ffffff", "#000000"),
            dialog_border: st("#ffff00", "#000000"),
            dialog_input: st("#ffffff", "#0000ff"),
            git_branch: st("#00ffff", "#000000"),
            git_modified: st("#ffff00", "#000000"),
            git_added: st("#00ff00", "#000000"),
            git_untracked: st("#ff00ff", "#000000"),
            git_deleted: st("#ff0000", "#000000"),
            git_renamed: st("#00ffff", "#000000"),
            git_conflict: st("#ff0000", "#000000"),
            git_ignored: st("#ff00ff", "#000000"),
            search_hit: st("#ffff00", ""),
            accent: bold(st("#000000", "#ffff00")),
            sidebar: st("#ffffff", "#000000"),
            tab: st("#ff00ff", "#000000"),
            tab_active: st("#ffff00", "#0000ff"),
            preview: st("#ffffff", "#000000"),
        }
    }

    /// Atari ST GEM desktop in medium resolution: black on white windows, the green desktop as
    /// chrome, one-pixel black frames, inverse selection. Every colour is an ST 9-bit palette
    /// value (3 bits a channel, 0..7 expanded to 0..255).
    pub fn atarist() -> Self {
        Theme {
            look: "gem".into(),
            crt: false,
            panel: st("#000000", "#ffffff"),
            border: st("#000000", "#ffffff"),
            header: st("#6d6d6d", "#ffffff"),
            directory: st("#006dff", "#ffffff"),
            executable: st("#006d00", "#ffffff"),
            hidden: st("#6d6d6d", "#ffffff"),
            symlink: st("#b649b6", "#ffffff"),
            cursor: st("#ffffff", "#000000"),
            marked: bold(st("#6d6d00", "#ffffff")),
            marked_cursor: bold(st("#ffff00", "#000000")),
            status: st("#6d6d6d", "#ffffff"),
            keybar_num: st("#ffffff", "#000000"),
            keybar_label: st("#000000", "#00ff00"),
            cmdline: st("#000000", "#ffffff"),
            dialog: st("#000000", "#00ff00"),
            dialog_border: st("#000000", "#00ff00"),
            dialog_input: st("#000000", "#ffffff"),
            git_branch: bold(st("#b649b6", "#ffffff")),
            git_modified: st("#6d6d00", "#ffffff"),
            git_added: st("#006d00", "#ffffff"),
            git_untracked: st("#db0000", "#ffffff"),
            git_deleted: st("#db0000", "#ffffff"),
            git_renamed: st("#0049ff", "#ffffff"),
            git_conflict: bold(st("#db0000", "#ffffff")),
            git_ignored: st("#6d6d6d", "#ffffff"),
            search_hit: bold(st("#b64900", "")),
            accent: st("#ffffff", "#000000"),
            sidebar: st("#000000", "#ffffff"),
            tab: st("#6d6d6d", "#ffffff"),
            tab_active: st("#000000", "#ffffff"),
            preview: st("#000000", "#ffffff"),
        }
    }

    /// Amiga Workbench 1.3: white on the Workbench blue, black-on-orange cursor as a selected
    /// icon, orange marks, dialogs white on blue as the 1.3 Preferences screen with white
    /// string gadgets. The four Workbench 1.x default colours plus 12-bit Amiga shades for git
    /// states.
    pub fn workbench() -> Self {
        Theme {
            look: "workbench".into(),
            crt: false,
            panel: st("#ffffff", "#0055aa"),
            border: st("#ffffff", "#0055aa"),
            header: bold(st("#ff8800", "#0055aa")),
            directory: bold(st("#ffffff", "#0055aa")),
            executable: st("#66ff66", "#0055aa"),
            hidden: st("#99bbee", "#0055aa"),
            symlink: st("#ffbbff", "#0055aa"),
            cursor: st("#000000", "#ff8800"),
            marked: bold(st("#ff8800", "#000000")),
            marked_cursor: bold(st("#000000", "#ff8800")),
            status: st("#ffffff", "#0055aa"),
            keybar_num: st("#ffffff", "#0055aa"),
            keybar_label: st("#000000", "#ffffff"),
            cmdline: st("#ffffff", "#0055aa"),
            dialog: st("#ffffff", "#0055aa"),
            dialog_border: st("#ffffff", "#0055aa"),
            dialog_input: st("#000000", "#ffffff"),
            git_branch: bold(st("#ffbbff", "#0055aa")),
            git_modified: st("#ffcc66", "#0055aa"),
            git_added: st("#66ff66", "#0055aa"),
            git_untracked: st("#ffbbbb", "#0055aa"),
            git_deleted: st("#ffbbbb", "#0055aa"),
            git_renamed: st("#88ddff", "#0055aa"),
            git_conflict: bold(st("#ffbbbb", "#0055aa")),
            git_ignored: st("#99bbee", "#0055aa"),
            search_hit: bold(st("#ffcc66", "")),
            accent: st("#000000", "#ff8800"),
            sidebar: st("#ffffff", "#0055aa"),
            tab: st("#99bbee", "#0055aa"),
            tab_active: st("#000000", "#ffffff"),
            preview: st("#ffffff", "#0055aa"),
        }
    }

    /// NeXTSTEP 3: the 2-bit greys (#ffffff, #aaaaaa, #555555, #000000) of the MegaPixel
    /// display: black text on grey, white text fields, cursor row and Terminal, white-on-black
    /// title bars. Lists are #cccccc, one step lighter than NeXT's #aaaaaa chrome, so the git
    /// colours (AppKit's named colours, darkened) keep a hue at 4.5:1; the hued slots have no
    /// NeXT original.
    pub fn next() -> Self {
        Theme {
            look: "next".into(),
            crt: false,
            panel: st("#000000", "#cccccc"),
            border: st("#555555", "#cccccc"),
            header: st("#000000", "#aaaaaa"),
            directory: bold(st("#000000", "#cccccc")),
            executable: st("#005500", "#cccccc"),
            hidden: st("#555555", "#cccccc"),
            symlink: st("#800080", "#cccccc"),
            cursor: st("#000000", "#ffffff"),
            marked: bold(st("#804000", "#cccccc")),
            marked_cursor: bold(st("#804000", "#ffffff")),
            status: st("#000000", "#aaaaaa"),
            keybar_num: st("#ffffff", "#000000"),
            keybar_label: st("#000000", "#aaaaaa"),
            cmdline: st("#000000", "#ffffff"),
            dialog: st("#000000", "#aaaaaa"),
            dialog_border: st("#000000", "#aaaaaa"),
            dialog_input: st("#000000", "#ffffff"),
            git_branch: bold(st("#800080", "#cccccc")),
            git_modified: st("#804000", "#cccccc"),
            git_added: st("#005500", "#cccccc"),
            git_untracked: st("#aa0000", "#cccccc"),
            git_deleted: st("#550000", "#cccccc"),
            git_renamed: st("#0000aa", "#cccccc"),
            git_conflict: bold(st("#aa0000", "#cccccc")),
            git_ignored: st("#555555", "#cccccc"),
            search_hit: bold(st("#804000", "")),
            accent: st("#ffffff", "#000000"),
            sidebar: st("#000000", "#aaaaaa"),
            tab: st("#555555", "#aaaaaa"),
            tab_active: st("#000000", "#cccccc"),
            preview: st("#000000", "#ffffff"),
        }
    }

    /// BeOS R5: black on white lists, panel grey chrome, the yellow window tab on a marked row
    /// under the cursor, the menu-selection slate blue as the cursor row. Values are the R5
    /// ui_color() defaults (216 grey, 255,203,0 tab, 115,120,184 selection, 0,0,229 keyboard
    /// navigation, 51,102,152 desktop), with black dialog frames as on an R5 window.
    pub fn beos() -> Self {
        Theme {
            look: "beos".into(),
            crt: false,
            panel: st("#000000", "#ffffff"),
            border: st("#acacac", "#ffffff"),
            header: st("#777777", "#ffffff"),
            directory: st("#000000", "#ffffff"),
            executable: st("#008000", "#ffffff"),
            hidden: st("#777777", "#ffffff"),
            symlink: st("#800080", "#ffffff"),
            cursor: st("#000000", "#7378b8"),
            marked: bold(st("#8f6b00", "#ffffff")),
            marked_cursor: bold(st("#000000", "#ffcb00")),
            status: st("#777777", "#d8d8d8"),
            keybar_num: st("#555555", "#d8d8d8"),
            keybar_label: st("#000000", "#d8d8d8"),
            cmdline: st("#000000", "#d8d8d8"),
            dialog: st("#000000", "#d8d8d8"),
            dialog_border: st("#000000", "#d8d8d8"),
            dialog_input: st("#000000", "#ffffff"),
            git_branch: bold(st("#800080", "#ffffff")),
            git_modified: st("#8f6b00", "#ffffff"),
            git_added: st("#008000", "#ffffff"),
            git_untracked: st("#cc0000", "#ffffff"),
            git_deleted: st("#cc0000", "#ffffff"),
            git_renamed: st("#0000e5", "#ffffff"),
            git_conflict: bold(st("#cc0000", "#ffffff")),
            git_ignored: st("#777777", "#ffffff"),
            search_hit: bold(st("#a03800", "")),
            accent: st("#ffffff", "#336698"),
            sidebar: st("#000000", "#d8d8d8"),
            tab: st("#777777", "#d8d8d8"),
            tab_active: st("#000000", "#ffffff"),
            preview: st("#000000", "#d8d8d8"),
        }
    }

    /// OS/2 Warp 3's Workplace Shell: PM pale grey (204,204,204) folders, dialogs and chrome,
    /// black text, dark blue (0,0,128) selection with white text, and an OS/2 Window's light
    /// grey on black for the command line. Greens, browns and dim greys darkened from PM's
    /// table so they read on the grey.
    pub fn os2() -> Self {
        Theme {
            look: "win95".into(),
            crt: false,
            panel: st("#000000", "#cccccc"),
            border: st("#808080", "#cccccc"),
            header: st("#666666", "#cccccc"),
            directory: st("#000000", "#cccccc"),
            executable: st("#005a00", "#cccccc"),
            hidden: st("#666666", "#cccccc"),
            symlink: st("#800080", "#cccccc"),
            cursor: st("#ffffff", "#000080"),
            marked: bold(st("#5c4d00", "#cccccc")),
            marked_cursor: bold(st("#ffff00", "#000080")),
            status: st("#666666", "#cccccc"),
            keybar_num: st("#000080", "#cccccc"),
            keybar_label: st("#000000", "#cccccc"),
            cmdline: st("#aaaaaa", "#000000"),
            dialog: st("#000000", "#cccccc"),
            dialog_border: st("#000080", "#cccccc"),
            dialog_input: st("#000000", "#cccccc"),
            git_branch: bold(st("#800080", "#cccccc")),
            git_modified: st("#5c4d00", "#cccccc"),
            git_added: st("#005a00", "#cccccc"),
            git_untracked: st("#800000", "#cccccc"),
            git_deleted: st("#800000", "#cccccc"),
            git_renamed: st("#0000ff", "#cccccc"),
            git_conflict: bold(st("#800000", "#cccccc")),
            git_ignored: st("#666666", "#cccccc"),
            search_hit: bold(st("#803300", "")),
            accent: st("#cccccc", "#000080"),
            sidebar: st("#000000", "#cccccc"),
            tab: st("#666666", "#cccccc"),
            tab_active: st("#000000", "#cccccc"),
            preview: st("#000000", "#cccccc"),
        }
    }

    /// Every built-in theme with its group (a `theme.group.*` key), in the order both apps
    /// list them: a group's themes stand together.
    pub const BUILTIN: &[BuiltinTheme] = &[
        ("cyber", "theme.group.coxswain", Theme::cyber),
        ("dark", "theme.group.coxswain", Theme::dark),
        ("light", "theme.group.coxswain", Theme::light),
        ("midnight", "theme.group.coxswain", Theme::midnight),
        ("nord", "theme.group.coxswain", Theme::nord),
        ("amber", "theme.group.coxswain", Theme::amber),
        ("paper", "theme.group.coxswain", Theme::paper),
        ("nc", "theme.group.dos", Theme::nc),
        ("mc", "theme.group.dos", Theme::mc),
        ("xtree", "theme.group.dos", Theme::xtree),
        ("dosshell", "theme.group.dos", Theme::dosshell),
        ("win31", "theme.group.windows", Theme::win31),
        ("win95", "theme.group.windows", Theme::win95),
        ("winxp", "theme.group.windows", Theme::winxp),
        ("win7", "theme.group.windows", Theme::win7),
        ("win10", "theme.group.windows", Theme::win10),
        ("win11", "theme.group.windows", Theme::win11),
        ("win11-dark", "theme.group.windows", Theme::win11_dark),
        ("system7", "theme.group.mac", Theme::system7),
        ("platinum", "theme.group.mac", Theme::platinum),
        ("aqua", "theme.group.mac", Theme::aqua),
        ("macos", "theme.group.mac", Theme::macos),
        ("macos-dark", "theme.group.mac", Theme::macos_dark),
        ("next", "theme.group.desktops", Theme::next),
        ("beos", "theme.group.desktops", Theme::beos),
        ("os2", "theme.group.desktops", Theme::os2),
        ("workbench", "theme.group.desktops", Theme::workbench),
        ("atarist", "theme.group.desktops", Theme::atarist),
        ("c64", "theme.group.home", Theme::c64),
        ("spectrum", "theme.group.home", Theme::spectrum),
        ("cpc", "theme.group.home", Theme::cpc),
        ("msx", "theme.group.home", Theme::msx),
        ("bbc", "theme.group.home", Theme::bbc),
    ];

    /// Every built-in theme: (name, theme), in the order both apps list them.
    pub fn builtin() -> Vec<(&'static str, Theme)> {
        Self::BUILTIN.iter().map(|(n, _, f)| (*n, f())).collect()
    }

    /// The group a theme is listed under: a `theme.group.*` key; *Your own* for one not built in.
    pub fn group(name: &str) -> &'static str {
        Self::BUILTIN.iter().find(|(n, ..)| *n == name).map_or("theme.group.own", |(_, g, _)| g)
    }

    pub fn from_palette(p: &Palette) -> Self {
        // A dark theme's buttons are dark letters on a pastel accent: semibold, or they are
        // hard to read at 13 px (a light theme's are white on a deep accent and need nothing).
        let dark = color_to_rgb(p.bg).is_some_and(|(r, g, b)| u32::from(r) * 30 + u32::from(g) * 59 + u32::from(b) * 11 < 12_800);
        let accent = st(p.bg, p.accent);
        Theme {
            look: p.look.into(),
            crt: p.look == "crt",
            panel: st(p.fg, p.bg),
            border: st(p.border, p.bg),
            header: st(p.dim, p.bg),
            directory: st(p.dir, p.bg),
            executable: st(p.green, p.bg),
            hidden: st(p.dim, p.bg),
            symlink: st(p.magenta, p.bg),
            cursor: st(p.sel_fg, p.sel_bg),
            marked: bold(st(p.yellow, p.bg)),
            marked_cursor: bold(st(p.yellow, p.sel_bg)),
            status: st(p.dim, p.alt),
            keybar_num: st(p.dim, p.alt),
            keybar_label: st(p.fg, p.raised),
            cmdline: st(p.fg, p.alt),
            dialog: st(p.fg, p.raised),
            dialog_border: st(p.accent, p.raised),
            dialog_input: st(p.fg, p.bg),
            git_branch: bold(st(p.magenta, p.bg)),
            git_modified: st(p.yellow, p.bg),
            git_added: st(p.green, p.bg),
            git_untracked: st(p.red, p.bg),
            git_deleted: st(p.red, p.bg),
            git_renamed: st(p.blue, p.bg),
            git_conflict: bold(st(p.red, p.bg)),
            git_ignored: st(p.dim, p.bg),
            search_hit: bold(st(p.orange, "")),
            accent: if dark { bold(accent) } else { accent },
            sidebar: st(p.fg, p.alt),
            tab: st(p.dim, p.alt),
            tab_active: st(p.fg, p.bg),
            preview: st(p.fg, p.alt),
        }
    }
}

/// The handful of colors a modern theme is derived from.
pub struct Palette {
    pub look: &'static str,
    pub bg: &'static str,
    pub alt: &'static str,
    pub raised: &'static str,
    pub fg: &'static str,
    pub dim: &'static str,
    pub border: &'static str,
    pub accent: &'static str,
    pub sel_bg: &'static str,
    /// Text on `sel_bg`, the cursor row.
    pub sel_fg: &'static str,
    pub dir: &'static str,
    pub red: &'static str,
    pub green: &'static str,
    pub yellow: &'static str,
    pub blue: &'static str,
    pub magenta: &'static str,
    pub orange: &'static str,
}

/// CGA palette for the 16 named colors, so the GUI can render names too.
pub fn color_to_rgb(color: &str) -> Option<(u8, u8, u8)> {
    if let Some(hex) = color.strip_prefix('#') {
        let v = u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 6)?;
        return Some(((v >> 16) as u8, (v >> 8) as u8, v as u8));
    }
    Some(match color.to_ascii_lowercase().replace(['_', '-', ' '], "").as_str() {
        "black" => (0, 0, 0),
        "blue" => (0, 0, 170),
        "green" => (0, 170, 0),
        "cyan" => (0, 170, 170),
        "red" => (170, 0, 0),
        "magenta" => (170, 0, 170),
        "yellow" => (170, 85, 0),
        "gray" | "grey" => (170, 170, 170),
        "darkgray" | "darkgrey" => (85, 85, 85),
        "lightblue" => (85, 85, 255),
        "lightgreen" => (85, 255, 85),
        "lightcyan" => (85, 255, 255),
        "lightred" => (255, 85, 85),
        "lightmagenta" => (255, 85, 255),
        "lightyellow" => (255, 255, 85),
        "white" => (255, 255, 255),
        _ => return None,
    })
}

// ---------------------------------------------------------------- glyphs

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Glyphs {
    pub branch: String,
    pub ahead: String,
    pub behind: String,
    pub staged: String,
    pub modified: String,
    pub untracked: String,
    pub deleted: String,
    pub renamed: String,
    pub conflict: String,
    pub ignored: String,
    pub stash: String,
    pub clean: String,
    pub folder: String,
    pub file: String,
    pub symlink: String,
    /// A file only in the cloud (OneDrive, Dropbox, iCloud …), not downloaded.
    pub cloud: String,
}

impl Default for Glyphs {
    fn default() -> Self {
        Glyphs::nerd()
    }
}

impl Glyphs {
    /// Nerd Font glyphs, as oh-my-posh's git segment uses.
    pub fn nerd() -> Self {
        let s = |x: &str| x.to_string();
        Glyphs {
            branch: s("\u{e0a0}"),
            ahead: s("\u{2191}"),
            behind: s("\u{2193}"),
            staged: s("\u{f046}"),
            modified: s("\u{f044}"),
            untracked: s("\u{f128}"),
            deleted: s("\u{f014}"),
            renamed: s("\u{f45a}"),
            conflict: s("\u{f071}"),
            ignored: s("\u{f070}"),
            stash: s("\u{eb4b}"),
            clean: s("\u{f00c}"),
            folder: s("\u{f07b}"),
            file: s("\u{f15b}"),
            symlink: s("\u{f0c1}"),
            cloud: s("\u{f0c2}"),
        }
    }

    pub fn ascii() -> Self {
        let s = |x: &str| x.to_string();
        Glyphs {
            branch: s("git:"),
            ahead: s("^"),
            behind: s("v"),
            staged: s("+"),
            modified: s("~"),
            untracked: s("?"),
            deleted: s("-"),
            renamed: s(">"),
            conflict: s("!"),
            ignored: s("."),
            stash: s("$"),
            clean: s("="),
            folder: s("/"),
            file: s(" "),
            symlink: s("@"),
            cloud: s("*"),
        }
    }
}

// ---------------------------------------------------------------- config

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserCommand {
    /// Single key that picks the entry inside the F2 menu.
    pub key: String,
    pub label: String,
    /// Run by the shell in the active panel's directory. `%f` = file under cursor,
    /// `%d` = directory, `%s` = marked files (or the cursor file), each shell-quoted.
    pub command: String,
    /// Wait for Enter afterwards so the output can be read.
    #[serde(default)]
    pub wait: bool,
    /// Offered only where it fits: on a `file`, or in a `git` repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<crate::user_menu::When>,
}

impl UserCommand {
    /// Substitute `%f`, `%d`, `%s` (shell-quoted) and `%%`, for `Line::command`. A name that
    /// starts with `-` comes as `./-name`, so a program does not take it for an option. On
    /// Windows `%d` is `.`, the folder the command runs in: cmd.exe would expand a `%` in it.
    pub fn expand(&self, dir: &std::path::Path, file: Option<&std::path::Path>, selected: &[PathBuf]) -> crate::tools::Line {
        let mut line = crate::tools::Line::default();
        let file_q = file
            .map(|f| {
                let name = f.file_name().map(|n| n.to_string_lossy()).unwrap_or_else(|| f.to_string_lossy());
                let name = if name.starts_with('-') { format!(".{}{name}", std::path::MAIN_SEPARATOR) } else { name.into_owned() };
                line.word(&name)
            })
            .unwrap_or_default();
        let sel = if selected.is_empty() { file_q.clone() } else { selected.iter().map(|p| line.word(&p.to_string_lossy())).collect::<Vec<_>>().join(" ") };
        let dir_q = if cfg!(windows) { ".".to_string() } else { quote(&dir.to_string_lossy()) };
        let mut out = String::new();
        let mut it = self.command.chars().peekable();
        while let Some(c) = it.next() {
            match (c, it.peek()) {
                ('%', Some('f')) => out += &file_q,
                ('%', Some('d')) => out += &dir_q,
                ('%', Some('s')) => out += &sel,
                ('%', Some('%')) => out.push('%'),
                _ => {
                    out.push(c);
                    continue;
                }
            }
            it.next();
        }
        line.cmd = out;
        line
    }
}

/// Quote a word for the platform shell.
pub fn quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./+,:@".contains(c)) {
        s.to_string()
    } else if cfg!(windows) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Quote a word for `shell`, a `$SHELL`, for a line the user then runs there: fish reads `\\`
/// and `\'` inside single quotes, so `quote`'s `'x\'` would end its quoting early and run what
/// follows. Any other shell takes `quote`'s.
pub fn quote_for(shell: &str, s: &str) -> String {
    let fish = std::path::Path::new(shell).file_name().is_some_and(|n| n == "fish");
    let q = quote(s);
    if cfg!(windows) || !fish || q == s {
        return q;
    }
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Previews made by external tools (desktop app): LaTeX, Typst, LibreOffice, Ghostscript,
/// PlantUML, Kroki's diagram programs, pandoc, DuckDB, ffmpeg and ImageMagick; and OCRmyPDF for
/// *Make PDF searchable*, ffmpeg and ImageMagick for their conversions (both apps). Each runs from a locally installed tool or from a container image.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PreviewConfig {
    /// "auto" (an installed tool first, else a container), "local" or "container".
    pub prefer: String,
    /// Per kind of file (`preview_tools::KINDS`), overrides `prefer`: "local", "container" or
    /// "off" (no preview by a tool): `tools = { latex = "container", video = "off" }`.
    pub tools: BTreeMap<String, String>,
    /// "auto" (podman, else docker), "podman", "docker", or "off" for no containers.
    pub container: String,
    /// Container image per tool. An empty image means that tool never runs in a container.
    /// Containers run without network, with the file's folder mounted read-only.
    pub images: BTreeMap<String, String>,
    /// Seconds before a conversion is stopped. A first container run also pulls the image,
    /// which the timeout does not cover.
    pub timeout: u64,
    /// Build a LaTeX document by itself when it is shown and its sources changed, instead of
    /// waiting for *Build PDF*.
    pub latex_auto: bool,
    /// Keep the pulled images up to date: "off", "weekly" or "monthly". The desktop app pulls
    /// them in the background, never on battery, and removes each replaced copy by its id.
    pub image_updates: String,
    /// Remove an image of `images` that no preview used for 90 days (desktop app).
    pub image_prune: bool,
    /// E-books calibre reads (MOBI, Kindle, FictionBook, LIT, Palm): previews, their text for
    /// search, and *Convert e-book*; an installed calibre first, else `images.calibre`.
    pub ebooks: bool,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        PreviewConfig {
            prefer: "auto".into(),
            tools: BTreeMap::new(),
            container: "auto".into(),
            // Every image's default and why: `preview_tools::IMAGES`.
            images: crate::preview_tools::default_images(),
            timeout: 120,
            latex_auto: true,
            image_updates: "off".into(),
            image_prune: false,
            ebooks: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    /// Where file names are found (`name_roots`). Empty = `/` (Unix) or every fixed drive (Windows).
    #[serde(rename = "name_roots")]
    pub roots: Vec<PathBuf>,
    /// Left out of the names (`name_exclude`): a path (`/proc`) leaves out that tree, a bare name
    /// (`node_modules`) every folder with that name.
    #[serde(rename = "name_exclude")]
    pub exclude: Vec<String>,
    pub max_results: usize,
    /// Follow changes live (inotify / FSEvents / ReadDirectoryChanges). Off = hourly rebuild only.
    pub watch: bool,
    /// Keep the text of files, to search in it (Find file, Tab).
    pub text: bool,
    /// The folders whose text is kept. Empty = the home folder.
    pub text_roots: Vec<PathBuf>,
    /// Folders left out by name, wherever they are. Hidden folders and folders holding a
    /// `.nosearch` file are left out too.
    pub text_exclude: Vec<String>,
    /// Folders whose files are found by name, and counted in folder sizes, but never read.
    pub names_only: Vec<PathBuf>,
    /// Search by meaning too (a language model, downloaded when this is turned on).
    pub meaning: bool,
    /// Which model makes the vectors: "builtin" (downloaded, runs here), "ollama"
    /// (an Ollama server's `/api/embed`) or "openai" (any `/v1/embeddings`: LM Studio,
    /// Lemonade, llama.cpp, vLLM, OpenAI itself).
    pub meaning_engine: String,
    /// The server: empty for Ollama on this machine (`http://localhost:11434`); for "openai" the
    /// base URL, e.g. `http://localhost:8000/api/v1` for Lemonade.
    pub meaning_url: String,
    /// The server's embedding model, e.g. `bge-m3`.
    pub meaning_model: String,
    /// The environment variable that holds the server's API key, if it wants one; the key
    /// itself is never written into this file.
    pub meaning_key_env: String,
    /// Where the built-in model runs: "auto" (Apple's GPU through Metal on a Mac that has one,
    /// the CPU elsewhere) or "cpu".
    pub meaning_device: String,
    /// Ask: the chat model that answers questions from the closest passages, on the server
    /// above (Ollama on this machine when the vectors are the built-in model's), e.g.
    /// `qwen3:8b`. Empty: Ask is not set up.
    pub ask_model: String,
    /// The chat model for questions about code (most of what Ask found is code, or the question
    /// names something in code), on the same server, e.g. `qwen3-coder:30b`. Empty: Ask's own.
    pub ask_code_model: String,
    /// Let Ask's chat model think before it answers (Qwen3, DeepSeek-R1 …): better reasoning,
    /// many seconds before the first word. Off: it is asked not to, where it can be.
    pub ask_think: bool,
    /// Tokens a chat model on a server is given, the sources, the question and the answer
    /// together: Ollama is asked for this context (`num_ctx`); an OpenAI-style server must have
    /// at least as much. More reads more of the files, and holds more of the server's memory:
    /// 8,192 keeps `qwen3:8b` and `bge-m3` both on an 8 GB card, where Ollama's own 32,768 pushed
    /// them out of it and every question loaded both again. A built-in model has its own.
    pub ask_context: usize,
    /// Larger files are left out. Bytes.
    pub text_max_size: u64,
    /// Search inside archives too: their entries by name, and the text of their files.
    pub archives: bool,
    /// Look inside the archives everywhere the name index reaches, caches and programs' data
    /// folders too, for their names; off, only those in the folders whose text is read, less
    /// caches (`store::cache_folder`). Their text is read in those folders either way.
    pub archives_everywhere: bool,
    /// Keep the history of the git repositories in the text roots too: commit messages,
    /// authors and changed paths (the newest 2000 commits of each), found like text.
    pub history: bool,
    /// Files that are only in the cloud (OneDrive, Dropbox, Google Drive, Proton Drive, iCloud,
    /// cloud mounts on Linux): "local-only" finds them by name and never reads them, as reading
    /// one downloads it; "all" reads them like the rest.
    pub cloud: String,
    /// Folders in the cloud whose files are read even so: a cloud mount on Linux, say.
    pub cloud_read: Vec<PathBuf>,
    /// Read the formats no reader here knows (OneNote, XPS, CHM, Outlook data files …) with
    /// Apache Tika, in a container without network (`[preview.images] tika`).
    pub tika: bool,
    /// Transcribe speech with Whisper when asked to (a file or a folder at a time): the model
    /// is downloaded once this is on, and the image pulled when no whisper.cpp is installed.
    pub whisper: bool,
    /// The Whisper model: `tiny`, `base`, `small` or `large-v3-turbo`; empty: the one this
    /// machine runs well.
    pub whisper_model: String,
}

/// The trees the kernel makes up, never indexed: Linux's `/proc`, `/sys`, `/dev` and `/run`;
/// the BSDs have no `/sys` or `/run`, illumos has `/devices` and `/system` (contracts, objects).
fn kernel_trees() -> &'static [&'static str] {
    if cfg!(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd")) {
        &["/proc", "/dev"]
    } else if cfg!(any(target_os = "illumos", target_os = "solaris")) {
        &["/proc", "/dev", "/devices", "/system"]
    } else {
        &["/proc", "/sys", "/dev", "/run"]
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            roots: vec![],
            exclude: kernel_trees().iter().copied().chain(["/tmp/.X11-unix"]).map(String::from).collect(),
            max_results: 10_000,
            watch: true,
            text: true,
            text_roots: vec![],
            text_exclude: ["node_modules", "target", "build", "dist", "out", "obj", "vendor", "__pycache__", "Trash"].map(String::from).to_vec(),
            names_only: vec![],
            meaning: false,
            meaning_engine: "builtin".into(),
            meaning_url: String::new(),
            meaning_model: String::new(),
            meaning_key_env: String::new(),
            meaning_device: "auto".into(),
            ask_model: String::new(),
            ask_code_model: String::new(),
            ask_think: false,
            ask_context: 8192,
            text_max_size: 20 * 1024 * 1024,
            archives: true,
            archives_everywhere: false,
            history: true,
            cloud: "local-only".into(),
            cloud_read: vec![],
            tika: false,
            whisper: false,
            whisper_model: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiConfig {
    /// The desktop app's own theme, so the two apps can differ.
    pub theme: String,
    /// CSS font-family for the interface.
    pub font: String,
    /// For file icons and git glyphs: a Nerd Font.
    pub icon_font: String,
    /// For the text preview and the command line.
    pub mono_font: String,
    pub font_size: f32,
    /// Row height as a multiple of the font size.
    pub line_height: f32,
}

impl Default for GuiConfig {
    fn default() -> Self {
        GuiConfig {
            theme: "cyber".into(),
            font: "Inter, 'Segoe UI Variable', 'Segoe UI', system-ui, -apple-system, 'Noto Sans', sans-serif".into(),
            icon_font: "'Symbols Nerd Font Mono', 'JetBrainsMono Nerd Font', 'MesloLGS Nerd Font', 'MesloLGM Nerd Font Mono', 'FiraCode Nerd Font', 'CaskaydiaCove Nerd Font', 'Hack Nerd Font', monospace".into(),
            mono_font: "'JetBrains Mono', 'Cascadia Code', 'MesloLGS Nerd Font', Menlo, Consolas, monospace".into(),
            font_size: 13.0,
            line_height: 1.9,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The terminal app's theme: a built-in (`cyber`, `nc`, `win95`, see `Theme::builtin`) or a
    /// `[themes.<name>]` table.
    pub theme: String,
    /// `nerd` or `ascii`, or a full `[glyph_set]` table.
    pub glyphs: String,
    pub glyph_set: Option<Glyphs>,
    /// "auto" (the system's language, or the nearest one Coxswain has) or a code such as "da",
    /// "en-AU" or "es-AR"; see `i18n::LANGUAGES`.
    pub language: String,
    pub show_hidden: bool,
    /// Measure folders in the background and show their sizes. The desktop app has its own
    /// switch in the columns menu; this is where it starts.
    pub folder_sizes: bool,
    /// Overrides `$EDITOR` / `$PAGER`.
    pub editor: Option<String>,
    pub viewer: Option<String>,
    /// F3 on a CycloneDX BOM opens the terminal app's BOM viewer (F3 again shows the source).
    /// Off, F3 opens it in the viewer like any file.
    pub bom_viewer: bool,
    /// F3 on build provenance (an in-toto attestation) opens the terminal app's provenance viewer
    /// (F3 again shows the source). Off, F3 opens it in the viewer like any file.
    pub provenance_viewer: bool,
    pub confirm_delete: bool,
    /// What a right-click on a row does: `mark` it (Norton Commander) or open the action
    /// `menu` (a file explorer). Ctrl+right-click does the other one in the desktop app.
    pub right_click: String,
    /// A short hint on the status line that fits what is under the cursor, each a few times.
    pub hints: bool,
    /// Look for a newer release on GitHub at startup (at most once a day).
    pub check_updates: bool,
    /// Action -> keys. Listing an action replaces its default keys; `[]` unbinds it. An action
    /// this version does not know (a newer one's, in a config shared with it) is left out.
    #[serde(deserialize_with = "known_actions")]
    pub keys: BTreeMap<Action, Vec<String>>,
    pub themes: BTreeMap<String, Theme>,
    /// F2's entries. None (no `[[user_menu]]`): the built-in ones (`user_menu::entries`).
    pub user_menu: Option<Vec<UserCommand>>,
    pub search: SearchConfig,
    pub preview: PreviewConfig,
    pub gui: GuiConfig,
    pub git: GitConfig,
    pub archive: ArchiveConfig,
    pub chess: ChessConfig,
    /// The names in `[keys]` that are no action of this version, left out (`keys`): a notice
    /// names them.
    #[serde(skip)]
    pub unknown_actions: Vec<String>,
    /// The settings config.toml gives that could not be used, each left at its default
    /// (`Config::lenient`): a notice names each one's line and what is wrong.
    #[serde(skip)]
    pub refused: Vec<Refused>,
}

/// A setting in config.toml that could not be used: its line (from 1) and what is wrong with
/// it, as a sentence ("`theme` must be text").
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Refused {
    pub line: usize,
    pub problem: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ChessConfig {
    /// How moves are written: `figurines` (♘f3) or `letters`, in the app language's letters
    /// (Nf3 in English, Sf3 in German). The files keep English SAN.
    pub notation: String,
    /// The board's colours: `theme` (from the theme's own), or a tournament board, `green` or
    /// `brown`.
    pub board: String,
    /// A Polyglot opening book (`.bin`) whose moves the board shows for each position. Empty:
    /// the first book in the game file's own folder, if it has one.
    pub book: String,
    /// How the terminal app draws the pieces: `auto` (the finest block characters the terminal
    /// is known to draw), `octants`, `sextants`, `quadrants`, `halves` or `figurines`.
    pub pieces: String,
}

impl Default for ChessConfig {
    fn default() -> Self {
        ChessConfig { notation: "figurines".into(), board: "theme".into(), book: String::new(), pieces: "auto".into() }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchiveConfig {
    /// Open disk images (.iso, .dmg, .vhd, .vhdx, .vmdk, .qcow2, .vdi, .wim, a raw .img) like
    /// folders, read-only, with an installed 7-Zip. Off: Enter on one does what it does with
    /// any file; the preview shows its facts either way.
    pub disk_images: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfig {
    /// Show when each file and folder was last committed, and by whom: a column in the desktop
    /// app's details view and the preview, the info line in the terminal app.
    pub last_commit: bool,
    /// Where "Open in your git tool" hands off: a tool's name as Settings lists it (`lazygit`,
    /// `Sublime Merge`) or a command (`%d` is the repository). Empty: the first one installed,
    /// else a terminal in the repository (`handoff`).
    pub tool: String,
    /// Where a folder has both `.jj` and `.git`, ask git, not Jujutsu (`vcs`).
    pub prefer_git: bool,
}

impl Default for GitConfig {
    fn default() -> Self {
        GitConfig { last_commit: true, tool: String::new(), prefer_git: false }
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut c = Config {
            theme: "nc".into(),
            glyphs: "nerd".into(),
            glyph_set: None,
            language: "auto".into(),
            show_hidden: true,
            folder_sizes: true,
            editor: None,
            viewer: None,
            bom_viewer: true,
            provenance_viewer: true,
            confirm_delete: true,
            right_click: "mark".into(),
            hints: true,
            check_updates: true,
            keys: BTreeMap::new(),
            themes: BTreeMap::new(),
            user_menu: None,
            search: SearchConfig::default(),
            preview: PreviewConfig::default(),
            gui: GuiConfig::default(),
            git: GitConfig::default(),
            archive: ArchiveConfig::default(),
            chess: ChessConfig::default(),
            unknown_actions: vec![],
            refused: vec![],
        };
        c.fill_defaults();
        c
    }
}

/// `text` read whole, or its first mistake: the lines (from 0, inclusive) it is on, to be left
/// out, and what is wrong in words.
fn mistake(text: &str) -> Result<Config, (usize, usize, String)> {
    let line_of = |at: usize| text[..at.min(text.len())].matches('\n').count();
    // Bad TOML: its line, or a table header and the lines under it.
    let doc = match toml_edit::Document::parse(text) {
        Ok(d) => d,
        Err(e) => {
            let at = line_of(e.span().map_or(0, |s| s.start));
            let header = text.split('\n').nth(at).is_some_and(|l| l.trim_start().starts_with('['));
            let to = if header { table_end(text, at) } else { at };
            let why = e.message().lines().next().unwrap_or_default().to_string();
            return Err((at, to, crate::t!("config.bad_toml", "why" => why)));
        }
    };
    match toml::from_str::<Config>(text) {
        Err(e) => {
            let at = e.span().map_or(0, |s| s.start);
            let (path, from, to) = entry_at(text, doc.as_table(), "", at).unwrap_or_else(|| (String::new(), line_of(at), line_of(at)));
            Err((from, to, problem(&path, e.message())))
        }
        Ok(_) => Config::parse(text).map_err(|why| {
            // Parsed, so it is the keys: the first entry of `[keys]` with a key name that does
            // not exist or a key an earlier entry has.
            let keys = doc.get("keys").and_then(|k| k.as_table_like());
            let mut seen: HashMap<Key, &str> = HashMap::new();
            for (name, item) in keys.into_iter().flat_map(|t| t.iter()) {
                let list = item.as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>()).unwrap_or_default();
                let wrong = list.iter().find_map(|k| match k.parse::<Key>() {
                    Err(e) => Some(e),
                    Ok(key) => match seen.insert(key, name) {
                        Some(other) if other != name => Some(crate::t!("err.key_twice", "key" => k, "a" => other, "b" => name)),
                        _ => None,
                    },
                });
                if let Some(wrong) = wrong {
                    let span = keys.and_then(|t| t.key(name)).and_then(|k| k.span());
                    let from = span.map_or(0, |s| line_of(s.start));
                    let to = item.span().map_or(from, |s| line_of(s.end.saturating_sub(1)));
                    return (from, to, crate::t!("config.wrong", "key" => format!("keys.{name}"), "why" => wrong));
                }
            }
            (0, 0, why)
        }),
    }
}

/// The last line (from 0) of the table whose header is on line `at`: the line before the next
/// header, or the file's last.
// ponytail: a line of a multi-line string that starts with `[` ends it early; rare in a config.
fn table_end(text: &str, at: usize) -> usize {
    let lines: Vec<&str> = text.split('\n').collect();
    (at + 1..lines.len()).find(|&i| lines[i].trim_start().starts_with('[')).map_or(lines.len() - 1, |i| i - 1)
}

/// The setting of `table` that byte `at` of `text` is in: its dotted name and its first and
/// last line (from 0). A table of an array of tables (`[[user_menu]]`) whose own value is
/// wrong (a key missing) is the whole table.
fn entry_at(text: &str, table: &toml_edit::Table, prefix: &str, at: usize) -> Option<(String, usize, usize)> {
    let line_of = |i: usize| text[..i.min(text.len())].matches('\n').count();
    for (name, item) in table.iter() {
        let path = if prefix.is_empty() { name.to_string() } else { format!("{prefix}.{name}") };
        match item {
            toml_edit::Item::Table(t) => {
                if let Some(found) = entry_at(text, t, &path, at) {
                    return Some(found);
                }
                if let Some(s) = t.span().filter(|s| s.contains(&at)) {
                    let from = line_of(s.start);
                    return Some((path, from, table_end(text, from)));
                }
            }
            toml_edit::Item::ArrayOfTables(a) => {
                for t in a.iter() {
                    if let Some(found) = entry_at(text, t, &path, at) {
                        return Some(found);
                    }
                }
                // The entry whose header comes last before `at`.
                let head = a.iter().filter_map(|t| t.span()).map(|s| s.start).filter(|&s| s <= at).max()?;
                let from = line_of(head);
                return Some((path, from, table_end(text, from)));
            }
            toml_edit::Item::Value(v) => {
                let start = table.key(name).and_then(|k| k.span()).map_or(0, |s| s.start);
                let end = v.span().map_or(start, |s| s.end);
                if (start..end.max(start + 1)).contains(&at) {
                    // Inside an inline table (`images = { latex = 5 }`): named by its key there,
                    // the whole entry left out.
                    let mut path = path;
                    let mut inner = v.as_inline_table();
                    while let Some(t) = inner.take() {
                        if let Some((k, v)) = t.iter().find(|(_, v)| v.span().is_some_and(|s| s.contains(&at))) {
                            path = format!("{path}.{k}");
                            inner = v.as_inline_table();
                        }
                    }
                    return Some((path, line_of(start), line_of(end.saturating_sub(1).max(start))));
                }
            }
            toml_edit::Item::None => {}
        }
    }
    None
}

/// What is wrong with the setting `key`, in words, from what serde said.
fn problem(key: &str, said: &str) -> String {
    let said = said.lines().next().unwrap_or_default();
    let expected = said.rsplit_once("expected ").map(|(_, e)| e).unwrap_or_default();
    let must = if said.starts_with("unknown variant") {
        Some(crate::t!("config.must_one_of", "key" => key, "values" => expected.trim_start_matches("one of ")))
    } else if !said.starts_with("invalid type") {
        None
    } else if expected.contains("string") {
        Some(crate::t!("config.must_text", "key" => key))
    } else if expected.contains("bool") {
        Some(crate::t!("config.must_bool", "key" => key))
    } else if ["integer", "i64", "u64", "u32", "u16", "u8", "usize", "f64", "f32", "float", "number"].iter().any(|n| expected.contains(n)) {
        Some(crate::t!("config.must_number", "key" => key))
    } else if expected.contains("sequence") {
        Some(crate::t!("config.must_list", "key" => key))
    } else if expected.contains("map") || expected.contains("struct") {
        Some(crate::t!("config.must_table", "key" => key))
    } else {
        None
    };
    match must {
        Some(m) => m,
        None if key.is_empty() => said.to_string(),
        None => crate::t!("config.wrong", "key" => key, "why" => said),
    }
}

/// Most terminals never pass on Ctrl with a punctuation mark (Ctrl+,): the terminal app shows
/// another key of the action when it has one.
fn delivered(key: &str) -> bool {
    let rest = key.get(..5).filter(|p| p.eq_ignore_ascii_case("ctrl+")).map(|_| &key[5..]);
    !rest.is_some_and(|r| r.chars().count() == 1 && r.chars().all(|c| c.is_ascii_punctuation()))
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        Some(crate::dirs::config_dir()?.join("coxswain").join("config.toml"))
    }

    /// config.toml ready to be opened at `[keys]` (Settings → Keys): its path and the line
    /// (from 1) of the table. A file without one gets a commented example at its end first.
    pub fn prepare_keys() -> Result<(PathBuf, usize), String> {
        prepare_at(|l| l.starts_with("[keys]"), KEYS_EXAMPLE)
    }

    /// Where Coxswain keeps things, (what, path), for `--paths`.
    pub fn paths() -> Vec<(&'static str, Option<PathBuf>)> {
        vec![
            ("config", Config::path()),
            ("state", crate::state::AppState::path()),
            ("cache", crate::helper::folder()),
            ("name index", crate::index::Index::cache_path()),
            ("search store", crate::store::Store::path()),
            ("models", crate::models::folder()),
            ("previews", crate::dirs::cache_dir().map(|d| d.join("coxswain").join("previews"))),
            ("archive looks", crate::dirs::cache_dir().map(|d| d.join("coxswain").join("peek"))),
        ]
    }

    /// Load the user config, falling back to defaults when the file does not exist. A mistake
    /// in it costs only the setting it is in (`lenient`); only a file that cannot be read at
    /// all is an error.
    pub fn load() -> Result<Config, String> {
        match Config::path().map(std::fs::read_to_string) {
            Some(Ok(text)) => Ok(Config::lenient(&text)),
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("config: {e}")),
            _ => Ok(Config::default()),
        }
    }

    /// The search settings, for the helper above all: as the user set them even when the file
    /// has a mistake elsewhere (`lenient`), or the defaults would read the folders left out
    /// and the archives turned off, and read them again once the mistake is fixed.
    pub fn load_search() -> SearchConfig {
        let text = Config::path().and_then(|p| std::fs::read_to_string(p).ok());
        Config::lenient(text.as_deref().unwrap_or_default()).search
    }

    /// `text` (a config.toml) read as far as it can be: each setting with a mistake (bad
    /// TOML, a wrong type, a key name that does not exist, a key bound twice) is left at its
    /// default, the rest kept as the user set it; `refused` names each mistake and its line.
    pub fn lenient(text: &str) -> Config {
        if let Ok(c) = Config::parse(text) {
            return c;
        }
        // The broken lines are blanked one mistake at a time, so the lines keep their numbers.
        let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        let mut refused = vec![];
        loop {
            let cur = lines.join("\n");
            let (from, to, problem) = match mistake(&cur) {
                Ok(mut c) => {
                    refused.sort_by_key(|r: &Refused| r.line);
                    c.refused = refused;
                    return c;
                }
                Err(m) => m,
            };
            refused.push(Refused { line: from + 1, problem });
            let end = to.min(lines.len().saturating_sub(1));
            // Nothing left to blank there: the defaults, rather than a loop.
            if from > end || lines[from..=end].iter().all(|l| l.trim().is_empty()) {
                return Config { refused, ..Config::default() };
            }
            lines[from..=end].iter_mut().for_each(String::clear);
        }
    }

    /// The settings `new_text` would save, where `old_text` is the file now: refused only when
    /// it has a mistake `old_text` did not have, so Settings still save while the user has
    /// not mended a line of their own yet.
    pub fn parse_saved(old_text: &str, new_text: &str) -> Result<Config, String> {
        Config::parse(new_text).or_else(|e| {
            let (old, new) = (Config::lenient(old_text), Config::lenient(new_text));
            let had = |r: &Refused| old.refused.iter().any(|o| o.problem == r.problem);
            if new.refused.iter().all(had) { Ok(new) } else { Err(e) }
        })
    }

    /// `text` (a config.toml) with the value at `keys` (`["search", "meaning"]`) set, its
    /// comments and layout kept.
    pub fn edit(text: &str, keys: &[&str], value: toml_edit::Value) -> Result<String, String> {
        let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("config: {e}"))?;
        let (last, parents) = keys.split_last().ok_or("config: no key")?;
        // Parents as tables or as inline tables (`images = { latex = "…" }`): both parse.
        let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
        let mut inline = false;
        for k in parents {
            if table.get(k).is_none() {
                // A table cannot sit inside an inline one.
                let new = if inline {
                    toml_edit::Item::Value(toml_edit::InlineTable::new().into())
                } else {
                    let mut t = toml_edit::Table::new();
                    t.set_implicit(true);
                    toml_edit::Item::Table(t)
                };
                table.insert(k, new);
            }
            let item = table.get_mut(k).expect("there now");
            inline |= item.is_inline_table();
            table = item.as_table_like_mut().ok_or_else(|| crate::t!("err.not_a_table", "key" => k))?;
        }
        // Update in place where the key exists, so its comments stay.
        match table.get_mut(last).and_then(|i| i.as_value_mut()) {
            Some(old) => {
                let decor = old.decor().clone();
                *old = value;
                *old.decor_mut() = decor;
            }
            None => {
                table.insert(last, toml_edit::value(value));
            }
        }
        Ok(doc.to_string())
    }

    /// `text` (a config.toml) without the key at `keys`, so its default applies; the rest kept.
    pub fn unset(text: &str, keys: &[&str]) -> Result<String, String> {
        let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("config: {e}"))?;
        let (last, parents) = keys.split_last().ok_or("config: no key")?;
        let table = parents.iter().try_fold(doc.as_table_mut() as &mut dyn toml_edit::TableLike, |t, k| {
            t.get_mut(k).and_then(|i| i.as_table_like_mut())
        });
        if let Some(t) = table {
            t.remove(last);
        }
        Ok(doc.to_string())
    }

    /// Set one value in the user's config.toml and write it; only what still parses is written.
    pub fn save_value(keys: &[&str], value: toml_edit::Value) -> Result<Config, String> {
        let path = Config::path().ok_or_else(|| crate::t!("err.no_config_folder"))?;
        update_config(&path, |text| {
            let new = Config::edit(text, keys, value)?;
            let cfg = Config::parse_saved(text, &new)?;
            Ok((Some(new), cfg))
        })
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let mut c: Config = toml::from_str(text).map_err(|e| format!("config: {e}"))?;
        if let Ok(doc) = text.parse::<toml::Table>()
            && let Some(keys) = doc.get("keys").and_then(toml::Value::as_table)
        {
            c.unknown_actions = keys.keys().filter(|n| !Action::ALL.iter().any(|a| a.name() == *n)).cloned().collect();
        }
        c.fill_defaults();
        c.keymap()?;
        crate::cloud::follow(&c.search);
        crate::vcs::follow(&c.git);
        Ok(c)
    }

    fn fill_defaults(&mut self) {
        // Setting one image (`images.latex = ...`) must not drop the others' defaults.
        for (tool, image) in PreviewConfig::default().images {
            self.preview.images.entry(tool).or_insert(image);
        }
        // A key the user gave an action is that action's: another action's default loses it.
        let mine: std::collections::HashSet<Key> = self.keys.values().flatten().filter_map(|k| k.parse().ok()).collect();
        for &a in Action::ALL {
            self.keys.entry(a).or_insert_with(|| {
                a.default_keys()
                    .iter()
                    .filter(|k| !k.parse::<Key>().is_ok_and(|k| mine.contains(&k)))
                    .map(|k| k.to_string())
                    .collect()
            });
        }
        for (name, theme) in Theme::builtin() {
            self.themes.entry(name.into()).or_insert(theme);
        }
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("config serializes")
    }

    pub fn keymap(&self) -> Result<HashMap<Key, Action>, String> {
        let mut map = HashMap::new();
        for (&action, keys) in &self.keys {
            for k in keys {
                if let Some(prev) = map.insert(k.parse::<Key>()?, action).filter(|&p| p != action) {
                    return Err(crate::t!("err.key_twice", "key" => k, "a" => prev.name(), "b" => action.name()));
                }
            }
        }
        Ok(map)
    }

    /// First key bound to an action, for display.
    pub fn key_for(&self, action: Action) -> Option<&str> {
        self.keys.get(&action)?.first().map(String::as_str)
    }

    /// The key a short text names for `action`: its Ctrl key when it has one (Find's Ctrl+F
    /// over Alt+F7), else its first; one a terminal delivers when it has one, as both apps'
    /// texts are read in either (Settings' F12 over Ctrl+,).
    pub fn handy_key(&self, action: Action) -> Option<&str> {
        let keys = self.keys.get(&action)?;
        let ctrl = |k: &&String| k.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("ctrl+")) && delivered(k);
        keys.iter().find(ctrl).or_else(|| keys.iter().find(|k| delivered(k))).or(keys.first()).map(String::as_str)
    }

    /// The first key of `action` a terminal delivers, else its first: what the terminal app shows.
    pub fn tty_key(&self, action: Action) -> Option<&str> {
        let keys = self.keys.get(&action)?;
        keys.iter().find(|k| delivered(k)).or(keys.first()).map(String::as_str)
    }

    /// The keys of `action` as texts show them, "F8 / Delete", or "—" with none.
    pub fn keys_shown(&self, action: Action) -> String {
        match self.keys.get(&action).filter(|k| !k.is_empty()) {
            Some(k) => k.iter().map(|k| written(k)).collect::<Vec<_>>().join(" / "),
            None => "—".into(),
        }
    }

    pub fn theme(&self) -> Theme {
        self.themes.get(&self.theme).cloned().unwrap_or_else(Theme::nc)
    }

    pub fn gui_theme(&self, name: Option<&str>) -> Theme {
        self.themes.get(name.unwrap_or(&self.gui.theme)).cloned().unwrap_or_else(Theme::cyber)
    }

    pub fn glyphs(&self) -> Glyphs {
        match (&self.glyph_set, self.glyphs.as_str()) {
            (Some(g), _) => g.clone(),
            (None, "ascii") => Glyphs::ascii(),
            _ => Glyphs::nerd(),
        }
    }

    /// Whether entries get the glyph set's one folder, file and symlink glyph (ASCII, or the
    /// user's own set) instead of a Nerd Font icon per kind of file.
    pub fn plain_glyphs(&self) -> bool {
        self.glyph_set.is_some() || self.glyphs == "ascii"
    }
}

/// What `[keys]` starts with when Settings opens config.toml there and it has none.
const KEYS_EXAMPLE: &str = "# Keys of your own: an action and its keys, in place of its default ones; [] unbinds it.
# Every action with its keys: coxswain --dump-config. Both apps read them when they start.
[keys]
# copy = [\"F5\", \"Ctrl+K\"]
";

/// config.toml ready to be opened where `found` says (a line, the `#` of a commented one taken
/// off): its path and that line (from 1). When no line is found, `example` is written at the
/// end first, the rest kept as it is.
pub fn prepare_at(found: impl Fn(&str) -> bool, example: &str) -> Result<(PathBuf, usize), String> {
    let path = Config::path().ok_or_else(|| crate::t!("err.no_config_folder"))?;
    let line = update_config(&path, |text| {
        let (line, new) = at_line(text, found, example);
        Ok((new, line))
    })?;
    Ok((path, line))
}

/// Change the config.toml at `path`: `edit` gets its text (empty when there is none yet) and
/// gives the new text to write, or `None` to leave it, and what to hand back. One change at a
/// time, in every path that writes it, so two at once never lose one; through a link (a config
/// kept with dotfiles) to the file itself; a file that cannot be read is never written over;
/// and written through a file beside it, so a failed write never leaves half a config.
pub(crate) fn update_config<T>(
    path: &std::path::Path,
    edit: impl FnOnce(&str) -> Result<(Option<String>, T), String>,
) -> Result<T, String> {
    static SAVING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = SAVING.lock().unwrap_or_else(|e| e.into_inner());
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let (new, out) = edit(&text)?;
    if let Some(new) = new {
        crate::fs::replace(&path, new.as_bytes(), false).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(out)
}

/// The line (from 1) of `text` that `found` matches, commented out or not; when there is none,
/// `example` is added at the end and the line is where it matches there, and the new text
/// comes back too.
pub(crate) fn at_line(text: &str, found: impl Fn(&str) -> bool, example: &str) -> (usize, Option<String>) {
    let at = |t: &str| t.lines().position(|l| found(l.trim_start_matches(['#', ' ', '\t'])));
    if let Some(i) = at(text) {
        return (i + 1, None);
    }
    let mut new = text.to_string();
    if !new.is_empty() {
        new += if new.ends_with('\n') { "\n" } else { "\n\n" };
    }
    let line = new.lines().count() + 1 + at(example).unwrap_or(0);
    new += example;
    (line, Some(new))
}

/// `editor` opening `file` at `line`: `+line` for the editors known to take it, else the file.
pub fn edit_command(editor: &str, file: &std::path::Path, line: usize) -> crate::tools::Line {
    let name = editor.split_whitespace().next().and_then(|p| std::path::Path::new(p).file_stem()).map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let plus = ["vi", "vim", "nvim", "gvim", "view", "nano", "pico", "emacs", "emacsclient", "micro", "mg", "joe", "jed", "ne", "kak", "mcedit"].contains(&name.as_str());
    let at = if plus { format!(" +{line}") } else { String::new() };
    let mut l = crate::tools::Line::default();
    l.cmd = format!("{editor}{at} {}", l.word(&file.to_string_lossy()));
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_keys_given_twice_and_as_written() {
        // A key the user gives an action leaves the action it is a default of.
        let cfg = Config::parse("[keys]\npack = [\"F5\"]\n").unwrap();
        assert_eq!(cfg.keymap().unwrap()[&"F5".parse::<Key>().unwrap()], Action::Pack);
        assert!(!cfg.keys[&Action::Copy].iter().any(|k| k == "F5"));
        // One the user gives two actions is an error that names both.
        let e = Config::parse("[keys]\ncopy = [\"F9\"]\npack = [\"f9\"]\n").unwrap_err();
        assert!(e.contains("copy") && e.contains("pack"), "{e}");
        let cfg = Config::parse("[keys]\nsearch = [\"Alt+F7\", \"ctrl+f\"]\ndelete = [\"F8\", \"del\"]\n").unwrap();
        assert_eq!(cfg.handy_key(Action::Search), Some("ctrl+f"));
        // Settings' Ctrl+, never reaches most terminals: texts and the terminal app name F12.
        let d = Config::default();
        assert_eq!((d.handy_key(Action::Settings), d.tty_key(Action::Settings)), (Some("F12"), Some("F12")));
        assert_eq!((d.key_for(Action::Settings), d.tty_key(Action::Search)), (Some("Ctrl+,"), Some("Alt+F7")));
        assert_eq!(written("ctrl+f"), "Ctrl+F");
        assert_eq!(cfg.keys_shown(Action::Delete), "F8 / Delete");
        let none = Config::parse("[keys]\nhelp = []\n").unwrap();
        assert_eq!(none.keys_shown(Action::Help), "—");
    }

    #[test]
    fn config_edit_and_unset_go_into_inline_tables() {
        for text in ["[preview]\nimages = { latex = \"x\" }\n", "preview = { theme = \"y\" }\n"] {
            let new = Config::edit(text, &["preview", "images", "latex"], "z".into()).unwrap();
            let cfg = Config::parse(&new).unwrap();
            assert_eq!(cfg.preview.images["latex"], "z", "{new}");
            let back = Config::unset(&new, &["preview", "images", "latex"]).unwrap();
            assert!(!back.contains("\"z\""), "{back}");
            Config::parse(&back).unwrap();
        }
    }

    #[test]
    fn config_opens_at_its_keys() {
        let keys = |l: &str| l.starts_with("[keys]");
        assert_eq!(at_line("a = 1\n[keys]\ncopy = [\"F5\"]\n", keys, KEYS_EXAMPLE), (2, None));
        assert_eq!(at_line("# [keys]\n", keys, KEYS_EXAMPLE), (1, None), "a commented-out table is found too");
        let (line, new) = at_line("a = 1", keys, KEYS_EXAMPLE);
        let new = new.unwrap();
        assert!(new.starts_with("a = 1\n\n# Keys"), "{new}");
        assert_eq!(new.lines().nth(line - 1), Some("[keys]"));
        Config::parse(&new).expect("still a config that is read");
        assert_eq!(edit_command("nvim", std::path::Path::new("/c/config.toml"), 7).cmd, "nvim +7 /c/config.toml");
        assert_eq!(edit_command("/usr/bin/nvim -p", std::path::Path::new("/c/config.toml"), 7).cmd, "/usr/bin/nvim -p +7 /c/config.toml");
        assert_eq!(edit_command("code --wait", std::path::Path::new("/c/config.toml"), 7).cmd, "code --wait /c/config.toml");
    }

    #[test]
    fn every_action_is_in_a_named_group() {
        // The table makes a group compulsory; this checks each one is used and has a heading.
        for &g in Group::ALL {
            assert!(g.actions().next().is_some(), "{g:?} is empty");
            assert!(!g.label().starts_with("group."), "{g:?} has no heading");
        }
        assert_eq!(Group::ALL.iter().map(|g| g.actions().count()).sum::<usize>(), Action::ALL.len());
    }

    #[test]
    fn preview_images_keep_defaults() {
        let c = Config::parse("[preview]\nprefer = \"container\"\nimages.latex = \"texlive:medium\"\n").unwrap();
        assert_eq!(c.preview.prefer, "container");
        assert_eq!(c.preview.images["latex"], "texlive:medium");
        assert_eq!(c.preview.images["plantuml"], "docker.io/plantuml/plantuml:latest");
        assert_eq!(Config::default().preview.timeout, 120);
    }

    #[test]
    fn config_key_roundtrip() {
        for s in ["F5", "Ctrl+R", "Alt+F7", "Shift+F6", "Ctrl+PageUp", "Space", "+", "Alt+.", "Tab", "A"] {
            let k: Key = s.parse().unwrap();
            assert_eq!(k.to_string().parse::<Key>().unwrap(), k, "{s}");
        }
        assert_eq!("a".parse::<Key>().unwrap().to_string(), "a");
        assert_eq!("Shift+a".parse::<Key>().unwrap(), "A".parse::<Key>().unwrap());
        assert_eq!("ctrl+alt+x".parse::<Key>().unwrap(), Key::new(KeyCode::Char('x'), true, true, false));
        assert_eq!("Ctrl+F".parse::<Key>().unwrap(), Key::new(KeyCode::Char('f'), true, false, false));
        assert!("Ctrl+Shift+F".parse::<Key>().unwrap().shift);
        assert!("Ctrl+Nope".parse::<Key>().is_err());
    }

    #[test]
    fn config_every_theme_is_in_one_group() {
        let reference = crate::i18n::catalogue(crate::i18n::REFERENCE);
        // Every `theme.<name>` text is a built-in theme, listed once.
        let mut named: Vec<&str> = reference.keys().filter_map(|k| k.strip_prefix("theme.")).filter(|k| !k.starts_with("group.")).collect();
        let mut listed: Vec<&str> = Theme::BUILTIN.iter().map(|(n, ..)| *n).collect();
        named.sort_unstable();
        listed.sort_unstable();
        assert_eq!(named, listed);
        let mut seen: Vec<&str> = vec![];
        for &(name, group, _) in Theme::BUILTIN {
            assert!(reference.contains_key(group), "{name}: no text for {group}");
            // A group's themes stand together.
            if seen.last() != Some(&group) {
                assert!(!seen.contains(&group), "{name} is apart from its group");
                seen.push(group);
            }
            assert_eq!(Theme::group(name), group);
        }
        assert!(reference.contains_key("theme.group.own"));
        assert_eq!(Theme::group("mine"), "theme.group.own");
    }

    #[test]
    fn config_defaults_are_norton() {
        let c = Config::default();
        let km = c.keymap().unwrap();
        assert_eq!(km[&"F5".parse().unwrap()], Action::Copy);
        assert_eq!(km[&"F10".parse().unwrap()], Action::Quit);
        assert_eq!(km[&"Alt+F7".parse().unwrap()], Action::Search);
        assert_eq!(c.theme().look, "dos");
        assert_eq!(c.gui_theme(None).look, "crt");
        // The tube comes with the CRT look, and only there; `crt = true` gives it to any theme.
        assert!(c.gui_theme(None).crt && !c.theme().crt);
        assert!(Theme::builtin().iter().all(|(_, t)| t.crt == (t.look == "crt")));
        let own = Config::parse("[themes.mine]\nlook = \"win95\"\ncrt = true\n").unwrap();
        assert!(own.themes["mine"].crt && own.themes["mine"].look == "win95");
        assert!(c.to_toml().contains("crt = true") && !Config::parse("[themes.x]\nlook = \"modern\"\n").unwrap().to_toml().contains("[themes.x]\ncrt"));
        // Exact RGB everywhere: named colors would follow the terminal's own palette.
        for (_, t) in Theme::builtin() {
            assert!(t.slots().iter().all(|(_, s)| [&s.fg, &s.bg].iter().all(|c| c.is_empty() || c.starts_with('#'))));
        }
    }

    #[test]
    fn config_user_overrides_merge() {
        let c = Config::parse(
            "theme = \"mine\"\n[keys]\nquit = [\"Ctrl+Q\"]\n[themes.mine.panel]\nfg = \"#ffffff\"\n",
        )
        .unwrap();
        let km = c.keymap().unwrap();
        assert_eq!(km[&"Ctrl+Q".parse().unwrap()], Action::Quit);
        assert!(!km.contains_key(&"F10".parse().unwrap()));
        assert_eq!(km[&"F5".parse().unwrap()], Action::Copy);
        // Unset slots inherit the NC scheme.
        assert_eq!(c.theme().panel.fg, "#ffffff");
        assert_eq!(c.theme().cursor, Theme::nc().cursor);
        assert!(Config::parse("[keys]\nquit = [\"Hyper+Q\"]").is_err());
        // A config.toml of a newer version, with an action this one does not know: it starts,
        // with the rest of the settings.
        let newer = Config::parse("theme = \"mc\"\n[keys]\nlater_action = [\"Alt+Shift+K\"]\nquit = [\"Ctrl+Q\"]\n[search]\nmax_results = 7\n").unwrap();
        assert_eq!((newer.theme.as_str(), newer.search.max_results), ("mc", 7));
        assert_eq!(newer.keymap().unwrap()[&"Ctrl+Q".parse().unwrap()], Action::Quit);
        assert_eq!(newer.unknown_actions, ["later_action"]);
        // A key bound twice: the helper still reads with the user's search settings.
        let twice = "[keys]\nquit = [\"F5\"]\ncopy = [\"F5\"]\n[search]\narchives = false\ntext_exclude = [\"secret\"]\n";
        assert!(Config::parse(twice).is_err());
        let c = Config::lenient(twice);
        assert_eq!((c.search.archives, c.search.text_exclude.as_slice()), (false, ["secret".to_string()].as_slice()));
        // The first binding stays, the second is left out and named.
        assert_eq!(c.keymap().unwrap()[&"F5".parse().unwrap()], Action::Quit);
        assert_eq!(c.refused.len(), 1);
        assert_eq!(c.refused[0].line, 3);
    }

    /// A config.toml with mistakes: each costs only its own setting, and is named with its line.
    #[test]
    fn config_lenient_keeps_what_it_can_read() {
        crate::i18n::set_language("en-GB");
        let lines = |c: &Config| c.refused.iter().map(|r| r.line).collect::<Vec<_>>();
        // A wrong type, at the top and in a table.
        let c = Config::lenient("show_hidden = false\ntheme = 5\n[search]\nmax_results = \"many\"\narchives = false\n");
        assert_eq!((c.show_hidden, c.theme.as_str(), c.search.max_results, c.search.archives), (false, "nc", SearchConfig::default().max_results, false));
        assert_eq!(lines(&c), [2, 4]);
        assert_eq!(c.refused[0].problem, "`theme` must be text");
        assert_eq!(c.refused[1].problem, "`search.max_results` must be a number");
        // Bad TOML: the line; a broken table header: its table.
        let c = Config::lenient("show_hidden = false\nconfirm_delete = \n[search\narchives = false\n[gui]\nfont_size = 20.0\n");
        assert_eq!((c.show_hidden, c.confirm_delete, c.search.archives, c.gui.font_size), (false, true, true, 20.0));
        assert_eq!(lines(&c), [2, 3]);
        assert!(c.refused[0].problem.starts_with("not valid TOML: "), "{:?}", c.refused);
        // An unknown key name, and a value spread over lines.
        let c = Config::lenient("[keys]\nquit = [\"Ctlr+Q\"]\nhelp = [\"F2\"]\n[search]\ntext_exclude = [\n  \"/a\",\n  5,\n]\nmax_results = 9\n");
        assert_eq!(c.keymap().unwrap()[&"F2".parse().unwrap()], Action::Help);
        assert_eq!((c.search.text_exclude.clone(), c.search.max_results), (SearchConfig::default().text_exclude, 9));
        assert_eq!(lines(&c), [2, 5]);
        assert_eq!(c.refused[0].problem, "`keys.quit`: unknown key 'Ctlr+Q'");
        // A wrong type inside an inline table.
        let c = Config::lenient("hints = false\npreview = { prefer = 5 }\n");
        assert!(!c.hints);
        assert_eq!(lines(&c), [2]);
        assert_eq!(c.refused[0].problem, "`preview.prefer` must be text");
        // A file without mistakes has none.
        assert!(Config::lenient("theme = \"mc\"\n").refused.is_empty());
        // Settings save while the old mistake stands, not when they would add one.
        let old = "theme = 5\n";
        assert!(Config::parse_saved(old, "theme = 5\nhints = false\n").is_ok_and(|c| !c.hints));
        assert!(Config::parse_saved(old, "theme = 5\nhints = 3\n").is_err());
    }

    #[test]
    fn config_user_command_expand() {
        let u = UserCommand { key: "x".into(), label: "x".into(), command: "vim %f %s 100%%".into(), wait: false, when: None };
        let dir = std::path::Path::new("/a b");
        let out = u.expand(dir, Some(&dir.join("it's.txt")), &[]);
        if cfg!(unix) {
            assert_eq!(out.cmd, "vim 'it'\\''s.txt' 'it'\\''s.txt' 100%");
        }
        // A name that starts with `-` is no option to the program.
        let out = u.expand(dir, Some(&dir.join("-rf")), &[]);
        let sep = std::path::MAIN_SEPARATOR;
        // (Windows quotes it, for the `\`.)
        let shown = out.shown().replace('"', "");
        assert_eq!(shown, format!("vim .{sep}-rf .{sep}-rf 100%"));
    }

    /// The line runs in sh whatever `$SHELL` is: in fish `\'` inside single quotes would end
    /// them, and these names would run `echo INJECTED`.
    #[cfg(unix)]
    #[test]
    fn config_user_command_names_stay_names() {
        let dir = std::env::temp_dir();
        let run = |command: &str, file: &str, sel: &[PathBuf]| {
            let u = UserCommand { key: "x".into(), label: "x".into(), command: command.into(), wait: false, when: None };
            let mut c = u.expand(&dir, Some(&dir.join(file)), sel).command();
            assert_eq!(c.get_program(), "sh");
            let out = c.env("SHELL", "fish").output().unwrap();
            String::from_utf8(out.stdout).unwrap()
        };
        let evil = "x\\';echo INJECTED;#";
        assert_eq!(run("printf '%%s\\n' %f", evil, &[]), format!("{evil}\n"));
        let a = dir.join("a\\");
        let b = dir.join(" ;echo INJECTED;#");
        let got = run("printf '%%s\\n' %s", evil, &[a.clone(), b.clone()]);
        assert_eq!(got, format!("{}\n{}\n", a.display(), b.display()));
        assert!(!got.lines().any(|l| l == "INJECTED"), "{got}");
    }

    /// A name pasted into the command line reaches fish as it is, and sh still gets `quote`'s.
    #[cfg(unix)]
    #[test]
    fn config_quote_for_fish() {
        let evil = "x\\';echo INJECTED;#";
        assert_eq!(quote_for("/usr/bin/fish", evil), "'x\\\\\\';echo INJECTED;#'");
        assert_eq!(quote_for("/bin/zsh", evil), quote(evil));
        assert_eq!(quote_for("/usr/bin/fish", "plain.txt"), "plain.txt");
        let Some(fish) = crate::tools::which("fish") else { return };
        let home = std::env::temp_dir().join("coxswain-quote-fish");
        for name in [evil, "a\\", "it's \\\\ here", " ;echo INJECTED;#"] {
            let out = std::process::Command::new(&fish)
                .args(["--no-config", "-c", &format!("printf '%s\\n' {}", quote_for("fish", name))])
                .env("HOME", &home)
                .env("XDG_CONFIG_HOME", &home)
                .env("XDG_DATA_HOME", &home)
                .env("XDG_CACHE_HOME", &home)
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{name}\n"), "{name}");
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    /// cmd.exe gets the line as it is (Rust would write `"` as `\"`), and a `%PATH%` in a
    /// name is not expanded.
    #[cfg(windows)]
    #[test]
    fn config_user_command_names_stay_names() {
        let dir = std::env::temp_dir();
        let u = UserCommand { key: "x".into(), label: "x".into(), command: "echo \"a b\" %f".into(), wait: false, when: None };
        let out = u.expand(&dir, Some(&dir.join("%PATH%x & echo INJECTED.txt")), &[]).command().output().unwrap();
        let got = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert_eq!(got, "\"a b\" \"%PATH%x & echo INJECTED.txt\"");
    }

    #[test]
    fn config_dump_parses_back() {
        let text = Config::default().to_toml();
        let c = Config::parse(&text).unwrap();
        assert_eq!(c.keys, Config::default().keys);
        assert_eq!(color_to_rgb("#0a0b0c"), Some((10, 11, 12)));
        assert_eq!(color_to_rgb("lightcyan"), Some((85, 255, 255)));
    }
}

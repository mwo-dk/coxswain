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

// ---------------------------------------------------------------- actions

macro_rules! actions {
    ($($variant:ident = $name:literal, $label:literal, [$($key:literal),*];)*) => {
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
        }
    };
}

// Norton Commander defaults. Plain printable keys (+ - *) only fire while the command line
// is empty, as in NC.
actions! {
    Help = "help", "Help", ["F1"];
    UserMenu = "user_menu", "Menu", ["F2"];
    View = "view", "View", ["F3"];
    Edit = "edit", "Edit", ["F4"];
    Copy = "copy", "Copy", ["F5"];
    Move = "move", "RenMov", ["F6"];
    Mkdir = "mkdir", "Mkdir", ["F7"];
    Delete = "delete", "Delete", ["F8", "Delete"];
    DeleteForever = "delete_forever", "Delete permanently", ["Shift+F8", "Shift+Delete"];
    Menu = "menu", "PullDn", ["F9"];
    Quit = "quit", "Quit", ["F10"];
    Up = "up", "Up", ["Up"];
    Down = "down", "Down", ["Down"];
    PageUp = "page_up", "Page up", ["PageUp", "Left"];
    PageDown = "page_down", "Page down", ["PageDown", "Right"];
    Home = "home", "First", ["Home"];
    End = "end", "Last", ["End"];
    Open = "open", "Open", ["Enter"];
    Parent = "parent", "Parent dir", ["Ctrl+PageUp", "Backspace"];
    SwitchPanel = "switch_panel", "Other panel", ["Tab"];
    Mark = "mark", "Mark", ["Insert", "Shift+Down"];
    SelectGroup = "select_group", "Select group", ["+"];
    UnselectGroup = "unselect_group", "Unselect group", ["-"];
    InvertSelection = "invert_selection", "Invert selection", ["*"];
    MarkAll = "mark_all", "Mark all", ["Ctrl+A"];
    Search = "search", "Find file", ["Alt+F7", "Ctrl+F"];
    SearchText = "search_text", "Search inside files", ["Shift+F7", "Ctrl+Shift+F"];
    Ask = "ask", "Ask your files", ["Ctrl+F7"];
    Refresh = "refresh", "Reread", ["Ctrl+R"];
    SwapPanels = "swap_panels", "Swap panels", ["Ctrl+U"];
    TogglePanels = "toggle_panels", "Panels on/off", ["Ctrl+O"];
    ToggleHidden = "toggle_hidden", "Hidden files", ["Alt+."];
    GotoLeft = "goto_left", "Left: go to", ["Alt+F1"];
    GotoRight = "goto_right", "Right: go to", ["Alt+F2"];
    SameDir = "same_dir", "Other panel here", ["Alt+O"];
    SortName = "sort_name", "Sort by name", ["Ctrl+F3"];
    SortExt = "sort_ext", "Sort by extension", ["Ctrl+F4"];
    SortTime = "sort_time", "Sort by time", ["Ctrl+F5"];
    SortSize = "sort_size", "Sort by size", ["Ctrl+F6"];
    CopyPath = "copy_path", "Path to command line", ["Ctrl+Enter", "Ctrl+J"];
    NewTab = "new_tab", "New tab", ["Ctrl+T"];
    CloseTab = "close_tab", "Close tab", ["Ctrl+W"];
    NextTab = "next_tab", "Next tab", ["Ctrl+Tab"];
    PrevTab = "prev_tab", "Previous tab", ["Ctrl+Shift+Tab"];
    TogglePreview = "toggle_preview", "Preview", ["Space"];
    ToggleView = "toggle_view", "Details/columns/thumbnails", ["Alt+V"];
    ToggleSidebar = "toggle_sidebar", "Sidebar", ["Ctrl+B"];
    EditPath = "edit_path", "Edit path", ["Ctrl+L"];
    // No key of its own: sizes appear by themselves (`folder_sizes`), and Ctrl+Space belongs
    // to the system on a Mac.
    DirSizes = "dir_sizes", "Folder sizes", [];
    BatchRename = "batch_rename", "Batch rename", ["Ctrl+M"];
    Tag = "tag", "Colour tag", ["Alt+T"];
    Notes = "notes", "Folder notes", ["Alt+N"];
    Back = "back", "Back", ["Alt+Left"];
    Forward = "forward", "Forward", ["Alt+Right"];
    ClipCopy = "clip_copy", "Copy to clipboard", ["Ctrl+C"];
    ClipCut = "clip_cut", "Cut to clipboard", ["Ctrl+X"];
    Paste = "paste", "Paste", ["Ctrl+V"];
    Properties = "properties", "Properties", ["Alt+Enter"];
    Extract = "extract", "Extract archive", ["Ctrl+E"];
    Pack = "pack", "Pack into an archive", ["Alt+F5"];
    Columns = "columns", "Columns and folder sizes", [];
    Duplicates = "duplicates", "Find duplicates", ["Ctrl+D"];
    Settings = "settings", "Settings", ["Ctrl+,"];
    History = "history", "Git history", ["Ctrl+G"];
}

impl Action {
    /// Actions only the GUI implements.
    pub fn gui_only(self) -> bool {
        use Action::*;
        matches!(
            self,
            NewTab | CloseTab | NextTab | PrevTab | TogglePreview | ToggleView | ToggleSidebar | EditPath | BatchRename | Tag | Notes | Back | Forward
                | ClipCopy | ClipCut | Paste | Properties | Columns | Duplicates | Settings
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
            /// The desktop app's shapes and chrome: `modern`, `crt`, `dos`, `win31`, `win95`,
            /// `winxp`, `win7`, `win10`, `win11`, `system7`, `platinum`, `aqua` or `macos`.
            /// Colors come from the slots; the terminal app ignores it.
            pub look: String,
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

impl Theme {
    /// Norton Commander 5: cyan on blue, black-on-cyan cursor, yellow marks. Exact CGA RGB, so
    /// the terminal's own palette (often a pale "blue") does not wash it out.
    pub fn nc() -> Self {
        Theme {
            look: "dos".into(),
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

    /// Every built-in theme: (name, theme).
    pub fn builtin() -> Vec<(&'static str, Theme)> {
        vec![
            ("cyber", Theme::cyber()),
            ("dark", Theme::dark()),
            ("light", Theme::light()),
            ("nord", Theme::nord()),
            ("midnight", Theme::midnight()),
            ("nc", Theme::nc()),
            ("win31", Theme::win31()),
            ("win95", Theme::win95()),
            ("winxp", Theme::winxp()),
            ("win7", Theme::win7()),
            ("win10", Theme::win10()),
            ("win11", Theme::win11()),
            ("win11-dark", Theme::win11_dark()),
            ("system7", Theme::system7()),
            ("platinum", Theme::platinum()),
            ("aqua", Theme::aqua()),
            ("macos", Theme::macos()),
            ("macos-dark", Theme::macos_dark()),
        ]
    }

    pub fn from_palette(p: &Palette) -> Self {
        Theme {
            look: p.look.into(),
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
            accent: st(p.bg, p.accent),
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
}

impl UserCommand {
    /// Substitute `%f`, `%d`, `%s` (shell-quoted) and `%%`.
    pub fn expand(&self, dir: &std::path::Path, file: Option<&std::path::Path>, selected: &[PathBuf]) -> String {
        let q = |p: &std::path::Path| quote(&p.to_string_lossy());
        let file_q = file.map(|f| q(f.file_name().map(std::path::Path::new).unwrap_or(f))).unwrap_or_default();
        let sel = if selected.is_empty() { file_q.clone() } else { selected.iter().map(|p| q(p)).collect::<Vec<_>>().join(" ") };
        let mut out = String::new();
        let mut it = self.command.chars().peekable();
        while let Some(c) = it.next() {
            match (c, it.peek()) {
                ('%', Some('f')) => out += &file_q,
                ('%', Some('d')) => out += &q(dir),
                ('%', Some('s')) => out += &sel,
                ('%', Some('%')) => out.push('%'),
                _ => {
                    out.push(c);
                    continue;
                }
            }
            it.next();
        }
        out
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

/// Previews made by external tools (desktop app): LaTeX, LibreOffice, PlantUML, pandoc and
/// DuckDB. Each runs from a locally installed tool or from a container image.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PreviewConfig {
    /// "auto" (an installed tool first, else a container), "local" or "container".
    pub prefer: String,
    /// Per tool, overrides `prefer`: `prefer_tool = { latex = "container" }`.
    pub prefer_tool: BTreeMap<String, String>,
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
}

impl Default for PreviewConfig {
    fn default() -> Self {
        let images = [
            ("latex", "docker.io/texlive/texlive:latest"),
            ("plantuml", "docker.io/plantuml/plantuml:latest"),
            ("pandoc", "docker.io/pandoc/core:latest"),
            // No official images for these; set one you trust (see docs/previews/containers.md).
            ("libreoffice", ""),
            ("duckdb", ""),
        ];
        PreviewConfig {
            prefer: "auto".into(),
            prefer_tool: BTreeMap::new(),
            container: "auto".into(),
            images: images.map(|(k, v)| (k.to_string(), v.to_string())).into_iter().collect(),
            timeout: 120,
            latex_auto: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    /// Roots to index. Empty = `/` (Unix) or every fixed drive (Windows).
    pub roots: Vec<PathBuf>,
    /// Skipped while indexing: a path (`/proc`) skips that tree, a bare name (`node_modules`)
    /// skips every directory with that name.
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
    /// Which model makes the vectors: "builtin" (downloaded, runs on the CPU here), "ollama"
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
    /// Ask: the chat model that answers questions from the closest passages, on the server
    /// above (Ollama on this machine when the vectors are the built-in model's), e.g.
    /// `qwen3:8b`. Empty: Ask is not set up.
    pub ask_model: String,
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
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            roots: vec![],
            exclude: ["/proc", "/sys", "/dev", "/run", "/tmp/.X11-unix"].map(String::from).to_vec(),
            max_results: 10_000,
            watch: true,
            text: true,
            text_roots: vec![],
            text_exclude: ["node_modules", "target", "build", "dist", "out", "vendor", "__pycache__", "Trash"].map(String::from).to_vec(),
            names_only: vec![],
            meaning: false,
            meaning_engine: "builtin".into(),
            meaning_url: String::new(),
            meaning_model: String::new(),
            meaning_key_env: String::new(),
            ask_model: String::new(),
            text_max_size: 20 * 1024 * 1024,
            archives: true,
            archives_everywhere: false,
            history: true,
            cloud: "local-only".into(),
            cloud_read: vec![],
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
    pub confirm_delete: bool,
    /// Look for a newer release on GitHub at startup (at most once a day).
    pub check_updates: bool,
    /// Action -> keys. Listing an action replaces its default keys; `[]` unbinds it.
    pub keys: BTreeMap<Action, Vec<String>>,
    pub themes: BTreeMap<String, Theme>,
    pub user_menu: Vec<UserCommand>,
    pub search: SearchConfig,
    pub preview: PreviewConfig,
    pub gui: GuiConfig,
    pub git: GitConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfig {
    /// Show when each file and folder was last committed, and by whom: a column in the desktop
    /// app's details view and the preview, the info line in the terminal app.
    pub last_commit: bool,
}

impl Default for GitConfig {
    fn default() -> Self {
        GitConfig { last_commit: true }
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
            confirm_delete: true,
            check_updates: true,
            keys: BTreeMap::new(),
            themes: BTreeMap::new(),
            user_menu: vec![
                UserCommand { key: "s".into(), label: "git status".into(), command: "git status".into(), wait: true },
                UserCommand { key: "l".into(), label: "git log".into(), command: "git log --oneline --graph --decorate -50".into(), wait: true },
                UserCommand { key: "d".into(), label: "git diff (file)".into(), command: "git diff -- %f".into(), wait: true },
                UserCommand { key: "b".into(), label: "git blame (file)".into(), command: "git blame -- %f | less".into(), wait: false },
            ],
            search: SearchConfig::default(),
            preview: PreviewConfig::default(),
            gui: GuiConfig::default(),
            git: GitConfig::default(),
        };
        c.fill_defaults();
        c
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("coxswain").join("config.toml"))
    }

    /// Where Coxswain keeps things, (what, path), for `--paths`.
    pub fn paths() -> Vec<(&'static str, Option<PathBuf>)> {
        vec![
            ("config", Config::path()),
            ("state", crate::state::AppState::path()),
            ("cache", crate::helper::folder()),
            ("name index", crate::index::Index::cache_path()),
            ("search store", crate::store::Store::path()),
            ("model", crate::meaning::folder()),
            ("previews", dirs::cache_dir().map(|d| d.join("coxswain").join("previews"))),
            ("archive looks", dirs::cache_dir().map(|d| d.join("coxswain").join("peek"))),
        ]
    }

    /// Load the user config, falling back to defaults when the file does not exist.
    pub fn load() -> Result<Config, String> {
        match Config::path().map(std::fs::read_to_string) {
            Some(Ok(text)) => Config::parse(&text),
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("config: {e}")),
            _ => Ok(Config::default()),
        }
    }

    /// `text` (a config.toml) with the value at `keys` (`["search", "meaning"]`) set, its
    /// comments and layout kept.
    pub fn edit(text: &str, keys: &[&str], value: toml_edit::Value) -> Result<String, String> {
        let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("config: {e}"))?;
        let (last, parents) = keys.split_last().ok_or("config: no key")?;
        let mut table = doc.as_table_mut();
        for k in parents {
            let item = table.entry(k).or_insert_with(|| {
                let mut t = toml_edit::Table::new();
                t.set_implicit(true);
                toml_edit::Item::Table(t)
            });
            table = item.as_table_mut().ok_or_else(|| crate::t!("err.not_a_table", "key" => k))?;
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

    /// Set one value in the user's config.toml and write it; only what still parses is written.
    pub fn save_value(keys: &[&str], value: toml_edit::Value) -> Result<Config, String> {
        let path = Config::path().ok_or_else(|| crate::t!("err.no_config_folder"))?;
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let new = Config::edit(&text, keys, value)?;
        let cfg = Config::parse(&new)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, new).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(cfg)
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let mut c: Config = toml::from_str(text).map_err(|e| format!("config: {e}"))?;
        c.fill_defaults();
        c.keymap()?;
        crate::cloud::follow(&c.search);
        Ok(c)
    }

    fn fill_defaults(&mut self) {
        // Setting one image (`images.latex = ...`) must not drop the others' defaults.
        for (tool, image) in PreviewConfig::default().images {
            self.preview.images.entry(tool).or_insert(image);
        }
        for &a in Action::ALL {
            self.keys.entry(a).or_insert_with(|| a.default_keys().iter().map(|k| k.to_string()).collect());
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
                map.insert(k.parse::<Key>()?, action);
            }
        }
        Ok(map)
    }

    /// First key bound to an action, for display.
    pub fn key_for(&self, action: Action) -> Option<&str> {
        self.keys.get(&action)?.first().map(String::as_str)
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn config_defaults_are_norton() {
        let c = Config::default();
        let km = c.keymap().unwrap();
        assert_eq!(km[&"F5".parse().unwrap()], Action::Copy);
        assert_eq!(km[&"F10".parse().unwrap()], Action::Quit);
        assert_eq!(km[&"Alt+F7".parse().unwrap()], Action::Search);
        assert_eq!(c.theme().look, "dos");
        assert_eq!(c.gui_theme(None).look, "crt");
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
    }

    #[test]
    fn config_user_command_expand() {
        let u = UserCommand { key: "x".into(), label: "x".into(), command: "vim %f %s 100%%".into(), wait: false };
        let dir = std::path::Path::new("/a b");
        let out = u.expand(dir, Some(&dir.join("it's.txt")), &[]);
        if cfg!(unix) {
            assert_eq!(out, "vim 'it'\\''s.txt' 'it'\\''s.txt' 100%");
        }
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

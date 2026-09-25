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
            pub fn label(self) -> &'static str { match self { $(Action::$variant => $label),* } }
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
    Search = "search", "Find file", ["Alt+F7", "Ctrl+F"];
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
        pub struct Theme { $(pub $slot: Style),* }

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
    search_hit,
);

impl Default for Theme {
    fn default() -> Self {
        Theme::nc()
    }
}

impl Theme {
    /// Norton Commander 5: cyan on blue, black-on-cyan cursor, yellow marks.
    pub fn nc() -> Self {
        Theme {
            panel: st("lightcyan", "blue"),
            border: st("lightcyan", "blue"),
            header: bold(st("yellow", "blue")),
            directory: bold(st("white", "blue")),
            executable: st("lightgreen", "blue"),
            hidden: st("cyan", "blue"),
            symlink: st("lightmagenta", "blue"),
            cursor: st("black", "cyan"),
            marked: bold(st("yellow", "blue")),
            marked_cursor: bold(st("yellow", "cyan")),
            status: st("lightcyan", "blue"),
            keybar_num: st("white", "black"),
            keybar_label: st("black", "cyan"),
            cmdline: st("gray", "black"),
            dialog: st("black", "gray"),
            dialog_border: st("white", "gray"),
            dialog_input: st("black", "cyan"),
            git_branch: bold(st("lightmagenta", "blue")),
            git_modified: st("yellow", "blue"),
            git_added: st("lightgreen", "blue"),
            git_untracked: st("lightred", "blue"),
            git_deleted: st("red", "blue"),
            git_renamed: st("lightblue", "blue"),
            git_conflict: bold(st("lightred", "blue")),
            git_ignored: st("darkgray", "blue"),
            search_hit: bold(st("yellow", "")),
        }
    }

    /// A modern dark scheme (Tokyo Night-ish).
    pub fn midnight() -> Self {
        let (bg, fg, dim, alt) = ("#1a1b26", "#c0caf5", "#565f89", "#24283b");
        Theme {
            panel: st(fg, bg),
            border: st("#3b4261", bg),
            header: bold(st("#7aa2f7", bg)),
            directory: bold(st("#7aa2f7", bg)),
            executable: st("#9ece6a", bg),
            hidden: st(dim, bg),
            symlink: st("#bb9af7", bg),
            cursor: st(bg, "#7aa2f7"),
            marked: bold(st("#e0af68", bg)),
            marked_cursor: bold(st("#e0af68", "#3d59a1")),
            status: st(dim, bg),
            keybar_num: st(fg, bg),
            keybar_label: st(bg, "#414868"),
            cmdline: st(fg, bg),
            dialog: st(fg, alt),
            dialog_border: st("#7aa2f7", alt),
            dialog_input: st(fg, "#414868"),
            git_branch: bold(st("#bb9af7", bg)),
            git_modified: st("#e0af68", bg),
            git_added: st("#9ece6a", bg),
            git_untracked: st("#f7768e", bg),
            git_deleted: st("#db4b4b", bg),
            git_renamed: st("#2ac3de", bg),
            git_conflict: bold(st("#ff007c", bg)),
            git_ignored: st("#414868", bg),
            search_hit: bold(st("#ff9e64", "")),
        }
    }
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
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            roots: vec![],
            exclude: ["/proc", "/sys", "/dev", "/run", "/tmp/.X11-unix"].map(String::from).to_vec(),
            max_results: 10_000,
            watch: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Name of a built-in (`nc`, `midnight`) or a `[themes.<name>]` table.
    pub theme: String,
    /// `nerd` or `ascii`, or a full `[glyph_set]` table.
    pub glyphs: String,
    pub glyph_set: Option<Glyphs>,
    pub show_hidden: bool,
    /// Overrides `$EDITOR` / `$PAGER`.
    pub editor: Option<String>,
    pub viewer: Option<String>,
    pub confirm_delete: bool,
    /// Action -> keys. Listing an action replaces its default keys; `[]` unbinds it.
    pub keys: BTreeMap<Action, Vec<String>>,
    pub themes: BTreeMap<String, Theme>,
    pub user_menu: Vec<UserCommand>,
    pub search: SearchConfig,
}

impl Default for Config {
    fn default() -> Self {
        let mut c = Config {
            theme: "nc".into(),
            glyphs: "nerd".into(),
            glyph_set: None,
            show_hidden: true,
            editor: None,
            viewer: None,
            confirm_delete: true,
            keys: BTreeMap::new(),
            themes: BTreeMap::new(),
            user_menu: vec![
                UserCommand { key: "s".into(), label: "git status".into(), command: "git status".into(), wait: true },
                UserCommand { key: "l".into(), label: "git log".into(), command: "git log --oneline --graph --decorate -50".into(), wait: true },
                UserCommand { key: "d".into(), label: "git diff (file)".into(), command: "git diff -- %f".into(), wait: true },
                UserCommand { key: "b".into(), label: "git blame (file)".into(), command: "git blame -- %f | less".into(), wait: false },
            ],
            search: SearchConfig::default(),
        };
        c.fill_defaults();
        c
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("coxswain").join("config.toml"))
    }

    /// Load the user config, falling back to defaults when the file does not exist.
    pub fn load() -> Result<Config, String> {
        match Config::path().map(std::fs::read_to_string) {
            Some(Ok(text)) => Config::parse(&text),
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("config: {e}")),
            _ => Ok(Config::default()),
        }
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let mut c: Config = toml::from_str(text).map_err(|e| format!("config: {e}"))?;
        c.fill_defaults();
        c.keymap()?;
        Ok(c)
    }

    fn fill_defaults(&mut self) {
        for &a in Action::ALL {
            self.keys.entry(a).or_insert_with(|| a.default_keys().iter().map(|k| k.to_string()).collect());
        }
        self.themes.entry("nc".into()).or_insert_with(Theme::nc);
        self.themes.entry("midnight".into()).or_insert_with(Theme::midnight);
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

    pub fn glyphs(&self) -> Glyphs {
        match (&self.glyph_set, self.glyphs.as_str()) {
            (Some(g), _) => g.clone(),
            (None, "ascii") => Glyphs::ascii(),
            _ => Glyphs::nerd(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(c.theme().panel.bg, "blue");
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

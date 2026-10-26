//! *Open a terminal here*: the terminals installed on this system, the one `terminal` in
//! config.toml names (or a command of one's own, `{dir}` the folder), and how each is told to
//! open in a folder, and to run a program there (the git tool's handoff). Nothing is started
//! here; F2's built-in entry and the handoff start what this says.

use crate::config::quote;
use std::path::Path;

/// A terminal Coxswain knows: its name in config.toml, the name Settings shows, and where it
/// is found (a program on PATH; on a Mac also an app in /Applications or ~/Applications).
pub struct Term {
    pub name: &'static str,
    pub label: &'static str,
    /// A Mac app's name, as `open -a` takes it.
    app: Option<&'static str>,
    /// The program on PATH.
    bin: Option<&'static str>,
}

const fn term(name: &'static str, label: &'static str, app: Option<&'static str>, bin: Option<&'static str>) -> Term {
    Term { name, label, app, bin }
}

const MAC: &[Term] = &[
    term("terminal", "Terminal", Some("Terminal"), None),
    term("iterm2", "iTerm2", Some("iTerm"), None),
    term("ghostty", "Ghostty", Some("Ghostty"), Some("ghostty")),
    term("alacritty", "Alacritty", Some("Alacritty"), Some("alacritty")),
    term("kitty", "kitty", Some("kitty"), Some("kitty")),
    term("wezterm", "WezTerm", Some("WezTerm"), Some("wezterm")),
];

const WINDOWS: &[Term] = &[
    term("wt", "Windows Terminal", None, Some("wt")),
    term("pwsh", "PowerShell 7", None, Some("pwsh")),
    term("powershell", "Windows PowerShell", None, Some("powershell")),
    term("cmd", "cmd", None, Some("cmd")),
    term("alacritty", "Alacritty", None, Some("alacritty")),
    term("wezterm", "WezTerm", None, Some("wezterm")),
];

/// The BSDs, illumos and Linux: after `$TERMINAL`, in this order.
const UNIX: &[Term] = &[
    term("x-terminal-emulator", "x-terminal-emulator", None, Some("x-terminal-emulator")),
    term("foot", "foot", None, Some("foot")),
    term("alacritty", "Alacritty", None, Some("alacritty")),
    term("kitty", "kitty", None, Some("kitty")),
    term("wezterm", "WezTerm", None, Some("wezterm")),
    term("ghostty", "Ghostty", None, Some("ghostty")),
    term("konsole", "Konsole", None, Some("konsole")),
    term("gnome-terminal", "GNOME Terminal", None, Some("gnome-terminal")),
    term("ptyxis", "Ptyxis", None, Some("ptyxis")),
    term("kgx", "Console (kgx)", None, Some("kgx")),
    term("xfce4-terminal", "Xfce Terminal", None, Some("xfce4-terminal")),
    term("mate-terminal", "MATE Terminal", None, Some("mate-terminal")),
    term("lxterminal", "LXTerminal", None, Some("lxterminal")),
    term("qterminal", "QTerminal", None, Some("qterminal")),
    term("terminator", "Terminator", None, Some("terminator")),
    term("tilix", "Tilix", None, Some("tilix")),
    term("urxvt", "urxvt", None, Some("urxvt")),
    term("st", "st", None, Some("st")),
    term("xterm", "xterm", None, Some("xterm")),
];

/// Windows Terminal's class ids (stable, Preview) as *Default terminal application* names them.
const WT_CLSIDS: [&str; 2] = ["{E12CFF52-A866-4C77-9A90-F570A7AA2C6B}", "{86633F1F-6454-40EC-89CE-DA4EBA977EE2}"];
/// The console host (*Windows Console Host*).
const CONHOST_CLSID: &str = "{B23D10C0-E52E-411E-9D5B-C09FDF709C7D}";

/// What a system offers: programs on PATH, the environment, Mac apps and, on Windows, the
/// *Default terminal application* setting. Faked in tests.
pub struct Sys<'a> {
    pub os: &'a str,
    pub has: &'a dyn Fn(&str) -> bool,
    pub var: &'a dyn Fn(&str) -> Option<String>,
    pub app: &'a dyn Fn(&str) -> bool,
    /// `HKCU\Console\%%Startup` `DelegationTerminal`: a class id, or none.
    pub delegated: Option<String>,
}

/// `f` with this machine's `Sys`.
pub fn here<R>(f: impl FnOnce(&Sys) -> R) -> R {
    let var = |v: &str| std::env::var(v).ok().filter(|s| !s.is_empty());
    let has = |p: &str| crate::user_menu::installed(p);
    let app = |name: &str| {
        let bundle = format!("{name}.app");
        let home = crate::dirs::home_dir().map(|h| h.join("Applications"));
        ["/Applications", "/System/Applications/Utilities", "/Applications/Utilities"]
            .iter()
            .map(std::path::PathBuf::from)
            .chain(home)
            .any(|d| d.join(&bundle).is_dir())
    };
    let os = if crate::termux::active() { "android" } else { std::env::consts::OS };
    f(&Sys { os, has: &has, var: &var, app: &app, delegated: delegated() })
}

/// Windows' *Default terminal application* (Settings → System → For developers → Terminal).
#[cfg(windows)]
fn delegated() -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let mut data = [0u16; 128];
    let mut len = (data.len() * 2) as u32;
    // SAFETY: the strings are NUL-terminated; `data` holds `len` bytes; RRF_RT_REG_SZ ends it with a NUL.
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, wide(r"Console\%%Startup").as_ptr(), wide("DelegationTerminal").as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), data.as_mut_ptr().cast(), &mut len)
    };
    (r == 0).then(|| String::from_utf16_lossy(&data[..(len as usize / 2).saturating_sub(1)]))
}

#[cfg(not(windows))]
fn delegated() -> Option<String> {
    None
}

fn known(os: &str) -> &'static [Term] {
    match os {
        "macos" => MAC,
        "windows" => WINDOWS,
        "android" => &[],
        _ => UNIX,
    }
}

fn found(t: &Term, s: &Sys) -> bool {
    match (s.os, t.name) {
        // Always there.
        ("macos", "terminal") | ("windows", "cmd" | "powershell") => true,
        _ => t.app.is_some_and(|a| (s.app)(a)) || t.bin.is_some_and(|b| (s.has)(b)),
    }
}

/// The terminals installed, in the order Settings lists them.
pub fn installed(s: &Sys) -> Vec<&'static Term> {
    known(s.os).iter().filter(|t| found(t, s)).collect()
}

/// The one `choice` (config.toml's `terminal`) names: by its name or its label, any case.
fn named(os: &str, choice: &str) -> Option<&'static Term> {
    known(os).iter().find(|t| t.name.eq_ignore_ascii_case(choice) || t.label.eq_ignore_ascii_case(choice))
}

/// What opens when `terminal` is not set: on a Mac Terminal; on Windows the *Default terminal
/// application* (Windows Terminal, else the console), else Windows Terminal when installed;
/// elsewhere `$TERMINAL` (a command line) when its program is installed, else the first of the
/// known ones installed. None without a desktop (a console, ssh) and in Termux.
fn auto(s: &Sys) -> Option<Pick> {
    match s.os {
        "android" => None,
        "macos" => Some(Pick::Known(&MAC[0])),
        "windows" => {
            let d = s.delegated.as_deref().unwrap_or_default();
            let wt = if d.eq_ignore_ascii_case(CONHOST_CLSID) {
                false
            } else {
                WT_CLSIDS.iter().any(|c| d.eq_ignore_ascii_case(c)) || (s.has)("wt")
            };
            Some(Pick::Known(&WINDOWS[if wt { 0 } else { 2 }]))
        }
        _ => {
            (s.var)("WAYLAND_DISPLAY").or_else(|| (s.var)("DISPLAY"))?;
            if let Some(own) = (s.var)("TERMINAL").filter(|t| t.split_whitespace().next().is_some_and(s.has)) {
                return Some(Pick::Words(own.split_whitespace().map(String::from).collect()));
            }
            installed(s).first().copied().map(Pick::Known)
        }
    }
}

/// The name Settings shows for what opens when none is chosen, if anything does.
pub fn auto_label(s: &Sys) -> Option<String> {
    Some(match auto(s)? {
        Pick::Known(t) => t.label.to_string(),
        Pick::Words(w) => w.join(" "),
        Pick::Command(c) => c,
    })
}

/// For Settings, in both apps: the name of what opens when none is chosen (none: nothing
/// would), and the terminals installed here, by name and label.
pub fn for_settings() -> (Option<String>, Vec<(&'static str, &'static str)>) {
    here(|s| (auto_label(s), installed(s).iter().map(|t| (t.name, t.label)).collect()))
}

enum Pick {
    Known(&'static Term),
    /// `$TERMINAL`: a program and its arguments.
    Words(Vec<String>),
    /// A command of the user's own, `{dir}` the folder.
    Command(String),
}

fn pick(choice: &str, s: &Sys) -> Option<Pick> {
    let choice = choice.trim();
    if choice.is_empty() {
        return auto(s);
    }
    if s.os == "android" {
        return None;
    }
    Some(named(s.os, choice).map_or_else(|| Pick::Command(choice.to_string()), Pick::Known))
}

/// How to start a terminal: a program with its arguments, or a command line for the shell
/// (a command of the user's own).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum How {
    Spawn(String, Vec<String>),
    Line(String),
}

fn owned(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// For a terminal on the BSDs, illumos, Linux (and a Mac's or Windows' Alacritty, kitty,
/// WezTerm and Ghostty) by its program's name: the arguments that open it in `dir`, and those
/// put before a program it is to run.
fn flags(program: &str, dir: &str) -> (Vec<String>, Vec<String>) {
    let name = Path::new(program).file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
    let eq = |f: &str| vec![format!("{f}={dir}")];
    let two = |f: &str| vec![f.to_string(), dir.to_string()];
    let (open, run): (Vec<String>, &[&str]) = match name.as_str() {
        "alacritty" => (two("--working-directory"), &["-e"]),
        "kitty" => (two("--directory"), &[]),
        "wezterm" => (owned(&["start", "--cwd", dir]), &["--"]),
        "ghostty" => (eq("--working-directory"), &["-e"]),
        "foot" => (eq("--working-directory"), &[]),
        "konsole" => (two("--workdir"), &["-e"]),
        "gnome-terminal" | "ptyxis" => (eq("--working-directory"), &["--"]),
        "kgx" | "lxterminal" | "tilix" => (eq("--working-directory"), &["-e"]),
        "xfce4-terminal" | "mate-terminal" | "terminator" => (eq("--working-directory"), &["-x"]),
        "qterminal" => (two("--workdir"), &["-e"]),
        "urxvt" => (two("-cd"), &["-e"]),
        "st" => (two("-d"), &["-e"]),
        // xterm and x-terminal-emulator (which may be any of them): the folder they start in.
        _ => (vec![], &["-e"]),
    };
    (open, owned(run))
}

/// A Mac app told to do `line` (a shell line) in a new window, through AppleScript, which takes
/// `"` and `\` escaped.
fn applescript(app: &str, line: &str) -> How {
    let line = line.replace('\\', "\\\\").replace('"', "\\\"");
    let script: Vec<String> = if app == "iTerm" {
        owned(&[
            "tell application \"iTerm\"",
            "set w to (create window with default profile)",
            &format!("tell current session of w to write text \"{line}\""),
            "activate",
            "end tell",
        ])
    } else {
        owned(&[&format!("tell application \"Terminal\" to do script \"{line}\""), "tell application \"Terminal\" to activate"])
    };
    How::Spawn("osascript".into(), script.into_iter().flat_map(|l| ["-e".to_string(), l]).collect())
}

/// A new terminal window in `dir` running `inner` (a program and its arguments), or the shell
/// when `inner` is empty, with the terminal `choice` names (config.toml's `terminal`; empty: as
/// `auto` says). It is started in `dir` too. On Windows `dir` is `.`, so that no path is on a
/// line cmd.exe or wt parses (either would run what follows a `&` in a folder name). None when
/// there is none to open.
pub fn open(choice: &str, dir: &str, inner: &[String], s: &Sys) -> Option<How> {
    let windows = s.os == "windows";
    let dir = if windows { "." } else { dir };
    let console = |program: Vec<String>| How::Spawn("cmd".into(), [owned(&["/C", "start", ""]), program].concat());
    Some(match pick(choice, s)? {
        Pick::Command(c) => {
            let q = if windows { ".".to_string() } else { quote(dir) };
            let mut line = c.replace("{dir}", &q);
            if !inner.is_empty() {
                let first = line.split_whitespace().next().unwrap_or_default().to_string();
                let words: Vec<String> = flags(&first, dir).1.into_iter().chain(inner.iter().map(|a| quote(a))).collect();
                line = format!("{line} {}", words.join(" "));
            }
            How::Line(line)
        }
        Pick::Words(mut w) => {
            let program = w.remove(0);
            let (open, run) = flags(&program, dir);
            w.extend(open);
            if !inner.is_empty() {
                w.extend(run);
                w.extend(inner.iter().cloned());
            }
            How::Spawn(program, w)
        }
        Pick::Known(t) => match (s.os, t.name) {
            ("macos", "terminal" | "iterm2") => match inner {
                [] => How::Spawn("open".into(), owned(&["-a", t.app.unwrap_or_default(), dir])),
                _ => {
                    let words: Vec<String> = inner.iter().map(|a| quote(a)).collect();
                    applescript(t.app.unwrap_or_default(), &format!("cd {} && {}", quote(dir), words.join(" ")))
                }
            },
            ("windows", "wt") => How::Spawn("wt".into(), [owned(&["-d", dir]), inner.to_vec()].concat()),
            ("windows", "pwsh" | "powershell" | "cmd") => match inner {
                [] if t.name == "pwsh" => console(owned(&["pwsh", "-WorkingDirectory", dir])),
                [] => console(owned(&[t.name])),
                _ => console(inner.to_vec()),
            },
            _ => {
                let bin = t.bin.unwrap_or(t.name);
                let (open, run) = flags(bin, dir);
                let mut args = open;
                if !inner.is_empty() {
                    args.extend(run);
                    args.extend(inner.iter().cloned());
                }
                // A Mac app: a new one of it, through `open`, when it is in Applications.
                match t.app {
                    Some(app) if s.os == "macos" && (s.app)(app) => How::Spawn("open".into(), [owned(&["-na", app, "--args"]), args].concat()),
                    _ => How::Spawn(bin.into(), args),
                }
            }
        },
    })
}

/// F2's *Open a terminal here*: a line for the shell, run in the folder (`%d`), the terminal
/// left running on its own when Coxswain quits. None when there is none to open.
pub fn line(choice: &str, s: &Sys) -> Option<String> {
    // `%` is F2's placeholder sign: a command's own is doubled.
    let choice = if pick(choice, s).is_some_and(|p| matches!(p, Pick::Command(_))) { choice.replace('%', "%%") } else { choice.to_string() };
    let how = open(&choice, DIR, &[], s)?;
    let word = |w: &str| if w.contains(DIR) { w.replace(DIR, "%d") } else if s.os == "windows" { w.to_string() } else { quote(w) };
    Some(match (s.os, how) {
        ("windows", How::Spawn(p, a)) => {
            let rest = if p == "cmd" && a.starts_with(&owned(&["/C", "start", ""])) { a[3..].to_vec() } else { [vec![p], a].concat() };
            format!("start \"\" {}", rest.iter().map(|w| word(w)).collect::<Vec<_>>().join(" "))
        }
        ("windows", How::Line(l)) => format!("start \"\" {l}"),
        (_, How::Spawn(p, a)) => {
            let words = std::iter::once(&p).chain(&a).map(|w| word(w)).collect::<Vec<_>>().join(" ");
            // `open` hands the app to the system and returns at once.
            if p == "open" { words } else { format!("nohup {words} >/dev/null 2>&1 &") }
        }
        (_, How::Line(l)) => format!("nohup {} >/dev/null 2>&1 &", l.replace(&quote(DIR), "%d")),
    })
}

/// Where F2's line takes the folder: `%d` once it is a line.
const DIR: &str = "\u{1}dir\u{1}";

#[cfg(test)]
mod tests {
    use super::*;

    fn sys<'a>(os: &'a str, has: &'a dyn Fn(&str) -> bool, var: &'a dyn Fn(&str) -> Option<String>, app: &'a dyn Fn(&str) -> bool, delegated: Option<&str>) -> Sys<'a> {
        Sys { os, has, var, app, delegated: delegated.map(String::from) }
    }

    /// `open` and `line` on `os` with these programs, environment and Mac apps.
    fn on(os: &str, programs: &[&str], env: &[(&str, &str)], apps: &[&str], delegated: Option<&str>, choice: &str, inner: &[&str]) -> (Option<How>, Option<String>) {
        let has = |p: &str| programs.contains(&p);
        let var = |v: &str| env.iter().find(|(k, _)| *k == v).map(|(_, x)| x.to_string());
        let app = |a: &str| apps.contains(&a);
        let s = sys(os, &has, &var, &app, delegated);
        let inner = owned(inner);
        (open(choice, "/w/my dir", &inner, &s), line(choice, &s))
    }

    fn spawn(p: &str, a: &[&str]) -> Option<How> {
        Some(How::Spawn(p.into(), owned(a)))
    }

    const X: &[(&str, &str)] = &[("DISPLAY", ":0")];

    #[test]
    fn terminals_on_a_mac() {
        let (o, l) = on("macos", &[], &[], &[], None, "", &[]);
        assert_eq!(o, spawn("open", &["-a", "Terminal", "/w/my dir"]));
        assert_eq!(l.as_deref(), Some("open -a Terminal %d"));
        let (o, l) = on("macos", &[], &[], &["iTerm"], None, "iTerm2", &[]);
        assert_eq!(o, spawn("open", &["-a", "iTerm", "/w/my dir"]));
        assert_eq!(l.as_deref(), Some("open -a iTerm %d"));
        let (o, _) = on("macos", &[], &[], &["iTerm"], None, "iterm2", &["tig"]);
        let Some(How::Spawn(p, a)) = o else { panic!() };
        assert_eq!(p, "osascript");
        assert!(a.iter().any(|x| x.contains("write text \"cd '/w/my dir' && tig\"")), "{a:?}");
        let (o, _) = on("macos", &[], &[], &[], None, "", &["tig"]);
        let Some(How::Spawn(p, a)) = o else { panic!() };
        assert!(p == "osascript" && a[1].contains("do script") && a[1].ends_with("&& tig\""), "{a:?}");
        // An app in Applications: a new one through `open`; only on PATH, the program itself.
        let (o, l) = on("macos", &["ghostty"], &[], &["Ghostty"], None, "ghostty", &[]);
        assert_eq!(o, spawn("open", &["-na", "Ghostty", "--args", "--working-directory=/w/my dir"]));
        assert_eq!(l.as_deref(), Some("open -na Ghostty --args --working-directory=%d"));
        let (o, _) = on("macos", &["alacritty"], &[], &[], None, "Alacritty", &["tig"]);
        assert_eq!(o, spawn("alacritty", &["--working-directory", "/w/my dir", "-e", "tig"]));
        let (o, _) = on("macos", &[], &[], &["kitty"], None, "kitty", &[]);
        assert_eq!(o, spawn("open", &["-na", "kitty", "--args", "--directory", "/w/my dir"]));
        let (o, l) = on("macos", &["wezterm"], &[], &[], None, "WezTerm", &[]);
        assert_eq!(o, spawn("wezterm", &["start", "--cwd", "/w/my dir"]));
        assert_eq!(l.as_deref(), Some("nohup wezterm start --cwd %d >/dev/null 2>&1 &"));
    }

    #[test]
    fn terminals_on_windows() {
        let (o, l) = on("windows", &["wt"], &[], &[], None, "", &[]);
        assert_eq!(o, spawn("wt", &["-d", "."]));
        assert_eq!(l.as_deref(), Some(r#"start "" wt -d ."#));
        let (o, l) = on("windows", &[], &[], &[], None, "", &[]);
        assert_eq!(o, spawn("cmd", &["/C", "start", "", "powershell"]), "no Windows Terminal: PowerShell");
        assert_eq!(l.as_deref(), Some(r#"start "" powershell"#));
        // Windows' Default terminal application.
        let (o, _) = on("windows", &["wt"], &[], &[], Some(CONHOST_CLSID), "", &[]);
        assert_eq!(o, spawn("cmd", &["/C", "start", "", "powershell"]), "the console host chosen");
        let (o, _) = on("windows", &["wt"], &[], &[], Some("{e12cff52-a866-4c77-9a90-f570a7aa2c6b}"), "", &["tig"]);
        assert_eq!(o, spawn("wt", &["-d", ".", "tig"]), "Windows Terminal chosen");
        let (o, l) = on("windows", &["pwsh"], &[], &[], None, "pwsh", &[]);
        assert_eq!(o, spawn("cmd", &["/C", "start", "", "pwsh", "-WorkingDirectory", "."]));
        assert_eq!(l.as_deref(), Some(r#"start "" pwsh -WorkingDirectory ."#));
        let (o, _) = on("windows", &["pwsh"], &[], &[], None, "PowerShell 7", &["tig"]);
        assert_eq!(o, spawn("cmd", &["/C", "start", "", "tig"]));
        let (_, l) = on("windows", &[], &[], &[], None, "cmd", &[]);
        assert_eq!(l.as_deref(), Some(r#"start "" cmd"#));
        let (o, l) = on("windows", &["alacritty"], &[], &[], None, "alacritty", &[]);
        assert_eq!(o, spawn("alacritty", &["--working-directory", "."]));
        assert_eq!(l.as_deref(), Some(r#"start "" alacritty --working-directory ."#));
        let (o, _) = on("windows", &["wezterm"], &[], &[], None, "wezterm", &["tig"]);
        assert_eq!(o, spawn("wezterm", &["start", "--cwd", ".", "--", "tig"]));
        // A command of one's own: no folder name reaches cmd.exe.
        let (o, l) = on("windows", &[], &[], &[], None, "mytty --cd {dir}", &[]);
        assert_eq!(o, Some(How::Line("mytty --cd .".into())));
        assert_eq!(l.as_deref(), Some(r#"start "" mytty --cd ."#));
    }

    #[test]
    fn terminals_on_the_bsds_and_linux() {
        let (o, l) = on("linux", &["xterm", "x-terminal-emulator"], X, &[], None, "", &[]);
        assert_eq!(o, spawn("x-terminal-emulator", &[]));
        assert_eq!(l.as_deref(), Some("nohup x-terminal-emulator >/dev/null 2>&1 &"));
        let (o, _) = on("freebsd", &["xterm", "kitty"], X, &[], None, "", &["tig"]);
        assert_eq!(o, spawn("kitty", &["--directory", "/w/my dir", "tig"]));
        let own = [("WAYLAND_DISPLAY", "wayland-0"), ("TERMINAL", "foot --app-id x")];
        let (o, l) = on("linux", &["foot", "xterm"], &own, &[], None, "", &[]);
        assert_eq!(o, spawn("foot", &["--app-id", "x", "--working-directory=/w/my dir"]), "$TERMINAL first");
        assert_eq!(l.as_deref(), Some("nohup foot --app-id x --working-directory=%d >/dev/null 2>&1 &"));
        let gone = [("DISPLAY", ":0"), ("TERMINAL", "nowhere")];
        assert_eq!(on("openbsd", &["xterm"], &gone, &[], None, "", &[]).0, spawn("xterm", &[]), "a $TERMINAL not installed is passed over");
        assert_eq!(on("linux", &[], X, &[], None, "", &[]), (None, None), "none installed");
        assert_eq!(on("linux", &["xterm"], &[], &[], None, "", &[]), (None, None), "no display");
        assert_eq!(on("android", &["xterm"], X, &[], None, "xterm", &[]), (None, None), "Termux is the terminal");
        // Chosen by name, each with its own flags.
        for (choice, open, run) in [
            ("ghostty", vec!["--working-directory=/w/my dir"], vec!["-e"]),
            ("Alacritty", vec!["--working-directory", "/w/my dir"], vec!["-e"]),
            ("wezterm", vec!["start", "--cwd", "/w/my dir"], vec!["--"]),
            ("gnome-terminal", vec!["--working-directory=/w/my dir"], vec!["--"]),
            ("konsole", vec!["--workdir", "/w/my dir"], vec!["-e"]),
            ("xfce4-terminal", vec!["--working-directory=/w/my dir"], vec!["-x"]),
            ("xterm", vec![], vec!["-e"]),
        ] {
            let (o, _) = on("netbsd", &[], X, &[], None, choice, &["tig"]);
            let Some(How::Spawn(_, a)) = o else { panic!("{choice}") };
            assert_eq!(a, [open, run, vec!["tig"]].concat(), "{choice}");
        }
        // A command of one's own: {dir} is the folder, quoted for the shell; F2 gives it as %d.
        let (o, l) = on("linux", &[], X, &[], None, "footclient -D {dir} 100%", &[]);
        assert_eq!(o, Some(How::Line("footclient -D '/w/my dir' 100%".into())));
        assert_eq!(l.as_deref(), Some("nohup footclient -D %d 100%% >/dev/null 2>&1 &"));
        let (o, _) = on("linux", &[], X, &[], None, "alacritty --class x", &["tig", "-a"]);
        assert_eq!(o, Some(How::Line("alacritty --class x -e tig -a".into())), "a program to run after the terminal's own flag");
    }

    #[test]
    fn terminals_installed_and_auto() {
        let has = |p: &str| ["kitty", "xterm"].contains(&p);
        let var = |v: &str| (v == "DISPLAY").then(|| ":0".to_string());
        let app = |a: &str| a == "iTerm";
        let s = sys("linux", &has, &var, &app, None);
        assert_eq!(installed(&s).iter().map(|t| t.name).collect::<Vec<_>>(), ["kitty", "xterm"]);
        assert_eq!(auto_label(&s).as_deref(), Some("kitty"));
        let mac = sys("macos", &has, &var, &app, None);
        assert_eq!(installed(&mac).iter().map(|t| t.name).collect::<Vec<_>>(), ["terminal", "iterm2", "kitty"]);
        assert_eq!(auto_label(&mac).as_deref(), Some("Terminal"));
        let win = sys("windows", &has, &var, &app, Some(WT_CLSIDS[0]));
        assert_eq!(installed(&win).iter().map(|t| t.name).collect::<Vec<_>>(), ["powershell", "cmd"]);
        assert_eq!(auto_label(&win).as_deref(), Some("Windows Terminal"));
    }
}

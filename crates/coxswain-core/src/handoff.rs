//! The handoff: what Coxswain does not do with a repository itself (commit, push, pull, fetch,
//! the upstream, the config) opens in the user's own tool. `[git] tool` names it; empty, the
//! first one installed of lazygit, gitui, tig and the desktop clients of the system, and with
//! none, a terminal in the repository. Looked up when the action is used or Settings lists the
//! tools, never at startup.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::quote;
use crate::t;

/// A handoff found no tool: the notice that says what to install shows (`notices`).
pub static MISSING: AtomicBool = AtomicBool::new(false);

/// What opens, and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handoff {
    /// The repository's top folder: the tool runs there.
    pub root: PathBuf,
    /// The tool's name; none when a terminal opens because no tool was found.
    pub name: Option<String>,
    pub run: Run,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    /// A shell command line, run in the terminal the terminal app runs in, which waits for it.
    Here(String),
    /// A program and its arguments, started on its own in `root`.
    // ponytail: arguments as UTF-8 text, so a repository path that is not UTF-8 is mangled;
    // OsString when one turns up.
    Spawn(String, Vec<String>),
}

/// A tool Coxswain knows: its name (in Settings and `[git] tool`), whether it is a window of its
/// own (else it runs in a terminal), where it is (a program on PATH, or a path; `~` and
/// `%VAR%` are expanded), and its arguments (`{dir}` is the repository; none: it reads the
/// folder it runs in).
struct Known {
    name: &'static str,
    window: bool,
    at: &'static [&'static str],
    args: &'static [&'static str],
}

const fn k(
    name: &'static str,
    window: bool,
    at: &'static [&'static str],
    args: &'static [&'static str],
) -> Known {
    Known {
        name,
        window,
        at,
        args,
    }
}

const DIR: &[&str] = &["{dir}"];

/// The tools of `os` (`std::env::consts::OS`, or `android` in Termux), in the order they are
/// tried: the terminal ones first, then the desktop clients.
fn known(os: &str) -> Vec<Known> {
    let mut v = vec![
        k("lazygit", false, &["lazygit"], &[]),
        k("gitui", false, &["gitui"], &[]),
        k("tig", false, &["tig"], &[]),
    ];
    let git_gui: &[&str] = match os {
        "android" => return v,
        "windows" => &["git-gui"],
        "macos" => &[
            "git-gui",
            "/opt/homebrew/libexec/git-core/git-gui",
            "/usr/local/libexec/git-core/git-gui",
        ],
        _ => &[
            "git-gui",
            "/usr/lib/git-core/git-gui",
            "/usr/libexec/git-core/git-gui",
            "/usr/local/libexec/git-core/git-gui",
            "/usr/pkg/libexec/git-core/git-gui",
        ],
    };
    v.extend(match os {
        "macos" => vec![
            k(
                "GitHub Desktop",
                true,
                &[
                    "/Applications/GitHub Desktop.app",
                    "~/Applications/GitHub Desktop.app",
                ],
                DIR,
            ),
            k(
                "GitKraken",
                true,
                &[
                    "/Applications/GitKraken.app",
                    "~/Applications/GitKraken.app",
                ],
                &["-p", "{dir}"],
            ),
            k(
                "Sublime Merge",
                true,
                &[
                    "smerge",
                    "/Applications/Sublime Merge.app/Contents/SharedSupport/bin/smerge",
                ],
                DIR,
            ),
            k(
                "Fork",
                true,
                &["/Applications/Fork.app", "~/Applications/Fork.app"],
                DIR,
            ),
            k(
                "Sourcetree",
                true,
                &[
                    "/Applications/Sourcetree.app",
                    "~/Applications/Sourcetree.app",
                ],
                DIR,
            ),
        ],
        "windows" => vec![
            k(
                "GitHub Desktop",
                true,
                &[r"%LOCALAPPDATA%\GitHubDesktop\GitHubDesktop.exe"],
                &["--cli-open={dir}"],
            ),
            k(
                "GitKraken",
                true,
                &[r"%LOCALAPPDATA%\gitkraken\gitkraken.exe"],
                &["-p", "{dir}"],
            ),
            k(
                "Sublime Merge",
                true,
                &["smerge", r"C:\Program Files\Sublime Merge\smerge.exe"],
                DIR,
            ),
            k(
                "Fork",
                true,
                &[
                    r"%LOCALAPPDATA%\Fork\current\Fork.exe",
                    r"%LOCALAPPDATA%\Fork\Fork.exe",
                ],
                DIR,
            ),
            k(
                "Sourcetree",
                true,
                &[r"%LOCALAPPDATA%\SourceTree\SourceTree.exe"],
                &["-f", "{dir}"],
            ),
        ],
        // FreeBSD, the other BSDs, illumos, Linux.
        _ => vec![
            k(
                "GitHub Desktop",
                true,
                &["github-desktop"],
                &["--cli-open={dir}"],
            ),
            k("GitKraken", true, &["gitkraken"], &["-p", "{dir}"]),
            k(
                "Sublime Merge",
                true,
                &["smerge", "/opt/sublime_merge/sublime_merge"],
                DIR,
            ),
            k("gitg", true, &["gitg"], &[]),
            k("git-cola", true, &["git-cola"], &[]),
        ],
    });
    v.push(k("git gui", true, git_gui, &[]));
    v.push(k("gitk", true, &["gitk"], &[]));
    v
}

/// A tool found: its name, whether it is a window, and its program and arguments.
struct Found {
    name: &'static str,
    window: bool,
    program: String,
    args: Vec<String>,
}

/// `t` where `look` finds it, with its arguments for the repository `root`. A Mac's app is
/// started with `open -a`.
fn find(t: &Known, root: &Path, look: &dyn Fn(&str) -> Option<PathBuf>) -> Option<Found> {
    let at = t.at.iter().find_map(|a| look(a))?;
    let dir = root.to_string_lossy();
    let mut args: Vec<String> = t.args.iter().map(|a| a.replace("{dir}", &dir)).collect();
    let mut program = at.to_string_lossy().into_owned();
    if program.ends_with(".app") {
        let mut open = vec!["-a".to_string(), program];
        if t.args != DIR {
            open.push("--args".into());
        }
        open.append(&mut args);
        (program, args) = ("open".into(), open);
    }
    Some(Found {
        name: t.name,
        window: t.window,
        program,
        args,
    })
}

/// The names of the tools installed here, in the order they are tried: for Settings.
pub fn installed() -> Vec<&'static str> {
    known(os())
        .into_iter()
        .filter(|t| display() || !t.window)
        .filter(|t| t.at.iter().any(|a| look(a).is_some()))
        .map(|t| t.name)
        .collect()
}

/// The handoff from `dir` with the tool `[git] tool` names, for the terminal app (`terminal`)
/// or the desktop app. Nothing is run; outside a repository, the reason it cannot be.
pub fn handoff(tool: &str, dir: &Path, terminal: bool) -> Result<Handoff, String> {
    let var = |v: &str| std::env::var(v).ok().filter(|s| !s.is_empty());
    let has = |p: &str| crate::user_menu::installed(p);
    let r = plan(os(), tool, dir, terminal, &look, &has, &var);
    if matches!(&r, Ok(Handoff { name: None, .. }) | Err(Missing::Nothing)) {
        MISSING.store(true, Ordering::Relaxed);
    }
    r.map_err(Missing::text)
}

/// Why there is no handoff.
#[derive(Debug, PartialEq, Eq)]
enum Missing {
    NoRepo,
    /// The tool `[git] tool` names is one Coxswain knows, and it is not installed.
    NotFound(String),
    /// No tool, and no terminal to open either.
    Nothing,
}

impl Missing {
    fn text(self) -> String {
        match self {
            Missing::NoRepo => t!("gittool.no_repo"),
            Missing::NotFound(name) => t!("gittool.not_found", "name" => name),
            Missing::Nothing => t!("gittool.nothing"),
        }
    }
}

fn os() -> &'static str {
    if crate::termux::active() {
        "android"
    } else {
        std::env::consts::OS
    }
}

/// A desktop to open windows on: always on a Mac and Windows.
fn display() -> bool {
    cfg!(any(windows, target_os = "macos"))
        || ["WAYLAND_DISPLAY", "DISPLAY"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|s| !s.is_empty()))
}

/// Where `at` is: a program on PATH by its name (so a Flatpak runs it on the host), a path
/// as it is, with `~` and `%VAR%` expanded.
fn look(at: &str) -> Option<PathBuf> {
    if !at.contains(['/', '\\']) {
        return crate::tools::which(at).map(|_| PathBuf::from(at));
    }
    let mut s = at.to_string();
    if let Some(rest) = s.strip_prefix("~/") {
        s = std::env::home_dir()?
            .join(rest)
            .to_string_lossy()
            .into_owned();
    }
    while let Some(a) = s.find('%') {
        let b = a + 1 + s[a + 1..].find('%')?;
        let v = std::env::var(&s[a + 1..b]).ok()?;
        s.replace_range(a..=b, &v);
    }
    let p = PathBuf::from(s);
    p.exists().then_some(p)
}

/// The handoff on `os`, with programs found by `look` (a tool's places), `has` (a terminal on
/// PATH) and the environment `var`.
fn plan(
    os: &str,
    tool: &str,
    dir: &Path,
    terminal: bool,
    look: &dyn Fn(&str) -> Option<PathBuf>,
    has: &dyn Fn(&str) -> bool,
    var: &dyn Fn(&str) -> Option<String>,
) -> Result<Handoff, Missing> {
    // Only inside a repository; asking git would run a program for that.
    let root = dir
        .ancestors()
        .find(|a| a.join(".git").exists())
        .ok_or(Missing::NoRepo)?
        .to_path_buf();
    let desktop = matches!(os, "macos" | "windows")
        || os != "android" && var("WAYLAND_DISPLAY").or_else(|| var("DISPLAY")).is_some();
    let tools = known(os);
    let tool = tool.trim();
    let found = if tool.is_empty() {
        tools
            .iter()
            .filter(|t| desktop || !t.window)
            .find_map(|t| find(t, &root, look))
    } else if let Some(t) = tools.iter().find(|t| t.name.eq_ignore_ascii_case(tool)) {
        Some(find(t, &root, look).ok_or_else(|| Missing::NotFound(t.name.into()))?)
    } else {
        // A command of the user's own: `%d` is the repository. One that starts with a terminal
        // tool runs in a terminal; any other the desktop app starts on its own, and the
        // terminal app runs in its terminal (a window of its own then holds it until closed).
        // On Windows cmd.exe reads the line, and expands `%` even inside quotes, so the path never
        // goes into it: `%d` is `.`, the repository the command runs in.
        let at = if os == "windows" { ".".into() } else { quote(&root.to_string_lossy()) };
        let cmd = tool.replace("%d", &at);
        let first = cmd
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        let stem = Path::new(&first).file_stem().unwrap_or_default();
        let in_terminal = tools
            .iter()
            .any(|t| !t.window && stem.eq_ignore_ascii_case(t.name));
        let run = if terminal {
            Run::Here(cmd)
        } else if in_terminal {
            let (sh, flag) = shell(os, var);
            let (p, a) = in_emulator(os, &root, &[sh, flag.into(), cmd], has, var)
                .ok_or(Missing::Nothing)?;
            Run::Spawn(p, a)
        } else {
            let (sh, flag) = shell(os, var);
            Run::Spawn(sh, vec![flag.into(), cmd])
        };
        return Ok(Handoff {
            root,
            name: Some(first.clone()),
            run,
        });
    };
    // No tool: a shell in the repository, in this terminal or a new one.
    let Some(f) = found else {
        let run = if terminal {
            Run::Here(quote(&shell(os, var).0))
        } else {
            let (p, a) = in_emulator(os, &root, &[], has, var).ok_or(Missing::Nothing)?;
            Run::Spawn(p, a)
        };
        return Ok(Handoff {
            root,
            name: None,
            run,
        });
    };
    let name = Some(f.name.to_string());
    let run = if f.window {
        Run::Spawn(f.program, f.args)
    } else if terminal {
        let words: Vec<String> = std::iter::once(&f.program)
            .chain(&f.args)
            .map(|a| quote(a))
            .collect();
        Run::Here(words.join(" "))
    } else {
        let inner: Vec<String> = std::iter::once(f.program).chain(f.args).collect();
        let (p, a) = in_emulator(os, &root, &inner, has, var).ok_or(Missing::Nothing)?;
        Run::Spawn(p, a)
    };
    Ok(Handoff { root, name, run })
}

/// The user's shell on `os` and its flag for a command line.
fn shell(os: &str, var: &dyn Fn(&str) -> Option<String>) -> (String, &'static str) {
    if os == "windows" {
        ("cmd".into(), "/C")
    } else {
        (var("SHELL").unwrap_or_else(|| "sh".into()), "-c")
    }
}

/// A new terminal window in `root` running `inner` (a program and its arguments), or the shell
/// when `inner` is empty: Terminal on a Mac, Windows Terminal (else a console) on Windows,
/// `$TERMINAL` or the first terminal installed elsewhere, as F2's *Open a terminal here*.
fn in_emulator(
    os: &str,
    root: &Path,
    inner: &[String],
    has: &dyn Fn(&str) -> bool,
    var: &dyn Fn(&str) -> Option<String>,
) -> Option<(String, Vec<String>)> {
    let dir = root.to_string_lossy().into_owned();
    let owned = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match os {
        "android" => None,
        "macos" if inner.is_empty() => Some(("open".into(), owned(&["-a", "Terminal", &dir]))),
        "macos" => {
            // Terminal runs a line through AppleScript, which takes `"` and `\` escaped.
            let line = format!(
                "cd {} && {}",
                quote(&dir),
                inner.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
            );
            let line = line.replace('\\', "\\\\").replace('"', "\\\"");
            Some((
                "osascript".into(),
                owned(&[
                    "-e",
                    &format!("tell application \"Terminal\" to do script \"{line}\""),
                    "-e",
                    "tell application \"Terminal\" to activate",
                ]),
            ))
        }
        "windows" => {
            let inner = if inner.is_empty() {
                vec!["powershell".to_string()]
            } else {
                inner.to_vec()
            };
            // The window starts in `root` (the spawn's own folder), so no path is on a line that
            // cmd.exe or wt parses: either would run what follows a `&`, `;` or `|` in a folder name.
            Some(if has("wt") {
                ("wt".into(), [owned(&["-d", "."]), inner].concat())
            } else {
                ("cmd".into(), [owned(&["/C", "start", ""]), inner].concat())
            })
        }
        _ => {
            let line = crate::user_menu::emulator(has, var)?;
            let mut words = line.split_whitespace().map(String::from);
            let program = words.next()?;
            let mut args: Vec<String> = words.collect();
            if !inner.is_empty() {
                // How each terminal is told what to run: most take -e.
                let name = Path::new(&program).file_name().unwrap_or_default();
                let run: &[&str] = match name.to_string_lossy().as_ref() {
                    "foot" | "kitty" => &[],
                    "gnome-terminal" | "ptyxis" => &["--"],
                    "wezterm" => &["start", "--"],
                    "xfce4-terminal" | "mate-terminal" | "terminator" => &["-x"],
                    _ => &["-e"],
                };
                args.extend(owned(run));
                args.extend(inner.iter().cloned());
            }
            Some((program, args))
        }
    }
}

/// Start `h` (a `Run::Spawn`) in its repository, on its own: in a Flatpak, on the host. A
/// program that fails at once says why.
pub fn spawn(h: &Handoff) -> std::io::Result<()> {
    let Run::Spawn(program, args) = &h.run else {
        return Err(std::io::Error::other("not a program to start"));
    };
    let mut c = crate::tools::user_command(program);
    c.args(args).current_dir(&h.root);
    crate::tools::spawn_watched(c, std::time::Duration::from_secs(1))
}

/// What the status line says once the tool has opened.
pub fn opened(h: &Handoff) -> String {
    match &h.name {
        Some(name) => t!("gittool.opened", "name" => name),
        None => t!("gittool.terminal"),
    }
}

/// The line that installs a git tool here, if Coxswain knows one: lazygit, else tig.
pub fn install() -> Option<String> {
    crate::tools::install("lazygit").or_else(|| crate::tools::install("tig"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake PATH: a temporary folder holding (empty) programs, and a repository.
    struct Fake {
        bin: PathBuf,
        repo: PathBuf,
        top: PathBuf,
    }

    impl Fake {
        fn new(name: &str, programs: &[&str]) -> Fake {
            let top = std::env::temp_dir()
                .join(format!("coxswain-handoff-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&top);
            let (bin, repo) = (top.join("bin"), top.join("repo"));
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::create_dir_all(repo.join("src")).unwrap();
            for p in programs {
                std::fs::write(bin.join(p), "").unwrap();
            }
            Fake { bin, repo, top }
        }

        /// `plan` with this PATH: a program by its name in `bin`.
        fn plan(
            &self,
            os: &str,
            tool: &str,
            dir: &Path,
            terminal: bool,
            env: &[(&str, &str)],
        ) -> Result<Handoff, Missing> {
            // Only the fake PATH: the machine's own paths (git gui in git's folder) are not seen.
            let look = |a: &str| -> Option<PathBuf> {
                let bare = !a.contains(['/', '\\']);
                (bare && self.bin.join(a).is_file()).then(|| PathBuf::from(a))
            };
            let has = |p: &str| self.bin.join(p).is_file();
            let var = |v: &str| {
                env.iter()
                    .find(|(k, _)| *k == v)
                    .map(|(_, x)| x.to_string())
            };
            plan(os, tool, dir, terminal, &look, &has, &var)
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.top);
        }
    }

    const X: &[(&str, &str)] = &[("DISPLAY", ":0"), ("SHELL", "/bin/zsh")];

    fn spawn(p: &str, a: &[&str]) -> Run {
        Run::Spawn(p.into(), a.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn handoff_detection_order() {
        let f = Fake::new("order", &["gitk", "tig", "gitui", "smerge", "foot"]);
        let src = f.repo.join("src");
        let h = f.plan("linux", "", &src, true, X).unwrap();
        assert_eq!(h.root, f.repo, "the repository's top folder");
        assert_eq!(h.name.as_deref(), Some("gitui"), "gitui before tig");
        assert_eq!(h.run, Run::Here("gitui".into()), "in this terminal");
        std::fs::write(f.bin.join("lazygit"), "").unwrap();
        let h = f.plan("freebsd", "", &src, true, X).unwrap();
        assert_eq!(h.name.as_deref(), Some("lazygit"), "lazygit first");
        // The desktop app: a terminal tool in a new terminal.
        let h = f.plan("linux", "", &src, false, X).unwrap();
        assert_eq!(h.run, spawn("foot", &["lazygit"]));
        // No terminal tools: the desktop clients, in order.
        for t in ["lazygit", "gitui", "tig"] {
            std::fs::remove_file(f.bin.join(t)).unwrap();
        }
        let repo = f.repo.to_string_lossy();
        let h = f.plan("openbsd", "", &src, true, X).unwrap();
        assert_eq!(h.name.as_deref(), Some("Sublime Merge"));
        assert_eq!(
            h.run,
            spawn("smerge", &[&repo]),
            "the repository as argument"
        );
        std::fs::remove_file(f.bin.join("smerge")).unwrap();
        let h = f.plan("linux", "", &src, false, X).unwrap();
        assert_eq!(
            (h.name.as_deref(), h.run),
            (Some("gitk"), spawn("gitk", &[]))
        );
        // Without a desktop (ssh), no window: the shell in this terminal.
        let h = f
            .plan("linux", "", &src, true, &[("SHELL", "/bin/zsh")])
            .unwrap();
        assert_eq!((h.name, h.run), (None, Run::Here("/bin/zsh".into())));
    }

    #[test]
    fn handoff_terminal_fallback() {
        let f = Fake::new("fallback", &["kitty"]);
        let repo = f.repo.to_string_lossy().into_owned();
        let h = f.plan("linux", "", &f.repo, false, X).unwrap();
        assert_eq!(
            (h.name, h.run),
            (None, spawn("kitty", &[])),
            "a terminal in the repository"
        );
        let h = f.plan("windows", "", &f.repo, false, &[]).unwrap();
        assert_eq!(
            h.run,
            spawn("cmd", &["/C", "start", "", "powershell"])
        );
        let h = f.plan("macos", "", &f.repo, false, &[]).unwrap();
        assert_eq!(h.run, spawn("open", &["-a", "Terminal", &repo]));
        let h = f.plan("windows", "", &f.repo, true, &[]).unwrap();
        assert_eq!(h.run, Run::Here("cmd".into()));
        // Nothing at all: no tool, no terminal, no desktop.
        let none = Fake::new("none", &[]);
        assert_eq!(
            none.plan("linux", "", &none.repo, false, X),
            Err(Missing::Nothing)
        );
    }

    #[test]
    fn handoff_commands_per_terminal_and_system() {
        let f = Fake::new("terms", &["tig", "gnome-terminal", "wt"]);
        let repo = f.repo.to_string_lossy().into_owned();
        let h = f.plan("linux", "", &f.repo, false, X).unwrap();
        assert_eq!(h.run, spawn("gnome-terminal", &["--", "tig"]));
        let own = [("DISPLAY", ":0"), ("TERMINAL", "wezterm"), ("SHELL", "sh")];
        std::fs::write(f.bin.join("wezterm"), "").unwrap();
        let h = f.plan("netbsd", "", &f.repo, false, &own).unwrap();
        assert_eq!(
            h.run,
            spawn("wezterm", &["start", "--", "tig"]),
            "$TERMINAL first"
        );
        let h = f.plan("windows", "", &f.repo, false, &[]).unwrap();
        assert_eq!(h.run, spawn("wt", &["-d", ".", "tig"]));
        // No folder name reaches cmd.exe: `a&calc` would run calc.
        let evil = Fake::new("x&calc", &["cmd"]);
        let h = evil.plan("windows", "mytool %d", &evil.repo, false, &[]).unwrap();
        assert!(!format!("{:?}", h.run).contains("calc"), "{:?}", h.run);
        let Run::Spawn(p, a) = f.plan("macos", "", &f.repo, false, &[]).unwrap().run else {
            panic!()
        };
        assert_eq!(p, "osascript");
        assert!(
            a[1].contains("do script") && a[1].ends_with("&& tig\""),
            "{}",
            a[1]
        );
    }

    #[test]
    fn handoff_setting_names_a_tool_or_a_command() {
        let f = Fake::new("setting", &["lazygit", "gitk", "xterm"]);
        let repo = f.repo.to_string_lossy().into_owned();
        let h = f.plan("linux", "GITK", &f.repo, true, X).unwrap();
        assert_eq!(
            (h.name.as_deref(), h.run),
            (Some("gitk"), spawn("gitk", &[])),
            "by name"
        );
        assert_eq!(
            f.plan("linux", "Sublime Merge", &f.repo, true, X),
            Err(Missing::NotFound("Sublime Merge".into())),
            "named but not installed"
        );
        // A command of one's own: %d is the repository.
        let h = f.plan("linux", "gitui -d %d", &f.repo, true, X).unwrap();
        assert_eq!(h.run, Run::Here(format!("gitui -d {}", quote(&repo))));
        let h = f.plan("linux", "gitui -d %d", &f.repo, false, X).unwrap();
        let line = format!("gitui -d {}", quote(&repo));
        assert_eq!(
            h.run,
            spawn("xterm", &["-e", "/bin/zsh", "-c", &line]),
            "in a terminal"
        );
        let h = f.plan("linux", "my-client", &f.repo, false, X).unwrap();
        assert_eq!(h.run, spawn("/bin/zsh", &["-c", "my-client"]), "on its own");
    }

    #[test]
    fn handoff_mac_apps_open_with_open() {
        let f = Fake::new("mac", &[]);
        let tools = known("macos");
        let fork = tools.iter().find(|t| t.name == "Fork").unwrap();
        let kraken = tools.iter().find(|t| t.name == "GitKraken").unwrap();
        let app = |_: &str| Some(PathBuf::from("/Applications/X.app"));
        let repo = f.repo.to_string_lossy().into_owned();
        let got = find(fork, &f.repo, &app).unwrap();
        assert_eq!(
            (got.program.as_str(), got.args),
            (
                "open",
                vec!["-a".into(), "/Applications/X.app".into(), repo.clone()]
            )
        );
        let got = find(kraken, &f.repo, &app).unwrap();
        assert_eq!(
            got.args,
            ["-a", "/Applications/X.app", "--args", "-p", &repo]
        );
    }

    #[test]
    fn handoff_nothing_outside_a_repository() {
        let f = Fake::new("norepo", &["lazygit", "xterm"]);
        let outside = f.top.join("bin");
        assert_eq!(f.plan("linux", "", &outside, true, X), Err(Missing::NoRepo));
        assert_eq!(
            f.plan("linux", "my-tool", &outside, false, X),
            Err(Missing::NoRepo)
        );
        MISSING.store(false, Ordering::Relaxed);
        assert!(handoff("", &outside, true).is_err());
        assert!(!MISSING.load(Ordering::Relaxed), "not a missing tool");
    }

    #[test]
    fn handoff_tools_on_every_system() {
        for os in [
            "freebsd", "netbsd", "openbsd", "illumos", "linux", "android", "macos", "windows",
        ] {
            let names: Vec<&str> = known(os).iter().map(|t| t.name).collect();
            assert_eq!(names[..3], ["lazygit", "gitui", "tig"], "{os}");
            assert_eq!(
                os == "android",
                names.len() == 3,
                "{os}: no windows in Termux"
            );
            if os != "android" {
                assert_eq!(names.last(), Some(&"gitk"), "{os}");
            }
        }
    }
}
